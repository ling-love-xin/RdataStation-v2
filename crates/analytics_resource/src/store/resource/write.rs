//! 归档写入、改名、别名与内容更新。
//!
//! 写入口径（P0-1）：凡「登记行 + 版本快照 + 履历事件」必须落在**同一个 SQLite 事务**里。
//! 原来三步各自 autocommit，中途失败会留下「快照写了、索引没更新、时间线还少一条」的半截状态；
//! 时间线一旦要用来追溯，它自己就不能撒谎。

use super::*;
use crate::model::TimelineEventKind;

impl AnalyticsResourceStore {
    /// 在**调用方连接**上插入一条归档行（供事务内复用）。
    fn insert_archive_on(
        inner: &rusqlite::Connection,
        input: &NewArchiveInput,
        id: &str,
        now: &str,
    ) -> Result<(), CoreError> {
        inner
            .execute(
                r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, file_size,
                row_count, column_count, version, parent_version_id, parent_resource_id, source_query,
                created_at, updated_at,
                kind, content_hash, file_rel_path, readonly,
                promoted_from, source_connection_id, source_table, definition_sql, archived_at,
                payload_mode, source_path
            ) VALUES (?, ?, ?, ?, '{}', ?, ?, ?, ?, 1, NULL, NULL, NULL, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    id,
                    &input.resource_type,
                    &input.name,
                    &input.alias,
                    &input.scope,
                    clamp_file_size(input.file_size),
                    input.row_count,
                    input.column_count,
                    now,
                    now,
                    input.kind.as_db_str(),
                    &input.content_hash,
                    &input.file_rel_path,
                    &input.binding.promoted_from,
                    &input.binding.source_connection_id,
                    &input.binding.source_table,
                    &input.definition_sql,
                    now,
                    input.payload_mode.as_db_str(),
                    &input.source_path,
                ],
            )
            .map_err(|e| persistence_err("insert", e))?;
        Ok(())
    }

    /// 写入一条**归档行**（迁移 020 的新列在此落地）。
    ///
    /// 与 `create_resource`（v1 通用入口）刻意分开：归档行的 `kind` / 指纹 / 本体路径 / 只读
    /// 是语义必填，走通用入口会得到"看起来像存档、实际没有任何凭证"的行。
    ///
    /// 保留无事务版本给并发建行 / 索引补登 / 存储层回归用例；服务层的首次归档走
    /// [`Self::insert_archive_with_event`]，那一条才带履历事件。
    pub async fn insert_archive(
        &self,
        input: NewArchiveInput,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let id = format!("ar_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        Self::insert_archive_on(conn.inner()?, &input, &id, &now)?;
        self.get_resource_by_id(&id).await
    }

    /// 服务层首次归档：**登记行 + 履历事件**同一事务（P0-1）。
    ///
    /// 事件写失败要连登记行一起回滚，否则会出现「时间线说归档了、列表里却没有」——
    /// 那正是资产库最不该有的自相矛盾。
    pub async fn insert_archive_with_event(
        &self,
        input: NewArchiveInput,
        detail: Option<&str>,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let id = format!("ar_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();

        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| persistence_err("begin_immediate", e))?;
        let result = (|| -> Result<(), CoreError> {
            Self::insert_archive_on(inner, &input, &id, &now)?;
            Self::record_event_on(
                inner,
                &id,
                Some(1),
                TimelineEventKind::Archived,
                detail,
                Some(&input.content_hash),
            )
        })();
        match result {
            Ok(()) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| persistence_err("commit", e))?;
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        self.get_resource_by_id(&id).await
    }

    /// 改**显示名**（原型 §3.2：行右键「重命名…」/ `F2`）。
    ///
    /// 为何不铸 [`update_resource`](Self::update_resource)：后者要 `version + 1` **并写一份
    /// 版本快照**——那是**内容**版本的口径（指纹变了才涨，原型 §1 原则 2），
    /// 改个名字不该在版本历史里多出一条、也不该让「版本」排序跟着跳。
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

    /// 改**别名**（详情面板头部点别名；原型 §3.1）。
    ///
    /// 与 [`rename_resource`](Self::rename_resource) 同一取舍：只动 `alias` 一列，
    /// **不涨版本、不写快照**。`None`（或空串）= 清除别名（存 NULL，不存空串）。
    pub async fn set_alias(
        &self,
        id: &str,
        alias: Option<&str>,
    ) -> Result<AnalyticsResource, CoreError> {
        let alias = alias.map(str::trim).filter(|value| !value.is_empty());
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let affected = inner
            .execute(
                "UPDATE analytics_resources SET alias = ? WHERE id = ? AND deleted_at IS NULL",
                rusqlite::params![alias, id],
            )
            .map_err(|e| persistence_err("alias", e))?;
        if affected == 0 {
            return Err(persistence_err("alias", "资源不存在或已删除"));
        }
        Self::get_resource_by_id_on(inner, id)
    }

    /// 在**调用方连接**上把新指纹写入存档行（版本 +1），供事务内复用。
    fn update_archive_content_on(
        inner: &rusqlite::Connection,
        id: &str,
        content_hash: &str,
        snapshot_id: &str,
        file_size: Option<i64>,
        analysis: Option<&crate::analysis::AnalysisFacts>,
    ) -> Result<(), CoreError> {
        let now = Utc::now().to_rfc3339();
        // 分析表：定义与规模随新内容一起更新；文件型：三列保持原样（不该被清掉）。
        let (definition_sql, row_count, column_count) = match analysis {
            Some(facts) => (
                facts.definition_sql.clone(),
                clamp_row_count(facts.row_count),
                Some(facts.column_count()),
            ),
            None => (None, None, None),
        };
        let affected = inner
            .execute(
                r#"
            UPDATE analytics_resources
            SET content_hash = ?, version = version + 1, parent_version_id = ?, updated_at = ?,
                file_size = ?,
                definition_sql = COALESCE(?, definition_sql),
                row_count = COALESCE(?, row_count),
                column_count = COALESCE(?, column_count)
            WHERE id = ? AND deleted_at IS NULL
            "#,
                rusqlite::params![
                    content_hash,
                    snapshot_id,
                    &now,
                    clamp_file_size(file_size),
                    definition_sql,
                    row_count,
                    column_count,
                    id
                ],
            )
            .map_err(|e| persistence_err("update", e))?;
        if affected == 0 {
            return Err(persistence_err("update", "存档不存在或已删除"));
        }
        Ok(())
    }

    /// 再归档：把新内容指纹写入已有存档行（版本 +1），`parent_version_id` 指向写前快照行。
    ///
    /// 只动索引行，**不碰文件系统**：旧内容的版本副本与本体替换由调用方（归档服务）负责。
    /// `file_size` 必须与新内容一起给：只换指纹不换体积会让「大小」排序永远停在旧值。
    ///
    /// 服务层请用 [`Self::commit_new_version`]（快照 + 索引 + 履历同一事务）；本方法保留给
    /// 索引修复与存储层回归用例。
    pub async fn update_archive_content(
        &self,
        id: &str,
        content_hash: &str,
        snapshot_id: &str,
        file_size: Option<i64>,
        analysis: Option<&crate::analysis::AnalysisFacts>,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        Self::update_archive_content_on(
            conn.inner()?,
            id,
            content_hash,
            snapshot_id,
            file_size,
            analysis,
        )?;
        self.get_resource_by_id(id).await
    }

    /// 再归档 / 历史还原的**统一写入口**（P0-1）：写前快照 + 索引 +1 + 履历事件，同一事务。
    ///
    /// 原来这三步分散在服务层，彼此 autocommit：快照写成功而索引失败会留下"有个版本、
    /// 行却没涨"，事件写失败则时间线凭空断一截。现在要么三件一起成立，要么一起回滚。
    ///
    /// 调用方负责把新内容先放到暂存区，并在**本方法返回之后**再替换本体（见
    /// `PayloadStore::stage_payload` / `commit_staged`）——顺序反了就回到旧问题。
    #[allow(clippy::too_many_arguments)]
    pub async fn commit_new_version(
        &self,
        id: &str,
        old_version: i32,
        snapshot: &str,
        content_hash: &str,
        file_size: Option<i64>,
        analysis: Option<&crate::analysis::AnalysisFacts>,
        event_kind: TimelineEventKind,
        event_detail: Option<&str>,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        inner
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| persistence_err("begin_immediate", e))?;
        let result = (|| -> Result<(), CoreError> {
            let snapshot_id = Self::save_resource_version_on(inner, id, old_version, snapshot)?;
            Self::update_archive_content_on(
                inner,
                id,
                content_hash,
                &snapshot_id,
                file_size,
                analysis,
            )?;
            Self::record_event_on(
                inner,
                id,
                Some(old_version + 1),
                event_kind,
                event_detail,
                Some(content_hash),
            )
        })();
        match result {
            Ok(()) => {
                inner
                    .execute_batch("COMMIT")
                    .map_err(|e| persistence_err("commit", e))?;
            }
            Err(error) => {
                let _ = inner.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
        self.get_resource_by_id(id).await
    }
}
