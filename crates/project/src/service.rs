//! 项目服务（M1）：项目增删改查与生命周期编排。
//!
//! 统一收口「项目名册」（全局系统库 `project_info`）与「项目本体」（`.RSmeta`），
//! 供项目管理 UI（选择器 / 新建对话框 / 设置视图）调用。语义见
//! `docs/architecture/project/project-prototype-design.md` §2。
//!
//! 按 GPUI-kit 编码指南「feature crate 组织同一业务能力的 model/service/view/command/dialog」，
//! 本模块与 `models` / `store` / `lock` 同属 `rds-project`；同步入口 + 内部自建 tokio 运行时。

use std::path::{Path, PathBuf};

use engine::migration::{MigrationManager, MigrationType};
use engine::persistence::global_db::{GlobalDatabaseManager, ProjectInfoRecord};

use crate::store::check_project_missing_drivers;
use crate::{AcquireOutcome, LockInfo, ProjectLock, ProjectStatus, ProjectStore};

/// 项目根下的内部元数据目录（统一来源：`store::RS_META_DIR_NAME`）。
pub use crate::store::RS_META_DIR_NAME;

/// 项目名册条目（视图模型：名册记录 + 运行时探测结果）。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub path: PathBuf,
    pub status: String,
    pub is_pinned: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_opened_at: Option<String>,
    /// 磁盘上路径是否仍存在。
    pub path_exists: bool,
    /// 是否已被其他实例占用（`probe` 结果）。
    pub lock: Option<LockInfo>,
    /// 缺失驱动名列表（仅最近列表 / 打开时计算）。
    pub missing_drivers: Vec<String>,
}

/// 新建项目输入。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CreateProjectInput {
    pub name: String,
    /// 项目根目录（父目录 + 名称由对话框拼好）。
    pub path: PathBuf,
    pub description: Option<String>,
    /// 是否初始化分析库（`analytics.duckdb`）——恒真于 `ProjectStore::create`，保留开关位。
    pub init_analytics: bool,
    /// 是否创建示例草稿。
    pub sample_drafts: bool,
}

impl CreateProjectInput {
    /// 以必填项构造（其余字段走 builder，保证后续新增字段不破坏调用方）。
    pub fn new(name: impl Into<String>, path: PathBuf) -> Self {
        Self {
            name: name.into(),
            path,
            description: None,
            init_analytics: true,
            sample_drafts: false,
        }
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_sample_drafts(mut self, sample_drafts: bool) -> Self {
        self.sample_drafts = sample_drafts;
        self
    }
}

/// 目标目录状态（新建项目时的冲突判定）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetDirState {
    /// 目录不存在（可直接创建）。
    Missing,
    /// 目录存在但为空（可创建）。
    Empty,
    /// 目录已是有效项目（应改为打开）。
    ExistingProject,
    /// 目录存在且非空且非项目（阻止创建）。
    NonEmpty,
}

/// 打开项目的结果。
#[non_exhaustive]
pub struct OpenedProject {
    pub store: ProjectStore,
    /// 写锁句柄（只读打开时为 `None`）；`Drop` 即释放。
    pub lock: Option<ProjectLock>,
    pub read_only: bool,
    pub summary: ProjectSummary,
}

impl OpenedProject {
    /// 拆解为各部件（`#[non_exhaustive]` 下跨 crate 无法字面解构时的取用方式）。
    pub fn into_parts(self) -> (ProjectStore, Option<ProjectLock>, bool, ProjectSummary) {
        (self.store, self.lock, self.read_only, self.summary)
    }
}

/// 打开项目的三种结果（供「只读打开 / 仍要打开 / 取消」逃生口使用）。
pub enum OpenOutcome {
    Opened(OpenedProject),
    /// 已被其他实例占用，携带占用者信息。
    Busy(LockInfo),
}

/// 服务入口：要求全局系统库已初始化。
fn manager() -> Result<&'static GlobalDatabaseManager, String> {
    engine::migration::get_global_db_manager()
        .ok_or_else(|| "项目服务未就绪：全局系统库未初始化".to_string())
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Runtime::new().map_err(|e| format!("无法启动异步运行时: {e}"))
}

/// 校验项目名称（对话框与重命名共用）。
pub fn validate_project_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("项目名称不能为空".to_string());
    }
    if n.chars().count() > 64 {
        return Err("项目名称不能超过 64 个字符".to_string());
    }
    if n.chars().any(|c| "\\/:*?\"<>|".contains(c)) {
        return Err("项目名称不能包含 \\ / : * ? \" < > |".to_string());
    }
    Ok(())
}

/// 判断目录是否是有效项目（含 `.RSmeta`，兼容旧 `.rdata-station` 结构）。
pub fn is_valid_project(path: &Path) -> bool {
    path.join(RS_META_DIR_NAME).exists() || path.join(".rdata-station").exists()
}

/// 探测目标目录状态。
pub fn inspect_target(path: &Path) -> TargetDirState {
    if !path.exists() {
        return TargetDirState::Missing;
    }
    if is_valid_project(path) {
        return TargetDirState::ExistingProject;
    }
    match std::fs::read_dir(path) {
        Ok(mut entries) => {
            if entries.next().is_none() {
                TargetDirState::Empty
            } else {
                TargetDirState::NonEmpty
            }
        }
        Err(_) => TargetDirState::NonEmpty,
    }
}

// ==================== Read ====================

/// 最近项目（按固定 + 最后打开倒序）。
pub fn list_recent(limit: usize) -> Result<Vec<ProjectSummary>, String> {
    let manager = manager()?;
    let runtime = runtime()?;
    let records = runtime
        .block_on(manager.get_recent_projects(limit))
        .map_err(|e| format!("查询最近项目失败: {e}"))?;
    Ok(records.into_iter().map(|r| summarize(r, true)).collect())
}

/// 全部项目（按固定 + 最后打开倒序）。
pub fn list_all() -> Result<Vec<ProjectSummary>, String> {
    let manager = manager()?;
    let runtime = runtime()?;
    let records = runtime
        .block_on(manager.get_all_projects())
        .map_err(|e| format!("查询项目列表失败: {e}"))?;
    Ok(records.into_iter().map(|r| summarize(r, false)).collect())
}

/// 已移除（软删）项目。
pub fn list_removed() -> Result<Vec<ProjectSummary>, String> {
    let manager = manager()?;
    let runtime = runtime()?;
    let records = runtime
        .block_on(manager.list_removed_projects())
        .map_err(|e| format!("查询已移除项目失败: {e}"))?;
    Ok(records.into_iter().map(|r| summarize(r, false)).collect())
}

/// 名册记录 → 视图模型（`with_drivers` 控制是否做缺失驱动自检，列表页仅最近项计算）。
fn summarize(record: ProjectInfoRecord, with_drivers: bool) -> ProjectSummary {
    let path = PathBuf::from(&record.path);
    let path_exists = path.is_dir();
    let lock = if path_exists {
        ProjectLock::probe(&path).ok().flatten()
    } else {
        None
    };
    let missing_drivers = if with_drivers && path_exists {
        missing_driver_names(&path)
    } else {
        Vec::new()
    };
    ProjectSummary {
        id: record.id,
        name: record.name,
        description: record.description,
        path,
        status: record.status,
        is_pinned: record.is_pinned,
        created_at: record.created_at,
        updated_at: record.updated_at,
        last_opened_at: record.last_opened_at,
        path_exists,
        lock,
        missing_drivers,
    }
}

fn missing_driver_names(path: &Path) -> Vec<String> {
    let Ok(runtime) = runtime() else {
        return Vec::new();
    };
    match runtime.block_on(check_project_missing_drivers(path)) {
        Ok(list) => list.into_iter().map(|d| d.driver_name).collect(),
        Err(_) => Vec::new(),
    }
}

// ==================== Create ====================

/// 新建项目：建 `.RSmeta` → 迁移 project.db / analytics.duckdb → 登记名册 → 驱动自检。
pub fn create(input: CreateProjectInput) -> Result<ProjectSummary, String> {
    validate_project_name(&input.name)?;

    match inspect_target(&input.path) {
        TargetDirState::ExistingProject => {
            return Err("该目录已是项目，请直接打开".to_string());
        }
        TargetDirState::NonEmpty => {
            return Err("目录非空，请选择空目录".to_string());
        }
        TargetDirState::Missing | TargetDirState::Empty => {}
    }

    let name = input.name.trim().to_string();
    let store =
        ProjectStore::create(&name, &input.path).map_err(|e| format!("创建项目失败: {e}"))?;
    if input.description.is_some() {
        // ProjectStore::create 后补写描述（保持单一写入入口）。
        // 失败不阻断创建，仅记录。
        let mut store = store;
        let desc = input.description.clone();
        if let Err(e) = store.update_info(|info| info.description = desc.clone()) {
            tracing::warn!("写入项目描述失败: {e}");
        }
        return register_and_summarize(store.info());
    }
    register_and_summarize(store.info())
}

/// 登记名册（upsert）并返回视图模型。
fn register_and_summarize(info: &crate::ProjectInfo) -> Result<ProjectSummary, String> {
    let manager = manager()?;
    let runtime = runtime()?;
    let path_str = info
        .path
        .local_path()
        .map(|p| p.to_string_lossy().to_string());
    let Some(path_str) = path_str else {
        return Err("项目路径无效".to_string());
    };
    runtime
        .block_on(manager.save_project_info_smart(
            &info.id,
            &info.name,
            info.description.as_deref(),
            &path_str,
            "active",
            None,
        ))
        .map_err(|e| format!("登记项目失败: {e}"))?;

    let records = runtime
        .block_on(manager.get_all_projects())
        .map_err(|e| format!("回读项目失败: {e}"))?;
    records
        .into_iter()
        .find(|r| r.id == info.id)
        .map(|r| summarize(r, true))
        .ok_or_else(|| "项目登记后未找到记录".to_string())
}

// ==================== Open / Close ====================

/// 打开项目：校验结构 → 抢项目锁 → 载入 → 更新最后打开时间。
pub fn open(root: &Path) -> Result<OpenOutcome, String> {
    if !root.is_dir() {
        return Err(format!("项目路径不存在: {}", root.display()));
    }
    if !is_valid_project(root) {
        return Err(format!(
            "项目结构不完整（缺少 {}）: {}",
            RS_META_DIR_NAME,
            root.display()
        ));
    }

    // 抢锁：被占用则交回调用方决定（只读 / 仍要 / 取消）。
    match ProjectLock::acquire(root).map_err(|e| format!("获取项目锁失败: {e}"))? {
        AcquireOutcome::Busy(info) => return Ok(OpenOutcome::Busy(info)),
        AcquireOutcome::Acquired(lock) => {
            let opened = load_and_register(root, Some(lock), false)?;
            Ok(OpenOutcome::Opened(opened))
        }
    }
}

/// 只读打开：不获取写锁（逃生口「只读打开」）。
pub fn open_read_only(root: &Path) -> Result<OpenedProject, String> {
    if !is_valid_project(root) {
        return Err(format!("项目结构不完整: {}", root.display()));
    }
    load_and_register(root, None, true)
}

/// 载入项目并登记/更新名册（打开路径的公共部分）。
fn load_and_register(
    root: &Path,
    lock: Option<ProjectLock>,
    read_only: bool,
) -> Result<OpenedProject, String> {
    let mut store = ProjectStore::load(root).map_err(|e| format!("加载项目失败: {e}"))?;
    // 只读模式下不写 project.json。
    if !read_only {
        if let Err(e) = store.update_info(|info| {
            info.last_opened_at = Some(chrono::Utc::now());
        }) {
            tracing::warn!("更新项目最后打开时间失败: {e}");
        }
    }

    let info = store.info().clone();
    let manager = manager()?;
    let runtime = runtime()?;
    let path_str = root.to_string_lossy().to_string();
    let existing = runtime
        .block_on(manager.open_project_by_path(&path_str))
        .map_err(|e| format!("更新项目打开记录失败: {e}"))?;
    if existing.is_none() {
        runtime
            .block_on(manager.save_project_info_smart(
                &info.id,
                &info.name,
                info.description.as_deref(),
                &path_str,
                "active",
                None,
            ))
            .map_err(|e| format!("登记项目失败: {e}"))?;
        // 二次打开以写入 last_opened_at，使其进入最近列表。
        let _ = runtime.block_on(manager.open_project_by_path(&path_str));
    }

    let records = runtime
        .block_on(manager.get_all_projects())
        .map_err(|e| format!("回读项目失败: {e}"))?;
    let summary = records
        .into_iter()
        .find(|r| r.id == info.id)
        .map(|r| summarize(r, true))
        .ok_or_else(|| "项目登记后未找到记录".to_string())?;

    Ok(OpenedProject {
        store,
        lock,
        read_only,
        summary,
    })
}

/// 关闭项目：释放锁（`ProjectLock::release`）。
pub fn close(opened: OpenedProject) {
    if let Some(lock) = opened.lock {
        let _ = lock.release();
    }
}

// ==================== Update ====================

/// 重命名（仅显示名 + 描述）：同步名册与项目 `project.json`。
pub fn update_meta(
    id: &str,
    root: &Path,
    name: &str,
    description: Option<&str>,
) -> Result<(), String> {
    validate_project_name(name)?;
    let manager = manager()?;
    let runtime = runtime()?;
    runtime
        .block_on(manager.update_project_info(id, name.trim(), description))
        .map_err(|e| format!("更新项目信息失败: {e}"))?;

    // 同步项目本体（project.json / project.db）；失败仅告警，不阻断名册更新。
    if is_valid_project(root) {
        match ProjectStore::load(root) {
            Ok(mut store) => {
                let name = name.trim().to_string();
                let desc = description.map(|s| s.to_string());
                if let Err(e) = store.update_info(|info| {
                    info.name = name.clone();
                    info.description = desc.clone();
                }) {
                    tracing::warn!("同步项目 project.json 失败: {e}");
                }
            }
            Err(e) => tracing::warn!("加载项目以同步重命名失败: {e}"),
        }
    }
    Ok(())
}

/// 固定 / 取消固定。
pub fn set_pinned(id: &str, pinned: bool) -> Result<(), String> {
    let manager = manager()?;
    let runtime = runtime()?;
    runtime
        .block_on(manager.set_project_pinned(id, pinned))
        .map_err(|e| format!("更新固定状态失败: {e}"))
}

/// 归档 / 取消归档（`ProjectStatus::Active ↔ Archived`）。
pub fn set_archived(id: &str, root: &Path, archived: bool) -> Result<(), String> {
    let status = if archived { "archived" } else { "active" };
    let manager = manager()?;
    let runtime = runtime()?;
    let path_str = root.to_string_lossy().to_string();
    // 复用 upsert，仅改状态，保留固定/软删列。
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    runtime
        .block_on(manager.save_project_info_smart(&id, &name, None, &path_str, status, None))
        .map_err(|e| format!("更新归档状态失败: {e}"))?;
    Ok(())
}

// ==================== Delete ====================

/// 软删：移出名册（磁盘保留，可经 `restore` 找回）。
pub fn soft_remove(id: &str) -> Result<(), String> {
    let manager = manager()?;
    let runtime = runtime()?;
    runtime
        .block_on(manager.soft_delete_project(id))
        .map_err(|e| format!("移除项目失败: {e}"))
}

/// 恢复被软删的项目。
pub fn restore(id: &str) -> Result<(), String> {
    let manager = manager()?;
    let runtime = runtime()?;
    runtime
        .block_on(manager.restore_project(id))
        .map_err(|e| format!("恢复项目失败: {e}"))
}

/// 硬删：删除项目内部元数据 `.RSmeta`（保留用户文件），并清理名册登记。
///
/// 删除前应由 UI 完成「输入项目名」二次确认；调用方须保证项目已关闭（锁已释放）。
pub fn delete_disk(id: &str, root: &Path) -> Result<(), String> {
    let meta_dir = root.join(RS_META_DIR_NAME);
    if meta_dir.exists() {
        std::fs::remove_dir_all(&meta_dir)
            .map_err(|e| format!("删除项目元数据失败（{}）: {e}", meta_dir.display()))?;
    }
    let manager = manager()?;
    let runtime = runtime()?;
    runtime
        .block_on(manager.delete_project_info(id))
        .map_err(|e| format!("清理项目登记失败: {e}"))?;
    Ok(())
}

/// 移除名册登记但不动磁盘（用于路径已失效的项目「移出列表」）。
pub fn forget(id: &str) -> Result<(), String> {
    let manager = manager()?;
    let runtime = runtime()?;
    runtime
        .block_on(manager.delete_project_info(id))
        .map_err(|e| format!("移出列表失败: {e}"))
}

/// 会话解析用：是否已有可用项目（供 `project_session` 收敛）。
pub fn has_recent_available() -> bool {
    list_recent(1)
        .map(|v| v.iter().any(|p| p.path_exists))
        .unwrap_or(false)
}

/// 重新定位：旧路径失效时，把项目指到新目录（同步名册与 `project.json`）。
pub fn relocate(id: &str, new_root: &Path) -> Result<(), String> {
    if !is_valid_project(new_root) {
        return Err(format!(
            "目标目录不是有效项目（缺少 {}）: {}",
            RS_META_DIR_NAME,
            new_root.display()
        ));
    }
    // 同步项目本体路径（project.json / project.db）。
    let mut store = ProjectStore::load(new_root).map_err(|e| format!("加载项目失败: {e}"))?;
    store
        .update_info(|info| info.path = crate::ProjectPath::local(new_root))
        .map_err(|e| format!("写入项目路径失败: {e}"))?;
    let info = store.info().clone();
    let status = status_key(info.status);

    let manager = manager()?;
    let runtime = runtime()?;
    let path_str = new_root.to_string_lossy().to_string();
    runtime
        .block_on(manager.save_project_info_smart(
            id,
            &info.name,
            info.description.as_deref(),
            &path_str,
            status,
            None,
        ))
        .map_err(|e| format!("更新名册路径失败: {e}"))?;
    Ok(())
}

/// `ProjectStatus` → 名册存储字符串。
fn status_key(status: ProjectStatus) -> &'static str {
    match status {
        ProjectStatus::Active => "active",
        ProjectStatus::Archived => "archived",
        ProjectStatus::Syncing => "syncing",
        ProjectStatus::Offline => "offline",
    }
}

/// 项目版本台账行（只读展示）。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ProjectVersionRow {
    pub id: String,
    pub parent_id: Option<String>,
    pub message: String,
    pub created_by: Option<String>,
    pub created_at: String,
}

/// 确保项目元数据库存在且迁移到最新（项目版本表可能需要补齐）。
fn ensure_project_meta(root: &Path) -> Result<std::path::PathBuf, String> {
    let db_path = root.join(RS_META_DIR_NAME).join("project.db");
    if !db_path.exists() {
        return Err(format!("项目元数据库不存在: {}", db_path.display()));
    }
    MigrationManager::new()
        .migrate(&db_path, MigrationType::ProjectMeta)
        .map_err(|e| format!("项目迁移失败: {e}"))?;
    Ok(db_path)
}

/// 列出项目版本（新的在前）。
pub fn list_versions(root: &Path) -> Result<Vec<ProjectVersionRow>, String> {
    let db_path = ensure_project_meta(root)?;
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| format!("打开项目库失败: {e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT id, parent_id, message, created_by, created_at \
             FROM project_versions ORDER BY created_at DESC",
        )
        .map_err(|e| format!("查询版本失败: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ProjectVersionRow {
                id: r.get(0)?,
                parent_id: r.get(1).ok(),
                message: r.get(2)?,
                created_by: r.get(3).ok(),
                created_at: r.get(4)?,
            })
        })
        .map_err(|e| format!("读取版本失败: {e}"))?;
    Ok(rows.flatten().collect())
}

/// 创建版本快照（手动记录）。
pub fn create_version(root: &Path, message: &str) -> Result<(), String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("版本说明不能为空".to_string());
    }
    let db_path = ensure_project_meta(root)?;
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| format!("打开项目库失败: {e}"))?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO project_versions (id, parent_id, message, created_by, created_at) \
         VALUES (?1, NULL, ?2, NULL, ?3)",
        rusqlite::params![id, message, now],
    )
    .map_err(|e| format!("写入版本失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_name_rules() {
        assert!(validate_project_name("营销分析").is_ok());
        assert!(validate_project_name("  ").is_err());
        assert!(validate_project_name("a/b").is_err());
        assert!(validate_project_name("a:b").is_err());
        assert!(validate_project_name(&"x".repeat(65)).is_err());
        assert!(validate_project_name(&"x".repeat(64)).is_ok());
    }

    #[test]
    fn inspect_target_states() {
        let base = std::env::temp_dir().join(format!("rds_proj_svc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);

        // 不存在 → Missing
        assert_eq!(inspect_target(&base), TargetDirState::Missing);

        // 空目录 → Empty
        std::fs::create_dir_all(&base).expect("mkdir");
        assert_eq!(inspect_target(&base), TargetDirState::Empty);

        // 含 .RSmeta → ExistingProject
        std::fs::create_dir_all(base.join(RS_META_DIR_NAME)).expect("mkdir meta");
        assert_eq!(inspect_target(&base), TargetDirState::ExistingProject);
        assert!(is_valid_project(&base));

        // 非空非项目 → NonEmpty
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("mkdir2");
        std::fs::write(base.join("a.txt"), "x").expect("write");
        assert_eq!(inspect_target(&base), TargetDirState::NonEmpty);

        let _ = std::fs::remove_dir_all(&base);
    }
}
