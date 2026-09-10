//! Round 31：mock_generator 集成测试——全链路生成落盘 + 类型映射 + 空列校验。

use std::path::PathBuf;

use rds_workbench::services::db_navigator::{NavColumn, NavTable};
use rds_workbench::services::mock_generator::{generate_for_table, parse_data_type};
use mock::models::ColumnDataType;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rds_mg_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn demo_table() -> NavTable {
    NavTable {
        name: "mock_orders".to_string(),
        columns: vec![
            NavColumn {
                name: "order_id".to_string(),
                data_type: "INTEGER".to_string(),
                is_primary_key: true,
                is_nullable: false,
            },
            NavColumn {
                name: "status".to_string(),
                data_type: "VARCHAR".to_string(),
                is_primary_key: false,
                is_nullable: true,
            },
            NavColumn {
                name: "amount".to_string(),
                data_type: "DECIMAL(12,2)".to_string(),
                is_primary_key: false,
                is_nullable: false,
            },
        ],
    }
}

#[test]
fn generate_for_table_writes_rows_to_duckdb() {
    let dir = temp_dir("write");
    let db_path = dir.join("analytics.duckdb");

    // 建表后必须显式 drop 连接（Windows 同一 DuckDB 文件仅 1 连接句柄）。
    {
        let conn = duckdb::Connection::open(&db_path).expect("open duckdb");
        conn.execute_batch(
            "CREATE TABLE mock_orders (
                 order_id INTEGER PRIMARY KEY,
                 status VARCHAR,
                 amount DECIMAL(12,2)
             );",
        )
        .expect("create table");
    }

    let out = generate_for_table(&db_path, &demo_table(), 50).expect("generate mock");
    assert_eq!(out.row_count, 50);

    let conn = duckdb::Connection::open(&db_path).expect("open duckdb");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM mock_orders", [], |r| r.get(0))
        .expect("count rows");
    assert_eq!(count, 50, "应写入 50 行");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generate_for_table_empty_columns_errors() {
    let dir = temp_dir("empty");
    let db_path = dir.join("analytics.duckdb");
    {
        let conn = duckdb::Connection::open(&db_path).expect("open duckdb");
        conn.execute_batch("CREATE TABLE empty_t (id INTEGER);").expect("create");
    }

    let table = NavTable {
        name: "empty_t".to_string(),
        columns: vec![],
    };
    let err = generate_for_table(&db_path, &table, 10).expect_err("should error");
    assert!(err.contains("没有可用的列"), "err: {err}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parse_data_type_maps_common_types() {
    assert!(matches!(
        parse_data_type("INTEGER"),
        ColumnDataType::Integer
    ));
    assert!(matches!(
        parse_data_type("VARCHAR(64)"),
        ColumnDataType::Varchar { .. }
    ));
    assert!(matches!(
        parse_data_type("DECIMAL(12,2)"),
        ColumnDataType::Decimal { .. }
    ));
    assert!(matches!(
        parse_data_type("BOOLEAN"),
        ColumnDataType::Boolean
    ));
    assert!(matches!(
        parse_data_type("TIMESTAMP"),
        ColumnDataType::Timestamp
    ));
    assert!(matches!(parse_data_type("WEIRD"), ColumnDataType::Text));
}
