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
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use rusqlite::{Connection, OptionalExtension};
use std::io::{Read, Write};

use crate::core::error::{CommonError, CoreError, StorageError};
use crate::core::migration::{MigrationManager, MigrationType};

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
    /// - 全局连接：{data_dir}/RdataStation/system/global_metadata/conn_{id}.sqlite
    /// - 项目连接：{project_path}/meta/connection_metadata/conn_{id}.sqlite
    fn build_metadata_path(
        conn_id: &str,
        connection_type: ConnectionType,
        project_path: Option<&str>,
    ) -> Result<PathBuf, CoreError> {
        let dir = match connection_type {
            ConnectionType::Global => {
                // 全局连接：存储到系统目录下的全局元数据目录
                let system_dir = crate::core::migration::get_system_dir()?;
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

        // 执行迁移
        MigrationManager::new().migrate(&self.db_path, MigrationType::ConnectionMetadata)?;

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
    /// 当连接被删除时调用
    pub fn delete(&self) -> Result<(), CoreError> {
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

/// 元数据缓存操作封装
///
/// 提供常用的元数据缓存读写操作
#[allow(dead_code)]
pub struct MetadataCacheOps {
    conn: Connection,
    /// 是否启用压缩（默认对大于 1KB 的数据启用压缩）
    compression_threshold: usize,
}

#[allow(dead_code)]
impl MetadataCacheOps {
    /// 创建新的元数据缓存操作实例
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            compression_threshold: 1024, // 1KB 阈值
        }
    }

    /// 创建带压缩配置的元数据缓存操作实例
    pub fn with_compression(conn: Connection, compression_threshold: usize) -> Self {
        Self {
            conn,
            compression_threshold,
        }
    }

    /// 获取底层数据库连接（用于版本迁移等操作）
    pub fn get_connection(&self) -> &Connection {
        &self.conn
    }

    /// 压缩数据
    fn compress_data(&self, data: &str) -> Result<Vec<u8>, CoreError> {
        if data.len() < self.compression_threshold {
            return Ok(data.as_bytes().to_vec());
        }

        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data.as_bytes()).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to compress data: {}",
                e
            )))
        })?;

        let compressed = encoder.finish().map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to finish compression: {}",
                e
            )))
        })?;

        Ok(compressed)
    }

    /// 解压数据
    fn decompress_data(&self, data: &[u8]) -> Result<String, CoreError> {
        let mut decoder = GzDecoder::new(data);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to decompress data: {}",
                e
            )))
        })?;

        Ok(decompressed)
    }

    /// 检查数据是否被压缩（通过 magic bytes 检查）
    fn is_compressed(&self, data: &[u8]) -> bool {
        data.len() > 2 && data[0] == 0x1f && data[1] == 0x8b
    }

    /// 获取表列表
    pub fn list_tables(
        &self,
        database_name: &str,
        schema_name: Option<&str>,
    ) -> Result<Vec<TableInfo>, CoreError> {
        let schema_filter = schema_name.unwrap_or("%");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, schema_name, table_name, comment, last_sync FROM metadata
             WHERE obj_type = 'table' AND database_name = ?1 AND schema_name LIKE ?2
             ORDER BY table_name",
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "list_tables".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let tables = stmt
            .query_map(rusqlite::params![database_name, schema_filter], |row| {
                Ok(TableInfo {
                    id: row.get(0)?,
                    schema_name: row.get(1)?,
                    name: row.get(2)?,
                    comment: row.get(3).ok(),
                    last_sync: row.get(4)?,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_tables".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for table in tables {
            result.push(table.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_table".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    /// 获取列列表
    pub fn list_columns(
        &self,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
    ) -> Result<Vec<ColumnInfo>, CoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, data_type, is_nullable, is_primary, is_unique, comment, last_sync
             FROM metadata
             WHERE obj_type = 'column' AND database_name = ?1 AND schema_name = ?2 AND table_name = ?3
             ORDER BY name"
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "list_columns".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let columns = stmt
            .query_map(
                rusqlite::params![database_name, schema_name, table_name],
                |row| {
                    Ok(ColumnInfo {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        data_type: row.get(2)?,
                        is_nullable: row.get::<_, i32>(3)? != 0,
                        is_primary: row.get::<_, i32>(4)? != 0,
                        is_unique: row.get::<_, i32>(5)? != 0,
                        comment: row.get(6).ok(),
                        last_sync: row.get(7)?,
                    })
                },
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "query_columns".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for column in columns {
            result.push(column.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_column".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
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

    /// 保存表元数据（兼容旧接口，通过 schema/table 名）
    pub fn save_table_metadata(
        &self,
        _id: &str,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
        comment: Option<&str>,
        last_sync: i64,
    ) -> Result<(), CoreError> {
        let schema_id = self.get_schema_id(database_name, schema_name)?;
        let schema_id = match schema_id {
            Some(id) => id,
            None => self.save_schema(database_name, schema_name, None, None)?,
        };

        self.conn.execute(
            "INSERT OR REPLACE INTO tables
             (schema_id, table_name, table_type, table_comment, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, 'TABLE', ?3, 3, 1, ?4, ?4)",
            rusqlite::params![schema_id, table_name, comment, last_sync],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_table_metadata".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(())
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
    #[allow(clippy::too_many_arguments)]
    pub fn save_column(
        &self,
        table_id: i64,
        column_name: &str,
        data_type: &str,
        ordinal_position: i32,
        is_nullable: bool,
        is_primary: bool,
        _is_unique: bool,
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
            rusqlite::params![table_id, column_name, ordinal_position, data_type, is_nullable as i32, is_primary as i32, is_primary as i32, column_default, comment, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_column".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// 保存列元数据（兼容旧接口）
    #[allow(clippy::too_many_arguments)]
    pub fn save_column_metadata(
        &self,
        _id: &str,
        database_name: &str,
        schema_name: &str,
        table_name: &str,
        column_name: &str,
        data_type: &str,
        is_nullable: bool,
        is_primary: bool,
        _is_unique: bool,
        last_sync: i64,
    ) -> Result<(), CoreError> {
        let schema_id = self
            .get_schema_id(database_name, schema_name)?
            .ok_or_else(|| {
                CoreError::common(CommonError::General("Schema not found".to_string()))
            })?;
        let table_id = self.get_table_id(schema_id, table_name)?.ok_or_else(|| {
            CoreError::common(CommonError::General("Table not found".to_string()))
        })?;

        self.conn.execute(
            "INSERT OR REPLACE INTO columns
             (table_id, column_name, data_type, is_nullable, is_identity, is_primary, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 3, 1, ?7, ?7)",
            rusqlite::params![table_id, column_name, data_type, is_nullable as i32, is_primary as i32, is_primary as i32, last_sync],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_column_metadata".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(())
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
             LEFT JOIN foreign_key_columns fkc ON c.table_id = fkc.table_id AND c.column_name = fkc.column_name \
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
             (id, view_definition, is_updatable, check_option, introspect_level, is_loaded, last_sync)
             VALUES (?1, ?2, ?3, ?4, 3, 1, ?5)",
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
             (table_id, trigger_name, trigger_event, trigger_timing, trigger_body, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, ?3, ?4, ?5, 3, 1, ?6, ?6)",
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

    // ==================== Sequence 操作 ====================

    /// 保存 Sequence 元数据（基础版）
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
             (schema_id, sequence_name, data_type, start_value, increment_by, introspect_level, is_loaded, last_sync, last_accessed)
             VALUES (?1, ?2, ?3, ?4, ?5, 3, 1, ?6, ?6)",
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

    /// 记录同步日志
    pub fn log_sync(
        &self,
        id: &str,
        start_at: i64,
        end_at: i64,
        success: bool,
        message: Option<&str>,
        objects_fetched: i64,
    ) -> Result<(), CoreError> {
        self.conn
            .execute(
                "INSERT INTO sync_log (id, start_at, end_at, success, message, objects_fetched)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    id,
                    start_at,
                    end_at,
                    success as i32,
                    message.unwrap_or(""),
                    objects_fetched
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "log_sync".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
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
        detail: &crate::core::driver::NodeDetail,
    ) -> Result<i64, CoreError> {
        let table_name = &detail.node.name;
        let table_type = match detail.node.kind {
            crate::core::driver::SchemaObjectKind::View => "VIEW",
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
        indexes: Vec<crate::core::driver::IndexDetail>,
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
        constraints: Vec<crate::core::driver::ConstraintDetail>,
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
        indexes: Vec<crate::core::driver::IndexDetail>,
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
        constraints: Vec<crate::core::driver::ConstraintDetail>,
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
    ) -> Result<Option<crate::core::driver::NodeDetail>, CoreError> {
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
            crate::core::driver::SchemaObjectKind::View
        } else {
            crate::core::driver::SchemaObjectKind::Table
        };

        let mut stmt = self.conn.prepare(
            "SELECT column_name, data_type, is_nullable, COALESCE(is_primary, 0) AS is_primary_key,
             0 AS is_foreign_key, column_default, column_comment, extra
             FROM columns WHERE table_id = ?1 ORDER BY ordinal_position"
        ).map_err(|e| CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: "load_node_detail_columns".to_string(),
            reason: e.to_string(),
        }))?;

        let columns: Vec<crate::core::driver::ColumnDetail> = stmt
            .query_map(rusqlite::params![table_id], |row| {
                let extra_json: Option<String> = row.get(7)?;
                let extra = if let Some(ref json_str) = extra_json {
                    serde_json::from_str(json_str).unwrap_or_default()
                } else {
                    std::collections::HashMap::new()
                };

                Ok(crate::core::driver::ColumnDetail {
                    name: row.get(0)?,
                    data_type: row.get(1)?,
                    nullable: row.get::<_, i32>(2)? != 0,
                    is_primary_key: row.get::<_, i32>(3)? != 0,
                    is_foreign_key: row.get::<_, i32>(4)? != 0,
                    default_value: row.get(5)?,
                    comment: row.get(6)?,
                    extra,
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

        Ok(Some(crate::core::driver::NodeDetail {
            node: crate::core::driver::NodeInfo {
                name: table_name.to_string(),
                kind,
                icon: Some(if table_type == "VIEW" {
                    "view".to_string()
                } else {
                    "table".to_string()
                }),
                comment: table_comment,
            },
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

    /// 获取最后同步时间
    ///
    /// # 参数
    /// * `database_name` - 数据库名称
    /// * `schema_name` - 模式名称
    pub fn get_last_sync_time(
        &self,
        database_name: &str,
        schema_name: &str,
    ) -> Result<Option<i64>, CoreError> {
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
                    operation: "get_last_sync_time".to_string(),
                    reason: e.to_string(),
                })
            })?
            .flatten();

        Ok(last_sync)
    }

    /// 获取缓存统计信息
    ///
    /// # 参数
    /// * `database_name` - 数据库名称
    /// * `schema_name` - 模式名称
    pub fn get_cache_stats(
        &self,
        database_name: &str,
        schema_name: &str,
    ) -> Result<CacheStats, CoreError> {
        let table_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM metadata
             WHERE obj_type = 'table' AND database_name = ?1 AND schema_name = ?2",
                rusqlite::params![database_name, schema_name],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "count_tables".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let column_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM metadata
             WHERE obj_type = 'column' AND database_name = ?1 AND schema_name = ?2",
                rusqlite::params![database_name, schema_name],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "count_columns".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let last_sync = self.get_last_sync_time(database_name, schema_name)?;

        Ok(CacheStats {
            table_count: table_count as usize,
            column_count: column_count as usize,
            last_sync,
        })
    }

    /// 同步 FTS5 索引
    ///
    /// 将规范化表的数据同步到 FTS5 虚拟表，支持增量更新
    pub fn sync_fts_index(&mut self, sync_type: Option<&str>) -> Result<(), CoreError> {
        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "begin_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        // 清理旧的 FTS 数据
        let _ = match sync_type {
            Some(t) => match t {
                "schema" => tx.execute("DELETE FROM metadata_fts WHERE search_type = 'schema'", []),
                "table" => tx.execute("DELETE FROM metadata_fts WHERE search_type = 'table'", []),
                "column" => tx.execute("DELETE FROM metadata_fts WHERE search_type = 'column'", []),
                "view" => tx.execute("DELETE FROM metadata_fts WHERE search_type = 'view'", []),
                "routine" => {
                    tx.execute("DELETE FROM metadata_fts WHERE search_type = 'routine'", [])
                }
                _ => tx.execute("DELETE FROM metadata_fts", []),
            },
            None => tx.execute("DELETE FROM metadata_fts", []),
        };

        // 同步 schemas
        if sync_type.is_none() || sync_type == Some("schema") {
            tx.execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'schema', schema_name, schema_name, '', schema_name
                 FROM schemata WHERE is_loaded = 1",
                [],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "sync_fts_schema".to_string(),
                    reason: e.to_string(),
                }
            ))?;
        }

        // 同步 tables
        if sync_type.is_none() || sync_type == Some("table") {
            tx.execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'table', s.schema_name, t.table_name, s.schema_name,
                        s.schema_name || ' ' || t.table_name || ' ' || COALESCE(t.table_comment, '')
                 FROM tables t
                 INNER JOIN schemata s ON t.schema_id = s.id
                 WHERE t.is_loaded = 1",
                [],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "sync_fts_table".to_string(),
                    reason: e.to_string(),
                }
            ))?;
        }

        // 同步 columns
        if sync_type.is_none() || sync_type == Some("column") {
            tx.execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'column', s.schema_name, c.column_name, t.table_name,
                        s.schema_name || ' ' || t.table_name || ' ' || c.column_name || ' ' || c.data_type || ' ' || COALESCE(c.column_comment, '')
                 FROM columns c
                 INNER JOIN tables t ON c.table_id = t.id
                 INNER JOIN schemata s ON t.schema_id = s.id
                 WHERE c.is_loaded = 1",
                [],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "sync_fts_column".to_string(),
                    reason: e.to_string(),
                }
            ))?;
        }

        // 同步 views
        if sync_type.is_none() || sync_type == Some("view") {
            tx.execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'view', s.schema_name, v.view_name, s.schema_name,
                        s.schema_name || ' ' || v.view_name || ' ' || COALESCE(v.view_comment, '')
                 FROM views v
                 INNER JOIN schemata s ON v.schema_id = s.id
                 WHERE v.is_loaded = 1",
                [],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "sync_fts_view".to_string(),
                    reason: e.to_string(),
                }
            ))?;
        }

        // 同步 routines
        if sync_type.is_none() || sync_type == Some("routine") {
            tx.execute(
                "INSERT INTO metadata_fts (search_type, schema_name, object_name, parent_name, search_content)
                 SELECT 'routine', s.schema_name, r.routine_name, s.schema_name,
                        s.schema_name || ' ' || r.routine_name || ' ' || r.routine_type || ' ' || COALESCE(r.routine_comment, '')
                 FROM routines r
                 INNER JOIN schemata s ON r.schema_id = s.id
                 WHERE r.is_loaded = 1",
                [],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "sync_fts_routine".to_string(),
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
        let search_pattern = format!("{}*", query);

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

    /// 删除 Schema 及关联数据（级联）
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

        // 删除 tables（会通过触发器级联删除 columns 和 indexes）
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

        // 删除 views
        let view_count = tx
            .execute(
                "DELETE FROM views WHERE schema_id = ?1",
                rusqlite::params![schema_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "delete_views".to_string(),
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

        Ok(schema_deleted + table_count + view_count + routine_count)
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

    /// 更新同步状态
    pub fn update_sync_status(
        &mut self,
        connection_id: &str,
        status: &str,
        progress: i32,
        current_object: Option<&str>,
    ) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let current_status: Option<String> = self
            .conn
            .query_row(
                "SELECT status FROM connection_sync_status WHERE connection_id = ?1",
                rusqlite::params![connection_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_sync_status".to_string(),
                    reason: e.to_string(),
                })
            })?;

        match current_status {
            Some(_) => {
                self.conn
                    .execute(
                        "UPDATE connection_sync_status
                     SET status = ?1, progress = ?2, current_object = ?3,
                         started_at = COALESCE(started_at, ?4),
                         completed_at = CASE WHEN ?1 = 'completed' THEN ?4 ELSE NULL END
                     WHERE connection_id = ?5",
                        rusqlite::params![status, progress, current_object, now, connection_id],
                    )
                    .map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "sqlite".to_string(),
                            operation: "update_sync_status".to_string(),
                            reason: e.to_string(),
                        })
                    })?;
            }
            None => {
                self.conn
                    .execute(
                        "INSERT INTO connection_sync_status
                     (connection_id, status, progress, current_object, started_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![connection_id, status, progress, current_object, now],
                    )
                    .map_err(|e| {
                        CoreError::storage(StorageError::Persistence {
                            store: "sqlite".to_string(),
                            operation: "insert_sync_status".to_string(),
                            reason: e.to_string(),
                        })
                    })?;
            }
        }

        Ok(())
    }

    /// 获取同步状态
    pub fn get_sync_status(
        &self,
        connection_id: &str,
    ) -> Result<Option<SyncStatusInfo>, CoreError> {
        let status: Option<SyncStatusInfo> = self
            .conn
            .query_row(
                "SELECT connection_id, status, progress, total_objects, synced_objects,
                    current_object, started_at, completed_at, last_error
             FROM connection_sync_status WHERE connection_id = ?1",
                rusqlite::params![connection_id],
                |row| {
                    Ok(SyncStatusInfo {
                        connection_id: row.get(0)?,
                        status: row.get(1)?,
                        progress: row.get(2)?,
                        total_objects: row.get(3)?,
                        synced_objects: row.get(4)?,
                        current_object: row.get(5)?,
                        started_at: row.get(6)?,
                        completed_at: row.get(7)?,
                        last_error: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_sync_status".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(status)
    }

    /// 检查连接是否正在同步
    pub fn is_syncing(&self, connection_id: &str) -> Result<bool, CoreError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM connection_sync_status
             WHERE connection_id = ?1 AND status IN ('indexing', 'syncing')",
                rusqlite::params![connection_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "check_syncing".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(count > 0)
    }

    /// 取消同步
    pub fn cancel_sync(&self, connection_id: &str) -> Result<(), CoreError> {
        self.conn
            .execute(
                "UPDATE connection_sync_status SET status = 'cancelled', completed_at = ?1
             WHERE connection_id = ?2 AND status IN ('indexing', 'syncing')",
                rusqlite::params![
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| CoreError::common(CommonError::General(format!(
                            "获取系统时间失败: {}",
                            e
                        ))))?
                        .as_secs() as i64,
                    connection_id
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "cancel_sync".to_string(),
                    reason: e.to_string(),
                })
            })?;

        self.conn
            .execute(
                "UPDATE sync_tasks SET status = 'cancelled', completed_at = ?1
             WHERE connection_id = ?2 AND status IN ('pending', 'running')",
                rusqlite::params![
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| CoreError::common(CommonError::General(format!(
                            "获取系统时间失败: {}",
                            e
                        ))))?
                        .as_secs() as i64,
                    connection_id
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "cancel_sync_tasks".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }

    // ==================== V6: 后台同步任务队列 ====================

    /// 入队同步任务
    pub fn enqueue_sync_task(
        &self,
        connection_id: &str,
        task_type: &str,
        object_name: &str,
        parent_name: Option<&str>,
        priority: i32,
    ) -> Result<i64, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT INTO sync_tasks (connection_id, task_type, object_name, parent_name, priority, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![connection_id, task_type, object_name, parent_name, priority, now],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "enqueue_sync_task".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(self.conn.last_insert_rowid())
    }

    /// 入队多个同步任务（批量）
    pub fn enqueue_sync_tasks_batch(
        &mut self,
        tasks: Vec<SyncTaskInput>,
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
        for task in tasks {
            tx.execute(
                "INSERT INTO sync_tasks (connection_id, task_type, object_name, parent_name, priority, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![task.connection_id, task.task_type, task.object_name, task.parent_name, task.priority, now],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "enqueue_sync_task_batch".to_string(),
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

    /// 获取下一个待处理任务
    pub fn get_next_sync_task(
        &self,
        connection_id: &str,
    ) -> Result<Option<SyncTaskInfo>, CoreError> {
        let task: Option<SyncTaskInfo> = self.conn.query_row(
            "SELECT id, connection_id, task_type, object_name, parent_name, priority, status, created_at
             FROM sync_tasks
             WHERE connection_id = ?1 AND status = 'pending'
             ORDER BY priority ASC, created_at ASC
             LIMIT 1",
            rusqlite::params![connection_id],
            |row| {
                Ok(SyncTaskInfo {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    task_type: row.get(2)?,
                    object_name: row.get(3)?,
                    parent_name: row.get(4)?,
                    priority: row.get(5)?,
                    status: row.get(6)?,
                    created_at: row.get(7)?,
                })
            },
        ).optional().map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_next_sync_task".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(task)
    }

    /// 认领任务（标记为 running）
    pub fn claim_sync_task(&self, task_id: i64) -> Result<bool, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let affected = self
            .conn
            .execute(
                "UPDATE sync_tasks SET status = 'running', started_at = ?1
             WHERE id = ?2 AND status = 'pending'",
                rusqlite::params![now, task_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "claim_sync_task".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(affected > 0)
    }

    /// 完成同步任务
    pub fn complete_sync_task(
        &self,
        task_id: i64,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let status = if success { "completed" } else { "failed" };

        self.conn
            .execute(
                "UPDATE sync_tasks SET status = ?1, completed_at = ?2, error_message = ?3
             WHERE id = ?4",
                rusqlite::params![status, now, error_message, task_id],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "complete_sync_task".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(())
    }

    /// 获取待处理任务数量
    pub fn get_pending_task_count(&self, connection_id: &str) -> Result<i64, CoreError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sync_tasks WHERE connection_id = ?1 AND status = 'pending'",
                rusqlite::params![connection_id],
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_pending_task_count".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(count)
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

    /// 分块获取表名（避免 OOM）
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    /// * `schema_id` - Schema ID
    /// * `offset` - 偏移量
    /// * `limit` - 每块大小
    pub fn get_tables_chunk(
        &self,
        connection_id: &str,
        schema_id: Option<i64>,
        offset: i64,
        limit: i64,
    ) -> Result<ChunkResult<IndexEntry>, CoreError> {
        let (count_sql, query_sql) = match schema_id {
            Some(_sid) => (
                "SELECT COUNT(*) FROM metadata_index WHERE connection_id = ?1 AND object_type = 'table' AND schema_id = ?2",
                "SELECT id, schema_id, object_type, object_name, parent_name, path,
                        introspect_level, is_loaded, last_sync, row_count_estimate, sort_weight
                 FROM metadata_index
                 WHERE connection_id = ?1 AND object_type = 'table' AND schema_id = ?2
                 ORDER BY sort_weight DESC, object_name ASC
                 LIMIT ?3 OFFSET ?4",
            ),
            None => (
                "SELECT COUNT(*) FROM metadata_index WHERE connection_id = ?1 AND object_type = 'table'",
                "SELECT id, schema_id, object_type, object_name, parent_name, path,
                        introspect_level, is_loaded, last_sync, row_count_estimate, sort_weight
                 FROM metadata_index
                 WHERE connection_id = ?1 AND object_type = 'table'
                 ORDER BY sort_weight DESC, object_name ASC
                 LIMIT ?2 OFFSET ?3",
            ),
        };

        let total: i64 = match schema_id {
            Some(sid) => self
                .conn
                .query_row(count_sql, rusqlite::params![connection_id, sid], |row| {
                    row.get(0)
                })
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "count_tables_chunk".to_string(),
                        reason: e.to_string(),
                    })
                })?,
            None => self
                .conn
                .query_row(count_sql, rusqlite::params![connection_id], |row| {
                    row.get(0)
                })
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "count_tables_chunk".to_string(),
                        reason: e.to_string(),
                    })
                })?,
        };

        let mut stmt = self.conn.prepare(query_sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_tables_chunk".to_string(),
                reason: e.to_string(),
            })
        })?;

        let entries = match schema_id {
            Some(sid) => stmt
                .query_map(
                    rusqlite::params![connection_id, sid, limit, offset],
                    IndexEntry::from_row,
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "query_tables_chunk".to_string(),
                        reason: e.to_string(),
                    })
                })?,
            None => stmt
                .query_map(
                    rusqlite::params![connection_id, limit, offset],
                    IndexEntry::from_row,
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "query_tables_chunk".to_string(),
                        reason: e.to_string(),
                    })
                })?,
        };

        let mut result = Vec::new();
        for entry in entries {
            result.push(entry.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "fetch_table_chunk".to_string(),
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

    // ==================== V6: 预热核心逻辑 ====================

    /// 批量保存索引条目（内部使用，自动处理事务）
    ///
    /// # 参数
    /// * `entries` - 索引条目列表
    /// * `batch_size` - 每批次大小
    pub fn save_index_entries_internal(
        &mut self,
        entries: Vec<IndexEntryInput>,
        batch_size: usize,
    ) -> Result<usize, CoreError> {
        let mut total_saved = 0;
        let tx = self.conn.transaction().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "begin_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        for chunk in entries.chunks(batch_size) {
            for entry in chunk {
                tx.execute(
                    "INSERT OR REPLACE INTO metadata_index
                     (connection_id, schema_id, object_type, object_name, parent_name, path,
                      introspect_level, is_loaded, last_sync, sort_weight)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
                    rusqlite::params![
                        entry.connection_id,
                        entry.schema_id,
                        entry.object_type,
                        entry.object_name,
                        entry.parent_name,
                        entry.path,
                        entry.introspect_level,
                        entry.last_sync,
                        entry.sort_weight.unwrap_or(0),
                    ],
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "save_index_entry".to_string(),
                        reason: e.to_string(),
                    })
                })?;
                total_saved += 1;
            }
        }

        tx.commit().map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "commit_transaction".to_string(),
                reason: e.to_string(),
            })
        })?;

        Ok(total_saved)
    }

    /// 构建元数据索引（用于预热）
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    /// * `schemas` - Schema 列表（名称）
    /// * `tables_per_schema` - 每个 schema 的表信息 (schema_name, tables)
    /// * `columns_per_table` - 每个表的列信息 (schema, table, columns)
    ///
    /// # DataGrip 风格
    /// - Level 1: 仅索引（名称）
    /// - Level 2: 概要（无源码）
    /// - Level 3: 完整
    pub fn build_metadata_index(
        &mut self,
        connection_id: &str,
        schemas: Vec<String>,
        tables_per_schema: Vec<(String, Vec<String>)>,
        columns_per_table: Vec<(String, String, Vec<String>)>,
    ) -> Result<IndexBuildResult, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let schema_count = schemas.len();
        let table_count: usize = tables_per_schema.iter().map(|(_, t)| t.len()).sum();
        let column_count: usize = columns_per_table.iter().map(|(_, _, c)| c.len()).sum();

        let mut all_entries = Vec::new();
        let mut schema_ids = std::collections::HashMap::new();

        for (i, schema_name) in schemas.iter().enumerate() {
            let schema_id = (i + 1) as i64;
            schema_ids.insert(schema_name.clone(), schema_id);

            all_entries.push(IndexEntryInput {
                connection_id: connection_id.to_string(),
                schema_id: None,
                object_type: "schema".to_string(),
                object_name: schema_name.clone(),
                parent_name: None,
                path: schema_name.clone(),
                introspect_level: 1,
                row_count_estimate: None,
                sort_weight: Some(0),
                last_sync: Some(now),
            });
        }

        for (schema_name, tables) in &tables_per_schema {
            let schema_id = schema_ids.get(schema_name).copied();
            for table_name in tables {
                let path = format!("{}/{}", schema_name, table_name);
                all_entries.push(IndexEntryInput {
                    connection_id: connection_id.to_string(),
                    schema_id,
                    object_type: "table".to_string(),
                    object_name: table_name.clone(),
                    parent_name: Some(schema_name.clone()),
                    path,
                    introspect_level: 1,
                    row_count_estimate: None,
                    sort_weight: Some(0),
                    last_sync: Some(now),
                });
            }
        }

        for (schema_name, table_name, columns) in &columns_per_table {
            let schema_id = schema_ids.get(schema_name).copied();
            let parent_name = table_name.clone();
            let base_path = format!("{}/{}", schema_name, table_name);
            for col_name in columns {
                let path = format!("{}/{}", base_path, col_name);
                all_entries.push(IndexEntryInput {
                    connection_id: connection_id.to_string(),
                    schema_id,
                    object_type: "column".to_string(),
                    object_name: col_name.clone(),
                    parent_name: Some(parent_name.clone()),
                    path,
                    introspect_level: 1,
                    row_count_estimate: None,
                    sort_weight: Some(0),
                    last_sync: Some(now),
                });
            }
        }

        let saved_count = self.save_index_entries_internal(all_entries, 500)?;

        self.update_sync_status(connection_id, "completed", 100, None)
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "update_sync_status".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(IndexBuildResult {
            schema_count,
            table_count,
            column_count,
            total_entries: saved_count,
        })
    }

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

/// V6: 分页索引结果
#[derive(Debug, Clone)]
pub struct PaginatedIndexResult {
    pub entries: Vec<IndexEntry>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

/// V6: 同步状态信息
#[derive(Debug, Clone)]
pub struct SyncStatusInfo {
    pub connection_id: String,
    pub status: String,
    pub progress: i32,
    pub total_objects: Option<i32>,
    pub synced_objects: Option<i32>,
    pub current_object: Option<String>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub last_error: Option<String>,
}

/// V6: 同步任务输入
#[derive(Debug, Clone)]
pub struct SyncTaskInput {
    pub connection_id: String,
    pub task_type: String,
    pub object_name: String,
    pub parent_name: Option<String>,
    pub priority: i32,
}

/// V6: 同步任务信息
#[derive(Debug, Clone)]
pub struct SyncTaskInfo {
    pub id: i64,
    pub connection_id: String,
    pub task_type: String,
    pub object_name: String,
    pub parent_name: Option<String>,
    pub priority: i32,
    pub status: String,
    pub created_at: Option<i64>,
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

impl MetadataCacheOps {
    // =======================================================================
    // V7: 增量同步 - Hash 计算
    // =======================================================================

    /// 计算对象 Hash（用于增量同步）
    pub fn calculate_object_hash(
        object_type: &str,
        name: &str,
        parent: Option<&str>,
        extra_data: Option<&str>,
    ) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(object_type.as_bytes());
        hasher.update(b"|");
        hasher.update(name.as_bytes());
        hasher.update(b"|");
        if let Some(p) = parent {
            hasher.update(p.as_bytes());
        }
        hasher.update(b"|");
        if let Some(e) = extra_data {
            hasher.update(e.as_bytes());
        }

        let result = hasher.finalize();
        hex::encode(result)
    }

    // =======================================================================
    // V7: 增量同步 - 快照管理
    // =======================================================================

    /// 保存快照
    pub fn save_snapshot(
        &mut self,
        connection_id: &str,
        snapshot_type: &str,
        snapshots: Vec<SyncSnapshot>,
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

        // 清除该连接的旧快照
        tx.execute(
            "DELETE FROM sync_snapshot WHERE connection_id = ?1 AND snapshot_type = ?2",
            rusqlite::params![connection_id, snapshot_type],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "clear_old_snapshot".to_string(),
                reason: e.to_string(),
            })
        })?;

        let mut count = 0;
        for snapshot in snapshots {
            tx.execute(
                "INSERT INTO sync_snapshot
                 (connection_id, snapshot_type, object_type, object_name, parent_name, object_hash, snapshot_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    connection_id,
                    snapshot_type,
                    snapshot.object_type,
                    snapshot.object_name,
                    snapshot.parent_name,
                    snapshot.object_hash,
                    now,
                ],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_snapshot".to_string(),
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

    /// 获取快照
    pub fn get_snapshot(
        &self,
        connection_id: &str,
        snapshot_type: &str,
    ) -> Result<Vec<SyncSnapshot>, CoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, connection_id, snapshot_type, object_type, object_name, parent_name, object_hash, snapshot_at
             FROM sync_snapshot
             WHERE connection_id = ?1 AND snapshot_type = ?2
             ORDER BY object_type, object_name"
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_get_snapshot".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let snapshots = stmt
            .query_map(rusqlite::params![connection_id, snapshot_type], |row| {
                Ok(SyncSnapshot {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    snapshot_type: row.get(2)?,
                    object_type: row.get(3)?,
                    object_name: row.get(4)?,
                    parent_name: row.get(5)?,
                    object_hash: row.get(6)?,
                    snapshot_at: row.get(7)?,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_snapshot".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for snapshot in snapshots {
            result.push(snapshot.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "parse_snapshot".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    /// 检查是否已有快照
    pub fn has_snapshot(
        &self,
        connection_id: &str,
        snapshot_type: &str,
    ) -> Result<bool, CoreError> {
        let count: i32 = self.conn.query_row(
            "SELECT COUNT(*) FROM sync_snapshot WHERE connection_id = ?1 AND snapshot_type = ?2",
            rusqlite::params![connection_id, snapshot_type],
            |row| row.get(0),
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "check_snapshot_exists".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(count > 0)
    }

    // =======================================================================
    // V7: 增量同步 - 变更检测
    // =======================================================================

    /// 检测 Schema 变更
    pub fn detect_schema_changes(
        &self,
        connection_id: &str,
    ) -> Result<Vec<SyncOperation>, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let mut stmt = self.conn.prepare(
            "SELECT object_type, object_name, parent_name, object_hash, snapshot_hash, operation_type
             FROM v_schema_changes
             WHERE connection_id = ?1"
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_detect_schema_changes".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let operations = stmt
            .query_map(rusqlite::params![connection_id], |row| {
                Ok(SyncOperation {
                    id: None,
                    connection_id: connection_id.to_string(),
                    operation_type: row.get(5)?,
                    object_type: row.get(0)?,
                    object_name: row.get(1)?,
                    parent_name: row.get(2)?,
                    old_hash: row.get(4)?,
                    new_hash: row.get(3)?,
                    detected_at: now,
                    processed_at: None,
                    status: "pending".to_string(),
                    priority: 5,
                    error_message: None,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "detect_schema_changes".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for op in operations {
            result.push(op.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "parse_change_operation".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    /// 检测 Table 变更
    pub fn detect_table_changes(
        &self,
        connection_id: &str,
    ) -> Result<Vec<SyncOperation>, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let mut stmt = self.conn.prepare(
            "SELECT object_type, object_name, parent_name, object_hash, snapshot_hash, operation_type
             FROM v_table_changes
             WHERE connection_id = ?1"
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_detect_table_changes".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let operations = stmt
            .query_map(rusqlite::params![connection_id], |row| {
                Ok(SyncOperation {
                    id: None,
                    connection_id: connection_id.to_string(),
                    operation_type: row.get(5)?,
                    object_type: row.get(0)?,
                    object_name: row.get(1)?,
                    parent_name: row.get(2)?,
                    old_hash: row.get(4)?,
                    new_hash: row.get(3)?,
                    detected_at: now,
                    processed_at: None,
                    status: "pending".to_string(),
                    priority: 5,
                    error_message: None,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "detect_table_changes".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for op in operations {
            result.push(op.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "parse_change_operation".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    /// 检测 Column 变更
    pub fn detect_column_changes(
        &self,
        connection_id: &str,
    ) -> Result<Vec<SyncOperation>, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let mut stmt = self.conn.prepare(
            "SELECT object_type, object_name, parent_name, object_hash, snapshot_hash, operation_type
             FROM v_column_changes
             WHERE connection_id = ?1"
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_detect_column_changes".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let operations = stmt
            .query_map(rusqlite::params![connection_id], |row| {
                Ok(SyncOperation {
                    id: None,
                    connection_id: connection_id.to_string(),
                    operation_type: row.get(5)?,
                    object_type: row.get(0)?,
                    object_name: row.get(1)?,
                    parent_name: row.get(2)?,
                    old_hash: row.get(4)?,
                    new_hash: row.get(3)?,
                    detected_at: now,
                    processed_at: None,
                    status: "pending".to_string(),
                    priority: 5,
                    error_message: None,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "detect_column_changes".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for op in operations {
            result.push(op.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "parse_change_operation".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    /// 检测所有变更
    pub fn detect_all_changes(
        &self,
        connection_id: &str,
    ) -> Result<ChangeDetectionResult, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let schema_changes = self.detect_schema_changes(connection_id)?;
        let table_changes = self.detect_table_changes(connection_id)?;
        let column_changes = self.detect_column_changes(connection_id)?;

        let all_changes: Vec<_> = schema_changes
            .into_iter()
            .chain(table_changes)
            .chain(column_changes)
            .collect();

        let mut result = ChangeDetectionResult {
            connection_id: connection_id.to_string(),
            create_count: 0,
            update_count: 0,
            delete_count: 0,
            no_change_count: 0,
            total: all_changes.len(),
            detected_at: now,
        };

        for change in &all_changes {
            match change.operation_type.as_str() {
                "create" => result.create_count += 1,
                "update" => result.update_count += 1,
                "delete" => result.delete_count += 1,
                "no_change" => result.no_change_count += 1,
                _ => {}
            }
        }

        Ok(result)
    }

    // =======================================================================
    // V7: 增量同步 - 操作管理
    // =======================================================================

    /// 保存变更操作
    pub fn save_sync_operations(
        &mut self,
        operations: Vec<SyncOperation>,
    ) -> Result<usize, CoreError> {
        let mut count = 0;
        for op in operations {
            self.conn.execute(
                "INSERT INTO sync_operations
                 (connection_id, operation_type, object_type, object_name, parent_name, old_hash, new_hash, detected_at, status, priority)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    op.connection_id,
                    op.operation_type,
                    op.object_type,
                    op.object_name,
                    op.parent_name,
                    op.old_hash,
                    op.new_hash,
                    op.detected_at,
                    op.status,
                    op.priority,
                ],
            ).map_err(|e| CoreError::storage(
                StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "save_sync_operation".to_string(),
                    reason: e.to_string(),
                }
            ))?;
            count += 1;
        }

        Ok(count)
    }

    /// 获取待处理的变更操作
    pub fn get_pending_operations(
        &self,
        connection_id: &str,
        limit: u32,
    ) -> Result<Vec<SyncOperation>, CoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, connection_id, operation_type, object_type, object_name, parent_name, old_hash, new_hash, detected_at, processed_at, status, priority, error_message
             FROM sync_operations
             WHERE connection_id = ?1 AND status = 'pending'
             ORDER BY priority DESC, detected_at ASC
             LIMIT ?2"
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_get_pending_operations".to_string(),
                reason: e.to_string(),
            }
        ))?;

        let operations = stmt
            .query_map(rusqlite::params![connection_id, limit], |row| {
                Ok(SyncOperation {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    operation_type: row.get(2)?,
                    object_type: row.get(3)?,
                    object_name: row.get(4)?,
                    parent_name: row.get(5)?,
                    old_hash: row.get(6)?,
                    new_hash: row.get(7)?,
                    detected_at: row.get(8)?,
                    processed_at: row.get(9)?,
                    status: row.get(10)?,
                    priority: row.get(11)?,
                    error_message: row.get(12)?,
                })
            })
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "get_pending_operations".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let mut result = Vec::new();
        for op in operations {
            result.push(op.map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "parse_operation".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }

        Ok(result)
    }

    /// 标记操作为已处理
    pub fn mark_operation_processed(
        &mut self,
        operation_id: i64,
        success: bool,
        error_msg: Option<&str>,
    ) -> Result<(), CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let status = if success { "completed" } else { "failed" };

        self.conn.execute(
            "UPDATE sync_operations SET status = ?1, processed_at = ?2, error_message = ?3 WHERE id = ?4",
            rusqlite::params![status, now, error_msg, operation_id],
        ).map_err(|e| CoreError::storage(
            StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "mark_operation_processed".to_string(),
                reason: e.to_string(),
            }
        ))?;

        Ok(())
    }

    /// 清除旧的同步操作
    pub fn clear_old_operations(
        &mut self,
        connection_id: &str,
        days: u32,
    ) -> Result<usize, CoreError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| {
                CoreError::common(CommonError::General(format!("获取系统时间失败: {}", e)))
            })?
            .as_secs() as i64;

        let cutoff = now - (days as i64) * 86400;

        let count = self
            .conn
            .execute(
                "DELETE FROM sync_operations WHERE connection_id = ?1 AND detected_at < ?2",
                rusqlite::params![connection_id, cutoff],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "clear_old_operations".to_string(),
                    reason: e.to_string(),
                })
            })?;

        Ok(count)
    }

    // =======================================================================
    // V7: 增量同步 - 便捷方法
    // =======================================================================

    /// 执行完整的增量同步流程
    pub fn incremental_sync(
        &mut self,
        connection_id: &str,
    ) -> Result<ChangeDetectionResult, CoreError> {
        // 1. 检测变更
        let detection_result = self.detect_all_changes(connection_id)?;

        // 2. 如果是第一次同步（没有快照），执行全量同步
        let has_snapshot = self.has_snapshot(connection_id, "full")?;
        if !has_snapshot {
            return Ok(detection_result);
        }

        // 3. 保存检测到的变更
        let mut schema_ops = self.detect_schema_changes(connection_id)?;
        let mut table_ops = self.detect_table_changes(connection_id)?;
        let mut column_ops = self.detect_column_changes(connection_id)?;

        let all_ops: Vec<_> = schema_ops
            .drain(..)
            .chain(table_ops.drain(..))
            .chain(column_ops.drain(..))
            .filter(|op| op.operation_type != "no_change")
            .collect();

        if !all_ops.is_empty() {
            self.save_sync_operations(all_ops)?;
        }

        Ok(detection_result)
    }
}

/// 表信息
#[derive(Debug, Clone)]
pub struct TableInfo {
    pub id: String,
    pub name: String,
    pub schema_name: String,
    pub comment: Option<String>,
    pub last_sync: i64,
}

/// 列信息
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub id: String,
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary: bool,
    pub is_unique: bool,
    pub comment: Option<String>,
    pub last_sync: i64,
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

    #[test]
    fn test_metadata_cache_ops() -> Result<(), CoreError> {
        let db_path = test_temp_dir("ops").join("test_metadata.sqlite");

        let conn = Connection::open(&db_path)?;
        conn.execute("PRAGMA journal_mode=WAL", [])?;

        let ops = MetadataCacheOps::new(conn);

        let result = ops.list_tables("test_db", None);
        assert!(result.is_err());
        Ok(())
    }
}
