//! 真机回归：`QueryResult.is_read_only` 是**结论**，不是装饰。
//!
//! ## 为什么单开一个文件
//!
//! 这个字段的生产者一度形同虚设：四个网络驱动（`postgres` / `mysql` 走 sqlx，
//! `postgres_native` / `mysql_native` 走原生协议）的 `build_query_result` 参数带下划线、
//! 值被丢弃；SQLite 两条路径同样丢掉；DuckDB 的直连路径自己内联了一份只认三个前缀的
//! 判定（连 `EXPLAIN` / `PRAGMA` 都漏了，于是 `EXPLAIN SELECT 1` 被判成写语句）。
//! 唯一消费者（`SqlService` 的历史记录：`Some(false)` 才记 `rows_affected`）
//! 因此在查询路径上**永远拿不到值**。
//!
//! 本轮把口径收敛成一份（`driver::utils::is_read_only_sql`，四种方言并排）并让结论
//! 真正落到字段上。这个文件钉的就是「字段有值、且口径正确」：
//!
//! | 库 | 语句 | 期望 |
//! | --- | --- | --- |
//! | PG（两个驱动） | `SELECT 1` | `Some(true)` |
//! | PG（两个驱动） | `SHOW search_path`（元信息语句） | `Some(true)` |
//! | MySQL（两个驱动） | `SELECT 1` | `Some(true)` |
//! | MySQL（两个驱动） | `SHOW TABLES` | `Some(true)` |
//! | SQLite | `SELECT 1` / `PRAGMA database_list` | `Some(true)` |
//! | DuckDB | `SELECT 1` / `EXPLAIN SELECT 1` | `Some(true)` |
//!
//! ## 边界（如实说清）
//!
//! * **不写 DML**：真机上是别人的数据，这里只跑不会落盘的语句。写语句那条路
//!   （`affected_rows_result` → `Some(false)`）由 `driver::utils` 的单测钉住。
//! * 真机上的 `SET` 不走这里：它不返回结果集，走 `execute` 那条路能否拿到结论
//!   （`SET` 被算作只读 → 走 `query`）由上面那份单测钉住。
//!
//! 未设环境变量时跳过（与仓内其余真机套件同规矩）：
//!
//! ```text
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! RDS_TEST_SQLITE_PATH='D:\FossilT\T.fossil' \
//! RDS_TEST_DUCKDB_PATH='D:\data\123' \
//!   cargo test -p rds-engine --test read_only_flag_from_real_servers -- --nocapture
//! ```

use rds_engine::driver::Database;
use rds_engine::driver::native::duckdb::DuckDbDatabase;
use rds_engine::driver::native::mysql::MySqlDatabase;
use rds_engine::driver::native::mysql_native::MySqlNativeDatabase;
use rds_engine::driver::native::postgres::PostgresDatabase;
use rds_engine::driver::native::postgres_native::PostgresNativeDatabase;
use rds_engine::driver::native::sqlite::SqliteDatabase;

/// 这一条语句必须报「只读」——**包括它是从哪个驱动出来的**。
///
/// 失败信息里带 SQL 与驱动名：这条断言的价值全在「哪个驱动在哪种语句上漏了」。
async fn assert_read_only<D: Database>(db: &D, driver: &str, sql: &str) {
    let result = db
        .query(sql)
        .await
        .unwrap_or_else(|e| panic!("{driver}: `{sql}` 执行失败：{e}"));
    assert_eq!(
        result.is_read_only,
        Some(true),
        "{driver}: `{sql}` 的 is_read_only 应为 Some(true)（出口层按它分「返回行 / 影响行」）"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_reports_read_only() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    println!("\n=== PostgreSQL（sqlx）===");
    let db = PostgresDatabase::new(&url).await.expect("连上 PG(sqlx)");
    assert_read_only(&db, "PG(sqlx)", "SELECT 1").await;
    assert_read_only(&db, "PG(sqlx)", "SHOW search_path").await;

    println!("\n=== PostgreSQL（Official）===");
    let db = PostgresNativeDatabase::new(&url)
        .await
        .expect("连上 PG(Official)");
    assert_read_only(&db, "PG(Official)", "SELECT 1").await;
    assert_read_only(&db, "PG(Official)", "SHOW search_path").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_reports_read_only() {
    let Ok(url) = std::env::var("RDS_TEST_MYSQL_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
        return;
    };
    println!("\n=== MySQL（sqlx）===");
    let db = MySqlDatabase::new(&url).await.expect("连上 MySQL(sqlx)");
    assert_read_only(&db, "MySQL(sqlx)", "SELECT 1").await;
    assert_read_only(&db, "MySQL(sqlx)", "SHOW TABLES").await;

    println!("\n=== MySQL（Official）===");
    let db = MySqlNativeDatabase::new(&url)
        .await
        .expect("连上 MySQL(Official)");
    assert_read_only(&db, "MySQL(Official)", "SELECT 1").await;
    assert_read_only(&db, "MySQL(Official)", "SHOW TABLES").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_reports_read_only() {
    let Ok(path) = std::env::var("RDS_TEST_SQLITE_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_SQLITE_PATH，跳过");
        return;
    };
    println!("\n=== SQLite ===");
    let db = SqliteDatabase::new(&path).expect("打开 SQLite");
    assert_read_only(&db, "SQLite", "SELECT 1").await;
    // `PRAGMA` 是 SQLite 的查询式语句：漏判会让它走写路径、丢掉结果集。
    assert_read_only(&db, "SQLite", "PRAGMA database_list").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn duckdb_reports_read_only() {
    let Ok(path) = std::env::var("RDS_TEST_DUCKDB_PATH") else {
        eprintln!("⏭️  未设 RDS_TEST_DUCKDB_PATH，跳过");
        return;
    };
    println!("\n=== DuckDB ===");
    let db = DuckDbDatabase::new(&path).expect("打开 DuckDB");
    assert_read_only(&db, "DuckDB", "SELECT 1").await;
    // 这条是回归点：直连路径的旧内联判定只认 SELECT / SHOW / DESCRIBE，
    // 把 `EXPLAIN` 判成了写语句（同一句话在 `query` 与事务两条路上结论不同）。
    assert_read_only(&db, "DuckDB", "EXPLAIN SELECT 1").await;
    assert_read_only(&db, "DuckDB", "PRAGMA table_info('sqlite_master')").await;
}
