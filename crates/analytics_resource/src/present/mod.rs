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
use crate::model::{ArchiveKind, ArchiveStatus, ORIGIN_RESOURCES, PayloadMode, RowScope, TRASH_ID_REF_PREFIX};
use crate::payload::RESOURCES_DIR_NAME;
use crate::preview::Preview;
use crate::resource_view::{ArchiveCounts, ArchiveRow, GroupOption, ResourcesSnapshot, TagOption};

/// 按资源的标签映射：`resource_id → 标签行`（来自 `AnalyticsResourceStore::tags_by_resource`，
/// 一次查完；逐行查会把一次刷新变成 N+1 次查询）。
pub type ResourceTags = std::collections::HashMap<String, Vec<crate::models::AnalyticsTag>>;

/// 单行状态映射：`resource_id → 状态`（来自 `IndexRepair::scan` 的结果）。
pub type ArchiveStatuses = std::collections::HashMap<String, ArchiveStatus>;

/// 内容预览映射：`resource_id → 预览`（宿主在工作线程上按 `preview` 模块的常量读完）。
///
/// 为什么不在 `build_snapshot` 里现读：**装配层是纯函数**（零 I/O），文件读在宿主侧发生。
pub type PreviewMap = std::collections::HashMap<String, Preview>;

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
        if entity_trash_row(entry).is_none() {
            let origin = entry.manifest.origin.clone();
            match foreign.iter_mut().find(|(o, _)| o == &origin) {
                Some((_, count)) => *count += 1,
                None => foreign.push((origin, 1)),
            }
            continue;
        }
        rows.push(entity_trash_row(entry).expect("上面已判 Some").1);
    }
    TrashSnapshot { rows, foreign }
}

/// 实体回收站条目 → 对话框行（copy 口径：本体在 `ProjectTrash`）。
///
/// 返回删除时间供双来源归并排序；非本模块条目返回 `None`（留给 `build_trash_snapshot`
/// 统计 foreign 条数）。
pub fn entity_trash_row(entry: &TrashEntry) -> Option<(DateTime<Utc>, TrashRow)> {
    let manifest = &entry.manifest;
    if manifest.origin != ORIGIN_RESOURCES {
        return None;
    }
    // “原位置”回答的是“还原到哪”：rel_path 为空（极端旧条目）时退回到文件名。
    let original_label = if manifest.original_rel_path.trim().is_empty() {
        manifest.name.clone()
    } else {
        format!("{RESOURCES_DIR_NAME}/{}", manifest.original_rel_path)
    };
    let row = TrashRow {
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
        payload_mode: PayloadMode::Copy,
        resource_id: String::new(),
        has_payload: true,
    };
    Some((manifest.deleted_at, row))
}

/// reference 软删登记行 → 回收站行（第二来源；§12.1.1 ③）。
///
/// 这类条目没有实体 manifest（本体一直在源位置），回收站清单要把它们一并列出；
/// copy 软删行已由实体条目覆盖，这里只收 reference，避免双来源重复。返回删除时间供归并。
pub fn reference_trash_rows(deleted: &[AnalyticsResource]) -> Vec<(DateTime<Utc>, TrashRow)> {
    deleted
        .iter()
        .filter(|r| PayloadMode::from_db_str(&r.payload_mode) == PayloadMode::Reference)
        .map(|r| {
            let kind = ArchiveKind::from_db_str(&r.kind);
            let original_label = r
                .source_path
                .clone()
                .or_else(|| r.file_rel_path.clone())
                .unwrap_or_else(|| r.name.clone());
            let when = r.deleted_at.unwrap_or(r.updated_at);
            let row = TrashRow {
                trash_id: format!("{TRASH_ID_REF_PREFIX}{}", r.id),
                name: r.name.clone(),
                original_label,
                kind_label: crate::detail_view::kind_label(kind).to_string(),
                time_label: format_timestamp(when),
                size_label: format_size(r.file_size),
                payload_mode: PayloadMode::Reference,
                resource_id: r.id.clone(),
                has_payload: false,
            };
            (when, row)
        })
        .collect()
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
        global_rows,
        global_resources,
        statuses,
        history_counts,
        tags: tags_by_resource,
        tag_dictionary,
        folders: folders_by_resource,
        group_dictionary,
        previews,
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
        // 预览同理：宿主没读到就给缺省（「仅元信息」），不在渲染期补读数。
        let preview = previews.get(&resource.id).cloned().unwrap_or_default();
        // 分组名从“按行的归属 + 分组字典”拼：认不出的 id 当未分组（与 `filter` 的脏数据口径一致）。
        let group = folders_by_resource
            .get(&resource.id)
            .and_then(|folder_id| group_dictionary.iter().find(|group| &group.id == folder_id))
            .map(|group| group.name.clone());
        details.insert(
            resource.id.clone(),
            to_detail(
                resource,
                row.status,
                history_count,
                DetailExtras {
                    tags: row.tags.clone(),
                    group,
                    preview,
                },
            ),
        );
        rows.push(row);
    }

    // 全局行详情（§13 收尾 A4）：轻量基本信息区——全局共享层无项目侧标签 / 分组 / 预览，
    // 历史版本计数也不进项目版本表；动作区由渲染层按 `scope` 收敛到「分发到本项目」。
    for resource in &global_resources {
        let preview = previews.get(&resource.id).cloned().unwrap_or_default();
        details.insert(
            resource.id.clone(),
            to_detail(
                resource,
                ArchiveStatus::Normal,
                0,
                DetailExtras {
                    tags: Vec::new(),
                    group: None,
                    preview,
                },
            ),
        );
    }

    ResourcesSnapshot {
        rows,
        global_rows,
        counts,
        read_only,
        details,
        tags: tag_dictionary,
        groups: group_dictionary,
    }
}

#[cfg(test)]
mod tests;
