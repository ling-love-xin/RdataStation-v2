//! 编辑器共享状态 —— Notebook 端口（notebook_*/kernel_*/submit_cell/apply_cell_note/cancel_for）
//!
//! 自 shared.rs 拆分（2026-10-29）：按能力域切分 `impl EditorShared` 方法块。
//! 端口方法全 pub（跨域互调）；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为。

use super::*;

impl EditorShared {

    /// 某文档的笔记状态（没有 = 默认会话 + 空单元）
    pub fn notebook_state(&self, document: &crate::model::DocumentId) -> crate::cell::NotebookState {
        self.notebooks
            .borrow()
            .get(document)
            .cloned()
            .unwrap_or_default()
    }

    /// 某文档的单元序列（快照；视图渲染读它）
    pub fn notebook_cells(&self, document: &crate::model::DocumentId) -> Vec<crate::cell::Cell> {
        self.notebook_state(document).cells
    }

    /// 某文档的当前会话
    pub fn notebook_session(&self, document: &crate::model::DocumentId) -> crate::session::Session {
        self.notebook_state(document).session
    }

    /// 替换某文档的整份笔记状态（打开 `.rdsnote` / 落盘前取快照用）
    pub fn set_notebook_state(
        &self,
        document: &crate::model::DocumentId,
        state: crate::cell::NotebookState,
    ) {
        self.notebooks.borrow_mut().insert(document.clone(), state);
    }

    /// 替换某文档的单元序列（新增 / 删除 / 重排后回写）
    pub fn set_notebook_cells(
        &self,
        document: &crate::model::DocumentId,
        cells: Vec<crate::cell::Cell>,
    ) {
        let mut notebooks = self.notebooks.borrow_mut();
        let state = notebooks.entry(document.clone()).or_default();
        state.cells = cells;
    }

    /// 【#20】回写会话默认内核（“上次用过的内核”；执行提交成功后调）
    pub fn set_session_default_kernel(
        &self,
        document: &crate::model::DocumentId,
        kernel_id: Option<String>,
    ) {
        let mut notebooks = self.notebooks.borrow_mut();
        if let Some(state) = notebooks.get_mut(document) {
            state.session.default_kernel = kernel_id;
        }
    }

    /// 【#16】重设某个单元的执行源（连接 / 内核 id；`None` = 回会话默认）；
    /// 返回是否真变（调用方按需 notify）
    pub fn set_cell_execution_source(
        &self,
        document: &crate::model::DocumentId,
        cell_id: &str,
        source: Option<crate::cell::ExecutionSource>,
    ) -> bool {
        let mut notebooks = self.notebooks.borrow_mut();
        let Some(state) = notebooks.get_mut(document) else {
            return false;
        };
        let Some(cell) = state.cells.iter_mut().find(|cell| cell.id == cell_id) else {
            return false;
        };
        if cell.execution_source == source {
            return false;
        }
        cell.execution_source = source;
        true
    }

    /// 【#17 → #20】确保某个内核已启动（无则 `spawn` + 取事件流存好）；
    /// `command` = 注册表解析出的解释器可执行路径（见 [`crate::kernel::resolve_kernel`]）。
    /// 失败给可读原因（解释器没装 ipykernel / 握手超时等）
    pub fn ensure_kernel(&self, kernel_id: &str, command: &str) -> Result<(), String> {
        let mut kernels = self.kernels.borrow_mut();
        if kernels.contains_key(kernel_id) {
            return Ok(());
        }
        let mut session = rds_kernel::KernelSession::spawn(std::path::Path::new(command))
            .map_err(|e| e.to_string())?;
        let events = session.take_events();
        kernels.insert(
            kernel_id.to_string(),
            KernelEntry {
                session,
                events,
                python: std::path::PathBuf::from(command),
            },
        );
        Ok(())
    }

    /// 【#17】向某内核提交一段代码（排到单命令队列；结果经事件流回传）
    pub fn kernel_execute(&self, kernel_id: &str, code: &str) -> Result<(), String> {
        let kernels = self.kernels.borrow();
        let Some(entry) = kernels.get(kernel_id) else {
            return Err(format!("内核 {kernel_id} 未启动"));
        };
        entry.session.execute(code.to_string());
        Ok(())
    }

    /// 【#17】非阻塞取走某内核当前积压的事件（UI 线程轮询；`try_recv` 不等待）
    pub fn kernel_drain(&self, kernel_id: &str) -> Vec<rds_kernel::KernelEvent> {
        let mut kernels = self.kernels.borrow_mut();
        let Some(entry) = kernels.get_mut(kernel_id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        while let Ok(event) = entry.events.try_recv() {
            out.push(event);
        }
        out
    }

    /// 【#17】关闭并摘除某内核（笔记本关闭时逐个调；不 panic 于未启动）
    pub fn kernel_shutdown(&self, kernel_id: &str) {
        let mut kernels = self.kernels.borrow_mut();
        if let Some(entry) = kernels.remove(kernel_id) {
            entry.session.shutdown();
        }
    }

    /// 【2d-笔记级】当前**活跃**（已启动）的内核 id 列表（「重启」用它逐个关闭；
    /// 空 = 一个都没起过）
    pub fn active_kernel_ids(&self) -> Vec<String> {
        self.kernels.borrow().keys().cloned().collect()
    }

    /// 会话切换：绑定换掉 + 全部输出 stale（T2d-2）
    pub fn switch_notebook_session(
        &self,
        document: &crate::model::DocumentId,
        session: crate::session::Session,
    ) {
        let mut notebooks = self.notebooks.borrow_mut();
        let state = notebooks.entry(document.clone()).or_default();
        state.switch_session(session);
    }

    /// 提交一次**单元执行**（分析模式；走 `run_cell`，连接 / schema / 通道从服务层取）
    ///
    /// 与 [`Self::submit`] 同一口径：没有执行器 / 有执行在跑 → 如实拒绝，不静默。
    /// 【#17】单元执行的目标连接：**执行源指定优先**，否则文档 / 会话默认
    pub fn cell_connection_for(
        &self,
        document: &crate::model::DocumentId,
        request: &crate::cell::CellRequest,
    ) -> Option<String> {
        request
            .conn_id
            .clone()
            .or_else(|| self.service().connection_for(document))
    }

    pub fn submit_cell(
        &self,
        document: crate::model::DocumentId,
        request: &crate::cell::CellRequest,
    ) -> Result<(), String> {
        let (connection, schema, channel) = {
            let service = self.service.borrow();
            (
                // 执行源指定连接优先（#17），否则文档 / 会话默认
                self.cell_connection_for(&document, request),
                service.schema_for(&document),
                service.channel_for(&document),
            )
        };
        let guard = self.exec.borrow();
        let Some(queue) = guard.as_ref() else {
            return Err("当前未接入执行".to_string());
        };
        queue.submit(
            document,
            &crate::execution::ExecTarget::Cell(request.clone()),
            connection,
            schema,
            crate::store::ResultPlacement::Replace,
            crate::execution::RunOptions {
                use_transaction: false,
            },
            channel,
        )
        .map_err(|e| e.message().to_string())
    }

    /// 【2d】单元执行的结论（宿主 / 视图轮询取走）
    pub fn drain_cell_notes(&self) -> Vec<crate::cell::CellNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|channel| channel.drain_cell_notes())
            .unwrap_or_default()
    }

    /// 把一条单元回执**落到某文档的笔记状态**上（Running → Done / Error）
    ///
    /// 回执带会话 id：切会话后到期的回执不落（会话已经换了，别把旧结果贴进新会话）。
    pub fn apply_cell_note(
        &self,
        document: &crate::model::DocumentId,
        note: &crate::cell::CellNote,
    ) {
        let mut notebooks = self.notebooks.borrow_mut();
        let Some(state) = notebooks.get_mut(document) else {
            return;
        };
        if state.session.id != note.session_id {
            return; // 会话已切换：回执作废
        }
        let Some(cell) = state.cell_mut(&note.cell_id) else {
            return;
        };
        if cell.state != crate::cell::CellState::Running {
            return; // 没在跑的回执不落（防止迟到的旧回执覆盖新结果）
        }
        match &note.result {
            Ok(outcome) => {
                cell.mark_done(crate::cell::Output::Table(build_inline_entry(document, outcome)))
            }
            Err(reason) => cell.mark_error(reason.clone()),
        }
    }

    /// 中断**当前文档绑定的连接**的执行（B3；阶段②：取消按连接，只命中该连接的作业）；
    /// 没在跑就回绝（理由可读，不是静默）
    pub fn cancel_for(&self, document: &crate::model::DocumentId) -> Result<(), String> {
        let connection = {
            let service = self.service.borrow();
            service.connection_for(document)
        };
        let guard = self.exec.borrow();
        match guard.as_ref() {
            Some(channel) => channel.cancel(connection),
            None => Err("当前未接入执行".to_string()),
        }
    }

    /// 中断尝试的结果（主线程轮询：中断失败 / 没在跑 都要留痕）
    pub fn drain_cancel_notes(&self) -> Vec<String> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|channel| channel.drain_cancel_notes())
            .unwrap_or_default()
    }
}
