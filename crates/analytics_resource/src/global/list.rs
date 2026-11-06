//! 全局清单查询与行映射。

use super::*;

/// 列出全局资源（scope = 'global' 的活跃行，按更新时间倒序）。
///
/// 展示层（资源列表「全局实例」维度）与宿主「升级到全局」后的确认都用它。
/// 列序与 [`RESOURCE_COLUMNS`] 严格一致，复用 [`map_resource_row`]。
pub fn list_global_resources(
    store: &GlobalInstanceStore,
) -> Result<Vec<AnalyticsResource>, CoreError> {
    store.with_sqlite(|conn| {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM analytics_resources
                 WHERE scope = 'global' AND deleted_at IS NULL
                 ORDER BY updated_at DESC",
                RESOURCE_COLUMNS
            ))
            .map_err(sqlite_err("list_global"))?;
        let rows = stmt
            .query_map([], |row| map_resource_row(row))
            .map_err(sqlite_err("list_global_rows"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sqlite_err("list_global_collect"))
    })
}

/// 全局登记行 → 展示行（§13 展示层）。
///
/// 全局行是**只读 + 可分发**：没有项目标签 / 分组 / 状态扫描语义，tail 只给
/// 体积与更新时间（来源详情在版本链，详情面板接入是后续批次）。`scope = Global`
/// 让行渲染给出「全局」徽标并置灰全部项目侧动作。
pub fn to_global_rows(
    resources: &[AnalyticsResource],
    now: chrono::DateTime<Utc>,
) -> Vec<crate::resource_view::ArchiveRow> {
    resources
        .iter()
        .map(|resource| {
            let kind = crate::model::ArchiveKind::from_db_str(&resource.kind);
            let tail = {
                let mut parts = Vec::new();
                if let Some(size) = resource.file_size {
                    parts.push(crate::present::format_size(Some(size)));
                }
                parts.push(crate::present::format_relative_time(
                    now,
                    resource.updated_at,
                ));
                parts.join(" · ")
            };
            crate::resource_view::ArchiveRow {
                id: resource.id.clone(),
                name: resource.name.clone(),
                scope: crate::model::RowScope::Global,
                alias: None,
                kind,
                version: resource.version,
                status: crate::model::ArchiveStatus::Normal,
                tail,
                source_table: resource.source_table.clone(),
                tags: Vec::new(),
                folder_id: None,
                updated_epoch: resource.updated_at.timestamp(),
                archived_epoch: resource.archived_at.map(|when| when.timestamp()),
                size_bytes: resource.file_size.map(i64::from),
            }
        })
        .collect()
}

