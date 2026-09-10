//! 实际数据库连接集成测试
//!
//! 覆盖 4 种数据库类型的真实连接：
//! - MySQL:     mysql://localhost:3306/  root/root
//! - PostgreSQL: postgresql://localhost:5432/business_db  postgres/postgresql
//! - SQLite:    文件型自建数据库
//! - DuckDB:    文件型自建数据库
//!
//! 运行方式：
//!   cargo test --test real_db_integration_tests -- --ignored  # 含网络数据库
//!   cargo test --test real_db_integration_tests                # 仅文件数据库
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use rdata_station_lib::core::driver::native::duckdb::DuckDbDatabase;
use rdata_station_lib::core::driver::native::mysql::MySqlDatabase;
use rdata_station_lib::core::driver::native::postgres::PostgresDatabase;
use rdata_station_lib::core::driver::native::sqlite::SqliteDatabase;
use rdata_station_lib::core::driver::Database;
use std::fs;
use std::path::PathBuf;

// ==================== 测试常量 ====================

const MYSQL_URL: &str = "mysql://root:root@localhost:3306/";
const PG_URL: &str = "postgres://postgres:postgresql@localhost:5432/business_db";

// ==================== 文件数据库辅助函数 ====================

fn test_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("rd_real_db_test");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn cleanup_db(path: &PathBuf) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("db-wal"));
    let _ = fs::remove_file(path.with_extension("db-shm"));
}

// ====================================================================
// MySQL 实际连接测试
// ====================================================================

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_connect() {
    let db = MySqlDatabase::new(MYSQL_URL).await;
    assert!(db.is_ok(), "MySQL 连接失败: {:?}", db.err());
    let db = db.unwrap();

    let version = db.meta().server_version;
    assert!(version.is_some(), "服务器版本不应为空");
    println!("  MySQL 版本: {}", version.unwrap());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_query_select_one() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let result = db.query("SELECT 1 AS val").await;
    assert!(result.is_ok(), "SELECT 1 失败: {:?}", result.err());

    let qr = result.unwrap();
    assert_eq!(qr.columns, vec!["val"]);
    assert_eq!(qr.total_rows(), 1);
    println!("  SELECT 1 => {} 行", qr.total_rows());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_query_select_version() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");
    let result = db.query("SELECT VERSION() AS version").await;
    assert!(result.is_ok(), "SELECT VERSION() 失败: {:?}", result.err());

    let qr = result.unwrap();
    assert_eq!(qr.total_rows(), 1);
    assert_eq!(qr.columns, vec!["version"]);
    println!("  MySQL VERSION() => {:?}", qr.rows);
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_create_table_and_crud() {
    let setup_db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");

    // 创建测试数据库
    setup_db
        .query("CREATE DATABASE IF NOT EXISTS _rd_real_test_db")
        .await
        .expect("创建测试数据库失败");

    let db_url = format!("{}_rd_real_test_db", MYSQL_URL);
    let db = MySqlDatabase::new(&db_url)
        .await
        .expect("连接测试数据库失败");

    // 创建测试表
    db.query(
        "CREATE TABLE IF NOT EXISTS _rd_real_test (
            id INT AUTO_INCREMENT PRIMARY KEY,
            name VARCHAR(100),
            value INT
        )",
    )
    .await
    .expect("CREATE TABLE 失败");

    // 插入
    db.query("INSERT INTO _rd_real_test (name, value) VALUES ('test1', 100), ('test2', 200)")
        .await
        .expect("INSERT 失败");

    // 查询
    let select = db.query("SELECT * FROM _rd_real_test ORDER BY id").await;
    assert!(select.is_ok(), "SELECT 失败: {:?}", select.err());
    let qr = select.unwrap();
    assert_eq!(qr.total_rows(), 2);
    println!("  INSERT 2 行 => 查询到 {} 行", qr.total_rows());

    // 更新
    db.query("UPDATE _rd_real_test SET value = 999 WHERE name = 'test1'")
        .await
        .expect("UPDATE 失败");

    // 验证更新
    let verify = db
        .query("SELECT value FROM _rd_real_test WHERE name = 'test1'")
        .await;
    assert!(verify.is_ok(), "验证查询失败: {:?}", verify.err());

    // 删除
    db.query("DELETE FROM _rd_real_test WHERE name = 'test2'")
        .await
        .expect("DELETE 失败");

    // 清理
    db.query("DROP TABLE IF EXISTS _rd_real_test")
        .await
        .expect("DROP TABLE 失败");
    setup_db
        .query("DROP DATABASE IF EXISTS _rd_real_test_db")
        .await
        .expect("清理测试数据库失败");

    println!("  MySQL CRUD 全流程通过");
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_metadata_list_tables() {
    let db = MySqlDatabase::new(MYSQL_URL).await.expect("连接失败");

    // 列出 mysql 系统库的表（mysql 库始终存在且无需额外创建数据库）
    let tables = db.list_tables("mysql", None).await;
    assert!(tables.is_ok(), "list_tables 失败: {:?}", tables.err());

    let table_list = tables.unwrap();
    println!("  mysql 库的表 ({} 个)", table_list.len());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_wrong_password_rejected() {
    let db = MySqlDatabase::new("mysql://root:wrong_password@localhost:3306/").await;
    assert!(db.is_err(), "错误密码应该返回错误，但连接成功了");
    println!("  错误密码正确拒绝: {:?}", db.err());
}

#[tokio::test]
#[ignore = "需要运行中的 MySQL 服务（root/root@localhost:3306）"]
async fn test_mysql_wrong_host_rejected() {
    let db = MySqlDatabase::new("mysql://root:root@192.0.2.1:3306/").await;
    assert!(db.is_err(), "不可达主机应该返回错误，但连接成功了");
    println!("  不可达主机正确拒绝");
}

// ====================================================================
// PostgreSQL 实际连接测试
// ====================================================================

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_connect() {
    let db = PostgresDatabase::new(PG_URL).await;
    assert!(db.is_ok(), "PostgreSQL 连接失败: {:?}", db.err());
    let db = db.unwrap();

    let version = db.meta().server_version;
    assert!(version.is_some(), "服务器版本不应为空");
    println!("  PostgreSQL 版本: {}", version.unwrap());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_query_select_one() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db.query("SELECT 1 AS val").await;
    assert!(result.is_ok(), "SELECT 1 失败: {:?}", result.err());

    let qr = result.unwrap();
    assert_eq!(qr.columns, vec!["val"]);
    assert_eq!(qr.total_rows(), 1);
    println!("  SELECT 1 => {} 行", qr.total_rows());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_query_select_version() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db.query("SELECT version() AS version").await;
    assert!(result.is_ok(), "SELECT version() 失败: {:?}", result.err());

    let qr = result.unwrap();
    assert_eq!(qr.total_rows(), 1);
    println!("  PG version() 查询成功");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_schemas() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db.list_schemas("business_db").await;
    assert!(result.is_ok(), "list_schemas 失败: {:?}", result.err());

    let schemas = result.unwrap();
    assert!(!schemas.is_empty(), "schema 列表不应为空");
    println!(
        "  business_db 的 schema ({} 个): {:?}",
        schemas.len(),
        schemas
    );
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_tables_in_public() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");
    let result = db.list_tables("business_db", Some("public")).await;
    assert!(result.is_ok(), "list_tables 失败: {:?}", result.err());

    let tables = result.unwrap();
    println!("  public schema 的表 ({} 个)", tables.len());
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_create_table_and_crud() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");

    // 创建测试表
    db.query(
        "CREATE TABLE IF NOT EXISTS _rd_real_test (
            id SERIAL PRIMARY KEY,
            name VARCHAR(100),
            value INTEGER
        )",
    )
    .await
    .expect("CREATE TABLE 失败");

    // 插入
    db.query("INSERT INTO _rd_real_test (name, value) VALUES ('pg_test1', 100), ('pg_test2', 200)")
        .await
        .expect("INSERT 失败");

    // 查询
    let select = db.query("SELECT * FROM _rd_real_test ORDER BY id").await;
    assert!(select.is_ok(), "SELECT 失败: {:?}", select.err());
    let qr = select.unwrap();
    assert_eq!(qr.total_rows(), 2);
    println!("  INSERT 2 行 => 查询到 {} 行", qr.total_rows());

    // 更新
    db.query("UPDATE _rd_real_test SET value = 999 WHERE name = 'pg_test1'")
        .await
        .expect("UPDATE 失败");

    // 删除
    db.query("DELETE FROM _rd_real_test WHERE name = 'pg_test2'")
        .await
        .expect("DELETE 失败");

    // 清理
    db.query("DROP TABLE IF EXISTS _rd_real_test")
        .await
        .expect("DROP TABLE 失败");

    println!("  PostgreSQL CRUD 全流程通过");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_list_columns() {
    let db = PostgresDatabase::new(PG_URL).await.expect("连接失败");

    // 创建表
    db.query(
        "CREATE TABLE IF NOT EXISTS _rd_col_test (
            id SERIAL PRIMARY KEY,
            name VARCHAR(100) NOT NULL,
            age INTEGER DEFAULT 0
        )",
    )
    .await
    .expect("创建表失败");

    let columns = db
        .list_columns("business_db", Some("public"), "_rd_col_test")
        .await;
    assert!(columns.is_ok(), "list_columns 失败: {:?}", columns.err());

    let col_list = columns.unwrap();
    assert_eq!(col_list.len(), 3, "应有 3 列");
    println!(
        "  _rd_col_test 的列: {:?}",
        col_list.iter().map(|c| &c.name).collect::<Vec<_>>()
    );

    // 清理
    db.query("DROP TABLE IF EXISTS _rd_col_test")
        .await
        .expect("清理表失败");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_wrong_password_rejected() {
    let db = PostgresDatabase::new("postgres://postgres:wrong_password@localhost:5432/business_db")
        .await;
    assert!(db.is_err(), "错误密码应该返回错误，但连接成功了");
    println!("  错误密码正确拒绝");
}

#[tokio::test]
#[ignore = "需要运行中的 PostgreSQL 服务（postgres/postgresql@localhost:5432）"]
async fn test_pg_wrong_host_rejected() {
    let db =
        PostgresDatabase::new("postgres://postgres:postgresql@192.0.2.1:5432/business_db").await;
    assert!(db.is_err(), "不可达主机应该返回错误，但连接成功了");
    println!("  不可达主机正确拒绝");
}

// ====================================================================
// SQLite 实际连接测试（文件数据库，无需网络）
// ====================================================================

#[tokio::test]
async fn test_sqlite_create_and_connect() {
    let dir = test_dir();
    let db_path = dir.join("real_sqlite_test.db");
    cleanup_db(&db_path);

    let db = SqliteDatabase::new(db_path.to_str().unwrap());
    assert!(db.is_ok(), "创建 SQLite 数据库失败: {:?}", db.err());
    assert!(db_path.exists(), "数据库文件应已创建");
    println!("  SQLite 文件: {:?}", db_path);

    let db = db.unwrap();
    let version = db.meta().server_version;
    assert!(version.is_some(), "SQLite 版本不应为空");
    println!("  SQLite 版本: {}", version.unwrap());

    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_sqlite_create_table_and_crud() {
    let dir = test_dir();
    let db_path = dir.join("real_sqlite_crud.db");
    cleanup_db(&db_path);

    let db = SqliteDatabase::new(db_path.to_str().unwrap()).expect("创建数据库失败");

    // 创建表
    db.query(
        "CREATE TABLE users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            age INTEGER DEFAULT 0
        )",
    )
    .await
    .expect("CREATE TABLE 失败");

    // 插入数据
    db.query("INSERT INTO users (name, age) VALUES ('Alice', 30)")
        .await
        .expect("INSERT Alice 失败");
    db.query("INSERT INTO users (name, age) VALUES ('Bob', 25)")
        .await
        .expect("INSERT Bob 失败");
    db.query("INSERT INTO users (name, age) VALUES ('Charlie', 35)")
        .await
        .expect("INSERT Charlie 失败");

    // 查询全部
    let result = db
        .query("SELECT * FROM users ORDER BY id")
        .await
        .expect("SELECT 失败");
    assert_eq!(result.total_rows(), 3);

    // 查询条件
    let result = db
        .query("SELECT name, age FROM users WHERE age > 28")
        .await
        .expect("条件查询失败");
    assert_eq!(result.total_rows(), 2); // Alice, Charlie

    // 更新
    db.query("UPDATE users SET age = 26 WHERE name = 'Bob'")
        .await
        .expect("UPDATE 失败");

    let verify = db
        .query("SELECT age FROM users WHERE name = 'Bob'")
        .await
        .expect("验证失败");
    assert_eq!(verify.total_rows(), 1);

    // 删除
    db.query("DELETE FROM users WHERE name = 'Charlie'")
        .await
        .expect("DELETE 失败");

    let count = db
        .query("SELECT COUNT(*) AS cnt FROM users")
        .await
        .expect("计数失败");
    assert_eq!(count.total_rows(), 1);

    // 清理
    db.query("DROP TABLE IF EXISTS users")
        .await
        .expect("DROP TABLE 失败");

    println!("  SQLite CRUD 全流程通过");
    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_sqlite_transaction() {
    let dir = test_dir();
    let db_path = dir.join("real_sqlite_tx.db");
    cleanup_db(&db_path);

    let db = SqliteDatabase::new(db_path.to_str().unwrap()).expect("创建数据库失败");

    db.query("CREATE TABLE tx_test (id INTEGER PRIMARY KEY, value TEXT)")
        .await
        .expect("CREATE TABLE 失败");

    // 开始事务
    let mut tx = db
        .begin_transaction()
        .await
        .expect("begin_transaction 失败");

    // 在事务中插入
    tx.query("INSERT INTO tx_test (id, value) VALUES (1, 'tx_val1')")
        .await
        .expect("事务内 INSERT 1 失败");
    tx.query("INSERT INTO tx_test (id, value) VALUES (2, 'tx_val2')")
        .await
        .expect("事务内 INSERT 2 失败");

    // 提交事务
    tx.commit().await.expect("提交事务失败");

    // 验证提交后数据可见
    let result = db
        .query("SELECT COUNT(*) AS cnt FROM tx_test")
        .await
        .expect("查询失败");
    assert_eq!(result.total_rows(), 1);

    println!("  SQLite 事务测试通过");
    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_sqlite_transaction_rollback() {
    let dir = test_dir();
    let db_path = dir.join("real_sqlite_rollback.db");
    cleanup_db(&db_path);

    let db = SqliteDatabase::new(db_path.to_str().unwrap()).expect("创建数据库失败");

    db.query("CREATE TABLE rollback_test (id INTEGER PRIMARY KEY, value TEXT)")
        .await
        .expect("CREATE TABLE 失败");

    // 开始事务
    let mut tx = db
        .begin_transaction()
        .await
        .expect("begin_transaction 失败");

    tx.query("INSERT INTO rollback_test (id, value) VALUES (1, 'should_rollback')")
        .await
        .expect("事务内 INSERT 失败");

    // 回滚事务
    tx.rollback().await.expect("回滚事务失败");

    // 验证回滚后数据不可见
    let result = db
        .query("SELECT COUNT(*) AS cnt FROM rollback_test")
        .await
        .expect("查询失败");
    assert_eq!(result.total_rows(), 1);

    println!("  SQLite 回滚测试通过");
    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_sqlite_list_tables() {
    let dir = test_dir();
    let db_path = dir.join("real_sqlite_meta.db");
    cleanup_db(&db_path);

    let db = SqliteDatabase::new(db_path.to_str().unwrap()).expect("创建数据库失败");

    db.query("CREATE TABLE t1 (id INTEGER)")
        .await
        .expect("CREATE t1 失败");
    db.query("CREATE TABLE t2 (id INTEGER)")
        .await
        .expect("CREATE t2 失败");

    let tables = db
        .list_tables("main", None)
        .await
        .expect("list_tables 失败");
    assert_eq!(tables.len(), 2);
    println!(
        "  SQLite 表列表: {:?}",
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    db.query("DROP TABLE t1").await.expect("DROP t1 失败");
    db.query("DROP TABLE t2").await.expect("DROP t2 失败");

    println!("  SQLite 元数据测试通过");
    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_sqlite_in_memory() {
    let db = SqliteDatabase::new(":memory:").expect("创建内存数据库失败");

    db.query("CREATE TABLE mem_test (id INTEGER, name TEXT)")
        .await
        .expect("CREATE TABLE 失败");
    db.query("INSERT INTO mem_test (id, name) VALUES (1, 'mem')")
        .await
        .expect("INSERT 失败");

    let result = db
        .query("SELECT * FROM mem_test")
        .await
        .expect("SELECT 失败");
    assert_eq!(result.total_rows(), 1);

    println!("  SQLite 内存数据库测试通过");
}

// ====================================================================
// DuckDB 实际连接测试（文件数据库，无需网络）
// ====================================================================

#[tokio::test]
async fn test_duckdb_create_and_connect() {
    let dir = test_dir();
    let db_path = dir.join("real_duckdb_test.db");
    cleanup_db(&db_path);

    let db = DuckDbDatabase::new(db_path.to_str().unwrap());
    assert!(db.is_ok(), "创建 DuckDB 数据库失败: {:?}", db.err());
    assert!(db_path.exists(), "数据库文件应已创建");
    println!("  DuckDB 文件: {:?}", db_path);

    let db = db.unwrap();
    let version = db.meta().server_version;
    assert!(version.is_some(), "DuckDB 版本不应为空");
    println!("  DuckDB 版本: {}", version.unwrap());

    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_duckdb_in_memory() {
    let db = DuckDbDatabase::new(":memory:").expect("创建内存 DuckDB 失败");

    let result = db.query("SELECT 42 AS answer").await.expect("SELECT 失败");
    assert_eq!(result.total_rows(), 1);
    assert_eq!(result.columns, vec!["answer"]);

    println!("  DuckDB 内存数据库测试通过");
}

#[tokio::test]
async fn test_duckdb_create_table_and_crud() {
    let dir = test_dir();
    let db_path = dir.join("real_duckdb_crud.db");
    cleanup_db(&db_path);

    let db = DuckDbDatabase::new(db_path.to_str().unwrap()).expect("创建数据库失败");

    // 创建表
    db.query(
        "CREATE TABLE products (
            id INTEGER PRIMARY KEY,
            name VARCHAR,
            price DOUBLE
        )",
    )
    .await
    .expect("CREATE TABLE 失败");

    // 插入
    db.query("INSERT INTO products VALUES (1, 'Apple', 3.5)")
        .await
        .expect("INSERT 1 失败");
    db.query("INSERT INTO products VALUES (2, 'Banana', 2.0)")
        .await
        .expect("INSERT 2 失败");
    db.query("INSERT INTO products VALUES (3, 'Cherry', 5.0)")
        .await
        .expect("INSERT 3 失败");

    // 查询全部
    let result = db
        .query("SELECT * FROM products ORDER BY id")
        .await
        .expect("SELECT 失败");
    assert_eq!(result.total_rows(), 3);

    // 聚合查询（DuckDB 强项）
    let result = db
        .query("SELECT AVG(price) AS avg_price, SUM(price) AS total FROM products")
        .await
        .expect("聚合查询失败");
    assert_eq!(result.total_rows(), 1);
    println!("  聚合查询: {:?}", result.columns);

    // 更新
    db.query("UPDATE products SET price = 4.0 WHERE name = 'Apple'")
        .await
        .expect("UPDATE 失败");

    // 删除
    db.query("DELETE FROM products WHERE name = 'Cherry'")
        .await
        .expect("DELETE 失败");

    let count = db
        .query("SELECT COUNT(*) AS cnt FROM products")
        .await
        .expect("计数失败");
    assert_eq!(count.total_rows(), 1);

    // 清理
    db.query("DROP TABLE IF EXISTS products")
        .await
        .expect("DROP TABLE 失败");

    println!("  DuckDB CRUD 全流程通过");
    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_duckdb_list_tables() {
    let dir = test_dir();
    let db_path = dir.join("real_duckdb_meta.db");
    cleanup_db(&db_path);

    let db = DuckDbDatabase::new(db_path.to_str().unwrap()).expect("创建数据库失败");

    db.query("CREATE TABLE duck_t1 (id INTEGER)")
        .await
        .expect("CREATE t1 失败");
    db.query("CREATE TABLE duck_t2 (id INTEGER)")
        .await
        .expect("CREATE t2 失败");

    let tables = db
        .list_tables("main", None)
        .await
        .expect("list_tables 失败");
    assert_eq!(tables.len(), 2);
    println!(
        "  DuckDB 表列表: {:?}",
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    db.query("DROP TABLE duck_t1").await.expect("DROP t1 失败");
    db.query("DROP TABLE duck_t2").await.expect("DROP t2 失败");

    println!("  DuckDB 元数据测试通过");
    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_duckdb_analytics_query() {
    // DuckDB 是分析型数据库，测试其分析能力
    let dir = test_dir();
    let db_path = dir.join("real_duckdb_analytics.db");
    cleanup_db(&db_path);

    let db = DuckDbDatabase::new(db_path.to_str().unwrap()).expect("创建数据库失败");

    // 创建稍大的数据集
    db.query(
        "CREATE TABLE sales (
            id INTEGER,
            category VARCHAR,
            amount DOUBLE,
            sale_date DATE
        )",
    )
    .await
    .expect("CREATE TABLE 失败");

    db.query("INSERT INTO sales VALUES (1, 'Electronics', 999.99, '2024-01-15')")
        .await
        .expect("INSERT 失败");
    db.query("INSERT INTO sales VALUES (2, 'Clothing', 49.99, '2024-01-16')")
        .await
        .expect("INSERT 失败");
    db.query("INSERT INTO sales VALUES (3, 'Electronics', 1499.99, '2024-01-17')")
        .await
        .expect("INSERT 失败");
    db.query("INSERT INTO sales VALUES (4, 'Food', 9.99, '2024-01-18')")
        .await
        .expect("INSERT 失败");
    db.query("INSERT INTO sales VALUES (5, 'Electronics', 299.99, '2024-01-19')")
        .await
        .expect("INSERT 失败");

    // 按类别聚合
    let result = db
        .query(
            "SELECT category, COUNT(*) AS cnt, SUM(amount) AS total
             FROM sales GROUP BY category ORDER BY total DESC",
        )
        .await
        .expect("聚合查询失败");
    assert_eq!(result.total_rows(), 3);
    println!("  DuckDB 分析查询: {} 行结果", result.total_rows());

    // 窗口函数
    let result = db
        .query(
            "SELECT category, amount,
             RANK() OVER (PARTITION BY category ORDER BY amount DESC) AS rank
             FROM sales",
        )
        .await
        .expect("窗口函数查询失败");
    assert_eq!(result.total_rows(), 5);
    println!("  DuckDB 窗口函数查询: {} 行", result.total_rows());

    db.query("DROP TABLE IF EXISTS sales")
        .await
        .expect("DROP TABLE 失败");

    println!("  DuckDB 分析查询测试通过");
    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_duckdb_transaction() {
    let dir = test_dir();
    let db_path = dir.join("real_duckdb_tx.db");
    cleanup_db(&db_path);

    let db = DuckDbDatabase::new(db_path.to_str().unwrap()).expect("创建数据库失败");

    db.query("CREATE TABLE tx_test (id INTEGER, value TEXT)")
        .await
        .expect("CREATE TABLE 失败");

    let mut tx = db
        .begin_transaction()
        .await
        .expect("begin_transaction 失败");
    tx.query("INSERT INTO tx_test (id, value) VALUES (1, 'duck_tx')")
        .await
        .expect("事务内 INSERT 失败");
    tx.commit().await.expect("提交事务失败");

    let result = db
        .query("SELECT COUNT(*) AS cnt FROM tx_test")
        .await
        .expect("查询失败");
    assert_eq!(result.total_rows(), 1);

    println!("  DuckDB 事务测试通过");
    cleanup_db(&db_path);
}

#[tokio::test]
async fn test_duckdb_transaction_rollback() {
    let dir = test_dir();
    let db_path = dir.join("real_duckdb_rollback.db");
    cleanup_db(&db_path);

    let db = DuckDbDatabase::new(db_path.to_str().unwrap()).expect("创建数据库失败");

    db.query("CREATE TABLE rollback_test (id INTEGER, value TEXT)")
        .await
        .expect("CREATE TABLE 失败");

    let mut tx = db
        .begin_transaction()
        .await
        .expect("begin_transaction 失败");
    tx.query("INSERT INTO rollback_test (id, value) VALUES (1, 'should_rollback')")
        .await
        .expect("事务内 INSERT 失败");
    tx.rollback().await.expect("回滚事务失败");

    let result = db
        .query("SELECT COUNT(*) AS cnt FROM rollback_test")
        .await
        .expect("查询失败");
    assert_eq!(result.total_rows(), 1);

    println!("  DuckDB 回滚测试通过");
    cleanup_db(&db_path);
}
