//! 右 Dock 洞察面板的**状态宿主**（M8 Phase 1 落地；视图片段见 [`crate::view`]）。
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
//! - **不持色值**：模型给语义（`Emphasis` / `NoteLevel` / `ColumnKind`），视图侧映射到
//!   `theme.colors.*`。
//! - **渲染零副作用**：点击只改自身状态 + `emit` 事件，宿主收到事件后再动数据。
//!
//! 本文件只放**状态**：目标 / 五态 / 各 Tab 载荷 / 折叠与选择，以及把它们推进去的那些方法。
//! 渲染按 Tab 拆在 `crate::view` 下（位移规格见
//! `docs/architecture/insight/insight-dev-plan.md` §11）——视图侧读得到字段，所以字段是
//! `pub(crate)`；对外 API 仍是那些 `pub` 方法。
//!
//! 视图归属：本文件按 **方案 A**（视图入 `crates/insight`）落地，宿主桥由 workbench
//! 侧订阅 [`InsightEvent`] 装配。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
use gpui_kit::*;

use crate::commands::InsightRefresh;
use crate::model::{
    ColumnProfileView, HistoryView, InsightPanelState, InsightTarget, MultiColumnView,
    MultiResultView, PanelData, PanelTab, QualityNote, TableProfileView,
    SNAPSHOT_RETENTION_DAYS,
};
use crate::rule_view::RulesView;
use crate::schema_view::{SchemaExportFormat, SchemaSection};
use crate::ui;

#[cfg(test)]
mod tests;

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
    /// 请宿主保存当前列的一次快照（重取领域画像 + 双写 + 回填历史）
    SnapshotSaveRequested {
        temp_table: String,
        column: String,
        /// 取样来源描述（源目标才有）：进快照的 `entity_source`，
        /// 否则快照存的是 `temp_table=tmp_i_…`——一个过了 30 分钟就没人认识的随机名。
        source_label: Option<String>,
    },
    /// 请宿主读取某列的历次快照
    HistoryRequested { column: String },
    /// 请宿主对比某一版与最新一版（读两版正文，方向固定为「选中 → 最新」）
    VersionCompareRequested { column: String, version_id: String },
    /// 请宿主清理旧快照（按保留天数删正文与版本链，成对删）
    SnapshotCleanupRequested { column: String },
    /// 请宿主把结构报告存成文件（Phase 4.3：选路径 + 写文件）。
    ///
    /// `content` 由面板算好（导出的是「用户看到的这份结论」，D36）——宿主只选路径与写文件，
    /// 不必知道 JSON 的分组键或 Markdown 的转义规则。
    SchemaExportRequested {
        format: SchemaExportFormat,
        /// 默认文件名主名（不含扩展名，已净化）
        file_stem: String,
        content: String,
    },
}

/// 列画像四区（顺序即渲染顺序）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColumnSection {
    Basics,
    Distribution,
    Quality,
    Sample,
}

impl ColumnSection {
    pub(crate) const ALL: [ColumnSection; 4] = [
        ColumnSection::Basics,
        ColumnSection::Distribution,
        ColumnSection::Quality,
        ColumnSection::Sample,
    ];

    pub(crate) fn title(self) -> &'static str {
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
    pub(crate) target: Option<InsightTarget>,
    pub(crate) state: InsightPanelState,
    /// 各 Tab 的载荷（按 Tab 分开存：切 Tab 不丢别的视角已取到的数据）
    pub(crate) data: PanelData,
    pub(crate) tab: PanelTab,
    /// 四区折叠态：属用户偏好，⟳ 重算与切换 Tab 都不得清空
    pub(crate) open_sections: [bool; 4],
    /// 未打开项目（宿主告知）。
    ///
    /// 无项目时画像仍可算（临时表在内存里），但**规则管理与快照不可用**——
    /// 它们都落在项目目录下（原型 §4 / 架构 §8）。
    pub(crate) project_open: bool,
    /// 规则管理对话框（Phase 2.3）。
    ///
    /// 随面板在构造期创建：对话框数据靠 `jobs::attach_rules` 后台取，
    /// 而「开关一条规则」与面板数据同属一个会话，实体提前存在才接得上订阅。
    pub(crate) rules: Entity<RulesView>,
    /// 「多列」Tab 的选择状态（数据在 `PanelData::Multi` 里，这里只放“用户选了什么”）
    ///
    /// 选中的**列顺序就是参数顺序**（`col1` / `col2` …），所以用 `Vec` 而不是集合。
    pub(crate) multi_selected: Vec<String>,
    pub(crate) multi_rule: Option<String>,
    /// 多列规则执行中（按钮置灰；结果区显示提示）
    pub(crate) multi_running: bool,
    /// 多列执行的失败提示（列表照旧可见：整页转错误态会让用户以为表单坏了）
    pub(crate) multi_notice: Option<String>,
    /// 快照保存中（按钮置灰；历史列表照旧可见）
    pub(crate) history_saving: bool,
    /// 快照清理中（删除不可撤销，按钮同样要置灰防连点）
    pub(crate) history_cleaning: bool,
    /// 历史 Tab 的对比基准（选中的那一版的 `version_id`）。
    ///
    /// 它与载荷里的 `diff` 说的是同一件事：出数时由载荷反推（[`Self::set_history`]），
    /// 只为了「点了以后、结果还没到」这一小段能亮着并显示取数提示。
    pub(crate) compare_target: Option<String>,
    /// 快照保存 / 读取的失败提示。
    ///
    /// 存成**行内提示**而不是错误态：保存失败时最要紧的是「已有的历史还在」，
    /// 整页转错误态反而会让人以为快照丢了。
    pub(crate) history_notice: Option<String>,
    /// 结构四区的展开态（顺序同 `SchemaSection::ALL`；默认全展开）
    pub(crate) open_schema_sections: [bool; SchemaSection::ALL.len()],
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
            history_saving: false,
            history_cleaning: false,
            compare_target: None,
            history_notice: None,
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
        // 对比基准只对旧目标（旧列）成立，跟着载荷一起清
        self.compare_target = None;
        self.state = InsightPanelState::Loading;
        if self.emit_request_for_tab(self.tab, cx) {
            cx.notify();
        }
    }

    /// 取样来源解析出样本表（接缝侧取样后回填）：源目标的保存 / 多列 / 下钻都读它。
    pub fn set_source_sample(&mut self, temp_table: impl Into<String>, cx: &mut Context<Self>) {
        self.data.source_sample = Some(temp_table.into());
        cx.notify();
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

    /// 结构 Tab 的导出请求（Phase 4.3：导出 JSON / Markdown）。
    ///
    /// 内容在这里算好（与界面同源的视图模型，D36）；没有报告时不发事件——
    /// 导出按钮只在有报告时渲染，这里是兼底。
    pub fn request_schema_export(&mut self, format: SchemaExportFormat, cx: &mut Context<Self>) {
        let Some(report) = self.data.schema.as_ref() else {
            return;
        };
        cx.emit(InsightEvent::SchemaExportRequested {
            format,
            file_stem: crate::schema_view::schema_export_file_stem(&report.schema_name),
            content: match format {
                SchemaExportFormat::Json => report.to_json(),
                SchemaExportFormat::Markdown => report.to_markdown(),
            },
        });
    }

    /// 出数（快照历史）。
    ///
    /// 保存中标记在出数时落回：它挂在「保存动作」上，不挂在面板状态上
    /// （面板状态只有四态，多一个 `Saving` 会让渲染分派多出一条不可能的分支）。
    ///
    /// 对比基准**从载荷反推**（`history.diff`）：列表刷新后「最新」就变了，
    /// 留着一个指向旧「当前」的选中位比不选中更坏（D43）。
    pub fn set_history(&mut self, history: HistoryView, cx: &mut Context<Self>) {
        self.compare_target = history
            .diff
            .as_ref()
            .map(|diff| diff.baseline_version.clone());
        self.data = std::mem::take(&mut self.data).with_history(history);
        self.history_saving = false;
        self.history_cleaning = false;
        self.history_notice = None;
        self.state = InsightPanelState::Data;
        cx.notify();
    }

    /// 历史 Tab 的版本行被点：选/取消对比基准（方向固定为「选中 → 最新」）。
    ///
    /// 只改选择位并发事件，差值由接缝读两版正文后回填（渲染路径零 I/O）。
    pub fn toggle_compare_version(&mut self, version_id: &str, cx: &mut Context<Self>) {
        if self.compare_target.as_deref() == Some(version_id) {
            self.dismiss_diff(cx);
            return;
        }
        // 最新一版没有更新的版本可比（列表行也不给它点击入口，这里是状态层的兜底）
        if self.is_latest_version(version_id) {
            return;
        }
        let Some(InsightTarget::Column { column, .. }) = self.target.clone() else {
            return;
        };
        self.compare_target = Some(version_id.to_string());
        // 上一个基准的差值已经不成立了：先放掉，渲染依此显示「读取版本正文…」
        self.clear_diff();
        cx.emit(InsightEvent::VersionCompareRequested {
            column,
            version_id: version_id.to_string(),
        });
        cx.notify();
    }

    /// 关掉对比面板（点 ✕ 或再点一次选中的那一行）
    pub fn dismiss_diff(&mut self, cx: &mut Context<Self>) {
        self.compare_target = None;
        self.clear_diff();
        cx.notify();
    }

    /// 对比取数失败：挂行内提示，并**把选中位一并放掉**。
    ///
    /// 不放掉会留下「选中亮着、面板却没出来」的死状态——选中的意义就是「面板该在」。
    pub fn set_compare_notice(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.compare_target = None;
        self.clear_diff();
        self.set_history_notice(message, cx);
    }

    /// 对比基准（选中的那一版的 `version_id`；`None` = 没在对比）
    pub fn compare_target(&self) -> Option<&str> {
        self.compare_target.as_deref()
    }

    /// 这一版是不是列表里的「当前」（对比固定为「选中 → 最新」，拿最新当基准无从比）
    fn is_latest_version(&self, version_id: &str) -> bool {
        self.data
            .history
            .as_ref()
            .and_then(|history| history.entries.first())
            .is_some_and(|entry| entry.version_id == version_id)
    }

    /// 放下对比结果（选择位由调用方处理）
    fn clear_diff(&mut self) {
        if let Some(history) = self.data.history.take() {
            self.data.history = Some(history.without_diff());
        }
    }

    /// 历史 Tab 的「清理」：先弹确认框（删快照不可撤销），确认后才发请求。
    ///
    /// 天数取自 `SNAPSHOT_RETENTION_DAYS`（与接缝实际切档同一个常量）：
    /// 对话框里写的数字必须就是真会执行的那个。
    pub fn begin_cleanup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let days = SNAPSHOT_RETENTION_DAYS;
        let current = self
            .data
            .as_history()
            .and_then(|history| history.stats_line())
            .unwrap_or_default();
        let entity = cx.entity();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let mut body = div()
                .v_flex()
                .w_full()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .text_color(theme.colors.foreground)
                        .child(format!("删除 {days} 天前的快照。")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("正文与版本链一并删除，不可撤销。"),
                );
            // 空的时候不摆一行「当前：」——那是没话找话
            if !current.is_empty() {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("当前：{current}")),
                );
            }
            dialog
                .title("清理旧快照")
                .child(body)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("清理")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok({
                    let entity = entity.clone();
                    move |_, _window, app| {
                        entity.update(app, |view, cx| view.request_cleanup(cx));
                        true
                    }
                })
                .on_cancel(|_, _, _| true)
        });
    }

    /// 确认后的发请求（清完的列表与回执由接缝回填）
    pub fn request_cleanup(&mut self, cx: &mut Context<Self>) {
        let Some(InsightTarget::Column { column, .. }) = self.target.clone() else {
            return;
        };
        self.history_cleaning = true;
        self.history_notice = None;
        cx.emit(InsightEvent::SnapshotCleanupRequested { column });
        cx.notify();
    }

    /// 快照是否正在清理（按钮置灰的依据；也供测试断言）
    pub fn history_cleaning(&self) -> bool {
        self.history_cleaning
    }

    /// 历史 Tab 的「保存」：发请求（重取领域画像 + 双写都归接缝）
    pub fn request_snapshot_save(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target.clone() else {
            return;
        };
        // 列目标与源列目标都能存：看到的是同一列的画像，就不该因为入口不同而不能留档
        let column = match &target {
            InsightTarget::Column { column, .. } | InsightTarget::SourceColumn { column, .. } => {
                column.clone()
            }
            _ => return,
        };
        let Some(temp_table) = self.data.sample_table(&target).map(str::to_string) else {
            return;
        };
        let source_label = target.source().map(|source| source.label().to_string());
        self.history_saving = true;
        self.history_notice = None;
        cx.emit(InsightEvent::SnapshotSaveRequested {
            temp_table,
            column,
            source_label,
        });
        cx.notify();
    }

    /// 保存 / 读取历史 / 清理 失败：只挂一条行内提示（列表与目标头照旧可见）
    pub fn set_history_notice(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.history_saving = false;
        self.history_cleaning = false;
        self.history_notice = Some(message.into());
        cx.notify();
    }

    /// 快照是否正在保存（按钮置灰的依据；也供测试断言）
    pub fn history_saving(&self) -> bool {
        self.history_saving
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
        let Some(temp_table) = self
            .target
            .as_ref()
            .and_then(|target| self.data.sample_table(target))
            .map(str::to_string)
        else {
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
    pub(crate) fn multi_ready(&self) -> bool {
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
        let Some(target) = self.target.clone() else {
            return;
        };
        // 源表也能评：用的是取样出来的那张临时表
        if !matches!(
            target,
            InsightTarget::Table { .. } | InsightTarget::SourceTable { .. }
        ) {
            return;
        }
        let Some(temp_table) = self.data.sample_table(&target).map(str::to_string) else {
            return;
        };
        let table_name = target.table_name();
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
        self.compare_target = None;
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
        if self.emit_request_for_tab(self.tab, cx) {
            cx.notify();
        }
    }

    /// 按 Tab 发对应的取数请求（**不管载荷在不在**：要不要发由调用方判）。
    ///
    /// 发了请求就进加载态；**发不出去也要落一个状态**：调用方是为了「点了有反应」
    /// 才先摆骨架的，这里若只是沉默返回，骨架就会一直转下去（最典型的是无项目时
    /// 切到「历史」：那是「没得看」，不是「在加载」）。
    ///
    /// 返回值 = 状态是否变过（调用方据此决定要不要重绘）。
    fn emit_request_for_tab(&mut self, tab: PanelTab, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.target.clone() else {
            return false;
        };
        // 列目标才需要列名：表 / 多列目标没有「哪一列」这回事，取整表；
        // 快照落项目目录，所以「历史」还要项目已打开（无项目看不了，不是错误）
        let request = match tab {
            PanelTab::Column => match &target {
                InsightTarget::Column { .. } | InsightTarget::SourceColumn { .. } => {
                    Some(InsightEvent::ProfileRequested { target })
                }
                _ => None,
            },
            PanelTab::Table => match &target {
                // 已经有源目标 → 原样再发（取样在接缝里做）
                InsightTarget::SourceTable { .. } => Some(InsightEvent::ProfileRequested { target }),
                // 源列目标看整表：同一个来源，换成表目标（取样口径一致）
                InsightTarget::SourceColumn { source, .. } => {
                    Some(InsightEvent::ProfileRequested {
                        target: InsightTarget::SourceTable {
                            source: source.clone(),
                            table_name: target.table_name(),
                        },
                    })
                }
                _ => Some(InsightEvent::ProfileRequested {
                    target: InsightTarget::Table {
                        temp_table: target.temp_table().to_string(),
                        table_name: target.table_name(),
                    },
                }),
            },
            PanelTab::MultiColumn => match self.data.sample_table(&target) {
                Some(temp_table) => Some(InsightEvent::MultiColumnRequested {
                    temp_table: temp_table.to_string(),
                    table_name: target.table_name(),
                }),
                // 源目标还没解析出样本表（还没看过列 / 表）→ 没有可分析的临时表
                None => None,
            },
            PanelTab::Schema => match &target {
                InsightTarget::Schema {
                    conn_id,
                    database,
                    schema,
                } => Some(InsightEvent::SchemaReportRequested {
                    conn_id: conn_id.clone(),
                    database: database.clone(),
                    schema: schema.clone().unwrap_or_default(),
                }),
                _ => None,
            },
            PanelTab::History => match &target {
                // 历史是「某列的历次快照」：只有列目标（含源列）说得清“看谁的历史”
                InsightTarget::Column { column, .. } | InsightTarget::SourceColumn { column, .. }
                    if self.project_open =>
                {
                    Some(InsightEvent::HistoryRequested {
                        column: column.clone(),
                    })
                }
                _ => None,
            },
        };
        match request {
            Some(event) => {
                self.state = InsightPanelState::Loading;
                cx.emit(event);
            }
            // 这个 Tab 在当前目标 / 项目状态下取不了数：落空态（引导文案由
            // `empty_hint()` 按 Tab 给），别留着骨架一直转
            None => self.state = InsightPanelState::Empty,
        }
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
            PanelTab::History => self.data.history.is_none(),
        };
        if !missing {
            return;
        }
        if self.emit_request_for_tab(tab, cx) {
            cx.notify();
        }
    }

    pub(crate) fn set_tab_from_index(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(tab) = PanelTab::from_index(ix) {
            self.set_tab(tab, cx);
        }
    }

    /// 同步四区展开态（`Accordion` 回调只给「当前展开的下标集合」）
    pub(crate) fn set_open_sections(&mut self, open: &[usize], cx: &mut Context<Self>) {
        for (i, slot) in self.open_sections.iter_mut().enumerate() {
            *slot = open.contains(&i);
        }
        cx.notify();
    }

    /// ⟳ 的 Action 入口（与面板头按钮同一条路径）
    fn on_refresh(&mut self, _: &InsightRefresh, _window: &mut Window, cx: &mut Context<Self>) {
        self.reload(cx);
    }

    /// 同步结构四区的展开态（与列画像同口径：Accordion 只给「当前展开的下标集合」）
    pub(crate) fn set_open_schema_sections(&mut self, open: &[usize], cx: &mut Context<Self>) {
        for (i, slot) in self.open_schema_sections.iter_mut().enumerate() {
            *slot = open.contains(&i);
        }
        cx.notify();
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
