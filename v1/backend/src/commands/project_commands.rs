//! 项目相关命令
//!
//! 处理项目的创建、加载、配置管理等操作

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;
use tokio::sync::{Mutex, RwLock};

use crate::core::error::CoreError;
use crate::core::persistence::project_store::{
    ProjectStore, SqlHistoryRecord, StoredConnection, WorkbenchState,
};
use crate::core::project::{
    ProjectConfig, ProjectInfo, ProjectPath, ProjectStore as CoreProjectStore,
};
use crate::core::services::driver_service::MissingDriver;

/// 项目存储状态
pub struct ProjectState {
    pub store: Arc<Mutex<Option<ProjectStore>>>,
}

impl Default for ProjectState {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectState {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(None)),
        }
    }
}

/// 最近项目缓存
struct RecentProjectsCache {
    cache: RwLock<Option<(Vec<ProjectInfoResponse>, std::time::Instant)>>,
    ttl: std::time::Duration,
}

impl RecentProjectsCache {
    fn new(ttl_secs: u64) -> Self {
        Self {
            cache: RwLock::new(None),
            ttl: std::time::Duration::from_secs(ttl_secs),
        }
    }

    async fn get(&self) -> Option<Vec<ProjectInfoResponse>> {
        let guard = self.cache.read().await;
        if let Some((data, timestamp)) = guard.as_ref() {
            if timestamp.elapsed() < self.ttl {
                return Some(data.clone());
            }
        }
        None
    }

    async fn set(&self, data: Vec<ProjectInfoResponse>) {
        let mut guard = self.cache.write().await;
        *guard = Some((data, std::time::Instant::now()));
    }

    async fn invalidate(&self) {
        let mut guard = self.cache.write().await;
        *guard = None;
    }
}

fn get_recent_projects_cache() -> &'static RecentProjectsCache {
    use std::sync::OnceLock;
    static CACHE: OnceLock<RecentProjectsCache> = OnceLock::new();
    CACHE.get_or_init(|| RecentProjectsCache::new(30))
}

// ==================== Project Commands ====================

/// 项目错误类型
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("项目不存在: {0}")]
    NotFound(String),

    #[error("项目路径无效: {0}")]
    InvalidPath(String),

    #[error("项目结构不完整: {0}")]
    InvalidStructure(String),

    #[error("路径冲突: {0}")]
    PathConflict(String),

    #[error("数据库错误: {0}")]
    Database(String),

    #[error("IO 错误: {0}")]
    Io(String),

    #[error("操作失败: {0}")]
    OperationFailed(String),
}

impl ProjectError {
    /// 获取错误代码（用于前端识别）
    pub fn code(&self) -> &'static str {
        match self {
            ProjectError::NotFound(_) => "PROJECT_NOT_FOUND",
            ProjectError::InvalidPath(_) => "PROJECT_INVALID_PATH",
            ProjectError::InvalidStructure(_) => "PROJECT_INVALID_STRUCTURE",
            ProjectError::PathConflict(_) => "PROJECT_PATH_CONFLICT",
            ProjectError::Database(_) => "PROJECT_DATABASE_ERROR",
            ProjectError::Io(_) => "PROJECT_IO_ERROR",
            ProjectError::OperationFailed(_) => "PROJECT_OPERATION_FAILED",
        }
    }
}

impl From<ProjectError> for String {
    fn from(err: ProjectError) -> Self {
        format!("[{}] {}", err.code(), err)
    }
}

/// 项目信息响应
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, specta::Type)]
pub struct ProjectInfoResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub path: ProjectPathResponse,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_opened_at: Option<String>,
    pub version: String,
    pub missing_drivers: Vec<MissingDriver>,
}

/// 项目路径响应
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, specta::Type)]
#[serde(tag = "type")]
pub enum ProjectPathResponse {
    Local { path: String },
    Remote { url: String, project_id: String },
}

impl From<ProjectInfo> for ProjectInfoResponse {
    fn from(info: ProjectInfo) -> Self {
        Self {
            id: info.id,
            name: info.name,
            description: info.description,
            path: match info.path {
                ProjectPath::Local { path } => ProjectPathResponse::Local {
                    path: path.display().to_string(),
                },
                ProjectPath::Remote { url, project_id } => {
                    ProjectPathResponse::Remote { url, project_id }
                }
            },
            status: match info.status {
                crate::core::project::ProjectStatus::Active => "active".to_string(),
                crate::core::project::ProjectStatus::Archived => "archived".to_string(),
                crate::core::project::ProjectStatus::Syncing => "syncing".to_string(),
                crate::core::project::ProjectStatus::Offline => "offline".to_string(),
            },
            created_at: info.created_at.to_rfc3339(),
            updated_at: info.updated_at.to_rfc3339(),
            last_opened_at: info.last_opened_at.map(|t| t.to_rfc3339()),
            version: info.version_count.to_string(),
            missing_drivers: Vec::new(),
        }
    }
}

/// 从 ProjectInfoRecord 转换为 ProjectInfoResponse
impl From<crate::core::persistence::global_db::ProjectInfoRecord> for ProjectInfoResponse {
    fn from(record: crate::core::persistence::global_db::ProjectInfoRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            description: record.description,
            path: ProjectPathResponse::Local { path: record.path },
            status: record.status,
            created_at: record.created_at,
            updated_at: record.updated_at,
            last_opened_at: record.last_opened_at,
            version: "1".to_string(),
            missing_drivers: Vec::new(),
        }
    }
}

/// 从 ProjectInfo 构建响应（使用指定路径）
fn build_project_response(info: &ProjectInfo, actual_path: String) -> ProjectInfoResponse {
    ProjectInfoResponse {
        id: info.id.clone(),
        name: info.name.clone(),
        description: info.description.clone(),
        path: ProjectPathResponse::Local { path: actual_path },
        status: "active".to_string(),
        created_at: info.created_at.to_rfc3339(),
        updated_at: info.updated_at.to_rfc3339(),
        last_opened_at: info.last_opened_at.map(|t| t.to_rfc3339()),
        version: info.version_count.to_string(),
        missing_drivers: Vec::new(),
    }
}

/// 创建项目请求
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct CreateProjectInput {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
}

/// 创建项目响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct CreateProjectResponse {
    pub project: ProjectInfoResponse,
}

/// 创建新项目
#[tauri::command]
#[specta::specta]
pub async fn create_project(input: CreateProjectInput) -> Result<CreateProjectResponse, CoreError> {
    let path = PathBuf::from(&input.path);

    // 检查目录是否已存在
    if path.exists() {
        // 如果目录已存在，尝试加载现有项目
        match CoreProjectStore::load(&path) {
            Ok(store) => {
                return Ok(CreateProjectResponse {
                    project: store.info().clone().into(),
                });
            }
            Err(_) => {
                // 目录存在但不是有效项目，创建新项目
                std::fs::create_dir_all(&path)
                    .map_err(|e| CoreError::from(format!("创建目录失败: {}", e)))?;
            }
        }
    }

    let store =
        CoreProjectStore::create(&input.name, &path).map_err(|e| CoreError::from(e.to_string()))?;

    seed_default_drivers_for_project(&path).await;

    Ok(CreateProjectResponse {
        project: store.info().clone().into(),
    })
}

/// 获取项目配置
#[tauri::command]
#[specta::specta]
pub async fn get_project_config(path: String) -> Result<ProjectConfig, CoreError> {
    let path = PathBuf::from(path);

    let mut store = CoreProjectStore::load(&path).map_err(|e| CoreError::from(e.to_string()))?;

    store
        .load_config()
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 更新项目配置请求
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct UpdateProjectConfigInput {
    pub path: String,
    pub config: ProjectConfig,
}

/// 更新项目配置
#[tauri::command]
#[specta::specta]
pub async fn update_project_config(input: UpdateProjectConfigInput) -> Result<(), CoreError> {
    tracing::info!(
        path = %input.path,
        "Updating project configuration"
    );

    let path = PathBuf::from(&input.path);

    // 加载项目
    let _store = CoreProjectStore::load(&path).map_err(|e| {
        CoreError::from(ProjectError::OperationFailed(format!("加载项目失败: {}", e)).to_string())
    })?;

    // 更新 project.json 文件
    let project_json_path = path.join(".RSmeta").join("project.json");
    let config_json = serde_json::to_string_pretty(&input.config).map_err(|e| {
        CoreError::from(ProjectError::OperationFailed(format!("序列化配置失败: {}", e)).to_string())
    })?;

    std::fs::write(&project_json_path, config_json).map_err(|e| {
        CoreError::from(ProjectError::Io(format!("写入配置文件失败: {}", e)).to_string())
    })?;

    // 使缓存失效
    get_recent_projects_cache().invalidate().await;

    tracing::info!(path = %input.path, "Project configuration updated successfully");

    Ok(())
}

/// 获取最近项目列表
#[tauri::command]
#[specta::specta]
pub async fn get_recent_projects(
    limit: Option<usize>,
) -> Result<Vec<ProjectInfoResponse>, CoreError> {
    let limit = limit.unwrap_or(10);

    // 尝试从缓存获取
    if let Some(cached) = get_recent_projects_cache().get().await {
        // 如果缓存中的数据量足够，直接返回
        if cached.len() >= limit {
            return Ok(cached.into_iter().take(limit).collect());
        }
    }

    // 缓存未命中或数据不足，从数据库查询
    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    let projects = global_db.get_recent_projects(limit).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("获取最近项目失败: {}", e)).to_string())
    })?;

    let response: Vec<ProjectInfoResponse> = projects.into_iter().map(|p| p.into()).collect();

    // 更新缓存
    get_recent_projects_cache().set(response.clone()).await;

    Ok(response)
}

/// 打开项目（更新最后打开时间并返回项目信息）
#[tauri::command]
#[specta::specta]
pub async fn open_project_by_id(id: String) -> Result<ProjectInfoResponse, CoreError> {
    tracing::info!(project_id = %id, "Opening project by ID");
    let start = std::time::Instant::now();

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    // 使用单次数据库操作完成更新和查询
    let project = global_db
        .open_project(&id)
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("打开项目失败: {}", e)).to_string())
        })?
        .ok_or_else(|| CoreError::from(ProjectError::NotFound(id.clone()).to_string()))?;

    let project_path_for_check = project.path.clone();

    let duration = start.elapsed();
    tracing::info!(
        project_id = %id,
        duration_ms = duration.as_millis(),
        "Project opened successfully"
    );

    let mut response: ProjectInfoResponse = project.into();

    let missing_drivers = match crate::core::project::store::check_project_missing_drivers(
        std::path::Path::new(&project_path_for_check),
    )
    .await
    {
        Ok(drivers) => drivers,
        Err(e) => {
            tracing::warn!("驱动自检失败: {}", e);
            Vec::new()
        }
    };
    response.missing_drivers = missing_drivers;

    Ok(response)
}

/// 根据路径打开项目
#[tauri::command]
#[specta::specta]
pub async fn open_project_by_path(path: String) -> Result<ProjectInfoResponse, CoreError> {
    tracing::info!(path = %path, "Opening project by path");
    let start = std::time::Instant::now();

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    // 尝试使用单次操作打开项目
    let project = global_db.open_project_by_path(&path).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("打开项目失败: {}", e)).to_string())
    })?;

    match project {
        Some(p) => {
            let project_path_for_check = p.path.clone();
            let duration = start.elapsed();
            tracing::info!(
                path = %path,
                project_id = %p.id,
                duration_ms = duration.as_millis(),
                "Project opened successfully"
            );
            let mut response: ProjectInfoResponse = p.into();
            let missing_drivers = match crate::core::project::store::check_project_missing_drivers(
                std::path::Path::new(&project_path_for_check),
            )
            .await
            {
                Ok(drivers) => drivers,
                Err(e) => {
                    tracing::warn!("驱动自检失败: {}", e);
                    Vec::new()
                }
            };
            response.missing_drivers = missing_drivers;
            crate::core::insight::load_user_rules(&std::path::PathBuf::from(&path));
            Ok(response)
        }
        None => {
            // 项目不存在，尝试从路径加载并创建
            tracing::info!(path = %path, "Project not found in database, loading from filesystem");

            let path_buf = std::path::PathBuf::from(&path);

            // 验证路径是否存在
            if !path_buf.exists() {
                return Err(CoreError::from(
                    ProjectError::InvalidPath(format!("项目路径不存在: {}", path)).to_string(),
                ));
            }

            // 验证项目结构完整性
            let meta_dir = path_buf.join(".RSmeta");
            if !meta_dir.exists() {
                // 尝试迁移旧结构
                let old_meta_dir = path_buf.join(".rdata-station");
                if !old_meta_dir.exists() {
                    return Err(CoreError::from(
                        ProjectError::InvalidStructure(format!(
                            "项目结构不完整，缺少 .RSmeta 目录: {}",
                            path
                        ))
                        .to_string(),
                    ));
                }
            }

            let store = crate::core::project::ProjectStore::load(&path_buf).map_err(|e| {
                CoreError::from(
                    ProjectError::OperationFailed(format!("加载项目失败: {}", e)).to_string(),
                )
            })?;

            let info = store.info();
            let now = chrono::Utc::now().to_rfc3339();

            // 使用用户选择的实际路径（而非 project.json 中的路径）
            let actual_path = path.clone();

            tracing::info!(
                project_id = %info.id,
                original_path = ?info.path,
                actual_path = %actual_path,
                "Loading project from filesystem with actual path"
            );

            // 保存到全局数据库（使用智能保存处理路径冲突）
            global_db
                .save_project_info_smart(
                    &info.id,
                    &info.name,
                    info.description.as_deref(),
                    &actual_path,
                    "active",
                    Some(&now),
                )
                .await
                .map_err(|e| {
                    CoreError::from(
                        ProjectError::Database(format!("保存项目信息失败: {}", e)).to_string(),
                    )
                })?;

            let duration = start.elapsed();
            tracing::info!(
                path = %path,
                project_id = %info.id,
                duration_ms = duration.as_millis(),
                "Project loaded from filesystem and saved to database"
            );

            crate::core::insight::load_user_rules(&path_buf);
            let mut response = build_project_response(info, actual_path);
            let missing_drivers = match crate::core::project::store::check_project_missing_drivers(
                std::path::Path::new(&path),
            )
            .await
            {
                Ok(drivers) => drivers,
                Err(e) => {
                    tracing::warn!("驱动自检失败: {}", e);
                    Vec::new()
                }
            };
            response.missing_drivers = missing_drivers;
            Ok(response)
        }
    }
}

/// 创建并保存项目到全局数据库
#[tauri::command]
#[specta::specta]
pub async fn create_and_save_project(
    input: CreateProjectInput,
) -> Result<ProjectInfoResponse, CoreError> {
    tracing::info!(name = %input.name, path = %input.path, "Creating new project");
    let start = std::time::Instant::now();

    let path = std::path::PathBuf::from(&input.path);

    // 如果路径已存在，检查是否是有效项目
    if path.exists() {
        // 尝试加载现有项目
        match crate::core::project::ProjectStore::load(&path) {
            Ok(store) => {
                // 是有效项目，返回现有项目信息
                let info = store.info();
                let now = chrono::Utc::now().to_rfc3339();

                // 更新全局数据库中的最后打开时间
                let global_db =
                    crate::core::migration::get_global_db_manager().ok_or_else(|| {
                        CoreError::from(
                            ProjectError::OperationFailed("全局数据库未初始化".to_string())
                                .to_string(),
                        )
                    })?;

                global_db
                    .save_project_info(
                        &info.id,
                        &info.name,
                        info.description.as_deref(),
                        &input.path,
                        "active",
                        Some(&now),
                    )
                    .await
                    .map_err(|e| {
                        CoreError::from(
                            ProjectError::Database(format!("保存项目信息失败: {}", e)).to_string(),
                        )
                    })?;

                let duration = start.elapsed();
                tracing::info!(
                    name = %input.name,
                    project_id = %info.id,
                    duration_ms = duration.as_millis(),
                    "Project already exists, returning existing project"
                );

                return Ok(build_project_response(info, input.path));
            }
            Err(_) => {
                // 路径存在但不是有效项目，检查目录是否为空
                let is_empty = path
                    .read_dir()
                    .map(|mut entries| entries.next().is_none())
                    .unwrap_or(false);

                if !is_empty {
                    // 目录非空，报错
                    return Err(CoreError::from(
                        ProjectError::PathConflict(format!(
                            "路径已存在且非空，请选择空目录: {}",
                            input.path
                        ))
                        .to_string(),
                    ));
                }
                // 目录为空，继续创建新项目
                tracing::info!(path = %input.path, "Path exists but empty, creating new project");
            }
        }
    }

    // 创建新项目
    let store = crate::core::project::ProjectStore::create(&input.name, &path).map_err(|e| {
        CoreError::from(ProjectError::OperationFailed(format!("创建项目失败: {}", e)).to_string())
    })?;

    let info = store.info();
    let now = chrono::Utc::now().to_rfc3339();

    // 保存到全局数据库
    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    global_db
        .save_project_info(
            &info.id,
            &info.name,
            info.description.as_deref(),
            &input.path,
            "active",
            Some(&now),
        )
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("保存项目信息失败: {}", e)).to_string())
        })?;

    seed_default_drivers_for_project(&path).await;

    let duration = start.elapsed();
    tracing::info!(
        name = %input.name,
        project_id = %info.id,
        duration_ms = duration.as_millis(),
        "Project created successfully"
    );

    Ok(build_project_response(info, input.path))
}

/// 添加到最近项目（更新最后打开时间）
#[tauri::command]
#[specta::specta]
pub async fn add_recent_project(project_id: String) -> Result<(), CoreError> {
    tracing::debug!(project_id = %project_id, "Adding project to recent list");

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    let now = chrono::Utc::now().to_rfc3339();
    global_db
        .update_project_last_opened(&project_id, &now)
        .await
        .map_err(|e| {
            CoreError::from(
                ProjectError::Database(format!("更新项目最后打开时间失败: {}", e)).to_string(),
            )
        })?;

    // 使缓存失效
    get_recent_projects_cache().invalidate().await;

    Ok(())
}

/// 验证项目有效性（检查路径和项目结构）
#[tauri::command]
#[specta::specta]
pub async fn validate_project(project_id: String) -> Result<bool, CoreError> {
    tracing::debug!(project_id = %project_id, "Validating project");

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    // 获取项目信息
    let project = global_db
        .get_project_by_id(&project_id)
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("获取项目信息失败: {}", e)).to_string())
        })?
        .ok_or_else(|| CoreError::from(ProjectError::NotFound(project_id.clone()).to_string()))?;

    let path_buf = std::path::PathBuf::from(&project.path);

    // 检查路径是否存在
    if !path_buf.exists() {
        tracing::warn!(
            project_id = %project_id,
            path = %project.path,
            "Project path does not exist"
        );
        return Ok(false);
    }

    // 检查项目结构
    let meta_dir = path_buf.join(".RSmeta");
    if !meta_dir.exists() {
        let old_meta_dir = path_buf.join(".rdata-station");
        if !old_meta_dir.exists() {
            tracing::warn!(
                project_id = %project_id,
                path = %project.path,
                "Project metadata directory not found"
            );
            return Ok(false);
        }
    }

    tracing::debug!(project_id = %project_id, "Project validation passed");
    Ok(true)
}

/// 删除项目（从全局数据库中移除）
#[tauri::command]
#[specta::specta]
pub async fn delete_project(project_id: String) -> Result<(), CoreError> {
    tracing::info!(project_id = %project_id, "Deleting project from global database");

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    global_db.delete_project(&project_id).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("删除项目失败: {}", e)).to_string())
    })?;

    // 使缓存失效
    get_recent_projects_cache().invalidate().await;

    tracing::info!(project_id = %project_id, "Project deleted successfully");

    Ok(())
}

/// 更新项目信息（名称、描述）
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct UpdateProjectInput {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn update_project(input: UpdateProjectInput) -> Result<(), CoreError> {
    tracing::info!(project_id = %input.id, name = %input.name, "Updating project info");

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    global_db
        .update_project_info(&input.id, &input.name, input.description.as_deref())
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("更新项目信息失败: {}", e)).to_string())
        })?;

    // 使缓存失效
    get_recent_projects_cache().invalidate().await;

    tracing::info!(project_id = %input.id, "Project updated successfully");

    Ok(())
}

// ==================== Project Store Commands ====================

/// 初始化项目存储
#[tauri::command]
pub async fn init_project_store(
    project_path: String,
    state: State<'_, ProjectState>,
) -> Result<(), CoreError> {
    let new_path = PathBuf::from(&project_path);

    // 获取锁并保持到初始化完成，防止并发调用
    let mut guard = state.store.lock().await;

    // 检查是否已经初始化了相同路径
    if let Some(store) = guard.as_ref() {
        let current_path = store.db_manager.project_path();
        if current_path == &new_path {
            tracing::info!(path = %project_path, "Project store already initialized for this path");
            return Ok(());
        }
    }

    // 关闭旧存储
    if let Some(store) = guard.take() {
        let _old_path = store.db_manager.project_path().clone();
        tracing::info!(path = %project_path, "Closing previous project store");
        if let Err(e) = store.db_manager.close().await {
            tracing::warn!(error = %e, "Failed to close project store");
        }

        drop(store);
    }

    // 等待文件句柄释放
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // 创建新存储（锁仍然持有，防止并发调用）
    let store = ProjectStore::new(&new_path).await.map_err(|e| {
        tracing::error!(error = %e, path = %project_path, "Failed to initialize project store");
        e.to_string()
    })?;

    *guard = Some(store);

    tracing::info!(path = %project_path, "Project store initialized successfully");

    // 加载项目启用的插件
    if let Some(global_db) = crate::core::migration::get_global_db_manager() {
        let plugin_service = crate::core::services::plugin_service::PluginService::new(global_db);
        let db_manager = match guard.as_ref() {
            Some(store) => store.db_manager.clone(),
            None => {
                tracing::warn!("Project store not initialized after set, skipping plugin load");
                return Ok(());
            }
        };
        let project_conn_store =
            crate::core::persistence::project_connection_store::ProjectConnectionStore::new(
                db_manager,
            );

        if let Ok(project_plugins) = plugin_service
            .load_project_plugins_on_open(&project_conn_store)
            .await
        {
            tracing::info!(
                path = %project_path,
                plugin_count = project_plugins.len(),
                "Project plugins loaded on open"
            );
            // TODO: 实际上加载这些插件到 PluginManager
        }
    }

    Ok(())
}

/// 关闭项目存储
#[tauri::command]
pub async fn close_project_store(state: State<'_, ProjectState>) -> Result<(), CoreError> {
    let mut guard = state.store.lock().await;
    if let Some(store) = guard.take() {
        let _project_path = store.db_manager.project_path().clone();

        if let Err(e) = store.db_manager.close().await {
            tracing::warn!(error = %e, "Failed to close project store");
        }
    }
    Ok(())
}

/// 保存连接信息到项目存储
#[tauri::command]
pub async fn save_project_store_connection(
    connection: StoredConnection,
    state: State<'_, ProjectState>,
) -> Result<(), CoreError> {
    let guard = state.store.lock().await;
    let store = guard.as_ref().ok_or_else(|| {
        CoreError::from(
            ProjectError::OperationFailed(
                "项目存储未初始化，请先调用 init_project_store".to_string(),
            )
            .to_string(),
        )
    })?;
    store.save_connection(&connection).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("保存连接失败: {}", e)).to_string())
    })
}

/// 获取项目存储中的所有连接
#[tauri::command]
pub async fn get_project_store_connections(
    state: State<'_, ProjectState>,
) -> Result<Vec<StoredConnection>, CoreError> {
    let guard = state.store.lock().await;
    let store = guard.as_ref().ok_or_else(|| {
        CoreError::from(
            ProjectError::OperationFailed(
                "项目存储未初始化，请先调用 init_project_store".to_string(),
            )
            .to_string(),
        )
    })?;
    store.get_connections().await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("获取连接列表失败: {}", e)).to_string())
    })
}

/// 获取项目存储中的单个连接
#[tauri::command]
pub async fn get_project_store_connection(
    id: String,
    state: State<'_, ProjectState>,
) -> Result<Option<StoredConnection>, CoreError> {
    let guard = state.store.lock().await;
    let store = guard.as_ref().ok_or_else(|| {
        CoreError::from(
            ProjectError::OperationFailed(
                "项目存储未初始化，请先调用 init_project_store".to_string(),
            )
            .to_string(),
        )
    })?;
    store.get_connection(&id).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("获取连接失败: {}", e)).to_string())
    })
}

/// 删除项目存储中的连接
#[tauri::command]
pub async fn delete_project_store_connection(
    id: String,
    state: State<'_, ProjectState>,
) -> Result<(), CoreError> {
    let guard = state.store.lock().await;
    let store = guard.as_ref().ok_or_else(|| {
        CoreError::from(
            ProjectError::OperationFailed(
                "项目存储未初始化，请先调用 init_project_store".to_string(),
            )
            .to_string(),
        )
    })?;
    store.delete_connection(&id).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("删除连接失败: {}", e)).to_string())
    })
}

/// 保存 SQL 历史到项目存储
#[tauri::command]
pub async fn save_project_store_sql_history(
    record: SqlHistoryRecord,
    state: State<'_, ProjectState>,
) -> Result<(), CoreError> {
    let guard = state.store.lock().await;
    let store = guard.as_ref().ok_or_else(|| {
        CoreError::from(
            ProjectError::OperationFailed(
                "项目存储未初始化，请先调用 init_project_store".to_string(),
            )
            .to_string(),
        )
    })?;
    store.save_sql_history(&record).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("保存 SQL 历史失败: {}", e)).to_string())
    })
}

/// 获取项目存储中的 SQL 历史
#[tauri::command]
pub async fn get_project_store_sql_history(
    connection_id: Option<String>,
    limit: Option<usize>,
    state: State<'_, ProjectState>,
) -> Result<Vec<SqlHistoryRecord>, CoreError> {
    let guard = state.store.lock().await;
    let store = guard.as_ref().ok_or_else(|| {
        CoreError::from(
            ProjectError::OperationFailed(
                "项目存储未初始化，请先调用 init_project_store".to_string(),
            )
            .to_string(),
        )
    })?;
    store
        .get_sql_history(connection_id.as_deref(), limit)
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("获取 SQL 历史失败: {}", e)).to_string())
        })
}

/// 保存工作台状态到项目存储
#[tauri::command]
pub async fn save_project_store_workbench_state(
    state_data: WorkbenchState,
    state: State<'_, ProjectState>,
) -> Result<(), CoreError> {
    let guard = state.store.lock().await;
    let store = guard.as_ref().ok_or_else(|| {
        CoreError::from(
            ProjectError::OperationFailed(
                "项目存储未初始化，请先调用 init_project_store".to_string(),
            )
            .to_string(),
        )
    })?;
    store.save_workbench_state(&state_data).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("保存工作台状态失败: {}", e)).to_string())
    })
}

/// 获取项目存储中的工作台状态
#[tauri::command]
pub async fn get_project_store_workbench_state(
    state: State<'_, ProjectState>,
) -> Result<Option<WorkbenchState>, CoreError> {
    let guard = state.store.lock().await;
    let store = guard.as_ref().ok_or_else(|| {
        CoreError::from(
            ProjectError::OperationFailed(
                "项目存储未初始化，请先调用 init_project_store".to_string(),
            )
            .to_string(),
        )
    })?;
    store.get_workbench_state().await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("获取工作台状态失败: {}", e)).to_string())
    })
}

// ==================== 系统级项目管理命令 ====================

/// 获取所有项目信息（系统级）
#[tauri::command]
#[specta::specta]
pub async fn get_all_projects() -> Result<Vec<ProjectInfoResponse>, CoreError> {
    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    let projects = global_db.get_all_projects().await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("获取所有项目失败: {}", e)).to_string())
    })?;

    Ok(projects.into_iter().map(|p| p.into()).collect())
}

/// 重命名项目
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct RenameProjectInput {
    pub project_id: String,
    pub new_name: String,
}

#[tauri::command]
#[specta::specta]
pub async fn rename_project(input: RenameProjectInput) -> Result<(), CoreError> {
    tracing::info!(
        project_id = %input.project_id,
        new_name = %input.new_name,
        "Renaming project"
    );

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    // 验证项目存在
    let project = global_db
        .get_project_by_id(&input.project_id)
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("获取项目信息失败: {}", e)).to_string())
        })?
        .ok_or_else(|| {
            CoreError::from(ProjectError::NotFound(input.project_id.clone()).to_string())
        })?;

    // 更新名称
    global_db
        .update_project_info(
            &input.project_id,
            &input.new_name,
            project.description.as_deref(),
        )
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("重命名项目失败: {}", e)).to_string())
        })?;

    // 使缓存失效
    get_recent_projects_cache().invalidate().await;

    tracing::info!(
        project_id = %input.project_id,
        old_name = %project.name,
        new_name = %input.new_name,
        "Project renamed successfully"
    );

    Ok(())
}

/// 项目验证结果
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct ProjectValidationResult {
    pub is_valid: bool,
    pub path_exists: bool,
    pub meta_dir_exists: bool,
    pub project_json_valid: bool,
    pub errors: Vec<String>,
}

/// 验证项目完整性
#[tauri::command]
#[specta::specta]
pub async fn validate_project_full(
    project_id: String,
) -> Result<ProjectValidationResult, CoreError> {
    tracing::debug!(project_id = %project_id, "Validating project completeness");

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    let project = global_db
        .get_project_by_id(&project_id)
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("获取项目信息失败: {}", e)).to_string())
        })?
        .ok_or_else(|| CoreError::from(ProjectError::NotFound(project_id.clone()).to_string()))?;

    let path_buf = std::path::PathBuf::from(&project.path);
    let mut errors = Vec::new();

    // 1. 检查路径存在性
    let path_exists = path_buf.exists();
    if !path_exists {
        errors.push(format!("项目路径不存在: {}", project.path));
    }

    // 2. 检查 .RSmeta 目录
    let meta_dir = path_buf.join(".RSmeta");
    let meta_dir_exists = meta_dir.exists();
    if !meta_dir_exists {
        errors.push("缺少 .RSmeta 目录".to_string());
    }

    // 3. 检查 project.json 文件
    let project_json = path_buf.join(".RSmeta").join("project.json");
    let project_json_valid = project_json.exists()
        && std::fs::read_to_string(&project_json)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some();
    if !project_json_valid {
        errors.push("project.json 文件无效或不存在".to_string());
    }

    let is_valid = errors.is_empty();

    tracing::debug!(
        project_id = %project_id,
        is_valid = is_valid,
        errors_count = errors.len(),
        "Project validation completed"
    );

    Ok(ProjectValidationResult {
        is_valid,
        path_exists,
        meta_dir_exists,
        project_json_valid,
        errors,
    })
}

/// 从最近项目中移除（不删除物理文件）
///
/// 从全局数据库中删除项目记录，使缓存失效。
/// 返回被移除的项目信息，供前端做 UI 回滚。
#[tauri::command]
#[specta::specta]
pub async fn remove_from_recent(project_id: String) -> Result<ProjectInfoResponse, CoreError> {
    tracing::info!(project_id = %project_id, "Removing project from recent list");

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    let project = global_db
        .get_project_by_id(&project_id)
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("获取项目信息失败: {}", e)).to_string())
        })?
        .ok_or_else(|| CoreError::from(ProjectError::NotFound(project_id.clone()).to_string()))?;

    let response: ProjectInfoResponse = project.into();

    global_db.delete_project(&project_id).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("移除项目失败: {}", e)).to_string())
    })?;

    get_recent_projects_cache().invalidate().await;

    tracing::info!(
        project_id = %project_id,
        name = %response.name,
        "Project removed from recent list"
    );

    Ok(response)
}

/// 物理删除项目（磁盘目录 + 数据库记录）
///
/// 先删除磁盘目录，再删除数据库记录，保证数据一致性。
/// 如果磁盘删除失败，数据库记录不受影响。
#[tauri::command]
#[specta::specta]
pub async fn delete_project_disk(project_id: String) -> Result<(), CoreError> {
    tracing::info!(project_id = %project_id, "Physically deleting project from disk");

    let global_db = crate::core::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::from(ProjectError::OperationFailed("全局数据库未初始化".to_string()).to_string())
    })?;

    let project = global_db
        .get_project_by_id(&project_id)
        .await
        .map_err(|e| {
            CoreError::from(ProjectError::Database(format!("获取项目信息失败: {}", e)).to_string())
        })?
        .ok_or_else(|| CoreError::from(ProjectError::NotFound(project_id.clone()).to_string()))?;

    let project_path = std::path::PathBuf::from(&project.path);
    if project_path.exists() {
        std::fs::remove_dir_all(&project_path).map_err(|e| {
            CoreError::from(
                ProjectError::OperationFailed(format!(
                    "删除项目目录失败: {} (路径: {})",
                    e, project.path
                ))
                .to_string(),
            )
        })?;
        tracing::info!(path = %project.path, "Project directory deleted");
    }

    global_db.delete_project(&project_id).await.map_err(|e| {
        CoreError::from(ProjectError::Database(format!("删除项目记录失败: {}", e)).to_string())
    })?;

    get_recent_projects_cache().invalidate().await;

    tracing::info!(project_id = %project_id, "Project physically deleted");
    Ok(())
}

/// 为新项目种子 4 个内置 Native 驱动（MySQL / PostgreSQL / SQLite / DuckDB）
///
/// 如果 project.db 尚未创建或打开失败，仅记录警告，不阻止项目创建。
async fn seed_default_drivers_for_project(project_path: &std::path::Path) {
    let db_manager =
        match crate::core::persistence::project_db::ProjectDatabaseManager::open(project_path, 3)
            .await
        {
            Ok(manager) => manager,
            Err(e) => {
                tracing::warn!("打开项目数据库失败，跳过驱动种子: {}", e);
                return;
            }
        };

    let store = crate::core::persistence::project_connection_store::ProjectConnectionStore::new(
        std::sync::Arc::new(db_manager),
    );

    if let Err(e) = store.seed_default_drivers().await {
        tracing::warn!("种子默认驱动失败: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::project::models::ProjectInfo;
    use crate::core::project::ProjectPath;
    use crate::core::project::ProjectStatus;
    use chrono::Utc;

    fn sample_project_info() -> ProjectInfo {
        ProjectInfo {
            id: "test-id".to_string(),
            name: "Test Project".to_string(),
            description: Some("A test project".to_string()),
            path: ProjectPath::Local {
                path: std::path::PathBuf::from("/tmp/test-project"),
            },
            status: ProjectStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_opened_at: Some(Utc::now()),
            created_by: Some("test-user".to_string()),
            version_count: 3,
        }
    }

    #[test]
    fn test_project_info_response_from_active() {
        let info = sample_project_info();
        let response: ProjectInfoResponse = info.into();

        assert_eq!(response.id, "test-id");
        assert_eq!(response.name, "Test Project");
        assert_eq!(response.description, Some("A test project".to_string()));
        assert_eq!(response.status, "active");
        assert_eq!(response.version, "3");
        assert!(response.last_opened_at.is_some());

        match response.path {
            ProjectPathResponse::Local { path } => assert!(path.contains("test-project")),
            _ => panic!("Expected Local path"),
        }
    }

    #[test]
    fn test_project_info_response_from_archived() {
        let mut info = sample_project_info();
        info.status = ProjectStatus::Archived;

        let response: ProjectInfoResponse = info.into();
        assert_eq!(response.status, "archived");
    }

    #[test]
    fn test_project_info_response_from_syncing() {
        let mut info = sample_project_info();
        info.status = ProjectStatus::Syncing;

        let response: ProjectInfoResponse = info.into();
        assert_eq!(response.status, "syncing");
    }

    #[test]
    fn test_project_info_response_from_offline() {
        let mut info = sample_project_info();
        info.status = ProjectStatus::Offline;

        let response: ProjectInfoResponse = info.into();
        assert_eq!(response.status, "offline");
    }

    #[test]
    fn test_project_info_response_no_description() {
        let mut info = sample_project_info();
        info.description = None;

        let response: ProjectInfoResponse = info.into();
        assert_eq!(response.description, None);
    }

    #[test]
    fn test_project_info_response_remote_path() {
        let mut info = sample_project_info();
        info.path = ProjectPath::Remote {
            url: "https://ducklake.example.com".to_string(),
            project_id: "dl-proj-1".to_string(),
        };

        let response: ProjectInfoResponse = info.into();
        match response.path {
            ProjectPathResponse::Remote { url, project_id } => {
                assert_eq!(url, "https://ducklake.example.com");
                assert_eq!(project_id, "dl-proj-1");
            }
            _ => panic!("Expected Remote path"),
        }
    }

    #[test]
    fn test_project_error_formatting() {
        let err = ProjectError::NotFound("p1".to_string());
        let err_str: String = err.into();
        assert!(err_str.contains("p1"));

        let err = ProjectError::PathConflict("/conflict".to_string());
        let err_str: String = err.into();
        assert!(err_str.contains("/conflict"));

        let err = ProjectError::OperationFailed("disk full".to_string());
        let err_str: String = err.into();
        assert!(err_str.contains("disk full"));
    }

    #[test]
    fn test_create_project_input_fields() {
        let input = CreateProjectInput {
            name: "New Project".to_string(),
            path: "/tmp/new-project".to_string(),
            description: Some("desc".to_string()),
        };

        assert_eq!(input.name, "New Project");
        assert_eq!(input.path, "/tmp/new-project");
        assert_eq!(input.description, Some("desc".to_string()));
    }

    #[test]
    fn test_update_project_input_fields() {
        let input = UpdateProjectInput {
            id: "p1".to_string(),
            name: "Renamed".to_string(),
            description: Some("New desc".to_string()),
        };

        assert_eq!(input.id, "p1");
        assert_eq!(input.name, "Renamed");
        assert_eq!(input.description, Some("New desc".to_string()));
    }
}
