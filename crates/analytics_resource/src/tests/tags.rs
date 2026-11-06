//! 根级用例分组。

use super::*;

#[tokio::test]
async fn t008_tag_create_and_list() {
    let (store, dir) = create_test_store().await;
    let tag = store
        .create_tag(CreateTagRequest {
            name: "important".to_string(),
            scope: "project".to_string(),
            color: None,
            icon: None,
        })
        .await
        .expect("create tag");
    assert_eq!(tag.name, "important");

    let tags = store.list_tags(None).await.expect("list tags");
    assert_eq!(tags.len(), 1);
    cleanup(dir);
}

#[tokio::test]
async fn t009_tag_resource_bidirectional() {
    let (store, dir) = create_test_store().await;
    let resource = store
        .insert_archive(NewArchiveInput {
            resource_type: "table".to_string(),
            name: "tagged".to_string(),
            alias: None,
            kind: ArchiveKind::Analysis,
            content_hash: "aabbccdd00112233".to_string(),
            definition_sql: None,
            row_count: None,
            column_count: None,
            file_rel_path: "analysis/tagged.sql".to_string(),
            payload_mode: crate::model::PayloadMode::Copy,
            source_path: None,
            file_size: None,
            binding: ArchiveBinding::default(),
            scope: "project".to_string(),
        })
        .await
        .expect("create archive");
    let tag = store
        .create_tag(CreateTagRequest {
            name: "urgent".to_string(),
            scope: "project".to_string(),
            color: Some("#f00".to_string()),
            icon: None,
        })
        .await
        .expect("create tag");

    store
        .add_tag_to_resource(&resource.id, &tag.id)
        .await
        .expect("tag");

    let resource_tags = store
        .get_tags_for_resource(&resource.id)
        .await
        .expect("get tags");
    assert_eq!(resource_tags.len(), 1);

    let tagged_resources = store
        .get_resources_by_tag(&tag.id)
        .await
        .expect("get resources");
    assert_eq!(tagged_resources.len(), 1);
    cleanup(dir);
}

/// 标签的新建 / 改名 / 删除（P2.1 补 v1 缺失的两项）：同名（未删）一律拒，
/// 删除必须连关联一起清（否则“重建同名标签”会把旧归属带回来）。
#[tokio::test]
async fn t017_tag_rename_and_delete_keep_names_and_links_clean() {
    let (store, dir) = create_test_store().await;
    let resource = store
        .insert_archive(NewArchiveInput {
            resource_type: "table".to_string(),
            name: "tagged".to_string(),
            alias: None,
            kind: ArchiveKind::Analysis,
            content_hash: "aabbccdd00112233".to_string(),
            definition_sql: None,
            row_count: None,
            column_count: None,
            file_rel_path: "analysis/tagged.sql".to_string(),
            payload_mode: crate::model::PayloadMode::Copy,
            source_path: None,
            file_size: None,
            binding: ArchiveBinding::default(),
            scope: "project".to_string(),
        })
        .await
        .expect("create archive");
    let tag = store
        .create_tag(CreateTagRequest {
            name: "重要".to_string(),
            scope: "project".to_string(),
            color: None,
            icon: None,
        })
        .await
        .expect("create tag");

    // 同名（未删）拒绝：错误要能读，而不是 SQLite 的英文原话。
    let error = store
        .create_tag(CreateTagRequest {
            name: "重要".to_string(),
            scope: "project".to_string(),
            color: None,
            icon: None,
        })
        .await
        .expect_err("同名应被拒");
    assert!(error.to_string().contains("已经有同名标签"), "{error}");
    // 空名也拒（没有无名标签）。
    assert!(
        store
            .create_tag(CreateTagRequest {
                name: "   ".to_string(),
                scope: "project".to_string(),
                color: None,
                icon: None,
            })
            .await
            .is_err()
    );

    // 改名：幂等（改成自己）与同名拒绝两条都走一遍。
    let renamed = store.rename_tag(&tag.id, "重要").await.expect("幂等改名");
    assert_eq!(renamed.name, "重要");
    store
        .create_tag(CreateTagRequest {
            name: "待办".to_string(),
            scope: "project".to_string(),
            color: None,
            icon: None,
        })
        .await
        .expect("second tag");
    let error = store
        .rename_tag(&tag.id, "待办")
        .await
        .expect_err("改名撞名应被拒");
    assert!(error.to_string().contains("已经有同名标签"), "{error}");
    let renamed = store.rename_tag(&tag.id, "很重要").await.expect("rename");
    assert_eq!(renamed.name, "很重要");

    // 批量查询 + 用量计数（筛选菜单与详情面板各用一份）。
    store
        .add_tag_to_resource(&resource.id, &tag.id)
        .await
        .expect("tag");
    let by_resource = store.tags_by_resource().await.expect("tags by resource");
    assert_eq!(by_resource.get(&resource.id).map(Vec::len), Some(1));
    assert_eq!(
        by_resource[&resource.id][0].name, "很重要",
        "同一资源的标签按名字升序且已改名"
    );
    let counts = store.tag_usage_counts().await.expect("counts");
    assert_eq!(counts.get(&tag.id).copied(), Some(1));

    // 删除：只软删定义行（规格 D2），标签从活跃列表消失（查询侧过滤软删行）；
    // 关联保留在库里，但同名重建的新 id 不会把它带回。
    store.delete_tag(&tag.id).await.expect("delete tag");
    assert!(
        store
            .get_tags_for_resource(&resource.id)
            .await
            .expect("get tags")
            .is_empty()
    );
    assert!(
        store
            .tags_by_resource()
            .await
            .expect("by resource")
            .is_empty()
    );
    assert_eq!(store.list_tags(None).await.expect("list").len(), 1);
    assert!(store.delete_tag(&tag.id).await.is_err(), "重复删除应报错");
    store
        .create_tag(CreateTagRequest {
            name: "很重要".to_string(),
            scope: "project".to_string(),
            color: None,
            icon: None,
        })
        .await
        .expect("删掉后可以重建同名");
    cleanup(dir);
}
