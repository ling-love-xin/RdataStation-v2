#[cfg(test)]
mod tests {
    use super::super::*;
    use engine::persistence::ProjectSqlitePool;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    const MIGRATION_SQL: &str =
        include_str!("../../store/migrations/project_meta/007_analytics_resources.sql");

    const ARCHIVE_MIGRATION_SQL: &str =
        include_str!("../../store/migrations/project_meta/020_analytics_resource_archive.sql");

    const PAYLOAD_MODE_MIGRATION_SQL: &str =
        include_str!("../../store/migrations/project_meta/023_analytics_resource_payload_mode.sql");
    /// 关联表去 tag 外键（标签定义上移全局库，规格 §7.2；跨库无法 FK 约束）。
    const TAG_LINK_FK_MIGRATION_SQL: &str = include_str!(
        "../../store/migrations/project_meta/024_analytics_resource_tags_drop_tag_fk.sql"
    );

    /// 测试库跑**四段**迁移：007（表结构）+ 020（分析存档新列）+ 023（copy/reference 两档）+
    /// 024（关联表去 tag 外键）。
    ///
    /// 跑齐是刻意的：只跑 007 的话，测试库的表结构与生产库不一致，行映射一旦引用新列
    /// 就会在测试里报 "no such column"（是缺陷，不是噪声）。
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
            let inner = conn.inner().expect("get connection");
            inner
                .execute_batch(MIGRATION_SQL)
                .expect("run migration 007");
            inner
                .execute_batch(ARCHIVE_MIGRATION_SQL)
                .expect("run migration 020");
            inner
                .execute_batch(PAYLOAD_MODE_MIGRATION_SQL)
                .expect("run migration 023");
            inner
                .execute_batch(TAG_LINK_FK_MIGRATION_SQL)
                .expect("run migration 024");
        }
        // 全局标签库注入本测试独立路径（避免共享进程级隔离根 → 跨测试同名标签冲突）。
        let store = AnalyticsResourceStore::new_for_test(pool, dir.join("global.db"));
        (store, dir)
    }

    fn cleanup(dir: PathBuf) {
        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn t001_create_and_get_resource() {
        let (store, dir) = create_test_store().await;
        let created = store
            .insert_archive(NewArchiveInput {
                resource_type: "table".to_string(),
                name: "test_table".to_string(),
                alias: None,
                kind: ArchiveKind::Analysis,
                content_hash: "aabbccdd00112233".to_string(),
                definition_sql: None,
                row_count: None,
                column_count: None,
                file_rel_path: "analysis/test_table.sql".to_string(),
                payload_mode: crate::model::PayloadMode::Copy,
                source_path: None,
                file_size: None,
                binding: ArchiveBinding::default(),
                scope: "project".to_string(),
            })
            .await
            .expect("create archive");
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

    /// 版本历史（v2 快照语义）：`save_resource_version` 写写前快照、
    /// `update_archive_content` 递增版本并指向该快照，历史里能看到 v1。
    #[tokio::test]
    async fn t011_version_history() {
        let (store, dir) = create_test_store().await;
        let created = store
            .insert_archive(NewArchiveInput {
                resource_type: "table".to_string(),
                name: "versioned".to_string(),
                alias: None,
                kind: ArchiveKind::Analysis,
                content_hash: "aabbccdd00112233".to_string(),
                definition_sql: None,
                row_count: None,
                column_count: None,
                file_rel_path: "analysis/versioned.sql".to_string(),
                payload_mode: crate::model::PayloadMode::Copy,
                source_path: None,
                file_size: None,
                binding: ArchiveBinding::default(),
                scope: "project".to_string(),
            })
            .await
            .expect("create archive");

        let snapshot = serde_json::to_string(&created).expect("snapshot json");
        let snapshot_id = store
            .save_resource_version(&created.id, created.version, &snapshot)
            .await
            .expect("save snapshot");
        store
            .update_archive_content(&created.id, "aabbccdd00112244", &snapshot_id, None, None)
            .await
            .expect("update content");

        let versions = store
            .get_resource_versions(&created.id)
            .await
            .expect("versions");
        assert_eq!(versions.len(), 1, "只写了一条写前快照");
        assert_eq!(versions[0].version, 1);
        cleanup(dir);
    }

    #[tokio::test]
    async fn t012_concurrent_create() {
        let (store, dir) = create_test_store().await;

        let make_input = |name: &str, hash: &str| NewArchiveInput {
            resource_type: "table".to_string(),
            name: name.to_string(),
            alias: None,
            kind: ArchiveKind::Analysis,
            content_hash: hash.to_string(),
            definition_sql: None,
            row_count: None,
            column_count: None,
            file_rel_path: format!("analysis/{name}.sql"),
            payload_mode: crate::model::PayloadMode::Copy,
            source_path: None,
            file_size: None,
            binding: ArchiveBinding::default(),
            scope: "project".to_string(),
        };

        let (r1, r2, r3) = tokio::join!(
            store.insert_archive(make_input("concurrent_a", "aaaaaaaa1111")),
            store.insert_archive(make_input("concurrent_b", "bbbbbbbb2222")),
            store.insert_archive(make_input("concurrent_c", "cccccccc3333")),
        );

        assert!(r1.is_ok(), "concurrent_a failed: {:?}", r1.err());
        assert!(r2.is_ok(), "concurrent_b failed: {:?}", r2.err());
        assert!(r3.is_ok(), "concurrent_c failed: {:?}", r3.err());

        let all = store.list_file_archives().await.expect("list all");
        assert_eq!(all.len(), 3, "三个并发归档都应落库");

        drop(store);
        cleanup(dir);
    }

    #[tokio::test]
    async fn t013_invalid_resource_id() {
        let (store, dir) = create_test_store().await;

        let result = store.get_resource_by_id("nonexistent-id").await;
        assert!(result.is_err(), "non-existent ID should return error");

        let result = store
            .update_archive_content("nonexistent-id", "aabbccdd00112233", "arv_x", None, None)
            .await;
        assert!(result.is_err(), "update non-existent should fail");

        drop(store);
        cleanup(dir);
    }

    /// 020 增量迁移：新列存在、旧行默认值正确、`CHECK` 生效。
    ///
    /// 这是**库层契约**的守门用例：SQL 语法（`ALTER TABLE ... CHECK`）、默认值
    /// （`kind = 'file'` / `readonly = 1` / `content_hash` 为空）一旦被改坏，这里先红。
    #[tokio::test]
    async fn t016_archive_migration_adds_columns_and_defaults() {
        let (store, dir) = create_test_store().await;

        // 旧行（v1 时代写入、不带 kind）应靠默认值满足新约束。
        // v1 的通用创建入口已随 v1 CRUD 一起移除，这里用裸 SQL 直接造旧行
        // （只写 v1 时代就存在的列，kind / content_hash 等 020 新列靠默认值补齐）。
        let old_id = format!("ar_{}", uuid::Uuid::new_v4().simple());
        let now = chrono::Utc::now().to_rfc3339();
        {
            let conn = store.pool.acquire().await.expect("acquire");
            conn.inner()
                .expect("inner")
                .execute(
                    r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count, file_size,
                version, parent_version_id, parent_resource_id, source_query, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
                    rusqlite::params![
                        &old_id,
                        "file",
                        "old_style_row",
                        None::<String>,
                        "{}",
                        "project",
                        None::<i64>,
                        None::<i64>,
                        None::<i64>,
                        1,
                        None::<String>,
                        None::<String>,
                        None::<String>,
                        &now,
                        &now,
                    ],
                )
                .expect("insert old row");
        }

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
                rusqlite::params![&old_id],
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
                    rusqlite::params![&old_id],
                )
                .is_err(),
            "kind 的 CHECK 约束应拒绝非法值"
        );

        drop(conn);
        drop(store);
        cleanup(dir);
    }

    /// 重命名只改显示名：不涨版本、不写版本快照、其它字段都不动（原型 §1 原则 2）。
    #[tokio::test]
    async fn t019_rename_changes_display_name_only() {
        let (store, dir) = create_test_store().await;
        // 用**归档行**做样本（而不是通用 `create_resource`）：只有它身上才有指纹 / 本体路径 /
        // 归档时间这些“改名绝对不能碰”的字段。
        let created = store
            .insert_archive(NewArchiveInput {
                resource_type: "file".to_string(),
                name: "dau.sql".to_string(),
                alias: Some("月报草稿".to_string()),
                kind: ArchiveKind::File,
                content_hash: "aabbccddeeff0011".to_string(),
                definition_sql: None,
                row_count: None,
                column_count: None,
                file_rel_path: "reports/dau.sql".to_string(),
                payload_mode: crate::model::PayloadMode::Copy,
                source_path: None,
                file_size: Some(1024),
                binding: ArchiveBinding::default(),
                scope: "project".to_string(),
            })
            .await
            .expect("insert archive");
        assert_eq!(created.file_rel_path.as_deref(), Some("reports/dau.sql"));

        let renamed = store
            .rename_resource(&created.id, "月报")
            .await
            .expect("rename");

        assert_eq!(renamed.name, "月报");
        assert_eq!(
            renamed.version, created.version,
            "改名不是内容变更：版本不涨"
        );
        assert_eq!(renamed.alias, created.alias, "别名与显示名是两回事");
        assert_eq!(
            renamed.file_rel_path, created.file_rel_path,
            "显示名与本体位置分离：路径不动"
        );
        assert_eq!(renamed.content_hash, created.content_hash, "指纹不动");
        assert_eq!(renamed.archived_at, created.archived_at, "归档凭证不动");
        assert_eq!(renamed.file_size, created.file_size);
        // `updated_at` 由 `trg_ar_updated_at` 触发器写（`CURRENT_TIMESTAMP`，**秒级**）：这里只钉
        // “它被更新到当前时刻附近”，不比纳秒——插入走的是 `Utc::now()`（带亚秒），触发器把亚秒
        // 抹掉后，同一秒内 `renamed` 可能比 `created` “小”（既有表行为，见开发方案第十三刀的备注）。
        let drift = (chrono::Utc::now() - renamed.updated_at)
            .num_seconds()
            .abs();
        assert!(
            drift <= 2,
            "改名后 `updated_at` 应跟到当前时刻附近：{}（偏差 {drift}s）",
            renamed.updated_at
        );
        assert!(
            store
                .get_resource_versions(&created.id)
                .await
                .expect("versions")
                .is_empty(),
            "不得写版本快照（那是内容版本的口径）"
        );

        // 已软删的行改不了名（与内容更新的守卫口径一致）。
        store
            .soft_delete_archive(&created.id)
            .await
            .expect("soft delete");
        assert!(
            store
                .rename_resource(&created.id, "消失的月报")
                .await
                .is_err()
        );

        cleanup(dir);
    }

    /// 改别名同样只动一列：不涨版本、不写快照；空串 = 清除（存 NULL，不存空串）。
    #[tokio::test]
    async fn t020_alias_is_display_only_and_empty_clears_it() {
        let (store, dir) = create_test_store().await;
        let created = store
            .insert_archive(NewArchiveInput {
                resource_type: "file".to_string(),
                name: "dau.sql".to_string(),
                alias: None,
                kind: ArchiveKind::File,
                content_hash: "1122334455667788".to_string(),
                definition_sql: None,
                row_count: None,
                column_count: None,
                file_rel_path: "reports/dau.sql".to_string(),
                payload_mode: crate::model::PayloadMode::Copy,
                source_path: None,
                file_size: Some(2048),
                binding: ArchiveBinding::default(),
                scope: "project".to_string(),
            })
            .await
            .expect("insert archive");
        assert_eq!(created.alias, None, "归档时没填就是没有");

        let aliased = store
            .set_alias(&created.id, Some("月报"))
            .await
            .expect("set alias");
        assert_eq!(aliased.alias.as_deref(), Some("月报"));
        assert_eq!(aliased.name, created.name, "别名不动显示名（两回事）");
        assert_eq!(
            aliased.version, created.version,
            "别名不是内容变更：版本不涨"
        );
        assert_eq!(aliased.content_hash, created.content_hash);
        assert_eq!(aliased.file_rel_path, created.file_rel_path);
        assert!(
            store
                .get_resource_versions(&created.id)
                .await
                .expect("versions")
                .is_empty(),
            "不得写版本快照"
        );

        // 首尾空格被去掉；空串 / 全空格 = 清除（回到 NULL，而不是存一个空别名）。
        let trimmed = store
            .set_alias(&created.id, Some("  季度月报  "))
            .await
            .expect("trim");
        assert_eq!(trimmed.alias.as_deref(), Some("季度月报"));
        let cleared = store
            .set_alias(&created.id, Some("   "))
            .await
            .expect("clear");
        assert_eq!(cleared.alias, None);

        store
            .soft_delete_archive(&created.id)
            .await
            .expect("soft delete");
        assert!(store.set_alias(&created.id, Some("x")).await.is_err());

        cleanup(dir);
    }
}
