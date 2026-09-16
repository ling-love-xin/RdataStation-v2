//! 数据源导航**视图状态**持久化（展开 / 选中 / 过滤）。
//!
//! - 项目级 → `{project}/.RSmeta/project.db`
//! - 全局级 → `{system}/global.db`
//!
//! 只负责 `navigator_state` 表；连接的组织元数据（标签 / 分组 / 排序）归
//! [`super::connection_org_store`]。打开时确保表存在（DDL 与 engine 迁移
//! `global/018`、`project_meta/017` 一致），不依赖迁移执行顺序。
//! 状态与缓存一样**不自动删除**。
//!
//! 为什么住在 engine 而不是导航视图侧：它是**纯持久化**（读写一张表），
//! 而 [`NavState`] 是行映射类型；视图下沉到 `database` 后，视图侧不应为了存状态
//! 再引一份 `rusqlite`（engine 已有）。原先在 `workbench::services::nav_store`。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use shared::error::{CoreError, StorageError};

/// 导航视图状态（展开节点 / 选中节点 / 搜索词 + 格式版本）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavState {
    /// 已展开节点 key
    pub expanded_keys: Vec<String>,
    /// 选中节点 key
    pub selected_key: Option<String>,
    /// 搜索过滤词
    pub filter_text: String,
    /// 格式版本（便于后续迁移）
    pub version: u32,
}

impl Default for NavState {
    fn default() -> Self {
        Self {
            expanded_keys: Vec::new(),
            selected_key: None,
            filter_text: String::new(),
            version: 1,
        }
    }
}

/// 导航状态存储（持有一个 SQLite 连接）。
pub struct NavigatorStateStore {
    conn: Connection,
}

impl NavigatorStateStore {
    /// 打开指定库并确保 `navigator_state` 存在（幂等，兼容未执行迁移的旧库）。
    ///
    /// `open_global` / `open_project` 之外的路径入口（多环境 / 测试注入）。
    pub fn open_at(path: PathBuf) -> Result<Self, CoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "create_navigator_state_dir".to_string(),
                    reason: e.to_string(),
                })
            })?;
        }
        let conn = Connection::open(&path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "open_navigator_state".to_string(),
                reason: e.to_string(),
            })
        })?;
        let store = Self { conn };
        store.ensure_tables()?;
        Ok(store)
    }

    /// 全局库（`{system}/global.db`）。
    pub fn open_global() -> Result<Self, CoreError> {
        let path = crate::migration::get_global_db_path()?;
        Self::open_at(path)
    }

    /// 项目库（`{project}/.RSmeta/project.db`）。
    ///
    /// 目录名与 `project::store::RS_META_DIR_NAME` 保持一致（历史误写为 `.RSMETA`）。
    pub fn open_project(root: &Path) -> Result<Self, CoreError> {
        Self::open_at(root.join(".RSmeta").join("project.db"))
    }

    fn ensure_tables(&self) -> Result<(), CoreError> {
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS navigator_state (
                    conn_id       TEXT PRIMARY KEY,
                    scope         TEXT NOT NULL DEFAULT 'global',
                    expanded_keys TEXT NOT NULL DEFAULT '[]',
                    selected_key  TEXT,
                    filter_text   TEXT NOT NULL DEFAULT '',
                    version       INTEGER NOT NULL DEFAULT 1,
                    updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )",
                [],
            )
            .map_err(|e| self.err("ensure_navigator_state", e))?;
        Ok(())
    }

    /// 读取导航状态（缺失返回默认）。
    pub fn load_state(&self, conn_id: &str) -> NavState {
        let row: Result<Option<(String, Option<String>, String, i64)>, _> = self
            .conn
            .query_row(
                "SELECT expanded_keys, selected_key, filter_text, version
                 FROM navigator_state WHERE conn_id = ?1",
                params![conn_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional();
        match row {
            Ok(Some((expanded, selected_key, filter_text, version))) => NavState {
                expanded_keys: serde_json::from_str(&expanded).unwrap_or_default(),
                selected_key,
                filter_text,
                version: version as u32,
            },
            _ => NavState::default(),
        }
    }

    /// 保存导航状态。
    pub fn save_state(&self, conn_id: &str, scope: &str, state: &NavState) -> Result<(), CoreError> {
        let expanded = serde_json::to_string(&state.expanded_keys).unwrap_or_else(|_| "[]".into());
        self.conn
            .execute(
                "INSERT OR REPLACE INTO navigator_state
                 (conn_id, scope, expanded_keys, selected_key, filter_text, version, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, CURRENT_TIMESTAMP)",
                params![
                    conn_id,
                    scope,
                    expanded,
                    state.selected_key,
                    state.filter_text,
                    state.version as i64
                ],
            )
            .map_err(|e| self.err("save_navigator_state", e))?;
        Ok(())
    }

    fn err(&self, operation: &str, e: rusqlite::Error) -> CoreError {
        CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: operation.to_string(),
            reason: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rds_navstate_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    #[test]
    fn project_state_roundtrip() {
        let dir = temp_dir("state");
        let store = NavigatorStateStore::open_at(dir.join("p.db")).expect("open");
        let state = NavState {
            expanded_keys: vec!["P_a/db".into(), "P_a/db/public".into()],
            selected_key: Some("P_a/db/public".into()),
            filter_text: "order".into(),
            version: 1,
        };
        store.save_state("P_a", "project", &state).expect("save");
        let loaded = store.load_state("P_a");
        assert_eq!(loaded.expanded_keys, state.expanded_keys);
        assert_eq!(loaded.selected_key.as_deref(), Some("P_a/db/public"));
        assert_eq!(loaded.filter_text, "order");
        // 缺失连接返回默认
        assert!(store.load_state("P_missing").expanded_keys.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
