//! row_ops 域（196-435 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。

use result_panel::changes::{row_copy_changes, CellChange};
use result_panel::rowset::{LocalRow, RowOp, RowRef};

use super::*;

impl EditorHostPanel {
    /// 【#28-B3】新增一条本地空行：只落本地叠加层 + 行级历史，不下发 INSERT、不进格级变更集。
    /// `anchor_view` = 选中视图行（0 基）；`None` = 末尾区段。本期落点即末尾区段，
    /// anchor 只作为历史元信息留档（D5）。
    pub(crate) fn insert_local_row(&mut self, anchor_view: Option<usize>, cx: &mut Context<Self>) {
        let document = self.document.clone();
        // 锚点身份先从网格真值翻译成 RowRef（store 闭包内不能再借 grid 实体）
        let anchor_ref =
            anchor_view.and_then(|view| self.grid.read(cx).delegate().reference_of(view));
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

    /// 【第 67 批】把某一格**重置为数据原值**（对齐 DBeaver `CMD_CELL_RESET`，
    /// `ResultSetViewer.fillEditMenu:3536` → `ResultSetModel.resetCellValue:571-590`）。
    ///
    /// 两支与 DBeaver 同序：
    /// 1. 底层行已被标记删除 → 撤销删除标记（行回正常态），这一格不动；
    /// 2. 否则从变更集移除这一格 —— 网格的显示值本来就从 `entry.rows` 取，
    ///    变更集里没这条之后就自动回落成数据原值，不需要额外写回。
    ///
    /// **不碰撤销栈**：DBeaver 的 `resetCellValue` 也是直接改行状态 / 回写值，不走 undo
    /// context。本仓照此，所以撤销栈里那条 `DeleteData` 还在（之后按「撤销」会再跑一次
    /// `unmark_deleted`，对未删行是幂等的）。
    ///
    /// 本地行的格值落在快照里、不进变更集（#28-B4 口径），没有「重置这一格」这件事，
    /// 静默不动作。`col` 是**组件列**（含行号槽，数据格从 1 起），与面板的
    /// `begin_edit_cell_at` 同口径。
    pub(crate) fn reset_result_cell(&mut self, view: usize, col: usize, cx: &mut Context<Self>) {
        // 行号槽不是数据格（与 `begin_edit_cell_at` 面板侧同一条门控）
        let Some(column) = col.checked_sub(1) else {
            return;
        };
        let document = self.document.clone();
        let Some(reference) = self.grid.read(cx).delegate().reference_of(view) else {
            return;
        };
        let applied = self.shared.update_results(|store| {
            let active = store.active_index(&document).unwrap_or(0);
            match reference {
                RowRef::Data(ix) => {
                    if store
                        .local_rows(&document, active)
                        .is_some_and(|set| set.is_deleted(ix))
                    {
                        store.mutate_local_rows(&document, active, |set| {
                            set.unmark_deleted(ix);
                        });
                        return true;
                    }
                    let mut reset = false;
                    store.mutate_change_set(&document, active, |changes| {
                        reset = changes.remove(ix, column);
                    });
                    reset
                }
                RowRef::Local(_) => false,
            }
        });
        if applied {
            self.sync_result_view(cx);
        }
    }

    /// 【第 55 批】把**相邻行**的值复制到当前行（对齐 DBeaver `copyRowValues:5062-5156`，
    /// 菜单命令 `CMD_ROW_COPY_FROM_ABOVE` / `_BELOW`）。
    ///
    /// 与 [`Self::duplicate_local_row`] 的区别：那条**新增**一条本地行；这条逐格**改写
    /// 现有行**——底层行走变更集（原值取库里那一份，故可撤销、可提交），本地行走快照
    /// （不进变更集 / 撤销栈，与本地行格编辑同口径）。
    ///
    /// 门控：源行越界、目标行被标记删除、列数为 0 都静默不动作（DBeaver 在 `:5091-5093`
    /// 也是 `break`）。要写哪几格由纯函数 [`row_copy_changes`] 算（已改过的格保留库里原值、
    /// 值本来就一样的格不写）。返回真正写进的格数。
    pub(crate) fn copy_row_values_from_adjacent(
        &mut self,
        target_view: usize,
        above: bool,
        cx: &mut Context<Self>,
    ) -> usize {
        let document = self.document.clone();
        // 目标 / 源行身份先从网格真值翻译成 RowRef（store 闭包内不能再借 grid 实体）
        let target_ref = self.grid.read(cx).delegate().reference_of(target_view);
        // 源视图行：上一行 = target - 1（首行没有），下一行 = target + 1（末行 reference_of 给 None）
        let source_view = if above {
            target_view.checked_sub(1)
        } else {
            target_view.checked_add(1)
        };
        let source_ref = source_view.and_then(|view| self.grid.read(cx).delegate().reference_of(view));
        let (Some(target_ref), Some(source_ref)) = (target_ref, source_ref) else {
            return 0;
        };
        if source_view == Some(target_view) {
            return 0;
        }
        let written = self.shared.update_results(|store| {
            let active = store.active_index(&document).unwrap_or(0);
            let columns = store.active(&document).map(|entry| entry.columns.len()).unwrap_or(0);
            // 源行 cells 快照（数据列序；底层行取 rows，本地行取快照）
            let source_cells: Vec<String> = match source_ref {
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
            if columns == 0 || source_cells.is_empty() {
                return 0;
            }
            match target_ref {
                RowRef::Data(ix) => {
                    // 被标记删除的底层行不再改值（撤销删除后才恢复，与行级动作同一门控）
                    if store
                        .local_rows(&document, active)
                        .is_some_and(|set| set.is_deleted(ix))
                    {
                        return 0;
                    }
                    let target_cells: Vec<String> = store
                        .active(&document)
                        .and_then(|entry| entry.rows.get(ix))
                        .cloned()
                        .unwrap_or_default();
                    // 要写哪几格由纯函数算（`changes::row_copy_changes` 带用例）：已改过的格
                    // 展示值取集合里的 `edited`、原值仍取库里那份；值本来就一样的格不写。
                    let writes: Vec<CellChange> = store
                        .change_set(&document, active)
                        .map(|changes| {
                            row_copy_changes(ix, &target_cells, &source_cells, changes)
                        })
                        .unwrap_or_default();
                    if writes.is_empty() {
                        return 0;
                    }
                    store.mutate_change_set(&document, active, |changes| {
                        for change in &writes {
                            changes.upsert(change.clone());
                        }
                    });
                    let count = writes.len();
                    store.mutate_history(&document, active, |history| {
                        for change in writes {
                            history.push(change);
                        }
                    });
                    count
                }
                RowRef::Local(id) => {
                    // 本地行：只写回快照，不进变更集 / 撤销栈（#28-B4 拍板口径）
                    let mut count = 0;
                    store.mutate_local_rows(&document, active, |set| {
                        let Some(row) = set.local_row_mut(id) else {
                            return;
                        };
                        for column in 0..columns.min(source_cells.len()) {
                            let Some(cell) = row.cells.get_mut(column) else {
                                continue;
                            };
                            let edited = source_cells[column].clone();
                            if *cell != edited {
                                *cell = edited;
                                count += 1;
                            }
                        }
                    });
                    count
                }
            }
        });
        if written > 0 {
            self.sync_result_view(cx);
        }
        written
    }

    /// 【#28-B3】组合撤销：格级 `EditHistory` 非空先走格级（现有 [`Self::undo_result_edit`]），
    /// 栈空再走行级 `RowHistory`；两个栈互不混栈。
    pub(crate) fn undo_row_action(&mut self, cx: &mut Context<Self>) {
        let document = self.document.clone();
        let active = self.shared.results().active_index(&document).unwrap_or(0);
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
                RowOp::RemoveLocal {
                    local_id, cells, ..
                } => {
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
        let active = self.shared.results().active_index(&document).unwrap_or(0);
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
                    local_id, snapshot, ..
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
