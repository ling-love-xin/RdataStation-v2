/**
 * 连接元数据缓存管理模块
 *
 * 每个数据库连接都有独立的 SQLite 文件用于缓存元数据。
 * 元数据缓存的存储位置跟随连接信息：
 * - 全局连接：存储到 system/global_metadata/ 目录
 * - 项目连接：存储到 project/meta/connection_metadata/ 目录
 *
 * 设计理由：
 * - 大型数据库（如 Oracle）可能有 10 万+ 张表，元数据记录可达数百万条
 * - 独立文件避免单文件过大，提高查询性能
 * - 跟随连接信息，简化项目迁移
 */
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};

use shared::error::{CommonError, CoreError, StorageError};
use crate::migration::{MigrationManager, MigrationType};
use crate::persistence::metadata_cache_pool::{MetadataCachePool, PooledMetadataConnection};

/// 连接类型枚举
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionType {
    /// 全局连接（不跟随项目）
    Global,
    /// 项目连接（跟随项目）
    Project,
}

/// 元数据缓存管理器
///
/// 为每个数据库连接管理独立的元数据缓存 SQLite 文件
pub struct MetadataCacheManager {
    /// 缓存数据库路径
    db_path: PathBuf,
    /// 连接 ID
    conn_id: String,
    /// 连接类型
    connection_type: ConnectionType,
}

impl MetadataCacheManager {
    /// 创建元数据缓存管理器
    ///
    /// # 参数
    /// * `conn_id` - 连接 ID
    /// * `connection_type` - 连接类型（全局/项目）
    /// * `project_path` - 项目路径（仅项目连接需要）
    pub fn new(
        conn_id: &str,
        connection_type: ConnectionType,
        project_path: Option<&str>,
    ) -> Result<Self, CoreError> {
        let db_path = Self::build_metadata_path(conn_id, connection_type, project_path)?;

        Ok(Self {
            db_path,
            conn_id: conn_id.to_string(),
            connection_type,
        })
    }

    /// 将连接 ID 安全的转换为文件名片段
    ///
    /// 连接 ID 可能包含 Windows 非法字符（如 `:` `@` `/` `\` 等），
    /// 这些字符不能出现在文件路径中。使用哈希映射将非法字符替换为安全替代品。
    fn sanitize_conn_id_for_filename(conn_id: &str) -> String {
        conn_id
            .replace(':', "_")
            .replace('@', "_at_")
            .replace(['/', '\\', '*', '?', '"', '<', '>', '|'], "_")
    }

    /// 构建元数据缓存数据库路径
    ///
    /// 元数据文件跟随连接信息：
    /// - 全局连接：<RDS_HOME>/data/system/global_metadata/conn_{id}.sqlite
    /// - 项目连接：{project_path}/meta/connection_metadata/conn_{id}.sqlite
    fn build_metadata_path(
        conn_id: &str,
        connection_type: ConnectionType,
        project_path: Option<&str>,
    ) -> Result<PathBuf, CoreError> {
        let dir = match connection_type {
            ConnectionType::Global => {
                // 全局连接：存储到系统目录下的全局元数据目录
                let system_dir = crate::migration::get_system_dir()?;
                system_dir.join("global_metadata")
            }
            ConnectionType::Project => {
                // 项目连接：存储到项目元数据目录下的连接元数据子目录
                let project_path = project_path.ok_or_else(|| {
                    CoreError::common(CommonError::General(
                        "Project path is required for project connection".to_string(),
                    ))
                })?;
                PathBuf::from(project_path).join("meta/connection_metadata")
            }
        };

        // 确保目录存在
        std::fs::create_dir_all(&dir).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to create metadata directory {:?}: {}",
                dir, e
            )))
        })?;

        // conn_id 可能包含 Windows 文件系统非法字符（如 : @ / 等）
        let safe_id = Self::sanitize_conn_id_for_filename(conn_id);
        Ok(dir.join(format!("conn_{}.sqlite", safe_id)))
    }

    /// 打开元数据缓存数据库
    ///
    /// 如果数据库不存在，将自动创建并执行迁移
    pub fn open(&self) -> Result<Connection, CoreError> {
        let conn = Connection::open(&self.db_path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "open_metadata".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 启用 WAL 模式（PRAGMA journal_mode=WAL 会返回结果，使用 query_row）
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "set_wal_mode".to_string(),
                    reason: e.to_string(),
                })
            })?;

        // 设置 Memory-Mapped I/O（256MB，对于大型数据库效果显著）
        // 使用 execute 设置 PRAGMA，忽略可能的返回值
        let _ = conn.execute("PRAGMA mmap_size=268435456", []).map_err(|e| {
            tracing::warn!("Failed to set mmap_size: {}", e);
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "set_mmap_size".to_string(),
                reason: e.to_string(),
            })
        });

        // 设置缓存大小（-1000 表示 1000KB）
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
                operation: "set_foreign_keys".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 设置同步模式为 NORMAL（在 WAL 模式下，NORMAL 提供良好的性能/安全性平衡）
        conn.execute("PRAGMA synchronous=NORMAL", []).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "set_synchronous".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 执行迁移（幂等；与连接池创建路径共用同一实现）
        ensure_schema_at(&self.db_path)?;

        Ok(conn)
    }

    /// 获取元数据缓存数据库路径
    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    /// 获取连接 ID
    pub fn conn_id(&self) -> &str {
        &self.conn_id
    }

    /// 获取连接类型
    pub fn connection_type(&self) -> ConnectionType {
        self.connection_type
    }

    /// 删除元数据缓存文件
    ///
    /// 当连接被删除时调用。**先丢弃连接池**：池里握着该文件的打开句柄，
    /// Windows 上带着句柄删文件会失败（缓存管理对话框的「删除」会没反应）。
    pub fn delete(&self) -> Result<(), CoreError> {
        MetadataCachePool::drop_pool(&self.db_path);

        if self.db_path.exists() {
            std::fs::remove_file(&self.db_path).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to delete metadata cache {:?}: {}",
                    self.db_path, e
                )))
            })?;
        }
        Ok(())
    }

    /// 检查元数据缓存是否存在
    pub fn exists(&self) -> bool {
        self.db_path.exists()
    }

    /// 获取元数据缓存文件大小（字节）
    pub fn size(&self) -> Result<u64, CoreError> {
        let metadata = std::fs::metadata(&self.db_path).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to get metadata cache size: {}",
                e
            )))
        })?;
        Ok(metadata.len())
    }
}

/// 确保指定缓存文件的目录与表结构就绪（**幂等**）。
///
/// 单独暴露的原因：连接池创建时调一次，此后池化连接的取用都不必再校验表结构
/// —— `MetadataCacheManager::open()` 里那次校验正是缓存热路径上的固定开销来源。
pub fn ensure_schema_at(db_path: &Path) -> Result<(), CoreError> {
    if let Some(parent) = db_path.parent() {
        if parent.as_os_str() != "" {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to create metadata cache dir {:?}: {}",
                    parent, e
                )))
            })?;
        }
    }

    MigrationManager::new().migrate(db_path, MigrationType::ConnectionMetadata)?;
    Ok(())
}

/// 元数据缓存操作封装
///
/// 提供常用的元数据缓存读写操作
pub struct MetadataCacheOps {
    conn: CacheConn,
}

/// 缓存库连接的两种来源。
///
/// 实现 `Deref` / `DerefMut` 到 `rusqlite::Connection`，因此既有的 ~90 个方法一个都不用改。
enum CacheConn {
    /// 自有连接（`MetadataCacheOps::new`；一次性打开）
    Owned(Connection),
    /// 池化连接（`MetadataCacheOps::from_pooled`；Drop 时自动归还）
    Pooled(PooledMetadataConnection),
}

impl std::ops::Deref for CacheConn {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        match self {
            Self::Owned(conn) => conn,
            Self::Pooled(guard) => guard,
        }
    }
}

impl std::ops::DerefMut for CacheConn {
    fn deref_mut(&mut self) -> &mut Connection {
        match self {
            Self::Owned(conn) => conn,
            Self::Pooled(guard) => guard,
        }
    }
}

impl MetadataCacheOps {
    /// 创建新的元数据缓存操作实例
    pub fn new(conn: Connection) -> Self {
        Self {
            conn: CacheConn::Owned(conn),
        }
    }

    /// 用**池化连接**创建（推荐：把「开文件 + PRAGMA + 迁移校验」挪出热路径）。
    ///
    /// 守卫在 `self` 被丢弃时归还连接，因此调用方无需管归还。
    pub fn from_pooled(guard: PooledMetadataConnection) -> Self {
        Self {
            conn: CacheConn::Pooled(guard),
        }
    }

    /// 获取底层数据库连接（用于版本迁移等操作）
    pub fn get_connection(&self) -> &Connection {
        &self.conn
    }

    // ==================== Schema 操作 ====================

    /// 保存 Schema 元数据
    pub fn save_schema(
        &self,
        catalog_name: &str,
        schema_name: &str,
        owner: Option<&str>,
        comment: Option<&str>,
    ) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO schemata
             (catalog_name, schema_name, owner, comment, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, ?3, ?4, 3, 1, ?5, ?5)",
            rusqlite::params![catalog_name, schema_name, owner, comment, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_schema".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// 获取 Schema 列表
    pub fn list_schemas(&self, catalog_name: Option<&str>) -> Result<Vec<SchemaInfo>, CoreError> {
        let query = match catalog_name {
            Some(_) => "SELECT id, catalog_name, schema_name, owner, comment, last_sync, \
                        default_character_set_name, default_collation_name, introspect_level, is_loaded \
                        FROM schemata WHERE catalog_name = ?1 ORDER BY schema_name",
            None => "SELECT id, catalog_name, schema_name, owner, comment, last_sync, \
                     default_character_set_name, default_collation_name, introspect_level, is_loaded \
                     FROM schemata ORDER BY schema_name",
        };

        let mut stmt = self.conn.prepare(query).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "list_schemas".to_string(),
                reason: e.to_string(),
            })
        })?;

        let schemas = match catalog_name {
            Some(cat) => stmt.query_map(rusqlite::params![cat], SchemaInfo::from_row),
            None => stmt.query_map([], SchemaInfo::from_row),
        }
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "query_schemas".to_string(),
                reason: e.to_string(),
            })
        })?;

        let mut result = Vec::new();
        for schema in schemas {
            result.push(schema.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_schema".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    /// 获取 Schema ID
    pub fn get_schema_id(
        &self,
        catalog_name: &str,
        schema_name: &str,
    ) -> Result<Option<i64>, CoreError> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM schemata WHERE catalog_name = ?1 AND schema_name = ?2",
                rusqlite::params![catalog_name, schema_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_schema_id".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(id)
    }

    // ==================== Table 操作（规范化） ====================

    /// 保存表元数据（规范化结构）
    pub fn save_table(
        &self,
        schema_id: i64,
        table_name: &str,
        table_type: &str,
        comment: Option<&str>,
        engine: Option<&str>,
        row_count_estimate: Option<i64>,
    ) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO tables
             (schema_id, table_name, table_type, table_comment, engine, row_count_estimate, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 3, 1, ?7, ?7)",
            rusqlite::params![schema_id, table_name, table_type, comment, engine, row_count_estimate, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_table".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }


    /// 保存表元数据并填充统计信息
    #[allow(clippy::too_many_arguments)]
    pub fn save_table_with_stats(
        &self,
        schema_id: i64,
        table_name: &str,
        table_type: &str,
        comment: Option<&str>,
        engine: Option<&str>,
        row_count_estimate: Option<i64>,
        data_length: Option<i64>,
        index_length: Option<i64>,
    ) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO tables
             (schema_id, table_name, table_type, table_comment, engine, row_count_estimate,
              data_length, index_length, introspect_level, is_loaded, last_sync, last_accessed, stats_last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 3, 1, ?9, ?9, ?9)",
            rusqlite::params![schema_id, table_name, table_type, comment, engine, row_count_estimate, data_length, index_length, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_table_with_stats".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let table_id = self.conn.last_insert_rowid();
        // 保存后更新所属 schema 的聚合统计
        let _ = self.update_schema_stats(schema_id);
        Ok(table_id)
    }

    /// 更新 Schema 聚合统计
    pub fn update_schema_stats(&self, schema_id: i64) -> Result<(), CoreError> {
        self.conn.execute(
            "UPDATE schemata SET
                total_tables = (SELECT COUNT(*) FROM tables WHERE schema_id = ?1 AND table_type IN ('TABLE', 'PARTITIONED TABLE', 'SYSTEM TABLE', 'GLOBAL TEMPORARY', 'LOCAL TEMPORARY') AND hidden = 0),
                total_views = (SELECT COUNT(*) FROM tables WHERE schema_id = ?1 AND table_type IN ('VIEW', 'MATERIALIZED VIEW') AND hidden = 0),
                total_procedures = (SELECT COUNT(*) FROM routines WHERE schema_id = ?1 AND routine_type = 'PROCEDURE'),
                total_functions = (SELECT COUNT(*) FROM routines WHERE schema_id = ?1 AND routine_type = 'FUNCTION'),
                total_size_bytes = (SELECT COALESCE(SUM(data_length), 0) + COALESCE(SUM(index_length), 0) FROM tables WHERE schema_id = ?1),
                row_count_total = (SELECT COALESCE(SUM(row_count_estimate), 0) FROM tables WHERE schema_id = ?1)
            WHERE id = ?1",
            rusqlite::params![schema_id],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "update_schema_stats".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(())
    }

    /// 获取 Schema 对象数量统计
    pub fn get_schema_stats(&self, schema_id: i64) -> Result<Option<SchemaInfo>, CoreError> {
        let schema = self
            .conn
            .query_row(
                "SELECT s.id, s.catalog_name, s.schema_name, s.owner, s.comment, s.last_sync, \
                 s.default_character_set_name, s.default_collation_name, s.introspect_level, s.is_loaded, \
                 s.total_tables, s.total_views, s.total_procedures, s.total_functions, \
                 s.total_size_bytes, s.row_count_total
                 FROM schemata s WHERE s.id = ?1",
                rusqlite::params![schema_id],
                SchemaInfo::from_row_with_stats,
            )
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_schema_stats".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(schema)
    }

    /// 获取 Schema 列表（含 V10 统计）
    pub fn list_schemas_with_stats(
        &self,
        catalog_name: Option<&str>,
    ) -> Result<Vec<SchemaInfo>, CoreError> {
        let query = match catalog_name {
            Some(_) => "SELECT id, catalog_name, schema_name, owner, comment, last_sync, \
                        default_character_set_name, default_collation_name, introspect_level, is_loaded, \
                        total_tables, total_views, total_procedures, total_functions, total_size_bytes, row_count_total
                        FROM schemata WHERE catalog_name = ?1 ORDER BY schema_name",
            None => "SELECT id, catalog_name, schema_name, owner, comment, last_sync, \
                     default_character_set_name, default_collation_name, introspect_level, is_loaded, \
                     total_tables, total_views, total_procedures, total_functions, total_size_bytes, row_count_total
                     FROM schemata ORDER BY schema_name",
        };

        let mut stmt = self.conn.prepare(query).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "list_schemas_with_stats".to_string(),
                reason: e.to_string(),
            })
        })?;

        let schemas = match catalog_name {
            Some(cat) => stmt.query_map(rusqlite::params![cat], SchemaInfo::from_row_with_stats),
            None => stmt.query_map([], SchemaInfo::from_row_with_stats),
        }
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "query_schemas_with_stats".to_string(),
                reason: e.to_string(),
            })
        })?;

        let mut result = Vec::new();
        for schema in schemas {
            result.push(schema.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_schema_with_stats".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    /// 获取表列表（规范化）
    pub fn list_tables_normalized(
        &self,
        schema_id: i64,
        table_type: Option<&str>,
    ) -> Result<Vec<TableDetailInfo>, CoreError> {
        let query = match table_type {
            Some(_t) => "SELECT t.id, t.table_name, t.table_type, t.table_comment, t.engine, t.row_count_estimate, \
                        t.created_at, t.last_altered_at, t.last_sync, s.schema_name, \
                        t.data_length, t.index_length, t.display_order, t.hidden, t.favorite, t.color_label, t.user_comment
                        FROM tables t INNER JOIN schemata s ON t.schema_id = s.id
                        WHERE t.schema_id = ?1 AND t.table_type = ?2 ORDER BY t.table_name",
            None => "SELECT t.id, t.table_name, t.table_type, t.table_comment, t.engine, t.row_count_estimate, \
                     t.created_at, t.last_altered_at, t.last_sync, s.schema_name, \
                     t.data_length, t.index_length, t.display_order, t.hidden, t.favorite, t.color_label, t.user_comment
                     FROM tables t INNER JOIN schemata s ON t.schema_id = s.id
                     WHERE t.schema_id = ?1 ORDER BY t.table_name",
        };

        let mut stmt = self.conn.prepare(query).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "list_tables_normalized".to_string(),
                reason: e.to_string(),
            })
        })?;

        let tables = match table_type {
            Some(t) => stmt.query_map(rusqlite::params![schema_id, t], TableDetailInfo::from_row),
            None => stmt.query_map(rusqlite::params![schema_id], TableDetailInfo::from_row),
        }
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "query_tables_normalized".to_string(),
                reason: e.to_string(),
            })
        })?;

        let mut result = Vec::new();
        for table in tables {
            result.push(table.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_table_normalized".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    /// 获取表 ID
    pub fn get_table_id(&self, schema_id: i64, table_name: &str) -> Result<Option<i64>, CoreError> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM tables WHERE schema_id = ?1 AND table_name = ?2",
                rusqlite::params![schema_id, table_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_table_id".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(id)
    }

    // ==================== Column 操作（规范化） ====================

    /// 保存列元数据
    ///
    /// `is_identity`（自增/生成标识）与 `is_primary`（主键）是**两个独立维度**，见迁移
    /// 008 的抬头（其存在的原因就是这两列曾被拥挤在一起）。历史上本方法的第 7 个参数
    /// 叫 `_is_unique`、被直接丢弃，而 `is_identity` 又拿 `is_primary` 顶替——迁移把列拆开
    /// 了，写入侧没跟上；`columns` 表也没有 `is_unique` 列（唯一性由 `indexes` 表达），
    /// 所以该参数已换成真正有含义的 `is_identity`。
    #[allow(clippy::too_many_arguments)]
    pub fn save_column(
        &self,
        table_id: i64,
        column_name: &str,
        data_type: &str,
        ordinal_position: i32,
        is_nullable: bool,
        is_primary: bool,
        is_identity: bool,
        column_default: Option<&str>,
        comment: Option<&str>,
    ) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO columns
             (table_id, column_name, ordinal_position, data_type, is_nullable, is_identity, is_primary, column_default, column_comment, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 3, 1, ?10, ?10)",
            rusqlite::params![table_id, column_name, ordinal_position, data_type, is_nullable as i32, is_identity as i32, is_primary as i32, column_default, comment, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_column".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }


    /// 获取列列表（规范化）
    pub fn list_columns_normalized(
        &self,
        table_id: i64,
    ) -> Result<Vec<ColumnDetailInfo>, CoreError> {
        let has_fk_table: bool = self.conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='foreign_key_columns'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0) > 0;

        let sql = if has_fk_table {
            "SELECT c.id, c.column_name, c.ordinal_position, c.data_type, c.is_nullable, c.is_identity, \
             COALESCE(c.is_primary, 0) AS is_primary_key, \
             CASE WHEN fkc.column_name IS NOT NULL THEN 1 ELSE 0 END AS is_foreign_key, \
             c.column_default, c.column_comment, \
             c.character_maximum_length, c.numeric_precision, c.numeric_scale, \
             c.character_set_name, c.collation_name, COALESCE(c.is_generated, 0) AS is_generated, \
             c.extra \
             FROM columns c \
             LEFT JOIN foreign_keys fk ON fk.table_id = c.table_id \
             LEFT JOIN foreign_key_columns fkc ON fkc.foreign_key_id = fk.id AND fkc.column_name = c.column_name \
             WHERE c.table_id = ?1 ORDER BY c.ordinal_position"
        } else {
            "SELECT c.id, c.column_name, c.ordinal_position, c.data_type, c.is_nullable, c.is_identity, \
             COALESCE(c.is_primary, 0) AS is_primary_key, \
             0 AS is_foreign_key, \
             c.column_default, c.column_comment, \
             c.character_maximum_length, c.numeric_precision, c.numeric_scale, \
             c.character_set_name, c.collation_name, COALESCE(c.is_generated, 0) AS is_generated, \
             c.extra \
             FROM columns c \
             WHERE c.table_id = ?1 ORDER BY c.ordinal_position"
        };

        let mut stmt = self.conn.prepare(sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "list_columns_normalized".to_string(),
                reason: e.to_string(),
            })
        })?;

        let columns = stmt
            .query_map(rusqlite::params![table_id], ColumnDetailInfo::from_row)
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_columns_normalized".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for column in columns {
            result.push(column.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_column_normalized".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    // ==================== Index 操作 ====================

    /// 保存索引元数据
    pub fn save_index(
        &self,
        table_id: i64,
        index_name: &str,
        index_type: Option<&str>,
        is_unique: bool,
        is_primary: bool,
        comment: Option<&str>,
    ) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO indexes
             (table_id, index_name, index_type, is_unique, is_primary, index_comment, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 3, 1, ?7, ?7)",
            rusqlite::params![table_id, index_name, index_type, is_unique as i32, is_primary as i32, comment, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_index".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// 保存索引列
    pub fn save_index_column(
        &self,
        index_id: i64,
        column_name: &str,
        ordinal_position: i32,
        sort_order: Option<&str>,
    ) -> Result<(), CoreError> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO index_columns
             (index_id, column_name, ordinal_position, sort_order)
             VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![index_id, column_name, ordinal_position, sort_order],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_index_column".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }

    /// 获取索引列表
    pub fn list_indexes(&self, table_id: i64) -> Result<Vec<IndexDetailInfo>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, index_name, index_type, is_unique, is_primary, index_comment
             FROM indexes WHERE table_id = ?1 ORDER BY index_name",
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "list_indexes".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let indexes = stmt
            .query_map(rusqlite::params![table_id], |row| {
                Ok(IndexDetailInfo {
                    id: row.get(0)?,
                    index_name: row.get(1)?,
                    index_type: row.get(2)?,
                    is_unique: row.get::<_, i32>(3)? != 0,
                    is_primary: row.get::<_, i32>(4)? != 0,
                    index_comment: row.get(5)?,
                    columns: Vec::new(),
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_indexes".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for index in indexes {
            let mut idx = index.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_index".to_string(),
                    reason: e.to_string(),
                })
            })?;

            idx.columns = self.list_index_columns(idx.id)?;
            result.push(idx);
        }

        Ok(result)
    }

    /// 获取索引列列表
    pub fn list_index_columns(&self, index_id: i64) -> Result<Vec<IndexColumnInfo>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, column_name, ordinal_position, sort_order, is_included_column
             FROM index_columns WHERE index_id = ?1 ORDER BY ordinal_position",
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "list_index_columns".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let columns = stmt
            .query_map(rusqlite::params![index_id], IndexColumnInfo::from_row)
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_index_columns".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for col in columns {
            result.push(col.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_index_column".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    // ==================== View 操作 ====================

    /// 保存视图定义
    ///
    /// `view_definitions` 与视图（`tables` 里 `table_type = 'VIEW'` 的那一行）是 1:1：
    /// 这里同时写 `id = table_id` 以满足「一条视图一行定义」并让重复保存幂等。
    /// 注意 `table_id` 是 `NOT NULL` 且带 `ON DELETE CASCADE`——历史实现的 INSERT
    /// 漏了这列，语句直接违反非空约束，视图定义因此从未真正落库（调用方还吞了错）。
    pub fn save_view(
        &self,
        table_id: i64,
        view_definition: &str,
        is_updatable: Option<bool>,
        check_option: Option<&str>,
    ) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO view_definitions
             (id, table_id, view_definition, is_updatable, check_option, introspect_level, is_loaded, last_sync)
             VALUES (?1, ?1, ?2, ?3, ?4, 3, 1, ?5)",
            rusqlite::params![table_id, view_definition, is_updatable.map(|b| b as i32), check_option, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_view".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(())
    }

    /// 获取视图列表
    pub fn list_views(&self, schema_id: i64) -> Result<Vec<ViewDetailInfo>, CoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.table_name, t.table_comment, v.view_definition, v.is_updatable, v.check_option
             FROM tables t INNER JOIN view_definitions v ON t.id = v.id
             WHERE t.schema_id = ?1 AND t.table_type = 'VIEW' ORDER BY t.table_name"
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "list_views".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let views = stmt
            .query_map(rusqlite::params![schema_id], ViewDetailInfo::from_row)
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_views".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for view in views {
            result.push(view.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_view".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    // ==================== Routine 操作 ====================

    /// 保存 Routine 元数据
    #[allow(clippy::too_many_arguments)]
    pub fn save_routine(
        &self,
        schema_id: i64,
        routine_name: &str,
        routine_type: &str,
        data_type: Option<&str>,
        routine_definition: Option<&str>,
        external_language: Option<&str>,
        is_deterministic: Option<bool>,
        comment: Option<&str>,
    ) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO routines
             (schema_id, routine_name, routine_type, data_type, routine_definition, external_language, is_deterministic, routine_comment, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 3, 1, ?9, ?9)",
            rusqlite::params![schema_id, routine_name, routine_type, data_type, routine_definition, external_language, is_deterministic.map(|b| b as i32), comment, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_routine".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// 保存 Routine 参数
    pub fn save_routine_parameter(
        &self,
        routine_id: i64,
        parameter_name: &str,
        ordinal_position: i32,
        parameter_mode: Option<&str>,
        data_type: Option<&str>,
        parameter_default: Option<&str>,
    ) -> Result<(), CoreError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO routine_parameters
             (routine_id, parameter_name, ordinal_position, parameter_mode, data_type, parameter_default)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![routine_id, parameter_name, ordinal_position, parameter_mode, data_type, parameter_default],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_routine_parameter".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(())
    }

    /// 获取 Routine 列表
    pub fn list_routines(
        &self,
        schema_id: i64,
        routine_type: Option<&str>,
    ) -> Result<Vec<RoutineDetailInfo>, CoreError> {
        let mut result = Vec::new();

        if let Some(rt) = routine_type {
            let query = "SELECT id, routine_name, routine_type, data_type, routine_definition, external_language, is_deterministic, routine_comment
                         FROM routines WHERE schema_id = ?1 AND routine_type = ?2 ORDER BY routine_name";
            let mut stmt = self.conn.prepare(query).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "list_routines".to_string(),
                    reason: e.to_string(),
                })
            })?;

            let routines = stmt
                .query_map(rusqlite::params![schema_id, rt], |row| {
                    Ok(RoutineDetailInfo {
                        id: row.get(0)?,
                        routine_name: row.get(1)?,
                        routine_type: row.get(2)?,
                        data_type: row.get(3)?,
                        routine_definition: row.get(4)?,
                        external_language: row.get(5)?,
                        is_deterministic: row.get::<_, Option<i32>>(6)?.map(|v| v != 0),
                        routine_comment: row.get(7)?,
                        parameters: Vec::new(),
                    })
                })
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "query_routines".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            for routine in routines {
                let mut r = routine.map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "fetch_routine".to_string(),
                        reason: e.to_string(),
                    })
                })?;
                r.parameters = self.list_routine_parameters(r.id)?;
                result.push(r);
            }
        } else {
            let query = "SELECT id, routine_name, routine_type, data_type, routine_definition, external_language, is_deterministic, routine_comment
                         FROM routines WHERE schema_id = ?1 ORDER BY routine_name";
            let mut stmt = self.conn.prepare(query).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "list_routines".to_string(),
                    reason: e.to_string(),
                })
            })?;

            let routines = stmt
                .query_map(rusqlite::params![schema_id], |row| {
                    Ok(RoutineDetailInfo {
                        id: row.get(0)?,
                        routine_name: row.get(1)?,
                        routine_type: row.get(2)?,
                        data_type: row.get(3)?,
                        routine_definition: row.get(4)?,
                        external_language: row.get(5)?,
                        is_deterministic: row.get::<_, Option<i32>>(6)?.map(|v| v != 0),
                        routine_comment: row.get(7)?,
                        parameters: Vec::new(),
                    })
                })
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "query_routines".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            for routine in routines {
                let mut r = routine.map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "fetch_routine".to_string(),
                        reason: e.to_string(),
                    })
                })?;
                r.parameters = self.list_routine_parameters(r.id)?;
                result.push(r);
            }
        }

        Ok(result)
    }

    /// 获取 Routine 参数列表
    pub fn list_routine_parameters(
        &self,
        routine_id: i64,
    ) -> Result<Vec<RoutineParameterInfo>, CoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parameter_name, ordinal_position, parameter_mode, data_type, parameter_default
             FROM routine_parameters WHERE routine_id = ?1 ORDER BY ordinal_position"
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "list_routine_parameters".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let params = stmt
            .query_map(
                rusqlite::params![routine_id],
                RoutineParameterInfo::from_row,
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_routine_parameters".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for param in params {
            result.push(param.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_routine_parameter".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    // ==================== Trigger 操作 ====================

    /// 保存 Trigger 元数据（基础版）
    pub fn save_trigger(
        &self,
        _schema_id: i64,
        table_id: Option<i64>,
        name: &str,
        event_manipulation: &str,
        action_timing: &str,
        action_statement: Option<&str>,
    ) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO triggers
             (table_id, trigger_name, trigger_event, trigger_timing, trigger_body, introspect_level, is_loaded, last_sync)
             VALUES (?1, ?2, ?3, ?4, ?5, 3, 1, ?6)",
            rusqlite::params![table_id, name, event_manipulation, action_timing, action_statement, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_trigger".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// 按「schema + 表名」登记一个触发器（导航路径）。
    ///
    /// `triggers.table_id` 是 `NOT NULL REFERENCES tables(id)`——触发器必须先有表行，
    /// 而导航展开「触发器」文件夹时未必展开过表文件夹。所以这里**补齐最小表行**：
    /// 只写名字与类型，其余留空。这不是“伪造元数据”——它是同一张表在缓存里的占位行，
    /// 后续真正展开表时会走 `save_table` 的 `ON CONFLICT` 更新同一行。
    pub fn save_trigger_for_table(
        &self,
        schema_id: i64,
        table_name: &str,
        trigger_name: &str,
    ) -> Result<i64, CoreError> {
        let table_id = match self.get_table_id(schema_id, table_name)? {
            Some(id) => id,
            None => self.save_table(schema_id, table_name, "TABLE", None, None, None)?,
        };
        self.save_trigger(schema_id, Some(table_id), trigger_name, "", "", None)
    }

    // ==================== Sequence 操作 ====================

    /// 保存 Sequence 元数据（基础版）
    ///
    /// 2026-09-19：本方法与 `save_trigger` 的 SQL 曾引用 `last_accessed` 列，
    /// 而 `sequences` / `triggers` 两张表**没有这一列**（004 建表时就没给）——
    /// 语句一直报 `no such column`，因为零调用而被掩盖。接线时发现并修正。
    pub fn save_sequence(
        &self,
        schema_id: i64,
        name: &str,
        data_type: &str,
        start_value: Option<i64>,
        increment: Option<i64>,
    ) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO sequences
             (schema_id, sequence_name, data_type, start_value, increment_by, introspect_level, is_loaded, last_sync)
             VALUES (?1, ?2, ?3, ?4, ?5, 3, 1, ?6)",
            rusqlite::params![schema_id, name, data_type, start_value, increment.unwrap_or(1), now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_sequence".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// 只存名字的序列行（导航路径）：驱动内省只给名字，其余字段留 NULL。
    ///
    /// 为什么单独一个方法：`save_sequence` 要求 `data_type: &str`——导航没有这个信息，
    /// 硬填一个“BIGINT”之类就是造数据。多了这个方法，导航侧可以只登记“存在这个序列”。
    pub fn save_sequence_name(&self, schema_id: i64, name: &str) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO sequences
             (schema_id, sequence_name, data_type, start_value, increment_by, introspect_level, is_loaded, last_sync)
             VALUES (?1, ?2, NULL, NULL, NULL, 3, 1, ?3)",
            rusqlite::params![schema_id, name, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_sequence_name".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// 获取某 schema 的序列名（导航树只需要名字）。
    ///
    /// 写入侧 [`Self::save_sequence`] 一直存在，读取侧此前**缺失**——表与写入都在、
    /// 只有读没有，结果就是缓存接不上、每次展开序列文件夹都回源库。
    pub fn list_sequences(&self, schema_id: i64) -> Result<Vec<String>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT sequence_name FROM sequences WHERE schema_id = ?1 ORDER BY sequence_name",
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "prepare_list_sequences".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let rows = stmt
            .query_map(rusqlite::params![schema_id], |row| row.get::<_, String>(0))
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_sequences".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut names = Vec::new();
        for row in rows {
            names.push(row.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_sequence".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }
        Ok(names)
    }

    /// 获取某 schema 的触发器（名字 + 所属表 + 注释）。
    ///
    /// 触发器行挂在 `tables` 上（`table_id NOT NULL`），所以这里 JOIN 取所属表名；
    /// 写侧（[`NavCache::put_triggers`](crate::persistence) 的上游）对“所属表不在缓存”的
    /// 触发器会选择不写——那时这里读不到它们是如实降级，不是丢数据。
    pub fn list_triggers(&self, schema_id: i64) -> Result<Vec<TriggerInfo>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.trigger_name, tb.table_name, t.trigger_comment \
                 FROM triggers t INNER JOIN tables tb ON t.table_id = tb.id \
                 WHERE tb.schema_id = ?1 ORDER BY t.trigger_name",
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "prepare_list_triggers".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let rows = stmt
            .query_map(rusqlite::params![schema_id], |row| {
                Ok(TriggerInfo {
                    name: row.get(0)?,
                    table_name: row.get(1)?,
                    comment: row.get(2)?,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_triggers".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut triggers = Vec::new();
        for row in rows {
            triggers.push(row.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_trigger".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }
        Ok(triggers)
    }

    /// 清除指定连接的元数据
    pub fn clear_metadata(
        &self,
        database_name: &str,
        schema_name: &str,
        table_name: Option<&str>,
    ) -> Result<usize, CoreError> {
        let affected = match table_name {
            Some(t) => {
                self.conn.execute(
                    "DELETE FROM metadata WHERE database_name = ?1 AND schema_name = ?2 AND table_name = ?3",
                    rusqlite::params![database_name, schema_name, t],
                ).map_err(|e| CoreError::storage(
                    StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "clear_metadata".to_string(),
                        reason: e.to_string(),
                    }
                ))?
            }
            None => {
                self.conn.execute(
                    "DELETE FROM metadata WHERE database_name = ?1 AND schema_name = ?2",
                    rusqlite::params![database_name, schema_name],
                ).map_err(|e| CoreError::storage(
                    StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "clear_metadata".to_string(),
                        reason: e.to_string(),
                    }
                ))?
            }
        };

        Ok(affected)
    }

    /// 批量保存表元数据
    pub fn save_tables_batch(
        &mut self,
        tables: Vec<(String, String, String, String, Option<String>)>,
    ) -> Result<(), CoreError> {
        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "begin_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        for (id, database_name, schema_name, table_name, comment) in tables {
            tx.execute(
                "INSERT OR REPLACE INTO metadata
                 (id, obj_type, database_name, schema_name, table_name, name, comment, last_sync)
                 VALUES (?1, 'table', ?2, ?3, ?4, ?4, ?5, ?6)",
                rusqlite::params![
                    id,
                    database_name,
                    schema_name,
                    table_name,
                    comment,
                    current_time
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_table_batch".to_string(),
                    reason: e.to_string(),
                })
            })?;
        }

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "commit_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(())
    }

    /// 批量保存列元数据
    #[allow(clippy::type_complexity)]
    pub fn save_columns_batch(
        &mut self,
        columns: Vec<(
            String,
            String,
            String,
            String,
            String,
            String,
            bool,
            bool,
            bool,
        )>,
    ) -> Result<(), CoreError> {
        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "begin_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        for (
            id,
            database_name,
            schema_name,
            table_name,
            column_name,
            data_type,
            is_nullable,
            is_primary,
            is_unique,
        ) in columns
        {
            tx.execute(
                "INSERT OR REPLACE INTO metadata
                 (id, obj_type, database_name, schema_name, table_name, name, data_type, is_nullable, is_primary, is_unique, last_sync)
                 VALUES (?1, 'column', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    id, database_name, schema_name, table_name, column_name,
                    data_type, is_nullable as i32, is_primary as i32, is_unique as i32, current_time
                ],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_column_batch".to_string(),
                    reason: e.to_string(),
                }
            ))?;
        }

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "commit_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(())
    }

    /// 保存 MetadataBrowser::get_table_detail() 的结果到缓存
    /// 使用规范化的 tables + columns 表写入
    pub fn save_node_detail(
        &mut self,
        database_name: &str,
        schema_name: &str,
        detail: &crate::driver::NodeDetail,
    ) -> Result<i64, CoreError> {
        let table_name = &detail.node.name;
        let table_type = match detail.node.kind {
            crate::driver::SchemaObjectKind::View => "VIEW",
            _ => "TABLE",
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let schema_id = match self.get_schema_id(database_name, schema_name)? {
            Some(id) => id,
            None => self.save_schema(database_name, schema_name, None, None)?,
        };

        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_node_detail_tx".to_string(),
                reason: e.to_string(),
            })
        })?;

        let table_id: i64 = tx.query_row(
            "INSERT INTO tables (schema_id, table_name, table_type, table_comment, row_count_estimate, \
             engine, created_at, last_altered_at, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?6, 3, 1, ?6, ?6)
             ON CONFLICT(schema_id, table_name) DO UPDATE SET
             table_type = excluded.table_type,
             table_comment = excluded.table_comment,
             row_count_estimate = excluded.row_count_estimate,
             last_altered_at = excluded.last_altered_at,
             last_sync = excluded.last_sync
             RETURNING id",
            rusqlite::params![
                schema_id, table_name, table_type,
                detail.node.comment,
                detail.row_count_estimate,
                now,
            ],
            |row| row.get(0),
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_node_detail_table".to_string(),
                reason: e.to_string(),
            }
        ))?;

        for (idx, col) in detail.columns.iter().enumerate() {
            let char_max_len = col
                .extra
                .get("character_maximum_length")
                .cloned()
                .unwrap_or_default();
            let num_prec = col
                .extra
                .get("numeric_precision")
                .cloned()
                .unwrap_or_default();
            let num_scale = col.extra.get("numeric_scale").cloned().unwrap_or_default();
            let charset = col
                .extra
                .get("character_set_name")
                .cloned()
                .unwrap_or_default();
            let collation = col.extra.get("collation_name").cloned().unwrap_or_default();
            let is_identity: i32 = if col.extra.contains_key("identity_generation")
                || col
                    .extra
                    .get("extra_info")
                    .map(|v| v.contains("auto_increment"))
                    .unwrap_or(false)
            {
                1
            } else {
                0
            };

            tx.execute(
                "INSERT OR REPLACE INTO columns \
                 (table_id, column_name, ordinal_position, data_type, is_nullable, is_identity, is_primary, \
                  is_foreign_key, column_default, column_comment, character_maximum_length, numeric_precision, \
                  numeric_scale, character_set_name, collation_name, is_generated, extra, \
                  introspect_level, is_loaded, last_sync, last_accessed) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, 0, '{}', \
                         3, 1, ?16, ?16)",
                rusqlite::params![
                    table_id,
                    col.name,
                    (idx + 1) as i32,
                    col.data_type,
                    col.nullable as i32,
                    is_identity,
                    col.is_primary_key as i32,
                    col.is_foreign_key as i32,
                    col.default_value,
                    col.comment,
                    char_max_len,
                    num_prec,
                    num_scale,
                    charset,
                    collation,
                    now,
                ],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_node_detail_column".to_string(),
                    reason: e.to_string(),
                }
            ))?;
        }

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_node_detail_commit".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(table_id)
    }

    /// 保存表的索引信息到缓存
    ///
    /// # 参数
    /// * `table_id` - 表 ID
    /// * `indexes` - 索引详情列表
    pub fn save_table_indexes(
        &mut self,
        table_id: i64,
        indexes: Vec<crate::driver::IndexDetail>,
    ) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_table_indexes_tx".to_string(),
                reason: e.to_string(),
            })
        })?;

        tx.execute(
            "DELETE FROM index_columns WHERE index_id IN (SELECT id FROM indexes WHERE table_id = ?1)",
            rusqlite::params![table_id],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "delete_index_columns".to_string(),
                reason: e.to_string(),
            }
        ))?;

        tx.execute(
            "DELETE FROM indexes WHERE table_id = ?1",
            rusqlite::params![table_id],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "delete_indexes".to_string(),
                reason: e.to_string(),
            })
        })?;

        for idx in &indexes {
            let index_id: i64 = tx.query_row(
                "INSERT INTO indexes (table_id, index_name, index_type, is_unique, is_primary, index_comment, \
                 introspect_level, is_loaded, last_sync, last_accessed)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 3, 1, ?7, ?7)
                 RETURNING id",
                rusqlite::params![
                    table_id,
                    idx.name,
                    idx.index_type,
                    idx.is_unique as i32,
                    idx.is_primary as i32,
                    idx.comment,
                    now,
                ],
                |row| row.get(0),
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_index".to_string(),
                    reason: e.to_string(),
                }
            ))?;

            for (col_idx, col_name) in idx.column_names.iter().enumerate() {
                tx.execute(
                    "INSERT INTO index_columns (index_id, column_name, ordinal_position, is_included_column) \
                     VALUES (?1, ?2, ?3, 0)",
                    rusqlite::params![index_id, col_name, (col_idx + 1) as i32],
                ).map_err(|e| CoreError::storage(
                    StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "save_index_column".to_string(),
                        reason: e.to_string(),
                    }
                ))?;
            }
        }

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_table_indexes_commit".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(())
    }

    /// 保存表的约束（外键等）信息到缓存
    ///
    /// # 参数
    /// * `table_id` - 表 ID
    /// * `constraints` - 约束详情列表
    pub fn save_table_constraints(
        &mut self,
        table_id: i64,
        constraints: Vec<crate::driver::ConstraintDetail>,
    ) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_table_constraints_tx".to_string(),
                reason: e.to_string(),
            })
        })?;

        tx.execute(
            "DELETE FROM foreign_key_columns WHERE foreign_key_id IN (SELECT id FROM foreign_keys WHERE table_id = ?1)",
            rusqlite::params![table_id],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "delete_foreign_key_columns".to_string(),
                reason: e.to_string(),
            }
        ))?;

        tx.execute(
            "DELETE FROM foreign_keys WHERE table_id = ?1",
            rusqlite::params![table_id],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "delete_foreign_keys".to_string(),
                reason: e.to_string(),
            })
        })?;

        for constraint in &constraints {
            if constraint.constraint_type != "FOREIGN KEY" {
                continue;
            }

            let fk_id: i64 = tx.query_row(
                "INSERT INTO foreign_keys (table_id, constraint_name, delete_rule, update_rule, deferrability, \
                 introspect_level, is_loaded, last_sync, last_accessed)
                 VALUES (?1, ?2, ?3, ?4, 'NOT DEFERRABLE', 3, 1, ?5, ?5)
                 RETURNING id",
                rusqlite::params![
                    table_id,
                    constraint.name,
                    constraint.delete_rule,
                    constraint.update_rule,
                    now,
                ],
                |row| row.get(0),
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_foreign_key".to_string(),
                    reason: e.to_string(),
                }
            ))?;

            for (col_idx, col_name) in constraint.column_names.iter().enumerate() {
                let ref_col = constraint
                    .referenced_columns
                    .get(col_idx)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                tx.execute(
                    "INSERT INTO foreign_key_columns (foreign_key_id, ordinal_position, column_name, ref_column_name) \
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![fk_id, (col_idx + 1) as i32, col_name, ref_col],
                ).map_err(|e| CoreError::storage(
                    StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "save_foreign_key_column".to_string(),
                        reason: e.to_string(),
                    }
                ))?;
            }
        }

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_table_constraints_commit".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(())
    }

    /// 通过 schema_name + table_name 查找 table_id 并保存索引
    pub fn save_indexes_for_table(
        &mut self,
        _conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
        indexes: Vec<crate::driver::IndexDetail>,
    ) -> Result<(), CoreError> {
        let schema_id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM schemata WHERE catalog_name = ? AND schema_name = ?",
                rusqlite::params![catalog, schema],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "find_schema_id".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let table_id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM tables WHERE schema_id = ? AND table_name = ?",
                rusqlite::params![schema_id, table],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "find_table_id".to_string(),
                    reason: e.to_string(),
                })
            })?;

        self.save_table_indexes(table_id, indexes)
    }

    /// 通过 schema_name + table_name 查找 table_id 并保存约束
    pub fn save_constraints_for_table(
        &mut self,
        _conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
        constraints: Vec<crate::driver::ConstraintDetail>,
    ) -> Result<(), CoreError> {
        let schema_id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM schemata WHERE catalog_name = ? AND schema_name = ?",
                rusqlite::params![catalog, schema],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "find_schema_id".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let table_id: i64 = self
            .conn
            .query_row(
                "SELECT id FROM tables WHERE schema_id = ? AND table_name = ?",
                rusqlite::params![schema_id, table],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "find_table_id".to_string(),
                    reason: e.to_string(),
                })
            })?;

        self.save_table_constraints(table_id, constraints)
    }

    /// 从缓存加载表详情（MetadataBrowser::get_table_detail() 格式）
    pub fn load_node_detail(
        &self,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<Option<crate::driver::NodeDetail>, CoreError> {
        let table_row = self.conn.query_row(
            "SELECT t.id, t.table_type, t.table_comment, t.row_count_estimate \
             FROM tables t \
             JOIN schemata s ON t.schema_id = s.id \
             WHERE s.catalog_name = ?1 AND s.schema_name = ?2 AND t.table_name = ?3",
            rusqlite::params![database_name, schema_name, table_name],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            },
        );

        let (table_id, table_type, table_comment, row_count_estimate) = match table_row {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => {
                return Err(CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_node_detail_table".to_string(),
                    reason: e.to_string(),
                }))
            }
        };

        let kind = if table_type == "VIEW" {
            crate::driver::SchemaObjectKind::View
        } else {
            crate::driver::SchemaObjectKind::Table
        };

        let mut stmt = self.conn.prepare(
            "SELECT column_name, data_type, is_nullable, COALESCE(is_primary, 0) AS is_primary_key,
             0 AS is_foreign_key, column_default, column_comment, extra,
             COALESCE(ordinal_position, 0) AS ordinal_position
             FROM columns WHERE table_id = ?1 ORDER BY ordinal_position"
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "load_node_detail_columns".to_string(),
            reason: e.to_string(),
        }))?;

        let columns: Vec<crate::driver::ColumnDetail> = stmt
            .query_map(rusqlite::params![table_id], |row| {
                let extra_json: Option<String> = row.get(7)?;
                let extra = if let Some(ref json_str) = extra_json {
                    serde_json::from_str(json_str).unwrap_or_default()
                } else {
                    std::collections::HashMap::new()
                };

                Ok(crate::driver::ColumnDetail {
                    name: row.get(0)?,
                    data_type: row.get(1)?,
                    nullable: row.get::<_, i32>(2)? != 0,
                    is_primary_key: row.get::<_, i32>(3)? != 0,
                    is_foreign_key: row.get::<_, i32>(4)? != 0,
                    default_value: row.get(5)?,
                    comment: row.get(6)?,
                    extra,
                    // 缓存里存了外键的列对，但那份 join 一张表可以配对到多条外键约束
                    // （同一列属于两个外键时出重复行）—— 在那边收敛之前不从这里给目标
                    references: None,
                    ordinal: u32::try_from(row.get::<_, i32>(8)?).unwrap_or(0),
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_node_detail_map".to_string(),
                    reason: e.to_string(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_node_detail_collect".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let index_count = self.load_index_count(table_id)?;

        Ok(Some(crate::driver::NodeDetail {
            node: crate::driver::NodeInfo::new(table_name.to_string(), kind)
                .with_comment(table_comment),
            columns,
            index_count: Some(index_count as u32),
            row_count_estimate: row_count_estimate.map(|n| n as u32),
        }))
    }

    /// 加载表的索引数量
    fn load_index_count(&self, table_id: i64) -> Result<usize, CoreError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM indexes WHERE table_id = ?1",
                rusqlite::params![table_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_index_count".to_string(),
                    reason: e.to_string(),
                })
            })?
            .unwrap_or(0);

        Ok(count as usize)
    }

    /// 从缓存加载表的索引详情
    pub fn load_table_indexes(&self, table_id: i64) -> Result<Vec<IndexDetailInfo>, CoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT i.id, i.index_name, i.index_type, i.is_unique, i.is_primary, i.index_comment \
             FROM indexes i WHERE i.table_id = ?1 ORDER BY i.index_name"
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "load_table_indexes_prepare".to_string(),
            reason: e.to_string(),
        }))?;

        let indexes: Vec<(i64, IndexDetailInfo)> = stmt
            .query_map(rusqlite::params![table_id], |row| {
                let index_id: i64 = row.get(0)?;
                Ok((
                    index_id,
                    IndexDetailInfo {
                        id: index_id,
                        index_name: row.get(1)?,
                        index_type: row.get(2)?,
                        is_unique: row.get::<_, i32>(3)? != 0,
                        is_primary: row.get::<_, i32>(4)? != 0,
                        index_comment: row.get(5)?,
                        columns: Vec::new(),
                    },
                ))
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_table_indexes_map".to_string(),
                    reason: e.to_string(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_table_indexes_collect".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::with_capacity(indexes.len());
        for (index_id, mut idx) in indexes {
            idx.columns = self.load_index_columns(index_id)?;
            result.push(idx);
        }

        Ok(result)
    }

    /// 加载索引的列信息
    fn load_index_columns(&self, index_id: i64) -> Result<Vec<IndexColumnInfo>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, column_name, ordinal_position, sort_order, is_included_column \
             FROM index_columns WHERE index_id = ?1 ORDER BY ordinal_position",
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_index_columns_prepare".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let columns = stmt
            .query_map(rusqlite::params![index_id], IndexColumnInfo::from_row)
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_index_columns_query".to_string(),
                    reason: e.to_string(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_index_columns_collect".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(columns)
    }

    /// 从缓存加载表的外键约束
    pub fn load_table_foreign_keys(
        &self,
        table_id: i64,
    ) -> Result<Vec<ForeignKeyDetailInfo>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT fk.id, fk.constraint_name, fk.delete_rule, fk.update_rule, \
                    fk.ref_schema_id, fk.ref_table_id \
             FROM foreign_keys fk WHERE fk.table_id = ?1 ORDER BY fk.constraint_name",
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_table_foreign_keys_prepare".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let fks: Vec<(i64, ForeignKeyDetailInfo)> = stmt
            .query_map(rusqlite::params![table_id], |row| {
                let fk_id: i64 = row.get(0)?;
                Ok((
                    fk_id,
                    ForeignKeyDetailInfo {
                        id: fk_id,
                        constraint_name: row.get(1)?,
                        delete_rule: row.get(2)?,
                        update_rule: row.get(3)?,
                        ref_schema_id: row.get(4)?,
                        ref_table_id: row.get(5)?,
                        columns: Vec::new(),
                    },
                ))
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_table_foreign_keys_map".to_string(),
                    reason: e.to_string(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_table_foreign_keys_collect".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::with_capacity(fks.len());
        for (fk_id, mut fk) in fks {
            fk.columns = self.load_foreign_key_columns(fk_id)?;
            result.push(fk);
        }

        Ok(result)
    }

    /// 加载外键的列映射信息
    fn load_foreign_key_columns(&self, fk_id: i64) -> Result<Vec<ForeignKeyColumnInfo>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, ordinal_position, column_name, ref_column_name \
             FROM foreign_key_columns WHERE foreign_key_id = ?1 ORDER BY ordinal_position",
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_foreign_key_columns_prepare".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let columns = stmt
            .query_map(rusqlite::params![fk_id], ForeignKeyColumnInfo::from_row)
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_foreign_key_columns_query".to_string(),
                    reason: e.to_string(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "load_foreign_key_columns_collect".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(columns)
    }

    /// 检查缓存是否有效（默认 24 小时）
    ///
    /// # 参数
    /// * `database_name` - 数据库名称
    /// * `schema_name` - 模式名称
    /// * `max_age_seconds` - 最大缓存时间（秒），默认 86400 秒（24 小时）
    pub fn is_cache_valid(
        &self,
        database_name: &str,
        schema_name: &str,
        max_age_seconds: Option<i64>,
    ) -> Result<bool, CoreError> {
        let max_age = max_age_seconds.unwrap_or(86400);
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let last_sync: Option<i64> = self
            .conn
            .query_row(
                "SELECT MAX(last_sync) FROM metadata
             WHERE database_name = ?1 AND schema_name = ?2",
                rusqlite::params![database_name, schema_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "check_cache_validity".to_string(),
                    reason: e.to_string(),
                })
            })?
            .flatten();

        match last_sync {
            Some(last_sync_time) => Ok((current_time - last_sync_time) < max_age),
            None => Ok(false),
        }
    }

    /// 重建某 schema 的 FTS5 索引（幂等：先删该 schema 行，再按规范化表重插）。
    ///
    /// **为什么按 schema 而不是全库**：写入侧挂在冷启动内省上（一次一个 schema），
    /// 全库重建会让每次内省都付出整库代价。删除按 `schema_name` 定位——`metadata_fts`
    /// 没有 schema_id，与 `delete_schema` 的口径一致。
    ///
    /// 语料：schema 名 / 对象名 / 注释（列另加数据类型、例程另加类型）；
    /// **不含视图定义与例程源码**（那是按需加载的详情层）。
    ///
    /// 历史缺陷（本函数取代的 `sync_fts_index`）：尾部两个 INSERT 写的是
    /// `FROM views v` / `FROM v_routines`，而规范化模型里**没有这两张表**
    /// （视图是 `tables.table_type='VIEW'`，例程在 `routines`）——语句报错、整个同步
    /// 永远跑不通，这也是它一直零调用的真因。
    pub fn rebuild_fts_schema(&mut self, schema: &str) -> Result<usize, CoreError> {
        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "begin_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 先删后插：刷新后已不存在的对象不得留在索引里
        tx.execute(
            "DELETE FROM metadata_fts WHERE schema_name = ?1",
            rusqlite::params![schema],
        )
        .map_err(|e| fts_err("clear_fts_schema", e))?;

        let mut inserted = 0usize;

        // schema
        inserted += tx
            .execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'schema', schema_name, schema_name, '', schema_name
                 FROM schemata WHERE schema_name = ?1 AND is_loaded = 1",
                rusqlite::params![schema],
            )
            .map_err(|e| fts_err("sync_fts_schema", e))?;

        // 表（排除视图：视图单独一类，与导航树的分类一致）
        inserted += tx
            .execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'table', s.schema_name, t.table_name, s.schema_name,
                        s.schema_name || ' ' || t.table_name || ' ' || COALESCE(t.table_comment, '')
                 FROM tables t INNER JOIN schemata s ON t.schema_id = s.id
                 WHERE s.schema_name = ?1 AND t.is_loaded = 1 AND t.table_type <> 'VIEW'",
                rusqlite::params![schema],
            )
            .map_err(|e| fts_err("sync_fts_table", e))?;

        // 视图（视图是 `tables` 里 table_type='VIEW' 的行；注释在 `table_comment`）
        inserted += tx
            .execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'view', s.schema_name, t.table_name, s.schema_name,
                        s.schema_name || ' ' || t.table_name || ' ' || COALESCE(t.table_comment, '')
                 FROM tables t INNER JOIN schemata s ON t.schema_id = s.id
                 WHERE s.schema_name = ?1 AND t.is_loaded = 1 AND t.table_type = 'VIEW'",
                rusqlite::params![schema],
            )
            .map_err(|e| fts_err("sync_fts_view", e))?;

        // 列（多带上数据类型与列注释：全文档档里“按类型找列”是高频用法）
        inserted += tx
            .execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'column', s.schema_name, c.column_name, t.table_name,
                        s.schema_name || ' ' || t.table_name || ' ' || c.column_name || ' '
                        || COALESCE(c.data_type, '') || ' ' || COALESCE(c.column_comment, '')
                 FROM columns c
                 INNER JOIN tables t ON c.table_id = t.id
                 INNER JOIN schemata s ON t.schema_id = s.id
                 WHERE s.schema_name = ?1 AND c.is_loaded = 1",
                rusqlite::params![schema],
            )
            .map_err(|e| fts_err("sync_fts_column", e))?;

        // 例程
        inserted += tx
            .execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'routine', s.schema_name, r.routine_name, s.schema_name,
                        s.schema_name || ' ' || r.routine_name || ' ' || COALESCE(r.routine_type, '') || ' '
                        || COALESCE(r.routine_comment, '')
                 FROM routines r INNER JOIN schemata s ON r.schema_id = s.id
                 WHERE s.schema_name = ?1 AND r.is_loaded = 1",
                rusqlite::params![schema],
            )
            .map_err(|e| fts_err("sync_fts_routine", e))?;

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "commit_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;
        Ok(inserted)
    }

    /// FTS5 全文搜索
    ///
    /// # 参数
    /// * `query` - 搜索关键词
    /// * `search_type` - 搜索类型（可选）：schema, table, column, view, routine
    pub fn search_fts(
        &self,
        query: &str,
        search_type: Option<&str>,
    ) -> Result<Vec<FtsSearchResult>, CoreError> {
        // 查询词清洗：裸拼 `format!("{}*", query)` 会被用户输入里的 `"` `*` `(` `NEAR` `-`
        // 破坏 MATCH 语法（报错或语义反转）——拆词后逐词加引号、末词保留前缀，见 `fts_match_query`。
        let Some(search_pattern) = fts_match_query(query) else {
            return Ok(Vec::new());
        };

        let sql = match search_type {
            Some(_t) => {
                "SELECT search_type, schema_name, object_name, parent_name,
                               snippet(metadata_fts, 4, '<mark>', '</mark>', '...', 32) as snippet
                        FROM metadata_fts WHERE search_content MATCH ?1 AND search_type = ?2
                        ORDER BY rank LIMIT 50"
            }
            None => {
                "SELECT search_type, schema_name, object_name, parent_name,
                            snippet(metadata_fts, 4, '<mark>', '</mark>', '...', 32) as snippet
                     FROM metadata_fts WHERE search_content MATCH ?1
                     ORDER BY rank LIMIT 50"
            }
        };

        let mut stmt = self.conn.prepare(sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "search_fts".to_string(),
                reason: e.to_string(),
            })
        })?;

        let mut result = Vec::new();

        if let Some(t) = search_type {
            let rows = stmt
                .query_map(rusqlite::params![search_pattern, t], |row| {
                    Ok(FtsSearchResult {
                        search_type: row.get(0)?,
                        schema_name: row.get(1)?,
                        object_name: row.get(2)?,
                        parent_name: row.get(3)?,
                        snippet: row.get(4)?,
                    })
                })
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "search_fts".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            for r in rows {
                result.push(r.map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "fetch_fts_result".to_string(),
                        reason: e.to_string(),
                    })
                })?);
            }
        } else {
            let rows = stmt
                .query_map(rusqlite::params![search_pattern], |row| {
                    Ok(FtsSearchResult {
                        search_type: row.get(0)?,
                        schema_name: row.get(1)?,
                        object_name: row.get(2)?,
                        parent_name: row.get(3)?,
                        snippet: row.get(4)?,
                    })
                })
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "search_fts".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            for r in rows {
                result.push(r.map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "fetch_fts_result".to_string(),
                        reason: e.to_string(),
                    })
                })?);
            }
        }

        Ok(result)
    }

    /// 删除 Schema 及关联数据（级联），返回删除的行数
    ///
    /// 计数含：schema 本身 + 表/视图（规范化模型里视图是 `tables` 的一行）+ 例程；
    /// 子表（columns / indexes / view_definitions 等）由外键级联删除，不计入。
    pub fn delete_schema(&mut self, schema_id: i64) -> Result<usize, CoreError> {
        // 先获取 schema 信息用于 FTS 清理
        let schema_name: String = self
            .conn
            .query_row(
                "SELECT schema_name FROM schemata WHERE id = ?1",
                rusqlite::params![schema_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_schema_name".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "begin_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 删除 tables —— **视图也在这张表里**（`table_type = 'VIEW'`）：规范化模型没有
        // 独立的 `views` 表（见迁移 005/006/007 的注释），本语句同时清掉表与视图。
        // columns / indexes / foreign_keys / check_constraints / view_definitions /
        // triggers 均由外键 `ON DELETE CASCADE` 级联（`open()` 已设 `PRAGMA foreign_keys=ON`）。
        //
        // 谨记：这里曾写有 `DELETE FROM views`，而该表根本不存在——整条语句报
        // "no such table: views"、事务回滚，调用方（`NavCache::prune_schema`）用
        // `let _ =` 吞掉错误，于是「刷新元数据」永远清不掉旧行。
        let table_count = tx
            .execute(
                "DELETE FROM tables WHERE schema_id = ?1",
                rusqlite::params![schema_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "delete_tables".to_string(),
                    reason: e.to_string(),
                })
            })?;

        // 删除 routines
        let routine_count = tx
            .execute(
                "DELETE FROM routines WHERE schema_id = ?1",
                rusqlite::params![schema_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "delete_routines".to_string(),
                    reason: e.to_string(),
                })
            })?;

        // 删除 schema 本身
        let schema_deleted = tx
            .execute(
                "DELETE FROM schemata WHERE id = ?1",
                rusqlite::params![schema_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "delete_schema".to_string(),
                    reason: e.to_string(),
                })
            })?;

        // 清 `metadata_index` 的对应行。
        //
        // 它**不在外键级联链上**（建表时 `schema_id` 只是普通列，没有 REFERENCES），
        // 所以必须显式删：否则刷新/删除 schema 后，分页读与计数会读到已不存在的对象
        // （测试 `rebuild_schema_index_powers_chunks_and_counts` 就抓到了这个孤儿）。
        tx.execute(
            "DELETE FROM metadata_index WHERE schema_id = ?1",
            rusqlite::params![schema_id],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "delete_schema_index".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 清理 FTS 索引
        tx.execute(
            "DELETE FROM metadata_fts WHERE schema_name = ?1",
            rusqlite::params![schema_name],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "delete_fts".to_string(),
                reason: e.to_string(),
            })
        })?;

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "commit_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(schema_deleted + table_count + routine_count)
    }

    // ==================== V6: 索引表与懒加载 ====================

    /// 保存索引表条目（支持分页懒加载）
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    /// * `object_type` - 对象类型：schema, table, view, column, index, routine
    /// * `object_name` - 对象名称
    /// * `parent_name` - 父对象名称（如表名对于列）
    /// * `path` - 层级路径
    /// * `introspect_level` - 自省级别（1=索引, 2=概要, 3=详情）
    #[allow(clippy::too_many_arguments)]
    pub fn save_index_entry(
        &self,
        connection_id: &str,
        schema_id: Option<i64>,
        object_type: &str,
        object_name: &str,
        parent_name: Option<&str>,
        path: &str,
        introspect_level: i32,
    ) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn
            .execute(
                "INSERT OR REPLACE INTO metadata_index
             (connection_id, schema_id, object_type, object_name, parent_name, path,
              introspect_level, is_loaded, last_sync)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                     CASE WHEN ?7 >= 3 THEN 1 ELSE 0 END, ?8)",
                rusqlite::params![
                    connection_id,
                    schema_id,
                    object_type,
                    object_name,
                    parent_name,
                    path,
                    introspect_level,
                    now
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_index_entry".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }

    /// 批量保存索引表条目（高效批量插入）
    pub fn save_index_entries_batch(
        &mut self,
        entries: Vec<IndexEntryInput>,
    ) -> Result<usize, CoreError> {
        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "begin_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let mut count = 0;
        for entry in entries {
            tx.execute(
                "INSERT OR REPLACE INTO metadata_index
                 (connection_id, schema_id, object_type, object_name, parent_name, path,
                  introspect_level, is_loaded, last_sync, row_count_estimate, sort_weight)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7,
                         CASE WHEN ?7 >= 3 THEN 1 ELSE 0 END, ?8, ?9, ?10)",
                rusqlite::params![
                    entry.connection_id,
                    entry.schema_id,
                    entry.object_type,
                    entry.object_name,
                    entry.parent_name,
                    entry.path,
                    entry.introspect_level,
                    now,
                    entry.row_count_estimate,
                    entry.sort_weight
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_index_batch".to_string(),
                    reason: e.to_string(),
                })
            })?;
            count += 1;
        }

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "commit_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(count)
    }

    /// 按已写入的 `tables` / `columns` 重建某 schema 的 `metadata_index` 行（幂等：先删后插）。
    ///
    /// **为什么需要**：`metadata_index` 是「大 schema 分页 / 计数 / 搜索」的数据源，
    /// 但它的两个写入方（`build_metadata_index`、`save_index_entry`）此前零调用 → 表恒空 →
    /// 分页读（`get_objects_chunk`）接了也是空。这里把写入侧挂到**缓存写入路径**上：
    /// 冷启动内省一个 schema 后重建一次；命中路径只读不写，不触发重建。
    ///
    /// 粒度与 `schemata`/`tables`/`columns` 三层对齐（schema / table / view / column），
    /// `path` 形如 `schema/table/column`（供虚拟树按路径定位）。
    pub fn rebuild_schema_index(
        &mut self,
        connection_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<usize, CoreError> {
        let Some(schema_id) = self.get_schema_id(catalog, schema)? else {
            return Ok(0);
        };

        // 该 schema 已缓存的表 / 视图（含 id 与行数估算，不必逐对象再查）
        let tables = self.list_tables_normalized(schema_id, None)?;

        let mut entries: Vec<IndexEntryInput> = Vec::with_capacity(tables.len() + 1);
        // 本次重建的统一时间戳（与 `save_index_entries_batch` 的写入语义对齐）
        let synced_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        entries.push(IndexEntryInput {
            connection_id: connection_id.to_string(),
            schema_id: Some(schema_id),
            object_type: "schema".to_string(),
            object_name: schema.to_string(),
            parent_name: None,
            path: schema.to_string(),
            introspect_level: 3,
            row_count_estimate: None,
            sort_weight: None,
            last_sync: Some(synced_at),
        });

        for table in &tables {
            let is_view = table.table_type == "VIEW";
            entries.push(IndexEntryInput {
                connection_id: connection_id.to_string(),
                schema_id: Some(schema_id),
                object_type: if is_view { "view" } else { "table" }.to_string(),
                object_name: table.table_name.clone(),
                parent_name: None,
                path: format!("{schema}/{}", table.table_name),
                introspect_level: 3,
                row_count_estimate: table.row_count_estimate,
                sort_weight: None,
                last_sync: Some(synced_at),
            });

            for column in self.list_columns_normalized(table.id)? {
                entries.push(IndexEntryInput {
                    connection_id: connection_id.to_string(),
                    schema_id: Some(schema_id),
                    object_type: "column".to_string(),
                    object_name: column.column_name.clone(),
                    parent_name: Some(table.table_name.clone()),
                    path: format!("{schema}/{}/{}", table.table_name, column.column_name),
                    introspect_level: 3,
                    row_count_estimate: None,
                    sort_weight: None,
                    last_sync: Some(synced_at),
                });
            }
        }

        // 先删后插：刷新后已不存在的对象不得留在索引里。
        // （注：`metadata_index` 的 UNIQUE 含 `parent_name`，而顶层对象该列为 NULL ——
        //  SQLite 视 NULL 为互不相同，所以**不能**依赖 OR REPLACE 去重，必须显式删。）
        self.conn
            .execute(
                "DELETE FROM metadata_index WHERE connection_id = ?1 AND schema_id = ?2",
                rusqlite::params![connection_id, schema_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "clear_schema_index".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let indexed = self.save_index_entries_batch(entries)?;
        // FTS 写侧与索引**同批**（内省一个 schema → 两个索引一起更新）：
        // 搜索路径只读；FTS 失败不该拖垮导航赖以分页 / 计数的 `metadata_index`，
        // 因此只告警留痕（搜索侧拿不到结果时会回落到名称档）。
        if let Err(e) = self.rebuild_fts_schema(schema) {
            tracing::warn!(schema, error = %e, "FTS 索引重建失败（本次仅有名称档可用）");
        }
        Ok(indexed)
    }

    /// 分页获取索引条目（支持懒加载）
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    /// * `object_type` - 对象类型
    /// * `schema_id` - Schema ID（可选）
    /// * `page` - 页码（从 1 开始）
    /// * `page_size` - 每页数量
    pub fn get_index_entries(
        &self,
        connection_id: &str,
        object_type: &str,
        schema_id: Option<i64>,
        page: u32,
        page_size: u32,
    ) -> Result<PaginatedIndexResult, CoreError> {
        let offset = (page - 1) * page_size;

        let count_sql = match schema_id {
            Some(_) => {
                "SELECT COUNT(*) FROM metadata_index
                       WHERE connection_id = ?1 AND object_type = ?2 AND schema_id = ?3"
            }
            None => {
                "SELECT COUNT(*) FROM metadata_index
                     WHERE connection_id = ?1 AND object_type = ?2"
            }
        };

        let total: i64 = match schema_id {
            Some(sid) => self
                .conn
                .query_row(
                    count_sql,
                    rusqlite::params![connection_id, object_type, sid],
                    |row| row.get(0),
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "count_index_entries".to_string(),
                        reason: e.to_string(),
                    })
                })?,
            None => self
                .conn
                .query_row(
                    count_sql,
                    rusqlite::params![connection_id, object_type],
                    |row| row.get(0),
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "count_index_entries".to_string(),
                        reason: e.to_string(),
                    })
                })?,
        };

        let query_sql = match schema_id {
            Some(_) => "SELECT id, schema_id, object_type, object_name, parent_name, path,
                               introspect_level, is_loaded, last_sync, row_count_estimate, sort_weight
                        FROM metadata_index
                        WHERE connection_id = ?1 AND object_type = ?2 AND schema_id = ?3
                        ORDER BY sort_weight DESC, object_name ASC
                        LIMIT ?4 OFFSET ?5",
            None => "SELECT id, schema_id, object_type, object_name, parent_name, path,
                            introspect_level, is_loaded, last_sync, row_count_estimate, sort_weight
                     FROM metadata_index
                     WHERE connection_id = ?1 AND object_type = ?2
                     ORDER BY sort_weight DESC, object_name ASC
                     LIMIT ?3 OFFSET ?4",
        };

        let mut stmt = self.conn.prepare(query_sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_index_entries".to_string(),
                reason: e.to_string(),
            })
        })?;

        let entries = match schema_id {
            Some(sid) => stmt
                .query_map(
                    rusqlite::params![connection_id, object_type, sid, page_size, offset],
                    IndexEntry::from_row,
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "query_index_entries".to_string(),
                        reason: e.to_string(),
                    })
                })?,
            None => stmt
                .query_map(
                    rusqlite::params![connection_id, object_type, page_size, offset],
                    IndexEntry::from_row,
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "query_index_entries".to_string(),
                        reason: e.to_string(),
                    })
                })?,
        };

        let mut result = Vec::new();
        for entry in entries {
            result.push(entry.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_index_entry".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(PaginatedIndexResult {
            entries: result,
            total: total as usize,
            page,
            page_size,
            total_pages: ((total as f64) / (page_size as f64)).ceil() as u32,
        })
    }

    // ==================== V6: 分块读取 ====================

    /// 根据对象数量计算内省级别（DataGrip 风格）
    ///
    /// # 参数
    /// * `object_count` - 对象数量
    /// * `is_current_schema` - 是否为当前 schema
    ///
    /// # DataGrip 规则
    /// - N <= 1000 (当前) / N <= 3000 (非当前) → Level 3 (完整加载)
    /// - N <= 3000 (当前) / N <= 10000 (非当前) → Level 2 (概要)
    /// - 否则 → Level 1 (仅索引)
    pub fn calculate_introspect_level(&self, object_count: i64, is_current_schema: bool) -> i32 {
        if is_current_schema {
            if object_count <= 1000 {
                3 // Level 3: 完整加载
            } else if object_count <= 3000 {
                2 // Level 2: 概要
            } else {
                1 // Level 1: 仅索引
            }
        } else {
            if object_count <= 3000 {
                3 // Level 3: 完整加载
            } else if object_count <= 10000 {
                2 // Level 2: 概要
            } else {
                1 // Level 1: 仅索引
            }
        }
    }

    /// 获取 schema 的对象数量统计
    pub fn get_schema_object_counts(
        &self,
        connection_id: &str,
        schema_id: i64,
    ) -> Result<SchemaObjectCounts, CoreError> {
        let table_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM metadata_index WHERE connection_id = ?1 AND schema_id = ?2 AND object_type = 'table'",
            rusqlite::params![connection_id, schema_id],
            |row| row.get(0),
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "count_tables".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let view_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM metadata_index WHERE connection_id = ?1 AND schema_id = ?2 AND object_type = 'view'",
            rusqlite::params![connection_id, schema_id],
            |row| row.get(0),
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "count_views".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let column_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM metadata_index WHERE connection_id = ?1 AND schema_id = ?2 AND object_type = 'column'",
            rusqlite::params![connection_id, schema_id],
            |row| row.get(0),
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "count_columns".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let routine_count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM metadata_index WHERE connection_id = ?1 AND schema_id = ?2 AND object_type = 'routine'",
            rusqlite::params![connection_id, schema_id],
            |row| row.get(0),
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "count_routines".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(SchemaObjectCounts {
            table_count: table_count as usize,
            view_count: view_count as usize,
            column_count: column_count as usize,
            routine_count: routine_count as usize,
            total: (table_count + view_count + column_count + routine_count) as usize,
        })
    }

    /// 分块获取某类别的对象名（避免大 schema 一次物化整表 / OOM）。
    ///
    /// **为什么泛化**：原实现把 `object_type = 'table'` 写死在 SQL 里（`get_tables_chunk`），
    /// 而导航树「视图」文件夹在视图数量很大的 schema 下同样需要分块读——两者除类别外
    /// 查询完全一致，故合并为一个实现，由 `object_type` 参数区分。
    ///
    /// 排序固定为 `sort_weight DESC, object_name ASC`：当前没有写入方填 `sort_weight`
    /// （全为 NULL，SQLite 的 DESC 把 NULL 排在最后），因此实际等价于**按名称升序**，
    /// 与索引里的顺序稳定一致——这一点是分块读的前提（offset 翻页不能有随机顺序）。
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    /// * `schema_id` - Schema ID（`None` = 不限定 schema，跨 schema 取）
    /// * `object_type` - 类别：`table` / `view`（取值受 `metadata_index` 的 CHECK 约束）
    /// * `offset` - 偏移量
    /// * `limit` - 每块大小
    pub fn get_objects_chunk(
        &self,
        connection_id: &str,
        schema_id: Option<i64>,
        object_type: &str,
        offset: i64,
        limit: i64,
    ) -> Result<ChunkResult<IndexEntry>, CoreError> {
        let (count_sql, query_sql) = match schema_id {
            Some(_sid) => (
                "SELECT COUNT(*) FROM metadata_index WHERE connection_id = ?1 AND object_type = ?2 AND schema_id = ?3",
                "SELECT id, schema_id, object_type, object_name, parent_name, path,
                        introspect_level, is_loaded, last_sync, row_count_estimate, sort_weight
                 FROM metadata_index
                 WHERE connection_id = ?1 AND object_type = ?2 AND schema_id = ?3
                 ORDER BY sort_weight DESC, object_name ASC
                 LIMIT ?4 OFFSET ?5",
            ),
            None => (
                "SELECT COUNT(*) FROM metadata_index WHERE connection_id = ?1 AND object_type = ?2",
                "SELECT id, schema_id, object_type, object_name, parent_name, path,
                        introspect_level, is_loaded, last_sync, row_count_estimate, sort_weight
                 FROM metadata_index
                 WHERE connection_id = ?1 AND object_type = ?2
                 ORDER BY sort_weight DESC, object_name ASC
                 LIMIT ?3 OFFSET ?4",
            ),
        };

        let total: i64 = match schema_id {
            Some(sid) => self
                .conn
                .query_row(
                    count_sql,
                    rusqlite::params![connection_id, object_type, sid],
                    |row| row.get(0),
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "count_objects_chunk".to_string(),
                        reason: e.to_string(),
                    })
                })?,
            None => self
                .conn
                .query_row(
                    count_sql,
                    rusqlite::params![connection_id, object_type],
                    |row| row.get(0),
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "count_objects_chunk".to_string(),
                        reason: e.to_string(),
                    })
                })?,
        };

        let mut stmt = self.conn.prepare(query_sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_objects_chunk".to_string(),
                reason: e.to_string(),
            })
        })?;

        let entries = match schema_id {
            Some(sid) => stmt
                .query_map(
                    rusqlite::params![connection_id, object_type, sid, limit, offset],
                    IndexEntry::from_row,
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "query_objects_chunk".to_string(),
                        reason: e.to_string(),
                    })
                })?,
            None => stmt
                .query_map(
                    rusqlite::params![connection_id, object_type, limit, offset],
                    IndexEntry::from_row,
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "query_objects_chunk".to_string(),
                        reason: e.to_string(),
                    })
                })?,
        };

        let mut result = Vec::new();
        for entry in entries {
            result.push(entry.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_object_chunk".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(ChunkResult {
            items: result,
            total: total as usize,
            offset: offset as usize,
            limit: limit as usize,
            has_more: (offset + limit) < total,
        })
    }

    /// 目标对象在其（连接 + schema + 类别）里的**零基位次**（与 [`Self::get_objects_chunk`] 同一排序口径）。
    ///
    /// **为什么需要它**：大 schema 分页后，树里只加载了首屏。从搜索命中「定位」到第 9000 条
    /// 不能靠一页页追加（45 次往返），必须一次算出它落在哪一页。
    ///
    /// 返回 `None` = 索引里没有这个对象（索引未重建 / 对象已删）——调用方须**如实告知**，
    /// 不要假装「它在别处」或默默不动作。
    ///
    /// 同名多行（同一 schema 下重名）取**最早**的那个位次；`is_loaded` 不参与过滤，
    /// 与分页查询保持一致（否则位次与页内容会错位）。
    pub fn get_object_position(
        &self,
        connection_id: &str,
        schema_id: Option<i64>,
        object_type: &str,
        object_name: &str,
    ) -> Result<Option<i64>, CoreError> {
        // 排序键必须与 `get_objects_chunk` **逐字一致**（`sort_weight DESC, object_name ASC`），
        // 否则算出来的页里没有目标。
        //
        // 不用「COUNT 比它小的行」那种写法：`sort_weight` 目前**没有写入方**（恒为 NULL），
        // 而 SQL 里 `NULL > NULL` / `NULL = NULL` 都是 NULL（一个都不计）——位次会静默退化成 0，
        // 「定位」永远跳到第一页。窗口函数直接复用同一个 ORDER BY，不碰 NULL 语义。
        let sql = match schema_id {
            Some(_sid) => {
                "SELECT pos - 1 FROM (
                     SELECT ROW_NUMBER() OVER (ORDER BY sort_weight DESC, object_name ASC) AS pos,
                            object_name
                     FROM metadata_index
                     WHERE connection_id = ?1 AND object_type = ?2 AND schema_id = ?3
                 ) WHERE object_name = ?4
                 ORDER BY pos LIMIT 1"
            }
            None => {
                "SELECT pos - 1 FROM (
                     SELECT ROW_NUMBER() OVER (ORDER BY sort_weight DESC, object_name ASC) AS pos,
                            object_name
                     FROM metadata_index
                     WHERE connection_id = ?1 AND object_type = ?2
                 ) WHERE object_name = ?3
                 ORDER BY pos LIMIT 1"
            }
        };

        let position: Option<i64> = match schema_id {
            Some(sid) => self
                .conn
                .query_row(
                    sql,
                    rusqlite::params![connection_id, object_type, sid, object_name],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|e| Self::index_error("get_object_position", &e))?,
            None => self
                .conn
                .query_row(
                    sql,
                    rusqlite::params![connection_id, object_type, object_name],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|e| Self::index_error("get_object_position", &e))?,
        };

        Ok(position.map(|p| p.max(0)))
    }

    /// 索引类查询的统一错误包装（与既有写法同形，便于日志定位）。
    fn index_error(operation: &str, e: &rusqlite::Error) -> CoreError {
        CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: operation.to_string(),
            reason: e.to_string(),
        })
    }

    /// 转义 LIKE 的通配符（`%` `_`）与转义符自身，使用户输入按**字面量**匹配。
    fn like_escape(needle: &str) -> String {
        let mut out = String::with_capacity(needle.len());
        for ch in needle.chars() {
            if matches!(ch, '\\' | '%' | '_') {
                out.push('\\');
            }
            out.push(ch);
        }
        out
    }

    /// 名称档**前缀段** SQL 模板：一段扫一个类别，`{category}` 由
    /// [`Self::SEARCH_INDEX_CATEGORY_PREDICATES`] 的编译期常量替换（不含用户输入）。
    ///
    /// 为什么不用 `LIKE 'needle%'`：SQLite 的 LIKE 优化要求模式是**字面量**或列上有 NOCASE
    /// 排序规则，参数化的中缀 / 前缀模式一律退化成全表扫；范围比较则能走
    /// `idx_metadata_index_conn_type_object_lower`（见迁移 013）。
    ///
    /// 为什么按类别分开扫：排序键拿掉 `LENGTH` 后是「完全相等 → 类别 → 名序」，
    /// 而索引是「连接 + 类别 + 名序」——同一个类别内 `ORDER BY LOWER(object_name)` 恰好
    /// 就是索引序，于是**不用排序、不用窗口**：等值列在前、范围列紧随，沿索引走、取够 limit 条即停。
    /// 类别之间的次序由多次扫描的合并顺序（`search_index` 里的稳定排序）保证。
    ///
    /// 上界的取值：`needle || U+10FFFF`（合法 UTF-8 的最大字符）。UTF-8 的字节序与码点序一致，
    /// 且任何合法字符串的首字节 ≤ `0xF4`，所以 `>= needle AND <= 上界` 恰好等价于「以 needle 开头」。
    /// 注意两侧都必须是**未转义**的 needle：范围比较按字节比对，转义符会变成真字符。
    ///
    /// 参数顺序：连接 / 下界 / 上界 / limit。
    const SQL_SEARCH_INDEX_PREFIX_TEMPLATE: &str = "
        SELECT mi.object_type, mi.object_name, mi.parent_name,
               s.catalog_name, s.schema_name, mi.row_count_estimate
        FROM metadata_index mi
        LEFT JOIN schemata s ON s.id = mi.schema_id
        WHERE mi.connection_id = ?1
          AND {category}
          AND LOWER(mi.object_name) >= ?2 AND LOWER(mi.object_name) <= ?3
        ORDER BY LOWER(mi.object_name)
        LIMIT ?4";

    /// 前缀段要扫的**类别**（与排序里的类别权重同序），一笔查询一段：表 → 视图 → schema → 列。
    ///
    /// 为何只列这四类：它们是 `metadata_index` 当前**仅有**的写入面
    /// （`rebuild_schema_index` / `build_metadata_index` 只写这四种）；
    /// 其余 CHECK 允许的类型（`index` / `routine` / `routine_param`）现无写入路径，
    /// 且它们既不能在索引里与范围列同扫（写完得跟一个 `NOT IN`，就退化成整连接全扫、还要排序），
    /// 所以把这类行交给**回落段**（中缀全扫能覆盖一切类型）——它们本来也排在类别末位，可观测差异极小。
    const SEARCH_INDEX_CATEGORY_PREDICATES: [&str; 4] = [
        "mi.object_type = 'table'",
        "mi.object_type = 'view'",
        "mi.object_type = 'schema'",
        "mi.object_type = 'column'",
    ];

    /// 类别权重（与回落段 SQL 的 `CASE … END` 必须一致）：表 → 视图 → schema → 列 → 其余。
    fn search_index_category_rank(object_type: &str) -> usize {
        match object_type {
            "table" => 0,
            "view" => 1,
            "schema" => 2,
            "column" => 3,
            _ => 4,
        }
    }

    /// 名称档**中缀回落段** SQL（模板，同上按类别替换 `{category}`）：前缀段没装满 `limit` 时
    /// 补足剩余名额（名额 = limit - 前缀命中数）。
    ///
    /// `NOT (范围)` 与前缀段的**并集**互补，两段的排序口径一致（完全相等 → 类别 → 名序），
    /// 所以「前缀段 + 回落段」拼接起来就是优化前那条单语句的结果（逐条相同，见
    /// `search_index_two_stage_matches_single_query_oracle`）。
    ///
    /// 为什么也按类别扫：`LIKE '%needle%'` 本身用不上索引（前导通配符），但**排序用得上**——
    /// 沿索引名序走、把 LIKE 当过滤谓词，取够 `limit` 条即停（实测 10 万对象 / debug：
    /// 单类 0.2 ms 级，对比「全表扫 + 全量排序」的 40 ms 级）；且免排序后中缀行的次序与
    /// 前缀段同口径（都是 `LOWER(object_name)` 名序），不再是 `object_name` 原始串序。
    /// 只有在前缀没填满时才会走到这里；命中的是稀疏词时最坏要扫完该类整段（与旧写法的
    /// 全表扫同量级，仍免了排序）。
    ///
    /// 参数顺序：连接 / 中缀模式（已转义） / 下界 / 上界 / 剩余名额。
    const SQL_SEARCH_INDEX_FALLBACK_TEMPLATE: &str = "
        SELECT mi.object_type, mi.object_name, mi.parent_name,
               s.catalog_name, s.schema_name, mi.row_count_estimate
        FROM metadata_index mi
        LEFT JOIN schemata s ON s.id = mi.schema_id
        WHERE mi.connection_id = ?1
          AND {category}
          AND LOWER(mi.object_name) LIKE ?2 ESCAPE '\\'
          AND NOT (LOWER(mi.object_name) >= ?3 AND LOWER(mi.object_name) <= ?4)
        ORDER BY LOWER(mi.object_name)
        LIMIT ?5";

    /// 把模板里的 `{category}` 换成实参，得到一段查询（实参来自编译期常量，无注入面）。
    fn search_index_category_sql(template: &str, category: &str) -> String {
        template.replace("{category}", category)
    }

    /// 命中档位（与 SQL 排序口径一致）：0 = 完全相等；1 = 前缀；2 = 其余（中缀）。
    ///
    /// 范围比较在 Rust 里重现一遍的原因：合并时要在 Rust 侧定档（不能只靠「哪段查出来的」——
    /// 两段合并后要按统一键排序）。`to_ascii_lowercase` 与 SQLite 内建 `LOWER()` 同为 ASCII 折叠，
    /// `str` 比较与 SQLite 默认 BINARY 排序同为字节序，两侧口径一致。
    fn search_index_rank(object_name: &str, needle: &str, upper: &str) -> usize {
        if object_name.eq_ignore_ascii_case(needle) {
            return 0;
        }
        let lower = object_name.to_ascii_lowercase();
        if lower.as_str() >= needle && lower.as_str() <= upper {
            1
        } else {
            2
        }
    }

    /// 执行名称档的一段查询（两段共用同一套行映射与错误口径，`params` 顺序见各 SQL 常量）。
    fn query_index_hits(
        &self,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<IndexSearchHit>, CoreError> {
        let mut stmt = self.conn.prepare(sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_search_index".to_string(),
                reason: e.to_string(),
            })
        })?;

        let rows = stmt
            .query_map(params, |row| {
                Ok(IndexSearchHit {
                    object_type: row.get(0)?,
                    object_name: row.get(1)?,
                    parent_name: row.get(2)?,
                    catalog_name: row.get(3)?,
                    schema_name: row.get(4)?,
                    row_count_estimate: row.get(5)?,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "search_index".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "read_search_index_hit".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }
        Ok(out)
    }

    /// 按名称模糊搜索索引（跨 schema；结果带定位所需的 catalog / schema / 父对象）。
    ///
    /// **为什么名称档用 `metadata_index` 而不是 `metadata_fts`**：
    /// - 用户要的是「按名字找对象」：敲 `ord` 应当命中 `order_items`（**中缀**匹配）。
    ///   `metadata_index` + LIKE 天然支持；trigram 的 FTS 虽是子串匹配，但 **< 3 字查不到**
    ///   （不足一个 trigram），而名称档恰恰经常只敲一两个字；
    /// - 名称档还要给「完全相等→前缀」的稳定排序与类别权重（见下），LIKE 一次性算完。
    ///
    /// FTS 的独有价值是**内容**（注释 / 数据类型）：写入侧已接线
    /// （`rebuild_fts_schema` 跟内省同批，见 `rebuild_schema_index`），读侧 `search_fts`
    /// 用于 Quick Open 的 `#` 全文档档；重建后由 `fts_rebuild_is_schema_scoped_and_idempotent`
    /// 与 `search_fts_returns_identity_snippet_and_survives_operator_input` 两项测试钉住。
    ///
    /// 排序：名称完全相等 → 前缀命中 → 其余；同档内按类别（表 / 视图 / schema / 列 / 其余）、
    /// 名称，末位用 `id` 兜底（同名同长的列很多，如各表的 `id`：没有这层兜底，
    /// 同分行的相对次序由扫描路径决定，按键重搜时结果会跳）。
    /// `limit` 是硬上限（搜索是交互操作，不应被超大 schema 拖死）；非正数按空结果处理。
    ///
    /// **两段式（优化）**：
    /// - 前缀段：按类别各扫一段索引（等值连接 + 范围名序，输出即索引序、取够 `limit` 即停）；
    /// - 回落段：前缀没装满 `limit` 才跑，同样按类别扫（`LIKE` 当过滤谓词、排序仍靠索引），
    ///   补齐剩余名额。两段合并后在 Rust 侧按「档位 → 类别」稳定排序——**全程无 SQL 排序、
    /// 无窗口**，前缀命中多少都不影响耗时。
    ///
    /// 排序键里**没有 `LENGTH(object_name)`**（曾经的「短名优先」）：保留它则前缀段必须
    /// 把全部命中取回来排序，要么慢、要么靠名序窗口取近似（实测 8 万命中时前者 178 ms、
    /// 后者有尾部偏差）。拿掉后排序退化为「相等 / 类别 / 名序」，恰好能吃索引；
    /// 对「以同一前缀开头」的名字集，名序在绝大多数情形下就是短名在前（前缀本身最短）。
    pub fn search_index(
        &self,
        connection_id: &str,
        needle: &str,
        limit: i64,
    ) -> Result<Vec<IndexSearchHit>, CoreError> {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let escaped = Self::like_escape(&needle);
        // 上界 = needle || U+10FFFF（`char::MAX`，合法 UTF-8 的最大字符）
        let upper = format!("{needle}{}", char::MAX);

        // 前缀段：每个类别各取「名序前 limit 条」（走索引、取够即停）。
        // 每类取满 limit 条就够：排序是「档位优先、档内按类别」，任何一类最多也就能占满整页。
        let mut hits = Vec::new();
        for category in Self::SEARCH_INDEX_CATEGORY_PREDICATES {
            let sql = Self::search_index_category_sql(Self::SQL_SEARCH_INDEX_PREFIX_TEMPLATE, category);
            hits.extend(self.query_index_hits(
                &sql,
                &[&connection_id, &needle, &upper, &limit],
            )?);
        }
        if hits.len() as i64 >= limit {
            // 前缀段已装满一页：中缀命中档位更低，不可能进榜，直接收尾
            Self::sort_and_truncate(&mut hits, &needle, &upper, limit);
            return Ok(hits);
        }

        // 中缀回落段：只补剩余名额（此时前缀命中已全部到手，且都排在前面）
        let contains = format!("%{escaped}%");
        let rest = limit - hits.len() as i64;
        for category in Self::SEARCH_INDEX_CATEGORY_PREDICATES {
            let sql = Self::search_index_category_sql(Self::SQL_SEARCH_INDEX_FALLBACK_TEMPLATE, category);
            hits.extend(self.query_index_hits(
                &sql,
                &[&connection_id, &contains, &needle, &upper, &rest],
            )?);
        }
        Self::sort_and_truncate(&mut hits, &needle, &upper, limit);
        Ok(hits)
    }

    /// 两段结果的合并收尾：按「档位（0 相等 / 1 前缀 / 2 中缀）→ 类别」排序，取前 `limit` 条。
    ///
    /// `sort_by_key` 是**稳定排序**：同键行保留查询时的次序（类别循环序 + 类内索引名序），
    /// 所以不需要把 `id` / `LENGTH` / 名称拿进排序键——这也是能去掉窗口、做到精确的原因。
    fn sort_and_truncate(hits: &mut Vec<IndexSearchHit>, needle: &str, upper: &str, limit: i64) {
        hits.sort_by_key(|h| {
            (
                Self::search_index_rank(&h.object_name, needle, upper),
                Self::search_index_category_rank(&h.object_type),
            )
        });
        hits.truncate(limit as usize);
    }

    // ==================== V6: 预热核心逻辑 ====================

    /// 入队后台同步任务（批量）
    pub fn enqueue_indexing_tasks(
        &mut self,
        connection_id: &str,
        tasks: Vec<(String, String, String)>, // (task_type, object_name, parent_name)
    ) -> Result<usize, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "begin_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        let mut count = 0;
        for (task_type, object_name, parent_name) in tasks {
            tx.execute(
                "INSERT INTO sync_tasks (connection_id, task_type, object_name, parent_name, priority, created_at)
                 VALUES (?1, ?2, ?3, ?4, 5, ?5)",
                rusqlite::params![connection_id, task_type, object_name, parent_name, now],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "enqueue_indexing_task".to_string(),
                    reason: e.to_string(),
                }
            ))?;
            count += 1;
        }

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "commit_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(count)
    }
}

/// V6: 索引构建结果
#[derive(Debug, Clone)]
pub struct IndexBuildResult {
    pub schema_count: usize,
    pub table_count: usize,
    pub column_count: usize,
    pub total_entries: usize,
}

/// FTS 搜索结果
#[derive(Debug, Clone)]
pub struct FtsSearchResult {
    pub search_type: String,
    pub schema_name: String,
    pub object_name: String,
    pub parent_name: String,
    pub snippet: String,
}

/// V6: 索引表条目输入
#[derive(Debug, Clone)]
pub struct IndexEntryInput {
    pub connection_id: String,
    pub schema_id: Option<i64>,
    pub object_type: String,
    pub object_name: String,
    pub parent_name: Option<String>,
    pub path: String,
    pub introspect_level: i32,
    pub row_count_estimate: Option<i64>,
    pub sort_weight: Option<i32>,
    pub last_sync: Option<i64>,
}

/// 触发器缓存行（读侧视图：`triggers` JOIN `tables`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerInfo {
    pub name: String,
    /// 所属表（`triggers.table_id` 指向的表名）
    pub table_name: String,
    pub comment: Option<String>,
}

/// V6: 索引表条目
#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub id: i64,
    pub schema_id: Option<i64>,
    pub object_type: String,
    pub object_name: String,
    pub parent_name: Option<String>,
    pub path: String,
    pub introspect_level: i32,
    pub is_loaded: bool,
    pub last_sync: Option<i64>,
    pub row_count_estimate: Option<i64>,
    pub sort_weight: Option<i32>,
}

impl IndexEntry {
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            schema_id: row.get(1)?,
            object_type: row.get(2)?,
            object_name: row.get(3)?,
            parent_name: row.get(4)?,
            path: row.get(5)?,
            introspect_level: row.get(6)?,
            is_loaded: row.get::<_, i32>(7)? != 0,
            last_sync: row.get(8)?,
            row_count_estimate: row.get(9)?,
            sort_weight: row.get(10)?,
        })
    }
}

/// 索引名称搜索的命中行（已联结 `schemata`，带定位所需的 catalog / schema）。
///
/// 与 [`IndexEntry`] 的区别：搜索是“给人看的”，故带上 schema 名与 catalog 名
/// （索引表本身只存 `schema_id`），这样视图侧不必再为一行的定位多查一次。
#[derive(Debug, Clone, PartialEq)]
pub struct IndexSearchHit {
    /// 类别：`table` / `view` / `schema` / `column`（取值受建表 CHECK 约束）。
    pub object_type: String,
    /// 对象名。
    pub object_name: String,
    /// 列命中时的所属表名；其余为 `None`。
    pub parent_name: Option<String>,
    /// 所属 catalog（`schemata.catalog_name`，可能为 NULL）。
    pub catalog_name: Option<String>,
    /// 所属 schema 名（`schema_id` 联结不到行时为 `None`）。
    pub schema_name: Option<String>,
    /// 行数估算（只有表上有值）。
    pub row_count_estimate: Option<i64>,
}

/// V6: 分页索引结果
#[derive(Debug, Clone)]
pub struct PaginatedIndexResult {
    pub entries: Vec<IndexEntry>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

/// V6: Schema 对象数量统计
#[derive(Debug, Clone)]
pub struct SchemaObjectCounts {
    pub table_count: usize,
    pub view_count: usize,
    pub column_count: usize,
    pub routine_count: usize,
    pub total: usize,
}

/// V6: 分块读取结果
#[derive(Debug, Clone)]
pub struct ChunkResult<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

/// 缓存统计信息
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub table_count: usize,
    pub column_count: usize,
    pub last_sync: Option<i64>,
}

// ===========================================================================
// V7: 增量同步支持
// ===========================================================================

/// V7: 变更检测结果
#[derive(Debug, Clone)]
pub struct ChangeDetectionResult {
    pub connection_id: String,
    pub create_count: usize,
    pub update_count: usize,
    pub delete_count: usize,
    pub no_change_count: usize,
    pub total: usize,
    pub detected_at: i64,
}

/// V7: 同步操作
#[derive(Debug, Clone)]
pub struct SyncOperation {
    pub id: Option<i64>,
    pub connection_id: String,
    pub operation_type: String, // create/update/delete/no_change
    pub object_type: String,
    pub object_name: String,
    pub parent_name: Option<String>,
    pub old_hash: Option<String>,
    pub new_hash: Option<String>,
    pub detected_at: i64,
    pub processed_at: Option<i64>,
    pub status: String,
    pub priority: i32,
    pub error_message: Option<String>,
}

/// V7: 快照类型
#[derive(Debug, Clone)]
pub struct SyncSnapshot {
    pub id: Option<i64>,
    pub connection_id: String,
    pub snapshot_type: String, // schema/table/column/index/view/routine/full
    pub object_type: String,
    pub object_name: String,
    pub parent_name: Option<String>,
    pub object_hash: Option<String>,
    pub snapshot_at: i64,
}

/// Schema 信息
#[derive(Debug, Clone)]
pub struct SchemaInfo {
    pub id: i64,
    pub catalog_name: Option<String>,
    pub schema_name: String,
    pub owner: Option<String>,
    pub comment: Option<String>,
    pub last_sync: Option<i64>,
    pub default_character_set_name: Option<String>,
    pub default_collation_name: Option<String>,
    pub introspect_level: Option<i32>,
    pub is_loaded: Option<i32>,
    /// V10: 企业级统计
    pub total_tables: Option<i32>,
    pub total_views: Option<i32>,
    pub total_procedures: Option<i32>,
    pub total_functions: Option<i32>,
    pub total_size_bytes: Option<i64>,
    pub row_count_total: Option<i64>,
}

impl SchemaInfo {
    /// 从 schemata 表读取基本字段（兼容旧查询）
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            catalog_name: row.get(1)?,
            schema_name: row.get(2)?,
            owner: row.get(3)?,
            comment: row.get(4)?,
            last_sync: row.get(5)?,
            default_character_set_name: row.get(6)?,
            default_collation_name: row.get(7)?,
            introspect_level: row.get(8)?,
            is_loaded: row.get(9)?,
            total_tables: None,
            total_views: None,
            total_procedures: None,
            total_functions: None,
            total_size_bytes: None,
            row_count_total: None,
        })
    }

    /// 从 schemata 表读取完整字段（含 V10 统计）
    pub fn from_row_with_stats(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            catalog_name: row.get(1)?,
            schema_name: row.get(2)?,
            owner: row.get(3)?,
            comment: row.get(4)?,
            last_sync: row.get(5)?,
            default_character_set_name: row.get(6)?,
            default_collation_name: row.get(7)?,
            introspect_level: row.get(8)?,
            is_loaded: row.get(9)?,
            total_tables: row.get(10)?,
            total_views: row.get(11)?,
            total_procedures: row.get(12)?,
            total_functions: row.get(13)?,
            total_size_bytes: row.get(14)?,
            row_count_total: row.get(15)?,
        })
    }
}

/// 表详情信息（规范化）
#[derive(Debug, Clone)]
pub struct TableDetailInfo {
    pub id: i64,
    pub table_name: String,
    pub table_type: String,
    pub table_comment: Option<String>,
    pub engine: Option<String>,
    pub row_count_estimate: Option<i64>,
    pub created_at: Option<i64>,
    pub last_altered_at: Option<i64>,
    pub last_sync: Option<i64>,
    pub schema_name: String,
    /// V10: 存储空间
    pub data_length: Option<i64>,
    pub index_length: Option<i64>,
    /// V10: 显示控制
    pub display_order: Option<i32>,
    pub hidden: Option<bool>,
    pub favorite: Option<bool>,
    pub color_label: Option<String>,
    pub user_comment: Option<String>,
}

impl TableDetailInfo {
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            table_name: row.get(1)?,
            table_type: row.get(2)?,
            table_comment: row.get(3)?,
            engine: row.get(4)?,
            row_count_estimate: row.get(5)?,
            created_at: row.get(6)?,
            last_altered_at: row.get(7)?,
            last_sync: row.get(8)?,
            schema_name: row.get(9)?,
            data_length: row.get(10)?,
            index_length: row.get(11)?,
            display_order: row.get(12)?,
            hidden: row.get::<_, Option<i32>>(13)?.map(|v| v != 0),
            favorite: row.get::<_, Option<i32>>(14)?.map(|v| v != 0),
            color_label: row.get(15)?,
            user_comment: row.get(16)?,
        })
    }
}

/// 列详情信息（规范化）
#[derive(Debug, Clone)]
pub struct ColumnDetailInfo {
    pub id: i64,
    pub column_name: String,
    pub ordinal_position: i32,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_identity: bool,
    pub is_primary_key: bool,
    pub is_foreign_key: bool,
    pub column_default: Option<String>,
    pub column_comment: Option<String>,
    pub character_maximum_length: Option<i32>,
    pub numeric_precision: Option<i32>,
    pub numeric_scale: Option<i32>,
    pub character_set_name: Option<String>,
    pub collation_name: Option<String>,
    pub is_generated: bool,
    /// 扩展属性（JSON 格式，来自 columns.extra 列）
    pub extra: std::collections::HashMap<String, String>,
}

impl ColumnDetailInfo {
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let extra_json: Option<String> = row.get(16)?;
        let extra = if let Some(ref json_str) = extra_json {
            serde_json::from_str(json_str).unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };

        Ok(Self {
            id: row.get(0)?,
            column_name: row.get(1)?,
            ordinal_position: row.get(2)?,
            data_type: row.get(3)?,
            is_nullable: row.get::<_, i32>(4)? != 0,
            is_identity: row.get::<_, i32>(5)? != 0,
            is_primary_key: row.get::<_, i32>(6)? != 0,
            is_foreign_key: row.get::<_, i32>(7)? != 0,
            column_default: row.get(8)?,
            column_comment: row.get(9)?,
            character_maximum_length: row.get(10)?,
            numeric_precision: row.get(11)?,
            numeric_scale: row.get(12)?,
            character_set_name: row.get(13)?,
            collation_name: row.get(14)?,
            is_generated: row.get::<_, i32>(15)? != 0,
            extra,
        })
    }
}

/// 索引详情信息
#[derive(Debug, Clone)]
pub struct IndexDetailInfo {
    pub id: i64,
    pub index_name: String,
    pub index_type: Option<String>,
    pub is_unique: bool,
    pub is_primary: bool,
    pub index_comment: Option<String>,
    pub columns: Vec<IndexColumnInfo>,
}

/// 索引列信息
#[derive(Debug, Clone)]
pub struct IndexColumnInfo {
    pub id: i64,
    pub column_name: String,
    pub ordinal_position: i32,
    pub sort_order: Option<String>,
    pub is_included_column: bool,
}

impl IndexColumnInfo {
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            column_name: row.get(1)?,
            ordinal_position: row.get(2)?,
            sort_order: row.get(3)?,
            is_included_column: row.get::<_, i32>(4)? != 0,
        })
    }
}

/// 外键约束详情信息
#[derive(Debug, Clone)]
pub struct ForeignKeyDetailInfo {
    pub id: i64,
    pub constraint_name: String,
    pub delete_rule: Option<String>,
    pub update_rule: Option<String>,
    pub ref_schema_id: Option<i64>,
    pub ref_table_id: Option<i64>,
    pub columns: Vec<ForeignKeyColumnInfo>,
}

/// 外键列映射信息
#[derive(Debug, Clone)]
pub struct ForeignKeyColumnInfo {
    pub id: i64,
    pub ordinal_position: i32,
    pub column_name: String,
    pub ref_column_name: String,
}

impl ForeignKeyColumnInfo {
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            ordinal_position: row.get(1)?,
            column_name: row.get(2)?,
            ref_column_name: row.get(3)?,
        })
    }
}

/// 视图详情信息
#[derive(Debug, Clone)]
pub struct ViewDetailInfo {
    pub id: i64,
    pub table_name: String,
    pub table_comment: Option<String>,
    pub view_definition: Option<String>,
    pub is_updatable: Option<bool>,
    pub check_option: Option<String>,
}

impl ViewDetailInfo {
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            table_name: row.get(1)?,
            table_comment: row.get(2)?,
            view_definition: row.get(3)?,
            is_updatable: row.get::<_, Option<i32>>(4)?.map(|v| v != 0),
            check_option: row.get(5)?,
        })
    }
}

/// Routine 详情信息
#[derive(Debug, Clone)]
pub struct RoutineDetailInfo {
    pub id: i64,
    pub routine_name: String,
    pub routine_type: String,
    pub data_type: Option<String>,
    pub routine_definition: Option<String>,
    pub external_language: Option<String>,
    pub is_deterministic: Option<bool>,
    pub routine_comment: Option<String>,
    pub parameters: Vec<RoutineParameterInfo>,
}

/// Routine 参数信息
#[derive(Debug, Clone)]
pub struct RoutineParameterInfo {
    pub id: i64,
    pub parameter_name: String,
    pub ordinal_position: i32,
    pub parameter_mode: Option<String>,
    pub data_type: Option<String>,
    pub parameter_default: Option<String>,
}

impl RoutineParameterInfo {
    pub fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            parameter_name: row.get(1)?,
            ordinal_position: row.get(2)?,
            parameter_mode: row.get(3)?,
            data_type: row.get(4)?,
            parameter_default: row.get(5)?,
        })
    }
}

/// FTS 写入失败的统一错误（`store` / `operation` 口径与文件内其它算子一致）。
fn fts_err(operation: &str, e: rusqlite::Error) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "sqlite".to_string(),
        operation: operation.to_string(),
        reason: e.to_string(),
    })
}

/// 用户输入 → FTS5 `MATCH` 查询串（`None` = 没有可用词，调用方直接返回空结果）。
///
/// 语义：按「非字母数字 / 非下划线」拆词 → 逐词加双引号 → 末词附 `*` 做前缀匹配。
///
/// **为什么不整串加引号**：整串加引号会变成一个短语，敲 `order` 就再也命不中
/// `order_items`；拆开后每词是独立短语（隐式 AND），末词前缀又能让「敲一半」也命中。
///
/// 安全：双引号本身是**分隔符**（`a"b` 拆成 `a` 与 `b`），因此词里不可能再带引号；
/// 剩下 `*` `(` `NEAR` `-` 等全部被引号包住、按字面处理，不再有语法注入面。
fn fts_match_query(raw: &str) -> Option<String> {
    let tokens: Vec<&str> = raw
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let mut out = String::new();
    let last = tokens.len() - 1;
    for (ix, token) in tokens.iter().enumerate() {
        if ix > 0 {
            out.push(' ');
        }
        out.push('"');
        out.push_str(token);
        out.push('"');
        if ix == last {
            // 前缀：`"abc"*`（末词敲一半也能命中）
            out.push('*');
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rdata_test_metadata_cache_{}", name));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_metadata_cache_manager_global() {
        let conn_id = "test_mysql_001";

        let manager = MetadataCacheManager::new(conn_id, ConnectionType::Global, None).unwrap();
        assert!(manager
            .db_path()
            .to_string_lossy()
            .contains("global_metadata"));
        assert!(manager.db_path().to_string_lossy().contains(conn_id));
    }

    #[test]
    fn test_metadata_cache_manager_project() -> Result<(), CoreError> {
        let project_path = test_temp_dir("project")
            .to_str()
            .ok_or_else(|| CoreError::common(CommonError::General("Invalid path".to_string())))?
            .to_string();
        let conn_id = "test_pg_001";

        let manager =
            MetadataCacheManager::new(conn_id, ConnectionType::Project, Some(&project_path))?;
        assert!(manager
            .db_path()
            .to_string_lossy()
            .contains("meta/connection_metadata"));
        assert!(manager.db_path().to_string_lossy().contains(conn_id));
        Ok(())
    }

    /// 建一个跑过迁移、且 `PRAGMA foreign_keys=ON` 的缓存库（项目型 → 文件落在临时目录）。
    /// 返回 (操作句柄, 库文件路径, 临时根)。
    fn fresh_ops(tag: &str) -> (MetadataCacheOps, PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("rdata_test_mdc_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");

        let project = dir.to_string_lossy().to_string();
        let manager =
            MetadataCacheManager::new("test_conn_mdc", ConnectionType::Project, Some(&project))
                .expect("构造缓存管理器");
        let db_path = manager.db_path().clone();
        let conn = manager.open().expect("打开缓存库（含迁移与 PRAGMA）");
        (MetadataCacheOps::new(conn), db_path, dir)
    }

    /// 回归：`delete_schema` 必须真正删掉表 / 视图 / 列。
    ///
    /// 历史缺陷：函数里有 `DELETE FROM views`，而规范化模型没有 `views` 表（视图是
    /// `tables` 里 `table_type='VIEW'` 的行）——语句报 `no such table: views`、事务回滚；
    /// 调用方 `NavCache::prune_schema` 又用 `let _ =` 吞错，于是「刷新元数据」静默失效。
    #[test]
    fn delete_schema_clears_tables_views_and_children() -> Result<(), CoreError> {
        let (mut ops, db_path, dir) = fresh_ops("delete_schema");

        let schema_id = ops.save_schema("main", "public", None, None)?;
        let table_id = ops.save_table(schema_id, "t1", "TABLE", Some("表"), None, None)?;
        ops.save_column(table_id, "id", "INTEGER", 0, false, true, false, None, None)?;
        let view_id = ops.save_table(schema_id, "v1", "VIEW", None, None, None)?;
        ops.save_view(view_id, "SELECT 1", None, None)?;

        // 前置：视图定义确实落库（`table_id` 是 NOT NULL，历史实现漏写该列）
        let views: i64 = ops
            .get_connection()
            .query_row("SELECT COUNT(*) FROM view_definitions", [], |r| r.get(0))?;
        assert_eq!(views, 1, "save_view 应写入 view_definitions");

        let deleted = ops.delete_schema(schema_id)?;
        assert!(deleted >= 2, "至少删掉 schema + 表/视图，实际 {deleted}");
        assert!(
            ops.list_tables_normalized(schema_id, None)?.is_empty(),
            "表/视图应被清空"
        );
        assert!(ops.get_schema_id("main", "public")?.is_none(), "schema 行应删除");

        // 级联：子表不得留孤儿行（依赖 PRAGMA foreign_keys=ON + ON DELETE CASCADE）
        drop(ops);
        let raw = Connection::open(&db_path)?;
        for (table, column) in [
            ("columns", "table_id"),
            ("view_definitions", "table_id"),
            ("indexes", "table_id"),
        ] {
            let orphans: i64 = raw.query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
                rusqlite::params![table_id],
                |r| r.get(0),
            )?;
            assert_eq!(orphans, 0, "{table} 应无孤儿行（级联删除）");
        }

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// `columns.is_identity`（自增）与 `columns.is_primary`（主键）必须各自独立写入。
    ///
    /// 迁移 008 专门为此拆列（抬头写着「之前 is_primary 被映射到 is_identity 列，
    /// 但主键 ≠ 自增列」），但写入侧一直把同一个值塞进两列——本测试锁死这个语义。
    #[test]
    fn save_column_keeps_identity_and_primary_independent() -> Result<(), CoreError> {
        let (ops, _db_path, dir) = fresh_ops("column_flags");
        let schema_id = ops.save_schema("main", "public", None, None)?;
        let table_id = ops.save_table(schema_id, "t1", "TABLE", None, None, None)?;

        // 自增主键：两列都为真
        ops.save_column(table_id, "id", "INTEGER", 0, false, true, true, None, None)?;
        // 普通主键（如 uuid）：是主键但**不是**自增
        ops.save_column(table_id, "code", "TEXT", 1, false, true, false, None, None)?;

        let cols = ops.list_columns_normalized(table_id)?;
        let id = cols.iter().find(|c| c.column_name == "id").expect("id 列");
        assert!(id.is_primary_key && id.is_identity, "id 应同时是主键与自增");

        let code = cols
            .iter()
            .find(|c| c.column_name == "code")
            .expect("code 列");
        assert!(code.is_primary_key, "code 应是主键");
        assert!(!code.is_identity, "code 不是自增列（旧实现会误报为自增）");

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 索引名称搜索：中缀命中、通配符按字面量、排序（相等 → 前缀 → 其余，同类再按名称）。
    #[test]
    fn search_index_matches_infix_and_escapes_wildcards() -> Result<(), CoreError> {
        let (mut ops, _db_path, dir) = fresh_ops("index_search");
        let conn_id = "P_search_conn";
        let schema_id = ops.save_schema("main", "public", None, None)?;
        for name in ["order", "orders", "order_items", "customer_orders", "a%b"] {
            ops.save_table(schema_id, name, "TABLE", None, None, None)?;
        }
        let v1 = ops.save_table(schema_id, "order_view", "VIEW", None, None, None)?;
        ops.save_view(v1, "SELECT 1", None, None)?;
        // 列名含 order：搜索不应漏掉列（列排在表 / 视图之后）
        let t = ops.get_table_id(schema_id, "orders")?.expect("orders 表");
        ops.save_column(t, "order_id", "INTEGER", 0, false, true, false, None, None)?;
        ops.rebuild_schema_index(conn_id, "main", "public")?;

        let hits = ops.search_index(conn_id, "order", 50)?;
        let names: Vec<&str> = hits.iter().map(|h| h.object_name.as_str()).collect();
        assert_eq!(
            names.first().copied(),
            Some("order"),
            "名称完全相等的排最前（得到：{names:?}）"
        );
        // 前缀命中整体优先于中缀命中（`customer_orders` 只含子串，排尾）
        let pos = |n: &str| names.iter().position(|x| x == &n).expect("应在结果里");
        assert!(
            pos("orders") < pos("customer_orders"),
            "前缀命中应整体排在子串命中之前（得到：{names:?}）"
        );
        for want in ["order_items", "customer_orders", "order_view", "order_id"] {
            assert!(names.contains(&want), "命中不得漏 {want}（得到：{names:?}）");
        }
        // 表在前、列在后（类别权重）
        assert!(
            pos("order_items") < pos("order_id"),
            "表应排在列之前（得到：{names:?}）"
        );
        // 命中行带定位信息：表 → schema / catalog
        let table_hit = hits.iter().find(|h| h.object_name == "orders").expect("表命中");
        assert_eq!(table_hit.object_type, "table");
        assert_eq!(table_hit.schema_name.as_deref(), Some("public"));
        assert_eq!(table_hit.catalog_name.as_deref(), Some("main"));
        assert_eq!(table_hit.parent_name, None);
        // 列命中带所属表
        let col_hit = hits.iter().find(|h| h.object_name == "order_id").expect("列命中");
        assert_eq!(col_hit.object_type, "column");
        assert_eq!(col_hit.parent_name.as_deref(), Some("orders"));

        // 通配符按字面量：`a%b` 不得匹配 `aXXXb`（而真表名 a%b 必须命中）
        let literal = ops.search_index(conn_id, "a%b", 50)?;
        assert_eq!(
            literal.iter().map(|h| h.object_name.as_str()).collect::<Vec<_>>(),
            vec!["a%b"],
            "% 应被转义为字面量"
        );
        let underscore = ops.search_index(conn_id, "a_", 50)?;
        assert!(underscore.is_empty(), "`_` 是单字符通配符，必须转义");

        // 大小写不敏感 + 空词不搜
        assert!(!ops.search_index(conn_id, "ORDERS", 50)?.is_empty());
        assert!(ops.search_index(conn_id, "   ", 50)?.is_empty());

        // limit 是硬上限
        assert_eq!(ops.search_index(conn_id, "order", 2)?.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 优化参照物：优化前那条「单条中缀 LIKE」SQL（**只用来比对**，不要照它改生产代码）。
    ///
    /// 与两段式实现的差异是「不切分」；排序口径两边一致（相等 → 类别 → 名称 → id），
    /// 同分行的相对次序才不会因为切分而漂移（前缀段靠索引序，同名字按行号，与 `id` 同序）。
    fn oracle_search_index(
        ops: &MetadataCacheOps,
        connection_id: &str,
        needle: &str,
        limit: i64,
    ) -> Result<Vec<IndexSearchHit>, CoreError> {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let escaped = MetadataCacheOps::like_escape(&needle);
        let contains = format!("%{escaped}%");
        let prefix = format!("{escaped}%");

        let mut stmt = ops
            .get_connection()
            .prepare(
                "SELECT mi.object_type, mi.object_name, mi.parent_name,
                        s.catalog_name, s.schema_name, mi.row_count_estimate
                 FROM metadata_index mi
                 LEFT JOIN schemata s ON s.id = mi.schema_id
                 WHERE mi.connection_id = ?1 AND LOWER(mi.object_name) LIKE ?2 ESCAPE '\\'
                 ORDER BY
                   CASE WHEN LOWER(mi.object_name) = ?3 THEN 0
                        WHEN LOWER(mi.object_name) LIKE ?4 ESCAPE '\\' THEN 1
                        ELSE 2 END,
                   CASE mi.object_type
                        WHEN 'table' THEN 0 WHEN 'view' THEN 1 WHEN 'schema' THEN 2
                        WHEN 'column' THEN 3 ELSE 4 END,
                   mi.object_name, mi.id
                 LIMIT ?5",
            )?;
        let rows = stmt.query_map(
            rusqlite::params![connection_id, contains, needle, prefix, limit],
            |row| {
                Ok(IndexSearchHit {
                    object_type: row.get(0)?,
                    object_name: row.get(1)?,
                    parent_name: row.get(2)?,
                    catalog_name: row.get(3)?,
                    schema_name: row.get(4)?,
                    row_count_estimate: row.get(5)?,
                })
            },
        )?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 两段式（分类索引序前缀段 + 中缀回落）必须与优化前的单条查询**逐条相同**。
    ///
    /// 现在**没有窗口**：前缀段是「每类取名序前 limit 条」的并集，对任意多命中都精确
    /// （超大数据集另见 `search_index_prefix_stage_is_exact_and_category_first`）。
    ///
    /// 覆盖：前缀装满 `limit`（早退）/ 前缀不足（回落）/ 零前缀 / 转义字符 / 大小写 /
    /// 同名同类同长的列（排序完全同分，靠行号与 `id` 同序兜底）。
    #[test]
    fn search_index_two_stage_matches_single_query_oracle() -> Result<(), CoreError> {
        let (mut ops, _db_path, dir) = fresh_ops("index_two_stage");
        let conn_id = "P_two_stage";
        let public = ops.save_schema("main", "public", None, None)?;
        let archive = ops.save_schema("main", "archive", None, None)?;

        for name in [
            "order",
            "orders",
            "orders_log",
            "orders_archive",
            "order_items",
            "reorder_point",
            "x_orders",
            "order_100%",
        ] {
            ops.save_table(public, name, "TABLE", None, None, None)?;
        }
        // 两列同名同长跨表：排序键上完全同分
        let orders = ops.get_table_id(public, "orders")?.expect("orders 表");
        ops.save_column(orders, "order_id", "INTEGER", 0, false, true, false, None, None)?;
        let items = ops.get_table_id(public, "order_items")?.expect("order_items 表");
        ops.save_column(items, "order_id", "INTEGER", 0, false, true, false, None, None)?;
        // 与表同名但不同类别（视图在另一个 schema，绕过 UNIQUE(schema_id, table_name)）
        let view = ops.save_table(archive, "orders", "VIEW", None, None, None)?;
        ops.save_view(view, "SELECT 1", None, None)?;

        ops.rebuild_schema_index(conn_id, "main", "public")?;
        ops.rebuild_schema_index(conn_id, "main", "archive")?;

        for needle in [
            "order",
            "orders",
            "orders_",
            "order_id",
            "order_100%",
            "reorder",
            "ORDERS",
            "zzz_no_hit",
            "   ",
        ] {
            for limit in [1i64, 2, 3, 5, 50] {
                let got = ops.search_index(conn_id, needle, limit)?;
                let want = oracle_search_index(&ops, conn_id, needle, limit)?;
                assert_eq!(
                    got, want,
                    "needle={needle:?} limit={limit}：两段式必须与单条查询逐条相同"
                );
            }
        }

        // 两条分支都真的被走到：前缀装满 → 早退；零前缀 → 全靠回落段
        assert_eq!(ops.search_index(conn_id, "order", 2)?.len(), 2, "前缀段应填满 limit");
        assert_eq!(
            ops.search_index(conn_id, "reorder", 5)?.len(),
            1,
            "无前缀命中时全靠回落段"
        );

        // 前缀命中整体优先于中缀命中（两段式正确性的前提）
        let names: Vec<String> = ops
            .search_index(conn_id, "order", 50)?
            .into_iter()
            .map(|h| h.object_name)
            .collect();
        let pos = |n: &str| {
            names
                .iter()
                .position(|x| x == n)
                .unwrap_or_else(|| panic!("结果里应有 {n}（得到：{names:?}）"))
        };
        assert!(pos("orders_archive") < pos("reorder_point"), "前缀应在前：{names:?}");
        assert!(pos("order_id") < pos("reorder_point"), "前缀应在前：{names:?}");

        // limit 非正数按空结果（而不是 SQLite 的「-1 = 不限」）
        assert!(ops.search_index(conn_id, "order", 0)?.is_empty());
        assert!(ops.search_index(conn_id, "order", -1)?.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 两段的 SQL 都必须真的走 `idx_metadata_index_conn_type_object_lower`，且**不排序**。
    ///
    /// 用 `EXPLAIN QUERY PLAN` 钉住三件事：① 用本索引而非 `idx_metadata_index_level`；
    /// ② 等值条件参与索引定位（`connection_id=? AND object_type=?`）；
    /// ③ 不出现 `TEMP B-TREE`——输出序就是索引序，这是「无需窗口、无需排序」的全部依据
    ///   （中缀段同样靠它：`LIKE` 只作为过滤谓词，排序仍由索引提供）。
    /// 量级问题（10 万对象全表扫百毫秒级）只靠用例测不出来，但计划能断言：
    /// 初版单列表达式索引就被规划器弃用过（改用 `connection_id` 等值扫），改成复合索引才生效。
    #[test]
    fn search_index_stage_sqls_use_expression_index() -> Result<(), CoreError> {
        let (ops, _db_path, dir) = fresh_ops("index_stage_plan");
        let conn = ops.get_connection();
        let category = MetadataCacheOps::SEARCH_INDEX_CATEGORY_PREDICATES[0];
        let upper = format!("ord{}", char::MAX);

        let check = |stage: &str, sql: String, params: &[&dyn rusqlite::ToSql]| -> Result<(), CoreError> {
            let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
            let rows = stmt.query_map(params, |row| row.get::<_, String>(3))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            let plan = out.join("\n");
            println!("[计划] {stage}: {plan}");

            assert!(
                plan.contains("idx_metadata_index_conn_type_object_lower"),
                "{stage}应走复合表达式索引（迁移 013）。实际计划：\n{plan}"
            );
            assert!(
                plan.contains("SEARCH"),
                "{stage}应用索引定位（SEARCH）而非全表扫（SCAN）。实际计划：\n{plan}"
            );
            assert!(
                plan.contains("connection_id=?") && plan.contains("object_type=?"),
                "{stage}的等值条件必须参与索引定位。实际计划：\n{plan}"
            );
            assert!(
                !plan.contains("TEMP B-TREE"),
                "{stage}的输出序应即索引序（不允许再排序）。实际计划：\n{plan}"
            );
            Ok(())
        };

        check(
            "前缀段",
            MetadataCacheOps::search_index_category_sql(
                MetadataCacheOps::SQL_SEARCH_INDEX_PREFIX_TEMPLATE,
                category,
            ),
            &[&"P_plan", &"ord", &upper, &10i64],
        )?;
        check(
            "回落段",
            MetadataCacheOps::search_index_category_sql(
                MetadataCacheOps::SQL_SEARCH_INDEX_FALLBACK_TEMPLATE,
                category,
            ),
            &[&"P_plan", &"%ord%", &"ord", &upper, &10i64],
        )?;

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 造一条索引行（索引类用例共用；`schema_id` 有值 → 命中行能带上 schema / catalog）。
    fn index_entry(
        connection_id: &str,
        schema_id: i64,
        object_type: &str,
        name: &str,
        parent_name: Option<&str>,
    ) -> IndexEntryInput {
        IndexEntryInput {
            connection_id: connection_id.to_string(),
            schema_id: Some(schema_id),
            object_type: object_type.to_string(),
            object_name: name.to_string(),
            parent_name: parent_name.map(str::to_string),
            path: match parent_name {
                Some(parent) => format!("public/{parent}/{name}"),
                None => format!("public/{name}"),
            },
            introspect_level: 3,
            row_count_estimate: None,
            sort_weight: None,
            last_sync: Some(0),
        }
    }

    /// 前缀段的两条硬口径：**① 精确（无窗口、无截断）；② 先类别、后名序**。
    ///
    /// 为什么值得单独钉：这两条是「拿掉 `LENGTH` 排序键 + 分类索引序直出」换来的，
    /// 也正是它把上一版「名序窗口」的近似消掉（当时靠窗口 2000 换响应时间，命中超出就有尾部偏差）。
    #[test]
    fn search_index_prefix_stage_is_exact_and_category_first() -> Result<(), CoreError> {
        let (mut ops, _db_path, dir) = fresh_ops("index_exact");
        let conn_id = "P_exact";
        let sid = ops.save_schema("main", "public", None, None)?;

        // 大量前缀命中（远超旧窗口 2000）＋ 两个名字序/类别上的「刺」：
        //   `win_zz`：名序极靠后（字母在数字之后）——若还有窗口就会被挡在外面；
        //   `win_0000` 列：名序比所有 `win_0000x` 表都靠前（是它们的前缀），但类别上是列；
        //   `win` 列：完全相等，应跨类别排第一。
        let tables = 2_000usize;
        let mut entries: Vec<IndexEntryInput> = (0..tables)
            .map(|i| index_entry(conn_id, sid, "table", &format!("win_{i:05}"), None))
            .collect();
        entries.push(index_entry(conn_id, sid, "table", "win_zz", None));
        entries.push(index_entry(conn_id, sid, "column", "win_0000", Some("win_00000")));
        entries.push(index_entry(conn_id, sid, "column", "win", Some("win_00001")));
        ops.save_index_entries_batch(entries)?;

        // ① 精确：限足不截断时，名序靠后的 `win_zz` 与列 `win_0000` 都在（旧窗口会把它们挡在外面）
        let all = ops.search_index(conn_id, "win", 5_000)?;
        let names: Vec<&str> = all.iter().map(|h| h.object_name.as_str()).collect();
        assert_eq!(names.len(), tables + 3, "应取回全部命中：{names:?}");
        assert!(names.contains(&"win_zz"), "名序靠后的短名不得丢：{names:?}");
        assert!(names.contains(&"win_0000"), "列命中不得丢：{names:?}");
        // 与「一次全量排序」的参照物逐条相同（无窗口 → 任何限制下都应相同）
        for limit in [1i64, 5, 50, 5_000] {
            assert_eq!(
                ops.search_index(conn_id, "win", limit)?,
                oracle_search_index(&ops, conn_id, "win", limit)?,
                "无窗口后任意 limit 都应与全量排序逐条相同（limit={limit}）"
            );
        }

        // ② 先类别：`win` 是列（完全相等 → 跨类别第一）
        let first = ops.search_index(conn_id, "win", 1)?;
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].object_name, "win");
        assert_eq!(first[0].object_type, "column", "完全相等优先于类别权重");

        // ② 先类别：列 `win_0000` 在名序上比所有 `win_0000x` 表都靠前，但它是列 → 表仍先出
        //    （逗号）`win_000` 本身不是任何对象的名字，所以这里无 rank 0 干扰）
        let first = ops.search_index(conn_id, "win_000", 1)?;
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].object_type, "table", "同档内先类别：表先于列");
        assert_eq!(first[0].object_name, "win_00000", "表档内按名序");

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 索引写入侧：`rebuild_schema_index` 让 `metadata_index` 真正有数据，
    /// 分页读（`get_objects_chunk`）与计数（`get_schema_object_counts`）才有意义。
    #[test]
    fn rebuild_schema_index_powers_chunks_and_counts() -> Result<(), CoreError> {
        let (mut ops, _db_path, dir) = fresh_ops("index_rebuild");
        let (catalog, schema) = ("main", "public");
        let conn_id = "P_idx_conn";

        let schema_id = ops.save_schema(catalog, schema, None, None)?;
        let t1 = ops.save_table(schema_id, "t1", "TABLE", None, None, Some(100))?;
        ops.save_column(t1, "id", "INTEGER", 0, false, true, true, None, None)?;
        ops.save_column(t1, "name", "TEXT", 1, true, false, false, None, None)?;
        let v1 = ops.save_table(schema_id, "v1", "VIEW", None, None, None)?;
        ops.save_view(v1, "SELECT 1", None, None)?;

        let written = ops.rebuild_schema_index(conn_id, catalog, schema)?;
        // schema(1) + t1(1) + t1 的列(2) + v1(1) = 5
        assert_eq!(written, 5, "索引行数应与缓存内容一致");

        // 分页读：按类别分别取（表 / 视图各一条）
        let chunk = ops.get_objects_chunk(conn_id, Some(schema_id), "table", 0, 10)?;
        assert_eq!(chunk.total, 1, "索引里应有 1 张表");
        assert_eq!(chunk.items.len(), 1);
        assert_eq!(chunk.items[0].object_name, "t1");
        assert_eq!(chunk.items[0].path, "public/t1");
        assert_eq!(chunk.items[0].row_count_estimate, Some(100));
        assert!(!chunk.has_more, "limit 大于总数时不应再报有余量");

        let views = ops.get_objects_chunk(conn_id, Some(schema_id), "view", 0, 10)?;
        assert_eq!(views.total, 1, "视图也要能分块读（原实现只查 table）");
        assert_eq!(views.items[0].object_name, "v1");

        // 翻页边界：offset 到达 total 时返回空块且无余量
        let empty = ops.get_objects_chunk(conn_id, Some(schema_id), "table", 1, 10)?;
        assert!(empty.items.is_empty());
        assert!(!empty.has_more);

        // 计数：表 1 / 视图 1 / 列 2
        let counts = ops.get_schema_object_counts(conn_id, schema_id)?;
        assert_eq!(
            (counts.table_count, counts.view_count, counts.column_count),
            (1, 1, 2)
        );

        // 幂等：重建两次不得产生重复行（UNIQUE 含 NULL 列，不能靠 OR REPLACE）
        ops.rebuild_schema_index(conn_id, catalog, schema)?;
        let again = ops.get_schema_object_counts(conn_id, schema_id)?;
        assert_eq!(again.total, counts.total, "重建必须幂等");
        assert_eq!(again.column_count, 2);

        // 刷新后对象消失：索引必须跟着少行（先删后插的语义）
        ops.delete_schema(schema_id)?;
        let empty = ops.get_schema_object_counts(conn_id, schema_id)?;
        assert_eq!(empty.total, 0, "级联删除后索引不应留孤儿");

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 定位查询：位次必须与分页读**同口径**。
    ///
    /// 这条查询唯一的用途是「大 schema 从搜索命中直达那一页」——位次差一，
    /// 跳过去的那一页里就没有目标（用户看到「已定位」，行却不在）。
    /// 回归自一个真陷阱：最初用「COUNT 比它小的行」写法，而 `sort_weight` 恒为 NULL，
    /// SQL 里 `NULL > NULL` 不计入 → 位次永远为 0。
    #[test]
    fn object_position_matches_chunk_order() -> Result<(), CoreError> {
        let (mut ops, _db_path, dir) = fresh_ops("object_position");
        let (catalog, schema) = ("main", "public");
        let conn_id = "P_pos_conn";

        let schema_id = ops.save_schema(catalog, schema, None, None)?;
        // 名字刻意乱序插入：顺序若靠插入序，这里的断言会挂
        for name in ["gamma", "alpha", "beta"] {
            ops.save_table(schema_id, name, "TABLE", None, None, None)?;
        }
        // 同名的视图不进表的位次（类别参与限定）——注意 L2 里表与视图同属 `tables` 表、名字唯一，
        // 所以「表与视图同名」在缓存模型里就不存在，这里验证的是**类别过滤**本身。
        let v = ops.save_table(schema_id, "valpha", "VIEW", None, None, None)?;
        ops.save_view(v, "SELECT 1", None, None)?;
        // 另一个 schema 的同名表不进本位次（schema 参与限定）
        let other = ops.save_schema(catalog, "archive", None, None)?;
        ops.save_table(other, "alpha", "TABLE", None, None, None)?;
        ops.rebuild_schema_index(conn_id, catalog, schema)?;
        ops.rebuild_schema_index(conn_id, catalog, "archive")?;

        // 分页读出来的次序就是位次的口径，逐条对齐
        let chunk = ops.get_objects_chunk(conn_id, Some(schema_id), "table", 0, 10)?;
        let names: Vec<&str> = chunk
            .items
            .iter()
            .map(|e| e.object_name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "beta", "gamma"], "同权重时按名字升序");
        for (i, name) in names.iter().enumerate() {
            assert_eq!(
                ops.get_object_position(conn_id, Some(schema_id), "table", name)?,
                Some(i as i64),
                "{name} 的位次应与分页顺序一致"
            );
        }

        // 位次直接喂给 offset/limit，落到的页里必须真有它（这是定位的生死线）
        let target = "gamma";
        let position = ops
            .get_object_position(conn_id, Some(schema_id), "table", target)?
            .expect("gamma 应在索引里") as i64;
        let page = ops.get_objects_chunk(conn_id, Some(schema_id), "table", position, 1)?;
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].object_name, target, "按位次取样必须正好是它");

        // 索引里没有 → None（调用方据此如实告知，而不是假装定位成功）
        assert_eq!(
            ops.get_object_position(conn_id, Some(schema_id), "table", "nope")?,
            None
        );
        // 类别参与限定：表名拿去查视图档 → None（不会串类别给个假位次）
        assert_eq!(
            ops.get_object_position(conn_id, Some(schema_id), "view", "alpha")?,
            None
        );
        assert_eq!(
            ops.get_object_position(conn_id, Some(schema_id), "view", "valpha")?,
            Some(0)
        );
        // schema 参与限定：另一 schema 的 "alpha" 在本 schema 表档里仍只有这一条位次
        assert_eq!(
            ops.get_object_position(conn_id, Some(schema_id), "table", "alpha")?,
            Some(0)
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// FTS 写侧：跟 `rebuild_schema_index` 同批、按 schema 分域、幂等。
    ///
    /// 回归自两个真实缺陷：① `sync_fts_index` 尾部写的是 `FROM views` /
    /// `FROM v_routines`，而规范化模型没有这两张表 → 整条同步永远跑不通（也是它一直零调用的真因）；
    /// ② 旧表 contentless，SELECT 回来的对象身份全是 NULL。
    #[test]
    fn fts_rebuild_is_schema_scoped_and_idempotent() -> Result<(), CoreError> {
        let (mut ops, _db_path, dir) = fresh_ops("fts_sync");
        let public = ops.save_schema("main", "public", None, None)?;
        let other = ops.save_schema("main", "other", None, None)?;
        ops.save_table(
            public,
            "orders",
            "TABLE",
            Some("订单主表，含渠道与优惠信息"),
            None,
            None,
        )?;
        ops.save_table(public, "v_daily", "VIEW", Some("每日汇总视图"), None, None)?;
        let t = ops.get_table_id(public, "orders")?.expect("表 id");
        ops.save_column(
            t,
            "channel_code",
            "VARCHAR",
            0,
            true,
            false,
            false,
            None,
            Some("下单渠道"),
        )?;
        ops.save_table(other, "keep_me", "TABLE", Some("另一个 schema 的表"), None, None)?;

        let fts_count = |ops: &MetadataCacheOps, schema: &str| -> Result<i64, CoreError> {
            ops.conn
                .query_row(
                    "SELECT COUNT(*) FROM metadata_fts WHERE schema_name = ?1",
                    rusqlite::params![schema],
                    |r| r.get(0),
                )
                .map_err(|e| fts_err("count_fts", e))
        };

        ops.rebuild_schema_index("P_conn", "main", "public")?;
        let before = fts_count(&ops, "public")?;
        assert_eq!(before, 4, "schema / 表 / 视图 / 列 各一行");
        assert_eq!(fts_count(&ops, "other")?, 0, "只重建 public，不得顺手写 other");

        // 幂等：先删后插，重复重建不得翻倍
        ops.rebuild_schema_index("P_conn", "main", "public")?;
        assert_eq!(fts_count(&ops, "public")?, before, "重建必须幂等");

        // 级联：删 schema 后 FTS 不留孤儿
        ops.delete_schema(public)?;
        assert_eq!(fts_count(&ops, "public")?, 0, "删 schema 后不得留孤儿");
        assert_eq!(fts_count(&ops, "other")?, 0, "另一个 schema 本来就没索引");

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// FTS 读侧：取回对象身份与 snippet；操作符输入按字面处理不报错；中文 ≥3 字命中注释。
    #[test]
    fn search_fts_returns_identity_snippet_and_survives_operator_input() -> Result<(), CoreError> {
        let (mut ops, _db_path, dir) = fresh_ops("fts_read");
        let sid = ops.save_schema("main", "public", None, None)?;
        ops.save_table(
            sid,
            "orders",
            "TABLE",
            Some("订单主表，含渠道与优惠信息"),
            None,
            None,
        )?;
        let t = ops.get_table_id(sid, "orders")?.expect("表 id");
        ops.save_column(
            t,
            "channel_code",
            "VARCHAR",
            0,
            true,
            false,
            false,
            None,
            Some("下单渠道"),
        )?;
        ops.rebuild_schema_index("P_conn", "main", "public")?;

        // 名称前缀（ASCII）：能取回对象身份（旧 contentless 表这里直接报 Invalid column type Null）
        let hits = ops.search_fts("ord", None)?;
        assert!(
            hits.iter()
                .any(|h| h.object_name == "orders" && h.search_type == "table"),
            "`ord` 应命中 orders：{hits:?}"
        );

        // 中文注释：trigram 下 3 字命中，2 字不足一个 trigram（UI 门槛按 3 字）
        let hit = ops
            .search_fts("含渠道", None)?
            .into_iter()
            .find(|h| h.object_name == "orders")
            .expect("3 字中文子串应命中注释");
        assert!(
            hit.snippet.contains("<mark>"),
            "snippet 应带命中标记：{}",
            hit.snippet
        );
        assert!(
            ops.search_fts("渠道", None)?.is_empty(),
            "2 字不足一个 trigram：应无命中"
        );

        // 操作符输入：清洗后全部按字面处理，不得报错
        for probe in ["\"", "*", "NEAR", "(", "-", "ord OR x", "a\"b"] {
            let _ = ops.search_fts(probe, None)?;
        }

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// 查询词拆解：逐词加引号、末词前缀；纯操作符输入无可用词。
    #[test]
    fn fts_match_query_quotes_tokens_and_prefixes_the_last() {
        assert_eq!(fts_match_query("ord").as_deref(), Some("\"ord\"*"));
        assert_eq!(
            fts_match_query("order items").as_deref(),
            Some("\"order\" \"items\"*")
        );
        assert_eq!(fts_match_query("含渠道").as_deref(), Some("\"含渠道\"*"));
        // 引号是分隔符：`a"b` 拆成两个词，词里不会再带引号
        assert_eq!(fts_match_query("a\"b").as_deref(), Some("\"a\" \"b\"*"));
        // NEAR / OR 只是普通词，不会被当成语法
        assert_eq!(
            fts_match_query("NEAR OR x").as_deref(),
            Some("\"NEAR\" \"OR\" \"x\"*")
        );
        // 没有可用词 → None（调用方直接返回空结果）
        assert_eq!(fts_match_query("\"*()"), None);
        assert_eq!(fts_match_query("   "), None);
    }

    /// 规模基线（10 万对象）：`search_index` 的真实耗时。
    ///
    /// 默认 `#[ignore]`（跑一次几秒，不该拖慢日常回归）；需要时：
    /// `cargo test -p rds-engine --lib scale_baseline -- --ignored --nocapture`
    ///
    /// 为什么要它：原型设计 §18.2 列的优化（前缀 / 中缀两段式、object_name 索引、
    /// 结果短时缓存）都**先要有基线**——没有数字就判断不了值不值得做。
    ///
    /// 2026-09-19 实测（debug，单连接，8 万表 + 2 万列；「旧」= 单条中缀 LIKE + 全排序，
    /// 同轮并排跑）：
    /// - `table_`（8 万前缀命中）：现 ~1.5 ms / 旧 ~158 ms——前缀段取够 50 条即停，命中多少无所谓；
    /// - `_0042`（中缀，命中在列档）：现 ~12 ms / 旧 ~72 ms——回落段也改成「分类索引名序走 + LIKE 过滤」，
    ///   命中密集时提前停；
    /// - `table_000042` / 无命中：现 ~45–50 ms / 旧 ~80 ms——最坏档：该词在表档一条不中，
    ///   需沿索引扫完表档整段（~39 ms）才能确认无命中。要再降只能靠 trigram 索引（§18.2）。
    #[test]
    #[ignore = "规模基线：手动跑（--ignored --nocapture）"]
    fn scale_baseline_100k_objects() -> Result<(), CoreError> {
        use std::time::Instant;

        let (mut ops, _db_path, dir) = fresh_ops("scale_baseline");
        let sid = ops.save_schema("main", "public", None, None)?;
        let conn_id = "P_scale";

        // 10 万对象：8 万表 + 2 万列（列挂在每第 4 张表下，贴近真实形状）
        let tables = 80_000usize;
        let mut entries: Vec<IndexEntryInput> = Vec::with_capacity(tables + tables / 4);
        for i in 0..tables {
            let name = format!("table_{i:06}");
            entries.push(IndexEntryInput {
                connection_id: conn_id.to_string(),
                schema_id: Some(sid),
                object_type: "table".to_string(),
                object_name: name.clone(),
                parent_name: None,
                path: format!("public/{name}"),
                introspect_level: 3,
                row_count_estimate: None,
                sort_weight: None,
                last_sync: Some(0),
            });
            if i % 4 == 0 {
                let column = format!("col_{i:06}");
                entries.push(IndexEntryInput {
                    connection_id: conn_id.to_string(),
                    schema_id: Some(sid),
                    object_type: "column".to_string(),
                    object_name: column.clone(),
                    parent_name: Some(name.clone()),
                    path: format!("public/{name}/{column}"),
                    introspect_level: 3,
                    row_count_estimate: None,
                    sort_weight: None,
                    last_sync: Some(0),
                });
            }
        }
        let rows = entries.len();
        let t0 = Instant::now();
        ops.save_index_entries_batch(entries)?;
        println!("index write: {} 行 / {:?}", rows, t0.elapsed());

        // 四种命中面：全命中（前缀）、单命中（完全相等）、无命中、中缀（最坏：必须扫全表）
        // 并排跑旧写法（单条中缀 LIKE + 全排序）：两者同一轮同一台机器才有可比性
        for needle in ["table_", "table_000042", "zzz_no_hit", "_0042"] {
            let t = Instant::now();
            let hits = ops.search_index(conn_id, needle, 50)?;
            let now = t.elapsed();
            let t = Instant::now();
            let old = oracle_search_index(&ops, conn_id, needle, 50)?;
            println!(
                "search_index({needle:?}) -> {} hits：现 {now:?} / 旧 {:?}（旧写法也是 {} 条）",
                hits.len(),
                t.elapsed(),
                old.len()
            );
        }

        // 拆开看两段的成本（选型依据；改搜索实现时重跑对比）：
        //   ① 前缀段：一类（表档）一段索引扫，取够 limit 即停，与命中总数无关
        //   ② 回落段：同类扫但用 LIKE 过滤（排序仍靠索引、免全量排序）
        {
            let conn = ops.get_connection();
            let upper = format!("table_{}", char::MAX);
            let table_sql = MetadataCacheOps::search_index_category_sql(
                MetadataCacheOps::SQL_SEARCH_INDEX_PREFIX_TEMPLATE,
                MetadataCacheOps::SEARCH_INDEX_CATEGORY_PREDICATES[0],
            );

            let t = Instant::now();
            let n = conn
                .prepare(&table_sql)?
                .query_map(
                    rusqlite::params![conn_id, "table_", upper, 50i64],
                    |r| r.get::<_, String>(0),
                )?
                .count();
            println!("[分解] 前缀段·表档一段索引扫（取到 {n} 条，8 万命中下）: {:?}", t.elapsed());

            let t = Instant::now();
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM metadata_index mi \
                 WHERE mi.connection_id = ?1 AND LOWER(mi.object_name) LIKE ?2 ESCAPE '\\'",
                rusqlite::params![conn_id, "%table_%"],
                |r| r.get(0),
            )?;
            println!("[分解] 中缀 LIKE 全扫（{n} 命中）: {:?}", t.elapsed());

            let t = Instant::now();
            let infix_sql = MetadataCacheOps::search_index_category_sql(
                MetadataCacheOps::SQL_SEARCH_INDEX_FALLBACK_TEMPLATE,
                MetadataCacheOps::SEARCH_INDEX_CATEGORY_PREDICATES[0],
            );
            let n = conn
                .prepare(&infix_sql)?
                .query_map(
                    rusqlite::params![conn_id, "%table_%", "table_", upper, 50i64],
                    |r| r.get::<_, String>(0),
                )?
                .count();
            println!("[分解] 中缀段·表档索引名序走（取到 {n} 条，同 8 万命中）: {:?}", t.elapsed());
        }

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
