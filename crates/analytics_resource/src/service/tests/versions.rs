//! 服务层用例分组。

use super::*;

#[tokio::test]
async fn t120_undo_removes_copy_and_drops_the_row_keeps_draft() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let mut events = service.subscribe();
    let draft = write_draft(&dir, "undo.sql", b"select 1;").await;
    let outcome = service
        .archive(archive_req(&draft, "reports/undo.sql", None))
        .await
        .expect("archive");
    assert!(draft.exists(), "copy 语义：归档后草稿仍在（工作副本）");

    let undo = ArchiveUndo {
        resource_id: outcome.resource_id.clone(),
        name: "dau_report".to_string(),
        source_path: draft.clone(),
    };
    service.undo_archive(&undo).await.expect("undo");

    // copy 语义：撤销 = 删掉 `resources/` 副本 + 删登记行；草稿本来就在，不动它。
    assert!(draft.is_file(), "草稿（工作副本）不受撤销影响");
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
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
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

    // ① copy 语义：草稿在“原位”是常态（它是工作副本），撤销**不受它阻挡**——
    // 撤销删的是 `resources/` 副本与登记行，不碰草稿。
    fs::write(&draft, b"other").await.expect("write again");
    service
        .undo_archive(&undo)
        .await
        .expect("copy 语义下原草稿在位不阻挡撤销");
    assert!(draft.is_file(), "草稿原样保留");
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
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let mut events = service.subscribe();
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let first = service
        .archive(archive_req(&draft, "dau.sql", None))
        .await
        .expect("archive");
    let _ = events.recv().await.expect("archived event");

    // v2：内容变了 → v1 进历史（版本行 + 内容副本）。
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

    // 历史：v1、v2 都有行（写前快照语义），且两份副本都还在（还原不消耗副本）。
    let versions = service
        .store()
        .get_resource_versions(&first.resource_id)
        .await
        .expect("versions");
    let numbers: Vec<i32> = versions.iter().map(|v| v.version).collect();
    assert_eq!(numbers, vec![2, 1]);
    assert_eq!(
        service
            .payload()
            .version_copies(&first.resource_id)
            .await
            .expect("copies"),
        vec![2, 1]
    );

    assert_eq!(
        events.recv().await.expect("restored event").reason,
        ChangeReason::Restored
    );

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t123_restore_version_requires_copy_and_skips_identical_content() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
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

    // 副本被裁掉（或手工清理）：拒绝并说清原因，不动任何状态。
    assert!(
        service
            .payload()
            .delete_version_copy(&first.resource_id, 1)
            .await
            .expect("delete copy")
    );
    let error = service
        .restore_version(&first.resource_id, 1)
        .await
        .expect_err("no copy");
    assert!(
        error.to_string().contains("没有内容副本"),
        "原因要指向副本不在：{error}"
    );

    // 内容与当前一致的版本：幂等（不为“还原到自己”造一个无意义的新版本）。
    service
        .payload()
        .store_version_copy(&first.resource_id, 2, "dau.sql")
        .await
        .expect("copy v2");
    let outcome = service
        .restore_version(&first.resource_id, 2)
        .await
        .expect("idempotent restore");
    assert!(!outcome.created_new_version);
    assert_eq!(outcome.version, 2, "指纹未变不得递增版本");

    let _ = fs::remove_dir_all(&dir).await;
}
