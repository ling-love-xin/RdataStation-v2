//! 项目存储管理
//!
//! 负责项目的创建、加载、保存和管理。
//! 支持本地项目（现阶段）和远程项目（DuckLake 预留）。
//!
//! 项目目录结构：
//! ```
//! {project_path}/
//! └── .RSmeta/                           # 项目元数据目录
//!     ├── project.db                     # 项目统一 SQLite（project_info, connections, settings）
//!     ├── project.json                   # 项目配置文件（JSON，方便迁移）
//!     ├── analytics.duckdb               # 项目分析数据
//!     └── project_metadata/              # 数据库元数据目录（每个连接一个 .db 文件）
//! ```

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::core::error::CoreError;
use crate::core::migration::{MigrationManager, MigrationType};
use crate::core::persistence;
use crate::core::project::models::{
    ConnectionRef, Project, ProjectConfig, ProjectInfo, ProjectPath, QueryRef,
};

/// 项目元数据目录名称
const RS_META_DIR_NAME: &str = ".RSmeta";

/// 项目元数据数据库文件名
const PROJECT_DB_NAME: &str = "project.db";

/// 项目配置文件名
const PROJECT_JSON_NAME: &str = "project.json";

/// 分析数据库文件名
const ANALYTICS_DB_NAME: &str = "analytics.duckdb";

/// 连接元数据目录名称
const PROJECT_METADATA_DIR_NAME: &str = "project_metadata";

/// 项目存储
///
/// 管理单个项目的持久化操作
pub struct ProjectStore {
    /// 项目信息
    info: ProjectInfo,
    /// 配置缓存
    config: Option<ProjectConfig>,
    /// 连接列表缓存
    connections: Option<Vec<ConnectionRef>>,
    /// 查询列表缓存
    queries: Option<Vec<QueryRef>>,
}

impl ProjectStore {
    /// 创建新项目
    ///
    /// # Arguments
    ///
    /// * `name` - 项目名称
    /// * `path` - 项目路径（本地）
    ///
    /// # Returns
    ///
    /// 返回创建的项目存储实例
    pub fn create(name: impl Into<String>, path: &Path) -> Result<Self, CoreError> {
        // 创建 .RSmeta 目录结构
        let meta_dir = path.join(RS_META_DIR_NAME);
        let dirs = [
            meta_dir.clone(),
            meta_dir.join(PROJECT_METADATA_DIR_NAME),
            meta_dir.join("config"),
        ];

        for dir in &dirs {
            fs::create_dir_all(dir)
                .map_err(|e| persistence::io_to_core_error(e, path, "create directory"))?;
        }

        // 创建项目
        let project_path = ProjectPath::local(path);
        let project = Project::new(name, project_path);

        let store = Self {
            info: project.info,
            config: Some(project.config),
            connections: Some(project.connections),
            queries: Some(project.queries),
        };

        // 保存初始配置（settings.json）
        store.save_config()?;

        // 执行项目级迁移（创建 project.db 和表结构）
        let migration_manager = MigrationManager::new();

        let project_db_path = meta_dir.join(PROJECT_DB_NAME);
        migration_manager.migrate(&project_db_path, MigrationType::ProjectMeta)?;

        let analytics_db_path = meta_dir.join(ANALYTICS_DB_NAME);
        migration_manager.migrate(&analytics_db_path, MigrationType::ProjectAnalysis)?;

        // 保存项目信息（project.json + 同步到 project.db）
        store.save_info()?;

        Ok(store)
    }

    /// 加载已有项目
    ///
    /// # Arguments
    ///
    /// * `path` - 项目路径
    ///
    /// # Returns
    ///
    /// 返回加载的项目存储实例
    pub fn load(path: &Path) -> Result<Self, CoreError> {
        let meta_dir = path.join(RS_META_DIR_NAME);

        // 如果新目录不存在，尝试迁移旧目录结构
        if !meta_dir.exists() {
            let old_meta_dir = path.join(".rdata-station");
            if old_meta_dir.exists() {
                Self::migrate_old_structure(path)?;
            } else {
                return Err(persistence::persistence_to_core_error(
                    &meta_dir.display().to_string(),
                    "load",
                    "Project metadata directory not found",
                ));
            }
        }

        // 加载项目信息（优先从 project.json 加载）
        let info_path = meta_dir.join(PROJECT_JSON_NAME);
        let info: ProjectInfo = if info_path.exists() {
            let content = fs::read_to_string(&info_path)
                .map_err(|e| persistence::io_to_core_error(e, &info_path, "read"))?;
            serde_json::from_str(&content).map_err(|e| {
                persistence::deserialize_to_core_error("json", &content, &e.to_string())
            })?
        } else {
            // project.json 不存在，尝试从 project.db 读取
            let db_path = meta_dir.join(PROJECT_DB_NAME);
            if db_path.exists() {
                match Self::load_project_info_from_db(&db_path, path) {
                    Ok(info) => info,
                    Err(_) => {
                        // 数据库读取失败，使用默认名称
                        Project::new("Unnamed Project", ProjectPath::local(path)).info
                    }
                }
            } else {
                // 数据库也不存在，使用默认名称
                Project::new("Unnamed Project", ProjectPath::local(path)).info
            }
        };

        Ok(Self {
            info,
            config: None,
            connections: None,
            queries: None,
        })
    }

    /// 迁移旧目录结构到新结构
    ///
    /// 旧结构：.rdata-station/{meta,analytics,config,queries,extensions}
    /// 新结构：.RSmeta/{project.db, project.json, analytics.duckdb, project_metadata/}
    fn migrate_old_structure(project_path: &Path) -> Result<(), CoreError> {
        let old_meta_dir = project_path.join(".rdata-station");
        let new_meta_dir = project_path.join(RS_META_DIR_NAME);

        // 创建新目录
        fs::create_dir_all(&new_meta_dir)
            .map_err(|e| persistence::io_to_core_error(e, &new_meta_dir, "create new directory"))?;

        // 迁移 project.json
        let old_config_path = old_meta_dir.join("config").join("project.json");
        let new_config_path = new_meta_dir.join(PROJECT_JSON_NAME);
        if old_config_path.exists() && !new_config_path.exists() {
            fs::copy(&old_config_path, &new_config_path).map_err(|e| {
                persistence::io_to_core_error(e, &old_config_path, "copy project.json")
            })?;
        }

        // 迁移 analytics.duckdb
        let old_analytics_path = old_meta_dir.join("analytics").join("data.duckdb");
        let new_analytics_path = new_meta_dir.join(ANALYTICS_DB_NAME);
        if old_analytics_path.exists() && !new_analytics_path.exists() {
            fs::copy(&old_analytics_path, &new_analytics_path).map_err(|e| {
                persistence::io_to_core_error(e, &old_analytics_path, "copy analytics.duckdb")
            })?;
        }

        // 迁移 project.sqlite 到 project.db
        let old_project_db = old_meta_dir.join("meta").join("project.sqlite");
        let new_project_db = new_meta_dir.join(PROJECT_DB_NAME);
        if old_project_db.exists() && !new_project_db.exists() {
            fs::copy(&old_project_db, &new_project_db).map_err(|e| {
                persistence::io_to_core_error(e, &old_project_db, "copy project.sqlite")
            })?;
        }

        // 迁移 connections.db 到 project.db（合并）
        let old_connections_db = old_meta_dir.join("meta").join("connections.db");
        if old_connections_db.exists() {
            // 将 connections.db 的 connections 表合并到 project.db
            if new_project_db.exists() {
                Self::merge_connections_db(&old_connections_db, &new_project_db)?;
            }
        }

        // 创建 project_metadata 目录
        let metadata_dir = new_meta_dir.join(PROJECT_METADATA_DIR_NAME);
        fs::create_dir_all(&metadata_dir).map_err(|e| {
            persistence::io_to_core_error(e, &metadata_dir, "create metadata directory")
        })?;

        tracing::info!("Migrated project from old structure to new .RSmeta structure");

        Ok(())
    }

    /// 合并 connections.db 到 project.db
    fn merge_connections_db(old_db: &Path, new_db: &Path) -> Result<(), CoreError> {
        let old_conn = rusqlite::Connection::open(old_db).map_err(|e| {
            persistence::persistence_to_core_error("old_connections", "open", &e.to_string())
        })?;

        let new_conn = rusqlite::Connection::open(new_db).map_err(|e| {
            persistence::persistence_to_core_error("new_project_db", "open", &e.to_string())
        })?;

        // 从旧数据库读取所有连接
        let mut stmt = old_conn
            .prepare(
                "SELECT id, name, db_type, host, port, database, username, password,
                    properties, tags, is_active, created_at, updated_at
             FROM connections",
            )
            .map_err(|e| {
                persistence::persistence_to_core_error(
                    "read_connections",
                    "prepare",
                    &e.to_string(),
                )
            })?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, bool>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, String>(12)?,
                ))
            })
            .map_err(|e| {
                persistence::persistence_to_core_error("read_connections", "query", &e.to_string())
            })?;

        // 插入到新数据库
        for row in rows {
            let (
                id,
                name,
                db_type,
                host,
                port,
                database,
                username,
                password,
                properties,
                tags,
                is_active,
                created_at,
                updated_at,
            ) = row.map_err(|e| {
                persistence::persistence_to_core_error("read_connections", "map", &e.to_string())
            })?;

            let is_active_str = if is_active { "1" } else { "0" };

            new_conn
                .execute(
                    "INSERT OR IGNORE INTO connections
                 (id, name, db_type, host, port, database, username, password,
                  properties, tags, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    [
                        &id,
                        &name,
                        &db_type,
                        &host,
                        &port.to_string(),
                        &database,
                        &username.unwrap_or_default(),
                        &password.unwrap_or_default(),
                        &properties.unwrap_or_default(),
                        &tags.unwrap_or_default(),
                        is_active_str,
                        &created_at,
                        &updated_at,
                    ],
                )
                .map_err(|e| {
                    persistence::persistence_to_core_error(
                        "insert_connection",
                        "execute",
                        &e.to_string(),
                    )
                })?;
        }

        Ok(())
    }

    /// 获取项目信息
    pub fn info(&self) -> &ProjectInfo {
        &self.info
    }

    /// 获取项目信息（可变）
    pub fn info_mut(&mut self) -> &mut ProjectInfo {
        &mut self.info
    }

    /// 加载配置
    pub fn load_config(&mut self) -> Result<ProjectConfig, CoreError> {
        if let Some(ref config) = self.config {
            return Ok(config.clone());
        }

        let config_path = self.info.path.local_path().map(|p| {
            p.join(RS_META_DIR_NAME)
                .join("config")
                .join("settings.json")
        });

        let config = if let Some(path) = config_path {
            if path.exists() {
                let content = fs::read_to_string(&path)
                    .map_err(|e| persistence::io_to_core_error(e, &path, "read"))?;
                serde_json::from_str(&content).map_err(|e| {
                    persistence::deserialize_to_core_error("json", &content, &e.to_string())
                })?
            } else {
                ProjectConfig::default()
            }
        } else {
            ProjectConfig::default()
        };

        self.config = Some(config.clone());
        Ok(config)
    }

    /// 保存配置
    pub fn save_config(&self) -> Result<(), CoreError> {
        let config = self.config.as_ref().ok_or_else(|| {
            persistence::persistence_to_core_error("config", "save", "Config not loaded")
        })?;

        let config_path = self
            .info
            .path
            .local_path()
            .map(|p| {
                p.join(RS_META_DIR_NAME)
                    .join("config")
                    .join("settings.json")
            })
            .ok_or_else(|| {
                persistence::persistence_to_core_error("config", "save", "Invalid project path")
            })?;

        let content = serde_json::to_string_pretty(config)
            .map_err(|e| persistence::serialize_to_core_error("json", &e.to_string()))?;

        fs::write(&config_path, content)
            .map_err(|e| persistence::io_to_core_error(e, &config_path, "write"))?;

        Ok(())
    }

    /// 保存项目信息到 project.json
    pub fn save_info(&self) -> Result<(), CoreError> {
        let info_path = self
            .info
            .path
            .local_path()
            .map(|p| p.join(RS_META_DIR_NAME).join(PROJECT_JSON_NAME))
            .ok_or_else(|| {
                persistence::persistence_to_core_error("project", "save", "Invalid project path")
            })?;

        let content = serde_json::to_string_pretty(&self.info)
            .map_err(|e| persistence::serialize_to_core_error("json", &e.to_string()))?;

        fs::write(&info_path, content)
            .map_err(|e| persistence::io_to_core_error(e, &info_path, "write"))?;

        // 同步到 project.db 的 project_info 表
        self.sync_project_info_to_db()?;

        Ok(())
    }

    /// 同步项目信息到 project.db
    ///
    /// 保持 project.json 和 project_info 表数据一致
    fn sync_project_info_to_db(&self) -> Result<(), CoreError> {
        let meta_dir = self
            .info
            .path
            .local_path()
            .map(|p| p.join(RS_META_DIR_NAME))
            .ok_or_else(|| {
                persistence::persistence_to_core_error("project", "sync", "Invalid project path")
            })?;

        let db_path = meta_dir.join(PROJECT_DB_NAME);

        if !db_path.exists() {
            // project.db 尚未创建，跳过
            return Ok(());
        }

        let conn = rusqlite::Connection::open(&db_path).map_err(|e| {
            persistence::persistence_to_core_error("project.db", "open", &e.to_string())
        })?;

        conn.execute(
            "INSERT OR REPLACE INTO project
             (id, name, description, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            [
                &self.info.id,
                &self.info.name,
                self.info.description.as_deref().unwrap_or(""),
                &self.info.created_at.timestamp().to_string(),
                &self.info.updated_at.timestamp().to_string(),
            ],
        )
        .map_err(|e| persistence::persistence_to_core_error("project", "sync", &e.to_string()))?;

        Ok(())
    }

    /// 从 project.db 加载项目信息
    fn load_project_info_from_db(
        db_path: &Path,
        project_path: &Path,
    ) -> Result<ProjectInfo, CoreError> {
        let conn = rusqlite::Connection::open(db_path).map_err(|e| {
            persistence::persistence_to_core_error("project.db", "open", &e.to_string())
        })?;

        let mut stmt = conn
            .prepare("SELECT id, name, description, created_at, updated_at FROM project LIMIT 1")
            .map_err(|e| {
                persistence::persistence_to_core_error("project", "query", &e.to_string())
            })?;

        let info = stmt
            .query_row([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let description: Option<String> = row.get(2)?;
                let created_at: i64 = row.get(3)?;
                let updated_at: i64 = row.get(4)?;

                Ok(ProjectInfo {
                    id,
                    name,
                    description,
                    path: ProjectPath::local(project_path),
                    status: crate::core::project::models::ProjectStatus::Active,
                    created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default(),
                    updated_at: chrono::DateTime::from_timestamp(updated_at, 0).unwrap_or_default(),
                    last_opened_at: None,
                    created_by: None,
                    version_count: 0,
                })
            })
            .map_err(|e| {
                persistence::persistence_to_core_error("project", "load", &e.to_string())
            })?;

        Ok(info)
    }

    /// 更新项目信息
    pub fn update_info<F>(&mut self, f: F) -> Result<(), CoreError>
    where
        F: FnOnce(&mut ProjectInfo),
    {
        f(&mut self.info);
        self.save_info()
    }

    /// 获取项目元数据目录路径
    pub fn meta_dir(&self) -> Result<std::path::PathBuf, CoreError> {
        self.info
            .path
            .local_path()
            .map(|p| p.join(RS_META_DIR_NAME))
            .ok_or_else(|| {
                persistence::persistence_to_core_error(
                    "project",
                    "meta_dir",
                    "Invalid project path",
                )
            })
    }

    /// 获取项目数据库路径
    pub fn project_db_path(&self) -> Result<std::path::PathBuf, CoreError> {
        self.meta_dir().map(|p| p.join(PROJECT_DB_NAME))
    }

    /// 获取连接元数据目录路径
    pub fn project_metadata_dir(&self) -> Result<std::path::PathBuf, CoreError> {
        self.meta_dir().map(|p| p.join(PROJECT_METADATA_DIR_NAME))
    }

    /// 获取指定连接的元数据数据库路径
    pub fn connection_metadata_path(
        &self,
        connection_id: &str,
    ) -> Result<std::path::PathBuf, CoreError> {
        self.project_metadata_dir()
            .map(|p| p.join(format!("{}.db", connection_id)))
    }

    /// 创建连接元数据数据库
    pub fn create_connection_metadata(&self, connection_id: &str) -> Result<(), CoreError> {
        let db_path = self.connection_metadata_path(connection_id)?;

        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| persistence::io_to_core_error(e, parent, "create directory"))?;
        }

        // 创建连接元数据表
        let conn = rusqlite::Connection::open(&db_path).map_err(|e| {
            persistence::persistence_to_core_error("connection_metadata", "open", &e.to_string())
        })?;

        // 创建表结构
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS tables (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                schema TEXT,
                comment TEXT,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS columns (
                id TEXT PRIMARY KEY,
                table_id TEXT NOT NULL,
                name TEXT NOT NULL,
                data_type TEXT NOT NULL,
                is_nullable BOOLEAN DEFAULT 1,
                is_primary_key BOOLEAN DEFAULT 0,
                comment TEXT,
                ordinal_position INTEGER,
                FOREIGN KEY (table_id) REFERENCES tables(id)
            );

            CREATE TABLE IF NOT EXISTS indexes (
                id TEXT PRIMARY KEY,
                table_id TEXT NOT NULL,
                name TEXT NOT NULL,
                columns TEXT,
                is_unique BOOLEAN DEFAULT 0,
                FOREIGN KEY (table_id) REFERENCES tables(id)
            );

            CREATE TABLE IF NOT EXISTS views (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                definition TEXT,
                comment TEXT,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS functions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                return_type TEXT,
                definition TEXT,
                comment TEXT,
                created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .map_err(|e| {
            persistence::persistence_to_core_error(
                "connection_metadata",
                "init_tables",
                &e.to_string(),
            )
        })?;

        Ok(())
    }

    /// 删除连接元数据数据库
    pub fn delete_connection_metadata(&self, connection_id: &str) -> Result<(), CoreError> {
        let db_path = self.connection_metadata_path(connection_id)?;

        if db_path.exists() {
            fs::remove_file(&db_path).map_err(|e| {
                persistence::io_to_core_error(e, &db_path, "delete connection metadata")
            })?;
        }

        Ok(())
    }

    /// 转换为完整的 Project
    pub fn to_project(mut self) -> Result<Project, CoreError> {
        let config = self.load_config()?;
        let connections = self.load_connections()?;
        let queries = self.load_queries()?;

        Ok(Project {
            info: self.info,
            config,
            connections,
            queries,
        })
    }

    /// 加载连接列表（从 SQLite）
    fn load_connections(&mut self) -> Result<Vec<ConnectionRef>, CoreError> {
        if let Some(ref connections) = self.connections {
            return Ok(connections.clone());
        }

        // 从项目 SQLite 加载持久化连接列表（数据模型已定义，待实现 SQL 读取）
        tracing::debug!(
            project_id = %self.info.id,
            "connections table schema ready, SQL read not yet implemented"
        );
        let connections = Vec::new();
        self.connections = Some(connections.clone());
        Ok(connections)
    }

    /// 加载查询列表（从 SQLite）
    fn load_queries(&mut self) -> Result<Vec<QueryRef>, CoreError> {
        if let Some(ref queries) = self.queries {
            return Ok(queries.clone());
        }

        // 从项目 SQLite 加载持久化查询列表（数据模型已定义，待实现 SQL 读取）
        tracing::debug!(
            project_id = %self.info.id,
            "queries table schema ready, SQL read not yet implemented"
        );
        let queries = Vec::new();
        self.queries = Some(queries.clone());
        Ok(queries)
    }
}

/// 项目管理器
///
/// 管理所有项目的生命周期
pub struct ProjectManager {
    /// 当前打开的项目
    current_project: Option<Arc<Mutex<ProjectStore>>>,
    /// 最近项目列表（从系统配置加载）
    recent_projects: Vec<ProjectInfo>,
}

impl ProjectManager {
    /// 创建新的项目管理器
    pub fn new() -> Self {
        Self {
            current_project: None,
            recent_projects: Vec::new(),
        }
    }

    /// 创建新项目
    pub fn create_project(
        &mut self,
        name: impl Into<String>,
        path: &Path,
    ) -> Result<Arc<Mutex<ProjectStore>>, CoreError> {
        let store = ProjectStore::create(name, path)?;
        let project = Arc::new(Mutex::new(store));
        self.current_project = Some(project.clone());
        Ok(project)
    }

    /// 打开项目
    pub fn open_project(&mut self, path: &Path) -> Result<Arc<Mutex<ProjectStore>>, CoreError> {
        let mut store = ProjectStore::load(path)?;

        // 更新最后打开时间
        store.update_info(|info| {
            info.last_opened_at = Some(chrono::Utc::now());
        })?;

        let project = Arc::new(Mutex::new(store));
        self.current_project = Some(project.clone());
        Ok(project)
    }

    /// 获取当前项目
    pub fn current_project(&self) -> Option<Arc<Mutex<ProjectStore>>> {
        self.current_project.clone()
    }

    /// 关闭当前项目
    pub fn close_project(&mut self) {
        self.current_project = None;
    }

    /// 获取最近项目列表
    pub fn recent_projects(&self) -> &[ProjectInfo] {
        &self.recent_projects
    }

    /// 添加到最近项目
    pub fn add_recent_project(&mut self, info: ProjectInfo) {
        // 去重
        self.recent_projects.retain(|p| p.id != info.id);
        // 添加到开头
        self.recent_projects.insert(0, info);
        // 限制数量
        self.recent_projects.truncate(10);
    }
}

impl Default for ProjectManager {
    fn default() -> Self {
        Self::new()
    }
}

// ========== 项目打开自检 ==========

/// 项目打开时检测缺失的外部驱动文件
///
/// 比对项目的 project_drivers 表与全局 driver_files 表，
/// 返回本机未安装的驱动列表，供前端弹窗提示用户安装。
///
/// # Arguments
///
/// * `project_path` - 项目根目录路径
///
/// # Returns
///
/// 返回缺失驱动的列表，每个缺失驱动包含 driver_id、driver_name 和 download_url
pub async fn check_project_missing_drivers(
    project_path: &std::path::Path,
) -> Result<Vec<crate::core::services::driver_service::MissingDriver>, CoreError> {
    use crate::core::migration::get_global_db_manager;
    use crate::core::services::driver_service::MissingDriver;

    let global_db = get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database not initialized".to_string()))?;

    let meta_dir = project_path.join(RS_META_DIR_NAME);
    let project_db_path = meta_dir.join(PROJECT_DB_NAME);

    if !project_db_path.exists() {
        return Ok(Vec::new());
    }

    let enabled: Vec<String> = {
        let conn = rusqlite::Connection::open(&project_db_path)
            .map_err(|e| CoreError::from(format!("打开项目数据库失败: {}", e)))?;

        let mut stmt = conn
            .prepare("SELECT driver_id FROM project_drivers WHERE enabled = 1")
            .map_err(|e| CoreError::from(format!("查询项目驱动失败: {}", e)))?;

        let result: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| CoreError::from(format!("读取项目驱动失败: {}", e)))?
            .filter_map(|r| r.ok())
            .collect();
        result
    };

    let drivers = global_db
        .get_all_drivers()
        .await
        .map_err(|e| CoreError::from(format!("获取驱动列表失败: {}", e)))?;

    let mut missing = Vec::new();
    for driver in drivers {
        if !enabled.contains(&driver.id) {
            continue;
        }
        if driver.driver_kind != "native" {
            let version = driver.version.as_deref().unwrap_or("0.0.0");
            let installed = global_db
                .is_driver_installed(&driver.id, version)
                .await
                .map_err(|e| CoreError::from(format!("检查驱动安装状态失败: {}", e)))?;
            if !installed {
                missing.push(MissingDriver {
                    driver_id: driver.id,
                    driver_name: driver.name,
                    download_url: driver.download_url.unwrap_or_default(),
                });
            }
        }
    }

    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::CoreError;
    use std::path::PathBuf;

    fn test_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rdata_test_project_store_{}", name));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_project_store_create() -> Result<(), CoreError> {
        let project_path = test_temp_dir("create").join("test-project");

        let store = ProjectStore::create("Test Project", &project_path).expect("创建项目失败");

        assert_eq!(store.info().name, "Test Project");
        assert!(project_path.join(RS_META_DIR_NAME).exists());
        assert!(project_path
            .join(RS_META_DIR_NAME)
            .join(PROJECT_METADATA_DIR_NAME)
            .exists());
        assert!(project_path
            .join(RS_META_DIR_NAME)
            .join(ANALYTICS_DB_NAME)
            .exists());
        assert!(project_path.join(RS_META_DIR_NAME).join("config").exists());
        assert!(project_path.join(RS_META_DIR_NAME).join("queries").exists());
        Ok(())
    }

    #[test]
    fn test_project_store_load() -> Result<(), CoreError> {
        let project_path = test_temp_dir("load").join("test-project");

        // 先创建
        let _ = ProjectStore::create("Test Project", &project_path)?;

        // 再加载
        let store = ProjectStore::load(&project_path)?;
        assert_eq!(store.info().name, "Test Project");
        Ok(())
    }

    #[test]
    fn test_project_manager() -> Result<(), CoreError> {
        let mut manager = ProjectManager::new();

        // 创建项目
        let project_path = test_temp_dir("manager").join("managed-project");
        let _project = manager.create_project("Managed Project", &project_path)?;

        // 验证当前项目
        assert!(manager.current_project().is_some());

        // 关闭项目
        manager.close_project();
        assert!(manager.current_project().is_none());

        // 重新打开
        let _ = manager.open_project(&project_path)?;
        assert!(manager.current_project().is_some());
        Ok(())
    }
}
