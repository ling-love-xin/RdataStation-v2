//! 全局分析库表 -> 洞察取样（刀 7 · B 的端到端验证）。
//!
//! 链路：`SampleSource::on_global(表)` -> 短命会话只读挂全局分析库
//! （`<system>/analytics.duckdb`，别名 `rds_global`）-> 会话内建 `tmp_i_`
//! -> 搬进进程单例登记版 `tmp_i_`。
//!
//! 用**自己造**的全局分析库验证，不依赖宿主有没有初始化：
//! engine 的测试运行器会把系统目录指到临时根（实测 `.rds/tmp/rds-test-root-<pid>/data/system`），
//! 所以这里按 `get_system_dir()` 的落点造库——走的就是生产路径，不另开测试专用入口。

use duckdb::Connection;
use rds_insight::model::{ProbeAmount, ProbeColumns, ProbeConfig, ProbeSampleMode, SampleSource};

fn config() -> ProbeConfig {
    ProbeConfig {
        amount: ProbeAmount::RowCount(5),
        sample_mode: ProbeSampleMode::Head,
        columns: ProbeColumns::All,
        filter: None,
        seed: None,
    }
}

/// 造一台「全局分析库」并塞一张表；返回 (库路径, 表名)。
fn seed_global_analysis_db() -> (std::path::PathBuf, String) {
    let dir = engine::migration::get_system_dir().expect("系统目录");
    std::fs::create_dir_all(&dir).expect("建系统目录");
    let path = dir.join("analytics.duckdb");
    // 每次重建，避免上次残留的表混进断言
    let _ = std::fs::remove_file(&path);
    let conn = Connection::open(&path).expect("建全局分析库");
    conn.execute_batch(
        "create table probe_global(id integer, name varchar);
         insert into probe_global values (1,'a'),(2,'b'),(3,'c');",
    )
    .expect("写表");
    drop(conn); // DuckDB 独占文件：不放手，后面的只读挂载会撞锁
    (path, "probe_global".to_string())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_global_table_can_be_sampled_into_the_process_singleton() {
    let (path, table_name) = seed_global_analysis_db();
    assert!(path.ends_with("analytics.duckdb"), "落点 {}", path.display());

    let source = SampleSource::on_global(&table_name, "全局表");
    assert!(
        source.sql.contains("rds_global"),
        "SQL 走固定别名：{}",
        source.sql
    );

    let table =
        rds_insight::service::persistence::sample_source_to_analysis_table(&source, &config())
            .await
            .expect("全局表取样");

    // 落地必须是**单例上的登记版** `tmp_i_`（不带 memory. 前缀），下游才看得见
    assert!(table.starts_with("tmp_i_"), "实际 {table}");
    let singleton = engine::services::duckdb_service::DuckDbService::get_or_create_duckdb()
        .expect("单例");
    let conn = singleton.lock().expect("锁");
    let count: i64 = conn
        .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
            row.get(0)
        })
        .expect("单例里查得到这张表");
    assert_eq!(count, 3, "全局表 3 行应原样搬进单例，实际 {count}");
}

/// 元数据库（`global.db`，SQLite）**不是**分析库：拿它当前缀名去挂，必须给出可读错误，
/// 而不是悄悄返回一张空表让下游以为「没数据」。
#[tokio::test(flavor = "multi_thread")]
async fn the_metadata_db_is_not_mistaken_for_the_analysis_db() {
    let dir = engine::migration::get_system_dir().expect("系统目录");
    std::fs::create_dir_all(&dir).expect("建系统目录");
    let path = dir.join("global.db");
    let _ = std::fs::remove_file(&path);
    // 造一个 SQLite 文件（头部与真 global.db 同形），用 duckdb 驱动去挂应当失败
    let sqlite = rusqlite::Connection::open(&path).expect("建 SQLite 元数据库");
    sqlite
        .execute_batch("create table connections(id text primary key, driver text);")
        .expect("写表");
    drop(sqlite);

    let err = engine::analysis::global::open_mounted("C_meta_probe", &path, "rds_meta", "duckdb")
        .err()
        .expect("SQLite 文件不能用 duckdb 驱动挂上");
    let text = err.to_string();
    assert!(
        text.contains("rds_meta") || text.contains("占用") || text.contains("不是有效"),
        "错误要说清是哪一步失败：{text}"
    );
}
