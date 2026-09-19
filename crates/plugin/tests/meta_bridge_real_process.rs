//! `meta.*` 的真实进程端到端测试（P2 的第一半：驱动进得了导航与搜索）
//!
//! 靶子（`tests/fixture/sidecar.rs`）带的是一份写死的假 schema（`main` / `public` + `sales`）。
//! 这里考察的是**元数据契约**能不能真的过进程边界：
//!
//! - 三层都在：catalogs → schemas → objects（且 `meta.objects` **一次给全、含 kind**）
//! - 五个文件夹各自的类别分得开（表 / 视图 + 物化视图 / 例程 / 序列 / 触发器）
//! - 认不出的类别（`domain`）**跳过并留痕**，不画成表
//! - 对象详情带得出列、类型归一化（`canonical` / `format`）、主键、注释
//! - 能力为假时如实拒绝（`-32006`），而不是假装「没有」
//! - 连不上库有自己的一等码（`-32009`），不再与 SQL 错混在一起
//!
//! ⚠️ 这里**不连真库**：假 schema 是靶子写死的。接真库那一步要 PostgreSQL 在场（实机验收）。

use std::time::{Duration, Instant};

use serde_json::json;

use rds_plugin::manifest::BackendCommand;
use rds_plugin::sidecar::conn::CallError;
use rds_plugin::sidecar::driver::{DriverError, SessionDriver};
use rds_plugin::sidecar::lifecycle::{Concurrency, ProcessSpec};
use rds_plugin::sidecar::meta::MetaObjectKind;
use rds_plugin::sidecar::proto::RpcErrorCode;
use rds_plugin::sidecar::supervisor::{
    Deployment, SessionOpened, SidecarSupervisor, SupervisorError,
};

/// 起一个靶子实例并开一条会话，返回（supervisor, 会话 id）。
async fn session_for(plugin_id: &str) -> (SidecarSupervisor, String) {
    session_for_with(plugin_id, &[]).await
}

/// 同上，但给靶子带命令行参数（如 `--cap-na=sequences`）。
async fn session_for_with(plugin_id: &str, args: &[&str]) -> (SidecarSupervisor, String) {
    let mut supervisor = SidecarSupervisor::new();
    supervisor
        .deploy(Deployment {
            plugin_id: plugin_id.to_string(),
            spec: ProcessSpec::new(["fixture"], 1, Concurrency::Serial),
            command: BackendCommand {
                program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_rds-sidecar-fixture")),
                args: args.iter().map(|a| (*a).to_string()).collect(),
            },
            env: Vec::new(),
            protocol: Some("rds-driver/1".to_string()),
        })
        .unwrap();

    let opened = supervisor
        .open_session(plugin_id, "fixture", "s1", json!({}), Instant::now())
        .await
        .expect("开会话应当成功");
    assert!(matches!(opened, SessionOpened::Live { .. }), "{opened:?}");
    (supervisor, "s1".to_string())
}

/// 三层都在：catalog → schema → 对象。
#[tokio::test]
async fn the_tree_has_all_three_levels() {
    let (supervisor, session_id) = session_for("test.meta.tree").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    assert_eq!(
        driver.meta_catalogs().await.expect("catalog 清单应当拿得到"),
        vec!["main".to_string()]
    );

    let schemas = driver.meta_schemas("main").await.expect("schema 清单");
    assert_eq!(
        schemas.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["public", "sales"]
    );
    assert_eq!(schemas[0].comment.as_deref(), Some("默认 schema"));
    assert_eq!(schemas[1].comment, None);

    let objects = driver.meta_objects("main", "public").await.expect("对象清单");
    assert_eq!(
        objects.len(),
        9,
        "靶子 public 下 9 个对象（含一个导航摆不下的 domain）"
    );

    let nodes: Vec<_> = objects
        .into_iter()
        .filter_map(|o| o.into_node_info())
        .collect();
    assert_eq!(nodes.len(), 8, "domain 摆不进五个文件夹，应当被跳过而不是画成表");
    assert!(
        nodes.iter().all(|n| n.name != "order_state"),
        "认不出的类别不进树"
    );

    // 空 schema 是空，不是错
    let empty = driver.meta_objects("main", "没有这个 schema").await;
    assert!(empty.expect("空 schema 不该报错").is_empty());
}

/// `meta.objects` 一次给全，宿主按 kind 分流到五个文件夹。
#[tokio::test]
async fn one_introspection_serves_all_five_folders() {
    let (supervisor, session_id) = session_for("test.meta.folders").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let objects = driver.meta_objects("main", "public").await.expect("对象清单");
    let names = |kind: &MetaObjectKind| -> Vec<String> {
        objects
            .iter()
            .filter(|o| &o.kind == kind)
            .map(|o| o.name.clone())
            .collect()
    };

    assert_eq!(names(&MetaObjectKind::Table), vec!["orders", "customers"]);
    assert_eq!(names(&MetaObjectKind::View), vec!["recent_orders"]);
    assert_eq!(names(&MetaObjectKind::MaterializedView), vec!["mv_daily"]);
    assert_eq!(
        names(&MetaObjectKind::Procedure),
        vec!["refresh_orders"],
        "过程是过程"
    );
    assert_eq!(names(&MetaObjectKind::Function), vec!["order_total"]);
    assert_eq!(names(&MetaObjectKind::Sequence), vec!["orders_seq"]);
    assert_eq!(names(&MetaObjectKind::Trigger), vec!["trg_orders_audit"]);

    // 物化视图与视图进同一个文件夹（导航只有五处），但原始类别没丢
    let view_folder: Vec<String> = objects
        .iter()
        .filter(|o| o.nav_kind() == Some(engine::driver::SchemaObjectKind::View))
        .map(|o| o.name.clone())
        .collect();
    assert_eq!(view_folder, vec!["recent_orders", "mv_daily"]);

    // 触发器带得出所属表（NodeInfo::parent_name 唯一的使用者）
    let trigger = objects
        .into_iter()
        .find(|o| o.kind == MetaObjectKind::Trigger)
        .and_then(|o| o.into_node_info())
        .expect("触发器摆得进树");
    assert_eq!(trigger.parent_name.as_deref(), Some("orders"));
}

/// 对象详情：列、类型归一化、主键、注释都要过得了进程边界。
#[tokio::test]
async fn object_detail_carries_columns_and_canonical_types() {
    let (supervisor, session_id) = session_for("test.meta.detail").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let detail = driver
        .meta_object_detail("main", "public", "orders")
        .await
        .expect("详情应当拿得到");

    assert_eq!(detail.object.name, "orders");
    assert_eq!(detail.object.kind, MetaObjectKind::Table);
    assert_eq!(detail.indexes, Some(3));
    assert_eq!(detail.row_count, Some(12000));

    let names: Vec<&str> = detail.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["id", "customer_id", "amount", "created_at", "note"]);

    let id = detail.columns[0].clone().into_column_detail();
    assert!(id.is_primary_key && !id.nullable);
    assert_eq!(id.comment.as_deref(), Some("主键"));

    // 类型归一化：属性面板看原始类型，程序看 canonical（§4.5.1）
    let amount = detail.columns[2].clone().into_column_detail();
    assert_eq!(amount.data_type, "numeric(38,10)");
    assert_eq!(
        amount.extra.get("canonical").map(String::as_str),
        Some("DECIMAL")
    );
    assert_eq!(
        amount.extra.get("format").map(String::as_str),
        Some("decimal(scale=10)")
    );
    assert_eq!(amount.comment.as_deref(), Some("订单金额"));

    // 时间列：归一化到 TIMESTAMP，原始类型（含时区）不丢
    let created_at = &detail.columns[3];
    assert_eq!(created_at.type_raw, "timestamp with time zone");
    assert_eq!(created_at.canonical.as_deref(), Some("TIMESTAMP"));

    // 视图也有详情（点开视图要能看列）
    let view = driver
        .meta_object_detail("main", "public", "recent_orders")
        .await
        .expect("视图详情应当拿得到");
    assert_eq!(view.object.kind, MetaObjectKind::View);
    assert_eq!(view.columns.len(), 2);

    // 不存在的对象：如实报错（不是"空详情"）
    let missing = driver
        .meta_object_detail("main", "public", "没有这个表")
        .await
        .expect_err("不存在的对象应当报错");
    assert!(missing.is_sql_error(), "{missing:?}");
}

/// 例程源码：拿得到就是字符串，查不到就是 `None`（两种形状，没有第三种）。
#[tokio::test]
async fn routine_source_is_a_string_or_none() {
    let (supervisor, session_id) = session_for("test.meta.routine").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let source = driver
        .meta_routine_source("main", "public", "order_total")
        .await
        .expect("例程源码应当拿得到")
        .expect("靶子有这个函数");
    assert!(source.starts_with("CREATE FUNCTION order_total"), "{source}");

    // 表不是例程：`null`，不是错误
    let none = driver
        .meta_routine_source("main", "public", "orders")
        .await
        .expect("查不到不该报错");
    assert_eq!(none, None);
}

/// 没声明能力时：方法如实回 `-32006`（UI 据此置灰并说明），对象类别也**不报**。
#[tokio::test]
async fn a_denied_capability_is_reported_as_such() {
    let (supervisor, session_id) = session_for_with(
        "test.meta.gate",
        &["--cap-na=schemas", "--cap-na=routines"],
    )
    .await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let descriptor = driver.describe("fixture").await.expect("描述应当成功");
    assert!(!descriptor.supports("schemas"));
    assert!(!descriptor.supports("routines"));
    assert!(descriptor.supports("tables"), "只关掉点名的那两个");

    let denied = driver
        .meta_schemas("main")
        .await
        .expect_err("没声明 schemas 就该拒绝");
    assert!(denied.is_capability_denied(), "{denied:?}");

    let denied = driver
        .meta_routine_source("main", "public", "order_total")
        .await
        .expect_err("没声明 routines 就该拒绝");
    assert!(denied.is_capability_denied(), "{denied:?}");

    // 对象清单照常：但例程那两类不报（「看不到」与「没有」在驱动侧是同一件事）
    let objects = driver.meta_objects("main", "public").await.expect("对象清单");
    assert!(
        objects.iter().all(|o| !matches!(
            o.kind,
            MetaObjectKind::Procedure | MetaObjectKind::Function
        )),
        "没声明 routines 就不该报例程：{objects:?}"
    );
    assert!(
        objects.iter().any(|o| o.kind == MetaObjectKind::Sequence),
        "别的类别照旧"
    );
}

/// 连不上库有自己的一等码：`-32009`，落 `CallError::Rpc`，**不是** SQL 错。
#[tokio::test]
async fn a_connect_failure_carries_its_own_code() {
    let plugin_id = "test.meta.connect";
    let mut supervisor = SidecarSupervisor::new();
    supervisor
        .deploy(Deployment {
            plugin_id: plugin_id.to_string(),
            spec: ProcessSpec::new(["fixture"], 1, Concurrency::Serial),
            command: BackendCommand {
                program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_rds-sidecar-fixture")),
                args: Vec::new(),
            },
            env: Vec::new(),
            protocol: Some("rds-driver/1".to_string()),
        })
        .unwrap();

    // 靶子按 `params.fail` 故意拒绝这次连接
    let error = supervisor
        .open_session(
            plugin_id,
            "fixture",
            "s1",
            json!({ "fail": true }),
            Instant::now(),
        )
        .await
        .expect_err("连接失败应当报出来");

    match error {
        SupervisorError::Rpc { method, error } => {
            assert_eq!(method, "session.open");
            match error {
                CallError::Rpc { code, message, .. } => {
                    assert_eq!(code, RpcErrorCode::ConnectFailed);
                    assert_eq!(code.code(), -32009);
                    assert_eq!(code.name(), "connect_failed");
                    assert!(message.contains("连接失败"), "{message}");
                }
                other => panic!("连接失败应当是一等错误码：{other:?}"),
            }
        }
        other => panic!("开会话失败应当是 SupervisorError::Rpc：{other:?}"),
    }

    supervisor
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// 靶子自己也得守住「见 stdin EOF 即退」——元数据插件同样不能留后台进程。
///
/// 这条不是新检查（`sidecar_conformance.rs` 里有正式的），这里只确认元数据那条路
/// 收摊后进程真的走了：会话关掉 + supervisor 收摊，不靠 Drop。
#[tokio::test]
async fn the_metadata_session_shuts_down_cleanly() {
    let (mut supervisor, session_id) = session_for("test.meta.shutdown").await;
    {
        let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);
        driver.session_ping().await.expect("会话应当活着");
        driver.meta_catalogs().await.expect("catalog 清单");
    }
    supervisor
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
    assert!(
        supervisor.session_conn(&session_id).is_none(),
        "收摊后不该还留着会话"
    );
}

/// 描述里给的 `identifier_quote` / `default_schema` 是导航生成 SQL 的依据，别丢。
#[tokio::test]
async fn the_descriptor_still_has_the_quoting_hints() {
    let (supervisor, session_id) = session_for("test.meta.describe").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);
    let descriptor = driver.describe("fixture").await.expect("描述应当成功");
    assert_eq!(descriptor.identifier_quote.as_deref(), Some("\""));
    assert_eq!(descriptor.default_schema.as_deref(), Some("public"));
    // 元数据调用不该影响会话本身
    driver.meta_catalogs().await.expect("catalog 清单");
    driver.session_ping().await.expect("元数据调用后会话仍在");
}

/// 元数据调用的错误也能落成 `DriverError`（宿主那一侧要看得出是协议错还是对端报错）。
#[tokio::test]
async fn a_meta_call_error_maps_to_a_driver_error() {
    let (supervisor, session_id) = session_for("test.meta.error").await;
    let driver = SessionDriver::new(supervisor.session_conn(&session_id).unwrap(), &session_id);

    let error = driver
        .meta_object_detail("main", "public", "没有这个对象")
        .await
        .expect_err("不存在的对象应当报错");
    match error {
        DriverError::Rpc { code, .. } => assert_eq!(code, RpcErrorCode::SqlError),
        other => panic!("对端报的业务错应当落 DriverError::Rpc：{other:?}"),
    }
}
