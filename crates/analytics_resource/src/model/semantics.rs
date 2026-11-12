//! 业务断言与效力：把「这个数是什么意思」变成可留痕的一等公民。
//!
//! 归档凭证三件套（来源 / 代码 / 内容指纹）回答「是不是当初那份」；断言回答
//! 「当初那份是什么意思」。两者正交：数据一个字节没变、口径定义被澄清或修正，
//! 结论的含义照样变了——所以 `semantic_hash` 与 `content_hash` 必须并列。

use sha2::{Digest, Sha256};

/// 一次结论的业务上下文快照（选填，一条资源最多一份）。
///
/// 刻意不叫 `MetricDefinition`：M6 不是指标字典，这里存的是**这一次结论**的上下文，
/// 不是全局唯一口径定义。`metric_key` 只作为将来对接外部字典的稳定标识。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveAssertion {
    /// 这次要回答的业务问题。
    pub question: Option<String>,
    /// 指标稳定标识（可空；M6 只引用，不自建字典）。
    pub metric_key: Option<String>,
    /// 指标显示名。
    pub metric_name: Option<String>,
    /// 口径定义（自然语言 + 可选表达式）。
    pub definition: Option<String>,
    /// 假设与过滤条件（结构化 JSON 文本，原样存取、不解释）。
    pub assumptions: Option<String>,
    /// **业务**时间范围（与 `archived_at` 的生成时间分开）。
    pub time_window: Option<String>,
    /// 一句话结论摘要（转发时应该一起走）。
    pub conclusion: Option<String>,
    pub owner: Option<String>,
    pub confirmed_by: Option<String>,
    pub confirmed_at: Option<String>,
}

impl ArchiveAssertion {
    /// 是否一个字都没填（空断言不落库、不产生口径变更事件）。
    pub fn is_empty(&self) -> bool {
        [
            self.question.as_deref(),
            self.metric_key.as_deref(),
            self.metric_name.as_deref(),
            self.definition.as_deref(),
            self.assumptions.as_deref(),
            self.time_window.as_deref(),
            self.conclusion.as_deref(),
        ]
        .iter()
        .all(|value| value.map(str::trim).unwrap_or("").is_empty())
    }

    /// 口径指纹：**只有会改变结论含义的字段**参与。
    ///
    /// `question` / `conclusion` / `owner` 是描述与责任信息，改它们不改变"这个数怎么算"，
    /// 所以不进哈希——否则改个错别字也会在时间线上多一条"口径变更"。
    /// 各段之间用 NUL 分隔，避免拼接歧义（`ab` + `c` 与 `a` + `bc` 不能撞）。
    pub fn semantic_hash(&self) -> String {
        let mut hasher = Sha256::new();
        for part in [
            self.metric_key.as_deref(),
            self.definition.as_deref(),
            self.assumptions.as_deref(),
            self.time_window.as_deref(),
        ] {
            hasher.update(part.unwrap_or("").trim().as_bytes());
            hasher.update([0_u8]);
        }
        hex::encode(hasher.finalize())
    }

    /// 时间线事件里的那句人话（口径摘要，不塞整段 JSON）。
    pub fn summary(&self) -> String {
        let metric = self
            .metric_name
            .as_deref()
            .or(self.metric_key.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let definition = self
            .definition
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        match (metric, definition) {
            (Some(metric), Some(definition)) => format!("{metric}：{definition}"),
            (Some(metric), None) => metric.to_string(),
            (None, Some(definition)) => definition.to_string(),
            (None, None) => "业务断言已更新".to_string(),
        }
    }
}

/// 资产的业务效力（与 `ArchiveStatus` 的**物理完整性**正交）。
///
/// 不要合并进 `ArchiveStatus`：文件被外部改过（ContentChanged）不等于结论已废弃，
/// 混成一个枚举会推出错误结论。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Lifecycle {
    /// 候选：随手归档、未经确认（默认）。
    #[default]
    Draft,
    /// 认证：可作为正式依据引用。
    Certified,
    /// 废弃：不再适用，但保留追溯。
    Deprecated,
    /// 被新版本 / 新资产取代。
    Superseded,
}

impl Lifecycle {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Certified => "certified",
            Self::Deprecated => "deprecated",
            Self::Superseded => "superseded",
        }
    }

    pub fn from_db_str(value: &str) -> Self {
        match value {
            "certified" => Self::Certified,
            "deprecated" => Self::Deprecated,
            "superseded" => Self::Superseded,
            _ => Self::Draft,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "候选",
            Self::Certified => "正式资产",
            Self::Deprecated => "已废弃",
            Self::Superseded => "已被取代",
        }
    }

    /// 对应的时间线事件（Draft 是默认态，不产生事件）。
    pub fn event_kind(self) -> Option<crate::model::TimelineEventKind> {
        use crate::model::TimelineEventKind;
        match self {
            Self::Draft => None,
            Self::Certified => Some(TimelineEventKind::Certified),
            Self::Deprecated => Some(TimelineEventKind::Deprecated),
            Self::Superseded => Some(TimelineEventKind::Superseded),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assertion() -> ArchiveAssertion {
        ArchiveAssertion {
            metric_key: Some("dau".to_string()),
            definition: Some("自然日内去重登录用户".to_string()),
            assumptions: Some(r#"{"exclude":"internal"}"#.to_string()),
            time_window: Some("2026Q3".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn semantic_hash_is_stable_and_sensitive_to_definition() {
        let original = assertion();
        let same = assertion();
        assert_eq!(original.semantic_hash(), same.semantic_hash());

        let mut changed = assertion();
        changed.definition = Some("滚动 24 小时内去重登录用户".to_string());
        assert_ne!(original.semantic_hash(), changed.semantic_hash());
    }

    #[test]
    fn descriptive_fields_do_not_change_semantic_hash() {
        let original = assertion();
        let mut reworded = assertion();
        reworded.question = Some("换个问法".to_string());
        reworded.conclusion = Some("换个结论摘要".to_string());
        reworded.owner = Some("张三".to_string());
        assert_eq!(original.semantic_hash(), reworded.semantic_hash());
    }

    #[test]
    fn concatenation_is_unambiguous() {
        let mut a = ArchiveAssertion::default();
        a.metric_key = Some("ab".to_string());
        a.definition = Some("c".to_string());
        let mut b = ArchiveAssertion::default();
        b.metric_key = Some("a".to_string());
        b.definition = Some("bc".to_string());
        assert_ne!(a.semantic_hash(), b.semantic_hash());
    }

    #[test]
    fn empty_assertion_is_detected() {
        assert!(ArchiveAssertion::default().is_empty());
        assert!(!assertion().is_empty());
    }

    #[test]
    fn lifecycle_roundtrip_and_event() {
        for state in [
            Lifecycle::Draft,
            Lifecycle::Certified,
            Lifecycle::Deprecated,
            Lifecycle::Superseded,
        ] {
            assert_eq!(Lifecycle::from_db_str(state.as_db_str()), state);
        }
        assert_eq!(Lifecycle::from_db_str("future"), Lifecycle::Draft);
        assert!(Lifecycle::Draft.event_kind().is_none());
        assert!(Lifecycle::Certified.event_kind().is_some());
    }
}
