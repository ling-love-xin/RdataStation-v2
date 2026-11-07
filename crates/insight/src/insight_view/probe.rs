use super::*;

impl InsightView {
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
}
