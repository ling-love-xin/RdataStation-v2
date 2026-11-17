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
    let version_timeline = detail_version_timeline(
        resource,
        status,
        recent_versions,
        extras.version_copies.as_deref(),
    );
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
        version_timeline,
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

/// 版本**时间线**的节点：当前版本在最前，其后是最近几条历史版本。
///
/// 为什么当前版本也进时间线：详情里点节点看“那一版是什么样”，最常想看的正是现在这一版
/// （它的体积 / 指纹 / 本体还在不在）；只列历史会逼用户去别处找当前版本的信息。
/// 节点数上限 = 当前 1 + 历史 [`DETAIL_RECENT_VERSIONS`] 条，再多就去版本对话框看。
///
/// 体积口径与行/详情一致：文件型给体积、分析表型给规模、快照解析不出来**不补造**
/// （时间来自版本行本身，可信；体积留空）。
fn detail_version_timeline(
    resource: &AnalyticsResource,
    status: ArchiveStatus,
    versions: &[crate::store::rows::ResourceVersion],
    copies: Option<&[i32]>,
) -> Vec<DetailVersionLine> {
    // 先按时间线顺序摆好节点（当前 + 历史），再逐条与“下一节点”（版本号 -1）比差异。
    let mut nodes: Vec<TimelineNode> = Vec::with_capacity(versions.len() + 1);
    nodes.push(TimelineNode {
        version: resource.version,
        time_label: format_timestamp(resource.archived_at.unwrap_or(resource.updated_at)),
        // 当前节点按种类给量：表给规模、文件给体积（快照里只记了体积的历史版同理）。
        size_label: timeline_size_label(
            resource.row_count,
            resource.column_count,
            resource.file_size,
        ),
        file_size: resource.file_size,
        hash: resource.content_hash.clone(),
        is_current: true,
        // 当前版本的“副本”就是本体：本体缺位时这一版也还原不了（`resources/` 下没有它）。
        has_copy: status != ArchiveStatus::Missing,
    });
    for version in versions.iter().take(DETAIL_RECENT_VERSIONS) {
        let snapshot = serde_json::from_value::<AnalyticsResource>(version.snapshot.clone()).ok();
        nodes.push(TimelineNode {
            version: version.version,
            time_label: format_timestamp(version.created_at),
            size_label: snapshot.as_ref().map_or_else(String::new, |record| {
                timeline_size_label(record.row_count, record.column_count, record.file_size)
            }),
            file_size: snapshot.as_ref().and_then(|record| record.file_size),
            hash: snapshot.as_ref().and_then(|record| record.content_hash.clone()),
            is_current: false,
            // 没问到副本清单时按“还在”呈现（`None` = 不知道，不是“没有”）：
            // 误报“副本已裁剪”比漏报更糟——用户会以为能还原的版本不能还原。
            has_copy: copies.map_or(true, |copies| copies.contains(&version.version)),
        });
    }
    // 版本表按新在前给，但别依赖它：时间线必须严格降序（差异文案是“较 vN-1”）。
    nodes.sort_unstable_by(|a, b| b.version.cmp(&a.version));

    nodes
        .iter()
        .enumerate()
        .map(|(index, node)| DetailVersionLine {
            version: node.version,
            time_label: node.time_label.clone(),
            size_label: node.size_label.clone(),
            is_current: node.is_current,
            hash_short: crate::detail_view::short_hash(node.hash.as_deref()),
            delta_label: nodes
                .get(index + 1)
                .filter(|previous| previous.version == node.version - 1)
                .map(|previous| {
                    super::versions::version_delta_label(
                        previous.version,
                        previous.file_size,
                        previous.hash.as_deref(),
                        node.file_size,
                        node.hash.as_deref(),
                    )
                })
                .unwrap_or_default(),
            has_copy: node.has_copy,
        })
        .collect()
}

/// 时间线节点的取数中间态（`DetailVersionLine` 的输入）。
struct TimelineNode {
    version: i32,
    time_label: String,
    size_label: String,
    /// 只用于算「大小±」的差值（表型没有这个值，差值就不比）。
    file_size: Option<i32>,
    hash: Option<String>,
    is_current: bool,
    has_copy: bool,
}

/// 一条版本节点的“量”：记了行列就给规模（表型），否则给体积（文件型）。
///
/// 与列表行 / 详情头部同一口径，避免同一版在三处显示成三种样子。
fn timeline_size_label(rows: Option<i32>, columns: Option<i32>, file_size: Option<i32>) -> String {
    if rows.is_some() || columns.is_some() {
        format_scale(rows, columns)
    } else {
        format_size(file_size)
    }
}
