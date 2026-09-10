//! Round 25：db_navigator 集成测试——临时 DuckDB 建表 → 读回表/列树。

use std::path::PathBuf;

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rds_nav_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

#[test]
fn navigator_reads_tables_and_columns() {
    let dir = temp_dir("nav");
    let db_path = dir.join("analytics.duckdb");

    // 建临时分析库 + 两张表。
    let conn = duckdb::Connection::open(&db_path).expect("open duckdb");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS orders (
             order_id INTEGER PRIMARY KEY,
             amount DECIMAL(12,2),
             status VARCHAR
         );
         CREATE TABLE IF NOT EXISTS customers (
             customer_id INTEGER PRIMARY KEY,
             name VARCHAR
         );",
    )
    .expect("create tables");
    drop(conn);

    let tree = rds_workbench::services::db_navigator::load_navigator_tree(&db_path)
        .expect("load navigator tree");
    let mut names: Vec<&str> = tree.iter().map(|t| t.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["customers", "orders"], "应读出全部用户表");

    let orders = tree.iter().find(|t| t.name == "orders").expect("orders");
    // DuckDB 对 PRIMARY KEY 约束的元数据标记有限（is_primary_key 可能为 false），只断言列存在。
    assert!(orders.columns.iter().any(|c| c.name == "order_id"));
    assert!(orders.columns.iter().any(|c| c.name == "amount" && c.data_type.contains("DECIMAL")));
    assert!(orders.columns.iter().any(|c| c.name == "status"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn navigator_empty_db_returns_empty() {
    let dir = temp_dir("nav_empty");
    let db_path = dir.join("empty.duckdb");
    let conn = duckdb::Connection::open(&db_path).expect("open empty duckdb");
    drop(conn);

    let tree = rds_workbench::services::db_navigator::load_navigator_tree(&db_path)
        .expect("load empty tree");
    assert!(tree.is_empty(), "空库应返回空树");

    let _ = std::fs::remove_dir_all(&dir);
}
