//! 选择：单选、点击语义与多选。

use super::*;

impl ResourcesPanel {
    /// 宿主驱动选中（生产入口；列表选中变化也走它）。
    ///
    /// 单选语义：选择集重置为这一条（多选是用户手势的产物，不由宿主推入）。
    pub fn set_selected(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.multi = id.iter().cloned().collect();
        self.anchor = id.clone();
        self.selected = id;
        cx.notify();
    }
    /// 行点击（生产入口：行的 `on_click` 归一化后调它；窗口测试也走这里）。
    ///
    /// 四种语义见 [`RowClick`]；双击只更新选中，**打开由行的回调直接转给宿主**
    /// （面板不认识服务层，也不该在这里再拼一条打开请求）。
    pub fn handle_row_click(&mut self, id: &str, click: RowClick, cx: &mut Context<Self>) {
        let order: Vec<String> = self.view_rows.iter().map(|row| row.id.clone()).collect();
        let focus = apply_row_click(&mut self.multi, &mut self.anchor, &order, id, click);
        // 焦点行总是最后点的那条：详情面板跟它走；选择集空 = 没有焦点。
        self.selected = focus;
        cx.notify();
    }
    /// 全选当前可见行（`Ctrl+A`；原型 §2.3 说“当前分组”，分组未落 → 即全部可见行）。
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.multi = self.view_rows.iter().map(|row| row.id.clone()).collect();
        // 焦点行：原来的还在就保留（详情面板不跳），否则取第一条。
        let keep = self
            .selected
            .as_deref()
            .is_some_and(|selected| self.multi.iter().any(|id| id == selected));
        if !keep {
            self.selected = self.multi.first().cloned();
        }
        self.anchor = self.selected.clone();
        cx.notify();
    }
    /// 当前选择集（多选优先；空 = 没有选中）——批量动作与窗口测试读它。
    pub fn selected_ids(&self) -> Vec<String> {
        self.multi.clone()
    }
}
