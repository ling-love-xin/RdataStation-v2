//! Round 31：Mock 数据生成（M7）——基于导航树表结构生成测试数据，**仅落 DuckDB 分析引擎**。
//!
//! 复用 mock crate 全链路：`MockEngine::map_column`（列类型 → 生成器）→
//! `MockEngine::generate`（内存临时表生成，确定性 seed=42）→
//! `MockEngine::export(SqlInsert)`（导出 INSERT 语句到临时文件）→
//! 在分析库文件连接中执行写入。M7 约束：数据不回传各源数据库。

use std::path::Path;

use mock::models::{ColumnDataType, GeneratorConfig, Locale, MockConfig};
use mock::MockEngine;

use crate::services::db_navigator::NavTable;

/// 源库列类型字符串 → mock 列数据类型（宽松匹配，未知类型回退 Text）。
pub fn parse_data_type(s: &str) -> ColumnDataType {
    let up = s.to_uppercase();
    if up.contains("VARCHAR") || up.contains("CHAR") {
        ColumnDataType::Varchar { length: None }
    } else if up == "INTEGER" || up == "INT" || up == "TINYINT" || up == "SMALLINT" {
        ColumnDataType::Integer
    } else if up == "BIGINT" {
        ColumnDataType::BigInt
    } else if up.starts_with("DECIMAL") || up == "NUMERIC" {
        ColumnDataType::Decimal {
            precision: 18,
            scale: 2,
        }
    } else if up == "BOOLEAN" || up == "BOOL" {
        ColumnDataType::Boolean
    } else if up == "DOUBLE" {
        ColumnDataType::Double
    } else if up == "FLOAT" || up == "REAL" {
        ColumnDataType::Float
    } else if up == "DATE" {
        ColumnDataType::Date
    } else if up == "TIMESTAMP" || up == "DATETIME" {
        ColumnDataType::Timestamp
    } else if up == "UUID" {
        ColumnDataType::Uuid
    } else if up == "BLOB" {
        ColumnDataType::Blob
    } else {
        ColumnDataType::Text
    }
}

/// Mock 生成结果（行数 + 耗时）。
#[derive(Debug, Clone)]
pub struct MockOutcome {
    pub row_count: u32,
    pub elapsed_ms: u32,
}

/// 基于导航表结构，向指定 DuckDB 分析库生成并写入 `row_count` 行测试数据。
pub fn generate_for_table(
    duckdb_path: &Path,
    table: &NavTable,
    row_count: u32,
) -> Result<MockOutcome, String> {
    if table.columns.is_empty() {
        return Err(format!("表 {} 没有可用的列定义", table.name));
    }

    // 1) 列类型 → mock 生成器（自动选择，按列名/类型猜测）。
    let mut columns = Vec::new();
    for c in &table.columns {
        let resp = MockEngine::map_column(&c.name, &c.data_type)
            .map_err(|e| format!("列 {} 生成器映射失败: {}", c.name, e))?;
        columns.push(mock::models::ColumnDef {
            name: c.name.clone(),
            data_type: parse_data_type(&c.data_type),
            generator: resp.generator,
            nullable_ratio: if c.is_nullable { 0.1 } else { 0.0 },
            unique: c.is_primary_key,
            dependency: None,
        });
    }

    let config = MockConfig {
        table_name: table.name.clone(),
        row_count,
        seed: Some(42),
        locale: Locale::ZhCn,
        columns,
    };

    // 2) 内存临时表生成（mock crate 使用 engine 的 DuckDBManager 内存库）。
    let rt = tokio::runtime::Runtime::new().map_err(|e| format!("创建运行时失败: {e}"))?;
    let result = rt
        .block_on(MockEngine::generate(config))
        .map_err(|e| format!("Mock 生成失败: {e}"))?;

    // 3) 导出为 INSERT 语句到临时文件。
    let tmp = std::env::temp_dir().join(format!(
        "rds_mock_{}_{}.sql",
        table.name,
        std::process::id()
    ));
    MockEngine::export(
        &result.temp_table_name,
        &mock::models::MockExportFormat::SqlInsert,
        Some(tmp.to_str().ok_or("临时路径无效")?),
        Some(&table.name),
    )
    .map_err(|e| format!("导出 INSERT 失败: {e}"))?;

    // 4) 读回并在分析库文件连接中执行（M7：仅落分析引擎，不回传源库）。
    let sql_text = std::fs::read_to_string(&tmp).map_err(|e| format!("读取 INSERT 失败: {e}"))?;
    let conn = duckdb::Connection::open(duckdb_path).map_err(|e| format!("打开分析库失败: {e}"))?;
    conn.execute_batch(&sql_text)
        .map_err(|e| format!("写入分析库失败: {e}"))?;

    let _ = std::fs::remove_file(&tmp);
    Ok(MockOutcome {
        row_count: result.row_count,
        elapsed_ms: result.elapsed_ms,
    })
}

// 兼容导出（避免未使用告警）：列映射回读为生成器配置。
#[allow(dead_code)]
fn _generator_of(_: &GeneratorConfig) -> &'static str {
    "auto"
}
