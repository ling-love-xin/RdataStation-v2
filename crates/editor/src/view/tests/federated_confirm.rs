//! 联邦确认（09 原型「执行联邦前的确认」落地）
//!
//! headless 下鼠标点击到不了对话框层（本仓既有口径），所以这里的流程是：
//! 执行 → 断言「没入队、挂起成立」→ 直调确认（与按钮回调同一函数）→ 断言「入队了」。

use std::rc::Rc;

use gpui_kit::TestAppContext;

use crate::channel::{ChannelAvailability, ChannelAvailabilitySet, ChannelsPort, ExecChannel};
use crate::model::EditorMode;
use crate::sources::{SourceRow, SourceState, SourcesPort, SourcesSnapshot};
use crate::view::host::EditorHostPanel;

use super::{open_panel, shared_with_channel_recorder};

struct OpenChannels;

impl ChannelsPort for OpenChannels {
    fn availability(&self, _conn_id: Option<&str>) -> ChannelAvailabilitySet {
        ChannelAvailabilitySet {
            accelerated: ChannelAvailability::ok(),
            federated: ChannelAvailability::ok(),
        }
    }
}

/// 两个可用源：确认框该报「将跨 2 个源全部扫描」
struct TwoReadySources;

impl SourcesPort for TwoReadySources {
    fn snapshot(&self, _conn_id: &str) -> Option<SourcesSnapshot> {
        Some(SourcesSnapshot {
            sources: vec![
                SourceRow {
                    alias: "mysql_src".to_string(),
                    kind_label: "MySQL".to_string(),
                    state: SourceState::Ready { tables: 12 },
                    primary: true,
                },
                SourceRow {
                    alias: "pg_warehouse".to_string(),
                    kind_label: "PostgreSQL".to_string(),
                    state: SourceState::Ready { tables: 5 },
                    primary: false,
                },
            ],
            primary: Some("mysql_src".to_string()),
            note: None,
        })
    }
}

/// 联邦执行先弹确认：不点「执行」就不入队；取消只清挂起。
#[gpui_kit::test]
fn federated_runs_only_after_the_confirm(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen) = shared_with_channel_recorder("select * from a join b;");
    shared.attach_channels(Rc::new(OpenChannels));
    shared.attach_sources(Rc::new(TwoReadySources));
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_channel(ExecChannel::Federated, cx));
    });

    // 执行 → 被确认框拦住：挂起成立，队列里一条都没有
    let submitted = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute_preferring_selection(cx);
            panel.pending_federated_for_test()
        })
    });
    assert!(submitted, "联邦档执行前必须挂起等确认");
    assert!(
        seen.lock().expect("通道锁").is_empty(),
        "确认前不该有任何作业入队"
    );

    // 取消：只清挂起，什么都不跑
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.cancel_federated(cx));
    });
    assert!(
        !cx.update(|_window, cx| panel.read(cx).pending_federated_for_test()),
        "取消后不该还挂着"
    );
    assert!(seen.lock().expect("通道锁").is_empty(), "取消后仍然不入队");

    // 再来一次并确认 → 这次真的入队了，且通道是联邦
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.execute_preferring_selection(cx));
    });
    let finished = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.finish_federated(cx))
    });
    assert!(finished, "确认后应当提交成功");
    // 作业走后台队列（`ScriptRunner` 在另一条线程上跑），所以这里只断言**提交侧**的事实：
    // 在途计数立即可见、通道确实是联邦。等结果落地再断言会让用例依赖线程时序。
    let (pending, channel) = cx.update(|_window, cx| {
        let panel = panel.read(cx);
        (panel.pending, panel.channel())
    });
    assert!(pending > 0, "确认后应有在途作业，实际 {pending}");
    assert_eq!(channel, ExecChannel::Federated, "确认后仍应是联邦档");
}

/// 非联邦档不受影响（确认只属于联邦）
#[gpui_kit::test]
fn local_channel_still_runs_without_confirm(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, _seen) = shared_with_channel_recorder("select 1;");
    shared.attach_channels(Rc::new(OpenChannels));
    shared.attach_sources(Rc::new(TwoReadySources));
    let (panel, cx) = open_panel(cx, &shared, &id);

    let pending = cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute_preferring_selection(cx);
            panel.pending_federated_for_test()
        })
    });
    assert!(!pending, "非联邦档不该弹联邦确认");
    let pending_count = cx.update(|_window, cx| panel.read(cx).pending);
    assert!(pending_count > 0, "非联邦档应当直接入队，实际 {pending_count}");
}

#[allow(dead_code)]
fn _assert_types(_: EditorMode, _: &EditorHostPanel) {}
