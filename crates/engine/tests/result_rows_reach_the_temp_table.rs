//! 真机回归：查询结果的行要**真的**到达 DuckDB 临时表。
//!
//! ## 修的是什么
//!
//! 结果筛选（`engine::services::execution_service::re_execute_with_filter`，编辑器的
//! 「结果筛选」入口）此前这么拿行：
//!
//! ```ignore
//! let json_value = serde_json::to_value(&result.result)?;   // QueryResult → JSON
//! let rows = extract_rows_from_serialized(&json_value);     // 读 json["batches"]
//! ```
//!
//! 两条都不成立：契约序列化**不含** Arrow `batches`（只输出 `columns` / `rows` / …），
//! 而 native 驱动（MySQL / PostgreSQL / SQLite / DuckDB）**只填 `batches`**、
//! `rows` 字段恒为空。于是行永远是零行 —— 临时表建出来是空表，筛选 / 导出 / 分析
//! 全部在空表上跑，而且**不报错**（这正是它一直没被发现的原因）。
//!
//! 同一坑在洞察侧踩过一次（`insight::service::result_columns_and_rows` 有注释与回归），
//! 那里改成进程内直读 `to_rows()`；引擎侧这一处漏了。现在两处一致：
//! `duckdb_service::rows_as_json` 直读 Arrow，**不经 JSON 契约往返**。
//!
//! ## 这个文件钉什么
//!
//! 「驱动 → 行 → 临时表」整条链：真机查 3 行 → 直读拿到 3 行 → 建临时表 → **查回来还是 3 行**。
//! 只断言行数（列类型推断与值域不是这条链的事）。
//!
//! 未设环境变量时跳过（与仓内其余真机套件同规矩）：
//!
//! ```text
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil' \
//! RDS_TEST_DUCKDB_PATH='D:\data\123' \
//!   cargo test -p rds-engine --test result_rows_reach_the_temp_table -- --nocapture
//! ```

use rds_engine::driver::Database;
use rds_engine::driver::native::duckdb::DuckDbDatabase;
use rds_engine::driver::native::mysql::MySqlDatabase;
use rds_engine::driver::native::mysql_native::MySqlNativeDatabase;
use rds_engine::driver::native::postgres::PostgresDatabase;
use rds_engine::driver::native::postgres_native::PostgresNativeDatabase;
use rds_engine::driver::native::sqlite::SqliteDatabase;
use rds_engine::services::duckdb_service::{self, DuckDbService};

/// 三家方言都认的三行（`VALUES` 的派生表别名只有 PG / MySQL 认，所以用 `UNION ALL`）。
const THREE_ROWS: &str = "SELECT 1 AS x UNION ALL SELECT 2 UNION ALL SELECT 3";

/// 驱动 → 行 → 临时表 → 查回来。
async fn probe<D: Database>(db: &D, label: &str) {
    let result = db.query(THREE_ROWS).await.expect("查询三行");

    // ① 引擎侧的取数口：直读 Arrow（旧路径在这里返回零行）。
    let columns = result.columns.clone();
    let rows = duckdb_service::rows_as_json(&result);
    assert_eq!(
        rows.len(),
        3,
        "{label}: 行在 Arrow `batches` 里，直读必须拿得到（旧实现读 JSON 的 batches → 零行）"
    );

    // ② 建临时表并查回来：这一步证明「行真的到了表里」，而不只是中间变量不为空。
    let table = DuckDbService::create_duckdb_temp_table(&columns, &rows)
        .unwrap_or_else(|e| panic!("{label}: 建临时表失败：{e}"));
    let conn = DuckDbService::get_or_create_duckdb().expect("内存 DuckDB");
    let guard = conn.lock().expect("锁 DuckDB");
    let count: i64 = guard
        .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| r.get(0))
        .unwrap_or_else(|e| panic!("{label}: 查临时表失败：{e}"));
    assert_eq!(count, 3, "{label}: 临时表里应当正好三行（空表正是这条回归的症状）");

    // 收尾：临时表按来源定向回收（建表方的责任）。
    drop(guard);
    let conn = DuckDbService::get_or_create_duckdb().expect("内存 DuckDB");
    let guard = conn.lock().expect("锁 DuckDB");
    rds_engine::duckdb::drop_temp_table(&guard, rds_engine::duckdb::TempTableSource::Query, &table)
        .expect("删临时表");
    println!("✅ {label}: 3 行进表、3 行查回");
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_rows_reach_the_temp_table() {
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
async fn mysql_rows_reach_the_temp_table() {
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
async fn sqlite_rows_reach_the_temp_table() {
    let Ok(path) = std::env::var("RDS_TEST_SQLITE_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_SQLITE_PATH，跳过");
        return;
    };
    let db = SqliteDatabase::new(&path).expect("打开 SQLite");
    probe(&db, "SQLite").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duckdb_rows_reach_the_temp_table() {
    let Ok(path) = std::env::var("RDS_TEST_DUCKDB_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_DUCKDB_PATH，跳过");
        return;
    };
    let db = DuckDbDatabase::new(&path).expect("打开 DuckDB");
    probe(&db, "DuckDB").await;
}
