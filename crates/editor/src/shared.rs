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

use crate::channel::{ChannelAvailabilitySet, ChannelsHandle};
use crate::sources::SourcesHandle;
use crate::connection::{ConnectionOption, ConnectionsHandle, chip_for, status_text};
use crate::execution::{ExecQueue, QueryRunner};
use crate::service::{EditorService, OpenOutcome, OpenRequest};
use crate::session::{SavedSession, SessionStore};
use crate::store::ResultStore;

/// 另存为路径选择端口（由宿主注入：编辑器不依赖 `rfd`）
///
/// 入参 = （当前路径，默认文件名）；返回 = 用户选的路径（`None` = 用户取消，**不是错误**）。
pub type SavePathPicker =
    Rc<dyn Fn(Option<PathBuf>, String) -> Option<PathBuf>>;

/// 导出落盘的路径选择端口（由宿主注入：编辑器不依赖 `rfd`）
///
/// 入参 = （格式，默认文件名）；返回 = 用户选的路径（`None` = 用户取消，**不是错误**）。
/// 与另存为分开：导出的过滤条件与默认名规则不同（格式决定扩展名），且导出**不改文档身份**。
pub type ExportPathPicker = Rc<dyn Fn(crate::export::ExportFormat, String) -> Option<PathBuf>>;

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

/// 共享句柄（`Clone` 即克隆 `Rc`，各面板指向同一份状态）
#[derive(Clone)]
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
    /// 连接列表 / 建连端口：宿主注入后才有（B1；无宿主 = 选择器说“未接入”）
    connections: Rc<RefCell<Option<ConnectionsHandle>>>,
    /// 【B13】通道门控端口：宿主注入后才有（无宿主 = 加速 / 联邦两档都不可用并给原因）
    channels: Rc<RefCell<Option<ChannelsHandle>>>,
    /// 【B13/T1.6】源清单端口：宿主注入后才有（无宿主 = 「源清单 ▾」没有内容）
    sources: Rc<RefCell<Option<SourcesHandle>>>,
    /// 执行回执队列（宿主轮询取走；见 [`ExecReceipt`]）
    receipts: Rc<RefCell<Vec<ExecReceipt>>>,
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
            connections: Rc::new(RefCell::new(None)),
            channels: Rc::new(RefCell::new(None)),
            sources: Rc::new(RefCell::new(None)),
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
    pub fn results_active(&self, document: &crate::model::DocumentId) -> Option<crate::store::ResultEntry> {
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
        // 先把文档属性拷出来（不把服务层的 `Ref` 带到下面的借用里）：B1 的连接 + B13 的通道
        let (connection, channel) = {
            let service = self.service.borrow();
            (
                service.connection_for(&document),
                service.channel_for(&document),
            )
        };
        let guard = self.exec.borrow();
        let Some(channel_queue) = guard.as_ref() else {
            return Err(crate::execution::SubmitError::NoRunner);
        };
        channel_queue.submit(document, target, connection, placement, options, channel)
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

    /// 结果队列里已完成但尚未取走的执行（轮询泵调用）
    ///
    /// **顺带留一份回执**（[`ExecReceipt`]）：结果归编辑区，回执给宿主。
    pub fn drain_exec(&self) -> Vec<crate::execution::ExecOutcome> {
        let drained = {
            let guard = self.exec.borrow();
            guard.as_ref().map(|channel| channel.drain()).unwrap_or_default()
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
    pub fn pick_save_path(&self, current: Option<PathBuf>, default_name: String) -> Option<PathBuf> {
        let guard = self.save_path.borrow();
        guard.as_ref().and_then(|picker| picker(current, default_name))
    }

    /// 注入导出路径选择器（**宿主调用一次**：workbench 接 `rfd`）
    pub fn attach_export_path_picker(&self, picker: ExportPathPicker) {
        *self.export_path.borrow_mut() = Some(picker);
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
        guard.as_ref().and_then(|picker| picker(format, default_name))
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
        guard.as_ref().map(|port| port.options()).unwrap_or_default()
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
}
