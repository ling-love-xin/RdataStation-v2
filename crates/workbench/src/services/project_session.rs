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
//! 2. 全局系统库「最近打开项目」中路径仍存在的第一项；
//! 3. 均不可用 → `None`（相关视图进入空态，不阻塞应用启动）。

use std::path::PathBuf;

use engine::persistence::global_db::GlobalDatabaseManager;

/// 当前项目会话。
#[derive(Debug, Clone)]
pub struct ProjectSession {
    /// 项目根目录（本地路径，即草稿箱根）。
    pub root: PathBuf,
    /// 项目显示名（标题栏 / 面板头）。
    pub name: String,
}

impl ProjectSession {
    fn from_root(root: PathBuf) -> Self {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "未命名项目".to_string());
        Self { root, name }
    }
}

/// 解析当前项目会话（见模块文档的顺序）。
pub fn resolve() -> Option<ProjectSession> {
    from_env().or_else(from_recent_projects)
}

/// 1. 环境变量 `RDS_PROJECT_PATH`。
fn from_env() -> Option<ProjectSession> {
    let raw = std::env::var_os("RDS_PROJECT_PATH")?;
    let path = PathBuf::from(raw);
    if path.is_dir() {
        Some(ProjectSession::from_root(path))
    } else {
        tracing::warn!(
            "[ProjectSession] RDS_PROJECT_PATH 不是有效目录: {}",
            path.display()
        );
        None
    }
}

/// 2. 全局系统库最近打开项目（取前 5 条中路径仍存在的第一条）。
///
/// 单例未初始化时返回 `None`（不在此处降级建库，避免与启动装配重复打开）。
fn from_recent_projects() -> Option<ProjectSession> {
    let manager: &GlobalDatabaseManager = engine::migration::get_global_db_manager()?;
    let runtime = tokio::runtime::Runtime::new().ok()?;
    let records = runtime.block_on(manager.get_recent_projects(5)).ok()?;
    for record in records {
        let path = PathBuf::from(&record.path);
        if path.is_dir() {
            let mut session = ProjectSession::from_root(path);
            if !record.name.trim().is_empty() {
                session.name = record.name;
            }
            return Some(session);
        }
    }
    None
}
