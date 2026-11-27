//! 所有权收口真机探针：项目 Manager、全局 Manager、分析实例必须共用一条根连接。
//!
//! 2026-11-26 真机事实：
//! - 同一 DuckDB 文件在进程里只能有一条根连接，第二条即使 `READ_ONLY` 也会
//!   `File is already open ... same PID`；
//! - 因此 `ProjectDatabaseManager` / `GlobalDatabaseManager` 不再各自打开文件，
//!   而是从 `shared::duckdb_root` 取 `USE analytics` / `USE rds_global` 的会话克隆；
//! - 分析实例用同一条根连接，跨库 CTAS 直接成立。
//!
//! 本用例替代 2026-11-15 的 `analysis_db_exclusive_probe`：那条用例钉的是
//! “项目主写连接存活时第二个连接 ATTACH 会失败”，现在这条路已经不存在。

use std::path::PathBuf;

use rds_engine::duckdb::federation::session::{
    BaseMount, GLOBAL_BASE_ALIAS, PROJECT_BASE_ALIAS, set_bases,
};
use rds_engine::persistence::global_db::GlobalDatabaseManager;
use rds_engine::persistence::project_db::ProjectDatabaseManager;

fn temp_root(tag: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!(
        "rds_ownership_{tag}_{}_{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("临时项目目录");
    root
}

#[tokio::test(flavor = "multi_thread")]
async fn project_global_and_instance_share_one_root() {
    let root = temp_root("shared");
    let project_db = ProjectDatabaseManager::analysis_db_path(&root);
    let global_sqlite = root.join("global.db");
    let global_duckdb = rds_engine::persistence::global_paths::get_global_duckdb_path()
        .expect("测试数据根下的全局分析库路径");

    // 写模式打开项目：根连接此时挂上 analytics + rds_global。
    let project = ProjectDatabaseManager::open(&root, 2)
        .await
        .expect("项目库写模式打开");

    // 同一全局库再开 Manager：必须复用根连接，不能报第二连接/文件占用。
    let global = GlobalDatabaseManager::new(global_sqlite, global_duckdb.clone(), 2)
        .await
        .expect("全局库打开");

    // 分析实例用同一组底座；它会从根连接复制控制会话。
    let _ = rds_engine::duckdb::instance::drop_instance();
    set_bases(vec![
        BaseMount {
            alias: PROJECT_BASE_ALIAS.to_string(),
            path: project_db.to_string_lossy().into_owned(),
        },
        BaseMount {
            alias: GLOBAL_BASE_ALIAS.to_string(),
            path: global_duckdb.to_string_lossy().into_owned(),
        },
    ]);
    let instance = rds_engine::duckdb::instance::instance().expect("分析实例应能打开");
    let session = instance.process_session().expect("process 会话");

    // 项目 Manager 的句柄写项目库。
    {
        let handle = project.duckdb_conn().expect("项目写句柄");
        let guard = handle.acquire().await.expect("项目句柄");
        let mut slot = guard.lock().await;
        let conn = slot.as_mut().expect("项目连接");
        conn.execute_batch("CREATE TABLE analytics.owner_probe AS SELECT 1 AS id")
            .expect("项目句柄写项目库");
    }

    // 全局 Manager 的句柄写全局库。
    {
        let handle = global.duckdb_conn();
        let guard = handle.acquire().await.expect("全局句柄");
        let mut slot = guard.lock().await;
        let conn = slot.as_mut().expect("全局连接");
        conn.execute_batch("CREATE TABLE rds_global.owner_global AS SELECT 2 AS id")
            .expect("全局句柄写全局库");
    }

    // 实例看得到两边的表，并能同实例跨库 CTAS。
    let result = session
        .run("SELECT count(*) AS n FROM analytics.owner_probe")
        .expect("实例读项目表");
    assert_eq!(result.to_rows()[0][0].to_string(), "1");
    let result = session
        .run("SELECT count(*) AS n FROM rds_global.owner_global")
        .expect("实例读全局表");
    assert_eq!(result.to_rows()[0][0].to_string(), "1");
    session
        .run("CREATE TABLE analytics.owner_copy AS SELECT * FROM rds_global.owner_global")
        .expect("同实例跨库 CTAS");
    let result = session
        .run("SELECT count(*) AS n FROM analytics.owner_copy")
        .expect("读跨库副本");
    assert_eq!(result.to_rows()[0][0].to_string(), "1");

    drop(session);
    let _ = rds_engine::duckdb::instance::drop_instance();
    set_bases(Vec::new());
    drop(project);
    drop(global);
    shared::duckdb_root::reset();
    let _ = std::fs::remove_dir_all(&root);
}
