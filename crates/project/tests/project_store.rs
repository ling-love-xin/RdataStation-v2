//! 项目管理（M1）集成测试：项目存储与项目锁。
//!
//! 分层约定：库内单元测试（`src/store.rs` / `src/lock.rs` 的 `mod tests`）覆盖内部细节；
//! 本目录为**集成测试**（`tests/`），只经由 crate 公开 API 验证端到端行为。

use std::path::PathBuf;

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
