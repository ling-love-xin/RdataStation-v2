//! 面板构造：宿主、搜索框与列表的惰性装配。

use super::*;

impl ResourcesPanel {
    pub const PANEL_NAME: &'static str = "analytics_resource";
    pub fn new(host: Rc<dyn ResourcesHost>, cx: &mut Context<Self>) -> Self {
        Self {
            host,
            snapshot: ResourcesSnapshot::default(),
            view_rows: Vec::new(),
            view_items: Vec::new(),
            collapsed: std::collections::HashSet::new(),
            default_group: None,
            filter: ResourcesFilter::default(),
            instance: InstanceFilter::default(),
            sort_field: SortField::default(),
            sort_order: SortOrder::default(),
            search_input: None,
            _search_sub: None,
            list: None,
            selected: None,
            multi: Vec::new(),
            anchor: None,
            notice: None,
            loading: false,
            undo: None,
            announced_index_issue: Cell::new(false),
            focus_handle: cx.focus_handle(),
            group: None,
        }
    }
    /// 懒创建搜索框与订阅（首帧渲染时执行一次，见 `search_input` 字段注释）。
    pub(super) fn ensure_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_input.is_some() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索存档"));
        // 用户真在打字的那条路径：输入变化 → 条件更新 → 重算可见行。
        //
        // 注意：`InputState::set_value` **不会**发 `InputEvent::Change`（gpui-component 内部注释明说），
        // 所以程序性改词（清空筛选 / 宿主预填）必须自己同步条件（见 `set_query` / `clear_filter`），
        // 不能指望这个订阅——否则输入框里的字与实际生效的条件会静默不一致。
        let sub = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                // 不用 `set_query`：它会把同一个值再写回输入框（正在输入时是无谓的往返）。
                this.filter.query = emitter.read(cx).value().to_string();
                this.refresh_view_rows(cx);
                cx.notify();
            },
        );
        self.search_input = Some(input);
        self._search_sub = Some(sub);
    }
    /// 懒创建列表状态（首帧渲染时执行一次，理由同 [`ensure_search_input`](Self::ensure_search_input)）。
    ///
    /// 空态（无可见行）时面板不渲染列表，但状态照建：行回来时直接可用，
    /// 不用在"有行的那一帧"再走一次创建。
    pub(super) fn ensure_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list.is_some() {
            return;
        }
        let delegate = ArchiveListDelegate {
            host: self.host.clone(),
            panel: cx.entity().downgrade(),
            items: self.view_items.clone(),
            details: self.snapshot.details.clone(),
            groups: self.snapshot.groups.clone(),
            default_group: self.default_group.clone(),
            selected_id: self.selected.clone(),
            multi_ids: self.multi.iter().cloned().collect(),
            syncing_from_panel: false,
            focused_header: None,
            read_only: self.snapshot.read_only,
        };
        let state = cx.new(|cx| ListState::new(delegate, window, cx).selectable(true));
        self.list = Some(state);
    }
}
