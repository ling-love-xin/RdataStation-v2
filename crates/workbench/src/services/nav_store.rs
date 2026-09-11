//! 数据源导航持久化（Phase B）：导航状态 / 分组 / 标签。
//!
//! - 项目级 → `{project}/.RSMETA/project.db`
//! - 全局级 → `{system}/global.db`
//!
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

    fn ensure_tables(&self, is_project: bool) -> Result<(), String> {
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
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS connection_tags (
                    connection_id TEXT NOT NULL,
                    tag           TEXT NOT NULL,
                    created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    PRIMARY KEY (connection_id, tag)
                )",
                [],
            )
            .map_err(|e| e.to_string())?;
        if is_project {
            self.conn
                .execute(
                    "CREATE TABLE IF NOT EXISTS connection_groups (
                        id          TEXT PRIMARY KEY,
                        name        TEXT NOT NULL,
                        description TEXT,
                        sort_order  INTEGER NOT NULL DEFAULT 0,
                        created_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                        updated_at  TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                    )",
                    [],
                )
                .map_err(|e| e.to_string())?;
            self.conn
                .execute(
                    "CREATE TABLE IF NOT EXISTS connection_group_members (
                        group_id      TEXT NOT NULL,
                        connection_id TEXT NOT NULL,
                        sort_order    INTEGER NOT NULL DEFAULT 0,
                        PRIMARY KEY (group_id, connection_id)
                    )",
                    [],
                )
                .map_err(|e| e.to_string())?;
        }
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

    /// 读取连接标签（多值）。
    pub fn list_tags(&self, conn_id: &str) -> Vec<String> {
        let mut stmt = match self
            .conn
            .prepare("SELECT tag FROM connection_tags WHERE connection_id = ?1 ORDER BY tag")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![conn_id], |r| r.get::<_, String>(0));
        match rows {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 覆盖式设置连接标签。
    pub fn set_tags(&self, conn_id: &str, tags: &[String]) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM connection_tags WHERE connection_id = ?1",
                params![conn_id],
            )
            .map_err(|e| e.to_string())?;
        for tag in tags {
            let tag = tag.trim();
            if tag.is_empty() {
                continue;
            }
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO connection_tags (connection_id, tag) VALUES (?1, ?2)",
                    params![conn_id, tag],
                )
                .map_err(|e| e.to_string())?;
        }
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

    #[test]
    fn tags_roundtrip() {
        let dir = temp_dir("tags");
        let store = NavStore::open(dir.join("p.db"), true).expect("open");
        store
            .set_tags("P_a", &["prod".into(), "core".into(), "".into()])
            .expect("set");
        let tags = store.list_tags("P_a");
        assert_eq!(tags, vec!["core".to_string(), "prod".to_string()]);
        store.set_tags("P_a", &["dev".into()]).expect("reset");
        assert_eq!(store.list_tags("P_a"), vec!["dev".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
