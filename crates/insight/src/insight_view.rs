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
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::radio::Radio;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Size, Sizable as _, Theme};
use gpui_kit::*;

use crate::commands::InsightRefresh;
use crate::model::{
    ColumnKind, ColumnProfileView, DimensionView, DistributionBar, Emphasis, InsightPanelState,
    InsightTarget, MultiColumnView, MultiResultView, MultiRuleView, NoteLevel, PanelData, PanelTab,
    QualityNote, SampleCell, StatRow, TableColumnView, TableProfileView,
};
use crate::quality_scorer::Grade;
use crate::rule_view::RulesView;
use crate::schema_view::{SchemaReportView, SchemaSection, SchemaTone};
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
    /// 请宿主加载多列分析的表单（真实列清单 + `category = multi` 的规则）
    MultiColumnRequested {
        temp_table: String,
        table_name: String,
    },
    /// 请宿主执行一条多列规则（列按选择顺序对位到 `col1` / `col2` …）
    MultiRunRequested {
        temp_table: String,
        rule_id: String,
        columns: Vec<String>,
    },
    /// 请宿主加载 Schema 健康报告（走源库内省，要连接 ID + 库 + schema）
    SchemaReportRequested {
        conn_id: String,
        database: String,
        schema: String,
    },
    /// 请宿主把某张表下钻到表探查。
    ///
    /// 下钻要把**源表**变成面板能分析的临时表（登记临时表是宿主的活），
    /// 因此这里只报「看哪张表」，不自己拼临时表名。
    TableDrilldownRequested {
        conn_id: String,
        database: String,
        schema: String,
        table: String,
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
    /// 各 Tab 的载荷（按 Tab 分开存：切 Tab 不丢别的视角已取到的数据）
    data: PanelData,
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
    /// 「多列」Tab 的选择状态（数据在 `PanelData::Multi` 里，这里只放“用户选了什么”）
    ///
    /// 选中的**列顺序就是参数顺序**（`col1` / `col2` …），所以用 `Vec` 而不是集合。
    multi_selected: Vec<String>,
    multi_rule: Option<String>,
    /// 多列规则执行中（按钮置灰；结果区显示提示）
    multi_running: bool,
    /// 多列执行的失败提示（列表照旧可见：整页转错误态会让用户以为表单坏了）
    multi_notice: Option<String>,
    /// 结构四区的展开态（顺序同 `SchemaSection::ALL`；默认全展开）
    open_schema_sections: [bool; SchemaSection::ALL.len()],
}

impl EventEmitter<InsightEvent> for InsightView {}

impl InsightView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            target: None,
            state: InsightPanelState::Empty,
            data: PanelData::default(),
            tab: PanelTab::Column,
            open_sections: ColumnSection::default_open(),
            // 保守初值：宿主装配时会立刻告知真实项目状态（`set_project_open`）。
            // 宁可让依赖项目的入口先禁用，也不要给一个点了没用的按钮。
            project_open: false,
            // 无 I/O：真正的取数与写库在 `jobs::attach_rules` 接到事件之后
            rules: cx.new(RulesView::new),
            multi_selected: Vec::new(),
            multi_rule: None,
            multi_running: false,
            multi_notice: None,
            open_schema_sections: [true; SchemaSection::ALL.len()],
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

    /// 指向新目标：切到该目标的默认 Tab、清掉旧载荷、进入加载态，并请宿主取数。
    ///
    /// **清载荷是必需的**：载荷按 Tab 分开存，而它们都只对**旧目标**成立——
    /// 留着旧载荷会直接渲染出上一个目标的数据（比空白更坏：看起来像新目标的结果）。
    /// 折叠偏好不在此列：它们是用户口味，不随目标变。
    pub fn set_target(&mut self, target: InsightTarget, cx: &mut Context<Self>) {
        self.tab = target.default_tab();
        self.target = Some(target);
        self.data = PanelData::default();
        self.state = InsightPanelState::Loading;
        if self.emit_request_for_tab(self.tab, cx) {
            cx.notify();
        }
    }

    /// 数据态载荷（宿主与测试读；三个 Tab 各自的最近一次结果）
    pub fn data(&self) -> &PanelData {
        &self.data
    }

    /// 出数（Schema 健康报告）
    pub fn set_schema_report(
        &mut self,
        report: crate::schema_view::SchemaReportView,
        cx: &mut Context<Self>,
    ) {
        self.data = std::mem::take(&mut self.data).with_schema(report);
        self.state = InsightPanelState::Data;
        cx.notify();
    }

    /// 结构 Tab 的表格下钻请求（点报告里的表名）
    pub fn request_table_drilldown(&mut self, table: impl Into<String>, cx: &mut Context<Self>) {
        let Some(InsightTarget::Schema {
            conn_id,
            database,
            schema,
        }) = self.target.clone()
        else {
            return;
        };
        cx.emit(InsightEvent::TableDrilldownRequested {
            conn_id,
            database,
            schema: schema.unwrap_or_default(),
            table: table.into(),
        });
    }

    /// 出数（列画像）
    pub fn set_profile(&mut self, profile: ColumnProfileView, cx: &mut Context<Self>) {
        self.data = std::mem::take(&mut self.data).with_column(profile);
        self.state = InsightPanelState::Data;
        cx.notify();
    }

    /// 出数（表探查）
    pub fn set_table_profile(&mut self, profile: TableProfileView, cx: &mut Context<Self>) {
        self.data = std::mem::take(&mut self.data).with_table(profile);
        self.state = InsightPanelState::Data;
        cx.notify();
    }

    /// 出数（多列表单）：列清单与规则清单换了，选中项要跟着修剪
    ///
    /// 修剪而不是清空：临时表重建后列可能没变，把用户的选择无差别抹掉是坏体验。
    pub fn set_multi_view(&mut self, view: MultiColumnView, cx: &mut Context<Self>) {
        self.multi_selected.retain(|name| view.column(name).is_some());
        if let Some(rule) = self.multi_rule.clone()
            && !view.rules.iter().any(|r| r.id == rule)
        {
            self.multi_rule = None;
        }
        self.multi_selected.dedup();
        self.data = std::mem::take(&mut self.data).with_multi(view);
        self.state = InsightPanelState::Data;
        cx.notify();
    }

    /// 多列规则执行结果（保留表单：用户可以换个规则再跑）
    pub fn set_multi_result(
        &mut self,
        result: MultiResultView,
        notes: Vec<QualityNote>,
        cx: &mut Context<Self>,
    ) {
        self.multi_running = false;
        self.multi_notice = None;
        if let Some(view) = self.data.multi.clone() {
            self.data.multi = Some(view.with_result(result, notes));
        }
        cx.notify();
    }

    /// 多列执行失败：只挂一条提示（表单与已有结果照旧可见）
    pub fn set_multi_notice(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.multi_running = false;
        self.multi_notice = Some(message.into());
        cx.notify();
    }

    /// 多列选择：勾上就排在**末尾**（顺序即参数顺序），再点取消
    pub fn toggle_multi_column(&mut self, column: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.multi_selected.iter().position(|name| name == column) {
            self.multi_selected.remove(ix);
        } else {
            self.multi_selected.push(column.to_string());
        }
        self.multi_notice = None;
        cx.notify();
    }

    pub fn set_multi_rule(&mut self, rule_id: impl Into<String>, cx: &mut Context<Self>) {
        self.multi_rule = Some(rule_id.into());
        self.multi_notice = None;
        cx.notify();
    }

    /// 「执行分析」：校验通过才发请求（校验不过按钮本身就是置灰的，这里兵底）
    pub fn run_multi(&mut self, cx: &mut Context<Self>) {
        let Some(rule_id) = self.multi_rule.clone() else {
            return;
        };
        let Some(temp_table) = self.target.as_ref().map(|t| t.temp_table().to_string()) else {
            return;
        };
        if !self.multi_ready() {
            return;
        }
        self.multi_running = true;
        self.multi_notice = None;
        cx.emit(InsightEvent::MultiRunRequested {
            temp_table,
            rule_id,
            columns: self.multi_selected.clone(),
        });
        cx.notify();
    }

    /// 当前选法能不能跑（列数 + 类型族都满足选中的规则）
    fn multi_ready(&self) -> bool {
        let Some(view) = self.data.multi.as_ref() else {
            return false;
        };
        let Some(rule_id) = self.multi_rule.as_deref() else {
            return false;
        };
        view.rules
            .iter()
            .find(|rule| rule.id == rule_id)
            .is_some_and(|rule| rule.accepts(&view.kinds_of(&self.multi_selected)))
    }

    /// 选中列（供宿主与测试读）
    pub fn multi_selection(&self) -> &[String] {
        &self.multi_selected
    }

    pub fn multi_rule_id(&self) -> Option<&str> {
        self.multi_rule.as_deref()
    }

    pub fn multi_running(&self) -> bool {
        self.multi_running
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
        if let Some(profile) = &self.data.table {
            let total = profile.columns.len();
            let next = profile.evaluating(0, total);
            self.data.table = Some(next);
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
        self.data = PanelData::default();
        self.state = InsightPanelState::Empty;
        cx.notify();
    }

    /// ⟳ 与「重试」共用：重新加载**当前 Tab 看的东西**（保持 Tab 与折叠偏好）。无目标时不动。
    ///
    /// 按 Tab 而不是按目标种类发：用户在「表」Tab 上点 ⟳，想重算的是表探查，
    /// 而不是回到列画像。
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if self.target.is_none() {
            return;
        }
        self.state = InsightPanelState::Loading;
        if self.emit_request_for_tab(self.tab, cx) {
            cx.notify();
        }
    }

    /// 按 Tab 发对应的取数请求（**不管载荷在不在**：要不要发由调用方判）。
    ///
    /// 返回是否真的发了（目标种类与 Tab 不匹配时不发——比如拿列目标去要结构报告）。
    fn emit_request_for_tab(&mut self, tab: PanelTab, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.target.clone() else {
            return false;
        };
        // 列目标才需要列名：表 / 多列目标没有「哪一列」这回事，取整表
        let request = match tab {
            PanelTab::Column => match &target {
                InsightTarget::Column { .. } => InsightEvent::ProfileRequested { target },
                _ => return false,
            },
            PanelTab::Table => InsightEvent::ProfileRequested {
                target: InsightTarget::Table {
                    temp_table: target.temp_table().to_string(),
                    table_name: target.table_name(),
                },
            },
            PanelTab::MultiColumn => InsightEvent::MultiColumnRequested {
                temp_table: target.temp_table().to_string(),
                table_name: target.table_name(),
            },
            PanelTab::Schema => match &target {
                InsightTarget::Schema {
                    conn_id,
                    database,
                    schema,
                } => InsightEvent::SchemaReportRequested {
                    conn_id: conn_id.clone(),
                    database: database.clone(),
                    schema: schema.clone().unwrap_or_default(),
                },
                _ => return false,
            },
            PanelTab::History => return false,
        };
        cx.emit(request);
        true
    }

    /// 切 Tab：切过去就补一次取数（事件路径）。
    ///
    /// 「切到某个 Tab」= 「我要看这份数据」：载荷按 Tab 分开存，缺的那一份才请求，
    /// 已有的一份直接渲染（切回来仍看得到上次结果，折叠偏好也保留）。
    ///
    /// 不能放在 render 里做：那会造成「渲染一次发一次请求」。
    pub fn set_tab(&mut self, tab: PanelTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            cx.notify();
        }
        self.ensure_data_for_tab(tab, cx);
    }

    /// 补齐当前 Tab 需要的数据（事件路径调用）。
    ///
    /// 判定只看**该 Tab 的载荷在不在**：不在就取。不拿 `Loading` 当“正在取数”的挡板——
    /// 目标是表时先到的是表载荷，此时切到「多列」拉不到东西（载荷按 Tab 分开存，
    /// 所以“正在加载”并不代表“这个 Tab 正在加载”）。重复点同一 Tab 最多多发一次
    /// 幂等的内省，而 `is_error()` 不抢跑（错误态的入口是「重试」）。
    fn ensure_data_for_tab(&mut self, tab: PanelTab, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        if self.state.is_error() {
            return;
        }
        let missing = match tab {
            PanelTab::Column => self.data.column.is_none(),
            PanelTab::Table => self.data.table.is_none(),
            PanelTab::MultiColumn => !self
                .data
                .multi
                .as_ref()
                .is_some_and(|view| view.is_for(target.temp_table())),
            PanelTab::Schema => self.data.schema.is_none(),
            PanelTab::History => false,
        };
        if !missing {
            return;
        }
        self.state = InsightPanelState::Loading;
        if self.emit_request_for_tab(tab, cx) {
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
        if let Some(profile) = &self.data.column {
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
        if let Some(profile) = &self.data.table {
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
        self.data
            .column
            .as_ref()
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

    /// 内容主体。图标尺寸由 `render` 从 `rem_size()` 换算后传入（视图不写裸 px）。
    ///
    /// 渲染以**载荷**为准，状态只管两种例外：错误态整页接管；没载荷时才看
    /// 加载中（骨架）还是空态（入口提示）。这样「切到另一个 Tab 时后台还在取数」
    /// 不会把已取到的那个 Tab 的内容盖成骨架。
    fn render_body(
        &self,
        entity: &Entity<Self>,
        theme: &Theme,
        empty_icon: Pixels,
        inline_icon: Pixels,
    ) -> Vec<AnyElement> {
        if let InsightPanelState::Error { message, retryable } = &self.state {
            return vec![
                error_block(message, *retryable, entity.clone(), theme, inline_icon)
                    .into_any_element(),
            ];
        }

        let payload: Option<AnyElement> = match self.tab {
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
            // 后续期次落地：没有载荷也没有骨架，直接给期次提示
            PanelTab::History => None,
        };
        if let Some(element) = payload {
            return vec![element];
        }
        if self.state.is_loading() {
            return vec![skeleton(theme).into_any_element()];
        }
        vec![empty_state(IconName::Info, self.empty_hint(), theme, empty_icon).into_any_element()]
    }

    /// 同步结构四区的展开态（与列画像同口径：Accordion 只给「当前展开的下标集合」）
    fn set_open_schema_sections(&mut self, open: &[usize], cx: &mut Context<Self>) {
        for (i, slot) in self.open_schema_sections.iter_mut().enumerate() {
            *slot = open.contains(&i);
        }
        cx.notify();
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
        let score = self.data.column.as_ref()?.score.as_ref()?;
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

    /// 多列分析（Tab「多列」，Phase 3.3）：列多选 + 规则 + 执行 + 结果。
    ///
    /// 选择顺序就是参数顺序（`col1` / `col2` …），所以选中的列带序号——
    /// 没有序号的话「哪列进 col1」全靠猜，而方向性规则（相关系数）是不对称的。
    fn render_multi_view(
        &self,
        view: &MultiColumnView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let mut body = div().v_flex().w_full().gap_2();

        // 1) 列多选
        body = body.child(section_title("选择列（顺序即参数顺序）", theme));
        if view.columns.is_empty() {
            body = body.child(muted_line("该结果集没有可分析的列", theme));
        }
        for column in &view.columns {
            let picked = self
                .multi_selected
                .iter()
                .position(|name| name.as_str() == column.name.as_str())
                .map(|ix| ix + 1);
            body = body.child(multi_column_row(
                column,
                picked,
                entity,
                theme,
                inline_icon,
            ));
        }

        // 2) 规则单选
        body = body.child(section_title("选择规则", theme));
        if view.rules.is_empty() {
            body = body.child(muted_line(
                "没有可用的多列规则——可在面板头 ⚙ 里检查规则是否被禁用",
                theme,
            ));
        }
        let kinds = view.kinds_of(&self.multi_selected);
        for rule in &view.rules {
            let usable = rule.accepts(&kinds);
            body = body.child(multi_rule_row(
                rule,
                self.multi_rule.as_deref() == Some(rule.id.as_str()),
                usable,
                entity,
                theme,
            ));
        }

        // 3) 执行（不可用时置灰；原因由规则行的类型提示给出）
        body = body.child(
            div().h_flex().w_full().gap_2().child(div().flex_1()).child(
                Button::new("insight-multi-run")
                    .small()
                    .label(if self.multi_running { "分析中…" } else { "执行分析" })
                    .disabled(!self.multi_ready() || self.multi_running)
                    .tooltip("按选中顺序把列对位到规则的 col1 / col2 …")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| entity.update(app, |view, cx| view.run_multi(cx))
                    }),
            ),
        );

        // 4) 失败提示（表单照旧可见）
        if let Some(notice) = &self.multi_notice {
            body = body.child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.danger)
                    .child(Icon::new(IconName::TriangleAlert).size(inline_icon))
                    .child(div().flex_1().min_w_0().child(notice.clone())),
            );
        }

        // 5) 结果
        match &view.result {
            None => {
                body = body.child(div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("选好列与规则后点「执行分析」"));
            }
            Some(result) if result.is_empty() => {
                body = body.child(muted_line("规则没有返回数据", theme));
            }
            Some(MultiResultView::Single(rows)) => {
                body = body.child(section_title("结果", theme));
                for row in rows {
                    body = body.child(
                        div()
                            .h_flex()
                            .w_full()
                            .gap_2()
                            .py_0p5()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .text_ellipsis()
                                    .child(row.label.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_sm()
                                    .text_color(colors.foreground)
                                    .child(row.value.clone()),
                            ),
                    );
                }
            }
            Some(MultiResultView::Table { headers, rows }) => {
                body = body.child(section_title("结果", theme));
                body = body.child(multi_table_header(headers, theme));
                for row in rows {
                    body = body.child(multi_table_row(row, theme));
                }
            }
        }

        // 6) 质量门控提示（只有未过的项）
        for note in &view.notes {
            body = body.child(
                div()
                    .h_flex()
                    .w_full()
                    .gap_1()
                    .text_xs()
                    .text_color(colors.warning)
                    .child(Icon::new(IconName::TriangleAlert).size(inline_icon))
                    .child(div().flex_1().min_w_0().child(note.text.clone())),
            );
        }

        body
    }

    /// 结构洞察（Tab「结构」，Phase 4.2）：健康条 + 四个折叠区 + 表名下钻。
    ///
    /// 四个区**都渲染**（哪怕为空）：空区的意义是「检查过、没问题」，
    /// 直接隐藏会让人以为这项没做。
    fn render_schema_report(
        &self,
        report: &SchemaReportView,
        entity: &Entity<Self>,
        theme: &Theme,
        inline_icon: Pixels,
    ) -> Div {
        let colors = theme.colors;
        let color = grade_color(report.grade, theme);

        // 健康条：分数是这张报告的头号结论
        let body = div().v_flex().w_full().gap_2().child(
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
                                .text_size(rems(ui::INSIGHT_HEALTH_SCORE_FONT))
                                .text_color(color)
                                .child(format!("{:.0}", report.health)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(color)
                                .child(report.grade.label()),
                        )
                        .child(div().flex_1().min_w_0().text_xs().text_ellipsis().child(
                            format!(
                                "{} 表 · {} 列 · 需关注 {} 项",
                                report.table_count,
                                report.total_columns,
                                report.issue_count()
                            ),
                        )),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(report.summary.clone()),
                ),
        );

        // 四个折叠区（默认全展开：报告本身不长，折起来反而多点一下）
        let mut accordion = Accordion::new("insight-schema-sections")
            .multiple(true)
            .with_size(Size::Small)
            .on_toggle_click({
                let entity = entity.clone();
                move |open, _window, app| {
                    entity.update(app, |view, cx| view.set_open_schema_sections(open, cx));
                }
            });
        for (ix, section) in SchemaSection::ALL.iter().enumerate() {
            let group = report.group(*section);
            let open = self.open_schema_sections[ix];
            let title = schema_section_title(*section, group.map_or(0, |g| g.rows.len()), theme);
            accordion = accordion.item(|item| {
                item.title(title).open(open).children({
                    match group {
                        Some(group) if !group.is_empty() => group
                            .rows
                            .iter()
                            .map(|row| schema_row(row, entity, theme, inline_icon))
                            .collect::<Vec<_>>(),
                        _ => vec![muted_line(section.empty_hint(), theme).into_any_element()],
                    }
                })
            });
        }

        body.child(accordion)
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

// ==================== 多列分析的片段（Phase 3.3） ====================

fn section_title(text: &str, theme: &Theme) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 多列 Tab 的一列：勾选 + 序号 + 列名 + 类型徐标
fn multi_column_row(
    column: &TableColumnView,
    picked: Option<usize>,
    entity: &Entity<InsightView>,
    theme: &Theme,
    inline_icon: Pixels,
) -> Div {
    let colors = theme.colors;
    let name = column.name.clone();
    let row = div()
        .h_flex()
        .w_full()
        .gap_2()
        .h(rems(ui::ROW_HEIGHT))
        .px_1()
        .child(
            Checkbox::new(ElementId::Name(
                format!("insight-multi-col-{}", column.name).into(),
            ))
            .checked(picked.is_some())
            .on_click({
                let entity = entity.clone();
                let name = name.clone();
                move |_, _, app| {
                    entity.update(app, |view, cx| view.toggle_multi_column(&name, cx))
                }
            }),
        )
        .child(
            // 序号：顺序即参数顺序（col1 / col2 …）
            div()
                .flex_none()
                .w(rems(ui::INSIGHT_TABLE_INDEX_WIDTH))
                .text_xs()
                .text_color(if picked.is_some() {
                    colors.primary
                } else {
                    colors.muted_foreground
                })
                .child(match picked {
                    Some(ix) => format!("{ix}."),
                    None => "·".to_string(),
                }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(colors.foreground)
                .text_ellipsis()
                .child(column.name.clone()),
        )
        .child(kind_badge(column.kind, theme));
    if column.kind == ColumnKind::Unknown {
        return row.child(
            Icon::new(IconName::Info)
                .size(inline_icon)
                .text_color(colors.muted_foreground),
        );
    }
    row
}

/// 多列 Tab 的规则行：单选框 + 名称 + 类型提示（不可用时压暗并说明原因）
fn multi_rule_row(
    rule: &MultiRuleView,
    selected: bool,
    usable: bool,
    entity: &Entity<InsightView>,
    theme: &Theme,
) -> Div {
    let colors = theme.colors;
    let mut row = div()
        .h_flex()
        .w_full()
        .gap_2()
        .h(rems(ui::ROW_HEIGHT))
        .px_1()
        .child(
            Radio::new(ElementId::Name(
                format!("insight-multi-rule-{}", rule.id).into(),
            ))
            .checked(selected)
            .disabled(!usable)
            .on_click({
                let entity = entity.clone();
                let id = rule.id.clone();
                move |_, _, app| entity.update(app, |view, cx| view.set_multi_rule(id.clone(), cx))
            }),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(if usable {
                    colors.foreground
                } else {
                    colors.muted_foreground
                })
                .text_ellipsis()
                .child(rule.name.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(rule.types_hint()),
        );
    if !usable {
        // 不可用不是因为坏了，而是“这几列不对”：把原因写出来，别让点不动成为谜
        let need = if rule.arity() == 0 {
            "无需列".to_string()
        } else {
            format!("需 {} 列：{}", rule.arity(), rule.types_hint())
        };
        row = row.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(need),
        );
    }
    row
}

/// 列表结果的表头（列数动态：每列等宽，长文本省略尾）
fn multi_table_header(headers: &[String], theme: &Theme) -> Div {
    let colors = theme.colors;
    let mut row = div()
        .h_flex()
        .w_full()
        .gap_2()
        .px_1()
        .py_0p5()
        .bg(colors.list_hover)
        .text_xs()
        .text_color(colors.muted_foreground);
    for header in headers {
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                .child(header.clone()),
        );
    }
    row
}

fn multi_table_row(cells: &[String], theme: &Theme) -> Div {
    let colors = theme.colors;
    let mut row = div().h_flex().w_full().gap_2().px_1().py_0p5().text_xs();
    for (ix, cell) in cells.iter().enumerate() {
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .text_ellipsis()
                // 第一列是行标签（如交叉频次表的行值）：给正常前景色，其余列压一级
                .text_color(if ix == 0 {
                    colors.foreground
                } else {
                    colors.muted_foreground
                })
                .child(cell.clone()),
        );
    }
    row
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

// ==================== 结构洞察的片段（Phase 4.2） ====================

/// 分区标题：`外键候选 (3)`
fn schema_section_title(section: SchemaSection, count: usize, theme: &Theme) -> Div {
    div()
        .h_flex()
        .gap_2()
        .child(
            div()
                .text_sm()
                .text_color(theme.colors.foreground)
                .child(section.title()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(format!("({count})")),
        )
}

/// 报告的一行：主文案 + 说明 + 可下钻的表名热点
fn schema_row(
    row: &crate::schema_view::SchemaRowView,
    entity: &Entity<InsightView>,
    theme: &Theme,
    inline_icon: Pixels,
) -> AnyElement {
    let colors = theme.colors;
    let (tone_color, icon) = match row.tone {
        SchemaTone::Normal => (colors.muted_foreground, IconName::Info),
        SchemaTone::Warning => (colors.warning, IconName::TriangleAlert),
        SchemaTone::Danger => (colors.danger, IconName::TriangleAlert),
    };

    let mut body = div()
        .v_flex()
        .w_full()
        .gap_1()
        .py_1()
        .child(
            div()
                .h_flex()
                .gap_1()
                .text_sm()
                .text_color(colors.foreground)
                .child(Icon::new(icon).size(inline_icon).text_color(tone_color))
                .child(div().flex_1().min_w_0().text_ellipsis().child(row.title.clone())),
        )
        .child(
            div()
                .pl_4()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(row.detail.clone()),
        );

    // 涉及的表 → 下钻热点（最多几个，长清单会让面板变成一条链接列表）
    if !row.tables.is_empty() {
        let mut links = div().h_flex().flex_wrap().gap_1().pl_4();
        for table in row.tables.iter().take(ui::INSIGHT_SCHEMA_DRILLDOWN_LIMIT) {
            links = links.child(
                Button::new(ElementId::Name(
                    format!("insight-schema-to-{}", table).into(),
                ))
                .ghost()
                .xsmall()
                .label(table.clone())
                .tooltip("看这张表的表探查")
                .on_click({
                    let entity = entity.clone();
                    let table = table.clone();
                    move |_, _, app| {
                        entity.update(app, |view, cx| view.request_table_drilldown(table.clone(), cx))
                    }
                }),
            );
        }
        body = body.child(links);
    }
    body.into_any_element()
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

    use super::{truncate, InsightEvent, InsightView};
    use crate::model::{
        ColumnProfileView, InsightPanelState, InsightTarget, MultiColumnView, MultiResultView,
        MultiRuleView, PanelTab, TableProfileView,
    };
    // 领域类型从 crate 根再导出引用（`model.rs` 里对 `types` 的 `use` 是私有的）
    use crate::{
        BooleanStats, ColumnInsightFull, ColumnQualityEntry, ColumnStats, ColumnStatsDetail,
        DateTimeStats, DistributionBin, KeyValueRow, NoteLevel, NumericStats, QualityNote,
        TableColumnMeta, TableProfile, TableQuality, TextFrequency, TextStats,
    };

    /// 结构报告的典型形态：四区各一行（外键候选 / critical 不一致 / 孤立表 / 冗余列）
    fn schema_report() -> crate::schema_view::SchemaReportView {
        use crate::schema_analyzer::{
            ForeignKeyCandidate, OrphanTable, RedundantColumn, SchemaInsightReport, TypeMismatch,
            TypeMismatchEntry,
        };
        crate::schema_view::SchemaReportView::from_report(&SchemaInsightReport {
            schema_name: "public".into(),
            table_count: 4,
            total_columns: 21,
            fk_candidates: vec![ForeignKeyCandidate {
                source_table: "orders".into(),
                source_column: "user_id".into(),
                target_table: "users".into(),
                target_column: "id".into(),
                confidence: "high".into(),
                naming_pattern: "{table}_id".into(),
            }],
            type_mismatches: vec![TypeMismatch {
                column_name: "status".into(),
                tables: vec![
                    TypeMismatchEntry {
                        table_name: "orders".into(),
                        data_type: "VARCHAR".into(),
                    },
                    TypeMismatchEntry {
                        table_name: "users".into(),
                        data_type: "INTEGER".into(),
                    },
                ],
                severity: "critical".into(),
            }],
            orphan_tables: vec![OrphanTable {
                table_name: "logs".into(),
                column_count: 5,
                reason: "没有关联".into(),
            }],
            redundant_columns: vec![RedundantColumn {
                column_name: "created_at".into(),
                table_count: 3,
                tables: vec!["orders".into(), "users".into(), "logs".into()],
                suggestion: "考虑抽到公共表".into(),
            }],
            summary: "Schema健康评分 68 (一般)。4 张表, 21 个列".into(),
            health_score: 68.0,
            health_level: "一般".into(),
        })
    }

    /// 多列分析的典型形态：三列（数值 / 数值 / 文本）+ 两条规则（一条吃两数值、一条吃两文本）
    fn multi_view() -> MultiColumnView {
        let rule = |id: &str, name: &str, family: &str| MultiRuleView {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            applies_to: vec![family.into(), family.into()],
            column_params: vec!["col1".into(), "col2".into()],
            result_type: None,
        };
        MultiColumnView::from_profile(
            &TableProfile {
                table_name: "t_multi_tab".into(),
                db_type: "DuckDB".into(),
                columns: vec![
                    TableColumnMeta {
                        column_name: "id".into(),
                        data_type: "BIGINT".into(),
                        is_nullable: false,
                        is_primary_key: false,
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
                        column_name: "note".into(),
                        data_type: "VARCHAR".into(),
                        is_nullable: true,
                        is_primary_key: false,
                        ordinal_position: 3,
                    },
                ],
                row_count: Some(4),
                schema_name: None,
            },
            "orders",
            vec![
                rule("pair-numeric", "数值对相关系数", "Numeric"),
                rule("pair-text", "文本交叉频次", "Text"),
            ],
        )
    }

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
            let table = view.data().table.as_ref().expect("表探查数据）；");
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
            let table = view.data().table.as_ref().unwrap();
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

    /// 多列 Tab：切过去才取数（事件路径），同一临时表不重复取
    #[gpui_kit::test]
    fn switching_to_the_multi_tab_loads_the_form(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
        let events = crate::test_support::event_sink(&view, cx);

        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_target(
                    InsightTarget::Table {
                        temp_table: "t_multi_tab".into(),
                        table_name: "orders".into(),
                    },
                    cx,
                );
            });
        });
        // 先到达表探查数据（真实接线会在稍后回填）
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_table_profile(table_view(), cx))
        });

        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_tab(PanelTab::MultiColumn, cx))
        });
        assert!(
            view.read_with(cx, |view, _| view.state().is_loading()),
            "切到还没数据的 Tab 应先进加载态"
        );
        assert!(
            events.borrow().iter().any(|e| matches!(
                e,
                InsightEvent::MultiColumnRequested { temp_table, .. } if temp_table == "t_multi_tab"
            )),
            "切 Tab 应发一次取数请求：{:?}",
            events.borrow()
        );

        // 数据回填后再切回来：不再重复请求（这份数据就是这个临时表的）
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_multi_view(multi_view(), cx))
        });
        events.take();
        cx.update(|_window, cx| view.update(cx, |view, cx| view.set_tab(PanelTab::Table, cx)));
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_tab(PanelTab::MultiColumn, cx))
        });
        assert!(
            !events
                .borrow()
                .iter()
                .any(|e| matches!(e, InsightEvent::MultiColumnRequested { .. })),
            "同一临时表的数据已就位就不该再请求多列表单（切回「表」Tab 重取表探查是另一回事）：{:?}",
            events.borrow()
        );
        assert!(!view.read_with(cx, |view, _| view.state().is_loading()));
    }

    /// 多列选择：顺序即参数顺序；执行要过“列数 + 类型族”两道判定
    #[gpui_kit::test]
    fn multi_selection_order_drives_the_run_request(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
        let events = crate::test_support::event_sink(&view, cx);
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_target(
                    InsightTarget::Table {
                        temp_table: "t_multi_run".into(),
                        table_name: "orders".into(),
                    },
                    cx,
                );
                view.set_multi_view(multi_view(), cx);
            });
        });
        // `set_target` 自己会发一次画像请求：断言“不发请求”之前先把它清掉
        events.take();

        // 先选后发的顺序：amount → id（相关性是有方向的，顺序不能乱）
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.toggle_multi_column("amount", cx);
                view.toggle_multi_column("id", cx);
            });
        });
        assert_eq!(
            view.read_with(cx, |view, _| view.multi_selection().to_vec()),
            vec!["amount".to_string(), "id".to_string()]
        );

        // 规则没选时不能跑
        cx.update(|_window, cx| view.update(cx, |view, cx| view.run_multi(cx)));
        assert!(events.borrow().is_empty(), "没选规则不该发请求");

        // 选一条数值规则 → 可跑
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_multi_rule("pair-numeric", cx))
        });
        cx.update(|_window, cx| view.update(cx, |view, cx| view.run_multi(cx)));
        assert!(view.read_with(cx, |view, _| view.multi_running()));
        assert!(
            events.borrow().iter().any(|e| matches!(
                e,
                InsightEvent::MultiRunRequested { rule_id, columns, .. }
                    if rule_id == "pair-numeric" && columns.as_slice() == ["amount".to_string(), "id".to_string()]
            )),
            "应带选中顺序的列：{:?}",
            events.borrow()
        );

        // 换成一条只吃文本的规则：列不匹配 → 置灰且发不出请求
        events.take();
        // 先把上一次跑到一半的请求收尾（真实接线里由接缝回填，这里手动完成），
        // 否则 multi_running 会一直是 true，下面的断言就测不到东西
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_multi_result(
                    MultiResultView::Single(vec![KeyValueRow {
                        label: "correlation".into(),
                        value: "1".into(),
                    }]),
                    Vec::new(),
                    cx,
                )
            });
        });
        assert!(!view.read_with(cx, |view, _| view.multi_running()));
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_multi_rule("pair-text", cx);
                view.run_multi(cx);
            });
        });
        assert!(events.borrow().is_empty(), "类型不匹配不该发请求");
        assert!(!view.read_with(cx, |view, _| view.multi_running()));
    }

    /// 结果与失败：写结果不动表单；失败只挂提示（表单与已有结果照旧可见）
    #[gpui_kit::test]
    fn multi_result_keeps_the_form_and_failures_only_notify(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_target(
                    InsightTarget::Table {
                        temp_table: "t_multi_result".into(),
                        table_name: "orders".into(),
                    },
                    cx,
                );
                view.set_multi_view(multi_view(), cx);
                view.toggle_multi_column("amount", cx);
                view.set_multi_rule("pair-numeric", cx);
            });
        });

        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_multi_result(
                    MultiResultView::Single(vec![KeyValueRow {
                        label: "correlation".into(),
                        value: "0.98".into(),
                    }]),
                    vec![QualityNote {
                        level: NoteLevel::Warning,
                        text: "样本量偏少".into(),
                    }],
                    cx,
                )
            });
        });
        view.update(cx, |view, _| {
            let multi = view.data().multi.as_ref().expect("应仍是多列数据");
            assert!(multi.result.is_some());
            assert_eq!(multi.notes.len(), 1);
            assert_eq!(multi.columns.len(), 3, "写结果不动列清单");
            assert_eq!(view.multi_selection().len(), 1, "写结果不动选择");
            assert!(!view.multi_running());
        });

        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_multi_notice("列 [qty] 不存在", cx))
        });
        view.update(cx, |view, _| {
            let multi = view.data().multi.as_ref().expect("失败不该清掉表单");
            assert!(multi.result.is_some(), "已有结果也不该被清");
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));

        // 数据刷新会修剪选择：消失的列不再被选中，消失的规则不再被选
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                let mut next = multi_view();
                next.columns.retain(|c| c.name != "amount");
                next.rules.retain(|r| r.id != "pair-numeric");
                view.set_multi_view(next, cx);
            });
        });
        view.update(cx, |view, _| {
            assert!(view.multi_selection().is_empty(), "消失的列要从选择里摘掉");
            assert_eq!(view.multi_rule_id(), None, "消失的规则也要摘掉");
        });
    }

    /// 多列 Tab：表单、单值结果、表格结果、失败提示都要能渲染一帧
    #[gpui_kit::test]
    fn multi_tab_renders_form_results_and_notice(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
        let draw = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, cx| window.draw(cx).clear(cx));
        };

        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_target(
                    InsightTarget::Table {
                        temp_table: "t_multi_tab".into(),
                        table_name: "orders".into(),
                    },
                    cx,
                );
                view.set_tab(PanelTab::MultiColumn, cx);
                // 1) 空表单（没有列、没有规则）：不能渲染出半截控件
                view.set_multi_view(MultiColumnView::empty(), cx);
            });
        });
        draw(cx);

        // 2) 正常表单：未选任何列 / 规则
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_multi_view(multi_view(), cx))
        });
        draw(cx);

        // 3) 选中两列 + 一条规则（序号与单选框都要画出来）
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.toggle_multi_column("amount", cx);
                view.toggle_multi_column("note", cx);
                view.set_multi_rule("pair-numeric", cx);
            });
        });
        draw(cx);

        // 4) 单值结果 + 门控提示
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_multi_result(
                    MultiResultView::Single(vec![
                        KeyValueRow {
                            label: "correlation".into(),
                            value: "0.98".into(),
                        },
                        KeyValueRow {
                            label: "sample_size".into(),
                            value: "120".into(),
                        },
                    ]),
                    vec![QualityNote {
                        level: NoteLevel::Warning,
                        text: "样本量偏少".into(),
                    }],
                    cx,
                )
            });
        });
        draw(cx);

        // 5) 表格结果（列数动态）
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_multi_result(
                    MultiResultView::Table {
                        headers: vec!["row_label".into(), "cn".into(), "us".into()],
                        rows: vec![
                            vec!["paid".into(), "2".into(), "1".into()],
                            vec!["free".into(), "—".into(), "1".into()],
                        ],
                    },
                    Vec::new(),
                    cx,
                )
            });
        });
        draw(cx);

        // 6) 失败提示：表单与已有结果都还在
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_multi_notice("列 [qty] 不存在", cx))
        });
        draw(cx);
        view.update(cx, |view, _| {
            assert!(view.data().multi.as_ref().unwrap().result.is_some());
            assert_eq!(view.multi_selection().len(), 2);
        });
    }

    /// 结构 Tab：切过去才发请求；报告回填后可下钻（下钻只报「看哪张表」）
    #[gpui_kit::test]
    fn schema_tab_requests_the_report_and_drills_down(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
        let events = crate::test_support::event_sink(&view, cx);

        cx.update(|_window, cx| {
            view.update(cx, |view, cx| {
                view.set_target(
                    InsightTarget::Schema {
                        conn_id: "G_pg".into(),
                        database: "shop".into(),
                        schema: Some("public".into()),
                    },
                    cx,
                );
            });
        });
        assert_eq!(
            view.read_with(cx, |view, _| view.tab()),
            PanelTab::Schema,
            "结构目标直接落到「结构」Tab"
        );
        assert!(
            events.borrow().iter().any(|e| matches!(
                e,
                InsightEvent::SchemaReportRequested { conn_id, database, schema }
                    if conn_id == "G_pg" && database == "shop" && schema == "public"
            )),
            "结构目标要带齐连接 / 库 / schema：{:?}",
            events.borrow()
        );
        events.take();

        // 回填报告后可渲染（四区 + 健康条），下钻只报源表
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_schema_report(schema_report(), cx))
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.request_table_drilldown("orders", cx))
        });
        assert!(
            events.borrow().iter().any(|e| matches!(
                e,
                InsightEvent::TableDrilldownRequested { table, conn_id, database, schema }
                    if table == "orders" && conn_id == "G_pg" && database == "shop" && schema == "public"
            )),
            "下钻要带靶表的定位信息（登记临时表是宿主的活）：{:?}",
            events.borrow()
        );

        // 四区展开态可同步（Accordion 给的是当前展开的下标集合）
        cx.update(|_window, cx| {
            view.update(cx, |view, cx| view.set_open_schema_sections(&[1, 3], cx))
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        view.update(cx, |view, _| {
            assert_eq!(
                view.data().as_schema().map(|report| report.table_count),
                Some(4)
            );
        });
    }

    #[test]
    fn truncate_counts_characters_not_bytes() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 3), "abc…");
        // 中文按字符计数，不得切出半个字
        assert_eq!(truncate("中文测试", 2), "中文…");
    }
}
