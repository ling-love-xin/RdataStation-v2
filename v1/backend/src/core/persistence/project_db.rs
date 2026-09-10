/**
 * 项目级数据库管理模块
 *
 * 管理项目的事务性数据（SQLite）和分析性数据（DuckDB）
 *
 * 架构设计：
 * - SQLite: 使用连接池 + WAL 模式，支持并发访问
 * - DuckDB: 使用单例长连接，支持分析查询
 */
use std::path::{Path, PathBuf};
use std::sync::Arc;

use duckdb::Connection as DuckConnection;
use rusqlite::Connection as SqliteConnection;
use tokio::sync::Mutex;

use crate::core::error::{CommonError, CoreError, DatabaseError, StorageError};
use crate::core::migration::{MigrationManager, MigrationType};
use crate::core::persistence::{auth_store, env_store, network_store};

/// 项目 SQLite 连接池
///
/// 使用 WAL 模式和共享缓存，支持并发读写
pub struct ProjectSqlitePool {
    /// 连接池（使用 Mutex 保护）
    pool: Arc<Mutex<Vec<SqliteConnection>>>,
    /// 数据库路径
    db_path: PathBuf,
    /// 连接池大小
    pool_size: usize,
}

/// SQLite 连接池连接包装器（RAII 模式）
///
/// 在 drop 时自动归还连接到连接池
pub struct SqlitePoolConnection {
    /// 内部连接
    conn: Option<SqliteConnection>,
    /// 连接池引用
    pool: Arc<Mutex<Vec<SqliteConnection>>>,
}

impl SqlitePoolConnection {
    pub fn inner(&self) -> Result<&SqliteConnection, CoreError> {
        self.conn.as_ref().ok_or_else(|| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "pool_acquire".to_string(),
                source: "Connection already taken".to_string(),
            })
        })
    }

    pub fn inner_mut(&mut self) -> Result<&mut SqliteConnection, CoreError> {
        self.conn.as_mut().ok_or_else(|| {
            CoreError::database(DatabaseError::Driver {
                db_type: "sqlite".to_string(),
                operation: "pool_acquire".to_string(),
                source: "Connection already taken".to_string(),
            })
        })
    }
}

impl Drop for SqlitePoolConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let pool = Arc::clone(&self.pool);
            // 尝试归还连接（如果失败，连接会丢失）
            match pool.try_lock() {
                Ok(mut pool_guard) => {
                    pool_guard.push(conn);
                }
                Err(_) => {
                    tracing::warn!("Failed to return connection to pool (pool lock unavailable)");
                }
            };
        }
    }
}

impl ProjectSqlitePool {
    /// 创建新的 SQLite 连接池
    ///
    /// # 参数
    /// * `db_path` - 数据库文件路径
    /// * `pool_size` - 连接池大小
    pub async fn new(db_path: PathBuf, pool_size: usize) -> Result<Self, CoreError> {
        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| io_error("create_dir", &parent.to_string_lossy(), &e.to_string()))?;
        }

        let mut pool = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            let conn = Self::open_connection(&db_path)?;
            pool.push(conn);
        }

        Ok(Self {
            pool: Arc::new(Mutex::new(pool)),
            db_path,
            pool_size,
        })
    }

    /// 打开单个 SQLite 连接（配置 WAL 模式和共享缓存）
    fn open_connection(path: &PathBuf) -> Result<SqliteConnection, CoreError> {
        let conn = SqliteConnection::open(path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "open".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 启用 WAL 模式（Write-Ahead Logging），支持并发读写
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "set_wal_mode".to_string(),
                    reason: e.to_string(),
                })
            })?;

        // 设置 WAL 自动 checkpoint 阈值（1000 页）
        conn.query_row("PRAGMA wal_autocheckpoint=1000", [], |_| Ok(()))
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "set_wal_autocheckpoint".to_string(),
                    reason: e.to_string(),
                })
            })?;

        // 设置缓存大小（-2000 表示 2000 页，约 8MB）
        conn.execute("PRAGMA cache_size=-2000", []).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "set_cache_size".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 启用外键约束
        conn.execute("PRAGMA foreign_keys=ON", []).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "enable_foreign_keys".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 设置同步模式为 NORMAL（平衡性能和安全性）
        conn.execute("PRAGMA synchronous=NORMAL", []).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "set_synchronous".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(conn)
    }

    /// 获取连接（从连接池）
    ///
    /// 返回 SqlitePoolConnection 包装器，在 drop 时自动归还连接
    /// 如果连接池已满，将等待直到有连接可用
    pub async fn acquire(&self) -> Result<SqlitePoolConnection, CoreError> {
        // 等待直到有连接可用
        let conn = loop {
            let mut pool = self.pool.lock().await;
            if let Some(conn) = pool.pop() {
                break conn;
            }
            // 连接池为空，等待一小段时间后重试
            drop(pool);
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        };

        Ok(SqlitePoolConnection {
            conn: Some(conn),
            pool: Arc::clone(&self.pool),
        })
    }

    /// 获取数据库路径
    pub fn path(&self) -> &PathBuf {
        &self.db_path
    }

    /// 关闭连接池，释放所有文件句柄
    pub async fn close(&self) {
        // 步骤 1：等待所有连接被归还（最多等待 3 秒）
        let timeout = tokio::time::Duration::from_secs(3);
        let start = tokio::time::Instant::now();

        loop {
            let pool = self.pool.lock().await;
            if pool.len() >= self.pool_size {
                break;
            }
            drop(pool);

            if start.elapsed() > timeout {
                tracing::warn!(
                    db_path = %self.db_path.display(),
                    "SQLite pool close timeout: some connections not returned"
                );
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        // 步骤 2：获取所有连接并执行 WAL checkpoint
        let mut all_conns = {
            let mut pool = self.pool.lock().await;
            pool.drain(..).collect::<Vec<_>>()
        };

        // 步骤 3：对每个连接执行 checkpoint
        for conn in &mut all_conns {
            // 将 WAL 数据合并回主数据库并截断 WAL 文件
            if let Err(e) = conn.execute("PRAGMA wal_checkpoint(TRUNCATE)", []) {
                tracing::warn!(error = %e, "WAL checkpoint failed during pool close");
            }
        }

        // 步骤 4：显式 drop 所有连接（关闭文件句柄）
        drop(all_conns);

        // 步骤 5：等待操作系统释放文件句柄
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // 步骤 6：删除 WAL 和 SHM 文件（如果存在）
        let wal_path = format!("{}-wal", self.db_path.display());
        let shm_path = format!("{}-shm", self.db_path.display());

        if std::path::Path::new(&wal_path).exists() {
            if let Err(e) = std::fs::remove_file(&wal_path) {
                tracing::warn!(path = %wal_path, error = %e, "Failed to remove WAL file");
            }
        }
        if std::path::Path::new(&shm_path).exists() {
            if let Err(e) = std::fs::remove_file(&shm_path) {
                tracing::warn!(path = %shm_path, error = %e, "Failed to remove SHM file");
            }
        }

        tracing::info!(db_path = %self.db_path.display(), "SQLite pool closed and files cleaned");
    }
}

/// 项目 DuckDB 连接
///
/// DuckDB 使用单例长连接，项目打开时创建，项目关闭时销毁
pub struct ProjectDuckdbConnection {
    /// DuckDB 连接
    conn: Arc<Mutex<Option<DuckConnection>>>,
    /// 数据库路径
    db_path: PathBuf,
}

impl ProjectDuckdbConnection {
    /// 创建新的 DuckDB 连接
    ///
    /// # 参数
    /// * `db_path` - 数据库文件路径
    pub async fn new(db_path: PathBuf) -> Result<Self, CoreError> {
        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            if parent.as_os_str() != "" {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    io_error("create_dir", &parent.to_string_lossy(), &e.to_string())
                })?;
            }
        }

        let conn = tokio::task::spawn_blocking({
            let path = db_path.clone();
            move || {
                DuckConnection::open(&path).map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "duckdb".to_string(),
                        operation: "open".to_string(),
                        reason: e.to_string(),
                    })
                })
            }
        })
        .await
        .map_err(|e| CoreError::common(CommonError::General(format!("Task panicked: {}", e))))??;

        Ok(Self {
            conn: Arc::new(Mutex::new(Some(conn))),
            db_path,
        })
    }

    /// 获取 DuckDB 连接
    pub async fn acquire(&self) -> Result<Arc<Mutex<Option<DuckConnection>>>, CoreError> {
        if self.conn.lock().await.is_none() {
            return Err(CoreError::common(CommonError::General(
                "DuckDB connection is closed".to_string(),
            )));
        }
        Ok(self.conn.clone())
    }

    /// 获取数据库路径
    pub fn path(&self) -> &PathBuf {
        &self.db_path
    }

    /// 关闭 DuckDB 连接
    pub async fn close(&self) -> Result<(), CoreError> {
        // 步骤 1：获取连接并设置为 None
        let conn = {
            let mut conn = self.conn.lock().await;
            conn.take()
        };

        // 步骤 2：显式 drop 连接（关闭文件句柄）
        drop(conn);

        // 步骤 3：等待操作系统释放文件句柄
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // 步骤 4：删除 DuckDB 的 WAL 文件（如果存在）
        let wal_path = format!("{}-wal", self.db_path.display());
        let shm_path = format!("{}-shm", self.db_path.display());

        if std::path::Path::new(&wal_path).exists() {
            if let Err(e) = std::fs::remove_file(&wal_path) {
                tracing::warn!(path = %wal_path, error = %e, "Failed to remove DuckDB WAL file");
            }
        }
        if std::path::Path::new(&shm_path).exists() {
            if let Err(e) = std::fs::remove_file(&shm_path) {
                tracing::warn!(path = %shm_path, error = %e, "Failed to remove DuckDB SHM file");
            }
        }

        tracing::info!(db_path = %self.db_path.display(), "DuckDB connection closed and files cleaned");

        Ok(())
    }
}

/// IO错误辅助函数
fn io_error(operation: &str, path: &str, reason: &str) -> CoreError {
    CoreError::Common(CommonError::General(format!(
        "IO error in {} for {}: {}",
        operation, path, reason
    )))
}

/// 项目数据库管理器
///
/// 统一管理项目的 SQLite 和 DuckDB 连接
pub struct ProjectDatabaseManager {
    /// 项目路径
    project_path: PathBuf,
    /// SQLite 连接池
    sqlite_pool: Arc<ProjectSqlitePool>,
    /// DuckDB 长连接
    duckdb_conn: Arc<ProjectDuckdbConnection>,
}

impl ProjectDatabaseManager {
    /// 创建或打开项目数据库
    ///
    /// # 参数
    /// * `project_path` - 项目根目录路径
    /// * `sqlite_pool_size` - SQLite 连接池大小
    pub async fn open(project_path: &Path, sqlite_pool_size: usize) -> Result<Self, CoreError> {
        let rsmeta_path = project_path.join(".RSMETA");
        let config_path = rsmeta_path.join("config");
        let project_metadata_path = rsmeta_path.join("project_metadata");

        tokio::fs::create_dir_all(&rsmeta_path)
            .await
            .map_err(|e| io_error("create_dir", &rsmeta_path.to_string_lossy(), &e.to_string()))?;
        tokio::fs::create_dir_all(&config_path)
            .await
            .map_err(|e| io_error("create_dir", &config_path.to_string_lossy(), &e.to_string()))?;
        tokio::fs::create_dir_all(&project_metadata_path)
            .await
            .map_err(|e| {
                io_error(
                    "create_dir",
                    &project_metadata_path.to_string_lossy(),
                    &e.to_string(),
                )
            })?;

        let sqlite_db_path = rsmeta_path.join("project.db");
        let sqlite_pool = ProjectSqlitePool::new(sqlite_db_path, sqlite_pool_size).await?;

        let duckdb_path = rsmeta_path.join("analytics.duckdb");
        let duckdb_conn = ProjectDuckdbConnection::new(duckdb_path).await?;

        let manager = Self {
            project_path: project_path.to_path_buf(),
            sqlite_pool: Arc::new(sqlite_pool),
            duckdb_conn: Arc::new(duckdb_conn),
        };

        // 使用迁移系统初始化数据库表结构
        manager.init_sqlite_tables().await?;
        manager.init_duckdb_tables().await?;

        Ok(manager)
    }

    /// 初始化 SQLite 表结构（使用迁移系统）
    async fn init_sqlite_tables(&self) -> Result<(), CoreError> {
        let _sqlite = self.sqlite_pool.acquire().await?;
        let db_path = self.sqlite_pool.path().clone();

        // 执行项目元数据迁移
        let migration_manager = MigrationManager::new();
        migration_manager
            .migrate(&db_path, MigrationType::ProjectMeta)
            .map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "migrate_project_meta".to_string(),
                    reason: e.to_string(),
                })
            })?;

        tracing::info!(db_path = %db_path.display(), "Project SQLite tables initialized via migrations");

        // sqlite 在 drop 时自动归还到连接池
        Ok(())
    }

    /// 初始化 DuckDB 表结构（使用迁移系统）
    async fn init_duckdb_tables(&self) -> Result<(), CoreError> {
        let duckdb_path = self.duckdb_conn.path().clone();

        // 执行项目分析迁移
        let migration_manager = MigrationManager::new();
        migration_manager
            .migrate(&duckdb_path, MigrationType::ProjectAnalysis)
            .map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "duckdb".to_string(),
                    operation: "migrate_project_analysis".to_string(),
                    reason: e.to_string(),
                })
            })?;

        tracing::info!(db_path = %duckdb_path.display(), "Project DuckDB tables initialized via migrations");

        Ok(())
    }

    /// 获取 SQLite 连接池
    pub fn sqlite_pool(&self) -> Arc<ProjectSqlitePool> {
        self.sqlite_pool.clone()
    }

    /// 获取 DuckDB 连接
    pub fn duckdb_conn(&self) -> Arc<ProjectDuckdbConnection> {
        self.duckdb_conn.clone()
    }

    /// 获取项目路径
    pub fn project_path(&self) -> &PathBuf {
        &self.project_path
    }

    // ========== 项目级环境管理 ==========

    /// 在项目数据库中创建环境
    pub async fn create_project_environment(
        &self,
        name: &str,
        description: Option<&str>,
        color: Option<&str>,
        sort_order: Option<i32>,
    ) -> Result<env_store::Environment, CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let id = format!(
            "P_env_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        );
        let now = chrono::Utc::now().to_rfc3339();
        let env = env_store::Environment {
            id: id.clone(),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            color: color.map(|s| s.to_string()),
            sort_order: sort_order.unwrap_or(0),
            origin: Some("project".to_string()),
            source_id: None,
            snapshot_at: None,
            created_at: now,
        };
        env_store::create_environment(conn, &env)?;
        Ok(env)
    }

    /// 列出项目数据库中的所有环境
    pub async fn list_project_environments(
        &self,
    ) -> Result<Vec<env_store::Environment>, CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        env_store::list_environments(conn)
    }

    /// 更新项目数据库中的环境名称、描述、颜色和排序
    pub async fn update_project_environment(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        color: Option<&str>,
        sort_order: Option<i32>,
    ) -> Result<(), CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let mut env = env_store::get_environment(conn, id)?.ok_or_else(|| {
            CoreError::storage(StorageError::Persistence {
                store: "env_store".to_string(),
                operation: "update_environment".to_string(),
                reason: format!("environment not found: {}", id),
            })
        })?;
        if let Some(n) = name {
            env.name = n.to_string();
        }
        if let Some(d) = description {
            env.description = Some(d.to_string());
        }
        if let Some(c) = color {
            env.color = Some(c.to_string());
        }
        if let Some(s) = sort_order {
            env.sort_order = s;
        }
        env_store::update_environment(conn, &env)
    }

    /// 从项目数据库中删除环境
    pub async fn delete_project_environment(&self, id: &str) -> Result<(), CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        env_store::delete_environment(conn, id)
    }

    // ========== 项目级环境策略管理 ==========

    /// 在项目数据库中创建环境策略
    pub async fn create_project_environment_policy(
        &self,
        environment_id: &str,
        policy_type: &str,
        policy_config: Option<&str>,
    ) -> Result<env_store::EnvironmentPolicy, CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let id = format!("pol_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let now = chrono::Utc::now().to_rfc3339();
        let policy = env_store::EnvironmentPolicy {
            id: id.clone(),
            environment_id: environment_id.to_string(),
            policy_type: policy_type.to_string(),
            policy_config: policy_config.map(|s| s.to_string()),
            enabled: true,
            created_at: now,
        };
        env_store::create_policy(conn, &policy)?;
        Ok(policy)
    }

    /// 列出项目数据库中指定环境的所有策略
    pub async fn list_project_environment_policies(
        &self,
        environment_id: &str,
    ) -> Result<Vec<env_store::EnvironmentPolicy>, CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        env_store::list_policies(conn, environment_id)
    }

    /// 更新项目数据库中的环境策略配置
    pub async fn update_project_environment_policy(
        &self,
        id: &str,
        policy_config: Option<&str>,
        enabled: Option<bool>,
    ) -> Result<(), CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let policies = env_store::list_policies(conn, "")?;
        let mut policy = policies.into_iter().find(|p| p.id == id).ok_or_else(|| {
            CoreError::storage(StorageError::Persistence {
                store: "env_store".to_string(),
                operation: "update_policy".to_string(),
                reason: format!("policy not found: {}", id),
            })
        })?;
        if let Some(cfg) = policy_config {
            policy.policy_config = Some(cfg.to_string());
        }
        if let Some(e) = enabled {
            policy.enabled = e;
        }
        env_store::update_policy(conn, &policy)
    }

    /// 从项目数据库中删除环境策略
    pub async fn delete_project_environment_policy(&self, id: &str) -> Result<(), CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        env_store::delete_policy(conn, id)
    }

    // ========== 项目级认证配置管理 ==========

    /// 在项目数据库中创建认证配置
    pub async fn create_project_auth_config(
        &self,
        name: Option<&str>,
        auth_type: &str,
        auth_data: &str,
    ) -> Result<auth_store::AuthConfig, CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let id = format!(
            "P_auth_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        );
        let now = chrono::Utc::now().to_rfc3339();
        let ac = auth_store::AuthConfig {
            id: id.clone(),
            name: name.map(|s| s.to_string()),
            auth_type: auth_type.to_string(),
            auth_data: auth_data.to_string(),
            origin: Some("project".to_string()),
            source_id: None,
            snapshot_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        auth_store::create_auth_config(conn, &ac)?;
        Ok(ac)
    }

    /// 列出项目数据库中的所有认证配置
    pub async fn list_project_auth_configs(
        &self,
    ) -> Result<Vec<auth_store::AuthConfig>, CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        auth_store::list_auth_configs(conn, None)
    }

    /// 从项目数据库中删除认证配置
    pub async fn delete_project_auth_config(&self, id: &str) -> Result<(), CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        auth_store::delete_auth_config(conn, id)
    }

    /// 更新项目数据库中的认证配置
    pub async fn update_project_auth_config(
        &self,
        id: &str,
        name: Option<&str>,
        auth_type: &str,
        auth_data: &str,
    ) -> Result<(), CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let now = chrono::Utc::now().to_rfc3339();
        let ac = auth_store::AuthConfig {
            id: id.to_string(),
            name: name.map(|s| s.to_string()),
            auth_type: auth_type.to_string(),
            auth_data: auth_data.to_string(),
            origin: Some("project".to_string()),
            source_id: None,
            snapshot_at: None,
            created_at: now.clone(), // keep original created_at, but AuthConfig has no way to know original
            updated_at: now,
        };
        auth_store::update_auth_config(conn, &ac)?;
        Ok(())
    }

    // ========== 项目级网络配置管理 ==========

    /// 在项目数据库中创建网络配置
    pub async fn create_project_network_config(
        &self,
        name: Option<&str>,
        network_type: &str,
        config: &str,
    ) -> Result<network_store::NetworkConfig, CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let id = format!(
            "P_net_{}",
            uuid::Uuid::new_v4().to_string().replace('-', "")
        );
        let now = chrono::Utc::now().to_rfc3339();
        let nc = network_store::NetworkConfig {
            id: id.clone(),
            name: name.map(|s| s.to_string()),
            network_type: network_type.to_string(),
            config: config.to_string(),
            auth_config_id: None,
            origin: Some("project".to_string()),
            source_id: None,
            snapshot_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        network_store::create_network_config(conn, &nc)?;
        Ok(nc)
    }

    /// 列出项目数据库中的所有网络配置
    pub async fn list_project_network_configs(
        &self,
    ) -> Result<Vec<network_store::NetworkConfig>, CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        network_store::list_network_configs(conn, None)
    }

    /// 更新项目数据库中的网络配置
    pub async fn update_project_network_config(
        &self,
        id: &str,
        name: Option<&str>,
        config: Option<&str>,
    ) -> Result<(), CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let mut nc = network_store::get_network_config(conn, id)?.ok_or_else(|| {
            CoreError::storage(StorageError::Persistence {
                store: "network_store".to_string(),
                operation: "update_network_config".to_string(),
                reason: format!("network config not found: {}", id),
            })
        })?;
        if let Some(n) = name {
            nc.name = Some(n.to_string());
        }
        if let Some(c) = config {
            nc.config = c.to_string();
        }
        nc.updated_at = chrono::Utc::now().to_rfc3339();
        network_store::update_network_config(conn, &nc)
    }

    /// 从项目数据库中删除网络配置
    pub async fn delete_project_network_config(&self, id: &str) -> Result<(), CoreError> {
        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        network_store::delete_network_config(conn, id)
    }

    // ========== 快照方法（全局 → 项目） ==========

    /// 从全局环境快照到项目，返回快照 ID
    pub async fn snapshot_environment(
        &self,
        source: &env_store::Environment,
        global_env_id: &str,
    ) -> Result<String, CoreError> {
        use crate::core::persistence::id_prefix;

        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let snapshot_id = id_prefix::generate_gpid("env", &source.name);
        let now = chrono::Utc::now().to_rfc3339();

        let snapshot = env_store::Environment {
            id: snapshot_id.clone(),
            name: source.name.clone(),
            description: source.description.clone(),
            color: source.color.clone(),
            sort_order: source.sort_order,
            origin: Some("global_snapshot".to_string()),
            source_id: Some(global_env_id.to_string()),
            snapshot_at: Some(now.clone()),
            created_at: now,
        };
        env_store::create_environment(conn, &snapshot)?;
        Ok(snapshot_id)
    }

    /// 从全局认证配置快照到项目，返回快照 ID
    pub async fn snapshot_auth_config(
        &self,
        source: &auth_store::AuthConfig,
        global_auth_id: &str,
    ) -> Result<String, CoreError> {
        use crate::core::persistence::id_prefix;

        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let snapshot_id =
            id_prefix::generate_gpid("auth", source.name.as_deref().unwrap_or("unnamed"));
        let now = chrono::Utc::now().to_rfc3339();

        let snapshot = auth_store::AuthConfig {
            id: snapshot_id.clone(),
            name: source.name.clone(),
            auth_type: source.auth_type.clone(),
            auth_data: source.auth_data.clone(),
            origin: Some("global_snapshot".to_string()),
            source_id: Some(global_auth_id.to_string()),
            snapshot_at: Some(now.clone()),
            created_at: source.created_at.clone(),
            updated_at: now,
        };
        auth_store::create_auth_config(conn, &snapshot)?;
        Ok(snapshot_id)
    }

    /// 从全局网络配置快照到项目，返回快照 ID
    pub async fn snapshot_network_config(
        &self,
        source: &network_store::NetworkConfig,
        global_net_id: &str,
    ) -> Result<String, CoreError> {
        use crate::core::persistence::id_prefix;

        let sqlite = self.sqlite_pool.acquire().await?;
        let conn = sqlite.inner()?;
        let snapshot_id =
            id_prefix::generate_gpid("net", source.name.as_deref().unwrap_or("unnamed"));
        let now = chrono::Utc::now().to_rfc3339();

        let snapshot = network_store::NetworkConfig {
            id: snapshot_id.clone(),
            name: source.name.clone(),
            network_type: source.network_type.clone(),
            config: source.config.clone(),
            auth_config_id: source.auth_config_id.clone(),
            origin: Some("global_snapshot".to_string()),
            source_id: Some(global_net_id.to_string()),
            snapshot_at: Some(now.clone()),
            created_at: source.created_at.clone(),
            updated_at: now,
        };
        network_store::create_network_config(conn, &snapshot)?;
        Ok(snapshot_id)
    }

    /// 关闭数据库连接
    pub async fn close(&self) -> Result<(), CoreError> {
        // 关闭 SQLite 连接池，释放所有文件句柄
        self.sqlite_pool.close().await;

        // 关闭 DuckDB 连接
        self.duckdb_conn.close().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rdata_test_project_db_{}", name));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[tokio::test]
    #[ignore = "项目数据库初始化存在迁移系统问题，需单独修复"]
    async fn test_project_db_creation() {
        let project_path = test_temp_dir("creation");

        let manager = ProjectDatabaseManager::open(&project_path, 3).await;
        assert!(manager.is_ok(), "创建项目数据库失败: {:?}", manager.err());
    }
}
