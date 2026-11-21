//! edit_cmds 域（75-195 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。

use crate::changes::CellChange;
use crate::commands::{
    CommitResultChanges, CopyGridSelection, CopyRowValuesFromAbove, CopyRowValuesFromBelow,
    CycleResultPresentation, DeleteResultRow, DuplicateResultRow, EnterCellEdit, ExecuteSql,
    FetchAllData, FetchNextSegment, FocusRowJump, GotoColumn, InsertResultRow, RedoResultEdit,
    RejectPendingChanges, SelectCurrentColumn, SelectCurrentRow, SwitchFilterFocus, UndoResultEdit,
};

use super::*;

impl EditorHostPanel {
    /// `Ctrl+Enter`：执行（选区优先 → 光标所在语句）
    pub(crate) fn on_execute_sql(
        &mut self,
        _: &ExecuteSql,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.execute_preferring_selection(cx);
    }

    /// 【B14】结果网格里的 `Ctrl+C`（动作入口；键位在 `crates/app` 注册）
    pub(crate) fn on_copy_grid_selection(
        &mut self,
        _: &CopyGridSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_copy_grid_selection：结果网格复制（Ctrl+C）");
        self.copy_grid_selection(cx);
    }

    /// 【撤销】结果网格 `Ctrl+Z`（键位在 `crates/app` 注册，context = `RESULT_GRID_CONTEXT`；
    /// 编辑框内 `Ctrl+Z` 由输入内核处理，不走到这里）
    pub(crate) fn on_undo_result_edit(
        &mut self,
        _: &UndoResultEdit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_undo_result_edit：结果网格撤销（Ctrl+Z）");
        self.undo_result_edit(cx);
    }

    /// 【重做】结果网格 `Ctrl+Shift+Z`（同上；恢复最近一次撤销的编辑）
    pub(crate) fn on_redo_result_edit(
        &mut self,
        _: &RedoResultEdit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_redo_result_edit：结果网格重做（Ctrl+Shift+Z）");
        self.redo_result_edit(cx);
    }

    /// 【第 59 批】结果网格 `ENTER`：进入当前选中格的行内编辑
    /// （DBeaver `CMD_ROW_EDIT_INLINE` → `openValueEditor(true)`，`plugin.xml:1072`）。
    ///
    /// 与状态栏「修改」钮共用面板的 `begin_edit_cell`（emit `DoubleClickedCell`）——
    /// 键位只是多一个入口，链路不另开。焦点在编辑框时 `ENTER` 被输入内核先吃掉
    /// （`gpui-base` 的 `Input` context 绑了 `enter`），不会走到这里。
    pub(crate) fn on_enter_cell_edit(
        &mut self,
        _: &EnterCellEdit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.result_panel
            .update(cx, |panel, cx| panel.begin_edit_cell(cx));
    }

    /// 【第 59 批】结果网格 `Ctrl+反引号`：切到下一个呈现
    /// （DBeaver `CMD_SWITCH_PRESENTATION`，`plugin.xml:1059`）。
    ///
    /// 【第 60 批】两层 context（网格 / 面板）共用这一个 handler：焦点在网格时由网格那条
    /// 命中，焦点在面板（文本态）时由面板那条命中。切完要**移交焦点**（面板层拿不到
    /// window，见 `ResultPanel::presentation_focus_target` 的说明）。
    pub(crate) fn on_cycle_result_presentation(
        &mut self,
        _: &CycleResultPresentation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.result_panel
            .update(cx, |panel, cx| panel.cycle_presentation(cx));
        let target = self.result_panel.read(cx).presentation_focus_target(cx);
        window.focus(&target, cx);
    }

    /// 【第 68 批】结果网格 `Ctrl+S`：提交当前结果集的待提交变更
    /// （DBeaver `CMD_APPLY_CHANGES`，`plugin.xml:1082`；文案另见 `bundle_zh:69-70`）。
    ///
    /// 与结果工具栏「提交 N 处」钮、右键同一段（`commit_pending_from_ui` → 四道复核 +
    /// 失败落顶部提示），键位只是第三个入口，链路不另开。
    /// 同一个键在 `editor` context 是「保存文档」：焦点在网格时 `RESULT_GRID_CONTEXT`
    /// 更深、先命中（与 `database-nav` 的 `NavCommitStaged` 同模式）。
    pub(crate) fn on_commit_result_changes(
        &mut self,
        _: &CommitResultChanges,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_commit_result_changes：结果网格提交（Ctrl+S）");
        self.commit_pending_from_ui(cx);
    }

    /// 【第 69 批】行级键位组：状态栏三钮的键盘入口
    /// （DBeaver `plugin.xml:1073/1075/1079`，context 都是 `...resultset.focused`）。
    ///
    /// 键位没有「灰」这一态，所以四件事都要在**不可用时留状态栏提示**，
    /// 不静默吞键：只读结果集 / 没有当前行 / 行已标记删除 / 相邻行越界。
    /// 链路上与工具栏三钮同源（`insert_local_row` / `duplicate_local_row` /
    /// `delete_local_row` / `copy_row_values_from_adjacent`），键位只是多一个入口。
    pub(crate) fn on_insert_result_row(
        &mut self,
        _: &InsertResultRow,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.result_panel.read(cx).row_target_state(cx);
        if !target.actions.add {
            self.set_message(Some("结果集是只读的：不能插入行".to_string()), cx);
            return;
        }
        self.insert_local_row(target.row, cx);
    }

    pub(crate) fn on_duplicate_result_row(
        &mut self,
        _: &DuplicateResultRow,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.result_panel.read(cx).row_target_state(cx);
        if !target.actions.add {
            self.set_message(Some("结果集是只读的：不能复制行".to_string()), cx);
            return;
        }
        let Some(row) = target.row else {
            self.set_message(Some("先在网格里选中一行再复制".to_string()), cx);
            return;
        };
        self.duplicate_local_row(row, cx);
    }

    pub(crate) fn on_delete_result_row(
        &mut self,
        _: &DeleteResultRow,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.result_panel.read(cx).row_target_state(cx);
        if !target.actions.add {
            self.set_message(Some("结果集是只读的：不能删除行".to_string()), cx);
            return;
        }
        let Some(row) = target.row else {
            self.set_message(Some("先在网格里选中一行再删除".to_string()), cx);
            return;
        };
        if target.row_deleted {
            self.set_message(
                Some("这一行已标记删除；撤销删除后才能再删".to_string()),
                cx,
            );
            return;
        }
        self.delete_local_row(row, cx);
    }

    /// `CTRL+D`：把上一行的值填进当前行（`CMD_ROW_COPY_FROM_ABOVE`，`plugin.xml:1077`）
    pub(crate) fn on_copy_row_values_from_above(
        &mut self,
        _: &CopyRowValuesFromAbove,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_row_values_from(true, cx);
    }

    /// `CTRL+ALT+D`：把下一行的值填进当前行（`CMD_ROW_COPY_FROM_BELOW`，`plugin.xml:1078`）
    pub(crate) fn on_copy_row_values_from_below(
        &mut self,
        _: &CopyRowValuesFromBelow,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_row_values_from(false, cx);
    }

    /// 【第 70 批】结果网格 `CTRL+ALT+N`：取下一段
    /// （DBeaver `CMD_FETCH_PAGE` → `readNextSegment()`，`plugin.xml:1069`）。
    ///
    /// 与状态栏「获取数据」钮同一条路（`ResultRequest::FetchMore`）：判定全在
    /// `fetch_more` 里（没 SQL / 已经取完 / 执行器不支持 / 正在取），每条都留可读原因，
    /// 所以这里只转一次请求，不再复制一套门控。
    ///
    /// 与 `ALT+INSERT` 的差别：这条**不要求**有当前行（取数跟选中无关）。
    pub(crate) fn on_fetch_next_segment(
        &mut self,
        _: &FetchNextSegment,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.result_panel
            .update(cx, |panel, cx| panel.request_next_segment(cx));
    }

    /// 【第 72 批】结果网格 `CTRL+SHIFT+=`：取全部行
    /// （DBeaver `CMD_FETCH_ALL` → `readAllData()`，`plugin.xml:1070`）。
    ///
    /// 与状态栏「获取全部数据」钮同一条入口（`ResultRequest::AskFetchAll`）：DBeaver
    /// 那边点下去先是 `CONFIRM_RS_FETCH_ALL` 确认（`ResultSetViewer:4433-4440`），
    /// 确认过才真取。所以这条键**不直接开跑**，弹不弹由宿主判（`ask_fetch_all`）。
    pub(crate) fn on_fetch_all_data(
        &mut self,
        _: &FetchAllData,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.result_panel
            .update(cx, |panel, cx| panel.request_fetch_all(cx));
    }

    /// 【第 74 批】结果网格 `CTRL+R`：撤销全部未提交变更
    /// （DBeaver `CMD_REJECT_CHANGES` → `rejectChanges()`，`plugin.xml:1083`；
    /// `ResultSetHandlerMain:313-314`）。
    ///
    /// 与工具栏「回滚」钮同一条路（`ResultRequest::RollbackPending`）。不像 `CTRL+S`
    /// 那样要求可编辑：回滚只是丢本地改动、不写源库，只读结果集上按下去也只会得到
    /// 「没有可撤销的变更」（那条提示在 `discard_pending_changes` 里，与钮共用）。
    pub(crate) fn on_reject_pending_changes(
        &mut self,
        _: &RejectPendingChanges,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.result_panel
            .update(cx, |panel, cx| panel.request_rollback_pending(cx));
    }

    /// 【第 70 批】结果网格 `CTRL+G`：行号跳转
    /// （DBeaver `CMD_GO_TO_ROW` → `GotoLineDialog`，`plugin.xml:1091`）。
    ///
    /// 本仓不弹对话框：把状态栏那口常驻行号框回填当前行号并聚焦，敲 `ENTER`
    /// 后焦点交回网格（见 `ResultPanel::row_jump_focus` 的说明）。
    pub(crate) fn on_focus_row_jump(
        &mut self,
        _: &FocusRowJump,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self
            .result_panel
            .update(cx, |panel, cx| panel.row_jump_focus(window, cx));
        let Some(target) = target else {
            self.set_message(Some("这份结果还没有行可跳；先执行查询".to_string()), cx);
            return;
        };
        window.focus(&target, cx);
    }

    /// 【第 75 批】结果网格 `CTRL+SHIFT+G`：跳转到列
    /// （DBeaver `CMD_GO_TO_COLUMN` → `askForColumnToGo`，`plugin.xml:1090`；
    /// `ResultSetHandlerMain:526-534`）。
    ///
    /// 与 `CTRL+G` 的形态差别：行号有状态栏常驻输入框可落脚，列名没有——所以这条
    /// **开面板内浮层**（`ResultPanel::open_goto_column` 要 window 交焦点给输入框）。
    /// 没有列可跳时留一句提示（第 69 批「键位不可用要说话」口径）。
    pub(crate) fn on_goto_column(
        &mut self,
        _: &GotoColumn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let opened = self
            .result_panel
            .update(cx, |panel, cx| panel.open_goto_column(window, cx));
        if !opened {
            self.set_message(Some("这份结果还没有列可跳；先执行查询".to_string()), cx);
        }
    }

    /// 【第 77 批】`CTRL+ALT+SHIFT+T`：焦点在「筛选框」与「结果呈现」之间来回切
    /// （DBeaver `CMD_FOCUS_FILTER` → `ResultSetViewer.switchFilterFocus()`，
    /// `plugin.xml:1058`；`ResultSetHandlerMain:179-181`）。
    ///
    /// 键位挂 `RESULT_PANEL_CONTEXT`（上游同一条也绑 `...context.resultset`，不是
    /// `...focused` 变体）——焦点已经在筛选框里时也必须命中，否则「切回来」那一半按不到。
    /// 筛选区收起时留一句提示。
    pub(crate) fn on_switch_filter_focus(
        &mut self,
        _: &SwitchFilterFocus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let switched = self
            .result_panel
            .update(cx, |panel, cx| panel.switch_filter_focus(window, cx));
        if !switched {
            self.set_message(
                Some("筛选区已收起：在工具栏里打开「显示过滤器面板」后再按".to_string()),
                cx,
            );
        }
    }

    /// 【第 76 批】结果网格 `CTRL+ALT+C`：选中当前格所在的整列
    /// （DBeaver `CMD_SELECT_COLUMNS`，`plugin.xml:1088`；
    /// `SpreadsheetCommandHandler:106-118`）。
    ///
    /// 没有当前格 / 当前格在行号槽时留一句提示（第 69 批「键位不可用要说话」口径）。
    pub(crate) fn on_select_current_column(
        &mut self,
        _: &SelectCurrentColumn,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = self
            .result_panel
            .update(cx, |panel, cx| panel.select_current_column(cx));
        if !selected {
            self.set_message(Some("先在网格里选中一个数据格再选列".to_string()), cx);
        }
    }

    /// 【第 76 批】结果网格 `CTRL+ALT+R`：选中当前格所在的整行
    /// （DBeaver `CMD_SELECT_ROWS`，`plugin.xml:1089`；
    /// `SpreadsheetCommandHandler:119-131`）。
    pub(crate) fn on_select_current_row(
        &mut self,
        _: &SelectCurrentRow,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = self
            .result_panel
            .update(cx, |panel, cx| panel.select_current_row(cx));
        if !selected {
            self.set_message(Some("先在网格里选中一个数据格再选行".to_string()), cx);
        }
    }

    /// 邻行复制的共用段：门控 + 越界 / 已删 / 无写入 三种「按了但没改」的提示。
    fn copy_row_values_from(&mut self, above: bool, cx: &mut Context<Self>) {
        let target = self.result_panel.read(cx).row_target_state(cx);
        if !target.actions.add {
            self.set_message(Some("结果集是只读的：不能改写行值".to_string()), cx);
            return;
        }
        let Some(row) = target.row else {
            self.set_message(Some("先在网格里选中一行再复制".to_string()), cx);
            return;
        };
        if target.row_deleted {
            self.set_message(
                Some("这一行已标记删除；撤销删除后才能复制值".to_string()),
                cx,
            );
            return;
        }
        if self
            .result_panel
            .read(cx)
            .adjacent_row_view(row, above, cx)
            .is_none()
        {
            self.set_message(
                Some(
                    if above {
                        "当前行已是首行，上面没有可复制的行"
                    } else {
                        "当前行已是末行，下面没有可复制的行"
                    }
                    .to_string(),
                ),
                cx,
            );
            return;
        }
        if self.copy_row_values_from_adjacent(row, above, cx) == 0 {
            self.set_message(
                Some("相邻行没有可写入的值（相同，或这一行不可改写）".to_string()),
                cx,
            );
        }
    }

    /// 【撤销】撤销一步结果编辑：格子恢复「上一步的值」（DBeaver 粒度 = 每格编辑一步）。
    ///
    /// 写回语义：从 [`EditHistory`] 弹出栈顶一步；恢复值对比**数据原值**（`entry.rows`，
    /// 网格行数据本身就是原值，无需另改）——恢复值 = 原值 → 从变更集移除（没有待提交
    /// 变更）；否则变更集里这条的 `edited` 改为恢复值。然后 `sync_result_view` 重投影
    /// （脏格底色 / 待提交计数 / 状态行）。结果集变化导致行 / 列越界时不写回（数据没了）。
    pub(crate) fn undo_result_edit(&mut self, cx: &mut Context<Self>) {
        let document = self.document.clone();
        let applied = self.shared.update_results(|store| {
            let active = store.active_index(&document).unwrap_or(0);
            // 从历史栈弹出一步（在 mutate_history 闭包里取，避免跨闭包借用冲突）
            let mut step = None;
            store.mutate_history(&document, active, |history| step = history.undo());
            let Some(change) = step else {
                return false;
            };
            let Some(original_value) = store
                .active(&document)
                .and_then(|entry| entry.rows.get(change.row))
                .and_then(|row| row.get(change.column))
                .cloned()
            else {
                return false;
            };
            store.mutate_change_set(&document, active, |changes| {
                if change.original == original_value {
                    // 恢复成数据原值：这条变更不存在了
                    changes.remove(change.row, change.column);
                } else {
                    changes.upsert(CellChange::pending(
                        change.row,
                        change.column,
                        original_value,
                        change.original,
                    ));
                }
            });
            true
        });
        if applied {
            self.sync_result_view(cx);
        }
    }

    /// 【重做】恢复最近撤销的那一步：格子回到「被撤销时的值」（对称写回变更集 + 重投影）
    pub(crate) fn redo_result_edit(&mut self, cx: &mut Context<Self>) {
        let document = self.document.clone();
        let applied = self.shared.update_results(|store| {
            let active = store.active_index(&document).unwrap_or(0);
            let mut step = None;
            store.mutate_history(&document, active, |history| step = history.redo());
            let Some(change) = step else {
                return false;
            };
            let Some(original_value) = store
                .active(&document)
                .and_then(|entry| entry.rows.get(change.row))
                .and_then(|row| row.get(change.column))
                .cloned()
            else {
                return false;
            };
            store.mutate_change_set(&document, active, |changes| {
                if change.edited == original_value {
                    changes.remove(change.row, change.column);
                } else {
                    changes.upsert(CellChange::pending(
                        change.row,
                        change.column,
                        original_value,
                        change.edited,
                    ));
                }
            });
            true
        });
        if applied {
            self.sync_result_view(cx);
        }
    }
}
