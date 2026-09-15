//! 项目名册（全局库）端到端集成测试：创建登记 / 固定置顶 / 软删隐藏 / 已移除找回。
//!
//! 与 `project_store.rs` 的分工：那边只覆盖磁盘 `.RSmeta` 与实例锁；这里注入**临时全局库**
//! 单例，经 `service` 公开 API 走完整链路（建 `.RSmeta` + 写名册 + 查询 + 软删 + 恢复）。
//!
//! 全局库单例按进程只可注入一次，故全部用例合并为一个测试，避免并行测试相互干扰。

use std::path::PathBuf;
use std::sync::OnceLock;

use engine::persistence::global_db::GlobalDatabaseManager;
use rds_project::service;

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 临时全局库（进程内一次注入）+ 项目落点根目录。
fn base_dir() -> PathBuf {
    BASE_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("rds_registry_it_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let manager = rt
                .block_on(GlobalDatabaseManager::new(
                    dir.join("global.db"),
                    dir.join("analytics.duckdb"),
                    2,
                ))
                .expect("init global db");
            engine::migration::install_global_db_manager(manager).expect("inject singleton");
            // 连接池的后台任务依托该运行时存活，不可随初始化结束而销毁。
            std::mem::forget(rt);
            dir
        })
        .clone()
}

#[test]
fn registry_create_pin_soft_delete_and_restore() {
    let base = base_dir();

    // 1. 创建两个项目：磁盘 `.RSmeta` 落盘 + 名册登记。
    let a = service::create(service::CreateProjectInput::new(
        "集成项目 A",
        base.join("proj-a"),
    ))
    .expect("创建 A");
    let b = service::create(service::CreateProjectInput::new(
        "集成项目 B",
        base.join("proj-b"),
    ))
    .expect("创建 B");
    assert!(a.path.join(rds_project::service::RS_META_DIR_NAME).is_dir());

    // 2. 两个项目都在全部列表里。
    let all = service::list_all().expect("list_all");
    assert!(all.iter().any(|p| p.id == a.id));
    assert!(all.iter().any(|p| p.id == b.id));
    let removed = service::list_removed().expect("list_removed");
    assert!(removed.is_empty(), "新建项目不应出现在已移除列表");

    // 3. 固定 A → 置顶（SQL 按 `is_pinned DESC, last_opened_at DESC` 排序）。
    service::set_pinned(&a.id, true).expect("固定 A");
    let all = service::list_all().expect("list_all");
    assert_eq!(
        all.first().map(|p| p.id.clone()),
        Some(a.id.clone()),
        "固定项应置顶"
    );
    assert!(
        all.iter()
            .find(|p| p.id == a.id)
            .is_some_and(|p| p.is_pinned)
    );

    // 4. 软删 B → 从名册隐藏、磁盘保留、出现在已移除列表。
    service::soft_remove(&b.id).expect("软删 B");
    let all = service::list_all().expect("list_all");
    assert!(!all.iter().any(|p| p.id == b.id), "软删后应隐藏");
    assert!(
        b.path.exists(),
        "软删只改名册，磁盘目录必须保留：{}",
        b.path.display()
    );
    let removed = service::list_removed().expect("list_removed");
    assert!(removed.iter().any(|p| p.id == b.id), "已移除列表应可找回");

    // 5. 恢复 B → 回到名册、离开已移除列表。
    service::restore(&b.id).expect("恢复 B");
    let all = service::list_all().expect("list_all");
    assert!(all.iter().any(|p| p.id == b.id), "恢复后应回到名册");
    let removed = service::list_removed().expect("list_removed");
    assert!(!removed.iter().any(|p| p.id == b.id));

    // 6. 清理：只删测试自己的目录。
    let _ = std::fs::remove_dir_all(&base);
}
