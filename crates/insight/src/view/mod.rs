//! 洞察面板的渲染片段（规格见 `docs/architecture/insight/insight-dev-plan.md` §11）
//!
//! 状态宿主仍是 [`crate::insight_view::InsightView`]（它持 target / state / data 与
//! 各视角的载荷）；本目录只放**渲染与片段助手**：一组视图一个文件，共用的助手在本文件。
//!
//! ```text
//! 面板头（36px）  → header.rs（dock-head「洞察」+ Toolbar：规则管理/重算画像/全量统计
//!                   [spacer] ＋新建洞察最右——对齐 EDA 原型 right-toolbar）
//! 无项目提示      → header.rs
//! 洞察表集合      → set.rs（it-head「已洞察表 [计数]」+ 多选开关 → it-list 卡片行
//!                   → it-actions 质量对比 → it-note；关系排查结果区）
//! 选中目标下钻    → drill.rs（dz-title 目标名 + 摘要卡（grade-ring）/ 表画像卡 /
//!                   多列聚合统计占位；按目标类型自适应：表/列/schema）
//! ```
//!
//! 共用助手（本文件）：`kind_badge` · `ratio_bar` · `section_title` ·
//! `muted_line` · `tab_hint` · `empty_state` · `skeleton` · `error_block`；
//! `grade_color` 仅供下钻区 schema 摘要（Schema 健康分独立域）使用。
//!
//! ## 为什么不做成独立 crate
//!
//! 片段读的全是 [`InsightView`] 的字段（target / data / open_sections…），那是**一个面板
//! 实例的状态**；拆 crate 只会把同一份状态跨 crate 传两遍（与 editor `view/results/` 那次
//! 判定同口径）。
//!
//! ## 可见性约定（位移时定的，不是随手写的）
//!
//! - **片段入口**（`render_header` / `render_insight_set` / `render_drillzone`）
//!   对 crate 可见：宿主的 `impl Render` 在 `insight_view.rs` 里转发。
//! - 只在本目录内用的片段是 `pub(in crate::view)`；视图内部的
//!   小片段与助手是**模块私有**，子模块用 `use super::…` 取用（父模块私有项对子模块可见）。
//! - `InsightView` 的字段是 `pub(crate)`：片段与宿主不在同一个模块，读字段必须如此。
//!   对外 API（`InsightView` 的 `pub` 方法与 [`crate::insight_view::InsightEvent`]）没有变化。
//!
//! ## 2026-10-30 右栏对齐 EDA 原型
//!
//! 右栏不再有 Tab 条与目标头：body = 「洞察表集合（it-list）」+「选中目标下钻
//! （drillzone）」。旧 Tab 视角的渲染（`render_overview` / `render_table_profile` /
//! `render_multi_view` / `render_schema_report` / `render_history`）保留在对应文件，
//! 入口函数标 `#[allow(dead_code)]`——能力保留待中间页 / 资产库复用，不随 UI 移除。

pub(crate) mod column;
pub(crate) mod drill;
pub(crate) mod header;
pub(crate) mod history;
pub(crate) mod multi;
pub(crate) mod overview;
pub(crate) mod schema;
pub(crate) mod set;
pub(crate) mod table;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::empty::{Empty, EmptyHeader, EmptyMedia, EmptyTitle};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::ColorName;
use gpui_kit::component::{Icon, IconName, Sizable as _, Size, Theme};
use gpui_kit::*;

use crate::insight_view::InsightView;
use crate::model::{ColumnKind, PanelTab};
use crate::quality_scorer::Grade;

impl InsightView {
    /// 内容主体相关渲染已由下钻区（`drill.rs`）承担；旧 Tab 视角渲染保留在
    /// 各自文件（入口函数标 `#[allow(dead_code)]`，能力保留待中间页复用）。

    /// 空态引导文案（按 Tab 给）。右栏不再按 Tab 渲染空态（下钻区统一引导），
    /// 方法保留供 `tests.rs` 断言使用。
    #[allow(dead_code)]
    pub(crate) fn empty_hint(&self) -> &'static str {
        if self.target.is_none() {
            return "在结果表列头或导航树上右键，选择洞察";
        }
        // 同一个「历史」Tab，无项目与有项目是两句不同的话：
        // 前者要的是「先打开项目」，后者要的是「选一列」
        if self.tab == PanelTab::History && !self.project_open {
            return "打开项目后可保存与查看快照";
        }
        tab_hint(self.tab)
    }
}

/// 类型徽标（色按类型族；文案取类型族名）。用 `Tag` 的 **outline** 变体：
/// 透明底 + 语义色文字 + 同色描边，贴近原「色字无底」的克制视觉，同时拿到标准标签
/// 的语义色 / 圆角 / 尺寸档位（Small = 10px 字，适配 280px 窄面板）。
///
/// 配色与 EDA 列卡类型徽标同一套（`Tag::color(ColorName)`，明暗主题自动 scale）：
/// Numeric=绿 / Text=蓝 / DateTime=紫 / Boolean=青 / Unknown=灰——同一类型族
/// 在中间工作页与右栏只此一种配色，保证产品统一。
///
/// `pub(crate)`：右栏下钻区（`drill.rs`）与旧 Tab 视图共用。
pub(crate) fn kind_badge(kind: ColumnKind) -> impl IntoElement {
    let tag = match kind {
        ColumnKind::Numeric => Tag::color(ColorName::Green),
        ColumnKind::Text => Tag::color(ColorName::Blue),
        ColumnKind::DateTime => Tag::color(ColorName::Violet),
        ColumnKind::Boolean => Tag::color(ColorName::Teal),
        ColumnKind::Unknown => Tag::color(ColorName::Gray),
    };
    tag.outline().small().flex_none().child(kind.label())
}

/// 等级 → 主题角色（四档取色；「较差」与「差」共用 danger）
///
/// `pub(crate)`：仅供结构报告（Schema 健康分独立域）取色；表 / 列画像的事实统计
/// 不合成等级，不调用本助手。
pub(crate) fn grade_color(grade: Grade, theme: &Theme) -> Hsla {
    let colors = theme.colors;
    match grade {
        Grade::Excellent => colors.success,
        Grade::Good => colors.primary,
        Grade::Fair => colors.warning,
        Grade::Poor | Grade::Bad => colors.danger,
    }
}

/// 比例条（语义进度条）：事实占比维度 / 分布条 / 评估进度共用。
///
/// `Progress` 自带进度指示器 a11y 与数值补间，替代自绘底槽 + 填充（track 色取
/// 填充色的 20% 透明度，是标准进度条视觉）。高度档位映射：
/// `INSIGHT_RATIO_BAR_HEIGHT`（0.375rem = 6px）→ `Size::Small`、
/// `INSIGHT_HISTOGRAM_BAR_HEIGHT`（0.5rem = 8px）→ `Size::Medium`。
/// 元素 id 由调用方给业务键（维度名 / 值域标签 / 固定进度条）。
fn ratio_bar(id: impl Into<ElementId>, ratio: f32, size: Size, fill: Hsla) -> impl IntoElement {
    Progress::new(id)
        .value(ratio * 100.0)
        .color(fill)
        .with_size(size)
}

fn section_title(text: &str, theme: &Theme) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 未落地 Tab 的提示（不显示假数据）。仅被 [`InsightView::empty_hint`] 使用。
#[allow(dead_code)]
fn tab_hint(tab: PanelTab) -> &'static str {
    match tab {
        PanelTab::Overview => "打开表或列的洞察后，摘要会自动汇总",
        PanelTab::Column => "右键结果表中的列，查看列画像",
        PanelTab::Table => "右键表（或结果集）选择「查看统计」",
        PanelTab::MultiColumn => "先对表或列做一次洞察，再回本 Tab 选列分析",
        PanelTab::Schema => "结构洞察随连接库打开：请在导航树选择库",
        PanelTab::History => "历史按列记录：请先打开某一列的洞察",
    }
}

/// 空态：居中图标 + 一句引导
///
/// `pub(crate)`：右栏下钻区（`drill.rs`）无目标时复用同一版式。
pub(crate) fn empty_state(icon: IconName, hint: &str, theme: &Theme, icon_px: Pixels) -> Div {
    // 用 gpui-kit Empty 组件：自带居中布局 + 虚线边框 + 媒体/标题槽位
    let empty = Empty::new()
        .header(
            EmptyHeader::new()
                .media(
                    EmptyMedia::new()
                        .with_variant(gpui_kit::component::empty::EmptyMediaVariant::Icon)
                        .child(
                            Icon::new(icon)
                                .with_size(Size::Size(icon_px))
                                .text_color(theme.colors.muted_foreground),
                        ),
                )
                .title(EmptyTitle::new().child(SharedString::from(hint))),
        );
    div().w_full().flex_1().child(empty)
}

/// 加载骨架（基础统计 5 行 + 分布 4 条；`Skeleton` 自带 shimmer 循环动效，
/// 骨架屏是暂态指示，不阻塞 Tab 切换；分隔线保留手搓 div，它不是骨架条）
///
/// `pub(crate)`：中间工作页（`workspace.rs`）在加载态复用同一骨架。
pub(crate) fn skeleton(theme: &Theme) -> Div {
    let bar = |w: f32| Skeleton::new().w(rems(w)).h(rems(0.75)).rounded_sm();
    let mut skeleton = div().v_flex().gap_2().w_full();
    for w in [7.0, 5.5, 6.5, 4.0, 5.0] {
        skeleton = skeleton.child(bar(w));
    }
    skeleton = skeleton.child(div().h_px().w_full().bg(theme.colors.border));
    for w in [12.0, 9.0, 10.0, 6.0] {
        skeleton = skeleton.child(bar(w));
    }
    skeleton
}

/// 源目标取样加载态：说明 + 不确定进度条 + 骨架。
///
/// 源取样要「源库查询 + DuckDB 建分析临时表」两段（大表 CTAS 可能较久），
/// 但 CTAS 在库内完成、拿不到真百分比——所以给一句阶段说明 + `Progress::loading`
/// 不确定进度条，而不是干骨架（用户会以为卡住）。`Progress::loading` 自带
/// 滑动动效，属暂态指示，不必再包一层动画。
///
/// `pub(crate)`：右栏下钻区（`drill.rs`）加载态复用同一版式。
pub(crate) fn sample_loading(note: &str, theme: &Theme) -> Div {
    div()
        .v_flex()
        .gap_2()
        .w_full()
        .child(
            div()
                .v_flex()
                .gap_1()
                .child(
                    Progress::new("insight-sample-loading")
                        .loading(true)
                        .with_size(Size::Small)
                        .w_full(),
                )
                .child(
                    div()
                        .id("insight-sample-loading-note")
                        .test_support()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(note.to_string()),
                ),
        )
        .child(skeleton(theme))
}

/// 错误态：文案 + 可选重试
///
/// `pub(crate)`：中间工作页（`workspace.rs`）的错误态共用同一版式。
pub(crate) fn error_block(
    message: &str,
    retryable: bool,
    entity: Entity<InsightView>,
    theme: &Theme,
    icon_px: Pixels,
) -> Div {
    let mut block = div().v_flex().gap_2().py_2().child(
        div()
            .h_flex()
            .gap_1()
            .child(
                Icon::new(IconName::TriangleAlert)
                    .with_size(Size::Size(icon_px))
                    .text_color(theme.colors.danger),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.danger)
                    .child(message.to_string()),
            ),
    );
    if retryable {
        block = block.child(
            Button::new("insight-retry")
                .secondary()
                .xsmall()
                .label("重试")
                .on_click(move |_, _, app| {
                    entity.update(app, |view, cx| view.reload(cx));
                }),
        );
    }
    block
}

/// 弱化说明行（右栏下钻区与旧 Tab 视图共用）
pub(crate) fn muted_line(text: &str, theme: &Theme) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}
