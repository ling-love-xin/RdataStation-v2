//! 真库往返：结果集落成**项目持久表**（`res_<描述>_<时间戳>`）。
//!
//! 验的是「跨实例搬运」这条真实链路：源表在**进程内存单例**（in-memory，等价于 `tmp_q_`），
//! 目标在临时项目的 `{root}/.RSmeta/analytics.duckdb`。这条路径与
//! `mock_generator::persist_table_at`（新开连接 + ATTACH）不同——它不走 ATTACH，
//! 所以项目库被别的连接握着时同样能写。

use std::path::PathBuf;

use engine::services::duckdb_service::DuckDbService;
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

    // 源：内存单例里建一张带类型的表（等于 `tmp_q_` 的处境）
    {
        let singleton = DuckDbService::get_or_create_duckdb().expect("内存单例");
        let conn = singleton.lock().expect("单例锁");
        conn.execute_batch(
            "CREATE OR REPLACE TABLE tmp_q_probe AS \
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

/// 升级闭环（验收第 1 条）：表落进项目分析库**之后**，资产行也登记出来。
///
/// 钉住三件事：显示名不带 `res_` 前缀、`analytics_table` 非空（`resolve` 认得出是表型）、
/// 版本从 1 起——这正是「临时表 / 结果集 → 升级成资产」缺了半个身子的那一段。
#[test]
fn an_upgrade_registers_a_table_kind_asset_row() {
    let root = temp_root("asset");

    // 项目元数据库（资产登记的归处）先按生产口径迁移到位：生产由应用启动做，
    // 测试里必须显式做一次，否则只是个空 SQLite（登记会报 no such table）。
    engine::migration::MigrationManager::new()
        .migrate(
            &root.join(".RSmeta").join("project.db"),
            engine::migration::MigrationType::ProjectMeta,
        )
        .expect("项目元数据库迁移");

    {
        let singleton = DuckDbService::get_or_create_duckdb().expect("内存单例");
        let conn = singleton.lock().expect("单例锁");
        conn.execute_batch(
            "CREATE OR REPLACE TABLE tmp_q_asset AS \
             SELECT CAST(12.3400 AS DECIMAL(10,4)) AS amount, DATE '2026-11-15' AS biz_date",
        )
        .expect("源表");
    }

    let (table, rows, columns) =
        persist_result_table(&root, "tmp_q_asset", "结果 3").expect("落成项目持久表");
    assert_eq!(rows, 1);

    // 登记也自建 runtime 并 block_on（同一个 `#[test]`，不是 async 上下文）
    let runtime = tokio::runtime::Runtime::new().expect("登记用 runtime");
    let asset = runtime
        .block_on(analytics_resource::table_asset::register_at(
            &root,
            analytics_resource::table_asset::TableAssetRequest {
                name: "结果 3".to_string(),
                analytics_table: table.clone(),
                row_count: Some(i32::try_from(rows).unwrap_or(i32::MAX)),
                column_count: columns,
            },
        ))
        .expect("登记成表型资产");

    assert_eq!(asset.version, 1, "首次升级就是 v1");
    assert_eq!(asset.name, "结果 3", "资产名不带 res_ 前缀");

    // 读回来核对档位：`analytics_table` 列才是权威判据
    let pool = std::sync::Arc::new(
        runtime
            .block_on(engine::persistence::ProjectSqlitePool::new(
                root.join(".RSmeta").join("project.db"),
                2,
            ))
            .expect("项目元数据库连接池"),
    );
    let store = analytics_resource::AnalyticsResourceStore::new(pool);
    let row = runtime
        .block_on(store.get_resource_by_id(&asset.resource_id))
        .expect("资产行读得回来");
    assert_eq!(
        analytics_resource::ArchiveKind::resolve(&row.kind, row.analytics_table.as_deref()),
        analytics_resource::ArchiveKind::AnalyticsTable,
        "表型资产（不是文件型）"
    );
    assert_eq!(row.analytics_table.as_deref(), Some(table.as_str()));

    let _ = std::fs::remove_dir_all(&root);
}
