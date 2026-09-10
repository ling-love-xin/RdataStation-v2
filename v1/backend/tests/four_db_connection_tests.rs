use rdata_station_lib::core::driver::native::duckdb::DuckDbDatabase;
use rdata_station_lib::core::driver::native::mysql::MySqlDatabase;
use rdata_station_lib::core::driver::native::postgres::PostgresDatabase;
use rdata_station_lib::core::driver::native::sqlite::SqliteDatabase;
use rdata_station_lib::core::driver::Database;

/* ===== 本地数据库环境配置 ===== */
const MYSQL_URL: &str = "mysql://root:root@localhost:3306";
const PG_URL: &str = "postgres://postgres:postgresql@localhost:5432/business_db";
const SQLITE_PATH: &str = r"E:\myapps\tauirapps\RdataStation\TESTDB\sqliteTDB";
const DUCKDB_PATH: &str = r"E:\myapps\tauirapps\RdataStation\TESTDB\duckTDB";

/* ==================== MySQL 测试 ==================== */

#[tokio::test]
async fn test_mysql_ping() {
    let db = MySqlDatabase::new(MYSQL_URL)
        .await
        .expect("[MySQL] 连接失败");
    db.ping().await.expect("[MySQL] ping 失败");
    eprintln!("[MySQL] ✅ ping 成功");
}

#[tokio::test]
async fn test_mysql_list_catalogs() {
    let db = MySqlDatabase::new(MYSQL_URL).await.unwrap();
    let dbs = db.list_catalogs().await.expect("[MySQL] 列举 Catalog 失败");
    eprintln!("[MySQL] Catalog 列表 ({}) -> {:?}", dbs.len(), dbs);
    assert!(!dbs.is_empty(), "[MySQL] Catalog 列表为空");
}

#[tokio::test]
async fn test_mysql_execute_select() {
    let db = MySqlDatabase::new(MYSQL_URL).await.unwrap();
    let result = db
        .query("SELECT 1 AS test_col, NOW() AS ts")
        .await
        .expect("[MySQL] 查询失败");
    assert!(!result.batches.is_empty(), "[MySQL] 结果批次为空");
    let rows = result.total_rows();
    let cols = result.columns.join(", ");
    eprintln!("[MySQL] SELECT 返回 {} 行, 列=[{}]", rows, cols);
}

#[tokio::test]
async fn test_mysql_insert_and_select() {
    let db = MySqlDatabase::new(MYSQL_URL).await.unwrap();

    db.query("CREATE DATABASE IF NOT EXISTS rdata_test")
        .await
        .expect("[MySQL] 创建数据库失败");
    db.query("USE rdata_test").await.expect("[MySQL] USE 失败");
    db.query("CREATE TABLE IF NOT EXISTS _conn_test (id INT AUTO_INCREMENT PRIMARY KEY, val VARCHAR(100))")
        .await.expect("[MySQL] 建表失败");

    db.query("INSERT INTO _conn_test (val) VALUES ('hello')")
        .await
        .expect("[MySQL] INSERT 失败");

    let result = db
        .query("SELECT * FROM _conn_test WHERE val='hello'")
        .await
        .expect("[MySQL] SELECT 失败");
    assert!(result.total_rows() >= 1, "[MySQL] 未查询到插入的行");
    eprintln!(
        "[MySQL] INSERT → SELECT 验证通过 ({} 行)",
        result.total_rows()
    );

    db.query("DROP TABLE IF EXISTS _conn_test").await.ok();
    eprintln!("[MySQL] 清理完成");
}

/* ==================== PostgreSQL 测试 ==================== */

#[tokio::test]
async fn test_pg_ping() {
    let db = PostgresDatabase::new(PG_URL)
        .await
        .expect("[PostgreSQL] 连接失败");
    db.ping().await.expect("[PostgreSQL] ping 失败");
    eprintln!("[PostgreSQL] ✅ ping 成功");
}

#[tokio::test]
async fn test_pg_list_schemas() {
    let db = PostgresDatabase::new(PG_URL).await.unwrap();
    let schemas = db
        .list_schemas("business_db")
        .await
        .expect("[PG] 列举 schema 失败");
    eprintln!("[PostgreSQL] Schema 列表 -> {:?}", schemas);
    assert!(!schemas.is_empty(), "[PG] Schema 列表为空");
}

#[tokio::test]
async fn test_pg_list_tables() {
    let db = PostgresDatabase::new(PG_URL).await.unwrap();
    let tables = db
        .list_tables("business_db", Some("public"))
        .await
        .expect("[PG] 列举表失败");
    eprintln!(
        "[PostgreSQL] public 表 ({}): {:?}",
        tables.len(),
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_pg_execute_select() {
    let db = PostgresDatabase::new(PG_URL).await.unwrap();
    let result = db
        .query("SELECT 1::int AS test_col, NOW() AS ts")
        .await
        .expect("[PG] 查询失败");
    assert!(!result.batches.is_empty());
    eprintln!(
        "[PostgreSQL] SELECT 返回 {} 行, 列={}",
        result.total_rows(),
        result.columns.join(", ")
    );
}

#[tokio::test]
async fn test_pg_insert_and_select() {
    let db = PostgresDatabase::new(PG_URL).await.unwrap();

    db.query("CREATE TABLE IF NOT EXISTS _conn_test (id SERIAL PRIMARY KEY, val VARCHAR(100))")
        .await
        .expect("[PG] 建表失败");

    db.query("INSERT INTO _conn_test (val) VALUES ('hello')")
        .await
        .expect("[PG] INSERT 失败");

    let result = db
        .query("SELECT * FROM _conn_test WHERE val='hello'")
        .await
        .expect("[PG] SELECT 失败");
    assert!(result.total_rows() >= 1, "[PG] 未查询到插入的行");
    eprintln!(
        "[PostgreSQL] INSERT → SELECT 验证通过 ({} 行)",
        result.total_rows()
    );

    db.query("DROP TABLE IF EXISTS _conn_test").await.ok();
    eprintln!("[PostgreSQL] 清理完成");
}

/* ==================== SQLite 测试 ==================== */

#[tokio::test]
async fn test_sqlite_ping() {
    let db = SqliteDatabase::new(SQLITE_PATH).expect("[SQLite] 打开数据库失败");
    db.ping().await.expect("[SQLite] ping 失败");
    eprintln!("[SQLite] ✅ ping 成功");
}

#[tokio::test]
async fn test_sqlite_list_tables() {
    let db = SqliteDatabase::new(SQLITE_PATH).unwrap();
    let tables = db
        .list_tables("main", None)
        .await
        .expect("[SQLite] 列举表失败");
    eprintln!(
        "[SQLite] main 表 ({}) -> {:?}",
        tables.len(),
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_sqlite_execute_select() {
    let db = SqliteDatabase::new(SQLITE_PATH).unwrap();
    let result = db
        .query("SELECT 1 AS test_col, datetime('now') AS ts")
        .await
        .expect("[SQLite] 查询失败");
    assert!(!result.batches.is_empty());
    eprintln!(
        "[SQLite] SELECT 返回 {} 行, 列={}",
        result.total_rows(),
        result.columns.join(", ")
    );
}

#[tokio::test]
async fn test_sqlite_insert_and_select() {
    let db = SqliteDatabase::new(SQLITE_PATH).unwrap();

    db.query(
        "CREATE TABLE IF NOT EXISTS _conn_test (id INTEGER PRIMARY KEY AUTOINCREMENT, val TEXT)",
    )
    .await
    .expect("[SQLite] 建表失败");

    db.query("INSERT INTO _conn_test (val) VALUES ('hello')")
        .await
        .expect("[SQLite] INSERT 失败");

    let result = db
        .query("SELECT * FROM _conn_test WHERE val='hello'")
        .await
        .expect("[SQLite] SELECT 失败");
    assert!(result.total_rows() >= 1, "[SQLite] 未查询到插入的行");
    eprintln!(
        "[SQLite] INSERT → SELECT 验证通过 ({} 行)",
        result.total_rows()
    );

    db.query("DROP TABLE IF EXISTS _conn_test").await.ok();
    eprintln!("[SQLite] 清理完成");
}

/* ==================== DuckDB 测试 ==================== */

#[tokio::test]
async fn test_duckdb_ping() {
    let db = DuckDbDatabase::new(DUCKDB_PATH).expect("[DuckDB] 打开数据库失败");
    db.ping().await.expect("[DuckDB] ping 失败");
    eprintln!("[DuckDB] ✅ ping 成功");
}

#[tokio::test]
async fn test_duckdb_list_tables() {
    let db = DuckDbDatabase::new(DUCKDB_PATH).unwrap();
    let tables = db
        .list_tables("main", Some("main"))
        .await
        .expect("[DuckDB] 列举表失败");
    eprintln!(
        "[DuckDB] main 表 ({}): {:?}",
        tables.len(),
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_duckdb_execute_select() {
    let db = DuckDbDatabase::new(DUCKDB_PATH).unwrap();
    let result = db
        .query("SELECT 1 AS test_col, NOW() AS ts")
        .await
        .expect("[DuckDB] 查询失败");
    assert!(!result.batches.is_empty());
    eprintln!(
        "[DuckDB] SELECT 返回 {} 行, 列={}",
        result.total_rows(),
        result.columns.join(", ")
    );
}

#[tokio::test]
async fn test_duckdb_insert_and_select() {
    let db = DuckDbDatabase::new(DUCKDB_PATH).unwrap();

    db.query("CREATE TABLE IF NOT EXISTS _conn_test (id INTEGER PRIMARY KEY, val VARCHAR)")
        .await
        .expect("[DuckDB] 建表失败");

    db.query("INSERT INTO _conn_test VALUES (1, 'hello')")
        .await
        .expect("[DuckDB] INSERT 失败");

    let result = db
        .query("SELECT * FROM _conn_test WHERE val='hello'")
        .await
        .expect("[DuckDB] SELECT 失败");
    assert!(result.total_rows() >= 1, "[DuckDB] 未查询到插入的行");
    eprintln!(
        "[DuckDB] INSERT → SELECT 验证通过 ({} 行)",
        result.total_rows()
    );

    db.query("DROP TABLE IF EXISTS _conn_test").await.ok();
    eprintln!("[DuckDB] 清理完成");
}

#[tokio::test]
async fn test_duckdb_meta() {
    let db = DuckDbDatabase::new(DUCKDB_PATH).unwrap();
    let meta = db.meta();
    eprintln!(
        "[DuckDB] meta: supports_arrow={}, supports_federated={}, supports_streaming={}",
        meta.supports_arrow, meta.supports_federated, meta.supports_streaming
    );
    assert!(meta.supports_arrow, "[DuckDB] 应支持 Arrow");
    assert!(meta.supports_federated, "[DuckDB] 应支持联邦查询");
}
