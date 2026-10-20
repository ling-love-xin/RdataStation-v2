//! 真机回归：0 行结果的 SELECT 也要返回**列名**。
//!
//! ## 修的是什么
//!
//! MySQL / PostgreSQL 驱动（sqlx 与 tokio-postgres 两版）在 `rows.is_empty()` 时
//! 直接回 `columns: vec![]`——结果是集「有列无行」的信息被丢掉。洞察的空表取样
//! （`SELECT * FROM t LIMIT 500` 在空表 / 过滤后无行时）靠列名建「有列无行」的
//! 分析临时表，丢了列名就报「取样没有拿到列」。
//!
//! 现在 0 行分支退回 `prepare`（sqlx）/ `prepare`（tokio-postgres）取 statement 的
//! 列描述，列名与有行时的结果同源（都是驱动对语句的列描述）。
//!
//! ## 这个文件钉什么
//!
//! 对每个真机驱动：执行一个**必然 0 行**、但列结构明确的 SELECT，断言
//! `QueryResult.columns` 非空且与预期列名一致（顺序契约：`x` / `y`）。
//!
//! 未设环境变量时跳过（与仓内其余真机套件同规矩）：
//!
//! ```text
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil' \
//! RDS_TEST_DUCKDB_PATH='D:\data\123' \
//!   cargo test -p rds-engine --test zero_row_columns_reach_query_result -- --nocapture
//! ```

use rds_engine::driver::Database;
use rds_engine::driver::native::duckdb::DuckDbDatabase;
use rds_engine::driver::native::mysql::MySqlDatabase;
use rds_engine::driver::native::mysql_native::MySqlNativeDatabase;
use rds_engine::driver::native::postgres::PostgresDatabase;
use rds_engine::driver::native::postgres_native::PostgresNativeDatabase;
use rds_engine::driver::native::sqlite::SqliteDatabase;

/// 必然 0 行、但列结构明确的 SELECT（两列：`x` / `y`）。
/// `WHERE 1=0` 让结果集为空；派生表保证列名稳定。
const ZERO_ROWS: &str = "SELECT 1 AS x, 2 AS y WHERE 1=0";

/// 驱动 → 0 行结果 → 列名仍要回来。
async fn probe<D: Database>(db: &D, label: &str) {
    let result = db.query(ZERO_ROWS).await.expect("查询 0 行结果");

    // ① 0 行：行数恒为 0（这是前提，不是断言重点）。
    assert_eq!(result.total_rows(), 0, "{label}: 本用例应查到 0 行");

    // ② 列名不能丢：空结果仍是「有列无行」的结果集（洞察空表取样靠列名建临时表）。
    assert_eq!(
        result.columns,
        vec!["x".to_string(), "y".to_string()],
        "{label}: 0 行结果也要返回列名（x / y 顺序契约）"
    );

    println!("✅ {label}: 0 行结果返回列名 {:?}", result.columns);
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_zero_row_keeps_columns() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    let db = PostgresDatabase::new(&url).await.expect("连上 PG(sqlx)");
    probe(&db, "PG(sqlx)").await;

    let db = PostgresNativeDatabase::new(&url)
        .await
        .expect("连上 PG(Official)");
    probe(&db, "PG(Official)").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_zero_row_keeps_columns() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    let db = MySqlDatabase::new(&url).await.expect("连上 MySQL(sqlx)");
    probe(&db, "MySQL(sqlx)").await;

    let db = MySqlNativeDatabase::new(&url)
        .await
        .expect("连上 MySQL(Official)");
    probe(&db, "MySQL(Official)").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_zero_row_keeps_columns() {
    let Ok(path) = std::env::var("RDS_TEST_SQLITE_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_SQLITE_PATH，跳过");
        return;
    };
    let db = SqliteDatabase::new(&path).expect("打开 SQLite");
    probe(&db, "SQLite").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duckdb_zero_row_keeps_columns() {
    let Ok(path) = std::env::var("RDS_TEST_DUCKDB_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_DUCKDB_PATH，跳过");
        return;
    };
    let db = DuckDbDatabase::new(&path).expect("打开 DuckDB");
    probe(&db, "DuckDB").await;
}
