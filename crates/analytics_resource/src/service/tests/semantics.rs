//! 业务断言与效力的回归用例（迁移 028）。

use super::*;
use crate::model::{ArchiveAssertion, Lifecycle, TimelineEventKind};

async fn archived(service: &ArchiveService, dir: &std::path::Path) -> String {
    let draft = write_draft(dir, "dau.sql", b"select 1;").await;
    service
        .archive(archive_req(&draft, "reports/dau.sql", None))
        .await
        .expect("archive")
        .resource_id
}

fn assertion() -> ArchiveAssertion {
    ArchiveAssertion {
        question: Some("昨天的日活是多少".to_string()),
        metric_key: Some("dau".to_string()),
        metric_name: Some("日活".to_string()),
        definition: Some("自然日内去重登录用户，排除内部账号".to_string()),
        assumptions: Some(r#"{"exclude":"internal"}"#.to_string()),
        time_window: Some("2026Q3".to_string()),
        conclusion: Some("日活 12.4 万".to_string()),
        owner: Some("分析组".to_string()),
        confirmed_by: None,
        confirmed_at: None,
    }
}

#[tokio::test]
async fn t150_first_assertion_is_persisted_with_timeline_event() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let id = archived(&service, &dir).await;

    let hash = service
        .set_assertion(&id, assertion())
        .await
        .expect("set assertion");
    assert_eq!(hash.len(), 64, "口径指纹是 sha256 hex");

    let stored = service.assertion(&id).await.expect("read").expect("assertion");
    assert_eq!(stored.definition.as_deref(), Some("自然日内去重登录用户，排除内部账号"));
    assert_eq!(stored.semantic_hash(), hash);

    let events = service.store().get_resource_events(&id).await.expect("events");
    let semantic: Vec<_> = events
        .iter()
        .filter(|event| event.kind == TimelineEventKind::SemanticChanged)
        .collect();
    assert_eq!(semantic.len(), 1, "首次记录断言留一条履历");
    assert!(semantic[0].detail.as_deref().unwrap_or("").contains("业务断言已记录"));
    assert_eq!(semantic[0].content_hash.as_deref(), Some(hash.as_str()));

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t151_only_definition_changes_create_semantic_events() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let id = archived(&service, &dir).await;
    let original = assertion();
    let first_hash = service.set_assertion(&id, original.clone()).await.expect("first");

    // 只改描述性字段：口径没变，不该刷出一条"口径变更"。
    let mut reworded = original.clone();
    reworded.question = Some("换个问法".to_string());
    reworded.conclusion = Some("换个说法".to_string());
    let same_hash = service.set_assertion(&id, reworded).await.expect("reword");
    assert_eq!(first_hash, same_hash);
    let events = service.store().get_resource_events(&id).await.expect("events");
    assert_eq!(
        events.iter().filter(|e| e.kind == TimelineEventKind::SemanticChanged).count(),
        1,
        "描述性改动不产生口径变更事件"
    );

    // 真正改口径：指纹变化，时间线多一条。
    let mut changed = original;
    changed.definition = Some("滚动 24 小时内去重登录用户".to_string());
    let new_hash = service.set_assertion(&id, changed).await.expect("change");
    assert_ne!(first_hash, new_hash);
    let events = service.store().get_resource_events(&id).await.expect("events");
    let semantic: Vec<_> = events
        .iter()
        .filter(|event| event.kind == TimelineEventKind::SemanticChanged)
        .collect();
    assert_eq!(semantic.len(), 2);
    assert!(semantic[0].detail.as_deref().unwrap_or("").contains("口径变更"));

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t152_certification_records_lifecycle_and_event() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let id = archived(&service, &dir).await;
    assert_eq!(service.lifecycle(&id).await.expect("default"), Lifecycle::Draft);

    service
        .set_lifecycle(&id, Lifecycle::Certified, None, Some("评审通过"))
        .await
        .expect("certify");
    assert_eq!(service.lifecycle(&id).await.expect("read"), Lifecycle::Certified);

    let events = service.store().get_resource_events(&id).await.expect("events");
    let certified = events
        .iter()
        .find(|event| event.kind == TimelineEventKind::Certified)
        .expect("certified event");
    assert_eq!(certified.detail.as_deref(), Some("评审通过"));

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t153_empty_assertion_is_rejected() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let id = archived(&service, &dir).await;
    assert!(service.set_assertion(&id, ArchiveAssertion::default()).await.is_err());
    assert!(service.assertion(&id).await.expect("read").is_none());
    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t154_superseded_writes_ref_and_validation_is_deduplicated() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let id = archived(&service, &dir).await;

    service
        .set_lifecycle(&id, Lifecycle::Superseded, Some("ar_new"), Some("被新报表取代"))
        .await
        .expect("supersede");
    let refs = service.store().get_refs(&id).await.expect("refs");
    assert_eq!(refs.len(), 1, "被取代要落一条谱系");
    assert_eq!(refs[0].rel_type, "supersedes");
    assert_eq!(refs[0].from_id, id);
    assert_eq!(refs[0].to_id, "ar_new");

    // 主动校验是高频动作：同一条说明写两次，时间线只能留一条。
    let detail = "业务时间窗「2020Q3」已过期，请确认结论是否仍适用";
    service
        .store()
        .record_validation_if_absent(&id, 1, detail)
        .await
        .expect("validation 1");
    service
        .store()
        .record_validation_if_absent(&id, 1, detail)
        .await
        .expect("validation 2");
    let events = service.store().get_resource_events(&id).await.expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == TimelineEventKind::Validated)
            .count(),
        1
    );

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t155_dropping_back_to_draft_is_recorded() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let id = archived(&service, &dir).await;
    service
        .set_lifecycle(&id, Lifecycle::Certified, None, None)
        .await
        .expect("certify");
    service
        .set_lifecycle(&id, Lifecycle::Draft, None, None)
        .await
        .expect("back to draft");
    assert_eq!(service.lifecycle(&id).await.expect("read"), Lifecycle::Draft);
    let events = service.store().get_resource_events(&id).await.expect("events");
    assert!(
        events.iter().any(|event| event
            .detail
            .as_deref()
            .is_some_and(|text| text.contains("退回候选"))),
        "效力回退必须留痕：{events:?}"
    );
    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t156_clearing_expiry_keeps_manual_validation() {
    let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
    let id = archived(&service, &dir).await;
    service
        .store()
        .record_validation_if_absent(&id, 1, "业务时间窗「2020Q3」已过期")
        .await
        .expect("auto");
    service
        .store()
        .record_validation_if_absent(&id, 1, "人工核对：源表口径待确认")
        .await
        .expect("manual");
    service
        .store()
        .clear_window_validations(&id)
        .await
        .expect("clear");
    let events = service.store().get_resource_events(&id).await.expect("events");
    let details: Vec<&str> = events
        .iter()
        .filter_map(|event| event.detail.as_deref())
        .collect();
    assert!(!details.iter().any(|text| text.contains("业务时间窗")));
    assert!(details.iter().any(|text| text.contains("人工核对")));
    let _ = fs::remove_dir_all(&dir).await;
}
