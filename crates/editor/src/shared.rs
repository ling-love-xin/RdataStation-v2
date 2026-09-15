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

use crate::execution::{ExecChannel, QueryRunner};
use crate::service::{EditorService, OpenOutcome, OpenRequest};
use crate::session::{SavedSession, SessionStore};
use crate::store::ResultStore;

/// 另存为路径选择端口（由宿主注入：编辑器不依赖 `rfd`）
///
/// 入参 = （当前路径，默认文件名）；返回 = 用户选的路径（`None` = 用户取消，**不是错误**）。
pub type SavePathPicker =
    Rc<dyn Fn(Option<PathBuf>, String) -> Option<PathBuf>>;

/// 共享句柄（`Clone` 即克隆 `Rc`，各面板指向同一份状态）
#[derive(Clone)]
pub struct EditorShared {
    service: Rc<RefCell<EditorService>>,
    results: Rc<RefCell<ResultStore>>,
    /// 执行通道：宿主注入后才有（无宿主 = 无执行，这是真实的能力状态而非错误）
    exec: Rc<RefCell<Option<ExecChannel>>>,
    /// 会话存储：宿主注入后才有（无宿主 = 不持久化光标/模式）
    sessions: Rc<RefCell<Option<Rc<dyn SessionStore>>>>,
    /// 另存为路径选择器：宿主注入后才有（无宿主 = 另存为明确报“未接入”，不静默失败）
    save_path: Rc<RefCell<Option<SavePathPicker>>>,
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

    /// 便捷：打开文档（同路径重复打开 = 激活）
    pub fn open(&self, request: OpenRequest) -> OpenOutcome {
        self.update(|service| service.open(request))
    }

    /// 注入执行器（**宿主调用一次**；重复注入会换掉旧通道，旧通道的工作线程随之退出）
    pub fn attach_runner(&self, runner: Arc<dyn QueryRunner>) {
        *self.exec.borrow_mut() = Some(ExecChannel::new(runner));
    }

    /// 是否接了执行（未接时执行动作要明确说明，而不是静默）
    pub fn has_runner(&self) -> bool {
        self.exec.borrow().is_some()
    }

    /// 提交一次执行（借用边界内完成：不在 `Ref` 存活期间回调调用方）
    ///
    /// 对 `Option<ExecChannel>` 的借用刻意收在这里：`ExecChannel` 内部是
    /// `Sender + Arc`，调 `submit` 期间不会回到调用方，因此不构成重入风险。
    pub fn submit(
        &self,
        document: crate::model::DocumentId,
        target: &crate::execution::ExecTarget,
    ) -> Result<(), crate::execution::SubmitError> {
        let guard = self.exec.borrow();
        let Some(channel) = guard.as_ref() else {
            return Err(crate::execution::SubmitError::NoRunner);
        };
        channel.submit(document, target)
    }

    /// 结果队列里已完成但尚未取走的执行（轮询泵调用）
    pub fn drain_exec(&self) -> Vec<crate::execution::ExecOutcome> {
        let guard = self.exec.borrow();
        guard.as_ref().map(|channel| channel.drain()).unwrap_or_default()
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
}
