//! 服务层用例分组。

use super::*;

/// 新表资产模型下最小可用的登记行：一条 `analytics_resources` + 一份冻结快照索引。
///
/// 回收站只处理登记行，不要求真正归档过 DuckDB，所以快照物理名由调用方给。
async fn seed_table_asset(
    service: &ArchiveService,
    name: &str,
    physical_table: &str,
) -> crate::AnalyticsResource {
    use crate::model::{ArchiveBinding, ArchiveKind, NewArchiveInput, PayloadMode};
    use crate::store::table_asset::{NewTableSnapshot, SnapshotInstance};
    use crate::table_asset::manifest::{
        DataFingerprint, TableColumn, TableManifest, TableSchema,
    };

    let input = NewArchiveInput {
        resource_type: "analysis".to_string(),
        name: name.to_string(),
        alias: None,
        kind: ArchiveKind::AnalyticsTable,
        content_hash: "seed".to_string(),
        definition_sql: None,
        row_count: Some(1),
        column_count: Some(1),
        file_rel_path: String::new(),
        payload_mode: PayloadMode::Copy,
        source_path: None,
        file_size: None,
        structured: true,
        binding: ArchiveBinding {
            promoted_from: None,
            source_connection_id: None,
            source_table: Some(physical_table.to_string()),
        },
        scope: "project".to_string(),
    };
    let schema = TableSchema::new(vec![TableColumn::new("id", "INTEGER", true, None, 0)])
        .expect("schema");
    let data = DataFingerprint::new(1, 1, "1", "1", Some("1".to_string()), Some("1".to_string()));
    let manifest = TableManifest::build(&schema, &data).expect("manifest");
    let snapshot = NewTableSnapshot {
        snapshot_id: format!("snap_{physical_table}"),
        instance: SnapshotInstance::Project,
        physical_schema: "rds_archive".to_string(),
        physical_table: physical_table.to_string(),
    };
    service
        .store()
        .insert_table_asset_with_version(input, physical_table, &manifest, &snapshot, None)
        .await
        .expect("登记表资产")
}

/// 还原要重新过显示名唯一：先删后建的同名资产合法，旧条目复活时不能再造第二条。
#[tokio::test]
async fn t198_restore_refuses_a_name_taken_after_trash() {
    let (service, dir) = test_service().await;
    let first = write_draft(&dir, "first.sql", b"select 1;").await;
    let mut req = archive_req(&first, "reports/first.sql", None);
    req.name = "共同报表".to_string();
    let old = service.archive(req).await.expect("archive old");
    let trashed = service
        .move_to_trash(&[old.resource_id.clone()])
        .await
        .expect("trash old");

    // 旧条目软删后显示名重新可用：新资产用同一个名字是合法的。
    let second = write_draft(&dir, "second.sql", b"select 2;").await;
    let mut req = archive_req(&second, "reports/second.sql", None);
    req.name = "共同报表".to_string();
    let new = service.archive(req).await.expect("archive new with reused name");

    let error = service
        .restore_archive_from_trash(&trashed[0].trash_id)
        .await
        .expect_err("同名存活资产存在时必须拒绝还原");
    assert!(
        format!("{error}").contains("占用"),
        "错误要说清显示名被谁占用：{error}"
    );
    assert!(
        !service
            .payload()
            .resolve("reports/first.sql")
            .expect("resolve old path")
            .exists(),
        "拒绝必须发生在搬回本体之前"
    );
    assert_eq!(
        service
            .store()
            .alive_by_name("共同报表")
            .await
            .expect("alive by name")
            .len(),
        1,
        "不能出现第二条同名存活资产"
    );
    assert_eq!(new.version, 1, "新资产要正常落 v1");

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t199_table_restore_refuses_a_name_taken_after_trash() {
    let (service, dir) = test_service().await;
    let old = seed_table_asset(&service, "占用表", "arc_old").await;
    service
        .store()
        .soft_delete_archive(&old.id)
        .await
        .expect("soft delete old");
    let new = seed_table_asset(&service, "占用表", "arc_new").await;

    let trash_id = format!("{}{}", crate::model::TRASH_ID_TABLE_PREFIX, old.id);
    let error = service
        .restore_archive_from_trash(&trash_id)
        .await
        .expect_err("表型还原也要过显示名闸门");
    assert!(
        format!("{error}").contains("占用"),
        "错误要说清显示名被谁占用：{error}"
    );
    assert_eq!(
        service
            .store()
            .alive_by_name("占用表")
            .await
            .expect("alive by name")
            .len(),
        1,
        "不能出现第二条同名存活资产"
    );
    assert_eq!(new.name, "占用表");

    let _ = fs::remove_dir_all(&dir).await;
}

/// 移入回收站：本体离开 `resources/` 进入项目级回收站，索引行**软删**（列表与
/// “按路径查存活行”都看不到，但记录本身还在，还原才能恢复别名 / 标签 / 指纹）。
/// 永久删除与清空：都**只动本模块的**条目（回收站是共用的，删别人的东西不是清空）；
/// 登记行与它的标签 / 分组归属一并清掉（外键没有 `ON DELETE CASCADE`，不清就删不掉）。
/// 文件型资产直接进回收站（软删登记 + 本体搬走），还原后登记与 manifest 都回来。
#[tokio::test]
async fn t129_file_asset_trashes_and_restores_without_auto_unlock() {
    let (service, dir) = test_service().await;
    let first = write_draft(&dir, "locked.sql", b"select 1;").await;
    let archived = service
        .archive(archive_req(&first, "reports/locked.sql", None))
        .await
        .expect("archive v1");

    let second = write_draft(&dir, "locked-v2.sql", b"select 2;").await;
    service
        .archive(archive_req(
            &second,
            "reports/locked.sql",
            Some(&archived.resource_id),
        ))
        .await
        .expect("archive v2");
    assert_eq!(
        service
            .store()
            .list_file_asset_versions(&archived.resource_id)
            .await
            .expect("file versions")
            .len(),
        2,
        "v1 / v2 都应有 manifest"
    );

    let trashed = service
        .move_to_trash(&[archived.resource_id.clone()])
        .await
        .expect("文件资产直接进回收站");
    assert!(
        trashed.iter().any(|entry| entry.resource_id == archived.resource_id),
        "回收站回执要包含这条文件资产"
    );
    assert!(
        !service
            .payload()
            .resolve("reports/locked.sql")
            .expect("resolve")
            .exists(),
        "本体已离开 resources"
    );

    let restored = service
        .restore_archive_from_trash(&trashed[0].trash_id)
        .await
        .expect("restore file asset");
    assert_eq!(restored.id, archived.resource_id);
    assert!(
        service
            .payload()
            .resolve("reports/locked.sql")
            .expect("resolve")
            .exists(),
        "还原后本体回到 resources"
    );
    assert_eq!(
        service
            .store()
            .list_file_asset_versions(&archived.resource_id)
            .await
            .expect("file versions")
            .len(),
        2,
        "回收站不动 manifest"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn t158_purge_clears_versions_semantics_refs_but_keeps_history() {
    let (service, dir) = test_service().await;

    let draft = write_draft(&dir, "a.sql", b"select 1;").await;
    let first = service
        .archive(archive_req(&draft, "a.sql", None))
        .await
        .expect("archive");
    // 再归档一次：写前快照会往 versions 表落一行（带外键）。
    let draft2 = write_draft(&dir, "a.sql", b"select 2;").await;
    service
        .archive(archive_req(&draft2, "a.sql", Some(&first.resource_id)))
        .await
        .expect("re-archive");
    assert!(
        !service
            .store()
            .get_resource_versions(&first.resource_id)
            .await
            .expect("versions")
            .is_empty(),
        "前置条件：要有一条版本快照行"
    );

    service
        .set_assertion(&first.resource_id, crate::model::ArchiveAssertion {
            metric_name: Some("日活".to_string()),
            ..Default::default()
        })
        .await
        .expect("assertion");
    service
        .set_lifecycle(
            &first.resource_id,
            crate::model::Lifecycle::Superseded,
            Some("ar_new"),
            None,
        )
        .await
        .expect("lifecycle");
    assert!(
        !service
            .store()
            .get_refs(&first.resource_id)
            .await
            .expect("refs")
            .is_empty(),
        "前置条件：要有一条谱系"
    );

    service
        .store()
        .soft_delete_archive(&first.resource_id)
        .await
        .expect("soft delete");
    service
        .store()
        .purge_deleted_row(&first.resource_id)
        .await
        .expect("永久删除不应被外键拦下");

    assert!(
        service
            .store()
            .get_resource_by_id(&first.resource_id)
            .await
            .is_err(),
        "登记行应已删除"
    );
    assert!(service
        .store()
        .get_resource_versions(&first.resource_id)
        .await
        .expect("versions")
        .is_empty());
    assert!(service
        .store()
        .get_refs(&first.resource_id)
        .await
        .expect("refs")
        .is_empty());
    assert!(
        !service
            .store()
            .all_assertions()
            .await
            .expect("assertions")
            .contains_key(&first.resource_id),
        "断言不应留下孤儿"
    );
    assert!(
        !service
            .store()
            .get_resource_events(&first.resource_id)
            .await
            .expect("events")
            .is_empty(),
        "履历刻意保留：它要回答「这条结论曾经存在过」"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

/// 回收站出入要留履历：移入记 `Trashed`、还原记 `Untrashed`。
///
/// （2026-11-27：文件型资产不再有“解锁退回草稿箱”这一步，删除即进回收站；
/// 文件型语义由 `t129_file_asset_trashes_and_restores_without_auto_unlock` 钉住，
/// 表型回收站语义由 `table_asset_trash_cycles_the_registry_only` 覆盖。）
///
/// 少了这两条，「进过回收站又捞回来」在时间线上完全看不见——用户只看到归档与最终
/// 状态，中间那次危险动作消失。事件不回滚主动作（见 `record_timeline`），所以这里
/// 断言的是「动作之后履历里有它」，而不是两者同事务。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn table_asset_trash_cycles_the_registry_only() {
    let (service, dir) = test_service().await;
    let registered = seed_table_asset(&service, "订单宽表", "arc_t_order_wide").await;

    // 还原前预检要看「快照物理表还在」：先把本体建到归档 schema。
    let analytics_db = engine::persistence::ProjectDatabaseManager::analysis_db_path(&dir);
    std::fs::create_dir_all(dir.join(".RSmeta")).expect("建 .RSmeta");
    {
        let conn = duckdb::Connection::open(&analytics_db).expect("建项目分析库");
        conn.execute_batch(
            "CREATE SCHEMA IF NOT EXISTS rds_archive;
             CREATE TABLE rds_archive.arc_t_order_wide (id INTEGER);",
        )
        .expect("建表型快照");
    }

    let entry = service
        .move_to_trash(&[registered.id.clone()])
        .await
        .expect("表型应能移入回收站（只软删登记）")
        .remove(0);
    assert!(
        entry.trash_id.starts_with(crate::model::TRASH_ID_TABLE_PREFIX),
        "表型条目要用 tbl: 前缀，实际：{}",
        entry.trash_id
    );
    let soft = service
        .store()
        .get_resource_by_id(&registered.id)
        .await
        .expect("软删行仍可读");
    assert!(soft.deleted_at.is_some(), "登记行应软删");
    assert_eq!(
        soft.analytics_table.as_deref(),
        Some("arc_t_order_wide"),
        "表名不应随登记软删而丢失"
    );

    let listed = service
        .deleted_registry_archives()
        .await
        .expect("回收站清单第三来源");
    assert!(
        listed.iter().any(|r| r.id == registered.id),
        "表型软删行必须出现在回收站清单里"
    );

    let restored = service
        .restore_archive_from_trash(&entry.trash_id)
        .await
        .expect("还原表型登记");
    assert!(restored.deleted_at.is_none(), "还原后登记行复活");
    assert_eq!(
        restored.analytics_table.as_deref(),
        Some("arc_t_order_wide"),
        "还原本体指向不变"
    );

    let again = service
        .move_to_trash(&[registered.id.clone()])
        .await
        .expect("再次移入回收站")
        .remove(0);
    service
        .purge_archive(&again.trash_id)
        .await
        .expect("永久删除表型登记");
    assert!(
        service
            .store()
            .get_resource_by_id(&registered.id)
            .await
            .is_err(),
        "永久删除后登记行应彻底消失"
    );

    let _ = fs::remove_dir_all(&dir).await;
}

/// 表型登记还在回收站，但用户已经把分析库里的表手工 DROP：还原必须当场拒绝，
/// 而不是复活一条指向空处的 Missing 行。
#[tokio::test]
async fn table_restore_refuses_when_analytics_table_is_gone() {
    let (service, dir) = test_service().await;
    let registered = seed_table_asset(&service, "已丢的表", "arc_t_gone").await;

    // 项目分析库存在，但归档 schema 里没有 `arc_t_gone`。
    let analytics_db = engine::persistence::ProjectDatabaseManager::analysis_db_path(&dir);
    std::fs::create_dir_all(dir.join(".RSmeta")).expect("建 .RSmeta");
    {
        let conn = duckdb::Connection::open(&analytics_db).expect("建项目分析库");
        conn.execute_batch(
            "CREATE SCHEMA IF NOT EXISTS rds_archive;
             CREATE TABLE rds_archive.unrelated (id INTEGER);",
        )
        .expect("建无关表");
    }

    let entry = service
        .move_to_trash(&[registered.id.clone()])
        .await
        .expect("表型移入回收站")
        .remove(0);
    let error = service
        .restore_archive_from_trash(&entry.trash_id)
        .await
        .expect_err("表已不在，还原必须被拒");
    let message = format!("{error}");
    assert!(
        message.contains("已经没有表") && message.contains("arc_t_gone"),
        "错误要说清哪张表没了，实际：{message}"
    );
    assert!(
        service
            .store()
            .get_resource_by_id(&registered.id)
            .await
            .expect("软删行仍可读")
            .deleted_at
            .is_some(),
        "拒绝还原后登记行必须仍在回收站"
    );

    // 收掉这条登记（永久删除只清登记，不动分析库）。
    service
        .purge_archive(&entry.trash_id)
        .await
        .expect("永久删除表型登记");
    let _ = fs::remove_dir_all(&dir).await;
}
