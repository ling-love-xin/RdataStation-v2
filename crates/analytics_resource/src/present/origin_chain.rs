//! 来源链：顺着每一行的**直接上游**（`origin_kind` + `origin_ref`）往上追。
//!
//! 存储侧（`model/origin.rs`）故意只记一跳：A 的 v5 升级成全局 v1、全局 v1 下发给
//! B 的 v1，每层各自编号、各记直接上游。整条链由读取侧拼——冗余整条链会随上游新增
//! 版本而过期，而「往上看下一跳」永远是新的。
//!
//! 这里只做纯查找：`details` 是面板当前快照（项目行 + 全局行都在里面）。追不到就
//! **如实停住并说明**（已删 / 未刷新 / 脏记录），不猜、不跳过节点。

use std::collections::{HashMap, HashSet};

use crate::model::{ORIGIN_KIND_GLOBAL, ORIGIN_KIND_PROJECT};
use crate::detail_view::ArchiveDetail;

/// 最多往上追几跳：链条正常只有 A → 全局 → B 两跳；给 8 跳是给导入 / 再分发留余量，
/// 同时把环与脏数据挡在有限步内。
pub const MAX_CHAIN_HOPS: usize = 8;

/// 链条上的一个节点（已格式化为一句人读文案）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginChainNode {
    /// 例：`项目 · 订单宽表 · v5 · 文件`。
    pub label: String,
    /// 是否是打开详情的这一行（链的起点）。
    pub is_current: bool,
    /// `false` = 这一跳的资产不在当前快照里（已删 / 还没刷新 / 只在另一个项目）。
    pub resolved: bool,
}

/// 一条来源链（起点在最前，越往后越上游）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginChain {
    pub nodes: Vec<OriginChainNode>,
    /// 链为什么停在半路（None = 一直追到了没有上游的源头）。
    pub note: Option<String>,
}

/// 追一条详情往上的来源链。
pub fn build_origin_chain(
    detail: &ArchiveDetail,
    details: &HashMap<String, ArchiveDetail>,
) -> OriginChain {
    let mut nodes = vec![node_of(detail, true, true)];
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(detail.id.clone());

    let mut current = detail;
    let mut note: Option<String> = None;
    for _ in 0..MAX_CHAIN_HOPS {
        let Some(kind) = current.origin_kind.as_deref() else {
            break; // 没有上游 = 追到源头。
        };
        let Some(raw) = current.origin_ref.as_deref() else {
            note = Some("来源记录不完整（只有类型没有引用），链路停在这里".to_string());
            break;
        };
        let Some(hop) = parse_hop(kind, raw) else {
            note = Some(format!(
                "来源记录读不懂：{kind} / {raw}（可能是旧库或手工修库留下的脏值）"
            ));
            break;
        };
        if !visited.insert(hop.id.clone()) {
            note = Some(format!("来源链出现环：{} 又指回了上游，已停下", hop.id));
            break;
        }
        match details.get(&hop.id) {
            Some(next) => {
                nodes.push(node_of(next, false, true));
                current = next;
            }
            None => {
                nodes.push(OriginChainNode {
                    label: format!(
                        "{}（不在当前面板快照里：可能已删除，或在别的项目里）",
                        crate::model::describe(Some(kind), Some(raw))
                    ),
                    is_current: false,
                    resolved: false,
                });
                note = Some("上游资产不在当前快照里，链路到此为止".to_string());
                break;
            }
        }
    }

    if note.is_none() && current.origin_kind.is_some() {
        note = Some(format!("只展示最近 {MAX_CHAIN_HOPS} 跳（更早的来源未继续展开）"));
    }

    OriginChain { nodes, note }
}

/// 一跳的原样解析结果。
struct Hop {
    id: String,
}

fn parse_hop(kind: &str, raw: &str) -> Option<Hop> {
    let value = serde_json::from_str::<serde_json::Value>(raw).ok()?;
    let id = match kind {
        ORIGIN_KIND_PROJECT => value.get("resource_id").and_then(|v| v.as_str()),
        ORIGIN_KIND_GLOBAL => value.get("instance").and_then(|v| v.as_str()),
        _ => None,
    }?
    .trim()
    .to_string();
    (!id.is_empty()).then_some(Hop { id })
}

fn node_of(detail: &ArchiveDetail, is_current: bool, resolved: bool) -> OriginChainNode {
    let scope = match detail.scope {
        crate::model::RowScope::Project => "项目",
        crate::model::RowScope::Global => "全局",
    };
    OriginChainNode {
        label: format!(
            "{scope} · {} · v{} · {}",
            detail.name,
            detail.version,
            crate::detail_view::kind_label(detail.kind),
        ),
        is_current,
        resolved,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArchiveKind, ArchiveStatus, PayloadMode, RowScope};

    fn detail(id: &str, name: &str, scope: RowScope) -> ArchiveDetail {
        ArchiveDetail {
            assertion: None,
            lifecycle: Default::default(),
            id: id.to_string(),
            name: name.to_string(),
            alias: None,
            kind: ArchiveKind::File,
            version: 1,
            status: ArchiveStatus::Normal,
            readonly: false,
            payload_mode: PayloadMode::Copy,
            source_path: None,
            scope,
            size_label: String::new(),
            created_label: String::new(),
            modified_label: String::new(),
            archived_label: String::new(),
            promoted_from: None,
            origin_label: crate::model::describe(None, None),
            origin_kind: None,
            origin_ref: None,
            source_connection_id: None,
            source_table: None,
            content_hash: None,
            duckdb_readable: false,
            payload_rel_path: Some("a.sql".to_string()),
            analytics_table: None,
            has_definition: false,
            definition_sql: None,
            history_label: String::new(),
            version_timeline: Vec::new(),
            groups: Vec::new(),
            group: None,
            group_id: None,
            tags: Vec::new(),
            preview: crate::preview::Preview::default(),
            dispatch_label: String::new(),
            checkout_label: None,
        }
    }

    #[test]
    fn follows_two_hops_from_project_to_global_to_project() {
        let mut current = detail("arp_b", "B 的副本", RowScope::Project);
        current.origin_kind = Some(ORIGIN_KIND_GLOBAL.to_string());
        current.origin_ref = Some(r#"{"instance":"arg_g","version":1}"#.to_string());

        let mut global = detail("arg_g", "订单宽表", RowScope::Global);
        global.origin_kind = Some(ORIGIN_KIND_PROJECT.to_string());
        global.origin_ref = Some(r#"{"resource_id":"arp_a","version":5}"#.to_string());
        global.version = 1;

        let mut source = detail("arp_a", "订单宽表", RowScope::Project);
        source.version = 5;

        let mut details = HashMap::new();
        details.insert("arp_b".to_string(), current.clone());
        details.insert("arg_g".to_string(), global);
        details.insert("arp_a".to_string(), source);

        let chain = build_origin_chain(&current, &details);
        assert_eq!(chain.nodes.len(), 3, "当前 → 全局 → 项目 A");
        assert!(chain.nodes[0].is_current);
        assert_eq!(chain.nodes[0].label, "项目 · B 的副本 · v1 · 受管文件");
        assert_eq!(chain.nodes[1].label, "全局 · 订单宽表 · v1 · 受管文件");
        assert_eq!(chain.nodes[2].label, "项目 · 订单宽表 · v5 · 受管文件");
        assert!(chain.nodes.iter().all(|node| node.resolved));
        assert!(chain.note.is_none(), "追到源头不该留解释");
    }

    #[test]
    fn stops_honestly_when_upstream_is_not_in_the_snapshot() {
        let mut current = detail("arp_b", "副本", RowScope::Project);
        current.origin_kind = Some(ORIGIN_KIND_GLOBAL.to_string());
        current.origin_ref = Some(r#"{"instance":"arg_missing","version":1}"#.to_string());
        let chain = build_origin_chain(&current, &HashMap::new());
        assert_eq!(chain.nodes.len(), 2);
        assert!(!chain.nodes[1].resolved);
        assert!(chain.nodes[1].label.contains("arg_missing"));
        assert!(chain.note.as_deref().unwrap_or_default().contains("不在当前快照"));
    }

    #[test]
    fn dirty_ref_and_cycles_do_not_loop_forever() {
        let mut current = detail("arp_b", "副本", RowScope::Project);
        current.origin_kind = Some(ORIGIN_KIND_PROJECT.to_string());
        current.origin_ref = Some("not json".to_string());
        let chain = build_origin_chain(&current, &HashMap::new());
        assert_eq!(chain.nodes.len(), 1, "脏记录不产出假节点");
        assert!(chain.note.as_deref().unwrap_or_default().contains("读不懂"));

        // 自己指自己：立刻停，不转圈。
        let mut looped = detail("arp_a", "自指", RowScope::Project);
        looped.origin_kind = Some(ORIGIN_KIND_PROJECT.to_string());
        looped.origin_ref = Some(r#"{"resource_id":"arp_a","version":1}"#.to_string());
        let mut details = HashMap::new();
        details.insert("arp_a".to_string(), looped.clone());
        let chain = build_origin_chain(&looped, &details);
        assert_eq!(chain.nodes.len(), 1);
        assert!(chain.note.as_deref().unwrap_or_default().contains("环"));
    }
}
