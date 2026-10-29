//! edit_cmds 域（75-195 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。


use crate::changes::CellChange;
use crate::commands::{
    CopyGridSelection, ExecuteSql,
    RedoResultEdit, UndoResultEdit,
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
                        change.row, change.column, original_value, change.original,
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
                        change.row, change.column, original_value, change.edited,
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
