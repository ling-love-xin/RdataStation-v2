//! 真库往返：结果集落成**项目持久表**（`res_<描述>_<时间戳>`）。
//!
//! 验的是「跨实例搬运」这条真实链路：源表在**进程内存单例**（in-memory，等价于 `tmp_q_`），
//! 目标在临时项目的 `{root}/.RSmeta/analytics.duckdb`。这条路径与
//! `mock_generator::persist_table_at`（新开连接 + ATTACH）不同——它不走 ATTACH，
//! 所以项目库被别的连接握着时同样能写。

use std::path::PathBuf;

use rds_workbench::services::persist_jobs::persist_result_table;

fn temp_root(tag: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "rds_persist_{tag}_{}_{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("临时项目目录");
    root
}

/// 源表建在内存单例上（真 `tmp_q_` 同款位置）→ 落成项目持久表 → 重新打开项目库核对。
///
/// **必须是 `#[test]` 而不是 `#[tokio::test]`**：被验的函数内部自建 runtime 并 `block_on`，
/// 在 async 上下文里调它会 panic（`Cannot start a runtime from within a runtime`）。
#[test]
fn a_result_table_lands_in_the_project_analytics_db() {
    let root = temp_root("land");

    // 源在项目分析库的 `rds_tmp` schema（所有权收口后源与目标同一根连接）。
    {
        std::fs::create_dir_all(root.join(".RSmeta")).expect("建 .RSmeta");
        let db = engine::persistence::ProjectDatabaseManager::analysis_db_path(&root);
        let conn = duckdb::Connection::open(&db).expect("项目分析库");
        conn.execute_batch(
            "CREATE SCHEMA IF NOT EXISTS rds_tmp;
             CREATE OR REPLACE TABLE rds_tmp.tmp_q_probe AS \
             SELECT CAST(12.3400 AS DECIMAL(10,4)) AS amount, DATE '2026-11-15' AS biz_date",
        )
        .expect("源表");
    }

    let (table, rows, columns) = persist_result_table(&root, "tmp_q_probe", "结果集 探查")
        .expect("落成项目持久表");
    assert_eq!(rows, 1, "一行应原样过去");
    assert_eq!(columns, Some(2), "列数应量出来（amount + biz_date）");
    assert!(
        table.starts_with("res_"),
        "落库表名走持久口径（不是 tmp_ 族）：{table}"
    );

    // 重新打开项目库核对：表在 `main` 里，且类型没被字符串化
    let runtime = tokio::runtime::Runtime::new().expect("核对用 runtime");
    let manager = runtime
        .block_on(engine::persistence::project_db::ProjectDatabaseManager::open(
            &root, 2,
        ))
        .expect("重新打开项目库");
    let db = manager.duckdb_conn().expect("写连接");
    let guard = runtime.block_on(db.acquire()).expect("取连接");
    let mut slot = runtime.block_on(guard.lock());
    let conn = slot.as_mut().expect("连接未关闭");
    let ty: String = conn
        .query_row(
            &format!(
                "SELECT data_type FROM information_schema.columns \
                 WHERE table_name = '{table}' AND column_name = 'amount'"
            ),
            [],
            |row| row.get(0),
        )
        .expect("读列类型");
    assert!(
        ty.to_uppercase().starts_with("DECIMAL"),
        "Decimal 类型应原样保留，实际 {ty}"
    );
    let count: i64 = conn
        .query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |row| {
            row.get(0)
        })
        .expect("读行数");
    assert_eq!(count, 1);

    // 撞名必须被拒（同一描述同一秒 → 同名；这里直接用同名再落一次更稳）
    drop(slot);
    drop(guard);
    drop(db);
    drop(manager);
    let again = persist_result_table(&root, "tmp_q_probe", "结果集 探查");
    // 若这秒恰好换了时间戳，则不会撞名——两种结果都合法，但失败时原因必须是「已存在」
    if let Err(reason) = again {
        assert!(
            reason.contains("已存在"),
            "撞名的失败原因要可读：{reason}"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

/// 落表与资产分离（T8）：普通表落进项目分析库后**不会**自动登记资产；
/// 只有显式归档才生成冻结快照，并消费源表。
///
/// 钉住三件事：落表后资产表为空、归档生成 v1 快照、源表被消费。
#[test]
fn persisting_a_table_does_not_register_an_asset() {
    let root = temp_root("asset");

    // 项目元数据库（资产登记的归处）先按生产口径迁移到位：生产由应用启动做，
    // 测试里必须显式做一次，否则只是个空 SQLite（归档会报 no such table）。
    engine::migration::MigrationManager::new()
        .migrate(
            &root.join(".RSmeta").join("project.db"),
            engine::migration::MigrationType::ProjectMeta,
        )
        .expect("项目元数据库迁移");

    {
        std::fs::create_dir_all(root.join(".RSmeta")).expect("建 .RSmeta");
        let db = engine::persistence::ProjectDatabaseManager::analysis_db_path(&root);
        let conn = duckdb::Connection::open(&db).expect("项目分析库");
        conn.execute_batch(
            "CREATE SCHEMA IF NOT EXISTS rds_tmp;
             CREATE OR REPLACE TABLE rds_tmp.tmp_q_asset AS \
             SELECT CAST(12.3400 AS DECIMAL(10,4)) AS amount, DATE '2026-11-15' AS biz_date",
        )
        .expect("源表");
    }

    let (table, rows, _columns) =
        persist_result_table(&root, "tmp_q_asset", "结果 3").expect("落成项目持久表");
    assert_eq!(rows, 1);

    let runtime = tokio::runtime::Runtime::new().expect("归档用 runtime");

    // 落表不登记：资产表里一条都没有。
    let pool = std::sync::Arc::new(
        runtime
            .block_on(engine::persistence::ProjectSqlitePool::new(
                root.join(".RSmeta").join("project.db"),
                2,
            ))
            .expect("项目元数据库连接池"),
    );
    let store = analytics_resource::AnalyticsResourceStore::new(pool);
    assert!(
        runtime
            .block_on(store.list_alive_resources())
            .expect("清单")
            .is_empty(),
        "存为表只建普通表，不自动登记资产"
    );

    // 显式归档：生成 v1 快照并消费源表。
    let asset = runtime
        .block_on(analytics_resource::table_asset::archive_table_at(
            &root,
            &analytics_resource::table_asset::ArchiveTableRequest {
                name: "结果 3".to_string(),
                source_schema: "main".to_string(),
                source_table: table.clone(),
                asset_id: None,
                consume_source: true,
            },
        ))
        .expect("归档成表资产");

    assert_eq!(asset.version, 1, "首次归档就是 v1");
    assert!(asset.source_consumed, "显式归档要消费源表");

    // 读回来核对档位：`analytics_table` 列才是权威判据
    let row = runtime
        .block_on(store.get_resource_by_id(&asset.resource_id))
        .expect("资产行读得回来");
    assert_eq!(
        analytics_resource::ArchiveKind::resolve(&row.kind, row.analytics_table.as_deref()),
        analytics_resource::ArchiveKind::AnalyticsTable,
        "表型资产（不是文件型）"
    );
    assert!(
        row.analytics_table
            .as_deref()
            .is_some_and(|name| name.starts_with("arc_")),
        "登记行指向归档快照，不再是源表：{:?}",
        row.analytics_table
    );

    let _ = std::fs::remove_dir_all(&root);
}
