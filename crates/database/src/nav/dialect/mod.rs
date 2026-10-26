//! L3 方言 SPI（方案稿 §4.4 / D1-A，B3）：对象身份 → 可执行 SQL / 引号文本。
//!
//! ## 职责边界
//!
//! 所有「把对象身份变成一条 SQL 或一段引号文本」的族特异逻辑从 `sql_gen.rs` /
//! `table_data.rs` 迁出到本层；**新增数据库类型 = 注册一个方言 provider**，
//! 应用层（菜单 / 预览 / 属性面板通用骨架）零 `match`、零新增分支。
//!
//! - 本模块只承载**纯字符串拼装**：无 I/O、无异步、不持连接状态（与 B2 能力层同一口径）；
//! - 展示用无引号中性限定名（`sql_gen::qualified_name`）属通用展示层，不进本 SPI；
//! - DML 模板、`CREATE TABLE` 合成器属通用模板层，不进本 SPI；
//! - `drop_sql` / `truncate_table_sql` 在 B6 落地（删除表 / 视图 / 序列、清空表）；
//!   `routine_source_sql` / `view_data_sql` 仍未实现，D24 补例程签名时按需补 trait 方法。
//!
//! ## 分派键
//!
//! 经 L1 档案 `family_profile::registry().family_of(driver).family` 归一（postgres / pg /
//! postgres_native → postgresql；mysql_native → mysql；未知族回退 fallback 通用方言），
//! 不在本层另写一套别名归一表。

use std::sync::OnceLock;

use engine::ObjectKind;
use shared::error::{CoreError, DatabaseError};

use crate::nav::family_profile::registry;

mod duckdb;
mod mysql;
mod postgresql;
mod sqlite;

/// L3 方言 SPI：一族数据库的标识符策略 + DDL / 内省辅助 SQL 生成。
///
/// 方法全为同步纯函数；返回 `Err(CoreError::Driver)` = 本族 / 本类型不支持，
/// 调用方（菜单门控 / 任务入队）据此提前拦截，不生成半截方言错误语句。
pub trait DialectSpi: Send + Sync {
    /// 族 id（归一后，与 L1 档案 `DbFamilyProfile.family` 同源）。
    fn family(&self) -> &'static str;

    /// 引号包裹单个标识符；内嵌引号按方言双写。
    ///
    /// 默认双引号（PG / SQLite / DuckDB）；MySQL provider 覆盖为反引号（现状欠账，B3 补齐）。
    fn quote_ident(&self, ident: &str) -> String {
        format!("\"{}\"", ident.replace('"', "\"\""))
    }

    /// 可执行限定名（`catalog.schema.name`）：空段跳过、schema 与 catalog 相同时去重，
    /// 各段走 [`Self::quote_ident`]。规则与现状 `table_data::quoted_qualified` 等价。
    ///
    /// 限定名拼法不随对象类别变化，只随族的引号策略变化；schema 段是否出现由入参空值决定，
    /// 不由族策略隐藏。rename 专用的 schema 前缀策略（PG / MySQL 带、SQLite / DuckDB 不带）
    /// 由各 provider 在 `rename_sql` 内部实现，不走本方法。
    fn qualified_sql(&self, catalog: &str, schema: &str, name: &str) -> String {
        let mut parts: Vec<String> = Vec::new();
        if !catalog.is_empty() {
            parts.push(self.quote_ident(catalog));
        }
        if !schema.is_empty() && schema != catalog {
            parts.push(self.quote_ident(schema));
        }
        parts.push(self.quote_ident(name));
        parts.join(".")
    }

    /// 对象重命名 DDL（拍板 Q1-A：身份参数用 `engine::ObjectKind`，与 L2 / L4 同源）。
    ///
    /// - `schema`：所属模式（无独立 Schema 层的族收到 `None` 或空串）；
    /// - `parent`：父对象限定名（仅触发器 `ON <父表>` 需要，缺失时 provider 报 Err）；
    /// - 本族 / 本类型不支持时返回显式 `Err`，不生成半截 SQL。
    fn rename_sql(
        &self,
        kind: ObjectKind,
        schema: Option<&str>,
        old: &str,
        parent: Option<&str>,
        new: &str,
    ) -> Result<String, CoreError>;

    /// 删除对象 DDL（表 / 视图 / 序列）。不支持的类别 / 族返回 `Err(CoreError::Driver)`，
    /// 不生成半截 SQL；标识符引号由各 provider 经 `quote_ident` 转义。
    ///
    /// - `schema`：所属模式（无独立 Schema 层的族收到 `None` 或空串，provider 不拼前缀）；
    /// - `parent`：父对象限定名预留位（当前 Drop 族 v1 不消费，结构保留供触发器等扩展）。
    fn drop_sql(
        &self,
        kind: ObjectKind,
        schema: Option<&str>,
        name: &str,
        parent: Option<&str>,
    ) -> Result<String, CoreError>;

    /// 清空表 DDL（仅 Table）。SQLite 无 `TRUNCATE` 语句，provider 返回 `DELETE FROM` 形态；
    /// 非表类型 / 不支持的族返回 `Err`。
    fn truncate_table_sql(&self, schema: Option<&str>, name: &str) -> Result<String, CoreError>;

    /// 目录级行估计 SQL；`None` = 本族不做后台估计（不发真 `COUNT(*)`，大库逐表 COUNT
    /// 秒级以上，与「名单先上屏」目标冲突）。schema 名单引号转义由 provider 负责。
    fn row_estimate_sql(&self, schema: &str) -> Option<String> {
        let _ = schema;
        None
    }

    /// 目录级表数据段大小 SQL；`None` = 本族不查大小（徽标安静消失，不报错）。
    ///
    /// 返回两列：表名 + 数据段字节数（可为 `NULL`，由任务层解析为 `None`）；schema 转义
    /// 口径与 [`Self::row_estimate_sql`] 一致。本批只接数据段体积，不接索引体积。
    fn table_size_sql(&self, schema: &str) -> Option<String> {
        let _ = schema;
        None
    }
}

/// 构造「驱动不支持」错误（provider 共用；`operation` 传当前 trait 方法名，便于错误定位，
/// source 写族特异原因）。
pub(crate) fn unsupported_err(
    family: &str,
    operation: &str,
    source: impl Into<String>,
) -> CoreError {
    CoreError::database(DatabaseError::Driver {
        db_type: family.to_string(),
        operation: operation.to_string(),
        source: source.into(),
    })
}

/// 方言注册表：族 id → provider；未注册族回退内置通用 fallback 方言。
pub struct DialectRegistry {
    providers: Vec<Box<dyn DialectSpi>>,
    fallback: Box<dyn DialectSpi>,
}

impl Default for DialectRegistry {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            fallback: Box::new(FallbackDialect),
        }
    }
}

impl DialectRegistry {
    /// 注册一族方言 provider（新增数据库族在全局初始化时调用一次）。
    pub fn register(&mut self, provider: Box<dyn DialectSpi>) {
        self.providers.push(provider);
    }

    /// 按驱动 id 分派方言；别名经 L1 档案归一，未注册族返回 fallback 通用方言。
    pub fn dialect_of(&self, type_id: &str) -> &dyn DialectSpi {
        let family = registry().family_of(type_id).family;
        self.providers
            .iter()
            .find(|p| p.family() == family)
            .map(|p| p.as_ref())
            .unwrap_or(self.fallback.as_ref())
    }
}

/// 未知族回退方言：双引号策略；不提供重命名 / 删除 / 清空 / 行估计（安全网，不生成方言错误语句）。
struct FallbackDialect;

impl DialectSpi for FallbackDialect {
    fn family(&self) -> &'static str {
        "fallback"
    }

    fn rename_sql(
        &self,
        _kind: ObjectKind,
        _schema: Option<&str>,
        _old: &str,
        _parent: Option<&str>,
        _new: &str,
    ) -> Result<String, CoreError> {
        Err(unsupported_err(
            "fallback",
            "rename_sql",
            "该驱动暂不支持对象重命名",
        ))
    }

    fn drop_sql(
        &self,
        _kind: ObjectKind,
        _schema: Option<&str>,
        _name: &str,
        _parent: Option<&str>,
    ) -> Result<String, CoreError> {
        Err(unsupported_err(
            "fallback",
            "drop_sql",
            "该驱动暂不支持删除对象",
        ))
    }

    fn truncate_table_sql(&self, _schema: Option<&str>, _name: &str) -> Result<String, CoreError> {
        Err(unsupported_err(
            "fallback",
            "truncate_table_sql",
            "该驱动暂不支持清空表",
        ))
    }
}

/// 全局方言注册表（首次访问时注册全部内置族 provider）。
///
/// 新增数据库类型方言 = 在 `get_or_init` 闭包里加一行 `register`；
/// 消费方（`sql_gen` / `table_data` / 任务入队判定）零改动。
pub fn dialect_of(type_id: &str) -> &'static dyn DialectSpi {
    static REG: OnceLock<DialectRegistry> = OnceLock::new();
    let reg = REG.get_or_init(|| {
        let mut registry = DialectRegistry::default();
        postgresql::register(&mut registry);
        mysql::register(&mut registry);
        sqlite::register(&mut registry);
        duckdb::register(&mut registry);
        registry
    });
    reg.dialect_of(type_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::ObjectKind;

    /// 分派键归一：postgres / pg / postgres_native 归 postgresql；mysql_native 归 mysql；
    /// 未知族回退 fallback。
    #[test]
    fn dialect_of_normalizes_aliases_and_unknown_falls_back() {
        for d in ["postgresql", "postgres", "pg", "postgres_native", "PG"] {
            assert_eq!(dialect_of(d).family(), "postgresql", "{d} 应归 postgresql");
        }
        for d in ["mysql", "mysql_native", "MySQL"] {
            assert_eq!(dialect_of(d).family(), "mysql", "{d} 应归 mysql");
        }
        assert_eq!(dialect_of("sqlite").family(), "sqlite");
        assert_eq!(dialect_of("duckdb").family(), "duckdb");
        assert_eq!(dialect_of("redis").family(), "fallback");
    }

    /// fallback 方言：双引号策略；重命名 / 删除 / 清空 / 行估计一律拒绝或 None。
    #[test]
    fn fallback_dialect_safe_defaults() {
        let dialect = dialect_of("redis");
        assert_eq!(dialect.quote_ident("orders"), "\"orders\"");
        assert_eq!(dialect.qualified_sql("", "", "orders"), "\"orders\"");
        assert!(dialect
            .rename_sql(ObjectKind::Table, None, "t", None, "t2")
            .is_err());
        assert!(dialect
            .drop_sql(ObjectKind::Table, None, "t", None)
            .is_err());
        assert!(dialect
            .truncate_table_sql(None, "t")
            .is_err());
        assert!(dialect.row_estimate_sql("main").is_none());
        assert!(dialect.table_size_sql("main").is_none());
    }

    /// 通用三段限定名：空段跳过、schema==catalog 去重（与现状 `quoted_qualified` 等价）。
    #[test]
    fn qualified_sql_dedupes_repeated_segment_and_skips_empty() {
        let dialect = dialect_of("postgresql");
        assert_eq!(dialect.qualified_sql("main", "main", "orders"), "\"main\".\"orders\"");
        assert_eq!(
            dialect.qualified_sql("cat", "sch", "orders"),
            "\"cat\".\"sch\".\"orders\""
        );
        assert_eq!(dialect.qualified_sql("", "", "orders"), "\"orders\"");
        assert_eq!(dialect.qualified_sql("", "main", "orders"), "\"main\".\"orders\"");
    }
}
