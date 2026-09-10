use rdata_station_lib::core::driver::native::duckdb::DuckDbDatabase;
use rdata_station_lib::core::driver::native::mysql::MySqlDatabase;
use rdata_station_lib::core::driver::native::postgres::PostgresDatabase;
use rdata_station_lib::core::driver::native::sqlite::SqliteDatabase;
use rdata_station_lib::core::driver::{Database, MetadataBrowser};
use std::fs;

const SQLITE_PATH: &str = r"E:\Documents\new.db";
const PG_URL: &str = "postgres://postgres:postgresql@localhost:5432/postgres";
const MYSQL_URL: &str = "mysql://root:root@localhost:3306";
const DUCKDB_SRC: &str = r"E:\ccccc";

fn duckdb_temp_path() -> String {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("rdatastation_test_{}.duckdb", std::process::id()));
    let _ = fs::copy(DUCKDB_SRC, &path);
    path.to_string_lossy().into_owned()
}

#[tokio::test]
async fn test_sqlite_connect_and_ping() {
    let db = SqliteDatabase::new(SQLITE_PATH).expect("failed to open SQLite");
    db.ping().await.expect("SQLite ping failed");
}

#[tokio::test]
async fn test_sqlite_list_tables() {
    let db = SqliteDatabase::new(SQLITE_PATH).expect("failed to open SQLite");
    let tables = db
        .list_tables("main", None)
        .await
        .expect("failed to list tables");
    eprintln!(
        "SQLite tables: {:?}",
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_sqlite_list_columns() {
    let db = SqliteDatabase::new(SQLITE_PATH).expect("failed to open SQLite");
    let tables = db.list_tables("main", None).await.expect("list_tables");
    if let Some(table) = tables.first() {
        let columns = db
            .list_columns("main", None, &table.name)
            .await
            .expect("failed to list columns");
        eprintln!(
            "SQLite table [{}] columns: {:?}",
            table.name,
            columns
                .iter()
                .map(|c| format!("{} ({})", c.name, c.data_type))
                .collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn test_sqlite_execute_query() {
    let db = SqliteDatabase::new(SQLITE_PATH).expect("failed to open SQLite");
    let result = db
        .query("SELECT 1 AS test_col")
        .await
        .expect("query failed");
    assert!(!result.batches.is_empty(), "expected at least one batch");
    eprintln!(
        "SQLite SELECT 1 returned {} batch(es)",
        result.batches.len()
    );
}

#[tokio::test]
async fn test_postgres_connect_and_ping() {
    let pool = sqlx::PgPool::connect(PG_URL)
        .await
        .expect("failed to connect to PG");
    let db = PostgresDatabase::from_pool(pool);
    db.ping().await.expect("PostgreSQL ping failed");
}

#[tokio::test]
async fn test_postgres_list_schemas() {
    let pool = sqlx::PgPool::connect(PG_URL)
        .await
        .expect("failed to connect to PG");
    let db = PostgresDatabase::from_pool(pool);
    let schemas = db
        .list_schemas("postgres")
        .await
        .expect("failed to list schemas");
    eprintln!(
        "PostgreSQL schemas: {:?}",
        schemas.iter().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_postgres_list_tables() {
    let pool = sqlx::PgPool::connect(PG_URL)
        .await
        .expect("failed to connect to PG");
    let db = PostgresDatabase::from_pool(pool);
    let tables = db
        .list_tables("postgres", Some("public"))
        .await
        .expect("failed to list tables");
    eprintln!(
        "PostgreSQL public tables: {:?}",
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_postgres_execute_query() {
    let pool = sqlx::PgPool::connect(PG_URL)
        .await
        .expect("failed to connect to PG");
    let db = PostgresDatabase::from_pool(pool);
    let result = db
        .query("SELECT 1::int AS test_col")
        .await
        .expect("query failed");
    assert!(!result.batches.is_empty());
    eprintln!(
        "PostgreSQL SELECT 1 returned {} batch(es)",
        result.batches.len()
    );
}

#[tokio::test]
async fn test_postgres_list_functions() {
    let pool = sqlx::PgPool::connect(PG_URL)
        .await
        .expect("failed to connect to PG");
    let db = PostgresDatabase::from_pool(pool);
    let funcs = db
        .list_functions("postgres", Some("public"))
        .await
        .expect("failed to list functions");
    eprintln!(
        "PostgreSQL public functions: {:?}",
        funcs.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_duckdb_connect_and_ping() {
    let path = duckdb_temp_path();
    let db = DuckDbDatabase::new(&path).expect("failed to open DuckDB");
    db.ping().await.expect("DuckDB ping failed");
}

#[tokio::test]
async fn test_duckdb_list_tables() {
    let path = duckdb_temp_path();
    let db = DuckDbDatabase::new(&path).expect("failed to open DuckDB");
    let tables = db
        .list_tables("main", Some("main"))
        .await
        .expect("failed to list tables");
    eprintln!(
        "DuckDB tables: {:?}",
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_duckdb_execute_query() {
    let path = duckdb_temp_path();
    let db = DuckDbDatabase::new(&path).expect("failed to open DuckDB");
    let result = db
        .query("SELECT 1 AS test_col")
        .await
        .expect("query failed");
    assert!(!result.batches.is_empty());
    eprintln!(
        "DuckDB SELECT 1 returned {} batch(es)",
        result.batches.len()
    );
}

#[tokio::test]
async fn test_duckdb_get_table_detail() {
    let path = duckdb_temp_path();
    let db = DuckDbDatabase::new(&path).expect("failed to open DuckDB");
    let tables = db
        .list_tables("main", Some("main"))
        .await
        .expect("list_tables");
    if let Some(table) = tables.first() {
        let detail = db
            .get_table_detail("main", "main", &table.name)
            .await
            .expect("failed to get table detail");
        eprintln!(
            "DuckDB table [{}] index_count={:?}, columns={}",
            table.name,
            detail.index_count,
            detail.columns.len()
        );
    }
}

#[tokio::test]
async fn test_mysql_connect_and_ping() {
    let db = MySqlDatabase::new(MYSQL_URL)
        .await
        .expect("failed to connect to MySQL");
    db.ping().await.expect("MySQL ping failed");
}

#[tokio::test]
async fn test_mysql_list_catalogs() {
    let db = MySqlDatabase::new(MYSQL_URL)
        .await
        .expect("failed to connect to MySQL");
    let catalogs = db.list_catalogs().await.expect("failed to list catalogs");
    eprintln!("MySQL catalogs: {:?}", catalogs);
}

#[tokio::test]
async fn test_mysql_list_tables() {
    let db = MySqlDatabase::new(MYSQL_URL)
        .await
        .expect("failed to connect to MySQL");
    let catalogs = db.list_catalogs().await.expect("list_catalogs");
    if let Some(catalog) = catalogs.first() {
        let tables = db
            .list_tables(catalog, None)
            .await
            .expect("failed to list tables");
        eprintln!(
            "MySQL [{}] tables: {:?}",
            catalog,
            tables.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn test_mysql_execute_query() {
    let db = MySqlDatabase::new(MYSQL_URL)
        .await
        .expect("failed to connect to MySQL");
    let result = db
        .query("SELECT 1 AS test_col")
        .await
        .expect("query failed");
    assert!(!result.batches.is_empty());
    eprintln!("MySQL SELECT 1 returned {} batch(es)", result.batches.len());
}

#[tokio::test]
async fn test_mysql_list_functions() {
    let db = MySqlDatabase::new(MYSQL_URL)
        .await
        .expect("failed to connect to MySQL");
    let funcs = db
        .list_functions("mysql", None)
        .await
        .expect("failed to list functions");
    eprintln!(
        "MySQL functions: {:?}",
        funcs.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}
