//! 右 Dock 洞察面板的**内容视图**（M8 Phase 1 落地）。
//!
//! 结构（见 `docs/architecture/insight/insight-prototype-design.md` §2、§3）：
//!
//! ```text
//! 面板头（36px）：洞察 · ⚙规则管理 · ⟳重算
//! 目标头（有目标时）：列名 + 类型徽标 + 空值率
//! Tab 条（28px）：列 │ 表 │ 多列 │ 结构 │ 历史
//! 内容（四态）：空 / 加载 / 错误 / 数据（列画像四区）
//! ```
//!
//! 职责边界：
//! - **不做 I/O**：本视图只渲染 [`InsightPanelState`]，取数由宿主发起（D20）。
//! - **不持色值**：模型给语义（`Emphasis` / `NoteLevel` / `ColumnKind`），本文件映射到
//!   `theme.colors.*`。
//! - **渲染零副作用**：点击只改自身状态 + `emit` 事件，宿主收到事件后再动数据。
//!
//! 视图归属：本文件按 **方案 A**（视图入 `crates/insight`）落地，宿主桥由 workbench
//! 侧订阅 [`InsightEvent`] 装配。

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Size, Sizable as _, Theme};
use gpui_kit::*;

use crate::commands::InsightRefresh;
use crate::model::{
    ColumnKind, ColumnProfileView, DimensionView, DistributionBar, Emphasis, InsightPanelState,
    InsightTarget, NoteLevel, PanelData, PanelTab, QualityNote, SampleCell, StatRow,
    TableColumnView, TableProfileView,
};
use crate::quality_scorer::Grade;
use crate::rule_view::RulesView;
use crate::ui;

/// 面板向宿主发出的请求。
///
/// 面板**不自己取数**（D20）：换目标或点 ⟳ 只发事件，由 workbench 侧订阅后执行
/// （对齐「点击回调只改状态，副作用在事件路径」的约定）。
#[derive(Debug, Clone, PartialEq)]
pub enum InsightEvent {
    /// 请宿主加载该目标的画像。
    ///
    /// 带 payload 而不让宿主回读面板状态：宿主拿到事件就能直接开工，
    /// 不必再持面板实体（也避开了「读实体时它正在被更新」的租借冲突）。
    ProfileRequested { target: InsightTarget },
    /// 请宿主评估整表质量（逐列串行 + 进度回填）。
    ///
    /// 只带 `temp_table`：列清单由接缝在后台现读（元数据可能已变，
    /// 而带着一份可能过期的列名去跑统计，失败会指向不存在的那一列）。
    TableEvaluateRequested {
        temp_table: String,
        table_name: String,
    },
}

/// 列画像四区（顺序即渲染顺序）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnSection {
    Basics,
    Distribution,
    Quality,
    Sample,
}

impl ColumnSection {
    const ALL: [ColumnSection; 4] = [
        ColumnSection::Basics,
        ColumnSection::Distribution,
        ColumnSection::Quality,
        ColumnSection::Sample,
    ];

    fn title(self) -> &'static str {
        match self {
            ColumnSection::Basics => "基础统计",
            ColumnSection::Distribution => "数据分布",
            ColumnSection::Quality => "数据质量",
            ColumnSection::Sample => "样本数据",
        }
    }

    /// 默认展开态（原型 §2：基础统计与数据分布展开，质量与样本折叠）
    fn default_open() -> [bool; 4] {
        [true, true, false, false]
    }
}

/// 洞察面板视图。
pub struct InsightView {
    target: Option<InsightTarget>,
    state: InsightPanelState,
    tab: PanelTab,
    /// 四区折叠态：属用户偏好，⟳ 重算与切换 Tab 都不得清空
    open_sections: [bool; 4],
    /// 未打开项目（宿主告知）。
    ///
    /// 无项目时画像仍可算（临时表在内存里），但**规则管理与快照不可用**——
    /// 它们都落在项目目录下（原型 §4 / 架构 §8）。
    project_open: bool,
    /// 规则管理对话框（Phase 2.3）。
    ///
    /// 随面板在构造期创建：对话框数据靠 `jobs::attach_rules` 后台取，
    /// 而「开关一条规则」与面板数据同属一个会话，实体提前存在才接得上订阅。
    rules: Entity<RulesView>,
}

impl EventEmitter<InsightEvent> for InsightView {}

impl InsightView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            target: None,
            state: InsightPanelState::Empty,
            tab: PanelTab::Column,
            open_sections: ColumnSection::default_open(),
            // 保守初值：宿主装配时会立刻告知真实项目状态（`set_project_open`）。
            // 宁可让依赖项目的入口先禁用，也不要给一个点了没用的按钮。
            project_open: false,
            // 无 I/O：真正的取数与写库在 `jobs::attach_rules` 接到事件之后
            rules: cx.new(RulesView::new),
        }
    }

    /// 规则管理对话框实体（宿主用它接 `insight::jobs::attach_rules`）。
    pub fn rules_view(&self) -> &Entity<RulesView> {
        &self.rules
    }

    pub fn target(&self) -> Option<&InsightTarget> {
        self.target.as_ref()
    }

    pub fn state(&self) -> &InsightPanelState {
        &self.state
    }

    pub fn tab(&self) -> PanelTab {
        self.tab
    }

    /// 四区展开态（测试与宿主用）
    pub fn sections_open(&self) -> [bool; 4] {
        self.open_sections
    }

    /// 是否已打开项目（决定无项目提示；Phase 2 接入规则管理后也会约束 ⚙）
    pub fn project_open(&self) -> bool {
        self.project_open
    }

    /// 宿主告知项目开关（打开 / 切换 / 关闭项目时调用）
    pub fn set_project_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.project_open != open {
            self.project_open = open;
            cx.notify();
        }
    }

    /// 指向新目标：切到该目标的默认 Tab、进入加载态，并请宿主取数。折叠偏好保留。
    pub fn set_target(&mut self, target: InsightTarget, cx: &mut Context<Self>) {
        self.tab = target.default_tab();
        self.target = Some(target.clone());
        self.state = InsightPanelState::Loading;
        cx.emit(InsightEvent::ProfileRequested { target });
        cx.notify();
    }

    /// 出数（列画像）
    pub fn set_profile(&mut self, profile: ColumnProfileView, cx: &mut Context<Self>) {
        self.state = InsightPanelState::Data(PanelData::Column(profile));
        cx.notify();
    }

    /// 出数（表探查）
    pub fn set_table_profile(&mut self, profile: TableProfileView, cx: &mut Context<Self>) {
        self.state = InsightPanelState::Data(PanelData::Table(profile));
        cx.notify();
    }

    /// 「评估全表」：发请求并进入评估中（按钮变灰、进度行出现）。
    ///
    /// 整表重算不把已有分数清空——列分数是逐列长出来的，清空会让面板闪一下空白。
    pub fn request_table_evaluation(&mut self, cx: &mut Context<Self>) {
        let Some(InsightTarget::Table {
            temp_table,
            table_name,
        }) = self.target.clone()
        else {
            return;
        };
        if let InsightPanelState::Data(PanelData::Table(profile)) = &self.state {
            let total = profile.columns.len();
            let next = profile.evaluating(0, total);
            self.state = InsightPanelState::Data(PanelData::Table(next));
        }
        cx.emit(InsightEvent::TableEvaluateRequested {
            temp_table,
            table_name,
        });
        cx.notify();
    }

    /// 出错（`retryable` 决定是否给「重试」入口）
    pub fn set_error(&mut self, message: impl Into<String>, retryable: bool, cx: &mut Context<Self>) {
        self.state = InsightPanelState::Error {
            message: message.into(),
            retryable,
        };
        cx.notify();
    }

    /// 回到无目标空态（项目切换 / 目标失效且不可重试）
    pub fn clear_target(&mut self, cx: &mut Context<Self>) {
        self.target = None;
        self.state = InsightPanelState::Empty;
        cx.notify();
    }

    /// ⟳ 与「重试」共用：重新加载当前目标（保持 Tab 与折叠偏好）。无目标时不动。
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if let Some(target) = self.target.clone() {
            self.state = InsightPanelState::Loading;
            cx.emit(InsightEvent::ProfileRequested { target });
            cx.notify();
        }
    }

    /// 切 Tab（保留折叠偏好与已有数据：切回来仍看得到上次结果）
    pub fn set_tab(&mut self, tab: PanelTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            cx.notify();
        }
    }

    fn set_tab_from_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(tab) = PanelTab::from_index(ix) {
            self.set_tab(tab, cx);
        }
    }

    /// 同步四区展开态（`Accordion` 回调只给「当前展开的下标集合」）
    fn set_open_sections(&mut self, open: &[usize], cx: &mut Context<Self>) {
        for (i, slot) in self.open_sections.iter_mut().enumerate() {
            *slot = open.contains(&i);
        }
        cx.notify();
    }

    /// ⟳ 的 Action 入口（与面板头按钮同一条路径）
    fn on_refresh(&mut self, _: &InsightRefresh, _window: &mut Window, cx: &mut Context<Self>) {
        self.reload(cx);
    }

    // ==================== 渲染片段 ====================

    fn render_header(&self, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
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
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_color(colors.foreground)
                    .child("洞察"),
            )
            .child(
                Button::new("insight-rules")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Settings)
                    // 规则落在项目目录下（`{项目}/.RSmeta/insight-rules/`）：
                    // 无项目时不给入口，而不是给一个点了没反应的按钮
                    .disabled(!self.project_open)
                    .tooltip(if self.project_open {
                        "规则管理"
                    } else {
                        "规则管理（需先打开项目）"
                    })
                    .on_click({
                        let rules = self.rules.clone();
                        move |_, window, app| {
                            // 开窗 + 发一次取数请求；数据由接缝回填
                            rules.update(app, |view, cx| view.begin_load(window, cx));
                        }
                    }),
            )
            .child(
                Button::new("insight-refresh")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RotateCw)
                    .tooltip("重算")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            entity.update(app, |view, cx| view.reload(cx));
                        }
                    }),
            )
    }

    /// 无项目提示：画像仍可算（临时表在内存里），但依赖项目目录的能力不可用
    fn render_project_hint(&self, theme: &Theme) -> Option<Div> {
        if self.project_open {
            return None;
        }
        Some(
            div()
                .flex_none()
                .w_full()
                .px(rems(ui::PANEL_PADDING))
                .py_1()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("未打开项目：画像可用，规则管理与快照不可用"),
        )
    }

    /// 目标头：常显当前目标（列名 + 类型徽标 + 空值率）
    fn render_target_head(&self, theme: &Theme) -> Option<Div> {
        let target = self.target.as_ref()?;
        let colors = theme.colors;
        let mut head = div()
            .flex_none()
            .px(rems(ui::PANEL_PADDING))
            .py_1()
            .bg(colors.list_active)
            .v_flex()
            .gap_1()
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .text_sm()
                            .text_color(colors.foreground)
                            .text_ellipsis()
                            .child(target.title()),
                    )
                    .child(kind_badge(self.data_kind(), theme)),
            );
        if let Some(detail) = target.detail() {
            head = head.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .text_ellipsis()
                    .child(detail),
            );
        }
        if let Some(profile) = self.state.column() {
            head = head.child(
                div()
                    .text_xs()
                    .text_color(if profile.null_rate > crate::model::NULL_RATE_WARN {
                        colors.warning
                    } else {
                        colors.muted_foreground
                    })
                    .child(format!(
                        "空值率 {}",
                        crate::model::fmt_pct(profile.null_rate)
                    )),
            );
        }
        if let Some(profile) = self.state.table() {
            head = head.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("{} 行", crate::model::fmt_rows(profile.row_count))),
            );
        }
        Some(head)
    }

    /// 当前数据的类型族（无数据时退回目标声明的类型字符串）
    fn data_kind(&self) -> ColumnKind {
        self.state
            .column()
            .map(|profile| profile.kind)
            .unwrap_or(ColumnKind::Unknown)
    }

    fn render_tab_bar(&self, entity: &Entity<Self>, theme: &Theme) -> Div {
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

    /// 内容主体（四态）。图标尺寸由 `render` 从 `rem_size()` 换算后传入（视图不写裸 px）。
    fn render_body(
        &self,
        entity: &Entity<Self>,
        theme: &Theme,
        empty_icon: Pixels,
        inline_icon: Pixels,
    ) -> Vec<AnyElement> {
        match (&self.state, self.tab) {
            (InsightPanelState::Empty, _) => vec![
                empty_state(IconName::Info, self.empty_hint(), theme, empty_icon).into_any_element(),
            ],
            (InsightPanelState::Loading, _) => vec![skeleton(theme).into_any_element()],
            (InsightPanelState::Error { message, retryable }, _) => vec![
                error_block(message, *retryable, entity.clone(), theme, inline_icon)
                    .into_any_element(),
            ],
            (InsightPanelState::Data(data), PanelTab::Column) => match data.as_column() {
                Some(profile) => vec![self
                    .render_column_profile(profile, entity, theme, inline_icon)
                    .into_any_element()],
                // 数据态里装的不是列表：按「本 Tab 尚无内容」处理，不硬塞
                None => vec![
                    empty_state(IconName::Info, tab_hint(self.tab), theme, empty_icon)
                        .into_any_element(),
                ],
            },
            (InsightPanelState::Data(data), PanelTab::Table) => match data.as_table() {
                Some(profile) => vec![self
                    .render_table_profile(profile, entity, theme, inline_icon)
                    .into_any_element()],
                None => vec![
                    empty_state(IconName::Info, tab_hint(self.tab), theme, empty_icon)
                        .into_any_element(),
                ],
            },
            // 其余 Tab 在后续期次落地：给期次提示，不显示假数据
            (InsightPanelState::Data(_), tab) => vec![
                empty_state(IconName::Info, tab_hint(tab), theme, empty_icon).into_any_element(),
            ],
        }
    }

    fn empty_hint(&self) -> &'static str {
        if self.target.is_some() {
            tab_hint(self.tab)
        } else {
            "在结果表列头或导航树上右键，选择洞察"
        }
    }

    /// 评分卡（Phase 2）：**钉在滚动区之外**——分数是这列的头号结论，不该滚走。
    ///
    /// 只有「列」Tab 且列非全空时出现（后者见 `ColumnProfileView::score`：不产假分数）。
    fn render_score_card(&self, theme: &Theme) -> Option<Div> {
        if self.tab != PanelTab::Column {
            return None;
        }
        let score = self.state.column()?.score.as_ref()?;
        let colors = theme.colors;
        let color = grade_color(score.grade, theme);

        let mut card = div()
            .flex_none()
            .v_flex()
            .w_full()
            .gap_1()
            .px(rems(ui::PANEL_PADDING))
            .py_1()
            .border_t_1()
            .border_color(colors.border)
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(rems(ui::INSIGHT_SCORE_FONT))
                            .text_color(color)
                            .child(format!("{:.0}", score.overall)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(color)
                            .child(score.grade.label()),
                    ),
            );
        for dim in &score.dimensions {
            card = card.child(dimension_row(dim, theme));
        }
        Some(card.child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(score.summary.clone()),
        ))
    }

    /// 表探查（Tab「表」，Phase 3.1）：列元数据表 + 行数 + 评估入口 + 表级质量。
    ///
    /// 列名是下钻热点（点它切到该列的「列」Tab）——表→列是用户最常走的下一步。
    fn render_table_profile(
        &self,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let evaluating = profile.progress.is_some();

        let mut body = div().v_flex().w_full().gap_2();

        // 评估入口（串行逐列；评估中禁用，避免重复排队撞并发上限）
        body = body.child(
            div().h_flex().w_full().gap_2().child(div().flex_1()).child(
                Button::new("insight-eval-table")
                    .small()
                    .icon(if evaluating {
                        IconName::RotateCw
                    } else {
                        IconName::TriangleAlert
                    })
                    .label(if evaluating { "评估中…" } else { "评估全表" })
                    .disabled(evaluating || profile.columns.is_empty())
                    .tooltip("逐列串行评分（避免撞后端并发上限），进度会一列列回填")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            entity.update(app, |view, cx| view.request_table_evaluation(cx))
                        }
                    }),
            ),
        );

        // 表级质量：评估完才有（没评估就是没有，不是 0 分）
        if let Some(quality) = &profile.quality {
            let color = grade_color(quality.grade, theme);
            body = body.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .p_2()
                    .rounded_sm()
                    .bg(colors.list_hover)
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(rems(ui::INSIGHT_SCORE_FONT * 0.75))
                                    .text_color(color)
                                    .child(format!("{:.0}", quality.overall)),
                            )
                            .child(div().text_xs().text_color(color).child(quality.grade.label()))
                            .child(div().flex_1().min_w_0().text_xs().text_ellipsis().child(
                                if quality.problem_columns > 0 {
                                    format!("{} 列需关注", quality.problem_columns)
                                } else {
                                    "无问题列".to_string()
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(quality.summary.clone()),
                    ),
            );
        }

        // 进度行（进行中）：进度条 + 文字，让用户知道还在动
        if let Some(progress) = profile.progress {
            body = body.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("正在评估 {}/{} 列…", progress.done, progress.total)),
                    )
                    .child(ratio_bar(
                        progress.ratio(),
                        ui::INSIGHT_RATIO_BAR_HEIGHT,
                        colors.primary,
                        theme,
                    )),
            );
        }

        // 列元数据表：# 列名（PK） / 类型 / 可空 / 质量
        body = body.child(table_header_row(theme));
        if profile.columns.is_empty() {
            body = body.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("该临时表没有可见的列"),
            );
        }
        for column in &profile.columns {
            body = body.child(table_column_row(
                profile,
                column,
                entity,
                theme,
                inline_icon,
            ));
        }

        // 采样口径必须常驻：口径写错比不写更坏——这里写的是**实际**口径
        body.child(
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("行数与质量分基于全量统计；样本值为前 5 行，直方图至少 10 行"),
        )
    }

    /// 列画像四区
    fn render_column_profile(
        &self,
        profile: &ColumnProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let sections = ColumnSection::ALL;
        let mut accordion = Accordion::new("insight-column-zones")
            .multiple(true)
            .with_size(Size::Small)
            .on_toggle_click({
                let entity = entity.clone();
                move |open, _window, app| {
                    entity.update(app, |view, cx| view.set_open_sections(open, cx));
                }
            });

        for (i, section) in sections.iter().enumerate() {
            let open = self.open_sections[i];
            accordion = accordion.item(|item| {
                item.title(zone_title(section.title(), theme))
                    .open(open)
                    .children(zone_content(*section, profile, theme, inline_icon))
            });
        }

        div().v_flex().w_full().gap_2().child(accordion)
    }
}

impl Render for InsightView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let theme = cx.theme();
        // 图标尺寸从主题字号换算：常量保持 rem 倍率，视图不出现裸 px
        let rem = window.rem_size();
        let empty_icon = rem * ui::EMPTY_ICON_SIZE;
        let inline_icon = rem * ui::INSIGHT_INLINE_ICON_SIZE;

        div()
            .key_context("insight")
            // ⟳ 的键位（`Ctrl+Shift+R`，绑在 `insight` context 上；键位在 app 层注册）
            .on_action(cx.listener(Self::on_refresh))
            .v_flex()
            .size_full()
            .bg(theme.colors.background)
            .child(self.render_header(&entity, theme))
            .children(self.render_project_hint(theme))
            .children(self.render_target_head(theme))
            .child(self.render_tab_bar(&entity, theme))
            // 滚动归面板自己（Dock 的 #tab-content 不产生滚动）：滚动条贴内容边缘，
            // 内距放在滚动区的**内部**，避免出现滚动条与内距之间的空档。
            .child(
                div()
                    .id("insight-body")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .gap_2()
                            .p(rems(ui::PANEL_PADDING))
                            .children(self.render_body(
                                &entity,
                                theme,
                                empty_icon,
                                inline_icon,
                            )),
                    ),
            )
            .children(self.render_score_card(theme))
    }
}

// ==================== 片段助手（纯渲染） ====================

/// 类型徽标（色按类型族；文案取类型族名）
fn kind_badge(kind: ColumnKind, theme: &Theme) -> Div {
    let colors = theme.colors;
    let color = match kind {
        ColumnKind::Numeric => colors.info,
        ColumnKind::Text => colors.success,
        ColumnKind::DateTime => colors.warning,
        ColumnKind::Boolean => colors.primary,
        ColumnKind::Unknown => colors.muted_foreground,
    };
    div()
        .flex_none()
        .px_1()
        .rounded_sm()
        .text_xs()
        .text_color(color)
        .child(kind.label())
}

/// 等级 → 主题角色（四档取色；「较差」与「差」共用 danger）
fn grade_color(grade: Grade, theme: &Theme) -> Hsla {
    let colors = theme.colors;
    match grade {
        Grade::Excellent => colors.success,
        Grade::Good => colors.primary,
        Grade::Fair => colors.warning,
        Grade::Poor | Grade::Bad => colors.danger,
    }
}

/// 评分卡的一维：名称 + 细条 + 分数（条色按**该维自己的**等级，便于一眼看出短板）
fn dimension_row(dim: &DimensionView, theme: &Theme) -> Div {
    let colors = theme.colors;
    let color = grade_color(Grade::of(dim.score), theme);
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(dim.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(color)
                        .child(format!("{:.0}", dim.score)),
                ),
        )
        .child(ratio_bar(
            (dim.score / 100.0).clamp(0.0, 1.0) as f32,
            ui::INSIGHT_RATIO_BAR_HEIGHT,
            color,
            theme,
        ))
}

/// 比例条（底槽 + 填充）：分布区与评分卡共用，避免两处各画一遍
fn ratio_bar(ratio: f32, height: f32, fill: Hsla, theme: &Theme) -> Div {
    div()
        .w_full()
        .h(rems(height))
        .rounded_sm()
        .bg(theme.colors.border)
        .child(
            div()
                .h_full()
                .w(relative(ratio))
                .rounded_sm()
                .bg(fill),
        )
}

/// 表探查的表头行
fn table_header_row(theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .px_1()
        .py_0p5()
        .bg(colors.list_hover)
        .text_xs()
        .text_color(colors.muted_foreground)
        .child(div().w(rems(ui::INSIGHT_TABLE_INDEX_WIDTH)).child("#"))
        .child(div().flex_1().min_w_0().child("列名"))
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_TYPE_WIDTH))
                .text_ellipsis()
                .child("类型"),
        )
        .child(div().w(rems(ui::INSIGHT_TABLE_FLAG_WIDTH)).child("可空"))
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_QUALITY_WIDTH))
                .child("质量"),
        )
}

/// 表探查的一行：列名为下钻热点（点击切到该列的「列」Tab）
fn table_column_row(
    profile: &TableProfileView,
    column: &TableColumnView,
    entity: &Entity<InsightView>,
    theme: &Theme,
    inline_icon: Pixels,
) -> Div {
    let colors = theme.colors;
    let quality = match (column.score, column.grade()) {
        (Some(score), Some(grade)) => div()
            .text_color(grade_color(grade, theme))
            .child(format!("{:.0} {}", score, grade.label())),
        _ => div().text_color(colors.muted_foreground).child("—"),
    };

    let mut row = div()
        .h_flex()
        .w_full()
        .gap_2()
        .h(rems(ui::ROW_HEIGHT))
        .px_1()
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_INDEX_WIDTH))
                .text_color(colors.muted_foreground)
                .child(column.index.to_string()),
        )
        .child(
            div()
                .h_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    // 列名用 Button（ghost、无内距）：白搓 div 会丢 hover / 键盘 / a11y
                    Button::new(ElementId::Name(
                        format!("insight-table-col-{}", column.name).into(),
                    ))
                    .ghost()
                    .xsmall()
                    .label(column.name.clone())
                    .tooltip("看这一列的画像")
                    .on_click({
                        let entity = entity.clone();
                        let next = column.clone();
                        let temp_table = profile.table_name.clone();
                        move |_, _, app| {
                            entity.update(app, |view, cx| {
                                // 表头里的临时表名与目标一致：目标已换时以当前目标为准
                                let temp = match &view.target {
                                    Some(InsightTarget::Table { temp_table, .. }) => temp_table.clone(),
                                    _ => temp_table.clone(),
                                };
                                view.set_target(
                                    InsightTarget::Column {
                                        temp_table: temp,
                                        column: next.name.clone(),
                                        data_type: next.data_type.clone(),
                                    },
                                    cx,
                                );
                            });
                        }
                    }),
                )
                .child(if column.primary_key {
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.info)
                        .child("PK")
                } else {
                    div()
                }),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_TYPE_WIDTH))
                .text_xs()
                .text_color(colors.muted_foreground)
                .text_ellipsis()
                .child(column.data_type.clone()),
        )
        .child(
            div()
                .w(rems(ui::INSIGHT_TABLE_FLAG_WIDTH))
                .text_xs()
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
                .text_xs()
                .child(quality),
        );

    // 类型未识别：给一个可点的提示（不是所有列都能算质量）
    if column.kind == ColumnKind::Unknown {
        row = row.child(
            Icon::new(IconName::Info)
                .size(inline_icon)
                .text_color(colors.muted_foreground),
        );
    }
    row
}

fn zone_title(title: &str, theme: &Theme) -> Div {
    div()
        .text_size(rems(ui::INSIGHT_SECTION_TITLE_FONT))
        .text_color(theme.colors.foreground)
        .child(title.to_string())
}

/// 未落地 Tab 的提示（不显示假数据）
fn tab_hint(tab: PanelTab) -> &'static str {
    match tab {
        PanelTab::Column => "右键结果表中的列，查看列画像",
        PanelTab::Table => "右键表（或结果集）选择「查看统计」",
        PanelTab::MultiColumn => "多列分析将在 Phase 3 落地",
        PanelTab::Schema => "结构洞察将在 Phase 4 落地",
        PanelTab::History => "快照历史将在 Phase 5 落地",
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

/// 加载骨架（基础统计 5 行 + 分布 4 条；不阻塞 Tab 切换）
fn skeleton(theme: &Theme) -> Div {
    let bar = |w: f32| {
        div()
            .w(rems(w))
            .h(rems(0.75))
            .rounded_sm()
            .bg(theme.colors.border)
    };
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

/// 错误态：文案 + 可选重试
fn error_block(
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

/// 四区内容分派
fn zone_content(
    section: ColumnSection,
    profile: &ColumnProfileView,
    theme: &Theme,
    icon_px: Pixels,
) -> Vec<AnyElement> {
    match section {
        ColumnSection::Basics => profile
            .basics
            .iter()
            .map(|row| stat_row(row, theme).into_any_element())
            .collect(),
        ColumnSection::Distribution => distribution_zone(profile, theme),
        ColumnSection::Quality => quality_zone(&profile.notes, theme, icon_px),
        ColumnSection::Sample => sample_zone(&profile.sample, theme),
    }
}

/// 基础统计一行：键（次级色）+ 值（正文色，warning 强调）
fn stat_row(row: &StatRow, theme: &Theme) -> Div {
    let colors = theme.colors;
    div()
        .h_flex()
        .w_full()
        .gap_2()
        .child(
            div()
                .flex_none()
                .w(rems(4.5))
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(row.label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_ellipsis()
                .text_color(match row.emphasis {
                    Emphasis::Normal => colors.foreground,
                    Emphasis::Muted => colors.muted_foreground,
                    Emphasis::Warning => colors.warning,
                })
                .child(row.value.clone()),
        )
}

/// 数据分布：条 + 占比；无数可画时说明原因（不造假分布）
fn distribution_zone(profile: &ColumnProfileView, theme: &Theme) -> Vec<AnyElement> {
    if profile.distribution.is_empty() {
        let hint = match profile.kind {
            ColumnKind::Unknown => "类型未识别，未生成分布",
            _ => "样本不足，未生成分布",
        };
        return vec![muted_line(hint, theme).into_any_element()];
    }
    // 布尔列只有一条 True 占比：用矮条，其余用直方图条高
    let bar_height = if profile.kind == ColumnKind::Boolean {
        ui::INSIGHT_RATIO_BAR_HEIGHT
    } else {
        ui::INSIGHT_HISTOGRAM_BAR_HEIGHT
    };
    profile
        .distribution
        .iter()
        .map(|bar| distribution_bar(bar, bar_height, theme).into_any_element())
        .collect()
}

fn distribution_bar(bar: &DistributionBar, bar_height: f32, theme: &Theme) -> Div {
    let colors = theme.colors;
    let ratio = bar.ratio.clamp(0.0, 1.0) as f32;
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(colors.muted_foreground)
                        .child(bar.label.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(bar.text.clone()),
                ),
        )
        .child(ratio_bar(ratio, bar_height, colors.primary, theme))
}

/// 数据质量：提示列表（Info / Warning 两档）
fn quality_zone(notes: &[QualityNote], theme: &Theme, icon_px: Pixels) -> Vec<AnyElement> {
    if notes.is_empty() {
        return vec![muted_line("未发现需要提示的质量问题", theme).into_any_element()];
    }
    notes
        .iter()
        .map(|note| {
            let colors = theme.colors;
            let (icon, color) = match note.level {
                NoteLevel::Info => (IconName::Info, colors.info),
                NoteLevel::Warning => (IconName::TriangleAlert, colors.warning),
            };
            div()
                .h_flex()
                .w_full()
                .gap_1()
                .items_start()
                .child(
                    Icon::new(icon)
                        .with_size(Size::Size(icon_px))
                        .flex_none()
                        .text_color(color),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(color)
                        .child(note.text.clone()),
                )
                .into_any_element()
        })
        .collect()
}

/// 样本数据：序号 + 值（NULL 显式显示）
fn sample_zone(cells: &[SampleCell], theme: &Theme) -> Vec<AnyElement> {
    if cells.is_empty() {
        return vec![muted_line("无可显示的样本", theme).into_any_element()];
    }
    let colors = theme.colors;
    cells
        .iter()
        .map(|cell| {
            let is_null = cell.value.is_none();
            let text = match &cell.value {
                Some(v) => truncate(v, ui::INSIGHT_SAMPLE_MAX_CHARS),
                None => "NULL".to_string(),
            };
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w(rems(1.5))
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(cell.index.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(if is_null {
                            colors.muted_foreground
                        } else {
                            colors.foreground
                        })
                        .child(text),
                )
                .into_any_element()
        })
        .collect()
}

fn muted_line(text: &str, theme: &Theme) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 样本单元格截断（超长文本不得撑破 17.5rem 面板）
fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

// ==================== 测试 ====================
//
// 不写 `use gpui_kit::*` / `use super::*`：通配导入会把 gpui 的 `test` 属性宏带进作用域，
// 而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己（recursion limit）。

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::{truncate, InsightView};
    use crate::model::{
        ColumnProfileView, InsightPanelState, InsightTarget, PanelTab, TableProfileView,
    };
    // 领域类型从 crate 根再导出引用（`model.rs` 里对 `types` 的 `use` 是私有的）
    use crate::{
        BooleanStats, ColumnInsightFull, ColumnQualityEntry, ColumnStats, ColumnStatsDetail,
        DateTimeStats, DistributionBin, NumericStats, TableColumnMeta, TableProfile, TableQuality,
        TextFrequency, TextStats,
    };

    /// 三列表（数值 / 数值 / 未识别）——表探查的典型形态
    fn table_view() -> TableProfileView {
        TableProfileView::from_profile(
            &TableProfile {
                table_name: "t_insight_view_table".into(),
                db_type: "DuckDB".into(),
                columns: vec![
                    TableColumnMeta {
                        column_name: "id".into(),
                        data_type: "BIGINT".into(),
                        is_nullable: false,
                        is_primary_key: true,
                        ordinal_position: 1,
                    },
                    TableColumnMeta {
                        column_name: "amount".into(),
                        data_type: "DECIMAL(12,2)".into(),
                        is_nullable: true,
                        is_primary_key: false,
                        ordinal_position: 2,
                    },
                    TableColumnMeta {
                        column_name: "payload".into(),
                        data_type: "BLOB".into(),
                        is_nullable: true,
                        is_primary_key: false,
                        ordinal_position: 3,
                    },
                ],
                row_count: Some(124_000),
                schema_name: None,
            },
            "orders",
        )
    }

    fn column_target() -> InsightTarget {
        InsightTarget::Column {
            temp_table: "t_result_1".into(),
            column: "amount".into(),
            data_type: "DECIMAL(12,2)".into(),
        }
    }

    fn profile() -> ColumnProfileView {
        ColumnProfileView::from_domain(&ColumnInsightFull {
            stats: ColumnStats {
                column_name: "amount".into(),
                data_type: "DECIMAL(12,2)".into(),
                total_count: 100,
                null_count: 30,
                null_rate: 0.3,
                unique_count: Some(70),
                stats_detail: ColumnStatsDetail::Numeric(NumericStats {
                    min: 1.0,
                    max: 99.0,
                    avg: 50.0,
                    median: 50.0,
                    p25: 25.0,
                    p75: 75.0,
                    sum: 3500.0,
                    stddev: Some(10.0),
                    skewness: Some(2.0),
                    kurtosis: None,
                    is_extreme: Vec::new(),
                }),
            },
            sample: vec![serde_json::json!(42), serde_json::Value::Null],
            histogram: Some(vec![DistributionBin {
                label: "0–50".into(),
                count: 40,
                ratio: 0.4,
            }]),
        })
    }

    #[gpui_kit::test]
    fn starts_empty_without_target(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let view = cx.new(InsightView::new);
        view.update(cx, |view, _| {
            assert!(view.target().is_none());
            assert_eq!(view.state(), &InsightPanelState::Empty);
            assert_eq!(view.tab(), PanelTab::Column);
            assert_eq!(
                view.sections_open(),
                [true, true, false, false],
                "四区默认展开态：基础统计与数据分布展开"
            );
        });
    }

    #[gpui_kit::test]
    fn target_switches_tab_and_enters_loading(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let view = cx.new(InsightView::new);
        view.update(cx, |view, cx| view.set_target(column_target(), cx));
        view.update(cx, |view, _| {
            assert_eq!(view.tab(), PanelTab::Column, "列目标应落到「列」Tab");
            assert!(view.state().is_loading(), "取数前先给加载态");
        });

        // 表目标落到「表」Tab（同一份状态机的另一路）
        view.update(cx, |view, cx| {
            view.set_target(
                InsightTarget::Table {
                    temp_table: "t_result_1".into(),
                    table_name: "orders".into(),
                },
                cx,
            );
        });
        view.update(cx, |view, _| {
            assert_eq!(view.tab(), PanelTab::Table);
            assert!(view.state().is_loading());
        });
    }

    #[gpui_kit::test]
    fn profile_data_keeps_section_preferences(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let view = cx.new(InsightView::new);
        view.update(cx, |view, cx| {
            view.set_target(column_target(), cx);
            // 用户把「数据质量」也展开
            view.set_open_sections(&[0, 2], cx);
            assert_eq!(view.sections_open(), [true, false, true, false]);
        });
        view.update(cx, |view, cx| view.set_profile(profile(), cx));
        view.update(cx, |view, _| {
            assert!(view.state().is_data());
            assert_eq!(
                view.sections_open(),
                [true, false, true, false],
                "出数与重算不得清空用户的折叠偏好"
            );
        });
        // ⟳ 重算：回到加载态但偏好不变
        view.update(cx, |view, cx| view.reload(cx));
        view.update(cx, |view, _| {
            assert!(view.state().is_loading());
            assert_eq!(view.sections_open(), [true, false, true, false]);
        });
    }

    #[gpui_kit::test]
    fn error_state_carries_retryability(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let view = cx.new(InsightView::new);
        view.update(cx, |view, cx| {
            view.set_target(column_target(), cx);
            view.set_error("临时表已失效", true, cx);
        });
        view.update(cx, |view, _| {
            assert!(view.state().is_error());
            match view.state() {
                InsightPanelState::Error { retryable, .. } => assert!(*retryable),
                other => panic!("应为错误态，实际 {other:?}"),
            }
        });
        // 目标失效且不可重试时清空
        view.update(cx, |view, cx| {
            view.set_error("结果不存在", false, cx);
            view.clear_target(cx);
        });
        view.update(cx, |view, _| {
            assert_eq!(view.state(), &InsightPanelState::Empty);
            assert!(view.target().is_none());
        });
    }

    #[gpui_kit::test]
    fn set_tab_ignores_out_of_range_index(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let view = cx.new(InsightView::new);
        view.update(cx, |view, cx| {
            view.set_tab_from_index(99, cx);
            assert_eq!(view.tab(), PanelTab::Column, "越界下标不得改变 Tab");
            view.set_tab_from_index(4, cx);
            assert_eq!(view.tab(), PanelTab::History);
        });
    }

    #[gpui_kit::test]
    fn project_state_gates_rules_management(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let view = cx.new(InsightView::new);
        view.update(cx, |view, _| {
            assert!(
                !view.project_open(),
                "保守初值：宿主未告知项目状态前，依赖项目的入口先禁用"
            );
        });
        view.update(cx, |view, cx| view.set_project_open(true, cx));
        view.update(cx, |view, _| assert!(view.project_open()));
        view.update(cx, |view, cx| view.set_project_open(false, cx));
        view.update(cx, |view, _| assert!(!view.project_open()));
    }

    /// 四态 + 五个 Tab 都能渲染一帧而不 panic（渲染是纯读路径，不许有副作用）
    #[gpui_kit::test]
    fn renders_every_state_and_tab(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));

        // 空态（无目标）+ 无项目提示（`project_open` 初值为 false）
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
        // 告知项目已打开：提示行消失、⚙ 可用
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.set_project_open(true, cx));
            window.draw(cx).clear(cx);
        });
        // 加载态
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.set_target(column_target(), cx));
            window.draw(cx).clear(cx);
        });
        // 错误态（可重试 → 有「重试」按钮）
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.set_error("连接已断开", true, cx));
            window.draw(cx).clear(cx);
        });
        // 数据态：列 Tab 四区 + 其余四个 Tab 的期次提示
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.set_target(column_target(), cx);
                view.set_profile(profile(), cx);
            });
            // draw 必须在实体更新之外：渲染要读同一实体，套在 update 里会 double lease
            for tab in PanelTab::ALL {
                view.update(cx, |view, cx| view.set_tab(tab, cx));
                window.draw(cx).clear(cx);
            }
        });
        // 类型未识别（只给计数，不渲染分布）
        cx.update(|window, cx| {
            let unknown = ColumnProfileView::from_domain(&ColumnInsightFull {
                stats: ColumnStats {
                    column_name: "payload".into(),
                    data_type: "BLOB".into(),
                    total_count: 3,
                    null_count: 0,
                    null_rate: 0.0,
                    unique_count: None,
                    stats_detail: ColumnStatsDetail::Unknown,
                },
                sample: Vec::new(),
                histogram: None,
            });
            view.update(cx, |view, cx| {
                view.set_tab(PanelTab::Column, cx);
                view.set_profile(unknown, cx);
            });
            window.draw(cx).clear(cx);
        });
        // 其余类型族的分派（文本 / 时间 / 布尔）各渲染一帧
        for detail in [
            ColumnStatsDetail::Text(TextStats {
                min_length: 1,
                max_length: 9,
                top_values: vec![TextFrequency {
                    value: "paid".into(),
                    count: 8,
                    ratio: 0.8,
                }],
            }),
            ColumnStatsDetail::DateTime(DateTimeStats {
                earliest: "2024-01-01".into(),
                latest: "2024-02-01".into(),
                span_days: 31,
                monthly_distribution: Vec::new(),
            }),
            ColumnStatsDetail::Boolean(BooleanStats {
                true_count: 99,
                false_count: 1,
                true_ratio: 0.99,
            }),
        ] {
            cx.update(|window, cx| {
                let view_model = ColumnProfileView::from_domain(&ColumnInsightFull {
                    stats: ColumnStats {
                        column_name: "status".into(),
                        data_type: "TEXT".into(),
                        total_count: 10,
                        null_count: 0,
                        null_rate: 0.0,
                        unique_count: Some(2),
                        stats_detail: detail,
                    },
                    sample: vec![serde_json::json!("x")],
                    histogram: None,
                });
                view.update(cx, |view, cx| view.set_profile(view_model, cx));
                window.draw(cx).clear(cx);
            });
        }
    }

    /// 表探查（Tab「表」）：四类状态（未评估 / 评估中 / 评估完 / 空列清单）都能渲染一帧。
    ///
    /// 四种都由**同一份视图模型**的不同字段组合而成，所以这里逐帧画过去；
    /// 顺带盖住列名下钻热点（点它切到「列」Tab）。
    #[gpui_kit::test]
    fn table_tab_renders_every_evaluation_state(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
        let draw = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };

        let base = table_view();
        let temp_table = "t_insight_view_table".to_string();
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_target(
                    InsightTarget::Table {
                        temp_table: temp_table.clone(),
                        table_name: "orders".into(),
                    },
                    cx,
                );
                // 1) 刚探查出来：有列清单、没有分数
                view.set_table_profile(base.clone(), cx);
            });
        });
        draw(cx);
        view.update(cx, |view, _| {
            assert_eq!(view.tab(), PanelTab::Table, "表目标应落到「表」Tab");
            let table = view.state().table().expect("表探查数据）；");
            assert!(table.columns.iter().all(|c| c.score.is_none()));
        });

        // 2) 评估中：进度行 + 已算出的分数
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_table_profile(base.with_column_score("id", 95.0).evaluating(1, 3), cx)
            });
        });
        draw(cx);

        // 3) 评估完：表级摘要 + 每列分数
        let quality = TableQuality {
            table_name: "orders".into(),
            overall_score: 72.0,
            level: "良好".into(),
            column_scores: vec![ColumnQualityEntry {
                column_name: "id".into(),
                quality_score: 95.0,
                level: "优秀".into(),
                null_rate: 0.0,
            }],
            summary: "表质量良好 (72分)，3 列已评估".into(),
            scored_count: 3,
            total_columns: 3,
        };
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_table_profile(base.evaluating(3, 3).evaluated(&quality), cx)
            });
        });
        draw(cx);
        view.update(cx, |view, _| {
            let table = view.state().table().unwrap();
            assert!(table.quality.is_some());
            assert!(table.progress.is_none());
        });

        // 4) 空列清单：不渲染半截表头也不 panic
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                let empty = TableProfileView::from_profile(
                    &TableProfile {
                        table_name: temp_table.clone(),
                        db_type: "DuckDB".into(),
                        columns: Vec::new(),
                        row_count: None,
                        schema_name: None,
                    },
                    "empty_table",
                );
                view.set_table_profile(empty, cx);
            });
        });
        draw(cx);
    }

    #[test]
    fn truncate_counts_characters_not_bytes() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
        // 中文按字符计数，不得切出半个字
        assert_eq!(truncate("中文测试", 2), "中文…");
    }
}
