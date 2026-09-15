//! 连接组织元数据存储（分组 / 标签）——M3 连接模块与 M4 导航模块共用。
//!
//! 定位：**连接的组织元数据**（不是导航视图状态）。表与迁移
//! `global/018_add_navigator_state.sql`、`project_meta/017_add_navigator_groups_tags_state.sql`、
//! `project_meta/021_add_navigator_ungrouped_order.sql` 一致：
//! - `connection_tags`：连接 ↔ 标签（多值），随连接所在库（全局 / 项目）；
//! - `connection_groups` / `connection_group_members`：**项目级**分组（多对多 + 组内排序）；
//! - `navigator_ungrouped_order`：**项目级**「未分组」容器的手动顺序（见 [`UNGROUPED_SCOPE`]）。
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

/// 项目元数据目录名：与 `project::store::RS_META_DIR_NAME` 保持一致（历史误写为 `.RSMETA`，
/// 在大小写敏感的文件系统上会与项目模块分叉到两个目录）。
pub const RS_META_DIR_NAME: &str = ".RSmeta";
/// 项目库文件名。
const PROJECT_DB_NAME: &str = "project.db";

/// 「未分组」容器的作用域哨兵。
///
/// 它**不是** `connection_groups` 里的行（否则要处理一张不存在的分组）：成员由
/// “不属于任何分组”推导，顺序单独落在 `navigator_ungrouped_order`。
/// 导航视图（`workbench`）的分组伪 ID 必须与此保持同值。
pub const UNGROUPED_SCOPE: &str = "__ungrouped__";

/// 组内成员「**未手动排序**」的序号哨兵。
///
/// 必须为负：手动排序写的是 `0..n`（见 [`ConnectionOrgStore::set_member_order_all`]），
/// 所以负数不会与真实下标撞。用哨兵而不是 `NULL`，是为了免去 SQLite 重建表
/// （`NOT NULL` 列改可空必须重建），代价是这个魔数需要在读写两侧保持一致。
pub const MEMBER_ORDER_UNSET: i64 = -1;

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

    /// 打开项目库（`{project}/.RSmeta/project.db`）。
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
                    // `sort_order` 的列缺省保留 `0`（改缺省同样要重建表）；读写两侧**始终显式**
                    // 传值：新关系写 [`MEMBER_ORDER_UNSET`]，手动排序写 `0..n`。
                    "CREATE TABLE IF NOT EXISTS connection_group_members (
                        group_id      TEXT NOT NULL,
                        connection_id TEXT NOT NULL,
                        sort_order    INTEGER NOT NULL DEFAULT 0,
                        is_primary    INTEGER NOT NULL DEFAULT 0,
                        PRIMARY KEY (group_id, connection_id)
                    )",
                    [],
                )
                .map_err(|e| self.err("create_connection_group_members", e))?;
            // 旧库（无 `is_primary` 列）幂等补列：与迁移执行顺序无关。
            // 不落迁移的原因：项目库可能先被 `ensure_tables` 建表、后跑迁移，
            // 普通 `ALTER TABLE ADD COLUMN` 无法条件化，冲会触发重复列错误。
            self.ensure_member_primary_column()?;
            self.conn
                .execute(
                    "CREATE TABLE IF NOT EXISTS navigator_ungrouped_order (
                        connection_id TEXT PRIMARY KEY,
                        sort_order    INTEGER NOT NULL DEFAULT 0,
                        updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                    )",
                    [],
                )
                .map_err(|e| self.err("create_navigator_ungrouped_order", e))?;
        }
        Ok(())
    }

    /// 幂等确保 `connection_group_members.is_primary` 存在（老库补齐）。
    fn ensure_member_primary_column(&self) -> Result<(), CoreError> {
        let has = {
            let mut stmt = self
                .conn
                .prepare("PRAGMA table_info(connection_group_members)")
                .map_err(|e| self.err("pragma_cgm", e))?;
            let names = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(|e| self.err("pragma_cgm_rows", e))?;
            names.filter_map(Result::ok).any(|n| n == "is_primary")
        };
        if !has {
            self.conn
                .execute(
                    "ALTER TABLE connection_group_members
                     ADD COLUMN is_primary INTEGER NOT NULL DEFAULT 0",
                    [],
                )
                .map_err(|e| self.err("alter_cgm_is_primary", e))?;
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

    /// 全部「连接 → 标签」对（按连接 ID、标签升序）。
    ///
    /// 供导航面板一次性建立完整映射，避免渲染期逐连接查询。
    pub fn list_tag_pairs(&self) -> Vec<(String, String)> {
        let Ok(mut stmt) = self
            .conn
            .prepare("SELECT connection_id, tag FROM connection_tags ORDER BY connection_id, tag")
        else {
            return Vec::new();
        };
        match stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
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

    /// 重写分组之间的顺序（序号 = 下标；一次事务）。
    ///
    /// 只改 `sort_order`（名称 / 描述不动）：分组排序是纯重排，不应顺带覆盖正文。
    pub fn set_group_order(&self, group_ids: &[String]) -> Result<(), CoreError> {
        if !self.is_project {
            return Ok(());
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| self.err("begin_group_order", e))?;
        for (i, gid) in group_ids.iter().enumerate() {
            tx.execute(
                "UPDATE connection_groups SET sort_order = ?2, updated_at = CURRENT_TIMESTAMP
                 WHERE id = ?1",
                params![gid, i as i64],
            )
            .map_err(|e| self.err("group_order", e))?;
        }
        tx.commit()
            .map_err(|e| self.err("commit_group_order", e))?;
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
    ///
    /// 新成员一律标为**未手动排序**（[`MEMBER_ORDER_UNSET`]）；列缺省 `0` 在自动排序下
    /// 会与「手动排在第 0 位」不可分，所以不用缺省值。
    pub fn add_member(&self, group_id: &str, conn_id: &str) -> Result<(), CoreError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO connection_group_members (group_id, connection_id, sort_order)
                 VALUES (?1, ?2, ?3)",
                params![group_id, conn_id, MEMBER_ORDER_UNSET],
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

    /// 重写某分组的成员顺序（一次事务；序号 = 下标）。
    ///
    /// 只更新**已是成员**的行（非成员 `UPDATE` 影响 0 行，不报错）：调用方需先
    /// `add_member`。完整的 `0..n` 重写而非相对插入，避免序号空洞与并发对不上。
    pub fn set_member_order_all(
        &self,
        group_id: &str,
        conn_ids: &[String],
    ) -> Result<(), CoreError> {
        if !self.is_project {
            return Ok(());
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| self.err("begin_member_order_all", e))?;
        for (i, cid) in conn_ids.iter().enumerate() {
            tx.execute(
                "UPDATE connection_group_members SET sort_order = ?3
                 WHERE group_id = ?1 AND connection_id = ?2",
                params![group_id, cid, i as i64],
            )
            .map_err(|e| self.err("member_order_all", e))?;
        }
        tx.commit()
            .map_err(|e| self.err("commit_member_order_all", e))?;
        Ok(())
    }

    /// 「未分组」容器的显式顺序（未手动排序的连接不在结果中）。
    pub fn list_ungrouped_order(&self) -> Vec<String> {
        if !self.is_project {
            return Vec::new();
        }
        let Ok(mut stmt) = self
            .conn
            .prepare("SELECT connection_id FROM navigator_ungrouped_order ORDER BY sort_order ASC")
        else {
            return Vec::new();
        };
        match stmt.query_map([], |r| r.get::<_, String>(0)) {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 重写「未分组」容器的顺序（整体替换：序号 = 下标）。
    ///
    /// 整体替换（先清后写）而非补写：容器的成员是**推导**出来的（“不属于任何分组”），
    /// 表里只保留当前实际的未分组连接，避免旧行在连接重新回到未分组时“复活”旧位置。
    pub fn set_ungrouped_order(&self, conn_ids: &[String]) -> Result<(), CoreError> {
        if !self.is_project {
            return Ok(());
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| self.err("begin_ungrouped_order", e))?;
        tx.execute("DELETE FROM navigator_ungrouped_order", [])
            .map_err(|e| self.err("clear_ungrouped_order", e))?;
        for (i, cid) in conn_ids.iter().enumerate() {
            tx.execute(
                "INSERT INTO navigator_ungrouped_order (connection_id, sort_order) VALUES (?1, ?2)",
                params![cid, i as i64],
            )
            .map_err(|e| self.err("insert_ungrouped_order", e))?;
        }
        tx.commit()
            .map_err(|e| self.err("commit_ungrouped_order", e))?;
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

    /// 设置连接的**主组**（同一连接同时只能有一个主组）。
    ///
    /// 要求 `group_id` 已包含该连接（否则无行被置主，保持原状）；
    /// 与树渲染的「主组全亮 + 其它组引用行」配套，详见原型设计 §2.2。
    pub fn set_primary_group(&self, conn_id: &str, group_id: &str) -> Result<(), CoreError> {
        if !self.is_project {
            return Ok(());
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| self.err("begin_set_primary", e))?;
        tx.execute(
            "UPDATE connection_group_members SET is_primary = 0 WHERE connection_id = ?1",
            params![conn_id],
        )
        .map_err(|e| self.err("clear_primary", e))?;
        tx.execute(
            "UPDATE connection_group_members SET is_primary = 1
             WHERE connection_id = ?1 AND group_id = ?2",
            params![conn_id, group_id],
        )
        .map_err(|e| self.err("set_primary", e))?;
        tx.commit().map_err(|e| self.err("commit_set_primary", e))?;
        Ok(())
    }

    /// 清除连接的主组标记（回退到「按分组排序推导」）。
    pub fn clear_primary_group(&self, conn_id: &str) -> Result<(), CoreError> {
        if !self.is_project {
            return Ok(());
        }
        self.conn
            .execute(
                "UPDATE connection_group_members SET is_primary = 0 WHERE connection_id = ?1",
                params![conn_id],
            )
            .map_err(|e| self.err("clear_primary_group", e))?;
        Ok(())
    }

    /// 全部显式主组（连接 ID → 主组 ID）；未显式指定的连接不在结果中。
    pub fn list_primary_group_pairs(&self) -> Vec<(String, String)> {
        if !self.is_project {
            return Vec::new();
        }
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT connection_id, group_id FROM connection_group_members
             WHERE is_primary = 1",
        ) else {
            return Vec::new();
        };
        match stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
            Ok(iter) => iter.filter_map(Result::ok).collect(),
            Err(_) => Vec::new(),
        }
    }

    /// 分组内成员（手动排序优先，未排按连接 ID）。
    ///
    /// 未手动排序的成员排在同组**最后**；它们之间的**名称升序**由视图负责
    /// （名称不在本存储，见 [`ConnectionOrgStore::list_group_members_detailed`]）。
    pub fn list_group_members(&self, group_id: &str) -> Vec<String> {
        self.list_group_members_detailed(group_id)
            .into_iter()
            .map(|(id, _)| id)
            .collect()
    }

    /// 分组内成员 + **是否手动排序过**（`None` = 未排）。
    ///
    /// 顺序：[`MEMBER_ORDER_UNSET`] 的排最后，其余按序号；同段内按连接 ID 保证确定性。
    /// 视图拿这个「排 / 未排」分区去把未排段按名称重排，然后才落库。
    pub fn list_group_members_detailed(&self, group_id: &str) -> Vec<(String, Option<i64>)> {
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT connection_id, sort_order FROM connection_group_members
             WHERE group_id = ?1 ORDER BY (sort_order < 0) ASC, sort_order ASC, connection_id ASC",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map(params![group_id], |r| {
            let id: String = r.get(0)?;
            let order: i64 = r.get(1)?;
            Ok((id, (order >= 0).then_some(order)))
        });
        match rows {
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
        for gid in group_ids.iter() {
            self.conn
                .execute(
                    // 对话框只决定「属于哪些组」，不决定组内位置：新关系一律标未手动排序。
                    "INSERT OR IGNORE INTO connection_group_members (group_id, connection_id, sort_order)
                     VALUES (?1, ?2, ?3)",
                    params![gid, conn_id, MEMBER_ORDER_UNSET],
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
            self.conn
                .execute(
                    "DELETE FROM navigator_ungrouped_order WHERE connection_id = ?1",
                    params![conn_id],
                )
                .map_err(|e| self.err("cleanup_ungrouped_order", e))?;
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
        // 一次性映射（按连接 ID、标签升序），供导航面板建立缓存。
        assert_eq!(
            store.list_tag_pairs(),
            vec![
                ("P_a".to_string(), "core".to_string()),
                ("P_a".to_string(), "prod".to_string()),
                ("P_b".to_string(), "prod".to_string()),
            ]
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
    fn primary_group_is_exclusive_and_falls_back() {
        let dir = temp_dir("primary");
        let store = ConnectionOrgStore::open_at(dir.join("p.db"), true).expect("open");
        store.create_group("g1", "alpha", None).expect("g1");
        store.create_group("g2", "beta", None).expect("g2");
        store.add_member("g1", "P_a").expect("add g1");
        store.add_member("g2", "P_a").expect("add g2");

        // 默认无显式主组。
        assert!(store.list_primary_group_pairs().is_empty());

        // 设 g2 为主组：同一连接仅一行被置主（独占）。
        store.set_primary_group("P_a", "g2").expect("set g2");
        assert_eq!(
            store.list_primary_group_pairs(),
            vec![("P_a".to_string(), "g2".to_string())]
        );

        // 切到 g1：g2 被清除。
        store.set_primary_group("P_a", "g1").expect("set g1");
        assert_eq!(
            store.list_primary_group_pairs(),
            vec![("P_a".to_string(), "g1".to_string())]
        );

        // 设为不属于该连接的分组：不影响已有主组。
        store
            .set_primary_group("P_a", "g_missing")
            .expect("set missing");
        assert!(store.list_primary_group_pairs().is_empty());

        // 显式清除：回到无主组。
        store.set_primary_group("P_a", "g1").expect("set g1 again");
        store.clear_primary_group("P_a").expect("clear");
        assert!(store.list_primary_group_pairs().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn member_and_ungrouped_order_roundtrip() {
        let dir = temp_dir("order");
        let store = ConnectionOrgStore::open_at(dir.join("p.db"), true).expect("open");
        store.create_group("g1", "alpha", None).expect("g1");
        for c in ["P_a", "P_b", "P_c"] {
            store.add_member("g1", c).expect("add");
        }

        // 组内排序：整体重写（序号 = 下标）。
        store
            .set_member_order_all("g1", &["P_c".into(), "P_a".into(), "P_b".into()])
            .expect("order");
        assert_eq!(
            store.list_group_members("g1"),
            vec!["P_c".to_string(), "P_a".to_string(), "P_b".to_string()]
        );
        // 非成员不报错（`UPDATE` 影响 0 行）：默认排序下位置不变。
        store
            .set_member_order_all("g1", &["P_b".into(), "P_c".into(), "P_a".into()])
            .expect("order again");
        assert_eq!(
            store.list_group_members("g1"),
            vec!["P_b".to_string(), "P_c".to_string(), "P_a".to_string()]
        );

        // 新成员标为「未手动排序」（负哨兵），且排在同组**最后**（不占真实下标）。
        store.add_member("g1", "P_d").expect("add d");
        let detailed = store.list_group_members_detailed("g1");
        assert_eq!(detailed.len(), 4);
        assert_eq!(detailed[3].0, "P_d");
        assert_eq!(detailed[3].1, None);
        assert!(detailed[..3].iter().all(|(_, o)| o.is_some()));
        assert_eq!(
            store.list_group_members("g1"),
            vec![
                "P_b".to_string(),
                "P_c".to_string(),
                "P_a".to_string(),
                "P_d".to_string()
            ]
        );
        // 一旦整体重写，未排的也获得真实序号（视图把当前屏上顺序冻结下来）。
        store
            .set_member_order_all(
                "g1",
                &["P_d".into(), "P_b".into(), "P_c".into(), "P_a".into()],
            )
            .expect("order three");
        assert!(
            store
                .list_group_members_detailed("g1")
                .iter()
                .all(|(_, o)| o.is_some())
        );

        // 未分组容器：整体替换（不是补写），删掉的连接不会留下旧位置。
        assert!(store.list_ungrouped_order().is_empty());
        store
            .set_ungrouped_order(&["P_x".into(), "P_y".into()])
            .expect("ungrouped");
        assert_eq!(
            store.list_ungrouped_order(),
            vec!["P_x".to_string(), "P_y".to_string()]
        );
        store.set_ungrouped_order(&["P_y".into()]).expect("replace");
        assert_eq!(store.list_ungrouped_order(), vec!["P_y".to_string()]);

        // 删除连接 → 未分组顺序一并清理。
        store.remove_connection("P_y").expect("cleanup");
        assert!(store.list_ungrouped_order().is_empty());

        // 全局库无分组表：两者均为 no-op / 空。
        let global = ConnectionOrgStore::open_at(dir.join("g.db"), false).expect("open global");
        assert!(global.set_ungrouped_order(&["G_a".into()]).is_ok());
        assert!(global.list_ungrouped_order().is_empty());
        assert!(global.set_member_order_all("g1", &["G_a".into()]).is_ok());

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
    fn group_order_rewrite_keeps_names_and_descriptions() {
        let dir = temp_dir("group_order");
        let store = ConnectionOrgStore::open_at(dir.join("p.db"), true).expect("open");
        store
            .create_group("g1", "alpha", Some("第一组"))
            .expect("g1");
        store.create_group("g2", "beta", None).expect("g2");
        store.create_group("g3", "gamma", None).expect("g3");
        // 新建分组同 sort_order（缺省 0）→ 回退到名称升序：alpha / beta / gamma。
        assert_eq!(
            store.list_groups().iter().map(|g| g.id.clone()).collect::<Vec<_>>(),
            vec!["g1".to_string(), "g2".to_string(), "g3".to_string()]
        );

        store
            .set_group_order(&["g2".into(), "g3".into(), "g1".into()])
            .expect("order");
        assert_eq!(
            store
                .list_groups()
                .iter()
                .map(|g| g.id.clone())
                .collect::<Vec<_>>(),
            vec!["g2".to_string(), "g3".to_string(), "g1".to_string()]
        );
        // 重排不改正文：名称与描述原样保留。
        let g1 = store.list_groups().into_iter().find(|g| g.id == "g1").expect("g1");
        assert_eq!(g1.name, "alpha");
        assert_eq!(g1.description.as_deref(), Some("第一组"));
        assert_eq!(g1.sort_order, 2);

        // 全局库无分组表：no-op。
        let global = ConnectionOrgStore::open_at(dir.join("g.db"), false).expect("open global");
        assert!(global.set_group_order(&["g1".into()]).is_ok());

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
