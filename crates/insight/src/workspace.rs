//! 洞察**中间工作页**（D63 落地：右栏摘要版保留，中央可停靠洞察 tab 并存；
//! D66 完全对齐 `docs/architecture/insight/insight-workspace-prototype.html`）。
//!
//! 承载形态（D63 / D65 / D66）：
//! - 面板持有一个 [`InsightView`] 实体作为**画像状态宿主**（取数链同右栏），
//!   但中间页**不渲染右栏的五 Tab**，而是渲染原型规定的画像版式：
//!   工具栏（刷新 / 取样 / 导出 + 目标切换 + 当前目标 chip）、两段式取样进度、
//!   表画像四卡 + 质量评分环 + 列类型分布 + 列画像表 + 质量提示（列目标走列画像版式）。
//! - 多表对比（D64 阶段② / D66）：候选**只收「已进入洞察的目标」**，三态对齐原型
//!   （候选表勾选 → 运行中进度 → 结果表带健康着色，点行重开该表画像）。
//!   聚合会话内已完成的画像摘要（不重新取数）；负载大时退化为多 tab 切换。
//! - 用户口径：**导航树表行前不加勾选框**，对比候选 = 已打开过洞察的目标。

use std::time::Duration;

use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, TabGroup};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::progress::{Progress, ProgressCircle};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Size, Sizable as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::insight_view::InsightView;
use crate::model::{
    fmt_rows, ColumnKind, InsightPanelState, InsightTarget, TableColumnView, TableProfileView,
};
use crate::quality_scorer::Grade;
use crate::service::persistence::SOURCE_SAMPLE_LIMIT;
use crate::ui;
use crate::view::{error_block, grade_color, skeleton};

/// 中间工作页的两种形态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceMode {
    /// 单表画像（原型「洞察工作页」：工具栏 + 画像版式）
    Insight,
    /// 多表对比（D64 阶段②，不重新取数；三态：候选 → 运行 → 结果）
    Compare,
}

/// 会话内已进入洞察的目标（对比候选）。
///
/// 「只有进入过洞察的表才能选择对比」——候选源就是本列表：导航树 / 结果列表 /
/// SQL 编辑器等入口**在打开洞察时**都会经 [`InsightWorkspacePanel::open_target`] 登记。
/// 历史快照候选留待 `InsightTableReportStore` 落库后接入（D64 阶段②后半）。
#[derive(Debug, Clone)]
pub struct OpenedTarget {
    /// 稳定业务键（跨目标种类去重用，见 [`target_key`]）
    pub key: String,
    pub target: InsightTarget,
    /// 该目标最近一次完成的表画像摘要；未完成 / 非表目标为 `None`（不编假值）
    pub summary: Option<TargetSummary>,
}

/// 画像摘要：对比聚合的最小事实集（行数 / 列数 / 表级评分），**不重新取数**。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetSummary {
    pub row_count: Option<u64>,
    pub column_count: Option<usize>,
    /// 表级质量分（「评估全表」完成后才有，未评估不是 0 分）
    pub score: Option<f64>,
    pub grade: Option<Grade>,
}

impl TargetSummary {
    fn of_table(profile: &TableProfileView) -> Self {
        Self {
            row_count: Some(profile.row_count),
            column_count: Some(profile.columns.len()),
            score: profile.quality.as_ref().map(|q| q.overall),
            grade: profile.quality.as_ref().map(|q| q.grade),
        }
    }
}

/// 目标的稳定业务键：跨种类唯一、与连接 / 来源区分（同表不同连接不合并）。
fn target_key(t: &InsightTarget) -> String {
    match t {
        InsightTarget::Column { temp_table, column, .. } => format!("column:{temp_table}:{column}"),
        InsightTarget::Table { temp_table, table_name } => {
            format!("table:{temp_table}:{table_name}")
        }
        InsightTarget::SourceColumn { source, column, .. } => format!(
            "srccolumn:{}:{}:{column}",
            source.conn_id().unwrap_or("mem"),
            source.label()
        ),
        InsightTarget::SourceTable { source, table_name } => format!(
            "srctable:{}:{}:{table_name}",
            source.conn_id().unwrap_or("mem"),
            source.label()
        ),
        InsightTarget::MultiColumn { temp_table, columns } => {
            format!("multi:{temp_table}:{}", columns.join(","))
        }
        InsightTarget::Schema {
            conn_id,
            database,
            schema,
        } => format!("schema:{conn_id}:{database}:{:?}", schema),
    }
}

pub struct InsightWorkspacePanel {
    /// 完整画像状态宿主（复用右栏同款实体与取数链）
    pub(crate) view: Entity<InsightView>,
    mode: WorkspaceMode,
    /// 已进入洞察的目标（新目标在前；对比候选）
    opened: Vec<OpenedTarget>,
    /// 对比勾选的目标键
    compare_selected: Vec<String>,
    /// 对比「运行中」态（原型三态之一）：聚合本身瞬时，给一小段进度示意
    compare_running: bool,
    /// 对比「已完成」态（结果表可展示）
    compare_done: bool,
    /// 窗口焦点（Dock tab 聚焦用，对齐 property_panel / search_results 的中央面板协议）
    focus_handle: FocusHandle,
    /// 所在 Dock tab 组（`BasePanel::on_added_to` 注入；重复打开时用于聚焦自身）
    group: Option<gpui::WeakEntity<TabGroup>>,
}

impl InsightWorkspacePanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            view: cx.new(InsightView::new),
            mode: WorkspaceMode::Insight,
            opened: Vec::new(),
            compare_selected: Vec::new(),
            compare_running: false,
            compare_done: false,
            focus_handle: cx.focus_handle(),
            group: None,
        }
    }

    /// 完整画像实体（宿主装配时用它接 `jobs::attach` / `insight_actions::attach`）
    pub fn insight_view(&self) -> &Entity<InsightView> {
        &self.view
    }

    pub fn mode(&self) -> WorkspaceMode {
        self.mode
    }

    pub fn opened(&self) -> &[OpenedTarget] {
        &self.opened
    }

    pub fn compare_selected(&self) -> &[String] {
        &self.compare_selected
    }

    /// 指向新目标：先把当前目标的画像摘要存档进 `opened`（对比聚合不重取数的前提），
    /// 再登记 / 去重，最后交 [`InsightView::set_target`]。
    ///
    /// 入口语义（D65）：「查看统计」在中间开 tab（本面板由宿主建一次），
    /// 已有 tab 则复用——所以这里是**单 tab 随目标切换**；每表独立 tab 的
    /// 多 tab 形态是负载兜底，见 `insight-architecture.md` D64。
    pub fn open_target(&mut self, target: InsightTarget, cx: &mut Context<Self>) {
        self.archive_summary(cx);
        let key = target_key(&target);
        if let Some(entry) = self.opened.iter_mut().find(|e| e.key == key) {
            entry.target = target.clone();
        } else {
            self.opened.insert(
                0,
                OpenedTarget {
                    key,
                    target: target.clone(),
                    summary: None,
                },
            );
        }
        // 回到画像形态：打开某个目标意味着要看它的画像（D66 对齐原型入口语义）
        self.mode = WorkspaceMode::Insight;
        self.view.update(cx, |view, cx| view.set_target(target, cx));
        cx.notify();
    }

    /// 切换形态（画像 / 对比）。切到对比不打断取数：`view` 状态保留，切回即见。
    pub fn set_mode(&mut self, mode: WorkspaceMode, cx: &mut Context<Self>) {
        if self.mode != mode {
            self.mode = mode;
            cx.notify();
        }
    }

    /// 对比候选勾选（Checkbox 回调只给布尔，这里按 key 翻转）
    pub fn toggle_candidate(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.compare_selected.iter().position(|k| k == key) {
            self.compare_selected.remove(pos);
        } else {
            self.compare_selected.push(key.to_string());
        }
        cx.notify();
    }

    /// 对比列表点行：重新打开该目标画像（并切回画像形态）
    pub fn open_candidate(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(entry) = self.opened.iter().find(|e| e.key == key) else {
            return;
        };
        let target = entry.target.clone();
        // 复用 open_target：登记 / 摘要存档 / 切回画像形态都在同一条路径
        self.open_target(target, cx);
        cx.notify();
    }

    /// 刷新（⟳）：重发当前 Tab 的取数请求（画像 / 结构按目标 Tab 各走各的）
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| view.reload(cx));
    }

    /// 开始对比（原型「开始对比」按钮）：勾选集 → 运行中 → 结果。
    ///
    /// 阶段②聚合会话内已完成的画像摘要、**不重新取数**，结果瞬时可得；
    /// 「运行中」给 600ms 示意（对齐原型三态交互），随后落成结果表。
    pub fn start_compare(&mut self, cx: &mut Context<Self>) {
        if self.compare_selected.is_empty() || self.compare_running {
            return;
        }
        self.compare_running = true;
        self.compare_done = false;
        cx.notify();
        let weak = cx.entity().downgrade();
        let timer = cx.background_executor().timer(Duration::from_millis(600));
        cx.spawn(async move |_, cx| {
            timer.await;
            let _ = weak.update(cx, |panel, cx| {
                panel.compare_running = false;
                panel.compare_done = true;
                cx.notify();
            });
        })
        .detach();
    }

    /// 重新对比：清掉结果态回到候选态（勾选保留）
    fn reset_compare(&mut self, cx: &mut Context<Self>) {
        self.compare_running = false;
        self.compare_done = false;
        cx.notify();
    }

    /// 切到旧目标前，把**当前目标**已完成的表画像摘要写回 `opened`。
    fn archive_summary(&mut self, cx: &Context<Self>) {
        let Some(current_key) = self.view.read(cx).target().map(target_key) else {
            return;
        };
        let Some(profile) = self.view.read(cx).data().table.as_ref() else {
            return;
        };
        if let Some(entry) = self.opened.iter_mut().find(|e| e.key == current_key) {
            entry.summary = Some(TargetSummary::of_table(profile));
        }
    }
}

impl Render for InsightWorkspacePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let theme = cx.theme();
        // render 纯读：一次取当前目标与加载态，供各片段共用（避免片段里重复读实体）
        let rem = window.rem_size();
        let inline_icon = rem * ui::INSIGHT_INLINE_ICON_SIZE;
        let empty_icon = rem * ui::EMPTY_ICON_SIZE;
        let ring_px = rem * ui::INSIGHT_WS_RING_SIZE;

        div()
            .key_context("insight-workspace")
            .v_flex()
            .size_full()
            .bg(theme.colors.background)
            .child(self.render_toolbar(&entity, theme, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(match self.mode {
                        WorkspaceMode::Insight => self
                            .render_insight_content(
                                cx,
                                &entity,
                                theme,
                                inline_icon,
                                empty_icon,
                                ring_px,
                            )
                            .into_any_element(),
                        WorkspaceMode::Compare => {
                            self.render_compare(cx, &entity, theme).into_any_element()
                        }
                    }),
            )
    }
}

impl InsightWorkspacePanel {
    /// 工具栏（D66 对齐原型）：刷新 / 取样 / 导出 + 分隔线 + 目标切换菜单 + 当前目标 chip。
    ///
    /// - 刷新 = 重算当前画像；取样 = 重新取样并画像（源目标重走「取样 → 画像」两段）。
    /// - 导出报告尚无宿主写文件通道（列 / 结构导出各有事件，表画像导出未接）——
    ///   disabled + tooltip 标注，不做点了没反应的按钮。
    /// - 目标切换用语义 `Button::dropdown_menu`：方向键 / Escape / 焦点恢复由组件负责。
    fn render_toolbar(
        &self,
        entity: &Entity<Self>,
        theme: &Theme,
        cx: &Context<Self>,
    ) -> Div {
        let colors = theme.colors;
        // 菜单构造需要 opened 快照（闭包 'static，不能借面板状态）：只带 key + 标题
        let candidates: Vec<(String, String)> = self
            .opened
            .iter()
            .map(|e| (e.key.clone(), e.target.title()))
            .collect();
        let current_key = self.view.read(cx).target().map(target_key);
        // chip 文本：连接 / 来源描述 · 表名（原型「连接 · 表」）
        let chip = self.view.read(cx).target().map(|t| {
            (
                t.detail().unwrap_or_else(|| "DuckDB".into()),
                t.title(),
            )
        });

        div()
            .flex_none()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .px(rems(ui::PANEL_PADDING))
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(colors.border)
            .child(
                Button::new("insight-ws-refresh")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RotateCw)
                    .label("刷新")
                    .tooltip("重算当前画像")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            entity.update(app, |panel, cx| panel.refresh(cx));
                        }
                    }),
            )
            .child(
                Button::new("insight-ws-sample")
                    .ghost()
                    .xsmall()
                    .label("取样")
                    .tooltip("重新从源库取样并画像")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            entity.update(app, |panel, cx| panel.refresh(cx));
                        }
                    }),
            )
            .child(
                Button::new("insight-ws-export")
                    .ghost()
                    .xsmall()
                    .icon(IconName::FileText)
                    .label("导出")
                    .disabled(true)
                    .tooltip("导出画像报告（待接入）"),
            )
            .child(divider(theme))
            // 目标切换下拉：已进入洞察的目标 → 切换画像；末项 → 打开多表对比。
            // 菜单闭包要 'static：先克隆实体句柄，不借用参数引用
            .child({
                let entity_owned = entity.clone();
                Button::new("insight-ws-target-switch")
                    .ghost()
                    .xsmall()
                    // 常显下拉箭头：DropdownButton 的 caret 半块在 ghost 变体下
                    // hover 才显现，静止时只剩表名 label——用户看不出可下拉
                    .icon(IconName::ChevronDown)
                    .label(
                        chip.as_ref()
                            .map(|(_, title)| title.clone())
                            .unwrap_or_else(|| "切换目标".into()),
                    )
                    .tooltip("切换洞察目标")
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for (key, title) in &candidates {
                            let entity = entity_owned.clone();
                            let key = key.clone();
                            let item = PopupMenuItem::new(title.clone())
                                .checked(Some(key.as_str()) == current_key.as_deref())
                                .on_click(move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.open_candidate(&key, cx);
                                    });
                                });
                            menu = menu.item(item);
                        }
                        if !candidates.is_empty() {
                            menu = menu.separator();
                        }
                        let open_compare = entity_owned.clone();
                        menu.item(
                            PopupMenuItem::new("打开多表对比…").on_click(move |_, _, app| {
                                open_compare.update(app, |panel, cx| {
                                    panel.set_mode(WorkspaceMode::Compare, cx);
                                });
                            }),
                        )
                    })
            })
            .child(div().flex_1().min_w_0())
            // 当前目标 chip（原型 target-chip：连接 · 表）
            .when_some(chip, |this, (source, title)| {
                this.child(
                    div()
                        .flex_none()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .px_1()
                        .h(rems(ui::ROW_HEIGHT * 0.8))
                        .rounded_sm()
                        .border_1()
                        .border_color(colors.border)
                        .text_xs()
                        .child(
                            div()
                                .max_w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                .text_ellipsis()
                                .text_color(colors.muted_foreground)
                                .child(source),
                        )
                        .child(div().text_color(colors.muted_foreground).child("·"))
                        .child(
                            div()
                                .max_w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                .text_ellipsis()
                                .text_color(colors.foreground)
                                .child(title),
                        ),
                )
            })
    }

    /// 中间页画像版式（D66）：按载荷分派——错误 / 取样加载 / 骨架 / 表画像 / 列画像 / 空态。
    ///
    /// 注意：与右栏 `InsightView` 的五 Tab 不同，中间页是**原型画像版式**：
    /// 表目标显示四卡 + 质量评分环 + 列类型分布 + 列画像表 + 质量提示；
    /// 列目标复用列画像四区（`render_column_profile`，宽版）。
    fn render_insight_content(
        &self,
        cx: &Context<Self>,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
        empty_icon: Pixels,
        ring_px: Pixels,
    ) -> Div {
        let colors = theme.colors;

        if let InsightPanelState::Error { message, retryable } = self.view.read(cx).state() {
            // 错误态重试按钮挂在画像实体上（reload 走 view 的取数链）
            return error_block(message, *retryable, self.view.clone(), theme, inline_icon);
        }
        if matches!(self.view.read(cx).state(), InsightPanelState::Loading) {
            return match self.view.read(cx).loading_note() {
                // 源目标取样：两段式进度（源取样 → 画像）
                Some(note) if !note.is_empty() => {
                    self.render_sample_progress(cx, theme)
                }
                // 临时表目标画像通常很快：纯骨架
                _ => div().p(rems(ui::PANEL_PADDING)).child(skeleton(theme)),
            };
        }
        if let Some(profile) = self.view.read(cx).data().table.as_ref() {
            return self.render_table_overview(cx, profile, entity, theme, inline_icon, ring_px);
        }
        if let Some(profile) = self.view.read(cx).data().column.as_ref() {
            return self.render_column_overview(cx, profile, theme, inline_icon);
        }
        // 无目标 / 无数据：入口引导
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                Icon::new(IconName::Eye)
                    .with_size(Size::Size(empty_icon))
                    .text_color(colors.muted_foreground),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(colors.foreground)
                    .child("暂无洞察目标"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .text_center()
                    .child("在结果列表头或导航树上右键表，选择「查看统计」"),
            )
    }

    /// 表画像版式（原型 renderInsightContent）：四卡 + 质量评分环 + 列类型分布 + 列画像表 + 质量提示。
    ///
    /// 与原型的字段差异（数据模型无 → 用真实字段等价替代，不编假值）：
    /// - 原型「取样耗时」→ 第四卡显示**质量状态**（已评估分数 / 未评估）；
    /// - 原型「数据分布直方图」→ **列类型分布**（可从列元数据现算）；
    /// - 原型列画像表「空值率 / 唯一值 / 角标」→ 现有「可空 / 质量分」（列级画像才有前两者）。
    fn render_table_overview(
        &self,
        cx: &Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
        ring_px: Pixels,
    ) -> Div {
        let _ = cx;
        let columns = &profile.columns;
        let total = columns.len();
        let quality = profile.quality.as_ref();

        // 四卡：总行数 / 列数 / 取样 / 质量状态（真实字段，结构对齐原型四卡）
        let quality_value = match quality {
            Some(q) => format!("{} 分", q.overall as u32),
            None => "未评估".into(),
        };
        let quality_sub = match quality {
            Some(q) => q.grade.label().to_string(),
            None => "点下方按钮评估全表".into(),
        };
        let cards = div()
            .w_full()
            .flex()
            .items_stretch()
            .gap_2()
            .child(summary_card(theme, "总行数", fmt_rows(profile.row_count), "源库全表统计"))
            .child(summary_card(theme, "列数", total.to_string(), "含主键 / 索引"))
            .child(summary_card(
                theme,
                "取样",
                format!("{SOURCE_SAMPLE_LIMIT} 行"),
                "源库取样上限",
            ))
            .child(summary_card(theme, "质量", quality_value, quality_sub));

        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(cards)
            .child(self.render_quality_panel(profile, entity, theme, ring_px))
            .child(self.render_type_distribution(columns, theme))
            .child(self.render_column_table(columns, theme, inline_icon))
            .child(self.render_quality_notes(profile, theme))
    }

    /// 质量评分 panel：评分环（`ProgressCircle`，对齐原型 score-ring）+ 四维细条。
    ///
    /// 四维从列元数据现算（需关注 / 可空 / 主键 / 未识别），不编假维度。
    fn render_quality_panel(
        &self,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        ring_px: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let quality = profile.quality.as_ref();
        let total = profile.columns.len().max(1);
        let ring_color = match quality {
            Some(q) => grade_color(q.grade, theme),
            None => colors.muted_foreground,
        };

        let dims: Vec<(&'static str, usize, Hsla)> = {
            let problem = quality.map(|q| q.problem_columns).unwrap_or(0);
            let nullable = profile.columns.iter().filter(|c| c.nullable).count();
            let pk = profile.columns.iter().filter(|c| c.primary_key).count();
            let unknown = profile
                .columns
                .iter()
                .filter(|c| c.kind == ColumnKind::Unknown)
                .count();
            vec![
                ("需关注", problem, colors.danger),
                ("可空列", nullable, colors.warning),
                ("主键列", pk, colors.success),
                ("未识别", unknown, colors.muted_foreground),
            ]
        };

        // 评分环：中间放总分（ProgressCircle 的 children 居中渲染）
        let score_ring = ProgressCircle::new("insight-ws-ring")
            .value(quality.map(|q| q.overall as f32).unwrap_or(0.0))
            .color(ring_color)
            .with_size(Size::Size(ring_px))
            .accessibility_label("表质量评分")
            .child(
                div()
                    .v_flex()
                    .items_center()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(rems(ui::INSIGHT_SCORE_FONT * 0.6))
                            .text_color(ring_color)
                            .child(match quality {
                                Some(q) => format!("{:.0}", q.overall),
                                None => "—".into(),
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(match quality {
                                Some(_) => ring_color,
                                None => colors.muted_foreground,
                            })
                            .child(match quality {
                                Some(q) => q.grade.label(),
                                None => "未评估",
                            }),
                    ),
            );

        let mut dim_rows = div().flex_1().min_w_0().v_flex().gap_1();
        for (name, value, fill) in dims {
            let ratio = value as f32 / total as f32;
            dim_rows = dim_rows.child(
                div()
                    .h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                            .flex_none()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(name),
                    )
                    .child(
                        Progress::new(SharedString::from(format!("insight-ws-dim-{name}")))
                            .value(ratio * 100.0)
                            .color(fill)
                            .with_size(Size::Small)
                            .flex_1(),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{value}/{total}")),
                    ),
            );
        }

        // 未评估：给「评估全表」入口（串行逐列评分，与右栏表 Tab 同一请求）
        let evaluate_row = if quality.is_none() {
            Some(
                div()
                    .h_flex()
                    .w_full()
                    .gap_2()
                    .child(div().flex_1())
                    .child(
                        Button::new("insight-ws-eval-table")
                            .small()
                            .icon(IconName::TriangleAlert)
                            .label("评估全表")
                            .disabled(profile.columns.is_empty())
                            .tooltip("逐列串行评分（避免撞后端并发上限），进度会一列列回填")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        panel
                                            .view
                                            .update(cx, |view, cx| {
                                                view.request_table_evaluation(cx)
                                            });
                                    });
                                }
                            }),
                    ),
            )
        } else {
            None
        };

        div()
            .v_flex()
            .w_full()
            .gap_1()
            .border_1()
            .border_color(colors.border)
            .rounded_sm()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("质量评分"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(match quality {
                                Some(q) => q.summary.clone(),
                                None => "评估全表后显示质量提示".into(),
                            }),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_3()
                    .px_2()
                    .pb_2()
                    .items_center()
                    .child(score_ring)
                    .child(dim_rows),
            )
            .when_some(evaluate_row, |this, row| this.child(row.px_2().pb_2()))
    }

    /// 数据分布 panel：原型是「amount 直方图」；表级没有样本分布，展示**列类型分布**
    /// （按类型族计数，从列元数据现算——同一「分布」panel 结构，真实数据）。
    fn render_type_distribution(&self, columns: &[TableColumnView], theme: &Theme) -> Div {
        let colors = theme.colors;
        let total = columns.len().max(1);
        let mut counts: Vec<(ColumnKind, usize)> = Vec::new();
        for column in columns {
            if let Some(entry) = counts.iter_mut().find(|(k, _)| *k == column.kind) {
                entry.1 += 1;
            } else {
                counts.push((column.kind, 1));
            }
        }
        // 固定展示顺序（与类型族徽标一致），避免渲染顺序随数据跳动
        let order = [
            ColumnKind::Numeric,
            ColumnKind::Text,
            ColumnKind::DateTime,
            ColumnKind::Boolean,
            ColumnKind::Unknown,
        ];
        let fill = |kind: ColumnKind| match kind {
            ColumnKind::Numeric => colors.primary,
            ColumnKind::Text => colors.success,
            ColumnKind::DateTime => colors.warning,
            ColumnKind::Boolean => colors.info,
            ColumnKind::Unknown => colors.muted_foreground,
        };

        let mut rows = div().v_flex().w_full().gap_1();
        for kind in order {
            let Some(&(_, count)) = counts.iter().find(|(k, _)| *k == kind) else {
                continue;
            };
            let ratio = count as f32 / total as f32;
            rows = rows.child(
                div()
                    .h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                            .flex_none()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(kind.label()),
                    )
                    .child(
                        Progress::new(SharedString::from(format!(
                            "insight-ws-dist-{:?}",
                            kind
                        )))
                        .value(ratio * 100.0)
                        .color(fill(kind))
                        .with_size(Size::Medium)
                        .flex_1(),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{count} 列")),
                    ),
            );
        }
        panel_box(theme, "列类型分布", format!("{total} 列"), rows)
    }

    /// 列画像表：原型列 = 列名 / 类型 / 空值率 / 唯一值 / 角标；
    /// 表级画像没有空值率 / 唯一值（列级才有），用**可空 / 质量分**等价呈现。
    fn render_column_table(
        &self,
        columns: &[TableColumnView],
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let header = div()
            .h_flex()
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(div().w(rems(ui::INSIGHT_TABLE_INDEX_WIDTH)).child("#"))
            .child(div().flex_1().min_w_0().child("列名"))
            .child(
                div()
                    .w(rems(ui::INSIGHT_TABLE_TYPE_WIDTH))
                    .child("类型"),
            )
            .child(div().w(rems(ui::INSIGHT_TABLE_FLAG_WIDTH)).child("可空"))
            .child(
                div()
                    .w(rems(ui::INSIGHT_TABLE_QUALITY_WIDTH))
                    .child("质量"),
            );

        let mut body = div().v_flex().w_full();
        for column in columns {
            let quality_text = match (column.score, column.grade()) {
                (Some(score), Some(grade)) => div()
                    .text_color(grade_color(grade, theme))
                    .child(format!("{:.0} {}", score, grade.label())),
                _ => div().text_color(colors.muted_foreground).child("—"),
            };
            body = body.child(
                div()
                    .id(SharedString::from(format!("insight-ws-col-{}", column.name)))
                    .h_flex()
                    .w_full()
                    .gap_2()
                    .px_2()
                    .py_0p5()
                    .text_xs()
                    .text_color(colors.foreground)
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_TABLE_INDEX_WIDTH))
                            .text_color(colors.muted_foreground)
                            .child(column.index.to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .child(column.name.clone()),
                    )
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_TABLE_TYPE_WIDTH))
                            .text_color(colors.muted_foreground)
                            .text_ellipsis()
                            .child(column.data_type.clone()),
                    )
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_TABLE_FLAG_WIDTH))
                            .text_color(if column.nullable {
                                colors.muted_foreground
                            } else {
                                colors.foreground
                            })
                            .child(if column.nullable { "YES" } else { "NO" }),
                    )
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_TABLE_QUALITY_WIDTH))
                            .child(quality_text),
                    )
                    .when(column.kind == ColumnKind::Unknown, |this| {
                        this.child(
                            Icon::new(IconName::Info)
                                .size(inline_icon)
                                .text_color(colors.muted_foreground),
                        )
                    }),
            );
        }
        panel_box(theme, "列画像", format!("{} 列", columns.len()), header).child(div().w_full().child(body))
    }

    /// 质量提示：原型的 notes 列表。表级只有 `quality.summary` 一句 + 需关注列数，
    /// 不编多条假提示。
    fn render_quality_notes(&self, profile: &TableProfileView, theme: &Theme) -> Div {
        let colors = theme.colors;
        let Some(quality) = &profile.quality else {
            return panel_box(
                theme,
                "质量提示",
                String::new(),
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("评估全表后可查看质量提示"),
            );
        };
        let mut notes = div().v_flex().w_full().gap_1();
        notes = notes.child(note_line(IconName::Info, quality.summary.clone(), theme));
        if quality.problem_columns > 0 {
            notes = notes.child(note_line(
                IconName::TriangleAlert,
                format!("{} 列需关注（评分低于阈值）", quality.problem_columns),
                theme,
            ));
        }
        if quality.scored_columns == 0 {
            notes = notes.child(note_line(
                IconName::Info,
                "暂无可评分的列（类型均未识别）".into(),
                theme,
            ));
        }
        panel_box(theme, "质量提示", String::new(), notes)
    }

    /// 两段式取样进度（原型 renderSampleProgress）：源取样 → 画像。
    ///
    /// stage 从加载说明推断：说明含「取样」= 正在源取样；含「画像」= 取样完成在画像。
    fn render_sample_progress(&self, cx: &Context<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        let note = self
            .view
            .read(cx)
            .loading_note()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let stage = if note.contains("取样") {
            0
        } else if note.contains("画像") {
            1
        } else {
            0
        };
        let target = self.view.read(cx).target().cloned();

        // 信息表：取样行数 / 源目标 / 临时表（stage ≥ 1 时显示）
        let info = if stage >= 1 {
            let temp = target
                .as_ref()
                .and_then(|t| self.view.read(cx).data().sample_table(t))
                .unwrap_or("—");
            let detail = target
                .as_ref()
                .and_then(|t| t.detail())
                .unwrap_or_else(|| "—".into());
            Some(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_4()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(
                        div()
                            .child(format!("取样行数 {SOURCE_SAMPLE_LIMIT} / {SOURCE_SAMPLE_LIMIT}")),
                    )
                    .child(div().child(format!("源目标 {detail}")))
                    .child(div().child(format!("临时表 {temp}"))),
            )
        } else {
            None
        };

        div()
            .v_flex()
            .w_full()
            .gap_3()
            .p(rems(ui::PANEL_PADDING))
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .items_center()
                    .child(Icon::new(IconName::Eye).text_color(colors.info))
                    .child(
                        div()
                            .text_sm()
                            .text_color(colors.foreground)
                            .child("洞察分析中"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(target.as_ref().map(|t| t.title()).unwrap_or_else(|| "…".into())),
                    ),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(step_dot("源取样", stage == 0, stage >= 1, theme))
                    .child(div().w(rems(ui::INSIGHT_WS_NUM_WIDTH)).h_px().bg(colors.border))
                    .child(step_dot("画像", stage == 1, stage >= 2, theme)),
            )
            .child(
                div()
                    .h_flex()
                    .gap_1()
                    .items_center()
                    .child(pulse_dot(theme))
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(note),
                    ),
            )
            .child(
                Progress::new("insight-ws-sample-loading")
                    .loading(true)
                    .with_size(Size::Small)
                    .w_full(),
            )
            .when_some(info, |this, info| this.child(info))
            .child(skeleton(theme))
    }

    /// 列目标画像：复用列画像四区版式（宽版）。
    fn render_column_overview(
        &self,
        cx: &Context<Self>,
        profile: &crate::model::ColumnProfileView,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let view = self.view.clone();
        let rendered = view.read(cx).render_column_profile(profile, &view, theme, inline_icon);
        div().v_flex().w_full().p(rems(ui::PANEL_PADDING)).child(rendered).child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("列画像 · 右键结果表中的列可查看"),
        )
    }

    /// 对比页（D66 三态对齐原型）：
    /// 候选态（勾选表 + 开始对比）→ 运行态（进度示意）→ 结果态（健康着色 + 点行开画像）。
    fn render_compare(&self, cx: &Context<Self>, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        if self.opened.is_empty() {
            return div()
                .v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(colors.muted_foreground)
                        .child("还没有可对比的目标"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("先在导航树或结果列表上打开一张表的洞察，之后它才会出现在这里"),
                );
        }

        if self.compare_running {
            return self.render_compare_running(cx, theme);
        }
        if self.compare_done {
            return self.render_compare_result(cx, entity, theme);
        }
        self.render_compare_candidates(cx, entity, theme)
    }

    /// 对比候选态：标题 + 已选计数 + 「开始对比」+ 候选表（勾选 / 表名 / 数据源 / 行数 / 列数 / 状态）。
    fn render_compare_candidates(
        &self,
        cx: &Context<Self>,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let current_key = self.view.read(cx).target().map(target_key);
        let current_loading = matches!(self.view.read(cx).state(), InsightPanelState::Loading);

        div()
            .v_flex()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .h(rems(ui::PANEL_HEADER_HEIGHT))
                    .px(rems(ui::PANEL_PADDING))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("多表洞察对比"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("已选择 {} 张表", self.compare_selected.len())),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("insight-ws-compare-run")
                            .primary()
                            .small()
                            .icon(IconName::Play)
                            .label("开始对比")
                            .disabled(self.compare_selected.is_empty())
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |panel, cx| panel.start_compare(cx));
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .p(rems(ui::PANEL_PADDING))
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child("候选 = 已进入洞察的目标；聚合会话内已完成的画像摘要，不重新取数（D64 阶段②）。"),
                            )
                            .child(compare_table_header(theme))
                            .children(self.opened.iter().map(|entry| {
                                let key = entry.key.clone();
                                let selected = self.compare_selected.contains(&key);
                                // 状态徽标：当前目标且加载中 → 分析中；有摘要 → 已分析；其余 → 未分析
                                let (label, color) = if Some(key.as_str()) == current_key.as_deref()
                                    && current_loading
                                {
                                    ("分析中", colors.info)
                                } else if entry.summary.is_some() {
                                    ("已分析", colors.success)
                                } else {
                                    ("未分析", colors.muted_foreground)
                                };
                                compare_row(theme, SharedString::from(format!("insight-ws-cand-{key}")))
                                    .child(
                                        Checkbox::new(SharedString::from(format!(
                                            "insight-ws-check-{key}"
                                        )))
                                        .checked(selected)
                                        .on_click({
                                            let entity = entity.clone();
                                            let key = key.clone();
                                            move |_, _, app| {
                                                entity.update(app, |panel, cx| {
                                                    panel.toggle_candidate(&key, cx)
                                                });
                                            }
                                        }),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .text_color(colors.foreground)
                                            .text_ellipsis()
                                            .child(entry.target.title()),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .text_ellipsis()
                                            .child(entry.target.detail().unwrap_or_default()),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(
                                                entry
                                                    .summary
                                                    .and_then(|s| s.row_count)
                                                    .map(|n| fmt_rows(n))
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(
                                                entry
                                                    .summary
                                                    .and_then(|s| s.column_count)
                                                    .map(|n| n.to_string())
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_GRADE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(color)
                                            .child(label),
                                    )
                                    .on_click({
                                        let entity = entity.clone();
                                        let key = key.clone();
                                        move |_, _, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.open_candidate(&key, cx)
                                            });
                                        }
                                    })
                            })),
                    ),
            )
    }

    /// 对比运行态：进度条 + 每行「聚合中 / 排队中」（原型运行态；聚合无真实取数工序，
    /// 进度为示意，`start_compare` 600ms 后落结果）。
    fn render_compare_running(&self, _cx: &Context<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        let selected: Vec<_> = self
            .opened
            .iter()
            .filter(|e| self.compare_selected.contains(&e.key))
            .collect();
        div()
            .v_flex()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .h(rems(ui::PANEL_HEADER_HEIGHT))
                    .px(rems(ui::PANEL_PADDING))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("多表洞察对比"),
                    )
                    .child(div().flex_1())
                    .child(
                        Progress::new("insight-ws-compare-progress")
                            .loading(true)
                            .with_size(Size::Small)
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH * 2.0)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .p(rems(ui::PANEL_PADDING))
                            .gap_1()
                            .children(selected.into_iter().enumerate().map(|(i, entry)| {
                                let running = i == 0;
                                div()
                                    .id(SharedString::from(format!(
                                        "insight-ws-run-{}",
                                        entry.key
                                    )))
                                    .h(rems(ui::ROW_HEIGHT))
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_1()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .child(entry.target.title()),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_xs()
                                            .text_color(if running {
                                                colors.info
                                            } else {
                                                colors.muted_foreground
                                            })
                                            .child(if running {
                                                "聚合中…"
                                            } else {
                                                "排队中"
                                            }),
                                    )
                            })),
                    ),
            )
    }

    /// 对比结果态：表名 / 数据源 / 行数 / 列数 / 健康分 / 评级（健康着色）+ 点行重开画像。
    ///
    /// 原型阈值 85 / 70 硬编码；本实现按 `Grade` 语义取色（`grade_color`），
    /// 阈值由 `quality_scorer` 一处提供，不在视图里再写一份。
    fn render_compare_result(&self, _cx: &Context<Self>, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        let selected: Vec<_> = self
            .opened
            .iter()
            .filter(|e| self.compare_selected.contains(&e.key))
            .collect();
        div()
            .v_flex()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .h(rems(ui::PANEL_HEADER_HEIGHT))
                    .px(rems(ui::PANEL_PADDING))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("多表洞察对比"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{} 张表 · 已完成", selected.len())),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("insight-ws-compare-again")
                            .ghost()
                            .small()
                            .icon(IconName::RotateCw)
                            .label("重新对比")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |panel, cx| panel.reset_compare(cx));
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .p(rems(ui::PANEL_PADDING))
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child("对比为列表式汇总：行数 / 列数 / 健康度 / 评级；点某行可在中间打开该表洞察工作页。"),
                            )
                            .child(
                                div()
                                    .h_flex()
                                    .w_full()
                                    .gap_2()
                                    .px_1()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(div().flex_1().child("表"))
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                            .flex_none()
                                            .child("数据源"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .child("行数"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .child("列数"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SCORE_WIDTH))
                                            .flex_none()
                                            .child("健康分"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_GRADE_WIDTH))
                                            .flex_none()
                                            .child("评级"),
                                    ),
                            )
                            .children(selected.into_iter().map(|entry| {
                                let s = entry.summary;
                                let (score_text, grade_text, color) = match s.and_then(|s| s.score) {
                                    Some(score) => (
                                        format!("{score:.0} 分"),
                                        s.and_then(|s| s.grade)
                                            .map(|g| g.label().to_string())
                                            .unwrap_or_else(|| "—".into()),
                                        s.and_then(|s| s.grade)
                                            .map(|g| grade_color(g, theme))
                                            .unwrap_or(colors.muted_foreground),
                                    ),
                                    None => ("—".into(), "未评估".into(), colors.muted_foreground),
                                };
                                div()
                                    .id(SharedString::from(format!(
                                        "insight-ws-res-{}",
                                        entry.key
                                    )))
                                    .h(rems(ui::ROW_HEIGHT))
                                    .w_full()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_1()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_ellipsis()
                                            .child(entry.target.title()),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .text_ellipsis()
                                            .child(entry.target.detail().unwrap_or_default()),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(
                                                s.and_then(|s| s.row_count)
                                                    .map(|n| fmt_rows(n))
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(colors.muted_foreground)
                                            .child(
                                                s.and_then(|s| s.column_count)
                                                    .map(|n| n.to_string())
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SCORE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(color)
                                            .child(score_text),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_GRADE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(color)
                                            .child(grade_text),
                                    )
                                    .on_click({
                                        let entity = entity.clone();
                                        let key = entry.key.clone();
                                        move |_, _, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.open_candidate(&key, cx)
                                            });
                                        }
                                    })
                            })),
                    ),
            )
    }
}

// ===== 片段助手（只在中间工作页用） =====

/// 工具栏竖分隔线
fn divider(theme: &Theme) -> Div {
    div()
        .w_px()
        .h(rems(ui::ROW_HEIGHT * 0.6))
        .bg(theme.colors.border)
        .mx_1()
}

/// 四卡之一（原型 .card：键 + 值 + 说明）
fn summary_card(theme: &Theme, key: &str, value: String, sub: impl Into<String>) -> Div {
    let colors = theme.colors;
    let sub = sub.into();
    div()
        .flex_1()
        .min_w_0()
        .v_flex()
        .gap_0p5()
        .p_2()
        .rounded_sm()
        .border_1()
        .border_color(colors.border)
        .child(div().text_xs().text_color(colors.muted_foreground).child(key.to_string()))
        .child(
            div()
                .text_size(rems(ui::INSIGHT_SCORE_FONT * 0.75))
                .text_color(colors.foreground)
                .child(value),
        )
        .child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .text_ellipsis()
                .child(sub),
        )
}

/// 带标题的 panel 容器（原型 .panel：panel-head + panel-body）
fn panel_box(theme: &Theme, title: &str, tag: String, body: Div) -> Div {
    let colors = theme.colors;
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .border_1()
        .border_color(colors.border)
        .rounded_sm()
        .child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .px_2()
                .py_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child(title.to_string()),
                )
                .child(div().flex_1())
                .when(!tag.is_empty(), |this| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(tag),
                    )
                }),
        )
        .child(div().w_full().px_2().pb_2().child(body))
}

/// 质量提示的一行（图标 + 文案）
fn note_line(icon: IconName, text: String, theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .items_center()
        .child(Icon::new(icon).text_color(colors.info))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(text),
        )
}

/// 取样进度里的呼吸圆点（暂态指示，循环动效；`reduce_motion` 下静止在 1.0）
fn pulse_dot(theme: &Theme) -> impl IntoElement {
    div()
        .w(rems(0.375))
        .h(rems(0.375))
        .rounded_full()
        .bg(theme.colors.info)
        .with_animation(
            "insight-ws-pulse",
            gpui_kit::Animation::new(Duration::from_millis(1200))
                .repeat()
                .with_easing(gpui_kit::bounce(gpui_kit::ease_in_out)),
            |el, delta| el.opacity(1.0 - delta * 0.5),
        )
}

/// 两段式进度的一步（源取样 / 画像）：圆点 + 标签
fn step_dot(name: &str, active: bool, done: bool, theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .w(rems(0.75))
                .h(rems(0.75))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(if done {
                    colors.success
                } else if active {
                    colors.info
                } else {
                    colors.muted_foreground
                })
                .bg(if done || active {
                    colors.list_active
                } else {
                    colors.list_hover
                })
                .child(if done { "✓" } else { "" }),
        )
        .child(
            div()
                .text_xs()
                .text_color(if active || done {
                    colors.foreground
                } else {
                    colors.muted_foreground
                })
                .child(name.to_string()),
        )
}

/// 对比行原语：居中行 + 行高（列内容由调用方追加）。
///
/// 带 `id` 后是 `Stateful<Div>`：hover / 点击按 id 记录状态（业务键，非下标）。
fn compare_row(theme: &Theme, id: SharedString) -> gpui::Stateful<Div> {
    let colors = theme.colors;
    div()
        .id(id)
        .h(rems(ui::ROW_HEIGHT))
        .w_full()
        .flex()
        .items_center()
        .gap_2()
        .px_1()
        .rounded_sm()
        .hover(|this| this.bg(colors.sidebar_accent))
}

/// 对比候选表头（勾选 / 表名 / 数据源 / 行数 / 列数 / 状态）
fn compare_table_header(theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .px_1()
        .text_xs()
        .text_color(colors.muted_foreground)
        .child(div().w(rems(1.25)).flex_none())
        .child(div().flex_1().child("表"))
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                .flex_none()
                .child("数据源"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                .flex_none()
                .child("行数"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                .flex_none()
                .child("列数"),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_WS_GRADE_WIDTH))
                .flex_none()
                .child("状态"),
        )
}

// ===== 中央 Dock 面板协议（对齐 property_panel / search_results 的装配方式） =====

impl EventEmitter<gpui_kit::component::dock::PanelEvent> for InsightWorkspacePanel {}

impl Focusable for InsightWorkspacePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for InsightWorkspacePanel {
    fn panel_name(&self) -> &'static str {
        "insight-workspace"
    }

    fn on_added_to(
        &mut self,
        group: gpui::WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut gpui::Context<Self>,
    ) {
        self.group = Some(group);
    }

    fn on_removed(&mut self, _window: &mut Window, _cx: &mut gpui::Context<Self>) {
        // 中间洞察 tab 被 ✕ 关闭：面板随 Dock 移除而 drop（宿主只持弱句柄），
        // 下次入口再打开时按「未建」重建——目标集合（`opened`）会清空，
        // 与对比候选「会话内已进入洞察」口径一致（关掉工作页 = 放弃本轮候选）。
        self.group = None;
    }
}

impl ComponentPanel for InsightWorkspacePanel {
    /// Dock tab 标题 = 当前洞察目标表名（参考 VS Code「tab 标题 = 打开对象名」约束），
    /// 未打开目标时回退「洞察」。
    fn tab_name(&self, cx: &App) -> Option<SharedString> {
        Some(
            self.view
                .read(cx)
                .target()
                .map(|t| SharedString::from(t.title()))
                .unwrap_or_else(|| SharedString::from("洞察")),
        )
    }

    fn title(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let name = self
            .view
            .read(cx)
            .target()
            .map(|t| SharedString::from(t.title()))
            .unwrap_or_else(|| SharedString::from("洞察"));
        // 标题栏内超长表名省略（vscode 约束：长标题省略 + hover 提示全名）
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .max_w(px(240.))
            .overflow_hidden()
            .text_ellipsis()
            .child(name)
    }
}

/// 把中间洞察 tab 切到前台（窗口聚焦 + 在所在 Dock 组里选中自身）。
///
/// **必须在实体更新之外调用**（与 `focus_property_tab` 同因：`TabGroup` 切 tab 会回读
/// 面板实体，在更新闭包内调用会 double lease panic）。选中按面板 `entity_id` 定位。
pub fn focus_insight_workspace_tab(
    panel: &Entity<InsightWorkspacePanel>,
    window: &mut Window,
    cx: &mut App,
) {
    let (focus_handle, group) = {
        let view = panel.read(cx);
        (view.focus_handle.clone(), view.group.clone())
    };
    window.focus(&focus_handle, cx);
    let Some(group) = group else {
        return;
    };
    let me = panel.entity_id();
    group
        .update(cx, |group, cx| {
            let ix = group
                .panels()
                .iter()
                .position(|p| p.view().entity_id() == me);
            if let Some(ix) = ix {
                group.select_tab(ix, window, cx);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    // 不写 `use gpui_kit::*` / `use super::*`：通配导入会把 gpui 的 `test` 属性宏带进
    // 作用域，而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己（recursion limit）。

    use gpui_kit::{AppContext as _, TestAppContext};

    use super::{target_key, InsightWorkspacePanel};
    use crate::model::{InsightTarget, SampleSource, TableColumnView, TableProfileView};

    fn table_target(name: &str) -> InsightTarget {
        InsightTarget::SourceTable {
            source: SampleSource::new("conn-1", format!("SELECT * FROM {name}"), "analytics"),
            table_name: name.into(),
        }
    }

    /// 最小表画像（模拟 jobs 回填；quality 留空，摘要只验证行数 / 列数）
    fn profile(name: &str, rows: u64, cols: usize) -> TableProfileView {
        TableProfileView {
            table_name: name.into(),
            source_label: "DuckDB 临时表",
            row_count: rows,
            columns: (0..cols)
                .map(|i| TableColumnView {
                    index: (i + 1) as i32,
                    name: format!("c{i}"),
                    data_type: "INTEGER".into(),
                    kind: crate::model::ColumnKind::Numeric,
                    nullable: false,
                    primary_key: false,
                    score: None,
                })
                .collect(),
            quality: None,
            progress: None,
        }
    }

    #[test]
    fn target_key_is_stable_and_distinguishes_sources() {
        // 同目标两次构造 → 同 key（去重前提）
        assert_eq!(target_key(&table_target("orders")), target_key(&table_target("orders")));
        // 同表不同连接 → 不同 key（双栏 / 多连接下不误合并）
        let other_conn = InsightTarget::SourceTable {
            source: SampleSource::new("conn-2", "SELECT * FROM orders", "analytics"),
            table_name: "orders".into(),
        };
        assert_ne!(target_key(&table_target("orders")), target_key(&other_conn));
        // 不同目标种类不撞（列 vs 表）
        let col = InsightTarget::Column {
            temp_table: "t1".into(),
            column: "amount".into(),
            data_type: "DECIMAL".into(),
        };
        assert_ne!(target_key(&col), target_key(&table_target("orders")));
    }

    #[gpui_kit::test]
    fn open_target_registers_dedupes_and_archives(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let panel = cx.new(InsightWorkspacePanel::new);
        // 开 A → 注入画像 → 切 B：A 的摘要应存档进 opened
        panel.update(cx, |p, cx| p.open_target(table_target("orders"), cx));
        panel.update(cx, |p, cx| {
            p.insight_view()
                .update(cx, |v, cx| v.set_table_profile(profile("orders", 1000, 8), cx));
        });
        panel.update(cx, |p, cx| p.open_target(table_target("users"), cx));
        panel.update(cx, |p, _| {
            assert_eq!(p.opened().len(), 2, "两个不同表都应登记为对比候选");
            let orders = p
                .opened()
                .iter()
                .find(|e| e.target.title() == "orders")
                .expect("orders 应在候选里");
            let s = orders.summary.expect("切走前应把 orders 的画像摘要存档");
            assert_eq!(s.row_count, Some(1000));
            assert_eq!(s.column_count, Some(8));
        });
        // 重复打开同一表 → 去重（不新增候选）
        panel.update(cx, |p, cx| p.open_target(table_target("orders"), cx));
        panel.update(cx, |p, _| {
            assert_eq!(p.opened().len(), 2, "重复打开同表不应新增候选");
        });
    }

    #[gpui_kit::test]
    fn compare_toggle_and_open_candidate(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let panel = cx.new(InsightWorkspacePanel::new);
        panel.update(cx, |p, cx| p.open_target(table_target("orders"), cx));
        panel.update(cx, |p, cx| p.open_target(table_target("users"), cx));
        let orders_key = target_key(&table_target("orders"));
        // 勾选 → 选中集变化；再点一次 → 取消
        panel.update(cx, |p, cx| p.toggle_candidate(&orders_key, cx));
        panel.update(cx, |p, _| assert_eq!(p.compare_selected(), &[orders_key.clone()]));
        panel.update(cx, |p, cx| p.toggle_candidate(&orders_key, cx));
        panel.update(cx, |p, _| assert!(p.compare_selected().is_empty()));
        // 点候选行 → 切回画像形态并指向该目标
        panel.update(cx, |p, cx| p.open_candidate(&orders_key, cx));
        panel.update(cx, |p, cx| {
            assert_eq!(p.mode(), crate::workspace::WorkspaceMode::Insight);
            assert_eq!(p.insight_view().read(cx).target().map(|t| t.title()), Some("orders".into()));
        });
    }
}
