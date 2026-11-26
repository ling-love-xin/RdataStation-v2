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

use crate::services::duckdb_service::DuckDbService;
use crate::sql::SqlEngine;

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
            std::fs::create_dir_all(parent)
                .map_err(|e| invalid(format!("建目标库目录失败（{}）: {e}", parent.display())))?;
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

/// 持久（项目库）表的命名前缀。
///
/// **刻意不是 `tmp_` 任何一个来源的前缀**：TTL / 上限 / 按前缀清理只认 `tmp_q_` / `tmp_i_` /
/// `tmp_m_` / `tmp_p_`，持久表若带上其中任何一个，就会在 30 分钟后被当成临时表清掉。
pub const PERSIST_PREFIX: &str = "res_";

/// 持久表名：`res_<描述>_<紧凑时间戳>`（口径 = 结果集描述 + 时间戳，**不带随机尾巴**）。
///
/// 与临时表名的差别是有意的：
/// - 不要随机尾巴——持久表是用户可回访的东西，名字要能被人念出来、能在资源树里认出来；
/// - 同一秒建两张同描述的表会撞名：这时的正确行为是**报错让用户改名**（`PromoteMode::Create`），
///   而不是悄悄生成第二张长得很像的表。
pub fn persistent_table_name(description: &str) -> String {
    // 持久表名保留中文（临时表那份 ASCII 尺子在这里不适用，见 `sanitize_persistent_description`）
    let desc = super::temp_table::sanitize_persistent_description(description);
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    format!("{PERSIST_PREFIX}{desc}_{stamp}")
}

/// 把共享临时区（`rds_tmp`）里的表写进**已挂载的项目分析库**，返回写入行数。
///
/// 2026-11-26 所有权收口后，进程面 `tmp_q_` / `tmp_i_` 与项目 `analytics` 都在
/// `shared::duckdb_root` 的同一条根连接上：源在 `main` 的 `rds_tmp` schema，
/// 目标在 `analytics` 的 `main` schema。因此不再需要 Parquet 中转，直接
/// `CREATE [OR REPLACE] TABLE analytics.<目标> AS SELECT * FROM rds_tmp.<源>`。
///
/// **调用方不得持有 `dest` 的锁后再调本函数**：`dest` 是已经 `USE analytics` 的项目会话，
/// 函数只在它上面执行两条 SQL，不自己开连接、不 `ATTACH`。
pub fn write_shared_temp_to_project(
    dest: &Connection,
    source_table: &str,
    target_table: &str,
    mode: PromoteMode,
) -> Result<u64, CoreError> {
    if source_table.trim().is_empty() || target_table.trim().is_empty() {
        return Err(invalid("源表与目标表名都不能为空".to_string()));
    }
    let source = if source_table.starts_with("rds_tmp.") {
        source_table.to_string()
    } else {
        format!(
            "{}.{}",
            super::SHARED_TEMP_SCHEMA,
            quote_ident(source_table)
        )
    };
    let target = format!(
        "{}.{}",
        quote_ident(shared::duckdb_root::PROJECT_ALIAS),
        quote_ident(target_table)
    );
    let create = match mode {
        PromoteMode::Create => format!("CREATE TABLE {target} AS SELECT * FROM {source}"),
        PromoteMode::Replace => {
            format!("CREATE OR REPLACE TABLE {target} AS SELECT * FROM {source}")
        }
    };
    dest.execute_batch(&create)
        .map_err(|e| invalid(format!("在项目分析库建表 {target_table} 失败: {e}")))?;
    let count = dest
        .query_row(&format!("SELECT count(*) FROM {target}"), [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| invalid(format!("统计目标表行数失败: {e}")))?;
    Ok(count.max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::{
        persistent_table_name, promote_with_conn, write_shared_temp_to_project, PromoteMode,
    };

    /// 持久表名：带 `res_` 前缀、清洗非法字符、**不在任何临时表前缀下**（否则会被 TTL 清掉）
    #[test]
    fn persistent_names_never_look_like_temp_tables() {
        let name = persistent_table_name("结果 2 / 订单明细");
        assert!(name.starts_with("res_"), "{name}");
        // 中文必须留在名字里：持久表名是给人认的，`persistent_table_label` 靠它反向解析
        // （抹成 `res_tmp_…` 的话，一屏几十张表全叫同一个名字）
        assert!(name.contains("订单明细"), "{name}");
        assert!(name.contains("结果_2"), "空白与标点折叠成下划线：{name}");
        assert!(
            name.chars().all(|c| c.is_alphanumeric() || c == '_'),
            "只剩字母数字下划线：{name}"
        );
        assert!(
            !name.contains("  ") && !name.contains("//"),
            "不得留下原始空白与标点：{name}"
        );
        for temp_prefix in ["tmp_q_", "tmp_i_", "tmp_m_", "tmp_p_"] {
            assert!(
                !name.starts_with(temp_prefix),
                "持久表名不能落在临时表前缀下（TTL 会把它当临时表删掉）：{name}"
            );
        }
        // 描述为空时也要给出可用名字（不是 `res__<stamp>` 这种双下划线）
        let fallback = persistent_table_name("   ");
        assert!(fallback.starts_with("res_tmp_"), "{fallback}");
    }
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

        let count = promote_with_conn(&conn, &db, "tmp_q_src", "orders_copy", PromoteMode::Create)
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

    /// 同根 CTAS：源在 `rds_tmp`，目标在已挂载的 `analytics`，类型与引号原样过去。
    #[test]
    fn writes_shared_temp_into_the_attached_project_without_parquet() {
        let dir = dir("ctas");
        let dest_path = dir.join("analytics.duckdb");
        let literal = dest_path
            .to_string_lossy()
            .replace('\\', "/")
            .replace('\'', "''");
        let root = Connection::open_in_memory().expect("根内存库");
        root.execute_batch(&format!(
            "CREATE SCHEMA rds_tmp; \
             CREATE TABLE rds_tmp.tmp_q_orders AS SELECT \
               CAST(1234.5678 AS DECIMAL(12,4)) AS amount, \
               DATE '2026-11-15' AS biz_date, \
               'a''b' AS note; \
             ATTACH '{literal}' AS analytics (READ_WRITE)"
        ))
        .expect("建共享临时表并挂项目库");

        let written = write_shared_temp_to_project(
            &root,
            "tmp_q_orders",
            "orders_persisted",
            PromoteMode::Create,
        )
        .expect("同根 CTAS");
        assert_eq!(written, 1);

        let ty: String = root
            .query_row(
                "SELECT typeof(amount) FROM analytics.orders_persisted",
                [],
                |row| row.get(0),
            )
            .expect("读列类型");
        assert!(
            ty.to_uppercase().starts_with("DECIMAL"),
            "Decimal 类型应原样保留，实际 {ty}"
        );
        let note: String = root
            .query_row("SELECT note FROM analytics.orders_persisted", [], |row| {
                row.get(0)
            })
            .expect("读 note");
        assert_eq!(note, "a'b");

        assert!(write_shared_temp_to_project(
            &root,
            "tmp_q_orders",
            "orders_persisted",
            PromoteMode::Create
        )
        .is_err());
        assert_eq!(
            write_shared_temp_to_project(
                &root,
                "tmp_q_orders",
                "orders_persisted",
                PromoteMode::Replace
            )
            .expect("覆盖"),
            1
        );

        drop(root);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
