//! 编辑器宿主面板 —— Notebook 单元格与内核（sync_cell_editors/notebook_*/run_kernel_cell/advance_notebook_queue/drain_cell_notes）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。


use gpui_kit::component::input::{
    EditorState, InputEvent,
};
// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法


use crate::model::{DocumentId, EditorMode};
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）


use super::*;
impl EditorHostPanel {

    /// 【遗留 #10】把笔记本现有单元的编辑器实例补齐（面板进 Dock / 测试入口调用；
    /// 渲染期只读不建实体，创建一律走事件路径——构造期建会重入 panic）
    pub(crate) fn sync_cell_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.document_mode() != Some(EditorMode::Analysis) {
            return;
        }
        let cells = self.notebook_state().cells;
        for cell in &cells {
            self.ensure_cell_editor(cell, window, cx);
        }
    }

    /// 【遗留 #10】为一个单元确保内核编辑器实例存在（无则创建 + 订阅 Change → 回写正文）
    ///
    /// 订阅挂在单元编辑器实体上：Change 事件从它发出，面板此刻不在更新中，
    /// 回写 shared 不会重入（gpui-kit-dev「不要在回调里重入 update 当前实体」）。
    fn ensure_cell_editor(&mut self, cell: &crate::cell::Cell, window: &mut Window, cx: &mut Context<Self>) {
        if self.cell_editors.contains_key(&cell.id) {
            return;
        }
        let body = cell.body.clone();
        // 内核编辑器语言 = 单元语言的持久化键（EditorState::language 吃 "sql"/"markdown" 等）
        let language = cell.language.as_key();
        let editor = cx.new(|cx| {
            // `language` 是消费式 builder：链在 `new` 后面
            let mut state = EditorState::new(window, cx).language(language);
            state.set_value(body, window, cx);
            state
        });
        let document = self.document.clone();
        let shared = self.shared.clone();
        let cell_id = cell.id.clone();
        // 订阅挂面板上下文会 read 面板（subscribe_in 的 ensure_window），面板正被
        // update 时直接 panic；改用无窗口版 `subscribe`（回调里不需要 window，
        // 只回写 shared）——派发时面板已不在更新中
        let sub = cx.subscribe(
            &editor,
            move |_this, state, event: &InputEvent, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                // 正文从单元编辑器实体读（事件路径，可直接 read）
                let text = state.read(cx).text().to_string();
                let mut cells = shared.notebook_cells(&document);
                if let Some(target) = cells.iter_mut().find(|c| c.id == cell_id) {
                    target.body = text;
                }
                shared.set_notebook_cells(&document, cells);
                cx.notify();
            },
        );
        self.cell_editors.insert(cell.id.clone(), editor);
        self._cell_editor_subs.insert(cell.id.clone(), sub);
    }

    /// 取本文档的笔记状态（渲染用快照）
    pub(crate) fn notebook_state(&self) -> crate::cell::NotebookState {
        self.shared.notebook_state(&self.document)
    }

    /// 新增一个单元（追加到序列尾部；新建即 Idle，执行源默认 = 会话）
    pub(crate) fn notebook_add_cell(
        &mut self,
        _document: &DocumentId,
        language: crate::cell::CellLanguage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let id = format!(
            "cell-{}-{}",
            state.cells.len() + 1,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        );
        let body = match language {
            crate::cell::CellLanguage::Sql => "select 1;".to_string(),
            crate::cell::CellLanguage::Markdown => String::new(),
            // Python / R / Julia：给一段最小可执行样板，用户一进来就知道能跑
            crate::cell::CellLanguage::Python => "print('hello')".to_string(),
            crate::cell::CellLanguage::R => "print(1 + 1)".to_string(),
            crate::cell::CellLanguage::Julia => "println(1 + 1)".to_string(),
        };
        state.cells.push(crate::cell::Cell::new(id, language, body));
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【遗留 #10】新单元同步建内核编辑器（编辑能力随单元出现就位）
        if let Some(last) = self.notebook_state().cells.last() {
            let cell = last.clone();
            self.ensure_cell_editor(&cell, window, cx);
        }
        cx.notify();
    }

    /// 【#16】重设单元执行源（连接 / 内核；`None` = 会话默认）；同值 = 无操作
    pub(crate) fn notebook_set_execution_source(
        &mut self,
        _document: &DocumentId,
        id: &str,
        source: Option<crate::cell::ExecutionSource>,
        cx: &mut Context<Self>,
    ) {
        if self
            .shared
            .set_cell_execution_source(&self.document, id, source)
        {
            cx.notify();
        }
    }

    /// 删除一个单元（按 id；不存在 = 忽略）
    pub(crate) fn notebook_delete_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        state.cells.retain(|cell| cell.id != id);
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【遗留 #10】编辑器实例与订阅一并丢弃（句柄即生命周期）
        self.cell_editors.remove(id);
        self._cell_editor_subs.remove(id);
        cx.notify();
    }

    /// 上移 / 下移一个单元（`delta` = ±1；到边界不动）
    pub(crate) fn notebook_move_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        delta: isize,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let Some(pos) = state.cells.iter().position(|cell| cell.id == id) else {
            return;
        };
        let target = pos as isize + delta;
        if target < 0 || target >= state.cells.len() as isize {
            return; // 到边界不动（如实）
        }
        state.cells.swap(pos, target as usize);
        self.shared.set_notebook_cells(&self.document, state.cells);
        cx.notify();
    }

    /// 复制一个单元（副本插在原单元后面；新 id，正文 / 类型原样，输出清空）
    pub(crate) fn notebook_duplicate_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let Some(pos) = state.cells.iter().position(|cell| cell.id == id) else {
            return;
        };
        let original = state.cells[pos].clone();
        let mut copy = crate::cell::Cell::new(
            format!("{}-copy", original.id),
            original.language,
            original.body,
        );
        // 副本继承执行源（复制的就是同一语言的同一执行目标）
        copy.execution_source = original.execution_source.clone();
        state.cells.insert(pos + 1, copy);
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【遗留 #10】副本同步建编辑器实例（新 id）
        let copy = self.notebook_state().cells[pos + 1].clone();
        self.ensure_cell_editor(&copy, window, cx);
        cx.notify();
    }

    /// SQL ↔ 文本互转（正文原样，类型翻转；有输出则清空并标 Idle）
    pub(crate) fn notebook_convert_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let Some(cell) = state.cell_mut(id) else {
            return;
        };
        // SQL ↔ Markdown 互转（其余语言本版本不做 convert：python 单元要换执行目标，
        // 属于 #17 的职责）
        cell.language = if cell.language == crate::cell::CellLanguage::Sql {
            crate::cell::CellLanguage::Markdown
        } else {
            crate::cell::CellLanguage::Sql
        };
        cell.execution_source = None;
        cell.state = crate::cell::CellState::Idle;
        cell.outputs.clear();
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【遗留 #10】类型翻转 → 编辑器语言跟着变：丢弃旧实例重建（正文原样由
        // `ensure_cell_editor` 从 shared 读，不需要手动搬运）
        self.cell_editors.remove(id);
        self._cell_editor_subs.remove(id);
        if let Some(cell) = self.notebook_state().cells.iter().find(|c| c.id == id) {
            let cell = cell.clone();
            self.ensure_cell_editor(&cell, window, cx);
        }
        cx.notify();
    }

    /// 运行一个单元（仅 SQL 单元；Text / 空正文 / 有执行在跑都如实拒绝）
    pub(crate) fn notebook_run_cell(
        &mut self,
        _document: &DocumentId,
        id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        if state.has_running() {
            self.set_message(Some("已有单元在执行中".to_string()), cx);
            return;
        }
        // 会话 id 先克隆出来（`cell_mut` 会可变借用 state，不能同时借 `state.session`）
        let session_id = state.session.id.clone();
        // 判据先读出来、放开借用（内核分支要在 shared 上另起读，不能挂在同一个借用上）
        let (is_executable, is_sql, is_empty) = {
            let Some(cell) = state.cell_mut(id) else {
                return;
            };
            (
                cell.language.is_executable(),
                cell.language == crate::cell::CellLanguage::Sql,
                cell.body.trim().is_empty(),
            )
        };
        if !is_executable {
            self.set_message(Some("Markdown 单元不可执行（只能做备注）".to_string()), cx);
            return;
        }
        // SQL 走执行链（跑在连接上：单元指定连接优先，否则会话默认）；
        // Python / R / Julia 走内核执行（KernelSession，见 `run_kernel_cell`）
        if !is_sql {
            self.run_kernel_cell(&session_id, id, cx);
            return;
        }
        if is_empty {
            self.set_message(Some("这个 SQL 单元是空的，没有可执行的语句".to_string()), cx);
            return;
        }
        let Some(cell) = state.cell_mut(id) else {
            return;
        };
        // 执行源指定的连接（None = 文档 / 会话默认）
        let conn_id = match &cell.execution_source {
            Some(crate::cell::ExecutionSource::Connection(id)) => Some(id.clone()),
            _ => None,
        };
        let sql = cell.body.clone();
        cell.mark_running();
        let request = crate::cell::CellRequest {
            session_id,
            cell_id: id.to_string(),
            sql,
            conn_id,
        };
        self.shared.set_notebook_cells(&self.document, state.cells);
        match self.shared.submit_cell(self.document.clone(), &request) {
            Ok(()) => {
                if self.pending == 0 {
                    self.running_since = Some(std::time::Instant::now());
                }
                self.pending += 1;
                self.set_message(None, cx);
                self.ensure_exec_pump(cx);
            }
            Err(reason) => {
                // 提交被拒：把 Running 标回去（不假装在跑）
                let mut state = self.notebook_state();
                if let Some(cell) = state.cell_mut(id) {
                    cell.state = crate::cell::CellState::Idle;
                    cell.outputs.clear();
                }
                self.shared.set_notebook_cells(&self.document, state.cells);
                self.set_message(Some(reason), cx);
            }
        }
        cx.notify();
    }

    /// 【2d-笔记级】切换分析会话（绑定连接 / 环境）：换绑定 + 全部输出标 stale。
    ///
    /// `conn_id` = 新的默认连接（`None` = 跟随当前活动连接，与 SQL 模式 B1 同口径）；
    /// 环境维度暂不在此菜单改（分析环境跟随通用模式解释器，见 `env` 端口）。
    pub(crate) fn notebook_switch_connection(
        &mut self,
        conn_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        // 防并发：有单元在跑时切换会话，回执会因会话不匹配而作废（输出会被清），
        // 不如提前拒绝、让用户先中断。
        if self.notebook_state().has_running() {
            self.set_message(Some("有单元在执行中，先中断再切换会话".to_string()), cx);
            return;
        }
        let mut session = self.notebook_state().session;
        session.default_connection = conn_id;
        self.shared
            .switch_notebook_session(&self.document, session);
        self.set_message(None, cx);
        cx.notify();
    }

    /// 【2d-笔记级】全部运行：按顺序跑所有**可执行**单元（SQL / 内核）。
    ///
    /// 执行队列一次只跑一个（`ExecQueue` busy 检查），所以这里**只入队 + 提交队首**，
    /// 跑完一个（SQL 回执 / 内核收口）由 [`Self::advance_notebook_queue`] 自动提交下一个。
    /// 有单元在跑 → 拒绝；Markdown 备注 / 空单元不入队。
    pub(crate) fn notebook_run_all(&mut self, cx: &mut Context<Self>) {
        if self.notebook_state().has_running() || !self.notebook_pending.is_empty() {
            self.set_message(Some("已有单元在执行中".to_string()), cx);
            return;
        }
        let cells = self.notebook_state().cells;
        self.notebook_pending = cells
            .iter()
            .filter(|cell| cell.language.is_executable() && !cell.body.trim().is_empty())
            .map(|cell| cell.id.clone())
            .collect();
        if self.notebook_pending.is_empty() {
            self.set_message(Some("没有可执行的单元（可执行单元要有代码）".to_string()), cx);
            return;
        }
        self.advance_notebook_queue(cx);
    }

    /// 【2d-笔记级】推进「全部运行」队列：队首入执行，跑完一个自动接下一个。
    ///
    /// 守卫（防两个收口点重复推进）：队列空 / 还有单元在跑 → 不动。
    /// 队列走完 = 全部运行完成（消息位清掉，不打扰）。
    fn advance_notebook_queue(&mut self, cx: &mut Context<Self>) {
        if self.notebook_pending.is_empty() || self.notebook_state().has_running() {
            return;
        }
        let next = self.notebook_pending.remove(0);
        // 复用的是单发入口（notebook_run_cell）：它自己会 mark_running + 提交；
        // 此时无 running（上面守卫），不会自锁
        let document = self.document.clone();
        self.notebook_run_cell(&document, &next, cx);
    }

    /// 【2d-笔记级】重启：关闭当前**活跃**内核（下次执行自动重拉）。
    ///
    /// 空 = 没起过内核 → 提示无需重启；正在跑的内核单元也会被一并停掉
    /// （`kernel_running` 作废，回执不再回填）。
    pub(crate) fn notebook_restart_kernel(&mut self, cx: &mut Context<Self>) {
        let active = self.shared.active_kernel_ids();
        if active.is_empty() {
            self.set_message(Some("没有活跃内核，无需重启".to_string()), cx);
            return;
        }
        for id in &active {
            self.shared.kernel_shutdown(id);
        }
        // 正在跑的内核单元：作废运行标记（内核已关，回执不会再回来）
        self.kernel_running = None;
        // 「全部运行」队列一并作废：当前单元被中断，后续单元不该继续自动跑
        self.notebook_pending.clear();
        self.set_message(Some(format!("已重启 {} 个内核", active.len())), cx);
        cx.notify();
    }

    /// 【#17 → #20】跑一个内核单元（Python / R / Julia）：
    /// 解析实际内核（显式指定 > 会话默认 > 注册表第一个可用）→ 启动（复用）内核 →
    /// 标记 Running → 提交代码 → 内核泵轮询事件回填输出；成功后把内核回写为
    /// 会话默认（“上次用过的内核”）。
    ///
    /// 与 SQL 分支分开读 shared（`notebook_run_cell` 里不保留对 state 的借用再调这里）
    fn run_kernel_cell(
        &mut self,
        session_id: &str,
        cell_id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut state = self.notebook_state();
        let Some(cell) = state.cell_mut(cell_id) else {
            return;
        };
        if cell.body.trim().is_empty() {
            self.set_message(
                Some(format!("这个 {} 单元是空的，没有可执行的代码", cell.language.label())),
                cx,
            );
            return;
        }
        // 实际内核：执行源指定优先，否则会话默认 / 注册表第一个可用（#20 注册表路由）
        let source = cell.execution_source.clone();
        let language = cell.language;
        let kernel_id = match self.shared.resolve_kernel_for(&self.document, source.as_ref(), language) {
            Ok(spec) => spec.id.clone(),
            Err(reason) => {
                self.set_message(Some(reason), cx);
                return;
            }
        };
        let Some(command) = self
            .shared
            .kernel_specs()
            .into_iter()
            .find(|spec| spec.id == kernel_id)
            .map(|spec| spec.command)
        else {
            self.set_message(Some(format!("内核「{kernel_id}」已从注册表消失，请重启面板")), cx);
            return;
        };
        if let Err(reason) = self.shared.ensure_kernel(&kernel_id, &command) {
            self.set_message(Some(format!("内核 {kernel_id} 启动失败：{reason}")), cx);
            return;
        }
        let code = cell.body.clone();
        cell.mark_running();
        self.kernel_running = Some(KernelRun {
            kernel_id: kernel_id.clone(),
            cell_id: cell_id.to_string(),
            session_id: session_id.to_string(),
        });
        self.shared.set_notebook_cells(&self.document, state.cells);
        if let Err(reason) = self.shared.kernel_execute(&kernel_id, &code) {
            self.kernel_running = None;
            let mut state = self.notebook_state();
            if let Some(cell) = state.cell_mut(cell_id) {
                cell.state = crate::cell::CellState::Idle;
                cell.outputs.clear();
            }
            self.shared.set_notebook_cells(&self.document, state.cells);
            self.set_message(Some(reason), cx);
            return;
        }
        // 执行提交成功 → 回写会话默认（“上次用过的内核”）
        self.shared.set_session_default_kernel(&self.document, Some(kernel_id));
        self.set_message(None, cx);
        self.ensure_kernel_pump(cx);
    }

    /// 【#17】内核事件泵：60ms 一拍轮询当前内核单元的事件，回填输出直到 ExecuteReply /
    /// Failed（内核空闲即退出循环，下次执行重新起）
    fn ensure_kernel_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.kernel_pump.borrow().as_ref()
            && !task.is_ready()
        {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                let keep_going = weak
                    .update(cx, |this, cx| {
                        let before = this.kernel_running.is_some();
                        this.drain_kernel_events(cx);
                        if before {
                            cx.notify();
                        }
                        this.kernel_running.is_some()
                    })
                    .unwrap_or(false);
                if !keep_going {
                    return;
                }
            }
        });
        *self.kernel_pump.borrow_mut() = Some(task);
    }

    /// 【#17】取走内核事件并落到当前运行的单元：
    /// 流式输出实时上屏（Display / Stream 追加），ExecuteReply 收口（成功置 Done /
    /// 失败置 Error），Failed 给原因
    fn drain_kernel_events(&mut self, _cx: &mut Context<Self>) {
        let Some(run) = self.kernel_running.clone() else {
            return;
        };
        let events = self.shared.kernel_drain(&run.kernel_id);
        if events.is_empty() {
            return;
        }
        let mut state = self.notebook_state();
        // 会话 id 先克隆（`cell_mut` 之后不能同时借 `state.session`）
        let session_id = state.session.id.clone();
        let Some(cell) = state.cell_mut(&run.cell_id) else {
            // 单元没了：执行作废（不再等回执）
            self.kernel_running = None;
            return;
        };
        // 会话已切换：回执不落，执行作废
        if session_id != run.session_id {
            self.kernel_running = None;
            cell.state = crate::cell::CellState::Idle;
            cell.outputs.clear();
            self.shared.set_notebook_cells(&self.document, state.cells);
            return;
        }
        let mut finished = false;
        let mut failed_reason: Option<String> = None;
        for event in events {
            match event {
                rds_kernel::KernelEvent::Display(media) => {
                    // mime bundle → 原生输出（图片 / 表格 / markdown / 文本）
                    let outputs = crate::media::media_to_outputs(&media);
                    cell.outputs.extend(outputs);
                }
                rds_kernel::KernelEvent::Stream { name, text } => {
                    // stdout 无前缀；stderr 带标记（如实反映来源）
                    if name == "stderr" {
                        cell.outputs
                            .push(crate::cell::Output::Text(format!("stderr: {text}")));
                    } else {
                        cell.outputs.push(crate::cell::Output::Text(text));
                    }
                }
                rds_kernel::KernelEvent::ExecuteReply { status, .. } => {
                    finished = true;
                    if status != "ok" {
                        failed_reason = Some("内核返回执行失败".to_string());
                    }
                }
                rds_kernel::KernelEvent::Error { ename, evalue, .. } => {
                    cell.outputs.push(crate::cell::Output::Error(format!(
                        "{ename}: {evalue}"
                    )));
                }
                rds_kernel::KernelEvent::Status(_) => {}
                rds_kernel::KernelEvent::Failed(reason) => {
                    finished = true;
                    failed_reason = Some(reason);
                }
            }
        }
        if finished {
            self.kernel_running = None;
            if let Some(reason) = failed_reason {
                cell.state = crate::cell::CellState::Error;
                if !cell
                    .outputs
                    .iter()
                    .any(|o| matches!(o, crate::cell::Output::Error(_)))
                {
                    cell.outputs.push(crate::cell::Output::Error(reason));
                }
            } else {
                cell.state = crate::cell::CellState::Done;
            }
        }
        self.shared.set_notebook_cells(&self.document, state.cells);
        // 【2d-笔记级】全部运行队列推进：这个内核单元收口（ExecuteReply / Failed），
        // 接下一个（advance 内部守卫：队列空 / 还有单元在跑就不动）
        if finished {
            self.advance_notebook_queue(_cx);
        }
    }

    /// 中断单元执行（沿执行队列的取消；没在跑 = 如实回绝）
    pub(crate) fn notebook_cancel_run(&mut self, cx: &mut Context<Self>) {
        match self.shared.cancel_for(&self.document) {
            Ok(()) => self.set_message(Some("已请求中断".to_string()), cx),
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 【2d】收单元执行的结论（主线程轮询取走）：按会话 + 单元 id 回填内联输出
    pub(crate) fn drain_cell_notes(&mut self, cx: &mut Context<Self>) {
        let mut mine_arrived = 0usize;
        for note in self.shared.drain_cell_notes() {
            if note.document != self.document {
                continue;
            }
            mine_arrived += 1;
            self.shared.apply_cell_note(&self.document, &note);
        }
        // 回执按本文档的份数收口 pending（与脚本同口径）
        if mine_arrived > 0 {
            self.pending = self.pending.saturating_sub(mine_arrived);
            if self.pending == 0 {
                self.running_since = None;
            }
        }
        // 【2d-笔记级】全部运行队列推进：这个单元跑完（回执已落），接下一个
        if mine_arrived > 0 {
            self.advance_notebook_queue(cx);
        }
        cx.notify();
        cx.emit(EditorHostEvent::StatusChanged);
    }
}
