//! 共享临时区（`rds_tmp` schema）的落点规则：**建表进 `rds_tmp`，名字照旧按裸名用**。
//!
//! 为什么要这一层：单实例之后"跨会话共享的临时结果"有唯一去处 `rds_tmp`
//! （`docs/architecture/database/analysis-instance-sessions.md` §2）；但引用侧早就把表名按
//! 裸标识符拼进 SQL（洞察的规则 / 直方图 / 列统计模板是 `SELECT … FROM "{table}"`），
//! 返回限定名 `rds_tmp.tmp_i_x` 会被双引号当成"名字里带点的表"。
//!
//! 解法是 **`search_path`**（实测 2.0.1 可用：`SET search_path = 'main,rds_tmp'` 之后
//! 裸名与 `"双引号名"` 都解析到 `rds_tmp` 下的表）：物理表进 `rds_tmp`，
//! 调用方拿到的仍是裸名，既有模板零改动。
//!
//! 模块名不叫 `shared`：`crates/engine` 已有一个顶层 `shared` crate 模块
//! （`shared::error::CoreError` 到处在用），同目录再挂 `shared` 会让 `use shared::…` 二义（E0659）。
pub const SHARED_TEMP_SCHEMA: &str = "rds_tmp";

/// 让这条连接能把 `rds_tmp` 里的表按裸名解析（幂等；第一次顺带建 schema）。
///
/// 用在三处：实例控制连接（`open_tagged`）、每条会话（`open_session`，克隆虽继承设置，
/// 但显式设一次更稳）、以及**单测里直接 `open_in_memory` 的连接**（它们没有实例）。
pub fn ensure_shared_schema(conn: &duckdb::Connection) -> Result<(), String> {
    conn.execute_batch(&format!(
        "CREATE SCHEMA IF NOT EXISTS {SHARED_TEMP_SCHEMA}; \
         SET search_path = 'main,{SHARED_TEMP_SCHEMA}'"
    ))
    .map_err(|error| format!("准备 {SHARED_TEMP_SCHEMA} 共享临时区失败：{error}"))
}

/// 建表 / 删表用的限定名（`rds_tmp."名字"`）。保留给需要"钉死落点"的场合（清理、诊断）。
pub fn qualified(name: &str) -> String {
    format!("{SHARED_TEMP_SCHEMA}.\"{}\"", name.replace('"', "\"\""))
}
