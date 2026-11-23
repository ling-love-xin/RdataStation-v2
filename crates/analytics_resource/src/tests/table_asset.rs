//! 「升级为资产」登记闭环的回归用例。
//!
//! 钉住四件事：首次登记是表型（`analytics_table` 非空、`resolve` 认得出来）、
//! 同名再升级版本接续且旧表名进快照、空名字 / 空表名直接拒绝、显示名不带 `res_` 前缀。

use super::*;

use crate::table_asset::{ExistingTableOutcome, TableAssetRequest, register, register_existing};

fn request(name: &str, table: &str) -> TableAssetRequest {
    TableAssetRequest {
        name: name.to_string(),
        analytics_table: table.to_string(),
        row_count: Some(120),
        column_count: Some(6),
    }
}

/// 首次登记：一条表型行，本体是分析库里的那张表，显示名不带前缀。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_upgrade_registers_a_table_kind_asset() {
    let (store, dir) = create_test_store().await;

    let outcome = register(&store, request("结果 3", "res_结果_3_20260101010101"))
        .await
        .expect("登记应成功");
    assert_eq!(outcome.version, 1, "首次登记就是 v1");
    assert!(!outcome.created_new_version, "首次不叫「接续版本」");
    assert_eq!(outcome.name, "结果 3", "显示名不带 res_ 前缀");

    let row = store
        .get_resource_by_id(&outcome.resource_id)
        .await
        .expect("行应读得回来");
    assert_eq!(
        crate::ArchiveKind::resolve(&row.kind, row.analytics_table.as_deref()),
        crate::ArchiveKind::AnalyticsTable,
        "analytics_table 非空 → 表型"
    );
    assert_eq!(row.analytics_table.as_deref(), Some("res_结果_3_20260101010101"));
    assert_eq!(row.file_rel_path, None, "表型资产没有本体文件路径");
    assert_eq!(row.version, 1);
    assert_eq!(row.readonly, 1, "资产一律只读");

    cleanup(dir);
}

/// 同名再升级：版本接续、本体改指新表，旧表名留在版本快照里（两张表都在）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_upgrade_bumps_the_version_and_keeps_the_old_table_name() {
    let (store, dir) = create_test_store().await;

    let first = register(&store, request("结果 3", "res_结果_3_001"))
        .await
        .expect("首次登记");
    let second = register(&store, request("结果 3", "res_结果_3_002"))
        .await
        .expect("同名再升级");

    assert_eq!(second.resource_id, first.resource_id, "同名不另起一行");
    assert_eq!(second.version, 2);
    assert!(second.created_new_version);

    let row = store
        .get_resource_by_id(&second.resource_id)
        .await
        .expect("行应读得回来");
    assert_eq!(row.analytics_table.as_deref(), Some("res_结果_3_002"), "指向新表");

    // 旧表名在 v1 快照里：回退 = 把它指回来，而不是重新搬数据
    let versions = store
        .get_resource_versions(&second.resource_id)
        .await
        .expect("版本快照应读得出来");
    let v1 = versions
        .iter()
        .find(|version| version.version == 1)
        .expect("v1 快照应存在");
    let snapshot = v1.snapshot.to_string();
    assert!(
        snapshot.contains("res_结果_3_001"),
        "旧表名必须留在快照里（回退要靠它）: {snapshot}"
    );

    cleanup(dir);
}

/// 空显示名 / 空表名直接拒绝：不写半截行。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blank_name_or_table_is_rejected() {
    let (store, dir) = create_test_store().await;

    assert!(register(&store, request("  ", "res_x")).await.is_err(), "空显示名要拒");
    assert!(register(&store, request("结果 3", "  ")).await.is_err(), "空表名要拒");
    assert!(
        store.list_alive_resources().await.expect("清单").is_empty(),
        "被拒的请求不该留下任何行"
    );

    cleanup(dir);
}

/// 「登记为资产」：给库里已有的表补身份——v1、不搬数据、不走版本。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registering_an_existing_table_gives_it_an_identity() {
    let (store, dir) = create_test_store().await;

    let outcome = register_existing(&store, request("订单宽表", "orders_wide"))
        .await
        .expect("登记应成功");
    let ExistingTableOutcome::Registered(outcome) = outcome else {
        panic!("首次登记不该报「已登记过」");
    };
    assert_eq!(outcome.version, 1);
    assert!(!outcome.created_new_version, "登记不是新版本");

    let row = store
        .get_resource_by_id(&outcome.resource_id)
        .await
        .expect("行应读得回来");
    assert_eq!(row.analytics_table.as_deref(), Some("orders_wide"));
    assert_eq!(
        crate::ArchiveKind::resolve(&row.kind, row.analytics_table.as_deref()),
        crate::ArchiveKind::AnalyticsTable
    );

    cleanup(dir);
}

/// 同一张表再登记一次：不造第二条、不涨版本，交回原来那条（口径：定位而非重复登记）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registering_the_same_table_twice_returns_the_same_asset() {
    let (store, dir) = create_test_store().await;

    let first = register_existing(&store, request("订单宽表", "orders_wide"))
        .await
        .expect("首次登记");
    let first_id = first.outcome().resource_id.clone();

    let again = register_existing(&store, request("换个名字", "orders_wide"))
        .await
        .expect("再登记");
    let ExistingTableOutcome::AlreadyCurrent(again) = &again else {
        panic!("同一张表不该被登记第二次");
    };
    assert_eq!(again.resource_id, first_id, "交回同一条资产");
    assert_eq!(again.name, "订单宽表", "名字以首次登记为准，不被后来者覆盖");
    assert_eq!(again.version, 1, "不涨版本");
    assert_eq!(
        store
            .list_alive_resources()
            .await
            .expect("清单")
            .iter()
            .filter(|row| row.analytics_table.as_deref() == Some("orders_wide"))
            .count(),
        1,
        "库里只留一条指向这张表的资产"
    );

    cleanup(dir);
}
