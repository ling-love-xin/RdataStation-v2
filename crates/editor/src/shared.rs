//! 编辑器共享状态（各面板共用同一份文档集合、结果集与执行通道）
//!
//! 唯一权威是 `EditorService`（文档，A1）与 `ResultStore`（结果，A14）：这里只提供
//! **可克隆的跨面板句柄**，让"一个面板 = 一个标签 = 一份文档"的多面板结构共享同一份状态
//! （标签条、状态栏、关闭逻辑、结果网格都读它）。
//!
//! 约定：**视图只读**（`service()` / `results()`），**变更走事件路径**（`update()` /
//! `update_results()`）——与全仓"render 是纯读路径"一致，避免渲染期改状态。
//!
//! 执行通道（`exec`）由**宿主注入**（`attach_runner`）：editor 不知道连接从哪来，
//! 只要求一个 `QueryRunner`（见 `execution` 模块）。未注入时执行动作会明确报"未接入执行"，
//! 而不是静默什么都不做。

use std::cell::{Ref, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use crate::channel::{ChannelAvailabilitySet, ChannelsHandle, ExecChannel};
use crate::completion::{Catalog, CompletionHandle};
use crate::connection::{ConnectionOption, ConnectionsHandle, chip_for, status_text};
use crate::execution::{ExecQueue, QueryRunner};
use crate::model::DocumentId;
use crate::project::{ProjectHandle, ProjectState};
use crate::scan::ScanScope;
use crate::service::{EditorService, OpenOutcome, OpenRequest};
use crate::session::{SavedSession, SessionStore};
use crate::sources::SourcesHandle;
use crate::store::ResultStore;

/// 另存为路径选择端口（由宿主注入：编辑器不依赖 `rfd`）
///
/// 入参 = （当前路径，默认文件名）；返回 = 用户选的路径（`None` = 用户取消，**不是错误**）。
pub type SavePathPicker = Rc<dyn Fn(Option<PathBuf>, String) -> Option<PathBuf>>;

/// 导出落盘的路径选择端口（由宿主注入：编辑器不依赖 `rfd`）
///
/// 入参 = （格式，默认文件名）；返回 = 用户选的路径（`None` = 用户取消，**不是错误**）。
/// 与另存为分开：导出的过滤条件与默认名规则不同（格式决定扩展名），且导出**不改文档身份**。
pub type ExportPathPicker = Rc<dyn Fn(crate::export::ExportFormat, String) -> Option<PathBuf>>;

/// 【M8】结果集列头右键「洞察此列」交给**宿主**的请求。
///
/// 编辑器不依赖 `insight`（分层：洞察是另一个特性 crate）：它只说「这一列 + 产生它的
/// SQL + 在哪条连接上跑出来的」，宿主自己翻译成 `SampleSource`。
///
/// **不物化结果集**（记录在案的决策）：洞察侧拿这段 SQL 重跑一句带 `LIMIT` 的取样。
/// 所以这里不给行数据，也不需要在编辑器侧管临时表生命周期。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsightColumnRequest {
    /// 点的那一列（列名）
    pub column: String,
    /// 产生这份结果的 SQL（洞察侧据此取样）
    pub sql: String,
    /// 这份结果跑在哪条连接上（取样要走源库；未绑定的结果不会给这个入口）
    pub connection: String,
    /// 展示用来源（面板副标题 / 快照来源），如「结果 2」
    pub label: String,
}

/// 【M8】「洞察此列」端口（由宿主注入：workbench 接到右 Dock 洞察面板）
pub type InsightColumnPort = Rc<dyn Fn(InsightColumnRequest, &mut gpui_kit::App)>;

/// 【B8 余项】「打开历史面板」端口（由宿主注入：workbench 接到右 Dock 历史面板）。
///
/// 工具栏「⟲ 历史」按钮点击后经它通知宿主——编辑器不依赖工作台的 Dock。
pub type OpenHistoryPort = Rc<dyn Fn(&mut gpui_kit::App)>;

/// 一次执行完成后的**回执**（给宿主看，不给编辑区看）。
///
/// 编辑器的结果归编辑区（`drain_exec`），回执只回答“哪份文档、用了哪个连接、成没成”——
/// 草稿箱据此把“这份草稿最后一次用哪个连接”写回 `file_meta`（M5 Phase C-2）。
/// 编辑器不关心谁在听，也不做任何过滤（是不是草稿由消费方判定）。
#[derive(Debug, Clone)]
pub struct ExecReceipt {
    pub document: crate::model::DocumentId,
    /// 实际使用的连接（`None` = 未绑定，跟随当前活动连接）。
    pub connection: Option<String>,
    /// 成功才值得记“最近执行”；也留着失败，交给消费方决定。
    pub succeeded: bool,
}

/// 无人取走时回执队列的上限（只保留最近的一批，避免无限长）。
const MAX_EXEC_RECEIPTS: usize = 64;

/// 一个内核条目的运行态（`kernel_id` → 会话 + 事件流 + 启动命令）
pub struct KernelEntry {
    /// 真内核会话（进程 + ZMQ 四通道；命令经它发出）
    pub session: rds_kernel::KernelSession,
    /// spawn 后立即取走的唯一事件流（持续 poll；`take_events` 每次都会换新通道，
    /// 但取走之后 session 内部那个就作废了——这里只持有最初那个）
    pub events: tokio::sync::mpsc::UnboundedReceiver<rds_kernel::KernelEvent>,
    /// 启动该内核用的解释器可执行路径（指示器回显；来自注册表 spec，见 [`crate::kernel`]）
    pub python: std::path::PathBuf,
}

/// 【#17 → #20】旧的单解释器来源已被注册表取代（见 [`crate::kernel`]）：本函数仅保留给
/// 测试 / 集成环境（`RDS_KERNEL_PYTHON` 优先，否则系统 `python`），不再参与 UI 路由。
pub fn kernel_python_path() -> std::path::PathBuf {
    std::env::var_os("RDS_KERNEL_PYTHON")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("python"))
}

/// 从语言推导默认内核 id（**#20 已废弃**：UI 路由走 [`Self::resolve_kernel_for`]，
/// 显式指定 / 会话默认 / 注册表第一个可用；本函数仅保留给测试断言语言→内核档位）
pub fn default_kernel_id(language: crate::cell::CellLanguage) -> &'static str {
    match language {
        crate::cell::CellLanguage::Python => "python3",
        crate::cell::CellLanguage::R => "r",
        crate::cell::CellLanguage::Julia => "julia",
        crate::cell::CellLanguage::Sql | crate::cell::CellLanguage::Markdown => "python3",
    }
}

/// 共享句柄（`Clone` 即克隆 `Rc`，各面板指向同一份状态）
pub struct EditorShared {
    service: Rc<RefCell<EditorService>>,
    results: Rc<RefCell<ResultStore>>,
    /// 执行通道：宿主注入后才有（无宿主 = 无执行，这是真实的能力状态而非错误）
    exec: Rc<RefCell<Option<ExecQueue>>>,
    /// 会话存储：宿主注入后才有（无宿主 = 不持久化光标/模式）
    sessions: Rc<RefCell<Option<Rc<dyn SessionStore>>>>,
    /// 另存为路径选择器：宿主注入后才有（无宿主 = 另存为明确报“未接入”，不静默失败）
    save_path: Rc<RefCell<Option<SavePathPicker>>>,
    /// 导出落盘路径选择器：宿主注入后才有（无宿主 = 导出明确报“未接入”）
    export_path: Rc<RefCell<Option<ExportPathPicker>>>,
    /// 【M8】「洞察此列」端口：宿主注入后才有（未接时列头右键不摆这一项）
    insight_column: Rc<RefCell<Option<InsightColumnPort>>>,
    /// 【B8 余项】「打开历史面板」端口：宿主注入后才有（未接时工具栏「历史」按钮置灰并给原因）
    open_history: Rc<RefCell<Option<OpenHistoryPort>>>,
    /// 连接列表 / 建连端口：宿主注入后才有（B1；无宿主 = 选择器说“未接入”）
    connections: Rc<RefCell<Option<ConnectionsHandle>>>,
    /// 【T2c-1】库 / schema 列表端口：宿主注入后才有（无宿主 = 状态栏面包屑没有可切项）
    schemas: Rc<RefCell<Option<crate::connection::SchemasHandle>>>,
    /// 【B13】通道门控端口：宿主注入后才有（无宿主 = 加速 / 联邦两档都不可用并给原因）
    channels: Rc<RefCell<Option<ChannelsHandle>>>,
    /// 【B13/T1.6】源清单端口：宿主注入后才有（无宿主 = 「源清单 ▾」没有内容）
    sources: Rc<RefCell<Option<SourcesHandle>>>,
    /// 项目态端口：宿主注入后才有（无宿主 = 不读只；项目锁是宿主的事实）
    project: Rc<RefCell<Option<ProjectHandle>>>,
    /// 补全端口：宿主注入后才有（无宿主 = 只给关键字与函数，不给元数据候选）
    completion: Rc<RefCell<Option<CompletionHandle>>>,
    /// 【2b】环境端口：宿主注入后才有（无宿主 = 环境列表为空，通用模式运行不亮）
    env: Rc<RefCell<Option<crate::env::EnvHandle>>>,
    /// 【LSP】语言服务端口：宿主注入后才有（无宿主 = 无语言服务，通用模式不接 LSP）
    lsp: Rc<RefCell<Option<crate::lsp::LspPortHandle>>>,
    /// 【B7 切片三】表结构端口（键列）：宿主注入后才有（无宿主 = 导出菜单不摆
    /// `SQL Updates`——没有键列的 `UPDATE` 只能猜，宁可回绝）
    table_keys: Rc<RefCell<Option<crate::export::TableKeysHandle>>>,
    /// 【2d】分析笔记状态（每文档一份：会话 + 单元序列；输出是会话级内存状态）
    notebooks: Rc<RefCell<std::collections::HashMap<crate::model::DocumentId, crate::cell::NotebookState>>>,
    /// 【#17】分析模式的内核注册表（kernel_id → 真内核会话 + 事件流）
    ///
    /// 生命周期 = 笔记本生命周期：`notebook_close` 时逐个 shutdown。启动 / 执行都是
    /// 事件路径（`KernelSession::spawn` 内部起后台线程，不阻塞 UI）；render 不碰它。
    kernels: Rc<RefCell<std::collections::HashMap<String, KernelEntry>>>,
    /// 【#20】内核注册表端口（宿主注入 `rds_kernel::detect_kernels()` 的快照；
    /// 未注入 = 空列表，菜单如实置灰）
    kernel_port: Rc<RefCell<Option<Rc<dyn crate::kernel::KernelsPort>>>>,
    /// 执行回执队列（宿主轮询取走；见 [`ExecReceipt`]）
    receipts: Rc<RefCell<Vec<ExecReceipt>>>,
}

impl Clone for EditorShared {
    fn clone(&self) -> Self {
        // 全部字段都是 `Rc`，克隆 = 复制句柄（KernelEntry 不 Clone——tokio 事件接收端
        // 只能有一个消费者，所以这里不能 derive）
        Self {
            service: self.service.clone(),
            results: self.results.clone(),
            exec: self.exec.clone(),
            sessions: self.sessions.clone(),
            save_path: self.save_path.clone(),
            export_path: self.export_path.clone(),
            insight_column: self.insight_column.clone(),
            open_history: self.open_history.clone(),
            connections: self.connections.clone(),
            schemas: self.schemas.clone(),
            channels: self.channels.clone(),
            sources: self.sources.clone(),
            project: self.project.clone(),
            completion: self.completion.clone(),
            env: self.env.clone(),
            lsp: self.lsp.clone(),
            table_keys: self.table_keys.clone(),
            notebooks: self.notebooks.clone(),
            kernels: self.kernels.clone(),
            kernel_port: self.kernel_port.clone(),
            receipts: self.receipts.clone(),
        }
    }
}

impl Default for EditorShared {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorShared {
    pub fn new() -> Self {
        Self {
            service: Rc::new(RefCell::new(EditorService::new())),
            results: Rc::new(RefCell::new(ResultStore::new())),
            exec: Rc::new(RefCell::new(None)),
            sessions: Rc::new(RefCell::new(None)),
            save_path: Rc::new(RefCell::new(None)),
            export_path: Rc::new(RefCell::new(None)),
            insight_column: Rc::new(RefCell::new(None)),
            open_history: Rc::new(RefCell::new(None)),
            connections: Rc::new(RefCell::new(None)),
            schemas: Rc::new(RefCell::new(None)),
            channels: Rc::new(RefCell::new(None)),
            sources: Rc::new(RefCell::new(None)),
            project: Rc::new(RefCell::new(None)),
            completion: Rc::new(RefCell::new(None)),
            env: Rc::new(RefCell::new(None)),
            lsp: Rc::new(RefCell::new(None)),
            table_keys: Rc::new(RefCell::new(None)),
            notebooks: Rc::new(RefCell::new(std::collections::HashMap::new())),
            kernels: Rc::new(RefCell::new(std::collections::HashMap::new())),
            kernel_port: Rc::new(RefCell::new(None)),
            receipts: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// 只读访问文档集合（渲染路径）
    pub fn service(&self) -> Ref<'_, EditorService> {
        self.service.borrow()
    }

    /// 变更文档集合（事件路径）：返回闭包结果，借用边界内完成
    pub fn update<R>(&self, change: impl FnOnce(&mut EditorService) -> R) -> R {
        change(&mut self.service.borrow_mut())
    }

    /// 只读访问结果集（渲染路径）
    pub fn results(&self) -> Ref<'_, ResultStore> {
        self.results.borrow()
    }

    /// 变更结果集（事件路径）
    pub fn update_results<R>(&self, change: impl FnOnce(&mut ResultStore) -> R) -> R {
        change(&mut self.results.borrow_mut())
    }

    /// 某文档当前选中的结果集（克隆一份：调用方拿着它做決定，不拖住 `Ref`）
    pub fn results_active(
        &self,
        document: &crate::model::DocumentId,
    ) -> Option<crate::store::ResultEntry> {
        self.results.borrow().active(document).cloned()
    }

    /// 便捷：打开文档（同路径重复打开 = 激活）
    pub fn open(&self, request: OpenRequest) -> OpenOutcome {
        self.update(|service| service.open(request))
    }

    /// 注入执行器（**宿主调用一次**；重复注入会换掉旧通道，旧通道的工作线程随之退出）
    pub fn attach_runner(&self, runner: Arc<dyn QueryRunner>) {
        *self.exec.borrow_mut() = Some(ExecQueue::new(runner));
    }

    /// 是否接了执行（未接时执行动作要明确说明，而不是静默）
    pub fn has_runner(&self) -> bool {
        self.exec.borrow().is_some()
    }

    /// 提交一次执行（借用边界内完成：不在 `Ref` 存活期间回调调用方）
    ///
    /// 文档绑定的连接（B1）在这里取出并随请求交给执行器：调用方（面板 / 菜单）不需要知道
    /// 连接从哪来，只要知道“执行这份文档”。`placement` 决定结果落到当前结果集还是新结果集（B2）。
    ///
    /// 对 `Option<ExecQueue>` 的借用刻意收在这里：`ExecQueue` 内部是
    /// `Sender + Arc`，调 `submit` 期间不会回到调用方，因此不构成重入风险。
    pub fn submit(
        &self,
        document: crate::model::DocumentId,
        target: &crate::execution::ExecTarget,
        placement: crate::execution::ResultPlacement,
        options: crate::execution::RunOptions,
    ) -> Result<(), crate::execution::SubmitError> {
        // 先把文档属性拷出来（不把服务层的 `Ref` 带到下面的借用里）：B1 的连接 + T2c-1 的 schema + B13 的通道
        let (connection, schema, channel) = {
            let service = self.service.borrow();
            (
                service.connection_for(&document),
                service.schema_for(&document),
                service.channel_for(&document),
            )
        };
        let guard = self.exec.borrow();
        let Some(channel_queue) = guard.as_ref() else {
            return Err(crate::execution::SubmitError::NoRunner);
        };
        channel_queue.submit(
            document,
            target,
            connection,
            schema,
            placement,
            options,
            channel,
        )
    }

    /// 【B13】请一个源动作（重挂 / 换主源）
    ///
    /// 与 [`Self::submit`] 同一口径：连接从**服务层**取（调用方不需要知道绑定在哪）。
    pub fn request_source_action(
        &self,
        document: crate::model::DocumentId,
        connection: Option<String>,
        channel: crate::channel::ExecChannel,
        action: crate::execution::SourceAction,
    ) -> Result<(), String> {
        let guard = self.exec.borrow();
        let Some(queue) = guard.as_ref() else {
            return Err("当前未接入执行".to_string());
        };
        queue.request_source_action(document, connection, channel, action)
    }

    /// 【B13】源动作的回执（面板轮询取走）
    pub fn drain_source_notes(&self) -> Vec<crate::execution::SourceNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|queue| queue.drain_source_notes())
            .unwrap_or_default()
    }

    /// 【B7 切片二】请一次 DuckDB 导出（Parquet / XLSX）
    ///
    /// 与 [`Self::request_source_action`] 同一口径：没有执行器就直说（不静默失败）。
    pub fn request_duckdb_export(
        &self,
        request: crate::execution::DuckDbExportRequest,
    ) -> Result<(), String> {
        let guard = self.exec.borrow();
        let Some(queue) = guard.as_ref() else {
            return Err("当前未接入执行".to_string());
        };
        queue.request_duckdb_export(request)
    }

    /// 【B7 切片二】DuckDB 导出的回执（面板轮询取走）
    pub fn drain_export_notes(&self) -> Vec<crate::execution::ExportNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|queue| queue.drain_export_notes())
            .unwrap_or_default()
    }

    /// 结果队列里已完成但尚未取走的执行（轮询泵调用）
    ///
    /// **顺带留一份回执**（[`ExecReceipt`]）：结果归编辑区，回执给宿主。
    pub fn drain_exec(&self) -> Vec<crate::execution::ExecOutcome> {
        let drained = {
            let guard = self.exec.borrow();
            guard
                .as_ref()
                .map(|channel| channel.drain())
                .unwrap_or_default()
        };
        if !drained.is_empty() {
            let mut receipts = self.receipts.borrow_mut();
            for outcome in &drained {
                receipts.push(ExecReceipt {
                    document: outcome.document.clone(),
                    connection: outcome.connection.clone(),
                    succeeded: outcome.result.is_ok(),
                });
            }
            let overflow = receipts.len().saturating_sub(MAX_EXEC_RECEIPTS);
            if overflow > 0 {
                receipts.drain(0..overflow);
            }
        }
        drained
    }

    /// 取走执行回执（宿主轮询调用；每个回执只交付一次）
    pub fn drain_exec_receipts(&self) -> Vec<ExecReceipt> {
        std::mem::take(&mut self.receipts.borrow_mut())
    }

    /// 【2b】脚本执行的结论（宿主轮询取走；脚本结果不是网格形状，走独立通道）
    pub fn drain_script_notes(&self) -> Vec<crate::env::ScriptNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|channel| channel.drain_script_notes())
            .unwrap_or_default()
    }

    // ===== 【2d】分析笔记：会话级单元序列（每文档一份） =====

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
            crate::execution::ResultPlacement::Replace,
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

    /// 中断当前执行（B3）；没在跑就回绝（理由可读，不是静默）
    pub fn cancel(&self) -> Result<(), String> {
        let guard = self.exec.borrow();
        match guard.as_ref() {
            Some(channel) => channel.cancel(),
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

    /// 请一个事务动作（B4）：开始 / 提交 / 回滚；没在跑才接受（理由可读）
    pub fn request_transaction(
        &self,
        document: crate::model::DocumentId,
        action: crate::execution::TxAction,
    ) -> Result<(), String> {
        let connection = self.service.borrow().connection_for(&document);
        let guard = self.exec.borrow();
        match guard.as_ref() {
            Some(channel) => channel.request_transaction(document, action, connection),
            None => Err("当前未接入执行".to_string()),
        }
    }

    /// 事务动作的结论（主线程轮询）
    pub fn drain_tx_notes(&self) -> Vec<crate::execution::TxNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|channel| channel.drain_tx_notes())
            .unwrap_or_default()
    }

    /// 【B4】执行器支不支持事务（界面据此决定摆不摆 TX 区）
    pub fn has_transactions(&self) -> bool {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|channel| channel.supports_transactions())
            .unwrap_or(false)
    }

    /// 【T2a】请一次批量提交（把变更集写回源库）
    ///
    /// 与 [`Self::request_transaction`] 同一口径：连接从**服务层**取（调用方不需要知道绑定在哪），
    /// 没有执行器就直说（不静默失败）。`updates` 是编辑器侧生成好的 UPDATE 文本
    /// （`changes::commit_updates_rows`），`sql` 是原查询（血缘 / 历史），`table` / `key` 供诊断。
    pub fn request_commit_changes(
        &self,
        document: crate::model::DocumentId,
        table: String,
        key: crate::export::RowKey,
        sql: String,
        updates: String,
    ) -> Result<(), String> {
        let (connection, schema, channel) = {
            let service = self.service.borrow();
            (
                service.connection_for(&document),
                service.schema_for(&document),
                service.channel_for(&document),
            )
        };
        let guard = self.exec.borrow();
        match guard.as_ref() {
            Some(queue) => queue.request_commit_changes(
                document,
                connection,
                schema,
                channel,
                table,
                key,
                sql,
                updates,
            ),
            None => Err("当前未接入执行".to_string()),
        }
    }

    /// 【T2a】批量提交的回执（面板轮询取走）
    pub fn drain_commit_notes(&self) -> Vec<crate::execution::CommitNote> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .map(|queue| queue.drain_commit_notes())
            .unwrap_or_default()
    }

    /// 是否有执行在跑
    pub fn is_executing(&self) -> bool {
        let guard = self.exec.borrow();
        guard.as_ref().is_some_and(|channel| channel.is_busy())
    }

    /// 注入会话存储（**宿主调用一次**；A12）
    pub fn attach_session_store(&self, store: Rc<dyn SessionStore>) {
        *self.sessions.borrow_mut() = Some(store);
    }

    /// 是否接了会话存储（未接时会话不落库，但不影响编辑）
    pub fn has_session_store(&self) -> bool {
        self.sessions.borrow().is_some()
    }

    /// 保存一份会话（事件路径调用；未注入存储时静默成功）
    pub fn save_session(&self, session: &SavedSession) -> Result<(), String> {
        let guard = self.sessions.borrow();
        match guard.as_ref() {
            Some(store) => store.save(session),
            None => Ok(()),
        }
    }

    /// 取最近更新的会话（启动恢复用；未注入存储时视为没有）
    pub fn load_latest_session(&self) -> Result<Option<SavedSession>, String> {
        let guard = self.sessions.borrow();
        match guard.as_ref() {
            Some(store) => store.load_latest(),
            None => Ok(None),
        }
    }

    /// 注入另存为路径选择器（**宿主调用一次**：workbench 接 `rfd`）
    pub fn attach_save_path_picker(&self, picker: SavePathPicker) {
        *self.save_path.borrow_mut() = Some(picker);
    }

    /// 是否接了路径选择器（未接时“另存为”要明确报原因）
    pub fn has_save_path_picker(&self) -> bool {
        self.save_path.borrow().is_some()
    }

    /// 弹一次路径选择（未注入时返回 `None`，调用方必须先看 [`Self::has_save_path_picker`]）
    pub fn pick_save_path(
        &self,
        current: Option<PathBuf>,
        default_name: String,
    ) -> Option<PathBuf> {
        let guard = self.save_path.borrow();
        guard
            .as_ref()
            .and_then(|picker| picker(current, default_name))
    }

    /// 注入导出路径选择器（**宿主调用一次**：workbench 接 `rfd`）
    pub fn attach_export_path_picker(&self, picker: ExportPathPicker) {
        *self.export_path.borrow_mut() = Some(picker);
    }

    /// 【M8】注入「洞察此列」端口（**宿主调用一次**：workbench 接到洞察面板）
    pub fn attach_insight_column(&self, port: InsightColumnPort) {
        *self.insight_column.borrow_mut() = Some(port);
    }

    /// 【M8】「洞察此列」端口（未注入时列头右键不摆这一项）
    pub fn insight_column_port(&self) -> Option<InsightColumnPort> {
        self.insight_column.borrow().clone()
    }

    /// 【B8 余项】注入「打开历史面板」端口（由宿主在装配期调用一次）
    pub fn attach_open_history(&self, port: OpenHistoryPort) {
        *self.open_history.borrow_mut() = Some(port);
    }

    /// 【B8 余项】「打开历史面板」端口（未注入时工具栏按钮置灰并给原因）
    pub fn open_history_port(&self) -> Option<OpenHistoryPort> {
        self.open_history.borrow().clone()
    }

    /// 【M8】文档未绑定连接时执行会落到哪条连接上（口径在执行器：绑定优先 → 回退活动连接）
    pub fn active_connection(&self) -> Option<String> {
        let guard = self.exec.borrow();
        guard
            .as_ref()
            .and_then(|queue| queue.runner().active_connection())
    }

    /// 是否接了导出路径选择器（未接时导出要明确报原因）
    pub fn has_export_path_picker(&self) -> bool {
        self.export_path.borrow().is_some()
    }

    /// 弹一次导出路径选择（未注入时返回 `None`，调用方必须先看 [`Self::has_export_path_picker`]）
    pub fn pick_export_path(
        &self,
        format: crate::export::ExportFormat,
        default_name: String,
    ) -> Option<PathBuf> {
        let guard = self.export_path.borrow();
        guard
            .as_ref()
            .and_then(|picker| picker(format, default_name))
    }

    /// 注入连接端口（**宿主调用一次**：workbench 接 M3/M4 的连接列表与自动建连）
    pub fn attach_connections(&self, port: ConnectionsHandle) {
        *self.connections.borrow_mut() = Some(port);
    }

    /// 是否接了连接端口（未接时选择器要说“未接入”，而不是看起来有连接可选）
    pub fn has_connections(&self) -> bool {
        self.connections.borrow().is_some()
    }

    /// 当前可选项快照（**渲染路径可调**：实现必须是内存快照，不做 I/O）
    pub fn connection_options(&self) -> Vec<ConnectionOption> {
        let guard = self.connections.borrow();
        guard
            .as_ref()
            .map(|port| port.options())
            .unwrap_or_default()
    }

    /// 【T2c-1】注入 schema 端口（**宿主调用一次**：workbench 从导航缓存读库 / schema 列表）
    pub fn attach_schemas(&self, port: crate::connection::SchemasHandle) {
        *self.schemas.borrow_mut() = Some(port);
    }

    /// 【T2c-1】某连接的库 / schema 列表（**渲染路径可调**：端口实现必须是内存快照；
    /// 未接端口 = 空列表 → 状态栏不显示可切的面包屑，如实降级）
    pub fn schemas(&self, conn_id: Option<&str>) -> Vec<crate::connection::SchemaInfo> {
        let guard = self.schemas.borrow();
        match guard.as_ref() {
            Some(port) => conn_id.map(|id| port.schemas(id)).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// 【T2c-1】某文档绑定的 schema（`None` = 未选或文档不存在）
    pub fn document_schema(&self, id: &DocumentId) -> Option<String> {
        self.service().schema_for(id)
    }

    /// 【B7 切片三】注入表结构端口（**宿主调用一次**：workbench 从导航缓存读键列）
    pub fn attach_table_keys(&self, port: crate::export::TableKeysHandle) {
        *self.table_keys.borrow_mut() = Some(port);
    }

    /// 【#20】注入内核注册表端口（**宿主调用一次**：workbench 把
    /// `rds_kernel::detect_kernels()` 的快照注进来；探测在宿主侧后台线程做一次）
    pub fn attach_kernels(&self, port: Rc<dyn crate::kernel::KernelsPort>) {
        *self.kernel_port.borrow_mut() = Some(port);
    }

    /// 【#20】内核注册表快照（渲染路径可调：端口实现是内存读；未接端口 = 空）
    pub fn kernel_specs(&self) -> Vec<rds_kernel::KernelSpec> {
        let guard = self.kernel_port.borrow();
        guard
            .as_ref()
            .map(|port| port.list())
            .unwrap_or_default()
    }

    /// 【#20】执行源「内核」档的实际内核解析（**纯函数**，见 [`crate::kernel::resolve_kernel`]）：
    /// 显式指定 > 会话默认（上次用过的）> 注册表第一个可用；一个都没有给可读原因。
    pub fn resolve_kernel_for(
        &self,
        document: &crate::model::DocumentId,
        source: Option<&crate::cell::ExecutionSource>,
        language: crate::cell::CellLanguage,
    ) -> Result<rds_kernel::KernelSpec, String> {
        let session_default = self
            .notebook_state(document)
            .session
            .default_kernel
            .clone();
        crate::kernel::resolve_kernel(&self.kernel_specs(), source, session_default.as_deref(), language)
    }

    /// 【B7 切片三】某连接下表的主键列（**只在导出菜单弹出时查一次**——事件路径；
    /// 端口实现读导航 L2 缓存，非每帧调用）。未接端口 / 查不到 = `None`
    /// （菜单不摆 `SQL Updates`，如实降级）
    pub fn table_key_columns(&self, conn_id: Option<&str>, table: &str) -> Option<Vec<String>> {
        let guard = self.table_keys.borrow();
        match (guard.as_ref(), conn_id) {
            (Some(port), Some(conn)) => port.primary_key_columns(conn, table),
            _ => None,
        }
    }

    /// 【T2c-3】文档字节数（状态栏「大小」微标；O(1)）
    pub fn document_len(&self, id: &DocumentId) -> usize {
        self.service().document_len(id)
    }

    /// 【T2c-4】文档缩进宽度（状态栏「Sp」微标 + Tab 键缩进的权威值）
    pub fn document_tab_size(&self, id: &DocumentId) -> usize {
        self.service().document_tab_size(id)
    }

    /// 【T2c-4】重设文档缩进宽度；返回是否真变
    pub fn set_document_tab_size(&self, id: &DocumentId, size: usize) -> bool {
        self.update(|svc| svc.set_document_tab_size(id, size))
    }

    /// 【T2c-4】文档缩进是否用制表符（`true` = Tab 缩进，状态栏显「Tab」）
    pub fn document_hard_tabs(&self, id: &DocumentId) -> bool {
        self.service().document_hard_tabs(id)
    }

    /// 【T2c-4】重设缩进用制表符还是空格；返回是否真变
    pub fn set_document_hard_tabs(&self, id: &DocumentId, hard: bool) -> bool {
        self.update(|svc| svc.set_document_hard_tabs(id, hard))
    }

    /// 【2c】文档是否显示空白字符（空格 `•` / 制表符 `→`）
    pub fn document_show_whitespaces(&self, id: &DocumentId) -> bool {
        self.service().document_show_whitespaces(id)
    }

    /// 【2c】重设显示空白字符；返回是否真变
    pub fn set_document_show_whitespaces(&self, id: &DocumentId, show: bool) -> bool {
        self.update(|svc| svc.set_document_show_whitespaces(id, show))
    }

    /// 【2c】文档是否显示隐藏字符标注
    pub fn document_show_invisible(&self, id: &DocumentId) -> bool {
        self.service().document_show_invisible(id)
    }

    /// 【2c】重设显示隐藏字符标注；返回是否真变
    pub fn set_document_show_invisible(&self, id: &DocumentId, show: bool) -> bool {
        self.update(|svc| svc.set_document_show_invisible(id, show))
    }

    /// 【2c】文档当前保存格式（编码 + 换行）
    pub fn document_save_format(&self, id: &DocumentId) -> crate::encoding::SaveFormat {
        self.service().document_save_format(id)
    }

    /// 【2c】重设文档保存格式（编码 / 换行）；返回是否真变
    pub fn set_document_save_format(
        &self,
        id: &DocumentId,
        format: crate::encoding::SaveFormat,
    ) -> bool {
        self.update(|svc| svc.set_document_save_format(id, format))
    }

    /// 确保某个连接已建连（事件路径调用：可能真的去建连）
    pub fn ensure_connected(&self, conn_id: &str) -> Result<(), String> {
        let guard = self.connections.borrow();
        match guard.as_ref() {
            Some(port) => port.ensure_connected(conn_id),
            None => Err("当前未接入连接列表".to_string()),
        }
    }

    /// 某文档的绑定在状态栏 / 工具栏上的文案（纯函数：端口未接就是“未绑定”）
    pub fn connection_status_text(&self, conn_id: Option<&str>) -> String {
        status_text(conn_id, &self.connection_options())
    }

    /// 某文档的绑定的展示数据（工具栏用；`None` = 未绑定或认不出）
    pub fn connection_chip(
        &self,
        conn_id: Option<&str>,
    ) -> Option<crate::connection::ConnectionChip> {
        chip_for(conn_id, &self.connection_options())
    }

    /// 【B13】注入通道门控端口（**宿主调用一次**：workbench 读连接的本地加速开关与 DuckDB 就绪）
    pub fn attach_channels(&self, port: ChannelsHandle) {
        *self.channels.borrow_mut() = Some(port);
    }

    /// 是否接了通道端口（未接 = 加速 / 联邦都不可用，并给出原因，而不是假装可选）
    pub fn has_channels(&self) -> bool {
        self.channels.borrow().is_some()
    }

    /// 某连接上「本地加速 / 联邦」的可用性（**渲染路径可调**：实现必须是内存快照）
    pub fn channel_availability(&self, conn_id: Option<&str>) -> ChannelAvailabilitySet {
        let guard = self.channels.borrow();
        match guard.as_ref() {
            Some(port) => port.availability(conn_id),
            None => ChannelAvailabilitySet::blocked("尚未接入通道能力"),
        }
    }

    /// 【T1.6】注入源清单端口（**宿主调用一次**：workbench 读引擎的联邦会话快照）
    pub fn attach_sources(&self, port: SourcesHandle) {
        *self.sources.borrow_mut() = Some(port);
    }

    /// 是否接了源清单端口
    pub fn has_sources(&self) -> bool {
        self.sources.borrow().is_some()
    }

    /// 某连接上的联邦源清单（**渲染路径可调**：实现必须是内存快照；`None` = 还没会话）
    pub fn sources_snapshot(&self, conn_id: &str) -> Option<crate::sources::SourcesSnapshot> {
        let guard = self.sources.borrow();
        guard.as_ref()?.snapshot(conn_id)
    }

    /// 注入项目态端口（**宿主调用一次**：workbench 读项目锁）
    pub fn attach_project(&self, port: ProjectHandle) {
        *self.project.borrow_mut() = Some(port);
    }

    /// 项目态（**渲染路径可调**：实现必须是内存读；未接端口 = 不读只）
    pub fn project_state(&self) -> ProjectState {
        let guard = self.project.borrow();
        match guard.as_ref() {
            Some(port) => port.state(),
            None => ProjectState::default(),
        }
    }

    /// 项目现在是不是只读（闸门用的就是它）
    pub fn project_read_only(&self) -> bool {
        self.project_state().read_only
    }

    /// 注入补全端口（**宿主调用一次**：workbench 接连接级元数据缓存）
    pub fn attach_completion(&self, port: CompletionHandle) {
        *self.completion.borrow_mut() = Some(port);
    }

    /// 【2b】注入环境端口（**宿主调用一次**：workbench 接系统解释器探测）
    pub fn attach_env(&self, port: crate::env::EnvHandle) {
        *self.env.borrow_mut() = Some(port);
    }

    /// 【2b】可用环境列表（未接端口 = 空，如实；缺解释器 = 运行不亮）
    pub fn envs(&self) -> Vec<crate::env::Environment> {
        let port = self.env.borrow();
        port.as_ref().map(|port| port.list()).unwrap_or_default()
    }

    /// 【2b】该环境是否已安装（命令可达；运行按钮据此亮/灰）
    pub fn env_installed(&self, env_id: &str) -> bool {
        let port = self.env.borrow();
        port.as_ref()
            .map(|port| port.installed(env_id))
            .unwrap_or(false)
    }

    /// 【2b】环境端口句柄（管理环境 modal 用：增删改要直呼宿主端口；未接 = None）
    pub fn env_port(&self) -> Option<crate::env::EnvHandle> {
        self.env.borrow().clone()
    }

    /// 【LSP】注入语言服务端口（**宿主调用一次**：workbench / 插件宿主接「文档 → 服务」）
    pub fn attach_lsp(&self, port: crate::lsp::LspPortHandle) {
        *self.lsp.borrow_mut() = Some(port);
    }

    /// 【LSP】文档当前可用的语言服务（未接端口 / 宿主判无可服务 = `None`，如实）
    pub fn lsp_service(&self, document: &DocumentId) -> Option<crate::lsp::LspSession> {
        let port = self.lsp.borrow();
        port.as_ref().and_then(|port| port.service(document))
    }

    /// 【LSP】取走**属于本文档**的语言服务事件（未接端口 = 空；端口按 `uri` 认领）
    pub fn drain_lsp_events(&self, document: &DocumentId) -> Vec<crate::lsp::LspEvent> {
        let port = self.lsp.borrow();
        port.as_ref()
            .map(|port| port.drain_for(document))
            .unwrap_or_default()
    }

    /// 【LSP】本文档是否**想要**语言服务（未接端口 = false，如实）
    ///
    /// 与 [`Self::lsp_service`] 的分工：那个只在服务就绪时给会话，这个不管启没启都回"本该配
    /// 服务"。面板据此在服务未起 / 被停 / 崩溃时保持轮询，重启后自动重连。
    pub fn lsp_wants(&self, document: &DocumentId) -> bool {
        let port = self.lsp.borrow();
        port.as_ref()
            .is_some_and(|port| port.wants(document))
    }

    /// 是否接了补全端口（未接 = 只给关键字与函数）
    pub fn has_completion(&self) -> bool {
        self.completion.borrow().is_some()
    }

    /// 【B9 切片二】模板片段（菜单弹出时读；未接端口 = 空）
    ///
    /// 与 [`Self::completion_catalog`] 同一口径：端口实现必须是**内存读**。
    pub fn completion_templates(&self) -> Vec<crate::completion::TemplateSnippet> {
        let port = self.completion.borrow();
        port.as_ref()
            .map(|port| port.templates())
            .unwrap_or_default()
    }

    /// 某文档当前的**候选目录**（**编辑路径可调**：实现必须是内存快照；未绑定连接 = 空目录）
    pub fn completion_catalog(&self, document: &DocumentId) -> Catalog {
        let (connection, channel) = {
            let service = self.service.borrow();
            match service.find(document) {
                Some(doc) => (doc.connection().map(str::to_string), doc.channel()),
                None => return Catalog::default(),
            }
        };
        let port = self.completion.borrow();
        match port.as_ref() {
            Some(port) => port.catalog(connection.as_deref(), channel),
            None => Catalog::default(),
        }
    }

    /// 【T2a-2】行标识探测：表的主键列（**内存读**；结果集回执路径可调）
    ///
    /// 与 [`Self::completion_catalog`] 同一端口与口径：`conn_id` / `channel` 是**结果实际
    /// 跑在哪条连接、哪个通道**（来自 `ResultEntry`，不是文档当前绑定——执行时可能跟随
    /// 当前活动连接）。未绑定连接 / 未接端口 / 宿主探测不到 = `None`（如实不给行标识，
    /// 判据 `NoRowKey` 会把该结果整表置为只读，不编造键）。
    pub fn primary_key_for(
        &self,
        conn_id: Option<&str>,
        channel: ExecChannel,
        table: &str,
    ) -> Option<Vec<String>> {
        let port = self.completion.borrow();
        port.as_ref()
            .and_then(|port| port.primary_key(conn_id, channel, table))
    }

    /// 补全开不开：能力表（文本模式 / 分析模式的每单元由能力表回答）+ 编辑器只读 + 文件档位
    ///
    /// 大文件档位（>50MB）关重能力是 A13 的硬口径——**这里是那条口径的运行时落地**。
    pub fn completion_enabled(&self, document: &DocumentId) -> bool {
        let service = self.service.borrow();
        let Some(doc) = service.find(document) else {
            return false;
        };
        doc.capabilities().completion
            && doc.read_only().can_edit()
            && !doc.tier().disables_completion()
    }

    /// 折叠开不开：能力表 + 文件档位
    ///
    /// 与 [`Self::completion_enabled`] 同形，但**不看编辑器只读**——折叠是「看结构」的能力，
    /// 只读文档（查看历史快照 / 只读预览）照样能折，把它关掉只会让长脚本更难读。
    pub fn folding_enabled(&self, document: &DocumentId) -> bool {
        let service = self.service.borrow();
        let Some(doc) = service.find(document) else {
            return false;
        };
        doc.capabilities().folding && !doc.tier().disables_folding()
    }

    /// 【B18】后台扫描的范围（折叠 / 诊断两个重活开不开）
    ///
    /// 判据只有一处：档位的降级表 [`crate::limits::FileTier::plan`]——不在这里另写一套
    /// `matches!(tier, Normal)`。只读不影响扫描（看结构不需要写权限）。
    pub fn scan_scope(&self, document: &DocumentId) -> ScanScope {
        let service = self.service.borrow();
        let Some(doc) = service.find(document) else {
            return ScanScope::none();
        };
        let plan = doc.tier().plan();
        ScanScope {
            folds: plan.folding && doc.capabilities().folding,
            diagnostics: plan.diagnostics,
        }
    }
}

/// 【2d】把单元执行的结论拼成内联表格要的 [`crate::store::ResultEntry`]
///
/// 内联表格复用 2a 共用表格（吃 `ResultEntry`）：连接 / 通道在单元上下文里没有面板那一套
/// 结果区元数据，这里用 `None` / 默认通道如实带过（单元卡片自己展示连接信息）。
fn build_inline_entry(
    document: &crate::model::DocumentId,
    outcome: &crate::cell::CellOutcome,
) -> crate::store::ResultEntry {
    let mut entry = match &outcome.result {
        Ok(query) => crate::store::ResultEntry::success(
            document.clone(),
            outcome.sql.clone(),
            outcome.elapsed_ms,
            outcome.truncated,
            query.columns.clone(),
            query.rows.clone(),
        )
        .with_column_types(&query.column_types),
        Err(reason) => crate::store::ResultEntry::failure(
            document.clone(),
            outcome.sql.clone(),
            reason.clone(),
            outcome.elapsed_ms,
        ),
    };
    entry.affected_rows = outcome.affected_rows;
    entry.lineage = Some("单元执行".to_string());
    entry
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::EditorShared;
    use crate::cell::{Cell, CellLanguage, CellNote, CellOutcome, CellRequest, CellState, NotebookState, Output};
    use crate::model::DocumentId;
    use crate::session::Session;

    fn notebook(doc: &DocumentId) -> (EditorShared, NotebookState) {
        let shared = EditorShared::new();
        let mut state = NotebookState::new();
        let mut c1 = Cell::new("c1", CellLanguage::Sql, "select 1");
        c1.mark_running();
        state.cells = vec![c1, Cell::new("c2", CellLanguage::Markdown, "# 备注")];
        shared.set_notebook_state(doc, state.clone());
        (shared, state)
    }

    fn note(doc: &DocumentId, session_id: &str, cell_id: &str, ok: bool) -> CellNote {
        let result = if ok {
            Ok(CellOutcome {
                sql: "select 1".to_string(),
                elapsed_ms: 2,
                affected_rows: None,
                truncated: false,
                result: Ok(crate::execution::QueryData::default()),
            })
        } else {
            Err("表不存在".to_string())
        };
        CellNote {
            document: doc.clone(),
            session_id: session_id.to_string(),
            cell_id: cell_id.to_string(),
            result,
        }
    }

    /// 【2d】回执落单元：Running → Done（带内联 Table 输出）；失败 → Error
    #[test]
    fn applying_a_note_marks_the_running_cell_done_or_error() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);

        // 成功回执 → Done + Table 输出（内联表格吃 ResultEntry）
        shared.apply_cell_note(&doc, &note(&doc, "default", "c1", true));
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells[0].state, CellState::Done);
        assert!(
            matches!(&cells[0].outputs[0], Output::Table(entry) if entry.columns.is_empty()),
            "成功回执拼成内联 Table"
        );
        assert!(
            cells[0]
                .outputs
                .iter()
                .any(|o| matches!(o, Output::Table(_))),
            "回执输出是 Table"
        );
    }

    /// 会话切换后到期的回执**不落**（会话已经换了，别把旧结果贴进新会话）
    #[test]
    fn a_note_from_a_stale_session_is_dropped() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);

        // 先切会话（c1 在跑 → 状态回 Idle，输出清空）
        shared.switch_notebook_session(&doc, Session::new("s2", Some("P_orders".to_string()), None));
        // 旧会话（default）的回执到了：不落
        shared.apply_cell_note(&doc, &note(&doc, "default", "c1", true));
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells[0].state, CellState::Idle, "旧会话回执不落");
        assert!(cells[0].outputs.is_empty());
        assert_eq!(shared.notebook_session(&doc).id, "s2");
        assert_eq!(shared.notebook_session(&doc).default_connection.as_deref(), Some("P_orders"));
    }

    /// 没在跑的回执不落（防迟到的旧回执覆盖新结果）
    #[test]
    fn a_note_for_an_idle_cell_is_dropped() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);
        // c2 从没跑过（Idle）：回执到了不落
        shared.apply_cell_note(&doc, &note(&doc, "default", "c2", true));
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells[1].state, CellState::Idle);
        assert!(cells[1].outputs.is_empty());
    }

    /// 提交单元执行：没有执行器 = 如实拒绝（不静默）
    #[test]
    fn submit_cell_without_a_runner_is_rejected() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);
        let request = CellRequest {
            session_id: "default".to_string(),
            cell_id: "c1".to_string(),
            sql: "select 1".to_string(),
            conn_id: None,
        };
        let err = shared.submit_cell(doc.clone(), &request).expect_err("没接执行器该被拒");
        assert!(!err.is_empty());
    }

    /// 替换单元序列 / 会话是纯内存操作：快照原样往返
    #[test]
    fn cells_and_session_round_trip_through_the_holder() {
        let doc = DocumentId::new("nb");
        let shared = EditorShared::new();
        let mut state = NotebookState::new();
        state.switch_session(Session::new("s9", None, Some("py".to_string())));
        state.cells = vec![Cell::new("x1", CellLanguage::Sql, "select 9")];
        shared.set_notebook_state(&doc, state.clone());

        assert_eq!(shared.notebook_session(&doc).id, "s9");
        assert_eq!(shared.notebook_session(&doc).env.as_deref(), Some("py"));
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].id, "x1");
    }

    /// 【2d】切换会话会把全部有输出的单元标 stale（T2d-2 验收口径）
    #[test]
    fn switching_session_stales_cells_in_the_holder() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);
        // 先让 c1 跑出结果（Done），再切会话
        shared.apply_cell_note(&doc, &note(&doc, "default", "c1", true));
        assert_eq!(shared.notebook_cells(&doc)[0].state, CellState::Done);

        shared.switch_notebook_session(&doc, Session::default_session());
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells[0].state, CellState::Stale, "切换会话 → 有输出单元 stale");
    }
}
