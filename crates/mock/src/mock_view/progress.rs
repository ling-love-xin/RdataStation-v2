//! 进度条原语：三处共用（中央表头 / 右 Dock 清单行 / 状态栏）。
//!
//! # 为什么在组件库的 `Progress` 之外还要自己包一层
//!
//! 组件库已经给全了两件事，本模块**不重复实现**：
//!
//! - **宽度过渡**：换值不跳，走 `transition` 平滑到新百分比；
//! - **不定量滑动**：`Progress::loading(true)` 自带来回滑动的动画。
//!
//! 缺的是第三件——**定量阶段也要看得出「还在跑」**：大表生成时百分比几秒才动一格，
//! 一条静止的填充条与卡死没有区别。所以这里在 `Progress` 之上叠一条**扫光**
//! （横向往返的亮条）：填充段走到哪它扫到哪，读数归读数，活性归活性。
//! 不定量态**不叠**扫光：那时组件库自己就在滑动，两个动画叠在一起只会互相打架。
//!
//! # 清单行为什么只画「下缘 2px 线」
//!
//! 清单行只有 ~24px 高，且同时要承载选中底（`list_active`）、悬停底、左缘标识条。
//! 整行铺色块（用户提过的另一个方案）会与这三样叠色，并压低文字对比度；
//! 把进度压到行底 2px：不占行内布局、不与任何底色冲突、长表名被截断也不影响读数。
//! 集合任务（场景 N 张表 / 批量落库）逐表推进是**这张表好了没有**，
//! 用「满线 = 已过 / 扫光 = 正在过 / 不画 = 还没轮到」三态表达，比挤一个百分比更准。
//!
//! # ETA
//!
//! 剩余时间 = 平均每单位耗时 × 剩余单位数。每单位耗时用**累计均值 + 指数平滑**：
//! 第一个单位含建临时表的开销（偏大），逐次收敛；只看瞬时速率会让 ETA 每 120ms 跳一次。
//! 不足 1 秒不报（秒级以下的估计只有抖动，没有信息量）。

// 继承父模块（mock_view）的全部 use 绑定
use super::*;

use std::time::Duration;

// 组件尺寸档（与 `ui::PREVIEW_TABLE_SIZE` 同一套枚举：`Progress` 走 `Sizable`）
use gpui_kit::component::Size as ComponentSize;

/// 扫光往返一个周期的时长（1.6s：慢到能看清方向，快到不抢注意力）。
pub const SHEEN_PERIOD_MS: u64 = 1600;
/// 扫光条宽度（占容器宽度的比例；太窄看着像噪点，太宽就成整条在变色了）
const SHEEN_WIDTH: f32 = 0.28;
/// 扫光亮度（叠在填充色上的白色）
const SHEEN_ALPHA: f32 = 0.34;
/// 清单行扫光（「正在处理这张表」）的宽度比例：比进度条上的扫光宽一点，
/// 因为它身后没有填充段衬托，窄了会看成行底的一个小污点
const ROW_SWEEP_WIDTH: f32 = 0.34;

/// ETA 的指数平滑系数（旧样本权重；新样本 = 1 - 它）
pub const ETA_SMOOTHING: f64 = 0.7;
/// 低于这个剩余量就不报 ETA（毫秒）
pub const ETA_MIN_MS: f64 = 1_000.0;

/// 进度条的档位（高度与组件尺寸一一对应，避免各处写裸 px）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobBarSize {
    /// 清单行下缘线（4px 组件裁到 2px：只留一条线，不挤行内布局）
    Row,
    /// 状态栏迷你条（4px）
    Mini,
    /// 中央表头（6px）
    Head,
}

/// 进度条（容器）：组件库 `Progress` + 扫光。
///
/// `key` 必须是**这个位置的稳定标识**（不是随机数）：扫光的动画状态按 id 记，
/// id 每帧变会让动画每帧从头开始。
pub fn job_progress_bar(
    key: &str,
    percent: f32,
    indeterminate: bool,
    size: JobBarSize,
    color: Option<Hsla>,
    cx: &App,
) -> Div {
    let (bar_size, height) = match size {
        JobBarSize::Row => (ComponentSize::XSmall, px(2.)),
        JobBarSize::Mini => (ComponentSize::XSmall, px(4.)),
        JobBarSize::Head => (ComponentSize::Small, px(6.)),
    };
    let mut bar = Progress::new(ElementId::Name(SharedString::from(format!(
        "mock-bar-{key}"
    ))))
    // `Sizable` 的档位（`Styled::size` 同名但收长度：写 `.size()` 会撞到那边去）
    .with_size(bar_size)
    .w_full()
    .loading(indeterminate)
    .value(percent);
    if let Some(color) = color {
        bar = bar.color(color);
    }
    let mut root = div()
        .relative()
        .w_full()
        .h(height)
        .overflow_hidden()
        .child(bar);
    // 不定量态交给组件库自带的滑动动画；定量态叠扫光，否则百分比不动的那几秒里
    // 一条静止的填充条与卡死没有区别。`reduce_motion` 与组件库同口径：系统让少动就不动
    if !indeterminate && !cx.reduce_motion() {
        root = root.child(sheen(key, white().opacity(SHEEN_ALPHA), SHEEN_WIDTH, true));
    }
    root
}

/// 横向往返扫过的光条（定量条的活性提示；也单独用作清单行的「正在处理这张表」）。
///
/// 往返而不是单向：单向扫到头会瞬移回起点，看着像卡了一下。
/// `animated = false`（系统要求减少动效）时停在左侧三分之一处：那时的语义靠
/// 「没有填满」表达，不靠动。
fn sheen(key: &str, color: Hsla, width: f32, animated: bool) -> AnyElement {
    let span = (1.0 - width).max(0.0);
    let still = || {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(relative(width))
            .bg(color)
    };
    if animated {
        still()
            .with_animation(
                SharedString::from(format!("mock-bar-sheen-{key}")),
                Animation::new(Duration::from_millis(SHEEN_PERIOD_MS)).repeat(),
                move |this, delta| {
                    // 三角波：0→1→0，光条走到头再走回来（走到头瞬移回起点会看着像卡了一下）
                    let t = if delta < 0.5 {
                        delta * 2.0
                    } else {
                        (1.0 - delta) * 2.0
                    };
                    this.left(relative(t * span))
                },
            )
            .into_any_element()
    } else {
        still().left(relative(0.33)).into_any_element()
    }
}

/// 清单行下缘的进度线（三态容器的内容）。
///
/// 返回的是**内容**（不含定位）：定位由清单行加（`absolute` + `bottom_0`），
/// 这样三种形态共用同一套摆放，不会有一态忘了贴底。
pub fn row_progress_line(key: &str, state: RowProgress, color: Hsla, cx: &App) -> Div {
    match state {
        // 定量：组件库那根条（宽度过渡 + 扫光）裁到 2px 高
        RowProgress::Determinate(percent) => {
            job_progress_bar(key, f32::from(percent), false, JobBarSize::Row, None, cx)
        }
        // 不定量 / 集合任务里正在过的那张表：一条扫光表示「在动，但这一行没有百分比可报」
        RowProgress::Active => div()
            .relative()
            .w_full()
            .h(ui::ROW_PROGRESS_LINE_HEIGHT)
            .overflow_hidden()
            .child(sheen(key, color, ROW_SWEEP_WIDTH, !cx.reduce_motion())),
        // 已经过完了：满线（不再动）
        RowProgress::Done => div().w_full().h(ui::ROW_PROGRESS_LINE_HEIGHT).bg(color),
    }
}

/// ETA 文案（中央表头 / 清单用）：`None` = 不报（不足 1 秒）。
///
/// 分档而不是一律转秒：300 秒读成「~5 分 0 秒」比「~300 秒」少一次心算。
pub fn eta_text(eta: Duration) -> Option<String> {
    let secs = eta.as_secs();
    if secs == 0 {
        return None;
    }
    if secs < 60 {
        return Some(format!("剩余 ~{secs} 秒"));
    }
    if secs < 3600 {
        return Some(format!("剩余 ~{} 分 {:02} 秒", secs / 60, secs % 60));
    }
    Some(format!(
        "剩余 ~{} 时 {:02} 分",
        secs / 3600,
        secs % 3600 / 60
    ))
}

/// ETA 文案的紧凑版（状态栏：那一行还挤着连接名 / LSP / 三个开关）。
pub fn eta_text_compact(eta: Duration) -> Option<String> {
    let secs = eta.as_secs();
    if secs == 0 {
        return None;
    }
    if secs < 60 {
        return Some(format!("~{secs}s"));
    }
    if secs < 3600 {
        return Some(format!("~{}m{:02}s", secs / 60, secs % 60));
    }
    Some(format!("~{}h{:02}m", secs / 3600, secs % 3600 / 60))
}

/// 每单位耗时的下一个平滑值（EMA）。
///
/// 直接用「瞬时速率」会让 ETA 随每批抖动来回跳；累计均值又会一直背着第一个单位
/// （建临时表那一下）的开销。取中间：老样本占 [`ETA_SMOOTHING`]。
pub fn smooth_unit_ms(previous: Option<f64>, elapsed: Duration, units_done: usize) -> Option<f64> {
    if units_done == 0 {
        return None;
    }
    let raw = elapsed.as_secs_f64() * 1000.0 / units_done as f64;
    Some(match previous {
        Some(old) => old * ETA_SMOOTHING + raw * (1.0 - ETA_SMOOTHING),
        None => raw,
    })
}

/// 剩余时间（由平滑后的每单位耗时推算；不足 1 秒 → `None`）。
pub fn eta_from_unit_ms(
    unit_ms: Option<f64>,
    units_done: usize,
    units_total: usize,
) -> Option<Duration> {
    let unit_ms = unit_ms?;
    let remaining = units_total.saturating_sub(units_done);
    if remaining == 0 {
        return None;
    }
    let ms = unit_ms * remaining as f64;
    if !ms.is_finite() || ms < ETA_MIN_MS {
        return None;
    }
    Some(Duration::from_millis(ms as u64))
}
