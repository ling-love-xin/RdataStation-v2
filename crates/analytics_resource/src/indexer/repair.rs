//! 修复动作：补登、接受当前内容、删除孤儿登记。

use super::*;

impl<'a> IndexRepair<'a> {
    /// **补登**（人工确认后调用）：把已存在的本体登记为存档，指纹现算。
    ///
    /// 不搬动文件——本体本来就在 `resources/` 里；来源绑定留空（无从得知）。
    pub async fn adopt_file(
        &self,
        rel_path: &str,
        name: Option<&str>,
        kind: ArchiveKind,
    ) -> Result<AnalyticsResource, CoreError> {
        let path = self.payload.resolve(rel_path)?;
        if !path.is_file() {
            return Err(repair_err("adopt_file", "该路径下没有本体文件"));
        }
        if let Some(existing) = self.store.find_archive_by_rel_path(rel_path).await? {
            return Err(repair_err(
                "adopt_file",
                &format!("该本体已被存档「{}」登记", existing.name),
            ));
        }

        let content_hash = self.payload.content_hash(&path).await?;
        let file_size = self.payload.file_size(&path).await?;
        let display_name = name.map(str::to_string).unwrap_or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_else(|| rel_path.to_string())
        });

        self.store
            .insert_archive_with_event(
                NewArchiveInput {
                resource_type: kind.as_db_str().to_string(),
                name: display_name,
                alias: None,
                kind,
                content_hash,
                // 补登是文件型修复路径：没有分析配方与规模可填（不编造）。
                definition_sql: None,
                row_count: None,
                column_count: None,
                file_rel_path: rel_path.to_string(),
                // 补登的文件就在 `resources/` 下（copy 语义）；不做 reference。
                payload_mode: crate::model::PayloadMode::Copy,
                source_path: None,
                file_size,
                binding: ArchiveBinding::default(),
                scope: "project".to_string(),
                },
                Some("索引修复：补登未登记本体"),
            )
            .await
    }

    /// **接受当前内容**（`ContentChanged` 的修复）：指纹换成实际值、版本 +1、重新加只读。
    ///
    /// 诚实说明：这一版的"内容副本"**不可得**——旧内容在外部被覆盖时就已经没了，
    /// 因此该历史版本只有元数据（界面按"副本缺失"呈现，而不是假装能还原）。
    pub async fn accept_current_content(
        &self,
        resource_id: &str,
    ) -> Result<AnalyticsResource, CoreError> {
        let current = self.store.get_resource_by_id(resource_id).await?;
        let rel_path = current
            .file_rel_path
            .clone()
            .ok_or_else(|| repair_err("accept_current_content", "该存档没有本体路径"))?;
        let path = self.payload.resolve(&rel_path)?;
        if !path.is_file() {
            return Err(repair_err(
                "accept_current_content",
                "本体不存在：应走「有记录、无本体」的处理（还原或删除记录）",
            ));
        }

        // 外部改动往往把只读属性也带走了，这里重新加回。
        if let Err(e) = self.payload.set_readonly(&path, true).await {
            tracing::warn!(error = %e, path = %path.display(), "恢复只读属性失败");
        }

        let actual_hash = self.payload.content_hash(&path).await?;
        // 实际内容也跟着换：体积必须与指纹同批更新，否则列表里的「大小」永远停在旧值。
        let actual_size = self.payload.file_size(&path).await?;
        let snapshot = serde_json::to_string(&current).map_err(|e| {
            CoreError::storage(shared::error::StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;
        // 与主归档路径同一纪律（P0-1）：快照 + 索引 +1 + 履历事件同一事务。
        // 修复动作更要留痕——「这版的指纹为什么变了」正是半年后最容易被问到的问题。
        self.store
            .commit_new_version(
                resource_id,
                current.version,
                &snapshot,
                &actual_hash,
                actual_size,
                None,
                crate::model::TimelineEventKind::Validated,
                Some("索引修复：接受本体的当前内容（旧内容已不可得）"),
            )
            .await
    }

    /// **删除孤儿登记**（`MissingPayload` 的修复）：本体都没了，直接删行，不进回收站。
    pub async fn remove_orphan_record(&self, resource_id: &str) -> Result<(), CoreError> {
        self.store.remove_orphan_record(resource_id).await
    }
}
