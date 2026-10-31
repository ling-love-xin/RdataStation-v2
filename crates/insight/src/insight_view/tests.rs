//! `InsightView` 的测试（窗口级：`TestAppContext` + 真渲染）
//!
//! 与 `view/` 下的渲染片段分开放：片段是**渲染**，这里是**行为**（状态机 + 事件契约 +
//! 每个 Tab 的渲染冒烟）。宿主是 `insight_view.rs`，所以本模块是它的子模块
//! （`crate::insight_view::tests`）——状态宿主的私有入口（`set_tab_from_index` /
//! `set_open_sections`…）不必为了测试放宽可见性。
//!
//! 不写 `use gpui_kit::*` / `use super::*`：通配导入会把 gpui 的 `test` 属性宏带进作用域，
//! 而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己（recursion limit）。

use gpui_kit::{
    AppContext as _, Entity, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext,
    Window, div,
};

use super::{InsightEvent, InsightView};
use crate::model::{
    ColumnCheckState, ColumnChecker, ColumnProfileView, HistoryView, InsightPanelState,
    InsightTarget, MultiColumnView, MultiResultView, MultiRuleView, PanelTab,
    SNAPSHOT_RETENTION_DAYS, TableProfileView, VersionDiffView,
};
use crate::view::column::truncate;

// 领域类型从 crate 根再导出引用（`model.rs` 里对 `types` 的 `use` 是私有的）
use crate::{
    BooleanStats, ColumnInsightFull, ColumnStats, ColumnStatsDetail, DateTimeStats,
    DistributionBin, KeyValueRow, NoteLevel, NumericStats, QualityNote, SchemaExportFormat,
    TableColumnMeta, TableProfile, TextFrequency, TextStats,
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

/// 版本对比用的领域画像（行数 / 空值数可变，其余固定）
fn compare_insight(total_count: u32, null_count: u32) -> ColumnInsightFull {
    ColumnInsightFull {
        stats: ColumnStats {
            column_name: "amount".into(),
            data_type: "DECIMAL(12,2)".into(),
            total_count,
            null_count,
            null_rate: null_count as f64 / total_count.max(1) as f64,
            unique_count: Some(total_count / 2),
            stats_detail: ColumnStatsDetail::Numeric(NumericStats {
                min: 1.0,
                max: 99.0,
                avg: 50.0,
                median: 50.0,
                p25: 25.0,
                p75: 75.0,
                sum: 3500.0,
                stddev: Some(10.0),
                skewness: None,
                kurtosis: None,
                is_extreme: Vec::new(),
            }),
        },
        sample: vec![serde_json::json!(42), serde_json::Value::Null],
        histogram: None,
    }
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
        // 六 Tab（摘要首置）后，末位「历史」的索引是 5
        view.set_tab_from_index(5, cx);
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

/// 四态 + 六个 Tab 都能渲染一帧而不 panic（渲染是纯读路径，不许有副作用）
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
    // 数据态：列 Tab 四区 + 其余五个 Tab 的期次提示
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

/// 大表取样加载说明：源目标 Loading 且 note 已设置时渲染「说明 + 进度条」，
/// 没设置时是纯骨架（两种都必须能渲染一帧）。
///
/// note 由 `jobs::request_profile` 分阶段写入（取样中 → 画像中）；本测试直接
/// 走 `set_loading_note`，钉「有 note 渲染说明行、无 note 不渲染」这个契约。
#[gpui_kit::test]
fn source_loading_renders_sample_note_only_when_set(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));

    let source = crate::model::SampleSource::new("G_1", "SELECT 1", "shop.orders");
    // 源表目标 → 先弹探查确认窗口（C3），确认（默认配置）后才落到「表」Tab 并进入加载态
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_target(
                InsightTarget::SourceTable {
                    source,
                    table_name: "orders".into(),
                },
                cx,
            );
            view.confirm_probe_dialog(cx);
        });
        window.draw(cx).clear(cx);
    });
    // 取样说明未设置：纯骨架，不应出现说明行
    let note = cx.update(|window, _| {
        window.try_find(ElementId::Name(SharedString::from(
            "insight-sample-loading-note",
        )))
    });
    assert!(
        note.is_none(),
        "没写 loading_note 时不得渲染取样说明（临时表目标保持纯骨架）"
    );

    // jobs 写阶段说明后：说明行出现
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_loading_note(
                Some("正在取样并建分析临时表（大表可能需要一些时间）…".into()),
                cx,
            )
        });
        window.draw(cx).clear(cx);
    });
    let note = cx.update(|window, _| {
        window.find(ElementId::Name(SharedString::from(
            "insight-sample-loading-note",
        )))
    });
    assert!(note.visible(), "取样说明行应可见");

    // 阶段推进到「画像中」：文案跟着 jobs 的更新走（仍渲染说明行）
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_loading_note(Some("正在计算画像…".into()), cx)
        });
        window.draw(cx).clear(cx);
    });
    let note = cx.update(|window, _| {
        window.find(ElementId::Name(SharedString::from(
            "insight-sample-loading-note",
        )))
    });
    assert!(note.visible(), "画像阶段说明行应可见");
}

/// K22：切换洞察目标时，旧目标留下的阶段说明（loading_note）必须一并清空，
/// 不得挂到新目标上——换目标后界面不该还显示上一个目标的「正在取样」说明。
#[gpui_kit::test]
fn set_target_clears_previous_loading_note(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));

    let first = crate::model::SampleSource::new("G_1", "SELECT 1", "shop.orders");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_target(
                InsightTarget::SourceTable {
                    source: first,
                    table_name: "orders".into(),
                },
                cx,
            );
            view.confirm_probe_dialog(cx);
            view.set_loading_note(Some("正在取样并建分析临时表…".into()), cx);
        });
        window.draw(cx).clear(cx);
    });
    let note = cx.update(|window, _| {
        window.find(ElementId::Name(SharedString::from(
            "insight-sample-loading-note",
        )))
    });
    assert!(note.visible(), "前置：旧目标的阶段说明先渲染出来");

    // 切到另一张源表目标：确认（默认配置）后说明行必须随目标清空
    let second = crate::model::SampleSource::new("G_1", "SELECT 2", "shop.items");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_target(
                InsightTarget::SourceTable {
                    source: second,
                    table_name: "items".into(),
                },
                cx,
            );
            view.confirm_probe_dialog(cx);
        });
        window.draw(cx).clear(cx);
    });
    let note = cx.update(|window, _| {
        window.try_find(ElementId::Name(SharedString::from(
            "insight-sample-loading-note",
        )))
    });
    assert!(
        note.is_none(),
        "切换目标后上一目标的 loading_note 不得残留渲染"
    );
}

/// B1-a：源目标尚未取样（无样本表）时切到「多列」Tab，渲染专门取样引导空态，
/// 且不隐式发一次 `MultiColumnRequested`（与摘要「不触发取数」同口径）。
#[gpui_kit::test]
fn multi_tab_shows_sample_guide_without_request_when_no_sample_table(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
    let events = crate::test_support::event_sink(&view, cx);

    let source = crate::model::SampleSource::new("G_1", "SELECT 1", "shop.orders");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_target(
                InsightTarget::SourceTable {
                    source,
                    table_name: "orders".into(),
                },
                cx,
            );
            // 源目标先弹确认窗口（C3）；确认后才落到目标（样本表缺失语义照旧）
            view.confirm_probe_dialog(cx);
            view.set_tab(PanelTab::MultiColumn, cx);
        });
        window.draw(cx).clear(cx);
    });

    assert!(
        !events
            .borrow()
            .iter()
            .any(|e| matches!(e, InsightEvent::MultiColumnRequested { .. })),
        "样本表缺失时不得隐式发多列取数请求：{:?}",
        events.borrow()
    );

    // 右栏多列目标下钻 = dz-title + 聚合占位（2026-10-30 对齐原型后不再有
    // 「先取样」专门引导——聚合能力边界由 agg-placeholder 可见承担）
    let guide = cx.update(|window, _| {
        window.try_find(ElementId::Name(SharedString::from(
            "insight-multi-need-sample",
        )))
    });
    assert!(guide.is_none(), "右栏不再渲染多列专门取样引导（聚合占位承担）");
    let placeholder = cx.update(|window, _| {
        window.try_find(ElementId::Name(SharedString::from("insight-agg-placeholder")))
    });
    assert!(placeholder.is_some(), "多列聚合占位应可见（能力边界）");

    // 样本表就位后：专门引导不再出现（占位在加载期被骨架替换是正常路径）
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_source_sample("tmp_i_sample_1", cx);
            view.set_tab(PanelTab::MultiColumn, cx);
        });
        window.draw(cx).clear(cx);
    });
    let guide = cx.update(|window, _| {
        window.try_find(ElementId::Name(SharedString::from(
            "insight-multi-need-sample",
        )))
    });
    assert!(guide.is_none(), "样本表就位后专门引导应消失");
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
            // 1) 刚探查出来：有列清单、表级事实摘要为空（不编分数 / 等级）
            view.set_table_profile(base.clone(), cx);
        });
    });
    draw(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.tab(), PanelTab::Table, "表目标应落到「表」Tab");
        let table = view.data().table.as_ref().expect("表探查数据）；");
        assert!(table.quality.is_none(), "未评估不应出现表级事实摘要");
    });

    // 2) 评估中：进度行（逐列串行统计，不依赖旧评分合成）
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_table_profile(base.evaluating(1, 3), cx)
        });
    });
    draw(cx);

    // 3) 评估完：表级事实摘要 + 检查器四态计数（无总分 / 等级）
    let checks = vec![
        ColumnChecker {
            column: "id".into(),
            name: "非空率".into(),
            state: ColumnCheckState::Hit,
            detail: "非空率 100% ≥ 90%".into(),
        },
        ColumnChecker {
            column: "amount".into(),
            name: "近似正态".into(),
            state: ColumnCheckState::Miss,
            detail: "偏度 2.1（高度右偏）".into(),
        },
    ];
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_table_profile(
                base.evaluating(3, 3)
                    .finish_evaluation()
                    .with_checker(checks),
                cx,
            )
        });
    });
    draw(cx);
    view.update(cx, |view, _| {
        let table = view.data().table.as_ref().unwrap();
        let quality = table.quality.as_ref().expect("评估完应挂表级事实摘要");
        assert_eq!(quality.checker_hits, 1);
        assert_eq!(quality.checker_applicable, 2);
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
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_table_profile(table_view(), cx)));

    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_tab(PanelTab::MultiColumn, cx)));
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
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_multi_view(multi_view(), cx)));
    events.take();
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_tab(PanelTab::Table, cx)));
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_tab(PanelTab::MultiColumn, cx)));
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
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_multi_rule("pair-numeric", cx)));
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
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_multi_view(multi_view(), cx)));
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
    cx.update(|_window, cx| view.update(cx, |view, cx| view.request_table_drilldown("orders", cx)));
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
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_open_schema_sections(&[1, 3], cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |view, _| {
        assert_eq!(
            view.data().as_schema().map(|report| report.table_count),
            Some(4)
        );
    });
}

/// 回归：同一分区里多行引用同一张表时，下钻按钮的元素 id 必须两两不同。
///
/// 两个外键候选都指向 `users`（`orders.user_id` / `orders.ship_id`）。旧实现只按表名拼
/// 元素 id，同一帧会推出两个同 id 元素：a11y 节点重复，debug 构建直接 panic。
///
/// 测试窗口不激活 a11y（`Window::is_a11y_active` 恒 false），推不进 a11y 树，真渲染也
/// 复现不出这个 panic；所以这里按**渲染时的同一套下标**（分区 / 行 / 表）重建 id 清单，
/// 断言两两不同。顺带整帧绘制一遍，保证这块渲染本身不炸。
#[gpui_kit::test]
fn schema_drilldown_ids_are_unique_when_rows_share_a_table(cx: &mut TestAppContext) {
    use crate::schema_analyzer::{ForeignKeyCandidate, SchemaInsightReport};
    use std::collections::HashSet;

    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));

    let report = crate::schema_view::SchemaReportView::from_report(&SchemaInsightReport {
        schema_name: "public".into(),
        table_count: 2,
        total_columns: 8,
        fk_candidates: vec![
            ForeignKeyCandidate {
                source_table: "orders".into(),
                source_column: "user_id".into(),
                target_table: "users".into(),
                target_column: "id".into(),
                confidence: "high".into(),
                naming_pattern: "{table}_id".into(),
            },
            ForeignKeyCandidate {
                source_table: "orders".into(),
                source_column: "ship_id".into(),
                target_table: "users".into(),
                target_column: "id".into(),
                confidence: "medium".into(),
                naming_pattern: "{table}_id".into(),
            },
        ],
        type_mismatches: Vec::new(),
        orphan_tables: Vec::new(),
        redundant_columns: Vec::new(),
        summary: "两份到 users 的外键候选".into(),
        health_score: 80.0,
        health_level: "良好".into(),
    });

    // id 清单按渲染口径重建：分区 → 行 → 表，三级下标都进 id
    let ids: Vec<String> = crate::schema_view::SchemaSection::ALL
        .iter()
        .flat_map(|section| {
            let section = *section;
            report.group(section).into_iter().flat_map(move |group| {
                group
                    .rows
                    .iter()
                    .enumerate()
                    .flat_map(move |(row_ix, row)| {
                        row.tables
                            .iter()
                            .take(crate::ui::INSIGHT_SCHEMA_DRILLDOWN_LIMIT)
                            .enumerate()
                            .map(move |(table_ix, table)| {
                                crate::view::schema::drilldown_button_id(
                                    section, row_ix, table_ix, table,
                                )
                                .to_string()
                            })
                    })
            })
        })
        .collect();
    assert_eq!(ids.len(), 4, "两个候选各露出两张表：{ids:?}");
    assert_eq!(
        ids.len(),
        ids.iter().collect::<HashSet<_>>().len(),
        "同一分区里两行引用同一张表，下钻按钮 id 仍要两两不同：{ids:?}"
    );

    // 整帧绘制：这块渲染本身也不能炸
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
            view.set_schema_report(report, cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 结构 Tab 的导出：内容与文件名都在面板侧算好（导出与界面同源，D36），
/// 没有报告时不发事件（不发无人处理的请求）。
#[gpui_kit::test]
fn schema_export_carries_encoded_content_and_a_safe_name(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
    let events = crate::test_support::event_sink(&view, cx);

    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_target(
                InsightTarget::Schema {
                    conn_id: "G_pg".into(),
                    database: "shop".into(),
                    schema: Some("订单库".into()),
                },
                cx,
            );
        });
    });
    events.take();

    // 报告还没回来：导出请求发不出去（按钮此时也不渲染）
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.request_schema_export(SchemaExportFormat::Json, cx)
        });
    });
    assert!(events.borrow().is_empty(), "没有报告就不该发导出请求");

    // 回填报告：JSON 带真内容与保留中文的文件名
    let mut report = schema_report();
    report.schema_name = "订单库".into();
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_schema_report(report, cx)));
    events.take();
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.request_schema_export(SchemaExportFormat::Json, cx)
        });
    });
    let json = events
        .borrow()
        .iter()
        .find_map(|e| match e {
            InsightEvent::SchemaExportRequested {
                format,
                file_stem,
                content,
            } if *format == SchemaExportFormat::Json => Some((file_stem.clone(), content.clone())),
            _ => None,
        })
        .expect("应发出 JSON 导出请求");
    assert_eq!(json.0, "schema-订单库", "文件名保留中文（只挡非法字符）");
    assert!(
        json.1.contains("\"health_score\""),
        "内容应是真报告：{}",
        json.1
    );

    // Markdown 走同一个事件、不同编码
    events.take();
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.request_schema_export(SchemaExportFormat::Markdown, cx)
        });
    });
    assert!(
        events.borrow().iter().any(|e| matches!(
            e,
            InsightEvent::SchemaExportRequested { content, .. }
                if content.starts_with("# Schema 健康报告")
        )),
        "Markdown 应是同一个事件的不同编码：{:?}",
        events.borrow()
    );
}

/// 历史 Tab：保存入口发事件、切过去才取数、无项目时既不取数也不摆骨架
#[gpui_kit::test]
fn history_tab_saves_and_loads_only_with_a_project(cx: &mut TestAppContext) {
    use crate::store::{InsightStorageStats, InsightVersionEntry};

    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
    let events = crate::test_support::event_sink(&view, cx);

    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_target(column_target(), cx)));
    events.take();

    // 尚未告知项目状态（`project_open` 初值为 false）：切到历史**不请求、不转圈**，
    // 只给「打开项目后可保存与查看快照」的引导
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_tab(PanelTab::History, cx)));
    assert!(
        !events
            .borrow()
            .iter()
            .any(|e| matches!(e, InsightEvent::HistoryRequested { .. })),
        "无项目时历史读不到，不该发请求：{:?}",
        events.borrow()
    );
    view.update(cx, |view, _| {
        assert_eq!(view.state(), &InsightPanelState::Empty);
        assert_eq!(view.empty_hint(), "打开项目后可保存与查看快照");
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 告知项目已打开：再切回历史就补一次取数
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_project_open(true, cx);
            view.set_tab(PanelTab::Column, cx);
        });
    });
    events.take();
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_tab(PanelTab::History, cx)));
    assert!(
        events.borrow().iter().any(|e| matches!(
            e,
            InsightEvent::HistoryRequested { column } if column == "amount"
        )),
        "历史只问列名（别的定位由面板目标给）：{:?}",
        events.borrow()
    );
    events.take();

    // 保存：先置「保存中」（按钮置灰），发事件
    cx.update(|_window, cx| view.update(cx, |view, cx| view.request_snapshot_save(cx)));
    assert!(
        events.borrow().iter().any(|e| matches!(
            e,
            InsightEvent::SnapshotSaveRequested { temp_table, column, .. }
                if temp_table == "t_result_1" && column == "amount"
        )),
        "保存要带齐临时表与列：{:?}",
        events.borrow()
    );
    view.update(cx, |view, _| assert!(view.history_saving()));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 出数：保存中标记落回，列表渲染一帧
    let entries = vec![
        InsightVersionEntry {
            snapshot_id: "snap-2".into(),
            column_name: "amount".into(),
            data_type: Some("DECIMAL(12,2)".into()),
            stats_json: "{}".into(),
            version_id: "aaaaaaaa-1111".into(),
            parent_version_id: Some("bbbbbbbb-2222".into()),
            checksum: "sum-2".into(),
            created_at: "2026-09-15 14:22".into(),
        },
        InsightVersionEntry {
            snapshot_id: "snap-1".into(),
            column_name: "amount".into(),
            data_type: None,
            stats_json: "{}".into(),
            version_id: "bbbbbbbb-2222".into(),
            parent_version_id: None,
            checksum: "sum-1".into(),
            created_at: "2026-09-14 09:10".into(),
        },
    ];
    let stats = InsightStorageStats {
        total_snapshots: 2,
        unique_columns: 1,
        total_size_bytes: 2048.0,
        total_size_display: "2.0 KB".into(),
    };
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_history(
                HistoryView::from_entries("amount", &entries, Some(&stats)),
                cx,
            )
        });
    });
    view.update(cx, |view, _| {
        assert!(
            !view.history_saving(),
            "出数就认为保存结束（失败另有行内提示）"
        );
        assert_eq!(view.data().as_history().map(|h| h.entries.len()), Some(2));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 保存失败：只挂行内提示（目标头与已有历史照旧可见）
    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_history_notice("磁盘写满了", cx)));
    view.update(cx, |view, _| {
        assert!(!view.history_saving());
        assert_eq!(view.state(), &InsightPanelState::Data, "失败不抢整页错误态");
        assert_eq!(view.data().as_history().map(|h| h.entries.len()), Some(2));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 空历史（首版之前）也要能渲染：列表区给引导，不显示假数字
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_history(HistoryView::from_entries("amount", &[], None), cx)
        });
    });
    view.update(cx, |view, _| {
        let history = view.data().as_history().expect("空历史也是载荷");
        assert!(history.is_empty());
        assert!(history.stats_line().is_none(), "拿不到统计就不显示数字");
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 表目标上看不了历史（历史是「某列的历次快照」）：不发请求，也不留在骨架
#[gpui_kit::test]
fn history_tab_rejects_non_column_targets(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
    let events = crate::test_support::event_sink(&view, cx);

    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_project_open(true, cx);
            view.set_target(
                InsightTarget::Table {
                    temp_table: "t_history_table".into(),
                    table_name: "orders".into(),
                },
                cx,
            );
        });
    });
    events.take();

    cx.update(|_window, cx| view.update(cx, |view, cx| view.set_tab(PanelTab::History, cx)));
    assert!(
        !events
            .borrow()
            .iter()
            .any(|e| matches!(e, InsightEvent::HistoryRequested { .. })),
        "表目标说不清「看谁的历史」：{:?}",
        events.borrow()
    );
    // 关键：不能停在 Loading（那会一直转骨架），要落回空态给引导
    view.update(cx, |view, _| {
        assert_eq!(view.state(), &InsightPanelState::Empty);
        assert_eq!(view.empty_hint(), "历史按列记录：请先打开某一列的洞察");
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 历史 Tab 的对比（Phase 5.2）：点一版做基准 → 发事件；出数后面板才在；✕ 关掉
#[gpui_kit::test]
fn history_compare_selects_a_version_and_can_be_dismissed(cx: &mut TestAppContext) {
    use crate::store::InsightVersionEntry;

    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
    let events = crate::test_support::event_sink(&view, cx);

    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_project_open(true, cx);
            view.set_target(column_target(), cx);
            // 对比面板长在「历史」Tab 里：不切过去就先没得渲染
            view.set_tab(PanelTab::History, cx);
        });
    });
    let entries = |created_at: &str, version: &str, parent: Option<&str>| InsightVersionEntry {
        snapshot_id: format!("snap-{version}"),
        column_name: "amount".into(),
        data_type: Some("DECIMAL(12,2)".into()),
        stats_json: "{}".into(),
        version_id: version.into(),
        parent_version_id: parent.map(str::to_string),
        checksum: "sum".into(),
        created_at: created_at.into(),
    };
    let list = vec![
        entries("2026-09-15 14:22", "aaaaaaaa-1111", Some("bbbbbbbb-2222")),
        entries("2026-09-14 09:10", "bbbbbbbb-2222", None),
    ];

    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_history(HistoryView::from_entries("amount", &list, None), cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("insight-version-diff").is_none(),
        "没选版本时不该有对比面板"
    );
    events.take();

    // 最新那一版不给比：方向固定为「选中 → 最新」，拿它当基准无从比
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.toggle_compare_version("aaaaaaaa-1111", cx)
        })
    });
    assert!(
        events.take().is_empty(),
        "点最新一版不该发请求（它没有更新的版本可比）"
    );
    view.update(cx, |view, _| assert!(view.compare_target().is_none()));

    // 点更早的一版：选中位亮起 + 发请求（差值由接缝读两版正文后回填）
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.toggle_compare_version("bbbbbbbb-2222", cx)
        })
    });
    assert_eq!(
        view.read_with(cx, |view, _| view.compare_target().map(str::to_string)),
        Some("bbbbbbbb-2222".into())
    );
    assert!(
        events.borrow().iter().any(|e| matches!(
            e,
            InsightEvent::VersionCompareRequested { column, version_id }
                if column == "amount" && version_id == "bbbbbbbb-2222"
        )),
        "对比要带名单与版本号：{:?}",
        events.borrow()
    );
    // 结果未到时先给取数提示，不假装有差值
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("insight-version-diff").is_none());
    events.take();

    // 出数：载荷带 diff（右栏不再渲染历史对比面板——render_history 保留待中间页
    // 复用；此处断言状态层：diff 在载荷里、选中位跟载荷走、不抢整页状态）
    let diff = VersionDiffView::between(
        &compare_insight(1000, 20),
        "2026-09-14 09:10",
        &compare_insight(2000, 10),
        "2026-09-15 14:22",
    )
    .with_baseline_version("bbbbbbbb-2222");
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_history(
                HistoryView::from_entries("amount", &list, None).with_diff(diff),
                cx,
            )
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |view, _| {
        assert!(
            view.data().as_history().is_some_and(|h| h.diff.is_some()),
            "出数后载荷应带对比 diff"
        );
        assert_eq!(
            view.compare_target(),
            Some("bbbbbbbb-2222"),
            "选中位跟载荷走"
        );
        assert_eq!(view.state(), &InsightPanelState::Data, "对比不抢整页状态");
    });

    // ✕ 关掉：只改自身状态（不发事件）
    cx.update(|_window, cx| view.update(cx, |view, cx| view.dismiss_diff(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |view, _| {
        assert!(view.compare_target().is_none());
        assert!(
            view.data()
                .as_history()
                .is_some_and(|h| h.diff.is_none() && h.entries.len() == 2),
            "列表要留着（关掉的只是对比）"
        );
    });
    assert!(events.take().is_empty(), "关面板是本地状态，不该发事件");

    // 刷新列表（保存 / ⟳）：对比不再自动续取——留着会指向已变的「当前」
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.toggle_compare_version("bbbbbbbb-2222", cx)
        })
    });
    events.take();
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_history(HistoryView::from_entries("amount", &list, None), cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.update(cx, |view, _| {
        assert!(
            view.compare_target().is_none(),
            "列表刷新后选中位要落回（载荷里没有对比）"
        );
    });
}

/// 对比取数失败：挂行内提示并放掉选中位（不留「亮着但没面板」的死状态）
#[gpui_kit::test]
fn compare_failure_releases_the_selection(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));

    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_project_open(true, cx);
            view.set_target(column_target(), cx);
            view.set_tab(PanelTab::History, cx);
            view.set_history(
                HistoryView::from_entries(
                    "amount",
                    &[crate::store::InsightVersionEntry {
                        snapshot_id: "snap-1".into(),
                        column_name: "amount".into(),
                        data_type: Some("DOUBLE".into()),
                        stats_json: "{}".into(),
                        version_id: "bbbbbbbb-2222".into(),
                        parent_version_id: None,
                        checksum: "sum".into(),
                        created_at: "2026-09-14 09:10".into(),
                    }],
                    None,
                ),
                cx,
            );
        });
    });
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_compare_notice("这一版已不在历史里", cx)
        })
    });
    view.update(cx, |view, _| {
        assert!(view.compare_target().is_none());
        assert_eq!(view.state(), &InsightPanelState::Data, "不抢整页错误态");
        assert!(
            view.data()
                .as_history()
                .is_some_and(|h| !h.is_empty() && h.diff.is_none()),
            "列表照旧可见"
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 清理：入口要过确认框（删快照不可撤销），确认后才发请求，回执落在载荷里
#[gpui_kit::test]
fn cleanup_asks_first_and_reports_in_payload(cx: &mut TestAppContext) {
    use gpui_kit::component::{Root, WindowExt as _};

    use crate::model::CleanupOutcome;
    use crate::store::InsightVersionEntry;

    /// 窗口根必须是组件库的 `Root`（`open_dialog` / `render_dialog_layer` 依赖它）
    struct Harness {
        panel: Entity<InsightView>,
    }

    impl Render for Harness {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut gpui_kit::Context<Self>,
        ) -> impl IntoElement {
            let root = div().size_full().child(self.panel.clone());
            root
        }
    }

    cx.update(gpui_kit::init);
    let panel = cx.new(InsightView::new);
    let panel_in = panel.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let harness = cx.new(|_cx| Harness { panel: panel_in });
        Root::new(harness, window, cx)
    });

    // 有项目 + 一版快照：入口才可用
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| {
            view.set_project_open(true, cx);
            view.set_target(column_target(), cx);
            view.set_tab(PanelTab::History, cx);
            view.set_history(
                HistoryView::from_entries(
                    "amount",
                    &[InsightVersionEntry {
                        snapshot_id: "snap-1".into(),
                        column_name: "amount".into(),
                        data_type: Some("DOUBLE".into()),
                        stats_json: "{}".into(),
                        version_id: "aaaaaaaa-1111".into(),
                        parent_version_id: None,
                        checksum: "sum".into(),
                        created_at: "2026-09-15 14:22".into(),
                    }],
                    None,
                ),
                cx,
            );
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    panel.read_with(cx, |view, _| {
        assert!(
            view.history_busy_ready(view.data().as_history().expect("历史载荷")),
            "有快照且无动作在跑时，清理入口应可用"
        );
    });

    // 点入口：只弹确认框，**不发请求**（此时删库是不对的）
    cx.update(|window, cx| {
        panel.update(cx, |view, cx| view.begin_cleanup(window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "清理必须过一道确认框"
    );
    assert!(!panel.read_with(cx, |view, _| view.history_cleaning()));

    // 确认后才发请求（确认按钮的回调就是这个方法）
    cx.update(|_window, cx| panel.update(cx, |view, cx| view.request_cleanup(cx)));
    assert!(
        panel.read_with(cx, |view, _| view.history_cleaning()),
        "确认后立即进入「清理中」（按钮置灰的依据）"
    );
    // 清理中不允许再点一次（保存与清理都在动同一批快照）
    panel.read_with(cx, |view, _| {
        assert!(!view.history_busy_ready(view.data().as_history().expect("历史载荷")))
    });

    // 出数：列表 + 回执一起回来（没东西可清也是一种结果，不是错误）
    cx.update(|_window, cx| {
        panel.update(cx, |view, cx| {
            view.set_history(
                HistoryView::from_entries(
                    "amount",
                    &[InsightVersionEntry {
                        snapshot_id: "snap-1".into(),
                        column_name: "amount".into(),
                        data_type: Some("DOUBLE".into()),
                        stats_json: "{}".into(),
                        version_id: "aaaaaaaa-1111".into(),
                        parent_version_id: None,
                        checksum: "sum".into(),
                        created_at: "2026-09-15 14:22".into(),
                    }],
                    None,
                )
                .with_cleanup(CleanupOutcome {
                    days: SNAPSHOT_RETENTION_DAYS,
                    body_removed: 0,
                    meta_removed: 0,
                }),
                cx,
            );
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    panel.read_with(cx, |view, _| {
        assert!(!view.history_cleaning(), "出数后「清理中」要落回");
        assert_eq!(view.state(), &InsightPanelState::Data);
        let history = view.data().as_history().expect("历史载荷");
        let receipt = history.cleanup.as_ref().expect("回执应在载荷里");
        assert!(receipt.is_balanced() && receipt.days == SNAPSHOT_RETENTION_DAYS);
        assert_eq!(history.entries.len(), 1, "没东西可清：列表照旧");
    });
}

#[test]
fn truncate_counts_characters_not_bytes() {
    assert_eq!(truncate("abc", 5), "abc");
    assert_eq!(truncate("abcdef", 3), "abc…");
    // 中文按字符计数，不得切出半个字
    assert_eq!(truncate("中文测试", 2), "中文…");
}

/// 源目标构造（全量两段式测试用）：DuckDB 内存源（conn_id 为 `None`）。
fn source_target(name: &str) -> InsightTarget {
    InsightTarget::SourceTable {
        source: crate::SampleSource::on_duckdb(format!("SELECT * FROM {name}"), name.to_string()),
        table_name: name.into(),
    }
}

/// 全量统计是**两段式**：`request_full` 只发确认请求，不切 scope / 不发画像请求；
/// 宿主确认后调 `confirm_full` 才真正切换（取消 = 什么都不做）。
///
/// 2026-10-31 Q2 定稿：`confirm_full` 由「重取源全量建表」改为「切全量口径 +
/// 触发评估」——全量 = 评估确认窗口生成的表，不重取源；未评估才发评估请求。
#[gpui_kit::test]
fn request_full_waits_for_host_confirmation(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
    let events = crate::test_support::event_sink(&view, cx);

    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_target(source_target("orders"), cx);
            // 源目标进入洞察先弹确认窗口（C3）：确认（默认配置）后再请求全量
            view.confirm_probe_dialog(cx);
            view.request_full(cx);
        });
    });
    assert!(
        events
            .borrow()
            .iter()
            .any(|e| matches!(e, InsightEvent::FullProfileConfirmRequested { .. })),
        "第一段：只发确认请求：{:?}",
        events.borrow()
    );
    assert!(
        !events
            .borrow()
            .iter()
            .any(|e| matches!(e, InsightEvent::FullProfileRequested { .. })),
        "确认前不得直接发画像请求"
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.scope(), super::InsightScope::Sample, "确认前口径不变");
        assert!(view.state().is_loading() || view.state() == &InsightPanelState::Empty);
    });

    // 宿主确认后：切全量口径；Q2 定稿下不再重取源全量建表（全量 = 评估确认窗口
    // 生成的表，未评估时触发评估全表；测试环境无临时表 → 评估请求不发，只切口径）
    events.take();
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| view.confirm_full(cx));
    });
    assert!(
        !events
            .borrow()
            .iter()
            .any(|e| matches!(e, InsightEvent::FullProfileRequested { .. })),
        "确认后不得重取源全量（Q2：全量 = 评估确认窗口生成的表）"
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.scope(), super::InsightScope::Full);
    });
}

/// 临时表目标没有「取样 / 全量」概念：`request_full` 静默忽略（不发事件、不动状态）。
#[gpui_kit::test]
fn request_full_is_noop_for_temp_table_targets(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
    let events = crate::test_support::event_sink(&view, cx);

    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_target(
                InsightTarget::Table {
                    temp_table: "t_result_1".into(),
                    table_name: "orders".into(),
                },
                cx,
            )
        });
        view.update(cx, |view, cx| view.request_full(cx));
    });
    assert!(
        !events.borrow().iter().any(|e| matches!(
            e,
            InsightEvent::FullProfileConfirmRequested { .. }
                | InsightEvent::FullProfileRequested { .. }
        )),
        "临时表目标不发全量类事件（set_target 的普通取数事件不算）：{:?}",
        events.borrow()
    );
    view.read_with(cx, |view, _| {
        assert_eq!(view.scope(), super::InsightScope::Sample);
    });
}

/// 偏好推送：`apply_insight_prefs` 落字段；关闭「显示内存占用」时读数清空。
#[gpui_kit::test]
fn apply_insight_prefs_updates_fields(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));

    // 初值与设置默认一致
    view.read_with(cx, |view, _| {
        assert!(view.status_usage);
        assert_eq!(view.work_set_limit, 30);
    });
    // 宿主装配时用真实设置覆盖
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| view.apply_insight_prefs(false, 0, cx));
    });
    view.read_with(cx, |view, _| {
        assert!(!view.status_usage);
        assert_eq!(view.work_set_limit, 0, "0 = 不提示");
    });
}

/// C6 / R5 重新探查：已登记目标 + 已有画像时确认走「不闪空」分支——
/// 保留旧画像与阶段条说明、发重新取数请求（新表就绪后 jobs 一次性替换）。
#[gpui_kit::test]
fn reprobe_keeps_profile_until_table_rebuilt(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|_window, cx| InsightView::new(cx));
    let events = crate::test_support::event_sink(&view, cx);

    let target = InsightTarget::Table {
        temp_table: "t_reprobe".into(),
        table_name: "orders".into(),
    };
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| {
            view.set_target(target.clone(), cx);
            view.data.table = Some(TableProfileView {
                table_name: "orders".into(),
                source_label: "DuckDB 临时表",
                row_count: 3,
                dup_rows: None,
                columns: Vec::new(),
                quality: None,
                progress: None,
                sample: None,
            });
        });
    });
    // 重新探查 = 复用确认窗口（预填上次配置）
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| view.open_probe_dialog(target.clone(), cx));
    });
    view.read_with(cx, |view, _| {
        assert!(
            view.probe_dialog.is_some(),
            "重新探查应打开确认窗口（C6）"
        );
    });
    cx.update(|_window, cx| {
        view.update(cx, |view, cx| view.confirm_probe_dialog(cx));
    });
    view.read_with(cx, |view, _| {
        assert!(view.data.table.is_some(), "R5：确认后旧画像保留（不闪空）");
        assert!(
            view.loading_note.is_some(),
            "重新探查中应显示阶段条说明"
        );
        assert!(view.target().is_some(), "目标保留");
    });
    assert!(
        events.borrow().iter().any(|e| matches!(
            e,
            InsightEvent::ProfileRequested {
                target: t,
                config: Some(_)
            } if t == &target
        )),
        "应发带上次配置的重新取数请求：{:?}",
        events.borrow()
    );
}
