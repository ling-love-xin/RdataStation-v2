// ==================== 任务/Job ====================

use super::*;

impl MockPanel {
    /// 启动后台任务（生成 / 追加 / 出口）：校验输入 → 提交 → 起轮询泵。
    pub(crate) fn start_job(&mut self, kind: MockJobKind, cx: &mut Context<Self>) {
        if self.job.is_some() {
            self.fail("已有任务在进行中（请等它结束或先取消）", cx);
            return;
        }
        // 表配置驱动的任务才校验表配置：场景模板与出口都不读表配置（见 `MockJobKind::uses_draft`），
        // 拿它们的输入拦截会给出与当前动作无关的错误
        if kind.uses_draft() {
            if let Err(e) = self.sync_inputs(cx) {
                self.fail(e, cx);
                return;
            }
            if self.draft.columns.is_empty() {
                self.fail("请先添加列：导入源库结构，或手工加列", cx);
                return;
            }
        }
        if let Err(e) = self.host.start_job(&self.draft, kind.clone()) {
            self.fail(e, cx);
            return;
        }
        // 记下提交时的项目：出口任务的写入路径也是那一刻快照的（工作线程碰不了 `Shared`），
        // 收尾时用它判断归属（见 `finish_job`）
        self.job_project = self.host.project_root();
        let progress = MockJobProgress {
            phase: kind.phase(),
            batches_done: 0,
            batches_total: 0,
            rows_total: kind.rows_total(&self.draft),
        };
        self.error = None;
        self.outcome = Some(kind.running_label());
        self.job = Some(MockJobWatch {
            kind,
            progress,
            cancel_requested: false,
            started_at: Instant::now(),
            unit_ms: None,
            last_sample_at: Instant::now(),
            eta_shown: None,
            _pump: self.spawn_job_pump(cx),
        });
        cx.notify();
    }

    /// 起后台任务轮询泵：每 120ms 拉一次进度 / 结果，任务收尾后自退。
    ///
    /// 用「弱句柄 + 定时器」而非 render 内轮询：任务进行中没有任何其他事件会让
    /// 面板重绘，不主动唤醒就看不到进度。
    pub(crate) fn spawn_job_pump(&self, cx: &mut Context<Self>) -> Task<()> {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(120)).await;
                let keep = weak
                    .update(cx, |panel, cx| panel.poll_job(cx))
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        })
    }

    /// 拉一次任务状态（返回「是否继续轮询」；窗口测试直接调它，不依赖定时器）。
    pub(crate) fn poll_job(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(kind) = self.job.as_ref().map(|job| job.kind.clone()) else {
            return false;
        };

        // 结果优先：worker 收尾时一次性「清进度 + 写结果」，不会出现两者皆无的观测空洞
        if let Some(result) = self.host.take_job_done() {
            self.job = None;
            self.finish_job(&kind, result, cx);
            return false;
        }

        match self.host.job_state() {
            MockJobState::Running(progress) => {
                let mut notify = false;
                if let Some(job) = self.job.as_mut() {
                    // 取消要**持续压**：任务可能跨多个轮询周期才结束（S4 起取消是任务级令牌），
                    // 这里每个轮询周期重发一次直到任务真的结束——`cancel_job` 幂等，重发无副作用。
                    if job.cancel_requested {
                        self.host.cancel_job();
                    }
                    if job.progress != progress {
                        // 每前进一个单位取一次「每单位耗时」样本（EMA 平滑）：
                        // 首单位含建临时表的开销，逐次收敛；否则 ETA 每 120ms 抖一次
                        if progress.batches_done > job.progress.batches_done {
                            job.unit_ms = smooth_unit_ms(
                                job.unit_ms,
                                job.started_at.elapsed(),
                                progress.batches_done,
                            );
                        }
                        job.progress = progress;
                        job.last_sample_at = Instant::now();
                        notify = true;
                    }
                    // ETA 是「还要等多久」，不是「上次算出来多少」：卡住时它每秒往上涨，
                    // 秒数真的变了才唤醒（不然 120ms 一次唤醒，面板白重排 8 次）
                    let eta_secs = job.eta().map(|eta| eta.as_secs());
                    if eta_secs != job.eta_shown {
                        job.eta_shown = eta_secs;
                        notify = true;
                    }
                }
                if notify {
                    cx.notify();
                }
                true
            }
            MockJobState::Idle => {
                // 结果槽空且无进行中任务：后台线程异常（崩溃 / 被杀）
                self.job = None;
                self.fail("后台生成任务异常结束（工作线程已退出）", cx);
                false
            }
        }
    }

    /// 任务收尾：写回结果（成功文案 / 落库目标）或错误。
    ///
    /// **生成类**不论成败都先作废旧结果：任务内部重建过临时表，旧预览与临时表已不一致，
    /// 留着它就会让出口拿新数据配旧预览。**出口类**只读临时表，预览仍然对得上，
    /// 不能一并作废——否则「落库完想接着导出」就没得导了。
    pub(crate) fn finish_job(
        &mut self,
        kind: &MockJobKind,
        result: Result<MockJobDone, String>,
        cx: &mut Context<Self>,
    ) {
        if kind.generates() {
            self.results.clear();
            self.current = 0;
        }
        // 任务是在**哪个项目**下提交的：出口类任务的写入路径在提交那一刻快照（`JobPaths`），
        // 切项目不会改写入点。结果回来时若项目已变，得说清楚，别让用户在新项目里找一张不存在的表
        // （临时表名 / `landed_tables` 也都是上一个项目的）。
        let project_changed = self.job_project != self.host.project_root();
        // 历史在任务收尾时记（写库在后台）：出口类与取消不记，见 `RunRecord::of`。
        // 跨项目的结果**不记**：历史属于项目，把上一个项目跑出来的东西记到当前项目头上是误导。
        let record = if project_changed {
            None
        } else {
            RunRecord::of(kind, &result, &self.draft)
        };
        match result {
            Ok(MockJobDone::Generated(info)) => {
                self.project_switched = false;
                self.landed = None;
                self.error = None;
                self.outcome = Some(format!(
                    "已生成 {} 行（耗时 {} ms）→ 临时表 {}",
                    with_thousands(info.row_count as u64),
                    info.elapsed_ms,
                    info.temp_table_name
                ));
                self.results = vec![info];
                self.current = 0;
                self.scenario_source = None;
                // 单表结果没有跨表关系可言：清掉上一轮的场景关系快照
                self.last_relations.clear();
                self.host.notify(cx);
            }
            Ok(MockJobDone::ScenarioGenerated {
                template_name,
                tables,
            }) => {
                self.project_switched = false;
                self.landed = None;
                self.error = None;
                let total: u32 = tables.iter().map(|table| table.row_count).sum();
                self.outcome = Some(format!(
                    "已按「{template_name}」生成 {} 张表（合计 {} 行）：出口作用于当前选中的那张",
                    tables.len(),
                    with_thousands(total as u64)
                ));
                // 关系快照随结果留存：出口要按它提醒「只落这张会怎么悬空」，
                // 而工作副本随时可能被改或退出场景态
                self.last_relations = self.scenario.as_ref().map(relations_of).unwrap_or_default();
                self.results = tables;
                self.current = 0;
                self.scenario_source = Some(template_name);
                self.host.notify(cx);
            }
            Ok(MockJobDone::Appended { table, total_rows }) => {
                self.landed = Some(table.clone());
                self.error = None;
                self.outcome = Some(format!(
                    "已追加到 {table}（表内共 {} 行）",
                    with_thousands(total_rows.max(0) as u64)
                ));
                self.host.notify(cx);
            }
            Ok(MockJobDone::Persisted { table, rows }) => {
                self.landed = Some(table.clone());
                if !self.landed_tables.contains(&table) {
                    self.landed_tables.push(table.clone());
                }
                // 新表要能立刻作为「追加到既有表」的目标
                self.existing_tables = self.host.existing_tables();
                self.succeed(
                    format!(
                        "已在项目分析库新建表 {table}（{} 行）",
                        with_thousands(rows.max(0) as u64)
                    ),
                    cx,
                );
            }
            Ok(MockJobDone::PersistedAll { landed, failed }) => {
                for (table, _) in &landed {
                    if !self.landed_tables.contains(table) {
                        self.landed_tables.push(table.clone());
                    }
                }
                self.landed = landed.last().map(|(table, _)| table.clone());
                self.existing_tables = self.host.existing_tables();
                let mut parts: Vec<String> = Vec::new();
                if !landed.is_empty() {
                    parts.push(format!(
                        "已落库 {} 张：{}",
                        landed.len(),
                        landed
                            .iter()
                            .map(|(table, _)| table.as_str())
                            .collect::<Vec<_>>()
                            .join("、")
                    ));
                }
                if !failed.is_empty() {
                    parts.push(format!(
                        "失败 {} 张：{}",
                        failed.len(),
                        failed
                            .iter()
                            .map(|(table, reason)| format!("{table}（{reason}）"))
                            .collect::<Vec<_>>()
                            .join("；")
                    ));
                }
                if failed.is_empty() {
                    self.succeed(parts.join("；"), cx);
                } else {
                    // 部分失败：成功的保留（不回滚别人的表），失败的原因原样摆出来
                    self.error = Some(parts.join("；"));
                    self.outcome = None;
                    self.host.notify(cx);
                    cx.notify();
                }
            }
            Ok(MockJobDone::Exported { message }) => self.succeed(message, cx),
            Ok(MockJobDone::ScratchpadAll { saved, failed }) => {
                // 与 PersistedAll 同口径：成功的不回滚，部分失败时把原因摆出来
                let mut parts: Vec<String> = Vec::new();
                if !saved.is_empty() {
                    parts.push(format!(
                        "已存草稿箱 {} 张：{}",
                        saved.len(),
                        saved.join("、")
                    ));
                }
                if !failed.is_empty() {
                    parts.push(format!(
                        "失败 {} 张：{}",
                        failed.len(),
                        failed
                            .iter()
                            .map(|(table, reason)| format!("{table}（{reason}）"))
                            .collect::<Vec<_>>()
                            .join("；")
                    ));
                }
                if failed.is_empty() {
                    self.succeed(parts.join("；"), cx);
                } else {
                    self.error = Some(parts.join("；"));
                    self.outcome = None;
                    self.host.notify(cx);
                    cx.notify();
                }
            }
            Err(e) => {
                let cancelled = e.contains("取消");
                // 同名表已存在是落库失败的常见情形：刷新清单，引导到「追加到既有表」
                if !cancelled && matches!(kind, MockJobKind::Persist(_)) && e.contains("已存在")
                {
                    self.existing_tables = self.host.existing_tables();
                }
                self.outcome = None;
                self.error = Some(match (kind, cancelled) {
                    // 用户主动取消：不当错误报，但要提醒临时表可能残留部分行
                    (_, true) => format!("{e}（临时表可能残留部分行，下次生成会重建）"),
                    // 追加失败与生成失败区分开：追加还多一层「写入既有表」的语义
                    (MockJobKind::AppendTo(_), false) => format!("追加失败：{e}"),
                    (_, false) => e,
                });
            }
        }
        cx.notify();
        if let Some(run) = record {
            let draft = self.draft.clone();
            self.spawn_history(HistoryTask::Record { draft, run }, cx);
        }
        // 项目已变：给已写好的成败文案追加一句归属说明（结果本身仍展示——它就是刚跑出来的）
        if project_changed {
            let note =
                "（注意：这个任务是在上一个项目下提交的，写入的也是那个项目；当前项目没有变化）";
            match (&mut self.outcome, &mut self.error) {
                (Some(text), _) => text.push_str(note),
                (None, Some(text)) => text.push_str(note),
                (None, None) => self.outcome = Some(note.trim_start_matches('（').to_string()),
            }
        }
        // 全局通知：任务收尾说一句。中央 tab 的状态格只有「那张 tab 正开着」才看得见，
        // 批量落库 / 场景生成这类要跑一阵的任务，用户多半已经切到别处了，
        // 结果与失败必须还能追到（通知中心里留着，不用回来盯进度条）。
        // 放在最后：`project_changed` 的归属说明也要进通知正文。
        self.notify_job_result(kind, cx);
    }

    /// 把任务收尾结果推给全局通知系统（标题按动作类型，正文按结果）。
    ///
    /// 只在**收尾那一刻**推一次：进度不推——每批一条通知会把通知中心刷成进度墙，
    /// 实时进度在中央动作行右侧的状态格里看。
    /// 三种收尾态与字段一一对应：`succeed` 只设 `outcome`、`fail` 只设 `error`，
    /// 所以这里按字段读即可；部分失败（批量出口）也是 `error`，不再多编一个 warning 档。
    pub(crate) fn notify_job_result(&mut self, kind: &MockJobKind, cx: &mut Context<Self>) {
        let title = match kind {
            MockJobKind::Generate => "Mock 生成",
            MockJobKind::Scenario(_) => "Mock 场景生成",
            MockJobKind::AppendTo(_) => "Mock 追加",
            MockJobKind::Persist(_) => "Mock 落库",
            MockJobKind::PersistAll(_) => "Mock 批量落库",
            MockJobKind::Export { .. } => "Mock 导出",
            MockJobKind::Scratchpad { .. } => "Mock 存草稿箱",
            MockJobKind::ScratchpadAll { .. } => "Mock 批量存草稿箱",
        };
        // 取消：引擎回的 Err 里带「取消」；用户自己停的不算错误，按 info 报
        let cancelled = self
            .error
            .as_deref()
            .is_some_and(|err| err.contains("取消"));
        if let Some(err) = self.error.clone() {
            if cancelled {
                notification::notify_info(title, Some(err), cx);
            } else {
                notification::notify_error(title, Some(err), cx);
            }
        } else if let Some(message) = self.outcome.clone() {
            notification::notify_success(title, Some(message), cx);
        }
    }

    /// 请求取消进行中的任务：只有**含生成阶段**的任务能取消（引擎按批响应）。
    ///
    /// 出口类跑在 DuckDB / 文件系统内部，探不到中断点，强杀会留下半张表或半个文件，
    /// 所以不给取消：按钮也不渲染（见 `render_job_row`）。
    pub fn cancel_job(&mut self, cx: &mut Context<Self>) {
        let Some(job) = self.job.as_mut() else {
            return;
        };
        if !job.kind.generates() || job.cancel_requested {
            return;
        }
        job.cancel_requested = true;
        self.host.cancel_job();
        cx.notify();
    }
}
