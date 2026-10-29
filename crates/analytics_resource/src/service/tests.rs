    use super::*;
    use crate::model::{ArchiveBinding, ArchiveKind, PayloadMode};
    use engine::persistence::ProjectSqlitePool;
    use std::sync::Arc;
    use tokio::fs;

    const MIGRATION_SQL: &str =
        include_str!("../../../store/migrations/project_meta/007_analytics_resources.sql");
    const ARCHIVE_MIGRATION_SQL: &str =
        include_str!("../../../store/migrations/project_meta/020_analytics_resource_archive.sql");
    const PAYLOAD_MODE_MIGRATION_SQL: &str =
        include_str!("../../../store/migrations/project_meta/023_analytics_resource_payload_mode.sql");
    /// 关联表去 tag 外键（标签定义上移全局库，规格 §7.2；跨库无法 FK 约束）。
    const TAG_LINK_FK_MIGRATION_SQL: &str = include_str!(
        "../../../store/migrations/project_meta/024_analytics_resource_tags_drop_tag_fk.sql"
    );

    /// 测试服务：默认给「保留 n 份」（大多数用例只关心份数）。
    ///
    /// 需要全留 / 只留元数据这类策略的用例走 [`test_service_with`]。
    async fn test_service(keep_versions: u32) -> (ArchiveService, PathBuf) {
        test_service_with(KeepVersions::Keep(keep_versions)).await
    }

    async fn test_service_with(keep_versions: KeepVersions) -> (ArchiveService, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("rds_archive_{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).await.expect("create temp project");
        let pool = Arc::new(
            ProjectSqlitePool::new(dir.join("project.db"), 2)
                .await
                .expect("create pool"),
        );
        {
            let conn = pool.acquire().await.expect("acquire");
            let inner = conn.inner().expect("inner");
            inner.execute_batch(MIGRATION_SQL).expect("migration 007");
            inner
                .execute_batch(ARCHIVE_MIGRATION_SQL)
                .expect("migration 020");
            inner
                .execute_batch(PAYLOAD_MODE_MIGRATION_SQL)
                .expect("migration 023");
            inner
                .execute_batch(TAG_LINK_FK_MIGRATION_SQL)
                .expect("migration 024");
        }
        // 全局标签库注入本测试独立路径（避免共享进程级隔离根 → 跨测试同名标签冲突）。
        let store = AnalyticsResourceStore::new_for_test(pool, dir.join("global.db"));
        (
            ArchiveService::new(&dir, store).with_keep_versions(keep_versions),
            dir,
        )
    }

    fn archive_req(source: &std::path::Path, rel: &str, existing: Option<&str>) -> ArchiveRequest {
        ArchiveRequest {
            source_path: source.to_path_buf(),
            rel_path: rel.to_string(),
            name: "dau_report".to_string(),
            alias: None,
            kind: ArchiveKind::File,
            // 大多数服务用例是文件型归档；分析表那条路见 `archive_analysis_req`。
            payload_mode: PayloadMode::Copy,
            analysis: None,
            binding: ArchiveBinding {
                promoted_from: Some("scratchpad/dau.sql".to_string()),
                source_connection_id: Some("conn_1".to_string()),
                source_table: None,
            },
            tags: Vec::new(),
            group_id: None,
            keep_versions: None,
            existing_resource_id: existing.map(str::to_string),
        }
    }

    async fn write_draft(dir: &std::path::Path, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).await.expect("write draft");
        path
    }

    #[tokio::test]
    async fn t101_archive_first_time_copies_payload_and_writes_registry() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;

        let outcome = service
            .archive(archive_req(&draft, "reports/dau.sql", None))
            .await
            .expect("archive");

        assert!(outcome.created_new_version);
        assert_eq!(outcome.version, 1);
        // copy 语义（2026-10-13 裁决）：归档是**复制**，草稿保留为工作副本（checkout 语义）。
        assert!(draft.exists(), "归档是复制：草稿箱那份保留为工作副本");

        let payload = service
            .payload()
            .resources_dir()
            .join("reports")
            .join("dau.sql");
        assert!(payload.is_file());
        assert!(service.payload().is_readonly(&payload), "本体应只读");

        let row = service
            .store()
            .get_resource_by_id(&outcome.resource_id)
            .await
            .expect("row");
        assert_eq!(row.kind, "file");
        assert_eq!(
            row.content_hash.as_deref(),
            Some(outcome.content_hash.as_str())
        );
        assert_eq!(row.file_rel_path.as_deref(), Some("reports/dau.sql"));
        assert_eq!(row.readonly, 1);
        assert!(row.archived_at.is_some(), "归档时刻应记录");
        assert_eq!(
            row.file_size,
            Some(9),
            "归档时登记本体字节数（「大小」排序与行的尾巴都读它）"
        );
        assert_eq!(row.promoted_from.as_deref(), Some("scratchpad/dau.sql"));
        assert_eq!(row.source_connection_id.as_deref(), Some("conn_1"));

        let event = events.recv().await.expect("event");
        assert_eq!(event.reason, ChangeReason::Archived);
        assert_eq!(
            event.resource_id.as_deref(),
            Some(outcome.resource_id.as_str())
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t102_archive_refuses_occupied_target_and_keeps_source() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let first = write_draft(&dir, "a.sql", b"select 1;").await;
        service
            .archive(archive_req(&first, "same.sql", None))
            .await
            .expect("first archive");

        let second = write_draft(&dir, "b.sql", b"select 2;").await;
        let error = service
            .archive(archive_req(&second, "same.sql", None))
            .await
            .expect_err("conflict must fail");

        assert!(
            error.to_string().contains("已被存档"),
            "错误应指明占用者：{error}"
        );
        assert!(second.is_file(), "失败时源文件必须保留");

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t103_archive_rolls_back_payload_when_index_write_fails() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;

        // 故障注入：用触发器让"写索引"必然失败（比构造数据冲突更贴近真实失败路径）。
        {
            let conn = service.store().pool().acquire().await.expect("acquire");
            conn.inner()
                .expect("inner")
                .execute_batch(
                    "CREATE TRIGGER test_block_archive BEFORE INSERT ON analytics_resources \
                     BEGIN SELECT RAISE(ABORT, 'blocked for test'); END;",
                )
                .expect("create trigger");
        }

        let draft = write_draft(&dir, "draft.sql", b"select 1;").await;
        let error = service
            .archive(archive_req(&draft, "taken.sql", None))
            .await
            .expect_err("index write must fail");

        assert!(error.to_string().contains("blocked for test"));
        assert!(
            draft.is_file(),
            "索引失败必须把本体移回原处（错误：{error}）"
        );
        assert!(
            !service.payload().resources_dir().join("taken.sql").exists(),
            "回滚后本体不应留在 resources/"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t104_checkout_copies_payload_out_writable() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let outcome = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");
        let _ = events.recv().await;

        let dest = dir.join("scratchpad").join("dau（工作副本）.sql");
        let checkout = service
            .checkout(CheckoutRequest {
                resource_id: outcome.resource_id.clone(),
                dest_path: dest.clone(),
            })
            .await
            .expect("checkout");

        assert_eq!(checkout.version, 1);
        assert!(dest.is_file());
        assert!(!service.payload().is_readonly(&dest), "工作副本必须可写");
        assert!(
            service.payload().resources_dir().join("dau.sql").is_file(),
            "取回不得移动本体"
        );
        let event = events.recv().await.expect("event");
        assert_eq!(event.reason, ChangeReason::CheckedOut);

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t105_rearchive_with_same_content_is_idempotent() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let first = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");

        // 取回原样副本 → 内容未变 → 不应产生新版本。
        let copy = dir.join("dau（工作副本）.sql");
        service
            .checkout(CheckoutRequest {
                resource_id: first.resource_id.clone(),
                dest_path: copy.clone(),
            })
            .await
            .expect("checkout");

        let again = service
            .archive(archive_req(&copy, "dau.sql", Some(&first.resource_id)))
            .await
            .expect("re-archive");

        assert!(!again.created_new_version, "内容未变不应产生新版本");
        assert_eq!(again.version, 1);
        assert!(copy.is_file(), "幂等路径不删用户的工作副本");

        let versions = service
            .store()
            .get_resource_versions(&first.resource_id)
            .await
            .expect("versions");
        assert!(versions.is_empty(), "不应有历史版本行");

        let _ = fs::remove_dir_all(&dir).await;
    }

    /// 分析表的指纹口径（P4.1 / R4）：**定义 + 结构**进指纹，**行数不进**；
    /// 定义与规模同批登记，再归档时跟着更新。
    #[tokio::test]
    async fn t133_analysis_fingerprint_covers_definition_and_structure_only() {
        use crate::analysis::{AnalysisFacts, ColumnSpec};

        let (service, dir) = test_service(5).await;
        let draft = write_draft(
            &dir,
            "dau.csv",
            b"id,name
1,a
",
        )
        .await;
        let facts = AnalysisFacts {
            definition_sql: Some("select * from read_csv_auto('dau.csv')".to_string()),
            columns: vec![
                ColumnSpec::new("id", "INTEGER"),
                ColumnSpec::new("name", "VARCHAR"),
            ],
            row_count: Some(1),
        };
        let mut req = archive_req(&draft, "dau.csv", None);
        req.kind = ArchiveKind::Analysis;
        req.analysis = Some(facts.clone());
        let first = service.archive(req).await.expect("archive analysis");
        assert_eq!(
            first.content_hash,
            facts.fingerprint(),
            "分析表登记的是结构指纹，不是文件字节指纹"
        );

        let stored = service
            .store()
            .get_resource_by_id(&first.resource_id)
            .await
            .expect("reload");
        assert_eq!(stored.kind, "analysis");
        assert_eq!(stored.definition_sql, facts.definition_sql);
        assert_eq!(stored.row_count, Some(1), "行数作为元信息落库");
        assert_eq!(stored.column_count, Some(2));

        // 再归档：**同定义同结构、只多了一行** → 指纹不变 → 幂等（不涨版本、不覆盖本体）。
        let draft2 = write_draft(
            &dir,
            "dau2.csv",
            b"id,name
1,a
2,b
3,c
",
        )
        .await;
        let mut same_schema = archive_req(&draft2, "dau.csv", Some(&first.resource_id));
        same_schema.kind = ArchiveKind::Analysis;
        same_schema.analysis = Some(AnalysisFacts {
            row_count: Some(3),
            ..facts.clone()
        });
        let second = service.archive(same_schema).await.expect("re-archive same");
        assert_eq!(second.version, first.version, "行数变化不算内容变化");
        assert!(!second.created_new_version, "幂等：不产生新版本");

        // 结构变了（多一列）→ 指纹变 → 新版本，且定义 / 规模随更新。
        let draft3 = write_draft(
            &dir,
            "dau3.csv",
            b"id,name,ts
1,a,now
",
        )
        .await;
        let mut changed = archive_req(&draft3, "dau.csv", Some(&first.resource_id));
        changed.kind = ArchiveKind::Analysis;
        changed.analysis = Some(AnalysisFacts {
            definition_sql: Some("select id, name, ts from read_csv_auto('dau.csv')".to_string()),
            columns: vec![
                ColumnSpec::new("id", "INTEGER"),
                ColumnSpec::new("name", "VARCHAR"),
                ColumnSpec::new("ts", "VARCHAR"),
            ],
            row_count: Some(1),
        });
        let third = service.archive(changed).await.expect("re-archive changed");
        assert_eq!(third.version, first.version + 1, "结构变了要涨版本");
        let stored = service
            .store()
            .get_resource_by_id(&first.resource_id)
            .await
            .expect("reload");
        assert_eq!(stored.column_count, Some(3), "规模元信息跟着更新");
        assert!(stored.definition_sql.expect("定义").contains("ts"));
    }

    #[tokio::test]
    async fn t106_rearchive_changed_content_bumps_version_and_keeps_copy() {
        let (service, dir) = test_service(1).await;
        let mut events = service.subscribe();
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let first = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");
        assert_eq!(
            events.recv().await.expect("archived event").reason,
            ChangeReason::Archived
        );

        // 取回 → 改动 → 再归档。
        let copy = dir.join("dau（工作副本）.sql");
        service
            .checkout(CheckoutRequest {
                resource_id: first.resource_id.clone(),
                dest_path: copy.clone(),
            })
            .await
            .expect("checkout");
        assert_eq!(
            events.recv().await.expect("checked out event").reason,
            ChangeReason::CheckedOut
        );
        // 特意改成长度不同的内容：体积是否跟着换，看的是这个数。
        fs::write(&copy, b"select 22;").await.expect("edit copy");

        let second = service
            .archive(archive_req(&copy, "dau.sql", Some(&first.resource_id)))
            .await
            .expect("re-archive");

        assert!(second.created_new_version);
        assert_eq!(second.version, 2, "内容变了版本 +1");
        assert!(!copy.exists(), "再归档同样是移动");

        let payload = service.payload().resources_dir().join("dau.sql");
        assert_eq!(
            service
                .payload()
                .content_hash(&payload)
                .await
                .expect("hash"),
            second.content_hash,
            "本体应已被新内容覆盖"
        );
        assert!(service.payload().is_readonly(&payload), "替换后仍只读");

        let versions = service
            .store()
            .get_resource_versions(&first.resource_id)
            .await
            .expect("versions");
        assert_eq!(versions.len(), 1, "写前快照：只保留历史版本 v1");
        assert_eq!(versions[0].version, 1);

        let row = service
            .store()
            .get_resource_by_id(&first.resource_id)
            .await
            .expect("row");
        assert_eq!(row.version, 2);
        assert_eq!(
            row.file_size,
            Some(10),
            "体积跟着新内容一起换（不与指纹脱节）"
        );
        assert_eq!(
            row.parent_version_id.as_deref(),
            Some(versions[0].id.as_str()),
            "parent_version_id 应指向写前快照行"
        );

        let event = events.recv().await.expect("event");
        assert_eq!(event.reason, ChangeReason::Updated);

        let _ = fs::remove_dir_all(&dir).await;
    }

    /// 保留策略的两端：`All`（设置项 `-1`）一份不裁，`MetadataOnly`（`0`）副本全清——
    /// 两端都不动**版本行**（元数据永远保留，架构 §5.2）。
    #[tokio::test]
    async fn t130_keep_policy_ends_are_respected() {
        for (policy, expected_copies) in [
            (KeepVersions::All, vec![2, 1]),
            (KeepVersions::MetadataOnly, Vec::new()),
        ] {
            let (service, dir) = test_service_with(policy).await;
            let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
            let first = service
                .archive(archive_req(&draft, "dau.sql", None))
                .await
                .expect("archive");

            // 连着改两次内容：每次再归档都会把上一版留成一份内容副本。
            for (name, body) in [
                ("dau2.sql", &b"select 22;"[..]),
                ("dau3.sql", &b"select 333;"[..]),
            ] {
                let next = write_draft(&dir, name, body).await;
                service
                    .archive(archive_req(&next, "dau.sql", Some(&first.resource_id)))
                    .await
                    .expect("re-archive");
            }

            assert_eq!(
                service
                    .payload()
                    .version_copies(&first.resource_id)
                    .await
                    .expect("copies"),
                expected_copies,
                "{policy:?} 的副本保留结果"
            );
            assert_eq!(
                service
                    .store()
                    .get_resource_versions(&first.resource_id)
                    .await
                    .expect("version rows")
                    .len(),
                2,
                "裁剪只动副本：版本行两个（v1 / v2 写前快照）应全在"
            );
            let _ = fs::remove_dir_all(&dir).await;
        }
    }

    /// 归档时填的**标签与分组真的落上**（之前 `ArchiveRequest` 的这两项没人消费，填了等于白填）：
    /// 标签按名字找 / 找不到就建（同名复用），分组就是移动语义；不存在的分组只给回执说明、
    /// **不回滚归档本身**（文件已经归档好了，分组是能在面板上补做的附属项）。
    #[tokio::test]
    async fn t131_archive_applies_tags_and_group() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let folder = service
            .store()
            .create_folder(crate::models::CreateFolderRequest {
                name: "月报".to_string(),
                scope: "project".to_string(),
                parent_folder_id: None,
                color: None,
                icon: None,
            })
            .await
            .expect("create folder");

        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let mut req = archive_req(&draft, "dau.sql", None);
        req.alias = Some("日报".to_string());
        req.tags = vec!["报表".to_string(), "月度".to_string(), "报表".to_string()];
        req.group_id = Some(folder.id.clone());

        let outcome = service.archive(req).await.expect("archive");
        assert!(
            outcome.notes.is_empty(),
            "全落上时不该有说明：{:?}",
            outcome.notes
        );

        let row = service
            .store()
            .get_resource_by_id(&outcome.resource_id)
            .await
            .expect("row");
        assert_eq!(row.alias.as_deref(), Some("日报"), "别名应写进登记行");

        let tags = service
            .store()
            .get_tags_for_resource(&outcome.resource_id)
            .await
            .expect("tags");
        let mut names: Vec<&str> = tags.iter().map(|tag| tag.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["报表", "月度"],
            "两个标签都建好并挂上（重复名只算一次）"
        );

        assert_eq!(
            service
                .store()
                .folders_by_resource()
                .await
                .expect("folders"),
            std::collections::HashMap::from([(outcome.resource_id.clone(), folder.id.clone())]),
            "分组归属应落到关联表"
        );

        // 再归档另一份：同名标签**复用**，不另建一个。
        let second = write_draft(&dir, "dau2.sql", b"select 2;").await;
        let mut req = archive_req(&second, "dau2.sql", None);
        req.tags = vec!["报表".to_string()];
        service.archive(req).await.expect("archive 2");
        assert_eq!(
            service.store().list_tags(None).await.expect("tags").len(),
            2,
            "同名标签复用，不是每个存档建一个"
        );

        // 分组不存在（悬空 id）：归档仍然成功，但要在回执里说清。
        let third = write_draft(&dir, "dau3.sql", b"select 3;").await;
        let mut req = archive_req(&third, "dau3.sql", None);
        req.group_id = Some("af_不存在".to_string());
        let outcome = service.archive(req).await.expect("archive 3");
        assert_eq!(outcome.notes.len(), 1, "应有一条说明：{:?}", outcome.notes);
        assert!(
            outcome.notes[0].starts_with("分组未归入："),
            "说明要说清是哪一项没落上：{:?}",
            outcome.notes[0]
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn t107_checkout_rejects_target_inside_resources() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let draft = write_draft(&dir, "dau.sql", b"select 1;").await;
        let outcome = service
            .archive(archive_req(&draft, "dau.sql", None))
            .await
            .expect("archive");

        let inside = service.payload().resources_dir().join("escape.sql");
        assert!(
            service
                .checkout(CheckoutRequest {
                    resource_id: outcome.resource_id,
                    dest_path: inside,
                })
                .await
                .is_err(),
            "取回目标落在 resources/ 内必须被拒（那等于绕开只读守卫）"
        );

        let _ = fs::remove_dir_all(&dir).await;
    }

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
            .archive(archive_req(
                &draft2,
                "occupied2.sql",
                None,
            ))
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
            .create_tag(crate::models::CreateTagRequest {
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

    /// 构造 reference 归档请求：本体留原位、只登记元数据。
    fn reference_req(source: &std::path::Path, rel: &str) -> ArchiveRequest {
        let mut req = archive_req(source, rel, None);
        req.payload_mode = PayloadMode::Reference;
        req
    }

    /// reference 归档：本体不移动、源保留；登记 `file_rel_path`=源绝对路径 + `source_path`；
    /// 指纹照算（源文件字节 sha256）。
    #[tokio::test]
    async fn t134_reference_archive_keeps_source_in_place_and_registers_metadata() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        // 源在项目外（reference 的典型场景：大文件 / 外部路径），但要绝对路径。
        let outside = std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
        fs::write(&outside, b"a,b\n1,2\n").await.expect("write external source");

        let outcome = service
            .archive(reference_req(&outside, "data/big.csv"))
            .await
            .expect("reference archive");

        assert!(outcome.created_new_version);
        assert!(outside.is_file(), "reference：本体留原位，不搬走");
        assert!(
            !service.payload().resources_dir().join("data/big.csv").exists(),
            "reference：不复制进 resources/"
        );

        let row = service
            .store()
            .get_resource_by_id(&outcome.resource_id)
            .await
            .expect("row");
        assert_eq!(row.payload_mode, "reference");
        assert_eq!(
            row.file_rel_path.as_deref(),
            Some(outside.to_string_lossy().replace('\\', "/").as_str()),
            "reference 的 file_rel_path = 源绝对路径（唯一索引防同一源重复归档）"
        );
        assert_eq!(
            row.source_path.as_deref(),
            Some(outside.to_string_lossy().replace('\\', "/").as_str()),
            "source_path 记录源绝对路径"
        );
        assert!(row.content_hash.is_some(), "指纹照算（源文件字节）");

        let _ = fs::remove_dir_all(&dir).await;
        let _ = fs::remove_file(&outside).await;
    }

    /// reference 取回：从**源**复制出可写工作副本（不是从 `resources/`）。
    #[tokio::test]
    async fn t135_reference_checkout_copies_from_source() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let outside = std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
        fs::write(&outside, b"a,b\n1,2\n").await.expect("write external source");
        let archived = service
            .archive(reference_req(&outside, "data/big.csv"))
            .await
            .expect("reference archive");

        let dest = dir.join("scratchpad").join("big（工作副本）.csv");
        let checked = service
            .checkout(CheckoutRequest {
                resource_id: archived.resource_id.clone(),
                dest_path: dest.clone(),
            })
            .await
            .expect("reference checkout");

        assert_eq!(checked.dest_path, dest);
        assert_eq!(fs::read(&dest).await.expect("read"), b"a,b\n1,2\n");
        assert!(!service.payload().is_readonly(&dest), "工作副本必须可写");
        assert!(outside.is_file(), "源不受取回影响");

        let _ = fs::remove_dir_all(&dir).await;
        let _ = fs::remove_file(&outside).await;
    }

    /// reference 再归档：内容变了 → 版本 +1、指纹更新；源**不被覆盖**（本体在外部，M6 不动它）。
    #[tokio::test]
    async fn t136_reference_rearchive_updates_fingerprint_without_touching_source() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let outside = std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
        fs::write(&outside, b"a,b\n1,2\n").await.expect("write source v1");
        let archived = service
            .archive(reference_req(&outside, "data/big.csv"))
            .await
            .expect("reference archive");
        assert_eq!(archived.version, 1);

        // 源内容被外部改掉（活文件）→ 再归档 = 新版本（指纹变了）。
        fs::write(&outside, b"a,b,c\n1,2,3\n").await.expect("mutate source");
        let mut re = reference_req(&outside, "data/big.csv");
        re.existing_resource_id = Some(archived.resource_id.clone());
        let updated = service
            .archive(re).await
            .expect("reference re-archive");

        assert!(updated.created_new_version);
        assert_eq!(updated.version, 2);
        assert!(
            !service.payload().resources_dir().join("data/big.csv").exists(),
            "reference 再归档不产生 resources/ 副本"
        );
        // 源内容保持（不是被“还原/覆盖”成什么别的）——M6 不动外部本体。
        assert_eq!(fs::read(&outside).await.expect("read"), b"a,b,c\n1,2,3\n");

        let _ = fs::remove_dir_all(&dir).await;
        let _ = fs::remove_file(&outside).await;
    }

    /// reference 撤销：只删登记行（源本来就在原位，没什么要搬回去）。
    #[tokio::test]
    async fn t137_reference_undo_drops_row_only() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let outside = std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
        fs::write(&outside, b"a,b\n1,2\n").await.expect("write external source");
        let archived = service
            .archive(reference_req(&outside, "data/big.csv"))
            .await
            .expect("reference archive");

        service
            .undo_archive(&ArchiveUndo {
                resource_id: archived.resource_id.clone(),
                name: "big".to_string(),
                source_path: outside.clone(),
            })
            .await
            .expect("reference undo");

        assert!(outside.is_file(), "源不动");
        assert!(
            service
                .store()
                .get_resource_by_id(&archived.resource_id)
                .await
                .is_err(),
            "登记行已删"
        );

        let _ = fs::remove_dir_all(&dir).await;
        let _ = fs::remove_file(&outside).await;
    }

    /// reference 移入回收站（§12.1.1 ③）：本体留原位，只软删登记行；还原只复活登记行；
    /// 永久删除只清登记行 + 版本元数据——全流程都不碰源文件（与 copy 同一进回收站判据）。
    #[tokio::test]
    async fn t138_reference_trash_cycles_without_touching_source() {
        let (service, dir) = test_service(DEFAULT_KEEP_VERSIONS).await;
        let outside = std::env::temp_dir().join(format!("ref_src_{}.csv", uuid::Uuid::new_v4().simple()));
        fs::write(&outside, b"a,b\n1,2\n").await.expect("write external source");
        let archived = service
            .archive(reference_req(&outside, "data/big.csv"))
            .await
            .expect("reference archive");
        let id = archived.resource_id.clone();

        // 1) 移入回收站：成功，本体不动，登记行进入软删态；条目带 ref: 前缀。
        let trashed = service
            .move_to_trash(&[id.clone()])
            .await
            .expect("reference 可进回收站（与 copy 同一判据）");
        let trash_id = trashed[0].trash_id.clone();
        assert!(
            trash_id.starts_with(crate::model::TRASH_ID_REF_PREFIX),
            "reference 回收站条目带 ref: 前缀"
        );
        assert!(outside.is_file(), "本体留原位");
        assert!(
            service
                .store()
                .get_resource_by_id(&id)
                .await
                .expect("row")
                .deleted_at
                .is_some(),
            "登记行进入软删态"
        );

        // 2) 还原：只复活登记行，本体仍在源位置。
        let restored = service
            .restore_archive_from_trash(&trash_id)
            .await
            .expect("reference 还原登记行");
        assert_eq!(restored.id, id);
        assert!(outside.is_file(), "本体仍在源位置");
        assert!(
            service
                .store()
                .get_resource_by_id(&id)
                .await
                .expect("row")
                .deleted_at
                .is_none(),
            "登记行复活"
        );

        // 3) 再次移入 → 永久删除：只清登记行与版本元数据，本体不动、登记行不再可查。
        let trashed_again = service
            .move_to_trash(&[id.clone()])
            .await
            .expect("reference 再次移入回收站");
        service
            .purge_archive(&trashed_again[0].trash_id)
            .await
            .expect("reference 永久删除只清登记行");
        assert!(outside.is_file(), "永久删除不碰本体");
        assert!(
            service.store().get_resource_by_id(&id).await.is_err(),
            "登记行已被清除"
        );

        let _ = fs::remove_dir_all(&dir).await;
        let _ = fs::remove_file(&outside).await;
    }
