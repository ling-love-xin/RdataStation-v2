//! Round 26：query_runner 集成测试——临时 DuckDB 建表 + 数据 → 执行 SQL 读回。

use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rds_qr_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

#[test]
fn execute_sql_select_returns_columns_and_rows() {
    let dir = temp_dir("select");
    let db_path = dir.join("analytics.duckdb");

    let conn = duckdb::Connection::open(&db_path).expect("open duckdb");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS orders (
             order_id INTEGER,
             amount DECIMAL(12,2),
             status VARCHAR
         );
         INSERT INTO orders VALUES (1, 99.50, 'paid'), (2, 12.00, 'pending');",
    )
    .expect("seed data");
    drop(conn);

    let out = rds_workbench::services::query_runner::execute_sql(
        &db_path,
        "SELECT order_id, status FROM orders ORDER BY order_id",
    )
    .expect("execute sql");

    assert_eq!(out.columns, vec!["order_id", "status"]);
    assert_eq!(out.row_count, 2);
    assert_eq!(out.rows[0], vec!["1", "paid"]);
    assert_eq!(out.rows[1], vec!["2", "pending"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn execute_sql_errors_on_empty_sql() {
    let dir = temp_dir("empty");
    let db_path = dir.join("analytics.duckdb");
    let conn = duckdb::Connection::open(&db_path).expect("open duckdb");
    drop(conn);

    let err = rds_workbench::services::query_runner::execute_sql(&db_path, "   ")
        .expect_err("empty sql should error");
    assert!(err.contains("不能为空"), "err: {err}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn execute_sql_invalid_sql_reports_error() {
    let dir = temp_dir("bad");
    let db_path = dir.join("analytics.duckdb");
    let conn = duckdb::Connection::open(&db_path).expect("open duckdb");
    drop(conn);

    let err = rds_workbench::services::query_runner::execute_sql(&db_path, "SELEC nonsense")
        .expect_err("bad sql should error");
    assert!(err.contains("失败"), "err: {err}");

    let _ = std::fs::remove_dir_all(&dir);
}
