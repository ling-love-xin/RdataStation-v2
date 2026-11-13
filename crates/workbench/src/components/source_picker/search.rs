//! 跨根搜索：匹配档位与排序（`06-object-tree.html` §03）。
//!
//! 规范原文：
//! - 跨三个根，不限于当前 tab；
//! - 匹配档位决定排序：前缀 100 > 子串 70 > 子序列 40（`ods` 能命中 `orders`）；
//! - 同档内按使用频次（最近用过置顶），再按名称字典序；
//! - 最小单位是表 / 视图：不搜列名、不搜分组名、不搜索引（已裁决）；
//! - 上限 50 条，超出显示 refine 提示，不做分页；
//! - 大小写不敏感；不做拼音首字母匹配。

use super::node::{NodeId, Row};

/// 结果上限（§03：「上限 50 条，超出显示 refine 提示」）
pub const SEARCH_LIMIT: usize = 50;

/// 三档匹配质量。分值就是 §03 的 100 / 70 / 40，直接用于排序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitRank {
    Prefix,
    Substring,
    Subsequence,
}

impl HitRank {
    pub fn score(self) -> u32 {
        match self {
            Self::Prefix => 100,
            Self::Substring => 70,
            Self::Subsequence => 40,
        }
    }
}

/// 单条候选的匹配档位；不匹配返回 `None`。
pub fn rank(query: &str, label: &str) -> Option<HitRank> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let name = label.to_lowercase();
    if name.starts_with(&q) {
        Some(HitRank::Prefix)
    } else if name.contains(&q) {
        Some(HitRank::Substring)
    } else if is_subsequence(&q, &name) {
        Some(HitRank::Subsequence)
    } else {
        None
    }
}

/// 子序列：查询的字符按顺序出现即可（中间可隔字），顺序不可颠倒。
fn is_subsequence(q: &str, name: &str) -> bool {
    let mut chars = name.chars();
    for want in q.chars() {
        loop {
            match chars.next() {
                Some(c) if c == want => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

/// 一次搜索的结果。`truncated` 供 UI 显示 refine 提示。
#[derive(Debug)]
pub struct SearchOutcome<'a> {
    pub hits: Vec<&'a Row>,
    /// 命中总数（未截断前），用于 `50 / 137` 这类角标
    pub total: usize,
    pub truncated: bool,
}

/// 跨根搜索：过滤 + 排序 + 截断。
///
/// 候选只取 `kind` 可确认的行（`Table` / `File` / `Temp`）——「不搜分组名」是已裁决项；
/// 但**不**按 `selectable` 过滤：全局 `G_` 表要能被搜到并展示，只是不可确认（§09）。
pub fn search<'a>(query: &str, rows: &'a [Row], recent: &[NodeId]) -> SearchOutcome<'a> {
    if query.trim().is_empty() {
        return SearchOutcome {
            hits: Vec::new(),
            total: 0,
            truncated: false,
        };
    }

    let mut matched: Vec<(HitRank, &'a Row)> = rows
        .iter()
        .filter(|r| r.kind.is_confirmable())
        .filter_map(|r| rank(query, &r.label).map(|rk| (rk, r)))
        .collect();

    let total = matched.len();
    matched.sort_by(|(ra, a), (rb, b)| {
        rb.score()
            .cmp(&ra.score())
            .then_with(|| recent_pos(recent, &a.id).cmp(&recent_pos(recent, &b.id)))
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| a.label.cmp(&b.label))
    });

    let hits = matched
        .into_iter()
        .take(SEARCH_LIMIT)
        .map(|(_, row)| row)
        .collect();

    SearchOutcome {
        hits,
        total,
        truncated: total > SEARCH_LIMIT,
    }
}

/// 最近使用里的位次：越靠前越小；不在表内排最后。
fn recent_pos(recent: &[NodeId], id: &NodeId) -> usize {
    recent.iter().position(|r| r == id).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::source_picker::node::{NodeKind, TempScope};

    fn file_row(path: &str, label: &str) -> Row {
        Row::new(NodeId::Path(path.into()), 3, NodeKind::File, label)
    }

    #[test]
    fn rank_prefix_beats_substring_beats_subsequence() {
        assert_eq!(rank("ord", "orders"), Some(HitRank::Prefix));
        assert_eq!(rank("ord", "work_order"), Some(HitRank::Substring));
        assert_eq!(rank("ods", "orders"), Some(HitRank::Subsequence));
        assert_eq!(rank("zzz", "orders"), None);
    }

    #[test]
    fn rank_is_case_insensitive() {
        assert_eq!(rank("ORD", "Orders"), Some(HitRank::Prefix));
        assert_eq!(rank("orders", "ORDERS"), Some(HitRank::Prefix));
    }

    #[test]
    fn subsequence_respects_order() {
        // orders = o r d e r s；o→d→s 可行，o→s→d 不可行
        assert_eq!(rank("ods", "orders"), Some(HitRank::Subsequence));
        assert_eq!(rank("osd", "orders"), None);
    }

    #[test]
    fn search_sorts_by_rank_then_recent_then_alpha() {
        let rows = vec![
            file_row("/c.parquet", "o_r_d"),      // 子序列 40
            file_row("/b.parquet", "work_order"), // 子串 70
            file_row("/a.parquet", "orders"),     // 前缀 100
        ];
        let out = search("ord", &rows, &[]);
        assert_eq!(out.total, 3);
        let labels: Vec<&str> = out.hits.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["orders", "work_order", "o_r_d"]);
    }

    #[test]
    fn recent_wins_within_same_rank() {
        let a = file_row("/a.parquet", "orders");
        let b = file_row("/b.parquet", "order_items");
        let recent = vec![b.id.clone()];
        let rows = vec![a, b];
        let out = search("ord", &rows, &recent);
        assert_eq!(out.hits[0].label, "order_items", "最近用过的同档候选置顶");
    }

    #[test]
    fn alphabetical_within_same_rank_when_nothing_recent() {
        let rows = vec![
            file_row("/b.parquet", "b_orders"),
            file_row("/a.parquet", "a_orders"),
        ];
        let out = search("orders", &rows, &[]);
        assert_eq!(out.hits[0].label, "a_orders");
    }

    #[test]
    fn group_names_are_not_search_targets() {
        let rows = vec![
            Row::new(NodeId::Path("资产库".into()), 2, NodeKind::Group, "资产库 orders"),
            Row::new(
                NodeId::DbTable {
                    conn: "mysql_src".into(),
                    catalog: "analytics".into(),
                    schema: String::new(),
                    table: "orders".into(),
                },
                3,
                NodeKind::Table,
                "orders",
            ),
        ];
        let out = search("orders", &rows, &[]);
        assert_eq!(out.total, 1, "分组名不参与搜索");
        assert_eq!(out.hits[0].label, "orders");
    }

    #[test]
    fn disabled_rows_are_found_but_not_confirmable() {
        let global = Row::new(
            NodeId::Temp {
                scope: TempScope::Local,
                table: "G_orders".into(),
            },
            3,
            NodeKind::Table,
            "G_orders",
        )
        .disabled();
        let rows = vec![global];
        let out = search("orders", &rows, &[]);
        assert_eq!(out.total, 1, "要被搜到，否则用户找不到它为何缺");
        assert!(!out.hits[0].is_confirmable());
    }

    #[test]
    fn results_are_capped_and_flagged() {
        let rows: Vec<Row> = (0..60)
            .map(|i| file_row(&format!("/t{i}.parquet"), &format!("t_{i:03}")))
            .collect();
        let out = search("t_", &rows, &[]);
        assert_eq!(out.total, 60);
        assert_eq!(out.hits.len(), SEARCH_LIMIT);
        assert!(out.truncated);
    }

    #[test]
    fn empty_query_returns_nothing() {
        let rows = vec![file_row("/a.parquet", "orders")];
        let out = search("   ", &rows, &[]);
        assert_eq!(out.total, 0);
        assert!(!out.truncated);
        assert!(out.hits.is_empty());
    }

    #[test]
    fn navigation_rows_are_skipped() {
        let rows = vec![
            Row::new(NodeId::Path("conn".into()), 1, NodeKind::Conn, "orders_conn"),
            Row::new(NodeId::Path("dir".into()), 2, NodeKind::Folder, "orders_dir"),
        ];
        let out = search("orders", &rows, &[]);
        assert_eq!(out.total, 0);
    }
}
