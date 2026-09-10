//! 工作台上下文持久化模块
//!
//! 负责工作台布局、面板状态、编辑器内容等上下文信息的持久化

use std::sync::Arc;

use rusqlite::{params, Connection, OptionalExtension};

use crate::core::error::{CoreError, StorageError};
use crate::core::persistence::global_db::GlobalSqlitePool;

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
#[derive(Debug, Clone)]
pub struct EditorContext {
    /// 编辑器 ID
    pub id: String,
    /// 连接 ID
    pub connection_id: String,
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
                    layout.updated_at_ms,
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
                    updated_at_ms: row.get(6)?,
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
                 (id, connection_id, content, cursor_position, selection_start, selection_end, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    context.id,
                    context.connection_id,
                    context.content,
                    context.cursor_position,
                    context.selection_start,
                    context.selection_end,
                    context.updated_at_ms,
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
                "SELECT id, connection_id, content, cursor_position, selection_start, selection_end, updated_at_ms
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
                    content: row.get(2)?,
                    cursor_position: row.get(3)?,
                    selection_start: row.get(4)?,
                    selection_end: row.get(5)?,
                    updated_at_ms: row.get(6)?,
                })
            }).optional().map_err(|e| CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "load_editor_context".to_string(),
                reason: e.to_string(),
            }))?;

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
