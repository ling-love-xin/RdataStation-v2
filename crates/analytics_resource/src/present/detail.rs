//! 详情：附加信息与详情构造。

use super::*;

/// 详情装配的**随行信息**（宿主取数得到的按行数据；装配层只搬不查）。
///
/// 为什么打包成一个结构体：这三件事都来自宿主侧的映射（标签 / 分组字典 / 预览），
/// 分别当参数逐个加会让 `to_detail` 长到七个位置参数——又多又同样意思。
pub struct DetailExtras {
    /// 标签 chips（带 id：去标要用它）。
    pub tags: Vec<ArchiveTagChip>,
    /// 分组名（`None` = 未分组；分组字典里认不出的 id 也当未分组——脏数据不能变成一行“分组：?”）。
    pub group: Option<String>,
    /// 内容预览。
    pub preview: Preview,
}
/// 单行 → 详情快照（详情面板只读，格式化在这里做完）。
///
/// 与 [`to_row`](crate::present::to_row) 同一纪律：取值全部来自行模型与宿主推来的映射，
/// 不在渲染期算。
pub fn to_detail(
    resource: &AnalyticsResource,
    status: ArchiveStatus,
    history_count: i64,
    extras: DetailExtras,
) -> ArchiveDetail {
    let kind = ArchiveKind::from_db_str(&resource.kind);
    // 与行的尾巴同一口径：文件型给体积、分析表型给规模、引用型不给（不假装有值）。
    let size_label = match kind {
        ArchiveKind::File => format_size(resource.file_size),
        ArchiveKind::Analysis => format_scale(resource.row_count, resource.column_count),
        ArchiveKind::TableRef => String::new(),
    };
    ArchiveDetail {
        id: resource.id.clone(),
        name: resource.name.clone(),
        alias: resource.alias.clone(),
        kind,
        version: resource.version,
        status,
        readonly: resource.readonly != 0,
        payload_mode: PayloadMode::from_db_str(&resource.payload_mode),
        source_path: resource.source_path.clone(),
        scope: RowScope::from_db_str(&resource.scope),
        size_label,
        modified_label: format_timestamp(resource.updated_at),
        archived_label: resource
            .archived_at
            .map(format_timestamp)
            .unwrap_or_default(),
        promoted_from: resource.promoted_from.clone(),
        source_connection_id: resource.source_connection_id.clone(),
        source_table: resource.source_table.clone(),
        content_hash: resource.content_hash.clone(),
        payload_rel_path: resource.file_rel_path.clone(),
        // 没有历史版本时给空串：`detail_rows` 据此跳过"版本"分区（不产生空行）。
        history_label: if history_count > 0 {
            format!("{history_count} 个历史版本")
        } else {
            String::new()
        },
        // 标签与分组：标签由调用方给的映射提供（`tags_by_resource`，一次查完）；
        // 分组名由 `folders_by_resource` + 分组字典拼出（两个映射都在宿主手里）。
        tags: extras.tags,
        group: extras.group,
        // 内容预览（原型 §3.1）：宿主读完推来，装配层只负责搬（缺项 = 仅元信息）。
        preview: extras.preview,
    }
}
