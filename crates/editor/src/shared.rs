//! 编辑器共享状态（各面板共用同一份文档集合）
//!
//! 唯一权威是 `EditorService`（A1）：这里只提供**可克隆的跨面板句柄**，让"一个面板 = 一个
//! 标签 = 一份文档"的多面板结构共享同一份文档状态（标签条、状态栏、关闭逻辑都读它）。
//!
//! 约定：**视图只读**（`service()`），**变更走事件路径**（`update()`）——与全仓"render 是纯读路径"
//! 一致，避免渲染期改状态。

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use crate::service::{EditorService, OpenOutcome, OpenRequest};

/// 共享句柄（`Clone` 即克隆一个 `Rc`，各面板指向同一份状态）
#[derive(Clone)]
pub struct EditorShared(Rc<RefCell<EditorService>>);

impl Default for EditorShared {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorShared {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(EditorService::new())))
    }

    /// 只读访问（渲染路径）
    pub fn service(&self) -> Ref<'_, EditorService> {
        self.0.borrow()
    }

    /// 变更访问（事件路径）：返回闭包结果，借用边界内完成
    pub fn update<R>(&self, change: impl FnOnce(&mut EditorService) -> R) -> R {
        change(&mut self.0.borrow_mut())
    }

    /// 便捷：打开文档（同路径重复打开 = 激活）
    pub fn open(&self, request: OpenRequest) -> OpenOutcome {
        self.update(|service| service.open(request))
    }
}
