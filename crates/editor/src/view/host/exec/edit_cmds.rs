//! edit_cmds 域（75-195 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。

use crate::changes::CellChange;
use crate::commands::{
    CommitResultChanges, CopyGridSelection, CopyRowValuesFromAbove, CopyRowValuesFromBelow,
    CycleResultPresentation, DeleteResultRow, DuplicateResultRow, EnterCellEdit, ExecuteSql,
    InsertResultRow, RedoResultEdit, UndoResultEdit,
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
