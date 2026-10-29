//! 诊断访问器与测试辅助（*_for_test / grid / caret / edit_mode）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use result_panel::panel::ResultPanel;
use crate::view::results::error_card::ErrorCard;
use crate::view::results::grid::{ResultGridDelegate, ResultStatus, ResultToolbar};

impl EditorHostPanel {

    /// 【B6】编辑内核里的诊断条数（错误回填画了没有）
    pub fn diagnostics_for_test(&self, cx: &App) -> usize {
        self.editor
            .read(cx)
            .diagnostics()
            .map(|set| set.len())
            .unwrap_or(0)
    }

    /// 结果工具栏（⑥）的左段文案（供测试断言；`None` = 还没有结论）
    ///
    /// 就是 `ResultToolbar::segments()` 拼起来的那串，原型 §2.4 的 `行数 1,204 │ 耗时 1.2s │ orders`。
    /// Dock 化后状态在面板实体，经只读访问器转发。
    pub fn result_summary_for_test(&self, cx: &App) -> Option<String> {
        self.result_panel
            .read(cx)
            .toolbar()
            .map(|toolbar| toolbar.segments().join(" · "))
    }

    /// 结果工具栏（⑥）的真实输入（供测试断言各段与显隐）
    pub fn result_toolbar_for_test(&self, cx: &App) -> Option<ResultToolbar> {
        self.result_panel.read(cx).toolbar().cloned()
    }

    /// 结果状态行（⑦）的真实输入（供测试断言总行数 / 截断提示）
    pub fn result_status_for_test(&self, cx: &App) -> Option<ResultStatus> {
        self.result_panel.read(cx).status().cloned()
    }

    /// 测试用：当前呈现方式（网格 / 文本）——验证左竖条按钮真实点击链路
    pub fn presentation_for_test(&self, cx: &App) -> result_panel::ports::Presentation {
        self.result_panel.read(cx).presentation()
    }

    /// 当前选中结果集的可重跑 SQL / 可复制（供测试断言按钮显隐）
    pub fn result_actions_for_test(&self, cx: &App) -> (Option<String>, bool) {
        let panel = self.result_panel.read(cx);
        (panel.sql().map(str::to_string), panel.can_copy())
    }

    /// 【B9 切片二】补全弹层状态（供测试断言“真的弹了”、查询词与候选）
    ///
    /// 读的是内核的 `completion_menu_state()`（`#[doc(hidden)] pub`）——**不是我们的副本**：
    /// 断言的就是内核眼里“弹层开着”这件事。
    pub fn completion_menu_for_test(&self, cx: &App) -> (bool, String, Vec<String>) {
        let state = self.editor.read(cx);
        let menu = state.completion_menu_state();
        (
            menu.open,
            menu.query.clone(),
            menu.items.iter().map(|item| item.label.clone()).collect(),
        )
    }

    /// 【B6】错误卡片的内容（供测试断言摘要与定位文案）
    pub fn result_error_card_for_test(&self, cx: &App) -> Option<ErrorCard> {
        self.result_panel.read(cx).error_card().cloned()
    }

    /// 【2b】脚本输出卡片（供测试断言脚本执行的结果形态；`None` = 还没有结论）
    pub fn script_card_for_test(
        &self,
        _cx: &App,
    ) -> Option<crate::view::results::script_card::ScriptCard> {
        // 脚本卡真值留在宿主（回执写入；sync 经输入透传面板），这里直接读宿主
        self.script_card.clone()
    }

    /// 【B5】选中网格里的某一行（供测试断言状态行⑦的“已选第 N 行”；真机上点行就是这条路）
    pub fn select_grid_row_for_test(&mut self, row: usize, cx: &mut Context<Self>) {
        self.grid
            .update(cx, |state, cx| state.set_selected_row(row, cx));
    }

    /// 结果集标签（供测试断言“批量跑出三个结果集”与标签文案）
    pub fn result_tabs_for_test(&self, cx: &App) -> Vec<(String, bool)> {
        self.result_panel
            .read(cx)
            .tabs()
            .iter()
            .map(|tab| (tab.label.clone(), tab.failed))
            .collect()
    }

    /// 【B13】结果集标签的通道徽标（供测试断言“每份结果记住了自己是哪档跑的”）
    pub fn result_badges_for_test(&self, cx: &App) -> Vec<String> {
        self.result_panel.read(cx).tabs().iter().map(|tab| tab.badge()).collect()
    }

    /// 【B13】结果区顶部那一行提示（切通道后旧结果那一条）
    pub fn result_notice_for_test(&self, cx: &App) -> Option<String> {
        self.result_panel.read(cx).notice().map(str::to_string)
    }

    /// 当前选中的结果集下标（供测试断言点标签真的切了）
    pub fn result_active_for_test(&self, cx: &App) -> usize {
        self.result_panel.read(cx).active()
    }

    /// 还有几条语句没回填（供测试断言“批量执行至少知道自己在跑什么”）
    pub fn pending_for_test(&self) -> usize {
        self.pending
    }

    /// 结果网格当前行数（供测试断言网格真的拿到了数据）
    pub fn grid_row_count_for_test(&self, cx: &App) -> usize {
        // 【B15】看的是**视图行数**：筛选生效时网格里就是命中那些行
        self.grid.read(cx).delegate().visible_row_count()
    }

    /// 【并排对比】对比模式开关（供测试断言开关真的翻转）
    pub fn compare_mode_for_test(&self, cx: &App) -> bool {
        self.result_panel.read(cx).compare_mode()
    }

    /// 【并排对比】对比栏网格当前行数（供测试断言右栏真的拿到了数据）
    pub fn compare_grid_rows_for_test(&self, cx: &App) -> usize {
        self.compare_grid
            .as_ref()
            .map(|grid| grid.read(cx).delegate().visible_row_count())
            .unwrap_or(0)
    }

    /// 网格实体（测试用：滚动到底自动加载得真在表格上调 `load_more` 才算验到链路）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn grid_for_test(&self) -> Entity<TableState<ResultGridDelegate>> {
        self.grid.clone()
    }

    /// 结果面板实体（测试用：筛选防抖等面板内行为直接驱动面板方法；生产不经此路径）
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn result_panel_for_test(&self) -> Entity<ResultPanel> {
        self.result_panel.clone()
    }

    /// 结果面板实体（编辑器宿主内嵌：`v_resizable` 分栏渲染活动文档的结果区；2026-10-27 起
    /// 不再挂全局 dock 薄壳，此入口供编辑器内部与测试取面板实体）
    pub fn result_panel(&self) -> Entity<ResultPanel> {
        self.result_panel.clone()
    }

    /// 【类型样式】全局偏好即时生效入口：读当前 Settings 的 `result_panel.type_style()`
    /// 推给结果面板（面板再推 delegate 重绘）。workbench 设置页变更钩子
    /// （`on_settings_changed` 遍历 `editor_hosts`）调用；不随 `set_data` 清掉。
    pub fn apply_result_style(&self, cx: &mut Context<Self>) {
        if let Some(settings) = cx.try_global::<settings::model::Settings>() {
            let prefs = settings.result_panel.type_style();
            self.result_panel.update(cx, |p, cx| p.set_type_style(prefs, cx));
        }
    }

    /// 把光标放到指定位移（供测试断言“执行的是光标所在那句”；键位路径仍走真按键）
    pub fn set_caret_for_test(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.set_selected_range(offset..offset, cx));
    }

    /// 设置选区（供测试断言“有选区只格式化选区”；键位路径仍走真按键）
    pub fn set_selection_for_test(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |state, cx| state.set_selected_range(range, cx));
    }

    /// 【T2c-3 收尾】光标数（多光标 = 列编辑；经 vendor 公开的 `cursor_count` 读内核真实值）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cursor_count_for_test(&self, cx: &App) -> usize {
        self.editor.read(cx).cursor_count()
    }

    /// 【T2c-3 收尾】给活动光标加一个下方光标（列编辑多光标；调内核公开的 `add_cursor_below_line`）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn add_cursor_below_for_test(&mut self, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.add_cursor_below_line(cx));
    }

    /// 【T2c-3 收尾】直调内核公开的 `select_columnar_block` 框选一块（列编辑鼠标框选）——
    /// 同 Alt+Shift 拖拽产出的矩形选区，免鼠标捕获，供窗口用例验证框选路径真到了内核。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn select_columnar_block_for_test(
        &mut self,
        start_offset: usize,
        end_offset: usize,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |state, cx| {
            state.select_columnar_block(start_offset, end_offset, cx)
        });
    }

    /// 【T2c-3 收尾】所有光标字节偏移（活动在前；经 vendor 公开的 `cursor_offsets` 读内核真实值）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cursor_offsets_for_test(&self, cx: &App) -> Vec<usize> {
        self.editor.read(cx).cursor_offsets()
    }

    /// 【T2c-3 收尾】当前编辑模式（行 / 列）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn edit_mode_for_test(&self) -> EditMode {
        self.edit_mode
    }

    /// 【T2c-3 收尾】设置编辑模式（走生产代码同一条 `set_edit_mode`）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_edit_mode_for_test(&mut self, mode: EditMode, cx: &mut Context<Self>) {
        self.set_edit_mode(mode, cx);
    }

    /// 【T2c-3 收尾】按当前模式对齐内核光标（走生产代码同一条 `enforce_edit_mode`）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn enforce_edit_mode_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.enforce_edit_mode(window, cx);
    }

}
