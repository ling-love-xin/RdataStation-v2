//! 临时表 → 文件库（持久表）：引擎原语。
//!
//! 用途：结果集「物化成持久表」（默认仍是 `tmp_q_` 临时表，持久是显式升级），
//! 以及将来「临时表进资产库归档」那一步的同一段写路径。
//!
//! 为什么这一层要在 `engine` 里：现成实现只有 `mock` crate 的
//! `MockEngine::write_temp_table_to_database`，而 `mock` 依赖 `engine`——
//! 让引擎反向依赖 mock 是错的。所以引擎自己给出这条原语：
//! `ATTACH` 目标文件库 → `CREATE [OR REPLACE] TABLE … AS SELECT` → `DETACH`，
//! 数据全程在 DuckDB 内部流动，不经 Rust 字符串中转。
//!
//! 与原实现（mock）的差异是刻意的：mock 的 `Create` 带列约束定义（唯一列等），
//! 结果集没有那些约束需求，用 `CTAS` 即可——列名与类型直接继承临时表。
//!
//! # 一条实测硬约束（2026-11-15，Windows）
//!
//! 目标文件库**不能已被别的连接独占**：此时 `ATTACH` 直接失败
//! （`IO Error: Cannot open file …: 另一个程序正在使用此文件`）。所以：
//!
//! - 本原语适合写「自己创建 / 自己拥有」的文件库（新导出、独立文件）；
//! - 写**项目分析库**（`{项目}/.RSmeta/analytics.duckdb`）不能走这里——那个库在运行时
//!   由 `DuckDBManager` 持独占写连接（`duckdb/accel.rs:29` 原文：分析库写连接由
//!   `DuckDBManager` 独占）。结果集持久化要经那条既有连接写，见设计文档 §9。

use std::path::Path;

use duckdb::Connection;
use shared::error::{CommonError, CoreError};

use crate::sql::SqlEngine;
use crate::services::duckdb_service::DuckDbService;

use super::quote_ident;

/// 写持久表时对目标文件库使用的挂载别名（与用户数据里的别名冲突概率极低，
/// 且每次写前后都会先尽力解挂）。
pub const PROMOTE_ALIAS: &str = "rds_promote";

/// 目标表已存在时怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromoteMode {
    /// 新建：已存在则报错（不悄悄覆盖别人的表）
    Create,
    /// 覆盖：同名表整表替换
    Replace,
}

/// 把内存单例里的 `source_table` 写进 `db_path` 的 `target_table`，返回写入后的行数。
///
/// **调用方不得持有内存库连接锁**后再调本函数（否则自锁死）。
pub fn write_temp_table_to_file_db(
    db_path: &Path,
    source_table: &str,
    target_table: &str,
    mode: PromoteMode,
) -> Result<u64, CoreError> {
    if source_table.trim().is_empty() || target_table.trim().is_empty() {
        return Err(invalid("源临时表与目标表名都不能为空".to_string()));
    }
    if let Some(parent) = db_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                invalid(format!("建目标库目录失败（{}）: {e}", parent.display()))
            })?;
        }
    }

    let singleton = DuckDbService::get_or_create_duckdb()?;
    let conn = singleton
        .lock()
        .map_err(|e| invalid(format!("DuckDB 单例锁中毒: {e}")))?;
    promote_with_conn(&conn, db_path, source_table, target_table, mode)
}

/// 在**已拿到的连接**上执行同样的一步（供内部与测试复用；调用方自己负责锁）。
fn promote_with_conn(
    conn: &Connection,
    db_path: &Path,
    source_table: &str,
    target_table: &str,
    mode: PromoteMode,
) -> Result<u64, CoreError> {
    let path_text = db_path.to_string_lossy().to_string();
    // 上一次异常退出可能留下同名挂载：先尽力解挂（失败忽略，ATTACH 会报真错）
    let _ = conn.execute_batch(&SqlEngine::build_detach_database(PROMOTE_ALIAS));
    conn.execute_batch(&SqlEngine::build_attach_database(&path_text, PROMOTE_ALIAS))
        .map_err(|e| invalid(format!("挂载目标库失败（{}）: {e}", db_path.display())))?;

    let target = format!(
        "{}.main.{}",
        quote_ident(PROMOTE_ALIAS),
        quote_ident(target_table)
    );
    let source = quote_ident(source_table);
    let create = match mode {
        PromoteMode::Create => format!("CREATE TABLE {target} AS SELECT * FROM {source}"),
        PromoteMode::Replace => {
            format!("CREATE OR REPLACE TABLE {target} AS SELECT * FROM {source}")
        }
    };

    let written = (|| -> Result<u64, CoreError> {
        conn.execute_batch(&create)
            .map_err(|e| invalid(format!("在目标库建表 {target_table} 失败: {e}")))?;
        let count = conn
            .query_row(&format!("SELECT count(*) FROM {target}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|e| invalid(format!("统计目标表行数失败: {e}")))?;
        Ok(count.max(0) as u64)
    })();

    // 无论成败都解挂：留着挂载会让后续「同名库再写」在 ATTACH 时撞名
    let detached = conn.execute_batch(&SqlEngine::build_detach_database(PROMOTE_ALIAS));
    let count = written?; // 写入失败优先报写入（解挂失败不拖淡根因）
    detached.map_err(|e| invalid(format!("解挂目标库失败: {e}")))?;
    Ok(count)
}

fn invalid(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

#[cfg(test)]
mod tests {
    use super::{promote_with_conn, PromoteMode};
    use duckdb::Connection;

    fn dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_promote_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录");
        dir
    }

    #[test]
    fn promotes_a_temp_table_into_a_file_database() {
        let dir = dir("create");
        let db = dir.join("analytics.duckdb");
        // 用一条独立内存连接当「单例」：这个用例验的是写路径，不依赖进程单例
        let conn = Connection::open_in_memory().expect("内存库");
        conn.execute_batch("CREATE TABLE tmp_q_src AS SELECT 1 AS a UNION ALL SELECT 2 AS a")
            .expect("建临时表");

        let count =
            promote_with_conn(&conn, &db, "tmp_q_src", "orders_copy", PromoteMode::Create)
                .expect("写持久表");
        assert_eq!(count, 2, "写入行数应与临时表一致");

        // 直接打开目标库验证：表真的落盘了，列与数据都在
        // （验完必须立刻关掉：目标库被别的连接独占时 ATTACH 会失败，见模块头）
        {
            let file = Connection::open(&db).expect("打开目标库");
            let rows: i64 = file
                .query_row("SELECT count(*) FROM orders_copy", [], |r| r.get(0))
                .expect("读目标表");
            assert_eq!(rows, 2);
        }

        // 同模式下重名必须被拒（不悄悄覆盖别人的表）
        let again = promote_with_conn(&conn, &db, "tmp_q_src", "orders_copy", PromoteMode::Create);
        assert!(again.is_err(), "Create 模式撞名应报错");

        // Replace 模式允许整表替换
        let replaced =
            promote_with_conn(&conn, &db, "tmp_q_src", "orders_copy", PromoteMode::Replace)
                .expect("覆盖写");
        assert_eq!(replaced, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_names_are_rejected_before_touching_the_database() {
        let dir = dir("empty");
        let db = dir.join("a.duckdb");
        let conn = Connection::open_in_memory().expect("内存库");
        assert!(promote_with_conn(&conn, &db, "", "t", PromoteMode::Create).is_err());
        assert!(promote_with_conn(&conn, &db, "tmp_q_x", "", PromoteMode::Create).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
