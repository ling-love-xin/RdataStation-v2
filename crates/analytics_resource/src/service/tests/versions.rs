//! 服务层用例分组。

use super::*;

#[tokio::test]
async fn t120_undo_drops_the_row_and_restores_draft() {
    let (service, dir) = test_service().await;
    let mut events = service.subscribe();
    let draft = write_draft(&dir, "undo.sql", b"select 1;").await;
    let outcome = service
        .archive(archive_req(&draft, "reports/undo.sql", None))
        .await
        .expect("archive");
    assert!(!draft.exists(), "归档消费草稿");

    let undo = ArchiveUndo {
        resource_id: outcome.resource_id.clone(),
        name: "dau_report".to_string(),
        source_path: draft.clone(),
    };
    service.undo_archive(&undo).await.expect("undo");

    // 撤销 = 把本体送回原草稿位置 + 删登记行；立即反悔窗口内草稿应回来。
    assert!(draft.is_file(), "撤销应把本体送回原草稿位置");
    assert_eq!(
        fs::read(&draft).await.expect("read draft"),
        b"select 1;",
        "送回的是归档时那一份内容"
    );
    assert!(
        !service
            .payload()
            .resolve("reports/undo.sql")
            .expect("resolve")
            .exists(),
        "副本不再留在 resources/"
    );
    assert!(
        service
            .store()
            .get_resource_by_id(&outcome.resource_id)
            .await
            .is_err(),
        "登记行已硬删除"
    );
    // 发/收两端同批：先 Archived，再 Undone。
    let first = events.recv().await.expect("archived 事件");
    assert_eq!(first.reason, ChangeReason::Archived);
    let second = events.recv().await.expect("undone 事件");
    assert_eq!(second.reason, ChangeReason::Undone);

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t121_undo_refuses_when_rearchived_but_allows_with_draft_in_place() {
    let (service, dir) = test_service().await;
    let draft = write_draft(&dir, "occupied.sql", b"select 1;").await;
    let outcome = service
        .archive(archive_req(&draft, "occupied.sql", None))
        .await
        .expect("archive");
    let undo = ArchiveUndo {
        resource_id: outcome.resource_id.clone(),
        name: "dau_report".to_string(),
        source_path: draft.clone(),
    };

    // ① 撤销窗口内用户在原件位置又放了一个文件：不覆盖它，只删资产库本体。
    fs::write(&draft, b"other").await.expect("write again");
    service
        .undo_archive(&undo)
        .await
        .expect("原位已有文件不阻挡撤销");
    assert!(draft.is_file(), "用户新建的原位文件原样保留");
    assert!(
        !service
            .payload()
            .resolve("occupied.sql")
            .expect("resolve")
            .exists(),
        "副本已被撤销删掉"
    );

    // ② 已经再归档过（v2）：撤销窗口已过（否则会连带丢掉新内容）。
    let draft2 = write_draft(&dir, "occupied2.sql", b"select 2;").await;
    let re_archived = service
        .archive(archive_req(&draft2, "occupied2.sql", None))
        .await
        .expect("archive");
    // 先构造“v2 同档”场景：对同一资源再归档一次（版本 +1）。
    fs::write(&draft2, b"select 3;").await.expect("rewrite");
    service
        .archive(archive_req(
            &draft2,
            "occupied2.sql",
            Some(&re_archived.resource_id),
        ))
        .await
        .expect("re-archive to v2");
    let undo_v2 = ArchiveUndo {
        resource_id: re_archived.resource_id.clone(),
        name: "dau_report".to_string(),
        source_path: draft2.clone(),
    };
    assert!(
        service.undo_archive(&undo_v2).await.is_err(),
        "已有新版本不给撤销"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t122_restore_version_brings_old_content_forward() {
    let (service, dir) = test_service().await;
    let mut events = service.subscribe();
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let first = service
        .archive(archive_req(&draft, "dau.sql", None))
        .await
        .expect("archive");
    let _ = events.recv().await.expect("archived event");

    // v2：内容变了 → v1 进历史（版本行 + manifest）。
    let second_draft = write_draft(&dir, "dau2.sql", b"select 2;").await;
    let second = service
        .archive(archive_req(
            &second_draft,
            "dau.sql",
            Some(&first.resource_id),
        ))
        .await
        .expect("re-archive");
    assert_eq!(second.version, 2);
    let _ = events.recv().await.expect("updated event");

    // 从历史取回 v1（不改本体）：内容是 v1 的字节，且工作副本可写。
    let copy = dir.join("dau（v1 工作副本）.sql");
    let checked = service
        .checkout_version(&first.resource_id, 1, &copy)
        .await
        .expect("checkout version");
    assert_eq!(checked.version, 1);
    assert_eq!(fs::read(&copy).await.expect("read copy"), b"select 1;");
    assert!(!service.payload().is_readonly(&copy), "工作副本不应只读");
    let _ = events.recv().await.expect("checked out event");

    // 还原到 v1：生成 v3（不覆盖历史），本体内容回到 v1 的字节。
    let restored = service
        .restore_version(&first.resource_id, 1)
        .await
        .expect("restore");
    assert!(restored.created_new_version);
    assert_eq!(restored.version, 3, "还原 = 生成新版本，不是原地回滚");
    let payload = service.payload().resources_dir().join("dau.sql");
    assert_eq!(fs::read(&payload).await.expect("read"), b"select 1;");
    assert!(
        service.payload().is_readonly(&payload),
        "还原后的本体仍只读"
    );

    // 体积也回到 v1 的字节数（9 而不再是 v2 的）：「大小」一列不能停在还原前的值上。
    let row = service
        .store()
        .get_resource_by_id(&first.resource_id)
        .await
        .expect("row");
    assert_eq!(row.file_size, Some(9), "体积随本体一起回到历史值");

    // 历史：v1、v2 都有行（写前快照语义），且每版 manifest 都还在（还原不消耗 manifest）。
    let versions = service
        .store()
        .get_resource_versions(&first.resource_id)
        .await
        .expect("versions");
    let numbers: Vec<i32> = versions.iter().map(|v| v.version).collect();
    assert_eq!(numbers, vec![2, 1]);
    assert_eq!(
        service
            .store()
            .list_file_asset_versions(&first.resource_id)
            .await
            .expect("file versions")
            .iter()
            .map(|file| file.version)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );

    assert_eq!(
        events.recv().await.expect("restored event").reason,
        ChangeReason::Restored
    );

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t123_restore_version_requires_manifest_and_skips_identical_content() {
    let (service, dir) = test_service().await;
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let first = service
        .archive(archive_req(&draft, "dau.sql", None))
        .await
        .expect("archive");
    let second_draft = write_draft(&dir, "dau2.sql", b"select 2;").await;
    service
        .archive(archive_req(
            &second_draft,
            "dau.sql",
            Some(&first.resource_id),
        ))
        .await
        .expect("re-archive");

    // 不存在的版本没有 manifest：拒绝并说清原因，不动任何状态。
    let error = service
        .restore_version(&first.resource_id, 99)
        .await
        .expect_err("no manifest");
    assert!(
        error.to_string().contains("没有 manifest"),
        "原因要指向 manifest 不在：{error}"
    );

    // 内容与当前一致的版本：幂等（不为“还原到自己”造一个无意义的新版本）。
    let outcome = service
        .restore_version(&first.resource_id, 2)
        .await
        .expect("idempotent restore");
    assert!(!outcome.created_new_version);
    assert_eq!(outcome.version, 2, "指纹未变不得递增版本");

    let _ = fs::remove_dir_all(&dir).await;
}

/// 任意两版都能对比：跳过中间版本时不会偷偷拿当前本体去凑新侧。
///
/// 造 v1→v2→v3 三版，分别比 v1↔v3、v1↔v2、v3↔v1（反着点），检查两侧版本号、
/// patch 头与正文都指向选中的那两版。
#[tokio::test]
async fn t124_diff_compares_any_two_manifest_versions() {
    use crate::jobs::version::{VersionDiffJob, load_version_diff};
    use engine::persistence::ProjectDatabaseManager;

    // 用生产路径 `.RSmeta/project.db` 建测试项目：`load_version_diff` 走 `open_service`，
    // 它按项目根重新打开同一个库；若把库放在临时目录根部，作业侧会读到一个空库。
    let dir = crate::tests::temp_project_dir("archive-diff");
    tokio::fs::create_dir_all(dir.join(".RSmeta"))
        .await
        .expect("create .RSmeta");
    let manager = ProjectDatabaseManager::open(&dir, 2)
        .await
        .expect("open project db");
    let store = AnalyticsResourceStore::new_for_test(manager.sqlite_pool(), dir.join("global.db"));
    let service = ArchiveService::new(&dir, store);

    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let first = service
        .archive(archive_req(&draft, "dau.sql", None))
        .await
        .expect("archive v1");
    for (index, content) in [b"select 2;", b"select 3;"].into_iter().enumerate() {
        let draft = write_draft(&dir, &format!("dau{}.sql", index + 2), content).await;
        service
            .archive(archive_req(&draft, "dau.sql", Some(&first.resource_id)))
            .await
            .expect("re-archive");
    }

    let diff = |version, against| VersionDiffJob {
        project_root: dir.clone(),
        resource_id: first.resource_id.clone(),
        version,
        against,
    };

    // v1 ↔ v3：新侧是当前本体，但旧侧必须是 v1，不是 v2。
    let v1_v3 = load_version_diff(&diff(3, Some(1))).await.expect("v1↔v3");
    assert_eq!((v1_v3.seed.old_version, v1_v3.seed.new_version), (1, 3));
    assert!(v1_v3.seed.patch.contains("-select 1;"));
    assert!(v1_v3.seed.patch.contains("+select 3;"));
    assert!(!v1_v3.seed.patch.contains("select 2;"), "中间版本不该混进来");

    // v1 ↔ v2：两侧都走 manifest（新侧不是当前版本）。
    let v1_v2 = load_version_diff(&diff(2, Some(1))).await.expect("v1↔v2");
    assert_eq!((v1_v2.seed.old_version, v1_v2.seed.new_version), (1, 2));
    assert!(v1_v2.seed.patch.contains("+select 2;"));
    assert!(!v1_v2.seed.patch.contains("select 3;"), "当前版本不该混进来");

    // 反着点（先点 v3 再拿 v1 当基准）也排成 v1 → v3。
    let reversed = load_version_diff(&diff(1, Some(3))).await.expect("v3↔v1");
    assert_eq!((reversed.seed.old_version, reversed.seed.new_version), (1, 3));

    // Windows 上库还开着就删不掉目录；先放掉 service / manager，再清临时项目。
    drop(service);
    drop(manager);
    let _ = fs::remove_dir_all(&dir).await;
}

/// 表型资产的「回退」：没有副本文件，回退 = 把本体表名指回旧版那张表。
///
/// 钉住四件事：不要求副本、生成新版本且不覆盖历史、回退可以再退回去、重复退到
/// 同一版是幂等的（不造无意义版本）。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn table_asset_restore_points_the_asset_back_to_the_old_table() {
    use crate::table_asset::{TableAssetRequest, register};

    let (service, dir) = test_service().await;
    let request = |table: &str| TableAssetRequest {
        name: "结果 3".to_string(),
        analytics_table: table.to_string(),
        row_count: Some(120),
        column_count: Some(6),
    };
    let first = register(service.store(), request("res_3_001"))
        .await
        .expect("首次登记");
    let second = register(service.store(), request("res_3_002"))
        .await
        .expect("同名再升级");
    assert_eq!(second.version, 2);

    // v1 → 本体回到 res_3_001，版本接续到 v3（历史 v1/v2 一条不动）。
    let outcome = service
        .restore_version(&first.resource_id, 1)
        .await
        .expect("表型回退不该要求副本");
    assert_eq!(outcome.version, 3);
    assert!(outcome.created_new_version, "回退是生成新版本");
    let row = service
        .store()
        .get_resource_by_id(&first.resource_id)
        .await
        .expect("行应读得回来");
    assert_eq!(row.analytics_table.as_deref(), Some("res_3_001"), "指向 v1 那张表");
    assert_eq!(row.version, 3);
    assert_eq!(
        crate::ArchiveKind::resolve(&row.kind, row.analytics_table.as_deref()),
        crate::ArchiveKind::AnalyticsTable,
        "回退不改变档型"
    );

    // 快照表是“写前快照”：v1（首次升级前留的）与 v2（回退前留的），当前 v3 在资源行上。
    let versions = service
        .store()
        .get_resource_versions(&first.resource_id)
        .await
        .expect("版本快照");
    assert_eq!(versions.len(), 2, "v1 / v2 两条写前快照");
    let v2 = versions
        .iter()
        .find(|version| version.version == 2)
        .expect("v2 快照");
    assert_eq!(
        crate::table_asset::snapshot_table(&v2.snapshot).as_deref(),
        Some("res_3_002"),
        "v2 快照必须是整行（能解析出当时指向的表名）"
    );

    // 再退回 v2：本体又指回 res_3_002，v4。
    let back = service
        .restore_version(&first.resource_id, 2)
        .await
        .expect("回退回去");
    assert_eq!(back.version, 4);
    let row = service
        .store()
        .get_resource_by_id(&first.resource_id)
        .await
        .expect("行应读得回来");
    assert_eq!(row.analytics_table.as_deref(), Some("res_3_002"));

    // 幂等：已经指着 v2 那张表，再退 v2 不多造版本。
    let same = service
        .restore_version(&first.resource_id, 2)
        .await
        .expect("幂等回退");
    assert!(!same.created_new_version);
    assert_eq!(same.version, 4, "指向没变不得递增版本");

    let _ = fs::remove_dir_all(&dir).await;
}
