//! SQL 解析与转译命令
//!
//! 提供 SQL 解析、验证、格式化和跨方言转译功能

use crate::core::error::CoreError;
use crate::core::services::sql_parser_service::{
    self, FormatRequest, SqlDialect, TranspileRequest, ValidateRequest,
};

/// 解析 SQL
#[tauri::command]
#[specta::specta]
pub fn parse_sql(
    sql: String,
    dialect: Option<SqlDialect>,
) -> Result<sql_parser_service::ParseResult, CoreError> {
    Ok(sql_parser_service::parse_sql(&sql, dialect))
}

/// 格式化 SQL
#[tauri::command]
#[specta::specta]
pub fn format_sql(input: FormatRequest) -> Result<sql_parser_service::FormatResponse, CoreError> {
    Ok(sql_parser_service::format_sql(&input.sql, input.dialect))
}

/// 转译 SQL
#[tauri::command]
#[specta::specta]
pub fn transpile_sql(
    input: TranspileRequest,
) -> Result<sql_parser_service::TranspileResponse, CoreError> {
    Ok(sql_parser_service::transpile_sql(
        &input.sql,
        input.source_dialect,
        input.target_dialect,
    ))
}

/// 验证 SQL
#[tauri::command]
#[specta::specta]
pub fn validate_sql(
    input: ValidateRequest,
) -> Result<sql_parser_service::ValidateResponse, CoreError> {
    Ok(sql_parser_service::validate_sql(&input.sql, input.dialect))
}

/// 分割 SQL 语句
#[tauri::command]
#[specta::specta]
pub fn split_sql(
    sql: String,
    dialect: Option<sql_parser_service::SqlDialect>,
) -> Result<Vec<String>, CoreError> {
    Ok(sql_parser_service::split_sql(&sql, dialect))
}
