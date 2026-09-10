//! SQL 解析与转译服务
//!
//! 基于 core/sql 模块提供 SQL 解析、验证、格式化和跨方言转译功能。
//! 本模块不直接依赖 sqlglot-rust，所有 SQL 处理通过 SqlEngine 间接调用。

use crate::core::sql::SqlEngine;
use specta::Type;

/// SQL 方言枚举（前端兼容）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum SqlDialect {
    Generic,
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

impl SqlDialect {
    fn to_engine_dialect(&self) -> crate::core::sql::SqlDialect {
        match self {
            SqlDialect::Generic => crate::core::sql::SqlDialect::Ansi,
            SqlDialect::Mysql => crate::core::sql::SqlDialect::Mysql,
            SqlDialect::Postgres => crate::core::sql::SqlDialect::Postgres,
            SqlDialect::Sqlite => crate::core::sql::SqlDialect::Sqlite,
            SqlDialect::Duckdb => crate::core::sql::SqlDialect::Duckdb,
            SqlDialect::MsSQL => crate::core::sql::SqlDialect::MsSQL,
            SqlDialect::Oracle => crate::core::sql::SqlDialect::Oracle,
            SqlDialect::Snowflake => crate::core::sql::SqlDialect::Snowflake,
            SqlDialect::BigQuery => crate::core::sql::SqlDialect::BigQuery,
            SqlDialect::Redshift => crate::core::sql::SqlDialect::Redshift,
        }
    }
}

/// SQL 解析结果
#[derive(Debug, serde::Serialize, Type)]
pub struct ParseResult {
    pub success: bool,
    pub error: Option<String>,
    pub statements_count: u32,
}

/// SQL 格式化请求
#[derive(Debug, serde::Deserialize, Type)]
pub struct FormatRequest {
    pub sql: String,
    pub dialect: Option<SqlDialect>,
}

/// SQL 格式化响应
#[derive(Debug, serde::Serialize, Type)]
pub struct FormatResponse {
    pub formatted_sql: String,
    pub success: bool,
    pub error: Option<String>,
}

/// SQL 转译请求
#[derive(Debug, serde::Deserialize, Type)]
pub struct TranspileRequest {
    pub sql: String,
    pub source_dialect: SqlDialect,
    pub target_dialect: SqlDialect,
}

/// SQL 转译响应
#[derive(Debug, serde::Serialize, Type)]
pub struct TranspileResponse {
    pub transpiled_sql: String,
    pub success: bool,
    pub error: Option<String>,
}

/// SQL 验证请求
#[derive(Debug, serde::Deserialize, Type)]
pub struct ValidateRequest {
    pub sql: String,
    pub dialect: Option<SqlDialect>,
}

/// SQL 验证响应
#[derive(Debug, serde::Serialize, Type)]
pub struct ValidateResponse {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// 解析 SQL
pub fn parse_sql(sql: &str, dialect: Option<SqlDialect>) -> ParseResult {
    let engine_dialect = dialect.unwrap_or(SqlDialect::Generic).to_engine_dialect();

    match SqlEngine::validate(sql, engine_dialect) {
        Ok(()) => ParseResult {
            success: true,
            error: None,
            statements_count: 1,
        },
        Err(e) => ParseResult {
            success: false,
            error: Some(e),
            statements_count: 0,
        },
    }
}

/// 格式化 SQL
pub fn format_sql(sql: &str, dialect: Option<SqlDialect>) -> FormatResponse {
    let engine_dialect = dialect.unwrap_or(SqlDialect::Generic).to_engine_dialect();
    let formatted = SqlEngine::format(sql, engine_dialect);

    FormatResponse {
        formatted_sql: formatted,
        success: true,
        error: None,
    }
}

/// 转译 SQL
pub fn transpile_sql(
    sql: &str,
    source_dialect: SqlDialect,
    target_dialect: SqlDialect,
) -> TranspileResponse {
    let source = source_dialect.to_engine_dialect();
    let target = target_dialect.to_engine_dialect();

    match SqlEngine::transpile(sql, source, target) {
        Ok(result) => TranspileResponse {
            transpiled_sql: result,
            success: true,
            error: None,
        },
        Err(e) => TranspileResponse {
            transpiled_sql: sql.to_string(),
            success: false,
            error: Some(e),
        },
    }
}

/// 验证 SQL
pub fn validate_sql(sql: &str, dialect: Option<SqlDialect>) -> ValidateResponse {
    let engine_dialect = dialect.unwrap_or(SqlDialect::Generic).to_engine_dialect();

    match SqlEngine::validate(sql, engine_dialect) {
        Ok(()) => ValidateResponse {
            valid: true,
            errors: vec![],
            warnings: vec![],
        },
        Err(e) => ValidateResponse {
            valid: false,
            errors: vec![e],
            warnings: vec![],
        },
    }
}

/// 分割 SQL 语句
pub fn split_sql(sql: &str, _dialect: Option<SqlDialect>) -> Vec<String> {
    // sqlglot-rust 的 parse 返回单个 Statement，不支持多语句分割
    // 使用简单的分号分割作为主要方案
    sql.split(';')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
