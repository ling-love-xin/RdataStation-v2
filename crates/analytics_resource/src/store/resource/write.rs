//! 归档写入、改名、别名与内容更新。

use super::*;

impl AnalyticsResourceStore {
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
                row_count, column_count, version, parent_version_id, parent_resource_id, source_query,
                created_at, updated_at,
                kind, content_hash, file_rel_path, readonly,
                promoted_from, source_connection_id, source_table, definition_sql, archived_at,
                payload_mode, source_path
            ) VALUES (?, ?, ?, ?, '{}', ?, ?, ?, ?, 1, NULL, NULL, NULL, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?)
            "#,
                rusqlite::params![
                    &id,
                    &input.resource_type,
                    &input.name,
                    &input.alias,
                    &input.scope,
                    clamp_file_size(input.file_size),
                    input.row_count,
                    input.column_count,
                    &now,
                    &now,
                    input.kind.as_db_str(),
                    &input.content_hash,
                    &input.file_rel_path,
                    &input.binding.promoted_from,
                    &input.binding.source_connection_id,
                    &input.binding.source_table,
                    &input.definition_sql,
                    &now,
                    input.payload_mode.as_db_str(),
                    &input.source_path,
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

    /// 改**别名**（详情面板头部点别名；原型 §3.1）。
    ///
    /// 与 [`rename_resource`](Self::rename_resource) 同一取舍：只动 `alias` 一列，
    /// **不涨版本、不写快照**——别名是**给人看的第二个叫法**，不是内容。`None`（或空串）
    /// = 清除别名（存 NULL，不存空串：空串会在界面上变成“有一行空别名”）。
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
        analysis: Option<&crate::analysis::AnalysisFacts>,
    ) -> Result<AnalyticsResource, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        let now = Utc::now().to_rfc3339();
        // 分析表：定义与规模随新内容一起更新（结构可能真的变了——这正是它的意义）；
        // 文件型：这三列保持原样（分别是 None / 旧值，不该被清掉）。
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

        self.get_resource_by_id(id).await
    }
}
