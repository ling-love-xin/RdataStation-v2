//! 数据对象面板：**真实布局测量**（headless 窗口 + gpui 排版引擎）。
//!
//! 不是估算：把面板放进指定宽度的容器里真排一遍，再按 `debug_selector` 记录
//! 读出每一格的真实宽度（`debug_bounds` 只在 debug 构建里登记，发布物无副作用）。
//!
//! 为什么需要这组数：面板窄到多少时「名字」被挤成 0、行尾按钮会不会被顶出可视区、
//! 走马灯要滚多远——这些只能由排版引擎回答。数出来之后再定丢列优先级。
//!
//! 注意：**不通配导入**（`use gpui_kit::*` 会把 gpui 的 `test` 宏带进来，与
//! `#[gpui_kit::test]` 冲突）；依赖全部显式列举，与 `quick_open/tests` 同款。

use gpui_kit::base::StyledExt as _;
use gpui_kit::{
    div, px, rems, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window,
};

use crate::components::data_objects::{Channel, Group, Life, Object, State};
use crate::panels::Shared;

use super::DataObjectsTab;

fn long_name() -> &'static str {
    // 24 个字：真实表名常见长度（「客户订单明细表_华东区域_2026年度第二阶段对账」）
    "客户订单明细表_华东区域_2026年度第二阶段对账"
}

const LONG_STATE: &str = "未挂载 · 先在这条连接上开「本地加速」，挂上后可展开选表";

fn row(key: &str, name: &str, note: &str, state: State, pill: &'static str) -> Object {
    Object {
        key: key.to_string(),
        name: name.to_string(),
        note: note.to_string(),
        channel: Channel::Process,
        life: Life::Session,
        state,
        group: Group::Table,
        pill,
        version: None,
        sql_ref: Some(format!("\"{name}\"")),
    }
}

/// 文字**不被约束**时的宽度（= 字形全宽）。放在一条 `flex_none` 横排里量。
struct Natural {
    name: &'static str,
    note: &'static str,
    state: &'static str,
}

impl Render for Natural {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = self.name;
        let note = self.note;
        let state = self.state;
        div()
            .h_flex()
            .gap_3()
            .child(
                div()
                    .debug_selector(|| "natural-name".to_string())
                    .flex_none()
                    .text_sm()
                    .child(name.to_string()),
            )
            .child(
                div()
                    .debug_selector(|| "natural-note".to_string())
                    .flex_none()
                    .text_xs()
                    .child(note.to_string()),
            )
            .child(
                div()
                    .debug_selector(|| "natural-state".to_string())
                    .flex_none()
                    .text_xs()
                    .child(state.to_string()),
            )
    }
}

/// 面板 + 指定宽度容器（= 把右侧面板拖到这么窄）。
struct Host {
    panel: Entity<DataObjectsTab>,
    width: f32,
}

impl Render for Host {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // 高度只要够放几行：不是被测项，取窗口默认高度量级（37.5rem = 600px @16px）
        div()
            .v_flex()
            .w(px(self.width))
            .h(rems(37.5))
            .child(self.panel.clone())
    }
}

fn settle(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
    // 0.10.0 起未失效子树在 draw 时被原样重放：改状态后要 refresh 再画
    cx.update(|window, _| window.refresh());
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
    });
}

fn w(cx: &mut VisualTestContext, selector: &'static str) -> Option<f32> {
    cx.debug_bounds(selector).map(|b| b.size.width.into())
}

/// 右边缘（`左边距 + 宽`）：跟行宽比一下就知道这一格有没有被顶出去。
fn right(cx: &mut VisualTestContext, selector: &'static str) -> Option<f32> {
    cx.debug_bounds(selector)
        .map(|b| f32::from(b.origin.x) + f32::from(b.size.width))
}

fn fmt(v: Option<f32>) -> String {
    v.map(|x| format!("{x:.1}")).unwrap_or_else(|| "—".to_string())
}

/// 一条探针行的量值（`line` 是打给人看的一行字）。
struct Probe {
    line: String,
    row: Option<f32>,
    state_right: Option<f32>,
}

/// 面板宽度 → 行内各格真实宽度。
fn panel_at(cx: &mut TestAppContext, width: f32, objects: Vec<Object>, key: &'static str) -> Probe {
    use std::cell::RefCell;
    use std::rc::Rc;

    let shared = Shared::new();
    let slot: Rc<RefCell<Option<Entity<DataObjectsTab>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, mut vcx) = cx.add_window_view({
        let shared = shared.clone();
        move |_window, cx| {
            let panel = cx.new(|cx| DataObjectsTab::new(shared, cx));
            *slot_in.borrow_mut() = Some(panel.clone());
            Host { panel, width }
        }
    });
    let panel = slot.borrow().clone().expect("面板已建");
    // `new` 会自己 `refresh`（真去查库，headless 下拿到空清单）。这里按生产口径接手：
    // 上游取好的清单写进 `Shared`，再把面板那份可见行缓存作废——**不**动 epoch，
    // 否则 render 里排的 `defer_in` 会再跑一次 refresh 把清单又覆盖成空。
    *shared.data_objects.borrow_mut() = objects;
    vcx.update(|_, app| {
        panel.update(app, |tab, _| tab.rows_cache = None);
    });
    settle(&mut vcx);

    // (行, 名字, 副标题, 小标, 通道, 状态) 六个锚点
    let sels: (&'static str, &'static str, &'static str, &'static str, &'static str, &'static str) =
        match key {
            "result:probe" => (
                "data-object-row-result:probe",
                "data-object-name-result:probe",
                "data-object-note-result:probe",
                "data-object-pill-result:probe",
                "data-object-channel-result:probe",
                "data-object-state-result:probe",
            ),
            _ => (
                "data-object-row-conn:probe",
                "data-object-name-conn:probe",
                "data-object-note-conn:probe",
                "data-object-pill-conn:probe",
                "data-object-channel-conn:probe",
                "data-object-state-conn:probe",
            ),
        };

    let row_w = w(&mut vcx, sels.0);
    let line = format!(
        "PANEL={width:>5.0}  row={}  name={}  note={}  pill={}  channel={}  state={}  \
         stateRight={}",
        fmt(row_w),
        fmt(w(&mut vcx, sels.1)),
        fmt(w(&mut vcx, sels.2)),
        fmt(w(&mut vcx, sels.3)),
        fmt(w(&mut vcx, sels.4)),
        fmt(w(&mut vcx, sels.5)),
        fmt(right(&mut vcx, sels.5)),
    );
    Probe {
        line,
        row: row_w,
        state_right: right(&mut vcx, sels.5),
    }
}

#[gpui_kit::test]
fn measure_row_layout(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    // 1) 文字自然宽度（字形全宽，不受面板宽度影响）
    let (_, mut vcx) = cx.add_window_view(|_window, _cx| Natural {
        name: long_name(),
        note: "源表副本 · 载入内存",
        state: LONG_STATE,
    });
    settle(&mut vcx);
    println!(
        "NATURAL  name(14px)={}  note(12px)={}  state(12px)={}",
        fmt(w(&mut vcx, "natural-name")),
        fmt(w(&mut vcx, "natural-note")),
        fmt(w(&mut vcx, "natural-state")),
    );

    // 2) 正常行（结果集 · 可用）在各档面板宽度下的真实分布
    for width in [260.0f32, 300.0, 340.0, 380.0, 440.0, 520.0, 680.0] {
        let probe = panel_at(
            cx,
            width,
            vec![row(
                "result:probe",
                long_name(),
                "源表副本 · 载入内存",
                State::Ready,
                "载入",
            )],
            "result:probe",
        );
        println!("{}", probe.line);
        // 行宽恒等于可用宽：这些列都是定宽/可收缩，没有谁把行撑长
        assert_eq!(
            probe.row.map(|w| w.round()),
            Some(width),
            "行宽应等于面板宽（{width}）"
        );
    }

    // 3) 最宽的真状态（未挂载整句）：行上已被短码收口，**不许再顶出行外**
    for width in [300.0f32, 380.0, 520.0] {
        let probe = panel_at(
            cx,
            width,
            vec![row(
                "conn:probe",
                "商城库",
                "MySQL · db.example.com",
                State::Failed(LONG_STATE.to_string()),
                "源连接",
            )],
            "conn:probe",
        );
        println!("LONGSTATE {}", probe.line);
        let (row, right) = (
            probe.row.expect("行应有宽度"),
            probe.state_right.expect("状态格应有坐标"),
        );
        assert!(
            right <= row + 0.5,
            "面板 {width} 宽时状态块右缘 {right:.1} 顶出了行（行宽 {row:.1}）——\
             整句又被放回行上了"
        );
    }
}
