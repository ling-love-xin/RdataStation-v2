//! 标签定义存储（`tags` 表，两级：全局库 scope=global / 项目库 scope=project）。
//!
//! 连接模式仿 `engine::persistence::connection_org_store`：同步 rusqlite 连接 +
//! `open_at` 注入路径 + `ensure_tables` 幂等建表，面向 UI 线程的轻量调用
//! （规格 §3.2 / §3.3：关联表由三模块自建，本存储只管定义）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use shared::error::{CoreError, StorageError};

use crate::model::{Tag, SCOPE_GLOBAL, SCOPE_PROJECT, normalize_name};

/// 项目库文件名（与 `engine::persistence::connection_org_store` 的私有常量同值；
/// `.RSmeta` 拼写多源收敛见开发方案 P0.3）。
const PROJECT_DB_NAME: &str = "project.db";

/// 标签定义 CRUD（[`TagTable`] 实现；[`TagStore`] 在其上扩展关联与治理）。
pub trait TagDefinitions {
    /// 新建标签（定义）。`scope` 由存储域派生，不入参数；同名（活跃）拒绝。
    fn create_tag(&self, name: &str, color: Option<&str>, icon: Option<&str>) -> Result<Tag, CoreError>;
    /// 全局重命名（只改定义，关联自动跟随）。同名（活跃）拒绝；改成现名幂等返回原行。
    fn rename_tag(&self, id: &str, name: &str) -> Result<Tag, CoreError>;
    /// 软删定义行（规格 §5 决策 D2：不清关联，读取侧过滤 `deleted_at`）。
    fn delete_tag(&self, id: &str) -> Result<(), CoreError>;
    /// 恢复软删的定义行（管理覆盖层「查看已删标签 + 恢复」）。
    fn restore_tag(&self, id: &str) -> Result<Tag, CoreError>;
    /// 按 id 读取（含软删行，供恢复 / 审计）。
    fn get_tag(&self, id: &str) -> Result<Tag, CoreError>;
    /// 按名读取**活跃**定义（同名约束判定用）。
    fn get_tag_by_name(&self, name: &str) -> Result<Option<Tag>, CoreError>;
    /// 活跃标签（按名称升序）。
    fn list_tags(&self) -> Result<Vec<Tag>, CoreError>;
    /// 已软删标签（管理覆盖层）。
    fn list_deleted(&self) -> Result<Vec<Tag>, CoreError>;
    /// 按 id 集合批量读取**活跃**定义（跨库解析用：关联表 id 集合 → 全局定义；
    /// 规格 §7.2 跨库读取）。空集合返回空向量；顺序不保证，调用方自行排序。
    fn get_tags_by_ids(&self, ids: &[String]) -> Result<Vec<Tag>, CoreError>;
    /// 迁移专用：按给定 id 原样插入一行（保留软删标记与创建时间；scope 由存储域派生，
    /// 规格 §10.2 存量迁移）。撞部分唯一索引（同名活跃已存在）返回错误，调用方应改为
    /// 重映射关联（规格 D3 同名复用）。
    fn insert_migrated(
        &self,
        id: &str,
        name: &str,
        color: Option<&str>,
        icon: Option<&str>,
        created_at: &str,
        deleted_at: Option<&str>,
    ) -> Result<(), CoreError>;
}

/// 标签系统统一契约（规格 §4）：定义 + 关联 + 治理。
///
/// 三模块（导航连接 / 分析资源 / 草稿箱）各自实现；关联表由模块自建，
/// 定义部分可聚合复用 [`TagTable`]。
pub trait TagStore: TagDefinitions {
    /// 给对象打标（owner_id 由各模块的 id 语义决定；幂等）。
    fn attach(&self, owner_id: &str, tag_id: &str) -> Result<(), CoreError>;
    /// 移除对象上的标签（不存在则无操作）。
    fn detach(&self, owner_id: &str, tag_id: &str) -> Result<(), CoreError>;
    /// 对象的活跃标签（定义 + 关联解析，过滤软删）。
    fn tags_of(&self, owner_id: &str) -> Result<Vec<Tag>, CoreError>;
    /// 引用某标签的对象 id 列表。
    fn owners_of(&self, tag_id: &str) -> Result<Vec<String>, CoreError>;
    /// 每个标签的引用数（facet 排序用：引用数降序 → 名称升序）。
    fn usage_counts(&self) -> Result<Vec<(String, i64)>, CoreError>;
    /// 空标签清理：无引用的活跃标签 → 软删（规格 §5 清理策略）。
    fn cleanup_empty_tags(&self) -> Result<usize, CoreError>;
}

/// 标签定义存储（`tags` 表）。
pub struct TagTable {
    conn: Connection,
    scope: String,
}

impl TagTable {
    /// 打开全局库（`<RDS_HOME>/data/system/global.db`，scope=global）。
    pub fn open_global() -> Result<Self, CoreError> {
        let path = engine::migration::get_global_db_path()?;
        Self::open_at(path, SCOPE_GLOBAL)
    }

    /// 打开项目库（`{项目}/.RSmeta/project.db`，scope=project）。
    pub fn open_project(project_root: &Path) -> Result<Self, CoreError> {
        let path = project_root
            .join(engine::persistence::connection_org_store::RS_META_DIR_NAME)
            .join(PROJECT_DB_NAME);
        Self::open_at(path, SCOPE_PROJECT)
    }

    /// 打开指定库并确保表结构存在（幂等，兼容未执行迁移的旧库）。
    ///
    /// `open_global` / `open_project` 之外的路径入口（多环境 / 测试注入）。
    pub fn open_at(path: PathBuf, scope: &str) -> Result<Self, CoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "tags".to_string(),
                    operation: "create_tag_store_dir".to_string(),
                    reason: e.to_string(),
                })
            })?;
        }
        let conn = Connection::open(&path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "tags".to_string(),
                operation: "open_tag_store".to_string(),
                reason: e.to_string(),
            })
        })?;
        let store = Self {
            conn,
            scope: scope.to_string(),
        };
        store.ensure_tables()?;
        Ok(store)
    }

    /// 当前存储域的作用域。
    pub fn scope(&self) -> &str {
        &self.scope
    }

    /// 底层连接（关联表各模块自建、需与定义同库时复用）。
    ///
    /// 同步 `rusqlite::Connection`，非 `Sync`：调用方须保证不跨线程共享（草稿箱 / 分析资源
    /// 的关联操作都在各自存储的调用线程上完成）。定义方法仍走 [`TagDefinitions`] trait，
    /// 本访问器只给「同库自定义 SQL」用。
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 本库全部已知关联表的引用数（tag_id → 引用数）。
    ///
    /// 关联表由各模块自建（`connection_tags` / `scratchpad_tags` /
    /// `analytics_resource_tags`），标签定义与关联**同库**（规格 D7），单库即可统计
    /// 本 scope 域的全部引用；跨库部分（全局定义被项目库 `analytics_resource_tags`
    /// 引用，规格 D1 应用层解析）由覆盖层用项目库的计数补齐。
    /// 表名来自固定常量、不拼接外部输入（无注入面）。
    pub fn reference_counts(&self) -> Result<HashMap<String, i64>, CoreError> {
        let mut counts: HashMap<String, i64> = HashMap::new();
        for table in ["connection_tags", "scratchpad_tags", "analytics_resource_tags"] {
            let exists: bool = self
                .conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                    params![table],
                    |r| r.get(0),
                )
                .map_err(|e| self.err("check_link_table", e))?;
            if !exists {
                continue;
            }
            let sql = format!("SELECT tag_id, COUNT(*) FROM {table} GROUP BY tag_id");
            let mut stmt = self
                .conn
                .prepare(&sql)
                .map_err(|e| self.err("count_references", e))?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .map_err(|e| self.err("count_references_rows", e))?;
            for row in rows {
                let (id, n) = row.map_err(|e| self.err("count_references_row", e))?;
                *counts.entry(id).or_insert(0) += n;
            }
        }
        Ok(counts)
    }

    fn ensure_tables(&self) -> Result<(), CoreError> {
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS tags (
                    id         TEXT PRIMARY KEY,
                    name       TEXT NOT NULL,
                    color      TEXT,
                    icon       TEXT,
                    scope      TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    deleted_at TEXT
                )",
                [],
            )
            .map_err(|e| self.err("create_tags_table", e))?;
        // 部分唯一索引：软删后可同名重建（规格 §5）。
        self.conn
            .execute(
                "CREATE UNIQUE INDEX IF NOT EXISTS tags_name_scope_active
                 ON tags (name, scope) WHERE deleted_at IS NULL",
                [],
            )
            .map_err(|e| self.err("create_tags_unique_index", e))?;
        Ok(())
    }

    fn err(&self, operation: &str, reason: impl std::fmt::Display) -> CoreError {
        CoreError::storage(StorageError::Persistence {
            store: "tags".to_string(),
            operation: operation.to_string(),
            reason: reason.to_string(),
        })
    }
}

impl TagDefinitions for TagTable {
    fn create_tag(&self, name: &str, color: Option<&str>, icon: Option<&str>) -> Result<Tag, CoreError> {
        let Some(name) = normalize_name(name) else {
            return Err(self.err("create", "标签名不能为空"));
        };
        if self.tag_name_taken(&name)? {
            return Err(self.err("create", format!("已经有同名标签「{name}」")));
        }

        let id = format!("tag_{}", uuid::Uuid::new_v4().simple());
        let created_at = chrono::Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO tags (id, name, color, icon, scope, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    &id,
                    name,
                    color.map(str::to_owned),
                    icon.map(str::to_owned),
                    &self.scope,
                    &created_at
                ],
            )
            .map_err(|e| self.err("insert_tag", e))?;
        self.get_tag(&id)
    }

    fn rename_tag(&self, id: &str, name: &str) -> Result<Tag, CoreError> {
        let Some(name) = normalize_name(name) else {
            return Err(self.err("rename", "标签名不能为空"));
        };
        let current = self.get_tag(id)?;
        if current.name == name {
            return Ok(current);
        }
        if self.tag_name_taken(&name)? {
            return Err(self.err("rename", format!("已经有同名标签「{name}」")));
        }
        let affected = self
            .conn
            .execute(
                "UPDATE tags SET name = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                params![id, name],
            )
            .map_err(|e| self.err("rename_tag", e))?;
        if affected == 0 {
            return Err(self.err("rename", "标签不存在或已删除"));
        }
        self.get_tag(id)
    }

    fn delete_tag(&self, id: &str) -> Result<(), CoreError> {
        let affected = self
            .conn
            .execute(
                "UPDATE tags SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                params![id, chrono::Utc::now().to_rfc3339()],
            )
            .map_err(|e| self.err("delete_tag", e))?;
        if affected == 0 {
            return Err(self.err("delete", "标签不存在或已删除"));
        }
        Ok(())
    }

    fn restore_tag(&self, id: &str) -> Result<Tag, CoreError> {
        let affected = self
            .conn
            .execute(
                "UPDATE tags SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
                params![id],
            )
            .map_err(|e| self.err("restore_tag", e))?;
        if affected == 0 {
            return Err(self.err("restore", "标签不存在或未删除"));
        }
        self.get_tag(id)
    }

    fn get_tag(&self, id: &str) -> Result<Tag, CoreError> {
        self.conn
            .query_row(
                "SELECT id, name, color, icon, scope, created_at, deleted_at
                 FROM tags WHERE id = ?1",
                params![id],
                map_tag_row,
            )
            .map_err(|e| self.err("select_tag", e))
    }

    fn get_tag_by_name(&self, name: &str) -> Result<Option<Tag>, CoreError> {
        let found = self
            .conn
            .query_row(
                "SELECT id, name, color, icon, scope, created_at, deleted_at
                 FROM tags WHERE name = ?1 AND scope = ?2 AND deleted_at IS NULL",
                params![name, &self.scope],
                map_tag_row,
            )
            .optional()
            .map_err(|e| self.err("select_tag_by_name", e))?;
        Ok(found)
    }

    fn list_tags(&self) -> Result<Vec<Tag>, CoreError> {
        self.query_all(
            "SELECT id, name, color, icon, scope, created_at, deleted_at
             FROM tags WHERE deleted_at IS NULL ORDER BY name ASC",
        )
    }

    fn list_deleted(&self) -> Result<Vec<Tag>, CoreError> {
        self.query_all(
            "SELECT id, name, color, icon, scope, created_at, deleted_at
             FROM tags WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC",
        )
    }

    fn get_tags_by_ids(&self, ids: &[String]) -> Result<Vec<Tag>, CoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        // 参数化 IN 查询：占位符数量与输入一致，避免拼接（防注入）。
        let placeholders = vec!["?"; ids.len()].join(", ");
        let sql = format!(
            "SELECT id, name, color, icon, scope, created_at, deleted_at \
             FROM tags WHERE id IN ({placeholders}) AND deleted_at IS NULL"
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| self.err("prepare_tags_by_ids", e))?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(ids.iter().map(String::as_str)), map_tag_row)
            .map_err(|e| self.err("query_tags_by_ids", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| self.err("collect_tags_by_ids", e))
    }

    fn insert_migrated(
        &self,
        id: &str,
        name: &str,
        color: Option<&str>,
        icon: Option<&str>,
        created_at: &str,
        deleted_at: Option<&str>,
    ) -> Result<(), CoreError> {
        let affected = self
            .conn
            .execute(
                "INSERT INTO tags (id, name, color, icon, scope, created_at, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    name,
                    color.map(str::to_owned),
                    icon.map(str::to_owned),
                    &self.scope,
                    created_at,
                    deleted_at.map(str::to_owned),
                ],
            )
            .map_err(|e| self.err("insert_migrated", e))?;
        if affected == 0 {
            return Err(self.err("insert_migrated", "未写入任何行"));
        }
        Ok(())
    }
}

impl TagTable {
    /// 名字是否已被**活跃**标签占用（预查给中文报错；部分唯一索引兜底）。
    fn tag_name_taken(&self, name: &str) -> Result<bool, CoreError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM tags WHERE name = ?1 AND scope = ?2 AND deleted_at IS NULL",
                params![name, &self.scope],
                |row| row.get(0),
            )
            .map_err(|e| self.err("count_tag_name", e))?;
        Ok(count > 0)
    }

    fn query_all(&self, sql: &str) -> Result<Vec<Tag>, CoreError> {
        let mut stmt = self
            .conn
            .prepare(sql)
            .map_err(|e| self.err("prepare_tags", e))?;
        let rows = stmt
            .query_map([], map_tag_row)
            .map_err(|e| self.err("query_tags", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| self.err("collect_tags", e))
    }
}

/// 标签行映射（列序即 `SELECT` 的列序）。
fn map_tag_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        icon: row.get(3)?,
        scope: row.get(4)?,
        created_at: row.get(5)?,
        deleted_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 每个测试用独立临时库路径（并行互不污染），仿 `metadata_cache_index.rs::isolated`。
    fn isolated(f: impl FnOnce(&Path)) {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rds_tag_test_{}_{}",
            std::process::id(),
            seq
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        f(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_list_roundtrip_and_scope_derived() {
        isolated(|dir| {
            let store = TagTable::open_at(dir.join("p.db"), SCOPE_PROJECT).expect("open");
            assert_eq!(store.scope(), SCOPE_PROJECT);

            let t = store
                .create_tag("prod", Some("warning"), Some("db"))
                .expect("create");
            assert!(t.id.starts_with("tag_"));
            assert_eq!(t.name, "prod");
            assert_eq!(t.color.as_deref(), Some("warning"));
            assert_eq!(t.icon.as_deref(), Some("db"));
            assert_eq!(t.scope, SCOPE_PROJECT);
            assert!(t.deleted_at.is_none());

            // 可空字段
            let bare = store.create_tag("core", None, None).expect("create bare");
            assert!(bare.color.is_none() && bare.icon.is_none());

            // 活跃列表按名称升序
            let names: Vec<String> = store.list_tags().expect("list").into_iter().map(|t| t.name).collect();
            assert_eq!(names, vec!["core".to_string(), "prod".to_string()]);
            assert!(store.list_deleted().expect("deleted").is_empty());
        });
    }

    /// `reference_counts`：动态聚合本库已知关联表；表不存在跳过；多表计数叠加。
    #[test]
    fn reference_counts_aggregate_link_tables() {
        isolated(|dir| {
            let store = TagTable::open_at(dir.join("p.db"), SCOPE_PROJECT).expect("open");
            let a = store.create_tag("prod", None, None).expect("a");
            let b = store.create_tag("core", None, None).expect("b");
            // 无关联表时计数为空（无键）
            let counts = store.reference_counts().expect("counts");
            assert_eq!(counts.get(&a.id), None, "无关联表不应有计数键");

            // 造导航 + 草稿箱两张关联表（与分析资源表同列语义，表存在即统计）
            store
                .conn()
                .execute_batch(&format!(
                    "CREATE TABLE connection_tags (connection_id TEXT NOT NULL, tag TEXT NOT NULL, tag_id TEXT);
                     CREATE TABLE scratchpad_tags (entry_id TEXT NOT NULL, tag_id TEXT NOT NULL);
                     INSERT INTO connection_tags VALUES ('P1', 'x', '{0}'), ('P2', 'y', '{0}');
                     INSERT INTO scratchpad_tags VALUES ('e1', '{0}'), ('e2', '{1}');",
                    a.id, b.id
                ))
                .expect("seed link tables");
            let counts = store.reference_counts().expect("counts2");
            assert_eq!(counts.get(&a.id).copied(), Some(3), "2 导航 + 1 草稿");
            assert_eq!(counts.get(&b.id).copied(), Some(1));
        });
    }

    #[test]
    fn global_and_project_scopes_are_isolated() {
        isolated(|dir| {
            let global = TagTable::open_at(dir.join("g.db"), SCOPE_GLOBAL).expect("global");
            let project = TagTable::open_at(dir.join("p.db"), SCOPE_PROJECT).expect("project");

            global.create_tag("prod", None, None).expect("global prod");
            // 同名不同域互不冲突
            project.create_tag("prod", Some("danger"), None).expect("project prod");

            assert_eq!(global.list_tags().expect("g").len(), 1);
            assert_eq!(project.list_tags().expect("p").len(), 1);
            assert_eq!(global.get_tag_by_name("prod").expect("g by name").unwrap().scope, SCOPE_GLOBAL);
            assert_eq!(project.get_tag_by_name("prod").expect("p by name").unwrap().color.as_deref(), Some("danger"));
        });
    }

    #[test]
    fn create_rejects_duplicate_active_name() {
        isolated(|dir| {
            let store = TagTable::open_at(dir.join("p.db"), SCOPE_GLOBAL).expect("open");
            store.create_tag("prod", None, None).expect("first");
            let err = store.create_tag("prod", Some("danger"), None).expect_err("dup");
            let msg = format!("{err}");
            assert!(msg.contains("同名"), "报错应含中文可读提示：{msg}");
        });
    }

    #[test]
    fn create_rejects_blank_name() {
        isolated(|dir| {
            let store = TagTable::open_at(dir.join("p.db"), SCOPE_GLOBAL).expect("open");
            assert!(store.create_tag("   ", None, None).is_err());
        });
    }

    #[test]
    fn soft_delete_allows_recreate_same_name() {
        isolated(|dir| {
            let store = TagTable::open_at(dir.join("p.db"), SCOPE_GLOBAL).expect("open");
            let first = store.create_tag("prod", Some("warning"), None).expect("first");
            store.delete_tag(&first.id).expect("soft delete");

            // 软删后不再出现在活跃列表，出现在已删列表
            assert!(store.list_tags().expect("active").is_empty());
            assert_eq!(store.list_deleted().expect("deleted").len(), 1);

            // 部分唯一索引生效：软删后可同名重建，新 id（D2：旧关联不会带回）
            let second = store.create_tag("prod", Some("danger"), None).expect("recreate");
            assert_ne!(second.id, first.id);
            assert_eq!(store.list_tags().expect("active").len(), 1);
            assert_eq!(store.get_tag(&first.id).expect("old row").deleted_at.is_some(), true);
        });
    }

    #[test]
    fn rename_updates_and_rejects_collision() {
        isolated(|dir| {
            let store = TagTable::open_at(dir.join("p.db"), SCOPE_GLOBAL).expect("open");
            let a = store.create_tag("alpha", None, None).expect("a");
            let b = store.create_tag("beta", None, None).expect("b");

            // 幂等：改成现名返回原行
            let same = store.rename_tag(&a.id, " alpha ").expect("idempotent");
            assert_eq!(same.name, "alpha");

            // 冲突拒绝
            let err = store.rename_tag(&a.id, "beta").expect_err("collision");
            assert!(format!("{err}").contains("同名"));

            // 正常改名
            let renamed = store.rename_tag(&a.id, "gamma").expect("rename");
            assert_eq!(renamed.name, "gamma");
            assert!(store.get_tag_by_name("gamma").expect("by name").is_some());
            assert!(store.get_tag_by_name("alpha").expect("old gone").is_none());

            // 对已删标签改名报错
            store.delete_tag(&b.id).expect("delete b");
            assert!(store.rename_tag(&b.id, "delta").is_err());
        });
    }

    #[test]
    fn delete_and_restore_roundtrip() {
        isolated(|dir| {
            let store = TagTable::open_at(dir.join("p.db"), SCOPE_GLOBAL).expect("open");
            let t = store.create_tag("prod", None, None).expect("create");
            store.delete_tag(&t.id).expect("delete");

            // 重复删除报错
            assert!(store.delete_tag(&t.id).is_err());

            // 恢复已删标签成功 → 回到活跃；已活跃后重复恢复报错
            let restored = store.restore_tag(&t.id).expect("restore");
            assert!(restored.deleted_at.is_none());
            assert!(store.restore_tag(&t.id).is_err());
            assert_eq!(store.list_tags().expect("active").len(), 1);
        });
    }

    #[test]
    fn get_tags_by_ids_filters_deleted_and_empty() {
        isolated(|dir| {
            let store = TagTable::open_at(dir.join("p.db"), SCOPE_GLOBAL).expect("open");
            let a = store.create_tag("alpha", Some("warning"), None).expect("a");
            let b = store.create_tag("beta", None, Some("db")).expect("b");
            let c = store.create_tag("gamma", None, None).expect("c");

            // 软删 c：批量读只回活跃
            store.delete_tag(&c.id).expect("delete c");
            let got = store
                .get_tags_by_ids(&[a.id.clone(), b.id.clone(), c.id.clone()])
                .expect("by ids");
            let names: Vec<String> = got.iter().map(|t| t.name.clone()).collect();
            assert_eq!(names.len(), 2);
            assert!(names.contains(&"alpha".to_string()));
            assert!(names.contains(&"beta".to_string()));

            // 不存在的 id 静默跳过
            let got = store
                .get_tags_by_ids(&[a.id.clone(), "tag_missing".to_string()])
                .expect("by ids partial");
            assert_eq!(got.len(), 1);

            // 空集合返回空（不构造非法 SQL）
            assert!(store.get_tags_by_ids(&[]).expect("empty").is_empty());
        });
    }

    #[test]
    fn insert_migrated_keeps_id_and_soft_delete() {
        isolated(|dir| {
            let store = TagTable::open_at(dir.join("p.db"), SCOPE_GLOBAL).expect("open");
            // 原样迁入（保留 v1 前缀 id / 软删标记 / 创建时间）
            store
                .insert_migrated(
                    "at_legacy001",
                    "legacy",
                    Some("warning"),
                    None,
                    "2026-01-01T08:00:00+00:00",
                    Some("2026-02-01T08:00:00+00:00"),
                )
                .expect("insert migrated");

            let row = store.get_tag("at_legacy001").expect("read back");
            assert_eq!(row.name, "legacy");
            assert_eq!(row.color.as_deref(), Some("warning"));
            assert_eq!(row.scope, SCOPE_GLOBAL);
            assert!(row.deleted_at.is_some(), "软删标记保留");
            // 软删行不进活跃列表，也不占同名名额（后续同名活跃可建）
            assert!(store.list_tags().expect("active").is_empty());
            assert_eq!(store.list_deleted().expect("deleted").len(), 1);

            // 撞部分唯一索引：已迁过的活跃同名 → 报错
            store.create_tag("live", None, None).expect("create live");
            let err = store
                .insert_migrated(
                    "at_legacy002",
                    "live",
                    None,
                    None,
                    "2026-01-01T08:00:00+00:00",
                    None,
                )
                .expect_err("collision");
            assert!(format!("{err}").contains("UNIQUE"), "索引兜底报错：{err}");
        });
    }
}
