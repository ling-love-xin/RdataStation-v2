//! 跨模块测试辅助（仅测试构建）。
//!
//! 面板的契约是「只发事件、不自己取数」，所以多个测试模块都需要同一件事：
//! **接一个记录型宿主**，把面板发出的 [`InsightEvent`] 收下来断言。
//! 放这里而不是各测试模块各写一遍——订阅必须被持有这类细节只该错一次。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{AppContext as _, Context, Entity, Subscription, VisualTestContext};

use crate::insight_view::{InsightEvent, InsightView};

/// 宿主替身：真实宿主只多持一个订阅句柄与一个取数接缝
pub(crate) struct TestHost;

impl TestHost {
    pub(crate) fn new(_: &mut Context<Self>) -> Self {
        Self
    }
}

/// 事件的记录器。
///
/// 它同时持有宿主实体与订阅：**只留 `Subscription` 不够**——订阅者实体一旦释放，
/// 订阅就会被剪掉（本仓踩过的坑，表现是「事件一个都没收到」）。
pub(crate) struct EventSink {
    events: Rc<RefCell<Vec<InsightEvent>>>,
    _host: Entity<TestHost>,
    _sub: Subscription,
}

impl std::ops::Deref for EventSink {
    type Target = RefCell<Vec<InsightEvent>>;

    fn deref(&self) -> &Self::Target {
        &self.events
    }
}

impl EventSink {
    /// 取走已记录的事件（断言前清空，避免与前一阶段的请求混在一起）
    pub(crate) fn take(&self) -> Vec<InsightEvent> {
        std::mem::take(&mut self.events.borrow_mut())
    }
}

/// 给面板接一个记录型宿主。返回值**必须由调用方持有到测试结束**。
pub(crate) fn event_sink(view: &Entity<InsightView>, cx: &mut VisualTestContext) -> EventSink {
    let events: Rc<RefCell<Vec<InsightEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    let (host, sub) = cx.update(|_window, cx| {
        let host = cx.new(TestHost::new);
        let sub = host.update(cx, |_host, host_cx| {
            host_cx.subscribe(view, move |_this, _emitter, event: &InsightEvent, _cx| {
                sink.borrow_mut().push(event.clone());
            })
        });
        (host, sub)
    });
    EventSink {
        events,
        _host: host,
        _sub: sub,
    }
}
