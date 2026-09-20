//! 呈现层：索引行 → 面板快照（纯函数，零 I/O、零 GPUI 依赖）。
//!
//! 宿主桥的"重活"都在这儿：把 [`AnalyticsResource`] 转成 [`ArchiveRow`]（含字段优先级规则的
//! 尾巴文案）并统计 [`ArchiveCounts`]。剩下留给 workbench 的只有几行胶水（取数 → 调本模块 → 推面板），
//! 这样"怎么显示"能在没有窗口、没有数据库的环境里被单测锁住。
//!
//! 状态来源分两部分：**本体健康**（缺失 / 内容已变）由索引修复扫描给出，不在行模型里——
//! 故 [`build_snapshot`] 接收一份 `statuses` 映射（缺省即 `Normal`）。

use chrono::{DateTime, Utc};

use engine::persistence::trash::{TrashEntry, TrashKind};

use crate::AnalyticsResource;
use crate::detail_view::{ArchiveDetail, ArchiveTagChip};
use crate::dialogs::index_repair::{RepairGroup, RepairRow};
use crate::dialogs::trash::TrashRow;
use crate::dialogs::version::VersionRow;
use crate::model::{ArchiveKind, ArchiveStatus, ORIGIN_RESOURCES};
use crate::payload::RESOURCES_DIR_NAME;
use crate::resource_view::{ArchiveCounts, ArchiveRow, GroupOption, ResourcesSnapshot, TagOption};

/// 按资源的标签映射：`resource_id → 标签行`（来自 `AnalyticsResourceStore::tags_by_resource`，
/// 一次查完；逐行查会把一次刷新变成 N+1 次查询）。
pub type ResourceTags = std::collections::HashMap<String, Vec<crate::models::AnalyticsTag>>;

/// 单行状态映射：`resource_id → 状态`（来自 `IndexRepair::scan` 的结果）。
pub type ArchiveStatuses = std::collections::HashMap<String, ArchiveStatus>;

/// 历史版本数映射：`resource_id → 条数`（来自 `AnalyticsResourceStore::version_counts`）。
pub type VersionCounts = std::collections::HashMap<String, i64>;

/// 大小文案：按 1024 进制分档，`< 1 KB` 用字节（避免出现 `0.0 KB` 这种没信息量的值）。
pub fn format_size(bytes: Option<i32>) -> String {
    let Some(bytes) = bytes else {
        return String::new();
    };
    let bytes = bytes.max(0) as f64;
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bytes < KB {
        format!("{} B", bytes as i64)
    } else if bytes < MB {
        format!("{:.1} KB", bytes / KB)
    } else if bytes < GB {
        format!("{:.1} MB", bytes / MB)
    } else {
        format!("{:.1} GB", bytes / GB)
    }
}

/// 规模文案（分析表型）：`12,480 行 × 18 列`；缺失任一维度时退化为能给出的那部分。
pub fn format_scale(rows: Option<i32>, columns: Option<i32>) -> String {
    match (rows, columns) {
        (Some(rows), Some(columns)) => format!("{} 行 × {} 列", with_thousands(rows), columns),
        (Some(rows), None) => format!("{} 行", with_thousands(rows)),
        (None, Some(columns)) => format!("{} 列", columns),
        (None, None) => String::new(),
    }
}

/// 千分位（只在整数文案上用；避免引入依赖）。
fn with_thousands(value: i32) -> String {
    let digits = value.max(0).to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 相对时间：刚归档的东西说"刚刚"，久远的说"3 个月前"。
///
/// 刻意不显示绝对时间：面板窄（240px），绝对时间放不下；悬浮显示完整值由渲染层补。
pub fn format_relative_time(now: DateTime<Utc>, then: DateTime<Utc>) -> String {
    let seconds = (now - then).num_seconds();
    if seconds < 0 {
        // 时钟回拨 / 时区异常：不显示"-3 分钟前"这种怪东西。
        return "刚刚".to_string();
    }
    const MINUTE: i64 = 60;
    const HOUR: i64 = MINUTE * 60;
    const DAY: i64 = HOUR * 24;

    if seconds < MINUTE {
        "刚刚".to_string()
    } else if seconds < HOUR {
        format!("{} 分钟前", seconds / MINUTE)
    } else if seconds < DAY {
        format!("{} 小时前", seconds / HOUR)
    } else if seconds < DAY * 30 {
        format!("{} 天前", seconds / DAY)
    } else if seconds < DAY * 365 {
        format!("{} 个月前", seconds / (DAY * 30))
    } else {
        format!("{} 年前", seconds / (DAY * 365))
    }
}

/// 绝对时间文案（详情面板用）：`2026-09-16 14:03`。
///
/// 行上用相对时间（面板窄、扫读快），详情面板是看"归档凭证"的地方，要的是精确值。
/// 时区口径与仓库既有格式化一致（`connector.rs` 同）：直接展示库里的 UTC 值，
/// 本地时区转换是全局议题，不在本模块单独改。
pub fn format_timestamp(when: DateTime<Utc>) -> String {
    when.format("%Y-%m-%d %H:%M").to_string()
}

/// 一行存档的尾巴字段（**字段优先级**：大小/规模 > 相对时间；两者都无则空串）。
pub fn tail_for(resource: &AnalyticsResource, kind: ArchiveKind, now: DateTime<Utc>) -> String {
    let detail = match kind {
        ArchiveKind::File => format_size(resource.file_size),
        ArchiveKind::Analysis => format_scale(resource.row_count, resource.column_count),
        // 引用型没有可校验的指纹与体积：明说"无指纹"，而不是留空让人以为"还没算"。
        ArchiveKind::TableRef => "无指纹".to_string(),
    };
    let modified = format_relative_time(now, resource.updated_at);
    crate::resource_view::row_tail(&detail, &modified, resource.version)
}

/// 回收站快照（宿主工作线程用）：本模块的条目 + 别人条目的统计。
///
/// “别人的条目”只给 `origin` 与条数：显示名是宿主的事（crate 不认识别的模块叫什么）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashSnapshot {
    pub rows: Vec<TrashRow>,
    /// `origin` → 条数（不含本模块）。
    pub foreign: Vec<(String, usize)>,
}

/// 回收站条目 → 对话框行（**只收本模块的**：别人的条目在这里没有可做的动作）。
///
/// 排序沿用 `ProjectTrash::list` 的口径（删除时间倒序），这里不再排一次——两处各排一次，
/// 将来口径一变就会打架。
pub fn build_trash_snapshot(entries: &[TrashEntry]) -> TrashSnapshot {
    let mut rows = Vec::new();
    let mut foreign: Vec<(String, usize)> = Vec::new();
    for entry in entries {
        let manifest = &entry.manifest;
        if manifest.origin != ORIGIN_RESOURCES {
            match foreign
                .iter_mut()
                .find(|(origin, _)| origin == &manifest.origin)
            {
                Some((_, count)) => *count += 1,
                None => foreign.push((manifest.origin.clone(), 1)),
            }
            continue;
        }
        // “原位置”回答的是“还原到哪”：rel_path 为空（极端旧条目）时退回到文件名。
        let original_label = if manifest.original_rel_path.trim().is_empty() {
            manifest.name.clone()
        } else {
            format!("{RESOURCES_DIR_NAME}/{}", manifest.original_rel_path)
        };
        rows.push(TrashRow {
            trash_id: manifest.id.clone(),
            name: manifest.name.clone(),
            original_label,
            kind_label: match manifest.kind {
                TrashKind::File => "文件".to_string(),
                TrashKind::Folder => "目录".to_string(),
            },
            // 绝对时间：回收站是查凭证的地方（行上用相对时间的口径不适用于对话框）。
            time_label: format_timestamp(manifest.deleted_at),
            size_label: match manifest.kind {
                TrashKind::File => format_size(Some(manifest.size.min(i32::MAX as u64) as i32)),
                // 目录的“大小”没意义（要递归统计才有值），留空而不是写 0。
                TrashKind::Folder => String::new(),
            },
        });
    }
    TrashSnapshot { rows, foreign }
}

/// 按资源的分组映射：`resource_id → folder_id`（来自 `folders_by_resource`，一次查完；
/// 单层分组下最多一条，所以是 `Option` 语义的映射而不是列表）。
pub type FolderMemberships = std::collections::HashMap<String, String>;

/// 快照的取数输入：宿主在工作线程上备好的一切（字典与映射都是一次查完的结果）。
///
/// 用结构体而不是位置参数：这里的字段会随 Phase 2/3 继续长（标签、分组已各占两份），
/// 位置参数到七八个时调用点就没人看得懂了。
pub struct SnapshotInputs<'a> {
    pub resources: &'a [AnalyticsResource],
    pub statuses: &'a ArchiveStatuses,
    pub history_counts: &'a VersionCounts,
    pub tags: &'a ResourceTags,
    pub tag_dictionary: Vec<TagOption>,
    pub folders: &'a FolderMemberships,
    pub group_dictionary: Vec<GroupOption>,
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
pub fn tag_chips(tags: &[crate::models::AnalyticsTag]) -> Vec<ArchiveTagChip> {
    tags.iter()
        .map(|tag| ArchiveTagChip {
            id: tag.id.clone(),
            name: tag.name.clone(),
        })
        .collect()
}

/// 标签字典 + 用量 → 视图层的选项（筛选菜单与打标对话框共用）。
pub fn tag_options(
    tags: &[crate::models::AnalyticsTag],
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

/// 单行 → 详情快照（详情面板只读，格式化在这里做完）。
///
/// 与 [`to_row`](crate::present::to_row) 同一纪律：取值全部来自行模型与宿主推来的映射，
/// 不在渲染期算。
pub fn to_detail(
    resource: &AnalyticsResource,
    status: ArchiveStatus,
    history_count: i64,
    tags: &[ArchiveTagChip],
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
        // 分组属 Phase 2 余下那一刀（需要 `analytics_resource_folder` 的按行查询），此处不编造。
        tags: tags.to_vec(),
        group: None,
    }
}

/// 版本差异的中间态：快照与资源行两种来源先归一，再算差异。
struct Candidate {
    version: i32,
    size: Option<i32>,
    hash: Option<String>,
    time: DateTime<Utc>,
    is_current: bool,
    has_copy: bool,
}

/// 版本历史行（对话框用）：当前版本 + 历史版本，降序。
///
/// 合成两件事：
///
/// 1. **当前版本也占一行**——写前快照语义下版本表里没有当前版本（它在资源行上），
///    只列历史会让人以为“最新版不见了”；
/// 2. **相邻版本差异**（大小 ±N · 指纹是否变）——不做行级 diff，两个值就够一眼看出
///    “这一版改了多大”；
///
/// `payload_ok` = 当前本体在位（当前版本行的“副本”就是本体）；`copies` = 有内容副本的
/// 历史版本号（`PayloadStore::version_copies`）——两者都是宿主在取数线程上问过文件系统
/// 才有的，不在渲染期算。
///
/// 历史行的大小 / 指纹只能从快照 JSON 里取（表结构如此）；解析失败就**留空**，
/// 不拿当前版本的值顶替——那会伪造历史。
pub fn build_version_rows(
    current: &AnalyticsResource,
    versions: &[crate::models::ResourceVersion],
    copies: &[i32],
    payload_ok: bool,
) -> Vec<VersionRow> {
    let mut candidates = vec![Candidate {
        version: current.version,
        size: current.file_size,
        hash: current.content_hash.clone(),
        time: current.archived_at.unwrap_or(current.updated_at),
        is_current: true,
        has_copy: payload_ok,
    }];
    for version in versions {
        // 快照是当时那条资源记录的序列化；字段结构随迁移演进，解析不出来不算错。
        let snapshot = serde_json::from_value::<AnalyticsResource>(version.snapshot.clone()).ok();
        candidates.push(Candidate {
            version: version.version,
            size: snapshot.as_ref().and_then(|record| record.file_size),
            hash: snapshot
                .as_ref()
                .and_then(|record| record.content_hash.clone()),
            time: version.created_at,
            is_current: false,
            has_copy: copies.contains(&version.version),
        });
    }
    candidates.sort_unstable_by(|a, b| b.version.cmp(&a.version));

    candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| VersionRow {
            version: candidate.version,
            is_current: candidate.is_current,
            time_label: format_timestamp(candidate.time),
            size_label: format_size(candidate.size),
            hash_short: crate::detail_view::short_hash(candidate.hash.as_deref()),
            has_copy: candidate.has_copy,
            // 与**上一版**（版本号 -1）比：列表里它的下一行（降序）。
            delta_label: candidates
                .get(index + 1)
                .filter(|previous| previous.version == candidate.version - 1)
                .map(|previous| version_delta(previous.version, previous, candidate))
                .unwrap_or_default(),
        })
        .collect()
}

/// 相邻两版的差异文案：`较 v1 大小+1.2 KB · 指纹已变`。
///
/// 两侧都有值才比（快照里缺字段就不编）；两样都比不出来时给空串。
fn version_delta(previous_version: i32, previous: &Candidate, current: &Candidate) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let (Some(now), Some(before)) = (current.size, previous.size) {
        let delta = now - before;
        parts.push(match delta {
            0 => "大小不变".to_string(),
            delta if delta > 0 => format!("大小+{}", format_size(Some(delta))),
            delta => format!("大小-{}", format_size(Some(-delta))),
        });
    }
    if let (Some(now), Some(before)) = (current.hash.as_deref(), previous.hash.as_deref()) {
        parts.push(
            if now == before {
                "指纹相同"
            } else {
                "指纹已变"
            }
            .to_string(),
        );
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("较 v{previous_version} {}", parts.join(" · "))
}

/// 扫描报告 → 修复行（对话框用，按分组顺序排好）。
///
/// 格式化的活都在这里：指纹缩略（复用详情面板的 `short_hash`，前 12 位）与行标题 /
/// 副文案；对话框只渲染与派发动作。
///
/// 顺序：分组按 `RepairGroup::ALL`，组内按本体路径（稳定、与扫描结果同序）。
pub fn build_repair_rows(report: &crate::IndexScanReport) -> Vec<RepairRow> {
    let mut rows: Vec<RepairRow> = report
        .issues
        .iter()
        .map(|issue| match issue {
            crate::IndexIssue::UntrackedFile { rel_path } => RepairRow {
                group: RepairGroup::Untracked,
                title: file_name_of(rel_path),
                detail: format!("本体 resources/{rel_path}：存在但没有登记记录"),
                hash_detail: String::new(),
                rel_path: rel_path.clone(),
                resource_id: None,
            },
            crate::IndexIssue::MissingPayload {
                resource_id,
                name,
                rel_path,
            } => RepairRow {
                group: RepairGroup::Missing,
                title: name.clone(),
                detail: format!("期望位置 resources/{rel_path}：本体不在了"),
                hash_detail: String::new(),
                rel_path: rel_path.clone(),
                resource_id: Some(resource_id.clone()),
            },
            crate::IndexIssue::ContentChanged {
                resource_id,
                name,
                rel_path,
                expected_hash,
                actual_hash,
            } => RepairRow {
                group: RepairGroup::Changed,
                title: name.clone(),
                detail: format!("本体 resources/{rel_path}：内容与登记不符"),
                hash_detail: format!(
                    "登记 {} · 实际 {}",
                    crate::detail_view::short_hash(Some(expected_hash)),
                    crate::detail_view::short_hash(Some(actual_hash))
                ),
                rel_path: rel_path.clone(),
                resource_id: Some(resource_id.clone()),
            },
        })
        .collect();

    rows.sort_by(|a, b| {
        let group = group_order(a.group).cmp(&group_order(b.group));
        group.then_with(|| a.rel_path.cmp(&b.rel_path))
    });
    rows
}

/// 分组顺序（`RepairGroup::ALL` 的下标）。
fn group_order(group: RepairGroup) -> usize {
    RepairGroup::ALL
        .iter()
        .position(|candidate| *candidate == group)
        .unwrap_or(usize::MAX)
}

/// 相对路径 → 文件名（未登记行的标题：路径太长，行里放不下，完整路径在副文案里）。
fn file_name_of(rel_path: &str) -> String {
    rel_path.rsplit('/').next().unwrap_or(rel_path).to_string()
}

/// 组装面板快照：行（按状态与种类计分）+ 计数 + 只读标志 + 标签字典。
///
/// 计数口径与状态行文案一一对应：`缺失` 与 `索引异常`（内容已变）各自计数，
/// 因为它们在上是两个不同的可点入口（都进索引修复，但处理方式不同）。
pub fn build_snapshot(inputs: SnapshotInputs<'_>) -> ResourcesSnapshot {
    let SnapshotInputs {
        resources,
        statuses,
        history_counts,
        tags: tags_by_resource,
        tag_dictionary,
        folders: folders_by_resource,
        group_dictionary,
        read_only,
        now,
    } = inputs;
    let mut counts = ArchiveCounts {
        total: resources.len(),
        ..ArchiveCounts::default()
    };
    let mut rows = Vec::with_capacity(resources.len());
    let mut details = std::collections::HashMap::with_capacity(resources.len());

    for resource in resources {
        let row = to_row(
            resource,
            statuses,
            tags_by_resource,
            folders_by_resource,
            now,
        );
        match row.status {
            ArchiveStatus::Missing => counts.missing += 1,
            ArchiveStatus::ContentChanged => counts.drifted += 1,
            ArchiveStatus::Normal => match row.kind {
                ArchiveKind::File => counts.archived += 1,
                ArchiveKind::Analysis => counts.analysis += 1,
                ArchiveKind::TableRef => counts.table_ref += 1,
            },
        }
        let history_count = history_counts.get(&resource.id).copied().unwrap_or(0);
        // 标签 chips 就用行上那一份（同一装配来源，不会出现“行里有、详情里没有”）。
        details.insert(
            resource.id.clone(),
            to_detail(resource, row.status, history_count, &row.tags),
        );
        rows.push(row);
    }

    ResourcesSnapshot {
        rows,
        counts,
        read_only,
        details,
        tags: tag_dictionary,
        groups: group_dictionary,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArchiveStatuses, FolderMemberships, ResourceTags, SnapshotInputs, VersionCounts,
        build_repair_rows, build_snapshot, build_trash_snapshot, build_version_rows,
        format_relative_time, format_scale, format_size, format_timestamp, tail_for,
    };
    use crate::dialogs::index_repair::RepairGroup;
    use crate::model::{ArchiveKind, ArchiveStatus, ORIGIN_RESOURCES};
    use crate::models::{AnalyticsResource, ResourceVersion};
    use crate::resource_view::TagOption;
    use crate::{IndexIssue, IndexScanReport};
    use chrono::{DateTime, Duration, Utc};
    use engine::persistence::trash::{TrashEntry, TrashKind};
    use serde_json::Value;

    /// 空的标签 / 分组映射：测试里反复要，做成静态的（借用的生周期才够长）。
    fn empty_tags() -> &'static ResourceTags {
        static EMPTY: std::sync::OnceLock<ResourceTags> = std::sync::OnceLock::new();
        EMPTY.get_or_init(ResourceTags::new)
    }

    fn empty_folders() -> &'static FolderMemberships {
        static EMPTY: std::sync::OnceLock<FolderMemberships> = std::sync::OnceLock::new();
        EMPTY.get_or_init(FolderMemberships::new)
    }

    fn row_model(id: &str, kind: &str, file_size: Option<i32>) -> AnalyticsResource {
        let now = Utc::now();
        AnalyticsResource {
            id: id.to_string(),
            resource_type: kind.to_string(),
            name: format!("{id}.sql"),
            alias: None,
            config: Value::Null,
            scope: "project".to_string(),
            row_count: None,
            column_count: None,
            file_size,
            version: 1,
            parent_version_id: None,
            parent_resource_id: None,
            source_query: None,
            created_at: now,
            updated_at: now,
            created_by: None,
            deleted_at: None,
            kind: kind.to_string(),
            content_hash: Some("abc".to_string()),
            file_rel_path: Some(format!("{id}.sql")),
            readonly: 1,
            promoted_from: None,
            source_connection_id: None,
            source_table: None,
            definition_sql: None,
            archived_at: Some(now),
        }
    }

    fn trash_entry(
        id: &str,
        name: &str,
        origin: &str,
        rel_path: &str,
        kind: TrashKind,
        size: u64,
    ) -> TrashEntry {
        TrashEntry {
            manifest: engine::persistence::trash::TrashManifest {
                id: id.to_string(),
                name: name.to_string(),
                origin: origin.to_string(),
                original_rel_path: rel_path.to_string(),
                kind,
                deleted_at: Utc::now(),
                size,
            },
        }
    }

    /// 回收站快照只收本模块的条目；别人的只给“谁的、几条”（不混进行列表）。
    #[test]
    fn trash_snapshot_keeps_own_entries_and_counts_foreign() {
        let entries = vec![
            trash_entry(
                "t1",
                "dau.sql",
                ORIGIN_RESOURCES,
                "reports/dau.sql",
                TrashKind::File,
                2048,
            ),
            trash_entry(
                "t2",
                "draft.sql",
                "scratchpad",
                "draft.sql",
                TrashKind::File,
                10,
            ),
            trash_entry(
                "t3",
                "reports",
                "scratchpad",
                "reports",
                TrashKind::Folder,
                0,
            ),
        ];

        let snapshot = build_trash_snapshot(&entries);
        assert_eq!(snapshot.rows.len(), 1, "别人的条目不能混进行列表");
        assert_eq!(snapshot.rows[0].trash_id, "t1");
        assert_eq!(snapshot.rows[0].original_label, "resources/reports/dau.sql");
        assert_eq!(snapshot.rows[0].kind_label, "文件");
        assert_eq!(snapshot.rows[0].size_label, "2.0 KB");
        assert_eq!(snapshot.foreign, vec![("scratchpad".to_string(), 2)]);
    }

    /// 目录条目不给大小（要递归统计才有值，写 0 会被当成“空目录”）。
    #[test]
    fn trash_snapshot_leaves_folder_size_empty() {
        let entries = vec![trash_entry(
            "t1",
            "reports",
            ORIGIN_RESOURCES,
            "reports",
            TrashKind::Folder,
            0,
        )];
        let snapshot = build_trash_snapshot(&entries);
        assert_eq!(snapshot.rows[0].kind_label, "目录");
        assert!(snapshot.rows[0].size_label.is_empty());
        assert!(snapshot.foreign.is_empty());
    }

    #[test]
    fn repair_rows_group_in_order_and_keep_both_fingerprints() {
        let report = IndexScanReport {
            issues: vec![
                // 故意乱序给：输出应按分组 / 路径排好。
                IndexIssue::ContentChanged {
                    resource_id: "ar_2".to_string(),
                    name: "月报".to_string(),
                    rel_path: "reports/dau.sql".to_string(),
                    expected_hash: "1111111111111111".to_string(),
                    actual_hash: "2222222222222222".to_string(),
                },
                IndexIssue::UntrackedFile {
                    rel_path: "z/deep/外来表.sql".to_string(),
                },
                IndexIssue::MissingPayload {
                    resource_id: "ar_1".to_string(),
                    name: "周报".to_string(),
                    rel_path: "weekly.sql".to_string(),
                },
                IndexIssue::UntrackedFile {
                    rel_path: "a.sql".to_string(),
                },
            ],
        };

        let rows = build_repair_rows(&report);
        let groups: Vec<RepairGroup> = rows.iter().map(|row| row.group).collect();
        assert_eq!(
            groups,
            vec![
                RepairGroup::Untracked,
                RepairGroup::Untracked,
                RepairGroup::Missing,
                RepairGroup::Changed,
            ],
            "分组按原型顺序，组内按路径"
        );
        // 未登记行：标题取文件名（路径太长），完整路径在副文案里。
        assert_eq!(rows[0].title, "a.sql");
        assert!(rows[0].rel_path == "a.sql" && rows[0].resource_id.is_none());
        assert!(rows[0].detail.contains("resources/a.sql"));
        assert_eq!(rows[1].title, "外来表.sql", "嵌套路径取最后一段");
        // 指纹不匹配：两个指纹都缩到 12 位（期望 / 实际各一份，不藏任何一边）。
        assert_eq!(rows[3].hash_detail, "登记 111111111111 · 实际 222222222222");
        assert!(rows[3].detail.contains("reports/dau.sql"));
        assert_eq!(rows[3].resource_id.as_deref(), Some("ar_2"));
        assert!(rows[2].hash_detail.is_empty(), "缺本体没有指纹可对");
    }

    #[test]
    fn repair_rows_of_clean_report_are_empty() {
        assert!(build_repair_rows(&IndexScanReport::default()).is_empty());
    }

    #[test]
    fn size_formatting_stays_readable_at_every_scale() {
        assert_eq!(format_size(None), "");
        assert_eq!(format_size(Some(0)), "0 B");
        assert_eq!(format_size(Some(900)), "900 B");
        // 1 KB 以下不显示小数：`0.9 KB` 不如 `900 B` 直观。
        assert_eq!(format_size(Some(1023)), "1023 B");
        assert_eq!(format_size(Some(1229)), "1.2 KB");
        assert_eq!(format_size(Some(5 * 1024 * 1024)), "5.0 MB");
        assert_eq!(format_size(Some(-5)), "0 B", "负数按 0 处理，不显示负体积");
    }

    #[test]
    fn scale_uses_thousands_separator_and_tolerates_partial_info() {
        assert_eq!(format_scale(Some(12480), Some(18)), "12,480 行 × 18 列");
        assert_eq!(format_scale(Some(999), Some(3)), "999 行 × 3 列");
        assert_eq!(format_scale(Some(1000), None), "1,000 行");
        assert_eq!(format_scale(None, Some(7)), "7 列");
        assert_eq!(format_scale(None, None), "");
    }

    #[test]
    fn relative_time_buckets_and_clock_skew() {
        let now = Utc::now();
        assert_eq!(format_relative_time(now, now), "刚刚");
        assert_eq!(
            format_relative_time(now, now - Duration::minutes(5)),
            "5 分钟前"
        );
        assert_eq!(
            format_relative_time(now, now - Duration::hours(3)),
            "3 小时前"
        );
        assert_eq!(format_relative_time(now, now - Duration::days(2)), "2 天前");
        assert_eq!(
            format_relative_time(now, now - Duration::days(70)),
            "2 个月前"
        );
        assert_eq!(
            format_relative_time(now, now - Duration::days(800)),
            "2 年前"
        );
        // 时钟回拨不给"-5 分钟前"这种怪东西。
        assert_eq!(format_relative_time(now, now + Duration::hours(1)), "刚刚");
    }

    #[test]
    fn tail_follows_field_priority_per_kind() {
        let now = Utc::now();
        let file = row_model("ar_file", "file", Some(1229));
        assert_eq!(
            tail_for(&file, ArchiveKind::File, now),
            "1.2 KB · 刚刚",
            "文件型：大小 · 时间"
        );

        // 分析表型用规模替代大小。
        let mut analysis = row_model("ar_analysis", "analysis", None);
        analysis.row_count = Some(12480);
        analysis.column_count = Some(18);
        assert_eq!(
            tail_for(&analysis, ArchiveKind::Analysis, now),
            "12,480 行 × 18 列 · 刚刚"
        );

        // 引用型明说"无指纹"，不留空。
        let table_ref = row_model("ar_ref", "table_ref", None);
        assert_eq!(
            tail_for(&table_ref, ArchiveKind::TableRef, now),
            "无指纹 · 刚刚"
        );

        // 旧行（无体积）只剩时间，不留一个孤零零的分隔符。
        let legacy = row_model("ar_legacy", "file", None);
        assert_eq!(tail_for(&legacy, ArchiveKind::File, now), "刚刚");
    }

    #[test]
    fn details_carry_per_kind_size_and_real_history_count() {
        let now = Utc::now();
        let file = row_model("ar_file", "file", Some(1229));
        let analysis = {
            let mut row = row_model("ar_analysis", "analysis", None);
            row.row_count = Some(12480);
            row.column_count = Some(18);
            row
        };
        let table_ref = row_model("ar_ref", "table_ref", None);
        let resources = vec![file, analysis, table_ref];

        // 只有分析表有历史版本（版本数来自存储层的批量查询）。
        let mut counts = VersionCounts::new();
        counts.insert("ar_analysis".to_string(), 3);

        let snapshot = build_snapshot(SnapshotInputs {
            resources: &resources,
            statuses: &ArchiveStatuses::new(),
            history_counts: &counts,
            tags: empty_tags(),
            tag_dictionary: Vec::new(),
            folders: empty_folders(),
            group_dictionary: Vec::new(),
            read_only: false,
            now,
        });

        let file = snapshot.details.get("ar_file").expect("文件型详情");
        assert_eq!(file.size_label, "1.2 KB", "文件型给体积");
        assert_eq!(file.history_label, "", "无历史版本 → 空串（分区会被跳过）");
        assert!(file.readonly, "归档后只读");
        assert_eq!(file.content_hash.as_deref(), Some("abc"));
        // 详情用绝对时间（行上用相对时间）。
        assert_eq!(file.modified_label, format_timestamp(now));

        let analysis = snapshot.details.get("ar_analysis").expect("分析表详情");
        assert_eq!(analysis.size_label, "12,480 行 × 18 列");
        assert_eq!(analysis.history_label, "3 个历史版本");

        let table_ref = snapshot.details.get("ar_ref").expect("引用型详情");
        assert_eq!(table_ref.size_label, "", "引用型不给体积（不假装有值）");
    }

    /// 标签进快照：行带 **id + 名字**（筛选比 id、搜索比名字，一份数据两处用）、
    /// 详情带同一份 chip（同一装配来源）、顶层带词典（筛选菜单用）。
    #[test]
    fn snapshot_carries_tags_for_rows_details_and_dictionary() {
        let now = Utc::now();
        let resources = vec![
            row_model("ar_1", "file", Some(10)),
            row_model("ar_2", "file", Some(10)),
        ];
        let mut tags = ResourceTags::new();
        tags.insert(
            "ar_1".to_string(),
            vec![crate::models::AnalyticsTag {
                id: "at_1".to_string(),
                name: "报表".to_string(),
                color: None,
                icon: None,
                scope: "project".to_string(),
                created_at: now,
                deleted_at: None,
            }],
        );
        let dictionary = vec![TagOption {
            id: "at_1".to_string(),
            name: "报表".to_string(),
            count: 1,
        }];

        let snapshot = build_snapshot(SnapshotInputs {
            resources: &resources,
            statuses: &ArchiveStatuses::new(),
            history_counts: &VersionCounts::new(),
            tags: &tags,
            tag_dictionary: dictionary,
            folders: empty_folders(),
            group_dictionary: Vec::new(),
            read_only: false,
            now,
        });

        assert_eq!(snapshot.rows[0].tags, snapshot.details["ar_1"].tags);
        assert_eq!(snapshot.rows[0].tags[0].id, "at_1");
        assert_eq!(snapshot.rows[0].tags[0].name, "报表");
        assert!(
            snapshot.rows[1].tags.is_empty(),
            "没挂标签的行是空集，不是缺字段"
        );
        assert_eq!(snapshot.details["ar_1"].tags[0].name, "报表");
        assert!(snapshot.details["ar_2"].tags.is_empty());
        assert_eq!(snapshot.tags.len(), 1);
    }

    /// 搜索匹配面所需的字段（别名 / 来源表）在装配时就带上（原型 §2.2）。
    /// 行上不显示它们，但搜索要看：缺了这个装配，面板里搜别名就只会得到空列表。
    #[test]
    fn rows_carry_alias_and_source_table_for_the_search_surface() {
        let now = Utc::now();
        let mut alias = row_model("ar_1", "analysis", None);
        alias.alias = Some("月报".to_string());
        let mut table_ref = row_model("ar_2", "table_ref", None);
        table_ref.source_table = Some("dwd.dwd_orders".to_string());
        let resources = vec![alias, table_ref];

        let snapshot = build_snapshot(SnapshotInputs {
            resources: &resources,
            statuses: &ArchiveStatuses::new(),
            history_counts: &VersionCounts::new(),
            tags: empty_tags(),
            tag_dictionary: Vec::new(),
            folders: empty_folders(),
            group_dictionary: Vec::new(),
            read_only: false,
            now,
        });

        let haystack = crate::filter::search_haystack(&snapshot.rows[0]);
        assert!(haystack.contains("月报"), "别名进了匹配面：{haystack}");
        let haystack = crate::filter::search_haystack(&snapshot.rows[1]);
        assert!(
            haystack.contains("dwd.dwd_orders"),
            "来源表进了匹配面：{haystack}"
        );
    }

    #[test]
    fn timestamps_are_absolute_and_zero_padded() {
        let when = DateTime::parse_from_rfc3339("2026-09-16T04:05:00Z")
            .expect("rfc3339")
            .with_timezone(&Utc);
        assert_eq!(format_timestamp(when), "2026-09-16 04:05");
    }

    #[test]
    fn snapshot_counts_route_status_before_kind() {
        let now = Utc::now();
        let resources = vec![
            row_model("ar_1", "file", Some(1024)),
            row_model("ar_2", "analysis", None),
            row_model("ar_3", "table_ref", None),
            row_model("ar_4", "file", Some(10)),
            row_model("ar_5", "file", Some(10)),
        ];
        let mut statuses = ArchiveStatuses::new();
        // 异常态压过 kind：这两个不进各自 kind 的桶，而进异常桶。
        statuses.insert("ar_4".to_string(), ArchiveStatus::Missing);
        statuses.insert("ar_5".to_string(), ArchiveStatus::ContentChanged);

        let snapshot = build_snapshot(SnapshotInputs {
            resources: &resources,
            statuses: &statuses,
            history_counts: &VersionCounts::new(),
            tags: empty_tags(),
            tag_dictionary: Vec::new(),
            folders: empty_folders(),
            group_dictionary: Vec::new(),
            read_only: true,
            now,
        });

        assert_eq!(snapshot.rows.len(), 5);
        assert_eq!(snapshot.counts.total, 5);
        assert_eq!(snapshot.counts.archived, 1);
        assert_eq!(snapshot.counts.analysis, 1);
        assert_eq!(snapshot.counts.table_ref, 1);
        assert_eq!(snapshot.counts.missing, 1);
        assert_eq!(snapshot.counts.drifted, 1);
        assert!(snapshot.counts.has_issues());
        assert!(snapshot.read_only);
        // 行状态与计数同源（同一个 statuses 映射）。
        assert_eq!(snapshot.rows[3].status, ArchiveStatus::Missing);
    }

    /// 历史行：快照里带上当时的大小与指纹（生产路径就是资源记录的序列化）。
    fn history_version(version: i32, size: i32, hash: &str, at: DateTime<Utc>) -> ResourceVersion {
        let mut record = row_model("ar_1", "file", Some(size));
        record.version = version;
        record.content_hash = Some(hash.to_string());
        ResourceVersion {
            id: format!("arv_{version}"),
            resource_id: "ar_1".to_string(),
            version,
            snapshot: serde_json::to_value(&record).expect("snapshot json"),
            created_at: at,
        }
    }

    #[test]
    fn version_rows_include_current_and_compare_adjacent_versions() {
        let now = Utc::now();
        let mut current = row_model("ar_1", "file", Some(1024));
        current.version = 3;
        current.content_hash = Some("333333333333ffff".to_string());
        let history = vec![
            history_version(2, 2048, "222222222222ffff", now),
            history_version(1, 1024, "111111111111ffff", now),
        ];

        // 只有 v2 还留着副本：v1 的已被保留策略裁掉。
        let rows = build_version_rows(&current, &history, &[2], true);
        let versions: Vec<i32> = rows.iter().map(|row| row.version).collect();
        assert_eq!(versions, vec![3, 2, 1], "降序，且当前版本也在列表里");
        assert!(rows[0].is_current);
        assert!(!rows[1].is_current);
        assert!(rows[0].has_copy, "当前版本的“副本”就是本体");
        assert!(rows[1].has_copy);
        assert!(!rows[2].has_copy, "副本被裁的版本要露出来");
        assert_eq!(rows[0].hash_short, "333333333333", "指纹只展示前 12 位");
        assert_eq!(rows[0].delta_label, "较 v2 大小-1.0 KB · 指纹已变");
        assert_eq!(rows[1].delta_label, "较 v1 大小+1.0 KB · 指纹已变");
        assert!(rows[2].delta_label.is_empty(), "最旧一版没有可比的上一版");
        assert_eq!(rows[0].time_label, format_timestamp(now));
    }

    #[test]
    fn version_rows_tolerate_unreadable_snapshots() {
        let mut current = row_model("ar_1", "file", Some(1024));
        current.version = 2;
        let broken = ResourceVersion {
            id: "arv_1".to_string(),
            resource_id: "ar_1".to_string(),
            version: 1,
            snapshot: Value::Null,
            created_at: Utc::now(),
        };

        let rows = build_version_rows(&current, &[broken], &[], false);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].hash_short, "—", "解析不出来的快照不伪造指纹");
        assert_eq!(rows[1].size_label, "");
        assert!(
            rows[1].delta_label.is_empty(),
            "两侧都没值就不给差异（宁可不比也不编）"
        );
    }
}
