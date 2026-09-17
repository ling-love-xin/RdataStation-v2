use super::builder;
use super::formatter;
use super::parser;
use super::split;
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
/// 业务模块只 `use crate::sql::SqlEngine`，不直接依赖 sqlglot-rust。
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

    /// 语句切分（词法级）
    ///
    /// 返回每条语句在原文中的区间与起始行号，供「执行当前语句 / 批量执行 / 状态栏语句数」使用。
    /// 与解析无关：文本可以是未写完的状态（解析失败不能成为不能切分的理由）。
    pub fn split_statements(sql: &str) -> Vec<split::SqlStatement> {
        split::split_statements(sql)
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

    /// 生成跨库版本的 `CREATE TABLE`（目标为 `ATTACH` 进来的文件库，见 [`QualifiedTable`]）
    ///
    /// 参数：
    /// - `target`: 三段式表名（`catalog` = `ATTACH ... AS` 的别名）
    /// - `columns`: 列定义列表
    /// - `if_not_exists`: 是否添加 IF NOT EXISTS
    pub fn build_create_table_in(
        target: &builder::QualifiedTable<'_>,
        columns: &[ColumnDefInfo],
        if_not_exists: bool,
    ) -> String {
        builder::build_create_table_in(target, columns, if_not_exists)
    }

    /// 生成跨库版本的 `DROP TABLE`（写入失败时回滚刚建的表）
    pub fn build_drop_table_in(target: &builder::QualifiedTable<'_>, if_exists: bool) -> String {
        builder::build_drop_table_in(target, if_exists)
    }

    /// 生成 `ATTACH '<path>' AS "<alias>"`（DuckDB 专有，sqlglot AST 无此语句）
    pub fn build_attach_database(path: &str, alias: &str) -> String {
        builder::build_attach_database(path, alias)
    }

    /// 生成 `DETACH "<alias>"`
    pub fn build_detach_database(alias: &str) -> String {
        builder::build_detach_database(alias)
    }

    /// 生成 `INSERT INTO <target> (<cols>) SELECT <cols> FROM <source>`（跨库直写，不走 VALUES 文本）
    ///
    /// 参数：
    /// - `target`: 三段式表名（`catalog` = `ATTACH ... AS` 的别名）
    /// - `source_table`: 源**表名**（与 [`Self::build_create_table_as_select`] 同一约定）
    /// - `columns`: 列清单（插入列与选择列共用；目标表多出的列走默认值）
    pub fn build_insert_select(
        target: &builder::QualifiedTable<'_>,
        source_table: &str,
        columns: &[String],
    ) -> String {
        builder::build_insert_select(target, source_table, columns)
    }

    /// 生成 CREATE TABLE AS SELECT DDL
    ///
    /// 参数：
    /// - `table`: 新建表名
    /// - `source_table`: 源**表名**（不是 SELECT 语句）
    pub fn build_create_table_as_select(table: &str, source_table: &str) -> String {
        builder::build_create_table_as_select(table, source_table)
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
    ///
    /// **界面请用 [`Self::format_report`]**：它会告诉你“改了几条 / 哪几条没动”，
    /// 而这里只有文本，用户看不到“按了没反应”的真实原因。
    pub fn format(sql: &str, dialect: SqlDialect) -> String {
        formatter::format(sql, dialect)
    }

    /// 【B10】逐条格式化（带报告：改了几条 / 哪几条因为解析不了而逐字保留）
    ///
    /// 与 [`Self::format`] 的差别：它是**语句为单位**的（P0.4 的词法切分拿到区间，逐条
    /// 格式化再回填原位），所以脚本里有一句没写完，不影响其它语句被格式化。
    pub fn format_report(sql: &str, dialect: SqlDialect) -> formatter::FormatReport {
        formatter::format_with_report(sql, dialect)
    }

    /// 方言转换
    ///
    /// 将 SQL 从源方言转换为目标方言。
    /// 转换失败时返回错误信息。
    ///
    /// **只吃单条**：对脚本会静默丢弃第二条及以后的语句（架构 §12 #19）。
    /// 界面请用 [`Self::transpile_report`]。
    pub fn transpile(sql: &str, source: SqlDialect, target: SqlDialect) -> Result<String, String> {
        transpiler::transpile(sql, source, target)
    }

    /// 【B10】脚本级方言转译（带报告：翻了几条 / 哪几条因为解析不了而逐字保留）
    ///
    /// 与 [`Self::transpile`] 的差别：**先按词法切分再逐条转译**，脚本一条都不丢
    /// （整篇接口会静默截断——实测 `"SELECT 1; SELECT 2;"` → `"SELECT 1"`）。
    pub fn transpile_report(
        script: &str,
        source: SqlDialect,
        target: SqlDialect,
    ) -> transpiler::TranspileReport {
        transpiler::transpile_with_report(script, source, target)
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
