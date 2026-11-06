//! 列表行：快照输入、行构造、标签芯片与标签选项。

use super::*;

/// 快照的取数输入：宿主在工作线程上备好的一切（字典与映射都是一次查完的结果）。
///
/// 用结构体而不是位置参数：这里的字段会随 Phase 2/3 继续长（标签、分组已各占两份），
/// 位置参数到七八个时调用点就没人看得懂了。
pub struct SnapshotInputs<'a> {
    pub resources: &'a [AnalyticsResource],
    /// 全局实例行（§13）：已在调用侧转成 `ArchiveRow`（scope = Global）；与项目行
    /// 分开存，不参与计数 / 标签 / 分组统计。
    pub global_rows: Vec<ArchiveRow>,
    /// 全局实例**原始登记行**（与 `global_rows` 一一对应）：详情面板要展示基本信息，
    /// 行模型已丢原始字段，所以这里单独带回一份装配轻量 `ArchiveDetail`。
    pub global_resources: Vec<AnalyticsResource>,
    pub statuses: &'a ArchiveStatuses,
    pub history_counts: &'a VersionCounts,
    pub tags: &'a ResourceTags,
    pub tag_dictionary: Vec<TagOption>,
    pub folders: &'a FolderMemberships,
    pub group_dictionary: Vec<GroupOption>,
    /// 内容预览（宿主读完推来；缺项 = 没读到 / 不预览 → 「仅元信息」）。
    pub previews: &'a PreviewMap,
    pub read_only: bool,
    pub now: DateTime<Utc>,
}
/// 单行转换（状态缺省 `Normal`；标签 id 与分组从映射取，缺即无）。
pub fn to_row(
    resource: &AnalyticsResource,
    statuses: &ArchiveStatuses,
    tags: &ResourceTags,
    folders: &FolderMemberships,
    now: DateTime<Utc>,
) -> ArchiveRow {
    let kind = ArchiveKind::from_db_str(&resource.kind);
    let status = statuses
        .get(&resource.id)
        .copied()
        .unwrap_or(ArchiveStatus::Normal);
    ArchiveRow {
        id: resource.id.clone(),
        name: resource.name.clone(),
        // 项目行：本行是项目实例（全局行由双实例路径单独构造，见 resource_jobs）。
        scope: crate::model::RowScope::Project,
        // 别名与来源表：行上不显示，但它们是**搜索匹配面**的一员（原型 §2.2，见 `filter::search_haystack`）。
        alias: resource.alias.clone(),
        kind,
        version: resource.version,
        status,
        tail: tail_for(resource, kind, now),
        source_table: resource.source_table.clone(),
        // 标签**带名字**一起装：筛标签维比 id（名字会改），搜索比名字——一份数据两处用。
        // 详情面板的 chips 也直接用这份（见 `build_snapshot`），不再各算一遍。
        tags: tags
            .get(&resource.id)
            .map(|list| tag_chips(list))
            .unwrap_or_default(),
        folder_id: folders.get(&resource.id).cloned(),
        // 原始值原样带上（视图层排序只看它们，不去解析格式化过的尾巴）。
        updated_epoch: resource.updated_at.timestamp(),
        archived_epoch: resource.archived_at.map(|when| when.timestamp()),
        size_bytes: resource.file_size.map(i64::from),
    }
}
/// 标签行模型 → chip（视图只认 id + 名字，不关心 color / scope）。
pub fn tag_chips(tags: &[crate::store::rows::AnalyticsTag]) -> Vec<ArchiveTagChip> {
    tags.iter()
        .map(|tag| ArchiveTagChip {
            id: tag.id.clone(),
            name: tag.name.clone(),
        })
        .collect()
}
/// 标签字典 + 用量 → 视图层的选项（筛选菜单与打标对话框共用）。
pub fn tag_options(
    tags: &[crate::store::rows::AnalyticsTag],
    counts: &std::collections::HashMap<String, i64>,
) -> Vec<TagOption> {
    tags.iter()
        .map(|tag| TagOption {
            id: tag.id.clone(),
            name: tag.name.clone(),
            count: counts.get(&tag.id).copied().unwrap_or(0).max(0) as usize,
        })
        .collect()
}
