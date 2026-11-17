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
    /// 当前分组 id（与 `group` 同源；分组字典里认不出时给 `None`，与名字一致）。
    pub group_id: Option<String>,
    /// 可选分组（「移动…」菜单的选择项；全局行给空）。
    pub groups: Vec<DetailGroupOption>,
    /// 内容预览。
    pub preview: Preview,
    /// 全局行的使用面摘要（项目行与没人用过的全局行给空串 → 详情不画那一行）。
    pub dispatch_label: String,
    /// 该行**还有内容副本**的版本号；`None` = 没问到（不是“一份都没有”）。
    ///
    /// 两者要分开：没问到不能对用户说“只有 0 份可还原”，也不能把每一版都标成“副本已裁剪”。
    pub version_copies: Option<Vec<i32>>,
}
/// 单行 → 详情快照（详情面板只读，格式化在这里做完）。
///
/// 与 [`to_row`](crate::present::to_row) 同一纪律：取值全部来自行模型与宿主推来的映射，
/// 不在渲染期算。
pub fn to_detail(
    resource: &AnalyticsResource,
    status: ArchiveStatus,
    history_count: i64,
    recent_versions: &[crate::store::rows::ResourceVersion],
    extras: DetailExtras,
) -> ArchiveDetail {
    // 真实档位看 `analytics_table` 列（表型行的 `kind` 串仍是 analysis，见迁移 031）。
    let kind = ArchiveKind::resolve(&resource.kind, resource.analytics_table.as_deref());
    // 与行的尾巴同一口径：文件型给体积、分析表/分析库表给规模、引用型不给（不假装有值）。
    let size_label = match kind {
        ArchiveKind::File => format_size(resource.file_size),
        ArchiveKind::AnalyticsTable => {
            format_scale(resource.row_count, resource.column_count)
        }
        ArchiveKind::TableRef => String::new(),
    };
    // 先算（`extras` 的其余字段随后要按值取走）。
    let recent_version_lines = detail_version_lines(recent_versions, extras.version_copies.as_deref());
    let version_copies = extras.version_copies;
    ArchiveDetail {
        assertion: None,
        lifecycle: Default::default(),
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
        created_label: format_timestamp(resource.created_at),
        modified_label: format_timestamp(resource.updated_at),
        archived_label: resource
            .archived_at
            .map(format_timestamp)
            .unwrap_or_default(),
        promoted_from: resource.promoted_from.clone(),
        origin_label: crate::model::describe(
            resource.origin_kind.as_deref(),
            resource.origin_ref.as_deref(),
        ),
        source_connection_id: resource.source_connection_id.clone(),
        source_table: resource.source_table.clone(),
        content_hash: resource.content_hash.clone(),
        payload_rel_path: resource.file_rel_path.clone(),
        analytics_table: resource.analytics_table.clone(),
        has_definition: resource.definition_sql.is_some(),
        definition_sql: resource
            .definition_sql
            .as_deref()
            .map(str::trim)
            .filter(|sql| !sql.is_empty())
            .map(str::to_string),
        // 没有历史版本时给空串：`detail_rows` 据此跳过"版本"分区（不产生空行）。
        history_label: if history_count > 0 {
            // 有多少历史版**还能还原**（副本还在）：裁剪只删副本不删版本行，
            // 只报"N 个历史版本"会让用户以为每一版都点得开。
            // 只数**当前版本之前**的副本（当前版本那不是"历史"，它的本体就在 `resources/` 下）；
            // 没问到副本清单（`None`）时不加括号——不能把"不知道"说成"0 份可还原"。
            let restorable = version_copies
                .as_deref()
                .map(|copies| copies.iter().filter(|v| **v < resource.version).count() as i64);
            match restorable {
                Some(restorable) if restorable < history_count => {
                    format!("{history_count} 个历史版本（{restorable} 份可还原）")
                }
                _ => format!("{history_count} 个历史版本"),
            }
        } else {
            String::new()
        },
        recent_versions: recent_version_lines,
        groups: extras.groups,
        // 名字与 id 一起给：字典里认不出的 id 两个都给「无」，不出现“名字空、id 有”的错位。
        group_id: if extras.group.is_some() {
            extras.group_id
        } else {
            None
        },
        // 标签与分组：标签由调用方给的映射提供（`tags_by_resource`，一次查完）；
        // 分组名由 `folders_by_resource` + 分组字典拼出（两个映射都在宿主手里）。
        tags: extras.tags,
        group: extras.group,
        // 内容预览（原型 §3.1）：宿主读完推来，装配层只负责搬（缺项 = 仅元信息）。
        preview: extras.preview,
        dispatch_label: extras.dispatch_label,
    }
}

/// 最近几条历史版本 → 详情「版本」区的摘要行（最多 [`DETAIL_RECENT_VERSIONS`] 条）。
///
/// 体积口径与行/详情一致：文件型给体积、分析表型给规模、引用型不给（不假装有值）。
/// 快照解析不出来**不补造**——时间来自版本行本身（可信），体积留空。
fn detail_version_lines(
    versions: &[crate::store::rows::ResourceVersion],
    copies: Option<&[i32]>,
) -> Vec<DetailVersionLine> {
    versions
        .iter()
        .take(DETAIL_RECENT_VERSIONS)
        .map(|version| {
            let snapshot =
                serde_json::from_value::<AnalyticsResource>(version.snapshot.clone()).ok();
            // 历史版本按**快照里有什么**给标签：记了行数 / 列数就给规模，只记了体积就给
            // 体积。不按种类分——种类收成两档后，同一档里这两种情况都存在。
            let size_label = match snapshot.as_ref() {
                Some(record) if record.row_count.is_some() || record.column_count.is_some() => {
                    format_scale(record.row_count, record.column_count)
                }
                Some(record) => format_size(record.file_size),
                None => String::new(),
            };
            DetailVersionLine {
                version: version.version,
                time_label: format_timestamp(version.created_at),
                size_label,
                // 没问到副本清单时按“还在”呈现（`None` = 不知道，不是“没有”）：
                // 误报“副本已裁剪”比漏报更糟——用户会以为能还原的版本不能还原。
                has_copy: copies.map_or(true, |copies| copies.contains(&version.version)),
            }
        })
        .collect()
}
