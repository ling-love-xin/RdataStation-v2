//! 时间线行：把「版本节点」与「事件节点」合成一条可追溯的履历（纯函数，零 I/O）。
//!
//! 只有版本行时，用户看到的是一串指纹，答不出「这条结论经历过什么」；只有事件流时，
//! 又看不出哪一版的内容真的变了。两条流按时间合并，才是资产履历。

use super::*;
use crate::model::{TimelineEvent, TimelineRow};
use crate::store::rows::ResourceVersion;

struct Node {
    at: DateTime<Utc>,
    row: TimelineRow,
}

/// 合成时间线（时间倒序）。
///
/// `current` 也要占一个版本节点：写前快照语义下当前版本在资源行上、不进版本表，
/// 只列历史会让人以为最新版不见了（与 [`build_version_rows`](super::build_version_rows) 同一口径）。
pub fn build_timeline_rows(
    current: &AnalyticsResource,
    versions: &[ResourceVersion],
    events: &[TimelineEvent],
) -> Vec<TimelineRow> {
    let mut nodes: Vec<Node> = Vec::with_capacity(versions.len() + events.len() + 1);

    nodes.push(Node {
        at: current.archived_at.unwrap_or(current.updated_at),
        row: TimelineRow {
            key: format!("v-{}", current.version),
            time_label: format_timestamp(current.archived_at.unwrap_or(current.updated_at)),
            title: format!("v{} · 当前版本", current.version),
            detail: hash_detail(current.content_hash.as_deref()),
            is_state_change: false,
        },
    });

    for version in versions {
        nodes.push(Node {
            at: version.created_at,
            row: TimelineRow {
                key: format!("v-{}", version.version),
                time_label: format_timestamp(version.created_at),
                title: format!("v{} · 内容快照", version.version),
                detail: String::new(),
                is_state_change: false,
            },
        });
    }

    for event in events {
        nodes.push(Node {
            at: event.occurred_at,
            row: TimelineRow {
                key: format!("e-{}", event.id),
                time_label: format_timestamp(event.occurred_at),
                title: event.kind.label().to_string(),
                detail: event_detail(event),
                is_state_change: event.kind.is_state_change(),
            },
        });
    }

    // 同一时刻（秒级时间戳）事件排在版本节点之后：先说"发生了什么"，再说"内容长什么样"。
    nodes.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.row.key.cmp(&b.row.key)));
    nodes.into_iter().map(|node| node.row).collect()
}

fn hash_detail(hash: Option<&str>) -> String {
    match hash {
        Some(value) if !value.is_empty() => format!("内容指纹 {}", crate::detail_view::short_hash(Some(value))),
        _ => "无内容指纹（旧行）".to_string(),
    }
}

fn event_detail(event: &TimelineEvent) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(version) = event.version {
        parts.push(format!("v{version}"));
    }
    if let Some(hash) = event.content_hash.as_deref().filter(|value| !value.is_empty()) {
        parts.push(crate::detail_view::short_hash(Some(hash)));
    }
    if let Some(detail) = event.detail.as_deref().filter(|value| !value.is_empty()) {
        parts.push(detail.to_string());
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TimelineEventKind;

    fn resource(version: i32) -> AnalyticsResource {
        AnalyticsResource {
            id: "ar_1".to_string(),
            resource_type: "sql".to_string(),
            name: "月报".to_string(),
            alias: None,
            config: serde_json::Value::Null,
            scope: "project".to_string(),
            row_count: None,
            column_count: None,
            file_size: None,
            version,
            parent_version_id: None,
            parent_resource_id: None,
            source_query: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: None,
            deleted_at: None,
            kind: "file".to_string(),
            content_hash: Some("aabbccdd".to_string()),
            file_rel_path: Some("a.sql".to_string()),
            payload_mode: "copy".to_string(),
            source_path: None,
            readonly: 1,
            promoted_from: None,
            source_connection_id: None,
            source_table: None,
            definition_sql: None,
            archived_at: Some(Utc::now()),
        }
    }

    #[test]
    fn current_version_always_occupies_a_node() {
        let rows = build_timeline_rows(&resource(3), &[], &[]);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].title.contains("v3"));
        assert!(rows[0].title.contains("当前版本"));
    }

    #[test]
    fn events_are_merged_and_newest_first() {
        let older = TimelineEvent {
            id: "are_1".to_string(),
            resource_id: "ar_1".to_string(),
            version: Some(1),
            kind: TimelineEventKind::Archived,
            detail: Some("来自草稿箱".to_string()),
            content_hash: Some("1111".to_string()),
            occurred_at: Utc::now() - chrono::Duration::days(2),
        };
        let newer = TimelineEvent {
            id: "are_2".to_string(),
            resource_id: "ar_1".to_string(),
            version: Some(2),
            kind: TimelineEventKind::Updated,
            detail: None,
            content_hash: Some("2222".to_string()),
            occurred_at: Utc::now() - chrono::Duration::days(1),
        };
        let rows = build_timeline_rows(&resource(2), &[], &[older, newer]);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].title, "再归档 · 新版本");
        assert_eq!(rows[2].title, "归档");
        assert!(rows[2].detail.contains("v1"));
    }

    #[test]
    fn state_change_events_are_flagged() {
        let event = TimelineEvent {
            id: "are_3".to_string(),
            resource_id: "ar_1".to_string(),
            version: Some(2),
            kind: TimelineEventKind::SemanticChanged,
            detail: Some("日活口径改为自然日".to_string()),
            content_hash: None,
            occurred_at: Utc::now() - chrono::Duration::hours(1),
        };
        let rows = build_timeline_rows(&resource(2), &[], &[event]);
        let flagged = rows.iter().find(|row| row.key == "e-are_3").unwrap();
        assert!(flagged.is_state_change);
        assert!(flagged.detail.contains("日活口径"));
    }
}
