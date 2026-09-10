/**
 * 全局系统数据库管理模块
 *
 * 管理全局系统级的 SQLite 和 DuckDB 数据库连接池。
 * 这两个数据库在应用启动时创建，应用关闭时销毁，全程保持连接。
 *
 * 架构设计：
 * - SQLite: 使用连接池 + WAL 模式 + 共享缓存，支持并发访问
 * - DuckDB: 使用单例长连接，支持分析查询和联邦查询
 */
use std::path::PathBuf;
use std::sync::Arc;

use duckdb::Connection as DuckConnection;
use rusqlite::{Connection as SqliteConnection, OptionalExtension};
use tokio::sync::{Mutex, Semaphore};

use crate::core::driver::utils::quote_identifier;
use crate::core::error::{CommonError, CoreError, StorageError};
use crate::core::migration::{MigrationManager, MigrationType};
use crate::core::persistence::auth_store;
use crate::core::persistence::driver_store::{self, DataSourceType, Driver, DriverFile};
use crate::core::persistence::env_store;
use crate::core::persistence::network_store;
use crate::core::persistence::plugin_store;
use crate::core::persistence::sql_template_store::SqlTemplateStore;
use crate::core::persistence::workbench_context_store::WorkbenchContextStore;

/// 全局连接信息结构体
#[derive(Debug, Clone)]
pub struct GlobalConnectionInfo {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub schema_name: Option<String>,
    pub username: Option<String>,
    pub password_encrypted: Option<String>,
    pub options: Option<String>,
    pub tags: Option<String>,
    pub use_duckdb_fed: bool,
    pub metadata_path: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
    pub server_version: Option<String>,
    pub description: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
}

/// 全局 SQLite 连接池
///
/// 使用 WAL 模式和共享缓存，支持并发读写
pub struct GlobalSqlitePool {
    /// 连接池
    pool: Arc<Mutex<Vec<SqliteConnection>>>,
    /// 信号量，控制最大连接数
    semaphore: Arc<Semaphore>,
    /// 数据库路径
    db_path: PathBuf,
}

/// 全局 SQLite 连接池连接包装器（RAII 模式）
///
/// 持有 `OwnedSemaphorePermit` 确保连接到归还前信号量许可不释放。
/// 在 drop 时自动归还连接到连接池。
pub struct GlobalPooledConnection {
    conn: Option<SqliteConnection>,
    pool: Arc<Mutex<Vec<SqliteConnection>>>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl GlobalPooledConnection {
    pub fn inner(&self) -> Result<&SqliteConnection, CoreError> {
        self.conn.as_ref().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "Connection already returned to pool".to_string(),
            ))
        })
    }

    pub fn inner_mut(&mut self) -> Result<&mut SqliteConnection, CoreError> {
        self.conn.as_mut().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "Connection already returned to pool".to_string(),
            ))
        })
    }
}

impl Drop for GlobalPooledConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let pool = Arc::clone(&self.pool);
            match pool.try_lock() {
                Ok(mut pool_guard) => {
                    pool_guard.push(conn);
                }
                Err(_) => {
                    tracing::warn!(
                        "Failed to return global SQLite connection to pool (lock unavailable)"
                    );
                }
            };
        }
        // _permit drops automatically, releasing to semaphore
    }
}

impl GlobalSqlitePool {
    /// 创建新的 SQLite 连接池
    ///
    /// # 参数
    /// * `db_path` - 数据库文件路径
    /// * `pool_size` - 连接池大小
    pub async fn new(db_path: PathBuf, pool_size: usize) -> Result<Self, CoreError> {
        // 确保父目录存在
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to create directory {:?}: {}",
                    parent, e
                )))
            })?;
        }

        let mut pool = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            let conn = Self::open_connection(&db_path)?;
            pool.push(conn);
        }

        Ok(Self {
            pool: Arc::new(Mutex::new(pool)),
            semaphore: Arc::new(Semaphore::new(pool_size)),
            db_path,
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
        // PRAGMA journal_mode=WAL 会返回结果，所以使用 query_row
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "set_wal_mode".to_string(),
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
    /// 返回 `GlobalPooledConnection` 包装器，在 drop 时自动归还连接。
    /// 如果连接池暂时为空，将等待直到有连接可用。
    pub async fn acquire(&self) -> Result<GlobalPooledConnection, CoreError> {
        let permit = Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .map_err(|_| CoreError::common(CommonError::General("Semaphore closed".to_string())))?;

        let conn = loop {
            let mut pool = self.pool.lock().await;
            if let Some(conn) = pool.pop() {
                break conn;
            }
            drop(pool);
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        };

        Ok(GlobalPooledConnection {
            conn: Some(conn),
            pool: Arc::clone(&self.pool),
            _permit: permit,
        })
    }

    /// 同步获取连接（用于同步上下文）
    pub fn acquire_sync(&self) -> Result<GlobalPooledConnection, CoreError> {
        let rt = tokio::runtime::Handle::current();

        let permit = rt
            .block_on(Arc::clone(&self.semaphore).acquire_owned())
            .map_err(|_| CoreError::common(CommonError::General("Semaphore closed".to_string())))?;

        let conn = loop {
            let mut pool = rt.block_on(self.pool.lock());
            if let Some(conn) = pool.pop() {
                break conn;
            }
            drop(pool);
            std::thread::sleep(std::time::Duration::from_millis(10));
        };

        Ok(GlobalPooledConnection {
            conn: Some(conn),
            pool: Arc::clone(&self.pool),
            _permit: permit,
        })
    }

    /// 释放连接（归还到连接池）
    /// 直接 drop 包装器即可，此方法用于显式提前归还
    pub async fn release(&self, conn: GlobalPooledConnection) {
        drop(conn);
    }

    /// 同步释放连接
    pub fn release_sync(&self, conn: GlobalPooledConnection) {
        drop(conn);
    }

    /// 获取数据库路径
    pub fn path(&self) -> &PathBuf {
        &self.db_path
    }
}

/// 全局 DuckDB 连接
///
/// DuckDB 使用单例长连接，应用启动时创建，应用关闭时销毁。
/// 连接创建委托给 DuckDBManager 统一管理。
pub struct GlobalDuckdbConnection {
    /// DuckDB 连接
    conn: Arc<Mutex<Option<DuckConnection>>>,
    /// 数据库路径
    db_path: PathBuf,
}

impl GlobalDuckdbConnection {
    /// 创建新的 DuckDB 连接
    ///
    /// # 参数
    /// * `db_path` - 数据库文件路径，如果为 ":memory:" 则使用内存数据库
    pub async fn new(db_path: PathBuf) -> Result<Self, CoreError> {
        let conn = crate::core::DuckDBManager::open_file_with_retry(&db_path.to_string_lossy())?;

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
        let mut conn = self.conn.lock().await;
        *conn = None;
        Ok(())
    }
}

/// 全局连接更新输入参数
pub struct GlobalConnectionUpdateInput {
    pub conn_id: String,
    pub name: String,
    pub driver: Option<String>,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub schema_name: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub tags: Option<String>,
    pub server_version: Option<String>,
    pub description: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub options: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
    pub use_duckdb_fed: Option<bool>,
    pub metadata_path: Option<String>,
}

/// 全局系统数据库管理器
///
/// 统一管理全局 SQLite 和 DuckDB 连接
/// 保存全局连接输入参数
pub struct GlobalConnectionSaveInput<'a> {
    pub conn_id: &'a str,
    pub name: &'a str,
    pub db_type: &'a str,
    pub url: &'a str,
    pub username: Option<&'a str>,
    pub password: Option<&'a str>,
    pub tags: Option<&'a str>,
    pub server_version: Option<&'a str>,
    pub description: Option<&'a str>,
    pub driver_id: Option<&'a str>,
    pub environment_id: Option<&'a str>,
    pub auth_config_id: Option<&'a str>,
    pub auth_method: Option<&'a str>,
    pub network_config_id: Option<&'a str>,
    pub options: Option<&'a str>,
    pub driver_properties: Option<&'a str>,
    pub advanced_options: Option<&'a str>,
    pub use_duckdb_fed: Option<bool>,
    pub metadata_path: Option<&'a str>,
    pub schema_name: Option<&'a str>,
}

/// 全局系统数据库管理器
///
/// 统一管理全局 SQLite 和 DuckDB 连接
pub struct GlobalDatabaseManager {
    /// SQLite 连接池
    sqlite_pool: Arc<GlobalSqlitePool>,
    /// DuckDB 长连接
    duckdb_conn: Arc<GlobalDuckdbConnection>,
}

impl GlobalDatabaseManager {
    /// 创建全局系统数据库管理器
    ///
    /// # 参数
    /// * `sqlite_path` - SQLite 数据库路径
    /// * `duckdb_path` - DuckDB 数据库路径
    /// * `sqlite_pool_size` - SQLite 连接池大小
    pub async fn new(
        sqlite_path: PathBuf,
        duckdb_path: PathBuf,
        sqlite_pool_size: usize,
    ) -> Result<Self, CoreError> {
        // 确保全局元数据目录存在
        let system_dir = sqlite_path.parent().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "SQLite path has no parent directory".to_string(),
            ))
        })?;
        let global_metadata_path = system_dir.join("global_metadata");
        std::fs::create_dir_all(&global_metadata_path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to create global metadata directory: {}",
                e
            )))
        })?;

        let sqlite_pool = GlobalSqlitePool::new(sqlite_path.clone(), sqlite_pool_size).await?;
        let duckdb_conn = GlobalDuckdbConnection::new(duckdb_path.clone()).await?;

        let manager = Self {
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
        let sqlite_path = self.sqlite_pool.path().clone();

        // 执行全局迁移
        let migration_manager = MigrationManager::new();
        migration_manager
            .migrate(&sqlite_path, MigrationType::Global)
            .map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "migrate_global".to_string(),
                    reason: e.to_string(),
                })
            })?;

        // 修复旧数据库缺失的字段（向后兼容）
        Self::repair_global_tables(&self.sqlite_pool).await?;

        tracing::info!(db_path = %sqlite_path.display(), "Global SQLite tables initialized via migrations");

        Ok(())
    }

    /// 修复旧数据库缺失的字段
    ///
    /// 当旧数据库已有版本记录但表结构不完整时，补充缺失字段
    async fn repair_global_tables(pool: &Arc<GlobalSqlitePool>) -> Result<(), CoreError> {
        let conn = pool.acquire().await?;

        // 检查并添加缺失的字段
        let columns_to_add = [
            ("global_connections", "schema_name", "TEXT"),
            ("global_connections", "use_duckdb_fed", "BOOLEAN DEFAULT 0"),
            ("global_connections", "metadata_path", "TEXT"),
        ];

        for (table, column, column_type) in columns_to_add {
            let sql = format!("PRAGMA table_info({})", quote_identifier(table, '"'));
            let existing_columns: Result<Vec<String>, _> = conn
                .inner()?
                .prepare(&sql)
                .map(|mut stmt| {
                    stmt.query_map([], |row| row.get::<_, String>(1))
                        .map(|rows| rows.filter_map(|r| r.ok()).collect())
                })
                .map_err(|e| {
                    CoreError::Storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "check_column".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            if let Ok(cols) = existing_columns {
                if !cols.iter().any(|c| c == column) {
                    let alter_sql = format!(
                        "ALTER TABLE {} ADD COLUMN {} {}",
                        table, column, column_type
                    );
                    if let Err(e) = conn.inner()?.execute(&alter_sql, []) {
                        tracing::warn!("Failed to add column {}.{}: {}", table, column, e);
                    } else {
                        tracing::info!("Added missing column {}.{}", table, column);
                    }
                }
            }
        }

        Ok(())
    }

    /// 初始化 DuckDB 表结构（直接使用 DuckDB 连接）
    async fn init_duckdb_tables(&self) -> Result<(), CoreError> {
        let duckdb_path = self.duckdb_conn.path().clone();

        // 确保父目录存在
        if let Some(parent) = duckdb_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to create directory {:?}: {}",
                    parent, e
                )))
            })?;
        }

        // 使用 DuckDB 连接执行迁移
        let conn = Self::open_duckdb_for_migration(&duckdb_path)?;

        // 确保迁移版本表存在
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version     INTEGER PRIMARY KEY,
                name        TEXT NOT NULL,
                applied_at  INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| {
            CoreError::Storage(StorageError::Persistence {
                store: "duckdb".to_string(),
                operation: "create_schema_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 获取当前版本
        let current_version: u32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "duckdb".to_string(),
                    operation: "get_current_version".to_string(),
                    reason: e.to_string(),
                })
            })?;

        // 加载并执行迁移
        use include_dir::include_dir;
        const MIGRATIONS_DIR: include_dir::Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

        if let Some(dir) = MIGRATIONS_DIR.get_dir("project_analysis") {
            let mut migrations: Vec<_> = dir
                .files()
                .filter_map(|f| {
                    let filename = f.path().file_name()?.to_str()?;
                    if !filename.ends_with(".sql") {
                        return None;
                    }
                    let stem = filename.strip_suffix(".sql")?;
                    let parts: Vec<&str> = stem.splitn(2, '_').collect();
                    if parts.len() != 2 {
                        return None;
                    }
                    let version = parts[0].parse::<u32>().ok()?;
                    if version <= current_version {
                        return None;
                    }
                    let name = parts[1].to_string();
                    let sql = f.contents_utf8()?.to_string();
                    Some((version, name, sql))
                })
                .collect();

            migrations.sort_by_key(|m| m.0);

            for (version, name, sql) in migrations {
                conn.execute_batch(&sql).map_err(|e| {
                    CoreError::Storage(StorageError::Persistence {
                        store: "duckdb".to_string(),
                        operation: format!("migrate_{}", name),
                        reason: e.to_string(),
                    })
                })?;

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);

                conn.execute(
                    "INSERT INTO schema_version (version, name, applied_at) VALUES (?1, ?2, ?3)",
                    [
                        &version as &dyn duckdb::ToSql,
                        &name as &dyn duckdb::ToSql,
                        &now as &dyn duckdb::ToSql,
                    ],
                )
                .map_err(|e| {
                    CoreError::Storage(StorageError::Persistence {
                        store: "duckdb".to_string(),
                        operation: "record_version".to_string(),
                        reason: e.to_string(),
                    })
                })?;

                tracing::info!("Applied DuckDB migration {} (version {})", name, version);
            }
        }

        tracing::info!(db_path = %duckdb_path.display(), "Global DuckDB tables initialized via migrations");

        Ok(())
    }

    /// 为迁移打开 DuckDB 连接（带损坏文件修复）
    fn open_duckdb_for_migration(db_path: &PathBuf) -> Result<DuckConnection, CoreError> {
        match DuckConnection::open(db_path) {
            Ok(conn) => Ok(conn),
            Err(e) => {
                tracing::warn!(
                    "Failed to open DuckDB for migration: {}, attempting to recreate...",
                    e
                );
                if db_path.exists() {
                    if let Err(remove_err) = std::fs::remove_file(db_path) {
                        tracing::error!("Failed to remove corrupted DuckDB file: {}", remove_err);
                        return Err(CoreError::Storage(StorageError::Persistence {
                            store: "duckdb".to_string(),
                            operation: "open".to_string(),
                            reason: e.to_string(),
                        }));
                    }
                    tracing::info!("Removed corrupted DuckDB file: {}", db_path.display());
                }
                DuckConnection::open(db_path).map_err(|e| {
                    CoreError::Storage(StorageError::Persistence {
                        store: "duckdb".to_string(),
                        operation: "open".to_string(),
                        reason: e.to_string(),
                    })
                })
            }
        }
    }

    /// 获取 SQLite 连接池
    pub fn sqlite_pool(&self) -> Arc<GlobalSqlitePool> {
        self.sqlite_pool.clone()
    }

    /// 获取 DuckDB 连接
    pub fn duckdb_conn(&self) -> Arc<GlobalDuckdbConnection> {
        self.duckdb_conn.clone()
    }

    /// 保存全局连接信息到 SQLite
    pub async fn save_global_connection(
        &self,
        input: GlobalConnectionSaveInput<'_>,
    ) -> Result<(), CoreError> {
        // 输入校验：name、db_type、url 不能为空
        if input.name.is_empty() {
            return Err(CoreError::common(CommonError::InvalidArgument {
                param: "name".to_string(),
                reason: "连接名称不能为空".to_string(),
            }));
        }
        if input.db_type.is_empty() {
            return Err(CoreError::common(CommonError::InvalidArgument {
                param: "db_type".to_string(),
                reason: "数据库类型不能为空".to_string(),
            }));
        }
        if input.url.is_empty() {
            return Err(CoreError::common(CommonError::InvalidArgument {
                param: "url".to_string(),
                reason: "连接URL不能为空".to_string(),
            }));
        }

        let conn = self.sqlite_pool.acquire().await?;

        // A4-L1: 全局连接数量上限检查
        const MAX_GLOBAL_CONNECTIONS: usize = 50;
        let count: i64 = conn
            .inner()?
            .query_row(
                "SELECT COUNT(*) FROM global_connections WHERE is_active = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "count_global_connections".to_string(),
                    reason: e.to_string(),
                })
            })?;
        if count as usize >= MAX_GLOBAL_CONNECTIONS {
            return Err(CoreError::common(CommonError::InvalidArgument {
                param: "connection".to_string(),
                reason: format!(
                    "全局连接数已达上限（{}条），请删除不再使用的连接后再添加",
                    MAX_GLOBAL_CONNECTIONS
                ),
            }));
        }

        // A4-U1: 全局连接名称唯一性检查（排除自身，支持编辑）
        let dup_count: i64 = conn
            .inner()?
            .query_row(
                "SELECT COUNT(*) FROM global_connections WHERE name = ?1 AND is_active = 1 AND id != ?2",
                rusqlite::params![input.name, input.conn_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "check_duplicate_global_name".to_string(),
                    reason: e.to_string(),
                })
            })?;
        if dup_count > 0 {
            return Err(CoreError::common(CommonError::InvalidArgument {
                param: "name".to_string(),
                reason: format!("连接名称 \"{}\" 已存在，请使用其他名称", input.name),
            }));
        }

        // 解析 URL 提取 host, port, database, username, password
        let (host, port, database, url_username, url_password) =
            Self::parse_connection_url(input.db_type, input.url);

        // 优先使用传入的 username/password，如果为空则使用 URL 中解析的
        let final_username = input.username.or(url_username.as_deref()).unwrap_or("");
        let final_password = match input.password.or(url_password.as_deref()) {
            Some(p) if !p.is_empty() => crate::core::crypto::encrypt_password(p).map_err(|e| {
                CoreError::common(CommonError::General(format!("密码加密失败: {}", e)))
            })?,
            _ => String::new(),
        };

        // 默认标签：如果没有提供标签，添加 "global" 标签
        let tags_json = input
            .tags
            .filter(|t| !t.is_empty())
            .map(|t| {
                // 验证是否为合法 JSON 数组
                serde_json::from_str::<serde_json::Value>(t)
                    .map_err(|e| {
                        tracing::warn!("标签 JSON 格式无效: {}, 原始值: {}", e, t);
                        format!("invalid tags JSON: {}", t)
                    })
                    .map(|_| t.to_string())
            })
            .transpose()
            .map_err(|e| CoreError::common(CommonError::General(e)))?
            .unwrap_or_else(|| "[\"global\"]".to_string());

        conn.inner()?.execute(
            "INSERT OR REPLACE INTO global_connections
             (id, name, driver, host, port, database, schema_name, username, password_encrypted, options, tags, use_duckdb_fed, metadata_path, server_version, description, driver_id, environment_id, auth_config_id, auth_method, network_config_id, driver_properties, advanced_options, is_active, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, 1, CURRENT_TIMESTAMP)",
            [
                input.conn_id,
                input.name,
                input.db_type,
                host.as_deref().unwrap_or(""),
                &port.map(|p| p.to_string()).unwrap_or_default(),
                database.as_deref().unwrap_or(""),
                input.schema_name.unwrap_or(""),
                final_username,
                &final_password,
                input.options.unwrap_or(""),
                &tags_json,
                input.use_duckdb_fed.map(|v| if v { "1" } else { "0" }).unwrap_or("0"),
                input.metadata_path.unwrap_or(""),
                input.server_version.unwrap_or(""),
                input.description.unwrap_or(""),
                input.driver_id.unwrap_or(""),
                input.environment_id.unwrap_or(""),
                input.auth_config_id.unwrap_or(""),
                input.auth_method.unwrap_or(""),
                input.network_config_id.unwrap_or(""),
                input.driver_properties.unwrap_or(""),
                input.advanced_options.unwrap_or(""),
            ],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "save_global_connection".to_string(),
            reason: e.to_string()
        }))?;

        tracing::info!("全局连接信息已保存: {} ({})", input.name, input.conn_id);
        Ok(())
    }

    /// 更新全局连接信息
    pub async fn update_global_connection(
        &self,
        input: GlobalConnectionUpdateInput,
    ) -> Result<(), CoreError> {
        if input.name.is_empty() {
            return Err(CoreError::common(CommonError::InvalidArgument {
                param: "name".to_string(),
                reason: "连接名称不能为空".to_string(),
            }));
        }

        let conn = self.sqlite_pool.acquire().await?;

        // 名称唯一性检查（排除自身）
        let dup_count: i64 = conn
            .inner()?
            .query_row(
                "SELECT COUNT(*) FROM global_connections WHERE name = ?1 AND is_active = 1 AND id != ?2",
                rusqlite::params![input.name, input.conn_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "check_duplicate_global_name_update".to_string(),
                    reason: e.to_string(),
                })
            })?;
        if dup_count > 0 {
            return Err(CoreError::common(CommonError::InvalidArgument {
                param: "name".to_string(),
                reason: format!("连接名称 \"{}\" 已存在，请使用其他名称", input.name),
            }));
        }

        // 密码：如果提供新密码则加密，否则保留现有值
        let password_encrypted = match input.password {
            Some(p) if !p.is_empty() => {
                Some(crate::core::crypto::encrypt_password(&p).map_err(|e| {
                    CoreError::common(CommonError::General(format!("密码加密失败: {}", e)))
                })?)
            }
            _ => {
                let existing: Option<String> = conn
                    .inner()?
                    .query_row(
                        "SELECT password_encrypted FROM global_connections WHERE id = ?1",
                        [&input.conn_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "sqlite".to_string(),
                            operation: "read_existing_password_update".to_string(),
                            reason: e.to_string(),
                        })
                    })?
                    .flatten();
                existing
            }
        };

        let use_duckdb_fed_str = input
            .use_duckdb_fed
            .map(|v| if v { "1" } else { "0" })
            .unwrap_or("0");

        conn.inner()?
            .execute(
                "UPDATE global_connections SET
                name = ?2, driver = ?3, host = ?4, port = ?5, database = ?6,
                schema_name = ?7, username = ?8, password_encrypted = ?9,
                options = ?10, tags = ?11, use_duckdb_fed = ?12,
                metadata_path = ?13, server_version = ?14, description = ?15,
                driver_id = ?16, environment_id = ?17, auth_config_id = ?18,
                auth_method = ?19, network_config_id = ?20,
                driver_properties = ?21, advanced_options = ?22, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1 AND is_active = 1",
                rusqlite::params![
                    input.conn_id,
                    input.name,
                    input.driver.as_deref().unwrap_or(""),
                    input.host.as_deref().unwrap_or(""),
                    input.port.map(|p| p.to_string()).unwrap_or_default(),
                    input.database.as_deref().unwrap_or(""),
                    input.schema_name.as_deref().unwrap_or(""),
                    input.username.as_deref().unwrap_or(""),
                    password_encrypted.as_deref().unwrap_or(""),
                    input.options.as_deref().unwrap_or(""),
                    input.tags.as_deref().unwrap_or(""),
                    use_duckdb_fed_str,
                    input.metadata_path.as_deref().unwrap_or(""),
                    input.server_version.as_deref().unwrap_or(""),
                    input.description.as_deref().unwrap_or(""),
                    input.driver_id.as_deref().unwrap_or(""),
                    input.environment_id.as_deref().unwrap_or(""),
                    input.auth_config_id.as_deref().unwrap_or(""),
                    input.auth_method.as_deref().unwrap_or(""),
                    input.network_config_id.as_deref().unwrap_or(""),
                    input.driver_properties.as_deref().unwrap_or(""),
                    input.advanced_options.as_deref().unwrap_or(""),
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "update_global_connection".to_string(),
                    reason: e.to_string(),
                })
            })?;

        tracing::info!("全局连接信息已更新: {} ({})", input.name, input.conn_id);
        Ok(())
    }

    /// 获取所有全局连接
    ///
    /// # 返回
    /// 返回全局连接列表，每个连接包含 tags, username, password 字段
    pub async fn get_global_connections(
        &self,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<GlobalConnectionInfo>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let connections: Vec<GlobalConnectionInfo> = {
            let base_sql = format!(
                "SELECT id, name, driver, host, port, database, schema_name, username, password_encrypted, options, tags, use_duckdb_fed, metadata_path, is_active, created_at, updated_at, server_version, description, driver_id, environment_id, auth_config_id, auth_method, network_config_id, driver_properties, advanced_options
                 FROM global_connections
                 WHERE is_active = 1
                 ORDER BY updated_at DESC{}{}",
                limit.map_or(String::new(), |l| format!(" LIMIT {}", l)),
                offset.map_or(String::new(), |o| format!(" OFFSET {}", o)),
            );
            let mut stmt = conn.inner()?.prepare(&base_sql).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_global_connections".to_string(),
                    reason: e.to_string(),
                })
            })?;

            let rows = stmt
                .query_map([], |row| {
                    let tags: Option<String> = row.get(10).ok();
                    let use_duckdb_fed: bool = row.get(11).unwrap_or(false);
                    let metadata_path: Option<String> = row.get(12).ok();
                    let created_at: String = row.get(14).unwrap_or_default();
                    let updated_at: String = row.get(15).unwrap_or_default();
                    let server_version: Option<String> = row.get(16).ok();
                    let description: Option<String> = row.get(17).ok();
                    let driver_id: Option<String> = row.get(18).ok();
                    let environment_id: Option<String> = row.get(19).ok();
                    let auth_config_id: Option<String> = row.get(20).ok();
                    let auth_method: Option<String> = row.get(21).ok();
                    let network_config_id: Option<String> = row.get(22).ok();
                    let driver_properties: Option<String> = row.get(23).ok();
                    let advanced_options: Option<String> = row.get(24).ok();

                    Ok(GlobalConnectionInfo {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        driver: row.get(2)?,
                        host: row.get(3).ok(),
                        port: row.get(4).ok(),
                        database: row.get(5).ok(),
                        schema_name: row.get(6).ok(),
                        username: row.get(7).ok(),
                        password_encrypted: row.get(8).ok(),
                        options: row.get(9).ok(),
                        tags,
                        use_duckdb_fed,
                        metadata_path,
                        is_active: row.get(13).unwrap_or(true),
                        created_at,
                        updated_at,
                        server_version,
                        description,
                        driver_id,
                        environment_id,
                        auth_config_id,
                        auth_method,
                        network_config_id,
                        driver_properties,
                        advanced_options,
                    })
                })
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "get_global_connections_query".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            let mut result = Vec::new();
            for info in rows.flatten() {
                result.push(info);
            }
            result
        };

        Ok(connections)
    }

    /// 解析连接 URL，提取 host, port, database, username, password
    #[allow(clippy::type_complexity)]
    fn parse_connection_url(
        db_type: &str,
        url: &str,
    ) -> (
        Option<String>,
        Option<i32>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        // 移除协议前缀
        let prefix = format!("{}://", db_type);
        let clean_url = if url.starts_with(&prefix) {
            &url[prefix.len()..]
        } else {
            url
        };

        // 文件型数据库（sqlite, duckdb）
        if db_type == "sqlite" || db_type == "duckdb" {
            return (None, None, Some(clean_url.to_string()), None, None);
        }

        // 网络型数据库：user:pass@host:port/database
        // 分离认证信息和主机信息
        let (auth_part, host_database_part) = if let Some(at_pos) = clean_url.find('@') {
            // 有认证信息：user:pass@host:port/database
            let auth = &clean_url[..at_pos];
            let host_db = &clean_url[at_pos + 1..];
            (Some(auth), host_db)
        } else {
            // 无认证信息：host:port/database
            (None, clean_url)
        };

        // 解析认证信息
        let (username, password) = if let Some(auth) = auth_part {
            if let Some(colon_pos) = auth.find(':') {
                let user = &auth[..colon_pos];
                let pass = &auth[colon_pos + 1..];
                (Some(user.to_string()), Some(pass.to_string()))
            } else {
                (Some(auth.to_string()), None)
            }
        } else {
            (None, None)
        };

        // 解析主机和数据库
        let parts: Vec<&str> = host_database_part.split('/').collect();
        if parts.len() < 2 {
            return (None, None, None, username, password);
        }

        let database = parts.last().map(|s| s.to_string());
        let host_port = parts[0];

        let host_port_parts: Vec<&str> = host_port.split(':').collect();
        let host = host_port_parts.first().map(|s| s.to_string());
        let port = if host_port_parts.len() > 1 {
            host_port_parts[1].parse::<i32>().ok()
        } else {
            // 默认端口
            match db_type {
                "mysql" => Some(3306),
                "postgres" => Some(5432),
                _ => None,
            }
        };

        (host, port, database, username, password)
    }

    /// 保存导航器状态
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    /// * `expanded_keys` - 展开的节点键（JSON 数组）
    /// * `selected_keys` - 选中的节点键（JSON 数组）
    /// * `filter_config` - 过滤器配置（JSON 对象）
    pub async fn save_navigator_state(
        &self,
        connection_id: &str,
        expanded_keys: &str,
        selected_keys: &str,
        filter_config: &str,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let id = format!("nav_{}", connection_id);

        conn.inner()?
            .execute(
                "INSERT OR REPLACE INTO navigator_states
             (id, connection_id, expanded_keys, selected_keys, filter_config, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)",
                [
                    &id,
                    connection_id,
                    expanded_keys,
                    selected_keys,
                    filter_config,
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_navigator_state".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }

    /// 加载导航器状态
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    pub async fn load_navigator_state(
        &self,
        connection_id: &str,
    ) -> Result<Option<(String, String, String)>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let result = {
            let mut stmt = conn
                .inner()?
                .prepare(
                    "SELECT expanded_keys, selected_keys, filter_config
                 FROM navigator_states
                 WHERE connection_id = ?1",
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "load_navigator_state".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            stmt.query_row([&connection_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_navigator_state_query".to_string(),
                    reason: e.to_string(),
                })
            })?
        };

        Ok(result)
    }

    // ==================== 项目管理 ====================

    /// 项目相关 SQL 常量
    #[allow(dead_code)]
    const PROJECT_SELECT_COLUMNS: &'static str =
        "id, name, description, path, status, created_at, updated_at, last_opened_at";

    const PROJECT_CHECK_ID_EXISTS: &'static str = "SELECT COUNT(*) FROM project_info WHERE id = ?1";

    const PROJECT_CHECK_PATH_EXISTS: &'static str =
        "SELECT COUNT(*) FROM project_info WHERE path = ?1";

    const PROJECT_UPDATE_BY_ID: &'static str =
        "UPDATE project_info SET name = ?2, description = ?3, path = ?4, status = ?5, updated_at = CURRENT_TIMESTAMP, last_opened_at = ?6 WHERE id = ?1";

    const PROJECT_UPDATE_BY_PATH: &'static str =
        "UPDATE project_info SET id = ?1, name = ?2, description = ?3, status = ?5, updated_at = CURRENT_TIMESTAMP, last_opened_at = ?6 WHERE path = ?4";

    const PROJECT_INSERT: &'static str =
        "INSERT INTO project_info (id, name, description, path, status, updated_at, last_opened_at) VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, ?6)";

    const PROJECT_DELETE: &'static str = "DELETE FROM project_info WHERE id = ?1";

    const PROJECT_UPDATE_INFO: &'static str =
        "UPDATE project_info SET name = ?2, description = ?3, updated_at = CURRENT_TIMESTAMP WHERE id = ?1";

    const PROJECT_INSERT_OR_REPLACE: &'static str =
        "INSERT OR REPLACE INTO project_info (id, name, description, path, status, updated_at, last_opened_at) VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP, ?6)";

    const PROJECT_SELECT_ALL: &'static str =
        "SELECT id, name, description, path, status, created_at, updated_at, last_opened_at FROM project_info ORDER BY last_opened_at DESC";

    const PROJECT_UPDATE_LAST_OPENED: &'static str =
        "UPDATE project_info SET last_opened_at = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2";

    const PROJECT_OPEN_UPDATE: &'static str =
        "UPDATE project_info SET last_opened_at = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2";

    const PROJECT_OPEN_QUERY: &'static str =
        "SELECT id, name, description, path, status, created_at, updated_at, last_opened_at FROM project_info WHERE id = ?1";

    const PROJECT_OPEN_BY_PATH_UPDATE: &'static str =
        "UPDATE project_info SET last_opened_at = ?1, updated_at = CURRENT_TIMESTAMP WHERE path = ?2";

    const PROJECT_OPEN_BY_PATH_QUERY: &'static str =
        "SELECT id, name, description, path, status, created_at, updated_at, last_opened_at FROM project_info WHERE path = ?1";

    const PROJECT_GET_RECENT: &'static str =
        "SELECT id, name, description, path, status, created_at, updated_at, last_opened_at FROM project_info WHERE last_opened_at IS NOT NULL AND last_opened_at != '' ORDER BY last_opened_at DESC LIMIT ?1";

    const PROJECT_GET_BY_ID: &'static str =
        "SELECT id, name, description, path, status, created_at, updated_at, last_opened_at FROM project_info WHERE id = ?1";

    const PROJECT_GET_BY_PATH: &'static str =
        "SELECT id, name, description, path, status, created_at, updated_at, last_opened_at FROM project_info WHERE path = ?1";

    /// 辅助函数：将数据库行转换为 ProjectInfoRecord
    fn row_to_project_record(
        row: &rusqlite::Row<'_>,
    ) -> Result<ProjectInfoRecord, rusqlite::Error> {
        Ok(ProjectInfoRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2).ok(),
            path: row.get(3)?,
            status: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
            last_opened_at: row.get(7).ok(),
        })
    }

    /// 辅助函数：创建 SQLite 持久化错误
    fn sqlite_persistence_error(operation: &str, reason: String) -> CoreError {
        CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: operation.to_string(),
            reason,
        })
    }

    /// 智能保存项目信息（处理路径冲突）
    ///
    /// 逻辑：
    /// 1. 如果项目ID已存在，更新路径和其他信息
    /// 2. 如果路径已存在但ID不同，更新该记录的ID
    /// 3. 如果都不存在，插入新记录
    ///
    /// # 参数
    /// * `id` - 项目 ID
    /// * `name` - 项目名称
    /// * `description` - 项目描述
    /// * `path` - 项目路径
    /// * `status` - 项目状态
    /// * `last_opened_at` - 最后打开时间
    pub async fn save_project_info_smart(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        path: &str,
        status: &str,
        last_opened_at: Option<&str>,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let id_exists = conn
            .inner()?
            .query_row(Self::PROJECT_CHECK_ID_EXISTS, [id], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|e| Self::sqlite_persistence_error("check_project_id", e.to_string()))?
            > 0;

        let path_exists = conn
            .inner()?
            .query_row(Self::PROJECT_CHECK_PATH_EXISTS, [path], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|e| Self::sqlite_persistence_error("check_project_path", e.to_string()))?
            > 0;

        let desc = description.unwrap_or("");
        let last_opened = last_opened_at.unwrap_or("");

        if id_exists {
            conn.inner()?
                .execute(
                    Self::PROJECT_UPDATE_BY_ID,
                    [id, name, desc, path, status, last_opened],
                )
                .map_err(|e| {
                    Self::sqlite_persistence_error("update_project_by_id", e.to_string())
                })?;
        } else if path_exists {
            conn.inner()?
                .execute(
                    Self::PROJECT_UPDATE_BY_PATH,
                    [id, name, desc, path, status, last_opened],
                )
                .map_err(|e| {
                    Self::sqlite_persistence_error("update_project_by_path", e.to_string())
                })?;
        } else {
            conn.inner()?
                .execute(
                    Self::PROJECT_INSERT,
                    [id, name, desc, path, status, last_opened],
                )
                .map_err(|e| Self::sqlite_persistence_error("insert_project", e.to_string()))?;
        }

        Ok(())
    }

    /// 删除项目信息
    ///
    /// # 参数
    /// * `id` - 项目 ID
    pub async fn delete_project(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        conn.inner()?
            .execute(Self::PROJECT_DELETE, [id])
            .map_err(|e| Self::sqlite_persistence_error("delete_project", e.to_string()))?;

        Ok(())
    }

    /// 更新项目信息（名称、描述）
    ///
    /// # 参数
    /// * `id` - 项目 ID
    /// * `name` - 新项目名称
    /// * `description` - 新项目描述
    pub async fn update_project_info(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        conn.inner()?
            .execute(
                Self::PROJECT_UPDATE_INFO,
                [id, name, description.unwrap_or("")],
            )
            .map_err(|e| Self::sqlite_persistence_error("update_project_info", e.to_string()))?;

        Ok(())
    }

    /// 保存项目信息（简单插入或替换）
    ///
    /// # 参数
    /// * `id` - 项目 ID
    /// * `name` - 项目名称
    /// * `description` - 项目描述
    /// * `path` - 项目路径
    /// * `status` - 项目状态
    /// * `last_opened_at` - 最后打开时间
    pub async fn save_project_info(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        path: &str,
        status: &str,
        last_opened_at: Option<&str>,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        conn.inner()?
            .execute(
                Self::PROJECT_INSERT_OR_REPLACE,
                [
                    id,
                    name,
                    description.unwrap_or(""),
                    path,
                    status,
                    last_opened_at.unwrap_or(""),
                ],
            )
            .map_err(|e| Self::sqlite_persistence_error("save_project_info", e.to_string()))?;

        Ok(())
    }

    /// 获取所有项目信息
    ///
    /// # 返回
    /// 返回项目信息列表
    pub async fn get_all_projects(&self) -> Result<Vec<ProjectInfoRecord>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let projects = {
            let mut stmt = conn
                .inner()?
                .prepare(Self::PROJECT_SELECT_ALL)
                .map_err(|e| Self::sqlite_persistence_error("get_all_projects", e.to_string()))?;

            let rows = stmt
                .query_map([], Self::row_to_project_record)
                .map_err(|e| Self::sqlite_persistence_error("query_all_projects", e.to_string()))?;

            let mut result = Vec::new();
            for record in rows.flatten() {
                result.push(record);
            }
            result
        };

        Ok(projects)
    }

    /// 更新项目最后打开时间
    ///
    /// # 参数
    /// * `id` - 项目 ID
    /// * `last_opened_at` - 最后打开时间
    pub async fn update_project_last_opened(
        &self,
        id: &str,
        last_opened_at: &str,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        conn.inner()?
            .execute(Self::PROJECT_UPDATE_LAST_OPENED, [last_opened_at, id])
            .map_err(|e| {
                Self::sqlite_persistence_error("update_project_last_opened", e.to_string())
            })?;

        Ok(())
    }

    /// 删除项目信息
    ///
    /// # 参数
    /// * `id` - 项目 ID
    pub async fn delete_project_info(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        conn.inner()?
            .execute(Self::PROJECT_DELETE, [id])
            .map_err(|e| Self::sqlite_persistence_error("delete_project_info", e.to_string()))?;

        Ok(())
    }

    /// 打开项目（更新最后打开时间并返回项目信息）
    ///
    /// 使用单次数据库操作完成查询和更新
    ///
    /// # 参数
    /// * `id` - 项目 ID
    ///
    /// # 返回
    /// 返回更新后的项目信息（如果存在）
    pub async fn open_project(&self, id: &str) -> Result<Option<ProjectInfoRecord>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let now = chrono::Utc::now().to_rfc3339();

        let updated = conn
            .inner()?
            .execute(Self::PROJECT_OPEN_UPDATE, [&now, id])
            .map_err(|e| Self::sqlite_persistence_error("open_project_update", e.to_string()))?;

        if updated == 0 {
            return Ok(None);
        }

        let project = {
            let mut stmt = conn
                .inner()?
                .prepare(Self::PROJECT_OPEN_QUERY)
                .map_err(|e| Self::sqlite_persistence_error("open_project_query", e.to_string()))?;

            stmt.query_row([id], Self::row_to_project_record)
                .optional()
                .map_err(|e| Self::sqlite_persistence_error("open_project_query", e.to_string()))?
        };

        Ok(project)
    }

    /// 根据路径打开项目（更新最后打开时间并返回项目信息）
    ///
    /// # 参数
    /// * `path` - 项目路径
    ///
    /// # 返回
    /// 返回更新后的项目信息（如果存在）
    pub async fn open_project_by_path(
        &self,
        path: &str,
    ) -> Result<Option<ProjectInfoRecord>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let now = chrono::Utc::now().to_rfc3339();

        let updated = conn
            .inner()?
            .execute(Self::PROJECT_OPEN_BY_PATH_UPDATE, [&now, path])
            .map_err(|e| {
                Self::sqlite_persistence_error("open_project_by_path_update", e.to_string())
            })?;

        if updated == 0 {
            return Ok(None);
        }

        let project = {
            let mut stmt = conn
                .inner()?
                .prepare(Self::PROJECT_OPEN_BY_PATH_QUERY)
                .map_err(|e| {
                    Self::sqlite_persistence_error("open_project_by_path_query", e.to_string())
                })?;

            stmt.query_row([path], Self::row_to_project_record)
                .optional()
                .map_err(|e| {
                    Self::sqlite_persistence_error("open_project_by_path_query", e.to_string())
                })?
        };

        Ok(project)
    }

    /// 获取最近打开的项目（按 last_opened_at 降序）
    ///
    /// # 参数
    /// * `limit` - 返回数量限制
    ///
    /// # 返回
    /// 返回最近项目列表
    pub async fn get_recent_projects(
        &self,
        limit: usize,
    ) -> Result<Vec<ProjectInfoRecord>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let projects = {
            let mut stmt = conn
                .inner()?
                .prepare(Self::PROJECT_GET_RECENT)
                .map_err(|e| {
                    Self::sqlite_persistence_error("get_recent_projects", e.to_string())
                })?;

            let rows = stmt
                .query_map([limit as i64], Self::row_to_project_record)
                .map_err(|e| {
                    Self::sqlite_persistence_error("query_recent_projects", e.to_string())
                })?;

            let mut result = Vec::new();
            for record in rows.flatten() {
                result.push(record);
            }
            result
        };

        Ok(projects)
    }

    /// 根据 ID 获取项目信息（不更新最后打开时间）
    ///
    /// # 参数
    /// * `id` - 项目 ID
    ///
    /// # 返回
    /// 返回项目信息（如果存在）
    pub async fn get_project_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ProjectInfoRecord>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let project = {
            let mut stmt = conn
                .inner()?
                .prepare(Self::PROJECT_GET_BY_ID)
                .map_err(|e| Self::sqlite_persistence_error("get_project_by_id", e.to_string()))?;

            stmt.query_row([id], Self::row_to_project_record)
                .optional()
                .map_err(|e| Self::sqlite_persistence_error("query_project_by_id", e.to_string()))?
        };

        Ok(project)
    }

    /// 根据路径获取项目信息
    ///
    /// # 参数
    /// * `path` - 项目路径
    ///
    /// # 返回
    /// 返回项目信息（如果存在）
    pub async fn get_project_by_path(
        &self,
        path: &str,
    ) -> Result<Option<ProjectInfoRecord>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;

        let project = {
            let mut stmt = conn
                .inner()?
                .prepare(Self::PROJECT_GET_BY_PATH)
                .map_err(|e| {
                    Self::sqlite_persistence_error("get_project_by_path", e.to_string())
                })?;

            stmt.query_row([path], Self::row_to_project_record)
                .optional()
                .map_err(|e| {
                    Self::sqlite_persistence_error("query_project_by_path", e.to_string())
                })?
        };

        Ok(project)
    }

    /// 关闭所有连接
    pub async fn close(&self) -> Result<(), CoreError> {
        self.duckdb_conn.close().await?;
        Ok(())
    }

    /// 获取 SQL 模板存储
    pub fn get_sql_template_store(&self) -> Result<SqlTemplateStore, CoreError> {
        SqlTemplateStore::new(self.sqlite_pool.clone())
    }

    /// 获取工作台上下文存储
    pub fn get_workbench_context_store(&self) -> Result<WorkbenchContextStore, CoreError> {
        WorkbenchContextStore::new(self.sqlite_pool.clone())
    }

    /// 列出所有数据源类型
    pub async fn list_data_source_types(&self) -> Result<Vec<DataSourceType>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = driver_store::get_data_source_types(conn.inner()?)?;

        Ok(result)
    }

    /// 获取所有驱动定义
    pub async fn get_all_drivers(&self) -> Result<Vec<Driver>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = driver_store::get_all_drivers(conn.inner()?)?;

        Ok(result)
    }

    /// 根据 ID 获取单个驱动定义
    pub async fn get_driver(&self, id: &str) -> Result<Option<Driver>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = driver_store::get_driver(conn.inner()?, id)?;

        Ok(result)
    }

    /// 获取指定数据源类型下的所有已启用驱动
    pub async fn get_drivers_by_type(&self, type_id: &str) -> Result<Vec<Driver>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let drivers = driver_store::get_drivers_by_type(conn.inner()?, type_id)?;

        Ok(drivers)
    }

    /// 列出指定驱动在本机已安装的文件版本
    pub async fn list_driver_files(&self, driver_id: &str) -> Result<Vec<DriverFile>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = driver_store::list_driver_files(conn.inner()?, driver_id)?;

        Ok(result)
    }

    /// 注册驱动文件到本机安装记录
    pub async fn register_driver_file(&self, df: &DriverFile) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        driver_store::register_driver_file(conn.inner()?, df)?;

        Ok(())
    }

    /// 检查指定版本的驱动是否已安装
    pub async fn is_driver_installed(
        &self,
        driver_id: &str,
        version: &str,
    ) -> Result<bool, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = driver_store::is_driver_file_installed(conn.inner()?, driver_id, version)?;

        Ok(result)
    }

    /// 创建新环境
    pub async fn create_environment(&self, env: &env_store::Environment) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        env_store::create_environment(conn.inner()?, env)?;

        Ok(())
    }

    /// 列出所有环境
    pub async fn list_environments(&self) -> Result<Vec<env_store::Environment>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = env_store::list_environments(conn.inner()?)?;

        Ok(result)
    }

    /// 更新环境信息
    pub async fn update_environment(&self, env: &env_store::Environment) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        env_store::update_environment(conn.inner()?, env)?;

        Ok(())
    }

    /// 删除环境
    pub async fn delete_environment(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        env_store::delete_environment(conn.inner()?, id)?;

        Ok(())
    }

    /// 根据 ID 获取环境
    pub async fn get_environment(
        &self,
        id: &str,
    ) -> Result<Option<env_store::Environment>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = env_store::get_environment(conn.inner()?, id)?;

        Ok(result)
    }

    /// 为指定环境创建策略
    pub async fn create_environment_policy(
        &self,
        policy: &env_store::EnvironmentPolicy,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        env_store::create_policy(conn.inner()?, policy)?;

        Ok(())
    }

    /// 列出指定环境的所有策略
    pub async fn list_environment_policies(
        &self,
        environment_id: &str,
    ) -> Result<Vec<env_store::EnvironmentPolicy>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = env_store::list_policies(conn.inner()?, environment_id)?;

        Ok(result)
    }

    /// 更新环境策略
    pub async fn update_environment_policy(
        &self,
        policy: &env_store::EnvironmentPolicy,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        env_store::update_policy(conn.inner()?, policy)?;

        Ok(())
    }

    /// 删除环境策略
    pub async fn delete_environment_policy(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        env_store::delete_policy(conn.inner()?, id)?;

        Ok(())
    }

    /// 创建认证配置
    pub async fn create_auth_config(&self, ac: &auth_store::AuthConfig) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        auth_store::create_global_auth_config(conn.inner()?, ac)?;

        Ok(())
    }

    /// 列出认证配置
    pub async fn list_auth_configs(
        &self,
        auth_type: Option<&str>,
    ) -> Result<Vec<auth_store::AuthConfig>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = auth_store::list_global_auth_configs(conn.inner()?, auth_type)?;

        Ok(result)
    }

    /// 根据 ID 获取认证配置
    pub async fn get_auth_config(
        &self,
        id: &str,
    ) -> Result<Option<auth_store::AuthConfig>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = auth_store::get_global_auth_config(conn.inner()?, id)?;

        Ok(result)
    }

    /// 删除认证配置
    pub async fn delete_auth_config(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        auth_store::delete_auth_config(conn.inner()?, id)?;

        Ok(())
    }

    /// 更新认证配置
    pub async fn update_auth_config(&self, ac: &auth_store::AuthConfig) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        auth_store::update_auth_config(conn.inner()?, ac)?;

        Ok(())
    }

    /// 创建网络配置
    pub async fn create_network_config(
        &self,
        nc: &network_store::NetworkConfig,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        network_store::create_global_network_config(conn.inner()?, nc)?;

        Ok(())
    }

    /// 列出网络配置
    pub async fn list_network_configs(
        &self,
        network_type: Option<&str>,
    ) -> Result<Vec<network_store::NetworkConfig>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = network_store::list_global_network_configs(conn.inner()?, network_type)?;

        Ok(result)
    }

    /// 根据 ID 获取网络配置
    pub async fn get_network_config(
        &self,
        id: &str,
    ) -> Result<Option<network_store::NetworkConfig>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = network_store::get_global_network_config(conn.inner()?, id)?;

        Ok(result)
    }

    /// 更新网络配置
    pub async fn update_network_config(
        &self,
        nc: &network_store::NetworkConfig,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        network_store::update_network_config(conn.inner()?, nc)?;

        Ok(())
    }

    /// 删除网络配置
    pub async fn delete_network_config(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        network_store::delete_network_config(conn.inner()?, id)?;

        Ok(())
    }

    // ==================== 插件管理 ====================

    /// 注册插件到全局插件中心
    pub async fn register_plugin(&self, plugin: &plugin_store::Plugin) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        plugin_store::register_plugin(conn.inner()?, plugin)?;

        Ok(())
    }

    /// 根据 ID 获取插件
    pub async fn get_plugin(&self, id: &str) -> Result<Option<plugin_store::Plugin>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = plugin_store::get_plugin(conn.inner()?, id)?;

        Ok(result)
    }

    /// 根据 code 和 version 获取插件
    pub async fn get_plugin_by_code_version(
        &self,
        code: &str,
        version: &str,
    ) -> Result<Option<plugin_store::Plugin>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = plugin_store::get_plugin_by_code_version(conn.inner()?, code, version)?;

        Ok(result)
    }

    /// 获取所有已安装插件
    pub async fn get_all_plugins(&self) -> Result<Vec<plugin_store::Plugin>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = plugin_store::get_all_plugins(conn.inner()?)?;

        Ok(result)
    }

    /// 更新插件启用状态
    pub async fn update_plugin_enabled(&self, id: &str, is_enabled: bool) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        plugin_store::update_plugin_enabled(conn.inner()?, id, is_enabled)?;

        Ok(())
    }

    /// 删除插件
    pub async fn delete_plugin(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        plugin_store::delete_plugin(conn.inner()?, id)?;

        Ok(())
    }

    /// 注册插件依赖
    pub async fn register_plugin_dependency(
        &self,
        dep: &plugin_store::PluginDependency,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        plugin_store::register_plugin_dependency(conn.inner()?, dep)?;

        Ok(())
    }

    /// 获取插件的所有依赖
    pub async fn get_plugin_dependencies(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<plugin_store::PluginDependency>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = plugin_store::get_plugin_dependencies(conn.inner()?, plugin_id)?;

        Ok(result)
    }

    /// 设置插件全局配置
    pub async fn set_plugin_global_config(
        &self,
        config: &plugin_store::PluginGlobalConfig,
    ) -> Result<(), CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        plugin_store::set_plugin_global_config(conn.inner()?, config)?;

        Ok(())
    }

    /// 获取插件全局配置
    pub async fn get_plugin_global_configs(
        &self,
        plugin_id: &str,
    ) -> Result<Vec<plugin_store::PluginGlobalConfig>, CoreError> {
        let conn = self.sqlite_pool.acquire().await?;
        let result = plugin_store::get_plugin_global_configs(conn.inner()?, plugin_id)?;

        Ok(result)
    }
}

/// 项目信息记录
#[derive(Debug, Clone)]
pub struct ProjectInfoRecord {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub path: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_opened_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rdata_test_global_db_{}", name));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[tokio::test]
    #[ignore = "内部状态检查存在竞态条件"]
    async fn test_global_sqlite_pool() {
        let db_path = test_temp_dir("sqlite_pool").join("test.db");

        let pool = GlobalSqlitePool::new(db_path.clone(), 3)
            .await
            .expect("创建池失败");
        assert_eq!(pool.semaphore.available_permits(), 3);

        let _conn = pool.acquire().await.expect("获取连接失败");
        assert_eq!(pool.semaphore.available_permits(), 2);

        drop(_conn);
        assert_eq!(pool.semaphore.available_permits(), 3);
    }

    #[tokio::test]
    async fn test_global_duckdb_connection() -> Result<(), CoreError> {
        let db_path = test_temp_dir("duckdb_conn").join("test.duckdb");

        let conn = GlobalDuckdbConnection::new(db_path.clone()).await?;
        assert!(conn.acquire().await.is_ok());

        conn.close().await?;
        assert!(conn.acquire().await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_global_database_manager() {
        let base = test_temp_dir("manager");
        let sqlite_path = base.join("system.db");
        let duckdb_path = base.join("analytics.duckdb");

        let manager = GlobalDatabaseManager::new(sqlite_path, duckdb_path, 3)
            .await
            .unwrap();
        assert!(manager.sqlite_pool().acquire().await.is_ok());
        assert!(manager.duckdb_conn().acquire().await.is_ok());

        manager.close().await.unwrap();
    }
}
