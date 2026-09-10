/**
 * 项目存储 API
 *
 * 提供项目级数据的 CRUD 操作
 */
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::core::driver::utils::escape_sql_string;
use crate::core::error::{CoreError, StorageError};
use crate::core::persistence::project_db::ProjectDatabaseManager;

/// 连接信息
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StoredConnection {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub host: Option<String>,
    pub port: Option<i32>,
    pub database: Option<String>,
    pub schema_name: Option<String>,
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_encrypted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<String>,
    pub tags: Option<String>,
    pub use_duckdb_fed: bool,
    pub metadata_path: Option<String>,
    pub is_active: bool,
    pub server_version: Option<String>,
    pub description: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// SQL 历史记录
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SqlHistoryRecord {
    pub id: String,
    pub connection_id: Option<String>,
    pub sql_text: String,
    pub execution_time_ms: Option<i32>,
    pub rows_affected: Option<i32>,
    pub error_message: Option<String>,
    pub is_favorite: bool,
    pub created_at: String,
}

/// 项目设置
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProjectSetting {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

/// 工作台状态
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkbenchState {
    pub layout: Option<String>,
    pub open_panels: Option<String>,
    pub active_panel_id: Option<String>,
}

/// 项目存储
pub struct ProjectStore {
    pub db_manager: Arc<ProjectDatabaseManager>,
}

impl ProjectStore {
    /// 创建项目存储实例
    pub async fn new(project_path: &Path) -> Result<Self, CoreError> {
        let db_manager = Arc::new(ProjectDatabaseManager::open(project_path, 3).await?);

        Ok(Self { db_manager })
    }

    /// 获取项目数据库管理器
    pub fn project_db(&self) -> Arc<ProjectDatabaseManager> {
        Arc::clone(&self.db_manager)
    }

    // ==================== 连接管理 ====================

    /// 保存连接
    pub async fn save_connection(&self, conn: &StoredConnection) -> Result<(), CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        let host = conn.host.clone().unwrap_or_default();
        let port = conn.port.map(|p| p.to_string()).unwrap_or_default();
        let database = conn.database.clone().unwrap_or_default();
        let schema_name = conn.schema_name.clone().unwrap_or_default();
        let username = conn.username.clone().unwrap_or_default();
        let password = conn.password_encrypted.clone().unwrap_or_default();
        let options = conn.options.clone().unwrap_or_default();
        let tags = conn.tags.clone().unwrap_or_default();
        let use_duckdb_fed = conn.use_duckdb_fed.to_string();
        let metadata_path = conn.metadata_path.clone().unwrap_or_default();
        let is_active = conn.is_active.to_string();
        let server_version = conn.server_version.clone().unwrap_or_default();
        let description = conn.description.clone().unwrap_or_default();
        let driver_id = conn.driver_id.clone().unwrap_or_default();
        let environment_id = conn.environment_id.clone().unwrap_or_default();
        let auth_config_id = conn.auth_config_id.clone().unwrap_or_default();
        let auth_method = conn.auth_method.clone().unwrap_or_default();
        let network_config_id = conn.network_config_id.clone().unwrap_or_default();
        let driver_properties = conn.driver_properties.clone().unwrap_or_default();
        let advanced_options = conn.advanced_options.clone().unwrap_or_default();

        sqlite.inner()?.execute(
            "INSERT OR REPLACE INTO connections
             (id, name, driver, host, port, database, schema_name, username, password_encrypted, options, tags, use_duckdb_fed, metadata_path, is_active, server_version, description, driver_id, environment_id, auth_config_id, auth_method, network_config_id, driver_properties, advanced_options, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            [
                &conn.id,
                &conn.name,
                &conn.driver,
                &host,
                &port,
                &database,
                &schema_name,
                &username,
                &password,
                &options,
                &tags,
                &use_duckdb_fed,
                &metadata_path,
                &is_active,
                &server_version,
                &description,
                &driver_id,
                &environment_id,
                &auth_config_id,
                &auth_method,
                &network_config_id,
                &driver_properties,
                &advanced_options,
                &conn.created_at,
                &conn.updated_at,
            ],
        ).map_err(|e| CoreError::Storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "save_connection".to_string(),
            reason: e.to_string()
        }))?;

        // sqlite 在 drop 时自动归还到连接池

        Ok(())
    }

    /// 获取所有连接
    pub async fn get_connections(&self) -> Result<Vec<StoredConnection>, CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        let mut stmt = sqlite.inner()?.prepare(
            "SELECT id, name, driver, host, port, database, schema_name, username, password_encrypted, options, tags, use_duckdb_fed, metadata_path, is_active, created_at, updated_at, server_version, description, driver_id, environment_id, auth_config_id, auth_method, network_config_id, driver_properties, advanced_options
             FROM connections WHERE is_active = 1 ORDER BY created_at DESC"
        ).map_err(|e| CoreError::Storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "get_connections".to_string(),
            reason: e.to_string()
        }))?;

        let connections = stmt
            .query_map([], |row| {
                Ok(StoredConnection {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    driver: row.get(2)?,
                    host: row.get(3).ok(),
                    port: row.get::<_, i32>(4).ok(),
                    database: row.get(5).ok(),
                    schema_name: row.get(6).ok(),
                    username: row.get(7).ok(),
                    password_encrypted: row.get(8).ok(),
                    options: row.get(9).ok(),
                    tags: row.get(10).ok(),
                    use_duckdb_fed: row.get::<_, bool>(11)?,
                    metadata_path: row.get(12).ok(),
                    is_active: row.get::<_, bool>(13)?,
                    created_at: row.get(14)?,
                    updated_at: row.get(15)?,
                    server_version: row.get(16).ok(),
                    description: row.get(17).ok(),
                    driver_id: row.get(18).ok(),
                    environment_id: row.get(19).ok(),
                    auth_config_id: row.get(20).ok(),
                    auth_method: row.get(21).ok(),
                    network_config_id: row.get(22).ok(),
                    driver_properties: row.get(23).ok(),
                    advanced_options: row.get(24).ok(),
                })
            })
            .map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_connections".to_string(),
                    reason: e.to_string(),
                })
            })?;

        connections.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::Storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "collect_connections".to_string(),
                reason: e.to_string(),
            })
        })
        // sqlite 在 drop 时自动归还到连接池
    }

    /// 获取单个连接
    pub async fn get_connection(&self, id: &str) -> Result<Option<StoredConnection>, CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        let mut stmt = sqlite.inner()?.prepare(
            "SELECT id, name, driver, host, port, database, schema_name, username, password_encrypted, options, tags, use_duckdb_fed, metadata_path, is_active, created_at, updated_at, server_version, description, driver_id, environment_id, auth_config_id, auth_method, network_config_id, driver_properties, advanced_options
             FROM connections WHERE id = ?1 AND is_active = 1"
        ).map_err(|e| CoreError::Storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "prepare_get_connection".to_string(),
            reason: e.to_string()
        }))?;

        let result = stmt.query_row([id], |row| {
            Ok(StoredConnection {
                id: row.get(0)?,
                name: row.get(1)?,
                driver: row.get(2)?,
                host: row.get(3).ok(),
                port: row.get::<_, i32>(4).ok(),
                database: row.get(5).ok(),
                schema_name: row.get(6).ok(),
                username: row.get(7).ok(),
                password_encrypted: row.get(8).ok(),
                options: row.get(9).ok(),
                tags: row.get(10).ok(),
                use_duckdb_fed: row.get::<_, bool>(11)?,
                metadata_path: row.get(12).ok(),
                is_active: row.get::<_, bool>(13)?,
                created_at: row.get(14)?,
                updated_at: row.get(15)?,
                server_version: row.get(16).ok(),
                description: row.get(17).ok(),
                driver_id: row.get(18).ok(),
                environment_id: row.get(19).ok(),
                auth_config_id: row.get(20).ok(),
                auth_method: row.get(21).ok(),
                network_config_id: row.get(22).ok(),
                driver_properties: row.get(23).ok(),
                advanced_options: row.get(24).ok(),
            })
        });

        // sqlite 在 drop 时自动归还到连接池

        match result {
            Ok(conn) => Ok(Some(conn)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CoreError::Storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_connection".to_string(),
                reason: e.to_string(),
            })),
        }
    }

    /// 删除连接（软删除）
    pub async fn delete_connection(&self, id: &str) -> Result<(), CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        sqlite.inner()?.execute(
            "UPDATE connections SET is_active = 0, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            [id],
        ).map_err(|e| CoreError::Storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "delete_connection".to_string(),
            reason: e.to_string()
        }))?;

        // sqlite 在 drop 时自动归还到连接池

        Ok(())
    }

    // ==================== SQL 历史 ====================

    /// 保存 SQL 历史
    pub async fn save_sql_history(&self, record: &SqlHistoryRecord) -> Result<(), CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        sqlite.inner()?.execute(
            "INSERT INTO sql_history
             (id, connection_id, sql_text, execution_time_ms, rows_affected, error_message, is_favorite, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            [
                &record.id,
                &record.connection_id.clone().unwrap_or_default(),
                &record.sql_text,
                &record.execution_time_ms.map(|t| t.to_string()).unwrap_or_default(),
                &record.rows_affected.map(|r| r.to_string()).unwrap_or_default(),
                &record.error_message.clone().unwrap_or_default(),
                &record.is_favorite.to_string(),
                &record.created_at,
            ],
        ).map_err(|e| CoreError::Storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "save_sql_history".to_string(),
            reason: e.to_string()
        }))?;

        // sqlite 在 drop 时自动归还到连接池

        Ok(())
    }

    /// 获取 SQL 历史
    pub async fn get_sql_history(
        &self,
        connection_id: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<SqlHistoryRecord>, CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        let query = if let Some(conn_id) = connection_id {
            format!(
                "SELECT id, connection_id, sql_text, execution_time_ms, rows_affected, error_message, is_favorite, created_at
                 FROM sql_history WHERE connection_id = '{}' ORDER BY created_at DESC LIMIT {}",
                escape_sql_string(conn_id),
                limit.unwrap_or(100)
            )
        } else {
            format!(
                "SELECT id, connection_id, sql_text, execution_time_ms, rows_affected, error_message, is_favorite, created_at
                 FROM sql_history ORDER BY created_at DESC LIMIT {}",
                limit.unwrap_or(100)
            )
        };

        let mut stmt = sqlite.inner()?.prepare(&query).map_err(|e| {
            CoreError::Storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_sql_history".to_string(),
                reason: e.to_string(),
            })
        })?;

        let records = stmt
            .query_map([], |row| {
                Ok(SqlHistoryRecord {
                    id: row.get(0)?,
                    connection_id: row.get(1).ok().filter(|s: &String| !s.is_empty()),
                    sql_text: row.get(2)?,
                    execution_time_ms: row.get::<_, i32>(3).ok(),
                    rows_affected: row.get::<_, i32>(4).ok(),
                    error_message: row.get(5).ok().filter(|s: &String| !s.is_empty()),
                    is_favorite: row.get::<_, bool>(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_sql_history".to_string(),
                    reason: e.to_string(),
                })
            })?;

        // sqlite 在 drop 时自动归还到连接池

        records.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::Storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "collect_sql_history".to_string(),
                reason: e.to_string(),
            })
        })
    }

    // ==================== 设置管理 ====================

    /// 保存设置
    pub async fn save_setting(&self, key: &str, value: &str) -> Result<(), CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        sqlite.inner()?.execute(
            "INSERT OR REPLACE INTO project_settings (key, value, updated_at) VALUES (?1, ?2, CURRENT_TIMESTAMP)",
            [key, value],
        ).map_err(|e| CoreError::Storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "save_setting".to_string(),
            reason: e.to_string()
        }))?;

        // sqlite 在 drop 时自动归还到连接池

        Ok(())
    }

    /// 获取设置
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        let result: Result<String, _> = sqlite.inner()?.query_row(
            "SELECT value FROM project_settings WHERE key = ?1",
            [key],
            |row| row.get(0),
        );

        // sqlite 在 drop 时自动归还到连接池

        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CoreError::Storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_setting".to_string(),
                reason: e.to_string(),
            })),
        }
    }

    /// 获取所有设置
    pub async fn get_all_settings(&self) -> Result<HashMap<String, String>, CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        let mut stmt = sqlite
            .inner()?
            .prepare("SELECT key, value FROM project_settings")
            .map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "prepare_settings".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let settings = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_settings".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = HashMap::new();
        for setting in settings {
            let (key, value) = setting.map_err(|e| {
                CoreError::Storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "collect_settings".to_string(),
                    reason: e.to_string(),
                })
            })?;
            result.insert(key, value);
        }

        // sqlite 在 drop 时自动归还到连接池

        Ok(result)
    }

    // ==================== 工作台状态 ====================

    /// 保存工作台状态
    pub async fn save_workbench_state(&self, state: &WorkbenchState) -> Result<(), CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        sqlite.inner()?.execute(
            "INSERT OR REPLACE INTO workbench_state (id, layout, open_panels, active_panel_id, updated_at)
             VALUES ('default', ?1, ?2, ?3, CURRENT_TIMESTAMP)",
            [
                state.layout.as_deref().unwrap_or(""),
                state.open_panels.as_deref().unwrap_or(""),
                state.active_panel_id.as_deref().unwrap_or(""),
            ],
        ).map_err(|e| CoreError::Storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "save_workbench_state".to_string(),
            reason: e.to_string()
        }))?;

        // sqlite 在 drop 时自动归还到连接池

        Ok(())
    }

    /// 获取工作台状态
    pub async fn get_workbench_state(&self) -> Result<Option<WorkbenchState>, CoreError> {
        let sqlite = self.db_manager.sqlite_pool().acquire().await?;

        let result = sqlite.inner()?.query_row(
            "SELECT layout, open_panels, active_panel_id FROM workbench_state WHERE id = 'default'",
            [],
            |row| {
                Ok(WorkbenchState {
                    layout: row.get(0).ok().filter(|s: &String| !s.is_empty()),
                    open_panels: row.get(1).ok().filter(|s: &String| !s.is_empty()),
                    active_panel_id: row.get(2).ok().filter(|s: &String| !s.is_empty()),
                })
            },
        );

        // sqlite 在 drop 时自动归还到连接池

        match result {
            Ok(state) => Ok(Some(state)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CoreError::Storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_workbench_state".to_string(),
                reason: e.to_string(),
            })),
        }
    }
}
