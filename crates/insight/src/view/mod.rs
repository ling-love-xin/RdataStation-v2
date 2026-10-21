//! 洞察面板的渲染片段（按 Tab 位移，规格见 `docs/architecture/insight/insight-dev-plan.md` §11）
//!
//! 状态宿主仍是 [`crate::insight_view::InsightView`]（它持 target / state / data / tab 与
//! 五个 Tab 的载荷）；本目录只放**渲染与片段助手**：一组 Tab 一个文件，共用的助手在本文件。
//!
//! ```text
//! 面板头（36px）  → header.rs（「洞察」 · ⚙ 规则管理 · ⟳ 重算）
//! 无项目提示      → header.rs
//! 目标头          → header.rs（目标名 + 类型徽标 + 空值率 / 行数）
//! Tab 条（28px）  → mod.rs（列 │ 表 │ 多列 │ 结构 │ 历史）
//! 内容四态分派    → mod.rs（render_body：错误态整页接管，否则以载荷为准）
//!   列画像四区    → column.rs（评分卡 + zone_* / stat_row / distribution_* / quality_zone / sample_zone）
//!   表探查        → table.rs（评估入口与进度 + 列名下钻热点）
//!   多列分析      → multi.rs（列多选 + 规则单选 + 结果与提示）
//!   结构报告      → schema.rs（健康条 + 四区 + 下钻热点 + 导出菜单）
//!   快照历史      → history.rs（列表 + 对比面板 + 清理 + 存储用量）
//! ```
//!
//! 共用助手（本文件）：`kind_badge` · `grade_color` · `ratio_bar` · `section_title` ·
//! `muted_line` · `tab_hint` · `empty_state` · `skeleton` · `error_block`。
//!
//! ## 为什么不做成独立 crate
//!
//! 片段读的全是 [`InsightView`] 的字段（target / data / open_sections…），那是**一个面板
//! 实例的状态**；拆 crate 只会把同一份状态跨 crate 传两遍（与 editor `view/results/` 那次
//! 判定同口径）。
//!
//! ## 可见性约定（位移时定的，不是随手写的）
//!
//! - **片段入口**（`render_header` / `render_tab_bar` / `render_body` / `render_score_card`）
//!   对 crate 可见：宿主的 `impl Render` 在 `insight_view.rs` 里转发。
//! - 只在本目录内用的片段是 `pub(in crate::view)`（如 `render_table_profile`）；Tab 内部的
//!   小片段与助手是**模块私有**，子模块用 `use super::…` 取用（父模块私有项对子模块可见）。
//! - `InsightView` 的字段是 `pub(crate)`：片段与宿主不在同一个模块，读字段必须如此。
//!   对外 API（`InsightView` 的 `pub` 方法与 [`crate::insight_view::InsightEvent`]）没有变化。

pub(crate) mod column;
pub(crate) mod header;
pub(crate) mod history;
pub(crate) mod multi;
pub(crate) mod overview;
pub(crate) mod schema;
pub(crate) mod set;
pub(crate) mod table;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::skeleton::Skeleton;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{Icon, IconName, Size, Sizable as _, Theme};
use gpui_kit::*;

use crate::insight_view::InsightView;
use crate::model::{ColumnKind, InsightPanelState, PanelTab};
use crate::quality_scorer::Grade;
use crate::ui;

impl InsightView {
    pub(crate) fn render_tab_bar(&self, entity: &Entity<Self>, theme: &Theme) -> Div {
        div()
            .flex_none()
            .h(rems(ui::INSIGHT_TAB_HEIGHT))
            .border_b_1()
            .border_color(theme.colors.border)
            .child(
                TabBar::new("insight-tabs")
                    .underline()
                    .with_size(Size::Small)
                    .selected_index(self.tab.index())
                    .children(PanelTab::ALL.iter().map(|t| Tab::new().label(t.label())))
                    .on_click({
                        let entity = entity.clone();
                        move |ix, _window, app| {
                            entity.update(app, |view, cx| view.set_tab_from_index(*ix, cx));
                        }
                    }),
            )
    }

    /// 内容主体。图标尺寸由 `render` 从 `rem_size()` 换算后传入（视图不写裸 px）。
    ///
    /// 渲染以**载荷**为准，状态只管两种例外：错误态整页接管；没载荷时才看
    /// 加载中（骨架）还是空态（入口提示）。这样「切到另一个 Tab 时后台还在取数」
    /// 不会把已取到的那个 Tab 的内容盖成骨架。
    pub(crate) fn render_body(
        &self,
        entity: &Entity<Self>,
        theme: &Theme,
        empty_icon: Pixels,
        inline_icon: Pixels,
        overview_ring: Pixels,
    ) -> Vec<AnyElement> {
        if let InsightPanelState::Error { message, retryable } = &self.state {
            return vec![
                error_block(message, *retryable, entity.clone(), theme, inline_icon)
                    .into_any_element(),
            ];
        }

        let payload: Option<AnyElement> = match self.tab {
            PanelTab::Overview => self
                .render_overview(entity, theme, inline_icon, overview_ring)
                .map(|el| el.into_any_element()),
            PanelTab::Column => self.data.as_column().map(|profile| {
                self.render_column_profile(profile, entity, theme, inline_icon)
                    .into_any_element()
            }),
            PanelTab::Table => self.data.as_table().map(|profile| {
                self.render_table_profile(profile, entity, theme, inline_icon)
                    .into_any_element()
            }),
            PanelTab::MultiColumn => self.data.as_multi().map(|view| {
                self.render_multi_view(view, entity, theme, inline_icon)
                    .into_any_element()
            }),
            PanelTab::Schema => self.data.as_schema().map(|report| {
                self.render_schema_report(report, entity, theme, inline_icon)
                    .into_any_element()
            }),
            PanelTab::History => self.data.as_history().map(|history| {
                self.render_history(history, entity, theme, inline_icon)
                    .into_any_element()
            }),
        };
        if let Some(element) = payload {
            return vec![element];
        }
        if self.state.is_loading() {
            // 有阶段说明（源目标取样由 jobs 分阶段更新）→ 显示说明 + 不确定进度条；
            // 无说明（临时表目标画像通常很快）→ 纯骨架
            if let Some(note) = self.loading_note.as_deref() {
                return vec![sample_loading(note, theme).into_any_element()];
            }
            return vec![skeleton(theme).into_any_element()];
        }
        vec![empty_state(IconName::Info, self.empty_hint(), theme, empty_icon).into_any_element()]
    }

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
fn kind_badge(kind: ColumnKind) -> impl IntoElement {
    let tag = match kind {
        ColumnKind::Numeric => Tag::info(),
        ColumnKind::Text => Tag::success(),
        ColumnKind::DateTime => Tag::warning(),
        ColumnKind::Boolean => Tag::primary(),
        ColumnKind::Unknown => Tag::secondary(),
    };
    tag.outline().small().flex_none().child(kind.label())
}

/// 等级 → 主题角色（四档取色；「较差」与「差」共用 danger）
///
/// `pub(crate)`：中间工作页（`workspace.rs`）的质量分与对比结果同用这套语义色。
pub(crate) fn grade_color(grade: Grade, theme: &Theme) -> Hsla {
    let colors = theme.colors;
    match grade {
        Grade::Excellent => colors.success,
        Grade::Good => colors.primary,
        Grade::Fair => colors.warning,
        Grade::Poor | Grade::Bad => colors.danger,
    }
}

/// 比例条（语义进度条）：评分卡维度 / 分布条 / 评估进度共用。
///
/// `Progress` 自带进度指示器 a11y 与数值补间，替代自绘底槽 + 填充（track 色取
/// 填充色的 20% 透明度，是标准进度条视觉）。高度档位映射：
/// `INSIGHT_RATIO_BAR_HEIGHT`（0.375rem = 6px）→ `Size::Small`、
/// `INSIGHT_HISTOGRAM_BAR_HEIGHT`（0.5rem = 8px）→ `Size::Medium`。
/// 元素 id 由调用方给业务键（维度名 / 值域标签 / 固定进度条）。
fn ratio_bar(id: impl Into<ElementId>, ratio: f32, size: Size, fill: Hsla) -> impl IntoElement {
    Progress::new(id).value(ratio * 100.0).color(fill).with_size(size)
}

fn section_title(text: &str, theme: &Theme) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 未落地 Tab 的提示（不显示假数据）
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
fn empty_state(icon: IconName, hint: &str, theme: &Theme, icon_px: Pixels) -> Div {
    div()
        .v_flex()
        .items_center()
        .justify_center()
        .gap_2()
        .py_4()
        .child(
            Icon::new(icon)
                .with_size(Size::Size(icon_px))
                .text_color(theme.colors.muted_foreground),
        )
        .child(
            div()
                .text_xs()
                .text_center()
                .text_color(theme.colors.muted_foreground)
                .child(hint.to_string()),
        )
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
fn sample_loading(note: &str, theme: &Theme) -> Div {
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
    let mut block = div()
        .v_flex()
        .gap_2()
        .py_2()
        .child(
            div()
                .h_flex()
                .gap_1()
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .with_size(Size::Size(icon_px))
                        .text_color(theme.colors.danger),
                )
                .child(div().text_xs().text_color(theme.colors.danger).child(message.to_string())),
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

fn muted_line(text: &str, theme: &Theme) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}
