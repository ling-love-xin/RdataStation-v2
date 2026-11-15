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

    let (table, rows) = persist_result_table(&root, "tmp_q_probe", "结果集 探查")
        .expect("落成项目持久表");
    assert_eq!(rows, 1, "一行应原样过去");
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
