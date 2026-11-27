//! 当前项目会话（P0）
//!
//! 「每一个应用实例即一个项目」——工作台启动时确定**当前项目根**，供以下消费方共用，
//! 消除各处手填项目路径的路径分裂：
//! - 草稿箱（M5）：草稿箱根 = 项目目录，代码见 `rds-scratchpad`；
//! - 数据源连接（M3）：项目作用域连接 / `GP_` 快照的动作以本项目根为落点；
//! - 标题栏：项目名取自本会话。
//!
//! 解析顺序（前者优先）：
//! 1. 环境变量 `RDS_PROJECT_PATH`（启动参数注入 / 开发调试）；
//! 2. 设置 `startup.restore_last_workspace` 为真时，取「最近打开项目」中
//!    **路径仍是有效项目**（`project::service::is_valid_project`）的第一项；
//! 3. 其余一律 `None` → 空态：中央欢迎页（项目选择器），由用户自己点项目进来。
//!
//! 2026-11-27 口径：默认（设置未打开）启动**先进欢迎页**，不再自动接管「最近项目」；
//! 恢复上次项目变成显式开关。目录存在但 `.RSmeta` 缺失的半坏项目一律跳过。

use std::path::PathBuf;

use engine::persistence::global_db::GlobalDatabaseManager;
use gpui_kit::App;
use project::ui::OpenProject;

/// 解析当前项目会话（见模块文档的顺序）。
pub fn resolve(cx: &App) -> Option<OpenProject> {
    if let Some(project) = from_env() {
        return Some(project);
    }
    if !settings::SettingsService::startup_restore_last_workspace(cx) {
        // 默认路径：空态 → 欢迎页。最近项目列表本身照旧可点（`refresh_picker`）。
        return None;
    }
    from_recent_projects()
}

/// 1. 环境变量 `RDS_PROJECT_PATH`。
fn from_env() -> Option<OpenProject> {
    let raw = std::env::var_os("RDS_PROJECT_PATH")?;
    let path = PathBuf::from(raw);
    if path.is_dir() {
        Some(OpenProject::from_root(path))
    } else {
        tracing::warn!(
            "[ProjectSession] RDS_PROJECT_PATH 不是有效目录: {}",
            path.display()
        );
        None
    }
}

/// 2. 全局系统库最近打开项目（取前 5 条中**仍是有效项目**的第一条）。
///
/// 「有效」= 目录存在 **且** `is_valid_project`（`.RSmeta` / `.rdata-station` 在）；
/// 只判 `is_dir` 会把 DuckDB 已损坏、元数据被删的半坏项目也接管过来（并取写锁）。
/// 单例未初始化时返回 `None`（不在此处降级建库，避免与启动装配重复打开）。
fn from_recent_projects() -> Option<OpenProject> {
    let manager: &GlobalDatabaseManager = engine::migration::get_global_db_manager()?;
    let runtime = tokio::runtime::Runtime::new().ok()?;
    let records = runtime.block_on(manager.get_recent_projects(5)).ok()?;
    for record in records {
        let path = PathBuf::from(&record.path);
        if !(path.is_dir() && project::service::is_valid_project(&path)) {
            tracing::debug!(
                "[ProjectSession] 跳过无效的最近项目: {}",
                path.display()
            );
            continue;
        }
        let mut project = OpenProject::from_root(path);
        if !record.name.trim().is_empty() {
            project.name = record.name;
        }
        return Some(project);
    }
    None
}
