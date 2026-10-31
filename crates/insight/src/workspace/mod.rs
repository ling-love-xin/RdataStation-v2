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
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, TabGroup};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, Size, Theme};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::insight_view::{InsightEvent, InsightView};
use crate::model::{
    ColumnCheckState, ColumnChecker, ColumnKind, InsightPanelState, InsightSetEntry,
    InsightSetKind, InsightTarget, MultiResultView, NULL_RATE_NOTE, TableColumnView,
    TableProfileView, fmt_int, fmt_num, fmt_pct, fmt_rows, kurt_direction, normality_judge,
    skew_direction,
};
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
/// 值得关注列数），**不重新取数**；不含质量总分或等级（评级归归档后的资产库）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetSummary {
    pub row_count: Option<u64>,
    pub column_count: Option<usize>,
    /// 检查器命中计数（适用且通过；无评级语义）
    pub checker_hits: Option<usize>,
    pub checker_applicable: Option<usize>,
    /// 表级缺失率均值（0–1；列统计未跑为 `None`）
    pub null_rate: Option<f64>,
    /// 表级唯一率均值（0–1；列统计未跑为 `None`）
    pub unique_rate: Option<f64>,
    /// 值得关注列数（缺失率高于阈值 / 检查器未命中的列，评估后才有）
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
        // 值得关注列：缺失率 > 20% 或检查器未命中（事实口径，不依赖旧评分等级）
        let attention_columns = profile.quality.as_ref().map(|q| {
            let miss: std::collections::HashSet<&str> = q
                .checker
                .iter()
                .filter(|c| c.state == ColumnCheckState::Miss)
                .map(|c| c.column.as_str())
                .collect();
            profile
                .columns
                .iter()
                .filter(|c| {
                    miss.contains(c.name.as_str()) || c.null_rate.map(|r| r > 0.2).unwrap_or(false)
                })
                .count()
        });
        Self {
            row_count: Some(profile.row_count),
            column_count: Some(profile.columns.len()),
            checker_hits: profile.quality.as_ref().map(|q| q.checker_hits),
            checker_applicable: profile.quality.as_ref().map(|q| q.checker_applicable),
            null_rate,
            unique_rate,
            attention_columns,
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
            // 集合行「切换目标」的还原依据（宿主经 `request_insight_switch` 使用）
            target: self.target.clone(),
            is_current,
        })
    }
}

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
    probe_input_subs: RefCell<Vec<Subscription>>,
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
pub enum RelTab {
    #[default]
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
            picker_q: String::new(),
            picker_search_subs: RefCell::new(Vec::new()),
            rel_tab: RelTab::default(),
            corr_cols: Vec::new(),
            rel_rule: None,
            col_search_subs: RefCell::new(Vec::new()),
            col_open: RefCell::new(HashSet::new()),
            probe_input_subs: RefCell::new(Vec::new()),
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

    /// 顶部 tab 条（A 方案多标签，2026-10-30 落地；对齐原型 `.tab-strip`）：
    /// 每个进入过洞察的目标一个 tab，dot = 当前目标实时状态（Loading 黄 /
    /// 就绪绿 / 出错红；非当前 tab 是静态灰点表示「已打开过」），✕ 关闭该 tab，
    /// 最右 ＋ 打开「新建洞察」（宿主对话框）。
    ///
    /// 点击回调只改状态（`activate_opened` / `close_opened` / 发事件），
    /// 副作用留在事件路径——对齐 render 纯读约定。
    fn render_tab_strip(
        &mut self,
        entity: &Entity<Self>,
        theme: &Theme,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let colors = theme.colors;
        let current_key = self.view.read(cx).target().map(target_key);
        let current_ix = self
            .opened
            .iter()
            .position(|o| Some(o.key.clone()) == current_key);
        let weak = entity.downgrade();
        TabBar::new("insight-ws-tabs")
            // `Tab::max_width` 是 `pub(super)`（只能 tab 模块内部用）——tab 条级
            // `max_width` 才是公开 API（0.8.1 vendor tab_bar.rs:133）：每 tab 超宽
            // 省略号截断。它只收 `Pixels`，ui 常量是 rem 倍率 → ×主题基准字号换算
            .max_width(px(ui::INSIGHT_WS_TAB_MAX_WIDTH * 16.0))
            .children(self.opened.iter().enumerate().map(|(ix, entry)| {
                let key = entry.key.clone();
                let is_current = current_ix == Some(ix);
                let dot = if is_current {
                    match self.view.read(cx).state() {
                        InsightPanelState::Loading => Badge::new().dot().color(colors.warning),
                        InsightPanelState::Error { .. } => Badge::new().dot().color(colors.danger),
                        _ => Badge::new().dot().color(colors.success),
                    }
                } else {
                    Badge::new().dot().color(colors.muted_foreground)
                };
                let w_close = weak.clone();
                let w_activate = weak.clone();
                Tab::new()
                    .label(entry.target.title())
                    .prefix(dot)
                    .suffix(
                        Button::new(format!("insight-ws-tab-close-{key}"))
                            .ghost()
                            .icon(IconName::Close)
                            .on_click(move |_, _, app| {
                                let _ =
                                    w_close.update(app, |panel, cx| panel.close_opened(ix, cx));
                            }),
                    )
                    .on_click(move |_, _, app| {
                        let _ =
                            w_activate.update(app, |panel, cx| panel.activate_opened(ix, cx));
                    })
            }))
            .when_some(current_ix, |bar, ix| bar.selected_index(ix))
            .suffix(
                Button::new("insight-ws-tab-new")
                    .ghost()
                    .icon(IconName::Plus)
                    .on_click(move |_, _, app| {
                        let _ = weak.update(app, |panel, cx| {
                            panel.insight_view().update(cx, |_view, cx| {
                                cx.emit(InsightEvent::NewInsightRequested)
                            })
                        });
                    }),
            )
    }

    /// 指向新目标：先把当前目标的画像摘要存档进 `opened`（对比聚合不重取数的前提），
    /// 再登记 / 去重，最后交 [`InsightView::set_target`]。
    ///
    /// 入口语义（D65）：「查看统计」在中间开 tab（本面板由宿主建一次），
    /// **多 tab 形态（A 方案，2026-10-30 落地）**：每个进入过洞察的目标占一个
    /// 顶部 tab（`opened` 即 tab 列表，新目标在前）；打开已有 tab 只切目标、
    /// 不重登记（去重分支）。
    pub fn open_target(&mut self, target: InsightTarget, cx: &mut Context<Self>) {
        self.archive_summary(cx);
        let key = target_key(&target);
        let already = self.opened.iter().any(|e| e.key == key);
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
        // 已登记目标 = 本次会话确认过（切 tab / 关 tab / 候选重开）：直接取数；
        // 新目标走探查确认窗口（C3：进入洞察先定数据量与列）
        self.view.update(cx, |view, cx| {
            if already {
                view.set_target_loaded(target, cx);
            } else {
                view.set_target(target, cx);
            }
        });
        cx.notify();
    }

    /// 顶部 tab 切换（A 方案）：切到已登记的 `opened` 目标。
    ///
    /// 复用 `open_target` 的去重分支（已登记则只更新目标 + 切画像形态）；
    /// 与新建入口的区别是这里**必然已登记**，不会新增 tab。
    pub fn activate_opened(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.opened.get(index).cloned() else {
            return;
        };
        self.open_target(entry.target, cx);
    }

    /// 顶部 tab 关闭（A 方案 ✕）：从 `opened` 移除该目标。
    ///
    /// 关的是**当前目标**时：先存档摘要，再切相邻目标（优先右侧原位，
    /// 边界回退左侧）；关掉**最后一个** tab 时清空面板回无目标空态。
    /// 对比候选是 `opened` 的子集语义（候选 = 已进入洞察的目标），
    /// 关闭的 tab 同时离开候选池。
    pub fn close_opened(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.opened.len() {
            return;
        }
        let closing_key = self.opened[index].key.clone();
        self.opened.remove(index);
        let current_key = self.view.read(cx).target().map(target_key);
        if current_key.as_deref() != Some(closing_key.as_str()) {
            cx.notify();
            return;
        }
        let next = match next_index_after_close(self.opened.len(), index) {
            Some(ix) => ix,
            None => {
                self.view.update(cx, |view, cx| view.clear_target(cx));
                cx.notify();
                return;
            }
        };
        let target = self.opened[next].target.clone();
        self.open_target(target, cx);
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
            // A 方案多标签（2026-10-30）：顶部 tab 条在工具栏之上（对齐原型 tab-strip）
            .child(self.render_tab_strip(&entity, &theme, cx))
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
// ===== 子模块（拆分：渲染方法按域下沉，状态/协议/测试留在根） =====
mod compare;
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
    step_dot, summary_card, zone_note, zone_rows,
};
pub(crate) use markdown::{dist_text_bars, render_markdown_report, render_multi_result};
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
            .max_w(rems(ui::INSIGHT_WS_TITLE_MAX_WIDTH))
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
            p.insight_view().update(cx, |v, cx| v.confirm_probe_dialog(cx));
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
                    null_rate: None,
                    unique_rate: None,
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

    /// C3：源目标进入洞察先整页渲染探查确认窗口，确认后关闭并进入取样加载态。
    #[gpui_kit::test]
    fn source_target_renders_probe_dialog_until_confirmed(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt as _;
        use gpui_kit::{ElementId, SharedString};
        cx.update(gpui_kit::init);
        let (panel, cx) = cx.add_window_view(|_window, cx| InsightWorkspacePanel::new(cx));

        cx.update(|window, cx| {
            panel.update(cx, |p, cx| p.open_target(table_target("orders"), cx));
            window.draw(cx).clear(cx);
        });
        let confirm = cx.update(|window, _| {
            window.try_find(ElementId::Name(SharedString::from(
                "insight-probe-confirm",
            )))
        });
        assert!(
            confirm.is_some(),
            "源目标进入洞察应先渲染探查确认窗口（含确认探查按钮）"
        );
        // 「探查全部」提示区入口也在（决策文档 §5 硬约束）
        let all = cx.update(|window, _| {
            window.try_find(ElementId::Name(SharedString::from("insight-probe-all")))
        });
        assert!(all.is_some(), "提示区必含「探查全部」入口");

        cx.update(|window, cx| {
            panel.update(cx, |p, cx| {
                p.insight_view()
                    .update(cx, |v, cx| v.confirm_probe_dialog(cx));
            });
            window.draw(cx).clear(cx);
        });
        let confirm = cx.update(|window, _| {
            window.try_find(ElementId::Name(SharedString::from(
                "insight-probe-confirm",
            )))
        });
        assert!(confirm.is_none(), "确认探查后确认窗口应关闭并进入取数");
    }
}
