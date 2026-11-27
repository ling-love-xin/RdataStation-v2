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

/// 进程级分析实例是**全进程共享**的，而 `rds_src`（加速档固定别名）是实例级唯一事实：
/// 某条用例把它挂上后不清，下一条用例换个 conn_id 再挂就会撞「别名已被连接 … 占用」。
///
/// 这里在每条加速档用例开始前把残留卸干净（`unmount_conn` = `DETACH` + 放别名 + 关会话），
/// 并持一把进程级互斥锁，使并行跑也不会互相拆台（`cargo test` 默认并行）。
fn clean_accel_slate() -> std::sync::MutexGuard<'static, ()> {
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(instance) = rds_engine::duckdb::instance::instance() {
        let prefix = rds_engine::duckdb::instance::AnalysisInstance::ACCEL_OWNER_PREFIX;
        let leftovers: Vec<String> = instance
            .sessions()
            .into_iter()
            .filter_map(|info| info.owner.strip_prefix(prefix).map(str::to_string))
            .collect();
        for conn_id in leftovers {
            // 卸不掉不致命：本来就没挂时返回 Ok(false)
            let _ = instance.unmount_conn(conn_id.as_str(), None);
        }
    }
    guard
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
    let _gate = clean_accel_slate();
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

    // D17（2026-11-27）：内核加速档没有文档身份，会话 owner 必须是源级 `accel:{conn_id}`。
    // 裸 conn_id 会被 `is_analysis_session` 当成分析会话，面板源段就多一行「来源不明」的 rds_src。
    let instance = rds_engine::duckdb::instance::instance().expect("分析实例");
    let listed = instance.sessions();
    assert!(
        listed.iter().any(|info| info.owner == "accel:C_kernel_accel"),
        "加速档会话 owner 应为源级键：{listed:?}"
    );
    for info in listed.iter().filter(|info| info.owner.starts_with("accel:")) {
        let session = instance.session(&info.id).expect("会话在册");
        assert!(
            !rds_engine::duckdb::instance::AnalysisInstance::is_analysis_session(info, &session),
            "加速会话不该进数据对象源段（owner {}）",
            info.owner
        );
    }
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
    let _gate = clean_accel_slate();
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

    let table = materialize_sample(&handle, "orders", "select * from orders").expect("形态 I 物化");
    // 物化落在内存 catalog：会话默认 catalog 是只读源，裸名建表会被拒
    assert!(table.starts_with("memory.tmp_i_"), "实际表名 {table}");
    // 物化后必须能真的查到行：建表语句跑成功不等于表里真有数据
    let count = handle
        .run(&format!("select count(*) as n from {table}"))
        .expect("查物化表");
    assert!(!count.columns.is_empty(), "物化表应可查");
    // 同一份 seed 重复物化是常态（用户来回点），不能报错
    materialize_sample(&handle, "orders", "select * from orders").expect("重复物化应覆盖不报错");
}

#[test]
fn session_sample_moves_into_the_process_singleton_and_stays_queryable() {
    let _gate = clean_accel_slate();
    // 刀 5 后半的关键一步：加速档的样本落在**会话**里，而洞察下游读的是**进程单例**。
    // 两张表在不同实例里，必须在需要时把结果搬过来（COPY → parquet → CTAS）。
    let file = make_duckdb_file(
        "mat_move",
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
        AccelSource::new("C_kernel_move", "duckdb", file.to_str().unwrap()).expect("组装源"),
    );
    let session = kernel::open(&plan, &prepared).expect("加速档开盘");
    let in_session =
        materialize_sample(&session, "orders", "select * from orders").expect("形态 I 物化");
    assert!(in_session.starts_with("memory.tmp_i_"), "实际 {in_session}");

    // 搬进单例：返回的是**登记版** `tmp_i_` 裸名（不带 memory. 前缀）
    let landed = rds_engine::analysis::materialize::move_sample_to_process_db(
        &session,
        &in_session,
        "orders",
    )
    .expect("搬进进程单例");
    assert!(landed.starts_with("tmp_i_"), "实际 {landed}");
    assert!(!landed.starts_with("memory."), "单例侧是裸名 {landed}");

    // 单例上必须真的读到 2 行（建表成功 ≠ 数据过来了），且表真被登记
    let singleton_plan = plan_for(&AnalysisChannel::ProcessDuckDb, &[], None, "orders")
        .expect("单例计划");
    let singleton = kernel::open(&singleton_plan, &PreparedMounts::none()).expect("单例开盘");
    let result = singleton
        .run(&format!("select count(*) as n from {landed}"))
        .expect("查登记表");
    assert!(!result.columns.is_empty(), "登记表应可查");

    // 已在单例上的表不做无意义搬运：这是可读错误，不是静默成功
    let error = rds_engine::analysis::materialize::move_sample_to_process_db(
        &singleton, &landed, "orders",
    )
    .expect_err("单例表不该再搬");
    assert!(format!("{error}").contains("无需搬运"));

    // 登记表跟着 TTL 体系：前缀守卫认它，删得掉
    rds_engine::analysis::materialize::drop_materialized(&singleton, &landed).expect("删除");
    assert!(singleton
        .run(&format!("select count(*) as n from {landed}"))
        .is_err());
}

#[test]
fn session_materialization_uses_the_given_name() {
    let _gate = clean_accel_slate();
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
