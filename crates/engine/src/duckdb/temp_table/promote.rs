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

/// 跨**实例**搬运：把 `source` 连接里的表写进 `dest` 连接上的表，返回写入行数。
///
/// 为什么不能 ATTACH：`tmp_q_` 在**进程内存单例**（in-memory 库，别的 DuckDB 实例根本挂不到它），
/// 而项目 `analytics.duckdb` 在运行期被 `ProjectDatabaseManager` 按次开关地握着——两边同时
/// 「另一个程序正在使用此文件」的实测见 `crates/engine/tests/analysis_db_exclusive_probe.rs`。
/// 于是数据只能**搬运**，两条连接的 SQL 上下文之间靠一个中转文件对接。
///
/// 中转格式用 **Parquet**（不是 CSV）：列名与类型（含 Decimal / Timestamp / 嵌套）原样过一遍，
 /// 不经过 Rust 侧任何值装箱，也不需要在目标库重新推断类型。
///
 /// **调用方不得持有 `source` / `dest` 的任何锁**后再调本函数。
pub fn write_table_across_connections(
    source: &Connection,
    dest: &Connection,
    source_table: &str,
    target_table: &str,
    mode: PromoteMode,
) -> Result<u64, CoreError> {
    if source_table.trim().is_empty() || target_table.trim().is_empty() {
        return Err(invalid("源表与目标表名都不能为空".to_string()));
    }
    let staging = std::env::temp_dir().join(format!(
        "rds_promote_{}_{}.parquet",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let staging_sql = staging.to_string_lossy().replace('\'', "''");

    // ① 源连接导出中转文件（DuckDB 内部流式写，不经过 Rust）
    let export = source.execute_batch(&format!(
        "COPY (SELECT * FROM {}) TO '{}' (FORMAT PARQUET)",
        quote_ident(source_table),
        staging_sql
    ));
    if let Err(error) = export {
        let _ = std::fs::remove_file(&staging);
        return Err(invalid(format!("导出中转文件失败: {error}")));
    }

    // ② 目标连接从文件建表（`CREATE OR REPLACE` / `CREATE` 由 mode 决定）
    let target = quote_ident(target_table);
    let reader = format!("read_parquet('{staging_sql}')");
    let create = match mode {
        PromoteMode::Create => format!("CREATE TABLE {target} AS SELECT * FROM {reader}"),
        PromoteMode::Replace => {
            format!("CREATE OR REPLACE TABLE {target} AS SELECT * FROM {reader}")
        }
    };
    let written = (|| -> Result<u64, CoreError> {
        dest.execute_batch(&create)
            .map_err(|e| invalid(format!("在目标库建表 {target_table} 失败: {e}")))?;
        let count = dest
            .query_row(&format!("SELECT count(*) FROM {target}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|e| invalid(format!("统计目标表行数失败: {e}")))?;
        Ok(count.max(0) as u64)
    })();

    // ③ 清理中转文件（成败都清：留着只会占盘）
    let _ = std::fs::remove_file(&staging);
    written
}

#[cfg(test)]
mod tests {
    use super::{promote_with_conn, write_table_across_connections, PromoteMode};
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

    /// 跨实例搬运：源是内存库（单例的等价物），目标是**另一个打开的文件库**（项目库的等价物）。
    /// 类型必须原样过去（Parquet 中转的意义就在这）。
    #[test]
    fn moves_a_table_across_two_connections_with_types_intact() {
        let dir = dir("transport");
        let dest_path = dir.join("analytics.duckdb");

        let source = Connection::open_in_memory().expect("源内存库");
        source
            .execute_batch(
                "CREATE TABLE tmp_q_orders AS SELECT \
                   CAST(1234.5678 AS DECIMAL(12,4)) AS amount, \
                   DATE '2026-11-15' AS biz_date, \
                   'a''b' AS note",
            )
            .expect("源表");

        let dest = Connection::open(&dest_path).expect("目标文件库");
        let written = write_table_across_connections(
            &source,
            &dest,
            "tmp_q_orders",
            "orders_persisted",
            PromoteMode::Create,
        )
        .expect("跨实例搬运");
        assert_eq!(written, 1);

        // 列类型跟着过去：Decimal 仍是 Decimal（不是被字符串化成 DOUBLE/TEXT）
        let ty: String = dest
            .query_row(
                "SELECT data_type FROM information_schema.columns \
                 WHERE table_name = 'orders_persisted' AND column_name = 'amount'",
                [],
                |row| row.get(0),
            )
            .expect("读列类型");
        assert!(
            ty.to_uppercase().starts_with("DECIMAL"),
            "Decimal 类型应原样保留，实际 {ty}"
        );

        // 引号与日期原样过一遍
        let note: String = dest
            .query_row("SELECT note FROM orders_persisted", [], |row| row.get(0))
            .expect("读 note");
        assert_eq!(note, "a'b");

        // Create 撞名被拒；Replace 允许覆盖
        assert!(write_table_across_connections(
            &source,
            &dest,
            "tmp_q_orders",
            "orders_persisted",
            PromoteMode::Create
        )
        .is_err());
        assert_eq!(
            write_table_across_connections(
                &source,
                &dest,
                "tmp_q_orders",
                "orders_persisted",
                PromoteMode::Replace
            )
            .expect("覆盖"),
            1
        );

        drop(dest);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
