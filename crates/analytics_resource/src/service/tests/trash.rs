//! 服务层用例分组。

use super::*;

use crate::model::ORIGIN_RESOURCES;
/// 移入回收站：本体离开 `resources/` 进入项目级回收站，索引行**软删**（列表与
/// “按路径查存活行”都看不到，但记录本身还在，还原才能恢复别名 / 标签 / 指纹）。
/// 永久删除与清空：都**只动本模块的**条目（回收站是共用的，删别人的东西不是清空）；
/// 登记行与它的标签 / 分组归属一并清掉（外键没有 `ON DELETE CASCADE`，不清就删不掉）。
#[tokio::test]
async fn t128_purge_and_empty_stay_within_our_origin() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let first_draft = write_draft(&dir, "a.sql", b"select 1;").await;
    let first = service
        .archive(archive_req(&first_draft, "a.sql", None))
        .await
        .expect("archive a");
    let second_draft = write_draft(&dir, "b.sql", b"select 2;").await;
    let second = service
        .archive(archive_req(&second_draft, "b.sql", None))
        .await
        .expect("archive b");

    // 给第一条挂标签与分组：删除时必须连关联一起清（外键无 CASCADE，项目库开了 foreign_keys）。
    let tag = service
        .store()
        .create_tag(crate::store::rows::CreateTagRequest {
            name: "important".to_string(),
            scope: "project".to_string(),
            color: None,
            icon: None,
        })
        .await
        .expect("tag");
    service
        .store()
        .add_tag_to_resource(&first.resource_id, &tag.id)
        .await
        .expect("link tag");

    let first_entry = service
        .move_to_trash(&[first.resource_id.clone()])
        .await
        .expect("trash a");
    let second_entry = service
        .move_to_trash(&[second.resource_id.clone()])
        .await
        .expect("trash b");

    // 别人的条目：永久删除必须被拒（只验证守卫，不真删）。
    let foreign_path = dir.join("draft.sql");
    fs::write(&foreign_path, b"select 3;").await.expect("write");
    let foreign = service
        .payload()
        .trash()
        .move_to_trash(&foreign_path, "scratchpad", "draft.sql")
        .await
        .expect("foreign entry");
    let error = service
        .purge_archive(&foreign.manifest.id)
        .await
        .expect_err("cross-module purge must fail");
    assert!(
        error.to_string().contains("scratchpad"),
        "错误要指明归属：{error}"
    );

    // 单条永久删除：本体与登记行都不在了。
    service
        .purge_archive(&first_entry[0].trash_id)
        .await
        .expect("purge a");
    assert!(
        service
            .store()
            .find_deleted_archive_by_rel_path("a.sql")
            .await
            .expect("find a")
            .is_none()
    );
    assert!(
        service
            .store()
            .get_resource_by_id(&first.resource_id)
            .await
            .is_err(),
        "登记行删了就该查不到（它已经不在回收站里等还原了）"
    );
    assert!(
        service
            .store()
            .get_tags_for_resource(&first.resource_id)
            .await
            .expect("tags")
            .is_empty()
    );

    // 清空：只删本模块的两条（含刚删过的），别人的原样留着。
    let purged = service.empty_trash().await.expect("empty");
    assert_eq!(purged, 1, "a 已单条删过，剩 b 一条");
    let left = service.payload().trash().list().await.expect("list");
    assert_eq!(left.len(), 1, "草稿箱的条目必须原样留着");
    assert_eq!(left[0].manifest.origin, "scratchpad");
    assert!(
        service
            .store()
            .find_deleted_archive_by_rel_path("b.sql")
            .await
            .expect("find b")
            .is_none()
    );
    assert!(second_entry.len() == 1);

    let _ = fs::remove_dir_all(&dir).await;
}

/// 由原相对路径还原（索引修复的「从回收站还原」）：命中就把本体搬回来并复活登记行；
/// 回收站里没有对应条目时**不静默**，把用户指向仅剩的出口（删记录）。
#[tokio::test]
async fn t129_restore_by_rel_path_finds_the_entry_or_says_why_not() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let draft = write_draft(&dir, "a.sql", b"select 1;").await;
    let archived = service
        .archive(archive_req(&draft, "reports/a.sql", None))
        .await
        .expect("archive");
    service
        .move_to_trash(&[archived.resource_id.clone()])
        .await
        .expect("trash");

    let revived = service
        .restore_archive_by_rel_path("reports/a.sql")
        .await
        .expect("restore by rel path");
    assert_eq!(revived.id, archived.resource_id);
    assert!(
        service
            .payload()
            .resources_dir()
            .join("reports")
            .join("a.sql")
            .is_file()
    );

    let error = service
        .restore_archive_by_rel_path("reports/a.sql")
        .await
        .expect_err("条目已经从回收站里出去了");
    assert!(
        error.to_string().contains("只能删掉这条登记记录"),
        "错误要指向仅剩的出口：{error}"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t124_move_to_trash_soft_deletes_row_and_keeps_payload_in_trash() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let mut events = service.subscribe();
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let archived = service
        .archive(archive_req(&draft, "reports/dau.sql", None))
        .await
        .expect("archive");
    let _ = events.recv().await.expect("archived event");

    let entries = service
        .move_to_trash(&[archived.resource_id.clone()])
        .await
        .expect("move to trash");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].resource_id, archived.resource_id);
    assert_eq!(entries[0].name, "dau_report");

    // 本体：离开 resources/；回收站条目带 origin 与原相对路径（还原要靠它回原位）。
    let payload = service
        .payload()
        .resources_dir()
        .join("reports")
        .join("dau.sql");
    assert!(
        !payload.exists(),
        "移入回收站是移动，本体不能留在 resources/"
    );
    let entry = service
        .payload()
        .trash()
        .get(&entries[0].trash_id)
        .await
        .expect("trash entry");
    assert_eq!(entry.manifest.origin, ORIGIN_RESOURCES);
    assert_eq!(entry.manifest.original_rel_path, "reports/dau.sql");

    // 索引：存活列表与按路径查找都看不见，但软删行仍在（deleted_at 非空）。
    assert!(
        service
            .store()
            .list_file_archives()
            .await
            .expect("list")
            .is_empty()
    );
    assert!(
        service
            .store()
            .find_archive_by_rel_path("reports/dau.sql")
            .await
            .expect("find")
            .is_none()
    );
    let row = service
        .store()
        .get_resource_by_id(&archived.resource_id)
        .await
        .expect("row");
    assert!(row.deleted_at.is_some(), "回收站里的行必须保留 deleted_at");

    assert_eq!(
        events.recv().await.expect("trashed event").reason,
        ChangeReason::Trashed
    );

    let _ = fs::remove_dir_all(&dir).await;
}

/// 从回收站还原：本体按原相对路径回原位、软删行复活；**跨模块条目一律拒绝**
/// （origin 校验——别的模块的本体不属于我们，误还原会把别人的数据搬到 resources/）。
#[tokio::test]
async fn t125_restore_from_trash_revives_row_and_rejects_foreign_origin() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let mut events = service.subscribe();
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let archived = service
        .archive(archive_req(&draft, "reports/dau.sql", None))
        .await
        .expect("archive");
    let _ = events.recv().await.expect("archived event");
    let trashed = service
        .move_to_trash(&[archived.resource_id.clone()])
        .await
        .expect("move to trash");
    let _ = events.recv().await.expect("trashed event");

    let revived = service
        .restore_archive_from_trash(&trashed[0].trash_id)
        .await
        .expect("restore");
    assert_eq!(revived.id, archived.resource_id);
    assert!(revived.deleted_at.is_none(), "还原必须清掉 deleted_at");
    assert_eq!(
        revived.file_rel_path.as_deref(),
        Some("reports/dau.sql"),
        "无同名冲突时登记行路径不应变"
    );
    assert!(
        service
            .payload()
            .resources_dir()
            .join("reports")
            .join("dau.sql")
            .is_file(),
        "本体应回到原相对路径"
    );
    assert_eq!(
        service
            .store()
            .list_file_archives()
            .await
            .expect("list")
            .len(),
        1,
        "还原后应重新出现在存活列表里"
    );
    assert_eq!(
        events.recv().await.expect("untrashed event").reason,
        ChangeReason::Untrashed
    );

    // 跨模块条目（模拟草稿箱的）：拒绝，且条目原样留在回收站里等它的主人。
    let foreign_path = dir.join("draft.sql");
    fs::write(&foreign_path, b"select 2;").await.expect("write");
    let foreign = service
        .payload()
        .trash()
        .move_to_trash(&foreign_path, "scratchpad", "draft.sql")
        .await
        .expect("foreign trash entry");
    let error = service
        .restore_archive_from_trash(&foreign.manifest.id)
        .await
        .expect_err("cross-module restore must fail");
    assert!(
        error.to_string().contains("scratchpad"),
        "错误要指明条目属于谁：{error}"
    );
    assert!(
        service
            .payload()
            .trash()
            .get(&foreign.manifest.id)
            .await
            .is_ok(),
        "被拒的条目必须留在回收站里"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

/// 两条不静默降级的边界：**同名不覆盖**（回收站层避让改名，登记行的本体路径
/// 跟着改），以及**本体缺失的行拒绝移入**（指向索引修复这个正规出口）。
#[tokio::test]
async fn t126_restore_avoids_name_conflict_and_trash_refuses_missing_payload() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let archived = service
        .archive(archive_req(&draft, "reports/dau.sql", None))
        .await
        .expect("archive");
    let trashed = service
        .move_to_trash(&[archived.resource_id.clone()])
        .await
        .expect("move to trash");

    // 原位被占用（模拟并存档 / 外部写入）：还原必须让路，不能覆盖。
    let occupied = service
        .payload()
        .resources_dir()
        .join("reports")
        .join("dau.sql");
    fs::write(&occupied, b"someone else").await.expect("occupy");

    let revived = service
        .restore_archive_from_trash(&trashed[0].trash_id)
        .await
        .expect("restore");
    assert_eq!(fs::read(&occupied).await.expect("read"), b"someone else");
    assert_eq!(
        revived.file_rel_path.as_deref(),
        Some("reports/dau_1.sql"),
        "避让改名后登记行必须跟着改，否则索引与本体又对不上"
    );
    assert!(
        service
            .payload()
            .resources_dir()
            .join("reports")
            .join("dau_1.sql")
            .is_file(),
        "避让后的本体应落在新路径"
    );

    // 本体缺失的行：拒绝移入（它的出口是索引修复，不是回收站）。
    let orphan_draft = write_draft(&dir, "orphan.sql", b"select 3;").await;
    let orphan = service
        .archive(archive_req(&orphan_draft, "orphan.sql", None))
        .await
        .expect("archive orphan");
    fs::remove_file(service.payload().resources_dir().join("orphan.sql"))
        .await
        .expect("remove payload behind the index");
    let error = service
        .move_to_trash(&[orphan.resource_id.clone()])
        .await
        .expect_err("missing payload must be refused");
    assert!(
        error.to_string().contains("本体不在"),
        "错误要指出本体缺失：{error}"
    );
    assert!(
        service
            .store()
            .get_resource_by_id(&orphan.resource_id)
            .await
            .expect("row")
            .deleted_at
            .is_none(),
        "被拒的行不能变成软删态"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

/// 批量移入回收站中途失败：**已进回收站的不回退**（见 `move_to_trash` 的取舍），
/// 但错误里必须说清“已移了几条”——否则用户只看到一句失败，列表却已经变了。
#[tokio::test]
async fn t127_batch_trash_reports_partial_progress() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let first_draft = write_draft(&dir, "a.sql", b"select 1;").await;
    let first = service
        .archive(archive_req(&first_draft, "a.sql", None))
        .await
        .expect("archive a");
    let second_draft = write_draft(&dir, "b.sql", b"select 2;").await;
    let second = service
        .archive(archive_req(&second_draft, "b.sql", None))
        .await
        .expect("archive b");
    fs::remove_file(service.payload().resources_dir().join("b.sql"))
        .await
        .expect("remove b payload behind the index");

    let error = service
        .move_to_trash(&[first.resource_id.clone(), second.resource_id.clone()])
        .await
        .expect_err("batch with a missing payload must fail");
    assert!(
        error.to_string().contains("已移入 1 项"),
        "错误要交代部分成功：{error}"
    );

    // 前一条真的进了回收站（本体走了、行软删），后一条一点没动。
    assert!(!service.payload().resources_dir().join("a.sql").exists());
    assert_eq!(
        service.payload().trash().list().await.expect("list").len(),
        1
    );
    assert!(
        service
            .store()
            .get_resource_by_id(&first.resource_id)
            .await
            .expect("a")
            .deleted_at
            .is_some()
    );
    assert!(
        service
            .store()
            .get_resource_by_id(&second.resource_id)
            .await
            .expect("b")
            .deleted_at
            .is_none(),
        "失败的那条不能被顺手软删"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

// ===== File 档 reference 模式（2026-10-13 裁决：两档都做、默认 copy）=====
