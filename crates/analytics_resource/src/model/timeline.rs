//! 版本时间线的领域类型：事件种类、事件行、呈现行。
//!
//! 设计边界：这里只描述「一条资产履历长什么样」，不碰数据库、不碰文件系统，也不碰 GPUI。
//! 时间线把两件原本分散的东西合起来看：**版本节点**（内容什么时候变过）与
//! **事件节点**（谁把它归档 / 取回 / 还原 / 撤销过）——只有前者时，用户看到的是一串指纹，
//! 答不出「这条结论经历过什么」。

use chrono::{DateTime, Utc};

/// 履历事件种类。
///
/// 与 `ChangeReason` 不是一回事：那是运行时广播（驱动本次重绘），这是落库的历史
/// （供几个月后追溯）。新增动作时两边都要考虑，但不必一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineEventKind {
    /// 首次归档。
    Archived,
    /// 再归档产生新版本（内容指纹变了）。
    Updated,
    /// 取回（检出）出工作副本。
    CheckedOut,
    /// 从历史版本还原（用旧内容生成新版本）。
    Restored,
    /// 撤销归档。
    Undone,
    /// 移入回收站。
    Trashed,
    /// 从回收站还原。
    Untrashed,
    /// 口径定义变更（数据没变、含义变了，同样要留痕）。
    SemanticChanged,
    /// 认证为正式资产。
    Certified,
    /// 标记废弃。
    Deprecated,
    /// 被更新版本取代。
    Superseded,
    /// 校验发现异常（源漂移 / 本体变化 / 时间窗过期）。
    Validated,
    /// 老库或未来版本写入的未知值：不让它毁掉整条时间线。
    Unknown,
}

impl TimelineEventKind {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Archived => "archived",
            Self::Updated => "updated",
            Self::CheckedOut => "checked_out",
            Self::Restored => "restored",
            Self::Undone => "undone",
            Self::Trashed => "trashed",
            Self::Untrashed => "untrashed",
            Self::SemanticChanged => "semantic_changed",
            Self::Certified => "certified",
            Self::Deprecated => "deprecated",
            Self::Superseded => "superseded",
            Self::Validated => "validated",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "archived" => Self::Archived,
            "updated" => Self::Updated,
            "checked_out" => Self::CheckedOut,
            "restored" => Self::Restored,
            "undone" => Self::Undone,
            "trashed" => Self::Trashed,
            "untrashed" => Self::Untrashed,
            "semantic_changed" => Self::SemanticChanged,
            "certified" => Self::Certified,
            "deprecated" => Self::Deprecated,
            "superseded" => Self::Superseded,
            "validated" => Self::Validated,
            _ => Self::Unknown,
        }
    }

    /// 界面文案（简体中文；不暴露库里的英文枚举值）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Archived => "归档",
            Self::Updated => "再归档 · 新版本",
            Self::CheckedOut => "取回（检出）",
            Self::Restored => "从历史还原",
            Self::Undone => "撤销归档",
            Self::Trashed => "移入回收站",
            Self::Untrashed => "从回收站还原",
            Self::SemanticChanged => "口径变更",
            Self::Certified => "认证为正式资产",
            Self::Deprecated => "标记废弃",
            Self::Superseded => "被新版本取代",
            Self::Validated => "校验发现异常",
            Self::Unknown => "未知事件",
        }
    }

    /// 是否需要用户在意（效力类 / 异常类事件在时间线上要更醒目）。
    pub fn is_state_change(self) -> bool {
        matches!(
            self,
            Self::SemanticChanged
                | Self::Certified
                | Self::Deprecated
                | Self::Superseded
                | Self::Validated
                | Self::Undone
        )
    }
}

/// 一条落库的履历事件（对应 `analytics_resource_events` 一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineEvent {
    pub id: String,
    pub resource_id: String,
    /// 事件发生时的版本号；`None` = 与版本无关（如撤销、回收站动作）。
    pub version: Option<i32>,
    pub kind: TimelineEventKind,
    pub detail: Option<String>,
    pub content_hash: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

/// 时间线上的一行（呈现层用，纯数据、零 I/O）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineRow {
    /// 稳定键（业务 id，不用下标——时间线会插入新事件）。
    pub key: String,
    pub time_label: String,
    pub title: String,
    pub detail: String,
    /// 效力 / 异常类事件：界面按它取强调色。
    pub is_state_change: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_db_roundtrip() {
        for kind in [
            TimelineEventKind::Archived,
            TimelineEventKind::Updated,
            TimelineEventKind::CheckedOut,
            TimelineEventKind::Restored,
            TimelineEventKind::Undone,
            TimelineEventKind::Trashed,
            TimelineEventKind::Untrashed,
            TimelineEventKind::SemanticChanged,
            TimelineEventKind::Certified,
            TimelineEventKind::Deprecated,
            TimelineEventKind::Superseded,
            TimelineEventKind::Validated,
        ] {
            assert_eq!(TimelineEventKind::from_db_str(kind.as_db_str()), kind);
        }
    }

    #[test]
    fn unknown_value_does_not_break_timeline() {
        assert_eq!(
            TimelineEventKind::from_db_str("future_kind"),
            TimelineEventKind::Unknown
        );
        assert_eq!(TimelineEventKind::Unknown.label(), "未知事件");
    }

    #[test]
    fn state_changes_are_flagged() {
        assert!(TimelineEventKind::SemanticChanged.is_state_change());
        assert!(TimelineEventKind::Undone.is_state_change());
        assert!(!TimelineEventKind::Archived.is_state_change());
    }
}
