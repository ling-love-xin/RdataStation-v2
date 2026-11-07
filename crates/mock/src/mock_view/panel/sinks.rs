// ==================== 出口/辅助方法 ====================

use super::*;

impl MockPanel {
    /// 出口：持久化为**项目**分析库新表（后台任务：大行数落库同样会阻塞界面）。
    pub fn persist_table(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.current_info().cloned() else {
            self.fail("请先生成（预览确认后再落库）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写入项目分析库", cx);
            return;
        }
        // 目标表名在 `start_job` 的 `sync_inputs` 里从输入框取（任务内部据此命名新表）
        self.start_job(MockJobKind::Persist(info), cx);
    }

    /// 出口：导出文件（调用方已选好路径；后台任务）。
    pub fn export_file(&mut self, format: &MockExportFormat, path: String, cx: &mut Context<Self>) {
        let Some(info) = self.current_info().cloned() else {
            self.fail("请先生成（预览确认后再导出）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写出文件", cx);
            return;
        }
        self.start_job(
            MockJobKind::Export {
                info,
                format: format.clone(),
                path,
            },
            cx,
        );
    }

    /// 出口：保存到草稿箱（`{项目}/mock/`；后台任务）。
    pub fn save_scratchpad(&mut self, format: &MockExportFormat, cx: &mut Context<Self>) {
        let Some(info) = self.current_info().cloned() else {
            self.fail("请先生成（预览确认后再保存）", cx);
            return;
        };
        if self.host.read_only() {
            self.fail("只读模式：不允许写出文件", cx);
            return;
        }
        self.start_job(
            MockJobKind::Scratchpad {
                info,
                format: format.clone(),
            },
            cx,
        );
    }


    /// 从输入框同步目标表名 / 行数 / 种子（生成与出口前调用）。
    pub(crate) fn sync_inputs(&mut self, cx: &App) -> Result<(), String> {
        let id = self.config_id;
        let inputs = self.config_inputs.get(&id);
        let name = inputs.map(|inputs| inputs.table.read(cx).value().to_string());
        let rows = inputs.map(|inputs| inputs.rows.read(cx).value().to_string());
        let seed = inputs.map(|inputs| inputs.seed.read(cx).value().to_string());
        match name {
            Some(value) => self.draft.table_name = validate_table_name(&value)?,
            None => self.draft.table_name = validate_table_name(&self.draft.table_name)?,
        }
        if let Some(value) = rows {
            self.draft.options.rows = parse_rows(&value)?;
        }
        if let Some(value) = seed {
            self.draft.options.seed = parse_seed(&value)?;
        }
        Ok(())
    }

    pub(crate) fn succeed(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = None;
        self.outcome = Some(message);
        self.host.notify(cx);
        cx.notify();
    }

    /// 项目级出口（落库 / 追加 / 草稿箱）能不能用：**有结果 + 非只读 + 已打开项目**。
    ///
    /// 三个条件就是三个出口的可行性判据，写在一处：视图据此禁用按钮（渲染期只读它），
    /// 禁用理由也在同一处（`render_actions` 的两行状态提示）。
    /// 为何不靠「点了再报错」：三个出口都是项目级的，未打开项目时按钮永远点得动、
    /// 每次都弹同一句拒绝，不如直接不可点（与「生成前禁用」同一口径）。
    pub fn sinks_ready(&self) -> bool {
        self.has_result() && !self.host.read_only() && self.host.project_root().is_some()
    }

    pub(crate) fn fail(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.outcome = None;
        self.error = Some(message.into());
        cx.notify();
    }

    /// 生成结果是否就绪（出口按钮是否可用）
    pub(crate) fn has_result(&self) -> bool {
        !self.results.is_empty()
    }

    /// 「另存为」的默认文件名：取**当前结果表**的名字（场景模板下与表配置无关）。
    ///
    /// 没结果时回退到表配置的目标表名：按钮在无结果时本来就点不开（`has_result` 拦着），
    /// 这里只保证函数本身不会给出空名字。
    pub(crate) fn export_file_name(&self, format: &MockExportFormat) -> String {
        let table = self
            .current_info()
            .map(|info| info.table_name.clone())
            .unwrap_or_else(|| self.draft.table_name.clone());
        mock_file_name(&table, format)
    }

    /// 测试用：直接写行数输入框（校验失败路径：非法行数止于视图，不触宿主）。
    #[cfg(test)]
    pub(crate) fn set_rows_input(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.rows_input().cloned() {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }

    // ==================== 渲染：配置面板 ====================

    /// 目标表名 + 行数 / 种子 / 语言。
    /// 进行中任务的进度归属（`None` = 空闲）。
    pub fn job_row_scope(&self) -> Option<JobRowScope> {
        let job = self.job.as_ref()?;
        Some(if job.kind.by_table() {
            JobRowScope::Collection
        } else {
            JobRowScope::Single
        })
    }

    /// 这张表引用的**出边关系**（清单里挂在它那一行下面的 `↳` 子行）。
    ///
    /// 真身在列上的 `dependency`；这里用生成时的关系快照（`last_relations`）——
    /// 用户可能已经退出场景态，也可能改过工作副本。
    pub fn outgoing_relations(&self, table: &str) -> Vec<ScenarioRelation> {
        self.last_relations
            .iter()
            .filter(|relation| relation.child_table == table)
            .cloned()
            .collect()
    }

    /// 清单行的状态点（生成状态优先，其次是落库状态）：
    /// 生成的是临时的，落库的才是项目库里的——两者不是一回事。
    pub fn table_status(&self, table: &str) -> TableStatus {
        if let Some(job) = self.job.as_ref() {
            // 只有生成类才叫「生成中」：出口类（写入 / 导出）只动已有结果，不改生成状态
            if !job.kind.by_table() && job.progress.phase.is_quantified() {
                let target = self
                    .results
                    .first()
                    .map(|info| info.table_name.as_str())
                    .unwrap_or(self.draft.table_name.as_str());
                if target == table {
                    return TableStatus::Generating;
                }
            }
        }
        if self.results.iter().any(|info| info.table_name == table) {
            // 悬空引用优先于「落了没」：它指的是可行动的事实（父表还没落，值会悬空），
            // 而「已落库」只说明本表自己落了——落没落都不影响那句⚠还成不成立
            if self.table_references_unlanded(table) {
                return TableStatus::DanglingParent;
            }
            if self.landed_tables.iter().any(|landed| landed == table) {
                return TableStatus::Persisted;
            }
            return TableStatus::NotPersisted;
        }
        if self.error.is_some() && self.draft.table_name == table {
            return TableStatus::Failed;
        }
        TableStatus::Idle
    }

    /// 这张表引用的父表里，还有没落库的吗（`⚠ 引用的表未落库` 的数据来源）。
    ///
    /// 只看**本会话的**落库记录（`landed_tables`）：项目库里本来就有同名表不算它落过。
    pub fn table_references_unlanded(&self, table: &str) -> bool {
        self.outgoing_relations(table).iter().any(|relation| {
            !self
                .landed_tables
                .iter()
                .any(|landed| landed == &relation.parent_table)
        })
    }
}
