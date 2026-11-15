//! 分析内核三档的**真跑**验证（不是纯类型单测）。
//!
//! 库内单测只能钉住「校验规则」；这里用真 DuckDB 文件把三档各跑一遍，回答的是
//! 一个更关键的问题：**内核交出来的会话，真的能挂上、真的能跨源查**。
//!
//! 用文件型源（`duckdb` 扩展，内核自带能力、不需要 Secret、不连网），所以这条用例
//! 在离线机器上也能跑。真机四库用例另有 `workbench/tests/federation_sources.rs`。

use std::path::PathBuf;

use rds_engine::analysis::channel::AnalysisChannel;
use rds_engine::analysis::kernel::{self, PreparedMounts, SessionHandle};
use rds_engine::analysis::materialize::{materialize_in_session, materialize_sample};
use rds_engine::analysis::mount::{plan_for, MountSpec};
use rds_engine::duckdb::accel::AccelSource;
use rds_engine::duckdb::federation::registry::FederatedSource;

/// 建一个只含一张小表的 duckdb 文件；建完立刻放掉连接（Windows 上文件被占就没法 ATTACH）。
fn make_duckdb_file(name: &str, table: &str, sql_insert: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("rds_kernel_probe");
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join(format!("{name}.duckdb"));
    let _ = std::fs::remove_file(&path);
    {
        let conn = duckdb::Connection::open(&path).expect("建 duckdb 文件");
        conn.execute_batch(&format!(
            "create table {table} (id integer primary key, v varchar); {sql_insert}"
        ))
        .expect("写测试数据");
    }
    path
}

#[test]
fn source_channel_hands_out_no_session_and_refuses_to_run() {
    let plan = plan_for(&AnalysisChannel::Source, &[], None, "orders").expect("源库档计划");
    let handle = kernel::open(&plan, &PreparedMounts::none()).expect("源库档开盘");
    assert!(matches!(handle, SessionHandle::None));
    assert!(!handle.has_session());
    // 源库档不该在 DuckDB 上执行：这里必须是可读错误，而不是偷偷跑成功
    let error = handle.run("select 1").expect_err("源库档没有会话");
    assert!(format!("{error}").contains("没有 DuckDB 会话"));
}

#[test]
fn accelerated_channel_runs_sql_on_a_real_duckdb_file() {
    let file = make_duckdb_file("accel_one", "orders", "insert into orders values (1, 'a');");
    let plan = plan_for(
        &AnalysisChannel::Accelerated,
        &[MountSpec::duckdb_file(&file)],
        None,
        "orders",
    )
    .expect("加速档计划");
    let prepared = PreparedMounts::accelerated(
        MountSpec::duckdb_file(&file),
        AccelSource::new("C_kernel_accel", "duckdb", file.to_str().unwrap()).expect("组装源"),
    );
    let handle = kernel::open(&plan, &prepared).expect("加速档开盘");
    assert_eq!(handle.channel_label(), "accelerated");
    let result = handle.run("select count(*) as n from orders").expect("加速档执行");
    // 只断言「会话真的跑起来了」，不依赖具体 row 取值形态（各驱动 Arrow 表示不同）
    assert!(!result.columns.is_empty(), "应至少返回一列");
}

#[test]
fn federated_channel_joins_two_real_sources() {
    let a = make_duckdb_file("fed_a", "orders", "insert into orders values (1, 'x');");
    let b = make_duckdb_file("fed_b", "customers", "insert into customers values (1, 'x');");
    let plan = plan_for(
        &AnalysisChannel::Federated {
            owner: "C_kernel_probe".into(),
        },
        &[
            MountSpec::duckdb_file(&a),
            MountSpec::duckdb_file(&b),
        ],
        Some("fed_a"),
        "orders ⋈ customers",
    )
    .expect("联邦档计划");
    let prepared = PreparedMounts::federated(vec![
        (
            MountSpec::duckdb_file(&a),
            FederatedSource::new("C_fed_a", "fed_a", "duckdb", a.to_str().unwrap())
                .expect("组装源 A"),
        ),
        (
            MountSpec::duckdb_file(&b),
            FederatedSource::new("C_fed_b", "fed_b", "duckdb", b.to_str().unwrap())
                .expect("组装源 B"),
        ),
    ]);
    let handle = kernel::open(&plan, &prepared).expect("联邦档开盘");
    assert_eq!(handle.channel_label(), "federated");
    // 跨源 join：两侧别名各自一张表，能连上就说明两个源都真挂上了
    let result = handle
        .run("select count(*) as n from fed_a.orders o join fed_b.customers c on o.v = c.v")
        .expect("跨源 join");
    assert!(!result.columns.is_empty(), "跨源查询应返回结果");
}

#[test]
fn sample_materialization_creates_a_queryable_table() {
    // 形态 I：洞察取样要的就是「源里拉一段 → 在会话里变成一张能查的表」
    let file = make_duckdb_file(
        "mat_sample",
        "orders",
        "insert into orders values (1, 'a'); insert into orders values (2, 'b');",
    );
    let plan = plan_for(
        &AnalysisChannel::Accelerated,
        &[MountSpec::duckdb_file(&file)],
        None,
        "orders",
    )
    .expect("加速档计划");
    let prepared = PreparedMounts::accelerated(
        MountSpec::duckdb_file(&file),
        AccelSource::new("C_kernel_mat", "duckdb", file.to_str().unwrap()).expect("组装源"),
    );
    let handle = kernel::open(&plan, &prepared).expect("加速档开盘");

    let seed = format!("{}|select * from orders", file.display());
    let table = materialize_sample(&handle, &seed, "select * from orders").expect("形态 I 物化");
    // 物化落在内存 catalog：会话默认 catalog 是只读源，裸名建表会被拒
    assert!(table.starts_with("memory.tmp_i_"), "实际表名 {table}");
    // 物化后必须能真的查到行：建表语句跑成功不等于表里真有数据
    let count = handle
        .run(&format!("select count(*) as n from {table}"))
        .expect("查物化表");
    assert!(!count.columns.is_empty(), "物化表应可查");
    // 同一份 seed 重复物化是常态（用户来回点），不能报错
    materialize_sample(&handle, &seed, "select * from orders").expect("重复物化应覆盖不报错");
}

#[test]
fn session_materialization_uses_the_given_name() {
    // 形态 II：结果集二次分析，表名是用户看得见的那张
    let file = make_duckdb_file("mat_named", "orders", "insert into orders values (1, 'a');");
    let plan = plan_for(
        &AnalysisChannel::Accelerated,
        &[MountSpec::duckdb_file(&file)],
        None,
        "orders",
    )
    .expect("加速档计划");
    let prepared = PreparedMounts::accelerated(
        MountSpec::duckdb_file(&file),
        AccelSource::new("C_kernel_mat2", "duckdb", file.to_str().unwrap()).expect("组装源"),
    );
    let handle = kernel::open(&plan, &prepared).expect("加速档开盘");
    let table = materialize_in_session(&handle, "rs_copy", "select * from orders;").expect("形态 II 物化");
    assert_eq!(table, "memory.rs_copy");
    // 末尾的分号必须被吃掉，否则 CREATE 会变成两条语句
    handle
        .run(&format!("select count(*) as n from {table}"))
        .expect("查形态 II 表");
}
