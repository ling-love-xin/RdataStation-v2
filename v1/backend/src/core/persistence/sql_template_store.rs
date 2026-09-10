//! SQL 模板存储模块
//!
//! 负责 SQL 模板的持久化存储和管理。
//! 使用系统 SQLite 数据库存储，支持模板的 CRUD 操作。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use crate::core::error::{CommonError, CoreError, StorageError};
use crate::core::persistence::global_db::GlobalSqlitePool;

/// SQL 模板
#[derive(Debug, Clone)]
pub struct SqlTemplate {
    /// 模板 ID（UUID）
    pub id: String,
    /// 模板名称
    pub name: String,
    /// 模板内容
    pub content: String,
    /// 数据库类型（mysql/postgresql/sqlite/duckdb，空表示通用）
    pub db_type: Option<String>,
    /// 模板分类
    pub category: String,
    /// 模板描述
    pub description: Option<String>,
    /// 标签列表（逗号分隔）
    pub tags: Option<String>,
    /// 是否内置模板
    pub is_builtin: bool,
    /// 创建时间（Unix 时间戳，毫秒）
    pub created_at_ms: u64,
    /// 更新时间（Unix 时间戳，毫秒）
    pub updated_at_ms: u64,
}

impl SqlTemplate {
    /// 创建新模板
    pub fn new(
        name: String,
        content: String,
        db_type: Option<String>,
        category: String,
        description: Option<String>,
        tags: Option<String>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            content,
            db_type,
            category,
            description,
            tags,
            is_builtin: false,
            created_at_ms: now,
            updated_at_ms: now,
        }
    }
}

/// SQL 模板存储服务
///
/// 持有连接池引用，每次操作时从池中获取连接，操作完成后归还
pub struct SqlTemplateStore {
    pool: Arc<GlobalSqlitePool>,
    initialized: bool,
}

impl SqlTemplateStore {
    /// 创建新的模板存储服务
    pub fn new(pool: Arc<GlobalSqlitePool>) -> Result<Self, CoreError> {
        let mut store = Self {
            pool,
            initialized: false,
        };
        store.init_table()?;
        store.seed_builtin_templates()?;
        store.initialized = true;
        Ok(store)
    }

    /// 获取连接（操作完成后会自动归还）
    fn with_connection<T, F: FnOnce(&Connection) -> Result<T, CoreError>>(
        &self,
        f: F,
    ) -> Result<T, CoreError> {
        let conn = self.pool.acquire_sync()?;
        f(conn.inner()?)
    }

    /// 初始化模板表
    fn init_table(&mut self) -> Result<(), CoreError> {
        let conn = self.pool.acquire_sync()?;
        conn.inner()?
            .execute(
                "CREATE TABLE IF NOT EXISTS sql_templates (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                content TEXT NOT NULL,
                db_type TEXT,
                category TEXT NOT NULL,
                description TEXT,
                tags TEXT,
                is_builtin INTEGER NOT NULL DEFAULT 0,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL
            )",
                [],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "init_sql_templates_table".to_string(),
                    reason: e.to_string(),
                })
            })?;
        Ok(())
    }

    /// 种子内置模板
    fn seed_builtin_templates(&mut self) -> Result<(), CoreError> {
        let builtin_templates = vec![
            SqlTemplate {
                id: "builtin_select_all".to_string(),
                name: "查询所有记录".to_string(),
                content: "SELECT * FROM {table};".to_string(),
                db_type: None,
                category: "查询".to_string(),
                description: Some("查询表中的所有记录".to_string()),
                tags: Some("查询,基础".to_string()),
                is_builtin: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            SqlTemplate {
                id: "builtin_count".to_string(),
                name: "统计记录数".to_string(),
                content: "SELECT COUNT(*) FROM {table};".to_string(),
                db_type: None,
                category: "查询".to_string(),
                description: Some("统计表中的记录总数".to_string()),
                tags: Some("统计,聚合".to_string()),
                is_builtin: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            SqlTemplate {
                id: "builtin_create_table".to_string(),
                name: "创建表".to_string(),
                content: "CREATE TABLE {table} (\n    id INTEGER PRIMARY KEY,\n    name TEXT NOT NULL,\n    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP\n);".to_string(),
                db_type: None,
                category: "DDL".to_string(),
                description: Some("创建新表的基础模板".to_string()),
                tags: Some("DDL,创建".to_string()),
                is_builtin: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            SqlTemplate {
                id: "builtin_insert".to_string(),
                name: "插入记录".to_string(),
                content: "INSERT INTO {table} (name, created_at) VALUES (?, CURRENT_TIMESTAMP);".to_string(),
                db_type: None,
                category: "DML".to_string(),
                description: Some("插入新记录的基础模板".to_string()),
                tags: Some("DML,插入".to_string()),
                is_builtin: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            SqlTemplate {
                id: "builtin_update".to_string(),
                name: "更新记录".to_string(),
                content: "UPDATE {table} SET name = ? WHERE id = ?;".to_string(),
                db_type: None,
                category: "DML".to_string(),
                description: Some("更新记录的基础模板".to_string()),
                tags: Some("DML,更新".to_string()),
                is_builtin: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
            SqlTemplate {
                id: "builtin_delete".to_string(),
                name: "删除记录".to_string(),
                content: "DELETE FROM {table} WHERE id = ?;".to_string(),
                db_type: None,
                category: "DML".to_string(),
                description: Some("删除记录的基础模板".to_string()),
                tags: Some("DML,删除".to_string()),
                is_builtin: true,
                created_at_ms: 0,
                updated_at_ms: 0,
            },
        ];

        let conn = self.pool.acquire_sync()?;
        for template in &builtin_templates {
            conn.inner()?.execute(
                "INSERT OR IGNORE INTO sql_templates
                 (id, name, content, db_type, category, description, tags, is_builtin, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    template.id,
                    template.name,
                    template.content,
                    template.db_type,
                    template.category,
                    template.description,
                    template.tags,
                    template.is_builtin as i32,
                    template.created_at_ms,
                    template.updated_at_ms,
                ],
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "seed_builtin_template".to_string(),
                reason: e.to_string(),
            }))?;
        }

        Ok(())
    }

    /// 保存模板
    pub fn save(&self, template: &SqlTemplate) -> Result<(), CoreError> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO sql_templates
                 (id, name, content, db_type, category, description, tags, is_builtin, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    template.id,
                    template.name,
                    template.content,
                    template.db_type,
                    template.category,
                    template.description,
                    template.tags,
                    template.is_builtin as i32,
                    template.created_at_ms,
                    template.updated_at_ms,
                ],
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_sql_template".to_string(),
                reason: e.to_string(),
            }))?;
            Ok(())
        })
    }

    /// 根据 ID 获取模板
    pub fn get_by_id(&self, id: &str) -> Result<Option<SqlTemplate>, CoreError> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, content, db_type, category, description, tags, is_builtin, created_at_ms, updated_at_ms
                 FROM sql_templates WHERE id = ?1"
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_get_template".to_string(),
                reason: e.to_string(),
            }))?;

            let template = stmt.query_row(params![id], |row| {
                Ok(SqlTemplate {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    content: row.get(2)?,
                    db_type: row.get(3)?,
                    category: row.get(4)?,
                    description: row.get(5)?,
                    tags: row.get(6)?,
                    is_builtin: row.get::<_, i32>(7)? != 0,
                    created_at_ms: row.get(8)?,
                    updated_at_ms: row.get(9)?,
                })
            }).optional().map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_template_by_id".to_string(),
                reason: e.to_string(),
            }))?;

            Ok(template)
        })
    }

    /// 获取所有模板
    pub fn get_all(&self) -> Result<Vec<SqlTemplate>, CoreError> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, content, db_type, category, description, tags, is_builtin, created_at_ms, updated_at_ms
                 FROM sql_templates ORDER BY is_builtin DESC, updated_at_ms DESC"
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_get_all_templates".to_string(),
                reason: e.to_string(),
            }))?;

            let templates = stmt.query_map([], |row| {
                Ok(SqlTemplate {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    content: row.get(2)?,
                    db_type: row.get(3)?,
                    category: row.get(4)?,
                    description: row.get(5)?,
                    tags: row.get(6)?,
                    is_builtin: row.get::<_, i32>(7)? != 0,
                    created_at_ms: row.get(8)?,
                    updated_at_ms: row.get(9)?,
                })
            }).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_all_templates".to_string(),
                reason: e.to_string(),
            }))?.filter_map(|r| r.ok()).collect();

            Ok(templates)
        })
    }

    /// 根据分类获取模板
    pub fn get_by_category(&self, category: &str) -> Result<Vec<SqlTemplate>, CoreError> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, content, db_type, category, description, tags, is_builtin, created_at_ms, updated_at_ms
                 FROM sql_templates WHERE category = ?1 ORDER BY is_builtin DESC, updated_at_ms DESC"
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_get_templates_by_category".to_string(),
                reason: e.to_string(),
            }))?;

            let templates = stmt.query_map(params![category], |row| {
                Ok(SqlTemplate {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    content: row.get(2)?,
                    db_type: row.get(3)?,
                    category: row.get(4)?,
                    description: row.get(5)?,
                    tags: row.get(6)?,
                    is_builtin: row.get::<_, i32>(7)? != 0,
                    created_at_ms: row.get(8)?,
                    updated_at_ms: row.get(9)?,
                })
            }).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_templates_by_category".to_string(),
                reason: e.to_string(),
            }))?.filter_map(|r| r.ok()).collect();

            Ok(templates)
        })
    }

    /// 根据数据库类型获取模板
    pub fn get_by_db_type(&self, db_type: &str) -> Result<Vec<SqlTemplate>, CoreError> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, content, db_type, category, description, tags, is_builtin, created_at_ms, updated_at_ms
                 FROM sql_templates WHERE db_type = ?1 OR db_type IS NULL ORDER BY is_builtin DESC, updated_at_ms DESC"
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_get_templates_by_db_type".to_string(),
                reason: e.to_string(),
            }))?;

            let templates = stmt.query_map(params![db_type], |row| {
                Ok(SqlTemplate {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    content: row.get(2)?,
                    db_type: row.get(3)?,
                    category: row.get(4)?,
                    description: row.get(5)?,
                    tags: row.get(6)?,
                    is_builtin: row.get::<_, i32>(7)? != 0,
                    created_at_ms: row.get(8)?,
                    updated_at_ms: row.get(9)?,
                })
            }).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "get_templates_by_db_type".to_string(),
                reason: e.to_string(),
            }))?.filter_map(|r| r.ok()).collect();

            Ok(templates)
        })
    }

    /// 删除模板（仅用户自定义模板）
    pub fn delete(&self, id: &str) -> Result<bool, CoreError> {
        self.with_connection(|conn| {
            let is_builtin = conn
                .query_row(
                    "SELECT is_builtin FROM sql_templates WHERE id = ?1",
                    params![id],
                    |row| row.get::<_, i32>(0),
                )
                .optional()
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "check_builtin_template".to_string(),
                        reason: e.to_string(),
                    })
                })?
                .unwrap_or(1);

            if is_builtin != 0 {
                return Err(CoreError::common(CommonError::NotSupported(
                    "Cannot delete builtin template".to_string(),
                )));
            }

            let rows = conn
                .execute(
                    "DELETE FROM sql_templates WHERE id = ?1 AND is_builtin = 0",
                    params![id],
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "delete_sql_template".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            Ok(rows > 0)
        })
    }

    /// 获取所有分类
    pub fn get_categories(&self) -> Result<Vec<String>, CoreError> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare("SELECT DISTINCT category FROM sql_templates ORDER BY category")
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "prepare_get_categories".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            let categories = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "get_categories".to_string(),
                        reason: e.to_string(),
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();

            Ok(categories)
        })
    }
}
