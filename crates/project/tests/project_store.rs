//! 项目管理（M1）集成测试：项目存储与项目锁。
//!
//! 分层约定：库内单元测试（`src/store.rs` / `src/lock.rs` 的 `mod tests`）覆盖内部细节；
//! 本目录为**集成测试**（`tests/`），只经由 crate 公开 API 验证端到端行为。

use std::path::PathBuf;

use rds_project::service::{load_default_connection, save_default_connection};
use rds_project::{AcquireOutcome, ProjectLock, ProjectManager, ProjectStore};

/// 独立临时目录（按用例名 + pid 隔离，运行前清理）。
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_project_it_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn create_then_load_roundtrip() {
    let root = temp_dir("create_load");

    let store = ProjectStore::create("集成测试项目", &root).expect("create");
    let id = store.info().id.clone();

    // `.RSmeta` 三大件应全部就位。
    assert!(root.join(".RSmeta").join("project.db").exists());
    assert!(root.join(".RSmeta").join("project.json").exists());
    assert!(root.join(".RSmeta").join("analytics.duckdb").exists());

    let loaded = ProjectStore::load(&root).expect("load");
    assert_eq!(loaded.info().id, id);
    assert_eq!(loaded.info().name, "集成测试项目");
    assert!(loaded.info().path.is_local());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn manager_tracks_current_and_recent() {
    let root = temp_dir("manager");

    let mut manager = ProjectManager::new();
    manager.create_project("M", &root).expect("create");
    assert!(manager.current_project().is_some());

    let info = manager
        .current_project()
        .expect("current")
        .lock()
        .expect("lock")
        .info()
        .clone();
    manager.add_recent_project(info);
    assert_eq!(manager.recent_projects().len(), 1);

    manager.close_project();
    assert!(manager.current_project().is_none());

    let _ = std::fs::remove_dir_all(&root);
}

/// U3 默认连接：写在**项目本体**（`.RSmeta/config/settings.json`），空值 = 不设默认。
#[test]
fn default_connection_roundtrip() {
    let root = temp_dir("default_conn");
    ProjectStore::create("默认连接项目", &root).expect("create");

    // 初始态：不设默认（不是错误）
    assert_eq!(load_default_connection(&root).expect("读"), None);

    save_default_connection(&root, Some("G_analytics")).expect("写");
    assert_eq!(
        load_default_connection(&root).expect("读"),
        Some("G_analytics".to_string())
    );
    // 落在项目本体，不进名册
    let raw = std::fs::read_to_string(root.join(".RSmeta").join("config").join("settings.json"))
        .expect("读 settings.json");
    assert!(raw.contains("G_analytics"), "实际：{raw}");

    // 清除默认：写回 null
    save_default_connection(&root, None).expect("清除");
    assert_eq!(load_default_connection(&root).expect("读"), None);

    // 非项目目录：读回 None（当没设默认），写失败（不假装成功）
    let plain = temp_dir("default_conn_plain");
    assert_eq!(load_default_connection(&plain).expect("读"), None);
    assert!(save_default_connection(&plain, Some("G_x")).is_err());

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&plain);
}

#[test]
fn lock_is_exclusive_and_reusable() {
    let root = temp_dir("lock");

    let lock = match ProjectLock::acquire(&root).expect("acquire") {
        AcquireOutcome::Acquired(lock) => lock,
        AcquireOutcome::Busy(_) => panic!("空目录不应被占用"),
    };
    // 持锁期间探测应报告占用。
    assert!(ProjectLock::probe(&root).expect("probe").is_some());

    lock.release().expect("release");
    // 释放后可再次获取。
    assert!(ProjectLock::probe(&root).expect("probe2").is_none());
    match ProjectLock::acquire(&root).expect("re-acquire") {
        AcquireOutcome::Acquired(_) => {}
        AcquireOutcome::Busy(_) => panic!("释放后应可重新获取"),
    }

    let _ = std::fs::remove_dir_all(&root);
}
