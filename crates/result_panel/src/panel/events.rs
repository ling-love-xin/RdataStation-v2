//! 事件域（`panel/` 第三块）：视图层动作（筛选 / 转置 / 侧栏开关 / 重命名 / 列序 / 选区）+ 过滤器 API。


use gpui_kit::*;
use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::table::{TableState, TableDelegate as _};

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use crate::error_card::ErrorCard;
use crate::grid::cell::{classify_value, json_tree_items, ViewerMode};
use crate::grid::{ResultGridDelegate, ResultStatus, ResultToolbar};
use crate::ports::{Presentation, ResultRequest, RightTab};
use crate::script_card::ScriptCard;
use crate::sets::ResultSetTab;
use crate::store::ColumnKind;
/// 【类型样式】全局偏好聚合（shell 层：settings 持久化与渲染消费共用）。
use workbench_shell::result_style::TypeStylePrefs;
use super::data::ResultPanel;
use super::sidebars::next_right_edge_state;

impl ResultPanel {

    pub fn on_filter_input(&mut self, text: String, cx: &mut Context<Self>) {
        if self.filter_text == text {
            return;
        }
        self.filter_text = text;
        self.filter_version = self.filter_version.wrapping_add(1);
        let version = self.filter_version;
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            executor.timer(std::time::Duration::from_millis(300)).await;
            _ = weak.update(cx, |panel, cx| {
                if panel.filter_version == version {
                    panel.apply_filter(cx);
                }
            });
        });
        *self.filter_debounce.borrow_mut() = Some(task);
    }

    /// 【B15】把当前筛选词应用到网格（事件路径：重算一次视图行序）。
    /// 下发源库开着且词非空时，同一份词也**入队**一次（编辑器重查，落新结果集）
    pub fn apply_filter(&mut self, cx: &mut Context<Self>) {
        self.apply_filter_inner(cx, true);
    }

    /// 【B15】应用筛选的公共实现；`requeue=false` 时不触发「下发源库」重查入队
    /// （drain 分派路径用，防队列死循环）
    pub(crate) fn apply_filter_inner(&mut self, cx: &mut Context<Self>, requeue: bool) {
        let filter = self.filter_text.clone();
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_filter(filter);
            state.refresh(cx);
        });
        self.refresh_filter_hint(cx);
        cx.notify();
        if requeue && self.pushdown && !self.filter_text.trim().is_empty() {
            self.queue
                .push(ResultRequest::PushdownFilter { value: self.filter_text.trim().to_string() });
        }
    }

    /// 清除筛选（工具栏那个 ✕）：输入框清空 + 立即应用空词。
    /// 需要 `window`（`InputState::set_value` 的签名）——编辑器分派
    /// [`ResultRequest::ClearFilter`] 时转手调用
    pub fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_text.clear();
        self.filter_version = self.filter_version.wrapping_add(1);
        let input = self.filter_input.clone();
        input.update(cx, |state, cx| state.set_value("", window, cx));
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_filter(String::new());
            state.refresh(cx);
        });
        self.refresh_filter_hint(cx);
        cx.notify();
    }

    /// 【B14】下发源库开关翻转（状态面板内改；开着且有词时重查入队）
    pub fn set_pushdown(&mut self, open: bool) {
        self.pushdown = open;
        if open && !self.filter_text.trim().is_empty() {
            self.queue.push(ResultRequest::PushdownFilter {
                value: self.filter_text.trim().to_string(),
            });
        }
    }

    /// 【P2-4】转置开关：面板状态翻转 + 网格同步（事件路径，`drain` 分派时调用）
    pub fn toggle_transpose(&mut self, cx: &mut Context<Self>) {
        let open = !self.transposed;
        self.transposed = open;
        // 【DBeaver 对齐】打开记录视图时，从当前选中行开始（而不是固定第 0 行）；
        // 没有选中行时保持 transpose_row 不变（默认 0）。
        if open {
            if let Some(status) = &self.status {
                if let Some(selected) = status.selected_row {
                    self.transpose_row = selected;
                }
            }
        }
        let row = self.transpose_row;
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_transposed(open, row);
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【P2-4】转置行导航：上一行 / 下一行（越界夹取；基准 = 当前视图序列长度，
    /// 本地行区段也纳入导航范围——#28-B1 拍板 D2 转置纳入本地行）
    pub fn transpose_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let max = self
            .grid
            .read(cx)
            .delegate()
            .view_len()
            .saturating_sub(1);
        let next = (self.transpose_row as isize + delta).clamp(0, max as isize) as usize;
        self.transpose_row = next;
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_transpose_row(next);
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【并排对比】对比模式开关（编辑器 `ToggleCompareMode` 分派回来调；翻转后
    /// 对比栏投影由编辑器 `sync_compare_grid` 续写——数据源在 ResultStore）
    pub fn toggle_compare_mode(&mut self, cx: &mut Context<Self>) {
        self.compare_mode = !self.compare_mode;
        cx.notify();
    }

    /// 【呈现】显示方式切换（网格 ↔ 文本）。面板级持续——切结果集不重置
    /// （用户选的是「怎么看结果」，不是某份结果的属性）。
    pub fn set_presentation(&mut self, presentation: Presentation, cx: &mut Context<Self>) {
        self.presentation = presentation;
        cx.notify();
    }

    /// 【右栏】标签页切换（列浏览器 / 值详情 / 计算 / 分组 / 引用）。
    /// 右栏收起时切页签会自动展开（DBeaver 口径：右缘竖条点图标即展开对应面板）。
    pub fn set_right_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.right_tab = tab;
        if !self.show_value_sidebar {
            // 窄展开 / 收起态点页签图标 → 宽展开（竖条可见性保持；收起态在窄态已置 true）
            self.edge_bar_visible = true;
            self.show_value_sidebar = true;
            self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
        }
        cx.notify();
    }

    /// 【1.2】筛选框展开 / 收起（点击筛选组展开按钮 / 展开条收起按钮触发）
    pub fn toggle_filter_expand(&mut self, cx: &mut Context<Self>) {
        self.filter_expanded = !self.filter_expanded;
        cx.notify();
    }

    /// 【2026-10-28 微调】SQL 预览**弹出窗口**：置 open 标志 + 懒建只读 Textarea 实体
    /// （`TextareaState::new` 需要 window，走端口事件路径调用——面板 render 只有
    /// &App 建不了）。SQL 变化时 `set_value` 刷新；render 期 `Textarea::new(&entity)`
    /// 直接挂浮层（只需 &Entity）。
    pub fn open_sql_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sql = self
            .exec_sql
            .clone()
            .or_else(|| self.result_sql.clone())
            .unwrap_or_default();
        if self.sql_textarea.is_none() {
            let textarea = cx.new(|cx| TextareaState::new(window, cx).default_value(sql));
            self.sql_textarea = Some(textarea);
        } else if let Some(ta) = self.sql_textarea.clone() {
            ta.update(cx, |state, cx| state.set_value(sql.clone(), window, cx));
        }
        self.sql_dialog_open = true;
        cx.notify();
    }

    /// 【2026-10-28 微调】关闭 SQL 预览弹窗（✕ 按钮 / Esc 触发）
    pub fn close_sql_dialog(&mut self, cx: &mut Context<Self>) {
        self.sql_dialog_open = false;
        cx.notify();
    }

    /// 【1.2】筛选框运行后宿主写入 wrapped SQL（执行完成后），并切到「执行SQL」页签    /// （右栏收起时自动展开——用户要在右侧栏看到执行 SQL）
    pub fn set_exec_sql(&mut self, sql: Option<String>, cx: &mut Context<Self>) {
        self.exec_sql = sql;
        self.right_tab = RightTab::ExecSql;
        if !self.show_value_sidebar {
            self.edge_bar_visible = true;
            self.show_value_sidebar = true;
            self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
        }
        cx.notify();
    }

    /// 【侧栏】右侧开关：**三态循环**（比左侧多一个展开态；开关在状态栏右端）——
    /// 收起（竖条 + 右栏都不显示）→ 窄展开（只显右缘竖条图标条）→ 宽展开（竖条 + 右栏
    /// 内容）→ 收起。epoch 递增让宽度动画重播（首帧 epoch=0 免动画，见 `sidebar_width_anim`）
    pub fn toggle_value_sidebar(&mut self, cx: &mut Context<Self>) {
        let (edge, show) = next_right_edge_state(self.edge_bar_visible, self.show_value_sidebar);
        self.edge_bar_visible = edge;
        self.show_value_sidebar = show;
        self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
        cx.notify();
    }

    /// 【侧栏】左侧栏（网格 / 文本 / 记录竖条）开关：只翻面板状态（网格数据不动）；
    /// epoch 递增同上
    pub fn toggle_left_sidebar(&mut self, cx: &mut Context<Self>) {
        self.show_left_sidebar = !self.show_left_sidebar;
        self.left_sidebar_epoch = self.left_sidebar_epoch.wrapping_add(1);
        cx.notify();
    }

    /// 【C1】进入第 `index` 份结果集 tab 的重命名态（预填当前 label；会话级）。
    ///
    /// 输入框实体懒创建：第一次进时建，后续复用。提交走 [`Self::commit_rename`]，
    /// 取消走 [`Self::cancel_rename`]（都由面板渲染的 ✓/✕ 按钮触发）。
    pub fn begin_rename(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        // 预填当前 label（自定义标题或自动序号都显示出来供改）
        let prefill = self.tabs[index].label.clone();
        if self.renaming_input.is_none() {
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx);
                state.set_placeholder("结果集标题（留空恢复序号）…", window, cx);
                state
            });
            self.renaming_input = Some(input);
        }
        if let Some(input) = self.renaming_input.clone() {
            input.update(cx, |state, cx| state.set_value(prefill, window, cx));
        }
        self.renaming = Some(index);
        cx.notify();
    }

    /// 【C1】提交重命名：把输入框当前值发回宿主（空串 = 恢复自动标签）。
    pub fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.renaming else {
            return;
        };
        let value = self
            .renaming_input
            .as_ref()
            .map(|input| input.read(cx).value().to_string())
            .unwrap_or_default();
        // 空串 / 纯空白 → None（恢复自动标签）
        let normalized = if value.trim().is_empty() {
            None
        } else {
            Some(value.trim().to_string())
        };
        self.queue.push(ResultRequest::RenameSet(index, normalized));
        self.renaming = None;
        cx.notify();
    }

    /// 【C1】取消重命名（不发请求，输入框内容保留下次再进）。
    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        cx.notify();
    }

    /// 【B1】切换列头直接显示注释（面板级临时开关；host 也可从全局偏好同步过来）。
    pub fn set_show_column_comments(&mut self, on: bool, cx: &mut Context<Self>) {
        self.show_column_comments = on;
        cx.notify();
    }

    /// 【B1】当前是否直接显示注释（delegate 读这个决定列头画不画第二行注释）。
    pub fn show_column_comments(&self) -> bool {
        self.show_column_comments
    }

    /// 【类型样式】同步全局偏好（数字格式 / NULL 占位 / 布尔映射 / 按族对齐与前景色）。
    ///
    /// host 构造时（`settings.result_panel.type_style()`）与 workbench 设置变更钩子
    /// （设置页改动 → 遍历 editor_hosts → 本方法）都会调用；面板再把偏好推给 delegate，
    /// 由 delegate 的 `render_td` / `render_th` 消费——不经过 `set_data`，不会丢。
    /// 【结果集 #296】宿主注入持久化的列序（重启恢复；空 = 默认序）。
    /// 只在文档重开时调用一次，换结果集不清空（列名匹配由 `apply_column_order` 做）。
    pub fn set_column_order(&mut self, order: Vec<String>) {
        self.column_order = order;
    }

    /// 【结果集 #296】当前持久化目标列序（宿主保存会话时读取）
    pub fn column_order(&self) -> Vec<String> {
        self.column_order.clone()
    }

    /// 【结果集 #296】按持久化列名序重排一份新结果的列与行（列名匹配，未匹配列
    /// 保持数据序附加在末尾——用户拖过的列还在就保持，新列不猜）。
    pub(crate) fn apply_column_order(&self, columns: Vec<String>, rows: Vec<Vec<String>>) -> (Vec<String>, Vec<Vec<String>>) {
        reorder_by(&self.column_order, columns, rows)
    }

    pub fn set_type_style(&mut self, prefs: TypeStylePrefs, cx: &mut Context<Self>) {        self.type_style = prefs.clone();
        self.grid.update(cx, |state, cx| {
            state.delegate_mut().set_type_style(prefs);
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【侧栏】最近选中格位置（`TableEvent::SelectCell` 事件路径写入；选行 / 清选 =
    /// `None`）。值详情只跟随**格**选中——选整行时不投影单格。
    ///
    /// 同时把选中格反推成**数据列序**写入 [`Self::selected_col`]，驱动右栏列浏览器
    /// 清单高亮与底部明细；选行 / 清选 / 转置视图 / 行号槽列时不反推（保持清单当前选中）。
    pub fn set_selection_pos(
        &mut self,
        pos: Option<(usize, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selection_pos = pos;
        if let Some((_row, col)) = pos {
            if col > 0 {
                let delegate = self.grid.read(cx).delegate();
                if !delegate.is_transposed() {
                    let data_col = delegate.data_column(col - 1);
                    self.selected_col = Some(data_col);
                    // 同步清单高亮：把面板权威的选中列换算成 List 过滤后行号，
                    // 驱动 kit List 内部 selected_index（过滤掉该列时无对应行 → 不清高亮）
                    let browser = self.browser_list.clone();
                    browser.update(cx, |list, cx| {
                        let ix = list
                            .delegate()
                            .matched()
                            .iter()
                            .position(|&c| c == data_col)
                            .map(IndexPath::new);
                        list.set_selected_index(ix, window, cx);
                    });
                }
            }
        }
        cx.notify();
    }

    /// 【值查看器】选中格变化时重建 JSON 展开树（事件路径：解析 + `set_items`；
    /// render 只摆 Tree，不解析 JSON——render 纯读红线）。
    /// 非 JSON 值 → 清空树；清选 → 空。
    pub fn rebuild_value_tree(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let items = match self.selection_pos {
            Some((row, col)) => {
                let grid = self.grid.read(cx);
                let delegate = grid.delegate();
                let value = delegate.cell_text(row, col, cx);
                let kind = if col == 0 {
                    ColumnKind::Unknown
                } else {
                    delegate.column_kind(col)
                };
                match classify_value(kind, &value) {
                    ViewerMode::Json => json_tree_items(&value).unwrap_or_default(),
                    _ => Vec::new(),
                }
            }
            None => Vec::new(),
        };
        self.json_tree.update(cx, |state, cx| state.set_items(items, cx));
    }

    /// 【列浏览器】清单单击选中一列（数据列序）：写清单选中态 + 网格列选中高亮联动。
    /// 清单选中 ≠ 网格焦点——这里只把网格列选中用于高亮，不回写 `selection_pos`
    /// （避免清单选中把值详情也拽走；值详情仍只跟网格格焦点）。
    pub fn select_column(&mut self, data_col: usize, cx: &mut Context<Self>) {
        self.selected_col = Some(data_col);
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            if let Some(view_col) = state.delegate().view_column(data_col) {
                state.set_selected_col(view_col, cx);
            }
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【列浏览器】清单双击定位：网格滚到该列首行并选中（列不可见 = 隐藏列，不动作）。
    pub fn locate_column(&mut self, data_col: usize, cx: &mut Context<Self>) {
        self.selected_col = Some(data_col);
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            if let Some(view_col) = state.delegate().view_column(data_col) {
                state.set_selected_cell(0, view_col, cx);
            }
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【列浏览器】清空搜索词（搜索框 ✕ / ESC 入口；`window` 转手给 List 内置
    /// 搜索框，与 `clear_filter` 同口径）
    pub fn clear_column_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let browser = self.browser_list.clone();
        browser.update(cx, |list, cx| list.set_query("", window, cx));
        cx.notify();
    }

    /// 计算 / 分组页当前作用列（**数据列序**）：网格选中格优先（`selection_pos`），
    /// 列浏览器清单选中列（`selected_col`）兜底；转置视图 / 行号槽列不给语义列，
    /// 兜底列被隐藏时不返回。
    pub(crate) fn inspect_column(&self, delegate: &ResultGridDelegate) -> Option<usize> {
        if self.transposed {
            return None;
        }
        if let Some((_, col)) = self.selection_pos {
            if col > 0 {
                let view_col = col - 1;
                if view_col < delegate.columns().len() {
                    return Some(delegate.data_column(view_col));
                }
            }
        }
        self.selected_col
            .filter(|c| *c < delegate.columns().len() && !delegate.is_hidden_column(*c))
    }


}



impl ResultPanel {

    pub fn apply_column_filter(&mut self, column: usize, value: &str, cx: &mut Context<Self>) {
        self.grid.update(cx, |grid, _| {
            grid.delegate_mut().set_column_filter(if value.is_empty() {
                None
            } else {
                Some((column, value.to_string()))
            });
        });
        self.refresh_filter_hint(cx);
        cx.notify();
    }

    /// 右键「按值筛选」核心：定稿筛选词并立即应用（不依赖窗口——headless 下窗口
    /// 不可用时真值照样生效；输入框回填由编辑器在窗口可用时转手调 [`Self::sync_filter_input`]）
    pub fn set_filter_value(&mut self, value: &str, cx: &mut Context<Self>) {
        self.filter_text = value.to_string();
        self.filter_version = self.filter_version.wrapping_add(1);
        self.apply_filter(cx);
    }

    /// 【B15】定稿筛选词但**不**触发「下发源库」重查入队——`drain_result_actions`
    /// 分派 [`ResultRequest::PushdownFilter`] 时用它：这条请求本身就是下发，再经
    /// `apply_filter` 入队一次会跟 drain 的 while 循环互相喂，队列无限增长撑爆内存。
    pub fn set_filter_value_dispatch(&mut self, value: &str, cx: &mut Context<Self>) {
        self.filter_text = value.to_string();
        self.filter_version = self.filter_version.wrapping_add(1);
        self.apply_filter_inner(cx, false);
    }

    /// 筛选输入框回填（所见即所得；窗口不可用跳过——真值已在 [`Self::set_filter_value`] 定稿）
    pub fn sync_filter_input(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.filter_input.clone();
        let text = value.to_string();
        input.update(cx, |state, cx| state.set_value(text, window, cx));
    }

    /// 【B15】把「已筛选 N / M 行」写进结果工具栏（统计跟随筛选——原型 §5.5；
    /// 全局筛选词与列级筛选任一生效都算“已筛选”）
    pub(crate) fn refresh_filter_hint(&mut self, cx: &mut Context<Self>) {
        let (visible, total, filtering) = {
            let delegate = self.grid.read(cx).delegate();
            (
                delegate.visible_row_count(),
                delegate.data_row_count(),
                !delegate.filter().trim().is_empty() || delegate.column_filter().is_some(),
            )
        };
        if let Some(toolbar) = self.toolbar.as_mut() {
            toolbar.filtered = filtering.then_some((visible, total));
        }
    }

    // ── 只读访问器（测试断言；编辑器接线也会用到） ────────────────────────────

    /// 当前是否有任何结果区内容（编辑器据此决定结果 dock 是否该开）
    pub fn has_content(&self) -> bool {
        self.toolbar.is_some()
            || self.script_card.is_some()
            || self.error_card.is_some()
    }

    /// 对比模式是否开启
    pub fn compare_mode(&self) -> bool {
        self.compare_mode
    }

    /// 当前选中结果集下标
    pub fn active(&self) -> usize {
        self.active
    }

    /// 工具栏投影（`None` = 还没有结论）
    pub fn toolbar(&self) -> Option<&ResultToolbar> {
        self.toolbar.as_ref()
    }

    /// 状态行投影（只有网格时才给）
    pub fn status(&self) -> Option<&ResultStatus> {
        self.status.as_ref()
    }

    /// 当前呈现方式（网格 / 文本）——供测试断言左竖条按钮点击链路
    pub fn presentation(&self) -> Presentation {
        self.presentation
    }

    /// 当前选中结果集的 SQL（「刷新」重跑它；`None` = 无可重跑）
    pub fn sql(&self) -> Option<&str> {
        self.result_sql.as_deref()
    }

    /// 【1.2】筛选框运行生成的 wrapped SQL（右栏「执行SQL」页签；`None` = 未运行过）
    pub fn exec_sql(&self) -> Option<&str> {
        self.exec_sql.as_deref()
    }

    /// 有没有可复制的东西（没网格不摆复制 / 转置 / 导出）
    pub fn can_copy(&self) -> bool {
        self.result_can_copy
    }

    /// 顶部提示行（切通道后「旧结果来自 X」）
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// 行选中 → 状态行「已选第 N 行」增量更新（表格 `TableEvent::SelectRow` 事件路径调用；
    /// sync 会全量覆写 status，这里只补事件之间的增量）
    pub fn set_selected_row(&mut self, row: Option<usize>, cx: &mut Context<Self>) {
        tracing::info!(row = ?row, "【探针】set_selected_row：结果网格行选中变化");
        if let Some(status) = self.status.as_mut() {
            status.selected_row = row;
        }
        cx.notify();
    }

    /// 格选中 → 状态行「当前格值」段增量更新（`TableEvent::SelectCell` 事件路径调用；
    /// 值由编辑器在事件里读网格 delegate 算好传入，面板不碰网格）
    pub fn set_selected_cell(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        if let Some(status) = self.status.as_mut() {
            status.selected_cell = value;
        }
        cx.notify();
    }

    /// 顶部提示行增量写入（编辑被拦等一次性提示；sync 会全量覆写 notice）
    pub fn set_notice(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        self.notice = text;
        cx.notify();
    }

    /// 结果集标签（测试断言标签条内容）
    pub fn tabs(&self) -> &[ResultSetTab] {
        &self.tabs
    }

    /// 错误卡片（`None` = 这次成功）
    pub fn error_card(&self) -> Option<&ErrorCard> {
        self.error_card.as_ref()
    }

    /// 脚本输出卡片
    pub fn script_card(&self) -> Option<&ScriptCard> {
        self.script_card.as_ref()
    }

    /// 主网格实体（编辑器 / 测试读选中行、数据行数等）
    pub fn grid(&self) -> &Entity<TableState<ResultGridDelegate>> {
        &self.grid
    }

    /// 对比栏网格实体（`None` = 没建）
    pub fn compare_grid(&self) -> Option<&Entity<TableState<ResultGridDelegate>>> {
        self.compare_grid.as_ref()
    }

    /// 筛选词（测试断言）
    pub fn filter_text(&self) -> &str {
        &self.filter_text
    }

    /// 下发源库开关（测试断言）
    pub fn pushdown(&self) -> bool {
        self.pushdown
    }

    /// 列浏览器清单当前选中列（数据列序；`None` = 未选中，渲染兜底第一可见列）
    pub fn selected_col(&self) -> Option<usize> {
        self.selected_col
    }

}



/// 【结果集 #296】按持久化列名序重排一份新结果的列与行（纯函数，测试直接覆盖）：
/// 列名匹配（保留持久化序），未匹配列按数据序附加末尾——用户拖过的列还在就保持，
/// 新列不猜。空 `order` = 默认序，原样直通。
pub(crate) fn reorder_by(
    order: &[String],
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
) -> (Vec<String>, Vec<Vec<String>>) {
    if order.is_empty() {
        return (columns, rows);
    }
    // 先按持久化序收集匹配列（重复列名只取第一个出现；去重防双写）
    let mut ordered: Vec<String> = Vec::new();
    for column in order {
        if columns.contains(column) && !ordered.contains(column) {
            ordered.push(column.clone());
        }
    }
    // 未匹配列按数据序附加末尾（新列不猜位置）
    let leftover: Vec<String> = columns
        .iter()
        .filter(|c| !order.contains(*c))
        .cloned()
        .collect();
    ordered.extend(leftover);
    // 行按同序重排（行宽与列数不符时原样保留——不猜形状）
    let row_len = columns.len();
    let mut perm: Vec<usize> = Vec::with_capacity(row_len);
    for column in &ordered {
        if let Some(at) = columns.iter().position(|c| c == column) {
            perm.push(at);
        }
    }
    let rows = rows
        .into_iter()
        .map(|row| {
            if row.len() != row_len {
                return row;
            }
            perm.iter().map(|at| row[*at].clone()).collect()
        })
        .collect();
    (ordered, rows)
}