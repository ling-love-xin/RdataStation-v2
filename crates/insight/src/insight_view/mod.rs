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
//! 本文件只放**状态**：目标 / 四态 / 各 Tab 载荷 / 折叠与选择，以及把它们推进去的那些方法。
//! 渲染按 Tab 拆在 `crate::view` 下（位移规格见
//! `docs/architecture/insight/insight-dev-plan.md` §11）——视图侧读得到字段，所以字段是
//! `pub(crate)`；对外 API 仍是那些 `pub` 方法。
//!
//! 视图归属：本文件按 **方案 A**（视图入 `crates/insight`）落地，宿主桥由 workbench
//! 侧订阅 [`InsightEvent`] 装配。

use gpui_kit::base::StyledExt as _;
pub use gpui_kit::component::button::ButtonVariant;
pub use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
pub use gpui_kit::*;

pub use std::sync::Arc;
pub use std::sync::atomic::AtomicBool;

pub use crate::commands::InsightRefresh;
pub use crate::model::{
    ColumnProfileView, HistoryView, InsightPanelState, InsightTarget, MultiColumnView,
    MultiResultView, PanelData, PanelTab, QualityNote, SNAPSHOT_RETENTION_DAYS, TableProfileView,
};
pub use crate::rule_view::RulesView;
pub use crate::schema_view::{SchemaExportFormat, SchemaSection};
pub use crate::ui;

pub use crate::model::InsightSetEntry;
pub use crate::model::OverlapView;

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
    ///
    /// `config` = 探查配置（确认窗口产物，C3）：`Some` 用确认的配置取数建表，
    /// `None` 走默认取样（前 500 行全列，`ProbeConfig::sample_default`）。
    ProfileRequested {
        target: InsightTarget,
        config: Option<crate::model::ProbeConfig>,
    },
    /// 请宿主轻量内省源查询的列清单（探查确认窗口「列」块的候选；`LIMIT 0` /
    /// `DESCRIBE`，不取数据）。失败时列候选为空，确认窗口默认「全部列」。
    ProbeColumnsRequested { source: crate::model::SampleSource },
    /// 请宿主以**全量**口径重新加载该源目标（大表手动全量：取样切换的
    /// 另一半——源目标取样后用户点「全量统计」触发；结果仍落成 `tmp_i_` 临时表，
    /// 后续分析链路与取样完全一致）。
    FullProfileRequested { target: InsightTarget },
    /// 请宿主**确认**后执行全量画像（大表全量是慢操作且占内存）。
    ///
    /// 面板在 `request_full` 时发这个而不是直接发 `FullProfileRequested`：确认
    /// 弹窗要窗口（宿主侧），且宿主能读设置——「全量统计前确认」关闭时宿主
    /// **不弹窗**、直接调 [`InsightView::confirm_full`]。确认期间面板 scope 保持不变；
    /// 取消 = 宿主什么都不做（scope / 状态都不动）。
    ///
    /// 2026-10-31 Q2 定稿：**全量 = 评估确认窗口生成的表**（不再重取源全量建表），
    /// `confirm_full` 由「重建全量表」改为「切全量口径 + 触发评估」；本事件保留为
    /// 宿主确认链的前段（弹窗文案说明评估对象），`FullProfileRequested` 分支保留兼容。
    FullProfileConfirmRequested { target: InsightTarget },
    /// 请宿主对**当前评估全表**做取样档查询级过滤现算（Q2 / C7）。
    ///
    /// 取样档不再重建取样表（旧行为会重取源）：对确认窗口生成的临时表做
    /// 行 LIMIT + 列投影的轻量查询，逐列现算迷你统计回填（列卡折叠态迷你图）。
    /// 只带 `temp_table`：列清单由接缝在后台现读（与评估同口径，不持过期列名）。
    SampleInsightRequested { temp_table: String },
    /// 请宿主评估整表质量（逐列串行 + 进度回填）。
    ///
    /// 只带 `temp_table`：列清单由接缝在后台现读（元数据可能已变，
    /// 而带着一份可能过期的列名去跑统计，失败会指向不存在的那一列）。
    TableEvaluateRequested {
        temp_table: String,
        table_name: String,
    },
    /// 请宿主补算**单列**明细（C5：中断列灰态卡的「补算」入口）。
    ///
    /// 评估全表被取消后，未完成列显示灰态 + 补算按钮；点补算对单列续跑
    /// （同一 `get_column_insight_full` 链路），回填后清除中断标记。
    ColumnEvaluateRequested { temp_table: String, column: String },
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
    /// 请宿主把某张已洞察表加入中间页对比候选并切到对比形态。
    ///
    /// 右栏集合行的「对比」按钮发出；候选勾选是中间页的状态（`compare_selected`），
    /// 这里只报稳定业务键，由宿主转发（不跨实体直接改中间页状态）。
    CompareRequested { key: String },
    /// 请宿主对某张已洞察表与**当前目标表**做列值重叠排查（T19 关系引擎）。
    ///
    /// 只报发起方业务键：被对比方 = 集合里 `is_current` 的那张（右栏集合卡
    /// 「关系」按钮语义是「与当前目标对比」），宿主从集合里解析表名。
    RelationRequested { key: String },
    /// 请宿主把某张已洞察表归档到资产库（T20：画像快照 JSON → 分析存档）。
    ///
    /// 归档在宿主侧（要 `ArchiveService` 与项目目录），这里只报业务键；
    /// 宿主取画像快照、落盘、入资产库索引并回执。
    ArchiveRequested { key: String },
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
    /// 请宿主把当前表画像的 EDA 报告存成 Markdown 文件（对齐原型「生成报告」）。
    ///
    /// 与 `SchemaExportRequested` 同一口径（D36）：`content` 由面板算好，
    /// 宿主只选路径与写文件（`rfd` 保存对话框 → `std::fs::write` → 状态栏回执）。
    /// 用户取消保存对话框不算失败。
    TableReportExportRequested {
        target: InsightTarget,
        /// 默认文件名主名（不含扩展名，已净化）
        file_stem: String,
        content: String,
    },
    /// 请宿主打开「新建洞察」对话框（面板头 ＋ 按钮发出）。
    ///
    /// 来源选择（已连接库 / 草稿箱 / 资源分析 / 外部文件）与确认都在宿主侧
    /// （要 Shared 的连接列表、scratchpad store、系统文件对话框），面板只发意图。
    NewInsightRequested,
    /// 请宿主把集合行所指目标**切为当前洞察对象**（2026-10-30 多标签 +
    /// 原型 it-row 整行点击语义）。
    ///
    /// 与 `CompareRequested` 的区别：对比进对比形态、不改变右栏目标；
    /// 切换 = 双栏联动（右栏摘要 + 中间页都指向该目标）。宿主经
    /// `request_insight_switch` 从集合条目还原 `InsightTarget` 后打开。
    ActivateTableRequested { key: String },
    /// 请宿主打开某张已洞察表的**版本 / 归档时间线**（原型 it-row 四钮「版本」）。
    ///
    /// 语义：表级快照历史由资产库归档承接（`InsightTableReportStore` 落库后为
    /// 表级版本线），宿主打开右栏归档面板定位该表即可；面板不自己持版本数据。
    TableHistoryRequested { key: String },
    /// 请宿主把一段 SQL 拿到中央 SQL 编辑器中查看（原型 col-card「在 SQL
    /// 编辑器中查看」）。
    ///
    /// 只报文本与标题：打开 / 绑定连接 / 聚焦都由宿主 `open_query_document`
    /// 完成（`run = false`：看语句，不执行）。
    OpenSqlRequested { sql: String, title: String },
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

/// 探查确认窗口的数据量模式（确认窗口「数据量」块，C1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeAmountMode {
    /// 全量（不带 LIMIT）
    Full,
    /// 前 N 行
    RowCount,
    /// 百分比
    Percent,
}

/// 探查确认窗口的编辑状态（源目标进入洞察时打开，C3）。
///
/// 编辑值用**字符串**承载（行数 / 百分比 / 过滤 / 种子都是用户文本输入），
/// 确认时才解析成 [`crate::model::ProbeConfig`]；解析失败给行内提示、不取数。
/// 列候选来自源查询的轻量内省（`jobs::request_probe_columns` 后台回填）；
/// 还没拿到时默认「全部列」（不阻塞确认）。
#[derive(Debug, Clone)]
pub(crate) struct ProbeDialogState {
    /// 待确认的源目标（确认后才设为面板当前目标并发取数）
    pub(crate) target: InsightTarget,
    /// 数据量模式
    pub(crate) amount_mode: ProbeAmountMode,
    /// 行数编辑值（`amount_mode == RowCount` 时有效）
    pub(crate) row_count: String,
    /// 百分比编辑值（`amount_mode == Percent` 时有效）
    pub(crate) percent: String,
    /// 采样方式（R2：用户选择，UI 说明差异）
    pub(crate) sample_mode: crate::model::ProbeSampleMode,
    /// 列集合（未选列在 EDA 上不存在，C4）
    pub(crate) columns: crate::model::ProbeColumns,
    /// 候选列清单（轻量内省结果；空 = 还没拿到 / 拿失败）
    pub(crate) column_candidates: Vec<String>,
    /// 过滤条件文本（R1）
    pub(crate) filter: String,
    /// reservoir 采样种子文本（Percent + Reservoir 时生效）
    pub(crate) seed: String,
    /// 解析失败提示（确认时写，行内展示）
    pub(crate) parse_notice: Option<String>,
}

impl ProbeDialogState {
    /// 新建确认窗口编辑态：预填上次探查配置（会话级记忆，R8 / C6）。
    pub(crate) fn new(target: InsightTarget, last: Option<&crate::model::ProbeConfig>) -> Self {
        let (amount_mode, row_count, percent) = match last.map(|c| &c.amount) {
            Some(crate::model::ProbeAmount::RowCount(n)) => {
                (ProbeAmountMode::RowCount, n.to_string(), String::new())
            }
            Some(crate::model::ProbeAmount::Percent(p)) => {
                (ProbeAmountMode::Percent, String::new(), p.to_string())
            }
            Some(crate::model::ProbeAmount::Full) | None => {
                (ProbeAmountMode::RowCount, "500".into(), String::new())
            }
        };
        Self {
            target,
            amount_mode,
            row_count,
            percent,
            sample_mode: last
                .map(|c| c.sample_mode)
                .unwrap_or(crate::model::ProbeSampleMode::Head),
            columns: last
                .map(|c| c.columns.clone())
                .unwrap_or(crate::model::ProbeColumns::All),
            column_candidates: Vec::new(),
            filter: last.and_then(|c| c.filter.clone()).unwrap_or_default(),
            seed: last
                .and_then(|c| c.seed.map(|s| s.to_string()))
                .unwrap_or_default(),
            parse_notice: None,
        }
    }

    /// 当前编辑态 → 探查配置；解析失败返回 `None` 并写行内提示。
    pub(crate) fn to_config(&mut self) -> Option<crate::model::ProbeConfig> {
        let amount = match self.amount_mode {
            ProbeAmountMode::Full => crate::model::ProbeAmount::Full,
            ProbeAmountMode::RowCount => {
                let n = self.row_count.trim().parse::<u32>().ok();
                match n.filter(|n| *n > 0) {
                    Some(n) => crate::model::ProbeAmount::RowCount(n),
                    None => {
                        self.parse_notice = Some("行数需为正整数（如 500）".into());
                        return None;
                    }
                }
            }
            ProbeAmountMode::Percent => {
                let p = self.percent.trim().parse::<f64>().ok();
                match p.filter(|p| (0.0..=100.0).contains(p)) {
                    Some(p) => crate::model::ProbeAmount::Percent(p),
                    None => {
                        self.parse_notice = Some("百分比需在 0–100 之间（如 5）".into());
                        return None;
                    }
                }
            }
        };
        let filter = {
            let f = self.filter.trim();
            if f.is_empty() {
                None
            } else {
                Some(f.to_string())
            }
        };
        let seed = {
            let s = self.seed.trim();
            if s.is_empty() {
                None
            } else {
                match s.parse::<u64>() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        self.parse_notice = Some("种子需为非负整数（可留空）".into());
                        return None;
                    }
                }
            }
        };
        self.parse_notice = None;
        Some(crate::model::ProbeConfig {
            amount,
            sample_mode: self.sample_mode,
            columns: self.columns.clone(),
            filter,
            seed,
        })
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
    /// 加载期的阶段说明（源目标取样：DuckDB 建分析临时表在大表上可能较久，
    /// 而 CTAS 在库内完成、拿不到真百分比——只给阶段文字 + 不确定进度条）。
    ///
    /// 由 [`crate::jobs::request_profile`] 分阶段更新：取样中 → 画像中；渲染在
    /// `state == Loading` 时读它，`None` 走纯骨架。
    pub(crate) loading_note: Option<SharedString>,
    /// 已洞察表集合（中间工作页 `opened` 的快照，宿主经 [`Self::set_insight_set`] 推送）。
    ///
    /// 集合是**会话级管理**（可对比 / 关系排查 / 归档），独立于当前目标的下钻——
    /// 当前目标高亮由条目上的 `is_current` 表达，不参与载荷。
    pub(crate) insight_set: Vec<InsightSetEntry>,
    /// 集合多选模式（原型 it-head「多选」开关：开启后行首出现勾选框）。
    ///
    /// 多选与当前目标下钻互斥：开启多选时行不再表达 `is_current` 高亮，
    /// 勾选 ≥2 张表后 it-actions 出现「质量对比」入口。
    pub(crate) set_multi: bool,
    /// 多选模式下勾选的行（按条目 `key` 记；质量对比 / 批量操作的对象）。
    pub(crate) selected_tables: std::collections::HashSet<String>,
    /// 跨洞察表关系排查的结果（T19；`None` = 尚未排查过）
    pub(crate) overlap: Option<OverlapView>,
    /// 关系排查执行中（集合卡关系区给进度提示）
    pub(crate) overlap_running: bool,
    /// 关系排查的失败提示（结果区给说明，不整页转错误态）
    pub(crate) overlap_notice: Option<String>,
    /// 当前画像的取样口径（中间页 scope 切换；右栏摘要跟随）。
    ///
    /// 默认取样；源目标手动切全量后为 `Full`（临时表目标没有取样概念，恒 `Sample`）。
    pub(crate) scope: InsightScope,
    /// 探查确认窗口（源目标进入洞察时打开，C3）：确认前不取数、取消幂等。
    pub(crate) probe_dialog: Option<ProbeDialogState>,
    /// 上次探查配置（会话级记忆，R8 / C6）：重取数 / 重新探查预填。
    pub(crate) last_probe_config: Option<crate::model::ProbeConfig>,
    /// 评估全表的取消令牌（C5）：jobs 逐列循环每列派发前检查；置位后停止派发、
    /// 已完成列保留、未完成列灰态可补算。新评估（`request_table_evaluation`）
    /// 重置为 false。
    pub(crate) eval_cancel: Arc<AtomicBool>,
    /// 宿主推送的洞察偏好（对应设置页 `insight.*`；insight 不依赖 settings，
    /// 由宿主在装配与设置变更时转发）。`status_usage` 关闭时 `memory` 恒 `None`。
    pub(crate) status_usage: bool,
    /// 洞察集合工作集提示阈值（表数超过该值集合卡提示先清理；0 = 不提示）。
    pub(crate) work_set_limit: usize,
    /// 分析引擎内存读数（后台轮询回填；工具栏显示「内存 X MB / 上限 · 表 N」）。
    pub(crate) memory: Option<engine::duckdb::MemoryStats>,
    /// 内存轮询任务是否在跑（防重复启动；偏好关闭后轮询自行退出）。
    pub(crate) memory_poll_running: bool,
}

/// 画像取样口径（2026-10-31 Q2 定稿语义）。
///
/// - 默认 `Sample`（取样档）：对**评估全表**做查询级过滤现算（行 LIMIT + 列投影，
///   不建子表），迷你统计随取样回填（C7）。
/// - `Full`（全量档）：评估**确认窗口生成的表**（确认窗口的行数/百分比已落到
///   建表层；进入 EDA 自动评估，见 `set_table_profile`）。
/// 临时表目标没有取样 / 全量概念，恒 `Sample`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InsightScope {
    /// 取样（默认）：评估全表的查询级过滤
    #[default]
    Sample,
    /// 全量：评估确认窗口生成的表
    Full,
}

impl InsightScope {
    pub fn label(self) -> &'static str {
        match self {
            InsightScope::Sample => "取样",
            InsightScope::Full => "全量",
        }
    }
}

impl EventEmitter<InsightEvent> for InsightView {}


pub mod state;
pub mod render;
