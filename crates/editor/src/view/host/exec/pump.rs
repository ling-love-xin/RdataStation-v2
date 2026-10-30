//! pump 域（831-1225 行区间）
//!
//! 自 view/host/exec.rs 拆分（2026-10-29 Step 10）：按能力域切分 `impl EditorHostPanel`。
//! 跨域互调经 `pub(crate)` 方法；本文件不得出现新的逻辑。


use crate::store::ResultPlacement;
use crate::view::fold;

use super::*;

impl EditorHostPanel {

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
    pub(crate) fn drain_lsp_events(&mut self, cx: &mut Context<Self>) {
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
            // 治理（2026-10-30）：事件落 error 日志——通知与日志双写，
            // 页面看到「SQL 执行失败」时日志可查（executor 层已埋，此处补宿主事件级）。
            if let Some(reason) = exec_failure_notice {
                tracing::error!(reason = %reason, "SQL 执行失败（宿主事件）");
                cx.emit(EditorHostEvent::ExecutionFailed(reason));
            }
        }
        // 执行收口（executing 徽标 / 耗时消失）：成功无提示时也要通知宿主刷新全局段
        cx.emit(EditorHostEvent::StatusChanged);
    }
}
