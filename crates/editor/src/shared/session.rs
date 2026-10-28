//! 编辑器共享状态 —— 会话端口（session store 挂载/save/load/pick_save_path/insight/历史/文档显示）
//!
//! 自 shared.rs 拆分（2026-10-29）：按能力域切分 `impl EditorShared` 方法块。
//! 端口方法全 pub（跨域互调）；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为。

use std::path::PathBuf;
use std::rc::Rc;

use crate::session::{SavedSession, SessionStore};

use super::*;

impl EditorShared {

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

    /// 取全部持久化会话（启动多文档恢复用；未注入存储时视为没有）
    pub fn load_all_sessions(&self) -> Result<Vec<SavedSession>, String> {
        let guard = self.sessions.borrow();
        match guard.as_ref() {
            Some(store) => store.load_all(),
            None => Ok(Vec::new()),
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

    /// 【#54 C6/C7】注入「跳转定义目标解析」端口（宿主在装配期调用一次；
    /// 未注入 = 内核走默认文档内移动，见 [`ShowDocumentPort`]）
    pub fn attach_show_document(&self, port: ShowDocumentPort) {
        *self.show_document_port.borrow_mut() = Some(port);
    }

    /// 【#54 C6/C7】「跳转定义目标解析」端口（host 装配编辑器内核时读一次，
    /// 有则挂 `lsp_mut().show_document`，无则内核默认）
    pub fn show_document_port(&self) -> Option<ShowDocumentPort> {
        self.show_document_port.borrow().clone()
    }
}
