//! 根级用例分组。

use super::*;

#[tokio::test]
async fn t006_folder_create_list() {
    let (store, dir) = create_test_store().await;
    let folder = store
        .create_folder(CreateFolderRequest {
            name: "my_folder".to_string(),
            scope: "project".to_string(),
            parent_folder_id: None,
            color: None,
            icon: None,
        })
        .await
        .expect("create folder");
    assert_eq!(folder.name, "my_folder");

    let folders = store.list_folders(None, None).await.expect("list folders");
    assert_eq!(folders.len(), 1);
    cleanup(dir);
}

#[tokio::test]
async fn t007_folder_resource_link() {
    let (store, dir) = create_test_store().await;
    let resource = store
        .insert_archive(NewArchiveInput {
            resource_type: "table".to_string(),
            name: "linked".to_string(),
            alias: None,
            kind: ArchiveKind::Analysis,
            content_hash: "aabbccdd00112233".to_string(),
            definition_sql: None,
            row_count: None,
            column_count: None,
            file_rel_path: "analysis/linked.sql".to_string(),
            payload_mode: crate::model::PayloadMode::Copy,
            source_path: None,
            file_size: None,
            binding: ArchiveBinding::default(),
            scope: "project".to_string(),
        })
        .await
        .expect("create archive");
    let folder = store
        .create_folder(CreateFolderRequest {
            name: "f1".to_string(),
            scope: "project".to_string(),
            parent_folder_id: None,
            color: None,
            icon: None,
        })
        .await
        .expect("create folder");

    store
        .add_resource_to_folder(&resource.id, &folder.id)
        .await
        .expect("link");

    let memberships = store.folders_by_resource().await.expect("memberships");
    assert_eq!(memberships.get(&resource.id), Some(&folder.id));

    store
        .remove_resource_from_folder(&resource.id, &folder.id)
        .await
        .expect("unlink");

    let after = store.folders_by_resource().await.expect("memberships");
    assert!(
        !after.contains_key(&resource.id),
        "移除后资源不应再属于任何分组"
    );
    cleanup(dir);
}

/// 分组（单层）：建 / 改名 / 删（成员回到未分组）/ **移动语义**（一个资源只在一个分组）。
#[tokio::test]
async fn t018_single_level_folders_move_and_clear_links() {
    let (store, dir) = create_test_store().await;
    let resource = store
        .insert_archive(NewArchiveInput {
            resource_type: "table".to_string(),
            name: "grouped".to_string(),
            alias: None,
            kind: ArchiveKind::Analysis,
            content_hash: "aabbccdd00112233".to_string(),
            definition_sql: None,
            row_count: None,
            column_count: None,
            file_rel_path: "analysis/grouped.sql".to_string(),
            payload_mode: crate::model::PayloadMode::Copy,
            source_path: None,
            file_size: None,
            binding: ArchiveBinding::default(),
            scope: "project".to_string(),
        })
        .await
        .expect("create archive");
    let folder = store
        .create_folder(CreateFolderRequest {
            name: "月报".to_string(),
            scope: "project".to_string(),
            parent_folder_id: None,
            color: None,
            icon: None,
        })
        .await
        .expect("create folder");

    // 同名（未删）拒绝；子分组被拒（单层分组在类型上就不存在）。
    let error = store
        .create_folder(CreateFolderRequest {
            name: "月报".to_string(),
            scope: "project".to_string(),
            parent_folder_id: None,
            color: None,
            icon: None,
        })
        .await
        .expect_err("同名应被拒");
    assert!(error.to_string().contains("已经有同名分组"), "{error}");
    assert!(
        store
            .create_folder(CreateFolderRequest {
                name: "子分组".to_string(),
                scope: "project".to_string(),
                parent_folder_id: Some(folder.id.clone()),
                color: None,
                icon: None,
            })
            .await
            .is_err()
    );

    // 改名：幂等 + 同名拒绝。
    assert_eq!(
        store
            .rename_folder(&folder.id, "月报")
            .await
            .expect("幂等")
            .name,
        "月报"
    );
    let renamed = store
        .rename_folder(&folder.id, "月度报表")
        .await
        .expect("rename");
    assert_eq!(renamed.name, "月度报表");

    // 移动语义：加进 A 再加进 B ⇒ 只属于 B（面板分区才不会把同一行画两遍）。
    let second = store
        .create_folder(CreateFolderRequest {
            name: "周报".to_string(),
            scope: "project".to_string(),
            parent_folder_id: None,
            color: None,
            icon: None,
        })
        .await
        .expect("second folder");
    store
        .add_resource_to_folder(&resource.id, &folder.id)
        .await
        .expect("add to first");
    store
        .add_resource_to_folder(&resource.id, &second.id)
        .await
        .expect("move to second");
    let memberships = store.folders_by_resource().await.expect("memberships");
    assert_eq!(memberships.get(&resource.id), Some(&second.id));
    assert_ne!(
        memberships.get(&resource.id),
        Some(&folder.id),
        "旧分组的成员列表里也不该再有它"
    );

    // 移回未分组：关联清掉，两边都不再包含它。
    store
        .clear_resource_folder(&resource.id)
        .await
        .expect("clear");
    assert!(
        store
            .folders_by_resource()
            .await
            .expect("memberships")
            .is_empty()
    );
    store
        .add_resource_to_folder(&resource.id, &second.id)
        .await
        .expect("add back");

    // 删除分组：成员回到未分组（存档不能被分组连坐），关联一并清掉。
    let freed = store
        .delete_folder(&second.id)
        .await
        .expect("delete folder");
    assert_eq!(freed, 1, "删除要报告有多少条回到未分组");
    assert!(
        store
            .folders_by_resource()
            .await
            .expect("memberships")
            .is_empty()
    );
    assert!(
        store.get_resource_by_id(&resource.id).await.is_ok(),
        "删分组不删存档"
    );
    assert!(
        store.delete_folder(&second.id).await.is_err(),
        "重复删除应报错"
    );
    assert_eq!(store.list_folders(None, None).await.expect("list").len(), 1);
    cleanup(dir);
}
