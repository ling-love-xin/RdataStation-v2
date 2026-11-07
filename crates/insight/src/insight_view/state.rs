use super::*;

impl InsightView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            target: None,
            state: InsightPanelState::Empty,
            data: PanelData::default(),
            tab: PanelTab::Column,
            open_sections: ColumnSection::default_open(),
            // 保守初值：宿主装配时会立刻告知真实项目状态（`set_project_open`）。
            // 宁可让依赖项目的入口先禁用，也不要给一个点了没用的按钮。
            project_open: false,
            // 无 I/O：真正的取数与写库在 `jobs::attach_rules` 接到事件之后
            rules: cx.new(RulesView::new),
            multi_selected: Vec::new(),
            multi_rule: None,
            multi_running: false,
            multi_notice: None,
            history_saving: false,
            history_cleaning: false,
            compare_target: None,
            history_notice: None,
            open_schema_sections: [true; SchemaSection::ALL.len()],
            loading_note: None,
            insight_set: Vec::new(),
            set_multi: false,
            selected_tables: std::collections::HashSet::new(),
            overlap: None,
            overlap_running: false,
            overlap_notice: None,
            scope: InsightScope::Sample,
            probe_dialog: None,
            last_probe_config: None,
            eval_cancel: Arc::new(AtomicBool::new(false)),
            // 初值与设置默认一致（宿主装配时会立刻用真实设置覆盖：`apply_insight_prefs`）
            status_usage: true,
            work_set_limit: 30,
            memory: None,
            memory_poll_running: false,
        }
    }

    /// 已洞察表集合（右栏渲染用）
    pub fn insight_set(&self) -> &[InsightSetEntry] {
        &self.insight_set
    }

    /// 宿主推送洞察表集合快照（中间页 `opened` 变化时调用；内容不变不重绘）。
    ///
    /// 在事件路径调用（`observe` 回调），只更新 Shared + `cx.notify()`。
    ///
    /// 顺带把集合同步成临时表的**保护名单**（T21）：集合内的表是用户的会话工作集
    /// （可能随时对比 / 归档），TTL 与上限的惰性清理必须跳过它们；离开集合的表
    /// 解除保护，恢复 30 分钟 TTL 管理。保护名单是内存登记，不是 I/O。
    pub fn set_insight_set(&mut self, entries: Vec<InsightSetEntry>, cx: &mut Context<Self>) {
        let manager = engine::DuckDBManager::temp_table_manager();
        let incoming: std::collections::HashSet<&str> = entries
            .iter()
            .filter_map(|entry| entry.temp_table.as_deref())
            .collect();
        for entry in &self.insight_set {
            if let Some(name) = entry.temp_table.as_deref() {
                if !incoming.contains(name) {
                    manager.unprotect(name);
                }
            }
        }
        for name in incoming {
            manager.protect(name);
        }
        if self.insight_set != entries {
            self.insight_set = entries;
            cx.notify();
        }
    }

    /// 集合多选模式开关（原型 it-head「多选」）。
    ///
    /// 关闭时清空勾选（勾选只在多选语境有意义，留着一批「上一轮选中的表」
    /// 会让质量对比按钮误亮）。
    pub fn toggle_multi_mode(&mut self, cx: &mut Context<Self>) {
        self.set_multi = !self.set_multi;
        if !self.set_multi {
            self.selected_tables.clear();
        }
        cx.notify();
    }

    /// 多选模式（渲染 / 测试读）
    pub fn multi_mode(&self) -> bool {
        self.set_multi
    }

    /// 勾选 / 取消勾选集合中的一张表（按条目 key）。
    pub fn toggle_table_selection(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.selected_tables.remove(key) {
            self.selected_tables.insert(key.to_string());
        }
        cx.notify();
    }

    /// 已勾选的表 key 集合（渲染 / 测试读）
    pub fn selected_tables(&self) -> &std::collections::HashSet<String> {
        &self.selected_tables
    }

    /// 多选质量对比：对每张勾选表逐个发「加入中间页对比候选并切对比形态」。
    ///
    /// 复用单表 `CompareRequested`（宿主内部累积候选，去重由中间页状态负责）——
    /// 不新开批量事件，保证「对比」只有一条宿主链路。
    pub fn request_quality_compare(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.selected_tables.iter().cloned().collect();
        if keys.is_empty() {
            return;
        }
        for key in keys {
            cx.emit(InsightEvent::CompareRequested { key });
        }
        cx.notify();
    }

    /// 关系排查结果（右栏集合卡「关系排查」区渲染；`None` = 未排查过）
    pub fn overlap(&self) -> Option<&OverlapView> {
        self.overlap.as_ref()
    }

    /// 关系排查执行中（结果区进度提示）
    pub fn overlap_running(&self) -> bool {
        self.overlap_running
    }

    /// 关系排查失败提示（结果区说明；`None` = 无失败）
    pub fn overlap_notice(&self) -> Option<&str> {
        self.overlap_notice.as_deref()
    }

    /// 回填一次关系排查结果（作业线程经 `jobs` 调）。
    ///
    /// 顺带收起 loading 与清掉旧失败提示——结果与「正在跑」互斥。
    pub fn set_overlap(&mut self, view: OverlapView, cx: &mut Context<Self>) {
        self.overlap = Some(view);
        self.overlap_running = false;
        self.overlap_notice = None;
        cx.notify();
    }

    /// 关系排查进行中（作业线程开始时置位）。
    pub fn begin_overlap(&mut self, cx: &mut Context<Self>) {
        self.overlap_running = true;
        self.overlap_notice = None;
        cx.notify();
    }

    /// 关系排查失败（作业线程报错时回填说明，保留上一次结果）。
    pub fn set_overlap_notice(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.overlap_running = false;
        self.overlap_notice = Some(message.into());
        cx.notify();
    }

    /// 收起关系排查结果区（结果 / 失败提示一起清）。
    pub fn dismiss_overlap(&mut self, cx: &mut Context<Self>) {
        self.overlap = None;
        self.overlap_running = false;
        self.overlap_notice = None;
        cx.notify();
    }

    /// 规则管理对话框实体（宿主用它接 `insight::jobs::attach_rules`）。
    pub fn rules_view(&self) -> &Entity<RulesView> {
        &self.rules
    }

    pub fn target(&self) -> Option<&InsightTarget> {
        self.target.as_ref()
    }

    pub fn state(&self) -> &InsightPanelState {
        &self.state
    }

    pub fn tab(&self) -> PanelTab {
        self.tab
    }

    /// 四区展开态（测试与宿主用）
    pub fn sections_open(&self) -> [bool; 4] {
        self.open_sections
    }

    /// 是否已打开项目（决定无项目提示；Phase 2 接入规则管理后也会约束 ⚙）
    pub fn project_open(&self) -> bool {
        self.project_open
    }

    /// 宿主告知项目开关（打开 / 切换 / 关闭项目时调用）
    pub fn set_project_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.project_open != open {
            self.project_open = open;
            cx.notify();
        }
    }

    /// 指向新目标：切到该目标的默认 Tab、清掉旧载荷、进入加载态，并请宿主取数。
    ///
    /// **清载荷是必需的**：载荷按 Tab 分开存，而它们都只对**旧目标**成立——
    /// 留着旧载荷会直接渲染出上一个目标的数据（比空白更坏：看起来像新目标的结果）。
    /// 折叠偏好不在此列：它们是用户口味，不随目标变。
    pub fn set_target(&mut self, target: InsightTarget, cx: &mut Context<Self>) {
        tracing::info!(target = %target.title(), "【探针】洞察面板目标切换");
        // 源目标：进入洞察先弹**探查确认窗口**（C3）——数据量 / 列由用户定（C1），
        // 确认前不取数、取消幂等（不建表不改状态）。临时表目标数据已既定，直接取数。
        if target.source().is_some() {
            self.open_probe_dialog(target, cx);
            return;
        }
        self.apply_target_and_request(target, cx);
    }

    /// 已确认目标的直接取数路径（切 tab / 关 tab / 候选重开等会话内导航复用）。
    ///
    /// `opened` 里已登记的目标 = 本次会话确认过（C3 只拦新进入洞察的源目标），
    /// 导航复用不再重复弹窗。
    pub fn set_target_loaded(&mut self, target: InsightTarget, cx: &mut Context<Self>) {
        tracing::info!(target = %target.title(), "【探针】洞察已确认目标直接取数（会话内导航）");
        self.apply_target_and_request(target, cx);
    }

    /// 切目标 + 发当前 Tab 取数请求（确认窗口 / 已登记目标 / 临时表目标共用）。
    fn apply_target_and_request(&mut self, target: InsightTarget, cx: &mut Context<Self>) {
        self.tab = target.default_tab();
        self.target = Some(target);
        self.data = PanelData::default();
        // 取样口径跟着目标重置（新目标默认取样）；对比基准只对旧目标成立，跟着载荷清
        self.scope = InsightScope::Sample;
        self.compare_target = None;
        // 阶段说明随目标生命周期走：旧目标的「取样中 / 画像中」提示不得挂到新目标上（K22）
        self.loading_note = None;
        self.state = InsightPanelState::Loading;
        if self.emit_request_for_tab(self.tab, cx) {
            cx.notify();
        }
    }

    // ===== 探查确认窗口（C3：进入洞察确认后取数；C6：重取数/重探查复用） =====

    /// 确认窗口是否开着（宿主联动用：确认阶段不推右栏——2026-11-05 循环修复）。
    pub fn probe_confirm_open(&self) -> bool {
        self.probe_dialog.is_some()
    }

    /// 确认窗口只读访问（render 纯读路径用）。
    pub(crate) fn probe_dialog(&self) -> Option<&ProbeDialogState> {
        self.probe_dialog.as_ref()
    }

    /// 打开探查确认窗口（源目标进入洞察 / 重取数 / 重新探查共用）。
    ///
    /// 目标**先不落** `self.target`：取消时面板回到无目标空态（幂等）；
    /// 确认后经 [`Self::confirm_probe_dialog`] 才真正切目标并带配置发取数请求。
    /// 打开同时发起列候选的轻量内省（`ProbeColumnsRequested`，失败默认全部列）。
    pub(crate) fn open_probe_dialog(&mut self, target: InsightTarget, cx: &mut Context<Self>) {
        tracing::info!(target = %target.title(), "【探针】洞察打开探查确认窗口");
        if let Some(source) = target.source() {
            cx.emit(InsightEvent::ProbeColumnsRequested {
                source: source.clone(),
            });
        }
        self.probe_dialog = Some(ProbeDialogState::new(
            target,
            self.last_probe_config.as_ref(),
        ));
        self.state = InsightPanelState::ProbeConfirm;
        cx.notify();
    }

    /// 确认探查：编辑态 → 配置 → 切目标并发取数请求。
    ///
    /// 解析失败写行内提示并保留窗口（不取数）；成功则窗口关闭，
    /// 配置记入会话级记忆（R8）供下次预填。
    /// 已登记目标 + 已有画像时**保留旧载荷**（R5 不闪空）：重跑建表，新表就绪后
    /// jobs 一次性替换；只更新取样口径与阶段条。新目标走 [`Self::apply_target_and_request`]
    /// 清空旧载荷（跨目标不串数据）。
    /// 返回「窗口是否应关闭」：解析失败（行内提示已写）为 `false`——对话框
    /// 按钮条的 `on_ok` 以它决定关不关；表单内调用丢弃返回值（状态自洽）。
    pub(crate) fn confirm_probe_dialog(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(mut dialog) = self.probe_dialog.take() else {
            return false;
        };
        let Some(config) = dialog.to_config() else {
            // 解析失败：行内提示已写入，窗口保留
            self.probe_dialog = Some(dialog);
            cx.notify();
            return false;
        };
        let target = dialog.target.clone();
        self.last_probe_config = Some(config.clone());
        // 已登记目标 + 已有画像 = 重新探查（C6 / R5）：不重设目标、不清载荷
        let re_probe = self
            .target
            .as_ref()
            .map(|t| t == &target && self.data.table.is_some())
            .unwrap_or(false);
        if re_probe {
            self.scope = InsightScope::Sample;
            self.loading_note = Some("重新探查中…".into());
            cx.emit(InsightEvent::ProfileRequested {
                target,
                config: Some(config),
            });
            cx.notify();
            return true;
        }
        // 新目标：切默认 Tab、清旧载荷、取样口径、清加载说明
        self.apply_target_and_request(target.clone(), cx);
        cx.emit(InsightEvent::ProfileRequested {
            target,
            config: Some(config),
        });
        cx.notify();
        true
    }

    /// 取消探查：关闭窗口、清目标（幂等，不建表不改状态）。
    pub(crate) fn cancel_probe_dialog(&mut self, cx: &mut Context<Self>) {
        if self.probe_dialog.is_none() {
            return;
        }
        tracing::info!("【探针】探查确认取消（幂等：不建表不改状态）");
        self.probe_dialog = None;
        self.target = None;
        self.data = PanelData::default();
        self.state = InsightPanelState::Empty;
        cx.notify();
    }

    /// 回填确认窗口的列候选（`ProbeColumnsRequested` 的后台结果）。
    ///
    /// 候选只在确认窗口还开着时回填；用户已取消则丢弃（目标都没了，列清单无意义）。
    pub(crate) fn set_probe_columns(&mut self, columns: Vec<String>, cx: &mut Context<Self>) {
        let Some(dialog) = self.probe_dialog.as_mut() else {
            return;
        };
        dialog.column_candidates = columns;
        cx.notify();
    }

    /// 编辑：数据量模式（全量 / 行数 / 百分比）。
    pub(crate) fn probe_set_amount_mode(&mut self, mode: ProbeAmountMode, cx: &mut Context<Self>) {
        let Some(dialog) = self.probe_dialog.as_mut() else {
            return;
        };
        dialog.amount_mode = mode;
        dialog.parse_notice = None;
        cx.notify();
    }

    /// 快捷预设：常用行数 / 常用百分比（确认窗口「数据量」块的快捷入口）。
    pub(crate) fn probe_quick_amount(
        &mut self,
        mode: ProbeAmountMode,
        value: String,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.probe_dialog.as_mut() else {
            return;
        };
        dialog.amount_mode = mode;
        match mode {
            ProbeAmountMode::Full => {}
            ProbeAmountMode::RowCount => dialog.row_count = value,
            ProbeAmountMode::Percent => dialog.percent = value,
        }
        dialog.parse_notice = None;
        cx.notify();
    }

    /// 编辑：行数文本（RowCount 模式）。
    pub(crate) fn probe_set_row_count(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some(dialog) = self.probe_dialog.as_mut() else {
            return;
        };
        dialog.row_count = value.to_string();
        dialog.parse_notice = None;
        cx.notify();
    }

    /// 编辑：百分比文本（Percent 模式）。
    pub(crate) fn probe_set_percent(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some(dialog) = self.probe_dialog.as_mut() else {
            return;
        };
        dialog.percent = value.to_string();
        dialog.parse_notice = None;
        cx.notify();
    }

    /// 编辑：采样方式（Head 前 N 行 / Reservoir 随机均匀）。
    pub(crate) fn probe_set_sample_mode(
        &mut self,
        mode: crate::model::ProbeSampleMode,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.probe_dialog.as_mut() else {
            return;
        };
        dialog.sample_mode = mode;
        dialog.parse_notice = None;
        cx.notify();
    }

    /// 编辑：列集合（未选列在 EDA 上不存在，C4）。
    pub(crate) fn probe_set_columns(
        &mut self,
        columns: crate::model::ProbeColumns,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = self.probe_dialog.as_mut() else {
            return;
        };
        dialog.columns = columns;
        dialog.parse_notice = None;
        cx.notify();
    }

    /// 编辑：过滤条件（R1，包内层 SELECT）。
    pub(crate) fn probe_set_filter(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some(dialog) = self.probe_dialog.as_mut() else {
            return;
        };
        dialog.filter = value.to_string();
        dialog.parse_notice = None;
        cx.notify();
    }

    /// 编辑：reservoir 采样种子文本。
    pub(crate) fn probe_set_seed(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some(dialog) = self.probe_dialog.as_mut() else {
            return;
        };
        dialog.seed = value.to_string();
        dialog.parse_notice = None;
        cx.notify();
    }

    /// 清空当前目标（多标签关掉最后一个 tab 时用）：置空态、不发取数请求。
    ///
    /// 与 [`Self::set_target`] 的区别：没有新目标可指，只是把面板退回「无目标」
    /// 初始态——右栏摘要版随之显示空态（原型 `archiveView` 之下的空工作台）。
    pub fn clear_target(&mut self, cx: &mut Context<Self>) {
        self.target = None;
        self.data = PanelData::default();
        self.scope = InsightScope::Sample;
        self.compare_target = None;
        self.loading_note = None;
        self.state = InsightPanelState::Empty;
        cx.notify();
    }

    /// 当前取样口径（中间页 scope 切换按钮读它；测试与宿主读）。
    pub fn scope(&self) -> InsightScope {
        self.scope
    }

    /// 宿主装配 / 设置变更时推送洞察偏好（对应设置页 `insight.*`）。
    ///
    /// `insight` 不依赖 `settings`（依赖只向下），设置值由宿主读取后转发到这里；
    /// 关闭「显示内存占用」时清空读数，轮询任务也会在下一次采样时自行退出。
    pub fn apply_insight_prefs(
        &mut self,
        status_usage: bool,
        work_set_limit: usize,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(
            status_usage,
            work_set_limit,
            "【探针】洞察设置项变更（宿主转发 prefs）"
        );
        self.status_usage = status_usage;
        self.work_set_limit = work_set_limit;
        if !status_usage {
            self.memory = None;
        } else {
            // 从关到开：轮询可能已退出，重新拉起（内部防重入）
            self.ensure_memory_poll(cx);
        }
        cx.notify();
    }

    /// 启动内存轮询（每 5 秒采样一次分析引擎内存读数，写回 `memory`）。
    ///
    /// 采样在后台执行器跑（`memory_stats` 拿全局连接锁 + 两条 SQL，不占 UI 线程）；
    /// `status_usage` 关闭或面板销毁时，`weak.update` 的返回会停下循环。
    /// 轮询是事件路径（`cx.spawn` 回调更新状态 + notify），不违反 render 纯读约束。
    pub(crate) fn ensure_memory_poll(&mut self, cx: &mut Context<Self>) {
        if self.memory_poll_running {
            return;
        }
        self.memory_poll_running = true;
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            loop {
                // 先采样再睡：启动即有读数，不必等第一轮 5 秒
                let stats = executor
                    .spawn(async {
                        engine::services::duckdb_service::DuckDbService::memory_stats()
                    })
                    .await
                    .ok();
                let keep = weak.update(cx, |view, cx| {
                    if !view.status_usage {
                        view.memory = None;
                        view.memory_poll_running = false;
                        cx.notify();
                        return false;
                    }
                    view.memory = stats.clone();
                    cx.notify();
                    true
                });
                // 实体已销毁（weak 升级失败）或偏好已关闭 → 退出轮询
                if !matches!(keep, Ok(true)) {
                    break;
                }
                executor.timer(std::time::Duration::from_secs(5)).await;
            }
        })
        .detach();
    }

    /// 切换为**全量**口径并重新画像（大表手动全量：源目标有效）。
    ///
    /// 非源目标（临时表 / 列 / 多列 / Schema）没有「取样」概念，静默忽略——
    /// 它们的数据已是既定内容，无需也不应再全量取一遍。
    ///
    /// 源目标：先请宿主**确认**（大表全量慢且占内存；设置「全量统计前确认」
    /// 关闭时宿主直接执行）。确认后由宿主调 [`Self::confirm_full`] 真正切口径。
    pub fn request_full(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        if !matches!(
            target,
            InsightTarget::SourceTable { .. } | InsightTarget::SourceColumn { .. }
        ) {
            return;
        }
        cx.emit(InsightEvent::FullProfileConfirmRequested { target });
        cx.notify();
    }

    /// 宿主确认全量后调用：切**全量口径**并评估确认窗口生成的表。
    ///
    /// 与 [`Self::request_full`] 是两段式的后半段（确认弹窗在宿主侧）；取消时宿主
    /// 不调本方法，scope / 状态保持原样。
    ///
    /// 2026-10-31 Q2 定稿：全量 = 评估确认窗口生成的表，**不再重取源全量建表**
    /// （确认窗口的行数/百分比已落到建表层）；若该表尚未评估则触发评估全表，
    /// 已评估只切口径（#23 自动评估已承接进入 EDA 的默认评估）。
    pub fn confirm_full(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        if !matches!(
            target,
            InsightTarget::SourceTable { .. } | InsightTarget::SourceColumn { .. }
        ) {
            return;
        }
        tracing::info!(target = %target.title(), "【探针】洞察确认全量口径（confirm_full）");
        self.scope = InsightScope::Full;
        // 已有评估产物时只切口径（数据不动）；未评估（含探查刚回填）则续跑评估。
        let evaluated = self
            .data
            .table
            .as_ref()
            .map(|t| t.quality.is_some())
            .unwrap_or(false);
        if !evaluated {
            self.request_table_evaluation(cx);
        } else {
            cx.notify();
        }
    }

    /// 切换回**取样档**（Q2 定稿）：对当前评估全表做查询级过滤现算。
    ///
    /// 旧行为是重新取样建表（重取源）；新语义取样 = 对**评估全表**（确认窗口
    /// 生成的临时表）的行 LIMIT + 列投影轻量现算（不建子表、不重取源），迷你统计
    /// 随查询回填（C7）。保持当前内容不闪空（R5）：现算毫秒级，不发 Loading。
    pub fn request_sample(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        let Some(temp_table) = self.data.sample_table(&target).map(str::to_string) else {
            return;
        };
        tracing::info!(target = %target.title(), "【探针】洞察切回取样档（request_sample）");
        self.scope = InsightScope::Sample;
        cx.emit(InsightEvent::SampleInsightRequested { temp_table });
        cx.notify();
    }

    /// 取样档迷你统计回填（Q3 / C7：迷你统计并入取样链路）。
    ///
    /// 只写 [`TableColumnView::mini_stats`]，**不覆盖评估产物**（stats_detail /
    /// histogram / sample_values / quality）：全量档读评估明细、取样档读本字段，
    /// 两档互不污染。按列名对位回填，未知列名忽略（接缝与面板共用同一份元数据，
    /// 理论上不出现，防御不 panic）。
    pub fn set_sample_mini_stats(
        &mut self,
        stats: Vec<(String, Option<crate::model::ColumnMiniStats>)>,
        cx: &mut Context<Self>,
    ) {
        if let Some(profile) = &mut self.data.table {
            for (name, mini) in stats {
                if let Some(col) = profile.columns.iter_mut().find(|c| c.name == name) {
                    col.mini_stats = mini;
                }
            }
        }
        cx.notify();
    }

    /// 导出当前表画像为 Markdown 报告（宿主选路径并写盘）。
    ///
    /// 无目标时静默忽略（工具栏按钮只在画像态可点，这里是防御）；`content`
    /// 由中间页算好（渲染的同一份事实，D36），事件只搬运不重算。
    pub fn request_report_export(&mut self, content: String, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        let file_stem = Self::sanitize_stem(&target.title());
        cx.emit(InsightEvent::TableReportExportRequested {
            target,
            file_stem,
            content,
        });
        cx.notify();
    }

    /// 导出文件名主名净化（与 Schema 报告同一口径：只换非法字符、保留中文，
    /// 空名兜底 `insight`）。保存对话框里用户要看得懂名字，不 ASCII 化。
    fn sanitize_stem(name: &str) -> String {
        let cleaned: String = name
            .trim()
            .chars()
            .map(|c| {
                if c.is_control() || "\\/:*?\"<>|".contains(c) {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        let cleaned = cleaned.trim_matches(|c: char| c == ' ' || c == '-' || c == '.');
        if cleaned.is_empty() {
            "insight".into()
        } else {
            cleaned.into()
        }
    }

    /// 取样来源解析出样本表（接缝侧取样后回填）：源目标的保存 / 多列 / 下钻都读它。
    pub fn set_source_sample(&mut self, temp_table: impl Into<String>, cx: &mut Context<Self>) {
        self.data.source_sample = Some(temp_table.into());
        cx.notify();
    }

    /// 更新加载期的阶段说明（jobs 分阶段调用：取样中 → 画像中）。
    ///
    /// `None` 清掉说明（切到非源目标时恢复纯骨架）。说明只是加载态的文字，
    /// 不改变 `state`——它必须仍是 `Loading` 才被渲染。
    pub fn set_loading_note(&mut self, note: Option<SharedString>, cx: &mut Context<Self>) {
        self.loading_note = note;
        cx.notify();
    }

    /// 当前加载期的阶段说明（`None` = 纯骨架；测试与宿主读）。
    pub fn loading_note(&self) -> Option<&SharedString> {
        self.loading_note.as_ref()
    }

    /// 取样口径的人话标签（加载进度卡 / 右栏摘要卡 / 中间页五卡共用）。
    ///
    /// 取数已配置化（`ProbeConfig`：全量 / 前 N 行 / 百分比），各处**不能再写死**
    /// `SOURCE_SAMPLE_LIMIT`——那会在用户选 1000 行或百分比时给出错误读数。
    /// `None` = 本次会话还没确认过探查配置（按默认值说明）。
    pub fn sample_amount_label(&self) -> String {
        match self.last_probe_config.as_ref().map(|c| &c.amount) {
            Some(crate::model::ProbeAmount::Full) => "全量（不设 LIMIT）".to_string(),
            Some(crate::model::ProbeAmount::RowCount(n)) => format!("前 {n} 行"),
            Some(crate::model::ProbeAmount::Percent(p)) => format!("约 {p}%"),
            None => format!(
                "前 {} 行（默认）",
                crate::service::persistence::SOURCE_SAMPLE_LIMIT
            ),
        }
    }

    /// 数据态载荷（宿主与测试读；三个 Tab 各自的最近一次结果）
    pub fn data(&self) -> &PanelData {
        &self.data
    }

    /// 出数（Schema 健康报告）
    pub fn set_schema_report(
        &mut self,
        report: crate::schema_view::SchemaReportView,
        cx: &mut Context<Self>,
    ) {
        self.data = std::mem::take(&mut self.data).with_schema(report);
        self.state = InsightPanelState::Data;
        cx.notify();
    }

    /// 结构 Tab 的表格下钻请求（点报告里的表名）
    pub fn request_table_drilldown(&mut self, table: impl Into<String>, cx: &mut Context<Self>) {
        let Some(InsightTarget::Schema {
            conn_id,
            database,
            schema,
        }) = self.target.clone()
        else {
            return;
        };
        cx.emit(InsightEvent::TableDrilldownRequested {
            conn_id,
            database,
            schema: schema.unwrap_or_default(),
            table: table.into(),
        });
    }

    /// 结构 Tab 的导出请求（Phase 4.3：导出 JSON / Markdown）。
    ///
    /// 内容在这里算好（与界面同源的视图模型，D36）；没有报告时不发事件——
    /// 导出按钮只在有报告时渲染，这里是兼底。
    pub fn request_schema_export(&mut self, format: SchemaExportFormat, cx: &mut Context<Self>) {
        let Some(report) = self.data.schema.as_ref() else {
            return;
        };
        cx.emit(InsightEvent::SchemaExportRequested {
            format,
            file_stem: crate::schema_view::schema_export_file_stem(&report.schema_name),
            content: match format {
                SchemaExportFormat::Json => report.to_json(),
                SchemaExportFormat::Markdown => report.to_markdown(),
            },
        });
    }

    /// 出数（快照历史）。
    ///
    /// 保存中标记在出数时落回：它挂在「保存动作」上，不挂在面板状态上
    /// （面板状态只有四态，多一个 `Saving` 会让渲染分派多出一条不可能的分支）。
    ///
    /// 对比基准**从载荷反推**（`history.diff`）：列表刷新后「最新」就变了，
    /// 留着一个指向旧「当前」的选中位比不选中更坏（D43）。
    pub fn set_history(&mut self, history: HistoryView, cx: &mut Context<Self>) {
        self.compare_target = history
            .diff
            .as_ref()
            .map(|diff| diff.baseline_version.clone());
        self.data = std::mem::take(&mut self.data).with_history(history);
        self.history_saving = false;
        self.history_cleaning = false;
        self.history_notice = None;
        self.state = InsightPanelState::Data;
        cx.notify();
    }

    /// 历史 Tab 的版本行被点：选/取消对比基准（方向固定为「选中 → 最新」）。
    ///
    /// 只改选择位并发事件，差值由接缝读两版正文后回填（渲染路径零 I/O）。
    pub fn toggle_compare_version(&mut self, version_id: &str, cx: &mut Context<Self>) {
        if self.compare_target.as_deref() == Some(version_id) {
            self.dismiss_diff(cx);
            return;
        }
        // 最新一版没有更新的版本可比（列表行也不给它点击入口，这里是状态层的兜底）
        if self.is_latest_version(version_id) {
            return;
        }
        let Some(InsightTarget::Column { column, .. }) = self.target.clone() else {
            return;
        };
        self.compare_target = Some(version_id.to_string());
        // 上一个基准的差值已经不成立了：先放掉，渲染依此显示「读取版本正文…」
        self.clear_diff();
        cx.emit(InsightEvent::VersionCompareRequested {
            column,
            version_id: version_id.to_string(),
        });
        cx.notify();
    }

    /// 关掉对比面板（点 ✕ 或再点一次选中的那一行）
    pub fn dismiss_diff(&mut self, cx: &mut Context<Self>) {
        self.compare_target = None;
        self.clear_diff();
        cx.notify();
    }

    /// 对比取数失败：挂行内提示，并**把选中位一并放掉**。
    ///
    /// 不放掉会留下「选中亮着、面板却没出来」的死状态——选中的意义就是「面板该在」。
    pub fn set_compare_notice(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.compare_target = None;
        self.clear_diff();
        self.set_history_notice(message, cx);
    }

    /// 对比基准（选中的那一版的 `version_id`；`None` = 没在对比）
    pub fn compare_target(&self) -> Option<&str> {
        self.compare_target.as_deref()
    }

    /// 这一版是不是列表里的「当前」（对比固定为「选中 → 最新」，拿最新当基准无从比）
    fn is_latest_version(&self, version_id: &str) -> bool {
        self.data
            .history
            .as_ref()
            .and_then(|history| history.entries.first())
            .is_some_and(|entry| entry.version_id == version_id)
    }

    /// 放下对比结果（选择位由调用方处理）
    fn clear_diff(&mut self) {
        if let Some(history) = self.data.history.take() {
            self.data.history = Some(history.without_diff());
        }
    }

    /// 历史 Tab 的「清理」：先弹确认框（删快照不可撤销），确认后才发请求。
    ///
    /// 天数取自 `SNAPSHOT_RETENTION_DAYS`（与接缝实际切档同一个常量）：
    /// 对话框里写的数字必须就是真会执行的那个。
    pub fn begin_cleanup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let days = SNAPSHOT_RETENTION_DAYS;
        let current = self
            .data
            .as_history()
            .and_then(|history| history.stats_line())
            .unwrap_or_default();
        let entity = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let mut body = div()
                .v_flex()
                .w_full()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .text_color(theme.colors.foreground)
                        .child(format!("删除 {days} 天前的快照。")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("正文与版本链一并删除，不可撤销。"),
                );
            // 空的时候不摆一行「当前：」——那是没话找话
            if !current.is_empty() {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("当前：{current}")),
                );
            }
            dialog
                .title("清理旧快照")
                .child(body)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("清理")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok({
                    let entity = entity.clone();
                    move |_, _window, app| {
                        entity.update(app, |view, cx| view.request_cleanup(cx));
                        true
                    }
                })
                .on_cancel(|_, _, _| true)
        });
    }

    /// 确认后的发请求（清完的列表与回执由接缝回填）
    pub fn request_cleanup(&mut self, cx: &mut Context<Self>) {
        let Some(InsightTarget::Column { column, .. }) = self.target.clone() else {
            return;
        };
        self.history_cleaning = true;
        self.history_notice = None;
        cx.emit(InsightEvent::SnapshotCleanupRequested { column });
        cx.notify();
    }

    /// 快照是否正在清理（按钮置灰的依据；也供测试断言）
    pub fn history_cleaning(&self) -> bool {
        self.history_cleaning
    }

    /// 历史 Tab 的「保存」：发请求（重取领域画像 + 双写都归接缝）
    pub fn request_snapshot_save(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        // 列目标与源列目标都能存：看到的是同一列的画像，就不该因为入口不同而不能留档
        let column = match &target {
            InsightTarget::Column { column, .. } | InsightTarget::SourceColumn { column, .. } => {
                column.clone()
            }
            _ => return,
        };
        let Some(temp_table) = self.data.sample_table(&target).map(str::to_string) else {
            return;
        };
        let source_label = target.source().map(|source| source.label().to_string());
        tracing::info!(column = %column, temp_table = %temp_table, "【探针】历史 Tab 保存快照请求");
        self.history_saving = true;
        self.history_notice = None;
        cx.emit(InsightEvent::SnapshotSaveRequested {
            temp_table,
            column,
            source_label,
        });
        cx.notify();
    }

    /// 保存 / 读取历史 / 清理 失败：只挂一条行内提示（列表与目标头照旧可见）
    pub fn set_history_notice(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.history_saving = false;
        self.history_cleaning = false;
        self.history_notice = Some(message.into());
        cx.notify();
    }

    /// 快照是否正在保存（按钮置灰的依据；也供测试断言）
    pub fn history_saving(&self) -> bool {
        self.history_saving
    }

    /// 出数（列画像）
    pub fn set_profile(&mut self, profile: ColumnProfileView, cx: &mut Context<Self>) {
        self.data = std::mem::take(&mut self.data).with_column(profile);
        self.state = InsightPanelState::Data;
        cx.notify();
    }

    /// 出数（表探查）
    pub fn set_table_profile(&mut self, profile: TableProfileView, cx: &mut Context<Self>) {
        // Q1：进入 EDA 自动触发评估全表——探查回填（确认窗口生成表的元数据）完成后，
        // 若列级明细尚未计算、不在评估中、且无表级事实摘要（区分「评估已完成」的注入/
        // 复算回填），直接续跑逐列评估。
        let auto_eval = !profile.columns.is_empty()
            && profile.progress.is_none()
            && profile.quality.is_none()
            && profile.columns.iter().all(|c| c.stats_detail.is_none());
        self.data = std::mem::take(&mut self.data).with_table(profile);
        self.state = InsightPanelState::Data;
        if auto_eval {
            self.request_table_evaluation(cx);
            // 取样档迷你图同步自动补算（C7）：默认 scope 是 Sample，列卡折叠态读
            // `mini_stats`——不补算的话首次进入「列分布」一张图都没有（评估产出的
            // `histogram` 要切到全量档才显示）。500 行轻查询，随评估一并发出。
            if self.scope == InsightScope::Sample {
                self.request_sample(cx);
            }
        }
        cx.notify();
    }

    /// 出数（多列表单）：列清单与规则清单换了，选中项要跟着修剪
    ///
    /// 修剪而不是清空：临时表重建后列可能没变，把用户的选择无差别抹掉是坏体验。
    pub fn set_multi_view(&mut self, view: MultiColumnView, cx: &mut Context<Self>) {
        self.multi_selected
            .retain(|name| view.column(name).is_some());
        if let Some(rule) = self.multi_rule.clone()
            && !view.rules.iter().any(|r| r.id == rule)
        {
            self.multi_rule = None;
        }
        self.multi_selected.dedup();
        self.data = std::mem::take(&mut self.data).with_multi(view);
        self.state = InsightPanelState::Data;
        cx.notify();
    }

    /// 多列规则执行结果（保留表单：用户可以换个规则再跑）
    pub fn set_multi_result(
        &mut self,
        result: MultiResultView,
        notes: Vec<QualityNote>,
        cx: &mut Context<Self>,
    ) {
        self.multi_running = false;
        self.multi_notice = None;
        if let Some(view) = self.data.multi.clone() {
            self.data.multi = Some(view.with_result(result, notes));
        }
        cx.notify();
    }

    /// 多列执行失败：只挂一条提示（表单与已有结果照旧可见）
    pub fn set_multi_notice(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.multi_running = false;
        self.multi_notice = Some(message.into());
        cx.notify();
    }

    /// 多列选择：勾上就排在**末尾**（顺序即参数顺序），再点取消
    pub fn toggle_multi_column(&mut self, column: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.multi_selected.iter().position(|name| name == column) {
            self.multi_selected.remove(ix);
        } else {
            self.multi_selected.push(column.to_string());
        }
        self.multi_notice = None;
        cx.notify();
    }

    pub fn set_multi_rule(&mut self, rule_id: impl Into<String>, cx: &mut Context<Self>) {
        self.multi_rule = Some(rule_id.into());
        self.multi_notice = None;
        cx.notify();
    }

    /// 「执行分析」：校验通过才发请求（校验不过按钮本身就是置灰的，这里兵底）
    pub fn run_multi(&mut self, cx: &mut Context<Self>) {
        let Some(rule_id) = self.multi_rule.clone() else {
            return;
        };
        let Some(temp_table) = self
            .target
            .as_ref()
            .and_then(|target| self.data.sample_table(target))
            .map(str::to_string)
        else {
            return;
        };
        if !self.multi_ready() {
            return;
        }
        self.multi_running = true;
        self.multi_notice = None;
        cx.emit(InsightEvent::MultiRunRequested {
            temp_table,
            rule_id,
            columns: self.multi_selected.clone(),
        });
        cx.notify();
    }

    /// 当前选法能不能跑（列数 + 类型族都满足选中的规则）
    pub(crate) fn multi_ready(&self) -> bool {
        let Some(view) = self.data.multi.as_ref() else {
            return false;
        };
        let Some(rule_id) = self.multi_rule.as_deref() else {
            return false;
        };
        view.rules
            .iter()
            .find(|rule| rule.id == rule_id)
            .is_some_and(|rule| rule.accepts(&view.kinds_of(&self.multi_selected)))
    }

    /// 选中列（供宿主与测试读）
    pub fn multi_selection(&self) -> &[String] {
        &self.multi_selected
    }

    pub fn multi_rule_id(&self) -> Option<&str> {
        self.multi_rule.as_deref()
    }

    pub fn multi_running(&self) -> bool {
        self.multi_running
    }

    /// 「评估全表」：发请求并进入评估中（按钮变灰、进度行出现）。
    ///
    /// 整表重算不把已有分数清空——列分数是逐列长出来的，清空会让面板闪一下空白。
    /// 新评估重置取消令牌（C5）：上一轮中断的标记随本次全量重评估自然清除。
    pub fn request_table_evaluation(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        // 源表也能评：用的是取样出来的那张临时表
        if !matches!(
            target,
            InsightTarget::Table { .. } | InsightTarget::SourceTable { .. }
        ) {
            return;
        }
        let Some(temp_table) = self.data.sample_table(&target).map(str::to_string) else {
            return;
        };
        self.eval_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let table_name = target.table_name();
        if let Some(profile) = &self.data.table {
            let total = profile.columns.len();
            let next = profile.evaluating(0, total);
            self.data.table = Some(next);
        }
        cx.emit(InsightEvent::TableEvaluateRequested {
            temp_table,
            table_name,
        });
        cx.notify();
    }

    /// 取消评估全表（C5）：置取消令牌，jobs 循环在下一列派发前停止。
    ///
    /// 已完成列明细保留；未完成列由 jobs 回填中断标记（灰态 + 可补算）。
    /// 取消是协作式（不中断正在跑的那一列查询，等它回来再停）。
    pub fn cancel_table_evaluation(&mut self, cx: &mut Context<Self>) {
        self.eval_cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        tracing::info!("【探针】取消评估全表（已完成列保留，未完成列灰态可补算）");
        cx.notify();
    }

    /// 补算单列明细（C5：中断列灰态卡的「补算」入口）。
    ///
    /// 与评估全表同一条 `get_column_insight_full` 链路，只回填这一列；
    /// 完成后清除该列的中断标记（`with_column_detail`）。
    pub fn request_column_evaluation(
        &mut self,
        temp_table: String,
        column: String,
        cx: &mut Context<Self>,
    ) {
        cx.emit(InsightEvent::ColumnEvaluateRequested { temp_table, column });
        cx.notify();
    }

    /// 出错（`retryable` 决定是否给「重试」入口）
    pub fn set_error(
        &mut self,
        message: impl Into<String>,
        retryable: bool,
        cx: &mut Context<Self>,
    ) {
        self.state = InsightPanelState::Error {
            message: message.into(),
            retryable,
        };
        cx.notify();
    }

    /// ⟳ 与「重试」共用：重新加载**当前 Tab 看的东西**（保持 Tab 与折叠偏好）。无目标时不动。
    ///
    /// 按 Tab 而不是按目标种类发：用户在「表」Tab 上点 ⟳，想重算的是表探查，
    /// 而不是回到列画像。
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.target.is_none() {
            return;
        }
        if self.emit_request_for_tab(self.tab, cx) {
            cx.notify();
        }
    }

    /// 按 Tab 发对应的取数请求（**不管载荷在不在**：要不要发由调用方判）。
    ///
    /// 发了请求就进加载态；**发不出去也要落一个状态**：调用方是为了「点了有反应」
    /// 才先摆骨架的，这里若只是沉默返回，骨架就会一直转下去（最典型的是无项目时
    /// 切到「历史」：那是「没得看」，不是「在加载」）。
    ///
    /// 返回值 = 状态是否变过（调用方据此决定要不要重绘）。
    fn emit_request_for_tab(&mut self, tab: PanelTab, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.target.clone() else {
            return false;
        };
        // 列目标才需要列名：表 / 多列目标没有「哪一列」这回事，取整表；
        // 快照落项目目录，所以「历史」还要项目已打开（无项目看不了，不是错误）
        let request = match tab {
            // 摘要不取数：只用已加载的载荷渲染（缺数据走空态，不重复请求）
            PanelTab::Overview => None,
            PanelTab::Column => match &target {
                InsightTarget::Column { .. } | InsightTarget::SourceColumn { .. } => {
                    Some(InsightEvent::ProfileRequested {
                        target,
                        config: None,
                    })
                }
                _ => None,
            },
            PanelTab::Table => match &target {
                // 已经有源目标 → 原样再发（取样在接缝里做）
                InsightTarget::SourceTable { .. } => Some(InsightEvent::ProfileRequested {
                    target,
                    config: None,
                }),
                // 源列目标看整表：同一个来源，换成表目标（取样口径一致）
                InsightTarget::SourceColumn { source, .. } => {
                    Some(InsightEvent::ProfileRequested {
                        target: InsightTarget::SourceTable {
                            source: source.clone(),
                            table_name: target.table_name(),
                        },
                        config: None,
                    })
                }
                _ => Some(InsightEvent::ProfileRequested {
                    target: InsightTarget::Table {
                        temp_table: target.temp_table().to_string(),
                        table_name: target.table_name(),
                    },
                    config: None,
                }),
            },
            PanelTab::MultiColumn => match self.data.sample_table(&target) {
                Some(temp_table) => Some(InsightEvent::MultiColumnRequested {
                    temp_table: temp_table.to_string(),
                    table_name: target.table_name(),
                }),
                // 源目标还没解析出样本表（还没看过列 / 表）→ 没有可分析的临时表
                None => None,
            },
            PanelTab::Schema => match &target {
                InsightTarget::Schema {
                    conn_id,
                    database,
                    schema,
                } => Some(InsightEvent::SchemaReportRequested {
                    conn_id: conn_id.clone(),
                    database: database.clone(),
                    schema: schema.clone().unwrap_or_default(),
                }),
                _ => None,
            },
            PanelTab::History => match &target {
                // 历史是「某列的历次快照」：只有列目标（含源列）说得清“看谁的历史”
                InsightTarget::Column { column, .. }
                | InsightTarget::SourceColumn { column, .. }
                    if self.project_open =>
                {
                    Some(InsightEvent::HistoryRequested {
                        column: column.clone(),
                    })
                }
                _ => None,
            },
        };
        match request {
            Some(event) => {
                self.state = InsightPanelState::Loading;
                cx.emit(event);
            }
            // 这个 Tab 在当前目标 / 项目状态下取不了数：落空态（引导文案由
            // `empty_hint()` 按 Tab 给），别留着骨架一直转
            None => self.state = InsightPanelState::Empty,
        }
        true
    }

    /// 切 Tab：切过去就补一次取数（事件路径）。
    ///
    /// 「切到某个 Tab」= 「我要看这份数据」：载荷按 Tab 分开存，缺的那一份才请求，
    /// 已有的一份直接渲染（切回来仍看得到上次结果，折叠偏好也保留）。
    ///
    /// 不能放在 render 里做：那会造成「渲染一次发一次请求」。
    pub fn set_tab(&mut self, tab: PanelTab, cx: &mut Context<Self>) {
        tracing::info!(tab = ?tab, old = ?self.tab, "【探针】洞察面板 Tab 切换");
        if self.tab != tab {
            self.tab = tab;
            cx.notify();
        }
        self.ensure_data_for_tab(tab, cx);
    }

    /// 补齐当前 Tab 需要的数据（事件路径调用）。
    ///
    /// 判定只看**该 Tab 的载荷在不在**：不在就取。不拿 `Loading` 当“正在取数”的挡板——
    /// 目标是表时先到的是表载荷，此时切到「多列」拉不到东西（载荷按 Tab 分开存，
    /// 所以“正在加载”并不代表“这个 Tab 正在加载”）。重复点同一 Tab 最多多发一次
    /// 幂等的内省，而 `is_error()` 不抢跑（错误态的入口是「重试」）。
    fn ensure_data_for_tab(&mut self, tab: PanelTab, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        if self.state.is_error() {
            return;
        }
        let missing = match tab {
            PanelTab::Overview => false,
            PanelTab::Column => self.data.column.is_none(),
            PanelTab::Table => self.data.table.is_none(),
            PanelTab::MultiColumn => !self
                .data
                .multi
                .as_ref()
                .is_some_and(|view| view.is_for(target.temp_table())),
            PanelTab::Schema => self.data.schema.is_none(),
            PanelTab::History => self.data.history.is_none(),
        };
        if !missing {
            return;
        }
        if self.emit_request_for_tab(tab, cx) {
            cx.notify();
        }
    }

    /// 按 Tab 下标切换（旧 TabBar 的 on_click 入口；UI 已移除 Tab 条，
    /// 方法保留供 `tests.rs` 大量面板状态断言使用）
    #[allow(dead_code)]
    pub(crate) fn set_tab_from_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(tab) = PanelTab::from_index(ix) {
            self.set_tab(tab, cx);
        }
    }

    /// 同步四区展开态（`Accordion` 回调只给「当前展开的下标集合」）
    pub(crate) fn set_open_sections(&mut self, open: &[usize], cx: &mut Context<Self>) {
        for (i, slot) in self.open_sections.iter_mut().enumerate() {
            *slot = open.contains(&i);
        }
        cx.notify();
    }

    /// ⟳ 的 Action 入口（与面板头按钮同一条路径）
    pub(crate) fn on_refresh(&mut self, _: &InsightRefresh, _window: &mut Window, cx: &mut Context<Self>) {
        self.reload(cx);
    }

    /// 同步结构四区的展开态（与列画像同口径：Accordion 只给「当前展开的下标集合」）
    pub(crate) fn set_open_schema_sections(&mut self, open: &[usize], cx: &mut Context<Self>) {
        for (i, slot) in self.open_schema_sections.iter_mut().enumerate() {
            *slot = open.contains(&i);
        }
        cx.notify();
    }
}

