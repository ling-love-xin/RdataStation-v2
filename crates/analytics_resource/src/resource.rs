use super::*;
use chrono::Utc;
use serde_json::Value;
use shared::error::{CommonError, CoreError, StorageError};

/// 归档资源表的固定列顺序：单行读取、列表、分页三处必须一致。
///
/// v1 三处各写一遍，且 JSON 解析策略不同（列表宽容 / 单行硬报错）——同一份坏数据会
/// "列表里看得到、点进去报错"。现统一走 [`map_resource_row`]：降级为 `null` 并记 warn，
/// 一行坏数据不应该毁掉整个列表。
pub(crate) const RESOURCE_COLUMNS: &str = "\
    id, resource_type, name, alias, config, scope, row_count, column_count, file_size, \
    version, parent_version_id, parent_resource_id, source_query, created_at, updated_at, \
    created_by, deleted_at, \
    kind, content_hash, file_rel_path, readonly, promoted_from, source_connection_id, \
    source_table, definition_sql, archived_at";

/// 每页上限：`page_size` 夹紧到 [1, MAX]。
///
/// v1 的 `total_pages` 在 `page_size == 0` 时整数除零 panic；`page_size < 0` 会让 SQLite 的
/// `LIMIT -N` 变成"无上限"（整表返回）。服务层直连调用没有 IPC 兜底，必须自己夹紧。
const MAX_PAGE_SIZE: i32 = 500;

/// 给固定列名加上表别名前缀（`r.id, r.resource_type, …`）。
///
/// JOIN 查询（如按标签反查资源）复用同一份列顺序，避免又抄一遍。
pub(crate) fn qualified_resource_columns(alias: &str) -> String {
    RESOURCE_COLUMNS
        .split(',')
        .map(|column| format!("{alias}.{}", column.trim()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 行 → 领域对象（列顺序与 [`RESOURCE_COLUMNS`] 严格一致）。
///
/// `pub(crate)`：回收站与标签查询同样需要读整行，**不得各自再抄一份映射**
/// （v1 抄了 4 份，加一列就得改 4 处，且 JSON 策略各不相同）。
pub(crate) fn map_resource_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AnalyticsResource> {
    let config_str: String = row.get(4)?;
    let config: Value = serde_json::from_str(&config_str).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "资源 config JSON 解析失败，降级为 null");
        Value::Null
    });

    Ok(AnalyticsResource {
        id: row.get(0)?,
        resource_type: row.get(1)?,
        name: row.get(2)?,
        alias: row.get(3)?,
        config,
        scope: row.get(5)?,
        row_count: row.get::<_, Option<i64>>(6)?.map(|v| v as i32),
        column_count: row.get(7)?,
        file_size: row.get::<_, Option<i64>>(8)?.map(|v| v as i32),
        version: row.get(9)?,
        parent_version_id: row.get(10)?,
        parent_resource_id: row.get(11)?,
        source_query: row.get(12)?,
        created_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(13)?)?,
        updated_at: AnalyticsResourceStore::parse_datetime_sqlite(row.get(14)?)?,
        created_by: row.get(15)?,
        deleted_at: row
            .get(16)
            .ok()
            .and_then(|s| AnalyticsResourceStore::parse_datetime(s).ok()),
        kind: row.get(17)?,
        content_hash: row.get(18)?,
        file_rel_path: row.get(19)?,
        readonly: row.get(20)?,
        promoted_from: row.get(21)?,
        source_connection_id: row.get(22)?,
        source_table: row.get(23)?,
        definition_sql: row.get(24)?,
        archived_at: row
            .get::<_, Option<String>>(25)
            .ok()
            .flatten()
            .and_then(|s| AnalyticsResourceStore::parse_datetime(s).ok()),
    })
}

/// 统一的持久层错误构造（v1 每处手写一遍四元组）。
fn persistence_err(operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resources".to_string(),
        operation: operation.to_string(),
        reason: reason.to_string(),
    })
}

/// 分页参数夹紧：页码最小 1，页大小落在 [1, MAX_PAGE_SIZE]。
fn normalize_pagination(page: i32, page_size: i32) -> (i32, i32) {
    (page.max(1), page_size.clamp(1, MAX_PAGE_SIZE))
}

/// LIKE 通配符转义（配合 SQL 的 `ESCAPE '\'`）：v1 未转义，输入 `%` 等于全表匹配。
fn escape_like(term: &str) -> String {
    let mut escaped = String::with_capacity(term.len() + 2);
    for ch in term.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

/// 体积写入前的夹紧。
///
/// 列在 SQLite 里是 64 位，而模型（v1 遗留）是 `i32`——直接把大文件的值写进去，
/// 读回时 `as i32` 会变出一个负数体积。超过 2 GiB 就记为 2 GiB（显示与排序都不失真到不可读）。
fn clamp_file_size(bytes: Option<i64>) -> Option<i64> {
    bytes.map(|value| value.clamp(0, i32::MAX as i64))
}

impl AnalyticsResourceStore {
    pub async fn create_resource(
        &self,
        req: CreateResourceRequest,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let id = format!("ar_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now();

        conn.inner()?
            .execute(
                r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count, file_size,
                version, parent_version_id, parent_resource_id, source_query, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    &id,
                    &req.resource_type,
                    &req.name,
                    &req.alias,
                    serde_json::to_string(&req.config)
                        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?,
                    &req.scope,
                    req.row_count,
                    req.column_count,
                    req.file_size,
                    1,
                    None::<String>,
                    req.parent_resource_id,
                    req.source_query,
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;

        self.get_resource_by_id(&id).await
    }

    /// 更新资源：读旧行 → 存写前快照 → 递增版本，三步在同一写事务内完成。
    ///
    /// 为何用 `BEGIN IMMEDIATE`：三步之间存在"读版本号 → 写版本号"的竞态。延迟事务不加写锁，
    /// 两个并发更新会读到同一版本号，而后到的快照被 v1 的 `INSERT OR IGNORE` 静默丢弃
    /// （`UNIQUE(resource_id, version)`）——表现为"版本号涨了，历史里却没有"。
    /// 取写锁后，真并发写者会明确报 `SQLITE_BUSY`，而不是静默丢版本。
    ///
    /// 注：池尚未设 `busy_timeout`（engine 侧待修，见开发方案 P0.2），因此本路径**不重试**
    /// BUSY——宁可报错也不静默丢数据；超时补齐后再评估是否需要退避重试。
    pub async fn update_resource(
        &self,
        id: &str,
        req: CreateResourceRequest,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let now = Utc::now();

        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| persistence_err("begin_immediate", e))?;

        let result = (|| -> Result<AnalyticsResource, CoreError> {
            let current = Self::get_resource_by_id_on(inner, id)?;

            let snapshot = serde_json::to_string(&current)
                .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;
            let snapshot_id =
                Self::save_resource_version_on(inner, id, current.version, &snapshot)?;

            let affected = inner
                .execute(
                    r#"
            UPDATE analytics_resources
            SET name = ?, alias = ?, config = ?, scope = ?, row_count = ?, column_count = ?, file_size = ?,
                version = version + 1, parent_version_id = ?, updated_at = ?
            WHERE id = ? AND deleted_at IS NULL
            "#,
                    rusqlite::params![
                        &req.name,
                        &req.alias,
                        serde_json::to_string(&req.config)
                            .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?,
                        &req.scope,
                        req.row_count,
                        req.column_count,
                        req.file_size,
                        // 指向本次写入的快照行（v1 传的是资源自身 id，该列恒等于 id、无信息量）。
                        Some(&snapshot_id),
                        now.to_rfc3339(),
                        id,
                    ],
                )
                .map_err(|e| persistence_err("update", e))?;

            // v1 不检查影响行数：更新一个已软删除的资源会"静默成功"并返回旧数据。
            if affected == 0 {
                return Err(persistence_err("update", "资源不存在或已删除"));
            }

            Self::get_resource_by_id_on(inner, id)
        })();

        match &result {
            Ok(_) => inner
                .execute_batch("COMMIT")
                .map_err(|e| persistence_err("commit", e))?,
            Err(_) => {
                // 回滚失败只记日志：要向上返回的是原始错误。
                if let Err(e) = inner.execute_batch("ROLLBACK") {
                    tracing::warn!(error = %e, "回滚资源更新事务失败");
                }
            }
        }

        result
    }

    pub async fn get_resource_by_id(&self, id: &str) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        Self::get_resource_by_id_on(conn.inner()?, id)
    }

    /// 单连接版本的读取：供"读 → 写"必须在同一事务内完成的路径复用。
    ///
    /// v1 的 `update_resource` 先占一条连接，调 `get_resource_by_id` 又占一条，存快照再占一条
    /// （池只有 3 条）——一次逻辑操作最坏同时持两条连接，并发时会互相等对方归还。
    pub(crate) fn get_resource_by_id_on(
        conn: &rusqlite::Connection,
        id: &str,
    ) -> Result<AnalyticsResource, CoreError> {
        let sql = format!("SELECT {RESOURCE_COLUMNS} FROM analytics_resources WHERE id = ?");
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;
        let resource = stmt
            .query_row(rusqlite::params![id], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        Ok(resource)
    }

    /// 写入一条**归档行**（迁移 020 的新列在此落地）。
    ///
    /// 与 `create_resource`（v1 通用入口）刻意分开：归档行的 `kind` / 指纹 / 本体路径 / 只读
    /// 是语义必填，走通用入口会得到"看起来像存档、实际没有任何凭证"的行。
    pub async fn insert_archive(
        &self,
        input: NewArchiveInput,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let id = format!("ar_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();

        conn.inner()?
            .execute(
                r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, file_size,
                version, parent_version_id, parent_resource_id, source_query,
                created_at, updated_at,
                kind, content_hash, file_rel_path, readonly,
                promoted_from, source_connection_id, source_table, archived_at
            ) VALUES (?, ?, ?, ?, '{}', ?, ?, 1, NULL, NULL, NULL, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    &id,
                    &input.resource_type,
                    &input.name,
                    &input.alias,
                    &input.scope,
                    clamp_file_size(input.file_size),
                    &now,
                    &now,
                    input.kind.as_db_str(),
                    &input.content_hash,
                    &input.file_rel_path,
                    &input.binding.promoted_from,
                    &input.binding.source_connection_id,
                    &input.binding.source_table,
                    &now,
                ],
            )
            .map_err(|e| persistence_err("insert", e))?;

        self.get_resource_by_id(&id).await
    }

    /// 改**显示名**（原型 §3.2：行右键「重命名…」/ `F2`）。
    ///
    /// 为何不铸 [`update_resource`](Self::update_resource)：后者要 `version + 1` **并写一份
    /// 版本快照**——那是**内容**版本的口径（指纹变了才涨，原型 §1 原则 2），
    /// 改个名字不该在版本历史里多出一条、也不该让「版本」排序跟着跳。
    ///
    /// 只动 `name`：`updated_at` 由表上的 `trg_ar_updated_at` 触发器维护
    /// （`UPDATE OF name` 就在它的触发列里），这里不手写——两处都写会打架，
    /// 而且触发器用的是 `CURRENT_TIMESTAMP`（秒级）。路径 / 指纹 / 标签 / 分组都不在这条路上
    /// （显示名与本体位置分离是本模块的原则，原型 §1）。
    pub async fn rename_resource(
        &self,
        id: &str,
        name: &str,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let affected = inner
            .execute(
                "UPDATE analytics_resources SET name = ? WHERE id = ? AND deleted_at IS NULL",
                rusqlite::params![name, id],
            )
            .map_err(|e| persistence_err("rename", e))?;
        if affected == 0 {
            return Err(persistence_err("rename", "资源不存在或已删除"));
        }
        Self::get_resource_by_id_on(inner, id)
    }

    /// 再归档：把新内容指纹写入已有存档行（版本 +1），`parent_version_id` 指向写前快照行。
    ///
    /// 只动索引行，**不碰文件系统**：旧内容的版本副本与本体覆盖由调用方（归档服务）负责。
    /// `file_size` 必须与新内容一起给：**只换指纹不换体积**会让「大小」排序与行的尾巴
    /// 永远停在旧值上（错得比没数据还难发现）。
    pub async fn update_archive_content(
        &self,
        id: &str,
        content_hash: &str,
        snapshot_id: &str,
        file_size: Option<i64>,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let now = Utc::now().to_rfc3339();

        let affected = inner
            .execute(
                r#"
            UPDATE analytics_resources
            SET content_hash = ?, version = version + 1, parent_version_id = ?, updated_at = ?,
                file_size = ?
            WHERE id = ? AND deleted_at IS NULL
            "#,
                rusqlite::params![
                    content_hash,
                    snapshot_id,
                    &now,
                    clamp_file_size(file_size),
                    id
                ],
            )
            .map_err(|e| persistence_err("update", e))?;

        if affected == 0 {
            return Err(persistence_err("update", "存档不存在或已删除"));
        }

        self.get_resource_by_id(id).await
    }

    /// 按本体路径查存档（索引 020 的局部唯一索引保证至多一条）。
    ///
    /// 两个用途：归档前的目标占用检测；索引修复里的"有记录无本体"检测。
    pub async fn find_archive_by_rel_path(
        &self,
        rel_path: &str,
    ) -> Result<Option<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE file_rel_path = ? AND deleted_at IS NULL LIMIT 1"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;

        match stmt.query_row(rusqlite::params![rel_path], map_resource_row) {
            Ok(resource) => Ok(Some(resource)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(persistence_err("select", e)),
        }
    }

    /// 列出全部"有本体路径"的存活存档（索引修复的比对基准；按本体路径升序）。
    pub async fn list_file_archives(&self) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE file_rel_path IS NOT NULL AND deleted_at IS NULL ORDER BY file_rel_path ASC"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;
        let rows = stmt
            .query_map([], map_resource_row)
            .map_err(|e| persistence_err("select", e))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| persistence_err("select", e))
    }

    /// 软删：标记 `deleted_at`（移入回收站的索引侧动作）。
    ///
    /// 与 `hard_delete_row` 的分工：**回收站里的行软删**（还原要恢复完整记录：别名 /
    /// 指纹 / 来源 / 标签——回收站 manifest 里没有这些），**撤销归档与索引修复硬删**
    /// （那些场景本就该不留痕）。两者都不作用于已软删的行。
    pub async fn soft_delete_archive(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let now = Utc::now().to_rfc3339();
        let affected = conn
            .inner()?
            .execute(
                "UPDATE analytics_resources SET deleted_at = ?, updated_at = ? \
                 WHERE id = ? AND deleted_at IS NULL",
                rusqlite::params![&now, &now, id],
            )
            .map_err(|e| persistence_err("soft_delete", e))?;
        if affected == 0 {
            return Err(persistence_err("soft_delete", "行不存在或已在回收站里"));
        }
        Ok(())
    }

    /// 复活一行（从回收站还原）：清 `deleted_at`；本体路径因重名避让变过时一并更新。
    pub async fn undelete_archive(
        &self,
        id: &str,
        rel_path: Option<&str>,
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let affected = conn
            .inner()?
            .execute(
                "UPDATE analytics_resources \
                 SET deleted_at = NULL, file_rel_path = COALESCE(?, file_rel_path), updated_at = ? \
                 WHERE id = ? AND deleted_at IS NOT NULL",
                rusqlite::params![rel_path, Utc::now().to_rfc3339(), id],
            )
            .map_err(|e| persistence_err("undelete", e))?;
        if affected == 0 {
            return Err(persistence_err("undelete", "行不存在或不在回收站里"));
        }
        Ok(())
    }

    /// 按本体路径找**已软删**的行（回收站还原用：区别于“当前存活的同路径行”）。
    pub async fn find_deleted_archive_by_rel_path(
        &self,
        rel_path: &str,
    ) -> Result<Option<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;
        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources \
             WHERE file_rel_path = ? AND deleted_at IS NOT NULL LIMIT 1"
        );
        let mut stmt = conn
            .inner()?
            .prepare(&sql)
            .map_err(|e| persistence_err("select", e))?;

        match stmt.query_row(rusqlite::params![rel_path], map_resource_row) {
            Ok(resource) => Ok(Some(resource)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(persistence_err("select", e)),
        }
    }

    /// 硬删除一行**存活**登记（不写回收站、不留痕）。
    ///
    /// 两个调用方语义不同但动作相同：**撤销归档**（刚发生的那次，本体已移回）与
    /// **索引修复**（删掉“有记录无本体”的孤儿记录）。软删的行（`deleted_at` 非空）由
    /// [`Self::purge_deleted_row`] 负责——那是“回收站永久删除”。
    pub async fn hard_delete_row(&self, id: &str) -> Result<(), CoreError> {
        self.delete_row_with_links(id, false).await
    }

    /// 永久删除一行**软删**登记（回收站的「永久删除」）：本体由 `ProjectTrash::purge` 删，
    /// 这里清掉登记行与它的标签 / 分组归属——留着只会是一条永远看不见、却还占着标签与
    /// 分组归属的幽灵行（v1 的“还原丢归属”就是这种半个状态）。
    pub async fn purge_deleted_row(&self, id: &str) -> Result<(), CoreError> {
        self.delete_row_with_links(id, true).await
    }

    /// 永久删除**全部**软删登记（回收站的「清空」）：返回删掉的行数。
    ///
    /// 与逐条 `purge_deleted_row` 的差别只在批量的写法（回收站里还有条目而登记行被手工删过
    /// 时，这种“幽灵行”只能靠它收尾）。
    pub async fn purge_all_deleted(&self) -> Result<usize, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| persistence_err("begin_immediate", e))?;

        let result = (|| -> Result<usize, CoreError> {
            for table in ["analytics_resource_folder", "analytics_resource_tags"] {
                inner
                    .execute(
                        &format!(
                            "DELETE FROM {table} WHERE resource_id IN \
                             (SELECT id FROM analytics_resources WHERE deleted_at IS NOT NULL)"
                        ),
                        [],
                    )
                    .map_err(|e| persistence_err("delete", e))?;
            }
            inner
                .execute(
                    "DELETE FROM analytics_resources WHERE deleted_at IS NOT NULL",
                    [],
                )
                .map_err(|e| persistence_err("delete", e))
        })();

        match result {
            Ok(count) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| persistence_err("commit", e))?;
                Ok(count)
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    /// 真要删行的那一步（`in_trash` 决定删软删行还是存活行）。
    ///
    /// **关联先清、同一事务**：`analytics_resource_folder` / `analytics_resource_tags` 是
    /// `resource_id` 上的外键且没有 `ON DELETE CASCADE`，而项目库开了 `foreign_keys=ON`
    /// ——不先清关联，`DELETE FROM analytics_resources` 会直接被外键拒掉（表现为“删不掉”）。
    async fn delete_row_with_links(&self, id: &str, in_trash: bool) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| persistence_err("begin_immediate", e))?;

        let result = (|| -> Result<(), CoreError> {
            for table in ["analytics_resource_folder", "analytics_resource_tags"] {
                inner
                    .execute(
                        &format!("DELETE FROM {table} WHERE resource_id = ?"),
                        rusqlite::params![id],
                    )
                    .map_err(|e| persistence_err("delete", e))?;
            }
            let guard = if in_trash {
                "AND deleted_at IS NOT NULL"
            } else {
                "AND deleted_at IS NULL"
            };
            let affected = inner
                .execute(
                    &format!("DELETE FROM analytics_resources WHERE id = ? {guard}"),
                    rusqlite::params![id],
                )
                .map_err(|e| persistence_err("delete", e))?;
            if affected == 0 {
                return Err(persistence_err(
                    "delete",
                    if in_trash {
                        "记录不存在或不在回收站里"
                    } else {
                        "记录不存在或已删除"
                    },
                ));
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| persistence_err("commit", e))?;
                Ok(())
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    pub async fn remove_orphan_record(&self, id: &str) -> Result<(), CoreError> {
        self.hard_delete_row(id).await
    }

    pub async fn list_resources(
        &self,
        scope: Option<&str>,
        resource_type: Option<&str>,
        folder_id: Option<&str>,
    ) -> Result<Vec<AnalyticsResource>, CoreError> {
        let conn = self.get_conn().await?;

        let mut sql =
            format!("SELECT {RESOURCE_COLUMNS} FROM analytics_resources WHERE deleted_at IS NULL");

        let mut params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(s) = scope {
            sql.push_str(" AND scope = ?");
            params.push(rusqlite::types::Value::Text(s.to_string()));
        }

        if let Some(t) = resource_type {
            sql.push_str(" AND resource_type = ?");
            params.push(rusqlite::types::Value::Text(t.to_string()));
        }

        if let Some(f) = folder_id {
            sql.push_str(
                r#"
                AND id IN (
                    SELECT resource_id FROM analytics_resource_folder WHERE folder_id = ?
                )
                "#,
            );
            params.push(rusqlite::types::Value::Text(f.to_string()));
        }

        sql.push_str(" ORDER BY created_at DESC");

        let mut stmt = conn.inner()?.prepare(&sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })?;

        let resources = stmt
            .query_map(rusqlite::params_from_iter(params), map_resource_row)
            .map_err(|e| persistence_err("select", e))?;

        resources
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| persistence_err("select", e))
    }

    pub async fn clone_resource(
        &self,
        id: &str,
        new_name: Option<&str>,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;

        let original = self.get_resource_by_id(id).await?;

        let cloned_id = format!("ar_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now();
        let default_name = format!("{}（副本）", original.name);
        let cloned_name = new_name.unwrap_or(&default_name);

        conn.inner()?
            .execute(
                r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count, file_size,
                version, parent_version_id, parent_resource_id, source_query, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    &cloned_id,
                    &original.resource_type,
                    cloned_name,
                    original.alias,
                    serde_json::to_string(&original.config)
                        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?,
                    &original.scope,
                    original.row_count,
                    original.column_count,
                    original.file_size,
                    1,
                    None::<String>,
                    Some(&original.id),
                    original.source_query,
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "insert".to_string(),
                    reason: e.to_string(),
                })
            })?;

        self.get_resource_by_id(&cloned_id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn list_resources_paginated(
        &self,
        scope: Option<&str>,
        resource_type: Option<&str>,
        folder_id: Option<&str>,
        search: Option<&str>,
        page: i32,
        page_size: i32,
        sort_by: Option<&str>,
        sort_order: Option<&str>,
    ) -> Result<ListResourcesOutput, CoreError> {
        let conn = self.get_conn().await?;

        // 边界夹紧：page ≥ 1、page_size ∈ [1, MAX_PAGE_SIZE]（否则 total_pages 会除零 panic）。
        let (page, page_size) = normalize_pagination(page, page_size);

        let mut where_clauses = vec!["deleted_at IS NULL".to_string()];
        let mut params: Vec<rusqlite::types::Value> = Vec::new();

        if let Some(s) = scope {
            where_clauses.push("scope = ?".to_string());
            params.push(rusqlite::types::Value::Text(s.to_string()));
        }

        if let Some(t) = resource_type {
            where_clauses.push("resource_type = ?".to_string());
            params.push(rusqlite::types::Value::Text(t.to_string()));
        }

        if let Some(f) = folder_id {
            where_clauses.push(
                "id IN (SELECT resource_id FROM analytics_resource_folder WHERE folder_id = ?)"
                    .to_string(),
            );
            params.push(rusqlite::types::Value::Text(f.to_string()));
        }

        if let Some(search_term) = search {
            where_clauses
                .push(r#"(name LIKE ? ESCAPE '\' OR alias LIKE ? ESCAPE '\')"#.to_string());
            let search_pattern = format!("%{}%", escape_like(search_term));
            params.push(rusqlite::types::Value::Text(search_pattern.clone()));
            params.push(rusqlite::types::Value::Text(search_pattern));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let count_sql = format!("SELECT COUNT(*) FROM analytics_resources {}", where_sql);

        let total: i32 = conn
            .inner()?
            .query_row(
                &count_sql,
                rusqlite::params_from_iter(params.iter()),
                |row| row.get(0),
            )
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources".to_string(),
                    operation: "count".to_string(),
                    reason: e.to_string(),
                })
            })?;

        let sort_field = match sort_by {
            Some("name") => "name",
            Some("created_at") => "created_at",
            Some("updated_at") => "updated_at",
            Some("row_count") => "row_count",
            Some("file_size") => "file_size",
            _ => "created_at",
        };

        let sort_dir = match sort_order {
            Some("desc") => "DESC",
            _ => "ASC",
        };

        let offset = (page - 1) * page_size;

        let sql = format!(
            "SELECT {RESOURCE_COLUMNS} FROM analytics_resources {where_sql} \
             ORDER BY {sort_field} {sort_dir} LIMIT ? OFFSET ?"
        );

        params.push(rusqlite::types::Value::Integer(page_size as i64));
        params.push(rusqlite::types::Value::Integer(offset as i64));

        let mut stmt = conn.inner()?.prepare(&sql).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources".to_string(),
                operation: "select".to_string(),
                reason: e.to_string(),
            })
        })?;

        let resources = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), map_resource_row)
            .map_err(|e| persistence_err("select", e))?;

        let items: Vec<AnalyticsResource> = resources
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| persistence_err("select", e))?;

        // page_size 已在入口夹紧到 ≥ 1，此除式安全。
        let total_pages = if total == 0 {
            1
        } else {
            (total + page_size - 1) / page_size
        };

        Ok(ListResourcesOutput {
            items,
            total,
            page,
            page_size,
            total_pages,
        })
    }
}
