use super::builder;
use super::formatter;
use super::parser;
use super::transpiler;

/// SQL 方言枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SqlDialect {
    Ansi,
    Mysql,
    Postgres,
    Sqlite,
    Duckdb,
    MsSQL,
    Oracle,
    Snowflake,
    BigQuery,
    Redshift,
}

/// SQL 语句类型（智能路由用）
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum SqlStatementType {
    Select,
    Insert,
    Update,
    Delete,
    Ddl,
    Unknown,
}

/// 列定义信息（用于 DDL 生成）
#[derive(Debug, Clone, serde::Serialize)]
pub struct ColumnDefInfo {
    pub name: String,
    pub data_type: String,
    pub unique: bool,
    pub nullable: bool,
}

/// ALTER TABLE 操作类型
#[derive(Debug, Clone)]
pub enum AlterOperation {
    AddColumn(ColumnDefInfo),
    DropColumn(String),
    RenameColumn { old_name: String, new_name: String },
    ModifyColumn(ColumnDefInfo),
}

/// DDL 解析结果（预留）
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DdlInfo {
    pub table_name: String,
    pub columns: Vec<ColumnDefInfo>,
    pub constraints: Vec<String>,
}

/// SQL 引擎 —— sqlglot-rust 的唯一接入点
///
/// 所有 SQL 处理能力通过此结构体的静态方法暴露。
/// 业务模块只 `use crate::core::sql::SqlEngine`，不直接依赖 sqlglot-rust。
pub struct SqlEngine;

impl SqlEngine {
    // ==================== 当前需要的（完整实现） ====================

    /// 解析 SQL 并识别语句类型（智能路由）
    ///
    /// 返回 `(语句类型, 规范化后的 SQL)`。
    /// 解析失败时返回 `(Unknown, 原始 SQL)`。
    pub fn parse_and_route(sql: &str, dialect: SqlDialect) -> (SqlStatementType, String) {
        parser::parse_and_route(sql, dialect)
    }

    /// 验证 SQL 语法
    ///
    /// 返回 `Ok(())` 表示语法有效，`Err(msg)` 表示语法错误。
    pub fn validate(sql: &str, dialect: SqlDialect) -> Result<(), String> {
        parser::validate(sql, dialect)
    }

    /// 生成 CREATE TABLE DDL
    ///
    /// 参数：
    /// - `table`: 表名
    /// - `columns`: 列定义列表
    /// - `if_not_exists`: 是否添加 IF NOT EXISTS
    pub fn build_create_table(
        table: &str,
        columns: &[ColumnDefInfo],
        if_not_exists: bool,
    ) -> String {
        builder::build_create_table(table, columns, if_not_exists)
    }

    /// 生成 DROP TABLE DDL
    pub fn build_drop_table(table: &str, if_exists: bool) -> String {
        builder::build_drop_table(table, if_exists)
    }

    /// 生成 CREATE TABLE AS SELECT DDL
    pub fn build_create_table_as_select(table: &str, select_sql: &str) -> String {
        builder::build_create_table_as_select(table, select_sql)
    }

    /// 生成 INSERT INTO DML
    ///
    /// 参数：
    /// - `table`: 目标表名
    /// - `columns`: 列名列表
    /// - `values`: 值矩阵，每行为一个 `Vec<String>`，`"NULL"` 字符串表示 NULL 值
    pub fn build_insert(table: &str, columns: &[String], values: &[Vec<String>]) -> String {
        builder::build_insert(table, columns, values)
    }

    /// 生成 SELECT * FROM table 查询
    pub fn build_select_all(table: &str, limit: Option<i64>) -> String {
        builder::build_select_all(table, limit)
    }

    /// 生成 SELECT cols FROM table 查询
    pub fn build_select(table: &str, columns: &[&str], limit: Option<i64>) -> String {
        builder::build_select(table, columns, limit)
    }

    /// 生成 ALTER TABLE DDL
    pub fn build_alter_table(table: &str, operations: &[AlterOperation]) -> String {
        builder::build_alter_table(table, operations)
    }

    /// 生成 CREATE INDEX DDL
    pub fn build_create_index(name: &str, table: &str, columns: &[String], unique: bool) -> String {
        builder::build_create_index(name, table, columns, unique)
    }

    /// SQL 格式化（美化打印）
    ///
    /// 解析失败时返回原始 SQL（优雅降级）。
    pub fn format(sql: &str, dialect: SqlDialect) -> String {
        formatter::format(sql, dialect)
    }

    /// 方言转换
    ///
    /// 将 SQL 从源方言转换为目标方言。
    /// 转换失败时返回错误信息。
    pub fn transpile(sql: &str, source: SqlDialect, target: SqlDialect) -> Result<String, String> {
        transpiler::transpile(sql, source, target)
    }

    // ==================== 近期需要的（预留） ====================

    /// SQL 优化（预留）
    ///
    /// 对 SQL 进行等价改写优化，如谓词下推、子查询展开等。
    pub fn optimize(sql: &str, _dialect: SqlDialect) -> String {
        tracing::warn!("SQL optimizer not yet implemented, returning original SQL");
        sql.to_string()
    }

    /// 解析 DDL 语句（预留）
    ///
    /// 从 CREATE TABLE / ALTER TABLE 语句中提取表名、列定义、约束信息。
    pub fn parse_ddl(_sql: &str, _dialect: SqlDialect) -> DdlInfo {
        tracing::warn!("DDL parser not yet implemented, returning empty DdlInfo");
        DdlInfo::default()
    }
}
