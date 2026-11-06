//! 根级用例分组。

use super::*;

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

/// 「最近 N 条」的批量取数：每个存档只取最新的 N 条，按版本降序；
/// 没有历史的存档**不出现在映射里**（详情层据此判定「只有当前版本」）。
#[tokio::test]
async fn t011b_recent_versions_take_top_n_per_resource() {
    let (store, dir) = create_test_store().await;
    let created = store
        .insert_archive(NewArchiveInput {
            resource_type: "table".to_string(),
            name: "recent".to_string(),
            alias: None,
            kind: ArchiveKind::Analysis,
            content_hash: "aabbccdd00112233".to_string(),
            definition_sql: None,
            row_count: None,
            column_count: None,
            file_rel_path: "analysis/recent.sql".to_string(),
            payload_mode: crate::model::PayloadMode::Copy,
            source_path: None,
            file_size: None,
            binding: ArchiveBinding::default(),
            scope: "project".to_string(),
        })
        .await
        .expect("create archive");
    // 造 4 条历史（v1..v4）：写前快照语义下这些是历史行，当前版本另算。
    for version in 1..=4 {
        let snapshot = serde_json::to_string(&created).expect("snapshot json");
        store
            .save_resource_version(&created.id, version, &snapshot)
            .await
            .expect("save snapshot");
    }

    let recent = store
        .recent_versions_by_resource(3)
        .await
        .expect("recent versions");
    let of_created = recent.get(&created.id).expect("该存档有历史");
    assert_eq!(of_created.len(), 3, "只要最近 3 条，不把 4 条全带出来");
    assert_eq!(
        of_created.iter().map(|v| v.version).collect::<Vec<_>>(),
        vec![4, 3, 2],
        "按版本降序取最新的三条"
    );
    // 没有历史的存档不占位（映射里缺项 = 只有当前版本）。
    assert_eq!(recent.len(), 1);
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
        let conn = store.pool().acquire().await.expect("acquire");
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

    let conn = store.pool().acquire().await.expect("acquire 2");
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
