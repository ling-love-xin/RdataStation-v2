//! 列表同步：过滤结果 → 可见行 → ListDelegate。

use super::*;

impl ResourcesPanel {
    /// 把面板的选中（行 id）同步成列表的选中（索引）。
    ///
    /// 只在真的不一致时才动：宿主推送快照 / 筛选走的是无窗口路径，改不了列表的索引，
    /// 于是在渲染时补一次（这是把面板的选中"镜像"给组件，不是业务状态变更）。
    pub(super) fn sync_list_selection(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(list) = self.list.clone() else {
            return;
        };
        let target = self
            .selected
            .as_deref()
            .and_then(|id| {
                // 列表的“行”含分组头：索引必须按 `view_items` 数（不能用 `view_rows` 的位置）。
                self.view_items
                    .iter()
                    .position(|item| matches!(item, VisibleItem::Row(row) if row.id == id))
            })
            .map(IndexPath::new);
        list.update(cx, |state, cx| {
            if state.selected_index() != target {
                // 先立"面板镜像"标志：否则组件的回调会反过来更新面板，而此刻面板正在渲染。
                state.delegate_mut().syncing_from_panel = true;
                state.set_selected_index(target, window, cx);
                state.delegate_mut().syncing_from_panel = false;
            }
        });
    }
    /// 重算可见行（筛选 → 排序）并清理悬空选中，同时把行副本推给列表委托。
    ///
    /// 选中清理放在这里而不是只放在 `set_snapshot` 里：**筛选与排序也会让行消失**，
    /// 只盯着快照会留下一个指向"看不见的行"的选中态。
    pub(super) fn refresh_view_rows(&mut self, cx: &mut Context<Self>) {
        // 实例维度在**面板层**拼接（不进 `filter::apply_view`：全局行没有项目标签 /
        // 分组 / 异常概念，让它们进 `matches` 只会制造"永远筛不出"的维度）。
        let source: Vec<ArchiveRow> = match self.instance {
            InstanceFilter::Project => self.snapshot.rows.clone(),
            InstanceFilter::Global => self.snapshot.global_rows.clone(),
            InstanceFilter::All => {
                let mut merged = self.snapshot.rows.clone();
                merged.extend(self.snapshot.global_rows.iter().cloned());
                merged
            }
        };
        self.view_rows =
            filter::apply_view(&source, &self.filter, self.sort_field, self.sort_order);
        // 全局行可被选中但不给详情（详情映射只有项目行）；选中清理按 view_rows 走。
        if let Some(selected) = self.selected.as_deref() {
            if !self.view_rows.iter().any(|row| row.id == selected) {
                self.selected = None;
            }
        }
        // 多选集合同样清悬空项（筛选 / 排序也会让行消失）。
        self.multi
            .retain(|id| self.view_rows.iter().any(|row| &row.id == id));
        if self.multi.is_empty() {
            self.anchor = None;
        } else if self.multi.len() == 1 {
            // 只剩一条 = 回到单选态：焦点行就是它（不然详情面板与列表会各指一个）。
            self.selected = self.multi.first().cloned();
        }
        // 分组被删后同样清掉它的折叠标记（不然那条 key 永远留着）。
        let alive: Vec<&str> = self.snapshot.groups.iter().map(|g| g.id.as_str()).collect();
        self.collapsed.retain(|key| {
            key == filter::GROUP_ALL
                || key == filter::GROUP_UNGROUPED
                || alive.contains(&key.as_str())
        });
        self.refresh_view_items(cx);
    }
    /// 重建可见项（分组头 + 行）。折叠 / 展开与注入都走它，免得两处各抄一遍参数。
    pub(super) fn refresh_view_items(&mut self, cx: &mut Context<Self>) {
        self.view_items =
            filter::build_visible_items(&self.view_rows, &self.snapshot.groups, &self.collapsed);
        self.push_rows_to_list(cx);
    }
    /// 把可见行推给列表委托。
    ///
    /// 列表尚未创建时什么都不做——首帧渲染会带着现有行创建它（`ensure_list`）。
    pub(super) fn push_rows_to_list(&self, cx: &mut Context<Self>) {
        let Some(list) = self.list.clone() else {
            return;
        };
        let items = self.view_items.clone();
        let details = self.snapshot.details.clone();
        let groups = self.snapshot.groups.clone();
        let default_group = self.default_group.clone();
        let selected = self.selected.clone();
        let multi = self.multi.iter().cloned().collect();
        let read_only = self.snapshot.read_only;
        list.update(cx, |state, cx| {
            state.delegate_mut().set_rows(
                items,
                details,
                groups,
                default_group,
                selected,
                multi,
                read_only,
                cx,
            );
        });
    }
}
