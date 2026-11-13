//! 时间线与事务收口的回归用例（P0-1 / 迁移 027）。

use super::*;
use crate::model::TimelineEventKind;

#[tokio::test]
async fn t140_first_archive_writes_row_and_event_in_one_shot() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
    let outcome = service
        .archive(archive_req(&draft, "reports/dau.sql", None))
        .await
        .expect("archive");

    let events = service
        .store()
        .get_resource_events(&outcome.resource_id)
        .await
        .expect("events");
    assert_eq!(events.len(), 1, "首次归档恰好一条履历：{events:?}");
    assert_eq!(events[0].kind, TimelineEventKind::Archived);
    assert_eq!(events[0].version, Some(1));
    assert_eq!(
        events[0].content_hash.as_deref(),
        Some(outcome.content_hash.as_str())
    );

    // 登记行确实在同一批写入。
    let row = service
        .store()
        .get_resource_by_id(&outcome.resource_id)
        .await
        .expect("row");
    assert_eq!(row.version, 1);

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t141_registry_rolls_back_when_event_write_fails() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let draft = write_draft(&dir, "dau.sql", b"select 1;").await;

    // 故障注入：拆掉事件表，让事务里的第二条 INSERT 失败。
    {
        let pool = service.store().pool().clone();
        let conn = pool.acquire().await.expect("acquire");
        conn.inner()
            .expect("inner")
            .execute_batch("DROP TABLE analytics_resource_events")
            .expect("drop events table");
    }

    let result = service
        .archive(archive_req(&draft, "reports/dau.sql", None))
        .await;
    assert!(result.is_err(), "履历写失败时归档必须整体失败");

    // 登记行不能留下半截状态——这正是 BEGIN IMMEDIATE 要挡的。
    let leftover = service
        .store()
        .find_archive_by_rel_path("reports/dau.sql")
        .await
        .expect("query");
    assert!(leftover.is_none(), "登记行应随事务一起回滚");
    assert!(
        !service
            .payload()
            .resources_dir()
            .join("reports/dau.sql")
            .exists(),
        "本体副本也应被服务层回滚"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t142_rearchive_commits_index_and_payload_consistently() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let first = write_draft(&dir, "v1.sql", b"select 1;").await;
    let outcome = service
        .archive(archive_req(&first, "reports/dau.sql", None))
        .await
        .expect("first archive");

    let second = write_draft(&dir, "v2.sql", b"select 2; -- longer").await;
    let updated = service
        .archive(archive_req(
            &second,
            "reports/dau.sql",
            Some(&outcome.resource_id),
        ))
        .await
        .expect("re-archive");
    assert!(updated.created_new_version);
    assert_eq!(updated.version, 2);

    // 索引与本体必须指向同一份内容（旧实现在「覆盖本体 → 更新索引」之间可能错位）。
    let row = service
        .store()
        .get_resource_by_id(&outcome.resource_id)
        .await
        .expect("row");
    let payload_path = service.payload().resources_dir().join("reports/dau.sql");
    let actual_hash = service
        .payload()
        .content_hash(&payload_path)
        .await
        .expect("hash");
    assert_eq!(
        row.content_hash.as_deref(),
        Some(actual_hash.as_str()),
        "索引指纹必须等于本体实际内容"
    );
    assert_eq!(
        std::fs::read_to_string(&payload_path).expect("read"),
        "select 2; -- longer"
    );

    // 履历：归档 + 再归档两条，Updated 带新版本号与新指纹（事件与索引同事务）。
    let events = service
        .store()
        .get_resource_events(&outcome.resource_id)
        .await
        .expect("events");
    assert_eq!(events.len(), 2, "履历应恰好两条：{events:?}");
    let updated_event = events
        .iter()
        .find(|event| event.kind == TimelineEventKind::Updated)
        .expect("updated event");
    assert_eq!(updated_event.version, Some(2));
    assert_eq!(
        updated_event.content_hash.as_deref(),
        Some(actual_hash.as_str())
    );

    // 成功路径不能留下 .tmp 暂存文件。
    let staging = service.payload().resources_dir().join("reports/.staging");
    let leftover = staging.exists()
        && std::fs::read_dir(&staging)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
    assert!(!leftover, "成功路径不应留下 .tmp 暂存文件");

    let _ = fs::remove_dir_all(&dir).await;
}
