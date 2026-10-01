//! project service 生命周期集成测试：打开/关闭/只读、归档、改名、软删/遗忘/磁盘删除、迁移、版本快照。
//!
//! 与 `project_registry.rs` 的分工：那边覆盖 创建登记/固定置顶/软删隐藏/恢复 的名册主链路；
//! 这里补齐 service 编排层**其余公开契约**——`open/open_read_only/close`、`set_archived`、
//! `update_meta`、`soft_remove/forget/delete_disk`、`relocate`、`create_version/list_versions`、
//! `list_recent/has_recent_available`。
//!
//! 全局库单例按进程只可注入一次，故经 `OnceLock` 注入一份临时全局库；各测试用独立目录与
//! 随机 id，只断言**自己的** id 状态，并行互不干扰。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rds_project::service::{self, CreateProjectInput};
use rds_project::{ProjectLock, ProjectSummary};

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 临时全局库（进程内一次注入）+ 项目落点根目录。
fn base_dir() -> PathBuf {
    BASE_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("rds_svc_it_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let manager = rt
                .block_on(engine::persistence::global_db::GlobalDatabaseManager::new(
                    dir.join("global.db"),
                    dir.join("analytics.duckdb"),
                    2,
                ))
                .expect("init global db");
            engine::migration::install_global_db_manager(manager).expect("inject singleton");
            // 连接池后台任务依托该运行时存活，不可随初始化结束而销毁。
            std::mem::forget(rt);
            dir
        })
        .clone()
}

fn project_path(name: &str) -> PathBuf {
    base_dir().join(name)
}

fn create_named(name: &str, path: &Path) -> ProjectSummary {
    service::create(CreateProjectInput::new(name, path.to_path_buf())).expect("create")
}

/// 探测锁是否被持有（占用中返回 Some）。
fn lock_busy(root: &Path) -> bool {
    ProjectLock::probe(root).map(|o| o.is_some()).unwrap_or(false)
}

/// 1) 打开/关闭/只读：锁的获取与释放、只读不取锁、最近列表与可用性。
#[test]
fn open_close_and_read_only_lock_lifecycle() {
    let root = project_path("proj-open");
    let s = create_named("打开流程项目", &root);

    // 打开 → 持锁。
    let opened = match service::open(&s.path).expect("open") {
        service::OpenOutcome::Opened(o) => o,
        service::OpenOutcome::Busy(_) => panic!("空项目不应被占用"),
    };
    assert_eq!(opened.summary.id, s.id);
    assert!(!opened.read_only, "默认读写打开");
    assert!(opened.lock.is_some(), "读写打开应持写锁");
    assert!(lock_busy(&s.path), "持锁期间应被探测为占用");

    // open 写入 last_opened_at → 应进入最近列表，可用性为真。
    let recent = service::list_recent(10).expect("list_recent");
    assert!(recent.iter().any(|p| p.id == s.id), "打开后应进入最近列表");
    assert!(recent.iter().find(|p| p.id == s.id).is_some_and(|p| p.path_exists));
    assert!(service::has_recent_available(), "有可用项目");

    // 关闭 → 释放锁。
    service::close(opened);
    assert!(!lock_busy(&s.path), "关闭后应释放锁");

    // 可再次打开（锁可重取）。
    let reopened = match service::open(&s.path).expect("reopen") {
        service::OpenOutcome::Opened(o) => o,
        service::OpenOutcome::Busy(_) => panic!("释放后应可重新打开"),
    };
    assert!(lock_busy(&s.path));

    // 只读打开：不取写锁。
    let ro = service::open_read_only(&s.path).expect("open_read_only");
    assert!(ro.read_only);
    assert!(ro.lock.is_none(), "只读打开不应取写锁");
    service::close(ro); // 无锁的 close 应安全
    assert!(lock_busy(&s.path), "只读打开不释放已持有的读写锁");

    service::close(reopened);

    // 打开不存在的路径 / 非项目目录 → 明确报错。
    assert!(service::open(&project_path("proj-open-missing")).is_err());
    let plain = project_path("proj-open-plain");
    std::fs::create_dir_all(&plain).expect("mkdir");
    assert!(service::open(&plain).is_err(), "非项目目录不能打开");
    assert!(service::open_read_only(&plain).is_err());
}

/// 2) 归档往返 + 改名：状态翻转与名册/本体同步。
#[test]
fn archive_roundtrip_and_update_meta() {
    let root = project_path("proj-meta");
    let s = create_named("元数据项目", &root);

    // 归档 → 名册状态 Archived。
    service::set_archived(&s.id, &s.path, true).expect("归档");
    let all = service::list_all().expect("list_all");
    let rec = all.into_iter().find(|p| p.id == s.id).expect("归档后仍在名册");
    assert_eq!(rec.status, "archived");

    // 取消归档 → 回 Active。
    service::set_archived(&s.id, &s.path, false).expect("取消归档");
    let rec = service::list_all()
        .expect("list_all2")
        .into_iter()
        .find(|p| p.id == s.id)
        .expect("记录");
    assert_eq!(rec.status, "active");

    // 改名 + 描述：名册与 project.json 双端同步。
    service::update_meta(&s.id, &s.path, "改名后", Some("新描述")).expect("update_meta");
    let rec = service::list_all()
        .expect("list_all3")
        .into_iter()
        .find(|p| p.id == s.id)
        .expect("记录");
    assert_eq!(rec.name, "改名后");
    assert_eq!(rec.description.as_deref(), Some("新描述"));
    let json = std::fs::read_to_string(s.path.join(".RSmeta").join("project.json"))
        .expect("读 project.json");
    assert!(json.contains("改名后"), "project.json 应同步新名，实际: {json}");

    // 非法名（空）→ Err。
    assert!(service::update_meta(&s.id, &s.path, "", None).is_err());
}

/// 3) 软删 → 遗忘 / 磁盘删除：名册状态与磁盘落点。
#[test]
fn soft_remove_forget_delete_disk() {
    // forget：清名册但不动磁盘。
    let a_root = project_path("proj-softa");
    let a = create_named("遗忘项目", &a_root);
    service::soft_remove(&a.id).expect("soft_remove");
    assert!(service::list_removed().expect("removed").iter().any(|p| p.id == a.id));
    service::forget(&a.id).expect("forget");
    assert!(
        !service::list_removed().expect("removed2").iter().any(|p| p.id == a.id),
        "forget 应清出名册"
    );
    assert!(a.path.exists(), "forget 不应动磁盘");

    // delete_disk：删 .RSmeta + 清名册，保留用户目录。
    let b_root = project_path("proj-softb");
    let b = create_named("磁盘删除项目", &b_root);
    std::fs::write(b_root.join("用户文件.txt"), "保留").expect("写用户文件");
    service::delete_disk(&b.id, &b_root).expect("delete_disk");
    assert!(!b_root.join(".RSmeta").exists(), ".RSmeta 应被删除");
    assert!(b_root.join("用户文件.txt").exists(), "用户文件应保留");
    assert!(
        !service::list_all().expect("all").iter().any(|p| p.id == b.id),
        "delete_disk 应清出名册"
    );
}

/// 4) 迁移：项目文件夹移动后，名册与 project.json 指向新目录。
#[test]
fn relocate_points_to_new_root() {
    let old_root = project_path("proj-reloc");
    let s = create_named("迁移项目", &old_root);

    // 模拟用户把项目文件夹移到新目录（.RSmeta 随之移动）。
    let new_root = project_path("proj-reloc2");
    std::fs::create_dir_all(&new_root).expect("mkdir new");
    std::fs::rename(
        old_root.join(".RSmeta"),
        new_root.join(".RSmeta"),
    )
    .expect("move .RSmeta");

    service::relocate(&s.id, &new_root).expect("relocate");

    // 名册指向新目录。
    let rec = service::list_all()
        .expect("all")
        .into_iter()
        .find(|p| p.id == s.id)
        .expect("记录");
    assert!(
        rec.path.to_string_lossy().contains("proj-reloc2"),
        "名册应指向新目录，实际: {}",
        rec.path.display()
    );
    // project.json 同步。
    let json = std::fs::read_to_string(new_root.join(".RSmeta").join("project.json"))
        .expect("读新 project.json");
    assert!(json.contains("proj-reloc2"), "project.json 应指向新目录，实际: {json}");

    // relocate 到非项目目录 → Err。
    let plain = project_path("proj-reloc-plain");
    std::fs::create_dir_all(&plain).expect("mkdir plain");
    assert!(service::relocate(&s.id, &plain).is_err(), "非项目目录不能作为迁移目标");
}

/// 5) 版本快照：空表 → 空消息拦截 → 写入 → 倒序读取。
#[test]
fn version_snapshot_lifecycle() {
    let root = project_path("proj-ver");
    let s = create_named("版本项目", &root);

    // 新项目版本表为空。
    assert!(service::list_versions(&s.path).expect("空列表").is_empty());

    // 空消息被拦截。
    assert!(service::create_version(&s.path, "   ").is_err(), "空版本说明应报错");

    service::create_version(&s.path, "v1 快照").expect("v1");
    service::create_version(&s.path, "v2 快照").expect("v2");

    let versions = service::list_versions(&s.path).expect("版本列表");
    assert_eq!(versions.len(), 2, "应写入 2 条版本");
    assert_eq!(versions[0].message, "v2 快照", "新的在前（ORDER BY created_at DESC）");
    assert_eq!(versions[1].message, "v1 快照");
    assert!(versions.iter().all(|v| !v.id.is_empty() && !v.created_at.is_empty()));

    // 非项目目录 → 明确报错（project.db 缺失）。
    let plain = project_path("proj-ver-plain");
    std::fs::create_dir_all(&plain).expect("mkdir plain");
    assert!(service::list_versions(&plain).is_err());
    assert!(service::create_version(&plain, "x").is_err());
}
