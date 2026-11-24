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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
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
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
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
    v.map(|x| format!("{x:.1}"))
        .unwrap_or_else(|| "—".to_string())
}

/// 左缘：走马灯位移就是靠它随时间往左走。
fn left(cx: &mut VisualTestContext, selector: &'static str) -> Option<f32> {
    cx.debug_bounds(selector).map(|b| b.origin.x.into())
}

/// 高度：工具栏 / 状态栏的档位就是靠它跟兄弟面板对。
fn hgt(cx: &mut VisualTestContext, selector: &'static str) -> Option<f32> {
    cx.debug_bounds(selector).map(|b| b.size.height.into())
}

/// 上缘：判「工具栏是不是面板第一行」。
fn top(cx: &mut VisualTestContext, selector: &'static str) -> Option<f32> {
    cx.debug_bounds(selector).map(|b| b.origin.y.into())
}

/// 下缘：判「状态栏是不是面板最后一行」。
fn bottom(cx: &mut VisualTestContext, selector: &'static str) -> Option<f32> {
    cx.debug_bounds(selector)
        .map(|b| f32::from(b.origin.y) + f32::from(b.size.height))
}

/// 一条探针行的量值（`line` 是打给人看的一行字）。
struct Probe {
    line: String,
    row: Option<f32>,
    name: Option<f32>,
    pill: Option<f32>,
    channel: Option<f32>,
    state_right: Option<f32>,
}

/// 面板宽度 → 行内各格真实宽度。
fn panel_at(cx: &mut TestAppContext, width: f32, objects: Vec<Object>, key: &'static str) -> Probe {
    let (_panel, vcx) = host_window(cx, width, objects);
    probe_in(vcx, width, key)
}

/// 建面板 + 指定宽容器（= 把右侧面板拖到这么窄），返回面板实体与可交互的测试窗口。
///
/// `new` 会自己 `refresh`（真去查库，headless 下拿到空清单）。这里按生产口径接手：
/// 上游取好的清单写进 `Shared`，再把面板那份可见行缓存作废——**不**动 epoch，
/// 否则 render 里排的 `defer_in` 会再跑一次 refresh 把清单又覆盖成空。
fn host_window<'a>(
    cx: &'a mut TestAppContext,
    width: f32,
    objects: Vec<Object>,
) -> (Entity<DataObjectsTab>, &'a mut VisualTestContext) {
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
    *shared.data_objects.borrow_mut() = objects;
    vcx.update(|_, app| {
        panel.update(app, |tab, _| tab.rows_cache = None);
    });
    settle(&mut vcx);
    (panel, vcx)
}

/// 已建好的窗口里量一条探针行。
fn probe_in(vcx: &mut VisualTestContext, width: f32, key: &'static str) -> Probe {
    // (行, 名字, 副标题, 小标, 通道, 状态) 六个锚点
    let sels: (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    ) = match key {
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

    let row_w = w(vcx, sels.0);
    let name_w = w(vcx, sels.1);
    let pill_w = w(vcx, sels.3);
    let channel_w = w(vcx, sels.4);
    let tier = crate::components::data_objects::layout::RowTier::for_width(width);
    let line = format!(
        "PANEL={width:>5.0}  tier={tier:?}  row={}  name={}  note={}  pill={}  channel={}  \
         state={}  stateRight={}",
        fmt(row_w),
        fmt(name_w),
        fmt(w(vcx, sels.2)),
        fmt(pill_w),
        fmt(channel_w),
        fmt(w(vcx, sels.5)),
        fmt(right(vcx, sels.5)),
    );
    Probe {
        line,
        row: row_w,
        name: name_w,
        pill: pill_w,
        channel: channel_w,
        state_right: right(vcx, sels.5),
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
        fmt(w(vcx, "natural-name")),
        fmt(w(vcx, "natural-note")),
        fmt(w(vcx, "natural-state")),
    );

    // 2) 正常行（结果集 · 可用）在各档面板宽度下的真实分布。
    //    这里钉的就是「行宽预算」本身：名字宽必须等于 `RowTier::name_space(面板宽)`，
    //    以及该档该出现的列必须真出现 / 真消失——预算表与真排版不许各说各话。
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
        let tier = crate::components::data_objects::layout::RowTier::for_width(width);
        // 行宽恒等于可用宽：这些列都是定宽/可收缩，没有谁把行撑长
        assert_eq!(
            probe.row.map(|w| w.round()),
            Some(width),
            "行宽应等于面板宽（{width}）"
        );
        // 名字吃到的宽度 = 预算算出来的宽度（预算是**上限**：按最宽的小标估，
        // 真实行可能比它少十来像素，但绝不该比它更宽——更宽说明布局里偷偷省了东西）
        let budget = tier.name_space(width);
        let actual = probe.name.expect("名字格应有宽度");
        assert!(
            actual <= budget + 2.0,
            "面板 {width} 宽 / {tier:?} 档：名字实测 {actual:.1} 比预算 {budget:.1} 还宽——\
             预算漏算了某一列的宽度"
        );
        assert!(
            budget - actual <= crate::components::data_objects::layout::PILL_COL_PX,
            "面板 {width} 宽 / {tier:?} 档：名字实测 {actual:.1} 比预算 {budget:.1} 窄太多——\
             预算把腾出来的宽度算多了（一个最宽小标 = {}px）",
            crate::components::data_objects::layout::PILL_COL_PX
        );
        assert_eq!(
            probe.channel.is_some(),
            tier.shows_channel(),
            "面板 {width} 宽 / {tier:?} 档：执行通道列的去留与预算不一致"
        );
        assert_eq!(
            probe.pill.is_some(),
            tier.shows_pill(),
            "面板 {width} 宽 / {tier:?} 档：小标列的去留与预算不一致"
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

    // 4) 常量校准：`ROW_CHROME_PX` 是实测拟合出来的，布局一改它就会漂——这里钉死。
    let wide = panel_at(
        cx,
        680.0,
        vec![row(
            "result:probe",
            long_name(),
            "源表副本 · 载入内存",
            State::Ready,
            "载入",
        )],
        "result:probe",
    );
    let (row, name) = (
        wide.row.expect("行应有宽度"),
        wide.name.expect("名字格应有宽度"),
    );
    let chrome = row - name;
    assert!(
        (chrome - crate::components::data_objects::layout::ROW_CHROME_PX).abs() <= 2.0,
        "固定开销实测 {chrome:.1}px，预算常量 {}px——常量该按这次实测更新（名字可用宽 = 面板宽 − 这个数）",
        crate::components::data_objects::layout::ROW_CHROME_PX
    );
}

/// 走马灯：**只有名字真的放不下、且鼠标停住这一行时才滚**。
///
/// 三件事必须都在真窗口里成立：
/// 1. 放不下的行 hover 后改画走马灯（文字按自然宽排，内层比格子宽出截断的那截）；
/// 2. 时间推进后内层真的在往左走（不是画了个静止的宽元素）；
/// 3. 放得下的行 hover 也不滚（短名字不该无缘无故动起来）。
#[gpui_kit::test]
fn marquee_runs_only_on_the_hovered_row_that_does_not_fit(cx: &mut TestAppContext) {
    use std::time::Duration;

    use gpui_kit::Modifiers;

    cx.update(gpui_kit::init);
    let objects = vec![row(
        "result:probe",
        long_name(),
        "源表副本 · 载入内存",
        State::Ready,
        "载入",
    )];

    // 1) 窄面板（260）：名字 210px 塞进 111px 的格子
    let (_panel, mut vcx) = host_window(cx, 260.0, objects.clone());
    let cell = w(vcx, "data-object-name-result:probe").expect("名字格应有宽度");
    let row_box = vcx
        .debug_bounds("data-object-row-result:probe")
        .expect("行应有坐标");
    vcx.simulate_mouse_move(row_box.center(), None, Modifiers::default());
    settle(&mut vcx);

    let inner =
        w(vcx, "data-object-marquee-result:probe").expect("名字放不下时 hover 应改画走马灯");
    let x0 = left(vcx, "data-object-marquee-result:probe").expect("走马灯应有坐标");
    println!("MARQUEE cell={cell:.1}  inner={inner:.1}  x0={x0:.1}");
    assert!(
        inner > cell + 40.0,
        "走马灯里的文字必须按自然宽排（格子 {cell:.1}，内层 {inner:.1}）——\
         内层若等于格子宽，说明它被压扁了，滚起来只是把截断的字挪来挪去"
    );

    // 时间推进：4s 一轮、两端各停 18%，1200ms 时正在中段滚动
    vcx.executor().advance_clock(Duration::from_millis(1200));
    settle(vcx);
    let x1 = left(vcx, "data-object-marquee-result:probe").expect("走马灯应有坐标");
    println!("MARQUEE at 1.2s  x={x1:.1}");
    assert!(
        x1 < x0 - 1.0,
        "1.2s 后文字应已向左移动：x0={x0:.1} → x1={x1:.1}"
    );
    // 位移不许超过「自然宽 − 格子宽」，否则结尾会被推出可视区（等于没滚完）
    let travel = inner - cell;
    assert!(
        (x0 - x1) <= travel + 1.0,
        "位移 {:.1}px 超过了可滚余量 {travel:.1}px",
        x0 - x1
    );

    // 2) 宽面板（680）：名字放得下，hover 也不许滚
    let (_panel, mut vcx) = host_window(cx, 680.0, objects);
    let wide_cell = w(vcx, "data-object-name-result:probe").expect("名字格应有宽度");
    let row_box = vcx
        .debug_bounds("data-object-row-result:probe")
        .expect("行应有坐标");
    vcx.simulate_mouse_move(row_box.center(), None, Modifiers::default());
    settle(&mut vcx);
    assert!(
        w(vcx, "data-object-marquee-result:probe").is_none(),
        "名字放得下（{wide_cell:.1}px 格子装 210px 的字）时不该出现走马灯"
    );
}

/// 面板骨架：工具栏在上、状态栏在下，档位与兄弟面板（资产库 / 数据库导航）一致。
///
/// 用户口径（2026-11-25）：面板头部那行「16 个 · 已选 0 · 进程 DuckDB」是**状态**，
/// 该落到下方状态栏；动作（加源 / 存快照）该在面板自己的工具栏里，不该上 Dock 标题栏。
/// 这里钉三件事：工具栏是面板第一行且 32px、状态栏是面板最后一行且 24px、
/// 两个动作按钮确实长在工具栏里（不在标题栏）。
#[gpui_kit::test]
fn toolbar_and_status_bar_sit_where_the_sibling_panels_put_them(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let objects = vec![row(
        "result:probe",
        "商城库",
        "MySQL · db.example.com",
        State::Ready,
        "载入",
    )];
    let (_panel, vcx) = host_window(cx, 520.0, objects);

    // 兄弟面板的档位（`workbench_shell::ui`）：工具栏 = CONTROL_HEIGHT_MD（2rem = 32px）、
    // 状态栏 = ROW_HEIGHT（1.5rem = 24px）
    let toolbar_h = hgt(vcx, "data-objects-toolbar").expect("工具栏应有坐标");
    let status_h = hgt(vcx, "data-objects-status-bar").expect("状态栏应有坐标");
    let toolbar_top = top(vcx, "data-objects-toolbar").expect("工具栏应有坐标");
    let status_bottom = bottom(vcx, "data-objects-status-bar").expect("状态栏应有坐标");
    println!(
        "CHROME  toolbar_h={toolbar_h:.1}  status_h={status_h:.1}  toolbar_top={toolbar_top:.1}  \
         status_bottom={status_bottom:.1}"
    );

    let rem = 16.0f32;
    assert!(
        (toolbar_h - workbench_shell::ui::CONTROL_HEIGHT_MD * rem).abs() <= 1.0,
        "工具栏高 {toolbar_h:.1}px，兄弟面板是 {}rem —— 同一排面板的工具栏必须同高",
        workbench_shell::ui::CONTROL_HEIGHT_MD
    );
    assert!(
        (status_h - workbench_shell::ui::ROW_HEIGHT * rem).abs() <= 1.0,
        "状态栏高 {status_h:.1}px，兄弟面板是 {}rem",
        workbench_shell::ui::ROW_HEIGHT
    );
    // 工具栏是面板第一行（面板有 `pt_2` = 8px 的顶部间隔）
    assert!(
        toolbar_top <= 8.5,
        "工具栏上缘 {toolbar_top:.1}px —— 它必须是面板的第一行（面板顶部只留 8px 间隔）"
    );
    // 状态栏是面板最后一行：面板高 600（`Host` 的 `rems(37.5)`），下缘要贴着底
    assert!(
        (status_bottom - 600.0).abs() <= 1.0,
        "状态栏下缘 {status_bottom:.1}px，面板底是 600px —— 状态栏必须贴在面板底部"
    );
}
