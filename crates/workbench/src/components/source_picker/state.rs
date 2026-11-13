//! picker 的树态：展开、搜索词、选中、最近使用（`06-object-tree.html` §07 步 2）。
//!
//! 为什么必须是独立状态对象：模态渲染闭包每帧重建 UI，树态与搜索态不能放在闭包里，
//! 否则一帧就丢。它现在由 `TreeState` 承载，`render_rows` 那一刀起由 `PickerState` 持有
//! （现在先不塞进 `PickerState`，避免出现没人读的字段）。
//!
//! 规范出处：
//! - §06 差异表：picker 的展开态是**纯内存，随对话框关闭丢弃**（不落设置）；
//! - §03：搜索词 `Esc` 只清词、不清选中；
//! - README：最近使用置顶 3–5 条 → 这里取 5；
//! - §08 未决「最近使用存哪」，故本结构只管内存语义，持久化留给调用方注入。

use std::collections::HashSet;

use super::node::{NodeId, NodeKind, Row};
use super::search::search;

/// 最近使用保留条数（README：置顶 3–5 条）
pub const RECENT_CAP: usize = 5;

/// 树与搜索的交互状态。
#[derive(Debug, Default, Clone)]
pub struct TreeState {
    expanded: HashSet<NodeId>,
    query: String,
    selected: Option<NodeId>,
    recent: Vec<NodeId>,
}

impl TreeState {
    pub fn new() -> Self {
        Self::default()
    }

    // ---- 展开态 --------------------------------------------------------

    pub fn is_expanded(&self, id: &NodeId) -> bool {
        self.expanded.contains(id)
    }

    /// 点 chevron / 按 `→`：展开；再点一次收起。
    pub fn toggle(&mut self, id: &NodeId) {
        if !self.expanded.remove(id) {
            self.expanded.insert(id.clone());
        }
    }

    pub fn expand(&mut self, id: &NodeId) {
        self.expanded.insert(id.clone());
    }

    /// 按 `←`：收起当前节点。
    pub fn collapse(&mut self, id: &NodeId) {
        self.expanded.remove(id);
    }

    /// 收起全部：清搜索词时一并回到「只剩根」的形态。
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
    }

    // ---- 搜索词 --------------------------------------------------------

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
    }

    /// `Esc` 语义（§03）：只清搜索词，**不清选中**——与草稿箱 / 资产库一致。
    pub fn clear_query(&mut self) {
        self.query.clear();
    }

    pub fn is_searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    // ---- 选中 ----------------------------------------------------------

    pub fn selected(&self) -> Option<&NodeId> {
        self.selected.as_ref()
    }

    pub fn select(&mut self, id: Option<NodeId>) {
        self.selected = id;
    }

    /// 上下键（§04）：在**可见**行里移动选中，只停在可确认的行
    /// （组 / 连接 / 目录跳过——它们点了是展开，不是选中）。
    ///
    /// 到头了返回 `false` 且不动选中：让焦点从末尾「跳回开头」是更常见的选择，
    /// 但那会让「一直按住 ↓」变成循环滚动，反而不好定位；这里选择停在边界。
    pub fn step_selection(&mut self, rows: &[Row], forward: bool) -> bool {
        let targets: Vec<NodeId> = self
            .visible_rows(rows)
            .iter()
            .filter(|r| r.is_confirmable())
            .map(|r| r.id.clone())
            .collect();
        if targets.is_empty() {
            return false;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|id| targets.iter().position(|t| t == id));
        let next = match current {
            Some(i) if forward => i + 1,
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        if next >= targets.len() {
            return false;
        }
        self.select(Some(targets[next].clone()));
        true
    }

    // ---- 最近使用 ------------------------------------------------------

    pub fn recent(&self) -> &[NodeId] {
        &self.recent
    }

    /// 确认选中时调用：去重后置顶，超出 `RECENT_CAP` 丢最旧的。
    pub fn push_recent(&mut self, id: NodeId) {
        self.recent.retain(|r| r != &id);
        self.recent.insert(0, id);
        self.recent.truncate(RECENT_CAP);
    }

    // ---- 可见行 --------------------------------------------------------

    /// 当前该渲染哪些行。
    ///
    /// - 有搜索词：跨三个根直接给命中（不要求先展开树，§03）；
    /// - 无搜索词：按折叠态拍平——被收起的节点的后代整段隐藏，
    ///   直到同级或更浅的一行出现为止（行是带 `depth` 的扁平列表，不需要显式父指针）。
    pub fn visible_rows<'a>(&self, rows: &'a [Row]) -> Vec<&'a Row> {
        if self.is_searching() {
            return search(&self.query, rows, &self.recent).hits;
        }
        self.flatten(rows)
    }

    fn flatten<'a>(&self, rows: &'a [Row]) -> Vec<&'a Row> {
        let mut out = Vec::with_capacity(rows.len());
        // 当前被收起节点所处的深度：其所有更深的后代都跳过
        let mut hidden_below: Option<u8> = None;
        for row in rows {
            if let Some(hd) = hidden_below {
                if row.depth > hd {
                    continue;
                }
                hidden_below = None;
            }
            out.push(row);
            let expandable = matches!(
                row.kind,
                NodeKind::Group | NodeKind::Conn | NodeKind::Folder
            );
            if expandable && !self.is_expanded(&row.id) {
                hidden_below = Some(row.depth);
            }
        }
        out
    }

    /// 选中行是否仍可见（收起祖先 / 改了搜索词后可能不再可见）。
    pub fn selected_is_visible(&self, rows: &[Row]) -> bool {
        match self.selected() {
            None => false,
            Some(id) => self.visible_rows(rows).iter().any(|r| &r.id == id),
        }
    }

    /// 搜索结果被截断时的提示（`total / SEARCH_LIMIT`，供「refine」角标）。
    pub fn search_progress(&self, rows: &[Row]) -> Option<(usize, usize)> {
        if !self.is_searching() {
            return None;
        }
        let out = search(&self.query, rows, &self.recent);
        out.truncated.then_some((out.hits.len(), out.total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::source_picker::node::TempScope;
    use crate::components::source_picker::search::SEARCH_LIMIT;

    fn group(label: &str, depth: u8) -> Row {
        Row::new(NodeId::Path(label.into()), depth, NodeKind::Group, label)
    }

    fn conn(label: &str, depth: u8) -> Row {
        Row::new(NodeId::Path(label.into()), depth, NodeKind::Conn, label)
    }

    fn folder(label: &str, depth: u8) -> Row {
        Row::new(NodeId::Path(label.into()), depth, NodeKind::Folder, label)
    }

    fn table(conn: &str, table: &str, depth: u8) -> Row {
        Row::new(
            NodeId::DbTable {
                conn: conn.into(),
                catalog: "analytics".into(),
                schema: String::new(),
                table: table.into(),
            },
            depth,
            NodeKind::Table,
            table,
        )
    }

    /// 数据库根：数据库(0) → mysql(1) → analytics(2) → orders(3)，外加同级 pg(1)
    fn db_rows() -> Vec<Row> {
        vec![
            group("数据库", 0),
            conn("mysql", 1),
            folder("analytics", 2),
            table("mysql", "orders", 3),
            conn("pg", 1),
            table("pg", "events", 3),
        ]
    }

    #[test]
    fn collapsed_root_hides_everything_below_it() {
        let rows = db_rows();
        let st = TreeState::new();
        let visible: Vec<String> = st.visible_rows(&rows).iter().map(|r| r.label.clone()).collect();
        assert_eq!(visible, vec!["数据库"], "根默认收起，只剩根一行");
    }

    #[test]
    fn expanding_a_node_reveals_only_its_direct_level() {
        let rows = db_rows();
        let mut st = TreeState::new();
        st.expand(&rows[0].id); // 展开「数据库」
        let visible: Vec<&str> = st.visible_rows(&rows).iter().map(|r| r.label.as_str()).collect();
        assert_eq!(visible, vec!["数据库", "mysql", "pg"], "连接层可见，表仍被收起挡住");

        st.expand(&rows[0].id); // 幂等：重复展开无副作用
        st.expand(&rows[1].id); // 展开 mysql
        st.expand(&rows[2].id); // 展开 analytics
        let visible: Vec<&str> = st.visible_rows(&rows).iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            visible,
            vec!["数据库", "mysql", "analytics", "orders", "pg"],
            "pg 的子表 events 仍未展开"
        );
    }

    #[test]
    fn collapsing_mid_node_keeps_later_siblings() {
        let rows = db_rows();
        let mut st = TreeState::new();
        for r in &rows[..3] {
            st.expand(&r.id);
        }
        st.collapse(&rows[1].id); // 收起 mysql
        let visible: Vec<&str> = st.visible_rows(&rows).iter().map(|r| r.label.as_str()).collect();
        assert_eq!(visible, vec!["数据库", "mysql", "pg"], "mysql 后代隐藏，同级 pg 不受影响");
    }

    #[test]
    fn toggle_is_idempotent_pair() {
        let id = NodeId::Path("x".into());
        let mut st = TreeState::new();
        assert!(!st.is_expanded(&id));
        st.toggle(&id);
        assert!(st.is_expanded(&id));
        st.toggle(&id);
        assert!(!st.is_expanded(&id));
    }

    #[test]
    fn search_ignores_collapsed_state() {
        let rows = db_rows();
        let mut st = TreeState::new(); // 全收起
        st.set_query("orders");
        let visible: Vec<&str> = st.visible_rows(&rows).iter().map(|r| r.label.as_str()).collect();
        assert_eq!(visible, vec!["orders"], "搜索跨根直达，不要求先展开");
    }

    #[test]
    fn esc_clears_query_but_keeps_selection() {
        let rows = db_rows();
        let mut st = TreeState::new();
        st.select(Some(rows[3].id.clone()));
        st.set_query("orders");
        st.clear_query();
        assert_eq!(st.query(), "");
        assert_eq!(st.selected(), Some(&rows[3].id), "Esc 不清选中（§03）");
    }

    #[test]
    fn selection_visibility_tracks_collapse() {
        let rows = db_rows();
        let mut st = TreeState::new();
        let target = rows[3].id.clone(); // orders
        st.select(Some(target.clone()));
        assert!(!st.selected_is_visible(&rows), "祖先全收起时选中行不可见");
        for r in &rows[..3] {
            st.expand(&r.id);
        }
        assert!(st.selected_is_visible(&rows));
        st.set_query("events");
        assert!(!st.selected_is_visible(&rows), "换了搜索词后不再出现在结果里");
    }

    #[test]
    fn recent_is_deduped_capped_and_newest_first() {
        let mut st = TreeState::new();
        for i in 0..7 {
            st.push_recent(NodeId::Path(format!("p{i}")));
        }
        assert_eq!(st.recent().len(), RECENT_CAP);
        assert_eq!(st.recent()[0], NodeId::Path("p6".into()), "最新置顶");
        assert!(!st.recent().contains(&NodeId::Path("p0".into())), "最旧的被淘汰");

        // 重复使用不会撑大，只是前移
        st.push_recent(NodeId::Path("p4".into()));
        assert_eq!(st.recent().len(), RECENT_CAP);
        assert_eq!(st.recent()[0], NodeId::Path("p4".into()));
    }

    #[test]
    fn recent_feeds_search_ranking() {
        let rows = vec![
            Row::new(
                NodeId::Temp {
                    scope: TempScope::Session("c".into()),
                    table: "order_items".into(),
                },
                3,
                NodeKind::Temp,
                "order_items",
            ),
            Row::new(NodeId::Path("orders".into()), 2, NodeKind::Folder, "orders_dir"),
            table("mysql", "orders", 3),
        ];
        let mut st = TreeState::new();
        st.push_recent(rows[0].id.clone());
        st.set_query("order");
        let visible: Vec<&str> = st.visible_rows(&rows).iter().map(|r| r.label.as_str()).collect();
        assert_eq!(visible, vec!["order_items", "orders"], "最近用过的排前；Folder 不参与");
    }

    /// 上下键只停在可确认行：组 / 连接 / 目录一律跳过。
    #[test]
    fn arrow_keys_skip_navigation_rows() {
        let rows = db_rows();
        let mut st = TreeState::new();
        for r in &rows[..3] {
            st.expand(&r.id); // 数据库 / mysql / analytics 全展开
        }
        assert!(st.step_selection(&rows, true), "从无选中开始，落到第一张可确认的表");
        assert_eq!(st.selected(), Some(&rows[3].id), "orders：跳过组、连接、目录");

        assert!(!st.step_selection(&rows, true), "后面只有收起的 pg 子树，走到底不动");
        assert_eq!(st.selected(), Some(&rows[3].id));

        st.expand(&rows[4].id); // 展开 pg
        assert!(st.step_selection(&rows, true), "pg 展开后能落到 events");
        assert_eq!(st.selected(), Some(&rows[5].id));

        assert!(st.step_selection(&rows, false), "↑ 回到 orders");
        assert_eq!(st.selected(), Some(&rows[3].id));
    }

    /// 搜索态下 ↑↓ 走的是命中列表，不是整棵树。
    #[test]
    fn arrow_keys_follow_search_hits() {
        let rows = db_rows();
        let mut st = TreeState::new();
        st.set_query("o"); // 命中 orders / pg… 逐个确认
        let hits: Vec<NodeId> = st
            .visible_rows(&rows)
            .iter()
            .filter(|r| r.is_confirmable())
            .map(|r| r.id.clone())
            .collect();
        assert!(!hits.is_empty());
        assert!(st.step_selection(&rows, true));
        assert_eq!(st.selected(), Some(&hits[0]));
    }

    #[test]
    fn search_progress_reports_truncation() {
        let rows: Vec<Row> = (0..SEARCH_LIMIT + 7)
            .map(|i| Row::new(NodeId::Path(format!("p{i}")), 3, NodeKind::File, format!("f{i:03}")))
            .collect();
        let mut st = TreeState::new();
        st.set_query("f");
        assert_eq!(
            st.search_progress(&rows),
            Some((SEARCH_LIMIT, SEARCH_LIMIT + 7)),
            "超出 50 条要给 refine 角标"
        );
        st.set_query("f057");
        assert_eq!(st.search_progress(&rows), None);
    }
}
