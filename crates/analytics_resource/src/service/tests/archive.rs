//! 服务层用例分组。

use super::*;

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
        .create_folder(crate::store::rows::CreateFolderRequest {
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
