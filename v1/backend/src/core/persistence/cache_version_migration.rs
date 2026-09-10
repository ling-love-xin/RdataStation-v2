/**
 * 缓存版本迁移管理器
 *
 * 负责后端 SQLite 缓存的版本管理和迁移：
 * - 检测缓存版本变化
 * - 自动执行版本迁移
 * - 支持回滚机制
 * - 记录迁移历史
 */
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use crate::core::error::{CommonError, CoreError, StorageError};

/// 当前缓存版本
///
/// 每次缓存结构变化时递增此版本号
pub const CURRENT_CACHE_VERSION: u32 = 8;

/// 缓存版本信息
#[derive(Debug, Clone)]
pub struct CacheVersionInfo {
    /// 当前版本号
    pub version: u32,
    /// 最后升级时间
    pub upgraded_at: Option<i64>,
    /// 升级原因
    pub upgrade_reason: Option<String>,
    /// 创建时间
    pub created_at: i64,
    /// 更新时间
    pub updated_at: i64,
}

/// 迁移记录
#[derive(Debug, Clone)]
pub struct CacheMigrationRecord {
    /// 迁移前版本
    pub from_version: u32,
    /// 迁移后版本
    pub to_version: u32,
    /// 迁移时间戳
    pub migrated_at: i64,
    /// 迁移原因
    pub reason: Option<String>,
    /// 迁移耗时（毫秒）
    pub duration_ms: Option<i64>,
    /// 是否成功
    pub success: bool,
}

/// 迁移策略
pub trait MigrationStrategy: Send + Sync {
    /// 目标版本
    fn target_version(&self) -> u32;

    /// 执行迁移
    fn migrate(&self, conn: &Connection) -> Result<(), CoreError>;

    /// 是否可回滚
    fn can_rollback(&self) -> bool {
        false
    }

    /// 回滚迁移
    fn rollback(&self, _conn: &Connection) -> Result<(), CoreError> {
        Err(CoreError::common(CommonError::General(
            "Rollback not implemented".to_string(),
        )))
    }

    /// 迁移原因
    fn reason(&self) -> &'static str {
        "自动版本升级"
    }
}

/// 版本 1 到版本 2 的迁移策略
pub struct V1ToV2Migration;

impl MigrationStrategy for V1ToV2Migration {
    fn target_version(&self) -> u32 {
        2
    }

    fn migrate(&self, conn: &Connection) -> Result<(), CoreError> {
        // 版本 2 的迁移 SQL 已经在 002_add_cache_version_and_compression.sql 中定义
        // 这里只需要确保迁移被正确记录

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        // 更新缓存版本记录
        conn.execute(
            "UPDATE cache_version SET version = ?1, upgraded_at = ?2, updated_at = ?3 WHERE id = 1",
            rusqlite::params![CURRENT_CACHE_VERSION, now, now],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "update_cache_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 记录迁移历史
        conn.execute(
            "INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![1, CURRENT_CACHE_VERSION, now, "升级到版本 2：添加缓存版本控制和压缩支持"],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "record_migration_history".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(())
    }

    fn reason(&self) -> &'static str {
        "升级到版本 2：添加缓存版本控制和压缩支持"
    }
}

/// 版本 2 到版本 3 的迁移策略
pub struct V2ToV3Migration;

impl MigrationStrategy for V2ToV3Migration {
    fn target_version(&self) -> u32 {
        3
    }

    fn migrate(&self, conn: &Connection) -> Result<(), CoreError> {
        // 版本 3 的迁移 SQL 已经在 003_add_fts_search.sql 中定义
        // 这里只需要确保迁移被正确记录

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        // 更新缓存版本记录
        conn.execute(
            "UPDATE cache_version SET version = ?1, upgraded_at = ?2, updated_at = ?3 WHERE id = 1",
            rusqlite::params![CURRENT_CACHE_VERSION, now, now],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "update_cache_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 记录迁移历史
        conn.execute(
            "INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![2, CURRENT_CACHE_VERSION, now, "升级到版本 3：添加 FTS5 全文搜索、自省级别支持和聚合视图"],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "record_migration_history".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(())
    }

    fn reason(&self) -> &'static str {
        "升级到版本 3：添加 FTS5 全文搜索、自省级别支持和聚合视图"
    }
}

/// 版本 3 到版本 4 的迁移策略（规范化重构）
pub struct V3ToV4Migration;

impl MigrationStrategy for V3ToV4Migration {
    fn target_version(&self) -> u32 {
        4
    }

    fn migrate(&self, conn: &Connection) -> Result<(), CoreError> {
        // 版本 4 的迁移 SQL 已经在 004_refactor_to_normalized.sql 中定义
        // 这里只需要确保迁移被正确记录

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        // 更新缓存版本记录
        conn.execute(
            "UPDATE cache_version SET version = ?1, upgraded_at = ?2, updated_at = ?3 WHERE id = 1",
            rusqlite::params![CURRENT_CACHE_VERSION, now, now],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "update_cache_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 记录迁移历史
        conn.execute(
            "INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![3, CURRENT_CACHE_VERSION, now, "规范化重构：从统一表拆分为 schemata/tables/columns/indexes 等独立表"],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "record_migration_history".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(())
    }

    fn reason(&self) -> &'static str {
        "规范化重构：从统一表拆分为 schemata/tables/columns/indexes 等独立表"
    }
}

/// 版本 4 到版本 5 的迁移策略（规范化表 FTS 同步、级联删除）
pub struct V4ToV5Migration;

impl MigrationStrategy for V4ToV5Migration {
    fn target_version(&self) -> u32 {
        5
    }

    fn migrate(&self, conn: &Connection) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        // 更新缓存版本记录
        conn.execute(
            "UPDATE cache_version SET version = ?1, upgraded_at = ?2, updated_at = ?3 WHERE id = 1",
            rusqlite::params![CURRENT_CACHE_VERSION, now, now],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "update_cache_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 记录迁移历史
        conn.execute(
            "INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![4, CURRENT_CACHE_VERSION, now, "规范化表 FTS 同步、级联删除支持、索引优化"],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "record_migration_history".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(())
    }

    fn reason(&self) -> &'static str {
        "规范化表 FTS 同步、级联删除支持、索引优化"
    }
}

/// 版本 5 到版本 6 的迁移策略（索引表支持分页懒加载）
pub struct V5ToV6Migration;

impl MigrationStrategy for V5ToV6Migration {
    fn target_version(&self) -> u32 {
        6
    }

    fn migrate(&self, conn: &Connection) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        // 更新缓存版本记录
        conn.execute(
            "UPDATE cache_version SET version = ?1, upgraded_at = ?2, updated_at = ?3 WHERE id = 1",
            rusqlite::params![CURRENT_CACHE_VERSION, now, now],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "update_cache_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 记录迁移历史
        conn.execute(
            "INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![5, CURRENT_CACHE_VERSION, now, "索引表支持分页懒加载、预热状态跟踪"],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "record_migration_history".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(())
    }

    fn reason(&self) -> &'static str {
        "索引表支持分页懒加载、预热状态跟踪"
    }
}

/// 版本 6 到版本 7 的迁移策略（增量同步支持）
pub struct V6ToV7Migration;

impl MigrationStrategy for V6ToV7Migration {
    fn target_version(&self) -> u32 {
        7
    }

    fn migrate(&self, conn: &Connection) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        // 更新缓存版本记录
        conn.execute(
            "UPDATE cache_version SET version = ?1, upgraded_at = ?2, updated_at = ?3 WHERE id = 1",
            rusqlite::params![CURRENT_CACHE_VERSION, now, now],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "update_cache_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 记录迁移历史
        conn.execute(
            "INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![6, CURRENT_CACHE_VERSION, now, "增量同步支持：减少 90%+ 预热时间，智能检测变化"],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "record_migration_history".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(())
    }

    fn reason(&self) -> &'static str {
        "增量同步支持：减少 90%+ 预热时间，智能检测变化"
    }
}

/// 版本 7 到版本 8 的迁移策略（添加 columns.is_primary 列）
pub struct V7ToV8Migration;

impl MigrationStrategy for V7ToV8Migration {
    fn target_version(&self) -> u32 {
        8
    }

    fn migrate(&self, conn: &Connection) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        conn.execute(
            "ALTER TABLE columns ADD COLUMN is_primary INTEGER DEFAULT 0",
            [],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "add_is_primary_column".to_string(),
                reason: e.to_string(),
            })
        })?;

        conn.execute(
            "UPDATE cache_version SET version = ?1, upgraded_at = ?2, updated_at = ?3 WHERE id = 1",
            rusqlite::params![CURRENT_CACHE_VERSION, now, now],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "update_cache_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        conn.execute(
            "INSERT INTO cache_migration_history (from_version, to_version, migrated_at, reason, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![7, CURRENT_CACHE_VERSION, now, "添加 columns.is_primary 字段，支持主键标记独立于 is_identity"],
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "record_migration_history".to_string(),
            reason: e.to_string(),
        }))?;

        Ok(())
    }

    fn reason(&self) -> &'static str {
        "添加 columns.is_primary 字段，支持主键标记独立于 is_identity"
    }
}

/// 缓存版本迁移管理器
pub struct CacheVersionManager {
    /// 注册的迁移策略
    strategies: Vec<Box<dyn MigrationStrategy>>,
}

impl CacheVersionManager {
    /// 创建新的版本迁移管理器
    pub fn new() -> Self {
        let mut manager = Self {
            strategies: Vec::new(),
        };

        // 注册默认迁移策略
        manager.register_strategy(Box::new(V1ToV2Migration));
        manager.register_strategy(Box::new(V2ToV3Migration));
        manager.register_strategy(Box::new(V3ToV4Migration));
        manager.register_strategy(Box::new(V4ToV5Migration));
        manager.register_strategy(Box::new(V5ToV6Migration));
        manager.register_strategy(Box::new(V6ToV7Migration));
        manager.register_strategy(Box::new(V7ToV8Migration));

        manager
    }

    /// 注册迁移策略
    pub fn register_strategy(&mut self, strategy: Box<dyn MigrationStrategy>) {
        self.strategies.push(strategy);
        // 按目标版本排序
        self.strategies.sort_by_key(|s| s.target_version());
    }

    /// 获取当前缓存版本
    pub fn get_current_version(&self, conn: &Connection) -> Result<u32, CoreError> {
        // 检查 cache_version 表是否存在
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master
             WHERE type='table' AND name='cache_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "check_cache_version_table".to_string(),
                    reason: e.to_string(),
                })
            })?;

        if !table_exists {
            // 表不存在，返回版本 0（需要初始化）
            return Ok(0);
        }

        let version: Option<u32> = conn
            .query_row(
                "SELECT version FROM cache_version WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_cache_version".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(version.unwrap_or(0))
    }

    /// 获取缓存版本详细信息
    pub fn get_version_info(
        &self,
        conn: &Connection,
    ) -> Result<Option<CacheVersionInfo>, CoreError> {
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master
             WHERE type='table' AND name='cache_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "check_cache_version_table".to_string(),
                    reason: e.to_string(),
                })
            })?;

        if !table_exists {
            return Ok(None);
        }

        let info = conn
            .query_row(
                "SELECT version, upgraded_at, upgrade_reason, created_at, updated_at
             FROM cache_version WHERE id = 1",
                [],
                |row| {
                    Ok(CacheVersionInfo {
                        version: row.get(0)?,
                        upgraded_at: row.get(1)?,
                        upgrade_reason: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_version_info".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(info)
    }

    /// 检查是否需要升级
    pub fn needs_upgrade(&self, conn: &Connection) -> Result<bool, CoreError> {
        let current_version = self.get_current_version(conn)?;
        Ok(current_version < CURRENT_CACHE_VERSION)
    }

    /// 执行版本迁移
    pub fn migrate(&self, conn: &Connection) -> Result<Vec<CacheMigrationRecord>, CoreError> {
        let current_version = self.get_current_version(conn)?;

        if current_version >= CURRENT_CACHE_VERSION {
            return Ok(Vec::new());
        }

        let mut records = Vec::new();

        for strategy in &self.strategies {
            let target = strategy.target_version();

            if target <= current_version || target > CURRENT_CACHE_VERSION {
                continue;
            }

            let start_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| {
                    CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
                })?
                .as_millis() as i64;

            tracing::info!(
                "开始缓存版本迁移：{} -> {} (原因: {})",
                current_version,
                target,
                strategy.reason()
            );

            match strategy.migrate(conn) {
                Ok(_) => {
                    let end_time = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| {
                            CoreError::common(CommonError::General(format!(
                                "获取系统时间失败: {}",
                                e
                            )))
                        })?
                        .as_millis() as i64;

                    let duration = end_time - start_time;

                    let record = CacheMigrationRecord {
                        from_version: current_version,
                        to_version: target,
                        migrated_at: end_time / 1000,
                        reason: Some(strategy.reason().to_string()),
                        duration_ms: Some(duration),
                        success: true,
                    };

                    records.push(record);

                    tracing::info!(
                        "缓存版本迁移成功：{} -> {} (耗时: {}ms)",
                        current_version,
                        target,
                        duration
                    );
                }
                Err(e) => {
                    let end_time = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| {
                            CoreError::common(CommonError::General(format!(
                                "获取系统时间失败: {}",
                                e
                            )))
                        })?
                        .as_millis() as i64;

                    let record = CacheMigrationRecord {
                        from_version: current_version,
                        to_version: target,
                        migrated_at: end_time / 1000,
                        reason: Some(strategy.reason().to_string()),
                        duration_ms: None,
                        success: false,
                    };

                    records.push(record);

                    tracing::error!(
                        "缓存版本迁移失败：{} -> {} (错误: {})",
                        current_version,
                        target,
                        e
                    );

                    // 尝试回滚
                    if strategy.can_rollback() {
                        if let Err(rollback_err) = strategy.rollback(conn) {
                            tracing::error!("回滚失败：{}", rollback_err);
                        }
                    }

                    return Err(e);
                }
            }
        }

        Ok(records)
    }

    /// 获取迁移历史
    pub fn get_migration_history(
        &self,
        conn: &Connection,
    ) -> Result<Vec<CacheMigrationRecord>, CoreError> {
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master
             WHERE type='table' AND name='cache_migration_history'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "check_migration_history_table".to_string(),
                    reason: e.to_string(),
                })
            })?;

        if !table_exists {
            return Ok(Vec::new());
        }

        let mut stmt = conn
            .prepare(
                "SELECT from_version, to_version, migrated_at, reason, duration_ms, success
             FROM cache_migration_history ORDER BY migrated_at DESC",
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "prepare_migration_history_query".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let history = stmt
            .query_map([], |row| {
                Ok(CacheMigrationRecord {
                    from_version: row.get(0)?,
                    to_version: row.get(1)?,
                    migrated_at: row.get(2)?,
                    reason: row.get(3)?,
                    duration_ms: row.get(4)?,
                    success: row.get::<_, i32>(5)? != 0,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_migration_history".to_string(),
                    reason: e.to_string(),
                })
            })?;

        history.collect::<Result<Vec<_>, _>>().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "collect_migration_history".to_string(),
                reason: e.to_string(),
            })
        })
    }

    /// 重置缓存版本（用于测试或手动重置）
    pub fn reset_version(&self, conn: &Connection, version: u32) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        conn.execute(
            "UPDATE cache_version SET version = ?1, updated_at = ?2 WHERE id = 1",
            rusqlite::params![version, now],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "reset_cache_version".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(())
    }
}

impl Default for CacheVersionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rdata_test_cache_migration_{}", name));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_cache_version_manager() -> Result<(), CoreError> {
        let db_path = test_temp_dir("version").join("test_cache_version.sqlite");

        let conn = Connection::open(&db_path)?;

        // 创建 cache_version 表
        conn.execute(
            "CREATE TABLE cache_version (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                version INTEGER NOT NULL DEFAULT 1,
                upgraded_at INTEGER,
                upgrade_reason TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "INSERT INTO cache_version (id, version, created_at, updated_at) VALUES (1, 1, strftime('%s', 'now'), strftime('%s', 'now'))",
            [],
        )?;

        let manager = CacheVersionManager::new();

        // 检查当前版本
        let version = manager.get_current_version(&conn)?;
        assert_eq!(version, 1);

        // 检查是否需要升级
        let needs_upgrade = manager.needs_upgrade(&conn)?;
        assert!(needs_upgrade);

        // 执行迁移
        let records = manager.migrate(&conn)?;
        assert_eq!(records.len(), 1);
        assert!(records[0].success);

        // 检查迁移后的版本
        let version = manager.get_current_version(&conn)?;
        assert_eq!(version, CURRENT_CACHE_VERSION);
        Ok(())
    }
}
