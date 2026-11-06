//! 状态位：提示、加载与撤销归档的到期计时。

use super::*;

impl ResourcesPanel {
    pub fn set_notice(&mut self, notice: Option<String>, cx: &mut Context<Self>) {
        self.notice = notice;
        cx.notify();
    }
    /// 置"正在取数"（**事件路径调用**：入队时置真、快照到达/失败时置假）。
    pub fn set_loading(&mut self, loading: bool, cx: &mut Context<Self>) {
        if self.loading == loading {
            return;
        }
        self.loading = loading;
        cx.notify();
    }
    /// 推进（或清掉）撤销凭据（**事件路径调用**：宿主归档成功后 / 撤销完成后）。
    ///
    /// 带凭据时同时安排 5 秒后自动消失（与 M5 撤销栏同一时长）：撤销窗口是"立即反悔"，
    /// 不是一条待办事项；过期就收，免得一个小时后点下去才发现已失效。
    pub fn set_undo(&mut self, undo: Option<ArchiveUndo>, cx: &mut Context<Self>) {
        let schedule = undo.clone();
        self.undo = undo;
        cx.notify();
        if let Some(token) = schedule {
            self.schedule_undo_expiry(token, cx);
        }
    }
    /// 安排撤销凭据 5 秒后自动消失（**仅当仍指向同一次归档**）。
    ///
    /// 守卫不能省：这 5 秒里可能又归档了一次，新凭据不该被旧计时器清掉（M5 同例）。
    pub(super) fn schedule_undo_expiry(&self, token: ArchiveUndo, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            executor.timer(std::time::Duration::from_secs(5)).await;
            let _ = weak.update(cx, |this, cx| {
                if this.undo.as_ref() == Some(&token) {
                    this.undo = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
