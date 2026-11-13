//! 呈现层：索引行 → 面板快照（纯函数，零 I/O、零 GPUI 依赖）。
//!
//! 宿主桥的"重活"都在这儿：把 [`AnalyticsResource`] 转成 [`ArchiveRow`]（含字段优先级规则的
//! 尾巴文案）并统计 [`ArchiveCounts`]。剩下留给 workbench 的只有几行胶水（取数 → 调本模块 → 推面板），
//! 这样"怎么显示"能在没有窗口、没有数据库的环境里被单测锁住。
//!
//! 状态来源分两部分：**本体健康**（缺失 / 内容已变）由索引修复扫描给出，不在行模型里——
//! 故 [`build_snapshot`] 接收一份 `statuses` 映射（缺省即 `Normal`）。

pub(crate) use chrono::{DateTime, Utc};

pub(crate) use engine::persistence::trash::{TrashEntry, TrashKind};

pub(crate) use crate::AnalyticsResource;
pub(crate) use crate::detail_view::{ArchiveDetail, ArchiveTagChip, DetailGroupOption, DetailVersionLine};
pub(crate) use crate::dialogs::index_repair::{RepairGroup, RepairRow};
pub(crate) use crate::dialogs::trash::TrashRow;
pub(crate) use crate::dialogs::version::VersionRow;
pub(crate) use crate::model::{
    ArchiveKind, ArchiveStatus, ORIGIN_RESOURCES, PayloadMode, RowScope, TRASH_ID_REF_PREFIX,
};
pub(crate) use crate::payload::RESOURCES_DIR_NAME;
pub(crate) use crate::preview::Preview;
pub(crate) use crate::resource_view::{ArchiveCounts, ArchiveRow, GroupOption, ResourcesSnapshot, TagOption};

/// 按资源的标签映射：`resource_id → 标签行`（来自 `AnalyticsResourceStore::tags_by_resource`，
/// 一次查完；逐行查会把一次刷新变成 N+1 次查询）。
pub type ResourceTags = std::collections::HashMap<String, Vec<crate::store::rows::AnalyticsTag>>;

/// 单行状态映射：`resource_id → 状态`（来自 `IndexRepair::scan` 的结果）。
pub type ArchiveStatuses = std::collections::HashMap<String, ArchiveStatus>;

/// 内容预览映射：`resource_id → 预览`（宿主在工作线程上按 `preview` 模块的常量读完）。
///
/// 为什么不在 `build_snapshot` 里现读：**装配层是纯函数**（零 I/O），文件读在宿主侧发生。
pub type PreviewMap = std::collections::HashMap<String, Preview>;

/// 历史版本数映射：`resource_id → 条数`（来自 `AnalyticsResourceStore::version_counts`）。
pub type VersionCounts = std::collections::HashMap<String, i64>;

/// 按资源的最近版本映射：`resource_id → 最近 N 条`（来自 `recent_versions_by_resource`）。
pub type RecentVersions = std::collections::HashMap<String, Vec<crate::store::rows::ResourceVersion>>;

/// 详情「版本」区最多列几条明细（原型 §3.1：最近 3 条 + 「查看全部…」）。
///
/// 取数与渲染共用这一处：改这里的数，SQL 的 top-N 与面板列几行一起变。
pub const DETAIL_RECENT_VERSIONS: usize = 3;

/// 大小文案：按 1024 进制分档，`< 1 KB` 用字节（避免出现 `0.0 KB` 这种没信息量的值）。
mod format;
mod trash;
mod rows;
mod detail;
mod versions;
mod timeline;
mod repair;
mod snapshot;

pub use self::format::{format_size, format_scale, format_relative_time, format_timestamp, tail_for};
pub use self::trash::{build_trash_snapshot, entity_trash_row, reference_trash_rows, FolderMemberships};
pub use self::rows::{SnapshotInputs, to_row, tag_options};
pub use self::detail::{DetailExtras, to_detail};
pub use self::versions::{build_version_rows};
pub use self::timeline::{build_timeline_rows};
pub use self::repair::{build_repair_rows};
pub use self::snapshot::{build_snapshot};

/// 业务断言 → 可搜文本（小写）：只进搜索匹配面，**不显示**在行上。
///
/// 与 `search_haystack` 的字段拼接口径一致：字段之间留空格，避免跨字段偶然连缀出假命中。
pub fn assertion_search_text(assertion: &crate::model::ArchiveAssertion) -> String {
    [
        assertion.question.as_deref(),
        assertion.metric_key.as_deref(),
        assertion.metric_name.as_deref(),
        assertion.definition.as_deref(),
        assertion.assumptions.as_deref(),
        assertion.time_window.as_deref(),
        assertion.conclusion.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase()
}

#[cfg(test)]
mod search_text_tests {
    use super::assertion_search_text;
    use crate::model::ArchiveAssertion;

    #[test]
    fn assertion_text_is_lowercase_and_covers_definition() {
        let assertion = ArchiveAssertion {
            metric_name: Some("DAU".to_string()),
            definition: Some("自然日内去重登录用户".to_string()),
            time_window: Some("2026Q3".to_string()),
            ..Default::default()
        };
        let text = assertion_search_text(&assertion);
        assert!(text.contains("dau"));
        assert!(text.contains("自然日内去重登录用户"));
        assert!(text.contains("2026q3"));
    }

    #[test]
    fn empty_assertion_yields_empty_text() {
        assert_eq!(assertion_search_text(&ArchiveAssertion::default()), "");
    }
}

#[cfg(test)]
mod tests;
