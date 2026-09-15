//! 工具栏的数据层：筛选与排序（纯函数，零 GPUI 依赖）。
//!
//! 独立成模块的理由：这些规则要被**面板**（行列表）与**后续的筛选/排序菜单**共同使用，
//! 而且必须能在没有窗口的环境下单测——放在 `resource_view.rs` 里会让"规则"和"渲染"互相拖累。
//!
//! 面板的空态判定依赖 [`ResourcesFilter::is_empty`]：
//! **空筛选 + 无行 = "还没有任何存档"**（引导归档）；**非空筛选 + 无行 = "没有匹配的存档"**（引导清筛选）。

use crate::model::{ArchiveKind, ArchiveStatus};
use crate::resource_view::ArchiveRow;

/// 排序字段。
///
/// 目前只支持**行上真实存在的键**（名称 / 版本号）。归档时间与大小需要 `ArchiveRow` 另带原始值
/// （`updated_epoch` / `size_bytes`）——拿格式化后的尾巴字符串比较，会在 `1.2 KB` 与 `900 B`
/// 之间得出错误顺序；宁可先不支持，也不做一个会静默排错的字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    #[default]
    Name,
    Version,
}

impl SortField {
    /// 菜单文案（与 `ui.rs` 的尺寸常量为同一类"就近常量"）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "名称",
            Self::Version => "版本",
        }
    }
}

/// 排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

impl SortOrder {
    /// 同一字段再点一次即翻转（沿用 M4 与 v1 `use-pagination` 的语义）。
    pub fn flipped(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

/// 工具栏筛选条件（全空 = 不限）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourcesFilter {
    /// 关键字：匹配显示名与尾部字段，**大小写不敏感**。
    pub query: String,
    /// 种类多选（空 = 不限）。
    pub kinds: Vec<ArchiveKind>,
    /// 只看需要处理的异常（缺失 / 内容已变），对应状态行的"修复…"场景。
    pub only_issues: bool,
}

impl ResourcesFilter {
    /// 是否为空条件（决定面板显示哪一种空态，见模块头注释）。
    pub fn is_empty(&self) -> bool {
        self.query.trim().is_empty() && self.kinds.is_empty() && !self.only_issues
    }

    /// 单行是否命中。
    pub fn matches(&self, row: &ArchiveRow) -> bool {
        let query = self.query.trim().to_lowercase();
        if !query.is_empty() {
            let haystack = format!("{}{}", row.name.to_lowercase(), row.tail.to_lowercase());
            if !haystack.contains(&query) {
                return false;
            }
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&row.kind) {
            return false;
        }
        if self.only_issues && row.status == ArchiveStatus::Normal {
            return false;
        }
        true
    }
}

/// 一行是否需要用户处理（与状态行的"异常"口径一致）。
pub fn needs_attention(row: &ArchiveRow) -> bool {
    row.status != ArchiveStatus::Normal
}

/// 筛选 → 排序（返回新集合，不改入参）。
///
/// 排序**带名称兜底且兜底键不随方向翻转**：否则同一版本号的多行顺序不可预测，
/// 会出现"同一份数据两次渲染顺序不同"的观感问题。
pub fn apply_view(
    rows: &[ArchiveRow],
    filter: &ResourcesFilter,
    field: SortField,
    order: SortOrder,
) -> Vec<ArchiveRow> {
    let mut result: Vec<ArchiveRow> = rows
        .iter()
        .filter(|row| filter.matches(row))
        .cloned()
        .collect();

    result.sort_by(|left, right| {
        let primary = match field {
            SortField::Name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
            SortField::Version => left.version.cmp(&right.version),
        };
        let primary = match order {
            SortOrder::Asc => primary,
            SortOrder::Desc => primary.reverse(),
        };
        if primary.is_eq() {
            return left.name.to_lowercase().cmp(&right.name.to_lowercase());
        }
        primary
    });

    result
}

#[cfg(test)]
mod tests {
    use super::{
        ResourcesFilter, SortField, SortOrder, apply_view, needs_attention,
    };
    use crate::model::{ArchiveKind, ArchiveStatus};
    use crate::resource_view::ArchiveRow;

    fn row(id: &str, name: &str, kind: ArchiveKind, status: ArchiveStatus, version: i32, tail: &str) -> ArchiveRow {
        ArchiveRow {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            version,
            status,
            tail: tail.to_string(),
        }
    }

    fn sample() -> Vec<ArchiveRow> {
        vec![
            row("ar_2", "Beta.sql", ArchiveKind::File, ArchiveStatus::Normal, 2, "1.2 KB · 3 天前"),
            row("ar_1", "alpha.sql", ArchiveKind::Analysis, ArchiveStatus::Normal, 5, "12,480 行 × 18 列"),
            row("ar_3", "gamma.sql", ArchiveKind::File, ArchiveStatus::Missing, 1, ""),
        ]
    }

    #[test]
    fn query_matches_name_and_tail_case_insensitively() {
        let rows = sample();

        let by_name = ResourcesFilter {
            query: "ALPHA".to_string(),
            ..ResourcesFilter::default()
        };
        let hit = apply_view(&rows, &by_name, SortField::Name, SortOrder::Asc);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, "ar_1");

        // 用户也会拿尾部线索找（"1.2"、"48" 之类）。
        let by_tail = ResourcesFilter {
            query: "1.2".to_string(),
            ..ResourcesFilter::default()
        };
        assert_eq!(apply_view(&rows, &by_tail, SortField::Name, SortOrder::Asc).len(), 1);
    }

    #[test]
    fn kind_and_issue_filters_narrow_the_list() {
        let rows = sample();

        let only_files = ResourcesFilter {
            kinds: vec![ArchiveKind::File],
            ..ResourcesFilter::default()
        };
        assert_eq!(apply_view(&rows, &only_files, SortField::Name, SortOrder::Asc).len(), 2);
        assert!(!only_files.is_empty());

        let issues = ResourcesFilter {
            only_issues: true,
            ..ResourcesFilter::default()
        };
        let hit = apply_view(&rows, &issues, SortField::Name, SortOrder::Asc);
        assert_eq!(hit.len(), 1);
        assert!(needs_attention(&hit[0]));
        assert_eq!(hit[0].id, "ar_3");

        // 全空条件是"空"，面板据此区分"无匹配"与"还没有任何存档"。
        assert!(ResourcesFilter::default().is_empty());
    }

    #[test]
    fn sort_is_stable_and_direction_aware() {
        let rows = sample();
        let none = ResourcesFilter::default();

        let asc = apply_view(&rows, &none, SortField::Name, SortOrder::Asc);
        assert_eq!(
            asc.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["ar_1", "ar_2", "ar_3"],
            "名称升序且大小写不敏感"
        );

        let desc = apply_view(&rows, &none, SortField::Name, SortOrder::Desc);
        assert_eq!(
            desc.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["ar_3", "ar_2", "ar_1"]
        );

        let by_version = apply_view(&rows, &none, SortField::Version, SortOrder::Desc);
        assert_eq!(
            by_version.iter().map(|r| r.version).collect::<Vec<_>>(),
            vec![5, 2, 1]
        );
    }

    #[test]
    fn order_flips_on_repeat_click() {
        assert_eq!(SortOrder::Asc.flipped(), SortOrder::Desc);
        assert_eq!(SortOrder::Desc.flipped(), SortOrder::Asc);
        assert_eq!(SortField::Name.label(), "名称");
        assert_eq!(SortField::Version.label(), "版本");
    }
}
