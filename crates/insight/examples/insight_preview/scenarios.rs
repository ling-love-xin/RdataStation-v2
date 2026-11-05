//! 预览场景：把洞察面板的各个形态做成可一键切换的**确定性数据**。
//!
//! ## 为什么是「构造假数据」而不是「连真库取样」
//!
//! 洞察的价值在**渲染**（布局 / 截断 / 换行 / 滚动），不在取数。真库取样还要先
//! 建连接、等建表，出问题时分不清是数据问题还是布局问题。这里用纯内存构造的
//! `*_View` 结构直接喂给面板：
//!
//! - **可复现**：同一个场景每次点出来的像素一模一样；
//! - **可极端**：能造出「30+ 字符表名」这种真库里少见的边界值；
//! - **零外部依赖**：不需要 DuckDB / 连接 / 项目库。
//!
//! ## 注入路径（全部是 pub API，没有碰 crate 内部字段）
//!
//! 面板遵循 D20「自己不取数」：`InsightView::set_*` 是宿主推送载荷的口子。
//! 预览工具扮演宿主，于是这套 `set_*` 就是天然的数据注入点。

// example 是独立 crate，只认 `gpui-kit` 这层门面：`gpui` 由它再导出，
// 不在这里单独声明依赖（依赖唯一入口在项目根 Cargo.toml 的 workspace 表）。
use gpui_kit::gpui::{App, Entity};

use rds_insight::model::{
    ColumnCheckState, ColumnChecker, ColumnMiniStats, HistoryEntryView, HistoryView,
    StorageStatsView, TableSample,
};
use rds_insight::{
    BooleanStats, ColumnKind, ColumnProfileView, ColumnStatsDetail, DateTimeStats, DistributionBar,
    DistributionBin, Emphasis, ExtremeValue, InsightTarget, InsightView, InsightWorkspacePanel,
    NoteLevel, NumericStats, PanelTab, QualityNote, SampleCell, StatRow, TableColumnView,
    TableEvalProgress, TableProfileView, TableQualityView, TextFrequency, TextStats, WorkspaceMode,
};

/// 预览用的数据源标签（`TableProfileView::source_label`：产品里恒为「DuckDB 临时表」）。
const SOURCE_LABEL: &str = "DuckDB 临时表";

// ============================================================================
// 场景清单
// ============================================================================

/// 一个可预览的形态。
///
/// 每个场景 = 「一组确定的载荷 + 一个确定的 Tab / 形态」，切换时先整体复位再重灌，
/// 所以**任意顺序切换都不会串状态**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    /// 无目标空态
    Empty,
    /// 数值列画像（四区：基础统计 / 分布 / 质量 / 样本）
    ColumnNumeric,
    /// 超长列名的列画像
    ColumnLongName,
    /// 表探查 + 已评估（检查器 / 列卡网格 / EDA 五卡）
    TableProfile,
    /// 超长表名的表探查
    TableLongName,
    /// 高缺失脏表（缺失榜 / 值得关注 / Unknown 类型列降级）
    TableMessy,
    /// 多表对比结果表
    CompareResult,
    /// 快照历史列表
    HistoryList,
    /// 六张目标同时进洞察：顶部标签条 + 右栏集合列表
    MultiTab,
    /// 取数失败的错误态（可重试）
    ErrorRetryable,
    /// 计算中的骨架态
    Loading,
}

impl Scenario {
    pub const ALL: [Scenario; 11] = [
        Scenario::Empty,
        Scenario::ColumnNumeric,
        Scenario::ColumnLongName,
        Scenario::TableProfile,
        Scenario::TableLongName,
        Scenario::TableMessy,
        Scenario::CompareResult,
        Scenario::HistoryList,
        Scenario::MultiTab,
        Scenario::ErrorRetryable,
        Scenario::Loading,
    ];

    /// 按钮标签（短名：一排十个按钮自己也要能在窄窗下排开）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "空态",
            Self::ColumnNumeric => "数值列",
            Self::ColumnLongName => "长列名",
            Self::TableProfile => "表画像",
            Self::TableLongName => "长表名",
            Self::TableMessy => "脏表",
            Self::CompareResult => "对比表",
            Self::HistoryList => "快照史",
            Self::MultiTab => "多标签",
            Self::ErrorRetryable => "错误态",
            Self::Loading => "加载中",
        }
    }

    /// 场景说明（顶栏显示：告诉验收者此刻该看什么）。
    pub fn hint(self) -> &'static str {
        match self {
            Self::Empty => "无目标空态：应显示占位提示，不应有残留数据",
            Self::ColumnNumeric => "数值列四区：基础统计 / 分布 / 质量提示 / 样本值",
            Self::ColumnLongName => "超长列名：主键名与 21px 数值都应省略，不越出卡片边框",
            Self::TableProfile => "表画像：右栏概览五卡 + 检查器命中环 + 中央 EDA 列卡网格",
            Self::TableLongName => "超长表名：工具栏 chip 与下拉 label 必须截断且保留尾部",
            Self::TableMessy => "高缺失脏表：缺失榜 / 值得关注 / Unknown 类型列的降级呈现",
            Self::CompareResult => "多表对比：列多时应出横向滚动条，不得整列不可达",
            Self::HistoryList => "快照历史：版本链 / 当前标记 / 存储用量",
            Self::MultiTab => "六目标并存：标签条应省略长名；集合列表应有高度上限并自身滚动",
            Self::ErrorRetryable => "错误态：整块提示 + 重试按钮，不可用沉默空态代替",
            Self::Loading => "骨架态：先看布局是否稳定，不应出现高度塌陷",
        }
    }
}

impl Scenario {
    /// 把该场景的数据灌进面板（调用方先做 [`Self::reset`]）。
    pub fn apply(self, panel: &Entity<InsightWorkspacePanel>, cx: &mut App) {
        match self {
            Self::Empty => {}
            Self::ColumnNumeric => {
                let (target, profile) = numeric_column();
                load_column(panel, target, profile, cx);
            }
            Self::ColumnLongName => {
                let (target, profile) = long_column();
                load_column(panel, target, profile, cx);
            }
            Self::TableProfile => {
                let (target, profile) = orders_table();
                load_table(panel, target, profile, PanelTab::Table, cx);
            }
            Self::TableLongName => {
                let (target, profile) = long_named_table();
                load_table(panel, target, profile, PanelTab::Table, cx);
            }
            Self::TableMessy => {
                let (target, profile) = messy_table();
                load_table(panel, target, profile, PanelTab::Table, cx);
            }
            Self::CompareResult => load_compare(panel, cx),
            Self::HistoryList => load_history(panel, cx),
            Self::MultiTab => load_multi_tab(panel, cx),
            Self::ErrorRetryable => {
                let (target, _) = orders_table();
                panel.update(cx, |panel, cx| {
                    // 先登记（进 Loading）再转错误态：tab 条与内容态一起验收。
                    panel.open_target(target, cx);
                    view_of(panel).update(cx, |v, cx| {
                        v.set_error(
                            "无法读取分析临时表 tmp_i_orders_a1b2：该临时表已失效，请重新取数",
                            true,
                            cx,
                        );
                    });
                });
            }
            Self::Loading => {
                let (target, _) = orders_table();
                panel.update(cx, |panel, cx| {
                    // 只登记不给载荷：`open_target` 进 Loading 并发出请求，预览没有
                    // 宿主接路由，于是停在骨架帧——顺带能验收「加载中」的 tab 圆点。
                    panel.open_target(target, cx);
                });
            }
        }
    }

    /// 复位：回到无目标空态（清 tab 列表 / 载荷 / 对比形态）。
    ///
    /// `clear_target` 会一并重置 `data` / `scope` / `state`，但**不管 `opened` 与
    /// 对比形态**——后者属于工作页私有状态，故用「逐个关 tab」走它的正常收尾逻辑
    /// （关到最后一个时它会自己 `clear_target`，双重保险）。
    pub fn reset(panel: &Entity<InsightWorkspacePanel>, cx: &mut App) {
        panel.update(cx, |panel, cx| {
            panel.set_mode(WorkspaceMode::Insight, cx);
            while !panel.opened().is_empty() {
                panel.close_opened(0, cx);
            }
            view_of(panel).update(cx, |v, cx| v.clear_target(cx));
        });
    }
}

// ============================================================================
// 注入流程
// ============================================================================

/// 借 [`InsightWorkspacePanel::insight_view`] 拿到内层 `InsightView` 实体。
///
/// 工作页内部就是「右栏 InsightView + 中央 EDA」的同构复用（D63），注入载荷一律
/// 打到这同一个实体上。
fn view_of(panel: &InsightWorkspacePanel) -> Entity<InsightView> {
    panel.insight_view().clone()
}

/// 列画像场景：目标是列，不需要登记进 tab 列表（`opened` 只收表级目标）。
fn load_column(
    panel: &Entity<InsightWorkspacePanel>,
    target: InsightTarget,
    profile: ColumnProfileView,
    cx: &mut App,
) {
    panel.update(cx, |panel, cx| {
        let view = view_of(panel);
        view.update(cx, |v, cx| {
            v.set_target_loaded(target, cx);
            v.set_profile(profile, cx);
        });
        view.update(cx, |v, cx| v.set_tab(PanelTab::Column, cx));
    });
}

/// 打开并灌注一张表：**先 `open_target` 登记、再 `set_table_profile` 灌数据**。
///
/// 顺序不能反。`open_target` 走 `apply_target_and_request`，它会无条件
/// `data = PanelData::default()` 清掉载荷并停在 `Loading` 等宿主回数；若先灌数据再
/// `open_target`，刚灌进去的画像会被整个抹掉——预览没有宿主接 `ProfileRequested`，
/// 于是永远停在骨架屏。先开后灌才是「登记（清、进 Loading）→ 灌数据（落 Data）」。
fn open_and_load_table(
    panel: &Entity<InsightWorkspacePanel>,
    target: InsightTarget,
    profile: TableProfileView,
    cx: &mut App,
) {
    panel.update(cx, |panel, cx| {
        panel.open_target(target, cx);
        view_of(panel).update(cx, |v, cx| v.set_table_profile(profile, cx));
    });
}

/// 单表画像场景：登记 + 灌数据后切到指定 Tab。
fn load_table(
    panel: &Entity<InsightWorkspacePanel>,
    target: InsightTarget,
    profile: TableProfileView,
    tab: PanelTab,
    cx: &mut App,
) {
    open_and_load_table(panel, target, profile, cx);
    panel.update(cx, |panel, cx| {
        view_of(panel).update(cx, |v, cx| v.set_tab(tab, cx));
    });
}

/// 对比场景：三张表都要有摘要，结果表才有真实行可读。
///
/// 摘要归档靠 `open_target` 内部的 `archive_summary`——它读「**当前**目标 + 当前
/// table 载荷」，写进当前目标自己的 `opened` 条目。所以逐个「打开并灌注」：切到下
/// 一张时，上一张（还停在数据态）的摘要就被归档了。最后一张切走前没人归档它，
/// 结尾再 `open_target` 一次已登记的第一张，触发归档的同时把当前目标也拉回第一张。
fn load_compare(panel: &Entity<InsightWorkspacePanel>, cx: &mut App) {
    let tables = vec![orders_table(), messy_table(), long_named_table()];

    for (target, profile) in &tables {
        open_and_load_table(panel, target.clone(), profile.clone(), cx);
    }

    panel.update(cx, |panel, cx| {
        // 归档最后一张（此刻它还停在数据态）+ 当前目标回到第一张。
        panel.open_target(tables[0].0.clone(), cx);
        // 对比三态是「候选 → 运行 → 结果」：直接全勾并起跑，0.6s 后自己落到结果态
        // （与产品里点「对比」的手感一致，顺带能验收运行态的进度条）。
        let keys: Vec<String> = panel.opened().iter().map(|e| e.key.clone()).collect();
        for key in keys {
            panel.toggle_candidate(&key, cx);
        }
        panel.start_compare(cx);
        panel.set_mode(WorkspaceMode::Compare, cx);
    });
}

fn load_history(panel: &Entity<InsightWorkspacePanel>, cx: &mut App) {
    let (target, _) = orders_table();
    panel.update(cx, |panel, cx| {
        // 先登记（清载荷进 Loading），再灌历史载荷并切到「历史」Tab。
        panel.open_target(target, cx);
        let view = view_of(panel);
        view.update(cx, |v, cx| {
            v.set_history(history_view(), cx);
            v.set_tab(PanelTab::History, cx);
        });
    });
}

/// 多标签场景：六张目标同时登记。
///
/// 两个验收点都只有在这个场景才够得着：
/// - **顶部标签条**：六张 tab 加上一张超长表名，才会触发 `INSIGHT_WS_TAB_MAX_WIDTH`
///   的省略号截断；
/// - **右栏集合列表**：列表堆到几十行才会显现 `INSIGHT_SET_LIST_MAX_HEIGHT` 的高度
///   上限与内部滚动——只有一两行时那处修复根本看不见。
fn load_multi_tab(panel: &Entity<InsightWorkspacePanel>, cx: &mut App) {
    let tables = vec![
        orders_table(),
        long_named_table(),
        messy_table(),
        synthetic_table("dim_customer"),
        synthetic_table("fct_user_retention_daily_snapshot"),
        synthetic_table("audit_log"),
    ];

    // 逐个「打开并灌注」：切到下一张时归档上一张的摘要（同 load_compare 的归档链）。
    for (target, profile) in &tables {
        open_and_load_table(panel, target.clone(), profile.clone(), cx);
    }

    panel.update(cx, |panel, cx| {
        // 归档最后一张 + 当前目标回到第一张并带上数据（画像形态有内容可看）。
        panel.open_target(tables[0].0.clone(), cx);
        view_of(panel).update(cx, |v, cx| v.set_table_profile(tables[0].1.clone(), cx));
        // 停在画像形态（不是对比）：要看的就是标签条与集合列表本身
        panel.set_mode(WorkspaceMode::Insight, cx);
    });
}

/// 合成一张简表（多标签场景凑数量用：结构一致、名字不同即可）。
fn synthetic_table(name: &str) -> (InsightTarget, TableProfileView) {
    let temp_table = format!("tmp_i_{}_s{:.0}", name, name.len() as f32);
    let target = InsightTarget::Table {
        temp_table: temp_table.clone(),
        table_name: name.into(),
    };
    let profile = TableProfileView {
        table_name: name.into(),
        source_label: SOURCE_LABEL,
        row_count: 4_096,
        dup_rows: Some(3),
        columns: vec![
            col(
                1,
                "id",
                "BIGINT",
                ColumnKind::Numeric,
                false,
                true,
                0.0,
                1.0,
                Some(numeric_detail()),
            ),
            col(
                2,
                "name",
                "VARCHAR",
                ColumnKind::Text,
                false,
                false,
                0.0104,
                0.8822,
                Some(text_detail()),
            ),
            col(
                3,
                "created_at",
                "TIMESTAMP",
                ColumnKind::DateTime,
                false,
                false,
                0.0,
                0.3311,
                Some(datetime_detail()),
            ),
        ],
        quality: Some(TableQualityView {
            checker: vec![
                checker("id", "非空", ColumnCheckState::Hit, "无缺失"),
                checker("name", "非空", ColumnCheckState::Hit, "无缺失"),
            ],
            checker_applicable: 2,
            checker_hits: 2,
        }),
        progress: None,
        sample: None,
    };
    (target, profile)
}

// ============================================================================
// 列画像
// ============================================================================

fn numeric_column() -> (InsightTarget, ColumnProfileView) {
    let target = InsightTarget::Column {
        temp_table: "tmp_i_orders_a1b2".into(),
        column: "amount".into(),
        data_type: "DECIMAL(18,2)".into(),
    };
    let profile = ColumnProfileView {
        column: "amount".into(),
        data_type: "DECIMAL(18,2)".into(),
        kind: ColumnKind::Numeric,
        total_count: 12_480,
        null_count: 0,
        null_rate: 0.0,
        basics: vec![
            stat_row("最小值", "12.00", Emphasis::Normal),
            stat_row("最大值", "9,860.50", Emphasis::Normal),
            stat_row("平均值", "742.36", Emphasis::Normal),
            stat_row("中位数", "528.75", Emphasis::Normal),
            stat_row("标准差", "1,043.82", Emphasis::Muted),
            stat_row("唯一值", "10,904", Emphasis::Normal),
        ],
        distribution: histogram_ratios()
            .into_iter()
            .map(|(label, ratio)| DistributionBar {
                label: label.into(),
                ratio,
                text: format!("{:.1}%", ratio * 100.0),
            })
            .collect(),
        notes: vec![
            QualityNote {
                level: NoteLevel::Info,
                text: "无缺失值".into(),
            },
            QualityNote {
                level: NoteLevel::Warning,
                text: "存在 23 个极端值（超过 3 倍标准差）".into(),
            },
        ],
        sample: (1..=5)
            .zip(["318.40", "1,205.00", "96.75", "2,840.10", "512.30"])
            .map(|(index, value)| SampleCell {
                index,
                value: Some(value.into()),
            })
            .collect(),
    };
    (target, profile)
}

/// 超长列名：验证「三行文本一致截断」（主键名与 21px 数值原先没 ellipsis，会画出边框外）。
fn long_column() -> (InsightTarget, ColumnProfileView) {
    let column = "customer_order_total_amount_including_tax_and_shipping_fees_usd";
    let target = InsightTarget::Column {
        temp_table: "tmp_i_order_line_items_detail_partitioned_by_month_v3_c9d0".into(),
        column: column.into(),
        data_type: "DECIMAL(24,6)".into(),
    };
    let profile = ColumnProfileView {
        column: column.into(),
        data_type: "DECIMAL(24,6)".into(),
        kind: ColumnKind::Numeric,
        total_count: 84_213_005,
        null_count: 1_204_887,
        null_rate: 0.0143,
        basics: vec![
            stat_row("最小值", "0.000000", Emphasis::Normal),
            stat_row("最大值", "8,241,905.750000", Emphasis::Normal),
            stat_row("平均值", "1,338.442911", Emphasis::Normal),
            stat_row("中位数", "702.105500", Emphasis::Normal),
        ],
        distribution: vec![
            DistributionBar {
                label: "0 – 1,648,381".into(),
                ratio: 0.7481,
                text: "74.8%".into(),
            },
            DistributionBar {
                label: "1,648,381 – 3,296,762".into(),
                ratio: 0.1502,
                text: "15.0%".into(),
            },
            DistributionBar {
                label: "3,296,762 – 4,945,143".into(),
                ratio: 0.0611,
                text: "6.1%".into(),
            },
            DistributionBar {
                label: "4,945,143 – 8,241,905".into(),
                ratio: 0.0406,
                text: "4.1%".into(),
            },
        ],
        notes: vec![QualityNote {
            level: NoteLevel::Warning,
            text: "列名超过 55 字符：标题区应省略显示，tooltip 给全名".into(),
        }],
        sample: vec![SampleCell {
            index: 1,
            value: Some("3,120.500000".into()),
        }],
    };
    (target, profile)
}

fn stat_row(label: &'static str, value: &str, emphasis: Emphasis) -> StatRow {
    StatRow {
        label,
        value: value.into(),
        emphasis,
    }
}

// ============================================================================
// 表画像
// ============================================================================

fn orders_table() -> (InsightTarget, TableProfileView) {
    let target = InsightTarget::Table {
        temp_table: "tmp_i_orders_a1b2".into(),
        table_name: "orders".into(),
    };
    let profile = TableProfileView {
        table_name: "orders".into(),
        source_label: SOURCE_LABEL,
        row_count: 12_480,
        dup_rows: Some(0),
        columns: vec![
            col(
                1,
                "order_id",
                "VARCHAR",
                ColumnKind::Text,
                false,
                true,
                0.0,
                1.0,
                Some(text_detail()),
            ),
            col(
                2,
                "customer_id",
                "VARCHAR",
                ColumnKind::Text,
                false,
                false,
                0.0,
                0.6218,
                Some(text_detail()),
            ),
            col(
                3,
                "amount",
                "DECIMAL(18,2)",
                ColumnKind::Numeric,
                false,
                false,
                0.0,
                0.8736,
                Some(numeric_detail()),
            ),
            col(
                4,
                "status",
                "VARCHAR",
                ColumnKind::Text,
                false,
                false,
                0.0,
                0.0231,
                Some(text_detail()),
            ),
            col(
                5,
                "order_date",
                "TIMESTAMP",
                ColumnKind::DateTime,
                false,
                false,
                0.0,
                0.4102,
                Some(datetime_detail()),
            ),
            col(
                6,
                "paid",
                "BOOLEAN",
                ColumnKind::Boolean,
                false,
                false,
                0.0,
                0.0088,
                Some(boolean_detail()),
            ),
            col(
                7,
                "coupon_code",
                "VARCHAR",
                ColumnKind::Text,
                true,
                false,
                0.7124,
                0.1402,
                Some(text_detail()),
            ),
            col(
                8,
                "payload",
                "BLOB",
                ColumnKind::Unknown,
                true,
                false,
                0.1533,
                0.0091,
                None,
            ),
        ],
        quality: Some(TableQualityView {
            checker: vec![
                checker("order_id", "非空", ColumnCheckState::Hit, "无缺失"),
                checker("order_id", "唯一率", ColumnCheckState::Hit, "100.0%"),
                checker("customer_id", "非空", ColumnCheckState::Hit, "无缺失"),
                checker(
                    "amount",
                    "取值范围",
                    ColumnCheckState::Hit,
                    "12.00 – 9,860.50",
                ),
                checker("coupon_code", "非空", ColumnCheckState::Miss, "缺失 71.2%"),
                checker(
                    "payload",
                    "类型可识别",
                    ColumnCheckState::NotApplicableType,
                    "BLOB 不支持",
                ),
            ],
            checker_applicable: 5,
            checker_hits: 4,
        }),
        progress: Some(TableEvalProgress { done: 8, total: 8 }),
        sample: Some(TableSample {
            columns: vec![
                "order_id".into(),
                "customer_id".into(),
                "amount".into(),
                "status".into(),
                "paid".into(),
            ],
            rows: vec![
                vec![
                    "10001".into(),
                    "C-0132".into(),
                    "318.40".into(),
                    "已付款".into(),
                    "true".into(),
                ],
                vec![
                    "10002".into(),
                    "C-0877".into(),
                    "1205.00".into(),
                    "待发货".into(),
                    "true".into(),
                ],
                vec![
                    "10003".into(),
                    "C-0451".into(),
                    "96.75".into(),
                    "已取消".into(),
                    "false".into(),
                ],
                vec![
                    "10004".into(),
                    "C-0903".into(),
                    "2840.10".into(),
                    "已付款".into(),
                    "true".into(),
                ],
                vec![
                    "10005".into(),
                    "C-0132".into(),
                    "512.30".into(),
                    "待发货".into(),
                    "true".into(),
                ],
            ],
        }),
    };
    (target, profile)
}

/// 超长表名：工具栏 chip 与下拉 label 的截断压力。
fn long_named_table() -> (InsightTarget, TableProfileView) {
    let name = "analytics_prod_customer_order_daily_summary_partitioned_by_month_v2";
    let target = InsightTarget::Table {
        temp_table: "tmp_i_analytics_prod_customer_order_daily_summary_partitioned_v2_e7f3".into(),
        table_name: name.into(),
    };
    let profile = TableProfileView {
        table_name: name.into(),
        source_label: SOURCE_LABEL,
        row_count: 3_902_114,
        dup_rows: Some(12),
        columns: vec![
            col(
                1,
                "summary_date",
                "DATE",
                ColumnKind::DateTime,
                false,
                true,
                0.0,
                0.1082,
                Some(datetime_detail()),
            ),
            col(
                2,
                "customer_id",
                "VARCHAR",
                ColumnKind::Text,
                false,
                true,
                0.0,
                0.7441,
                Some(text_detail()),
            ),
            col(
                3,
                "order_amount_net_of_discounts_and_refunds",
                "DECIMAL(20,4)",
                ColumnKind::Numeric,
                false,
                false,
                0.0021,
                0.9104,
                Some(numeric_detail()),
            ),
            col(
                4,
                "order_count",
                "BIGINT",
                ColumnKind::Numeric,
                false,
                false,
                0.0,
                0.0522,
                Some(numeric_detail()),
            ),
        ],
        quality: Some(TableQualityView {
            checker: vec![
                checker("summary_date", "非空", ColumnCheckState::Hit, "无缺失"),
                checker("customer_id", "非空", ColumnCheckState::Hit, "无缺失"),
                checker(
                    "order_amount_net_of_discounts_and_refunds",
                    "非空",
                    ColumnCheckState::Miss,
                    "缺失 0.2%",
                ),
            ],
            checker_applicable: 3,
            checker_hits: 2,
        }),
        progress: None,
        sample: None,
    };
    (target, profile)
}

/// 脏表：高缺失 + 大量 Unknown + 一个评估中断列。
fn messy_table() -> (InsightTarget, TableProfileView) {
    let target = InsightTarget::Table {
        temp_table: "tmp_i_staging_user_import_tmp_20261031_d4a8".into(),
        table_name: "staging_user_import_tmp".into(),
    };
    // 未完成的列：评估被取消后列卡显示灰态 + 「补算」入口（C5）
    let mut interrupted_col = col(
        9,
        "raw_json",
        "JSON",
        ColumnKind::Unknown,
        true,
        false,
        0.4122,
        0.7711,
        None,
    );
    interrupted_col.interrupted = true;

    let profile = TableProfileView {
        table_name: "staging_user_import_tmp".into(),
        source_label: SOURCE_LABEL,
        row_count: 8_431,
        dup_rows: Some(1_204),
        columns: vec![
            col(
                1,
                "id",
                "BIGINT",
                ColumnKind::Numeric,
                false,
                true,
                0.0,
                1.0,
                Some(numeric_detail()),
            ),
            col(
                2,
                "external_ref_id",
                "VARCHAR",
                ColumnKind::Text,
                true,
                false,
                0.8814,
                0.1204,
                Some(text_detail()),
            ),
            col(
                3,
                "email_bounced_at",
                "TIMESTAMP",
                ColumnKind::DateTime,
                true,
                false,
                0.9402,
                0.0031,
                Some(datetime_detail()),
            ),
            col(
                4,
                "mobile_verified",
                "BOOLEAN",
                ColumnKind::Boolean,
                true,
                false,
                0.6631,
                0.0074,
                Some(boolean_detail()),
            ),
            col(
                5,
                "legacy_notes",
                "TEXT",
                ColumnKind::Text,
                true,
                false,
                0.9912,
                0.0002,
                Some(text_detail()),
            ),
            col(
                6,
                "avatar_bin",
                "BLOB",
                ColumnKind::Unknown,
                true,
                false,
                0.5521,
                0.0009,
                None,
            ),
            col(
                7,
                "tags",
                "ARRAY<VARCHAR>",
                ColumnKind::Unknown,
                true,
                false,
                0.3314,
                0.1042,
                None,
            ),
            col(
                8,
                "score",
                "DOUBLE",
                ColumnKind::Numeric,
                true,
                false,
                0.2207,
                0.5518,
                Some(numeric_detail()),
            ),
            interrupted_col,
        ],
        quality: Some(TableQualityView {
            checker: vec![
                checker("id", "非空", ColumnCheckState::Hit, "无缺失"),
                checker(
                    "external_ref_id",
                    "非空",
                    ColumnCheckState::Miss,
                    "缺失 88.1%",
                ),
                checker(
                    "email_bounced_at",
                    "非空",
                    ColumnCheckState::Miss,
                    "缺失 94.0%",
                ),
                checker(
                    "mobile_verified",
                    "非空",
                    ColumnCheckState::Miss,
                    "缺失 66.3%",
                ),
                checker("legacy_notes", "非空", ColumnCheckState::Miss, "缺失 99.1%"),
                checker(
                    "avatar_bin",
                    "类型可识别",
                    ColumnCheckState::NotApplicableType,
                    "BLOB 不支持",
                ),
                checker(
                    "tags",
                    "类型可识别",
                    ColumnCheckState::NotApplicableType,
                    "ARRAY 不支持",
                ),
                checker("score", "非空", ColumnCheckState::Miss, "缺失 22.1%"),
            ],
            checker_applicable: 6,
            checker_hits: 1,
        }),
        progress: Some(TableEvalProgress { done: 8, total: 9 }),
        sample: None,
    };
    (target, profile)
}

fn col(
    index: i32,
    name: &str,
    data_type: &str,
    kind: ColumnKind,
    nullable: bool,
    primary_key: bool,
    null_rate: f64,
    unique_rate: f64,
    detail: Option<ColumnStatsDetail>,
) -> TableColumnView {
    TableColumnView {
        index,
        name: name.into(),
        data_type: data_type.into(),
        kind,
        nullable,
        primary_key,
        null_rate: Some(null_rate),
        null_count: None,
        unique_count: None,
        unique_rate: Some(unique_rate),
        stats_detail: detail,
        histogram: Some(histogram_bins()),
        sample_values: vec!["a".into(), "b".into(), "c".into()],
        mini_stats: Some(mini_stats(&kind)),
        interrupted: false,
    }
}

fn checker(column: &str, name: &str, state: ColumnCheckState, detail: &str) -> ColumnChecker {
    ColumnChecker {
        column: column.into(),
        name: name.into(),
        state,
        detail: detail.into(),
    }
}

// ============================================================================
// 快照历史
// ============================================================================

fn history_view() -> HistoryView {
    // 快照列表最新在前：第一条是「当前」，其余按版本链依次串下去
    let entries = vec![
        HistoryEntryView {
            version_id: "v9f2a1c8d3b7e5046".into(),
            short_version: "v9f2a1c8".into(),
            created_at: "2026-11-01 21:04:12".into(),
            data_type: "DECIMAL(18,2)".into(),
            has_parent: true,
            chain_truncated: false,
            is_latest: true,
        },
        HistoryEntryView {
            version_id: "v7c1e90ab2d4455f1".into(),
            short_version: "v7c1e90a".into(),
            created_at: "2026-10-31 18:22:47".into(),
            data_type: "DECIMAL(18,2)".into(),
            has_parent: true,
            chain_truncated: false,
            is_latest: false,
        },
        HistoryEntryView {
            version_id: "v3b8d2127ae09c944".into(),
            short_version: "v3b8d212".into(),
            created_at: "2026-10-29 09:11:30".into(),
            data_type: "DECIMAL(18,2)".into(),
            has_parent: true,
            chain_truncated: false,
            is_latest: false,
        },
        // 有父版本但父版已不在列表里（被清理剪掉了链头）
        HistoryEntryView {
            version_id: "v1a5f7730cc8e2108".into(),
            short_version: "v1a5f773".into(),
            created_at: "2026-10-24 14:57:03".into(),
            data_type: "DECIMAL(18,2)".into(),
            has_parent: true,
            chain_truncated: true,
            is_latest: false,
        },
    ];

    HistoryView {
        entity: "amount".into(),
        entries,
        stats: Some(StorageStatsView {
            total_snapshots: 4,
            unique_columns: 1,
            size_display: "12.4 KB".into(),
        }),
        truncated: false,
        diff: None,
        cleanup: None,
    }
}

// ============================================================================
// 列统计明细（列卡展开态那一屏）
// ============================================================================

fn numeric_detail() -> ColumnStatsDetail {
    ColumnStatsDetail::Numeric(NumericStats {
        min: 12.0,
        max: 9860.5,
        avg: 742.36,
        median: 528.75,
        p25: 214.0,
        p75: 968.4,
        sum: 9_264_652.8,
        stddev: Some(1043.82),
        skewness: Some(3.4182),
        kurtosis: Some(19.2204),
        zeros: Some(3),
        negatives: Some(0),
        p95: Some(3120.0),
        is_extreme: vec![
            ExtremeValue {
                value: 9860.5,
                kind: "max".into(),
            },
            ExtremeValue {
                value: 12.0,
                kind: "min".into(),
            },
        ],
    })
}

fn text_detail() -> ColumnStatsDetail {
    ColumnStatsDetail::Text(TextStats {
        min_length: 6,
        max_length: 18,
        top_values: top_frequencies(),
    })
}

fn datetime_detail() -> ColumnStatsDetail {
    ColumnStatsDetail::DateTime(DateTimeStats {
        earliest: "2026-01-04 08:12:41".into(),
        latest: "2026-10-31 22:07:19".into(),
        span_days: 300,
        monthly_distribution: vec![
            TextFrequency {
                value: "2026-01".into(),
                count: 1_204,
                ratio: 0.0965,
            },
            TextFrequency {
                value: "2026-02".into(),
                count: 1_118,
                ratio: 0.0896,
            },
            TextFrequency {
                value: "2026-03".into(),
                count: 1_402,
                ratio: 0.1123,
            },
        ],
    })
}

fn boolean_detail() -> ColumnStatsDetail {
    ColumnStatsDetail::Boolean(BooleanStats {
        true_count: 11_008,
        false_count: 1_472,
        true_ratio: 0.8821,
    })
}

fn top_frequencies() -> Vec<TextFrequency> {
    vec![
        TextFrequency {
            value: "C-0132".into(),
            count: 412,
            ratio: 0.0330,
        },
        TextFrequency {
            value: "C-0903".into(),
            count: 388,
            ratio: 0.0311,
        },
        TextFrequency {
            value: "C-0451".into(),
            count: 301,
            ratio: 0.0241,
        },
    ]
}

/// 直方图的「标签 + 占比」共用一份（分布条与迷你图来自同一组分箱）。
fn histogram_ratios() -> [(&'static str, f64); 6] {
    [
        ("12 – 997", 0.6128),
        ("997 – 1,982", 0.2215),
        ("1,982 – 2,967", 0.0982),
        ("2,967 – 3,952", 0.0431),
        ("3,952 – 4,937", 0.0169),
        ("4,937 – 9,860", 0.0075),
    ]
}

fn histogram_bins() -> Vec<DistributionBin> {
    histogram_ratios()
        .into_iter()
        .map(|(label, ratio)| DistributionBin {
            label: label.into(),
            count: (12_480.0 * ratio) as u32,
            ratio,
        })
        .collect()
}

fn mini_stats(kind: &ColumnKind) -> ColumnMiniStats {
    match kind {
        ColumnKind::Numeric => ColumnMiniStats::Numeric(histogram_bins()),
        ColumnKind::Boolean => ColumnMiniStats::Boolean { true_ratio: 0.8821 },
        ColumnKind::Text | ColumnKind::DateTime => ColumnMiniStats::Frequencies(top_frequencies()),
        // Unknown 类型列不给迷你统计（不编假值，与产品口径一致）
        ColumnKind::Unknown => ColumnMiniStats::Frequencies(Vec::new()),
    }
}
