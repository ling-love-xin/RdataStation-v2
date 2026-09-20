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
//! ## 一张表里的两类行
//!
//! - **按连接**（`conn_id` = 真实连接 id）：`expanded_keys`——展开态属于连接（展开的是
//!   它自己的子树），一行一连接；
//! - **面板级**（`conn_id` = [`PANEL_STATE_CONN_ID`]）：`selected_key` / `filter_text`——
//!   选中与搜索框都是**面板级唯一**的（v5 把「来源标签页」合并成一棵分组树后，一个搜索框
//!   管所有连接、同一时刻只有一个选中行），按连接存会互相覆盖、也不知道该听谁的；
//!   滚动位置不存偏移，由**选中锚点**在视图侧恢复（行集合是按需懒加载的，偏移对不上）。
//!
//! 为何住在 engine 而不是导航视图侧：它是**纯持久化**（读写一张表），
//! 而 [`NavState`] 是行映射类型；视图下沉到 `database` 后，视图侧不应为了存状态
//! 再引一份 `rusqlite`（engine 已有）。原先在 `workbench::services::nav_store`。

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use shared::error::{CoreError, StorageError};

/// 导航状态里**面板级**那一行保留的 `conn_id`（选中 / 搜索词）。
///
/// 它不是连接 id（真实 id 有 `G_` / `P_` / `GP_` / 遗留 `conn-` 前缀），不会撞车；
/// 与 `UNGROUPED_SCOPE` 同一类「保留键」做法：表结构不变，语义由常量指名。
pub const PANEL_STATE_CONN_ID: &str = "__panel__";

/// 导航视图状态（展开节点 / 选中节点 / 搜索词 + 格式版本）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavState {
    /// 已展开节点 key
    pub expanded_keys: Vec<String>,
    /// 选中节点 key（面板级行用它；按连接的行留空）
    pub selected_key: Option<String>,
    /// 搜索过滤词（面板级行用它；按连接的行留空——搜索框是面板级的）
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
    pub fn save_state(
        &self,
        conn_id: &str,
        scope: &str,
        state: &NavState,
    ) -> Result<(), CoreError> {
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

    /// 面板级保留行与按连接的行**互不干扰**（各自一行，字段各取所需）。
    #[test]
    fn panel_row_is_a_separate_row_from_connection_rows() {
        let dir = temp_dir("panel");
        let store = NavigatorStateStore::open_at(dir.join("p.db")).expect("open");
        store
            .save_state(
                "P_a",
                "project",
                &NavState {
                    expanded_keys: vec!["P_a/db".into()],
                    ..NavState::default()
                },
            )
            .expect("save conn");
        store
            .save_state(
                PANEL_STATE_CONN_ID,
                "project",
                &NavState {
                    selected_key: Some("P_a/db/public/orders".into()),
                    filter_text: "ord".into(),
                    ..NavState::default()
                },
            )
            .expect("save panel");

        let panel = store.load_state(PANEL_STATE_CONN_ID);
        assert_eq!(panel.selected_key.as_deref(), Some("P_a/db/public/orders"));
        assert_eq!(panel.filter_text, "ord");
        assert!(panel.expanded_keys.is_empty(), "面板级行不填展开态");

        let conn = store.load_state("P_a");
        assert_eq!(conn.expanded_keys, vec!["P_a/db".to_string()]);
        assert!(conn.selected_key.is_none(), "按连接的行不填选中");
        assert!(conn.filter_text.is_empty(), "按连接的行不填搜索词");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
