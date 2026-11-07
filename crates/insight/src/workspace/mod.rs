//! 洞察**中间工作页**（D63 落地：右栏摘要版保留，中央可停靠洞察 tab 并存；
//! D66 对齐 `docs/architecture/insight/insight-eda-prototype.html`）。
//!
//! 承载形态（D63 / D65 / D66）：
//! - 面板持有一个 [`InsightView`] 实体作为**画像状态宿主**（取数链同右栏），
//!   但中间页**不渲染右栏的六 Tab**，而是渲染原型规定的画像版式：
//!   工具栏（刷新 / 取样 / 导出 + 目标切换 + 当前目标 chip）、两段式取样进度、
//!   表画像五卡 + 检查器命中环 + 列类型分布 + 列画像表 + 事实提示（列目标走列画像版式）。
//! - 多表对比（D64 阶段② / D66）：候选**只收「已进入洞察的目标」**，三态对齐原型
//!   （候选表勾选 → 运行中进度 → 结果表按事实指标着色，点行重开该表画像）。
//!   聚合会话内已完成的画像摘要（不重新取数）；负载大时退化为多 tab 切换。
//! - 用户口径：**导航树表行前不加勾选框**，对比候选 = 已打开过洞察的目标。
//! - 本页只呈现事实统计与检查器命中，不合成质量总分 / 等级（评级归归档后的资产库）。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui_kit::base::{Disableable as _, StyledExt as _};
// 滚动锚点（值得关注「跳转到该列」）经 `gpui_kit::*` glob 的 trait 面可用：
// `overflow_y_scroll` / `track_scroll` / `anchor_scroll` 在 0.8.0 属
// `StatefulInteractiveElement`，元素必须带稳定 id 才能挂（滚动容器与列卡锚点均已加 id）。
// kit 的 `Scrollable` 组件内部持有自己的 handle，外部 `ScrollAnchor` 挂不上去，
// 因此中间页滚动容器用 gpui 基座滚动而非 `overflow_y_scrollbar`。
use gpui::{Anchor, ScrollAnchor, ScrollHandle};
use gpui_kit::assets::IconName as CatalogIcon;
use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::dock::TabGroup;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::insight_view::{InsightEvent, InsightView};
use crate::model::{
    ColumnCheckState, ColumnChecker, ColumnKind, InsightPanelState, InsightSetEntry,
    InsightTarget, MultiResultView, NULL_RATE_NOTE, TableColumnView,
    TableProfileView, fmt_int, fmt_num, fmt_pct, fmt_rows, kurt_direction, normality_judge,
    skew_direction,
};
use crate::ui;
pub(crate) mod view;
use crate::workspace::view::{error_block, skeleton};

mod types;
pub use types::*;

mod tab_strip;
mod actions;

// 以下类型已搬 types.rs：
// - WorkspaceMode
// - OpenedTarget + impl to_set_entry
// - TargetSummary + impl of_table
// - EdaTab
// - ColZone
// - RelTab

/// 关掉 index 后应切到的相邻 tab：优先右侧原位，越界回退左侧；
/// 最后一个也被关掉（`len_after == 0`）时返回 `None`（面板回空态）。
/// 抽出为纯函数：close_opened 的状态机里这是唯一决策点，单测直测。
fn next_index_after_close(len_after: usize, closed: usize) -> Option<usize> {
    if len_after == 0 {
        None
    } else {
        Some(closed.min(len_after - 1))
    }
}

/// 目标的稳定业务键：跨种类唯一、与连接 / 来源区分（同表不同连接不合并）。
fn target_key(t: &InsightTarget) -> String {
    match t {
        InsightTarget::Column {
            temp_table, column, ..
        } => format!("column:{temp_table}:{column}"),
        InsightTarget::Table {
            temp_table,
            table_name,
        } => {
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
        InsightTarget::MultiColumn {
            temp_table,
            columns,
        } => {
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
    /// 所在窗口句柄（`on_added_to` / 首帧 render 存下）：探查确认弹窗的开关发生在
    /// **事件路径**（宿主 `App` 上下文，没有窗口）——经句柄 `update_window` 才能开
    /// （`rule_view` 的 `begin_load` 存句柄同一手法）
    window: Option<gpui::AnyWindowHandle>,
    /// 探查确认弹窗的开合去重（observer 每帧都可能回调，只在状态**变迁**时开/关）
    probe_modal_open: bool,
    /// 所在 Dock tab 组（`BasePanel::on_added_to` 注入；重复打开时用于聚焦自身）
    group: Option<gpui::WeakEntity<TabGroup>>,
    // ============ EDA 中间页状态（原型 insight-eda-prototype.html） ============
    /// 报告主 Tab（列分布 / 缺失 / 关联性 / 样本）
    eda_tab: EdaTab,
    /// 列分布：列多选（默认全选；200+ 列时用户可缩到几个字段）
    sel_cols: Vec<String>,
    /// 列分布：渐进渲染开关（列数超过 `INSIGHT_WS_COL_RENDER_CAP` 时默认只渲染
    /// 前 CAP 张卡，「显示全部」置 true——200+ 列全量渲染会卡首帧；虚拟列表
    /// 见 dev-plan §10 #14，这是低风险缓解）
    col_show_all: RefCell<bool>,
    /// 列分布：类型筛选（`None` = 全部）
    kind_filter: Option<ColumnKind>,
    /// 列分布：搜索定位（子串命中列名）
    col_q: String,
    /// 列多选弹层内：列名过滤词（独立于 `col_q`——弹层只影响勾选清单，不驱动卡片列表）
    picker_q: String,
    /// 弹层内搜索框订阅（同键只挂一次订阅；`picker_q` 的只读镜像）
    picker_search_subs: RefCell<Vec<Subscription>>,
    /// 关联性：子 Tab（数值相关 / 分类关联 / 值重叠）
    rel_tab: RelTab,
    /// 关联性：数值相关的列对（2 列执行相关系数规则）
    corr_cols: Vec<String>,
    /// 关联性：当前选中的多列规则（数值 = correlation / spearman，分类 = cramer-v）
    rel_rule: Option<String>,
    /// 列搜索框订阅（渲染管线惰性创建 InputState；同键只挂一次订阅）
    col_search_subs: RefCell<Vec<Subscription>>,
    /// 列卡：展开态集合（点击列卡头切换；业务键列名）
    col_open: RefCell<HashSet<String>>,
    /// 探查确认窗口的输入框订阅（行数 / 百分比 / 过滤 / 种子，同键只挂一次）
    /// 列卡四区：只看规则属性（**按列**存；开启后该列展开体只显示检查器列表。
    /// 原型按单列理解——「开哪列管哪列」；空 = 全部未开启）
    col_rule_only: RefCell<HashMap<String, bool>>,
    /// 自定义规则 Tab 的摘要缓存（列名 → 摘要；首次切到该 Tab 时后台载入，
    /// 避免每卡每帧跑规则代入）
    col_rules: RefCell<HashMap<String, Vec<crate::model::ColumnRuleBrief>>>,
    /// 列卡四区：每列当前区（0=基础统计 / 1=分布 / 2=检查器 / 3=样本）
    col_detail_zone: RefCell<HashMap<String, usize>>,
    /// 中间页滚动句柄（值得关注「跳转到该列」用，见 `render_worth_section`）
    content_scroll: ScrollHandle,
    /// 每列卡滚动锚点：`ScrollAnchor` 需复用同一实例（anchor_scroll 才持续更新
    /// last_origin），同一列渲染时从本表取，不每帧重建
    col_anchors: RefCell<HashMap<String, ScrollAnchor>>,
}

// EdaTab / ColZone / RelTab 已搬 types.rs

impl InsightWorkspacePanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.new(InsightView::new);
        // 探查确认窗的开关跟随 view 的 `probe_dialog` 状态（源目标进入 → 开；
        // 确认 / 取消 → 关）。observer 在事件路径回调（可能没有窗口），
        // 句柄在 `on_added_to` / 首帧 render 存下。
        cx.observe(&view, |panel, _view, cx| panel.sync_probe_modal(cx))
            .detach();
        Self {
            view,
            mode: WorkspaceMode::Insight,
            opened: Vec::new(),
            compare_selected: Vec::new(),
            compare_running: false,
            compare_done: false,
            focus_handle: cx.focus_handle(),
            window: None,
            probe_modal_open: false,
            group: None,
            eda_tab: EdaTab::default(),
            sel_cols: Vec::new(),
            col_show_all: RefCell::new(false),
            kind_filter: None,
            col_q: String::new(),
            picker_q: String::new(),
            picker_search_subs: RefCell::new(Vec::new()),
            rel_tab: RelTab::default(),
            corr_cols: Vec::new(),
            rel_rule: None,
            col_search_subs: RefCell::new(Vec::new()),
            col_open: RefCell::new(HashSet::new()),
            col_rule_only: RefCell::new(HashMap::new()),
            col_rules: RefCell::new(HashMap::new()),
            col_detail_zone: RefCell::new(HashMap::new()),
            content_scroll: ScrollHandle::new(),
            col_anchors: RefCell::new(HashMap::new()),
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

    /// 右栏「已洞察表」集合快照（含当前目标高亮）。
    ///
    /// 宿主在观察回调里推给右栏 `InsightView::set_insight_set`——集合与当前
    /// 目标都在中间页这里（`opened` / `view.target`），右栏只读快照、不自己登记。
    pub fn insight_set_entries(&self, cx: &App) -> Vec<InsightSetEntry> {
        let current_key = self.view.read(cx).target().map(target_key);
        self.opened
            .iter()
            .map(|o| o.to_set_entry(o.key == current_key.as_deref().unwrap_or("")))
            .flatten()
            .collect()
    }

    /// 探查确认弹窗的开关（view 的 `probe_dialog` 状态**变迁**时由 observer 调）。
    ///
    /// - 开：表单体是独立实体 `probe_dialog::ProbeDialogBody`（对话框架构下逐帧重渲，
    ///   `rule_view` 同款——builder 闭包只跑一次，纯 Div 是静态快照）；
    /// - 关：`window.close_dialog`——面板自身只开这一个对话框，关的就是它；
    /// - 拿不到窗口（脱窗口单测 / 非常规装配）：不开不关，状态仍在、不静默丢。
    fn sync_probe_modal(&mut self, cx: &mut Context<Self>) {
        let want_open = self.view.read(cx).probe_dialog().is_some();
        if want_open == self.probe_modal_open {
            return;
        }
        let Some(handle) = self.window else {
            return;
        };
        let view = self.view.clone();
        let opened = cx.update_window(handle, move |_, window, cx| {
            if want_open {
                use gpui::AppContext as _;
                use gpui_kit::component::WindowExt as _;
                let body = cx.new(|cx| probe_dialog::ProbeDialogBody::new(view.clone(), cx));
                let theme = cx.theme().clone();
                let width = rems(ui::INSIGHT_PROBE_DIALOG_WIDTH).to_pixels(window.rem_size());
                window.open_dialog(cx, move |dialog, _window, _cx| {
                    use gpui_kit::component::dialog::DialogButtonProps;
                    dialog
                        .title("探查确认")
                        .w(width)
                        // builder 是 `Fn`（可能多次调用）：实体经 clone 进树，
                        // 同一实体可在多个元素树渲染（render 每树各跑一次）
                        .child(body.clone())
                        // 三按钮（取消 / 全量探查 / 探查）由 ProbeDialogBody 自绘在表单底部，
                        // 随状态重渲（主按钮文案随选择列变）；这里只保留 ESC/overlay 取消。
                        .button_props(DialogButtonProps::default().show_cancel(false))
                        .on_cancel({
                            let view = view.clone();
                            move |_, _window, app| {
                                view.update(app, |v, cx| v.cancel_probe_dialog(cx));
                                true
                            }
                        })
                        .overlay(true)
                        .border_color(theme.colors.border)
                });
            } else {
                use gpui_kit::component::WindowExt as _;
                window.close_dialog(cx);
            }
        });
        match opened {
            Ok(()) => self.probe_modal_open = want_open,
            Err(e) => tracing::warn!("[insight] 探查确认弹窗开关失败: {e}"),
        }
    }

    /// 顶部 tab 条（A 方案多标签，2026-10-30 落地；对齐原型 `.tab-strip`）：
    /// 每个进入过洞察的目标一个 tab，dot = 当前目标实时状态（Loading 黄 /
    /// 就绪绿 / 出错红；非当前 tab 是静态灰点表示「已打开过」），✕ 关闭该 tab，
    /// 指向新目标：先把当前目标的画像摘要存档进 `opened`（对比聚合不重取数的前提），
    /// 再登记 / 去重，最后交 [`InsightView::set_target`]。
    ///
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
        // 句柄兜底：宿主可能在面板进 Dock 之前就 open_target（观察者先于
        // on_added_to 回调）——首帧补存一次，幂等、不 notify（纯句柄暂存）
        if self.window.is_none() {
            self.window = Some(window.window_handle());
        }
        let entity = cx.entity();
        let theme = cx.theme().clone();
        // render 纯读：一次取当前目标与加载态，供各片段共用（避免片段里重复读实体）
        let rem = window.rem_size();
        let inline_icon = rem * ui::INSIGHT_INLINE_ICON_SIZE;
        let empty_icon = rem * ui::EMPTY_ICON_SIZE;
        let ring_px = rem * ui::INSIGHT_WS_RING_SIZE;
        // tab 截断宽度同样按主题字号换算（不再写死 ×16）
        let tab_max_width = rem * ui::INSIGHT_WS_TAB_MAX_WIDTH;

        div()
            .key_context("insight-workspace")
            .v_flex()
            .size_full()
            .bg(theme.colors.background)
            // A 方案多标签（2026-10-30）：顶部 tab 条在工具栏之上（对齐原型 tab-strip）
            .child(self.render_tab_strip(&entity, &theme, tab_max_width, cx))
            .child(self.render_toolbar(&entity, &theme, cx))
            .child(
                div()
                    // 稳定 id：`overflow_y_scroll` / `track_scroll` 在 0.8.0
                    // 属 `StatefulInteractiveElement`（元素须带 id 才能挂滚动句柄）
                    .id("insight-ws-scroll")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    // 中间页滚动权威点（值得关注「跳转到该列」挂同一条句柄）。
                    // 用 gpui 基座滚动（`overflow_y_scroll`）而非 kit `overflow_y_scrollbar`：
                    // 后者是 kit Scrollable 组件，内部持有自己的 handle，外部 `ScrollAnchor`
                    // 挂不上去；跳列定位依赖 `track_scroll` 的 handle。
                    .overflow_y_scroll()
                    // 原型不做响应式：内容有最小宽，窄窗横向滚动而不是压缩栅格
                    .overflow_x_scroll()
                    .track_scroll(&self.content_scroll)
                    .child(
                        div()
                            // 原型 `body{min-width:1000px}` 在中央工作页这一格的对应值
                            // （1000 − activity 48 − 右 dock 280 = 672px）：低于它整页
                            // 横向滚动，五卡恒 5 列、列卡一列一行（2026-11-06），不折行不塌列。
                            .w_full()
                            .min_w(rems(ui::INSIGHT_WS_MIN_WIDTH))
                            .child(match self.mode {
                                WorkspaceMode::Insight => self
                                    .render_insight_content(
                                        window,
                                        cx,
                                        &entity,
                                        &theme,
                                        inline_icon,
                                        empty_icon,
                                        ring_px,
                                    )
                                    .into_any_element(),
                                WorkspaceMode::Compare => {
                                    self.render_compare(cx, &entity, &theme).into_any_element()
                                }
                            }),
                    ),
            )
    }
}
// ===== 子模块（拆分：渲染方法按域下沉，状态/协议/测试留在根） =====
mod colcard;
mod compare;
mod dock;
mod eda;
mod helpers;
mod markdown;
mod overview;
mod probe_dialog;
mod rel;
mod sample;
mod toolbar;

// 自由函数 re-export：供各渲染子模块 `use super::*` 取用（跨模块可见性）
// 仅 re-export 有跨模块消费的自由函数；compare_row/compare_table_header/explore_missing_weight/escape_md
// 均只在定义模块内部使用，无需上提。
pub(crate) use helpers::{
    col_explore_score, column_checkers, dist_bars, divider, note_line, panel_box, pulse_dot,
    step_dot, summary_card, zone_note, zone_rows_grouped,
};
pub(crate) use markdown::{dist_text_bars, render_markdown_report, render_multi_result};
pub use dock::focus_insight_workspace_tab;

#[cfg(test)]
mod tests {
    // 不写 `use gpui_kit::*` / `use super::*`：通配导入会把 gpui 的 `test` 属性宏带进
    // 作用域，而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己（recursion limit）。

    use gpui_kit::{AppContext as _, TestAppContext};

    use super::{InsightWorkspacePanel, target_key};
    use crate::model::{InsightTarget, SampleSource, TableColumnView, TableProfileView};

    fn table_target(name: &str) -> InsightTarget {
        InsightTarget::SourceTable {
            source: SampleSource::new("conn-1", format!("SELECT * FROM {name}"), "analytics"),
            table_name: name.into(),
        }
    }

    /// 打开目标并确认探查（C3：新源目标先弹确认窗口；测试统一走「确认」驱动）。
    ///
    /// 已登记目标（切 tab / 关 tab）在 `open_target` 内直接取数、不弹窗，
    /// 所以这里只对**首次打开**的目标调用。
    fn open_confirm(
        panel: &gpui_kit::Entity<InsightWorkspacePanel>,
        target: InsightTarget,
        cx: &mut TestAppContext,
    ) {
        panel.update(cx, |p, cx| p.open_target(target, cx));
        panel.update(cx, |p, cx| {
            p.insight_view()
                .update(cx, |v, cx| v.confirm_probe_dialog(cx));
        });
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
                    null_count: None,
                    unique_count: None,
                    null_rate: None,
                    unique_rate: None,
                    extremes: None,
                    stats_detail: None,
                    histogram: None,
                    sample_values: Vec::new(),
                    mini_stats: None,
                    interrupted: false,
                })
                .collect(),
            quality: None,
            progress: None,
            sample: None,
            dup_rows: None,
        }
    }

    #[test]
    fn target_key_is_stable_and_distinguishes_sources() {
        // 同目标两次构造 → 同 key（去重前提）
        assert_eq!(
            target_key(&table_target("orders")),
            target_key(&table_target("orders"))
        );
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
        open_confirm(&panel, table_target("orders"), cx);
        panel.update(cx, |p, cx| {
            p.insight_view().update(cx, |v, cx| {
                v.set_table_profile(profile("orders", 1000, 8), cx)
            });
        });
        open_confirm(&panel, table_target("users"), cx);
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
        // 重复打开同一表 → 已登记（不弹窗、不新增候选）
        panel.update(cx, |p, cx| p.open_target(table_target("orders"), cx));
        panel.update(cx, |p, _| {
            assert_eq!(p.opened().len(), 2, "重复打开同表不应新增候选");
        });
    }

    #[gpui_kit::test]
    fn compare_toggle_and_open_candidate(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let panel = cx.new(InsightWorkspacePanel::new);
        open_confirm(&panel, table_target("orders"), cx);
        open_confirm(&panel, table_target("users"), cx);
        let orders_key = target_key(&table_target("orders"));
        // 勾选 → 选中集变化；再点一次 → 取消
        panel.update(cx, |p, cx| p.toggle_candidate(&orders_key, cx));
        panel.update(cx, |p, _| {
            assert_eq!(p.compare_selected(), &[orders_key.clone()])
        });
        panel.update(cx, |p, cx| p.toggle_candidate(&orders_key, cx));
        panel.update(cx, |p, _| assert!(p.compare_selected().is_empty()));
        // 点候选行 → 切回画像形态并指向该目标
        panel.update(cx, |p, cx| p.open_candidate(&orders_key, cx));
        panel.update(cx, |p, cx| {
            assert_eq!(p.mode(), crate::workspace::WorkspaceMode::Insight);
            assert_eq!(
                p.insight_view().read(cx).target().map(|t| t.title()),
                Some("orders".into())
            );
        });
    }

    #[test]
    fn markdown_report_carries_same_facts_and_escapes_cells() {
        // 出口事实（D36）：报告里出现与渲染一致的表名 / 行数 / 列数与检查器状态
        use crate::model::{ColumnCheckState, TableQualityView};
        let mut p = profile("orders", 1_234, 3);
        p.quality = Some(TableQualityView {
            checker: vec![
                crate::model::ColumnChecker {
                    column: "c0".into(),
                    name: "非空率".into(),
                    state: ColumnCheckState::Hit,
                    detail: "非空率 98.2% ≥ 90%".into(),
                },
                crate::model::ColumnChecker {
                    column: "c0".into(),
                    name: "近似正态".into(),
                    state: ColumnCheckState::Miss,
                    detail: "偏度 2.1（高度右偏）".into(),
                },
            ],
            checker_hits: 1,
            checker_applicable: 2,
        });
        p.columns[0].null_rate = Some(0.3);
        let md = super::render_markdown_report(&p);
        assert!(md.contains("# 洞察报告：orders"), "标题应含表名");
        assert!(md.contains("行数：1,234"), "行数应按 fmt_rows 千分位");
        assert!(md.contains("1/2 命中，其中未命中 1 条"));
        assert!(md.contains("c0 检查器未命中 1 条"), "值得关注应含未命中列");
        assert!(md.contains("c0 缺失率 30%"), "值得关注应含高缺失列");
        // 列明细表头与状态计数（无评分列，只呈现事实统计）
        assert!(md.contains("| 列 | 类型 | 缺失% | 唯一% | 检查器 |"));
        assert!(md.contains("| c0 | INTEGER | 30 | — | 1/2 |"));
        // 转义：含 | 的表名不应串列
        assert_eq!(super::markdown::escape_md("a|b\nc"), "a｜b c");
    }

    #[test]
    fn next_index_after_close_prefers_right_then_falls_back_left() {
        // 关中间的 tab（移除后长度 2、被关 index 1）→ 右侧原位（原右邻顶替 1 位）
        assert_eq!(super::next_index_after_close(2, 1), Some(1));
        // 关最右的 tab（移除后长度 2、被关 index 2 越界）→ 回退左侧末位
        assert_eq!(super::next_index_after_close(2, 2), Some(1));
        // 关第一个 → 原位（新首）
        assert_eq!(super::next_index_after_close(2, 0), Some(0));
        // 最后一个也被关掉 → None（面板回无目标空态）
        assert_eq!(super::next_index_after_close(0, 0), None);
    }

    #[gpui_kit::test]
    fn close_tab_switches_adjacent_and_last_close_empties(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let panel = cx.new(InsightWorkspacePanel::new);
        // 开三张表（最新在前：items, users, orders），激活最新的 items
        for name in ["orders", "users", "items"] {
            open_confirm(&panel, table_target(name), cx);
        }
        panel.update(cx, |p, cx| p.activate_opened(0, cx));
        // 关当前（index 0）→ 应切到相邻（原位 = users）
        panel.update(cx, |p, cx| p.close_opened(0, cx));
        panel.update(cx, |p, cx| {
            assert_eq!(p.opened().len(), 2);
            assert_eq!(
                p.insight_view().read(cx).target().map(|t| t.title()),
                Some("users".into()),
                "关掉当前 tab 应切到相邻目标"
            );
        });
        // 继续关到只剩一个 → 关掉它 → 回无目标空态
        panel.update(cx, |p, cx| p.close_opened(0, cx));
        panel.update(cx, |p, cx| p.close_opened(0, cx));
        panel.update(cx, |p, cx| {
            assert!(p.opened().is_empty());
            assert!(
                p.insight_view().read(cx).target().is_none(),
                "关掉最后一个 tab 面板应回空态"
            );
        });
        // 关闭非当前 tab：只移除、不影响当前目标
        for name in ["orders", "users"] {
            open_confirm(&panel, table_target(name), cx);
        }
        panel.update(cx, |p, cx| p.activate_opened(0, cx)); // 当前 = users
        panel.update(cx, |p, cx| p.close_opened(1, cx)); // 关 orders（非当前）
        panel.update(cx, |p, cx| {
            assert_eq!(p.opened().len(), 1);
            assert_eq!(
                p.insight_view().read(cx).target().map(|t| t.title()),
                Some("users".into()),
                "关非当前 tab 不应动当前目标"
            );
        });
    }

    /// C3（2026-11-02 模态化）：源目标进入洞察**弹出**探查确认窗口（不再内嵌中央区），
    /// 确认后弹层关闭并进入取样。窗口根包 `Root`（`open_dialog` 依赖它挂对话层）。
    #[gpui_kit::test]
    fn source_target_opens_probe_dialog_as_modal(cx: &mut TestAppContext) {
        use gpui_kit::component::{Root, WindowExt as _};

        cx.update(gpui_kit::init);
        let panel = cx.update(|cx| cx.new(InsightWorkspacePanel::new));
        let panel_for_root = panel.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| Root::new(panel_for_root, window, cx));

        cx.update(|window, cx| {
            panel.update(cx, |p, cx| p.open_target(table_target("orders"), cx));
            window.draw(cx).clear(cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "源目标进入洞察应弹出探查确认（模态）"
        );
        // 中央区不再内嵌确认表单：内嵌分支已删（render_insight_content 直接落到
        // 空态引导）；模态遮罩下主树不可观测（find 的可见性语义），故不在此断言主树

        cx.update(|_, cx| {
            panel.update(cx, |p, cx| {
                p.insight_view()
                    .update(cx, |v, cx| v.confirm_probe_dialog(cx));
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            !cx.update(|window, cx| window.has_active_dialog(cx)),
            "确认探查后弹层应关闭并进入取样加载"
        );
    }
}
