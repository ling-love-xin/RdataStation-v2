//! 只读访问器：选中、快照、行、过滤与排序。

use super::*;

impl ResourcesPanel {
    pub fn selected_id(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    pub fn snapshot(&self) -> &ResourcesSnapshot {
        &self.snapshot
    }
    /// 可见行（筛选 + 排序后）：render 与测试都读它。
    pub fn view_rows(&self) -> &[ArchiveRow] {
        &self.view_rows
    }
    /// 可见项（分组头 + 行）：列表委托渲染的就是它（测试用它验证折叠与分区）。
    pub fn view_items(&self) -> &[VisibleItem] {
        &self.view_items
    }
    /// 当前筛选条件。
    pub fn filter(&self) -> &ResourcesFilter {
        &self.filter
    }
    /// 当前排序（字段 + 方向）。
    pub fn sort(&self) -> (SortField, SortOrder) {
        (self.sort_field, self.sort_order)
    }
    /// 选中行的详情快照（右侧详情面板用）。
    ///
    /// 未选中、或快照里没有该行的详情时返回 `None`（详情面板据此显示空态）。
    pub fn selected_detail(&self) -> Option<&ArchiveDetail> {
        let id = self.selected.as_deref()?;
        self.snapshot.details.get(id)
    }
    /// 搜索框实体（宿主接 `Ctrl+F` 聚焦 / `Esc` 清空时用，见 Action 批）。
    ///
    /// 首帧渲染前为 `None`（懒创建，见字段注释）。
    pub fn search_input(&self) -> Option<&Entity<InputState>> {
        self.search_input.as_ref()
    }
}
