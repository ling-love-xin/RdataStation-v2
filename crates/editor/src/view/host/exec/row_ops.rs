//! row_ops 域（196-435 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。

use result_panel::rowset::{LocalRow, RowOp, RowRef};


use super::*;

impl EditorHostPanel {
    /// 【#28-B3】新增一条本地空行：只落本地叠加层 + 行级历史，不下发 INSERT、不进格级变更集。
    /// `anchor_view` = 选中视图行（0 基）；`None` = 末尾区段。本期落点即末尾区段，
    /// anchor 只作为历史元信息留档（D5）。
    pub(crate) fn insert_local_row(&mut self, anchor_view: Option<usize>, cx: &mut Context<Self>) {
        let document = self.document.clone();
        // 锚点身份先从网格真值翻译成 RowRef（store 闭包内不能再借 grid 实体）
        let anchor_ref = anchor_view.and_then(|view| {
            self.grid.read(cx).delegate().reference_of(view)
        });
        let applied = self.shared.update_results(|store| {
            let active = store.active_index(&document).unwrap_or(0);
            let Some(entry) = store.active(&document) else {
                return false;
            };
            let cols = entry.columns.len();
            // D5：锚定底层行号；锚到本地行本期记 None（区段始终在末尾，不细算前一个底层号）
            let anchor_data = match anchor_ref {
                Some(RowRef::Data(ix)) => Some(ix),
                Some(RowRef::Local(_)) => None,
                None => None,
            };
            let mut local_id = 0u64;
            store.mutate_local_rows(&document, active, |set| {
                local_id = set.next_id();
                set.push_local(LocalRow::new(local_id, vec![String::new(); cols]));
            });
            store.mutate_row_history(&document, active, |history| {
                history.push(RowOp::InsertLocal {
                    local_id,
                    anchor: anchor_data,
                });
            });
            true
        });
        if applied {
            self.sync_result_view(cx);
        }
    }

    /// 【#28-B3】复制当前选中行为一条本地行：底层行取 `rows` 全量快照，本地行取本地 cells 快照。
    pub(crate) fn duplicate_local_row(&mut self, view: usize, cx: &mut Context<Self>) {
        let document = self.document.clone();
        let Some(reference) = self.grid.read(cx).delegate().reference_of(view) else {
            return;
        };
        let applied = self.shared.update_results(|store| {
            let active = store.active_index(&document).unwrap_or(0);
            let snapshot = match reference {
                RowRef::Data(ix) => store
                   .active(&document)
                    .and_then(|entry| entry.rows.get(ix))
                    .cloned()
                    .unwrap_or_default(),
                RowRef::Local(id) => store
                    .local_rows(&document, active)
                    .and_then(|set| set.local_row(id))
                    .map(|row| row.cells.clone())
                    .unwrap_or_default(),
            };
            let anchor = match reference {
                RowRef::Data(ix) => Some(ix),
                RowRef::Local(_) => None,
            };
            let mut local_id = 0u64;
            store.mutate_local_rows(&document, active, |set| {
                local_id = set.next_id();
                set.push_local(LocalRow::new(local_id, snapshot.clone()));
            });
            store.mutate_row_history(&document, active, |history| {
                history.push(RowOp::DuplicateLocal {
                    local_id,
                    source: reference,
                    snapshot,
                    anchor,
                });
            });
            true
        });
        if applied {
            self.sync_result_view(cx);
        }
    }

    /// 【#28-B3】删除当前选中行：底层行只打删除标记（可撤销恢复），本地行直接移除并凭快照入栈。
    pub(crate) fn delete_local_row(&mut self, view: usize, cx: &mut Context<Self>) {
        let document = self.document.clone();
        let Some(reference) = self.grid.read(cx).delegate().reference_of(view) else {
            return;
        };
        let applied = self.shared.update_results(|store| {
            let active = store.active_index(&document).unwrap_or(0);
            match reference {
                RowRef::Data(ix) => {
                    // 已被标记删除的行不重复入栈（撤销栈里不出现重复动作）
                    let already = store
                        .local_rows(&document, active)
                        .is_some_and(|set| set.is_deleted(ix));
                    if already {
                        return false;
                    }
                    store.mutate_local_rows(&document, active, |set| set.mark_deleted(ix));
                    store.mutate_row_history(&document, active, |history| {
                        history.push(RowOp::DeleteData(ix));
                    });
                    true
                }
                RowRef::Local(id) => {
                    let mut removed: Option<LocalRow> = None;
                    store.mutate_local_rows(&document, active, |set| {
                        removed = set.remove_local(id);
                    });
                    let Some(row) = removed else {
                        return false;
                    };
                    store.mutate_row_history(&document, active, |history| {
                        history.push(RowOp::RemoveLocal {
                            local_id: id,
                            cells: row.cells.clone(),
                            anchor: None,
                        });
                    });
                    true
                }
            }
        });
        if applied {
            self.sync_result_view(cx);
        }
    }

    /// 【#28-B3】组合撤销：格级 `EditHistory` 非空先走格级（现有 [`Self::undo_result_edit`]），
    /// 栈空再走行级 `RowHistory`；两个栈互不混栈。
    pub(crate) fn undo_row_action(&mut self, cx: &mut Context<Self>) {
        let document = self.document.clone();
        let active = self
            .shared
            .results()
            .active_index(&document)
            .unwrap_or(0);
        let cell_can_undo = self
            .shared
            .results()
            .history(&document, active)
            .is_some_and(|history| history.can_undo());
        if cell_can_undo {
            self.undo_result_edit(cx);
            return;
        }
        let applied = self.shared.update_results(|store| {
            let active = store.active_index(&document).unwrap_or(0);
            let mut op = None;
            store.mutate_row_history(&document, active, |history| op = history.undo());
            let Some(op) = op else {
                return false;
            };
            match op {
                RowOp::InsertLocal { local_id, .. } => {
                    store.mutate_local_rows(&document, active, |set| {
                        set.remove_local(local_id);
                    });
                }
                RowOp::DuplicateLocal { local_id, .. } => {
                    store.mutate_local_rows(&document, active, |set| {
                        set.remove_local(local_id);
                    });
                }
                RowOp::DeleteData(ix) => {
                    store.mutate_local_rows(&document, active, |set| {
                        set.unmark_deleted(ix);
                    });
                }
                RowOp::RemoveLocal { local_id, cells, .. } => {
                    store.mutate_local_rows(&document, active, |set| {
                        set.push_local(LocalRow::new(local_id, cells));
                    });
                }
            }
            true
        });
        if applied {
            self.sync_result_view(cx);
        }
    }

    /// 【#28-B3】组合重做：与 [`Self::undo_row_action`] 对称；格级栈优先，空走行级栈。
    pub(crate) fn redo_row_action(&mut self, cx: &mut Context<Self>) {
        let document = self.document.clone();
        let active = self
            .shared
            .results()
            .active_index(&document)
            .unwrap_or(0);
        let cell_can_redo = self
            .shared
            .results()
            .history(&document, active)
            .is_some_and(|history| history.can_redo());
        if cell_can_redo {
            self.redo_result_edit(cx);
            return;
        }
        let applied = self.shared.update_results(|store| {
            let active = store.active_index(&document).unwrap_or(0);
            let mut op = None;
            store.mutate_row_history(&document, active, |history| op = history.redo());
            let Some(op) = op else {
                return false;
            };
            match op {
                // 重做只恢复动作本身，不回格编辑状态（本地行格编辑不进栈，拍板口径）
                RowOp::InsertLocal { local_id, .. } => {
                    store.mutate_local_rows(&document, active, |set| {
                        set.push_local(LocalRow::new(local_id, Vec::new()));
                    });
                }
                RowOp::DuplicateLocal {
                    local_id,
                    snapshot,
                    ..
                } => {
                    store.mutate_local_rows(&document, active, |set| {
                        set.push_local(LocalRow::new(local_id, snapshot));
                    });
                }
                RowOp::DeleteData(ix) => {
                    store.mutate_local_rows(&document, active, |set| {
                        set.mark_deleted(ix);
                    });
                }
                RowOp::RemoveLocal { local_id, .. } => {
                    store.mutate_local_rows(&document, active, |set| {
                        set.remove_local(local_id);
                    });
                }
            }
            true
        });
        if applied {
            self.sync_result_view(cx);
        }
    }


}
