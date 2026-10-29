//! 工具栏布局操作（move/separator/reset/execute_menu_kind / toolbar_layout_for_test）
//!
//! 自 host/toolbar.rs 拆分（2026-10-29 Step 8）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use crate::execution::{self, ExecMenuKind};

impl EditorHostPanel {
    pub(crate) fn insert_template(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("文档只读，无法插入模板".to_string()), cx);
            return;
        }
        let templates = self.shared.completion_templates();
        let Some(template) = templates.get(index) else {
            return;
        };
        let content = template.content.clone();
        let name = template.name.clone();
        let start = {
            let state = self.editor.read(cx);
            state.cursor()
        };
        self.editor
            .update(cx, |state, cx| state.insert(content.clone(), window, cx));
        if let Some((from, to)) = crate::completion::placeholder_range(&content) {
            self.editor.update(cx, |state, cx| {
                state.set_selected_range(start + from..start + to, cx)
            });
        }
        self.set_message(Some(format!("已插入模板「{name}」")), cx);
    }

    /// 【T2c-5】工具栏单项动作的统一分发（渲染只按 id 挂事件，语义集中在这里）
    ///
    /// 折叠 ▾ 菜单点击项也走这里（执行族）。`More` 是嵌套菜单本身、不参与折叠菜单，
    /// 这里兜底空转（不会真被调到——`render_collapsed_group` 已把它过滤掉）。
    /// 格式化的 `&mut Window` 从事件回调（按钮 / 菜单项都带窗口）透传进来。
    pub(crate) fn run_toolbar_item_action(
        &mut self,
        id: crate::toolbar::ToolbarItemId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::toolbar::ToolbarItemId;
        match id {
            ToolbarItemId::ExecRun => self.execute_preferring_selection(cx),
            ToolbarItemId::ExecAll => self.run_all(cx),
            ToolbarItemId::ExecCurrent => self.execute_menu_kind(ExecMenuKind::CurrentStatement, cx),
            ToolbarItemId::ExecSelection => self.execute_menu_kind(ExecMenuKind::Selection, cx),
            ToolbarItemId::ExecBatch => self.execute_menu_kind(ExecMenuKind::Batch, cx),
            ToolbarItemId::ExecNewSet => self.execute_menu_kind(ExecMenuKind::NewSet, cx),
            ToolbarItemId::Format => self.format_document(window, cx),
            // Explain / Translate / Template / TxAutoCommit / History 是带下拉的文档级
            // 按钮（`dropdown_menu` 回调内直接调方法，不走这条兜底）；折叠菜单只收执行族
            // （见 `render_collapsed_group`），这里兜底空转（不会真被调到）
            ToolbarItemId::Explain => {}
            ToolbarItemId::Translate => {}
            ToolbarItemId::Template => {}
            ToolbarItemId::TxAutoCommit => {}
            ToolbarItemId::History => {}
        }
    }

    /// 【T2c-5】按显式菜单目标执行（`target_for_menu`；工具栏显式目标按钮与折叠 ▾ 菜单共用）
    pub(crate) fn execute_menu_kind(&mut self, kind: ExecMenuKind, cx: &mut Context<Self>) {
        let (text, selection) = self.editor_snapshot(cx);
        let target = execution::target_for_menu(kind, &text, selection);
        self.execute(target, kind.placement(), cx);
    }

    /// 【T2c-5】面板：切换某项可见性（`fixed_visible` 项不可隐藏，面板上 Checkbox 置灰）
    ///
    /// 返回是否真的变了（测试断言「不可隐藏」用）。
    pub(crate) fn set_toolbar_visible(
        &mut self,
        id: crate::toolbar::ToolbarItemId,
        on: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.toolbar.set_visible(id, on);
        cx.notify();
        changed
    }

    /// 【T2c-5】面板：单项移动（可跨组；跨过分隔符自然形成新组）
    pub(crate) fn move_toolbar_item(
        &mut self,
        id: crate::toolbar::ToolbarItemId,
        dir: crate::toolbar::MoveDir,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.move_item(id, dir);
        cx.notify();
    }

    /// 【T2c-5】面板：整组移动（含段尾分隔符；`false` = 已在边缘）
    pub(crate) fn move_toolbar_group(
        &mut self,
        head: crate::toolbar::ToolbarItemId,
        dir: crate::toolbar::MoveDir,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.move_group(head, dir);
        cx.notify();
    }

    /// 【T2c-5】面板：切换某组折叠（折叠 = 只显示首组件 + ▾）
    pub(crate) fn toggle_toolbar_collapsed(
        &mut self,
        head: crate::toolbar::ToolbarItemId,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.toggle_collapsed(head);
        cx.notify();
    }

    /// 【T2c-5】面板：在左区末尾加一条竖线分隔符
    pub(crate) fn insert_toolbar_separator(&mut self, cx: &mut Context<Self>) {
        self.toolbar.insert_separator();
        cx.notify();
    }

    /// 【T2c-5】面板：移除某条竖线分隔符
    pub(crate) fn remove_toolbar_separator(&mut self, index: usize, cx: &mut Context<Self>) {
        self.toolbar.remove_separator(index);
        cx.notify();
    }

    /// 【T2c-5】面板：移动某条竖线分隔符（跨过一项即改变组边界）
    pub(crate) fn move_toolbar_separator(
        &mut self,
        index: usize,
        dir: crate::toolbar::MoveDir,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.move_separator(index, dir);
        cx.notify();
    }

    /// 【T2c-5】面板：重置默认（清折叠 / 可见性回默认 / 删新增分隔符）
    pub(crate) fn reset_toolbar(&mut self, cx: &mut Context<Self>) {
        self.toolbar.reset();
        cx.notify();
    }

    /// 【T2c-5】测试访问器：当前工具栏布局快照（断言折叠 / 可见性 / 顺序）
    pub fn toolbar_layout_for_test(&self) -> crate::toolbar::ToolbarLayout {
        self.toolbar.clone()
    }

}
