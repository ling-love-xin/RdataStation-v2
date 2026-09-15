#[cfg(test)]
mod tests {
    use super::super::*;
    use engine::persistence::ProjectSqlitePool;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    const MIGRATION_SQL: &str =
        include_str!("../../engine/migrations/project_meta/007_analytics_resources.sql");

    const ARCHIVE_MIGRATION_SQL: &str =
        include_str!("../../engine/migrations/project_meta/020_analytics_resource_archive.sql");

    async fn create_test_store() -> (AnalyticsResourceStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rds_test_{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("project.db");
        let pool = Arc::new(
            ProjectSqlitePool::new(db_path.clone(), 2)
                .await
                .expect("create pool"),
        );
        {
            let conn = pool.acquire().await.expect("acquire connection");
            conn.inner()
                .expect("get connection")
                .execute_batch(MIGRATION_SQL)
                .expect("run migration");
        }
        let store = AnalyticsResourceStore::new(pool);
        (store, dir)
    }

    fn cleanup(dir: PathBuf) {
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn t001_create_and_get_resource() {
        let (store, dir) = create_test_store().await;
        let req = CreateResourceRequest {
            resource_type: "table".to_string(),
            name: "test_table".to_string(),
            config: serde_json::json!({"key": "value"}),
            scope: "project".to_string(),
            alias: None,
            source_query: None,
            column_count: None,
            file_size: None,
            row_count: None,
            parent_resource_id: None,
        };
        let created = store.create_resource(req).await.expect("create resource");
        assert_eq!(created.name, "test_table");
        assert_eq!(created.version, 1);

        let fetched = store
            .get_resource_by_id(&created.id)
            .await
            .expect("get resource");
        assert_eq!(fetched.name, "test_table");
        cleanup(dir);
    }

    #[tokio::test]
    async fn t002_list_resources_filtered() {
        let (store, dir) = create_test_store().await;
        for i in 0..3 {
            store
                .create_resource(CreateResourceRequest {
                    resource_type: if i == 0 { "table" } else { "view" }.to_string(),
                    name: format!("res_{}", i),
                    config: serde_json::json!({}),
                    scope: "project".to_string(),
                    alias: None,
                    source_query: None,
                    column_count: None,
                    file_size: None,
                    row_count: None,
                    parent_resource_id: None,
                })
                .await
                .expect("create");
        }
        let all = store
            .list_resources(None, None, None)
            .await
            .expect("list all");
        assert_eq!(all.len(), 3);

        let tables = store
            .list_resources(None, Some("table"), None)
            .await
            .expect("filter type");
        assert_eq!(tables.len(), 1);
        cleanup(dir);
    }

    #[tokio::test]
    async fn t003_update_and_version() {
        let (store, dir) = create_test_store().await;
        let created = store
            .create_resource(CreateResourceRequest {
                resource_type: "table".to_string(),
                name: "v1".to_string(),
                config: serde_json::json!({}),
                scope: "project".to_string(),
                alias: None,
                source_query: None,
                column_count: None,
                file_size: None,
                row_count: None,
                parent_resource_id: None,
            })
            .await
            .expect("create");

        let updated = store
            .update_resource(
                &created.id,
                CreateResourceRequest {
                    resource_type: "table".to_string(),
                    name: "v2".to_string(),
                    config: serde_json::json!({}),
                    scope: "project".to_string(),
                    alias: None,
                    source_query: None,
                    column_count: None,
                    file_size: None,
                    row_count: None,
                    parent_resource_id: None,
                },
            )
            .await
            .expect("update");

        assert_eq!(updated.name, "v2");
        assert_eq!(updated.version, 2);
        cleanup(dir);
    }

    #[tokio::test]
    async fn t004_soft_delete_and_restore() {
        let (store, dir) = create_test_store().await;
        let created = store
            .create_resource(CreateResourceRequest {
                resource_type: "table".to_string(),
                name: "to_delete".to_string(),
                config: serde_json::json!({}),
                scope: "project".to_string(),
                alias: None,
                source_query: None,
                column_count: None,
                file_size: None,
                row_count: None,
                parent_resource_id: None,
            })
            .await
            .expect("create");

        store.delete_resource(&created.id).await.expect("delete");

        let remaining = store.list_resources(None, None, None).await.expect("list");
        assert_eq!(remaining.len(), 0);

        let recycle = store.get_recycle_items().await.expect("recycle");
        assert_eq!(recycle.len(), 1);

        let restored = store
            .restore_from_recycle(&recycle[0].id)
            .await
            .expect("restore");
        assert_eq!(restored.name, "to_delete");
        cleanup(dir);
    }

    #[tokio::test]
    async fn t005_resource_not_found() {
        let (store, dir) = create_test_store().await;
        let result = store.get_resource_by_id("nonexistent").await;
        assert!(result.is_err());
        cleanup(dir);
    }

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
            .create_resource(CreateResourceRequest {
                resource_type: "table".to_string(),
                name: "linked".to_string(),
                config: serde_json::json!({}),
                scope: "project".to_string(),
                alias: None,
                source_query: None,
                column_count: None,
                file_size: None,
                row_count: None,
                parent_resource_id: None,
            })
            .await
            .expect("create resource");
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

        let in_folder = store
            .list_resources(None, None, Some(&folder.id))
            .await
            .expect("list in folder");
        assert_eq!(in_folder.len(), 1);
        assert_eq!(in_folder[0].id, resource.id);

        store
            .remove_resource_from_folder(&resource.id, &folder.id)
            .await
            .expect("unlink");

        let empty = store
            .list_resources(None, None, Some(&folder.id))
            .await
            .expect("list");
        assert_eq!(empty.len(), 0);
        cleanup(dir);
    }

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
            .create_resource(CreateResourceRequest {
                resource_type: "table".to_string(),
                name: "tagged".to_string(),
                config: serde_json::json!({}),
                scope: "project".to_string(),
                alias: None,
                source_query: None,
                column_count: None,
                file_size: None,
                row_count: None,
                parent_resource_id: None,
            })
            .await
            .expect("create");
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

    #[tokio::test]
    async fn t010_paginated_list() {
        let (store, dir) = create_test_store().await;
        for i in 0..5 {
            store
                .create_resource(CreateResourceRequest {
                    resource_type: "table".to_string(),
                    name: format!("p{}", i),
                    config: serde_json::json!({}),
                    scope: "project".to_string(),
                    alias: None,
                    source_query: None,
                    column_count: None,
                    file_size: None,
                    row_count: None,
                    parent_resource_id: None,
                })
                .await
                .expect("create");
        }

        let page = store
            .list_resources_paginated(None, None, None, None, 1, 2, None, None)
            .await
            .expect("paginated");
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.total, 5);
        assert_eq!(page.total_pages, 3);
        cleanup(dir);
    }

    #[tokio::test]
    async fn t011_version_history() {
        let (store, dir) = create_test_store().await;
        let created = store
            .create_resource(CreateResourceRequest {
                resource_type: "table".to_string(),
                name: "versioned".to_string(),
                config: serde_json::json!({}),
                scope: "project".to_string(),
                alias: None,
                source_query: None,
                column_count: None,
                file_size: None,
                row_count: None,
                parent_resource_id: None,
            })
            .await
            .expect("create");

        store
            .update_resource(
                &created.id,
                CreateResourceRequest {
                    resource_type: "table".to_string(),
                    name: "versioned_v2".to_string(),
                    config: serde_json::json!({}),
                    scope: "project".to_string(),
                    alias: None,
                    source_query: None,
                    column_count: None,
                    file_size: None,
                    row_count: None,
                    parent_resource_id: None,
                },
            )
            .await
            .expect("update");

        let versions = store
            .get_resource_versions(&created.id)
            .await
            .expect("versions");
        assert!(!versions.is_empty());
        assert_eq!(versions[0].version, 1);
        cleanup(dir);
    }

    #[tokio::test]
    async fn t012_concurrent_create() {
        let (store, dir) = create_test_store().await;

        let make_req = |name: &str| CreateResourceRequest {
            resource_type: "table".to_string(),
            name: name.to_string(),
            config: serde_json::json!({}),
            scope: "project".to_string(),
            alias: None,
            source_query: None,
            column_count: None,
            file_size: None,
            row_count: None,
            parent_resource_id: None,
        };

        let (r1, r2, r3) = tokio::join!(
            store.create_resource(make_req("concurrent_a")),
            store.create_resource(make_req("concurrent_b")),
            store.create_resource(make_req("concurrent_c")),
        );

        assert!(r1.is_ok(), "concurrent_a failed: {:?}", r1.err());
        assert!(r2.is_ok(), "concurrent_b failed: {:?}", r2.err());
        assert!(r3.is_ok(), "concurrent_c failed: {:?}", r3.err());

        let all = store
            .list_resources(None, None, None)
            .await
            .expect("list all");
        assert_eq!(all.len(), 3);

        drop(store);
        cleanup(dir);
    }

    #[tokio::test]
    async fn t013_invalid_resource_id() {
        let (store, dir) = create_test_store().await;

        let result = store.get_resource_by_id("nonexistent-id").await;
        assert!(result.is_err(), "non-existent ID should return error");

        let result = store
            .update_resource(
                "nonexistent-id",
                CreateResourceRequest {
                    resource_type: "table".to_string(),
                    name: "ghost".to_string(),
                    config: serde_json::json!({}),
                    scope: "project".to_string(),
                    alias: None,
                    source_query: None,
                    column_count: None,
                    file_size: None,
                    row_count: None,
                    parent_resource_id: None,
                },
            )
            .await;
        assert!(result.is_err(), "update non-existent should fail");

        let result = store.delete_resource("nonexistent-id").await;
        assert!(result.is_err(), "delete non-existent should fail");

        drop(store);
        cleanup(dir);
    }

    #[tokio::test]
    async fn t014_restore_nonexistent_recycle() {
        let (store, dir) = create_test_store().await;

        let result = store.restore_from_recycle("nonexistent-recycle-id").await;
        assert!(
            result.is_err(),
            "restore from non-existent recycle should fail"
        );

        drop(store);
        cleanup(dir);
    }

    #[tokio::test]
    async fn t015_concurrent_update_same_resource() {
        let (store, dir) = create_test_store().await;

        let created = store
            .create_resource(CreateResourceRequest {
                resource_type: "table".to_string(),
                name: "concurrent_target".to_string(),
                config: serde_json::json!({"v": 0}),
                scope: "project".to_string(),
                alias: None,
                source_query: None,
                column_count: None,
                file_size: None,
                row_count: None,
                parent_resource_id: None,
            })
            .await
            .expect("create");

        let store = std::sync::Arc::new(store);
        let s1 = store.clone();
        let s2 = store.clone();
        let id1 = created.id.clone();
        let id2 = created.id.clone();

        let (r1, r2) = tokio::join!(
            s1.update_resource(
                &id1,
                CreateResourceRequest {
                    resource_type: "table".to_string(),
                    name: "update_a".to_string(),
                    config: serde_json::json!({"v": 1}),
                    scope: "project".to_string(),
                    alias: None,
                    source_query: None,
                    column_count: None,
                    file_size: None,
                    row_count: None,
                    parent_resource_id: None,
                },
            ),
            s2.update_resource(
                &id2,
                CreateResourceRequest {
                    resource_type: "table".to_string(),
                    name: "update_b".to_string(),
                    config: serde_json::json!({"v": 2}),
                    scope: "project".to_string(),
                    alias: None,
                    source_query: None,
                    column_count: None,
                    file_size: None,
                    row_count: None,
                    parent_resource_id: None,
                },
            ),
        );

        assert!(r1.is_ok(), "concurrent update A failed: {:?}", r1.err());
        assert!(r2.is_ok(), "concurrent update B failed: {:?}", r2.err());

        let versions = store
            .get_resource_versions(&created.id)
            .await
            .expect("versions");

        // 写前快照语义 + `UNIQUE(resource_id, version)`：两次更新各留下**一条写前快照**
        // （v1、v2），当前版本（v3）只在资源行上、不进版本表。
        // v1 原断言是 3 条（"original + 2 updates"），这在任何并发交错下都不可满足；
        // 要守住的不变式是"两次更新都留下快照、版本号单调递增到 3（不丢版本）"。
        assert_eq!(versions.len(), 2, "两次更新应留下两条写前快照");
        assert_eq!(versions[0].version, 2, "版本表按版本号倒序");
        assert_eq!(versions[1].version, 1);

        let latest = store
            .get_resource_by_id(&created.id)
            .await
            .expect("reload");
        assert_eq!(latest.version, 3, "版本号必须单调递增到 3（无丢失）");

        drop(store);
        drop(dir);
    }

    /// 020 增量迁移：新列存在、旧行默认值正确、`CHECK` 生效。
    ///
    /// 这是**库层契约**的守门用例：SQL 语法（`ALTER TABLE ... CHECK`）、默认值
    /// （`kind = 'file'` / `readonly = 1` / `content_hash` 为空）一旦被改坏，这里先红。
    #[tokio::test]
    async fn t016_archive_migration_adds_columns_and_defaults() {
        let (store, dir) = create_test_store().await;

        store
            .pool
            .acquire()
            .await
            .expect("acquire")
            .inner()
            .expect("inner")
            .execute_batch(ARCHIVE_MIGRATION_SQL)
            .expect("run 020 migration");

        // 旧行（v1 时代写入、不带 kind）应靠默认值满足新约束。
        let created = store
            .create_resource(CreateResourceRequest {
                resource_type: "file".to_string(),
                name: "old_style_row".to_string(),
                config: serde_json::json!({}),
                scope: "project".to_string(),
                alias: None,
                source_query: None,
                column_count: None,
                file_size: None,
                row_count: None,
                parent_resource_id: None,
            })
            .await
            .expect("create");

        let conn = store.pool.acquire().await.expect("acquire 2");
        let inner = conn.inner().expect("inner 2");

        let columns: Vec<String> = {
            let mut stmt = inner
                .prepare("PRAGMA table_info(analytics_resources)")
                .expect("pragma");
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .expect("query columns");
            rows.collect::<Result<Vec<_>, _>>().expect("collect")
        };
        for expected in [
            "kind",
            "content_hash",
            "file_rel_path",
            "readonly",
            "promoted_from",
            "source_connection_id",
            "source_table",
            "definition_sql",
            "archived_at",
        ] {
            assert!(columns.iter().any(|c| c == expected), "缺列：{expected}");
        }

        let (kind, readonly, hash): (String, i64, Option<String>) = inner
            .query_row(
                "SELECT kind, readonly, content_hash FROM analytics_resources WHERE id = ?",
                rusqlite::params![&created.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("select defaults");
        assert_eq!(kind, "file", "旧行默认 kind = file");
        assert_eq!(readonly, 1, "默认只读");
        assert_eq!(hash, None, "指纹待回填（不是错误）");

        assert!(
            inner
                .execute(
                    "UPDATE analytics_resources SET kind = 'nope' WHERE id = ?",
                    rusqlite::params![&created.id],
                )
                .is_err(),
            "kind 的 CHECK 约束应拒绝非法值"
        );

        drop(conn);
        drop(store);
        cleanup(dir);
    }
}
