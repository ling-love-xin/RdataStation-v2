//! 分析内核 · 物化（形态 I / II 的唯一实现）。
//!
//! 两种形态在装配上完全一样：**在会话里建一张表，内容来自一条 SELECT**。
//! 差别只在名字由谁给：
//!
//! - **形态 I（洞察取样）**：名字走仓库既有的临时表规范
//!   （`temp_table::generate_unique_name(TempTableSource::Insight, 描述)` → `tmp_i_…`），
//!   不自己拼前缀——TTL / 上限 / 按来源清理全靠那个前缀识别；
//! - **形态 II（结果集二次分析）**：名字由调用方给（用户看得见的那张表），
//!   这里只做标识符校验。
//!
//! # 已知边界（不装作没有）
//!
//! 会话把 `Connection` 藏在私有 `Mutex` 里，只对外给 `run`。所以这一层**只建表**，
//! 不做 [`temp_table`] 的 TTL 登记（登记需要 `&Connection`）。后果：表随会话存活，
//! 会话在进程级缓存里长期存在时不会自己消失。清理责任的落点：
//!
//! - 会话销毁（`drop_session` / 进程退出）——自动；
//! - 宿主显式 [`drop_materialized`]——洞察重跑、结果集被丢弃时该调它。
//!
//! 要把它彻底收进 TTL 体系，出路是给两种会话各加一个 `with_connection(closure)`
//! 只读借用入口，让内核能调 `temp_table::create_analysis_temp_table_as`。那会动会话
//! 的公开面，留给「刀 5 接洞察」时按实际需要做。

use shared::error::{CommonError, CoreError};

use crate::duckdb::temp_table::{generate_unique_name, TempTableSource};

use super::kernel::SessionHandle;

/// 物化落在 **内存 catalog** 上。
///
/// 会话的默认 catalog 指向只读挂载的源，未限定名建表会被 DuckDB 拒（实测：
/// `Cannot execute statement of type "CREATE" on database "rds_src" which is attached
/// in read-only mode!`）。所以建表一律写 `memory.<name>`，**返回值也带这个前缀**——
/// 调用方拿裸名去查会报「表不存在」，限定名的责任在物化这一侧。
pub const MATERIALIZE_CATALOG: &str = "memory";

/// 形态 I 的表名：走仓库临时表规范（`tmp_i_<描述>_<时间戳>_<随机>`）。
///
/// `description` 用「源的可读名」（表名 / 文件名），只保留字母数字下划线。
pub fn sample_table_name(description: &str) -> String {
    generate_unique_name(TempTableSource::Insight, description)
}

/// 表名合法性：只允许 `[A-Za-z_][A-Za-z0-9_]*`。
///
/// 这条校验不是洁癖：表名会拼进 `CREATE TABLE {name} AS …`，允许引号 / 空格 / 分号
/// 就等于把注入面开给了「用户给表起名」这个入口。
fn validate_identifier(name: &str) -> Result<(), CoreError> {
    if name.is_empty() {
        return Err(invalid("表名不能为空".to_string()));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap_or_default();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(invalid(format!(
            "表名 {name} 不合法：首字符只能是字母或下划线"
        )));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(invalid(format!(
            "表名 {name} 不合法：只允许字母、数字、下划线（不要引号 / 空格 / 分号）"
        )));
    }
    Ok(())
}

/// 在会话里建（或替换）一张表，内容来自 `select_sql`；返回 `memory.<name>`。
///
/// 用 `CREATE OR REPLACE` 而不是 `CREATE`：同一张表重复物化是常态（用户来回点），
/// 撞名报错只会把「重复」包装成「失败」。
pub fn materialize_in_session(
    handle: &SessionHandle,
    name: &str,
    select_sql: &str,
) -> Result<String, CoreError> {
    if !handle.has_session() {
        return Err(invalid(
            "源库直连档不能物化：没有 DuckDB 会话，取样应走驱动拉数".to_string(),
        ));
    }
    validate_identifier(name)?;
    let select = select_sql.trim();
    if select.is_empty() {
        return Err(invalid("物化需要一条 SELECT：空 SQL 建不出表".to_string()));
    }
    // 尾分号会让 `CREATE … AS <sql>;` 变成两条语句，DuckDB 直接拒（多语句 API 才行）
    let select = select.trim_end_matches(';').trim_end();
    handle.run(&format!(
        "CREATE OR REPLACE TABLE {MATERIALIZE_CATALOG}.{name} AS {select}"
    ))?;
    Ok(format!("{MATERIALIZE_CATALOG}.{name}"))
}

/// 形态 I 的便捷入口：按描述生成规范表名再物化。
pub fn materialize_sample(
    handle: &SessionHandle,
    description: &str,
    select_sql: &str,
) -> Result<String, CoreError> {
    let name = sample_table_name(description);
    materialize_in_session(handle, &name, select_sql)
}

/// 丢掉一张物化表（幂等）。`qualified` 接受 `memory.x` 或裸名 `x`。
///
/// 前缀守卫与 `temp_table::drop_temp_table` 同口径：只允许删 `tmp_i_` / `tmp_q_`
/// （洞察取样 / 查询结果），放开就等于给了一把「传什么名字删什么表」的刀。
pub fn drop_materialized(handle: &SessionHandle, qualified: &str) -> Result<(), CoreError> {
    let name = qualified
        .strip_prefix(&format!("{MATERIALIZE_CATALOG}."))
        .unwrap_or(qualified);
    let allowed = ["tmp_i_", "tmp_q_"];
    if !allowed.iter().any(|prefix| name.starts_with(prefix)) {
        return Err(invalid(format!(
            "拒绝删除非临时表：{name}（只允许 {} 前缀）",
            allowed.join(" / ")
        )));
    }
    // 源库档没有会话：前缀校验照做（守卫不能被「没有会话」这条捷径跳过），之后才是空操作
    if !handle.has_session() {
        return Ok(());
    }
    handle.run(&format!(
        "DROP TABLE IF EXISTS {MATERIALIZE_CATALOG}.{name}"
    ))?;
    Ok(())
}

fn invalid(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

#[cfg(test)]
mod tests {
    use super::{drop_materialized, materialize_in_session, materialize_sample, sample_table_name};
    use crate::analysis::kernel::SessionHandle;

    #[test]
    fn sample_name_follows_the_repo_temp_table_convention() {
        let name = sample_table_name("orders");
        assert!(name.starts_with("tmp_i_"), "实际 {name}");
        assert!(name.contains("orders"), "描述要进名字，便于人在库里认出来");
        // 每次生成都必须不同：同一秒内建两张也不会撞名（仓库口径：时间戳 + 随机尾巴）
        assert_ne!(sample_table_name("orders"), sample_table_name("orders"));
    }

    #[test]
    fn sample_name_sanitizes_description() {
        let name = sample_table_name("a b;drop table x");
        assert!(name.starts_with("tmp_i_"));
        assert!(!name.contains(' '), "空格不应进表名：{name}");
        assert!(!name.contains(';'), "分号不应进表名：{name}");
    }

    #[test]
    fn source_channel_cannot_materialize() {
        let err = materialize_in_session(&SessionHandle::None, "t", "select 1").unwrap_err();
        assert!(format!("{err}").contains("没有 DuckDB 会话"));
    }

    #[test]
    fn identifier_validation_rejects_bad_names() {
        assert!(super::validate_identifier("orders_2024").is_ok());
        assert!(super::validate_identifier("_tmp").is_ok());
        assert!(super::validate_identifier("").is_err());
        assert!(super::validate_identifier("1abc").is_err());
        assert!(super::validate_identifier("a b").is_err());
        assert!(super::validate_identifier("a;b").is_err());
    }

    #[test]
    fn materialize_sample_requires_a_session() {
        assert!(materialize_sample(&SessionHandle::None, "orders", "select 1").is_err());
    }

    #[test]
    fn drop_requires_a_temp_prefix() {
        // 前缀守卫：不能拿它删用户表
        let err = drop_materialized(&SessionHandle::None, "memory.orders").unwrap_err();
        assert!(format!("{err}").contains("拒绝删除非临时表"));
        // 源库档没有会话 → 幂等成功（没什么可删）
        assert!(drop_materialized(&SessionHandle::None, "memory.tmp_i_x").is_ok());
    }
}
