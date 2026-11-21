//! 事件域（`panel/` 第三块）：视图层动作（筛选 / 转置 / 侧栏开关 / 重命名 / 列序 / 选区）+ 过滤器 API。

use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::table::{TableDelegate as _, TableEvent, TableState};
use gpui_kit::Focusable as _;
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use super::data::{ResultPanel, SavedFilterSetting, FILTER_HISTORY_LIMIT};
use super::sidebars::next_right_edge_state;
use crate::error_card::ErrorCard;
use crate::grid::cell::{
    ViewerMode, classify_value, hex_to_bytes, image_format_of, json_tree_items,
};
use crate::grid::{ResultGridDelegate, ResultStatus, ResultToolbar, RowTargetState};
use crate::ports::{
    FindOption, GroupSort, Presentation, ResultRequest, RightTab, StatsItem, StatsMode,
};
use crate::script_card::ScriptCard;
use crate::sets::ResultSetTab;
use crate::store::ColumnKind;
/// 【类型样式】全局偏好聚合（shell 层：settings 持久化与渲染消费共用）。
use workbench_shell::result_style::TypeStylePrefs;

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

    /// 【筛选历史】把当前词记进历史（新的在前、去重、封顶 [`FILTER_HISTORY_LIMIT`]）。
    ///
    /// 只在**显式提交**时调（Enter / ▶ 执行 / 从历史里选一条再用）：DBeaver 的历史是
    /// 用户「确认过的筛选」（`ResultSetFilterPanel.java:296` 的 Enter 与执行钮）；
    /// 打字过程中的防抖状态入了历史只会变成一堆前缀。
    pub fn note_filter_commit(&mut self) {
        let text = self.filter_text.trim().to_string();
        if text.is_empty() {
            return;
        }
        // 已经在最前一条 ⇒ 不重复记（重复提交同一个词很常见：改了下再 Enter 回来）
        if self.filter_history.first().map(|s| s.as_str()) == Some(text.as_str()) {
            return;
        }
        self.filter_history.retain(|s| s != &text);
        self.filter_history.insert(0, text);
        self.filter_history.truncate(FILTER_HISTORY_LIMIT);
    }

    /// 【筛选历史】Enter 提交：立即应用（不等防抖），并记进历史。
    ///
    /// 对齐 DBeaver：Enter 触发 `setCustomDataFilter()`（`ResultSetFilterPanel.java:296`）。
    /// 展开态（多行框）里 Enter 是换行、**不提交**（同 `:288-290` 判 `filterExpanded`）。
    pub fn commit_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.filter_expanded {
            return;
        }
        let input = self.filter_input.clone();
        let text = input.read(cx).value().to_string();
        if self.filter_text == text {
            // 词没变但用户按了 Enter：仍算一次显式确认，历史照记、不重复刷网格
            self.filter_version = self.filter_version.wrapping_add(1);
            self.note_filter_commit();
            cx.notify();
            return;
        }
        self.filter_text = text;
        // 在途防抖作废（版本号一变，到点的旧任务自己跳过）
        self.filter_version = self.filter_version.wrapping_add(1);
        self.note_filter_commit();
        self.apply_filter(cx);
        let _ = window;
    }

    /// 【筛选历史】从下拉里选一条：回填输入框 + 立即应用（不需要窗口也能定稿，
    /// 输入框回填在有窗口时一并做）。
    pub fn apply_filter_from_history(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.filter_text = value.to_string();
        self.filter_version = self.filter_version.wrapping_add(1);
        let input = self.filter_input.clone();
        let text = value.to_string();
        input.update(cx, |state, cx| state.set_value(text, window, cx));
        // 选历史等于「再用一次这个口径」：把它提到最前
        self.note_filter_commit();
        self.apply_filter(cx);
    }

    /// 【筛选历史】只读快照（渲染下拉用）
    pub fn filter_history(&self) -> &[String] {
        &self.filter_history
    }

    // ── 【结果内查找】查找栏（对齐 DBeaver 的 `FindReplaceOverlay` / `SpreadsheetFindReplaceTarget`）

    /// 打开 / 关闭查找栏
    ///
    /// 对齐 DBeaver：筛选条工具条上的 `openFindReplaceButton` 是 CHECK 型
    /// （`ResultSetFilterPanel.java:356-385`）：选中打开浮层并进搜索框、取消关闭。
    /// 关闭时**保留**查找词（DBeaver 浮层 close 也不清词），下次打开接着用。
    pub fn toggle_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = !self.find_open;
        self.find_open = open;
        if open {
            let input = self.find_input.clone();
            input.update(cx, |state, cx| state.focus(window, cx));
        } else {
            // 关查找栏 = 撤掉「只显示匹配行」（DBeaver：`ResultsetFindReplaceOverlay.onClose`
            // 在 `findAllActionApplied` 时 `applyFilterAction(null)`，浮层关了过滤不残留）。
            self.set_find_filter_only(false, cx);
        }
        cx.notify();
    }

    /// 【结果内查找·命令语义】打开查找栏并聚焦（**幂等**，已开不关）
    ///
    /// 工具栏那颗是 CHECK 型开关（[`Self::toggle_find`]）；右键 filters 段首的
    /// 「在结果中查找…」对齐 DBeaver 的 `EDIT_FIND_AND_REPLACE` 命令
    /// （`ResultSetViewer.fillFiltersMenu:3869`）——命令只负责「把查找栏叫出来」，
    /// 已打开时应聚焦而不是反关。查找词与口径一概不动（与 `toggle_find` 同口径）。
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.find_open {
            self.find_open = true;
            cx.notify();
        }
        let input = self.find_input.clone();
        input.update(cx, |state, cx| state.focus(window, cx));
    }

    /// 「只显示匹配行」开关（查找栏上的勾选框）
    ///
    /// 对齐 DBeaver 的 `selectAllButton`（`SWT.CHECK`，`ResultsetFindReplaceOverlay:107-135`）：
    /// 勾上 → 把当前查找词作为 quickFilter 灌进视图；取消 → 撤掉。
    /// 与 DBeaver 的差异登记：它是「边敲边过滤」（250ms 去抖的 `scheduleFilter`），
    /// 本仓是「勾选框 + 词变时同步」（没有独立 job，靠 `find_query_changed` 驱动）。
    pub fn set_find_filter_only(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.find_filter_only == on {
            return;
        }
        self.find_filter_only = on;
        let needle = if on {
            Some(self.find_query.clone())
        } else {
            None
        };
        self.push_find_filter(needle, cx);
    }

    /// 三颗口径开关之一（大小写 / 正则 / 全词）
    ///
    /// 对齐 DBeaver `FindReplaceOverlay`：`createCaseSensitiveButton:740` /
    /// `createRegexSearchButton:726` / `createWholeWordsButton:751` 都是 `SWT.CHECK`，
    /// 点击后 `updateIncrementalSearch()` + `decorate()`——即**重算命中与高亮**，
    /// 行过滤开着时也按新口径重筛（同一个 `SearchQuickFilterInfo`）。
    pub fn set_find_option(&mut self, option: FindOption, on: bool, cx: &mut Context<Self>) {
        if option.get(self.find_criteria) == on {
            return;
        }
        option.set(&mut self.find_criteria, on);
        // 对齐 DBeaver `FindReplaceOverlay.createRegexSearchButton` → 勾正则后
        // `wholeWordSearchButton.setEnabled(info.isAvailable(WHOLE_WORD))`：正则与全词
        // 互斥，勾正则时把全词一并取消（否则 `\b` 会包进正则里又按正则解释，语义打架）
        if option == FindOption::Regex && on {
            self.find_criteria.whole_word = false;
        }
        // 口径变了：旧的光标可能已经不是命中了，清掉重新定位
        self.find_at = None;
        if self.find_filter_only {
            // 过滤开着 → 按新口径重推（内部会重算命中数并 notify）
            self.push_find_filter(Some(self.find_query.clone()), cx);
        } else {
            self.find_hits = self.count_find_hits(cx);
            cx.notify();
        }
        if self.find_hits > 0 {
            self.find_step(true, cx);
        }
    }

    /// 按当前词 + 当前口径重算命中数（面板与「N 个匹配」的唯一算法）
    fn count_find_hits(&self, cx: &App) -> usize {
        let delegate = self.grid.read(cx).delegate();
        delegate.count_find_matches(&self.find_query, self.find_criteria)
    }

    /// 把查找过滤词写进 delegate（`None` = 撤掉），并让视图重排
    fn push_find_filter(&mut self, needle: Option<String>, cx: &mut Context<Self>) {
        let criteria = self.find_criteria;
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_find_filter(needle, criteria);
            state.refresh(cx);
        });
        // 行集合变了：旧的光标（视图行, 视图列）可能已经不在视图里，清掉重新定位
        self.find_at = None;
        self.find_hits = self.count_find_hits(cx);
        cx.notify();
    }

    /// 查找词变化（订阅里真敲键时进）：重算命中数 + 定位到第一处命中
    pub fn find_query_changed(&mut self, text: String, cx: &mut Context<Self>) {
        self.find_query = text;
        self.find_at = None;
        // 「只显示匹配行」开着时词变要跟着过滤（DBeaver 的边敲边过滤；本仓没有
        // 独立 job，直接跟随每次输入，行集合与词始终一致）
        if self.find_filter_only {
            let criteria = self.find_criteria;
            let needle = Some(self.find_query.clone());
            let grid = self.grid.clone();
            grid.update(cx, |state, cx| {
                state.delegate_mut().set_find_filter(needle, criteria);
                state.refresh(cx);
            });
        }
        self.find_hits = self.count_find_hits(cx);
        if self.find_hits > 0 {
            // 换词后从头找：先把光标当成「文档开头之前」再走一步
            self.find_step(true, cx);
            return;
        }
        cx.notify();
    }

    /// 上一处 / 下一处命中（`forward = false` 是反向），找完一圈回卷
    ///
    /// DBeaver 的查找浮层有 Prev / Next 两个按钮（`FindReplaceOverlay`），Enter = Next；
    /// 本仓的定位 = 选中该格 + 滚到它（`set_selected_cell` + `scroll_to_row/col`）。
    pub fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let delegate = self.grid.read(cx).delegate();
        let query = self.find_query.clone();
        if query.trim().is_empty() {
            return;
        }
        let after = if forward {
            // 正向：从当前命中之后继续；没有当前命中就从开头
            self.find_at
        } else {
            // 反向：从当前命中之前继续；没有当前命中就从末尾
            self.find_at
        };
        let hit = if forward {
            delegate.find_next(&query, self.find_criteria, after)
        } else {
            delegate.find_prev(&query, self.find_criteria, after)
        };
        let Some(hit) = hit else {
            cx.notify();
            return;
        };
        self.find_at = Some((hit.row, hit.col));
        let (row, col) = (hit.row, hit.col);
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.set_selected_cell(row, col, cx);
            state.scroll_to_row(row, cx);
            state.scroll_to_col(col, cx);
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 关闭查找栏（查找栏右端的 ✕，也供工具栏查找钮 / Esc 走）
    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        if !self.find_open {
            return;
        }
        self.find_open = false;
        // 同 DBeaver 的 `onClose`：关浮层就撤掉曾应用的过滤（词本身保留）
        self.set_find_filter_only(false, cx);
        cx.notify();
    }

    /// 【结果内查找】只读快照（渲染「N 个匹配」与当前命中序号）
    pub fn find_state(&self) -> (bool, &str, usize) {
        (self.find_open, self.find_query.as_str(), self.find_hits)
    }

    /// 【状态栏·修改单元格】把「修改」钮转成**双击同一事件**：
    /// `TableEvent::DoubleClickedCell(组件行, 组件列)`。
    ///
    /// 语义：DBeaver 状态条的 `row.edit`（`CMD_ROW_EDIT`）打开外部值编辑器
    /// （`ResultSetHandlerMain.java:207-211` 的 `openValueEditor(false)`），本仓没有
    /// 「值编辑器 + 写回」通道，改用同族 `CMD_ROW_EDIT_INLINE`（`openValueEditor(true)`）
    /// 的就地编辑链路——事件由 editor 宿主订阅后进入行内编辑，门控（通道 / 项目 /
    /// 键列 / 只读）在那边再判一次（`host/construct.rs:325-339`）。
    ///
    /// 组件列 0 是行号槽（数据格从 1 起，见 `construct.rs:337` 的 `col.saturating_sub(1)`），
    /// 停在行号槽时什么都不做；没有选中格（整行模式 / 未选中）也不做。
    pub fn begin_edit_cell(&mut self, cx: &mut Context<Self>) {
        let Some((row, col)) = self.grid.read(cx).selected_cell() else {
            return;
        };
        self.begin_edit_cell_at(row, col, cx);
    }

    /// 【第 61 批】按**指定格**进入行内编辑（右键「修改单元格」）。
    ///
    /// 与 [`Self::begin_edit_cell`] 只差定位来源：那条读网格选中格，这条用右键落点。
    /// 右键 target 与选中格可以不同（右键不会改选区），所以不能复用选中格那条。
    ///
    /// `col` 是**组件列**（含行号槽，数据格从 1 起，见 `host/construct.rs:337`）；
    /// 行号槽与非数据位置直接忽略。其余门控（通道 / 项目 / 键列 / 只读）由
    /// `DoubleClickedCell` 的宿主订阅再判一次，与双击完全同路。
    pub fn begin_edit_cell_at(&mut self, row: usize, col: usize, cx: &mut Context<Self>) {
        // 【第 62 批】行号槽（组件列 0）与记录（整行 / 转置）视图都不发——
        // 对应 [`crate::grid::can_edit_cell`] 的 `has_current_cell` / `record_view` 两条；
        // 那边 `editable` 这条**有意不在面板层判**：只读结果集要让宿主的 `cell_editable`
        // 报 `Err(EditGate)` 在顶部说明原因（`host/construct.rs:340-354`），提前拦会吞掉提示。
        if col == 0 || self.transposed {
            return;
        }
        self.grid
            .update(cx, |_state, cx| cx.emit(TableEvent::DoubleClickedCell(row, col)));
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
            self.queue.push(ResultRequest::PushdownFilter {
                value: self.filter_text.trim().to_string(),
            });
        }
    }

    /// 清除筛选（工具栏那个 ✕ 的**窄口径**入口：只清筛选词）：输入框清空 + 立即应用空词。
    /// 需要 `window`（`InputState::set_value` 的签名）——编辑器分派
    /// [`ResultRequest::ClearFilter`] 时转手调用。
    ///
    /// 【2026-11-20】这颗钮的主点击已改为全清（[`Self::clear_filter_and_ordering`]），
    /// 与 DBeaver 一致；本方法保留给「只想清词」的路径（查找栏 / 测试直调）。
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

    /// 【筛选条 ✕】「删除所有过滤 / 排序」：清筛选词 + 自定义排序 + 列级筛选 + 本地排序。
    ///
    /// 对齐 DBeaver `CMD_FILTER_CLEAR_SETTING` → `ResultSetViewer.clearDataFilter(true)`
    /// （`:605-626`）：造一份新 filter 再 **把每列的视觉状态（`visible` / `options`）
    /// 搬回去**（`:609-623`，注释原文「keeping all visual state (visibility, etc.)」）。
    /// 由此本仓照抄两处边界：
    /// - **隐藏列不会被放出来**（DBeaver 显式保留可见性）；`column_hidden` / `column_frozen`
    ///   一概不动——想放隐藏列走同一份菜单里的「显示全部列」；
    /// - 清的是 **filter 与排序**（where / order / 每列 criteria / 每列 orderPosition），
    ///   对应本仓的 `filter_text` / `filter_order` / `delegate.column_filter` / `sort`。
    ///
    /// 这是 DBeaver 清除钮的**主点击**（`EraseItemListener.executeDefaultAction:1549-1551`），
    /// 也是它 tooltip 的说明（`ResultSetFilterPanel.java:319-325`）。需要 `&mut Window`
    /// 回填输入框（同 [`Self::clear_filter`] 的宿主转手口径）。
    pub fn clear_filter_and_ordering(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_text.clear();
        self.filter_version = self.filter_version.wrapping_add(1);
        let input = self.filter_input.clone();
        input.update(cx, |state, cx| state.set_value("", window, cx));
        // 自定义排序一起清（连同浮层里的草稿，否则重新打开浮层又看到旧排序）
        self.filter_order.clear();
        let order_input = self.filter_order_input.clone();
        order_input.update(cx, |state, cx| state.set_value("", window, cx));
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_filter(String::new());
            state.delegate_mut().set_column_filter(None);
            state.delegate_mut().clear_sort();
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
        let max = self.grid.read(cx).delegate().view_len().saturating_sub(1);
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
        // 【布局段】网格侧镜像同步：右键菜单给「网格 / 文本」标勾选态要读它
        self.grid.update(cx, |state, _cx| {
            state.delegate_mut().set_presentation_view(presentation);
        });
        cx.notify();
    }

    /// 【布局段】切到下一个呈现方式（对齐 DBeaver `ResultSetViewer.switchPresentation():1161-1172`
    /// 的循环语义）：本仓两档，网格 ⇄ 文本。
    ///
    /// 记录（转置）视图不参与循环——DBeaver 的记录模式是 spreadsheet presentation 内部的
    /// 开关，不在 `availablePresentations` 列表里。
    pub fn cycle_presentation(&mut self, cx: &mut Context<Self>) {
        self.set_presentation(self.presentation.next(), cx);
    }

    /// 【第 60 批】切换呈现后的**焦点目标**：文本态 → 面板根句柄；网格态 → 网格自身句柄。
    ///
    /// 文本态下网格整块不渲染，原焦点元素消失后 gpui 的 dispatch 退化到窗口根，
    /// 面板的 `RESULT_PANEL_CONTEXT` 不再在焦点路径上（`Ctrl+反引号` 就切不回来）；
    /// 网格态则要保证 `ENTER` 落在 `RESULT_GRID_CONTEXT` 上。宿主在切完后用
    /// `window.focus(&panel.read(cx).presentation_focus_target(cx), cx)` 移交
    /// （面板层拿不到 window：`render` 是 `&App`、事件方法用 `Context<Self>`）。
    pub fn presentation_focus_target(&self, cx: &App) -> FocusHandle {
        match self.presentation {
            Presentation::Text => self.focus_handle.clone(),
            Presentation::Grid => self.grid.read(cx).focus_handle(cx),
        }
    }

    /// 【第 70 批】`CTRL+ALT+N` 的取数入口：取下一段
    /// （DBeaver `CMD_FETCH_PAGE` → `readNextSegment()`，`plugin.xml:1069`）。
    ///
    /// 与状态栏「获取数据」钮**完全同一条路**：只把 [`crate::ports::ResultRequest::FetchMore`]
    /// 入队，剩下的判定（没 SQL / 已经取完 / 执行器不支持 / 正在取）全在宿主
    /// `fetch_more` 里——那边每条分支都留可读原因，键位这边不必再写一遍。
    /// DBeaver 在无更多数据时是静默 `return`（`ResultSetViewer:4384`），本仓键位层
    /// 统一走「不可用留提示」（第 69 批口径），所以这里也不额外拦。
    pub fn request_next_segment(&mut self, cx: &mut Context<Self>) {
        self.queue.request(crate::ports::ResultRequest::FetchMore, cx);
    }

    /// 【第 72 批】`CTRL+SHIFT+=` 的取全部入口（DBeaver `fetch.all`，`plugin.xml:1070`）
    ///
    /// 与状态栏「获取全部数据」钮**同一条路**：只送 [`crate::ports::ResultRequest::AskFetchAll`]
    /// （先问一声），不在这里直接开跑——DBeaver 点 `fetch.all` 也是先过
    /// `ConfirmationDialog`。弹不弹、要不要弹由宿主判（看 `can_fetch_more`）。
    pub fn request_fetch_all(&mut self, cx: &mut Context<Self>) {
        self.queue
            .request(crate::ports::ResultRequest::AskFetchAll, cx);
    }

    /// 【第 74 批】`CTRL+R` 的「撤销未提交变更」入口（DBeaver `CMD_REJECT_CHANGES`
    /// → `rejectChanges()`，`plugin.xml:1083`；`ResultSetHandlerMain:313-314`）。
    ///
    /// 与工具栏那颗「回滚」钮同一条路：只入队 [`crate::ports::ResultRequest::RollbackPending`]。
    /// 判定（有没有待提交 / 清完怎么提示）在宿主 `discard_pending_changes`，
    /// 这里不复制第二套真值表。
    pub fn request_rollback_pending(&mut self, cx: &mut Context<Self>) {
        self.queue
            .request(crate::ports::ResultRequest::RollbackPending, cx);
    }

    /// 【第 70 批】`CTRL+G` 的行号跳转入口：把那口行号输入框准备好并返回焦点句柄
    /// （DBeaver `CMD_GO_TO_ROW` → `GotoLineDialog`，`plugin.xml:1091`）。
    ///
    /// 本仓没有「弹一个输入框」这一层：状态栏中段本来就常驻一口可编辑输入框
    /// （`row_count_input`，订阅 `InputEvent::Change` 即时入队 `GoRow`，见构造期的
    /// `_row_count_sub`）。所以键位的职责只是照 DBeaver 的**行为**做三件事：
    /// 1. 让用户知道当前在哪一行；2. 把焦点交进去（宿主 `window.focus`，面板层拿不到
    /// window）；3. 用户敲完 `ENTER` 时焦点交回网格（见 `_row_count_sub` 的 `PressEnter` 分支）。
    ///
    /// **位置信息放 placeholder，不预填进 value**：DBeaver 的 `InputDialog` 预填当前行号
    /// 且默认全选（`ResultSetHandlerMain:506` + `InputDialog` 的 Text），敲第一下就覆盖；
    /// 但 gpui-kit 的 `InputState` 对外只暴露 `value` / `focus_handle`（`input/state.rs:325/335`），
    /// **没有 `select_all`**——预填之后用户再敲数字会追加成 `22` 这种误跳。所以这里清空
    /// value、把当前行号写进 placeholder：位置照样看得见，输入也不会被拼错。
    ///
    /// `status` 为 `None`（还没执行过）时没有可跳的行，返回 `None` 让宿主留提示。
    pub fn row_jump_focus(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        // 当前行（1 基）；没选中行时按 DBeaver 的口径从第 1 行起（`getCurrentRow() == null`
        // → `initialValue` 取 1）
        let current = self.status.as_ref()?.selected_row.unwrap_or(1);
        self.row_count_input.update(cx, |state, inner| {
            state.set_value("", window, inner);
            state.set_placeholder(current.to_string(), window, inner);
        });
        Some(self.row_count_input.read(cx).focus_handle(cx))
    }

    /// 打开跳列浮层（`HostPanel::on_goto_column` 直接调到这里，与 `CTRL+G` 的
    /// `row_jump_focus` 同构——都要 window 交焦点）：没有列可跳时返回 `false`
    /// 让宿主留提示；否则清空上一轮的值 / 错误，把焦点交给输入框。
    ///
    /// 当前列名的提示放 **placeholder**、不预填 value——同 [`Self::row_jump_focus`]
    /// 的理由（`InputState` 没有 `select_all`，预填后敲字会拼成「ORDER BYa」这种误跳）。
    pub fn open_goto_column(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let columns = self.grid.read(cx).delegate().columns().to_vec();
        if columns.is_empty() {
            return false;
        }
        let hint = self
            .selected_col
            .and_then(|data| columns.get(data).cloned())
            .unwrap_or_else(|| "列名或序号（从 1 起）".to_string());
        self.goto_column_open = true;
        self.goto_column_error = None;
        self.goto_column_input.update(cx, |state, inner| {
            state.set_value("", window, inner);
            state.set_placeholder(hint, window, inner);
        });
        let handle = self.goto_column_input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
        true
    }

    /// 关掉跳列浮层（取消 / Esc / ✕）：只收开关与错误，不动结果集，**焦点回网格**
    /// （否则焦点留在已隐藏的输入框上，网格键位全部失效——与 `row_jump_focus` 同一条坑）。
    pub fn close_goto_column(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.goto_column_open {
            return;
        }
        self.goto_column_open = false;
        self.goto_column_error = None;
        let handle = self.grid.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
        cx.notify();
    }

    /// 提交跳列（输入框 `ENTER`）：解析 → 命中就跳并收浮层；没命中就把原因留在
    /// 输入框下面，**浮层不关**（DBeaver 弹错后仍留在同一对话框，用户可以改了再试）。
    pub fn submit_goto_column(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.goto_column_input.read(cx).value().to_string();
        match self.goto_column_target(&text, cx) {
            Ok(data_col) => {
                self.goto_column_open = false;
                self.goto_column_error = None;
                self.goto_column(data_col, cx);
                let handle = self.grid.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
                cx.notify();
            }
            Err(reason) => {
                self.goto_column_error = Some(reason.into());
                cx.notify();
            }
        }
    }

    /// 跳列解析（对齐 DBeaver `askForColumnToGo:815-849` 的三段顺序）：
    ///
    /// 1. **先按名字**，大小写不敏感（上游 `attrsByName` 用 `name.toUpperCase()` 建映射）；
    ///    重名：上游 `put` 是后到覆盖，本仓取**第一个**（列序稳定，跳转结果可预期）。
    /// 2. 名字没命中，再按 **1 基序号**，范围 `1..=列数`（上游 `index >= 1 && index <=
    ///    attributes.length` 后取 `attributes[index-1]`）。
    /// 3. 都不中 → 「未知列名」（上游同一句 `"Unknown column name"`，不细分越界 / 打错）。
    ///
    /// 本仓额外一条：命中的列在**显示序里被隐藏**时报「已隐藏」——上游没有隐藏列这层，
    /// 本仓也不做「跳过去顺手取消隐藏」，让用户自己去列浏览器里恢复。
    pub fn goto_column_target(&self, input: &str, cx: &App) -> Result<usize, String> {
        let columns = self.grid.read(cx).delegate().columns();
        if columns.is_empty() {
            return Err("没有可跳转的列".to_string());
        }
        let text = input.trim();
        if text.is_empty() {
            return Err("请输入列名或序号".to_string());
        }
        let data_col = match columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case(text))
        {
            Some(index) => index,
            None => match text.parse::<usize>() {
                Ok(index) if (1..=columns.len()).contains(&index) => index - 1,
                _ => return Err(format!("未知列名：{text}")),
            },
        };
        if self.grid.read(cx).delegate().view_column(data_col).is_none() {
            let name = columns[data_col].clone();
            return Err(format!("「{name}」已隐藏，先在列浏览器里取消隐藏"));
        }
        Ok(data_col)
    }

    /// 定位到某列：**保持当前行**（DBeaver `setCurrentAttribute` 之后是
    /// `scrollToRow(CURRENT)`，`:531-534`——纵向滚回当前行，不是跳到首行）+ 横向滚到该列。
    ///
    /// 列映射：数据列 → 显示列（`view_column`）→ **组件列**（`+1`：组件列 0 是行号槽）。
    /// `selected_col` 一并写，右栏列浏览器 / 值详情跟着换列。
    fn goto_column(&mut self, data_col: usize, cx: &mut Context<Self>) {
        self.selected_col = Some(data_col);
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            if let Some(view_col) = state.delegate().view_column(data_col) {
                let row = state
                    .selected_cell()
                    .map(|(row, _)| row)
                    .or_else(|| state.selected_row())
                    .unwrap_or(0);
                state.set_selected_cell(row, view_col + 1, cx);
                state.scroll_to_row(row, cx);
                state.refresh(cx);
            }
        });
        cx.notify();
    }

    /// 【测试】跳列浮层是否开着（editor 用例跨 crate，读不到 `pub(crate)` 字段）
    pub fn goto_column_for_test(&self, _cx: &App) -> bool {
        self.goto_column_open
    }

    /// 【测试】回填跳列输入框（同上：跨 crate 拿不到 `goto_column_input`）
    pub fn set_goto_column_text_for_test(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.goto_column_input
            .update(cx, |state, inner| state.set_value(text, window, inner));
    }

    /// 【右栏】标签页切换（列浏览器 / 值详情 / 计算 / 分组 / 引用 / 执行SQL）。
    /// 右栏收起时切页签会自动展开（DBeaver 口径：右缘竖条点图标即展开对应面板）。
    /// 目标页若已被关闭，会先按规范序重新启用它（点得到就等于开得回来）。
    pub fn set_right_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.open_right_tab_only(tab);
        self.right_tab = tab;
        if !self.show_value_sidebar {
            // 窄展开 / 收起态点页签图标 → 宽展开（竖条可见性保持；收起态在窄态已置 true）
            self.edge_bar_visible = true;
            self.show_value_sidebar = true;
            self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
        }
        cx.notify();
    }

    /// 【右栏】把某页登记进已启用集合（已存在则不动），按**规范序**插入。
    fn open_right_tab_only(&mut self, tab: RightTab) {
        if self.open_tabs.contains(&tab) {
            return;
        }
        let canonical = right_tabs();
        let pos = canonical
            .iter()
            .position(|candidate| *candidate == tab)
            .unwrap_or(canonical.len());
        let insert_at = self
            .open_tabs
            .iter()
            .position(|existing| {
                canonical
                    .iter()
                    .position(|candidate| candidate == existing)
                    .unwrap_or(usize::MAX)
                    > pos
            })
            .unwrap_or(self.open_tabs.len());
        self.open_tabs.insert(insert_at, tab);
    }

    /// 【右栏】已启用的页集合（规范序）：页签条按它渲染，视图菜单勾选态读它。
    pub fn open_tabs(&self) -> &[RightTab] {
        &self.open_tabs
    }

    /// 【右栏】注入已启用的页集合（宿主在创建后按偏好 `result_panel.open_tabs` 调用）。
    ///
    /// 入参按**规范序**归一：只收已知页、重复丢弃、顺序按 [`RightTab::ALL`]；
    /// 结果为空 = 「偏好里没写过 / 被关空」→ 保持现状（出厂六页全开），对齐 DBeaver
    /// `activateDefaultPanels:1396-1400`（先把空的 id 清掉再补默认面板，所以空集在那边
    /// 也等于「恢复默认」）。当前页不在新集合里时落到集合第一页。
    pub fn set_open_tabs_initial(&mut self, tabs: &[RightTab], cx: &mut Context<Self>) {
        let normalized: Vec<RightTab> = RightTab::ALL
            .into_iter()
            .filter(|tab| tabs.contains(tab))
            .collect();
        if normalized.is_empty() || normalized == self.open_tabs {
            return;
        }
        if !normalized.contains(&self.right_tab) {
            self.right_tab = normalized[0];
        }
        self.open_tabs = normalized;
        cx.notify();
    }

    /// 【右栏】重新打开一页 = 登记进集合 + 置为当前页（视图菜单勾选项）。
    pub fn open_right_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        self.set_right_tab(tab, cx);
    }

    /// 【右栏】**只关掉这一页**（其余页仍在页签条里），对齐 DBeaver
    /// `CTabItem(panelFolder, SWT.CLOSE)` + `presentationSettings.enabledPanelIds` 的移除语义
    /// （`ResultSetViewer.addPanel:1357/:1388`）。
    ///
    /// 规则：
    /// - 关的是当前页 → 落到**规范序里紧随其后**的剩余页，没有则取前一页；
    /// - 集合被关空 → 整块面板收起（`set_panels_visible(false)`，与「关掉最后一个面板」
    ///   在那边导致页签夹无项一致）；再打开任一一页会重新展开。
    /// - 关一页**不**复位最大化 / 宽度等其它面板状态。
    pub fn close_right_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) {
        if !self.open_tabs.contains(&tab) {
            return;
        }
        self.open_tabs.retain(|existing| *existing != tab);
        if self.open_tabs.is_empty() {
            self.set_panels_visible(false, cx);
            return;
        }
        if self.right_tab == tab {
            self.right_tab = neighbor_after_close(&self.open_tabs, tab);
        }
        cx.notify();
    }

    /// 【右缘竖条】点某一页的图标：切到该页（未启用的页会先启用），**不收起整栏**。
    ///
    /// 取证 `ResultSetHandlerTogglePanel.java`：`toggleResultsPanel:64-69` 算
    /// `isVisible = rsv.isPanelsVisible() && rsv.isPanelVisible(panelId)`，真则
    /// `rsv.closePanel(panelId)`——**关掉的是那一页**；DBeaver 面板是 `CTabFolder` 多页签
    /// （`addPanel:1357` 的 `SWT.CLOSE`），其余页仍在。本仓第 24 批起同样是多页签模型
    /// （`open_tabs`），所以「点当前页就收起整栏」这条自造语义没有理由再留着：
    /// 点已选中的页 = 无操作，整栏显隐只走 `SetPanelsVisible`（竖条顶部总开关 / 工具栏）。
    ///
    /// 返回 `true` = 本次**切到了该页**（调用方要像切页签那样触发刷新，如引用页取外键）；
    /// 返回 `false` = 已经停在这一页（无操作，不需要刷新）。
    pub fn toggle_right_tab(&mut self, tab: RightTab, cx: &mut Context<Self>) -> bool {
        // 【toggle 语义】对齐 `ResultSetHandlerTogglePanel`：`toggleResultsPanel:64-69` 先算
        // `isVisible = isPanelsVisible() && isPanelVisible(id)`，再 `showResultsPanel(id, isVisible)`
        // ——`showResultsPanel:71-81` 是「可见 → `closePanel(id)`（关掉这一页）；不可见 →
        // `activatePanel(id, true, true)`（打开并激活，整栏收起时顺带展开）」。
        // 所以这一层就是「开/关该页」，不是「切页 / 无操作」：
        // - 已启用且整栏可见 → 关掉这一页（其余页仍在，`closePanel→removePanel:1441-1451`）；
        // - 其余（已启用但整栏收起 / 已启用但当前不是它 / 被关掉过）→ 打开并切到该页。
        let visible = self.panels_visible() && self.open_tabs.contains(&tab);
        if visible {
            self.close_right_tab(tab, cx);
            return false;
        }
        self.set_right_tab(tab, cx);
        true
    }

    /// 【引用页】回填外键清单（宿主取数回来后调用）。
    ///
    /// 与 DBeaver `ReferencesResultsContainer.fillKeysCombo`（`:359-377`）同口径：
    /// 清单为空就清掉选中；非空时**优先保住原选中项**（同名键仍存在就不改），
    /// 保不住再默认选第一条（`:341-343`）。
    pub fn set_references(
        &mut self,
        state: crate::references::ReferencesState,
        cx: &mut Context<Self>,
    ) {
        let same_state = self.references == state;
        self.references = state;
        let keys_len = match &self.references {
            crate::references::ReferencesState::Loaded(keys) => keys.len(),
            _ => 0,
        };
        if keys_len == 0 {
            self.active_reference = None;
        } else if self.active_reference.map(|index| index >= keys_len).unwrap_or(true) {
            self.active_reference = Some(0);
        }
        if !same_state {
            cx.notify();
        }
    }

    /// 【计算行数】回填宿主算回来的总行数文案（对齐 DBeaver `rowCountLabel`：
    /// 数字**就地**显示在状态行那颗钮上，不开新结果集）。
    pub fn set_row_count_notice(&mut self, note: String, cx: &mut Context<Self>) {
        if self.row_count_note.as_deref() == Some(note.as_str()) {
            return;
        }
        self.row_count_note = Some(note);
        cx.notify();
    }

    /// 【引用页】键下拉选中某一项（`None` = 清掉选中）
    pub fn set_active_reference(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        if self.active_reference == index {
            return;
        }
        self.active_reference = index;
        cx.notify();
    }

    /// 【引用页】当前选中的引用键（render 与「打开目标对象」都读它）
    pub fn active_reference_key(&self) -> Option<&crate::references::ReferenceKey> {
        match &self.references {
            crate::references::ReferencesState::Loaded(keys) => {
                keys.get(self.active_reference?)
            }
            _ => None,
        }
    }

    /// 【引用页】当前结果的**来源表身份 + 可见列**（宿主据此取约束）。
    ///
    /// 来源表只认同一个限定名：所有已注解列都指向同一张表才认，否则返回 `None`
    /// （口径与 `open_ddl_dialog` 的表名推导一致——来自多表 join / 计算列的结果
    /// 没有单一来源表，DBeaver 那边也只按「可见属性所属实体」收集，收集不到就空着）。
    ///
    /// 可见列 = delegate 的列减去面板记的隐藏列，**并且**只留来自该来源表的列：
    /// 结果里夹着计算列时，那些列不属于任何实体，DBeaver 的
    /// `entityAttributes.containsAll(attributes)` 也不会拿它们放行。
    pub fn reference_source(
        &self,
        cx: &App,
    ) -> Option<(String, String, Vec<String>)> {
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let columns = delegate.columns().to_vec();
        let annotations = delegate.column_annotations().to_vec();

        let source_tables: Vec<&str> = annotations
            .iter()
            .filter_map(|annotation| annotation.as_ref()?.source_table.as_deref())
            .collect();
        let table = match source_tables.first() {
            Some(first) if source_tables.iter().all(|table| *table == *first) => *first,
            _ => return None,
        };
        let (schema, name) = match table.split_once('.') {
            Some((schema, name)) => (schema.to_string(), name.to_string()),
            None => (String::new(), table.to_string()),
        };

        let visible: Vec<String> = columns
            .iter()
            .enumerate()
            .filter(|(_, column)| !self.column_hidden.contains(column))
            .filter(|(index, _)| {
                annotations
                    .get(*index)
                    .and_then(|annotation| annotation.as_ref())
                    .and_then(|annotation| annotation.source_table.as_deref())
                    == Some(table)
            })
            .map(|(_, column)| column.clone())
            .collect();
        Some((schema, name, visible))
    }

    /// 【引用页】进入该页签时的取数入口（宿主在 `SetRightTab(Reference)` 分派里调）：
    /// 状态置 `Loading` 并入队 [`crate::ports::ResultRequest::LoadReferences`]。
    ///
    /// 面板自己不判断「读过了没」——重复切页签重读一遍是 DBeaver 的行为
    /// （`activatePanel` 每次都 `refresh`），这里照它。
    pub fn begin_references_load(&mut self, cx: &mut Context<Self>) {
        let Some((schema, table, visible_columns)) = self.reference_source(cx) else {
            // 没有单一来源表：没有可读的约束，直接落在「无引用」
            self.set_references(
                crate::references::ReferencesState::Loaded(Vec::new()),
                cx,
            );
            return;
        };
        self.set_references(crate::references::ReferencesState::Loading, cx);
        self.queue.request(
            crate::ports::ResultRequest::LoadReferences {
                schema,
                table,
                visible_columns,
            },
            cx,
        );
    }

    /// 【引用页】「查看引用行」用的 SQL：选中键 + 当前行的外键值 → `SELECT …`。
    ///
    /// 拿不到就返回 `None`（按钮禁用），四种情形：没有选中键、没有选中行、
    /// 参与列被隐藏（DBeaver 只用**可见属性**，隐藏列的值不该拿来查）、
    /// 该列在当前行没有值。
    pub fn reference_rows_sql(&self, cx: &App) -> Option<String> {
        let key = self.active_reference_key()?.clone();
        let grid = self.grid.read(cx);
        let delegate = grid.delegate();
        let row = delegate.selected_view_row()?;
        let columns = delegate.columns().to_vec();
        let annotations = delegate.column_annotations().to_vec();
        // 显示序（隐藏列已被 delegate 排除）：`row_cell_text` 吃显示列号 + 1（首列是行号槽）
        let display = delegate.display_order();

        let mut values: Vec<crate::references::ReferenceValue> =
            Vec::with_capacity(key.columns.len());
        for column in key.columns.iter() {
            let data_index = columns
                .iter()
                .position(|candidate| candidate.eq_ignore_ascii_case(column))?;
            let data_type = annotations
                .get(data_index)
                .and_then(|annotation| annotation.as_ref())
                .and_then(|annotation| annotation.data_type.clone());
            let display_index = display
                .iter()
                .position(|candidate| candidate.eq_ignore_ascii_case(column))?;
            let text = delegate.row_cell_text(row, display_index + 1)?;
            values.push(crate::references::ReferenceValue {
                column: column.clone(),
                value: text.to_string(),
                data_type,
            });
        }
        crate::references::reference_rows_sql(&key, &values)
    }

    /// 【引用页】「查看引用行」：把上一条 SQL 交给编辑器另开一份结果集（方案 `B′`）。
    pub fn open_reference_rows(&mut self, cx: &mut Context<Self>) {
        let Some(sql) = self.reference_rows_sql(cx) else {
            return;
        };
        self.queue
            .request(crate::ports::ResultRequest::OpenReferenceRows { sql }, cx);
    }

    /// 【R2 计算页】启用一个统计项（对齐 DBeaver Calc「＋添加函数」）。
    ///
    /// 插入位置按 [`StatsItem::ALL`] 的固定顺序算（不是追加到末尾）：DBeaver 的
    /// `AddFunctionItemAction` 是 `enabledFunctions.add(func)` 追加，但它的函数集来自
    /// 注册表、顺序本身不固定；本仓六项有稳定的展示顺序（非空 → NULL → 最小 → 最大 →
    /// 均值 → 合计），删了再加回去要回到原位，否则行序会随操作乱跳。
    pub fn add_stats_item(&mut self, item: StatsItem, cx: &mut Context<Self>) {
        if self.stats_items.contains(&item) {
            return;
        }
        let rank = StatsItem::ALL
            .iter()
            .position(|candidate| *candidate == item)
            .unwrap_or(usize::MAX);
        let insert_at = self
            .stats_items
            .iter()
            .position(|existing| {
                StatsItem::ALL
                    .iter()
                    .position(|candidate| candidate == existing)
                    .unwrap_or(usize::MAX)
                    > rank
            })
            .unwrap_or(self.stats_items.len());
        self.stats_items.insert(insert_at, item);
        cx.notify();
    }

    /// 【R2 计算页】停用一个统计项（统计行行尾 × 触发；对齐「−删除函数」语义）
    pub fn remove_stats_item(&mut self, item: StatsItem, cx: &mut Context<Self>) {
        let before = self.stats_items.len();
        self.stats_items.retain(|existing| *existing != item);
        if self.stats_items.len() != before {
            cx.notify();
        }
    }

    /// 【R2 计算页】恢复默认函数集（对齐 DBeaver `ResetFunctionsAction`
    /// `:438-449`：`enabledFunctions.clear()` + `loadDefaultFunctions()` →
    /// 只留 `isDefault()` 打标的项，本仓 = 「非空」，见 `StatsItem::DEFAULT_SET`）
    pub fn reset_stats_items(&mut self, cx: &mut Context<Self>) {
        let defaults = StatsItem::DEFAULT_SET.to_vec();
        if self.stats_items == defaults {
            return;
        }
        self.stats_items = defaults;
        cx.notify();
    }

    /// 【计算页持久化】宿主构造期注入启用集：设置里的 id 列表 → 面板状态。
    ///
    /// 对齐 DBeaver `AggregateColumnsPanel.loadSettings`（`:129-155`）：那边把
    /// `enabledFunctions` 按存下的 `index` 顺序查回函数；本仓 `stats_items_from_ids`
    /// 只保留九项里的已知 id、丢弃未知项与重复项（顺序照抄）。空串合法（全停用）。
    pub fn set_stats_items_initial(&mut self, ids: &str, cx: &mut Context<Self>) {
        let items = crate::ports::stats_items_from_ids(ids);
        if items == self.stats_items {
            return;
        }
        self.stats_items = items;
        cx.notify();
    }

    /// 【计算页持久化】当前启用集的落盘形态（宿主在 ＋ / × / Reset 生效后读走这一串）。
    pub fn stats_items_ids(&self) -> String {
        crate::ports::stats_items_to_ids(&self.stats_items)
    }

    /// 【R2 计算页】切换聚合口径（数值 / 字符串；对齐 `aggregateAsStrings` CHECK）
    pub fn set_stats_mode(&mut self, mode: StatsMode, cx: &mut Context<Self>) {
        if self.stats_mode == mode {
            return;
        }
        self.stats_mode = mode;
        cx.notify();
    }

    /// 【R3 分组页】切换某个额外分组列：已在集合里则移除，不在则追加到末尾
    /// （追加顺序即分组键的拼接顺序——用户能感知地决定「按 A 再按 B」）
    pub fn toggle_group_column(&mut self, column: usize, cx: &mut Context<Self>) {
        if let Some(ix) = self.group_columns.iter().position(|c| *c == column) {
            self.group_columns.remove(ix);
        } else {
            self.group_columns.push(column);
        }
        cx.notify();
    }

    /// 【R3 分组页】把分组键里某一列上移 / 下移一位（`delta` = -1 / +1）。
    ///
    /// 对齐 DBeaver `GroupingColumnsContainer.moveColumns:126-152`：分组键顺序 = 分组嵌套顺序，
    /// 挪一位就是改层级。列不在键里 / 已在边界 → 什么都不做（不 notify，避免无谓重绘）。
    pub fn move_group_column(&mut self, column: usize, delta: i8, cx: &mut Context<Self>) {
        if !super::tabs::move_group_column_in_place(&mut self.group_columns, column, delta) {
            return;
        }
        cx.notify();
    }

    /// 【R3 分组页】切换分组结果排序口径（按值 / 按计数）
    pub fn set_group_sort(&mut self, sort: GroupSort, cx: &mut Context<Self>) {
        if self.group_sort == sort {
            return;
        }
        self.group_sort = sort;
        cx.notify();
    }

    /// 【R3 分组页】切换「仅重复」（只留行数 > 1 的分组）
    pub fn set_group_duplicates_only(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.group_duplicates_only == on {
            return;
        }
        self.group_duplicates_only = on;
        cx.notify();
    }

    /// 【分组页持久化】宿主构造期注入排序口径与「仅重复」：设置里的 id / bool → 面板状态。
    ///
    /// 对齐 DBeaver 两个结果集偏好：`RS_GROUPING_DEFAULT_SORTING`（`ChangeSortingAction`
    /// 读写，`""`/`ASC`/`DESC`）与 `RS_GROUPING_SHOW_DUPLICATES_ONLY`（`DuplicatesOnlyAction`
    /// 的 check box 状态）。本仓两态排序里 `countdesc` 对应那边的 `DESC`，`value` 是超集档；
    /// 未知 id 按出厂 `value` 处理（不猜，与设置服务侧的归一同一口径）。
    pub fn set_group_prefs_initial(
        &mut self,
        sort_id: &str,
        duplicates_only: bool,
        cx: &mut Context<Self>,
    ) {
        let sort = GroupSort::parse(sort_id).unwrap_or_default();
        if self.group_sort == sort && self.group_duplicates_only == duplicates_only {
            return;
        }
        self.group_sort = sort;
        self.group_duplicates_only = duplicates_only;
        cx.notify();
    }

    /// 【分组页持久化】当前排序口径的落盘 id（宿主在排序钮生效后读走这一串）
    pub fn group_sort_id(&self) -> &'static str {
        self.group_sort.id()
    }

    /// 【分组页持久化】当前「仅重复」开关值（宿主在该钮生效后读走）
    pub fn group_duplicates_only_value(&self) -> bool {
        self.group_duplicates_only
    }

    /// 【R3 分组页】清空分组列集合（对齐 DBeaver `ClearGroupingAction`；该动作在
    /// DBeaver 里只在分组列非空时可用，这里同口径：空集合直接 return 不 notify）。
    /// 清空后分组页回落到「网格选中格 / 清单选中列」兜底列，而不是空态
    pub fn clear_group_columns(&mut self, cx: &mut Context<Self>) {
        if self.group_columns.is_empty() {
            return;
        }
        self.group_columns.clear();
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
        // 复用同一浮层的另一场景（生成 DDL）可能改过标题，这里恢复成「原始 SQL」
        self.sql_dialog_title = None;
        cx.notify();
    }

    /// 【2026-10-28 微调】关闭 SQL 预览弹窗（✕ 按钮 / Esc 触发）
    pub fn close_sql_dialog(&mut self, cx: &mut Context<Self>) {
        self.sql_dialog_open = false;
        self.sql_dialog_title = None;
        cx.notify();
    }

    /// 【第 72 批】打开「获取全部数据」确认浮层
    /// （DBeaver `CONFIRM_RS_FETCH_ALL`，`ResultSetViewer.readAllData:4433-4440`）。
    ///
    /// **弹不弹由宿主判**：宿主先看结果条目 `can_fetch_more`（无下一段 / 有执行错误
    /// 都直接回一句提示，不打扰用户，对齐 DBeaver `:4430` 的静默 `return`），确认要
    /// 问才调这里。所以本方法只置标志，不再做一遍判定——两处真值表迟早会分叉。
    ///
    /// 不开 `filter_settings_open` 那类「回填输入框 + 交焦点」的活：浮层只有两颗
    /// 按钮，没有输入源，`&mut Window` 不是必需品（与 SQL 预览同一签名）。
    pub fn open_fetch_all_confirm(&mut self, cx: &mut Context<Self>) {
        if self.fetch_all_confirm_open {
            return;
        }
        self.fetch_all_confirm_open = true;
        cx.notify();
    }

    /// 【第 72 批】关掉确认浮层（取消 / Esc / ✕）：只收标志，不动结果集
    pub fn close_fetch_all_confirm(&mut self, cx: &mut Context<Self>) {
        if !self.fetch_all_confirm_open {
            return;
        }
        self.fetch_all_confirm_open = false;
        cx.notify();
    }

    /// 【第 72 批】确认浮层是否开着（供宿主派发与测试断言「真弹了」）
    pub fn fetch_all_confirm_state(&self) -> bool {
        self.fetch_all_confirm_open
    }

    /// 【对齐 DBeaver Metadata 面板「Generate DDL」】按当前结果集的列生成建表 DDL，
    /// 复用 SQL 预览浮层展示（DBeaver 也是把生成结果塞进 `ViewSQLDialog` 弹出来）。
    ///
    /// 素材取网格 delegate 的列名 / 类型 / 注解（`ColumnAnnotation.data_type` 是驱动
    /// 口径类型、`nullable` / `default_value` 也来自列元数据；`ColumnType.kind` 只用于
    /// 驱动没报类型时按值族推断——推断会在行尾注明）。表名取「所有已注解列一致的
    /// 来源表裸名」，否则 `new_table`（DBeaver 的表名来自迁移映射容器，本仓无此机制）。
    /// 出不了 SQL 时（缺类型 / 认不出值族）浮层里直接显示说明，不编类型。
    pub fn open_ddl_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // 网格读借用必须在本块内结束：后面建 Textarea 要 `&mut Context`
        let (table, ddl_columns) = {
            let grid = self.grid.read(cx);
            let delegate = grid.delegate();
        let columns = delegate.columns().to_vec();
        let types = delegate.column_types().to_vec();
        let annotations = delegate.column_annotations().to_vec();

        // 表名：所有「有来源表」的列指向同一张表才用它的裸名，否则占位
        let source_tables: Vec<&str> = annotations
            .iter()
            .filter_map(|annotation| annotation.as_ref()?.source_table.as_deref())
            .collect();
        let table = match source_tables.first() {
            Some(first) if source_tables.iter().all(|table| *table == *first) => first
                .rsplit('.')
                .next()
                .unwrap_or(*first)
                .to_string(),
            _ => "new_table".to_string(),
        };

        let ddl_columns: Vec<crate::ddl::DdlColumn> = columns
            .iter()
            .enumerate()
            .map(|(data_col, name)| {
                let annotation = annotations.get(data_col).cloned().flatten();
                let kind = types
                    .get(data_col)
                    .cloned()
                    .flatten()
                    .map(|ty| ty.kind)
                    .unwrap_or_default();
                // 驱动口径优先：注解里的 data_type（NavCache）→ 渲染层 ColumnType.name
                let driver_type = annotation
                    .as_ref()
                    .and_then(|annotation| annotation.data_type.as_deref())
                    .or_else(|| {
                        types
                            .get(data_col)
                            .and_then(|ty| ty.as_ref())
                            .map(|ty| ty.name.as_str())
                    });
                let (type_name, inferred) = match crate::ddl::resolve_type(driver_type, kind) {
                    Some((name, inferred)) => (Some(name), inferred),
                    None => (None, false),
                };
                crate::ddl::DdlColumn {
                    name: name.clone(),
                    type_name,
                    inferred,
                    nullable: annotation.as_ref().and_then(|annotation| annotation.nullable),
                    default_value: annotation
                        .as_ref()
                        .and_then(|annotation| annotation.default_value.clone()),
                }
            })
            .collect();
            (table, ddl_columns)
        };

        let text = match crate::ddl::create_table_ddl(&table, &ddl_columns) {
            crate::ddl::DdlOutcome::Sql(sql) => sql,
            crate::ddl::DdlOutcome::Unavailable(reason) => reason,
        };

        let title = gpui_kit::SharedString::from("生成 DDL（按结果集列建表）");
        if self.sql_textarea.is_none() {
            let textarea = cx.new(|cx| TextareaState::new(window, cx).default_value(text.clone()));
            self.sql_textarea = Some(textarea);
        } else if let Some(ta) = self.sql_textarea.clone() {
            ta.update(cx, |state, cx| state.set_value(text.clone(), window, cx));
        }
        self.sql_dialog_title = Some(title);
        self.sql_dialog_open = true;
        cx.notify();
    }

    // ── 【筛选设置】自定义条件 / 排序浮层
    //   对齐 DBeaver 筛选条右列第一颗 `filtersCustomButton`（FILTER 图标、`SWT.DROP_DOWN`，
    //   `ResultSetFilterPanel.java:330-338`）：主点击开 `FilterSettingsDialog`，
    //   其中 Custom 页签是 `whereText` + `orderText` 两个自由文本框，应用时
    //   `setWhere` / `setOrder`（`FilterSettingsDialog.java:435/493-500`）。

    /// 打开浮层：把**已生效**的自定义排序回填输入框（WHERE 一栏不在这里编辑——
    /// 筛选框本体就是它的唯一输入源），并把焦点交给排序框。
    pub fn open_filter_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let order = self.filter_order.clone();
        let input = self.filter_order_input.clone();
        input.update(cx, |state, cx| state.set_value(order, window, cx));
        input.update(cx, |state, cx| state.focus(window, cx));
        self.filter_settings_open = true;
        cx.notify();
    }

    /// 关闭浮层：丢弃草稿（不改任何已生效状态；重新打开会按已生效值回填）
    pub fn close_filter_settings(&mut self, cx: &mut Context<Self>) {
        self.filter_settings_open = false;
        cx.notify();
    }

    /// 【筛选设置】切换页签（`FilterSettingsTab`：列约束 / 自定义）
    pub fn set_filter_settings_tab(
        &mut self,
        tab: crate::ports::FilterSettingsTab,
        cx: &mut Context<Self>,
    ) {
        if self.filter_settings_tab != tab {
            self.filter_settings_tab = tab;
            cx.notify();
        }
    }

    /// 【筛选设置 · 列约束】当前列清单快照，按**数据列序**。
    ///
    /// DBeaver 的 Columns 页签是列约束树（名称 / 位置 / 可见 / 固定 / order / criteria，
    /// `FilterSettingsDialog.java:129-243`）。本仓「可见 / 冻结」的权威在网格 delegate（可写），
    /// `criteria` / `sort` 分别取列级筛选与本地排序（只读投影）——渲染期读，不做任何写入。
    pub fn column_constraint_rows(&self, cx: &App) -> Vec<crate::ports::ColumnConstraint> {
        let delegate = self.grid.read(cx).delegate();
        let filter = delegate.column_filter();
        let sort = delegate.sort_state();
        delegate
            .columns()
            .iter()
            .enumerate()
            .map(|(index, name)| crate::ports::ColumnConstraint {
                name: name.clone(),
                visible: !delegate.is_hidden_column(index),
                frozen: delegate.is_frozen(index),
                criteria: filter
                    .filter(|(column, _)| *column == index)
                    .map(|(_, text)| text.to_string()),
                sort: sort
                    .filter(|(column, _)| *column == index)
                    .map(|(_, descending)| descending),
            })
            .collect()
    }

    /// 应用：读出排序草稿 → 入队 [`ResultRequest::ApplyFilterSettings`]
    /// （WHERE 取筛选框当前文本，重查由宿主完成——面板不持有执行能力）
    pub fn apply_filter_settings(&mut self, cx: &mut Context<Self>) {
        let order = self
            .filter_order_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        self.filter_order = order.clone();
        self.filter_settings_open = false;
        self.queue.request(
            ResultRequest::ApplyFilterSettings {
                where_text: self.filter_text.trim().to_string(),
                order_text: order,
            },
            cx,
        );
    }

    /// 【筛选设置】只读快照（渲染浮层用：开关 + 已生效排序 + 当前筛选文本）
    pub fn filter_settings_state(&self) -> (bool, &str, &str) {
        (
            self.filter_settings_open,
            self.filter_order.as_str(),
            self.filter_text.trim(),
        )
    }

    /// 【筛选设置 · 存档】「保存过滤器设置」：把当前这份筛选记下来。
    ///
    /// 对齐 DBeaver `CMD_FILTER_SAVE_SETTING` → `saveDataFilter()`
    /// （`ResultSetViewer.java:648-664`）：存的是**当前生效的那一份**（面板读自己持有的
    /// `filter_text` / `filter_order`，不读输入框草稿——草稿没提交就不算数）。
    ///
    /// 与 DBeaver 的两处口径差：
    /// - 那边 `isDirty()` 为假就把旧键**删掉**（`:273-288`）；本仓没有「脏」概念，
    ///   空筛选也照存（存一份「无筛选」的存档比静默删掉更可解释）；
    /// - 那边只对 `DBSEntity` 提供（`ResultSetFilterPanel.java:1565-1570`）；
    ///   本仓用「有没有唯一来源表」等价表达，但**不禁止**存（`table` 只做记录）。
    ///
    /// 存完给一行提示（对齐 `:661-663` 的 `DBeaverNotifications.showNotification`）。
    pub fn save_filter_setting(&mut self, cx: &mut Context<Self>) {
        let table = self.unique_source_table(cx);
        let next = crate::panel::SavedFilterSetting {
            sql: self.result_sql.clone().unwrap_or_default(),
            filter: self.filter_text.trim().to_string(),
            order: self.filter_order.trim().to_string(),
            table,
        };
        // 【筛选存档落盘】与列布局同一条分工：面板推、宿主存。只在真的变了才推，
        // 免得用户连点两次「保存」就写两遍会话（写一次就是整份文档落库）
        let changed = self.saved_filter.as_ref() != Some(&next);
        self.saved_filter = Some(next);
        if changed {
            self.queue.push(ResultRequest::SavedFilterChanged);
        }
        self.set_notice(Some("筛选设置已保存".to_string()), cx);
        cx.notify();
    }

    /// 【筛选设置 · 存档】「重置过滤器设置」：忘掉存档。
    ///
    /// 对齐 DBeaver `CMD_FILTER_RESET_SETTING` → `resetSavedDataFilter()`
    /// （`ResultSetViewer.java:671-687`，`DataFilterRegistry.removeDataFilter`）。
    /// **只清存档**：当前正生效的筛选、筛选框里的词都不动——DBeaver 那边同样只删
    /// 配置项，随后同样发一条「已重置」通知。
    pub fn reset_filter_setting(&mut self, cx: &mut Context<Self>) {
        let had = self.saved_filter.is_some();
        self.saved_filter = None;
        // 【筛选存档落盘】真的删掉了才推落库（DBeaver 的 remove 是幂等的，本仓也照
        // 幂等发通知；但「本来就没有存档」时不该为一次空操作写一遍整份会话）
        if had {
            self.queue.push(ResultRequest::SavedFilterChanged);
        }
        self.set_notice(
            Some(if had {
                "筛选设置已重置".to_string()
            } else {
                // 没有存档也照发（DBeaver 的 remove 是幂等的，通知同样无条件发）
                "没有被保存的筛选设置".to_string()
            }),
            cx,
        );
        cx.notify();
    }

    /// 【筛选存档落盘】注入会话里读回来的存档（恢复路径）。
    ///
    /// **只注入、不套用**：套用交给下一次 `sync`（`saved_filter_applies` 判「同一份
    /// 结果 SQL + 当前没有筛选词」才写回筛选框），与 DBeaver「打开这张表的结果才
    /// `restoreDataFilter`」同口径——恢复会话时往往还没有结果集，现在就套等于无米之炊。
    /// 注入不推 [`ResultRequest::SavedFilterChanged`]：这不是用户改动，推了会回写成环。
    pub fn set_saved_filter(
        &mut self,
        saved: Option<crate::panel::SavedFilterSetting>,
        cx: &mut Context<Self>,
    ) {
        if self.saved_filter == saved {
            return;
        }
        self.saved_filter = saved;
        cx.notify();
    }

    /// 【筛选设置 · 存档】当前存档（渲染菜单项禁用态 / 测试断言用）
    pub fn saved_filter_setting(&self) -> Option<&crate::panel::SavedFilterSetting> {
        self.saved_filter.as_ref()
    }

    /// 【筛选设置 · 存档】取走「待回填筛选输入框的词」（宿主在窗口可用时转手调
    /// [`Self::sync_filter_input`]；取走即清，避免每次 sync 都回填一遍）
    pub fn take_filter_backfill(&mut self) -> Option<String> {
        self.filter_backfill.take()
    }

    /// 【筛选设置 · 存档】结果集有没有**唯一来源表**（`Some` = 能认成一张实体表）。
    ///
    /// 判据与 [`Self::reference_source`] 同源：所有已注解列都指向同一张表才认。
    /// 只用于记录在存档里（DBeaver 用 `instanceof DBSEntity` 做同一件事）。
    fn unique_source_table(&self, cx: &App) -> Option<String> {
        let delegate = self.grid.read(cx).delegate();
        let annotations = delegate.column_annotations().to_vec();
        let tables: Vec<&str> = annotations
            .iter()
            .filter_map(|annotation| annotation.as_ref()?.source_table.as_deref())
            .collect();
        match tables.first() {
            Some(first) if tables.iter().all(|table| *table == *first) => Some((*first).to_string()),
            _ => None,
        }
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

    /// 【右栏】集合被**关空**之后再显示时，恢复默认页集合。
    ///
    /// 对齐 `ResultSetViewer.showPanels:1478-1518`：展开路径带 `showDefaults=true`，
    /// 于是 `:1493-1496` 先 `activateDefaultPanels(getPresentationSettings())` 再放开 sash——
    /// 「面板被关光过一次」不会让页签夹继续空着，重新显示一定有页可显示。
    /// 本仓默认集合 = 出厂六页（`right_tabs()`；宿主注入的偏好集合走
    /// `set_open_tabs_initial`）。**只在空集时补**，非空的自定义集合原样保留。
    fn restore_default_tabs_when_empty(&mut self) {
        if !self.open_tabs.is_empty() {
            return;
        }
        self.open_tabs = right_tabs().to_vec();
        if !self.open_tabs.contains(&self.right_tab) {
            self.right_tab = self.open_tabs[0];
        }
    }

    /// 【右缘竖条】面板总开关：对齐 DBeaver 右缘竖条顶部那颗 `panelsButton`
    /// （`SWT.RIGHT | SWT.CHECK`，图标 `UIIcon.PANEL_CUSTOMIZE`，点击
    /// `showPanels(!isPanelsVisible(), true, true)`，checked = 面板可见；`ResultSetViewer.java:1038-1056`）。
    ///
    /// 本仓「面板可见」= 竖条与右栏内容都显（`edge_bar_visible && show_value_sidebar`）；
    /// 关 = 整条收起（与状态栏右端三态开关的收起态是同一定义）。重新打开走状态栏那个开关
    /// ——DBeaver 那边关掉后竖条也消失，重新打开走菜单命令 `CMD_TOGGLE_PANELS`，同一处境。
    ///
    /// **收起会吃掉最大化**：DBeaver 里面板可见性与最大化共用同一份 sash 可见性
    /// （`showPanels(false)` 就是 `setMaximizedControl(children[0])`，`:1487`），
    /// 关掉面板之后不可能还停在最大化。本仓同样复位 `panel_maximized`，
    /// 否则「面板已关但右栏被最大化强行挂着」会出现状态自相矛盾（竖条也一起没了）。
    pub fn set_panels_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if visible {
            if !self.panels_visible() {
                // 关空过就补回默认页集合（见 restore_default_tabs_when_empty 的取证）
                self.restore_default_tabs_when_empty();
                self.edge_bar_visible = true;
                self.show_value_sidebar = true;
                self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
                self.right_sidebar_dragging = false;
            }
        } else {
            self.edge_bar_visible = false;
            self.show_value_sidebar = false;
            self.panel_maximized = false;
            self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
            self.right_sidebar_dragging = false;
        }
        cx.notify();
    }

    /// 【右缘竖条】面板当前是否可见（= 竖条 + 右栏内容都在；总开关的 checked 态）
    pub fn panels_visible(&self) -> bool {
        self.edge_bar_visible && self.show_value_sidebar
    }

    /// 【侧栏】右侧开关：**三态循环**（比左侧多一个展开态；开关在状态栏右端）——
    /// 收起（竖条 + 右栏都不显示）→ 窄展开（只显右缘竖条图标条）→ 宽展开（竖条 + 右栏
    /// 内容）→ 收起。epoch 递增让宽度动画重播（首帧 epoch=0 免动画，见 `sidebar_width_anim`）
    pub fn toggle_value_sidebar(&mut self, cx: &mut Context<Self>) {
        let (edge, show) = next_right_edge_state(self.edge_bar_visible, self.show_value_sidebar);
        // 走「显示」方向时同样补默认页集合：这条路径是 DBeaver `CMD_TOGGLE_PANELS` 的等价物，
        // 关空过之后再显也不能留下空页签夹（见 restore_default_tabs_when_empty 的取证）
        if show {
            self.restore_default_tabs_when_empty();
        }
        self.edge_bar_visible = edge;
        self.show_value_sidebar = show;
        // 走到「收起」时最大化没有意义（不变量：最大化 ⇒ 面板展开），顺手复位
        if !show {
            self.panel_maximized = false;
        }
        self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
        // 【R5 可拖分栏】开合重新走 160ms 补间（拖拽期是直写，见 render_right_sidebar）
        self.right_sidebar_dragging = false;
        cx.notify();
    }

    /// 【R5 可拖分栏】拖右栏分栏边改尺寸（`delta` = 沿分栏法线的位移，像素，正向 = 变小）。
    ///
    /// 横排改宽度、竖排改高度，两条路径各自 clamp 到 `crate::ui` 的上下限；
    /// `right_sidebar_dragging` 置真让右栏接下来直写尺寸（不重播开合补间，否则手柄会黏手）。
    pub fn resize_right_sidebar(&mut self, delta: f32, cx: &mut Context<Self>) {
        if self.vertical_layout {
            self.right_sidebar_height =
                super::sidebars::clamp_right_sidebar_height(self.right_sidebar_height, delta);
        } else {
            self.right_sidebar_width =
                super::sidebars::clamp_right_sidebar_width(self.right_sidebar_width, delta);
        }
        self.right_sidebar_dragging = true;
        cx.notify();
    }

    /// 【R5 比例持久化】松手定稿：清拖拽标记，横排时交回当前宽度（像素）由宿主写全局偏好项。
    /// 返回 `None` = 本次不在拖拽（只是点了一下手柄）或竖排（高度没有对应偏好项），不需落盘。
    pub fn end_right_sidebar_resize(&mut self, cx: &mut Context<Self>) -> Option<Pixels> {
        if !self.right_sidebar_dragging {
            return None;
        }
        self.right_sidebar_dragging = false;
        cx.notify();
        // 竖排改的是高度：本仓没有与之对应的偏好项（宽度偏好是横向 px 口径，共用会串）
        (!self.vertical_layout).then_some(self.right_sidebar_width)
    }

    /// 【R5 布局方向切换】右栏在右侧 / 下方之间切换（对齐 DBeaver `toggleVerticalLayout`）：
    /// 只翻面板状态并清拖拽标记（切换瞬间的位移基准作废，否则下一次拖拽会跳一格）。
    pub fn toggle_vertical_layout(&mut self, cx: &mut Context<Self>) {
        self.vertical_layout = !self.vertical_layout;
        self.right_sidebar_dragging = false;
        cx.notify();
    }

    /// 【R5 面板最大化】收起 / 还原结果网格（对齐 DBeaver `togglePanelsMaximize`
    /// `ResultSetViewer.java:1551-1557` = `viewerSash.hideUp()/showUp()`）。
    ///
    /// 语义是**独占整块结果集区域**，不是「把右栏拖宽」：最大化期间网格不渲染、
    /// 右栏满宽（横排满宽 / 竖排满高），拖拽手柄也不挂（没有可拖的分栏了）。
    /// 面板总开关（`show_value_sidebar`）**保持原值不动**——还原后回到用户原本的开合态；
    /// 唯一的例外是「面板当前是关的」时进入最大化：面板关着就没有可最大化的东西，
    /// 这时先把面板展开（与 DBeaver 共用同一份 sash 可见性同一道理），再进入最大化。
    pub fn toggle_panel_maximize(&mut self, cx: &mut Context<Self>) {
        self.panel_maximized = !self.panel_maximized;
        if self.panel_maximized {
            // 不变量：最大化 ⇒ 面板可见（否则网格已收起、面板又不显示，整块区域空白）
            if !self.show_value_sidebar {
                self.show_value_sidebar = true;
                self.edge_bar_visible = true;
                self.right_sidebar_epoch = self.right_sidebar_epoch.wrapping_add(1);
            }
            // 最大化期间没有拖拽手柄可拖
            self.right_sidebar_dragging = false;
        }
        cx.notify();
    }

    /// 【R6 段大小】改「取下一段」的行数（对齐 DBeaver `resultSetSize` 的 modify 回调：
    /// `>0` 才写、`< MIN_SEGMENT_SIZE` 抬到下限）。只影响后续取段，不改已加载行。
    pub fn set_segment_rows(&mut self, rows: usize, cx: &mut Context<Self>) {
        let rows = rows.max(crate::ui::RESULT_SEGMENT_ROWS_MIN);
        if rows == self.segment_rows {
            return;
        }
        self.segment_rows = rows;
        cx.notify();
    }

    /// 【R6 段大小】宿主构造期注入持久化值：状态落位 + 输入框显示该值
    /// （过去只有占位 `1000`，持久化成 `500` 时框里仍是空 → 看不到真值）。
    pub fn set_segment_rows_initial(
        &mut self,
        rows: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = rows.max(crate::ui::RESULT_SEGMENT_ROWS_MIN);
        self.segment_rows = rows;
        // `set_value` 内部临时 `emit_events = false`：不会把注入当成用户输入再入队一次
        let input = self.segment_input.clone();
        input.update(cx, |state, cx| state.set_value(rows.to_string(), window, cx));
        cx.notify();
    }

    /// 【R5 比例持久化】初值注入：面板构造后由宿主把全局偏好项（px）写进来。
    /// 复用 `clamp_right_sidebar_width`（`dx = 0` 时它就是"夹取当前值"），
    /// 保证与拖拽路径同一套上下限，不在这里再写一份常量比较。
    pub fn set_right_sidebar_width_px(&mut self, width_px: f32, cx: &mut Context<Self>) {
        self.right_sidebar_width =
            super::sidebars::clamp_right_sidebar_width(px(width_px), 0.);
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

    /// 【P1-3】当前列宽快照（列名 → 像素宽；宿主保存会话时读取）
    pub fn column_widths(&self) -> Vec<(String, f32)> {
        self.column_widths.clone()
    }

    /// 【P1-3】注入持久化列宽（会话恢复时宿主调）：只写快照，落到 delegate 由下一次
    /// `sync` 在 `set_data` 之后统一回填（没数据时先别写一批对不上列的宽度）
    pub fn set_column_widths(&mut self, widths: Vec<(String, f32)>) {
        self.column_widths = widths;
    }

    /// 【列布局落盘】当前隐藏列快照（列名；宿主保存会话时读取）
    pub fn column_hidden(&self) -> Vec<String> {
        self.column_hidden.clone()
    }

    /// 【列布局落盘】注入持久化隐藏列（会话恢复时宿主调）：与列宽同口径，只写快照，
    /// 落到 delegate 由下一次 `sync` 在 `set_data` 之后统一回填
    pub fn set_column_hidden(&mut self, hidden: Vec<String>) {
        self.column_hidden = hidden;
    }

    /// 【列布局落盘】当前冻结列快照（列名；宿主保存会话时读取）
    pub fn column_frozen(&self) -> Vec<String> {
        self.column_frozen.clone()
    }

    /// 【列布局落盘】注入持久化冻结列（会话恢复时宿主调）
    pub fn set_column_frozen(&mut self, frozen: Vec<String>) {
        self.column_frozen = frozen;
    }

    /// 【结果集 #296】按持久化列名序重排一份新结果的列与行（列名匹配，未匹配列
    /// 保持数据序附加在末尾——用户拖过的列还在就保持，新列不猜）。
    pub(crate) fn apply_column_order(
        &self,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    ) -> (Vec<String>, Vec<Vec<String>>) {
        reorder_by(&self.column_order, columns, rows)
    }

    pub fn set_type_style(&mut self, prefs: TypeStylePrefs, cx: &mut Context<Self>) {
        self.type_style = prefs.clone();
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
        // 【P2-1】换了格（或清选）→ hex 转储回第 1 页，不把上一格翻到的页号带过去
        self.hex_page = 0;
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

    /// 【P2-1】值查看器 hex 转储翻页（页号 0 基）。
    ///
    /// 纯视图态，不重算值：只改页号并重渲（`render_hex_dump` 按页号取本页行）。
    /// 越界不在这里夹（渲染期还会按总页数夹一次）——但同页重复点击不白刷一帧。
    pub fn set_hex_page(&mut self, page: usize, cx: &mut Context<Self>) {
        if self.hex_page == page {
            return;
        }
        self.hex_page = page;
        cx.notify();
    }

    /// 【值查看器】选中格变化时重建 JSON 展开树（事件路径：解析 + `set_items`；
    /// render 只摆 Tree，不解析 JSON——render 纯读红线）。
    /// 非 JSON 值 → 清空树；清选 → 空。
    ///
    /// 同时重建图片（P2-2）：选中 BLOB 且魔数认出格式时，把 hex 解成 `Arc<Image>`
    /// 放 `value_image`。`Image::from_bytes` 只做内存构造（真解码在 `img` 的 layout
    /// 期按内容 id 走资产缓存），所以放在事件路径上，render 端零解析。
    pub fn rebuild_value_tree(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let (items, image) = match self.selection_pos {
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
                    ViewerMode::Json => (json_tree_items(&value).unwrap_or_default(), None),
                    ViewerMode::Image { format, .. } => (
                        Vec::new(),
                        image_format_of(format).map(|format| {
                            Arc::new(Image::from_bytes(format, hex_to_bytes(&value)))
                        }),
                    ),
                    _ => (Vec::new(), None),
                }
            }
            None => (Vec::new(), None),
        };
        self.value_image = image;
        self.json_tree
            .update(cx, |state, cx| state.set_items(items, cx));
    }

    /// 【P2-3】清单多选集合镜像（delegate 经 `ColumnSelectionChanged` 回传）：
    /// 面板只存一份给标题行「已选 N 列」与批量按钮用，选集权威仍在 delegate。
    pub fn set_multi_selected(&mut self, columns: Vec<usize>, cx: &mut Context<Self>) {
        if self.multi_selected == columns {
            return;
        }
        self.multi_selected = columns;
        cx.notify();
    }

    /// 【P2-3】批量显隐（列浏览器多选批量动作）：直接改网格 delegate（逐列复用单列
    /// 接口，隐藏仍有「至少 1 列可见」底线），再 notify 让清单的隐藏标记跟着刷新。
    /// 【列布局落盘】改完即取快照并推落库请求（与单列眼睛同一条通道）。
    pub fn set_columns_hidden(
        &mut self,
        columns: Vec<usize>,
        hidden: bool,
        cx: &mut Context<Self>,
    ) {
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().set_columns_hidden(&columns, hidden);
            state.refresh(cx);
        });
        self.snapshot_column_layout(cx);
        cx.notify();
    }

    /// 【筛选设置 · 列约束】工具条「全部显示」（对齐 DBeaver `dialog_toolbar_show_all`，
    /// `FilterSettingsDialog.java:304-310`：把每条约束 `setVisible(true)`）
    pub fn show_all_columns(&mut self, cx: &mut Context<Self>) {
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().show_all_columns();
            state.refresh(cx);
        });
        self.snapshot_column_layout(cx);
        cx.notify();
    }

    /// 【筛选设置 · 列约束】工具条「全部不显示」（DBeaver `dialog_toolbar_show_none`，
    /// `:311-317`：全部 `setVisible(false)`）。
    ///
    /// **口径差异（有意）**：本仓委托的逐列 `hide_column` 有「至少留 1 列可见」底线
    /// （隐藏到空表没有意义），因此这里会保留最后那一列——与 DBeaver 允许全隐不同。
    pub fn hide_all_columns(&mut self, cx: &mut Context<Self>) {
        let columns: Vec<usize> = {
            let delegate = self.grid.read(cx).delegate();
            (0..delegate.columns().len()).collect()
        };
        self.set_columns_hidden(columns, true, cx);
    }

    /// 【筛选设置 · 列约束】工具条「重置」（DBeaver `dialog_toolbar_reset`，`:318-325`：
    /// `dataFilter.reset()` 后重置两页文本）。
    ///
    /// 本仓只重置**列布局**这一支（显示全部 + 取消全部冻结），因为 where/order 的权威
    /// 分别在筛选框与自定义排序输入里，且列序恢复默认序需要组件侧的列序接口（尚未接 UI，
    /// 见 `result-panel.md §8.72`）。
    pub fn reset_column_layout(&mut self, cx: &mut Context<Self>) {
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().reset_column_layout();
            state.refresh(cx);
        });
        self.snapshot_column_layout(cx);
        cx.notify();
    }

    /// 【筛选设置 · 列约束】工具条「移顶 / 上移 / 下移 / 移底」（DBeaver `moveSelectedItems`，
    /// `FilterSettingsDialog.java:278-293`：作用在**选中项**上）。
    ///
    /// 本仓的「选中项」= 网格当前选中格所在列（`selected_col`，数据列序）；没有选中格
    /// 或该列被隐藏时不动作（与 DBeaver 按钮默认 `setEnabled(false)` 同口径）。
    /// `target = Some(0)` 移顶、`Some(last)` 移底、`None` 按 `step` 上移 / 下移一列。
    pub fn move_selected_column(&mut self, direction: crate::ports::ColumnMove, cx: &mut Context<Self>) {
        let Some(data) = self.selected_col else {
            return;
        };
        let grid = self.grid.clone();
        let moved = grid.update(cx, |state, cx| {
            use crate::ports::ColumnMove;
            let delegate = state.delegate_mut();
            let visible = delegate.visible_column_count();
            let Some(view) = delegate.view_column(data) else {
                return false;
            };
            let target = match direction {
                ColumnMove::Top => 0,
                ColumnMove::Up => view.saturating_sub(1),
                ColumnMove::Down => view + 1,
                ColumnMove::Bottom => visible.saturating_sub(1),
            };
            if target == view || visible == 0 {
                return false;
            }
            delegate.move_display_column(data, target);
            state.refresh(cx);
            true
        });
        if moved {
            self.snapshot_column_order(cx);
            cx.notify();
        }
    }

    /// 【筛选设置 · 列约束】工具条「排序」（列名 A→Z；DBeaver `ALPHA_SORTER`，
    /// `FilterSettingsDialog.java:295-302`）
    pub fn sort_columns_by_name(&mut self, cx: &mut Context<Self>) {
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            state.delegate_mut().sort_display_order_by_name();
            state.refresh(cx);
        });
        self.snapshot_column_order(cx);
        cx.notify();
    }

    /// 【工具栏配置】状态栏命令项显隐（纯视图状态；对齐 DBeaver 工具条配置的
    /// 「勾选 = 显示」，`undo` / `redo` / `fetch.page` 默认不勾）
    pub fn set_status_command_visible(
        &mut self,
        command: crate::ports::StatusCommand,
        visible: bool,
        cx: &mut Context<Self>,
    ) {
        if visible {
            self.hidden_status_commands.remove(&command);
        } else {
            self.hidden_status_commands.insert(command);
        }
        cx.notify();
    }

    /// 【工具栏配置】当前被隐藏的命令项（渲染与用例读它）
    pub fn hidden_status_commands(
        &self,
    ) -> &std::collections::BTreeSet<crate::ports::StatusCommand> {
        &self.hidden_status_commands
    }

    /// 【工具栏配置落盘回读】整批替换隐藏集合（宿主在面板创建时按全局偏好注入）。
    ///
    /// 与 [`Self::set_status_command_visible`] 的分工：那条是「用户点了某一项」的
    /// 增量语义（也负责重绘），这条是「从偏好文件恢复」的整批语义；值相同时直接返回
    /// （避免每份结果集创建都白 notify 一次）。
    pub fn set_hidden_status_commands(
        &mut self,
        hidden: std::collections::BTreeSet<crate::ports::StatusCommand>,
        cx: &mut Context<Self>,
    ) {
        if self.hidden_status_commands == hidden {
            return;
        }
        self.hidden_status_commands = hidden;
        cx.notify();
    }

    /// 【列序落盘】把 delegate 的显示序取回面板快照；变了就推 `ColumnOrderChanged`
    /// （与拖拽 `MoveColumn` 同一条通道，宿主据此落库）
    fn snapshot_column_order(&mut self, cx: &mut Context<Self>) {
        let order = self.grid.read(cx).delegate().display_order();
        if order != self.column_order {
            self.column_order = order.clone();
            self.queue.push(ResultRequest::ColumnOrderChanged { order });
        }
    }

    /// 【列布局落盘】把 delegate 的隐藏 / 冻结集合取回面板快照；有变化就推一条
    /// 「该存了」（宿主在同一次事件里 `save_session_now`）。
    ///
    /// 交互路径统一走这里：单列眼睛（`ToggleColumnVisibility` 的宿主分支）、批量显隐、
    /// 冻结切换。只在快照真的变了才推，避免一次点击触发两次写库。
    pub fn snapshot_column_layout(&mut self, cx: &mut Context<Self>) {
        let (hidden, frozen) = {
            let delegate = self.grid.read(cx).delegate();
            (
                delegate.hidden_column_snapshot(),
                delegate.frozen_column_snapshot(),
            )
        };
        let changed = hidden != self.column_hidden || frozen != self.column_frozen;
        self.column_hidden = hidden;
        self.column_frozen = frozen;
        if changed {
            self.queue.push(ResultRequest::ColumnLayoutChanged);
        }
    }

    /// 【列浏览器】清空多选集合（批量按钮「清空选择」；面板先写镜像，
    /// 再让 delegate 跟上，两边不会因为顺序不同而各说各话）
    pub fn clear_multi_selected(&mut self, cx: &mut Context<Self>) {
        if self.multi_selected.is_empty() {
            return;
        }
        self.multi_selected.clear();
        let browser = self.browser_list.clone();
        browser.update(cx, |list, cx| {
            list.delegate_mut().set_selected_columns(Vec::new());
            cx.notify();
        });
        cx.notify();
    }

    /// 【列浏览器】清单单击选中一列（数据列序）：写清单选中态 + 网格列选中高亮联动。
    /// 清单选中 ≠ 网格焦点——这里只把网格列选中用于高亮，不回写 `selection_pos`
    /// （避免清单选中把值详情也拽走；值详情仍只跟网格格焦点）。
    pub fn select_column(&mut self, data_col: usize, cx: &mut Context<Self>) {
        self.selected_col = Some(data_col);
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            if let Some(view_col) = state.delegate().view_column(data_col) {
                // 【第 75 批顺手修】`set_selected_col` 收的是**组件列**（0 是行号槽，
                // 与 `table_impl.rs` 的 `data_column(col_ix - 1)` 同一口径），原来直接喂
                // `view_col` 会让网格高亮左移一格（第 0 列时整列选中落在行号槽上）。
                state.set_selected_col(view_col + 1, cx);
            }
            state.refresh(cx);
        });
        cx.notify();
    }

    /// 【第 76 批】`CTRL+ALT+C` 选中当前格所在的整列
    /// （DBeaver `CMD_SELECT_COLUMNS`，`SpreadsheetCommandHandler.java:106-118`：
    /// 把选中列的所有行选成单元格，焦点格复位但焦点列保留）。
    ///
    /// 本仓选区是单区域的（组件只有 Row / Column / Cell 三态），等价表达 = **整列选中**；
    /// 当前格在行号槽 / 没有当前格时返回 `false`（宿主补提示）。
    pub fn select_current_column(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((_row, col_ix)) = self.selection_pos else {
            return false;
        };
        // 组件列 0 是行号槽（`table_impl.rs` 的 `data_column(col_ix - 1)` 口径）
        if col_ix == 0 {
            return false;
        }
        let data_col = self
            .grid
            .read(cx)
            .delegate()
            .data_column_checked(col_ix - 1);
        let Some(data_col) = data_col else {
            return false;
        };
        self.select_column(data_col, cx);
        true
    }

    /// 【第 76 批】`CTRL+ALT+R` 选中当前格所在的整行
    /// （DBeaver `CMD_SELECT_ROWS`，`SpreadsheetCommandHandler.java:119-131`：
    /// 把选中行的所有列选成单元格，焦点格复位但焦点行保留）。
    ///
    /// 本仓等价表达 = 组件整行选中（`set_selected_row`，delegate 据此整行铺色）；
    /// 同时把状态行那口「已选第 N 行」写进去（1 基）——行级三钮 / 「修改」钮都读
    /// `status.selected_row`（`row_target_state`），不写的话按完键位仍全是灰的。
    pub fn select_current_row(&mut self, cx: &mut Context<Self>) -> bool {
        let Some((row, _col)) = self.selection_pos else {
            return false;
        };
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| state.set_selected_row(row, cx));
        self.set_selected_row(Some(row + 1), cx);
        true
    }

    /// 【列浏览器】清单双击定位：网格滚到该列首行并选中（列不可见 = 隐藏列，不动作）。
    pub fn locate_column(&mut self, data_col: usize, cx: &mut Context<Self>) {
        self.selected_col = Some(data_col);
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| {
            if let Some(view_col) = state.delegate().view_column(data_col) {
                // 【第 75 批顺手修】同 `select_column`：组件列 = 显示列 + 1（0 是行号槽）。
                state.set_selected_cell(0, view_col + 1, cx);
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
        self.toolbar.is_some() || self.script_card.is_some() || self.error_card.is_some()
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

    /// 【第 69 批】行级动作的目标与可用态（单一实现）
    ///
    /// 状态栏三钮与键盘入口（`ALT+INSERT` / `CTRL+ALT+INSERT` / `ALT+DELETE` /
    /// `CTRL+D` / `CTRL+ALT+D`）都调这一个：当前行取状态行 1 基记录折 0 基，行删除态
    /// 读网格委托的镜像，可用态走纯函数 [`crate::grid::row_target_state`]。
    /// `render` 是纯读路径，宿主 handler 也只需要 `&App`，两边不会各自算一套。
    pub fn row_target_state(&self, cx: &App) -> RowTargetState {
        let selected_view_row = self
            .status
            .as_ref()
            .and_then(|status| status.selected_row)
            .map(|row| row.saturating_sub(1));
        let row_deleted = selected_view_row
            .is_some_and(|row| self.grid.read(cx).delegate().is_deleted_row(row));
        crate::grid::row_target_state(self.editable, selected_view_row, row_deleted)
    }

    /// 【第 69 批】邻行复制的源行（视图坐标；`None` = 越界，没有可复制的行）
    ///
    /// 与宿主 `copy_row_values_from_adjacent` 的「源行越界静默不动作」同一判据，
    /// 但宿主需要区分提示文案（首行没有上一行 / 末行没有下一行），所以先把源行
    /// 翻译成网格真值再回答。
    pub fn adjacent_row_view(&self, row: usize, above: bool, cx: &App) -> Option<usize> {
        let source = if above {
            row.checked_sub(1)
        } else {
            row.checked_add(1)
        };
        source.filter(|view| self.grid.read(cx).delegate().reference_of(*view).is_some())
    }

    /// 当前呈现方式（网格 / 文本）——供测试断言左竖条按钮点击链路
    pub fn presentation(&self) -> Presentation {
        self.presentation
    }

    /// 当前选中结果集的 SQL（「刷新」重跑它；`None` = 无可重跑）
    pub fn sql(&self) -> Option<&str> {
        self.result_sql.as_deref()
    }

    /// 【R6 段大小】「取下一段」一次取多少行（宿主下发给 `ExecTarget::Segment { limit }`）
    pub fn segment_rows(&self) -> usize {
        self.segment_rows
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

    /// 【选中统计】选中计数增量更新（`TableEvent::SelectCell/SelectRow/ClearSelection`
    /// 事件路径调用；计数由编辑器算好传入——整行选中时列数 / 格数 = 可见列数）。
    ///
    /// 与 `set_selected_row` 同路：sync 会全量覆写 status，这里只补事件之间的增量。
    pub fn set_selected_counts(
        &mut self,
        rows: usize,
        columns: usize,
        cells: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(status) = self.status.as_mut() {
            status.selected_rows = rows;
            status.selected_columns = columns;
            status.selected_cells = cells;
        }
        cx.notify();
    }

    /// 【选中统计】注入三项开关（宿主构造期读偏好 / 设置页切换后同步）。
    /// 同时把镜像同步进网格 delegate（右键菜单的「✓」态读它）。
    pub fn set_selection_stat_prefs(
        &mut self,
        prefs: crate::grid::SelectionStatPrefs,
        cx: &mut Context<Self>,
    ) {
        if self.selection_stat_prefs == prefs {
            return;
        }
        self.selection_stat_prefs = prefs;
        self.grid
            .update(cx, |state, _cx| state.delegate_mut().set_selection_stat_prefs(prefs));
        cx.notify();
    }

    /// 【选中统计】当前三项开关（宿主落盘时读走）。
    pub fn selection_stat_prefs(&self) -> crate::grid::SelectionStatPrefs {
        self.selection_stat_prefs
    }

    /// 【选中统计】切换三项之一（数据格右键 layout 段经队列回调；对齐 DBeaver
    /// `ToggleSelectionStatAction.run` 的「改偏好 + 重算统计」两步）。
    pub fn toggle_selection_stat(
        &mut self,
        kind: crate::grid::SelectionStatKind,
        on: bool,
        cx: &mut Context<Self>,
    ) {
        use crate::grid::SelectionStatKind;
        let target = match kind {
            SelectionStatKind::Rows => &mut self.selection_stat_prefs.rows,
            SelectionStatKind::Columns => &mut self.selection_stat_prefs.columns,
            SelectionStatKind::Cells => &mut self.selection_stat_prefs.cells,
        };
        if *target == on {
            return;
        }
        *target = on;
        // 网格镜像同步：菜单下次打开要标对「✓」
        let prefs = self.selection_stat_prefs;
        self.grid
            .update(cx, |state, _cx| state.delegate_mut().set_selection_stat_prefs(prefs));
        cx.notify();
    }

    /// 【筛选条显隐】注入偏好（宿主构造期调用；与 `set_selection_stat_prefs` 同口径）。
    pub fn set_show_filter_panel(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_filter_panel == show {
            return;
        }
        self.show_filter_panel = show;
        self.grid.update(cx, |state, _cx| {
            state.delegate_mut().set_show_filter_panel(show);
        });
        cx.notify();
    }

    /// 【筛选条显隐】当前是否显示筛选条（宿主落盘设置时读它）。
    pub fn show_filter_panel(&self) -> bool {
        self.show_filter_panel
    }

    /// 【筛选条显隐】切换（数据格右键 layout 段经队列回调；对齐 DBeaver
    /// `ToggleFilterPanelAction.run:5434-5441` 的「改偏好 + setVisible」两步）。
    ///
    /// 收起筛选条时顺带收掉展开态：展开条是筛选区的延伸，整条都不显示时不该留一条
    /// 顶宽输入框在外面（DBeaver 整块 setVisible 也把内部展开区一起藏掉）。
    pub fn toggle_filter_panel(&mut self, open: bool, cx: &mut Context<Self>) {
        if !open {
            self.filter_expanded = false;
        }
        self.set_show_filter_panel(open, cx);
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

    // ── 状态历史（对齐 DBeaver `HistoryStateItem` / `setNewState` / `navigateHistory`） ──

    /// 【状态历史】记一条状态（`sync` 拿到新结果 SQL 时调用；也可测试直接调）。
    ///
    /// 对齐 DBeaver `setNewState`（`ResultSetViewer.java:2419-2442`）：
    /// - 同一个 SQL 已经在历史里 → **把位置挪过去**（去重、不追加，`:2420-2426`）
    /// - 否则先**截断当前位置之后的所有项**（浏览器式：从中间走了新路，前向历史作废，`:2432`），
    ///   再追加到末尾，位置 = 末位
    ///
    /// 口径：一份状态 = 一份结果 SQL（本仓重查通道就是 `ExecTarget::Statement(sql)`）；
    /// 空 SQL（写语句 / 无结论）不入历史。
    pub(crate) fn note_state_sql(&mut self) {
        let Some(raw) = self.result_sql.as_deref() else {
            return;
        };
        let sql = raw.trim().to_string();
        if sql.is_empty() {
            return;
        }
        if let Some(index) = self.state_history.iter().position(|item| *item == sql) {
            self.history_pos = index;
            return;
        }
        // 首次入栈时历史为空：`truncate(0)` 无动作，等价于「直接追加」
        let keep = if self.state_history.is_empty() {
            0
        } else {
            self.history_pos + 1
        };
        self.state_history.truncate(keep);
        self.state_history.push(sql);
        self.history_pos = self.state_history.len().saturating_sub(1);
    }

    /// 【状态历史】能不能后退（DBeaver：`position > 0`，`ResultSetViewer.java:456-475`）
    pub fn history_can_back(&self) -> bool {
        !self.state_history.is_empty() && self.history_pos > 0
    }

    /// 【状态历史】能不能前进（DBeaver：`position < size - 1`）
    pub fn history_can_forward(&self) -> bool {
        self.history_pos + 1 < self.state_history.len()
    }

    /// 【状态历史】某一方向的目标状态：`delta = -1` 后退一格、`+1` 前进一格。
    /// 返回 `(目标下标, 该状态 SQL)`；方向到头 / 空历史 = `None`。
    /// 宿主拿到它之后自己重跑 SQL，再把位置交回 [`Self::set_history_pos`]。
    pub fn history_target(&self, delta: i32) -> Option<(usize, String)> {
        let pos = if delta < 0 {
            self.history_pos.checked_sub(1)?
        } else {
            self.history_pos + 1
        };
        self.state_history.get(pos).cloned().map(|sql| (pos, sql))
    }

    /// 【状态历史】写下当前位置（宿主执行完目标状态后回调；纯面板视图态）
    pub fn set_history_pos(&mut self, pos: usize, cx: &mut Context<Self>) {
        self.history_pos = pos;
        cx.notify();
    }

    /// 【状态历史】历史条数（测试断言）
    pub fn history_len(&self) -> usize {
        self.state_history.len()
    }

    /// 【状态历史】当前位置（测试断言）
    pub fn history_pos(&self) -> usize {
        self.history_pos
    }

    /// 【状态历史】按下标取状态 SQL（箭头菜单里选中某一条时，宿主用它重查）
    pub fn history_sql_at(&self, pos: usize) -> Option<String> {
        self.state_history.get(pos).cloned()
    }

    /// 【状态历史】某方向**全部可达状态**（对齐 DBeaver 的箭头菜单
    /// `ResultSetViewer.java:1456-1481`）：后退方向 = 当前位置之前的全部项、
    /// 前进方向 = 之后的全部项；两个方向都**近的在前**（菜单从上到下离用户越来越远）。
    pub fn history_menu_items(&self, delta: i32) -> Vec<(usize, String)> {
        let positions: Vec<usize> = if delta < 0 {
            (0..self.history_pos).rev().collect()
        } else {
            ((self.history_pos + 1)..self.state_history.len()).collect()
        };
        positions
            .into_iter()
            .filter_map(|pos| self.state_history.get(pos).cloned().map(|sql| (pos, sql)))
            .collect()
    }

    /// 【状态历史】某条状态的 tooltip 文案（按钮悬停提示 / 测试断言）：
    /// 取该方向要去的状态，没有就到头或用「先有结果才能后退」这类说明
    ///
    /// 对齐 DBeaver：有目标时 tooltip = `describeState() + " (" + 命令描述 + ")"`
    /// （`ResultSetFilterPanel.java:456-475`，命令描述取自
    /// `IWorkbenchCommandConstants.NAVIGATE_BACKWARD_HISTORY` /
    /// `NAVIGATE_FORWARD_HISTORY`，即「后退历史」/「前进历史」）；DBeaver 在按钮禁用时
    /// 不刷新 tooltip，本仓仍给一句说明（禁用态读者看不到，没有副作用）。
    pub fn history_hint(&self, delta: i32) -> String {
        let command = if delta < 0 {
            "后退历史"
        } else {
            "前进历史"
        };
        match self.history_target(delta) {
            Some((_, sql)) => format!("{} ({command})", state_label(&sql)),
            None if self.state_history.is_empty() => "还没有可回退的状态".to_string(),
            None if delta < 0 => "已经在最早的状态".to_string(),
            None => "已经在最新的状态".to_string(),
        }
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

/// 【右栏页内「视图菜单」】条目种类（纯函数给清单，测试直接覆盖；`PopupMenu` 内部不可读）。
///
/// 对齐 DBeaver：**每一页**的面板工具条右端都挂一颗「视图菜单」（`addDefaultPanelActions`，
/// `ResultSetViewer.java:1590-1614`，图标 `IMG_LCL_VIEW_MENU`），菜单内容由 `fillPanelsMenu()`
/// 生成（`:1559-1588`）：全部面板的 CHECK 条目 → 分隔 → `CMD_TOGGLE_MAXIMIZE` →
/// `CMD_ACTIVATE_PANELS` → 分隔 → `CMD_TOGGLE_PANELS`（面板总开关）→ `CMD_TOGGLE_LAYOUT`。
///
/// 顺序照 `fillPanelsMenu`（`ResultSetViewer.java:1559-1588`）：六页 → 分隔 →
/// `CMD_TOGGLE_MAXIMIZE` → `CMD_ACTIVATE_PANELS` → 分隔 → `CMD_TOGGLE_PANELS` → `CMD_TOGGLE_LAYOUT`。
///
/// 本仓口径「能力没有就不摆」：`ACTIVATE_PANELS`（把焦点交给面板）还没有落地端口，故不列
/// （不造点了没反应的条目）；`MAXIMIZE` 本批已落地真端口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelViewItem {
    /// 一页（勾选态 = 当前页；点击切到该页）
    Page(RightTab),
    /// 分隔线（DBeaver 在两段之间插 `Separator`）
    Separator,
    /// 面板总开关（≈ `CMD_TOGGLE_PANELS`）
    PanelsToggle,
    /// 布局方向（横排 / 竖排，≈ `CMD_TOGGLE_LAYOUT`）
    LayoutToggle,
    /// 面板最大化（≈ `CMD_TOGGLE_MAXIMIZE`：收起结果网格，让面板占满结果集区域）
    MaximizeToggle,
}

/// 【筛选设置 · 存档】换进来一份结果时要不要套用存档（纯函数，投影路径读）。
///
/// 四条判据同时成立才套，任一条不成立就原样不动：
/// 1. 存档里的筛选词**非空**——空存档等于「没有筛选可恢复」。DBeaver 同口径：
///    `ResultSetUtils.restoreDataFilter:174-189` 只在 `dataFilter.hasFilters()` 时返回；
/// 2. 存档记了它属于哪条 SQL，且非空（本仓按 SQL 收窄，见 [`SavedFilterSetting::sql`]）；
/// 3. 当前这份结果的 SQL 与存档一致——换了一条查询就别套别人的筛选；
/// 4. 当前**还没有**筛选词——用户手写的词优先，不能被存档顶掉。
pub(crate) fn saved_filter_applies(
    saved: &SavedFilterSetting,
    result_sql: Option<&str>,
    current_filter: &str,
) -> bool {
    if saved.filter.trim().is_empty() || saved.sql.trim().is_empty() {
        return false;
    }
    if !current_filter.trim().is_empty() {
        return false;
    }
    let Some(sql) = result_sql else {
        return false;
    };
    sql.trim() == saved.sql.trim()
}

/// 右栏六页的**规范顺序**（页签条的左右顺序 / 索引映射 / 视图菜单条目共用一份）。
///
/// 对齐 DBeaver 的 `CTabFolder panelFolder`（`ResultSetViewer.java:1430-1478`）：右栏是**一个**
/// 页签夹，六页都作为页签常驻在里面（`addPanel` 里 `new CTabItem(panelFolder, SWT.CLOSE)`），
/// 点页签即 `setActivePanel`；顺序就是 `availablePanels` 的注册顺序。
pub(crate) fn right_tabs() -> [RightTab; 6] {
    [
        RightTab::Browser,
        RightTab::Value,
        RightTab::Stats,
        RightTab::Group,
        RightTab::Reference,
        RightTab::ExecSql,
    ]
}

/// 【页内工具条】该页是否在「工具条与正文之间」画一条水平分隔线。
///
/// 对齐 DBeaver `IResultSetPanel.needsSeparator()`：`ResultSetViewer.addPanel:1345-1347`
/// 只在面板声明 `true` 时才 `UIUtils.createLabelSeparator(panelComposite, SWT.HORIZONTAL)`；
/// 全仓只有 `ValueViewerPanel.java:171-173` 返回 `true`，其余面板继承
/// `ResultSetPanelBase.java:26-28` 的 `false`（`MetaDataPanel` / `AggregateColumnsPanel` /
/// `GroupingPanel` / `ReferencesPanel` 均未覆写）。
pub(crate) fn page_needs_separator(tab: RightTab) -> bool {
    matches!(tab, RightTab::Value)
}

/// 【右缘竖条 / 视图菜单】点某一页时是否「关掉面板」而不是「切页」。
///
/// 【页签条】已启用集合内某页的下标（`TabBar::selected_index` 用；不在集合里 → 0）。
pub(crate) fn strip_index(open: &[RightTab], tab: RightTab) -> usize {
    open.iter().position(|existing| *existing == tab).unwrap_or(0)
}

/// 【页签条】下标 → 已启用集合内的页（`TabBar::on_click` 回调用；越界兜底默认页）。
pub(crate) fn tab_at_strip_index(open: &[RightTab], index: usize) -> RightTab {
    open.get(index).copied().unwrap_or(RightTab::Browser)
}

/// 【页签条】每个页签内侧那颗 × 的稳定选择器（`debug_selector` / 测试定位用）。
///
/// 必须**逐页不同**：六颗按钮同时存在，共用一个 id 会让 `debug_bounds` 只命中第一个、
/// 也会造成 a11y 重复 id（`gpui-kit` 0.9 已知坑）。返回 `&'static str` 是为了让
/// `TestAppContext::debug_bounds` 能按字面量定位。
pub(crate) fn tab_close_selector(tab: RightTab) -> &'static str {
    match tab {
        RightTab::Browser => "editor-result-right-tab-close-browser",
        RightTab::Value => "editor-result-right-tab-close-value",
        RightTab::Stats => "editor-result-right-tab-close-stats",
        RightTab::Group => "editor-result-right-tab-close-group",
        RightTab::Reference => "editor-result-right-tab-close-reference",
        RightTab::ExecSql => "editor-result-right-tab-close-execsql",
    }
}

/// 【关闭一页】关掉某页后当前页落到哪：**规范序里紧随其后**的剩余页，没有则取最后一个。
///
/// `remaining` 必须是规范序（`open_tabs` 的维护方式保证）。对齐 DBeaver 的
/// `CTabFolder` 行为：关掉选中的页签后焦点落到相邻页签，而不是回到第一页。
pub(crate) fn neighbor_after_close(remaining: &[RightTab], closed: RightTab) -> RightTab {
    let canonical = right_tabs();
    let closed_pos = canonical
        .iter()
        .position(|candidate| *candidate == closed)
        .unwrap_or(0);
    remaining
        .iter()
        .find(|candidate| {
            canonical
                .iter()
                .position(|tab| *tab == **candidate)
                .unwrap_or(0)
                > closed_pos
        })
        .or_else(|| remaining.last())
        .copied()
        .unwrap_or(RightTab::Browser)
}

/// 「视图菜单」条目清单（顺序照 `fillPanelsMenu:1559-1588`）：
/// 六页 → 分隔 → 最大化 → 分隔 → 面板总开关 → 布局方向
pub(crate) fn panel_view_menu_items() -> Vec<PanelViewItem> {
    let mut items: Vec<PanelViewItem> = right_tabs()
        .into_iter()
        .map(PanelViewItem::Page)
        .collect();
    items.push(PanelViewItem::Separator);
    // `CMD_TOGGLE_MAXIMIZE` 在原版里紧跟第一条分隔线（`:1578`），`CMD_ACTIVATE_PANELS` 之后
    // 还有一条分隔线（`:1582`）再放「面板总开关 / 布局方向」——本仓没有 actiave-panels 端口，
    // 但两条分隔线照留，菜单段位与那边一致。
    items.push(PanelViewItem::MaximizeToggle);
    items.push(PanelViewItem::Separator);
    items.push(PanelViewItem::PanelsToggle);
    items.push(PanelViewItem::LayoutToggle);
    items
}

/// 右栏页签的标题文案（页签条、视图菜单、竖条 tooltip 共用一份，避免三处各写一遍）
pub(crate) fn right_tab_label(tab: RightTab) -> &'static str {
    match tab {
        RightTab::Browser => "列浏览器",
        RightTab::Value => "值详情",
        RightTab::Stats => "计算",
        RightTab::Group => "分组",
        RightTab::Reference => "引用",
        RightTab::ExecSql => "执行SQL",
    }
}

/// 右栏页签的图标（与 `right_tab_label` 同源；竖条 / 页签条 / 视图菜单共用）
pub(crate) fn right_tab_icon(tab: RightTab) -> IconName {
    match tab {
        RightTab::Browser => IconName::Table2,
        RightTab::Value => IconName::Eye,
        RightTab::Stats => IconName::Sigma,
        RightTab::Group => IconName::Funnel,
        RightTab::Reference => IconName::Link,
        RightTab::ExecSql => IconName::Braces,
    }
}

/// 右栏页签的**提示文案**（对齐 DBeaver `addPanel` 里
/// `panelTab.setToolTipText(panelDescriptor.getDescription())`，`ResultSetViewer.java:1361`）。
///
/// 前五条按 `OSGI-INF/l10n/bundle.properties:15-24` 的 `panel.*.description` 逐条译：
/// `value-view` = Results cell value viewer、`column-aggregate` = Result cells calculations
/// (SUM, MAX, AVG, etc)、`results-metadata` = ResultSet metadata description、
/// `results-grouping` = Results grouping (GROUP BY)、
/// `results-references` = One or more rows referencing current row(-s)。
/// 「执行SQL」是本仓独有页（DBeaver 无对应面板），按本仓实能做描述。
pub(crate) fn right_tab_description(tab: RightTab) -> &'static str {
    match tab {
        RightTab::Browser => "结果集元数据说明（列清单 / 类型 / 注释，可筛可搜索）",
        RightTab::Value => "结果单元格值查看器（文本 / 十六进制转储，可保存至文件）",
        RightTab::Stats => "结果单元格计算（非空 / NULL / 最小 / 最大 / 均值 / 合计）",
        RightTab::Group => "结果分组（按选中列做 GROUP BY 计数）",
        RightTab::Reference => "引用当前行的其他行（外键清单 / 查看引用行）",
        RightTab::ExecSql => "面板内 SQL 草稿与执行入口",
    }
}

/// 【计算行数】这颗钮能不能点（纯函数）。
///
/// 两个条件：
/// - `has_data`：对齐 DBeaver `rowCountLabel` 的 `isActionEnabled = hasData()`；
/// - `is_analysis`：**本地分析结果不能算**——它跑在桥接过来的那份行上、临时表早就不在了，
///   重跑原 SQL 只会报错。与其给一颗点了必错的钮，不如置灰并在提示里说清原因
///   （与「分析结果不摆 ⟳ 刷新」同一条纪律，见 `copy.rs::refresh_active_result`）。
pub(crate) fn count_rows_available(has_data: bool, is_analysis: bool) -> bool {
    has_data && !is_analysis
}

/// 【计算行数】提示文案（纯函数）：分析结果讲「为什么不能算」，其余讲「点了会怎样」。
pub(crate) fn count_rows_tooltip(is_analysis: bool, computed: bool) -> &'static str {
    match (is_analysis, computed) {
        (true, _) => {
            "分析结果是当时那份已抓行算出来的，没有可重算的源查询——总行数就是这里的行数"
        }
        (false, true) => "总行数（对原 SQL 跑 count(*)）：点一下重新计算",
        (false, false) => "计算行数（对原 SQL 跑 count(*)，不产生新结果集）",
    }
}

/// 【左缘竖条】视图方式按钮的提示文案（纯函数）。
///
/// 对齐 `ResultSetViewer.updatePresentationInToolbar`（`:948-952`）：那里的每个
/// `VerticalButton` 是 `setText(pd.getLabel())` + `setToolTipText(pd.getDescription())`
/// ——**按钮上写名字、提示里写描述**。本仓按钮只有图标，所以提示承担两件事：
/// 「这是哪一档」+「这一档怎么用」，即 `标签（描述）`（与右缘竖条 `edge_tab_tooltip`
/// 同一形态）。
pub(crate) fn presentation_tooltip(presentation: Presentation) -> String {
    match presentation {
        Presentation::Grid => "网格视图（表头冻结，可排序 / 筛选 / 分栏）".to_string(),
        Presentation::Text => "文本视图（等宽文本呈现，便于整表复制或对照原样）".to_string(),
    }
}

/// 【左缘竖条】记录视图按钮（CHECK 语义，沉在竖条底部）的提示文案。
///
/// DBeaver 那颗是 `recordModeButton` + `ToggleModeAction`（`:964-965`），点一下在
/// 「网格」与「记录」之间切；本仓是转置开关，提示要直接说清会看到什么。
pub(crate) fn record_view_tooltip(active: bool) -> String {
    if active {
        "记录视图（本行已转置）：点一下回到网格".to_string()
    } else {
        "记录视图：当前行转置成 # / 字段 / 值 三列竖排".to_string()
    }
}

/// 【右缘竖条】某一页图标的勾选态 = 「整栏可见且该页已启用」。
///
/// 对齐 `ResultSetViewer.java:1079`：`panelButton.setChecked(panelsVisible && isPanelVisible(panel.getId()))`
/// ——竖条按钮是 `SWT.CHECK`，**多页可同时勾亮**（勾的是「这页开着」，不是「这页是当前页」）。
pub(crate) fn edge_tab_active(panels_visible: bool, is_open: bool) -> bool {
    panels_visible && is_open
}

/// 【视图菜单】勾选中的页项被点击时，是「关闭该页」还是「打开并切过去」。
///
/// 对齐 `ResultSetHandlerTogglePanel.toggleResultsPanel:64-69` 的
/// `isVisible = isPanelsVisible() && isPanelVisible(id)`：真 → `closePanel`，假 →
/// `activatePanel(id, true, true)`。菜单项勾选态是 `isPanelVisible(id)`（`updateElement:99`，
/// 不含整栏可见），所以「勾着但整栏收起」时点它仍是打开（顺带展开），不是关闭。
pub(crate) fn page_item_closes(panels_visible: bool, is_open: bool) -> bool {
    panels_visible && is_open
}

/// 【右缘竖条】某一页图标的提示文案。
///
/// 对齐 `ResultSetViewer.java:1055-1080`：竖条按钮 `setToolTipText(panel.getLabel())`，
/// 取得到命令描述时拼成 `label + " (" + toolTip + ")"`——即**页名 + 命令/面板描述**，
/// 而不是只有页名。本仓页签条已经用描述（`right_tab_description`），竖条这里同样拼起来。
///
/// 三态（对齐 `toggleResultsPanel:64-69` 的 `isPanelVisible = isPanelsVisible() && isPanelVisible(id)`）：
/// - 已启用且整栏可见（点一下会 `closePanel`）→ 报「点一下关闭这一页」；
/// - 已启用但整栏收起（点一下会 `activatePanel`，顺带展开）→ 报「展开并切到这一页」；
/// - 已关闭（不在 `open_tabs` 里，点一下会重新启用）→ 报「已关闭，点一下打开」。
pub(crate) fn edge_tab_tooltip(tab: RightTab, is_open: bool, panels_visible: bool) -> String {
    let base = format!("{}（{}）", right_tab_label(tab), right_tab_description(tab));
    if !is_open {
        return format!("{base} — 此页已关闭，点一下打开");
    }
    if panels_visible {
        return format!("{base} — 点一下关闭这一页");
    }
    format!("{base} — 点一下展开并切到这一页")
}

/// 「视图菜单」条目文案（动态条目按当前状态报动作，而不是报状态：
/// 菜单项是「点了会做什么」——对齐 DBeaver `CMD_TOGGLE_PANELS` / `CMD_TOGGLE_LAYOUT` 的命令标签）
pub(crate) fn panel_view_item_label(
    item: PanelViewItem,
    panels_visible: bool,
    vertical_layout: bool,
    panel_maximized: bool,
) -> String {
    match item {
        PanelViewItem::Page(tab) => right_tab_label(tab).to_string(),
        PanelViewItem::Separator => String::new(),
        PanelViewItem::PanelsToggle => {
            if panels_visible {
                "收起结果面板".to_string()
            } else {
                "展开结果面板".to_string()
            }
        }
        PanelViewItem::LayoutToggle => {
            if vertical_layout {
                "切换为横排布局（左右分栏）".to_string()
            } else {
                "切换为竖排布局（上下分栏）".to_string()
            }
        }
        // 最大化是「独占整块结果集区域」（DBeaver `togglePanelsMaximize` =
        // `viewerSash.hideUp()/showUp()`），文案报动作
        PanelViewItem::MaximizeToggle => {
            if panel_maximized {
                "还原结果网格".to_string()
            } else {
                "面板最大化（收起结果网格）".to_string()
            }
        }
    }
}

/// 【筛选条 ✕ 箭头菜单】条目清单（纯函数，测试直接覆盖；`PopupMenu` 内部不可读）。
///
/// 对齐 DBeaver `EraseItemListener.fillDropDownMenu`（`ResultSetViewer.java:1525-1545`）：
/// 那边第一项恒是 `CMD_FILTER_CLEAR_SETTING`（「删除所有过滤 / 排序」），其后按条件摆
/// 显示隐藏列 / 重置颜色 / 重置 transformers / 重置 pins，最后「全部重置」。
/// 本仓口径「能力没有就不摆」：只留**真有落点**的两条——
/// - `删除所有过滤 / 排序`（恒在：它就是清除钮的主点击动作，也是它的 tooltip 文案，
///   `ResultSetFilterPanel.java:319-325`；空态下点了是 no-op，DBeaver 那边同样恒可用）
/// - `显示全部列`（有隐藏列才摆，与列头 / 右键菜单同口径）
///
/// `filter_text` 不再影响条目清单（原先「有词才摆『清除筛选词』」那条已并进全清），
/// 参数保留是为了调用点不用跟着改，且将来若恢复窄口径项可直接用。
pub(crate) fn erase_menu_labels(_filter_text: &str, hidden_columns: usize) -> Vec<&'static str> {
    let mut labels = vec!["删除所有过滤 / 排序"];
    if hidden_columns > 0 {
        labels.push("显示全部列");
    }
    labels
}

/// 【段大小】解析状态行输入框的文本（纯函数，测试直接钉）。
///
/// 对齐 DBeaver `resultSetSize` 的 modify 回调（`ResultSetViewer.java:2138-2152`）：
/// `CommonUtils.toInt(text)` → 只有 `> 0` 才写偏好，`< MIN_SEGMENT_SIZE` 抬到下限；
/// 非数字 / 0 / 负数 = 无效输入（**不写**，输入框文本此刻仍保持用户所敲，
/// 由失焦回填收口，见 [`segment_rows_blur_display`]）。
/// 本仓返回 `None` 让调用方「什么都不做」，`Some(rows)` 才是有效提交。
pub(crate) fn parse_segment_rows(text: &str) -> Option<usize> {
    let rows = text.trim().parse::<usize>().ok()?;
    (rows > 0).then_some(rows)
}

/// 【段大小】失焦时输入框该显示什么（纯函数）。
///
/// 对齐 DBeaver 的 `focusLost` 监听器（`ResultSetViewer.java:2129-2137`）：
/// 取当前生效值 `getSegmentMaxRows()`，**只要文本框内容和它不一样就 `setText(realValue)`**
/// ——无效输入（`0` / `abc` / 空）与「格式怪但已生效」（`007`→`7` 已由 change 规范化）都靠这一步收口，
/// 不会把非法文本留在状态行里。
///
/// 返回 `None` = 文本已是生效值、无需改写；`Some(s)` = 回填成 `s`。
pub(crate) fn segment_rows_blur_display(text: &str, current: usize) -> Option<String> {
    let effective = current.to_string();
    (text.trim() != effective).then_some(effective)
}

/// 【状态历史】状态的一行描述（对齐 DBeaver `describeState`，`ResultSetViewer.java:5457-5473`）：/// 空白折叠成单空格（多行 SQL 在 tooltip 里不能换行），**超过 50 字符截断补 `…`**
/// （DBeaver 的阈值也是 50 字符，`...` 是 ASCII 三点——本仓用中文省略号跟界面文案一致）。
pub(crate) fn state_label(sql: &str) -> String {
    let flat = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 50;
    if flat.chars().count() <= MAX {
        return flat;
    }
    let mut out: String = flat.chars().take(MAX).collect();
    out.push('…');
    out
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
