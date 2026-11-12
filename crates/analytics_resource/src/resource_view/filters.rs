//! 筛选与排序：快照、类型、标签、查询与排序。

use super::*;

impl ResourcesPanel {
    /// 宿主推送数据（事件路径调用；面板不自己取数）。
    pub fn set_snapshot(&mut self, snapshot: ResourcesSnapshot, cx: &mut Context<Self>) {
        // 断言搜索索引随快照整份替换：它是筛选器里的派生数据，留着上一轮的会搜到已删的口径。
        self.filter.assertion_index = snapshot.assertion_index.clone();
        self.snapshot = snapshot;
        // 标签被删后清掉悬空条件（否则列表会“什么都没匹配”，而菜单上的勾还在）——
        // 与选中 / 多选的悬空清理同一个道理，但条件在筛选器里，所以在推送时先清。
        let alive: Vec<String> = self
            .snapshot
            .tags
            .iter()
            .map(|tag| tag.id.clone())
            .collect();
        self.filter.tags.retain(|id| alive.contains(id));
        self.refresh_view_rows(cx);
        cx.notify();
    }
    /// 换一套筛选条件（新建 / 清空走这里）。
    pub fn set_filter(&mut self, filter: ResourcesFilter, cx: &mut Context<Self>) {
        // 断言索引不随筛选条件一起被丢掉：它是快照下发的派生数据，整份替换 filter
        // 会让它变空，语义搜索要等下一次刷新才恢复（用户看到的是「搜口径没反应」）。
        let assertion_index = std::mem::take(&mut self.filter.assertion_index);
        self.filter = filter;
        self.filter.assertion_index = assertion_index;
        self.refresh_view_rows(cx);
        cx.notify();
    }
    /// 切换实例维度（§13：全部 / 项目 / 全局）。行源变了，悬空选中由
    /// [`Self::refresh_view_rows`] 顺带清理。
    pub fn set_instance(&mut self, instance: InstanceFilter, cx: &mut Context<Self>) {
        if self.instance == instance {
            return;
        }
        self.instance = instance;
        self.refresh_view_rows(cx);
        cx.notify();
    }
    /// 设置搜索词：条件与输入框**同一次改**（见 `ensure_search_input` 里的注释）。
    pub fn set_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.query = text.to_string();
        if let Some(input) = self.search_input.clone() {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
        self.refresh_view_rows(cx);
        cx.notify();
    }
    /// 勾选 / 取消一个种类（全选会被规范化为不限，见 `ResourcesFilter::toggle_kind`）。
    pub fn toggle_kind(&mut self, kind: ArchiveKind, cx: &mut Context<Self>) {
        self.filter.toggle_kind(kind);
        self.refresh_view_rows(cx);
        cx.notify();
    }
    /// 只看需处理的异常（缺失 / 内容已变）。
    pub fn toggle_only_issues(&mut self, cx: &mut Context<Self>) {
        self.filter.only_issues = !self.filter.only_issues;
        self.refresh_view_rows(cx);
    }
    /// 勾选 / 取消一个标签（多选是并集，见 `ResourcesFilter::tags`）。
    pub fn toggle_tag(&mut self, tag_id: &str, cx: &mut Context<Self>) {
        self.filter.toggle_tag(tag_id);
        self.refresh_view_rows(cx);
    }
    /// 选排序字段：同一字段再点一次翻转方向；换字段保持当前方向
    /// （沿用 M4 与 v1 `use-pagination` 的语义——用户刚调完方向再换字段，不该被重置）。
    ///
    /// 选完把（字段 + 方向）告诉宿主——它会把**字段**写进设置（`resources.default_sort`），
    /// 下次打开面板就还是这一列。
    pub fn choose_sort(&mut self, field: SortField, cx: &mut Context<Self>) {
        if field == self.sort_field {
            self.sort_order = self.sort_order.flipped();
        } else {
            self.sort_field = field;
        }
        self.host
            .remember_sort(self.sort_field, self.sort_order, cx);
        self.refresh_view_rows(cx);
        cx.notify();
    }
    /// 直接设排序（宿主在构造期注入设置项里的默认排序）。
    ///
    /// 与 [`choose_sort`](Self::choose_sort) 分开：这一条**不回调宿主**——否则注入的默认值
    /// 会被原样写回去，看不出写回到底是用户的动作还是自己的回声。
    pub fn set_sort(&mut self, field: SortField, order: SortOrder, cx: &mut Context<Self>) {
        self.sort_field = field;
        self.sort_order = order;
        self.refresh_view_rows(cx);
        cx.notify();
    }
    /// 清空筛选（含搜索框）：只在"没有匹配"的空态给这个入口。
    pub fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = ResourcesFilter::default();
        if let Some(input) = self.search_input.clone() {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        self.refresh_view_rows(cx);
        cx.notify();
    }
}
