//! 数据源导航视图状态持久化（Phase B）：展开 / 选中 / 过滤。
//!
//! - 项目级 → `{project}/.RSMETA/project.db`
//! - 全局级 → `{system}/global.db`
//!
//! 仅负责**视图状态**（`navigator_state`）；连接的组织元数据（标签 / 分组）
//! 归连接域存储 `engine::persistence::ConnectionOrgStore`。
//! 打开时确保表存在（与 engine 迁移 `global/018`、`project_meta/017` 的 DDL 一致），
//! 不依赖迁移执行顺序。状态与缓存一样**不自动删除**。

use std::path::{Path, PathBuf};

use database::model::NavState;
use rusqlite::{params, Connection, OptionalExtension};

/// 导航持久化存储（持有一个 SQLite 连接）。
pub struct NavStore {
    conn: Connection,
}

impl NavStore {
    fn open(path: PathBuf, is_project: bool) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建导航库目录失败: {e}"))?;
        }
        let conn = Connection::open(&path).map_err(|e| format!("打开导航库失败: {e}"))?;
        let store = Self { conn };
        store.ensure_tables(is_project)?;
        Ok(store)
    }

    /// 全局库（`{system}/global.db`）。
    pub fn open_global() -> Result<Self, String> {
        let path = engine::migration::get_global_db_path().map_err(|e| e.to_string())?;
        Self::open(path, false)
    }

    /// 项目库（`{project}/.RSMETA/project.db`）。
    pub fn open_project(root: &Path) -> Result<Self, String> {
        Self::open(root.join(".RSMETA").join("project.db"), true)
    }

    fn ensure_tables(&self, _is_project: bool) -> Result<(), String> {
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
            .map_err(|e| e.to_string())?;
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
    pub fn save_state(&self, conn_id: &str, scope: &str, state: &NavState) -> Result<(), String> {
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
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rds_navstore_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    #[test]
    fn project_state_roundtrip() {
        let dir = temp_dir("state");
        let store = NavStore::open(dir.join("p.db"), true).expect("open");
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
