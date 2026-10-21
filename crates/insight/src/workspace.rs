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

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui_kit::base::{Disableable as _, StyledExt as _};
// 滚动锚点（值得关注「跳转到该列」）经 `gpui_kit::*` glob 的 trait 面可用：
// `overflow_y_scroll` / `track_scroll` / `anchor_scroll` 在 0.8.0 属
// `StatefulInteractiveElement`，元素必须带稳定 id 才能挂（滚动容器与列卡锚点均已加 id）。
// kit 的 `Scrollable` 组件内部持有自己的 handle，外部 `ScrollAnchor` 挂不上去，
// 因此中间页滚动容器用 gpui 基座滚动而非 `overflow_y_scrollbar`。
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, TabGroup};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::assets::IconName as CatalogIcon;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Size, Sizable as _, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use gpui::{Anchor, ScrollAnchor, ScrollHandle};

use crate::insight_view::InsightView;
use crate::model::{
    fmt_int, fmt_num, fmt_pct, fmt_rows, kurt_direction, normality_judge, skew_direction,
    ColumnCheckState, ColumnChecker, ColumnKind, InsightPanelState, InsightSetEntry,
    InsightSetKind, InsightTarget, MultiResultView, TableColumnView, TableProfileView,
    NULL_RATE_NOTE,
};
use crate::quality_scorer::Grade;
use crate::service::persistence::SOURCE_SAMPLE_LIMIT;
use crate::ui;
use crate::view::{error_block, skeleton};

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

/// 画像摘要：对比聚合的最小事实集（行数 / 列数 / 检查器命中 / 缺失率 / 唯一率 /
/// 值得关注列数），**不重新取数**。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetSummary {
    pub row_count: Option<u64>,
    pub column_count: Option<usize>,
    /// 表级质量分（「评估全表」完成后才有，未评估不是 0 分）
    pub score: Option<f64>,
    pub grade: Option<Grade>,
    /// 检查器命中计数（适用且通过；无评级语义）
    pub checker_hits: Option<usize>,
    pub checker_applicable: Option<usize>,
    /// 表级缺失率均值（0–1；列统计未跑为 `None`）
    pub null_rate: Option<f64>,
    /// 表级唯一率均值（0–1；列统计未跑为 `None`）
    pub unique_rate: Option<f64>,
    /// 值得关注列数（评分低于阈值的列，评估后才有）
    pub attention_columns: Option<usize>,
}

impl TargetSummary {
    fn of_table(profile: &TableProfileView) -> Self {
        let (null_rate, unique_rate) = {
            let (mut null_sum, mut null_n, mut uniq_sum, mut uniq_n) = (0.0, 0usize, 0.0, 0usize);
            for c in &profile.columns {
                if let Some(n) = c.null_rate {
                    null_sum += n;
                    null_n += 1;
                }
                if let Some(u) = c.unique_rate {
                    uniq_sum += u;
                    uniq_n += 1;
                }
            }
            (
                (null_n > 0).then_some(null_sum / null_n as f64),
                (uniq_n > 0).then_some(uniq_sum / uniq_n as f64),
            )
        };
        Self {
            row_count: Some(profile.row_count),
            column_count: Some(profile.columns.len()),
            score: profile.quality.as_ref().map(|q| q.overall),
            grade: profile.quality.as_ref().map(|q| q.grade),
            checker_hits: profile.quality.as_ref().map(|q| q.checker_hits),
            checker_applicable: profile.quality.as_ref().map(|q| q.checker_applicable),
            null_rate,
            unique_rate,
            attention_columns: profile.quality.as_ref().map(|q| q.problem_columns),
        }
    }
}

impl OpenedTarget {
    /// 转成右栏「已洞察表」集合条目。
    ///
    /// 只收**表级**目标（列 / 多列 / Schema 没有独立的集合身份）；
    /// 来源种类判定：源表目标看有没有连接（有 = 源库表，`None` = 文件），
    /// 临时表目标（结果集 / 落库样本）一律按结果集呈现。
    pub(crate) fn to_set_entry(&self, is_current: bool) -> Option<InsightSetEntry> {
        let (kind, source, temp_table) = match &self.target {
            InsightTarget::SourceTable { source, .. } => {
                let kind = if source.conn_id().is_some() {
                    InsightSetKind::SourceTable
                } else {
                    InsightSetKind::File
                };
                (kind, source.label().to_string(), None)
            }
            InsightTarget::Table { temp_table, .. } => (
                InsightSetKind::ResultSet,
                temp_table.clone(),
                Some(temp_table.clone()),
            ),
            _ => return None,
        };
        let (row_count, column_count, checker_hits, checker_applicable) = match self.summary {
            Some(summary) => (
                summary.row_count,
                summary.column_count,
                summary.checker_hits,
                summary.checker_applicable,
            ),
            None => (None, None, None, None),
        };
        Some(InsightSetEntry {
            key: self.key.clone(),
            label: self.target.table_name(),
            kind,
            source,
            temp_table,
            row_count,
            column_count,
            checker_hits,
            checker_applicable,
            is_current,
        })
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
    // ============ EDA 中间页状态（原型 insight-eda-prototype.html） ============
    /// 报告主 Tab（列分布 / 缺失 / 关联性 / 样本）
    eda_tab: EdaTab,
    /// 列分布：列多选（默认全选；200+ 列时用户可缩到几个字段）
    sel_cols: Vec<String>,
    /// 列分布：类型筛选（`None` = 全部）
    kind_filter: Option<ColumnKind>,
    /// 列分布：搜索定位（子串命中列名）
    col_q: String,
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
    /// 列卡四区：只看规则属性（**按列**存；开启后该列展开体只显示检查器列表。
    /// 原型按单列理解——「开哪列管哪列」；空 = 全部未开启）
    col_rule_only: RefCell<HashMap<String, bool>>,
    /// 列卡四区：每列当前区（0=基础统计 / 1=分布 / 2=检查器 / 3=样本）
    col_detail_zone: RefCell<HashMap<String, usize>>,
    /// 中间页滚动句柄（值得关注「跳转到该列」用，见 `render_worth_section`）
    content_scroll: ScrollHandle,
    /// 每列卡滚动锚点：`ScrollAnchor` 需复用同一实例（anchor_scroll 才持续更新
    /// last_origin），同一列渲染时从本表取，不每帧重建
    col_anchors: RefCell<HashMap<String, ScrollAnchor>>,
}

/// 报告主 Tab（原型 .report-tabs：列分布 / 缺失 / 关联性 / 样本）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdaTab {
    #[default]
    Cols,
    Missing,
    Rel,
    Sample,
}

impl EdaTab {
    pub const ALL: [Self; 4] = [Self::Cols, Self::Missing, Self::Rel, Self::Sample];
    pub fn label(self) -> &'static str {
        match self {
            Self::Cols => "列分布",
            Self::Missing => "缺失",
            Self::Rel => "关联性",
            Self::Sample => "样本",
        }
    }
    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap()
    }
    fn of_index(i: usize) -> Self {
        Self::ALL[i.min(3)]
    }
}

/// 列卡展开四区（原型 .cc-zones：基础统计 / 分布 / 检查器 / 样本）。
/// 区状态按列存（`col_detail_zone`），不共用全局区，避免切列串台。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColZone {
    #[default]
    Stats,
    Dist,
    Quality,
    Sample,
}

impl ColZone {
    pub const ALL: [Self; 4] = [Self::Stats, Self::Dist, Self::Quality, Self::Sample];
    pub fn label(self) -> &'static str {
        match self {
            Self::Stats => "基础统计",
            Self::Dist => "分布",
            Self::Quality => "检查器",
            Self::Sample => "样本",
        }
    }
    fn index(self) -> usize {
        Self::ALL.iter().position(|z| *z == self).unwrap()
    }
    fn of_index(i: usize) -> Self {
        Self::ALL[i.min(3)]
    }
}

/// 关联性子 Tab（原型 .rel-tabs：数值相关 / 分类关联 / 值重叠）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RelTab {    #[default]
    Corr,
    Assoc,
    Overlap,
}

impl RelTab {
    pub const ALL: [Self; 3] = [Self::Corr, Self::Assoc, Self::Overlap];
    pub fn label(self) -> &'static str {
        match self {
            Self::Corr => "数值相关",
            Self::Assoc => "分类关联",
            Self::Overlap => "值重叠",
        }
    }
    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap()
    }
    fn of_index(i: usize) -> Self {
        Self::ALL[i.min(2)]
    }
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
            eda_tab: EdaTab::default(),
            sel_cols: Vec::new(),
            kind_filter: None,
            col_q: String::new(),
            rel_tab: RelTab::default(),
            corr_cols: Vec::new(),
            rel_rule: None,
            col_search_subs: RefCell::new(Vec::new()),
            col_open: RefCell::new(HashSet::new()),
            col_rule_only: RefCell::new(HashMap::new()),
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
        let theme = cx.theme().clone();
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
                    .track_scroll(&self.content_scroll)
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
                    .tooltip("重新从源库取样并画像（源目标）")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            // 与 scope 按钮同口径：切回取样口径并重新画像。
                            // 对临时表/结果集目标静默忽略（数据已是既定内容）。
                            entity.update(app, |panel, cx| {
                                panel.view.update(cx, |view, cx| view.request_sample(cx));
                            });
                        }
                    }),
            )
            .child(
                Button::new("insight-ws-export")
                    .ghost()
                    .xsmall()
                    .icon(IconName::FileText)
                    .label("导出")
                    .tooltip("导出画像报告（Markdown）")
                    .disabled(!matches!(
                        *self.view.read(cx).state(),
                        InsightPanelState::Data
                    ))
                    .on_click({
                        let view = self.view.clone();
                        move |_, _, app| {
                            // 报告内容由面板算好（D36：导出「用户看到的这份结论」）；
                            // 事件路径里组装，不占 render。无画像时 title 用于日志。
                            let title = view
                                .read(app)
                                .target()
                                .map(|t| t.title())
                                .unwrap_or_default();
                            let content = view.read(app).data().table.as_ref().map(render_markdown_report);
                            if let Some(content) = content {
                                view.update(app, |v, cx| {
                                    v.request_report_export(content, cx);
                                });
                            } else {
                                tracing::info!("导出报告：无画像数据（{title}）");
                            }
                        }
                    }),
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
            // 分析引擎内存占用（设置「显示内存占用」关闭时整段不渲染）
            .when(self.view.read(cx).status_usage, |bar| {
                bar.child(self.render_memory_usage(cx, theme))
            })
    }

    /// 内存占用只读文本：「内存 X MB / 上限 Y MB · 临时表 N」。
    ///
    /// 读数由后台轮询回填（`InsightView::memory`，5 秒一次）；还没读数时显示
    /// 「内存 —」（不比 0 更有信息量）。上限取分析引擎实际生效值（含设置页改动）。
    fn render_memory_usage(&self, cx: &Context<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        let Some(stats) = self.view.read(cx).memory else {
            return div()
                .flex_none()
                .px_2()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("内存 —");
        };
        let used_mb = stats.database_bytes.unwrap_or(0) / (1024 * 1024);
        let limit_mb = stats.limit_bytes / (1024 * 1024);
        let text = if limit_mb > 0 {
            format!("内存 {used_mb} MB / {limit_mb} MB · 表 {}", stats.table_count)
        } else {
            format!("内存 {used_mb} MB · 表 {}", stats.table_count)
        };
        div()
            .flex_none()
            .px_2()
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(text)
    }

    /// 中间页画像版式（D66）：按载荷分派——错误 / 取样加载 / 骨架 / 表画像 / 列画像 / 空态。
    ///
    /// 注意：与右栏 `InsightView` 的五 Tab 不同，中间页是**原型画像版式**：
    /// 表目标显示四卡 + 质量评分环 + 列类型分布 + 列画像表 + 质量提示；
    /// 列目标复用列画像四区（`render_column_profile`，宽版）。
    fn render_insight_content(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
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
        // 画像数据 clone 到局部（`render_table_overview` 需 `&mut Context` 建
        // `use_keyed_state`，不能再持有 `self.view.read(cx)` 的借用——借用结束在 clone 时）
        if let Some(profile) = self.view.read(cx).data().table.clone() {
            return self.render_table_overview(
                window,
                cx,
                &profile,
                entity,
                &theme,
                inline_icon,
                ring_px,
            );
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

    /// 表画像 EDA 版式（对齐 `insight-eda-prototype.html` 中间页）：
    /// 报告头（kind/scope 徽标 + 取样↔全量切换）→ 概览五卡 → 类型分布 →
    /// 值得关注 → 报告 Tab 四段（列分布 / 缺失 / 关联性 / 样本）。
    ///
    /// 与原型的字段差异（数据模型无 → 用真实字段等价替代，不编假值）：
    /// - 原型「探索价值」分卡 → 概览第五卡「值得关注」列数（缺失 / 高基数现算）；
    /// - 原型列卡「直方图 / 极值」→ 列卡折叠态统计行（类型 / 缺失 / 唯一 / 评分）；
    /// - 原型关联矩阵 → 关联性子 Tab（数值 / 分类走多列规则执行，值重叠走右栏集合）。
    fn render_table_overview(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
        ring_px: Pixels,
    ) -> Div {
        div()
            .v_flex()
            .w_full()
            .gap_2()
            .p(rems(ui::PANEL_PADDING))
            .child(self.render_report_head(cx, profile, entity, theme))
            .child(self.render_overview_cards(profile, theme))
            .child(self.render_type_bar(profile, theme))
            .child(self.render_worth_section(profile, &entity, theme))
            .child(self.render_eda_tabs(cx, entity, theme))
            .child(self.render_eda_body(
                window,
                cx,
                profile,
                entity,
                &theme,
                inline_icon,
                ring_px,
            ))
    }

    /// 报告头（原型 .report-head）：kind 徽标 + scope 徽标 + 取样↔全量切换 + 生成报告。
    fn render_report_head(
        &self,
        cx: &Context<Self>,
        _profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let scope = self.view.read(cx).scope();
        let is_full = scope == crate::insight_view::InsightScope::Full;
        // 取样↔全量切换：源目标才有「取样」概念；结果集 / 临时表目标天然全量（按钮置灰）
        let can_switch = matches!(
            self.view.read(cx).target(),
            Some(InsightTarget::SourceTable { .. }) | Some(InsightTarget::SourceColumn { .. })
        );
        div()
            .h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .py_0p5()
                    .rounded_sm()
                    .border_1()
                    .border_color(colors.border)
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("表画像"),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_1()
                    .py_0p5()
                    .rounded_sm()
                    .bg(colors.sidebar)
                    .border_1()
                    .border_color(colors.border)
                    .text_xs()
                    .text_color(if is_full {
                        colors.info
                    } else {
                        colors.muted_foreground
                    })
                    .child(scope.label()),
            )
            .child(div().flex_1().min_w_0())
            .when(can_switch, |this| {
                let entity = entity.clone();
                let go_full = !is_full;
                this.child(
                    Button::new("insight-ws-scope")
                        .ghost()
                        .xsmall()
                        .icon(if go_full { CatalogIcon::Expand } else { CatalogIcon::Shrink })
                        .label(if go_full { "全量统计" } else { "取样统计" })
                        .tooltip(if go_full {
                            "大表默认取样 500 行；全量需手动触发（可能较慢）"
                        } else {
                            "切回取样口径（500 行）"
                        })
                        .on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| {
                                if go_full {
                                    panel.view.update(cx, |view, cx| view.request_full(cx));
                                } else {
                                    panel.view.update(cx, |view, cx| view.request_sample(cx));
                                }
                            });
                        }),
                )
            })
    }

    /// 概览五卡（原型 .overview-cards）：总行数 / 列数 / 缺失率 / 唯一率 / 值得关注。
    ///
    /// 缺失率与唯一率是**有统计摘要的列**的均值（无摘要列不参与，避免 0 假装没缺失）；
    /// 值得关注 = 缺失 > 20% 或（唯一率 > 0.9 且非主键）或类型未识别的列数。
    fn render_overview_cards(&self, profile: &TableProfileView, theme: &Theme) -> Div {
        let columns = &profile.columns;
        let total = columns.len().max(1);
        let with_stats = columns.iter().filter(|c| c.null_rate.is_some()).count();
        let missing_avg = if with_stats > 0 {
            columns
                .iter()
                .filter_map(|c| c.null_rate)
                .sum::<f64>()
                / with_stats as f64
        } else {
            0.0
        };
        let unique_avg = if with_stats > 0 {
            columns
                .iter()
                .filter_map(|c| c.unique_rate)
                .sum::<f64>()
                / with_stats as f64
        } else {
            0.0
        };
        let worth = columns
            .iter()
            .filter(|c| {
                c.null_rate.map(|n| n > 0.2).unwrap_or(false)
                    || (!c.primary_key && c.unique_rate.map(|u| u > 0.9).unwrap_or(false))
                    || c.kind == ColumnKind::Unknown
            })
            .count();
        div()
            .w_full()
            .flex()
            .items_stretch()
            .gap_2()
            .child(summary_card(
                &theme,
                "总行数",
                fmt_rows(profile.row_count),
                "当前口径行数",
            ))
            .child(summary_card(theme, "列数", total.to_string(), "含主键 / 索引"))
            .child(summary_card(
                &theme,
                "缺失率",
                format!("{:.1}%", missing_avg * 100.0),
                "有统计列的均值",
            ))
            .child(summary_card(
                &theme,
                "唯一率",
                format!("{:.1}%", unique_avg * 100.0),
                "有统计列的均值",
            ))
            .child(summary_card(
                &theme,
                "值得关注",
                format!("{worth} / {total}"),
                "缺失 / 高基数 / 未识别",
            ))
    }

    /// 类型分布横向条（原型 .type-bar + 图例）：按类型族计数铺色条。
    fn render_type_bar(&self, profile: &TableProfileView, theme: &Theme) -> Div {
        let colors = theme.colors;
        let total = profile.columns.len().max(1);
        let counts: Vec<(ColumnKind, usize)> = [
            ColumnKind::Numeric,
            ColumnKind::Text,
            ColumnKind::DateTime,
            ColumnKind::Boolean,
            ColumnKind::Unknown,
        ]
        .into_iter()
        .map(|kind| (kind, profile.columns.iter().filter(|c| c.kind == kind).count()))
        .collect();
        let bar_colors: Vec<Hsla> = vec![
            colors.success,
            colors.info,
            colors.warning,
            colors.primary,
            colors.muted_foreground,
        ];
        let mut segs = div().w_full().h_1().flex().overflow_hidden().rounded_full();
        for (ix, (_kind, count)) in counts.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let w_ratio = (*count as f32 / total as f32).max(0.01);
            // 类型占比条：相对父宽（`w(relative(..))`），不引入绝对像素
            segs = segs.child(
                div()
                    .h_full()
                    .w(relative(w_ratio))
                    .bg(bar_colors[ix]),
            );
        }
        let mut legend = div().h_flex().w_full().items_center().gap_3();
        for (ix, (kind, count)) in counts.iter().enumerate() {
            legend = legend.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(div().w_2().h_2().rounded_sm().bg(bar_colors[ix]))
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{} {}", kind.label(), count)),
                    ),
            );
        }
        div().v_flex().w_full().gap_1().child(segs).child(legend)
    }

    /// 值得关注区（原型 .worth-section）：缺失 / 高基数 / 未识别 / 检查器未命中，
    /// 点击行**展开该列卡并滚动定位**（原型「点击跳转到该列」）。
    fn render_worth_section(
        &self,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let mut lines = div().v_flex().w_full().gap_1();
        let mut added = 0;
        // 检查器未命中（评估后才有）：按未命中条数降序，提示列质量问题
        if let Some(quality) = &profile.quality {
            let mut by_col: Vec<(&str, usize)> = Vec::new();
            for check in &quality.checker {
                if check.state == ColumnCheckState::Miss {
                    if let Some((_, n)) = by_col.iter_mut().find(|(c, _)| *c == check.column.as_str()) {
                        *n += 1;
                    } else {
                        by_col.push((check.column.as_str(), 1));
                    }
                }
            }
            by_col.sort_by(|a, b| b.1.cmp(&a.1));
            for (column, n) in by_col.iter().take(3) {
                lines = lines.child(self.render_worth_row(
                    entity,
                    IconName::CircleAlert,
                    format!("{column} 检查器未命中 {n} 条"),
                    column,
                    theme,
                ));
                added += 1;
            }
        }
        // 缺失 > 20% 的列（按缺失率降序）
        let mut missing: Vec<&TableColumnView> = profile
            .columns
            .iter()
            .filter(|c| c.null_rate.map(|n| n > 0.2).unwrap_or(false))
            .collect();
        missing.sort_by(|a, b| b.null_rate.partial_cmp(&a.null_rate).unwrap_or(std::cmp::Ordering::Equal));
        for col in missing.iter().take(3) {
            lines = lines.child(self.render_worth_row(
                entity,
                IconName::CircleAlert,
                format!(
                    "{} 缺失率 {:.0}%",
                    col.name,
                    col.null_rate.unwrap_or(0.0) * 100.0
                ),
                &col.name,
                theme,
            ));
            added += 1;
        }
        // 高基数（唯一率 > 0.9，非主键）
        let high_card: Vec<&TableColumnView> = profile
            .columns
            .iter()
            .filter(|c| !c.primary_key && c.unique_rate.map(|u| u > 0.9).unwrap_or(false))
            .collect();
        for col in high_card.iter().take(3) {
            lines = lines.child(self.render_worth_row(
                entity,
                IconName::FileText,
                format!("{} 高基数（唯一率 {:.0}%）", col.name, col.unique_rate.unwrap_or(0.0) * 100.0),
                &col.name,
                theme,
            ));
            added += 1;
        }
        // 未识别类型
        let unknown: Vec<&TableColumnView> = profile
            .columns
            .iter()
            .filter(|c| c.kind == ColumnKind::Unknown)
            .collect();
        for col in unknown.iter().take(3) {
            lines = lines.child(self.render_worth_row(
                entity,
                IconName::EyeOff,
                format!("{} 类型未识别（{}）", col.name, col.data_type),
                &col.name,
                theme,
            ));
            added += 1;
        }
        if added == 0 {
            lines = lines.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("未发现明显值得关注的列"),
            );
        }
        panel_box(theme, "值得关注", format!("{added} 项"), lines)
    }

    /// 值得关注行：点击展开该列卡 + 滚动定位（`ScrollAnchor::scroll_to` 下一帧生效）。
    ///
    /// 行级导航用 `div.on_click`（数据行，非标准控件）；滚动锚点取渲染时快照，
    /// 若该列不在当前筛选结果里则只展开不滚动（列卡不在树中，滚动无目标）。
    fn render_worth_row(
        &self,
        entity: &Entity<Self>,
        icon: IconName,
        text: String,
        column: &str,
        theme: &Theme,
    ) -> impl IntoElement {
        let colors = theme.colors;
        let anchor = self.col_anchors.borrow().get(column).cloned();
        let entity = entity.clone();
        let column = column.to_string();
        div()
            .id(SharedString::from(format!("insight-ws-worth-{column}")))
            .h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .px_2()
            .py_0p5()
            .cursor_pointer()
            .rounded(theme.radius)
            .hover(|style| style.bg(colors.list_hover))
            .child(Icon::new(icon).with_size(Size::Size(theme.font_size * ui::INSIGHT_INLINE_ICON_SIZE)).text_color(colors.info))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_ellipsis()
                    .text_color(colors.foreground)
                    .child(text),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("›"),
            )
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| {
                    panel.col_open.borrow_mut().insert(column.clone());
                    cx.notify();
                });
                if let Some(anchor) = &anchor {
                    anchor.scroll_to(window, app);
                }
            })
    }

    /// 报告主 Tab（原型 .report-tabs）：列分布 / 缺失 / 关联性 / 样本。
    fn render_eda_tabs(&self, _cx: &Context<Self>, entity: &Entity<Self>, theme: &Theme) -> Div {
        let current = self.eda_tab.index();
        let tabs = TabBar::new("insight-ws-eda-tabs")
            .underline()
            .selected_index(current)
            .children(EdaTab::ALL.iter().map(|t| Tab::new().label(t.label())))
            .on_click({
                let entity = entity.clone();
                move |index, _, app| {
                    entity.update(app, |panel, cx| {
                        if panel.eda_tab != EdaTab::of_index(*index) {
                            panel.eda_tab = EdaTab::of_index(*index);
                            cx.notify();
                        }
                    });
                }
            });
        div().w_full().border_b_1().border_color(theme.colors.border).child(tabs)
    }

    /// 报告主体：按主 Tab 分派四段内容。
    fn render_eda_body(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
        _ring_px: Pixels,
    ) -> Div {
        match self.eda_tab {
            EdaTab::Cols => self.render_col_section(window, cx, profile, entity, theme),
            EdaTab::Missing => self.render_missing_matrix(profile, theme),
            EdaTab::Rel => self.render_rel_section(cx, profile, entity, theme),
            EdaTab::Sample => self.render_sample_section(profile, theme, inline_icon),
        }
    }

    /// 列分布段：字段筛选器（搜索 + 类型 + 列多选）+ 列卡列表。
    fn render_col_section(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let columns = &profile.columns;
        // 显式勾选为空 = 全选（避免与列集同步的簿记；200+ 列默认全看）
        let selected: Vec<&TableColumnView> = columns
            .iter()
            .filter(|c| self.sel_cols.is_empty() || self.sel_cols.contains(&c.name))
            .filter(|c| {
                self.kind_filter.map(|k| c.kind == k).unwrap_or(true)
                    && (self.col_q.is_empty() || c.name.contains(&self.col_q))
            })
            .collect();
        let mut cards = div().v_flex().w_full().gap_1();
        for col in &selected {
            // 滚动锚点注册：复用同一 `ScrollAnchor` 实例（anchor_scroll 靠它持续
            // 更新 last_origin；每帧重建会让滚动位置永远停在原处）
            self.col_anchors
                .borrow_mut()
                .entry(col.name.clone())
                .or_insert_with(|| ScrollAnchor::for_handle(self.content_scroll.clone()));
            let anchor = self.col_anchors.borrow().get(&col.name).cloned();
            let col_name = col.name.clone();
            cards = cards.child(
                div()
                    // anchor_scroll 在 `StatefulInteractiveElement` 上（0.8.0
                    // gpui-fast div.rs:1557），元素必须带 id 才能挂锚点
                    .id(SharedString::from(format!("insight-ws-col-anchor-{col_name}")))
                    .anchor_scroll(anchor)
                    .child(self.render_col_card(profile, col, entity, theme)),
            );
        }
        if selected.is_empty() {
            cards = cards.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("没有符合筛选条件的列"),
            );
        }
        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(self.render_field_picker(window, cx, profile, entity, theme))
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("显示 {} / {} 列", selected.len(), columns.len())),
            )
            .child(cards)
    }

    /// 列卡（原型 .col-card）：折叠态 = 列名 + 类型徽标 + 探索价值徽标 + 统计行
    /// （缺失率 / 唯一率 / 质量分）；展开态 = 「只看规则属性」开关 + 四区
    /// （基础统计 / 分布 / 检查器 / 样本，对齐原型 `.cc-zones`）。
    fn render_col_card(
        &self,
        profile: &TableProfileView,
        col: &TableColumnView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> impl IntoElement {
        let colors = theme.colors;
        let score = col.score.map(|s| format!("{:.0}", s)).unwrap_or_else(|| "—".into());
        let checker = column_checkers(profile, col);
        let explore = col_explore_score(col, &checker);
        let open = self.col_open.borrow().contains(&col.name);
        let zone = ColZone::of_index(*self.col_detail_zone.borrow().get(&col.name).unwrap_or(&0));
        // 探索价值徽标（原型 .cc-explore）：≥3 高亮「关注」，≥1 平淡显示，<1 不显示
        let explore_badge = if explore >= 3.0 {
            Some(
                div()
                    .flex_none()
                    .text_xs()
                    .px_1()
                    .py_0p5()
                    .rounded(theme.radius)
                    .bg(colors.warning)
                    .text_color(colors.warning_foreground)
                    .child(format!("关注 {explore:.1}")),
            )
        } else if explore >= 1.0 {
            Some(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.warning)
                    .child(format!("探索 {explore:.1}")),
            )
        } else {
            None
        };
        let header = div()
            .h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_ellipsis()
                    .text_color(colors.foreground)
                    .child(col.name.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(col.kind.label()),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "缺失 {}%",
                        col.null_rate.map(|n| format!("{:.0}", n * 100.0)).unwrap_or_else(|| "—".into())
                    )),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "唯一 {}%",
                        col.unique_rate.map(|u| format!("{:.0}", u * 100.0)).unwrap_or_else(|| "—".into())
                    )),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("评分 {score}")),
            )
            .when_some(explore_badge, |this, badge| this.child(badge))
            // 展开切换（语义 Button，键盘可达；原型为整行点击，等价交互）
            .child({
                let entity = entity.clone();
                let name = col.name.clone();
                let toggled = open;
                Button::new(SharedString::from(format!("insight-ws-col-toggle-{}", name)))
                    .ghost()
                    .xsmall()
                    .icon(if toggled { IconName::ChevronDown } else { IconName::ChevronRight })
                    .tooltip(if toggled { "收起" } else { "展开四区（基础统计 / 分布 / 检查器 / 样本）" })
                    .on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            let mut open = panel.col_open.borrow_mut();
                            if toggled {
                                open.remove(&name);
                            } else {
                                open.insert(name.clone());
                            }
                            cx.notify();
                        });
                    })
            });
        let body = self.render_col_card_body(col, &checker, zone, entity, theme);
        Collapsible::new()
            .motion_id(SharedString::from(format!("insight-ws-col-motion-{}", col.name)))
            .open(open)
            .child(header)
            .content(body)
            .w_full()
    }

    /// 列卡展开体：只看规则开关 + （四区 Tab + 区内容 + 检查器汇总）或检查器列表。
    ///
    /// 「只看规则属性」开启后隐藏四区与汇总，只显示检查器列表（对齐原型
    /// `ruleOnly` 语义；开关始终可见，避免原型「开完关不回去」的交互缺陷）。
    fn render_col_card_body(
        &self,
        col: &TableColumnView,
        checker: &[ColumnChecker],
        zone: ColZone,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let rule_toggle = div()
            .h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(
                div().flex_1().min_w_0().child(
                    Switch::new(SharedString::from(format!(
                        "insight-ws-rule-only-{}",
                        col.name
                    )))
                    .checked(self.col_rule_only.borrow().get(&col.name).copied().unwrap_or(false))
                    .label("只看规则属性")
                    .on_click({
                        let entity = entity.clone();
                        let name = col.name.clone();
                        move |_, _, app| {
                            entity.update(app, |panel, cx| {
                                let on = panel
                                    .col_rule_only
                                    .borrow()
                                    .get(&name)
                                    .copied()
                                    .unwrap_or(false);
                                panel
                                    .col_rule_only
                                    .borrow_mut()
                                    .insert(name.clone(), !on);
                                cx.notify();
                            });
                        }
                    }),
                ),
            );
        let rule_only = self.col_rule_only.borrow().get(&col.name).copied().unwrap_or(false);
        if rule_only {
            return div()
                .v_flex()
                .w_full()
                .gap_1()
                .px_1()
                .pb_1()
                .child(rule_toggle)
                .child(self.render_checker_list(col, checker, theme));
        }
        // 四区 Tab（segmented；id 带列名防跨列串台）+ 当前区内容 + 检查器汇总
        let tabs = TabBar::new(SharedString::from(format!("insight-ws-col-zones-{}", col.name)))
            .segmented()
            .selected_index(zone.index())
            .children(ColZone::ALL.iter().map(|z| Tab::new().label(z.label())))
            .on_click({
                let entity = entity.clone();
                let name = col.name.clone();
                move |index, _, app| {
                    entity.update(app, |panel, cx| {
                        panel
                            .col_detail_zone
                            .borrow_mut()
                            .insert(name.clone(), *index);
                        cx.notify();
                    });
                }
            });
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .px_1()
            .pb_1()
            .child(rule_toggle)
            .child(tabs)
            .child(self.render_col_zone(col, checker, zone, theme))
            .child(self.render_checker_summary(col, checker, theme))
    }

    /// 当前区的具体内容（对齐原型 `zoneBody`）。
    fn render_col_zone(
        &self,
        col: &TableColumnView,
        checker: &[ColumnChecker],
        zone: ColZone,
        theme: &Theme,
    ) -> Div {
        match zone {
            ColZone::Stats => self.render_col_stats_zone(col, theme),
            ColZone::Dist => self.render_col_dist_zone(col, theme),
            ColZone::Quality => self.render_checker_list(col, checker, theme),
            ColZone::Sample => self.render_col_sample_zone(col, theme),
        }
    }

    /// 基础统计区：按列类型族给键值行（数据来自评估回填的 `stats_detail`）。
    fn render_col_stats_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        let Some(detail) = &col.stats_detail else {
            return zone_note(theme, "未评估全表，无列级统计（点工具栏「评估」）");
        };
        let rows: Vec<(String, String)> = match detail {
            crate::model::types::ColumnStatsDetail::Numeric(n) => vec![
                ("平均".into(), fmt_num(n.avg)),
                ("中位".into(), fmt_num(n.median)),
                ("最小".into(), fmt_num(n.min)),
                ("最大".into(), fmt_num(n.max)),
                ("P25".into(), fmt_num(n.p25)),
                ("P75".into(), fmt_num(n.p75)),
                ("标准差".into(), n.stddev.map(fmt_num).unwrap_or_else(|| "—".into())),
                ("偏度".into(), n.skewness.map(fmt_num).unwrap_or_else(|| "—".into())),
                ("峰度".into(), n.kurtosis.map(fmt_num).unwrap_or_else(|| "—".into())),
            ],
            crate::model::types::ColumnStatsDetail::Text(t) => vec![
                ("最短长度".into(), fmt_int(t.min_length)),
                ("最长长度".into(), fmt_int(t.max_length)),
                (
                    "Top 值".into(),
                    t.top_values
                        .iter()
                        .take(3)
                        .map(|f| format!("{}（{}）", f.value, f.count))
                        .collect::<Vec<_>>()
                        .join(" · "),
                ),
            ],
            crate::model::types::ColumnStatsDetail::DateTime(d) => vec![
                ("最早".into(), d.earliest.clone()),
                ("最晚".into(), d.latest.clone()),
                ("跨度（天）".into(), fmt_int(d.span_days.max(0) as u32)),
            ],
            crate::model::types::ColumnStatsDetail::Boolean(b) => vec![
                ("真".into(), fmt_int(b.true_count)),
                ("假".into(), fmt_int(b.false_count)),
                ("真占比".into(), fmt_pct(b.true_ratio)),
            ],
            crate::model::types::ColumnStatsDetail::Unknown => vec![("统计".into(), "无可用统计".into())],
        };
        zone_rows(theme, rows)
    }

    /// 分布区：数值列 = 直方图分箱条 + 偏度/峰度 + 正态性判定；文本 = Top 值条；
    /// 时间 = 月度分布；布尔 = 真占比条。数据全部来自 DuckDB 原生统计量。
    fn render_col_dist_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        let colors = theme.colors;
        let mut body = div().v_flex().w_full().gap_1();
        match &col.stats_detail {
            Some(crate::model::types::ColumnStatsDetail::Numeric(n)) => {
                if let Some(bins) = &col.histogram {
                    body = body.child(dist_bars(theme, &bins, 8));
                } else {
                    body = body.child(zone_note(theme, "样本行数不足，未生成直方图"));
                }
                if let (Some(skew), Some(kurt)) = (n.skewness, n.kurtosis) {
                    let mut line = div().text_xs().text_color(colors.muted_foreground);
                    if let Some(judge) = normality_judge(skew, kurt) {
                        line = line.text_color(colors.warning).child(format!(
                            "{judge}：偏度 {} · 峰度 {}",
                            fmt_num(skew),
                            fmt_num(kurt)
                        ));
                    } else {
                        line = line.child(format!(
                            "偏度 {}（{}）· 峰度 {}（{}）",
                            fmt_num(skew),
                            skew_direction(skew),
                            fmt_num(kurt),
                            kurt_direction(kurt)
                        ));
                    }
                    body = body.child(line);
                }
            }
            Some(crate::model::types::ColumnStatsDetail::Text(t)) => {
                body = body.child(dist_text_bars(theme, &t.top_values, 5));
            }
            Some(crate::model::types::ColumnStatsDetail::DateTime(d)) => {
                body = body.child(dist_text_bars(theme, &d.monthly_distribution, 5));
            }
            Some(crate::model::types::ColumnStatsDetail::Boolean(b)) => {
                let bins = vec![
                    crate::model::types::DistributionBin {
                        label: "真".into(),
                        count: b.true_count,
                        ratio: b.true_ratio,
                    },
                    crate::model::types::DistributionBin {
                        label: "假".into(),
                        count: b.false_count,
                        ratio: 1.0 - b.true_ratio,
                    },
                ];
                body = body.child(dist_bars(theme, &bins, 2));
            }
            None => {
                body = body.child(zone_note(theme, "未评估全表，无分布数据"));
            }
            Some(crate::model::types::ColumnStatsDetail::Unknown) => {
                body = body.child(zone_note(theme, "类型未识别，无分布数据"));
            }
        }
        body
    }

    /// 样本区：该列前 5 条样本值（评估回填）。
    fn render_col_sample_zone(&self, col: &TableColumnView, theme: &Theme) -> Div {
        let colors = theme.colors;
        if col.sample_values.is_empty() {
            return zone_note(theme, "无样本值（未评估或列为空）");
        }
        let mut rows = div().v_flex().w_full().gap_1();
        for (ix, value) in col.sample_values.iter().enumerate() {
            rows = rows.child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("#{}", ix + 1)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(if value == "NULL" {
                                colors.muted_foreground
                            } else {
                                colors.foreground
                            })
                            .child(value.clone()),
                    ),
            );
        }
        rows
    }

    /// 检查器命中汇总（原型 749 行）：`检查器 h/t 命中 · n 不适用` + 规则列表
    /// （dashed 分隔，弱化审计心智）。
    fn render_checker_summary(
        &self,
        col: &TableColumnView,
        checker: &[ColumnChecker],
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        if checker.is_empty() {
            return div().text_xs().text_color(colors.muted_foreground)
                .child("该列无检查器属性（未评估全表）");
        }
        let hits = checker.iter().filter(|c| c.state == ColumnCheckState::Hit).count();
        let applicable = checker.iter().filter(|c| c.state != ColumnCheckState::NotApplicableType).count();
        let nas = checker.len() - applicable;
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .border_t_1()
            .border_color(colors.border)
            .pt_1()
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "检查器 {hits}/{applicable} 命中{}",
                        if nas > 0 { format!(" · {nas} 不适用") } else { String::new() }
                    )),
            )
            .child(self.render_checker_list(col, checker, theme))
    }

    /// 检查器四态列表（原型 `ruleList`）：命中绿 / 未命中红 / 不适用灰。
    fn render_checker_list(
        &self,
        _col: &TableColumnView,
        checker: &[ColumnChecker],
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        if checker.is_empty() {
            return div().text_xs().text_color(colors.muted_foreground)
                .child("该列无检查器属性");
        }
        let mut rows = div().v_flex().w_full().gap_1();
        for check in checker {
            let (state_text, color) = match check.state {
                ColumnCheckState::Hit => ("命中", colors.success),
                ColumnCheckState::Miss => ("未命中", colors.danger),
                ColumnCheckState::NotApplicableType => ("不适用", colors.muted_foreground),
                ColumnCheckState::Excluded => ("已排除", colors.muted_foreground),
            };
            rows = rows.child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .w(rems(ui::INSIGHT_WS_GRADE_WIDTH))
                            .text_xs()
                            .text_color(color)
                            .child(state_text),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(colors.foreground)
                            .child(check.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(colors.muted_foreground)
                            .child(check.detail.clone()),
                    ),
            );
        }
        rows
    }

    /// 缺失矩阵（原型 .miss-section）：按缺失率降序的列 × Progress 条 + 百分比。
    fn render_missing_matrix(&self, profile: &TableProfileView, theme: &Theme) -> Div {
        let colors = theme.colors;
        let mut cols: Vec<&TableColumnView> = profile
            .columns
            .iter()
            .filter(|c| c.null_rate.is_some())
            .collect();
        cols.sort_by(|a, b| {
            b.null_rate
                .partial_cmp(&a.null_rate)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut rows = div().v_flex().w_full().gap_1();
        for col in cols.iter().take(12) {
            let ratio = col.null_rate.unwrap_or(0.0) as f32;
            let highlight = ratio > 0.2;
            rows = rows.child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                            .flex_none()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(if highlight { colors.warning } else { colors.muted_foreground })
                            .child(col.name.clone()),
                    )
                    .child(
                        div().flex_1().min_w_0().child(
                            Progress::new(SharedString::from(format!(
                                "insight-ws-miss-{}",
                                col.name
                            )))
                            .value(ratio)
                            .color(if highlight { colors.warning } else { colors.success }),
                        ),
                    )
                    .child(
                        div()
                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                            .flex_none()
                            .text_xs()
                            .text_color(if highlight { colors.warning } else { colors.muted_foreground })
                            .child(format!("{:.0}%", ratio * 100.0)),
                    ),
            );
        }
        let total = profile.columns.len();
        let with_stats = cols.len();
        let note = if with_stats == 0 {
            "列统计摘要不可用（查询失败或表为空）".to_string()
        } else if with_stats < total {
            format!("显示缺失最多的 {} 列（共 {} 列）", with_stats.min(12), total)
        } else {
            format!("共 {total} 列")
        };
        panel_box(theme, "缺失矩阵", note, rows)
    }

    /// 关联性区（原型 .rel-section）：子 Tab 三段——
    /// 数值相关 / 分类关联 = 多列规则执行（列对选择 + 规则选择 + 执行，结果即规则输出）；
    /// 值重叠 = 跨表入口引导（右栏集合卡「关系」是现有跨表值重叠排查）。
    ///
    /// 矩阵化两两相关需要 N×N 规则执行（负载重），后置到多列视图期次（见文档），
    /// 这里先给「选列对 → 跑规则 → 看输出」的科研可用路径。
    fn render_rel_section(
        &self,
        cx: &Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let current = self.rel_tab.index();
        let tabs = TabBar::new("insight-ws-rel-tabs")
            .segmented()
            .selected_index(current)
            .children(RelTab::ALL.iter().map(|t| Tab::new().label(t.label())))
            .on_click({
                let entity = entity.clone();
                move |index, _, app| {
                    entity.update(app, |panel, cx| {
                        if panel.rel_tab != RelTab::of_index(*index) {
                            panel.rel_tab = RelTab::of_index(*index);
                            cx.notify();
                        }
                    });
                }
            });
        let body = match self.rel_tab {
            RelTab::Corr => self.render_rel_corr(cx, profile, entity, theme),
            RelTab::Assoc => self.render_rel_assoc(cx, profile, entity, theme),
            RelTab::Overlap => {
                // 值重叠：右栏集合卡「关系」按钮 = 与当前目标做同名列值重叠排查
                panel_box(
                    &theme,
                    "值重叠",
                    "跨表排查".into(),
                    div()
                        .v_flex()
                        .w_full()
                        .gap_1()
                        .child(note_line(
                            IconName::Info,
                            "与另一张已洞察表的同名列做值重叠排查：在右侧「已洞察表」集合卡点「关系」".into(),
                            &theme,
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child("结果展示在右栏集合卡的关系区（T19 关系引擎，同名列类型族过滤）"),
                        ),
                )
            }
        };
        div().v_flex().w_full().gap_2().child(tabs).child(body)
    }

    /// 数值相关：列对（数值列多选 ≥2）→ 相关规则（Pearson / Spearman）→ 执行。
    ///
    /// 执行走多列规则链路（`MultiRunRequested` → 后台跑 DuckDB 规则 → 结果回填
    /// `PanelData::multi.result`），结果按规则输出形态渲染（键值行）。
    fn render_rel_corr(
        &self,
        cx: &Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let numeric: Vec<String> = profile
            .columns
            .iter()
            .filter(|c| c.kind == ColumnKind::Numeric)
            .map(|c| c.name.clone())
            .collect();
        let selected: Vec<String> = self
            .corr_cols
            .iter()
            .filter(|n| numeric.contains(n))
            .cloned()
            .collect();
        // 可用的相关规则（多列清单里按名字识别；未加载时给占位引导）
        let multi = self.view.read(cx).data().multi.clone();
        let rule = self.rel_rule.clone();
        let rules_ready = multi.as_ref().map(|m| !m.rules.is_empty()).unwrap_or(false);
        // 当前规则名（构建时快照，浮层内容闭包用）
        let rule_name = multi
            .as_ref()
            .and_then(|m| {
                m.rules.iter().find(|r| {
                    Some(&r.id) == rule.as_ref()
                        && (r.id.contains("correlation") || r.id.contains("spearman"))
                })
            })
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "Pearson 相关".into());
        let can_run = selected.len() >= 2 && rule.is_some();
        // 规则清单快照：浮层闭包 move 用（`multi` 本身留给结果渲染 when_some）
        let rules = multi.as_ref().map(|m| m.rules.clone()).unwrap_or_default();

        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    // 列对选择：Popover 多选（Checkbox）
                    .child({
                        let entity = entity.clone();
                        let current = selected.clone();
                        Popover::new("insight-ws-corr-cols")
                            .anchor(Anchor::TopRight)
                            .trigger(
                                Button::new("insight-ws-corr-cols-btn")
                                    .ghost()
                                    .xsmall()
                                    .icon(CatalogIcon::ListFilter)
                                    .label(format!("列对 {}/{}", selected.len(), numeric.len()))
                                    .tooltip("选 2 个以上数值列算相关"),
                            )
                            .content(move |_state, _window, cx| {
                                let colors = cx.theme().colors;
                                let entity = entity.clone();
                                let mut body = div()
                                    .v_flex()
                                    .w(rems(ui::INSIGHT_PICKER_WIDTH))
                                    .gap_1()
                                    .px_2()
                                    .py_2();
                                for name in &numeric {
                                    let entity = entity.clone();
                                    let name = name.clone();
                                    let label = name.clone();
                                    let checked = current.contains(&name);
                                    body = body.child(
                                        div()
                                            .h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                Checkbox::new(SharedString::from(format!(
                                                    "insight-ws-corr-{name}"
                                                )))
                                                .checked(checked)
                                                .on_click(move |_, _, app| {
                                                    entity.update(app, |panel, cx| {
                                                        if let Some(pos) = panel
                                                            .corr_cols
                                                            .iter()
                                                            .position(|n| n == &name)
                                                        {
                                                            panel.corr_cols.remove(pos);
                                                        } else {
                                                            panel.corr_cols.push(name.clone());
                                                        }
                                                        cx.notify();
                                                    });
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.foreground)
                                                    .child(label),
                                            ),
                                    );
                                }
                                body
                            })
                    })
                    // 规则选择：下拉（Pearson / Spearman；Kendall τ DuckDB 无直接函数，后置）
                    .child({
                        let entity = entity.clone();
                        let rule_name = rule_name.clone();
                        Button::new("insight-ws-corr-rule")
                            .ghost()
                            .xsmall()
                            .icon(CatalogIcon::Sigma)
                            .label(rule_name)
                            .tooltip("相关方法（DuckDB 原生可算的统计量）")
                            .dropdown_menu(move |menu, _, _| {
                                let mut menu = menu;
                                for r in rules.iter().filter(|r| {
                                    r.id.contains("correlation") || r.id.contains("spearman")
                                }) {
                                    let entity = entity.clone();
                                    let id = r.id.clone();
                                    let name = r.name.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(name)
                                            .checked(Some(&id) == rule.as_ref())
                                            .on_click(move |_, _, app| {
                                                entity.update(app, |panel, cx| {
                                                    panel.rel_rule = Some(id.clone());
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }
                                menu
                            })
                    })
                    // 执行：先确保多列规则清单已加载（未加载时点执行 = 请求清单）
                    .child(
                        Button::new("insight-ws-corr-run")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Play)
                            .label("计算")
                            .disabled(!can_run)
                            .tooltip(if selected.len() < 2 {
                                "至少选 2 个数值列"
                            } else {
                                "执行所选相关规则"
                            })
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        let Some(rule_id) = panel.rel_rule.clone() else {
                                            return;
                                        };
                                        if !rules_ready {
                                            // 规则清单未加载：先请求清单，用户在下一轮点执行
                                            panel.request_multi_rules(cx);
                                            return;
                                        }
                                        if let Some(temp_table) = panel
                                            .view
                                            .read(cx)
                                            .target()
                                            .and_then(|t| panel.view.read(cx).data().sample_table(t))
                                            .map(|s| s.to_string())
                                        {
                                            let columns = panel.corr_cols.clone();
                                            panel.view.update(cx, |_view, cx| {
                                                cx.emit(crate::insight_view::InsightEvent::MultiRunRequested {
                                                    temp_table,
                                                    rule_id,
                                                    columns,
                                                });
                                            });
                                        }
                                    });
                                }
                            }),
                    ),
            )
            // 结果：多列规则输出（键值行或表格）
            .when_some(multi.as_ref().and_then(|m| m.result.clone()), |this, result| {
                this.child(render_multi_result(&result, theme))
            })
            .when_some(
                if rules_ready { None } else { Some(()) },
                |this, ()| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("多列规则清单未加载：点「计算」会先请求清单（需已进入多列视图）"),
                    )
                },
            )
    }

    /// 分类关联：两列文本/分类 → Cramér's V（DuckDB 原生卡方现算，规则见
    /// `insight-rules/multi/cramer-v.rule.toml`）。
    fn render_rel_assoc(
        &self,
        cx: &Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let text_cols: Vec<String> = profile
            .columns
            .iter()
            .filter(|c| c.kind == ColumnKind::Text || c.kind == ColumnKind::Boolean)
            .map(|c| c.name.clone())
            .collect();
        let selected: Vec<String> = self
            .corr_cols
            .iter()
            .filter(|n| text_cols.contains(n))
            .cloned()
            .collect();
        let multi = self.view.read(cx).data().multi.clone();
        let rules_ready = multi.as_ref().map(|m| !m.rules.is_empty()).unwrap_or(false);
        let has_cramer = multi
            .as_ref()
            .map(|m| m.rules.iter().any(|r| r.id.contains("cramer")))
            .unwrap_or(false);
        let can_run = selected.len() >= 2 && has_cramer;
        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child({
                        let entity = entity.clone();
                        let current = selected.clone();
                        Popover::new("insight-ws-assoc-cols")
                            .anchor(Anchor::TopRight)
                            .trigger(
                                Button::new("insight-ws-assoc-cols-btn")
                                    .ghost()
                                    .xsmall()
                                    .icon(CatalogIcon::ListFilter)
                                    .label(format!("列对 {}/{}", selected.len(), text_cols.len()))
                                    .tooltip("选 2 个文本/分类列算关联"),
                            )
                            .content(move |_state, _window, cx| {
                                let colors = cx.theme().colors;
                                let entity = entity.clone();
                                let mut body = div()
                                    .v_flex()
                                    .w(rems(ui::INSIGHT_PICKER_WIDTH))
                                    .gap_1()
                                    .px_2()
                                    .py_2();
                                for name in &text_cols {
                                    let entity = entity.clone();
                                    let name = name.clone();
                                    let label = name.clone();
                                    let checked = current.contains(&name);
                                    body = body.child(
                                        div()
                                            .h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                Checkbox::new(SharedString::from(format!(
                                                    "insight-ws-assoc-{name}"
                                                )))
                                                .checked(checked)
                                                .on_click(move |_, _, app| {
                                                    entity.update(app, |panel, cx| {
                                                        if let Some(pos) = panel
                                                            .corr_cols
                                                            .iter()
                                                            .position(|n| n == &name)
                                                        {
                                                            panel.corr_cols.remove(pos);
                                                        } else {
                                                            panel.corr_cols.push(name.clone());
                                                        }
                                                        cx.notify();
                                                    });
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.foreground)
                                                    .child(label),
                                            ),
                                    );
                                }
                                body
                            })
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("Cramér's V（卡方独立性）"),
                    )
                    .child(
                        Button::new("insight-ws-assoc-run")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Play)
                            .label("计算")
                            .disabled(!can_run)
                            .tooltip(if selected.len() < 2 {
                                "至少选 2 个文本/分类列"
                            } else {
                                "执行 Cramér's V"
                            })
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        if !rules_ready {
                                            panel.request_multi_rules(cx);
                                            return;
                                        }
                                        let Some(rule_id) = panel
                                            .view
                                            .read(cx)
                                            .data()
                                            .multi
                                            .as_ref()
                                            .and_then(|m| {
                                                m.rules
                                                    .iter()
                                                    .find(|r| r.id.contains("cramer"))
                                                    .map(|r| r.id.clone())
                                            })
                                        else {
                                            return;
                                        };
                                        if let Some(temp_table) = panel
                                            .view
                                            .read(cx)
                                            .target()
                                            .and_then(|t| panel.view.read(cx).data().sample_table(t))
                                            .map(|s| s.to_string())
                                        {
                                            let columns = panel.corr_cols.clone();
                                            panel.view.update(cx, |_view, cx| {
                                                cx.emit(crate::insight_view::InsightEvent::MultiRunRequested {
                                                    temp_table,
                                                    rule_id,
                                                    columns,
                                                });
                                            });
                                        }
                                    });
                                }
                            }),
                    ),
            )
            .when_some(multi.as_ref().and_then(|m| m.result.clone()), |this, result| {
                this.child(render_multi_result(&result, theme))
            })
            .when_some(
                if rules_ready { None } else { Some(()) },
                |this, ()| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("多列规则清单未加载：点「计算」会先请求清单（需已进入多列视图）"),
                    )
                },
            )
    }

    /// 请求多列规则清单（关联性子 Tab 的执行前置：清单未加载时先拉一次）
    fn request_multi_rules(&self, cx: &mut Context<Self>) {
        let Some(target) = self.view.read(cx).target().cloned() else {
            return;
        };
        let Some(temp_table) = self.view.read(cx).data().sample_table(&target).map(|s| s.to_string())
        else {
            return;
        };
        let table_name = target.table_name().to_string();
        self.view.update(cx, |_view, cx| {
            cx.emit(crate::insight_view::InsightEvent::MultiColumnRequested {
                temp_table,
                table_name,
            });
        });
    }

    /// 样本区（原型 .sample-section）：前 5 行表格（列名 + 文本化值）。
    fn render_sample_section(
        &self,
        profile: &TableProfileView,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let Some(sample) = &profile.sample else {
            return panel_box(
                &theme,
                "样本",
                "前 5 行".into(),
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(CatalogIcon::Table).with_size(Size::Size(inline_icon)).text_color(colors.muted_foreground))
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("样本不可用（临时表为空或已过期）"),
                    ),
            );
        };
        if sample.rows.is_empty() {
            return panel_box(
                &theme,
                "样本",
                "前 5 行".into(),
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("表为空，没有可展示的样本行"),
            );
        }
        // 列头
        let mut head = div().h_flex().w_full().gap_2().px_2().py_1();
        for (ix, name) in sample.columns.iter().enumerate() {
            head = head.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_ellipsis()
                    .text_color(colors.foreground)
                    .child(name.clone())
                    .when(ix == 0, |this| this.w(rems(ui::INSIGHT_WS_SOURCE_WIDTH)).flex_none()),
            );
        }
        let mut body = div().v_flex().w_full();
        for row in &sample.rows {
            let mut line = div().h_flex().w_full().gap_2().px_2().py_1().border_t_1().border_color(colors.border);
            for (ix, cell) in row.iter().enumerate() {
                line = line.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(if cell == "NULL" { colors.muted_foreground } else { colors.foreground })
                        .child(cell.clone())
                        .when(ix == 0, |this| this.w(rems(ui::INSIGHT_WS_SOURCE_WIDTH)).flex_none()),
                );
            }
            body = body.child(line);
        }
        panel_box(
            theme,
            "样本",
            format!("前 {} 行（共 {} 行）", sample.rows.len(), fmt_rows(profile.row_count)),
            div().v_flex().w_full().child(head).child(body),
        )
    }

    /// 字段筛选器（原型 .field-picker）：搜索定位 + 类型筛选 + 列多选（200+ 列场景）。
    ///
    /// 组件选型（禁手搓）：搜索 = `Input`（`use_keyed_state` 惰性创建）；类型 =
    /// `Button::dropdown_menu`（`PopupMenuItem::checked`，与目标切换同款）；列多选 =
    /// `Popover` + `Checkbox` 逐列（原生浮层与键盘可达，不做自绘弹窗）。
    fn render_field_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        _theme: &Theme,
    ) -> Div {
        let columns = &profile.columns;
        let sel_len = if self.sel_cols.is_empty() {
            columns.len()
        } else {
            self.sel_cols.len()
        };
        let col_names: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
        let kind_items: Vec<(&'static str, Option<ColumnKind>)> = vec![
            ("全部", None),
            ("数值", Some(ColumnKind::Numeric)),
            ("文本", Some(ColumnKind::Text)),
            ("时间", Some(ColumnKind::DateTime)),
            ("布尔", Some(ColumnKind::Boolean)),
            ("未识别", Some(ColumnKind::Unknown)),
        ];
        // 搜索输入：渲染管线惰性创建（元素消失自动回收，同 property_panel 的常驻
        // SelectState 模式）；订阅只挂一次，Change → 写面板过滤词（只读镜像，
        // 输入权威仍在 InputState）。
        let search_state = window.use_keyed_state(
            SharedString::from("insight-ws-col-search"),
            cx,
            |window, cx| InputState::new(window, cx).placeholder("搜索列名"),
        );
        if self.col_search_subs.borrow().is_empty() {
            let sub = cx.subscribe_in(
                &search_state,
                window,
                |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    this.col_q = emitter.read(cx).value().to_string();
                    cx.notify();
                },
            );
            self.col_search_subs.borrow_mut().push(sub);
        }
        let kind_label = kind_items
            .iter()
            .find(|(_, k)| *k == self.kind_filter)
            .map(|(label, _)| *label)
            .unwrap_or("全部");

        div()
            .h_flex()
            .w_full()
            .items_center()
            .gap_2()
            // 搜索定位
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&search_state).cleanable(true)),
            )
            // 类型筛选：下拉（PopupMenuItem 自带选中态与键盘；闭包捕获构建时快照，
            // Popover/DropdownMenu 的 content 在每次 render 重建，新值自然生效）
            .child({
                let entity = entity.clone();
                let kind_now = self.kind_filter;
                Button::new("insight-ws-kind-filter")
                    .ghost()
                    .xsmall()
                    .icon(CatalogIcon::ListFilter)
                    .label(kind_label)
                    .tooltip("按类型族筛选列")
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for (label, kind) in &kind_items {
                            let entity = entity.clone();
                            // 解引用为 owned（ColumnKind: Copy）：on_click move 闭包不能捕获 & 迭代引用
                            let kind = *kind;
                            let checked = kind == kind_now;
                            menu = menu.item(
                                PopupMenuItem::new((*label).to_string())
                                    .checked(checked)
                                    .on_click(move |_, _, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.kind_filter = kind;
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        menu
                    })
            })
            // 列多选浮层：Popover（trigger = Button，content = Checkbox 列表）
            .child({
                let entity = entity.clone();
                // 当前显式选集（空 = 全选，此时回显全选）；浮层内容闭包捕获此快照
                let current: Vec<String> = if self.sel_cols.is_empty() {
                    col_names.clone()
                } else {
                    self.sel_cols.clone()
                };
                let all_sel = current.len() == col_names.len();
                Popover::new("insight-ws-col-pick")
                    .anchor(Anchor::TopRight)
                    .trigger(
                        Button::new("insight-ws-col-pick-btn")
                            .ghost()
                            .xsmall()
                            .icon(CatalogIcon::ListFilter)
                            .label(format!("列 {sel_len}/{}", columns.len()))
                            .tooltip("选择要看的列（200+ 列时建议缩到几个字段）"),
                    )
                    .content(move |_state, _window, cx| {
                        let colors = cx.theme().colors;
                        let all_entity = entity.clone();
                        let none_entity = entity.clone();
                        // 拥有副本：content 闭包 move 捕获 col_names 后，for 迭代与
                        // 子闭包各持一份，避免「Fn 闭包 move 后外部再借」冲突
                        let names = col_names.clone();
                        let all_names = names.clone();
                        // 反选按钮专属副本：其 on_click move 捕获后不挤占循环的 all_names
                        let none_names = all_names.clone();
                        let mut body = div()
                            .v_flex()
                            .w(rems(ui::INSIGHT_PICKER_WIDTH))
                            .gap_1()
                            .px_2()
                            .py_2()
                            .child(
                                div()
                                    .h_flex()
                                    .gap_2()
                                    .child(
                                        Button::new("insight-ws-col-all")
                                            .ghost()
                                            .xsmall()
                                            .disabled(all_sel)
                                            .label("全选")
                                            .on_click(move |_, _, app| {
                                                all_entity.update(app, |panel, cx| {
                                                    panel.sel_cols.clear();
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                    .child(
                                        Button::new("insight-ws-col-none")
                                            .ghost()
                                            .xsmall()
                                            .label("反选")
                                            .on_click(move |_, _, app| {
                                                none_entity.update(app, |panel, cx| {
                                                    // 显式清单 = 全集 − 当前（结果为空时退回全选语义，
                                                    // 避免「空 = 未筛选」歧义）
                                                    let names = none_names.clone();
                                                    let cur = panel.sel_cols.clone();
                                                    let next: Vec<String> = names
                                                        .iter()
                                                        .filter(|n| !cur.contains(n))
                                                        .map(|n| n.to_string())
                                                        .collect();
                                                    panel.sel_cols = if next.is_empty() {
                                                        Vec::new()
                                                    } else {
                                                        next
                                                    };
                                                    cx.notify();
                                                });
                                            }),
                                    ),
                            )
                            .child(div().w_full().h_px().bg(colors.border));
                        for name in &names {
                            let entity = entity.clone();
                            let name = name.clone();
                            let label = name.clone();
                            let all = all_names.clone();
                            let checked = current.contains(&name);
                            body = body.child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Checkbox::new(SharedString::from(format!(
                                            "insight-ws-col-{name}"
                                        )))
                                        .checked(checked)
                                        .on_click(move |_, _, app| {
                                            entity.update(app, |panel, cx| {
                                                // 首次点选时把「隐式全选」物化为显式清单
                                                if panel.sel_cols.is_empty() {
                                                    panel.sel_cols = all.clone();
                                                }
                                                if let Some(pos) = panel
                                                    .sel_cols
                                                    .iter()
                                                    .position(|n| n == &name)
                                                {
                                                    panel.sel_cols.remove(pos);
                                                } else {
                                                    panel.sel_cols.push(name.clone());
                                                }
                                                cx.notify();
                                            });
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(colors.foreground)
                                            .child(label),
                                    ),
                            );
                        }
                        body
                    })
            })
    }

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
                                    .child("对比为列表式汇总：行数 / 列数 / 检查器命中 / 缺失率 / 唯一率 / 值得关注列数（无评级，评级在归档后的资产库）；点某行可在中间打开该表洞察工作页。"),
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
                                            .child("检查器"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SCORE_WIDTH))
                                            .flex_none()
                                            .child("缺失率"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SCORE_WIDTH))
                                            .flex_none()
                                            .child("唯一率"),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .child("值得关注"),
                                    ),
                            )
                            .children(selected.into_iter().map(|entry| {
                                let s = entry.summary;
                                // 检查器命中：事实计数着色（全中=success / 全不中=danger / 未评估=muted）
                                let (checker_text, checker_color) =
                                    match (s.and_then(|s| s.checker_applicable), s.and_then(|s| s.checker_hits)) {
                                        (Some(applicable), Some(hits)) if applicable > 0 => (
                                            format!("{hits}/{applicable}"),
                                            if hits == applicable {
                                                colors.success
                                            } else if hits == 0 {
                                                colors.danger
                                            } else {
                                                colors.warning
                                            },
                                        ),
                                        _ => ("—".into(), colors.muted_foreground),
                                    };
                                // 缺失率 / 唯一率：均值超经验阈值标 warning
                                let miss_color = s
                                    .and_then(|s| s.null_rate)
                                    .map(|r| if r > NULL_RATE_NOTE { colors.warning } else { colors.muted_foreground })
                                    .unwrap_or(colors.muted_foreground);
                                let uniq_color = s
                                    .and_then(|s| s.unique_rate)
                                    .map(|r| if r > crate::quality_scorer::CHECKER_UNIQUE_MAX { colors.warning } else { colors.muted_foreground })
                                    .unwrap_or(colors.muted_foreground);
                                let attention_color = s
                                    .and_then(|s| s.attention_columns)
                                    .map(|n| if n > 0 { colors.danger } else { colors.muted_foreground })
                                    .unwrap_or(colors.muted_foreground);
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
                                            .text_color(checker_color)
                                            .child(checker_text),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SCORE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(miss_color)
                                            .child(
                                                s.and_then(|s| s.null_rate)
                                                    .map(fmt_pct)
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_SCORE_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(uniq_color)
                                            .child(
                                                s.and_then(|s| s.unique_rate)
                                                    .map(fmt_pct)
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                                            .flex_none()
                                            .text_xs()
                                            .text_color(attention_color)
                                            .child(
                                                s.and_then(|s| s.attention_columns)
                                                    .map(|n| format!("{n} 列"))
                                                    .unwrap_or_else(|| "—".into()),
                                            ),
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

/// 缺失率的探索价值权重（原型 exploreScore 规则：>20% +3、>5% +1.5、>0 +0.3）
fn explore_missing_weight(col: &TableColumnView) -> f64 {
    let Some(rate) = col.null_rate else {
        return 0.0;
    };
    if rate > 0.2 {
        3.0
    } else if rate > 0.05 {
        1.5
    } else if rate > 0.0 {
        0.3
    } else {
        0.0
    }
}

/// 该列检查器（评估后的 `TableQualityView::checker` 按列过滤）。
/// 未评估 / 无检查器 → 空表（不编假值）。
fn column_checkers(profile: &TableProfileView, col: &TableColumnView) -> Vec<ColumnChecker> {
    profile
        .quality
        .as_ref()
        .map(|q| {
            q.checker
                .iter()
                .filter(|c| c.column == col.name)
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// 列探索价值（原型 exploreScore：缺失权重 + 高基数/唯一率≈行数 +2
/// （主键天然高唯一不重复计）+ 未识别类型 +1.5 + 检查器未命中每条 +1.0）。
///
/// 检查器加分系数 1.0 是经验默认（原型「检查器未命中加分」未给系数）：一条未命中
/// ≈ 一条轻度缺失的信号强度（原型缺失权重 +1.5 与 +0.3 之间取中），线性累加使
/// 多规则未命中的列在「值得关注」里自然前置——探索分是**排序工具**（决定值得
/// 关注的先后与列卡徽标 ≥3/≥1 档位），极端值只影响排序不破坏分级，故不封顶。
/// 不适用-类型与排除态不计——它们不代表数据有问题。系数待原型定稿。
fn col_explore_score(col: &TableColumnView, checker: &[ColumnChecker]) -> f64 {
    let mut score = explore_missing_weight(col);
    if col
        .unique_rate
        .map(|u| u > 0.9)
        .unwrap_or(false) && !col.primary_key
    {
        score += 2.0;
    }
    if col.kind == ColumnKind::Unknown {
        score += 1.5;
    }
    score += checker
        .iter()
        .filter(|c| c.state == ColumnCheckState::Miss)
        .count() as f64
        * 1.0;
    score
}

/// 展开区的一行提示（无数据 / 未评估占位）
fn zone_note(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 展开区键值行列表
fn zone_rows(theme: &Theme, rows: Vec<(String, String)>) -> Div {
    let colors = theme.colors;
    let mut body = div().v_flex().w_full().gap_1();
    for (key, value) in rows {
        body = body.child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(key),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(colors.foreground)
                        .child(value),
                ),
        );
    }
    body
}

/// 分布条（直方图 / Top 值 / 月度 / 布尔共用）：label + 条 + 计数。
/// 条用原生 flex 宽度（布局不是控件，不必组件化）；取前 `limit` 条防爆版面。
fn dist_bars(theme: &Theme, bins: &[crate::model::types::DistributionBin], limit: usize) -> Div {    let colors = theme.colors;
    let mut body = div().v_flex().w_full().gap_1();
    for bin in bins.iter().take(limit) {
        body = body.child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                        .text_xs()
                        .text_ellipsis()
                        .text_color(colors.muted_foreground)
                        .child(bin.label.clone()),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        Progress::new(SharedString::from(format!(
                            "insight-ws-dist-{}",
                            bin.label
                        )))
                        .value(bin.ratio.clamp(0.0, 1.0) as f32)
                        .color(colors.primary),
                    ),
                )
                .child(
                    div()
                        .flex_none()
                        .w(rems(ui::INSIGHT_WS_NUM_WIDTH))
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(fmt_int(bin.count)),
                ),
        );
    }
    body
}

/// 导出报告正文（Markdown）：与中间页渲染**同一份事实**（D36——导出的是
/// 用户看到的这份结论，不重算）。概览 → 检查器命中 → 值得关注 → 列明细 →
/// 样本；表格单元格做 `|` 转义，避免 Markdown 表格串列。
fn render_markdown_report(profile: &TableProfileView) -> String {
    let mut md = String::new();
    md.push_str(&format!("# 洞察报告：{}\n\n", profile.table_name));
    md.push_str(&format!(
        "- 来源：{}\n- 行数：{}\n- 列数：{}\n\n",
        profile.source_label,
        fmt_rows(profile.row_count),
        profile.columns.len()
    ));
    // 检查器命中汇总（评估后才有；未评估不写这一节，避免「0 命中」误导）
    if let Some(q) = &profile.quality {
        let hits = q
            .checker
            .iter()
            .filter(|c| c.state == ColumnCheckState::Hit)
            .count();
        let applicable = q
            .checker
            .iter()
            .filter(|c| c.state != ColumnCheckState::NotApplicableType)
            .count();
        let misses = q
            .checker
            .iter()
            .filter(|c| c.state == ColumnCheckState::Miss)
            .count();
        md.push_str(&format!("## 检查器命中\n\n{hits}/{applicable} 命中，其中未命中 {misses} 条。\n\n"));
    }
    // 值得关注：检查器未命中 / 缺失 >20% / 高基数（非主键）/ 类型未识别
    let mut worth: Vec<String> = Vec::new();
    if let Some(q) = &profile.quality {
        let mut by_col: Vec<(&str, usize)> = Vec::new();
        for check in &q.checker {
            if check.state == ColumnCheckState::Miss {
                if let Some((_, n)) = by_col
                    .iter_mut()
                    .find(|(c, _)| *c == check.column.as_str())
                {
                    *n += 1;
                } else {
                    by_col.push((check.column.as_str(), 1));
                }
            }
        }
        by_col.sort_by(|a, b| b.1.cmp(&a.1));
        for (column, n) in by_col.iter().take(3) {
            worth.push(format!("{column} 检查器未命中 {n} 条"));
        }
    }
    for col in profile
        .columns
        .iter()
        .filter(|c| c.null_rate.map(|n| n > 0.2).unwrap_or(false))
        .take(3)
    {
        worth.push(format!(
            "{} 缺失率 {:.0}%",
            col.name,
            col.null_rate.unwrap_or(0.0) * 100.0
        ));
    }
    for col in profile
        .columns
        .iter()
        .filter(|c| !c.primary_key && c.unique_rate.map(|u| u > 0.9).unwrap_or(false))
        .take(3)
    {
        worth.push(format!(
            "{} 高基数（唯一率 {:.0}%）",
            col.name,
            col.unique_rate.unwrap_or(0.0) * 100.0
        ));
    }
    for col in profile
        .columns
        .iter()
        .filter(|c| c.kind == ColumnKind::Unknown)
        .take(3)
    {
        worth.push(format!("{} 类型未识别（{}）", col.name, col.data_type));
    }
    if !worth.is_empty() {
        md.push_str("## 值得关注\n\n");
        for line in worth {
            md.push_str(&format!("- {line}\n"));
        }
        md.push('\n');
    }
    // 列明细表：检查器列给出「命中/未命中/不适用」统计
    md.push_str("## 列明细\n\n| 列 | 类型 | 缺失% | 唯一% | 评分 | 检查器 |\n|---|---|---|---|---|---|\n");
    for col in &profile.columns {
        let checker_text = column_checkers(profile, col)
            .iter()
            .fold((0usize, 0usize), |(hit, miss), c| match c.state {
                ColumnCheckState::Hit => (hit + 1, miss),
                ColumnCheckState::Miss => (hit, miss + 1),
                _ => (hit, miss),
            });
        let checker_cell = if profile.quality.is_some() {
            format!("{}/{}", checker_text.0, checker_text.0 + checker_text.1)
        } else {
            "—".into()
        };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            escape_md(&col.name),
            escape_md(&col.data_type),
            col.null_rate
                .map(|n| format!("{:.0}", n * 100.0))
                .unwrap_or_else(|| "—".into()),
            col.unique_rate
                .map(|u| format!("{:.0}", u * 100.0))
                .unwrap_or_else(|| "—".into()),
            col.score.map(|s| format!("{s:.0}")).unwrap_or_else(|| "—".into()),
            checker_cell,
        ));
    }
    // 样本（前 5 行）
    if let Some(sample) = &profile.sample {
        md.push_str("\n## 样本（前 5 行）\n\n|");
        for c in &sample.columns {
            md.push_str(&format!(" {} |", escape_md(c)));
        }
        md.push('\n');
        for _ in &sample.columns {
            md.push_str("---|");
        }
        md.push('\n');
        for row in &sample.rows {
            md.push('|');
            for v in row {
                md.push_str(&format!(" {} |", escape_md(v)));
            }
            md.push('\n');
        }
        md.push('\n');
    }
    md
}

/// Markdown 转义：`|` 换成全角竖线（表格 cell 不串列），换行改空格。
fn escape_md(text: &str) -> String {
    text.replace('|', "｜")
        .replace(['\r', '\n'], " ")
}

/// 文本 / 时间分布条（Top 值、月度分布）：把 `TextFrequency` 转成占比条。
/// 条比例 = 该项计数 / 总计数（占比语义与数值直方图一致）。
fn dist_text_bars(
    theme: &Theme,
    freqs: &[crate::model::types::TextFrequency],
    limit: usize,
) -> Div {
    let total: u64 = freqs.iter().map(|f| f.count as u64).sum();
    let bins: Vec<crate::model::types::DistributionBin> = freqs
        .iter()
        .map(|f| crate::model::types::DistributionBin {
            label: f.value.clone(),
            count: f.count,
            ratio: if total > 0 { f.count as f64 / total as f64 } else { 0.0 },
        })
        .collect();
    dist_bars(theme, &bins, limit)
}

/// 多列规则结果（关联性区共用）：Single = 键值行，Table = 表格。
fn render_multi_result(result: &MultiResultView, theme: &Theme) -> Div {
    let colors = theme.colors;
    match result {
        MultiResultView::Single(rows) => {
            let mut body = div().v_flex().w_full().gap_1();
            for row in rows {
                body = body.child(
                    div()
                        .h_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                .flex_none()
                                .text_xs()
                                .text_ellipsis()
                                .text_color(colors.muted_foreground)
                                .child(row.label.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(colors.foreground)
                                .child(row.value.clone()),
                        ),
                );
            }
            panel_box(theme, "规则输出", String::new(), body)
        }
        MultiResultView::Table { headers, rows } => {
            let mut head = div().h_flex().w_full().gap_2().px_2().py_1();
            for name in headers {
                head = head.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_ellipsis()
                        .text_color(colors.foreground)
                        .child(name.clone()),
                );
            }
            let mut body = div().v_flex().w_full();
            for row in rows {
                let mut line = div()
                    .h_flex()
                    .w_full()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .border_t_1()
                    .border_color(colors.border);
                for cell in row {
                    line = line.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(colors.foreground)
                            .child(cell.clone()),
                    );
                }
                body = body.child(line);
            }
            panel_box(
                &theme,
                "规则输出",
                String::new(),
                div().v_flex().w_full().child(head).child(body),
            )
        }
    }
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
                    null_rate: None,
                    unique_rate: None,
                    stats_detail: None,
                    histogram: None,
                    sample_values: Vec::new(),
                })
                .collect(),
            quality: None,
            progress: None,
            sample: None,
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

    #[test]
    fn markdown_report_carries_same_facts_and_escapes_cells() {
        // 出口事实（D36）：报告里出现与渲染一致的表名 / 行数 / 列数与检查器状态
        use crate::model::{ColumnCheckState, TableQualityView};
        use crate::quality_scorer::Grade;
        let mut p = profile("orders", 1_234, 3);
        p.quality = Some(TableQualityView {
            overall: 62.0,
            grade: Grade::Fair,
            summary: "2 个问题列".into(),
            problem_columns: 1,
            scored_columns: 3,
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
        // 列明细表头与状态计数
        assert!(md.contains("| 列 | 类型 | 缺失% | 唯一% | 评分 | 检查器 |"));
        assert!(md.contains("| c0 | INTEGER | 30 | — | — | 1/2 |"));
        // 转义：含 | 的表名不应串列
        assert_eq!(super::escape_md("a|b\nc"), "a｜b c");
    }
}
