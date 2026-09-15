//! 工作台上下文持久化模块
//!
//! 负责工作台布局、面板状态、编辑器内容等上下文信息的持久化

use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};

use crate::persistence::global_db::GlobalSqlitePool;
use shared::error::{CoreError, StorageError};

/// 工作台面板类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelType {
    Navigator,
    Editor,
    Result,
    Output,
    Properties,
}

impl std::fmt::Display for PanelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PanelType::Navigator => write!(f, "navigator"),
            PanelType::Editor => write!(f, "editor"),
            PanelType::Result => write!(f, "result"),
            PanelType::Output => write!(f, "output"),
            PanelType::Properties => write!(f, "properties"),
        }
    }
}

impl std::str::FromStr for PanelType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "navigator" => Ok(PanelType::Navigator),
            "editor" => Ok(PanelType::Editor),
            "result" => Ok(PanelType::Result),
            "output" => Ok(PanelType::Output),
            "properties" => Ok(PanelType::Properties),
            _ => Err(format!("Unknown panel type: {}", s)),
        }
    }
}

/// 工作台布局状态
#[derive(Debug, Clone)]
pub struct WorkbenchLayout {
    /// 布局 ID
    pub id: String,
    /// 连接 ID
    pub connection_id: String,
    /// 面板布局配置（JSON）
    pub panel_config: String,
    /// 活动面板
    pub active_panel: String,
    /// 侧边栏是否可见
    pub sidebar_visible: bool,
    /// 底栏是否可见
    pub bottom_bar_visible: bool,
    /// 更新时间
    pub updated_at_ms: u64,
}

/// 编辑器上下文
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorContext {
    /// 编辑器 ID（有路径的文档用路径键，见 `editor::persist::session_id_for_path`）
    pub id: String,
    /// 连接 ID
    pub connection_id: String,
    /// 编辑器模式（`text` / `sql` / `analysis`；旧库补列时默认 `sql`）
    ///
    /// 模式是**文档属性**（见编辑器架构 §2），重启后要不丢，就得和光标一起落库。
    pub mode: String,
    /// 编辑器内容
    pub content: String,
    /// 光标位置
    pub cursor_position: usize,
    /// 选中范围
    pub selection_start: Option<usize>,
    pub selection_end: Option<usize>,
    /// 更新时间
    pub updated_at_ms: u64,
}

/// 工作台上下文存储服务
///
/// 持有连接池引用，每次操作时从池中获取连接，操作完成后归还
pub struct WorkbenchContextStore {
    pool: Arc<GlobalSqlitePool>,
}

impl WorkbenchContextStore {
    /// 创建新的工作台上下文存储服务
    pub fn new(pool: Arc<GlobalSqlitePool>) -> Result<Self, CoreError> {
        let store = Self { pool };
        store.init_tables()?;
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

    /// 初始化表
    fn init_tables(&self) -> Result<(), CoreError> {
        self.with_connection(|conn| {
            conn.execute(
                "CREATE TABLE IF NOT EXISTS workbench_layouts (
                    id TEXT PRIMARY KEY,
                    connection_id TEXT NOT NULL,
                    panel_config TEXT NOT NULL,
                    active_panel TEXT NOT NULL,
                    sidebar_visible INTEGER NOT NULL DEFAULT 1,
                    bottom_bar_visible INTEGER NOT NULL DEFAULT 1,
                    updated_at_ms INTEGER NOT NULL
                )",
                [],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "init_workbench_layouts_table".to_string(),
                    reason: e.to_string(),
                })
            })?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS editor_contexts (
                    id TEXT PRIMARY KEY,
                    connection_id TEXT NOT NULL,
                    mode TEXT NOT NULL DEFAULT 'sql',
                    content TEXT NOT NULL,
                    cursor_position INTEGER NOT NULL DEFAULT 0,
                    selection_start INTEGER,
                    selection_end INTEGER,
                    updated_at_ms INTEGER NOT NULL
                )",
                [],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "init_editor_contexts_table".to_string(),
                    reason: e.to_string(),
                })
            })?;

            // `CREATE TABLE IF NOT EXISTS` 不会给**老库**补列，这里幂等补上（同
            // `connection_org_store::ensure_member_primary_column` 的办法：先 PRAGMA 再决定）。
            ensure_column(
                conn,
                "editor_contexts",
                "mode",
                "TEXT NOT NULL DEFAULT 'sql'",
            )?;

            Ok(())
        })
    }

    /// 保存工作台布局
    pub fn save_layout(&self, layout: &WorkbenchLayout) -> Result<(), CoreError> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO workbench_layouts
                 (id, connection_id, panel_config, active_panel, sidebar_visible, bottom_bar_visible, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    layout.id,
                    layout.connection_id,
                    layout.panel_config,
                    layout.active_panel,
                    layout.sidebar_visible as i32,
                    layout.bottom_bar_visible as i32,
                    layout.updated_at_ms as i64,
                ],
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_workbench_layout".to_string(),
                reason: e.to_string(),
            }))?;

            Ok(())
        })
    }

    /// 加载工作台布局
    pub fn load_layout(&self, connection_id: &str) -> Result<Option<WorkbenchLayout>, CoreError> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, connection_id, panel_config, active_panel, sidebar_visible, bottom_bar_visible, updated_at_ms
                 FROM workbench_layouts WHERE connection_id = ?1"
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_load_workbench_layout".to_string(),
                reason: e.to_string(),
            }))?;

            let layout = stmt.query_row(params![connection_id], |row| {
                Ok(WorkbenchLayout {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    panel_config: row.get(2)?,
                    active_panel: row.get(3)?,
                    sidebar_visible: row.get::<_, i32>(4)? != 0,
                    bottom_bar_visible: row.get::<_, i32>(5)? != 0,
                    updated_at_ms: row.get::<_, i64>(6)? as u64,
                })
            }).optional().map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "load_workbench_layout".to_string(),
                reason: e.to_string(),
            }))?;

            Ok(layout)
        })
    }

    /// 保存编辑器上下文
    pub fn save_editor_context(&self, context: &EditorContext) -> Result<(), CoreError> {
        self.with_connection(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO editor_contexts
                 (id, connection_id, mode, content, cursor_position, selection_start, selection_end, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    context.id,
                    context.connection_id,
                    context.mode,
                    context.content,
                    context.cursor_position as i64,
                    context.selection_start.map(|v| v as i64),
                    context.selection_end.map(|v| v as i64),
                    context.updated_at_ms as i64,
                ],
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "save_editor_context".to_string(),
                reason: e.to_string(),
            }))?;

            Ok(())
        })
    }

    /// 加载编辑器上下文
    pub fn load_editor_context(&self, editor_id: &str) -> Result<Option<EditorContext>, CoreError> {
        self.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, connection_id, mode, content, cursor_position, selection_start, selection_end, updated_at_ms
                 FROM editor_contexts WHERE id = ?1"
            ).map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "prepare_load_editor_context".to_string(),
                reason: e.to_string(),
            }))?;

            let context = stmt.query_row(params![editor_id], |row| {
                Ok(EditorContext {
                    id: row.get(0)?,
                    connection_id: row.get(1)?,
                    mode: row.get(2)?,
                    content: row.get(3)?,
                    cursor_position: row.get::<_, i64>(4)? as usize,
                    selection_start: row.get::<_, Option<i64>>(5)?.map(|v| v as usize),
                    selection_end: row.get::<_, Option<i64>>(6)?.map(|v| v as usize),
                    updated_at_ms: row.get::<_, i64>(7)? as u64,
                })
            }).optional().map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "load_editor_context".to_string(),
                reason: e.to_string(),
            }))?;

            Ok(context)
        })
    }

    /// 加载**最近更新**的一份编辑器上下文（启动恢复用）
    ///
    /// 只取一条：1a 的恢复范围是“上次编辑的那份文档”（多文档恢复要等 layout 表配合）。
    /// 没有记录时返回 `None`。
    pub fn load_latest_editor_context(&self) -> Result<Option<EditorContext>, CoreError> {
        self.with_connection(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, connection_id, mode, content, cursor_position, selection_start, selection_end, updated_at_ms
                     FROM editor_contexts ORDER BY updated_at_ms DESC LIMIT 1",
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "prepare_load_latest_editor_context".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            let context = stmt
                .query_row([], |row| {
                    Ok(EditorContext {
                        id: row.get(0)?,
                        connection_id: row.get(1)?,
                        mode: row.get(2)?,
                        content: row.get(3)?,
                        cursor_position: row.get::<_, i64>(4)? as usize,
                        selection_start: row.get::<_, Option<i64>>(5)?.map(|v| v as usize),
                        selection_end: row.get::<_, Option<i64>>(6)?.map(|v| v as usize),
                        updated_at_ms: row.get::<_, i64>(7)? as u64,
                    })
                })
                .optional()
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "load_latest_editor_context".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            Ok(context)
        })
    }

    /// 删除连接相关的所有上下文
    pub fn delete_connection_contexts(&self, connection_id: &str) -> Result<usize, CoreError> {
        self.with_connection(|conn| {
            let layout_deleted = conn
                .execute(
                    "DELETE FROM workbench_layouts WHERE connection_id = ?1",
                    params![connection_id],
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "delete_workbench_layout".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            let editor_deleted = conn
                .execute(
                    "DELETE FROM editor_contexts WHERE connection_id = ?1",
                    params![connection_id],
                )
                .map_err(|e| {
                    CoreError::storage(StorageError::Persistence {
                        store: "sqlite".to_string(),
                        operation: "delete_editor_context".to_string(),
                        reason: e.to_string(),
                    })
                })?;

            Ok(layout_deleted + editor_deleted)
        })
    }
}

/// 幂等补列：`CREATE TABLE IF NOT EXISTS` 不会给老表加字段，SQLite 也没有
/// `ADD COLUMN IF NOT EXISTS`，所以先查 `PRAGMA table_info` 再决定。
///
/// 与 `connection_org_store::ensure_member_primary_column` 同一手法（项目里已有一处先例）。
fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), CoreError> {
    let present = {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: format!("pragma_{table}"),
                reason: e.to_string(),
            })
        })?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: format!("pragma_{table}_rows"),
                reason: e.to_string(),
            }))?;
        names.filter_map(Result::ok).any(|name| name == column)
    };

    if !present {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: format!("alter_{table}_{column}"),
                reason: e.to_string(),
            })
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::{EditorContext, WorkbenchContextStore};
    use crate::persistence::global_db::GlobalSqlitePool;

    fn temp_db(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "rds_wb_context_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db = dir.join("global.db");
        (dir, db)
    }

    /// 建池 + 开店（**建池在自建 runtime 里，开店在离开 async 上下文之后**）
    ///
    /// 存储层 API 是同步的（`acquire_sync` 自建 runtime），所以调用线程**不能**是某个
    /// runtime 的驱动线程：在 tokio 上下文里调它只会拿到一条可读错误。生产里的调用点
    /// 正是这样——GPUI 主线程（没有 runtime）。
    fn open_store(db: &PathBuf) -> WorkbenchContextStore {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let pool = runtime.block_on(async {
            Arc::new(
                GlobalSqlitePool::new(db.clone(), 1)
                    .await
                    .expect("create pool"),
            )
        });
        drop(runtime);
        WorkbenchContextStore::new(pool).expect("open store")
    }

    fn context(id: &str, mode: &str) -> EditorContext {
        EditorContext {
            id: id.to_string(),
            connection_id: "conn-1".to_string(),
            mode: mode.to_string(),
            content: "select 1;".to_string(),
            cursor_position: 4,
            selection_start: Some(1),
            selection_end: Some(3),
            updated_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn editor_context_round_trips_with_its_mode() {
        let (dir, db) = temp_db("roundtrip");
        let store = open_store(&db);

        store.save_editor_context(&context("doc.sql", "analysis")).expect("save");
        let loaded = store
            .load_editor_context("doc.sql")
            .expect("load")
            .expect("存在");

        assert_eq!(loaded.mode, "analysis", "模式必须一起持久化");
        assert_eq!(loaded.content, "select 1;");
        assert_eq!(loaded.cursor_position, 4);
        assert_eq!(loaded.selection_start, Some(1));
        assert_eq!(loaded.selection_end, Some(3));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn saving_again_replaces_the_same_editor_row() {
        let (dir, db) = temp_db("replace");
        let store = open_store(&db);

        store.save_editor_context(&context("doc.sql", "sql")).expect("save");
        let mut updated = context("doc.sql", "text");
        updated.content = "select 2;".to_string();
        store.save_editor_context(&updated).expect("save again");

        let loaded = store.load_editor_context("doc.sql").expect("load").expect("存在");
        assert_eq!(loaded.mode, "text");
        assert_eq!(loaded.content, "select 2;");
        assert_eq!(store.load_editor_context("nope.sql").expect("load"), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_latest_session_is_the_one_that_was_saved_last() {
        let (dir, db) = temp_db("latest");
        let store = open_store(&db);
        assert!(
            store.load_latest_editor_context().expect("load").is_none(),
            "空库没有可恢复的会话"
        );

        let mut older = context("older.sql", "sql");
        older.updated_at_ms = 1_000;
        let mut newer = context("newer.sql", "text");
        newer.updated_at_ms = 2_000;
        store.save_editor_context(&older).expect("save older");
        store.save_editor_context(&newer).expect("save newer");

        let latest = store
            .load_latest_editor_context()
            .expect("load")
            .expect("有记录");
        assert_eq!(latest.id, "newer.sql", "应当恢复最近更新的那份");
        assert_eq!(latest.mode, "text");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_old_table_gets_the_mode_column_added() {
        let (dir, db) = temp_db("upgrade");
        // 先造一张“老库”的 editor_contexts（没有 mode 列）
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let pool = runtime.block_on(async {
            let pool = Arc::new(
                GlobalSqlitePool::new(db.clone(), 1)
                    .await
                    .expect("create pool"),
            );
            {
                let conn = pool.acquire().await.expect("acquire");
                conn.inner()
                    .expect("inner")
                    .execute_batch(
                        "CREATE TABLE editor_contexts (
                            id TEXT PRIMARY KEY,
                            connection_id TEXT NOT NULL,
                            content TEXT NOT NULL,
                            cursor_position INTEGER NOT NULL DEFAULT 0,
                            selection_start INTEGER,
                            selection_end INTEGER,
                            updated_at_ms INTEGER NOT NULL
                        );
                        INSERT INTO editor_contexts (id, connection_id, content, cursor_position, updated_at_ms)
                        VALUES ('legacy.sql', 'conn-1', 'select 9;', 0, 1);",
                    )
                    .expect("seed legacy table");
            }
            pool
        });
        drop(runtime);

        // 打开 store 会补列（幂等）
        let store = WorkbenchContextStore::new(pool).expect("open store");
        let loaded = store
            .load_editor_context("legacy.sql")
            .expect("load")
            .expect("老行还在");
        assert_eq!(loaded.mode, "sql", "补列后旧行取默认模式");
        assert_eq!(loaded.content, "select 9;");

        // 再开一次也不该报错（幂等：列已存在）
        let store_again = open_store(&db);
        assert!(
            store_again.load_editor_context("legacy.sql").expect("load").is_some(),
            "重复打开不会因补列冲突而失败"
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
