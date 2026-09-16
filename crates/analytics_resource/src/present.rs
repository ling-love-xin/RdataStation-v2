//! 呈现层：索引行 → 面板快照（纯函数，零 I/O、零 GPUI 依赖）。
//!
//! 宿主桥的"重活"都在这儿：把 [`AnalyticsResource`] 转成 [`ArchiveRow`]（含字段优先级规则的
//! 尾巴文案）并统计 [`ArchiveCounts`]。剩下留给 workbench 的只有几行胶水（取数 → 调本模块 → 推面板），
//! 这样"怎么显示"能在没有窗口、没有数据库的环境里被单测锁住。
//!
//! 状态来源分两部分：**本体健康**（缺失 / 内容已变）由索引修复扫描给出，不在行模型里——
//! 故 [`build_snapshot`] 接收一份 `statuses` 映射（缺省即 `Normal`）。

use chrono::{DateTime, Utc};

use crate::detail_view::ArchiveDetail;
use crate::model::{ArchiveKind, ArchiveStatus};
use crate::resource_view::{ArchiveCounts, ArchiveRow, ResourcesSnapshot};
use crate::AnalyticsResource;

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

/// 单行转换（状态缺省 `Normal`）。
pub fn to_row(
    resource: &AnalyticsResource,
    statuses: &ArchiveStatuses,
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
        kind,
        version: resource.version,
        status,
        tail: tail_for(resource, kind, now),
    }
}

/// 单行 → 详情快照（详情面板只读，格式化在这里做完）。
///
/// 与 [`to_row`](crate::present::to_row) 同一纪律：取值全部来自行模型与宿主推来的映射，
/// 不在渲染期算。
pub fn to_detail(
    resource: &AnalyticsResource,
    status: ArchiveStatus,
    history_count: i64,
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
        archived_label: resource.archived_at.map(format_timestamp).unwrap_or_default(),
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
        // 标签与分组属 Phase 2（那时才有按行的标签数据与分组列），此处不编造。
        tags: Vec::new(),
        group: None,
    }
}

/// 组装面板快照：行（按状态与种类计分）+ 计数 + 只读标志。
///
/// 计数口径与状态行文案一一对应：`缺失` 与 `索引异常`（内容已变）各自计数，
/// 因为它们在界面上是两个不同的可点入口（都进索引修复，但处理方式不同）。
pub fn build_snapshot(
    resources: &[AnalyticsResource],
    statuses: &ArchiveStatuses,
    history_counts: &VersionCounts,
    read_only: bool,
    now: DateTime<Utc>,
) -> ResourcesSnapshot {
    let mut counts = ArchiveCounts {
        total: resources.len(),
        ..ArchiveCounts::default()
    };
    let mut rows = Vec::with_capacity(resources.len());
    let mut details = std::collections::HashMap::with_capacity(resources.len());

    for resource in resources {
        let row = to_row(resource, statuses, now);
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
        details.insert(
            resource.id.clone(),
            to_detail(resource, row.status, history_count),
        );
        rows.push(row);
    }

    ResourcesSnapshot {
        rows,
        counts,
        read_only,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArchiveStatuses, VersionCounts, build_snapshot, format_relative_time, format_scale,
        format_size, format_timestamp, tail_for,
    };
    use crate::model::{ArchiveKind, ArchiveStatus};
    use crate::models::AnalyticsResource;
    use chrono::{DateTime, Duration, Utc};
    use serde_json::Value;

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
        assert_eq!(format_relative_time(now, now - Duration::minutes(5)), "5 分钟前");
        assert_eq!(format_relative_time(now, now - Duration::hours(3)), "3 小时前");
        assert_eq!(format_relative_time(now, now - Duration::days(2)), "2 天前");
        assert_eq!(format_relative_time(now, now - Duration::days(70)), "2 个月前");
        assert_eq!(format_relative_time(now, now - Duration::days(800)), "2 年前");
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

        let snapshot = build_snapshot(&resources, &ArchiveStatuses::new(), &counts, false, now);

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

        let snapshot = build_snapshot(&resources, &statuses, &VersionCounts::new(), true, now);

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
}
