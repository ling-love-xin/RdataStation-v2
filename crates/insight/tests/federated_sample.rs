//! 联邦档取样：一条会话挂两源，每源各出一张登记版 `tmp_i_`。
//!
//! 用两台本地 DuckDB 文件当「两个源」：不需要外部数据库，挂载 / 取样 / 搬运三段的
//! 代码路径与真连接一致（`FederatedSource` + `kernel::open` + `move_sample_to_process_db`）。

use duckdb::Connection;
use engine::analysis::mount::MountSpec;
use engine::duckdb::federation::registry::FederatedSource;
use rds_insight::model::{ProbeAmount, ProbeColumns, ProbeConfig, ProbeSampleMode};
use rds_insight::service::persistence::{sample_federated_sources, FederatedSampleEntry};

fn config() -> ProbeConfig {
    ProbeConfig {
        amount: ProbeAmount::RowCount(10),
        sample_mode: ProbeSampleMode::Head,
        columns: ProbeColumns::All,
        filter: None,
        seed: None,
    }
}

fn seed(path: &std::path::Path, ddl: &str) {
    let conn = Connection::open(path).expect("建库");
    conn.execute_batch(ddl).expect("写表");
    drop(conn); // DuckDB 独占文件
}

/// 一条本地 DuckDB 文件源（别名 = 会话里的 catalog 名）。
fn file_entry(
    path: &std::path::Path,
    alias: &str,
    table: &str,
    label: &str,
) -> FederatedSampleEntry {
    let url = path.to_string_lossy().to_string();
    let source = FederatedSource::new(&format!("C_{alias}"), alias, "duckdb", &url)
        .expect("组装 duckdb 源");
    FederatedSampleEntry {
        mount: MountSpec::duckdb_file(path),
        source,
        sql: format!("SELECT * FROM {alias}.main.{table}"),
        label: label.to_string(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn each_federated_source_lands_its_own_registered_table() {
    let dir = std::env::temp_dir().join(format!("rds-fed-sample-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let a = dir.join("sales.duckdb");
    let b = dir.join("stock.duckdb");
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    seed(
        &a,
        "create table orders(id integer, amt decimal(10,2));
         insert into orders values (1, 10.50), (2, 20.25), (3, 30.00);",
    );
    seed(
        &b,
        "create table items(id integer);
         insert into items values (7), (8);",
    );

    let landed = sample_federated_sources(
        "C_insight_fed_sample",
        vec![
            file_entry(&a, "sales", "orders", "销售"),
            file_entry(&b, "stock", "items", "库存"),
        ],
        &config(),
    )
    .await
    .expect("联邦取样");

    assert_eq!(landed.len(), 2, "每源一份：{landed:?}");
    assert_eq!(landed[0].0, "销售");
    assert_eq!(landed[1].0, "库存");
    for (label, table) in &landed {
        assert!(table.starts_with("tmp_i_"), "{label} 落地不是登记版：{table}");
    }
    assert_ne!(landed[0].1, landed[1].1, "两源必须各建各的表");

    // 落到进程单例里，且行数与源一致（下游 EDA / 导出读的就是这张）
    let singleton =
        engine::services::duckdb_service::DuckDbService::get_or_create_duckdb().expect("单例");
    let conn = singleton.lock().expect("锁");
    let count = |table: &str| -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
            row.get(0)
        })
        .expect("单例里查得到")
    };
    assert_eq!(count(&landed[0].1), 3, "销售源 3 行");
    assert_eq!(count(&landed[1].1), 2, "库存源 2 行");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 撞别名必须在开会话之前拦住：两条源挂上去会是同一个 catalog，
/// 「SQL 里写哪个名字」不可判定——这种错不该留到查询时报「表不存在」。
#[tokio::test(flavor = "multi_thread")]
async fn duplicate_aliases_are_rejected_before_opening_a_session() {
    let dir = std::env::temp_dir().join(format!("rds-fed-dup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let a = dir.join("a.duckdb");
    let b = dir.join("b.duckdb");
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    seed(&a, "create table t(id integer); insert into t values (1);");
    seed(&b, "create table t(id integer); insert into t values (2);");

    let err = sample_federated_sources(
        "C_insight_fed_dup",
        vec![
            file_entry(&a, "same", "t", "甲"),
            file_entry(&b, "same", "t", "乙"),
        ],
        &config(),
    )
    .await
    .err()
    .expect("别名撞名必须拒");
    assert!(err.to_string().contains("撞名"), "{err}");

    let _ = std::fs::remove_dir_all(&dir);
}
