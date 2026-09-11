//! 连接组织元数据存储（分组 / 标签）——M3 连接模块与 M4 导航模块共用。
//!
//! 定位：**连接的组织元数据**（不是导航视图状态）。表与迁移
//! `global/018_add_navigator_state.sql`、`project_meta/017_add_navigator_groups_tags_state.sql` 一致：
//! - `connection_tags`：连接 ↔ 标签（多值），随连接所在库（全局 / 项目）；
//! - `connection_groups` / `connection_group_members`：**项目级**分组（多对多 + 组内排序）。
//!
//! 与导航视图状态（`navigator_state`：展开/选中/过滤）分离——后者归导航模块私有。
//! 本存储同步（rusqlite），面向 UI 线程的轻量调用（单条 / 少量行）。
//!
//! 一致性约定：
//! - 删除连接时调用 [`ConnectionOrgStore::remove_connection`] 清理标签与分组成员；
//! - `DataSourceService` 保存 / 更新连接时把 tags 同步到 `connection_tags`（权威检索表）。

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use shared::error::{CoreError, StorageError};

/// 项目元数据目录名（与 `project_db` 保持一致）。
const RS_META_DIR_NAME: &str = ".RSMETA";
/// 项目库文件名。
const PROJECT_DB_NAME: &str = "project.db";

/// 连接分组（项目级）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionGroup {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// 连接组织元数据存储（持有一个 SQLite 连接）。
pub struct ConnectionOrgStore {
    conn: Connection,
    is_project: bool,
}

impl ConnectionOrgStore {
    /// 打开全局库（`{system}/global.db`）。
    pub fn open_global() -> Result<Self, CoreError> {
        let path = crate::migration::get_global_db_path()?;
        Self::open_at(path, false)
    }

    /// 打开项目库（`{project}/.RSMETA/project.db`）。
    pub fn open_project(root: &Path) -> Result<Self, CoreError> {
        Self::open_at(root.join(RS_META_DIR_NAME).join(PROJECT_DB_NAME), true)
    }

    /// 打开指定库并确保表结构存在（幂等，兼容未执行迁移的旧库）。
    ///
    /// `open_global` / `open_project` 之外的路径入口（多环境 / 测试注入）。
    pub fn open_at(path: PathBuf, is_project: bool) -> Result<Self, CoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "sqlite".to_string(),
                    operation: "create_org_store_dir".to_string(),
                    reason: e.to_string(),
                })
            })?;
        }
        let conn = Connection::open(&path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "sqlite".to_string(),
                operation: "open_org_store".to_string(),
                reason: e.to_string(),
            })
        })?;
        let store = Self { conn, is_project };
        store.ensure_tables()?;
        Ok(store)
    }

    fn ensure_tables(&self) -> Result<(), CoreError> {
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
            .map_err(|e| self.err("create_connection_tags", e))?;
        if self.is_project {
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
                .map_err(|e| self.err("create_connection_groups", e))?;
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
                .map_err(|e| self.err("create_connection_group_members", e))?;
        }
        Ok(())
    }

    fn err(&self, operation: &str, e: rusqlite::Error) -> CoreError {
        CoreError::storage(StorageError::Persistence {
            store: "sqlite".to_string(),
            operation: operation.to_string(),
            reason: e.to_string(),
        })
    }

    // ==================== 标签 ====================

    /// 读取连接标签（按字母序）。
    pub fn list_tags(&self, conn_id: &str) -> Vec<String> {
        let Ok(mut stmt) = self
            .conn
            .prepare("SELECT tag FROM connection_tags WHERE connection_id = ?1 ORDER BY tag")
        else {
            return Vec::new();
        };
        match stmt.query_map(params![conn_id], |r| r.get::<_, String>(0)) {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 覆盖式设置连接标签（去空白、去重）。
    pub fn set_tags(&self, conn_id: &str, tags: &[String]) -> Result<(), CoreError> {
        self.conn
            .execute(
                "DELETE FROM connection_tags WHERE connection_id = ?1",
                params![conn_id],
            )
            .map_err(|e| self.err("clear_connection_tags", e))?;
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
                .map_err(|e| self.err("insert_connection_tag", e))?;
        }
        Ok(())
    }

    /// 按标签检索连接 ID（跨连接，按连接 ID 排序）。
    pub fn list_connections_by_tag(&self, tag: &str) -> Vec<String> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT connection_id FROM connection_tags WHERE tag = ?1 ORDER BY connection_id",
        ) else {
            return Vec::new();
        };
        match stmt.query_map(params![tag], |r| r.get::<_, String>(0)) {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 全部标签及使用次数（按次数降序、名称升序）。
    pub fn list_all_tags(&self) -> Vec<(String, i64)> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT tag, COUNT(*) AS n FROM connection_tags GROUP BY tag ORDER BY n DESC, tag ASC",
        ) else {
            return Vec::new();
        };
        match stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    // ==================== 分组（项目级） ====================

    /// 新建分组。
    pub fn create_group(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
    ) -> Result<(), CoreError> {
        self.conn
            .execute(
                "INSERT INTO connection_groups (id, name, description) VALUES (?1, ?2, ?3)",
                params![id, name, description],
            )
            .map_err(|e| self.err("create_group", e))?;
        Ok(())
    }

    /// 更新分组（名称 / 描述 / 排序）。
    pub fn update_group(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        sort_order: i64,
    ) -> Result<(), CoreError> {
        self.conn
            .execute(
                "UPDATE connection_groups
                 SET name = ?2, description = ?3, sort_order = ?4, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?1",
                params![id, name, description, sort_order],
            )
            .map_err(|e| self.err("update_group", e))?;
        Ok(())
    }

    /// 删除分组（不删除成员连接，仅解除关系）。
    pub fn delete_group(&self, id: &str) -> Result<(), CoreError> {
        self.conn
            .execute(
                "DELETE FROM connection_group_members WHERE group_id = ?1",
                params![id],
            )
            .map_err(|e| self.err("delete_group_members", e))?;
        self.conn
            .execute("DELETE FROM connection_groups WHERE id = ?1", params![id])
            .map_err(|e| self.err("delete_group", e))?;
        Ok(())
    }

    /// 列出全部分组（手动排序优先，未排按名称）。
    pub fn list_groups(&self) -> Vec<ConnectionGroup> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT id, name, description, sort_order, created_at, updated_at
             FROM connection_groups ORDER BY sort_order ASC, name ASC",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map([], |r| {
            Ok(ConnectionGroup {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                sort_order: r.get(3)?,
                created_at: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                updated_at: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        });
        match rows {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 把连接加入分组（多对多；已存在则忽略）。
    pub fn add_member(&self, group_id: &str, conn_id: &str) -> Result<(), CoreError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO connection_group_members (group_id, connection_id) VALUES (?1, ?2)",
                params![group_id, conn_id],
            )
            .map_err(|e| self.err("add_group_member", e))?;
        Ok(())
    }

    /// 把连接移出分组。
    pub fn remove_member(&self, group_id: &str, conn_id: &str) -> Result<(), CoreError> {
        self.conn
            .execute(
                "DELETE FROM connection_group_members WHERE group_id = ?1 AND connection_id = ?2",
                params![group_id, conn_id],
            )
            .map_err(|e| self.err("remove_group_member", e))?;
        Ok(())
    }

    /// 设置成员在组内的排序。
    pub fn set_member_order(
        &self,
        group_id: &str,
        conn_id: &str,
        sort_order: i64,
    ) -> Result<(), CoreError> {
        self.conn
            .execute(
                "UPDATE connection_group_members SET sort_order = ?3
                 WHERE group_id = ?1 AND connection_id = ?2",
                params![group_id, conn_id, sort_order],
            )
            .map_err(|e| self.err("set_group_member_order", e))?;
        Ok(())
    }

    /// 分组内成员（手动排序优先，未排按连接 ID）。
    pub fn list_group_members(&self, group_id: &str) -> Vec<String> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT connection_id FROM connection_group_members
             WHERE group_id = ?1 ORDER BY sort_order ASC, connection_id ASC",
        ) else {
            return Vec::new();
        };
        match stmt.query_map(params![group_id], |r| r.get::<_, String>(0)) {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 连接所属的全部组（按组排序）。
    pub fn list_groups_for_connection(&self, conn_id: &str) -> Vec<String> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT m.group_id FROM connection_group_members m
             JOIN connection_groups g ON g.id = m.group_id
             WHERE m.connection_id = ?1 ORDER BY g.sort_order ASC, g.name ASC",
        ) else {
            return Vec::new();
        };
        match stmt.query_map(params![conn_id], |r| r.get::<_, String>(0)) {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 替换某连接的分组成员（先清空已有关系，再按传入顺序写入；空切片 = 移出全部分组）。
    ///
    /// 仅项目库有分组表；全局库调用为 no-op（分组是项目级能力）。
    /// 供对话框保存 / 更新后同步使用，保证 UI 勾选与库内一致。
    pub fn set_connection_groups(
        &self,
        conn_id: &str,
        group_ids: &[String],
    ) -> Result<(), CoreError> {
        if !self.is_project {
            return Ok(());
        }
        self.conn
            .execute(
                "DELETE FROM connection_group_members WHERE connection_id = ?1",
                params![conn_id],
            )
            .map_err(|e| self.err("clear_connection_groups", e))?;
        for (i, gid) in group_ids.iter().enumerate() {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO connection_group_members (group_id, connection_id, sort_order)
                     VALUES (?1, ?2, ?3)",
                    params![gid, conn_id, i as i64],
                )
                .map_err(|e| self.err("insert_connection_group_member", e))?;
        }
        Ok(())
    }

    // ==================== 一致性清理 ====================

    /// 删除连接时清理其组织关系（标签 + 分组成员）。
    ///
    /// 由连接删除路径（`DataSourceService::delete`）调用，避免孤儿数据。
    pub fn remove_connection(&self, conn_id: &str) -> Result<(), CoreError> {
        self.conn
            .execute(
                "DELETE FROM connection_tags WHERE connection_id = ?1",
                params![conn_id],
            )
            .map_err(|e| self.err("cleanup_connection_tags", e))?;
        if self.is_project {
            self.conn
                .execute(
                    "DELETE FROM connection_group_members WHERE connection_id = ?1",
                    params![conn_id],
                )
                .map_err(|e| self.err("cleanup_group_members", e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rds_org_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    #[test]
    fn tags_roundtrip_and_search() {
        let dir = temp_dir("tags");
        let store = ConnectionOrgStore::open_at(dir.join("p.db"), true).expect("open");
        store
            .set_tags("P_a", &["prod".into(), "core".into(), "".into(), "prod".into()])
            .expect("set");
        assert_eq!(store.list_tags("P_a"), vec!["core".to_string(), "prod".to_string()]);
        store.set_tags("P_b", &["prod".into()]).expect("set b");

        assert_eq!(store.list_connections_by_tag("prod"), vec!["P_a".to_string(), "P_b".to_string()]);
        assert_eq!(
            store.list_all_tags(),
            vec![("prod".to_string(), 2), ("core".to_string(), 1)]
        );

        // 覆盖式：清空后旧标签不再命中
        store.set_tags("P_b", &[]).expect("clear");
        assert_eq!(store.list_connections_by_tag("prod"), vec!["P_a".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_connection_groups_replaces_membership() {
        let dir = temp_dir("set_groups");
        let store = ConnectionOrgStore::open_at(dir.join("p.db"), true).expect("open");
        store.create_group("g1", "alpha", None).expect("g1");
        store.create_group("g2", "beta", None).expect("g2");

        // 替换语义：先写两个组，再只留 g2，最后清空。
        store
            .set_connection_groups("P_a", &["g1".into(), "g2".into()])
            .expect("set two");
        let mut got = store.list_groups_for_connection("P_a");
        got.sort();
        assert_eq!(got, vec!["g1".to_string(), "g2".to_string()]);

        store
            .set_connection_groups("P_a", &["g2".into()])
            .expect("set one");
        assert_eq!(store.list_groups_for_connection("P_a"), vec!["g2".to_string()]);

        store.set_connection_groups("P_a", &[]).expect("clear");
        assert!(store.list_groups_for_connection("P_a").is_empty());

        // 全局库无分组表：调用为 no-op（不报错、不写入）。
        let global = ConnectionOrgStore::open_at(dir.join("g.db"), false).expect("open global");
        assert!(global.set_connection_groups("G_a", &["g1".into()]).is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn groups_many_to_many_and_cleanup() {
        let dir = temp_dir("groups");
        let store = ConnectionOrgStore::open_at(dir.join("p.db"), true).expect("open");
        store.create_group("g1", "alpha", Some("关键业务")).expect("g1");
        store.create_group("g2", "beta", None).expect("g2");

        store.add_member("g1", "P_a").expect("add");
        store.add_member("g2", "P_a").expect("add");
        store.add_member("g1", "P_b").expect("add");
        // 幂等：重复加入不报错、不产生重复
        store.add_member("g1", "P_a").expect("add again");

        assert_eq!(store.list_group_members("g1"), vec!["P_a".to_string(), "P_b".to_string()]);
        assert_eq!(store.list_groups_for_connection("P_a"), vec!["g1".to_string(), "g2".to_string()]);
        assert_eq!(store.list_groups().len(), 2);

        // 删除连接 → 关系清理，分组仍在
        store.remove_connection("P_a").expect("cleanup");
        assert!(store.list_group_members("g1") == vec!["P_b".to_string()]);
        assert!(store.list_groups_for_connection("P_a").is_empty());
        assert_eq!(store.list_groups().len(), 2);

        // 删除分组 → 成员关系清理，连接不受影响
        store.delete_group("g1").expect("delete group");
        assert!(store.list_group_members("g1").is_empty());
        assert_eq!(store.list_groups_for_connection("P_b").len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn global_store_has_no_groups() {
        let dir = temp_dir("global");
        let store = ConnectionOrgStore::open_at(dir.join("g.db"), false).expect("open");
        store.set_tags("G_a", &["shared".into()]).expect("tags");
        assert_eq!(store.list_tags("G_a"), vec!["shared".to_string()]);
        // 全局库不建分组表：分组定义恒为空
        assert!(store.list_groups().is_empty());
        // remove_connection 不触碰分组表也不报错
        store.remove_connection("G_a").expect("cleanup");
        assert!(store.list_tags("G_a").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
