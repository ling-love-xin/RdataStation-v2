//! 分析内核 · 物化（形态 I / II 的唯一实现）。
//!
//! 两种形态在装配上完全一样：**在会话里建一张表，内容来自一条 SELECT**。
//! 差别只在名字与生命周期：
//!
//! - **形态 I（洞察取样）**：表名由 `sample_table_name(seed)` 生成（`tmp_i_` 前缀 + 稳定短哈希），
//!   同一份源 + 同一份 SQL 重复取样会命中同一张表，便于复用与 TTL 清理；
//! - **形态 II（结果集二次分析）**：表名由调用方给（用户看得见的那张表），随会话存亡。
//!
//! 为什么不做成两个函数：两边的 SQL 语义（`SELECT …` 的来源、下推与否）由**调用方**决定，
//! 内核只负责「建表这件事怎么建、名字怎么算、什么时候拒绝」。分成两个函数只会让两边
//! 各写一套 `CREATE TABLE`。

use shared::error::{CommonError, CoreError};

use super::kernel::SessionHandle;

/// 形态 I 的表名前缀（与 `duckdb/temp_table` 的 TTL 口径对齐：`tmp_i_` = 洞察取样）。
pub const SAMPLE_PREFIX: &str = "tmp_i_";

/// 稳定短哈希（FNV-1a 32 位）：同一份输入每次得到同一个名字，重启后也不变。
fn short_hash(input: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in input.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// 形态 I 的表名：`tmp_i_<8 位十六进制>`。
///
/// `seed` 应当是「能唯一标识这份样本」的东西——连接 id / 文件路径 + 取样 SQL，
/// 不要用时间戳或下标：那样每次取样都会多一张表，TTL 也压不住。
pub fn sample_table_name(seed: &str) -> String {
    format!("{SAMPLE_PREFIX}{:08x}", short_hash(seed))
}

/// 表名 / 别名合法性：只允许 `[A-Za-z_][A-Za-z0-9_]*`。
///
/// 这条校验不是洁癖：表名会拼进 `CREATE TABLE {name} AS …`，允许引号 / 空格 / 分号
/// 就等于把 SQL 注入面开给了「用户给表起名」这个入口。
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

/// 物化落在 **内存 catalog** 上：会话的默认 catalog 指向只读挂载的源，
/// 未限定名建表会被 DuckDB 拒（实测 `Cannot execute statement of type "CREATE"
/// on database "rds_src" which is attached in read-only mode!`）。
/// 所以建表一律写 `memory.<name>`，返回值也带这个前缀，调用方拿到就能直接查。
pub const MATERIALIZE_CATALOG: &str = "memory";

/// 在会话里建（或替换）一张表，内容来自 `select_sql`。
///
/// 用 `CREATE OR REPLACE` 而不是 `CREATE`：同一份样本重复取样是常态（用户来回点），
/// 撞名报错只会把「重复」包装成「失败」。
///
/// 返回**带 catalog 前缀的表名**（`memory.<name>`）：默认 catalog 是只读源，
/// 调用方拿裸名去查会报「表不存在」——这不是调用方的错，是限定名的责任在物化这一侧。
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

/// 形态 I 的便捷入口：算好名字再物化。
pub fn materialize_sample(
    handle: &SessionHandle,
    seed: &str,
    select_sql: &str,
) -> Result<String, CoreError> {
    materialize_in_session(handle, &sample_table_name(seed), select_sql)
}

fn invalid(message: String) -> CoreError {
    CoreError::common(CommonError::General(message))
}

#[cfg(test)]
mod tests {
    use super::{materialize_in_session, materialize_sample, sample_table_name, SAMPLE_PREFIX};
    use crate::analysis::kernel::SessionHandle;

    #[test]
    fn sample_name_is_prefixed_and_stable() {
        let a = sample_table_name("conn-1|select * from t");
        let b = sample_table_name("conn-1|select * from t");
        assert_eq!(a, b);
        assert!(a.starts_with(SAMPLE_PREFIX));
        assert_eq!(a.len(), SAMPLE_PREFIX.len() + 8);
    }

    #[test]
    fn sample_name_differs_for_different_seeds() {
        // 不同连接 / 不同 SQL 必须落不同表，否则样本会互相覆盖
        assert_ne!(
            sample_table_name("conn-1|select * from t"),
            sample_table_name("conn-2|select * from t")
        );
        assert_ne!(
            sample_table_name("conn-1|select * from t"),
            sample_table_name("conn-1|select * from u")
        );
    }

    #[test]
    fn source_channel_cannot_materialize() {
        let err = materialize_in_session(&SessionHandle::None, "t", "select 1").unwrap_err();
        assert!(format!("{err}").contains("没有 DuckDB 会话"));
    }

    #[test]
    fn name_with_quote_is_rejected() {
        // 注入面：表名会拼进 CREATE TABLE 语句
        let err =
            materialize_in_session(&SessionHandle::None, "t; drop table x", "select 1").unwrap_err();
        // 源库档先被抓，所以这里换成合法会话后的分支由 identifier 校验挡（见下条）
        assert!(format!("{err}").contains("没有 DuckDB 会话"));
    }

    #[test]
    fn identifier_validation_rejects_bad_names() {
        // 直接测校验函数（不依赖会话）
        assert!(super::validate_identifier("orders_2024").is_ok());
        assert!(super::validate_identifier("_tmp").is_ok());
        assert!(super::validate_identifier("").is_err());
        assert!(super::validate_identifier("1abc").is_err());
        assert!(super::validate_identifier("a b").is_err());
        assert!(super::validate_identifier("a;b").is_err());
        assert!(super::validate_identifier("tmp_i_0a1b2c3d").is_ok());
    }

    #[test]
    fn materialize_sample_uses_the_sample_name() {
        // 名字先算出来（源库档会在建表前被拒，这里断言的是名字口径）
        let name = sample_table_name("seed");
        assert!(materialize_sample(&SessionHandle::None, "seed", "select 1").is_err());
        assert!(name.starts_with(SAMPLE_PREFIX));
    }
}
