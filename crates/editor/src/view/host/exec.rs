//! 编辑器宿主面板 —— 执行域（bound_connection/channel/on_execute_sql/run_all/execute/explain_current/tx_action/interrupt/submit_scan/ensure_exec_pump/drain_lsp_events/drain_exec_results）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。

use result_panel::rowset::{LocalRow, RowOp, RowRef};

// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法


use crate::changes::CellChange;
use crate::channel::{self, ExecChannel};
use crate::commands::{
    CopyGridSelection, ExecuteAll, ExecuteSql,
    RedoResultEdit, UndoResultEdit,
};
use crate::execution::{self, ExecTarget};
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）
use crate::store::ResultPlacement;
use crate::translate;
use crate::view::fold;


use super::*;
impl EditorHostPanel {

    /// 【B13】执行位置选择器（工具栏右侧：原型 §2.2 的「执行位置 ▾」）
    ///
    /// 三档**互斥**（原型 §5.7）：源库直连 / DuckDB 本地加速 / 联邦查询。菜单项由纯函数
    /// [`channel::menu_items`] 给（哪项能点、为什么不能点都在那儿定），这里只负责画：
    /// 不可用项**保留形态但置灰 + 行尾给原因**，当前项打勾。
    ///
    /// 本地跑的那两档额外给一个**维护动作**：`重新挂载源库（刷新表清单）`（见
    /// [`channel::refresh_item_label`]）——源库新建的表要重挂才看得见。
    /// 渲染入口在工具栏右区通道 pill（2026-10-27 起；见 [`render_channel_control`]）。

    /// 本文档绑定的连接 id（`None` = 未绑定，执行跟随当前活动连接）
    pub(crate) fn bound_connection(&self) -> Option<String> {
        self.with_document(|doc| doc.connection().map(str::to_string))
            .flatten()
    }

    /// 本文档当前的执行通道（B13；文档属性，读的是服务层的真值）
    pub fn channel(&self) -> ExecChannel {
        self.with_document(|doc| doc.channel()).unwrap_or_default()
    }

    /// 源库档能不能用：绑定的连接已建连（**未绑定算可用**——那是在跟随当前活动连接，
    /// 它到底连没连不归这里断言，真发错东西了由执行器报可读原因）
    fn source_channel_ready(&self) -> bool {
        match self.bound_connection() {
            None => true,
            Some(id) => self
                .shared
                .connection_options()
                .iter()
                .find(|option| option.id == id)
                .is_some_and(|option| option.connected),
        }
    }

    /// 【B13】「执行位置 ▾」的菜单项（纯函数算的：可用性 + 不可用原因 + 当前项打勾）
    ///
    /// 门控真值来自宿主注入的 `ChannelsPort`；**没注入端口就不给选**（如实报“尚未接入”）。
    pub fn channel_menu(&self) -> Vec<channel::ChannelMenuItem> {
        let availability = self
            .shared
            .channel_availability(self.bound_connection().as_deref());
        channel::menu_items(self.channel(), self.source_channel_ready(), &availability)
    }

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


    pub(crate) fn execute_preferring_selection(&mut self, cx: &mut Context<Self>) {
        let (text, selection) = self.editor_snapshot(cx);
        let target = execution::resolve_target(&text, selection);
        // 2026-10-27：主执行动作（快捷键）也是「每次执行 = 新结果 tab」——
        // 与工具栏执行按钮同口径（用户要求多次执行不挤在同一 tab）。
        self.execute(target, ResultPlacement::NewSet, cx);
    }

    /// `Ctrl+Shift+Enter`：执行全部
    pub(crate) fn on_execute_all(
        &mut self,
        _: &ExecuteAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_all(cx);
    }

    /// 执行整篇（**宿主发起的唯一入口**：导航「查看数据」打开文档后自动跑一次，B11）
    ///
    /// 与 `Ctrl+Shift+Enter` 同一条路：目标解析 → 提交 → 轮询回填。
    pub fn run_all(&mut self, cx: &mut Context<Self>) {
        let (text, _selection) = self.editor_snapshot(cx);
        let target = execution::all_target(&text);
        // 2026-10-27：执行全部同样是「每次执行 = 新结果 tab」（同主执行口径）。
        self.execute(target, ResultPlacement::NewSet, cx);
    }

    /// 提交一次执行
    ///
    /// `placement` 决定结果落到当前结果集还是新结果集（B2 的「在新结果标签中执行」/「批量执行」）；
    /// 自动提交关闭时（B4）本次执行先开一个事务。
    /// 拒绝都要留痕迹：文本模式（能力表禁止通信）、未接入执行、忙、空目标。
    ///
    /// 返回**是否真的提交了**：导出那条路要先记在途状态、再提交取段，
    /// 提交没成（忙 / 未接入）就得把在途状态撒回去（否则状态会残留到下一次）。
    pub(crate) fn execute(
        &mut self,
        target: ExecTarget,
        placement: ResultPlacement,
        cx: &mut Context<Self>,
    ) -> bool {
        self.execute_labeled(target, placement, None, cx)
    }

    /// 同 [`Self::execute`]，但给这一批回填的结果贴一个**自定义标题**（今天只有执行计划用）
    ///
    /// 标题是**按位**记的（`pending_labels`）：回填是一条语句一个结论，
    /// “这份是计划、那份是数据”不能靠一个布尔量猜。
    pub(crate) fn execute_labeled(
        &mut self,
        target: ExecTarget,
        placement: ResultPlacement,
        title: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let mode = self.document_mode();
        let allowed = self.execution_allowed();
        let channel = self.channel();
        let conn = self.bound_connection();
        let stmt_count = target.statements().len();
        let sql_preview = target
            .statements()
            .first()
            .map(|sql| sql.chars().take(80).collect::<String>())
            .unwrap_or_default();
        tracing::info!(
            target = "rds_editor::exec",
            doc = %self.document,
            mode = ?mode,
            execute_allowed = allowed,
            channel = ?channel,
            connection = ?conn,
            statements = stmt_count,
            kind = target.label(),
            sql = sql_preview,
            "提交执行：进入执行主入口"
        );
        if !allowed {
            tracing::warn!(
                target = "rds_editor::exec",
                doc = %self.document,
                mode = ?mode,
                channel = ?channel,
                connection = ?conn,
                kind = target.label(),
                "执行被拒：当前文档模式不与数据库通信（通用/分析模式无执行能力）"
            );
            self.set_message(Some("文本模式不与数据库通信".to_string()), cx);
            return false;
        }
        // 【B13】通道门控与写拒绝都在**提交之前**：注定被拒的语句不该跑一半（也不该让
        // “本地副本只读”这种事等驱动报一个谁也不懂的错）
        if let Err(reason) = self
            .channel_ready()
            .and_then(|()| self.channel_write_check(&target))
        {
            tracing::warn!(
                target = "rds_editor::exec",
                doc = %self.document,
                channel = ?channel,
                connection = ?conn,
                kind = target.label(),
                reason = %reason,
                "执行被拒：通道不可用或写语句被通道闸拦下"
            );
            self.set_message(Some(reason), cx);
            return false;
        }
        // 项目只读（标题栏的锁）：写语句同样在**提交之前**拒（旧 SQL 面板有这道闸，B12 遗失）
        if let Err(reason) = self.project_write_check(&target) {
            tracing::warn!(
                target = "rds_editor::exec",
                doc = %self.document,
                channel = ?channel,
                connection = ?conn,
                kind = target.label(),
                reason = %reason,
                "执行被拒：项目只读，写语句被项目闸拦下"
            );
            self.set_message(Some(reason), cx);
            return false;
        }
        // 【B16】参数绑定（DBeaver 式）：目标里有 `:name` → 挂起等表单，填完再替换执行。
        // 只扫**用户写的 SQL** 变体（`statements()` 为空的目标 = 内部生成，没有参数）。
        // 参数名按首次出现顺序去重（表单就按这个顺序摆）
        let names = {
            let mut seen: Vec<String> = Vec::new();
            for sql in target.statements() {
                for name in engine::sql::param_names(&sql) {
                    if !seen.contains(&name) {
                        seen.push(name);
                    }
                }
            }
            seen
        };
        if !names.is_empty() {
            tracing::info!(
                target = "rds_editor::exec",
                doc = %self.document,
                channel = ?channel,
                connection = ?conn,
                params = ?names,
                "执行挂起：参数表单待填，填完后再入队"
            );
            self.pending_bind = Some(PendingBind {
                target,
                placement,
                title,
            });
            self.open_bind_dialog(names, cx);
            return false;
        }
        // 预期回填条数 = 本次要跑的语句数（批量 > 1）：跑完这几条才算“不执行中”
        let expected = target.statements().len();
        // B4：自动提交关掉时，本次执行进事务（引擎在没有事务时会先自动开一个）；
        // B13：加速 / 联邦没有事务语义，那两档上不走事务
        let options = execution::RunOptions {
            use_transaction: !self.autocommit && channel.allows_transactions(),
        };
        match self
            .shared
            .submit(self.document.clone(), &target, placement, options)
        {
            Ok(()) => {
                tracing::info!(
                    target = "rds_editor::exec",
                    doc = %self.document,
                    channel = ?channel,
                    connection = ?conn,
                    statements = expected,
                    use_transaction = options.use_transaction,
                    kind = target.label(),
                    "执行已入队后台线程"
                );
                if self.pending == 0 {
                    // 批量期间一直计同一轮的时间：后一句不该把计时清零
                    self.running_since = Some(std::time::Instant::now());
                }
                self.pending += expected;
                self.pending_labels
                    .extend((0..expected).map(|_| title.clone()));
                self.set_message(None, cx);
                self.ensure_exec_pump(cx);
                true
            }
            Err(error) => {
                tracing::warn!(
                    target = "rds_editor::exec",
                    doc = %self.document,
                    channel = ?channel,
                    connection = ?conn,
                    kind = target.label(),
                    reason = %error.message(),
                    "执行提交被队列拒绝"
                );
                self.set_message(Some(error.message().to_string()), cx);
                false
            }
        }
    }

    /// 【B10】执行计划（原型 §5.1：`EXPLAIN` 当前语句 / 选区，**在当前通道上执行**）
    ///
    /// - 前缀**按方言生成**（SQLite 是 `EXPLAIN QUERY PLAN`；SQL Server / Oracle 要先改会话开关
    ///   或再查计划视图，如实说不支持）；
    /// - 方言取**当前通道**的：源库档用源库的，加速 / 联邦档用 DuckDB 的——那儿才是真要跑的引擎；
    /// - 结果落**新结果集**并贴「执行计划」标题（不抢用户正在看的那份）。
    pub(crate) fn explain_current(&mut self, cx: &mut Context<Self>) {
        if !self.execution_allowed() {
            self.set_message(Some("文本模式不与数据库通信".to_string()), cx);
            return;
        }
        let (text, selection) = self.editor_snapshot(cx);
        let sql = match execution::resolve_target(&text, selection) {
            ExecTarget::Selection(sql) | ExecTarget::Statement(sql) => sql,
            // `resolve_target` 只会给这三种；空文档 / 只有注释的文档没什么可解释的
            _ => {
                self.set_message(Some("没有可生成执行计划的语句".to_string()), cx);
                return;
            }
        };
        let dialect = if self.channel().runs_locally() {
            // 加速 / 联邦：语句（连带计划）都在 DuckDB 上跑，计划就该看 DuckDB 的
            engine::sql::SqlDialect::Duckdb
        } else {
            match self.source_dialect() {
                Ok(dialect) => dialect,
                Err(reason) => {
                    self.set_message(Some(reason), cx);
                    return;
                }
            }
        };
        let Some(explain) = engine::sql::explain_sql(dialect, &sql) else {
            self.set_message(
                Some(format!(
                    "{} 暂不支持生成执行计划（要先改会话开关 / 再查计划视图）",
                    translate::label_of(dialect)
                )),
                cx,
            );
            return;
        };
        if self.execute_labeled(
            ExecTarget::Statement(explain),
            ResultPlacement::NewSet,
            Some("执行计划".to_string()),
            cx,
        ) {
            self.set_message(Some("已提交执行计划（结果落新的结果集）".to_string()), cx);
        }
    }

    /// 【B4】请一个事务动作（开始 / 提交 / 回滚）：状态栏 TX 区的菜单调它
    pub(crate) fn tx_action(&mut self, action: execution::TxAction, cx: &mut Context<Self>) {
        match self
            .shared
            .request_transaction(self.document.clone(), action)
        {
            Ok(()) => {
                self.tx_pending += 1;
                self.set_message(Some(format!("{}…", action.label())), cx);
                self.ensure_exec_pump(cx);
            }
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 【B4】切换自动提交（下一次执行生效）
    pub(crate) fn toggle_autocommit(&mut self, cx: &mut Context<Self>) {
        self.autocommit = !self.autocommit;
        let text = if self.autocommit {
            "自动提交：开（每次执行自成一体）".to_string()
        } else {
            "自动提交：关（执行后停在事务里，待提交/回滚）".to_string()
        };
        self.set_message(Some(text), cx);
    }

    /// 【B4】用一次快照刷新 TX 区（时长从**首次看到**事务开始算）
    fn apply_tx_snapshot(&mut self, snapshot: execution::TxSnapshot) {
        self.tx_open = snapshot.in_transaction;
        match (snapshot.in_transaction, self.tx_since) {
            (true, None) => self.tx_since = Some(std::time::Instant::now()),
            (false, _) => self.tx_since = None,
            _ => {}
        }
    }

    /// TX 区文案（纯文本；按钮另画）：“TX 未开启” / “TX 已开启 3.4s”
    pub(crate) fn tx_text(&self) -> String {
        match (self.tx_open, self.tx_since) {
            (true, Some(since)) => {
                format!("TX 已开启 {}", result_panel::status::elapsed_text(since.elapsed()))
            }
            (true, None) => "TX 已开启".to_string(),
            (false, _) => "TX 未开启".to_string(),
        }
    }

    /// 中断当前执行（B3；原型 §5.1：入口在**状态栏 ■**）
    ///
    /// 同步只回绝“没在跑”；真正的中断在工作线程上做（端口实现允许阻塞）。
    /// 中断之后的剩余语句由通道标“已取消”（不是接着往下跑）。
    pub(crate) fn interrupt(&mut self, cx: &mut Context<Self>) {
        match self.shared.cancel_for(&self.document) {
            Ok(()) => self.set_message(Some("已请求中断…".to_string()), cx),
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 本文档现在能不能中断（状态栏据此决定要不要画■按钮）
    pub(crate) fn can_interrupt(&self) -> bool {
        self.pending > 0
    }

    /// 状态栏那行“执行中 3.4s…”读的耗时（供测试断言“跑完就不再计时”）
    pub fn elapsed_for_test(&self) -> Option<std::time::Duration> {
        self.running_since.map(|since| since.elapsed())
    }

    /// 【B4】TX 区文案（供测试断言）
    pub fn tx_text_for_test(&self) -> String {
        self.tx_text()
    }

    /// 【B4】自动提交开关的当前值
    pub fn autocommit_for_test(&self) -> bool {
        self.autocommit
    }

    /// 【B4】还没回执的事务动作数
    pub fn tx_pending_for_test(&self) -> usize {
        self.tx_pending
    }

    /// 当前模式是否允许执行（**读能力表**，不在视图里另写一份模式判断）
    ///
    /// 文本模式恒为 `false`（“不与数据库通信”是能力表里的硬约束）。
    pub(crate) fn execution_allowed(&self) -> bool {
        self.with_document(|doc| doc.capabilities().execute)
            .unwrap_or(false)
    }

    /// 本文档的文件档位提示（>50MB / ≥200MB；`None` = 不显示提示卡）
    pub fn tier_notice(&self) -> Option<&'static str> {
        self.with_document(|doc| doc.tier_notice()).flatten()
    }

    /// 启动结果轮询（已有存活任务时不重复启动）
    ///
    /// 与 `workbench` 的导航任务同一模式：**后台等、主线程回填**，空闲即退出。
    /// 执行期间顺带 `cx.notify()`：状态栏的耗时累加跟着这个节拍走（不另起一个定时器）。
    /// 【B18】提交一次后台扫描（**每次文本变更**一次；打开文档与重新加载各一次）
    ///
    /// 修订号只升不降（回绕也不怕：比较的是与当前值相等）；范围由档位的降级表回答
    /// （`EditorShared::scan_scope`），不在这里另写判据。
    pub(crate) fn submit_scan(&mut self, text: String, cx: &mut Context<Self>) {
        let scope = self.shared.scan_scope(&self.document);
        let channel = self
            .with_document(|doc| doc.channel())
            .unwrap_or_else(Default::default);
        // 【#54 B】语义诊断目录快照 + 方言：宿主按文档注入过就给（随快照进扫描线程），
        // 没注入 = 空目录 + Ansi（语义段跳过，词法诊断照常）。按文档取：多文档各绑
        // 各连接，扫描不跨文档串目录。
        let catalog = self
            .shared
            .catalog_snapshot_for(&self.document)
            .unwrap_or_else(|| std::sync::Arc::new(crate::completion::Catalog::default()));
        let dialect = self.sql_dialect_for_document();
        self.scan_revision = self.scan_revision.wrapping_add(1);
        self.scanner
            .submit(self.scan_revision, text, channel, scope, catalog, dialect);
        self.scan_pending = true;
        // 扫描不是“执行”，但它也要人按节拍把回执取回来——共用同一条轮询泵
        self.ensure_exec_pump(cx);
    }

    /// 【#54】文档绑定连接的 SQL 方言（未绑定 / 驱动未知 → Ansi 兜底，解析失败返回
    /// None 安全）。hover / 跳转 / 语义诊断共用这一个口径。
    pub(crate) fn sql_dialect_for_document(&self) -> engine::sql::SqlDialect {
        self.with_document(|doc| doc.connection().map(str::to_string))
            .flatten()
            .and_then(|conn_id| {
                self.shared
                    .connection_options()
                    .iter()
                    .find(|option| option.id == conn_id)
                    .and_then(|option| crate::connection::sql_dialect(&option.db_type))
            })
            .unwrap_or(engine::sql::SqlDialect::Ansi)
    }

    pub(crate) fn ensure_exec_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.exec_pump.borrow().as_ref()
            && !task.is_ready()
        {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                // 还有执行在跑（含别的文档）就继续等；一个都没有了就退出，下次提交重新起
                let keep_going = weak
                    .update(cx, |this, cx| {
                        // 【LSP】每拍重问宿主：握手就绪 / 服务被停 / 崩溃重启都在这里收敛
                        this.sync_lsp_session(cx);
                        this.drain_exec_results(cx);
                        if this.pending > 0 || this.tx_pending > 0 || this.refresh_pending > 0 {
                            cx.notify();
                        }
                        // 事务动作 / 重新挂载 / DuckDB 导出的回执还没到也要继续轮询（它们不在“执行忙”里）
                        this.shared.is_executing()
                            || this.tx_pending > 0
                            || this.refresh_pending > 0
                            || this.export_pending > 0
                            // 【B18】后台扫描在途（回执到手就结束，不为扫描留常驻定时器）
                            || this.scan_pending
                            // 【LSP】语言服务是常驻连接：诊断随时会来，会话健康就一直轮询
                            || this.lsp_alive
                            // 【LSP】本该有服务但还没起 / 被停（重启后自动重连靠这条）
                            || this.shared.lsp_wants(&this.document)
                    })
                    .unwrap_or(false);
                if !keep_going {
                    return;
                }
            }
        });
        *self.exec_pump.borrow_mut() = Some(task);
    }

    /// 【LSP】取走语言服务事件并按本文档折叠（诊断 → 三源合并；IoError → 摘服务 / 报告）
    ///
    /// 纯逻辑在 [`crate::lsp::host::fold_events`]（按 `uri` 认领 + 后者覆盖），这里只落面板状态：
    /// 有诊断重画错误标记；服务断开则摘 provider（补全、轮询一起停）并给状态栏原因。
    fn drain_lsp_events(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.lsp.clone() else {
            return;
        };
        let events = self.shared.drain_lsp_events(&self.document);
        if events.is_empty() {
            return;
        }
        let fold = crate::lsp::host::fold_events(&session.uri, events);
        let mut changed = false;
        if let Some(diags) = fold.diagnostics {
            self.lsp_diagnostics = diags;
            changed = true;
        }
        if let Some(reason) = fold.io_error {
            self.lsp_alive = false;
            self.lsp_diagnostics.clear();
            changed = true;
            // 服务死了：清僵尸会话（否则重启时 `same` 判据会挡住重连），并给状态栏原因
            self.lsp.take();
            self.set_message(Some(format!("语言服务已断开：{reason}")), cx);
            // 服务死了：补全不再走语言服务（摘 provider，回到无 / 本地路径）
            self.editor.update(cx, |state, _cx| {
                self.sync_completion_provider(state);
            });
        }
        if changed {
            self.apply_error_marks(self.error_reason.clone(), cx);
            cx.notify();
        }
    }

    /// 取回已完成的执行，填入结果存储与网格
    ///
    /// **一条语句一条结论**：批量执行会连续回来多条，每条各自落一个结果集（落位由 outcome
    /// 自带）。本文档的回填计数减到 0 才算执行完。
    pub(crate) fn drain_exec_results(&mut self, cx: &mut Context<Self>) {
        // 【LSP】语言服务事件先收（诊断异步推送，独立于执行通道）
        self.drain_lsp_events(cx);
        // 【B18】后台扫描的回执先收：**只接受与当前修订号相等的那一份**（陈旧丢弃），
        // 然后三件产物各归各位：语句数进状态栏、候选喂内核、诊断与执行错误一起重画
        if let Some(report) = self.scanner.try_recv() {
            self.scan_pending = false;
            if crate::scan::is_current(&report, self.scan_revision) {
                self.statements = report.statements;
                self.scan_diagnostics = report.diagnostics;
                let folds = report.folds;
                self.editor
                    .update(cx, |state, cx| fold::refresh(state, folds, cx));
                self.apply_error_marks(self.error_reason.clone(), cx);
                cx.notify();
            }
        }
        // 中断尝试的结果先收（失败 / 没在跑 都要留痕，不能默默把按钮点一下就算完）
        if let Some(note) = self.shared.drain_cancel_notes().into_iter().last() {
            self.set_message(Some(note), cx);
        }

        // B4：事务动作的结论（成功/失败都要让界面看见；成功时顺便刷新 TX 区）
        for note in self.shared.drain_tx_notes() {
            self.tx_pending = self.tx_pending.saturating_sub(1);
            if note.document != self.document {
                continue;
            }
            match note.result {
                Ok(snapshot) => {
                    self.apply_tx_snapshot(snapshot);
                    self.set_message(Some(format!("已{}", note.action.label())), cx);
                }
                Err(reason) => {
                    self.set_message(Some(format!("{}失败：{reason}", note.action.label())), cx)
                }
            }
        }

        // 【T2a-6】批量提交的回执（成功/冲突/失败都要说清楚；冲突格在变更集里标出来）
        //
        // 组装规则：执行器按语句顺序报影响行数（`per_statement`），面板拿**生成时存的**
        // 语句→(行,列) 索引（`CommitInFlight::per_line`）对齐——影响行数 0 = 这一行在源库
        // 已定位不到（键值被改/行被删），该行所有变更列标冲突；≥1 = 这一格真的写到了，标已提交。
        for note in self.shared.drain_commit_notes() {
            if note.document != self.document {
                continue;
            }
            match note.result {
                Ok(mut report) => {
                    let Some(inflight) = self.commit_in_flight.take() else {
                        continue; // 过期回执（没有在途提交）——不误清别家状态
                    };
                    // 组装冲突回执（执行器不编造行/列，这里补全）：影响行数 0 = 这一行在源库
                    // 已定位不到（键值被改 / 行被删），该行所有变更列标冲突；≥1 = 这一格真的写到了
                    let mut committed = 0usize;
                    for (i, (row, columns)) in inflight.per_line.iter().enumerate() {
                        let affected = report.per_statement.get(i).copied().unwrap_or(0);
                        for &column in columns {
                            if affected >= 1 {
                                committed += 1;
                                self.shared.update_results(|store| {
                                    store.mutate_change_set(
                                        &note.document,
                                        inflight.set_index,
                                        |changes| changes.mark_committed(*row, column),
                                    );
                                });
                            } else {
                                report.conflicts.push((
                                    *row,
                                    column,
                                    "影响行数 0：源库里的旧值已被别人改过".to_string(),
                                ));
                                self.shared.update_results(|store| {
                                    store.mutate_change_set(
                                        &note.document,
                                        inflight.set_index,
                                        |changes| changes.mark_conflict(*row, column),
                                    );
                                });
                            }
                        }
                    }
                    let mut message = format!("已提交 {committed} 条变更");
                    if !report.conflicts.is_empty() {
                        message.push_str(&format!(
                            "（{} 条冲突：源库里的旧值已被改过）",
                            report.conflicts.len()
                        ));
                    }
                    if inflight.skipped > 0 {
                        message.push_str(&format!("（{} 行键值为空，已跳过）", inflight.skipped));
                    }
                    // 本地新增 / 复制 / 删除的行本期不下发（#28-B1 排除矩阵）：回执如实提示
                    if inflight.local_pending {
                        message.push_str("（本地新增 / 复制 / 删除的行未随本次提交落库）");
                    }
                    // 先同步网格（冲突格标色）再落文案：`sync_result_view` 末尾会按结果成败
                    // 重写状态栏（本次无失败 → `set_message(None)`），顺序反了文案会被清掉
                    self.sync_result_view(cx);
                    self.set_message(Some(message), cx);
                }
                Err(reason) => {
                    self.commit_in_flight = None;
                    self.set_message(Some(format!("批量提交失败：{reason}")), cx);
                }
            }
        }

        // 【B13/T1.6】源动作的回执（成败都要说出来：它是用户按的一个动作）
        for note in self.shared.drain_source_notes() {
            self.refresh_pending = self.refresh_pending.saturating_sub(1);
            if note.document != self.document {
                continue;
            }
            let label = note.action.label();
            match note.result {
                Ok(done) => self.set_message(Some(done), cx),
                Err(reason) => self.set_message(Some(format!("{label}失败：{reason}")), cx),
            }
        }

        // 【B7 切片二】DuckDB 导出（Parquet / XLSX）的回执：文案与同步那条同口径
        for note in self.shared.drain_export_notes() {
            self.export_pending = self.export_pending.saturating_sub(1);
            if note.document != self.document {
                continue;
            }
            match note.result {
                Ok(rows) => {
                    // “（已筛选）”照 B7 的口径明示（导的是筛后的行集）
                    let filtered = if note.filtered { "（已筛选）" } else { "" };
                    // 【D1】猜型降级档在成功文案里追加口径说明：整列按文本存储，数字类型可能失真
                    let degraded = if note.degraded {
                        "；文本猜型降级档：数字列按文本存储"
                    } else {
                        ""
                    };
                    self.set_message(
                        Some(format!(
                            "已导出 {rows} 行{filtered}（{}）{degraded}→ {}",
                            note.format.label(),
                            note.path.display()
                        )),
                        cx,
                    )
                }
                Err(reason) => self.set_message(Some(format!("导出失败：{reason}")), cx),
            }
        }

        // 【2b】脚本执行的结论：独立通道，先于 exec outcomes 收（脚本与 SQL 互不挤占）
        self.drain_script_notes(cx);
        // 【2d】单元执行的结论：独立通道（结果走内联输出，不是结果区）
        self.drain_cell_notes(cx);

        // 多面板共享同一执行队列：只取本文档回执，别文档回执留在队列等归属面板自己取走
        // （否则空闲面板的泵会抢先 drain 走别的文档结论并丢弃，归属面板结果区不再刷新）。
        let outcomes = self.shared.drain_exec_for(&self.document);
        if !outcomes.is_empty() {
            tracing::info!(
                target = "rds_editor::exec",
                doc = %self.document,
                receipts = outcomes.len(),
                "回执泵：取到待回填执行结论"
            );
        }
        let mut mine_arrived = 0usize;
        // 【B6】这一轮新到的失败是哪条语句（回填之后要是有位置就跳过去）
        let mut fresh_failure: Option<String> = None;
        // 【B5b】取下一段失败的原因（要等结果区同步完再说，否则会被“选中那份没有错”冲掉）
        let mut segment_failure: Option<String> = None;
        // 【B14】执行自带的一句说明（如“已去掉原查询的 LIMIT”）——同样要等同步完再说
        let mut fresh_notice: Option<String> = None;
        // 【B14】落在**新结果集**上的失败：它不抢选中（B2 的语义），所以结果区不会报它——
        // 状态栏补一句，否则用户只看到标签上多个红点，不知道为什么
        let mut background_failure: Option<String> = None;
        // 执行失败原因的克隆——用于循环结束后弹全局通知（避免 if let 移动后再用）。
        // 延迟绑定：只在 `mine_arrived > 0` 分支里赋值，同分支消费，无未初始化路径。
        let exec_failure_notice;
        for outcome in outcomes {
            let is_mine = outcome.document == self.document;
            let outcome_ok = outcome.result.is_ok();
            let (cols, rows) = match &outcome.result {
                Ok(data) => (data.columns.len(), data.rows.len()),
                Err(_) => (0, 0),
            };
            tracing::info!(
                target = "rds_editor::exec",
                doc = %self.document,
                receipt_doc = %outcome.document,
                is_mine,
                ok = outcome_ok,
                cols,
                rows,
                placement = ?outcome.placement,
                "回执认领：判定结论归属与形状"
            );
            // 非本文档回执不归本面板处理（`drain_exec_for` 已按文档隔离，这里是双保险）：
            // 不入库、不探测主键 / 注释、不弹在途标题，避免污染本文档结果存储。
            if !is_mine {
                continue;
            }
            mine_arrived += 1;
            // B4：事务状态跟着结论回来（用户手敲 BEGIN / COMMIT 也走同一路径）
            self.apply_tx_snapshot(outcome.transaction);
            // 【B14】提示从**结论**里取（它随这次执行回来，不是猜的）
            if let Ok(data) = outcome.result.as_ref()
                && let Some(notice) = data.notice.clone()
            {
                fresh_notice = Some(notice);
            }
            let placement = outcome.placement;
            // 【B10】在途标题按位弹出
            let title = self.pending_labels.pop_front().flatten();
            // 【B4】物化回执信号先取出（`entry_from` 会消费 `outcome`，之后不能再读）；
            // 失败回执不带表名信号 = `None`，回填段整体跳过。
            let fact_receipt = match &outcome.result {
                Ok(data) => data.source_fact_table.clone().map(|table| {
                    (data.source_fact_set, data.source_fact_release, table)
                }),
                Err(_) => None,
            };
            let (mut entry, fact) = entry_from(outcome);
            if let Some(title) = title {
                entry = entry.with_title(title);
            }
            // 【T2a-2】行标识探测：结果来自哪张表 → 宿主导航缓存查主键（内存读，不阻塞）。
            // 探测不到 = `None` → 判据 `NoRowKey` 如实置只读；有主键但结果没 SELECT 键列
            // → `row_key()` 也为 `None`（写回没有可绑定的值），同样只读。
            entry.key_columns = probe_primary_key(self, &entry);
            // 【P1】字段注释：血缘 + 表列元数据（内存读；长度与 `columns` 一致）
            entry.column_annotations = annotate_columns(self, &entry);
            if entry.failed() {
                // 【B5b】取下一段失败：**已经抓到的行是用户的成果**，不能因为再抓失败就清掉。
                // 所以这次失败不入存储（选中那份原样不动），只把原因说出来。
                if placement == ResultPlacement::Append {
                    segment_failure = Some(format!(
                        "取下一段失败：{}",
                        entry.error.clone().unwrap_or_default()
                    ));
                    continue;
                }
                fresh_failure = Some(entry.sql.clone());
                if placement == ResultPlacement::NewSet {
                    background_failure = Some(format!(
                        "新结果集执行失败：{}",
                        entry.error.clone().unwrap_or_default()
                    ));
                }
            }
            self.shared
                .update_results(|store| store.push(entry, placement, fact));
            // 【B4】分析触发的物化回执：常驻表名回挂**原槽位**（`source_fact_set`），
            // 不挂到本次分析落的新结果集；目标表档时提升档位并释放 Rust 堆 batches。
            if let Some((set, release, table)) = fact_receipt {
                self.shared.update_results(|store| {
                    if release {
                        store.promote_fact_to_table(&self.document, set, table);
                    } else {
                        store.set_fact_table(&self.document, set, table);
                    }
                });
            }
        }
        if mine_arrived > 0 {
            self.pending = self.pending.saturating_sub(mine_arrived);
            if self.pending == 0 {
                self.running_since = None;
                // 【B5b】本文档这一轮执行全部结束：取段也结束了（滚动到底可以再次触发）
                self.fetching_more = false;
            }
            self.sync_result_view(cx);
            // 【B5b】取段失败的原因在这里说（同步结果区会按“选中那份”重写提示）
            let segment_reason = segment_failure.is_some();
            // 先克隆一份用于全局通知——避免下面 if let 移动后再用
            exec_failure_notice = background_failure.clone().or(segment_failure.clone());
            if let Some(reason) = segment_failure {
                self.set_message(Some(reason), cx);
            }
            // 【B14】执行自带的说明（成功且没有失败原因时才说，别把失败提示盖掉）
            if let Some(notice) = fresh_notice
                && self.message.is_none()
            {
                self.set_message(Some(notice), cx);
            }
            // 【B14】没被选中的那份失败了：至少让状态栏说一句
            if let Some(reason) = background_failure
                && self.message.is_none()
            {
                self.set_message(Some(reason), cx);
            }
            // 【B7】导出在跑就推进它（还要抓就再提交一段；抓完了/崩了就落盘或作废）
            self.advance_pending_export(segment_reason, cx);
            // 【B6】失败且能定位：把光标送到出错处并聚焦（拿不到位置就只留原因）
            if let Some(sql) = fresh_failure
                && self.result_panel.read(cx).sql().as_deref() == Some(sql.as_str())
                && self.error_site.is_some()
            {
                self.jump_to_error_site(cx);
            }
            // 执行失败 → 宿主弹全局错误通知（Error 类型，不自动消失）。
            // 用循环内提前克隆的副本，避免变量已被 if let 移动（2026-10-28 修正：
            // 原第二处 if 把引用放到了本块之外 → E0425 作用域不可见）。
            if let Some(reason) = exec_failure_notice {
                cx.emit(EditorHostEvent::ExecutionFailed(reason));
            }
        }
        // 执行收口（executing 徽标 / 耗时消失）：成功无提示时也要通知宿主刷新全局段
        cx.emit(EditorHostEvent::StatusChanged);
    }
}
