//! `mock_view` 的测试：纯逻辑（无窗口）+ 窗口级（GPUI headless）。
//!
//! 宿主能力全部用测试桥（记录调用，不接 workbench 与真实分析库）。
//!
//! 注意：这里**不使用** `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的
//! `test` 属性宏带入作用域，`#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己，
//! 造成无限递归（recursion limit reached）。所有依赖显式列举。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::IndexPath;
use gpui_kit::component::dock::{DockArea, DockPlacement};
use gpui_kit::component::list::{ListDelegate as _, ListState};
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::{
    App, AppContext as _, Entity, IntoElement, ParentElement, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};

use super::{
    DetailTarget, HistoryReply, JobRowScope, MockColumnSpec, MockDetailView, MockDraft,
    MockGenInfo, MockHost, MockJobDone, MockJobKind, MockJobPhase, MockJobProgress, MockJobState,
    MockPanel, MockPreview, MockRunOptions, RelationPick, ScenarioRelation, SchemaRequest,
    SchemaSource, TableStatus, focus_detail_tab, param_text, parse_percent_ratio, parse_rows,
    parse_seed, patch_param, search_generators, summarize_params, validate_table_name,
};
use crate::commands::GenerateMock;
use crate::generator_catalog::{self, ParamKind};
use crate::history;
use crate::models::{
    ColumnDataType, ColumnDef, ColumnDependency, GeneratorConfig, Locale, MockExportFormat,
    ScenarioTemplate, TemplateTable,
};
use crate::persistence::{
    MockGenerationColumn, MockGenerationDetail, MockGenerationTask, MockTemplateColumn,
    MockUserTemplate,
};
use crate::schema_map::ColumnMapper;

// ==================== 纯逻辑（无窗口） ====================

#[test]
fn parse_rows_accepts_positive_and_clamps() {
    assert_eq!(parse_rows("1000"), Ok(1000));
    assert_eq!(parse_rows(" 42 "), Ok(42));
    assert_eq!(parse_rows("99999999"), Ok(1_000_000));
    assert!(parse_rows("0").is_err());
    assert!(parse_rows("-5").is_err());
    assert!(parse_rows("abc").is_err());
    assert!(parse_rows("").is_err());
}

#[test]
fn parse_seed_treats_blank_as_random() {
    assert_eq!(parse_seed(""), Ok(None));
    assert_eq!(parse_seed("   "), Ok(None));
    assert_eq!(parse_seed("42"), Ok(Some(42)));
    assert!(parse_seed("abc").is_err());
    assert!(parse_seed("-1").is_err());
}

#[test]
fn parse_percent_ratio_clamps_and_rejects_garbage() {
    assert_eq!(parse_percent_ratio("10"), Some(0.1));
    assert_eq!(parse_percent_ratio("100"), Some(1.0));
    assert_eq!(parse_percent_ratio("250"), Some(1.0));
    assert_eq!(parse_percent_ratio("-30"), Some(0.0));
    assert_eq!(parse_percent_ratio("abc"), None);
    assert_eq!(parse_percent_ratio(""), None);
}

#[test]
fn validate_table_name_rejects_illegal_identifiers() {
    assert_eq!(validate_table_name(" orders "), Ok("orders".to_string()));
    assert_eq!(
        validate_table_name("mock_data2"),
        Ok("mock_data2".to_string())
    );
    assert!(validate_table_name("").is_err(), "空表名应被拒");
    assert!(validate_table_name("   ").is_err());
    assert!(validate_table_name("1orders").is_err(), "数字开头应被拒");
    assert!(validate_table_name("order id").is_err(), "空格应被拒");
    assert!(validate_table_name("orders;drop").is_err());
}

#[test]
fn patch_param_rewrites_scalar_fields() {
    let config = GeneratorConfig::AutoIncrement { start: 1, step: 1 };
    let patched = patch_param(&config, "start", "100", ParamKind::Int).expect("补丁应成功");
    assert!(matches!(
        patched,
        GeneratorConfig::AutoIncrement {
            start: 100,
            step: 1
        }
    ));

    // 非法输入保留原值（`None` → 调用方不写回）
    assert!(patch_param(&config, "start", "", ParamKind::Int).is_none());
    assert!(patch_param(&config, "start", "abc", ParamKind::Int).is_none());
    assert!(patch_param(&config, "nope", "1", ParamKind::Int).is_none());
}

#[test]
fn patch_param_handles_strings_floats_and_options() {
    let date = GeneratorConfig::date_time("2020-01-01", "2025-12-31");
    let patched = patch_param(&date, "min", "2019-06-01", ParamKind::Text).expect("补丁应成功");
    match patched {
        GeneratorConfig::DateTime { min, .. } => assert_eq!(min, "2019-06-01"),
        other => panic!("变体不应改变: {other:?}"),
    }

    let float = GeneratorConfig::RandomFloat {
        min: 0.0,
        max: 1.0,
        precision: 2,
    };
    let patched = patch_param(&float, "max", "9.5", ParamKind::Float).expect("补丁应成功");
    assert!(matches!(
        patched,
        GeneratorConfig::RandomFloat { max, .. } if (max - 9.5).abs() < f64::EPSILON
    ));

    // Option<u32> 字段：JSON 数字应能写回
    let image = GeneratorConfig::ImageUrlCustom {
        width: 200,
        height: 200,
        grayscale: false,
        blur_amount: Some(2),
        seed: None,
    };
    let patched = patch_param(&image, "seed", "42", ParamKind::Int).expect("补丁应成功");
    match patched {
        GeneratorConfig::ImageUrlCustom { seed, .. } => assert_eq!(seed, Some(42)),
        other => panic!("变体不应改变: {other:?}"),
    }
}

#[test]
fn summarize_params_reads_declared_fields() {
    let config = GeneratorConfig::RandomInt { min: 3, max: 7 };
    let text = summarize_params(&config);
    assert!(text.contains("最小值 3"), "{text}");
    assert!(text.contains("最大值 7"), "{text}");

    // 无参变体 → 空摘要
    assert!(summarize_params(&GeneratorConfig::UuidV4).is_empty());
}

#[test]
fn param_text_matches_serialized_value() {
    let config = GeneratorConfig::RandomInt { min: 3, max: 7 };
    assert_eq!(param_text(&config, "min"), "3");
    assert_eq!(param_text(&config, "missing"), "");
}

/// 智能映射可被面板直接复用（「恢复智能默认」路径）。
#[test]
fn column_mapper_supplies_manual_reset_values() {
    let mapped = ColumnMapper::infer("email", &ColumnDataType::Varchar { length: None });
    assert_eq!(mapped.confidence, "high");
    assert!(matches!(mapped.generator, GeneratorConfig::SafeEmail));
    assert!(!mapped.sample_value.is_empty());
}

/// 语言显示名必须齐备（13 种），未知值不 panic。
#[test]
fn locale_labels_are_stable() {
    for locale in super::LOCALES {
        assert!(!super::locale_label(&locale).is_empty());
    }
    assert_eq!(super::locale_label(&Locale::ZhCn), "简体中文");
    assert_eq!(super::locale_label(&Locale::En), "英语");
}

/// 13 种列类型都能给出非空显示名（下拉与字段行共用）。
#[test]
fn column_type_labels_are_complete() {
    for data_type in super::COLUMN_TYPES {
        assert!(
            !super::column_type_label(&data_type).is_empty(),
            "{data_type:?}"
        );
    }
    assert_eq!(
        super::column_type_label(&ColumnDataType::Integer),
        "INTEGER"
    );
}

// ==================== 工作日历展示（纯逻辑） ====================

/// 工作周掩码翻成人话：连续区间写成「周一~周五」，离散的逐天列出。
#[test]
fn work_week_mask_reads_as_plain_chinese() {
    assert_eq!(super::work_week_text("1111100"), "周一~周五");
    assert_eq!(super::work_week_text("1111110"), "周一~周六");
    assert_eq!(super::work_week_text("0111111"), "周二~周日");
    assert_eq!(super::work_week_text("1111111"), "每天");
    assert_eq!(super::work_week_text("1000000"), "周一");
    // 周一 + 周日不相邻（跨了整周），不能写成「周一~周日」
    assert_eq!(super::work_week_text("1000001"), "周一、周日");
    assert_eq!(super::work_week_text("1010100"), "周一、周三、周五");
    assert_eq!(super::work_week_text("0000000"), "（无上班日）");
    // 非法掩码原样返回（原因由生成前护栏给出）
    assert_eq!(super::work_week_text("111100"), "111100");
}

/// 字段卡片摘要在日历参数上不能失控：开关关着 / 列表为空时都不占位置。
#[test]
fn calendar_params_stay_out_of_the_summary_until_they_are_used() {
    let off = generator_catalog::default_of("sequential_date").expect("默认配置");
    let text = super::summarize_params(&off);
    assert!(!text.contains("工作周"), "未启用时不该占位置：{text}");
    assert!(!text.contains("仅工作日"), "关着的开关不该占位置：{text}");

    let on = super::patch_param_value(&off, "workdays_only", serde_json::Value::from(true))
        .expect("写回");
    let on = super::patch_param_value(
        &on,
        "skip_dates",
        serde_json::Value::Array(vec![serde_json::Value::from("2026-10-01")]),
    )
    .expect("写回");
    let text = super::summarize_params(&on);
    assert!(text.contains("仅工作日 是"), "{text}");
    assert!(
        text.contains("工作周（周一~周日，1 上班） 周一~周五"),
        "掩码应翻成人话：{text}"
    );
    assert!(text.contains("跳过日期（节假日） 1 项"), "{text}");
    assert!(!text.contains("上班日期（调休）"), "空列表不占位置：{text}");
}

/// 工作时段窗口：只在勾了开关 / 不是默认窗口时才进摘要。
#[test]
fn work_hours_window_shows_up_only_when_it_matters() {
    let base = generator_catalog::default_of("date_time_between").expect("默认配置");
    assert!(
        !super::summarize_params(&base).contains("工作时段"),
        "未启用时不该占位置"
    );

    let on = super::patch_param_value(&base, "work_hours_only", serde_json::Value::from(true))
        .expect("写回");
    assert!(super::summarize_params(&on).contains("仅工作时段 是"));
    assert!(
        !super::summarize_params(&on).contains("工作时段起"),
        "默认窗口不重复占位（开关标签已说明）"
    );

    let custom = super::patch_param_value(&on, "work_hour_end", serde_json::Value::from("22:30"))
        .expect("写回");
    let text = super::summarize_params(&custom);
    assert!(text.contains("工作时段止（HH:MM） 22:30"), "{text}");
}

// ==================== 生成器搜索（纯逻辑） ====================

/// 空查询＝全量目录（对话框初态），顺序与目录一致。
#[test]
fn generator_search_empty_query_returns_whole_catalog() {
    let hits = search_generators("");
    assert_eq!(hits.len(), generator_catalog::all_specs().len());
    assert_eq!(hits.len(), 143);
    assert_eq!(hits[0].name, generator_catalog::all_specs()[0].name);
}

/// 标签前缀命中排在标签包含之前（「邮箱」→ 邮箱地址 先于 安全邮箱）。
#[test]
fn generator_search_ranks_label_prefix_first() {
    let names: Vec<&str> = search_generators("邮箱").iter().map(|s| s.name).collect();
    let email = names
        .iter()
        .position(|name| *name == "email")
        .expect("邮箱地址应命中");
    let safe = names
        .iter()
        .position(|name| *name == "safe_email")
        .expect("安全邮箱应命中");
    assert!(email < safe, "前缀命中应更靠前: {names:?}");
}

/// 多词是 AND：每个词都要命中（可分别落在标签 / 名称 / 分类上）。
#[test]
fn generator_search_requires_every_term() {
    let names: Vec<&str> = search_generators("邮箱 地址")
        .iter()
        .map(|s| s.name)
        .collect();
    assert!(names.contains(&"email"), "{names:?}");
    assert!(
        !names.contains(&"safe_email"),
        "缺一个词就不该命中: {names:?}"
    );
    assert!(
        search_generators("邮箱 uuid").is_empty(),
        "没有同时命中两个词的生成器"
    );
}

/// 大小写不敏感；名称前缀也能命中（`uuid` → UUID v4）。
#[test]
fn generator_search_ignores_case() {
    let lower: Vec<&str> = search_generators("uuid").iter().map(|s| s.name).collect();
    let upper: Vec<&str> = search_generators("UUID").iter().map(|s| s.name).collect();
    assert_eq!(lower, upper, "大小写不应改变结果");
    assert!(
        lower.first().is_some_and(|name| name.starts_with("uuid")),
        "{lower:?}"
    );
}

/// 分类名也能搜到（如「约束」）；无命中＝空清单（对话框显 `List` 自带空态）。
#[test]
fn generator_search_matches_category_and_handles_miss() {
    assert!(search_generators("绝不存在的生成器").is_empty());

    let by_category = search_generators("约束");
    assert!(!by_category.is_empty(), "分类名应能搜到");
    for spec in by_category.iter() {
        assert!(
            spec.label.contains("约束")
                || spec.name.contains("约束")
                || spec.category.label().contains("约束"),
            "命中项必须在标签 / 名称 / 分类里含关键词: {}",
            spec.name
        );
    }
}

// ==================== 复杂参数（集合 / 加权选项） ====================

/// 取值集合：一行一个值，空行忽略；回读文本与写入一致（往返）。
#[test]
fn complex_values_round_trip() {
    let value = super::parse_complex_param("values", " 已发货 \n\n已取消\n").expect("解析");
    assert_eq!(value, serde_json::json!(["已发货", "已取消"]));

    let config = GeneratorConfig::Sequence {
        values: vec!["甲".to_string(), "乙".to_string()],
        cycle: true,
    };
    assert_eq!(super::complex_param_text(&config, "values"), "甲\n乙");
}

/// 加权选项：一行「值, 权重」，分隔符取**最后一个**（半角/全角逗号、制表符都能当分隔符，
/// 所以值里可以带逗号）。
#[test]
fn complex_choices_accept_separator_variants() {
    let value =
        super::parse_complex_param("choices", "北京, 北京市\t3\n上海，2\n广州,1").expect("解析");
    assert_eq!(
        value,
        serde_json::json!([["北京, 北京市", 3.0], ["上海", 2.0], ["广州", 1.0]])
    );

    let config = GeneratorConfig::Weighted {
        choices: vec![("北京".to_string(), 3.0), ("上海".to_string(), 0.5)],
    };
    assert_eq!(
        super::complex_param_text(&config, "choices"),
        "北京, 3\n上海, 0.5"
    );
}

/// 非法输入都给带行号的可读原因（不猜、不静默吞掉）。
#[test]
fn complex_param_errors_are_readable() {
    assert_eq!(
        super::parse_complex_param("values", " \n\n").unwrap_err(),
        "至少要有一个值"
    );
    assert!(
        super::parse_complex_param("choices", "北京, x")
            .unwrap_err()
            .contains("第 1 行")
    );
    assert!(
        super::parse_complex_param("choices", "北京, -1")
            .unwrap_err()
            .contains("不小于 0")
    );
    assert!(
        super::parse_complex_param("choices", "北京, 0\n上海, 0")
            .unwrap_err()
            .contains("至少一个权重")
    );
    assert!(
        super::parse_complex_param("choices", "北京")
            .unwrap_err()
            .contains("期望")
    );

    let too_many = (0..1001)
        .map(|index| format!("v{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        super::parse_complex_param("values", &too_many)
            .unwrap_err()
            .contains("最多"),
        "应有项数上限"
    );
}

/// 工作日历的两个日期列表：**允许清空**（“今年没有额外假日”是合法配置），
/// 并且能原样回填到多行文本框里。
#[test]
fn calendar_date_lists_round_trip_and_allow_empty() {
    let empty = super::parse_complex_param("skip_dates", " \n\n").expect("日历列表允许清空");
    assert_eq!(empty, serde_json::Value::Array(Vec::new()));

    let parsed =
        super::parse_complex_param("work_dates", "2026-10-10\n\n 2026-10-11 ").expect("解析");
    let mut config = generator_catalog::default_of("sequential_date").expect("默认配置");
    config = super::patch_param_value(&config, "work_dates", parsed).expect("写回");
    assert_eq!(
        super::complex_param_text(&config, "work_dates"),
        "2026-10-10\n2026-10-11"
    );
}

/// 参数摘要带上集合项数（字段行一眼可见「取值集合 3 项」）。
#[test]
fn summary_counts_complex_items() {
    let config = GeneratorConfig::ForeignKey {
        values: vec!["a".to_string(), "b".to_string(), "c".to_string()],
    };
    let summary = super::summarize_params(&config);
    assert!(summary.contains("3 项"), "{summary}");
}

/// 行数文案的千分位：只给数据行数用，`第 N 行` 这类行号不过它。
#[test]
fn thousands_grouping_covers_the_scenario_scale() {
    assert_eq!(super::with_thousands(0), "0");
    assert_eq!(super::with_thousands(999), "999");
    assert_eq!(super::with_thousands(1_000), "1,000");
    assert_eq!(super::with_thousands(21_500), "21,500");
    assert_eq!(super::with_thousands(161_000), "161,000");
    assert_eq!(super::with_thousands(1_234_567), "1,234,567");
}

/// 场景菜单文案：用真实模板规模（名称 + 张表 + 千分位行数）。
#[test]
fn scenario_menu_label_reads_the_template_scale() {
    let choices = super::builtin_scenario_choices();
    assert_eq!(choices.len(), 6, "内置 6 套");
    let ecommerce = choices
        .iter()
        .find(|choice| choice.id == "builtin:ecommerce")
        .expect("电商模板");
    assert_eq!(ecommerce.menu_label(), "电商系统（4 张表 · 21,500 行）");
    let social = choices
        .iter()
        .find(|choice| choice.id == "builtin:social_media")
        .expect("社交模板");
    assert_eq!(social.menu_label(), "社交平台（4 张表 · 161,000 行）");
}

// ==================== 测试宿主桥 ====================

/// 假宿主给场景任务回的三张表（表名，行数）。
const SCENARIO_TABLES: [(&str, u32); 3] = [("orders", 100), ("items", 250), ("users", 40)];

#[derive(Default)]
struct Recorder {
    notifies: Cell<usize>,
    opened_detail: Cell<usize>,
    /// 打开过的详情 tab（`DetailTarget::key()`，按顺序）
    opened_targets: RefCell<Vec<String>>,
    /// 提交的后台任务（种类）
    started: RefCell<Vec<MockJobKind>>,
    /// 取消请求次数
    cancels: Cell<usize>,
    /// (目标表, 行数, 种子, 追加目标)
    generated: RefCell<Vec<(String, u32, Option<u32>, Option<String>)>>,
    persisted: RefCell<Vec<String>>,
    appended: RefCell<Vec<String>>,
    /// 提交过的场景模板 id
    scenarios: RefCell<Vec<String>>,
    /// 每次场景任务提交时，工作副本里的关系（`子表.列→父表.列`）
    relations: RefCell<Vec<Vec<String>>>,
    exported: RefCell<Vec<(String, String)>>,
    scratchpads: RefCell<Vec<String>>,
    /// 出口类任务拿到的临时表名（证明任务里带的是上一次生成的结果）
    sink_temps: RefCell<Vec<String>>,
    imports: RefCell<Vec<SchemaRequest>>,
    read_only: Cell<bool>,
    tables: RefCell<Vec<String>>,
    sources: RefCell<Vec<SchemaSource>>,
    /// 任务启动后是否立即失败（校验失败 / 已在跑）
    start_error: RefCell<Option<String>>,
    /// 提交后停在「进行中」：不写完成结果（供进度 / 取消 / 异常用例观察）
    hold_job: Cell<bool>,
    /// 已完成未被取走的结果
    job_result: RefCell<Option<Result<MockJobDone, String>>>,
    /// 进行中时报告的进度快照
    job_progress: Cell<MockJobProgress>,
    /// 下一次 `job_state` 谎报 Idle（模拟工作线程异常退出）
    pretend_idle: Cell<bool>,
    /// 项目根（`None` = 未打开项目：面板应给出可读原因而不是空列表）
    project_root: RefCell<Option<std::path::PathBuf>>,
    /// 预览重查取样的请求：(临时表, 排序（列 + 是否降序）, limit)
    samples: RefCell<Vec<(String, Option<(String, bool)>, usize)>>,
    /// 内存库忙（模拟有任务在跑）：`preview_sample` 回 `Ok(None)`
    samples_busy: Cell<bool>,
    /// `preview_sample` 的失败原因（非空时回 `Err`）
    samples_error: RefCell<Option<String>>,
}

fn test_column(name: &str, generator: GeneratorConfig) -> MockColumnSpec {
    MockColumnSpec {
        id: name.len() as u64,
        def: ColumnDef {
            name: name.to_string(),
            data_type: ColumnDataType::Integer,
            generator,
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        },
        confidence: "high".to_string(),
        sample_value: "1, 2, 3...".to_string(),
    }
}

struct TestHost {
    rec: Rc<Recorder>,
}

impl TestHost {
    /// 造一份生成结果（预览两行，与旧测试的预期一致）。
    fn gen_info(&self, draft: &MockDraft) -> MockGenInfo {
        MockGenInfo {
            table_name: draft.table_name.clone(),
            temp_table_name: format!("temp_mock_{}", draft.table_name),
            columns: draft.columns.iter().map(|c| c.def.clone()).collect(),
            row_count: draft.options.rows,
            elapsed_ms: 7,
            preview: MockPreview {
                columns: draft.columns.iter().map(|c| c.def.name.clone()).collect(),
                rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            },
        }
    }

    /// 出口：新建表（同名已存在 → 报错，与真实装配层同一语义）。
    ///
    /// 目标表名取 `info.table_name`（**不是草稿**）：与 `mock_generator::persist_table_at`
    /// 同一口径——场景模板的多张表因此能各自落到自己的表名下。
    fn persist(&self, info: &MockGenInfo) -> Result<MockJobDone, String> {
        self.rec
            .persisted
            .borrow_mut()
            .push(info.table_name.clone());
        self.rec
            .sink_temps
            .borrow_mut()
            .push(info.temp_table_name.clone());
        if self
            .rec
            .tables
            .borrow()
            .iter()
            .any(|t| t == &info.table_name)
        {
            return Err(format!(
                "项目分析库已存在表 {}：请改用「追加到既有表」",
                info.table_name
            ));
        }
        Ok(MockJobDone::Persisted {
            table: info.table_name.clone(),
            rows: 5,
        })
    }
}

impl MockHost for TestHost {
    fn start_job(&self, draft: &MockDraft, kind: MockJobKind) -> Result<(), String> {
        if let Some(err) = self.rec.start_error.borrow().clone() {
            return Err(err);
        }
        self.rec.started.borrow_mut().push(kind.clone());
        let result = match &kind {
            MockJobKind::Generate => {
                self.rec.generated.borrow_mut().push((
                    draft.table_name.clone(),
                    draft.options.rows,
                    draft.options.seed,
                    None,
                ));
                Ok(MockJobDone::Generated(self.gen_info(draft)))
            }
            MockJobKind::AppendTo(table) => {
                self.rec.generated.borrow_mut().push((
                    draft.table_name.clone(),
                    draft.options.rows,
                    draft.options.seed,
                    Some(table.clone()),
                ));
                self.rec.appended.borrow_mut().push(table.clone());
                Ok(MockJobDone::Appended {
                    table: table.clone(),
                    total_rows: 100 + draft.options.rows as i64,
                })
            }
            MockJobKind::Persist(info) => self.persist(info),
            MockJobKind::PersistAll(infos) => {
                // 假宿主逐张走「新建」语义：已存在的报错，其余成功（与真实装配层同一口径）
                let mut landed = Vec::new();
                let mut failed = Vec::new();
                for info in infos {
                    self.rec
                        .persisted
                        .borrow_mut()
                        .push(info.table_name.clone());
                    self.rec
                        .sink_temps
                        .borrow_mut()
                        .push(info.temp_table_name.clone());
                    if self
                        .rec
                        .tables
                        .borrow()
                        .iter()
                        .any(|t| t == &info.table_name)
                    {
                        failed.push((
                            info.table_name.clone(),
                            format!(
                                "项目分析库已存在表 {}：请改用「追加到既有表」",
                                info.table_name
                            ),
                        ));
                    } else {
                        landed.push((info.table_name.clone(), 5));
                    }
                }
                Ok(MockJobDone::PersistedAll { landed, failed })
            }
            MockJobKind::Scenario(template) => {
                self.rec.scenarios.borrow_mut().push(template.id.clone());
                // 假宿主按**工作副本里的表**回结果（不是写死的三张）：
                // 这样「改关系 → 生成」的链路在窗口测试里也能验
                let tables = if template.tables.is_empty() {
                    SCENARIO_TABLES
                        .iter()
                        .map(|(table, rows)| ((*table).to_string(), *rows))
                        .collect::<Vec<_>>()
                } else {
                    template
                        .tables
                        .iter()
                        .map(|t| (t.name.clone(), t.row_count))
                        .collect()
                };
                let relations = template
                    .tables
                    .iter()
                    .flat_map(|t| {
                        t.columns.iter().filter_map(|c| {
                            let dep = c.dependency.as_ref()?;
                            Some(format!(
                                "{}.{}→{}.{}",
                                t.name,
                                c.name,
                                dep.ref_table.clone(),
                                dep.ref_column.clone()
                            ))
                        })
                    })
                    .collect::<Vec<_>>();
                self.rec.relations.borrow_mut().push(relations);
                Ok(MockJobDone::ScenarioGenerated {
                    template_name: template.name.clone(),
                    // 逐表回结果：够验证「切换当前表 → 出口作用于选中那张」
                    tables: tables
                        .into_iter()
                        .map(|(table, rows)| MockGenInfo {
                            table_name: table.clone(),
                            temp_table_name: format!("temp_mock_{table}"),
                            columns: draft.columns.iter().map(|c| c.def.clone()).collect(),
                            row_count: rows,
                            elapsed_ms: 3,
                            preview: MockPreview {
                                columns: vec!["id".to_string()],
                                rows: vec![vec![rows.to_string()]],
                            },
                        })
                        .collect(),
                })
            }
            MockJobKind::Export { info, format, path } => {
                self.rec
                    .sink_temps
                    .borrow_mut()
                    .push(info.temp_table_name.clone());
                self.rec
                    .exported
                    .borrow_mut()
                    .push((format!("{format:?}"), path.clone()));
                Ok(MockJobDone::Exported {
                    message: format!("已导出：{path}"),
                })
            }
            MockJobKind::Scratchpad { info, format } => {
                self.rec
                    .sink_temps
                    .borrow_mut()
                    .push(info.temp_table_name.clone());
                self.rec
                    .scratchpads
                    .borrow_mut()
                    .push(format!("{format:?}"));
                Ok(MockJobDone::Exported {
                    message: "已保存到草稿箱：/proj/mock/mock_x.csv".to_string(),
                })
            }
        };
        if !self.rec.hold_job.get() {
            *self.rec.job_result.borrow_mut() = Some(result);
        }
        Ok(())
    }

    fn job_state(&self) -> MockJobState {
        // 与真实实现同一语义：结果一旦写入，进度即清零（避免「既无进度也无结果」的空洞）
        if self.rec.pretend_idle.get() || self.rec.job_result.borrow().is_some() {
            return MockJobState::Idle;
        }
        MockJobState::Running(self.rec.job_progress.get())
    }

    fn take_job_done(&self) -> Option<Result<MockJobDone, String>> {
        self.rec.job_result.borrow_mut().take()
    }

    fn cancel_job(&self) {
        self.rec.cancels.set(self.rec.cancels.get() + 1);
        *self.rec.job_result.borrow_mut() = Some(Err("生成已取消".to_string()));
    }

    fn existing_tables(&self) -> Vec<String> {
        self.rec.tables.borrow().clone()
    }

    fn schema_sources(&self) -> Vec<SchemaSource> {
        self.rec.sources.borrow().clone()
    }

    fn import_columns(&self, request: &SchemaRequest) -> Result<Vec<MockColumnSpec>, String> {
        self.rec.imports.borrow_mut().push(request.clone());
        Ok(vec![
            test_column("id", GeneratorConfig::AutoIncrement { start: 1, step: 1 }),
            test_column("email", GeneratorConfig::SafeEmail),
        ])
    }

    /// 预览重查取样：记下请求，再按开关回「忙 / 失败 / 一份可辨认的重查结果」。
    ///
    /// 回的列只有被排的那一列（不排序时给一列探针），值里带方向与行号
    /// （`id:desc:1`）：断言时能看出「这份数据是哪次请求的产物」，也不会与生成时的取样碰巧一样。
    fn preview_sample(
        &self,
        temp_table: &str,
        order: Option<(&str, bool)>,
        limit: usize,
    ) -> Result<Option<MockPreview>, String> {
        self.rec.samples.borrow_mut().push((
            temp_table.to_string(),
            order.map(|(column, descending)| (column.to_string(), descending)),
            limit,
        ));
        if let Some(err) = self.rec.samples_error.borrow().clone() {
            return Err(err);
        }
        if self.rec.samples_busy.get() {
            return Ok(None);
        }
        let (column, dir) = match order {
            Some((column, true)) => (column.to_string(), "desc"),
            Some((column, false)) => (column.to_string(), "asc"),
            None => ("rows".to_string(), "plain"),
        };
        Ok(Some(MockPreview {
            columns: vec![column.clone()],
            rows: (1..=limit)
                .map(|n| vec![format!("{column}:{dir}:{n}")])
                .collect(),
        }))
    }

    fn export_dir(&self) -> String {
        "/proj".to_string()
    }

    fn read_only(&self) -> bool {
        self.rec.read_only.get()
    }

    fn project_root(&self) -> Option<std::path::PathBuf> {
        self.rec.project_root.borrow().clone()
    }

    fn open_detail(&self, target: DetailTarget, _window: &mut Window, _cx: &mut App) {
        self.rec.opened_detail.set(self.rec.opened_detail.get() + 1);
        self.rec.opened_targets.borrow_mut().push(target.key());
    }

    fn notify(&self, _cx: &mut App) {
        self.rec.notifies.set(self.rec.notifies.get() + 1);
    }
}

fn test_host(rec: &Rc<Recorder>) -> Rc<dyn MockHost> {
    Rc::new(TestHost { rec: rec.clone() })
}

/// 记录宿主（默认：两个连接、一张既有表）。
fn recorder() -> Rc<Recorder> {
    let rec = Rc::new(Recorder::default());
    *rec.sources.borrow_mut() = vec![
        SchemaSource {
            conn_id: "G_pg".to_string(),
            label: "生产库".to_string(),
            catalog: "shop".to_string(),
            schema: "public".to_string(),
        },
        SchemaSource {
            conn_id: "G_sqlite".to_string(),
            label: "本地库".to_string(),
            catalog: String::new(),
            schema: String::new(),
        },
    ];
    *rec.tables.borrow_mut() = vec!["orders".to_string()];
    rec
}

// ==================== 窗口级 ====================

struct Harness {
    panel: Entity<MockPanel>,
    detail: Entity<MockDetailView>,
}

impl Render for Harness {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        let mut root = div()
            .size_full()
            .child(div().w_96().child(self.panel.clone()))
            .child(self.detail.clone());
        if let Some(layer) = Root::render_dialog_layer(window, cx) {
            root = root.child(layer);
        }
        root
    }
}

/// 打开测试窗口（窗口根必须是 `Root`：`open_dialog` / `render_dialog_layer` 依赖它）。
fn open_harness(
    cx: &mut TestAppContext,
    host: Rc<dyn MockHost>,
) -> (
    Entity<MockPanel>,
    Entity<MockDetailView>,
    &mut VisualTestContext,
) {
    let slot: Rc<RefCell<Option<(Entity<MockPanel>, Entity<MockDetailView>)>>> =
        Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let panel = cx.new(|cx| MockPanel::new(host, cx));
        let detail = cx.new(|cx| MockDetailView::new(panel.clone(), DetailTarget::Draft, cx));
        *slot_in.borrow_mut() = Some((panel.clone(), detail.clone()));
        // 窗口根必须是 `Root`（Entity 才能交给它）：外包一层 Harness 实体
        let harness = cx.new(|_cx| Harness { panel, detail });
        Root::new(harness, window, cx)
    });
    let (panel, detail) = slot.borrow().clone().expect("panel 已创建");
    (panel, detail, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 手动驱动一次任务轮询（生产环境由 120ms 定时泵驱动）。
fn poll_job(cx: &mut VisualTestContext, panel: &Entity<MockPanel>) {
    panel.update(cx, |panel, cx| {
        let _ = panel.poll_job(cx);
    });
}

#[gpui_kit::test]
fn panel_renders_empty_state_and_exposes_defaults(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    draw(cx);

    // 首帧渲染不 panic；构造期不加载任何宿主数据（渲染期零 I/O）
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.draft().table_name, "mock_data");
        assert!(panel.draft().columns.is_empty());
        assert_eq!(panel.draft().options.rows, 1000);
        assert_eq!(panel.draft().options.seed, None);
        assert!(panel.sources().is_empty(), "候选只在事件路径加载");
        assert!(panel.gen_info().is_none());
    });
}

#[gpui_kit::test]
fn refresh_sources_loads_connections_and_tables(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| panel.refresh_sources(cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.sources().len(), 2);
        assert_eq!(panel.existing_tables(), ["orders".to_string()]);
    });
}

/// 连接清册变了只刷「导入结构」候选：既有表清单要开项目分析库（真 I/O），
/// 不能跟着连接保存这种高频动作一起重读（它是面板打开 / 切项目那一拍的事）。
#[gpui_kit::test]
fn refreshing_only_connections_keeps_the_existing_table_list(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| panel.refresh_sources(cx));
    // 面板已摆着：连接清册与既有表各自又变了
    rec.sources.borrow_mut().push(SchemaSource {
        conn_id: "P_new".to_string(),
        label: "新连接".to_string(),
        catalog: String::new(),
        schema: String::new(),
    });
    rec.tables.borrow_mut().push("events".to_string());

    panel.update(cx, |panel, cx| panel.refresh_connection_sources(cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.sources().len(), 3, "连接候选按新清册重读");
        assert_eq!(
            panel.existing_tables(),
            ["orders".to_string()],
            "既有表清单保持原样：它只有面板打开 / 切项目才重读"
        );
    });
}

/// 切项目（`forget_generated`）要把**当前项目的派生清单**一并清掉：
///
/// 「追加到既有表」的候选、连接清单都按项目作用域变——上一项目的表名摆在新项目下，
/// 点下去只会得到「项目分析库没有表 X」，而用户完全不知道为什么。
#[gpui_kit::test]
fn forgetting_generated_clears_project_scoped_lists(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| panel.refresh_sources(cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.existing_tables(), ["orders".to_string()]);
        assert_eq!(panel.sources().len(), 2);
    });

    panel.update(cx, |panel, cx| panel.forget_generated(2, cx));
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.existing_tables().is_empty(),
            "旧项目的既有表清单不该留在新项目下"
        );
        assert!(panel.sources().is_empty(), "连接清单随项目作用域变");
        assert!(
            panel.outcome().is_some_and(|o| o.contains("已切换项目")),
            "{:?}",
            panel.outcome()
        );
    });
}

/// 生成**不写库**（对齐 v1：生成只产临时表 + 预览）。
#[gpui_kit::test]
fn generate_produces_preview_without_touching_sinks(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    // 后台任务：提交后立即返回（UI 不阻塞），结果由轮询回填
    panel.update(cx, |panel, _cx| {
        assert!(panel.is_running(), "提交后应处于进行中");
        assert!(panel.gen_info().is_none(), "结果尚未回填");
    });
    poll_job(cx, &panel);
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert!(!panel.is_running(), "回填后应归位空闲");
        let outcome = panel.outcome().expect("应有成功文案");
        assert!(outcome.contains("已生成 1,000 行"), "{outcome}");
        assert!(outcome.contains("temp_mock_mock_data"), "{outcome}");
        assert!(panel.error().is_none());
        let info = panel.gen_info().expect("结果应就绪");
        assert_eq!(info.row_count, 1000);
        assert_eq!(info.preview.rows.len(), 2);
        assert_eq!(info.preview.columns, ["id".to_string()]);
    });

    let generated = rec.generated.borrow();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].0, "mock_data");
    assert_eq!(generated[0].1, 1000, "默认行数");
    assert_eq!(generated[0].2, None, "默认随机种子");
    assert_eq!(generated[0].3, None, "生成不带追加目标");
    assert_eq!(rec.started.borrow().len(), 1, "只应提交一个后台任务");
    // 关键语义：生成不落库、不落盘
    assert!(rec.persisted.borrow().is_empty(), "生成不应建表");
    assert!(rec.appended.borrow().is_empty(), "生成不应追加");
    assert!(rec.scratchpads.borrow().is_empty(), "生成不应写文件");
    assert!(rec.notifies.get() >= 1, "生成成功后应通知宿主");
}

/// 项目级出口的可用性判据（`sinks_ready`）= 有结果 + 非只读 + 已打开项目。
///
/// 三个条件缺一个就该禁用按钮：未打开项目时按钮永远点得动、每次都弹同一句拒绝，
/// 不如直接不可点（视图与三个出口入口共用同一判据）。
#[gpui_kit::test]
fn project_sinks_need_a_result_a_project_and_a_writable_session(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    // ① 没结果：不可用（先于项目与只读）
    panel.update(cx, |panel, _cx| {
        assert!(!panel.sinks_ready(), "没结果时没什么可落");
    });

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);

    // ② 有结果但未打开项目：仍不可用（Mock 只写项目分析库）
    panel.update(cx, |panel, _cx| {
        assert!(panel.gen_info().is_some(), "已有结果");
        assert!(!panel.sinks_ready(), "未打开项目：出口不可用");
    });

    // ③ 打开项目：可用
    *rec.project_root.borrow_mut() = Some(std::path::PathBuf::from("/proj/a"));
    panel.update(cx, |panel, _cx| assert!(panel.sinks_ready()));

    // ④ 只读：不可用（只读止于出口，生成与预览仍可用）
    rec.read_only.set(true);
    panel.update(cx, |panel, _cx| {
        assert!(!panel.sinks_ready(), "只读会话不允许写库 / 写文件");
        assert!(panel.gen_info().is_some(), "结果与预览不受只读影响");
    });
}

/// 状态栏指示的文案（`status_chip_text`）：量化任务给百分比，出口类给阶段名，没任务不给。
#[test]
fn status_chip_text_follows_the_job_phase() {
    assert!(
        super::status_chip_text(None).is_none(),
        "没任务就不画那一块"
    );

    let mut progress = MockJobProgress {
        phase: MockJobPhase::Generating,
        batches_done: 2,
        batches_total: 8,
        rows_total: 8_000,
    };
    assert_eq!(
        super::status_chip_text(Some(progress)).as_deref(),
        Some("◐ Mock 生成中 25%")
    );

    // 出口类（写入 / 导出）拿不到中间进度：只给阶段名，不给百分比（与 `render_job_row` 同口径）
    progress.phase = MockJobPhase::Writing;
    let text = super::status_chip_text(Some(progress)).expect("出口类也要给一句");
    assert!(text.starts_with("◐ Mock "), "{text}");
    assert!(!text.contains('%'), "出口类不给百分比：{text}");
}

/// 预览右键「拿得走」的两件事靠两个纯助手：整行 TSV 与「哪一格」的回退规则。
#[test]
fn preview_context_text_prefers_the_clicked_cell_and_falls_back_to_the_first_column() {
    let mut delegate = super::PreviewTableDelegate {
        panel: gpui_kit::WeakEntity::new_invalid(),
        table: "mock_data".to_string(),
        columns: vec!["id".to_string(), "name".to_string()],
        rows: vec![
            vec!["1".to_string(), "甲".to_string()],
            vec!["2".to_string(), "乙".to_string()],
        ],
        sort: None,
        widths: std::collections::HashMap::new(),
        context_cell: None,
    };

    // 没记过位置（右键落在行号槽或未记）：退到该行第一列
    assert_eq!(delegate.context_text(1).as_deref(), Some("2"));
    // 记了第 2 数据列：给那一格
    delegate.context_cell = Some((0, 2));
    assert_eq!(delegate.context_text(0).as_deref(), Some("甲"));
    assert_eq!(delegate.context_column(0).as_deref(), Some("name"));
    // 记的行与问的行不一致（位置可能已过期）：回退
    assert_eq!(delegate.context_text(1).as_deref(), Some("2"));
    // 行号槽不当数据（记成 0 列也走回退）
    delegate.context_cell = Some((0, 0));
    assert_eq!(delegate.context_text(0).as_deref(), Some("1"));
    assert_eq!(delegate.context_column(0).as_deref(), Some("id"));
    // 整行 TSV（拷进表格 / SQL 能直接分列）
    assert_eq!(delegate.row_text(0), "1\t甲");
    // 整列：取样窗口内的取值，一行一个（含行号槽与列下标的对应关系）
    assert_eq!(delegate.column_text(0), ["1", "2"]);
    assert_eq!(delegate.column_text(1), ["甲", "乙"]);
    assert!(
        delegate.column_text(9).is_empty(),
        "列下标越界给空（不 panic）"
    );
    // 换一份取样：清掉记录的位置（旧行号不再指向同一个值）
    assert!(delegate.set_preview(super::PreviewSnapshot {
        table: "mock_data".to_string(),
        columns: vec!["id".to_string()],
        rows: vec![vec!["9".to_string()]],
        sort: None,
    }));
    assert!(delegate.context_cell.is_none());
}

/// 排序是快照的一部分：只有**生效中的排序**变了也要重建表头（表头箭头就画在它上）。
///
/// 这条用例是**判别性**的：若 `set_preview` 只比行与列，点列头后箭头就会与实际数据脱钩
/// （面板拒绝重查时更明显：数据没变，只有排序要退回去）。
#[test]
fn preview_snapshot_change_includes_the_effective_sort() {
    let snapshot = |sort: Option<(String, bool)>| super::PreviewSnapshot {
        table: "mock_data".to_string(),
        columns: vec!["id".to_string(), "name".to_string()],
        rows: vec![vec!["1".to_string(), "甲".to_string()]],
        sort,
    };
    let mut delegate =
        super::PreviewTableDelegate::new(gpui_kit::WeakEntity::new_invalid(), snapshot(None));
    // 行 / 列都没变、只有排序变：仍要报「变了」（否则表头箭头不会重建）
    assert!(delegate.set_preview(snapshot(Some(("name".to_string(), true)))));
    assert_eq!(
        delegate.sort,
        Some((1, gpui_kit::component::table::ColumnSort::Descending))
    );
    // 同一份排序再来一次：没变
    assert!(!delegate.set_preview(snapshot(Some(("name".to_string(), true)))));
    // 方向翻了：变了
    assert!(delegate.set_preview(snapshot(Some(("name".to_string(), false)))));
    assert_eq!(
        delegate.sort,
        Some((1, gpui_kit::component::table::ColumnSort::Ascending))
    );
    // 列名对不上（列被换了）：当没有排序，而不是去查一个不存在的列
    assert!(delegate.set_preview(snapshot(Some(("gone".to_string(), true)))));
    assert!(delegate.sort.is_none());
    // 取消排序：从「有」回到「没有」也是变了
    assert!(delegate.set_preview(snapshot(Some(("name".to_string(), true)))));
    assert!(delegate.set_preview(snapshot(None)));
    assert!(delegate.sort.is_none());
}

/// 切项目时旧表 tab 不自动关（D35），但它们得说清为什么没结果：
/// 面板要能区分「切项目清掉的」与「这一轮没生成它」。
#[gpui_kit::test]
fn project_switch_marks_results_as_dropped_until_the_next_generation(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(panel.gen_info().is_some());
        assert!(
            !panel.results_dropped_by_project_switch(),
            "本轮生成的结果不作废"
        );
    });

    panel.update(cx, |panel, cx| panel.forget_generated(2, cx));
    panel.update(cx, |panel, _cx| {
        assert!(panel.gen_info().is_none());
        assert!(
            panel.results_dropped_by_project_switch(),
            "切项目清掉的结果要标记出来（旧 tab 的文案据此区分「另一个项目」与「这一轮」）"
        );
    });

    // 重新生成后标记清掉：新结果是当前项目的
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(panel.gen_info().is_some());
        assert!(!panel.results_dropped_by_project_switch());
    });
}

/// 预览表走组件库的 `DataTable`（不手搓网格）：表头与单元格都能从表格状态里读回来
/// （`TableState::dump`），且首列是行号槽。
///
/// 这条用例是**判别性**的：若 delegate 的列数 / 行数 / `cell_text` 任一接错，或预览表
/// 没在渲染那一拍建起来，它都会挂。
#[gpui_kit::test]
fn preview_table_dumps_the_sample_through_the_component_table(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    // 渲染一帧：预览表状态是懒创建的（`TableState::new` 需要 window）
    draw(cx);
    assert!(
        cx.debug_bounds("mock-preview-table").is_some(),
        "预览表应真的画进布局（组件库 DataTable 落在那个容器里）"
    );

    let table = cx
        .update(|_, cx| detail.read(cx).preview_table.clone())
        .expect("渲染后预览表应已创建");
    cx.update(|_, cx| {
        let state = table.read(cx);
        let (headers, rows) = state.dump(cx);
        assert_eq!(headers, ["#", "id"], "首列是行号槽，其后是数据列");
        assert_eq!(rows.len(), 2, "取样行数（测试宿主固定回两行）");
        assert_eq!(rows[0], ["1", "1"], "行号 + 值");
        assert_eq!(rows[1], ["2", "2"]);
    });
}

/// 预览的按列重排：取样换成**重查来的**排序结果；取消 / 取样被换掉后自动回到生成顺序。
///
/// 这条用例是**判别性**的：面板若就地重排取样窗口，`ordered` 里就看不到请求；
/// 若把排序当成与结果无关的独立状态，取样换掉后它还会继续生效。
#[gpui_kit::test]
fn preview_sort_requeries_and_falls_back_when_the_sample_changes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);

    // 草稿默认表名（假宿主按草稿名回临时表名 `temp_mock_{表}`）
    let table = panel.read_with(cx, |panel, _cx| panel.draft().table_name.clone());
    panel.update(cx, |panel, _cx| {
        let (preview, sort) = panel.preview_for(&table).expect("刚生成过，有结果");
        assert!(sort.is_none(), "没点过列头就没有排序");
        assert_eq!(preview.rows.len(), 2, "假宿主固定回两行");
    });

    // 点列头（降序）：面板把「临时表 + 列 + 方向 + 取几行」交给宿主重查，取样换成重查来的
    panel.update(cx, |panel, cx| {
        panel.sort_preview(&table, "id", Some(true), cx)
    });
    assert_eq!(
        rec.samples.borrow().as_slice(),
        [(
            format!("temp_mock_{table}"),
            Some(("id".to_string(), true)),
            super::PREVIEW_ROWS
        )],
        "重查的参数：临时表 + （列，方向）+ 取几行"
    );
    panel.update(cx, |panel, _cx| {
        let (preview, sort) = panel.preview_for(&table).expect("有结果");
        let sort = sort.expect("排序应已生效");
        assert_eq!(sort.column, "id");
        assert!(sort.descending);
        assert_eq!(sort.label(), "按 id 降序");
        assert_eq!(
            preview.rows.len(),
            super::PREVIEW_ROWS,
            "重查结果整份交过来"
        );
        assert_eq!(preview.rows[0], vec!["id:desc:1".to_string()]);
        assert!(panel.error().is_none(), "成功时不该留错误文案");
    });

    // 取消排序（表头点第三下 / 菜单里的「取消排序」）：回到生成时的取样，且不再重查
    panel.update(cx, |panel, cx| panel.sort_preview(&table, "id", None, cx));
    assert_eq!(rec.samples.borrow().len(), 1, "取消是纯状态，不该再查一次");
    panel.update(cx, |panel, _cx| {
        let (preview, sort) = panel.preview_for(&table).expect("有结果");
        assert!(sort.is_none());
        assert_eq!(preview.rows[0], vec!["1".to_string()], "回到生成顺序");
    });

    // 内存库忙（有任务在跑）：只写一句可读原因，取样保持原样
    rec.samples_busy.set(true);
    panel.update(cx, |panel, cx| {
        panel.sort_preview(&table, "id", Some(false), cx)
    });
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.preview_for(&table).expect("有结果").1.is_none(),
            "忙的时候不该装作排好了"
        );
        assert!(
            panel.error().is_some_and(|e| e.contains("内存库")),
            "要给一句能看懂的原因：{:?}",
            panel.error()
        );
        assert_eq!(panel.preview_for(&table).expect("有结果").0.rows.len(), 2);
    });
    rec.samples_busy.set(false);

    // 重查失败（列不存在等）：同样不动取样，原因原样摆出来
    *rec.samples_error.borrow_mut() = Some("按「id」排序取样失败：no such column".to_string());
    panel.update(cx, |panel, cx| {
        panel.sort_preview(&table, "id", Some(false), cx)
    });
    panel.update(cx, |panel, _cx| {
        assert!(panel.preview_for(&table).expect("有结果").1.is_none());
        assert!(panel.error().is_some_and(|e| e.contains("排序取样失败")));
    });
    rec.samples_error.borrow_mut().take();

    // 取样被换掉（相当于重新生成出不同的数据）：排序自动失效，不需要谁记得来清
    panel.update(cx, |panel, cx| {
        panel.sort_preview(&table, "id", Some(true), cx)
    });
    panel.update(cx, |panel, _cx| {
        assert!(panel.preview_for(&table).expect("有结果").1.is_some());
        panel.results[0].preview.rows = vec![vec!["9".to_string()]];
        let (preview, sort) = panel.preview_for(&table).expect("有结果");
        assert!(
            sort.is_none(),
            "取样换了，旧排序不能再生效（那是上一份数据的排序）"
        );
        assert_eq!(
            preview.rows,
            vec![vec!["9".to_string()]],
            "回到当前取样的原样"
        );
    });
    // 切项目（结果全没）：排序一并作废
    panel.update(cx, |panel, _cx| {
        panel.results[0].preview = MockPreview::default();
        panel.preview_cache = None;
    });
    panel.update(cx, |panel, cx| panel.forget_generated(1, cx));
    panel.update(cx, |panel, _cx| {
        assert!(panel.preview_cache.is_none());
        assert!(panel.preview_for(&table).is_none(), "结果没了就没有预览");
    });
}

/// 预览行数档：生成时只读了 10 行，要更多行就得**重查**临时表；回到 10 行则丢掉缓存即可。
///
/// 这条用例是**判别性**的：它同时看「请求里带的 limit」与「面板交出去的取样长度」——
/// 若只改计数不重查，取样永远是 10 行，菜单与标题就会说谎；它也看住「排序与行数共用一处缓存」。
#[gpui_kit::test]
fn preview_limit_requeries_more_rows_and_shares_the_cache_with_sorting(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);

    let table = panel.read_with(cx, |panel, _cx| panel.draft().table_name.clone());
    panel.update(cx, |panel, _cx| {
        assert_eq!(
            panel.preview_limit(),
            super::PREVIEW_ROWS,
            "默认就是生成那份"
        );
        assert_eq!(panel.preview_for(&table).expect("有结果").0.rows.len(), 2);
    });

    // 选 25 行：一次不带排序的重查，取样换成 25 行
    panel.update(cx, |panel, cx| panel.set_preview_limit(25, cx));
    assert_eq!(
        rec.samples.borrow().as_slice(),
        [(format!("temp_mock_{table}"), None, 25)],
        "「多要几行」也是重查（生成时只读了 10 行）"
    );
    panel.update(cx, |panel, _cx| {
        let (preview, sort) = panel.preview_for(&table).expect("有结果");
        assert!(sort.is_none(), "只是多要几行，不是排序");
        assert_eq!(preview.rows.len(), 25);
        assert_eq!(preview.rows[0], vec!["rows:plain:1".to_string()]);
    });

    // 再排序：带着**当前行数档**重查（排序与行数共用一处缓存）
    panel.update(cx, |panel, cx| {
        panel.sort_preview(&table, "id", Some(true), cx)
    });
    assert_eq!(rec.samples.borrow().len(), 2, "排序要再查一次");
    assert_eq!(
        rec.samples.borrow()[1],
        (
            format!("temp_mock_{table}"),
            Some(("id".to_string(), true)),
            25
        )
    );
    panel.update(cx, |panel, _cx| {
        let (preview, sort) = panel.preview_for(&table).expect("有结果");
        assert!(sort.is_some());
        assert_eq!(preview.rows.len(), 25, "排序后仍是 25 行");
        assert_eq!(preview.rows[0], vec!["id:desc:1".to_string()]);
    });

    // 行数改到 50：排序留着，重查带上排序与新的 limit
    panel.update(cx, |panel, cx| panel.set_preview_limit(50, cx));
    assert_eq!(rec.samples.borrow().len(), 3, "改行数要重查");
    assert_eq!(
        rec.samples.borrow()[2],
        (
            format!("temp_mock_{table}"),
            Some(("id".to_string(), true)),
            50
        )
    );
    panel.update(cx, |panel, _cx| {
        let (preview, sort) = panel.preview_for(&table).expect("有结果");
        assert!(sort.is_some(), "改行数不该把排序弄丢");
        assert_eq!(preview.rows.len(), 50);
    });

    // 内存库忙：档位（用户的选择）留着，但缓存因 limit 对不上而失效 → 回落生成那份取样
    rec.samples_busy.set(true);
    panel.update(cx, |panel, cx| panel.set_preview_limit(100, cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.preview_limit(), 100);
        assert!(panel.error().is_some_and(|e| e.contains("内存库")));
        assert_eq!(
            panel.preview_for(&table).expect("有结果").0.rows.len(),
            2,
            "缓存失效就回落生成那份（不留着旧的 50 行假装还在）"
        );
    });
    rec.samples_busy.set(false);

    // 回到 10 行：丢缓存即可，不必再查库
    let before = rec.samples.borrow().len();
    panel.update(cx, |panel, cx| {
        panel.set_preview_limit(super::PREVIEW_ROWS, cx)
    });
    assert_eq!(rec.samples.borrow().len(), before, "回到生成那份不必重查");
    panel.update(cx, |panel, _cx| {
        assert!(panel.preview_cache.is_none());
        assert_eq!(panel.preview_for(&table).expect("有结果").0.rows.len(), 2);
    });
}

/// 表头点一下 = 让面板重查（delegate 只转发），行号槽不参与排序；
/// 面板算完会把生效中的排序回推，表头箭头与表格内容跟着一起变。
///
/// 这条用例是**判别性**的：它从组件库的 `TableState` 里真的调 `perform_sort`——
/// delegate 的转发 / `column` 的排序标记 / 快照回推任一接错都会挂。
#[gpui_kit::test]
fn preview_header_click_requeries_through_the_panel(cx: &mut TestAppContext) {
    use gpui_kit::component::table::{ColumnSort, TableDelegate as _};

    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    // 渲染一帧：预览表是懒创建的（`TableState::new` 需要 window）
    draw(cx);
    let table = cx
        .update(|_, cx| detail.read(cx).preview_table.clone())
        .expect("渲染后预览表应已创建");

    // 行号槽（第 0 列）：`Column` 没开 sortable，delegate 也不该发请求
    cx.update(|window, cx| {
        table.update(cx, |state, cx| {
            state
                .delegate_mut()
                .perform_sort(0, ColumnSort::Descending, window, cx);
        });
    });
    assert!(rec.samples.borrow().is_empty(), "行号槽不排序");

    // 第 1 列（id）：转成一次面板重查
    cx.update(|window, cx| {
        table.update(cx, |state, cx| {
            state
                .delegate_mut()
                .perform_sort(1, ColumnSort::Descending, window, cx);
        });
    });
    assert_eq!(rec.samples.borrow().len(), 1, "数据列点一次 = 一次重查");
    assert_eq!(
        rec.samples.borrow()[0].1.as_ref().map(|(c, _)| c.as_str()),
        Some("id")
    );

    // 面板算完经观察者把快照推回来：箭头画在生效中的那一列，内容是重查来的那份
    cx.update(|_, cx| {
        let state = table.read(cx);
        assert_eq!(
            state.delegate().sort,
            Some((0, ColumnSort::Descending)),
            "生效中的排序列要让表头画出来"
        );
        let (headers, rows) = state.dump(cx);
        assert_eq!(headers, ["#", "id"]);
        assert_eq!(rows[0], ["1", "id:desc:1"], "显示的是重查来的取样");
        assert_eq!(rows[1], ["2", "id:desc:2"]);
    });
}

/// 字段区列搜索的匹配规则：列名 / 生成器名 / 类型名 / 置信度任一命中（子串、大小写不敏感），
/// 多词是 AND（与「搜索生成器」同一约定）。
#[test]
fn column_filter_matches_name_generator_type_or_confidence() {
    // 空词 = 不筛（去掉两边空白也算空）
    assert!(super::column_matches(
        "", "email", "邮箱", "VARCHAR", "high"
    ));
    assert!(super::column_matches(
        "  ", "email", "邮箱", "VARCHAR", "high"
    ));
    // 列名子串；大小写不敏感
    assert!(super::column_matches(
        "mai", "email", "邮箱", "VARCHAR", "high"
    ));
    assert!(super::column_matches(
        "EMAIL", "email", "邮箱", "VARCHAR", "high"
    ));
    // 生成器名也能搜（人常常记得「那列是邮箱」而不记得列名）
    assert!(super::column_matches(
        "邮箱", "contact", "邮箱", "VARCHAR", "high"
    ));
    // 类型名：卡片上那个 DuckDB 类型（找「所有时间列」）
    assert!(super::column_matches(
        "timestamp",
        "created_at",
        "日期时间",
        "TIMESTAMP",
        "high"
    ));
    // 置信度：卡片上就写着 high / low / manual（复核「按类型猜的」搜 low）
    assert!(super::column_matches(
        "low", "amount", "金额", "DECIMAL", "low"
    ));
    assert!(super::column_matches(
        "manual", "amount", "金额", "DECIMAL", "manual"
    ));
    // 多词 = AND（每个词各要命中一个靶子）
    assert!(super::column_matches(
        "mail varchar",
        "email",
        "邮箱",
        "VARCHAR",
        "high"
    ));
    assert!(!super::column_matches(
        "mail integer",
        "email",
        "邮箱",
        "VARCHAR",
        "high"
    ));
    // 都不命中就筛掉
    assert!(!super::column_matches(
        "phone", "email", "邮箱", "VARCHAR", "high"
    ));
}

/// 字段区筛选：搜索词一变，要画的列就跟着变（空结果交给一句可读空态）。
///
/// 这条用例是**判别性**的：它读的就是渲染那份 `visible_columns`，
/// 因此筛选逻辑与画出来的列表不可能是两份实现。
#[gpui_kit::test]
fn column_filter_narrows_the_field_list(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
        panel.add_column(
            "email".to_string(),
            ColumnDataType::Varchar { length: None },
            cx,
        );
        panel.add_column(
            "nickname".to_string(),
            ColumnDataType::Varchar { length: None },
            cx,
        );
    });
    // 渲染一帧：筛选框是懒创建的（`InputState` 要 window）
    draw(cx);

    let visible = |cx: &mut VisualTestContext| -> Vec<String> {
        cx.update(|_, cx| {
            let panel_ref = panel.read(cx);
            detail
                .read(cx)
                .visible_columns(panel_ref, cx)
                .iter()
                .map(|column| column.def.name.clone())
                .collect()
        })
    };
    assert_eq!(visible(cx), ["id", "email", "nickname"], "空词看不筛");

    let filter = panel
        .read_with(cx, |panel, _cx| panel.column_filter_input())
        .expect("渲染一帧后筛选框应已创建");
    let set = |cx: &mut VisualTestContext, text: &str| {
        cx.update(|window, cx| filter.update(cx, |input, cx| input.set_value(text, window, cx)));
    };

    set(cx, "mail");
    draw(cx);
    assert_eq!(visible(cx), ["email"], "按列名子串筛");

    set(cx, "EMAIL");
    assert_eq!(visible(cx), ["email"], "大小写不敏感");

    set(cx, "INTEGER");
    draw(cx);
    assert_eq!(visible(cx), ["id"], "按类型名筛（卡片上那个 DuckDB 类型）");

    set(cx, "mail VARCHAR");
    assert_eq!(
        visible(cx),
        ["email"],
        "多词是 AND：一个词命中列名、一个命中类型"
    );

    set(cx, "mail INTEGER");
    assert!(visible(cx).is_empty(), "两个词要各命中一个靶子");

    // 筛没了：不报错、不留空白（渲染一帧验证空态那条分支）
    set(cx, "zzz-no-such-column");
    draw(cx);
    assert!(visible(cx).is_empty());

    set(cx, "");
    draw(cx);
    assert_eq!(visible(cx).len(), 3, "清空后回到全部");
}

/// 表头的排序入口**真点击**：箭头在表头右端，点它才会重查（不是点表头正文）。
///
/// 组件 0.6.1 的分工：`on_col_head_click` 只做**列选择**（我们关掉了），排序挂在
/// `render_sort_icon` 上——所以「点列头就排序」是句错话，箭头才是按钮。
/// 箭头几何是组件的内部事（内距 / 尺寸都可能变），用例从表头右端往左探几次，
/// 只要有一次落在箭头上就算命中；行号槽那一侧探多少次都不该有反应。
///
/// 表头自己挂了 id 与悬停全文（`render_th`），本用例同时看住「点击还在」。
#[gpui_kit::test]
fn preview_header_sort_icon_is_really_clickable(cx: &mut TestAppContext) {
    const PROBE: [f32; 7] = [2., 6., 10., 14., 18., 22., 26.];

    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    draw(cx);

    // `debug_selector` 只是给用例找坐标（`.id(...)` 不登记坐标）
    for slot in ["mock-preview-th-0", "mock-preview-th-1"] {
        assert!(
            cx.debug_bounds(slot).is_some(),
            "{slot} 应登记坐标（行号槽也走同一个 render_th）"
        );
    }
    assert!(rec.samples.borrow().is_empty(), "还没点过就不该重查");

    // 行号槽：右侧没有箭头，点多少次都不该有反应
    let slot = cx.debug_bounds("mock-preview-th-0").expect("行号槽坐标");
    for offset in PROBE {
        cx.simulate_click(
            gpui_kit::Point::new(slot.right() + gpui_kit::px(offset), slot.center().y),
            gpui_kit::Modifiers::default(),
        );
    }
    assert_eq!(rec.samples.borrow().len(), 0, "行号槽不给排序");

    // 数据列：右端往左探，命中箭头即重查
    let head = cx.debug_bounds("mock-preview-th-1").expect("数据列坐标");
    let mut clicked = false;
    for offset in PROBE {
        cx.simulate_click(
            gpui_kit::Point::new(head.right() + gpui_kit::px(offset), head.center().y),
            gpui_kit::Modifiers::default(),
        );
        if !rec.samples.borrow().is_empty() {
            clicked = true;
            break;
        }
    }
    assert!(clicked, "表头右端应有可点的排序箭头（探了 {PROBE:?}）");
    assert_eq!(
        rec.samples.borrow()[0].1.as_ref().map(|(c, _)| c.as_str()),
        Some("id")
    );
}

/// 列宽要活过表头重建：组件拖完发一次 `ColumnWidthsChanged`，delegate 记住它，
/// 之后重查 / 改行数 / 重新生成重建表头时按列名恢复（否则用户拖好的宽度被打回默认档）。
///
/// 这条用例是**判别性**的：它拿真实的 `TableState` 发那个事件（而不是直调 `set_widths`），
/// 订阅 / 延迟投递这条链断在哪一环都会挂。
#[gpui_kit::test]
fn preview_column_widths_survive_a_rebuild(cx: &mut TestAppContext) {
    use gpui_kit::component::table::{TableDelegate as _, TableEvent};

    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));
    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    draw(cx);
    let table = cx
        .update(|_, cx| detail.read(cx).preview_table.clone())
        .expect("渲染后预览表应已创建");

    // 没拖过：走组件默认档（100px）
    cx.update(|_, cx| {
        assert_eq!(
            table.read(cx).delegate().column(1, cx).width,
            gpui_kit::px(100.)
        );
    });

    // 拖了一下：组件在 mouse-up 时报全部列宽（第 0 位是行号槽）
    let widened = gpui_kit::px(240.);
    cx.update(|_, cx| {
        table.update(cx, |_state, cx| {
            cx.emit(TableEvent::ColumnWidthsChanged(vec![
                gpui_kit::px(48.),
                widened,
            ]));
        });
    });
    cx.run_until_parked();

    cx.update(|_, cx| {
        let state = table.read(cx);
        assert_eq!(
            state.delegate().column(1, cx).width,
            widened,
            "宽度应已记下来"
        );
        assert_eq!(
            state.delegate().column(0, cx).width,
            super::ui::PREVIEW_ROW_NUMBER_WIDTH,
            "行号槽的宽度是钉死的，不跟事件跑"
        );
    });

    // 重建表头（重查 / 重新生成都走这一步）：`column()` 给的是记下的宽度
    cx.update(|_, cx| {
        table.update(cx, |state, cx| state.refresh(cx));
    });
    cx.update(|_, cx| {
        assert_eq!(
            table.read(cx).delegate().column(1, cx).width,
            widened,
            "重建之后还是用户拖的宽度（否则箭头 / 行数一变就回默认档）"
        );
    });

    // 列集合换了（下一次生成给了别的列）：与它们对不上的记录宽度要剔掉
    panel.update(cx, |panel, cx| {
        panel.results[0].preview.columns = vec!["amount".to_string()];
        cx.notify();
    });
    draw(cx);
    cx.update(|_, cx| {
        let state = table.read(cx);
        assert!(
            state.delegate().widths.is_empty(),
            "列换过之后，旧列名的宽度不应该留在表里（同名回归时会意外生效）"
        );
        assert_eq!(
            state.delegate().column(1, cx).width,
            gpui_kit::px(100.),
            "新列回到默认档"
        );
    });
}

/// `Ctrl+Enter` 真按键（`key_context("mock-detail")`，键位在生产由 `crates/app` 注册）：
/// 草稿 tab 上提交生成任务；结果表 tab 上什么都不做（D38：它是产物，要改回草稿改完再生成）。
///
/// 这条用例是**判别性**的：它自己注册生产那份键位，`track_focus` / `key_context` /
/// `on_action` 任一断，它都会挂——「注册了才宣传」的快捷键必须有这层保障。
#[gpui_kit::test]
fn generate_key_submits_on_the_draft_tab_only(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;
    use gpui_kit::KeyBinding;

    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));
    cx.update(|_, cx| {
        cx.bind_keys([KeyBinding::new(
            "ctrl-enter",
            GenerateMock,
            Some("mock-detail"),
        )]);
    });
    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    // 焦点必须在 tab 内：`track_focus` 后它才在 dispatch path 上（编辑器那边踩过这个坑）
    cx.update(|window, cx| {
        let handle = detail.read(cx).focus_handle(cx);
        handle.focus(window, cx);
    });

    cx.simulate_keystrokes("ctrl-enter");
    assert_eq!(
        rec.started.borrow().len(),
        1,
        "草稿 tab 上 Ctrl+Enter 应提交生成"
    );
    assert!(matches!(
        rec.started.borrow().first(),
        Some(MockJobKind::Generate)
    ));
    poll_job(cx, &panel);

    // 结果表 tab 上同一动作：不动（它没有「生成」这个动作）
    let table = cx.new(|cx| {
        MockDetailView::new(panel.clone(), DetailTarget::Table("orders".to_string()), cx)
    });
    let before = rec.started.borrow().len();
    cx.update(|window, cx| {
        table.update(cx, |view, cx| {
            view.on_generate_key(&GenerateMock, window, cx)
        });
    });
    assert_eq!(
        rec.started.borrow().len(),
        before,
        "结果表 tab 上没有「生成」这个动作（D38）"
    );
}

#[gpui_kit::test]
fn generate_without_columns_reports_readable_error(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));

    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("请先添加列：导入源库结构，或手工加列"));
        assert!(panel.outcome().is_none());
    });
    assert!(rec.generated.borrow().is_empty(), "无列不应触生成");
}

#[gpui_kit::test]
fn generate_rejects_invalid_rows_input(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    // 行数非法：应止于视图，不触宿主
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_rows_input("0", window, cx);
            panel.run_generate(cx);
        });
    });
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("行数需为正整数"));
        assert!(panel.outcome().is_none());
    });
    assert!(rec.generated.borrow().is_empty());
}

#[gpui_kit::test]
fn persist_creates_table_then_reports_existing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);

    // 未生成就落库 → 拒绝
    panel.update(cx, |panel, cx| panel.persist_table(cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("请先生成（预览确认后再落库）"));
    });
    assert!(rec.persisted.borrow().is_empty());

    // 生成后落库 → 新建表（任务制：等结果回填）
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, cx| panel.persist_table(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(
            panel
                .outcome()
                .is_some_and(|o| o.contains("已在项目分析库新建表 mock_data")),
            "{:?}",
            panel.outcome()
        );
        assert_eq!(panel.landed(), Some("mock_data"));
    });

    // 同名已存在 → 报错并引导「追加」
    *rec.tables.borrow_mut() = vec!["mock_data".to_string()];
    panel.update(cx, |panel, cx| panel.persist_table(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.error().is_some_and(|e| e.contains("已存在")),
            "{:?}",
            panel.error()
        );
        assert_eq!(panel.existing_tables(), ["mock_data".to_string()]);
    });
}

#[gpui_kit::test]
fn append_regenerates_with_target_and_reports_totals(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
        panel.append_table("orders".to_string(), cx);
    });
    // 追加是「生成 + 写入」一次任务：提交后同样不阻塞 UI
    panel.update(cx, |panel, _cx| {
        assert!(panel.is_running(), "追加也走后台任务");
    });
    poll_job(cx, &panel);

    panel.update(cx, |panel, _cx| {
        assert!(
            panel
                .outcome()
                .is_some_and(|o| o.contains("已追加到 orders（表内共 1,100 行）")),
            "{:?}",
            panel.outcome()
        );
        assert_eq!(panel.landed(), Some("orders"));
    });
    let generated = rec.generated.borrow();
    assert_eq!(generated.len(), 1);
    assert_eq!(
        generated[0].3.as_deref(),
        Some("orders"),
        "追加必须先按目标表重算自增起点"
    );
    assert_eq!(rec.appended.borrow().as_slice(), ["orders".to_string()]);
}

#[gpui_kit::test]
fn read_only_blocks_sinks_but_allows_generate(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    rec.read_only.set(true);
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(panel.gen_info().is_some(), "只读项目仍可生成预览");
    });

    panel.update(cx, |panel, cx| panel.persist_table(cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("只读模式：不允许写入项目分析库"));
    });
    panel.update(cx, |panel, cx| panel.append_table("orders".to_string(), cx));
    panel.update(cx, |panel, cx| {
        panel.save_scratchpad(&MockExportFormat::Csv, cx)
    });
    panel.update(cx, |panel, cx| {
        panel.export_file(&MockExportFormat::Csv, "/tmp/x.csv".to_string(), cx)
    });

    assert!(rec.persisted.borrow().is_empty(), "只读不应建表");
    assert!(rec.appended.borrow().is_empty(), "只读不应追加");
    assert!(rec.scratchpads.borrow().is_empty(), "只读不应写草稿箱");
    assert!(rec.exported.borrow().is_empty(), "只读不应写文件");
    assert_eq!(rec.started.borrow().len(), 1, "只读只应提交生成任务");
}

#[gpui_kit::test]
fn column_edits_track_draft_and_invalidate_result(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    draw(cx);
    panel.update(cx, |panel, cx| {
        panel.add_column("c1".to_string(), ColumnDataType::Integer, cx);
        panel.add_column(
            "c2".to_string(),
            ColumnDataType::Varchar { length: None },
            cx,
        );
    });
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.draft().columns.len(), 2);
        assert!(matches!(
            panel.draft().columns[1].def.generator,
            GeneratorConfig::Sentence { .. }
        ));
        assert_eq!(panel.draft().columns[1].confidence, "manual");
    });

    // 生成 → 改列 → 结果失效（不让出口拿旧结果落库）
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    let id = panel.read_with(cx, |panel, _cx| panel.draft().columns[0].id);
    panel.update(cx, |panel, cx| {
        panel.set_generator(id, "uuid_v4", cx);
    });
    panel.update(cx, |panel, _cx| {
        assert!(panel.gen_info().is_none(), "改列后旧结果应失效");
        assert!(matches!(
            panel.draft().columns[0].def.generator,
            GeneratorConfig::UuidV4
        ));
        assert_eq!(panel.draft().columns[0].confidence, "manual");
    });

    // 删除
    panel.update(cx, |panel, cx| panel.remove_column(id, cx));
    panel.update(cx, |panel, _cx| assert_eq!(panel.draft().columns.len(), 1));
}

#[gpui_kit::test]
fn reset_column_mapping_restores_inferred_generator(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    draw(cx);
    panel.update(cx, |panel, cx| {
        panel.add_column(
            "email".to_string(),
            ColumnDataType::Varchar { length: None },
            cx,
        );
    });
    let id = panel.read_with(cx, |panel, _cx| panel.draft().columns[0].id);
    panel.update(cx, |panel, cx| panel.set_generator(id, "uuid_v4", cx));
    panel.update(cx, |panel, cx| panel.reset_column_mapping(id, cx));

    panel.update(cx, |panel, _cx| {
        let column = &panel.draft().columns[0];
        assert!(matches!(column.def.generator, GeneratorConfig::SafeEmail));
        assert_eq!(column.confidence, "high");
        assert!(!column.sample_value.is_empty());
    });
}

/// 导航右键定向：按源库表导入结构并预填目标表名（v1 主路径）。
#[gpui_kit::test]
fn preset_from_source_imports_columns_and_table_name(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.preset_from_source(
            SchemaRequest {
                conn_id: "G_pg".to_string(),
                catalog: "shop".to_string(),
                schema: "public".to_string(),
                table: "customers".to_string(),
            },
            cx,
        );
    });
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.draft().table_name, "customers");
        assert_eq!(panel.draft().columns.len(), 2);
        assert!(
            panel
                .outcome()
                .is_some_and(|o| o.contains("已从 shop.public.customers 导入 2 列")),
            "{:?}",
            panel.outcome()
        );
        assert!(panel.error().is_none());
    });
    assert_eq!(rec.imports.borrow().len(), 1);
}

#[gpui_kit::test]
fn open_detail_calls_host(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open_detail(window, cx);
        });
    });
    assert_eq!(rec.opened_detail.get(), 1);
    assert_eq!(
        rec.opened_targets.borrow().as_slice(),
        ["draft".to_string()]
    );
}

/// 「一表一 tab」：**结果表一个表一个 tab**，视图身份是表名（不是下标）。
///
/// 为何锁：`results` 每次生成都会重建，用下标做身份会让 tab 指向别的表；
/// 同时右 Dock 的结果表清单是这些 tab 的**管理入口**（点一行就打开 / 切过去）。
#[gpui_kit::test]
fn each_result_table_gets_its_own_detail_tab(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    // 场景生成 → 三张结果表
    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);
    draw(cx);

    // 草稿与结果表是两种身份（键也不同）：同一个键只会有一个 tab
    assert_eq!(DetailTarget::Draft.key(), "draft");
    assert_eq!(DetailTarget::Table("items".into()).key(), "table:items");
    assert_ne!(
        DetailTarget::Draft.key(),
        DetailTarget::Table("items".into()).key()
    );

    // **生成本身不开 tab**（不淹用户）：清单里点一行才开
    assert!(
        rec.opened_targets.borrow().is_empty(),
        "{:?}",
        rec.opened_targets.borrow()
    );
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open_table_detail("items", window, cx);
        });
    });
    assert_eq!(
        rec.opened_targets.borrow().as_slice(),
        ["table:items".to_string()]
    );
}

/// 切 tab 就是切表：详情 tab 被激活时把那张表设为「当前表」（出口作用于它）。
///
/// 为何锁：中央 tab 与右 Dock 是同一状态（面板那行会打 ●「当前表」）——
/// 用户在中央切 tab，出口目标跟着走，不必回面板再选一次。
#[gpui_kit::test]
fn activating_a_table_tab_makes_it_current(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);

    panel.update(cx, |panel, cx| {
        assert_eq!(panel.current_result(), 0, "场景生成后停在第一张（orders）");
        panel.focus_table("items", cx);
        assert_eq!(panel.current_result(), 1, "切 tab 把当前表改成 items");
        assert_eq!(panel.current_detail_target().label(), "items");
        // 不在本轮结果里的表名不动状态（tab 可能是上一轮留下的）
        panel.focus_table("gone", cx);
        assert_eq!(panel.current_result(), 1);
    });
}

/// tab 标题：结果表写 `表名（N 行）`（行数实时取自结果），草稿写目标表名。
#[gpui_kit::test]
fn tab_titles_name_the_table_and_its_rows(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);

    let source = panel.clone();
    let (draft_view, orders_view, items_view) = cx.update(|_window, cx| {
        (
            cx.new(|cx| MockDetailView::new(source.clone(), DetailTarget::Draft, cx)),
            cx.new(|cx| {
                MockDetailView::new(source.clone(), DetailTarget::Table("orders".into()), cx)
            }),
            cx.new(|cx| {
                MockDetailView::new(source.clone(), DetailTarget::Table("items".into()), cx)
            }),
        )
    });

    // 读另一个实体（详情）**不能**再包在 `panel.update` 里：`tab_label` 会回读面板，
    // 而面板正被 update 持有 → gpui 会以「already being updated」panic。
    let draft_label = draft_view.read_with(cx, |view, cx| view.tab_label(cx));
    let orders_label = orders_view.read_with(cx, |view, cx| view.tab_label(cx));
    let items_label = items_view.read_with(cx, |view, cx| view.tab_label(cx));
    assert_eq!(draft_label, "Mock · mock_data");
    assert_eq!(orders_label, "Mock · orders（100 行）");
    assert_eq!(items_label, "Mock · items（250 行）");
}

/// 每张结果表 tab 认的是**自己那张表**（与面板的「当前表」无关）。
///
/// 为何锁：以前只有一个详情 tab，预览跟着「当前表」跑；现在两张 tab 并排看，
/// 必须各自认自己的表（标题与预览都走同一个「按表名查结果」的入口）。
#[gpui_kit::test]
fn a_table_tab_carries_its_own_table(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);

    // 面板停在 orders，但 items 的 tab 依旧认 items
    let source = panel.clone();
    let items_view = panel.update(cx, |_panel, cx| {
        cx.new(|cx| MockDetailView::new(source.clone(), DetailTarget::Table("items".into()), cx))
    });
    let (current, items_rows, items_columns) = panel.read_with(cx, |panel, _cx| {
        let items = panel
            .results()
            .iter()
            .find(|info| info.table_name == "items")
            .expect("items 在结果里");
        (
            panel.current_result(),
            items.row_count,
            items.preview.columns.clone(),
        )
    });
    assert_eq!(current, 0, "面板停在 orders");
    assert_eq!(items_rows, 250);
    assert_eq!(items_columns, vec!["id"]);
    // items 的 tab 认 items（标题与预览走同一个「按表名查结果」的入口）
    assert_eq!(
        items_view.read_with(cx, |view, _cx| view.target().label()),
        "items"
    );
    assert_eq!(
        items_view.read_with(cx, |view, cx| view.tab_label(cx)),
        "Mock · items（250 行）"
    );
}

/// 真实 Dock 里切 tab 会触发 `Panel::set_active` → 当前表跟着换。
///
/// 为何要这条：上一条用例直接调 `focus_table`（接口级），而**生产路径**是 Dock 切 tab
/// 回调 `set_active`。不验这一跳，「切 tab 就是切表」可能只是个没接上的钩子。
#[gpui_kit::test]
fn switching_tabs_in_a_real_dock_updates_the_current_table(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);

    let source = panel.clone();
    // `area` 要一直持有：DockArea 是 TabGroup 的宿主，丢掉它就等于丢掉 tab 组。
    let (area, group) = cx.update(|window, cx| {
        let area = cx.new(|cx| DockArea::new("mock-tabs-area", None, window, cx));
        let orders = cx.new(|cx| {
            MockDetailView::new(source.clone(), DetailTarget::Table("orders".into()), cx)
        });
        let items = cx
            .new(|cx| MockDetailView::new(source.clone(), DetailTarget::Table("items".into()), cx));
        area.update(cx, |area, cx| {
            area.add_panel(orders, DockPlacement::Center, None, window, cx);
            area.add_panel(items.clone(), DockPlacement::Center, None, window, cx);
        });
        let group = items
            .read(cx)
            .group
            .clone()
            .expect("进 Dock 后应注入 group")
            .upgrade()
            .expect("tab 组应存活");
        (area, group)
    });

    // 先切到 orders（第 0 张），再切回 items：当前表应跟着切 tab 走。
    // 注：`Panel::set_active` 是**排程**投递的（`spawn_in` + `reconcile_active`），
    // 所以要 `run_until_parked` 把回调推到位（与编辑器侧的对话框回调同一口径）。
    cx.update(|window, cx| group.update(cx, |group, cx| group.select_tab(0, window, cx)));
    cx.run_until_parked();
    assert_eq!(
        panel.read_with(cx, |panel, _cx| panel.current_detail_target().label()),
        "orders"
    );

    cx.update(|window, cx| group.update(cx, |group, cx| group.select_tab(1, window, cx)));
    cx.run_until_parked();
    assert_eq!(
        panel.read_with(cx, |panel, _cx| panel.current_detail_target().label()),
        "items",
        "在真实 Dock 里切 tab 也要把当前表换成 items"
    );

    drop(area);
}

/// 取消要**持续压**：引擎在生成开始会清一次取消标志（`reset_cancel`），
/// 提交后头几百毫秒点的取消会被那一下清掉——所以轮询周期里要重发（`cancel` 幂等）。
#[gpui_kit::test]
fn cancel_is_reasserted_on_every_poll_while_running(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    rec.hold_job.set(true);
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    panel.update(cx, |panel, cx| panel.cancel_job(cx));
    assert_eq!(rec.cancels.get(), 1, "点取消立刻发一次");

    // 模拟「标志被清掉、任务还在跑」：宿主写过取消结果，但任务没结束
    // （假宿主一收到取消就写结果，与真实实现不同，所以这里把它擦掉）
    *rec.job_result.borrow_mut() = None;
    panel.update(cx, |panel, cx| {
        let _ = panel.poll_job(cx);
    });
    assert_eq!(rec.cancels.get(), 2, "轮询到运行中的任务要补发取消");

    *rec.job_result.borrow_mut() = None;
    panel.update(cx, |panel, cx| {
        let _ = panel.poll_job(cx);
    });
    assert_eq!(rec.cancels.get(), 3, "只要任务还在跑就持续补发");
}

/// 任务跨项目收尾：写的是**上一个项目**——文案要说清归属，结果也不记进当前项目的历史。
///
/// 为何要有这一条：出口任务的写入路径与临时表名都是**提交那一刻**快照的，切项目不会改写入点；
/// 不说清，用户会在新项目里找一张不存在的表（旧行为就是这么误导的）。
#[gpui_kit::test]
fn a_job_finishing_after_a_project_switch_is_attributed_to_its_own_project(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    rec.hold_job.set(true);
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    *rec.project_root.borrow_mut() = Some(std::path::PathBuf::from("/proj/a"));
    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));

    // 项目切到 B，任务此刻才回来
    *rec.project_root.borrow_mut() = Some(std::path::PathBuf::from("/proj/b"));
    let info = {
        let draft = panel.read_with(cx, |panel, _cx| panel.draft().clone());
        TestHost { rec: rec.clone() }.gen_info(&draft)
    };
    *rec.job_result.borrow_mut() = Some(Ok(MockJobDone::Generated(info)));
    panel.update(cx, |panel, cx| {
        let _ = panel.poll_job(cx);
    });

    panel.update(cx, |panel, _cx| {
        assert!(panel.error().is_none(), "{:?}", panel.error());
        let outcome = panel.outcome().unwrap_or_default();
        assert!(outcome.contains("已生成"), "{outcome}");
        assert!(
            outcome.contains("上一个项目"),
            "要说清归属（否则用户会在新项目里找这张表）：{outcome}"
        );
        assert!(
            panel.history.is_empty(),
            "跨项目回来的结果不该记进当前项目的历史"
        );
        assert!(panel.history_error.is_none(), "{:?}", panel.history_error);
    });
}

#[gpui_kit::test]
fn import_dialog_opens_and_applies_selection(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.refresh_sources(cx);
    });
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.open_import_dialog(window, cx));
    });
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx), "导入结构对话框应打开");
        window.draw(cx).clear(cx);
    });
}

#[gpui_kit::test]
fn column_dialog_opens_with_params(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    let id = panel.read_with(cx, |panel, _cx| panel.draft().columns[0].id);
    cx.update(|window, cx| {
        detail.update(cx, |view, cx| view.open_column_dialog(id, window, cx));
    });
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx), "列编辑对话框应打开");
        window.draw(cx).clear(cx);
    });
}

/// 搜索对话框的委托：`List` 的搜索入口过滤目录，确认（点行 / 回车）后写回那一列的生成器。
#[gpui_kit::test]
fn generator_search_filters_then_applies_to_column(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));
    let id = panel.update(cx, |panel, cx| {
        panel.add_column(
            "email".to_string(),
            ColumnDataType::Varchar { length: None },
            cx,
        );
        panel.draft().columns[0].id
    });

    let mut expected = String::new();
    cx.update(|window, cx| {
        let list = cx.new(|cx| {
            ListState::new(
                // `current` 传 `None`：这里只验证「过滤 → 确认 → 写回」这条主路径
                super::GeneratorSearchDelegate::new(panel.clone(), id, None),
                window,
                cx,
            )
        });
        // 初态：全量目录（不输入也能直接翻）
        assert_eq!(list.read(cx).delegate().hits().len(), 143);

        // 与 `List` 搜索框同一入口：改 query 触发 `perform_search`
        list.update(cx, |state, cx| state.set_query("电话", window, cx));
        let hits = list.read(cx).delegate().hits();
        assert!(hits.len() < 143 && !hits.is_empty(), "应过滤到少数项");
        expected = hits[0].name.to_string();

        // 选中 + 确认（等价于点一行 / 回车）
        list.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(0)), window, cx);
        });
        list.update(cx, |state, cx| {
            state.delegate_mut().confirm(false, window, cx);
        });
    });

    assert!(!expected.is_empty(), "应有命中项");
    panel.update(cx, |panel, _cx| {
        assert_eq!(
            generator_catalog::spec_of(&panel.draft().columns[0].def.generator).name,
            expected,
            "确认后应写回该列"
        );
        assert_eq!(panel.draft().columns[0].confidence, "manual");
    });
}

/// 菜单里的「搜索生成器」入口能把对话框开出来（`List` 组件在对话框里正常挂载）。
#[gpui_kit::test]
fn generator_search_dialog_opens(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));
    let id = panel.update(cx, |panel, cx| {
        panel.add_column(
            "email".to_string(),
            ColumnDataType::Varchar { length: None },
            cx,
        );
        panel.draft().columns[0].id
    });

    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.open_generator_search(id, window, cx));
    });
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx), "搜索生成器对话框应打开");
        window.draw(cx).clear(cx);
    });
}

/// 详情视图渲染字段与预览；`MockDetailView` 的状态全部来自面板（单一权威）。
#[gpui_kit::test]
fn detail_view_renders_fields_and_preview(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));

    // 空态可渲染
    draw(cx);
    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
        panel.add_column(
            "email".to_string(),
            ColumnDataType::Varchar { length: None },
            cx,
        );
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    draw(cx);

    detail.update(cx, |view, _cx| {
        assert_eq!(view.panel().entity_id(), panel.entity_id());
    });
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.draft().columns.len(), 2);
        assert_eq!(
            panel.gen_info().map(|i| i.preview.rows.len()),
            Some(2),
            "预览应在详情 tab 显示"
        );
    });
}

/// `focus_detail_tab` 未加入 Dock 时应静默返回（不 panic）。
#[gpui_kit::test]
fn focus_tab_without_group_is_noop(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (_panel, detail, cx) = open_harness(cx, test_host(&rec));

    cx.update(|window, cx| {
        focus_detail_tab(&detail, window, cx);
    });
}

/// 已进 Dock 时聚焦必须**真的切到详情 tab**，且不得 panic。
///
/// `focus_detail_tab` 要在 TabGroup 里找自身 tab，而找/选的过程会触碰面板实体。
/// 旧实现把它放在 `update_entity(MockDetailView)` 闭包里调用（宿主接线如此），
/// 一旦详情视图已进 Dock 就会以 “cannot read … while it is already being updated” 崩溃——
/// 这正是「查看详情」按钮在真机上的崩溃路径；单测当时只覆盖了无 group 分支。
#[gpui_kit::test]
fn focus_tab_in_dock_selects_self(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));

    cx.update(|window, cx| {
        let area = cx.new(|cx| DockArea::new("mock-focus-area", None, window, cx));
        // 再建一个同类面板压在详情之上，再把组切到它：这样详情 tab **不是**当前激活项，
        // `select_tab` 不会走「已激活即返回」的捷径，切换才是真被验证。
        let other = cx.new(|cx| MockDetailView::new(panel.clone(), DetailTarget::Draft, cx));
        area.update(cx, |area, cx| {
            area.add_panel(detail.clone(), DockPlacement::Center, None, window, cx);
            area.add_panel(other, DockPlacement::Center, None, window, cx);
        });

        // group 由 `Panel::on_added_to` 注入；tests 是 mock_view 的子模块，可直接读私有字段
        let group = detail
            .read(cx)
            .group
            .clone()
            .expect("进 Dock 后应注入 group")
            .upgrade()
            .expect("tab 组应存活");
        group.update(cx, |group, cx| group.select_tab(1, window, cx));
        assert_ne!(group.read(cx).active_ix(), 0, "前置条件：详情 tab 不在前台");

        focus_detail_tab(&detail, window, cx);

        assert_eq!(
            group.read(cx).active_ix(),
            0,
            "聚焦后应切回详情 tab（不是静默 no-op）"
        );
    });
}

// ==================== 后台任务（进度 / 取消） ====================

/// 任务进行中：进度由宿主报告，面板镜像给 UI（含百分比与行数估算）。
#[gpui_kit::test]
fn job_progress_is_mirrored_while_running(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    // 任务停在「进行中」：不写结果，只报告 3/10 批
    rec.hold_job.set(true);
    rec.job_progress.set(MockJobProgress {
        phase: MockJobPhase::Generating,
        batches_done: 3,
        batches_total: 10,
        rows_total: 1000,
    });
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert!(panel.is_running(), "结果未到，应继续轮询");
        let progress = panel.job_progress().expect("应有进度");
        assert_eq!(progress.batches_done, 3);
        assert_eq!(progress.batches_total, 10);
        assert!(
            (progress.percent() - 30.0).abs() < 0.01,
            "应报 30%（实际 {}）",
            progress.percent()
        );
        assert_eq!(progress.rows_done(), 300, "按批次粒度估算已生成行数");
        assert!(
            panel.outcome().is_some_and(|o| o.contains("生成中")),
            "{:?}",
            panel.outcome()
        );
        assert!(panel.gen_info().is_none(), "未完成不应有结果");
    });
}

/// 取消：置位取消请求（按钮转「正在取消…」），结果以可读文案回传。
#[gpui_kit::test]
fn cancel_requests_host_and_reports_cancel_outcome(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    rec.hold_job.set(true);
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    panel.update(cx, |panel, cx| panel.cancel_job(cx));

    panel.update(cx, |panel, _cx| {
        assert!(panel.cancel_requested(), "应置位取消请求");
    });
    assert_eq!(rec.cancels.get(), 1, "应把取消请求转给宿主");

    // 重复点取消不重复下发
    panel.update(cx, |panel, cx| panel.cancel_job(cx));
    assert_eq!(rec.cancels.get(), 1);

    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(!panel.is_running(), "取消后任务应结束");
        let error = panel.error().expect("应有取消文案");
        assert!(error.contains("已取消"), "{error}");
        assert!(
            error.contains("残留"),
            "应提醒临时表可能残留部分行: {error}"
        );
        assert!(panel.gen_info().is_none(), "取消后旧结果作废");
    });
}

/// 提交失败（宿主拦住重复任务 / 工作线程不可用）直接落到错误行。
#[gpui_kit::test]
fn start_errors_are_surfaced(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    *rec.start_error.borrow_mut() = Some("已有任务在进行中".to_string());
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));

    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("已有任务在进行中"));
        assert!(!panel.is_running(), "提交失败不应进入进行中");
    });
    assert!(rec.generated.borrow().is_empty(), "提交失败不应触生成");
}

/// 后台线程异常退出（既无进度也无结果）：不静默挂死，给可读错误。
#[gpui_kit::test]
fn vanished_job_reports_readable_error(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    rec.hold_job.set(true);
    rec.pretend_idle.set(true);
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);

    panel.update(cx, |panel, _cx| {
        assert!(!panel.is_running());
        assert!(
            panel.error().is_some_and(|e| e.contains("异常结束")),
            "{:?}",
            panel.error()
        );
    });
}

/// 任务进行中重复点「生成」：不重复提交（宿主侧也会拦，这里是视图第一道）。
#[gpui_kit::test]
fn second_start_while_running_is_rejected(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    rec.hold_job.set(true);
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| {
        panel.run_generate(cx);
        panel.run_generate(cx);
    });

    panel.update(cx, |panel, _cx| {
        assert_eq!(
            panel.error(),
            Some("已有任务在进行中（请等它结束或先取消）")
        );
        assert!(panel.is_running(), "首个任务仍在进行");
    });
    assert_eq!(rec.started.borrow().len(), 1, "只应提交一次");
}

/// 出口（落库）也是后台任务：提交即返回、进行中报「写入项目分析库」阶段，完成后预览仍可用。
#[gpui_kit::test]
fn persist_job_runs_in_background_and_keeps_preview(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);

    // 落库：结果已就绪但还没轮询 → 仍在「进行中」（UI 不被写入阻塞）
    panel.update(cx, |panel, cx| panel.persist_table(cx));
    panel.update(cx, |panel, _cx| {
        assert!(panel.is_running(), "落库应走后台任务");
        assert_eq!(
            panel.job_progress().map(|p| p.phase),
            Some(MockJobPhase::Writing),
            "应报写入阶段"
        );
        assert!(panel.gen_info().is_some(), "写入期间预览仍在");
        assert!(
            panel
                .outcome()
                .is_some_and(|o| o.contains("写入项目分析库中")),
            "{:?}",
            panel.outcome()
        );
    });

    // 出口类不可取消：请求被忽略（不转发给宿主），按钮也不渲染
    panel.update(cx, |panel, cx| panel.cancel_job(cx));
    assert_eq!(rec.cancels.get(), 0, "出口类任务不应把取消转给宿主");
    panel.update(cx, |panel, _cx| assert!(!panel.cancel_requested()));

    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(!panel.is_running(), "结果回填后归位空闲");
        assert!(
            panel
                .outcome()
                .is_some_and(|o| o.contains("已在项目分析库新建表 mock_data（5 行）")),
            "{:?}",
            panel.outcome()
        );
        assert_eq!(panel.landed(), Some("mock_data"));
        assert!(
            panel.gen_info().is_some(),
            "出口只读临时表，预览不能一并作废（否则没法接着导出）"
        );
        assert_eq!(panel.existing_tables(), rec.tables.borrow().as_slice());
    });
    assert_eq!(
        rec.sink_temps.borrow().as_slice(),
        ["temp_mock_mock_data".to_string()],
        "出口任务要带上上一次生成的结果（临时表）"
    );
}

/// 导出与草稿箱同走后台任务（阶段 = 写出文件），完成后预览仍保留。
#[gpui_kit::test]
fn export_jobs_run_in_background(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);

    panel.update(cx, |panel, cx| {
        panel.export_file(&MockExportFormat::Csv, "/tmp/mock_data.csv".to_string(), cx)
    });
    panel.update(cx, |panel, _cx| {
        assert_eq!(
            panel.job_progress().map(|p| p.phase),
            Some(MockJobPhase::Exporting)
        );
        assert!(
            panel.outcome().is_some_and(|o| o.contains("导出中")),
            "{:?}",
            panel.outcome()
        );
    });
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.outcome(), Some("已导出：/tmp/mock_data.csv"));
        assert!(panel.error().is_none());
        assert!(panel.gen_info().is_some(), "导出后预览仍可用");
    });

    // 草稿箱：同一条链路（只换落地目录）
    panel.update(cx, |panel, cx| {
        panel.save_scratchpad(&MockExportFormat::Parquet, cx)
    });
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(
            panel
                .outcome()
                .is_some_and(|o| o.contains("已保存到草稿箱")),
            "{:?}",
            panel.outcome()
        );
    });

    assert_eq!(
        rec.exported.borrow().as_slice(),
        [("Csv".to_string(), "/tmp/mock_data.csv".to_string())]
    );
    assert_eq!(rec.scratchpads.borrow().as_slice(), ["Parquet".to_string()]);
    assert_eq!(
        rec.sink_temps.borrow().as_slice(),
        [
            "temp_mock_mock_data".to_string(),
            "temp_mock_mock_data".to_string()
        ]
    );
}

/// 出口失败（同名表已存在）：错误原文直出、刷新既有表清单、且预览仍保留（可改用「追加」）。
#[gpui_kit::test]
fn sink_failure_keeps_preview(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);

    *rec.tables.borrow_mut() = vec!["mock_data".to_string()];
    panel.update(cx, |panel, cx| panel.persist_table(cx));
    poll_job(cx, &panel);

    panel.update(cx, |panel, _cx| {
        assert!(!panel.is_running());
        assert!(
            panel.error().is_some_and(|e| e.contains("已存在")),
            "{:?}",
            panel.error()
        );
        assert!(panel.outcome().is_none());
        assert!(panel.gen_info().is_some(), "失败后预览仍可用来重试 / 追加");
        assert_eq!(panel.existing_tables(), ["mock_data".to_string()]);
    });
}

/// 没有生成结果时点出口：拒绝提交任务（不占后台线程）。
#[gpui_kit::test]
fn sinks_require_a_generation_first(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
        panel.export_file(&MockExportFormat::Csv, "/tmp/x.csv".to_string(), cx);
    });
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("请先生成（预览确认后再导出）"));
        assert!(!panel.is_running());
    });

    panel.update(cx, |panel, cx| {
        panel.save_scratchpad(&MockExportFormat::Csv, cx)
    });
    panel.update(cx, |panel, cx| panel.persist_table(cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("请先生成（预览确认后再落库）"));
        assert!(!panel.is_running());
    });
    assert!(rec.started.borrow().is_empty(), "不应提交任何任务");
}

/// 项目切换：面板作废旧生成结果（临时表已被宿主清掉），草稿保留。
#[gpui_kit::test]
fn forget_generated_drops_result_but_keeps_draft(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(panel.gen_info().is_some(), "前置条件：已有生成结果");
    });

    // 宿主清了 3 张临时表 → 结果作废 + 一句可读提示
    panel.update(cx, |panel, cx| panel.forget_generated(3, cx));
    panel.update(cx, |panel, _cx| {
        assert!(panel.gen_info().is_none(), "临时表已不在，旧结果必须作废");
        assert_eq!(panel.landed(), None);
        assert!(panel.error().is_none());
        let outcome = panel.outcome().expect("应提示清理");
        assert!(outcome.contains("3 张"), "{outcome}");
        assert_eq!(
            panel.draft().table_name,
            "mock_data",
            "草稿（用户配置）保留"
        );
        assert_eq!(panel.draft().columns.len(), 1);
    });

    // 没清到东西就不打扰用户
    panel.update(cx, |panel, cx| panel.forget_generated(0, cx));
    panel.update(cx, |panel, _cx| {
        assert!(panel.outcome().is_none(), "无临时表可清时不该报一句");
    });
}

/// 约束类生成器（取值集合 / 加权选项）能在列编辑对话框里编辑：
/// 合法输入写回生成器；非法输入**保留上一个有效值**并就地提示（带行号）。
#[gpui_kit::test]
fn complex_param_edits_write_back_and_keep_last_valid(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));

    let id = panel.update(cx, |panel, cx| {
        panel.add_column(
            "city".to_string(),
            ColumnDataType::Varchar { length: None },
            cx,
        );
        let id = panel.draft().columns[0].id;
        panel.set_generator(id, "weighted", cx);
        id
    });
    draw(cx);
    cx.update(|window, cx| {
        detail.update(cx, |view, cx| view.open_column_dialog(id, window, cx));
    });

    // 参数行里的这一项应当是**多行控件**（不是单行 Input）
    detail.update(cx, |view, _cx| {
        let draft = view.draft.as_ref().expect("对话框应已打开");
        let widget = draft
            .params
            .iter()
            .find(|p| p.key == "choices")
            .map(|p| &p.widget)
            .expect("加权选项应有一行参数");
        assert!(
            matches!(widget, super::ParamWidget::Complex(_)),
            "集合类参数应当用多行控件"
        );
        assert!(matches!(
            &draft.def.generator,
            GeneratorConfig::Weighted { choices } if choices.is_empty()
        ));
    });

    // 合法输入：写回生成器，错误清空
    detail.update(cx, |view, cx| {
        view.commit_complex_param("choices", "北京, 3\n上海, 1", cx)
    });
    detail.update(cx, |view, _cx| {
        let draft = view.draft.as_ref().expect("对话框应已打开");
        assert_eq!(draft.param_error, None);
        assert!(
            matches!(
                &draft.def.generator,
                GeneratorConfig::Weighted { choices }
                    if choices == &vec![("北京".to_string(), 3.0), ("上海".to_string(), 1.0)]
            ),
            "{:?}",
            draft.def.generator
        );
        assert_eq!(draft.confidence, "manual");
    });

    // 非法输入：生成器**不变** + 提示带行号
    detail.update(cx, |view, cx| {
        view.commit_complex_param("choices", "北京, 3\n上海, x", cx)
    });
    detail.update(cx, |view, _cx| {
        let draft = view.draft.as_ref().expect("对话框应已打开");
        let (key, message) = draft.param_error.clone().expect("应就地提示");
        assert_eq!(key, "choices");
        assert!(message.contains("第 2 行"), "{message}");
        assert!(
            matches!(
                &draft.def.generator,
                GeneratorConfig::Weighted { choices } if choices.len() == 2
            ),
            "非法输入不应改写生成器"
        );
    });
}

/// 约束类生成器的列编辑对话框能建起来并渲染（多行 `Textarea` 在对话框里正常挂载）。
#[gpui_kit::test]
fn constraint_column_dialog_opens(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, detail, cx) = open_harness(cx, test_host(&rec));

    let id = panel.update(cx, |panel, cx| {
        panel.add_column(
            "order_status".to_string(),
            ColumnDataType::Varchar { length: None },
            cx,
        );
        let id = panel.draft().columns[0].id;
        panel.set_generator(id, "foreign_key", cx);
        id
    });
    draw(cx);
    cx.update(|window, cx| {
        detail.update(cx, |view, cx| view.open_column_dialog(id, window, cx));
    });
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx), "列编辑对话框应打开");
        window.draw(cx).clear(cx);
    });
    detail.update(cx, |view, _cx| {
        let draft = view.draft.as_ref().expect("对话框应已打开");
        assert!(
            draft
                .params
                .iter()
                .any(|p| p.key == "values" && matches!(p.widget, super::ParamWidget::Complex(_))),
            "外键取值的取值集合应当是多行控件"
        );
    });
}

/// `MockRunOptions::new` 是 `#[non_exhaustive]` 结构的唯一构造入口。
#[test]
fn run_options_constructor_keeps_field_order() {
    let options = MockRunOptions::new(7, Some(9), Locale::En);
    assert_eq!(options.rows, 7);
    assert_eq!(options.seed, Some(9));
    assert_eq!(options.locale, Locale::En);
}

// ==================== 生成历史（D4/D5） ====================

/// 一条历史任务（面板渲染 / 重放用）。
fn history_task(id: &str, status: &str) -> MockGenerationTask {
    MockGenerationTask {
        id: id.to_string(),
        table_name: "orders".to_string(),
        table_alias: None,
        row_count: 500,
        seed: Some(7),
        locale: "ZH_CN".to_string(),
        scene_id: None,
        save_format: None,
        status: status.to_string(),
        error_message: (status != "success").then(|| "项目分析库已存在表 orders".to_string()),
        generated_rows: (status == "success").then_some(500),
        generation_time_ms: Some(42),
        created_at: Some("2026-09-16T15:02:03+00:00".to_string()),
        updated_at: Some("2026-09-16T15:02:03+00:00".to_string()),
    }
}

/// 一条历史任务的列（重放用）。
fn history_column(id: &str, task_id: &str, name: &str, order: i32) -> MockGenerationColumn {
    MockGenerationColumn {
        id: id.to_string(),
        task_id: task_id.to_string(),
        column_name: name.to_string(),
        column_type: "INTEGER".to_string(),
        generator: "random_int".to_string(),
        generator_params: Some(r#"{"min":1,"max":10}"#.to_string()),
        null_ratio: 0.0,
        is_unique: false,
        is_primary_key: false,
        is_foreign_key: false,
        ref_table: None,
        ref_column: None,
        comment: None,
        confidence: Some("high".to_string()),
        sort_order: order,
    }
}

/// 未打开项目：历史没有落点，给一句可读的原因（而不是看起来像「本项目没有记录」）。
#[gpui_kit::test]
fn history_without_project_reports_a_readable_reason(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| panel.refresh_history(cx));
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert!(!panel.history_loading, "未打开项目不该起后台任务");
        let reason = panel.history_error.as_deref().unwrap_or_default();
        assert!(reason.contains("未打开项目"), "{reason}");
        assert!(panel.history.is_empty());
    });
}

/// 列表回填后行能渲染（绘制一帧不 panic），失败行保留原因。
#[gpui_kit::test]
fn history_rows_render_and_keep_failure_reasons(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.accept_history(
            Ok(HistoryReply::Listed(history::HistorySnapshot {
                tasks: vec![
                    history_task("t_ok", "success"),
                    history_task("t_bad", "failed"),
                ],
                templates: Vec::new(),
            })),
            cx,
        );
    });
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert!(panel.history_loaded);
        assert_eq!(panel.history.len(), 2);
        assert_eq!(panel.history[1].status, "failed");
        assert_eq!(
            panel.history[1].error_message.as_deref(),
            Some("项目分析库已存在表 orders")
        );
        assert!(panel.history_error.is_none());
    });

    // 读失败：列表保留（能看到的旧数据比一片空白有用），错误另起一行
    panel.update(cx, |panel, cx| {
        panel.accept_history(Err("打开项目库失败：坏库".to_string()), cx);
    });
    draw(cx);
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.history.len(), 2, "读失败不该清掉已显示的列表");
        assert!(
            panel
                .history_error
                .as_deref()
                .unwrap_or_default()
                .contains("坏库")
        );
    });
}

/// 重放：草稿整体换成历史里那一套（表名 / 行数 / 种子 / 列），旧结果作废。
#[gpui_kit::test]
fn replay_writes_the_recorded_configuration_into_the_draft(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        // 先造出「旧结果 + 旧列」的状态，重放后应全部换掉
        panel.add_column("legacy".to_string(), ColumnDataType::Text, cx);
        panel.accept_history(
            Ok(HistoryReply::Replayed(Box::new(MockGenerationDetail {
                task: history_task("t1", "success"),
                columns: vec![
                    history_column("c0", "t1", "id", 0),
                    history_column("c1", "t1", "amount", 1),
                ],
            }))),
            cx,
        );
    });
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.draft().table_name, "orders");
        assert_eq!(panel.draft().options.rows, 500);
        assert_eq!(panel.draft().options.seed, Some(7));
        assert_eq!(panel.draft().options.locale, Locale::ZhCn);
        assert_eq!(
            panel
                .draft()
                .columns
                .iter()
                .map(|c| c.def.name.as_str())
                .collect::<Vec<_>>(),
            vec!["id", "amount"],
            "列换成历史里的那一套（旧列不留）"
        );
        assert!(
            panel
                .outcome
                .as_deref()
                .unwrap_or_default()
                .contains("已重放"),
            "要有「配置已摆回来」的回执"
        );
        assert!(panel.error.is_none());
        assert!(panel.gen_info().is_none(), "旧临时表已与新配置对不上，作废");
        // 表名输入框已跟着走（`table_pending` 在渲染时落进输入框并清空）
        let shown = panel
            .table_input
            .as_ref()
            .expect("渲染后输入框已创建")
            .read(_cx)
            .value()
            .to_string();
        assert_eq!(shown, "orders");
        // 列 id 重新编号：不沿用历史里的 id，避免与现存在列撞号
        assert!(panel.draft().columns.iter().all(|c| c.id < panel.next_id));
    });
}

// ==================== 用户模板（C4） ====================

/// 一个用户模板（列表 / 应用用）。
fn history_template(id: &str, name: &str) -> MockUserTemplate {
    MockUserTemplate {
        id: id.to_string(),
        name: name.to_string(),
        description: Some("2 列".to_string()),
        row_count: 750,
        seed: Some(99),
        locale: "EN".to_string(),
        created_at: Some("2026-09-16T15:02:03+00:00".to_string()),
        updated_at: Some("2026-09-16T15:02:03+00:00".to_string()),
    }
}

/// 模板里的列（与应用用同一形状）。
fn template_column(id: &str, template_id: &str, name: &str, order: i32) -> MockTemplateColumn {
    MockTemplateColumn {
        id: id.to_string(),
        template_id: template_id.to_string(),
        column_name: name.to_string(),
        column_type: "INTEGER".to_string(),
        generator: "safe_email".to_string(),
        generator_params: None,
        null_ratio: 0.0,
        is_unique: false,
        is_primary_key: false,
        is_foreign_key: false,
        ref_table: None,
        ref_column: None,
        comment: None,
        confidence: None,
        sort_order: order,
    }
}

/// 应用模板：行数 / 种子 / 语言 / 列都换成模板里那一套，**目标表名不动**。
#[gpui_kit::test]
fn applying_a_template_keeps_the_target_table_name(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        // 用户正在写别的表：应用模板不该把它换掉
        panel.draft.table_name = "orders_next".to_string();
        panel.add_column("legacy".to_string(), ColumnDataType::Text, cx);
        panel.accept_history(
            Ok(HistoryReply::Listed(history::HistorySnapshot {
                tasks: Vec::new(),
                templates: vec![history_template("tpl1", "电商主数据")],
            })),
            cx,
        );
    });
    draw(cx);
    panel.update(cx, |panel, _cx| assert_eq!(panel.templates.len(), 1));

    panel.update(cx, |panel, cx| {
        panel.accept_history(
            Ok(HistoryReply::TemplateApplied(Box::new((
                history_template("tpl1", "电商主数据"),
                vec![
                    template_column("tc0", "tpl1", "id", 0),
                    template_column("tc1", "tpl1", "email", 1),
                ],
            )))),
            cx,
        );
    });
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.draft().table_name, "orders_next", "表名保住");
        assert_eq!(panel.draft().options.rows, 750);
        assert_eq!(panel.draft().options.seed, Some(99));
        assert_eq!(panel.draft().options.locale, Locale::En);
        assert_eq!(
            panel
                .draft()
                .columns
                .iter()
                .map(|column| column.def.name.as_str())
                .collect::<Vec<_>>(),
            vec!["id", "email"],
            "列换成模板里的那一套"
        );
        assert!(panel.gen_info().is_none(), "旧临时表与新配置对不上，作废");
        let outcome = panel.outcome.clone().unwrap_or_default();
        assert!(outcome.contains("已应用模板 电商主数据"), "{outcome}");
    });
}

/// 存模板的两道门：空配置与空名字都在事件路径上拦住。
#[gpui_kit::test]
fn saving_a_template_rejects_empty_draft_and_empty_name(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    // 一列都没有：存下来套不出任何东西
    panel.update(cx, |panel, cx| {
        panel.save_template("我的模板".to_string(), cx);
    });
    panel.update(cx, |panel, _cx| {
        let note = panel.history_error.clone().unwrap_or_default();
        assert!(note.contains("先加列"), "{note}");
    });

    // 有列但名字是空白：拒绝（不静默变成「无标题模板」）
    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
        panel.save_template("   ".to_string(), cx);
    });
    panel.update(cx, |panel, _cx| {
        let note = panel.history_error.clone().unwrap_or_default();
        assert!(note.contains("模板名不能为空"), "{note}");
        assert!(panel.templates.is_empty());
    });

    // 两道门都过了：请求真的发出去（这里没有项目根，所以回一条可读原因而不是静默丢弃）
    panel.update(cx, |panel, cx| {
        panel.save_template("我的模板".to_string(), cx);
    });
    panel.update(cx, |panel, _cx| {
        let note = panel.history_error.clone().unwrap_or_default();
        assert!(note.contains("未打开项目"), "{note}");
    });
}

/// 删除失败（如未打开项目）时，列表不变且错误可见。
#[gpui_kit::test]
fn delete_history_keeps_the_list_and_reports_failure(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.accept_history(
            Ok(HistoryReply::Listed(history::HistorySnapshot {
                tasks: vec![history_task("t1", "success")],
                templates: Vec::new(),
            })),
            cx,
        );
        panel.delete_history("t1".to_string(), cx);
    });
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.history.len(), 1, "删除没成功就不该假装删掉了");
        assert!(
            panel
                .history_error
                .as_deref()
                .unwrap_or_default()
                .contains("未打开项目")
        );
    });
}

// ==================== 场景模板（C1） ====================

/// 测试用场景模板：三张表（`orders` / `items` / `users`，与 [`SCENARIO_TABLES`] 同名同量），
/// 两条引用——其中 `orders.user_id → users.id` 是**前向引用**（父表排在后面，合法）。
///
/// 面板行为用例用它而不是内置模板：绑内置模板的数字，改模板就会连带改一批用例。
fn toy_scenario() -> ScenarioTemplate {
    let id_col = || ColumnDef {
        name: "id".to_string(),
        data_type: ColumnDataType::Integer,
        generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
        nullable_ratio: 0.0,
        unique: true,
        dependency: None,
    };
    let fk_col = |name: &str, parent: &str, max: i32| ColumnDef {
        name: name.to_string(),
        data_type: ColumnDataType::Integer,
        generator: GeneratorConfig::RandomInt { min: 1, max },
        nullable_ratio: 0.0,
        unique: false,
        dependency: Some(ColumnDependency::foreign_key(parent, "id")),
    };
    ScenarioTemplate {
        id: "toy:scenario".to_string(),
        name: "玩具场景".to_string(),
        description: String::new(),
        category: "测试".to_string(),
        locale: "zh_cn".to_string(),
        tables: vec![
            TemplateTable {
                name: "orders".to_string(),
                row_count: 100,
                columns: vec![id_col(), fk_col("user_id", "users", 40)],
            },
            TemplateTable {
                name: "items".to_string(),
                row_count: 250,
                columns: vec![id_col(), fk_col("order_id", "orders", 100)],
            },
            TemplateTable {
                name: "users".to_string(),
                row_count: 40,
                columns: vec![id_col()],
            },
        ],
    }
}

/// 场景模板：一次产出多张结果表，默认选中第一张，并标出来源。
#[gpui_kit::test]
fn scenario_generation_fills_one_result_per_table(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    // 草稿里一列都没有：场景模板自带列定义，不该被草稿拦住
    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    panel.update(cx, |panel, _cx| {
        assert!(panel.is_running(), "场景生成也走后台任务");
        assert!(
            panel.scenario().is_some(),
            "生成期间工作副本还在（可直接再生成一次）"
        );
        assert!(matches!(
            panel.job_kind(),
            Some(MockJobKind::Scenario(template)) if template.id == "toy:scenario"
        ));
        assert!(
            panel.job_kind().is_some_and(MockJobKind::generates),
            "场景任务包含生成阶段：可取消"
        );
    });
    poll_job(cx, &panel);
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert_eq!(
            rec.scenarios.borrow().as_slice(),
            ["toy:scenario".to_string()]
        );
        assert_eq!(panel.results().len(), SCENARIO_TABLES.len());
        assert_eq!(panel.current_result(), 0, "默认选第一张");
        assert_eq!(panel.scenario_source(), Some("玩具场景"));
        assert_eq!(
            panel
                .results()
                .iter()
                .map(|info| info.table_name.as_str())
                .collect::<Vec<_>>(),
            vec!["orders", "items", "users"],
            "顺序即模板里的表序"
        );
        // 当前表 = 第一张：任务结果区与详情都看它
        let info = panel.gen_info().expect("结果应就绪");
        assert_eq!(info.table_name, "orders");
        assert_eq!(info.temp_table_name, "temp_mock_orders");
        assert_eq!(panel.draft().table_name, "mock_data", "场景不改草稿");
        let outcome = panel.outcome.clone().unwrap_or_default();
        assert!(outcome.contains("3 张表"), "{outcome}");
        assert!(outcome.contains("合计 390 行"), "{outcome}");
    });
}

/// 切换当前表：`gen_info`（结果区 / 详情）跟着走。
#[gpui_kit::test]
fn selecting_a_result_switches_what_the_panel_shows(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);
    draw(cx);

    panel.update(cx, |panel, cx| panel.select_result(2, cx));
    draw(cx);
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.current_result(), 2);
        assert_eq!(
            panel.gen_info().map(|i| i.table_name.as_str()),
            Some("users")
        );
    });

    // 越界不生效（渲染期不会 panic，也不会把当前表弄丢）
    panel.update(cx, |panel, cx| panel.select_result(9, cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.current_result(), 2, "越界下标不该改状态");
        assert_eq!(
            panel.gen_info().map(|i| i.table_name.as_str()),
            Some("users")
        );
    });
}

/// 出口作用于**当前选中**那张：目标表名取自结果，不是草稿。
#[gpui_kit::test]
fn exits_apply_to_the_selected_result_table(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);
    draw(cx);

    // 选中第二张（items）后落库
    panel.update(cx, |panel, cx| {
        panel.select_result(1, cx);
        panel.persist_table(cx);
    });
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(rec.persisted.borrow().as_slice(), ["items".to_string()]);
        assert_eq!(
            rec.sink_temps.borrow().as_slice(),
            ["temp_mock_items".to_string()]
        );
        assert_eq!(panel.landed(), Some("items"));
    });

    // 换到第三张再导出：文件名也跟当前表走
    panel.update(cx, |panel, cx| {
        panel.select_result(2, cx);
        let name = panel.export_file_name(&MockExportFormat::Csv);
        assert_eq!(name, "users.csv");
        panel.export_file(&MockExportFormat::Csv, "/tmp/users.csv".to_string(), cx);
    });
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(rec.sink_temps.borrow().as_slice()[1], "temp_mock_users");
        assert!(
            panel
                .outcome()
                .unwrap_or_default()
                .contains("/tmp/users.csv")
        );
    });
}

/// 单表生成回到「一张结果、无来源」：场景态不残留。
#[gpui_kit::test]
fn single_table_generation_clears_the_scenario_state(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| assert_eq!(panel.results().len(), 3));

    panel.update(cx, |panel, cx| panel.run_generate(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.results().len(), 1);
        assert_eq!(panel.current_result(), 0);
        assert_eq!(panel.scenario_source(), None);
        assert_eq!(
            panel.gen_info().map(|i| i.table_name.as_str()),
            Some("mock_data")
        );
    });
}

/// 进行中的场景任务按「张表」报进度，不拿行数当量纲。
#[gpui_kit::test]
fn scenario_job_reports_tables_as_its_unit(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    rec.hold_job.set(true);
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    // 宿主报「已生成 2 / 3 张表」（批次槽位复用为表计数）
    rec.job_progress.set(MockJobProgress {
        phase: MockJobPhase::Generating,
        batches_done: 2,
        batches_total: 3,
        rows_total: 0,
    });
    poll_job(cx, &panel);
    draw(cx);

    panel.update(cx, |panel, _cx| {
        let progress = panel.job_progress().expect("进行中应有进度");
        assert_eq!(progress.rows_total, 0, "场景任务的行数由模板定，草稿不参与");
        assert_eq!(progress.rows_done(), 0, "没有行数量纲：别算出行数");
        assert_eq!(progress.batches_total, 3);
        assert!(
            panel
                .job_kind()
                .is_some_and(|kind| { matches!(kind, MockJobKind::Scenario(_)) })
        );
    });

    // 取消：场景任务含生成阶段，引擎逐表响应
    panel.update(cx, |panel, cx| panel.cancel_job(cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(rec.cancels.get(), 1, "取消应直达宿主");
        assert_eq!(panel.results().len(), 0, "还没回填结果");
    });
}

/// 选模板只**载入工作副本**，不立即生成——用户要能先看清单、调关系。
#[gpui_kit::test]
fn opening_a_scenario_loads_a_work_copy_without_generating(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| panel.open_scenario("builtin:ecommerce", cx));
    draw(cx);
    panel.update(cx, |panel, _cx| {
        assert!(!panel.is_running(), "载入模板不该提交任务");
        assert!(rec.scenarios.borrow().is_empty());
        assert!(rec.started.borrow().is_empty());
        let template = panel.scenario().expect("工作副本已就位");
        assert_eq!(template.id, "builtin:ecommerce");
        assert_eq!(template.name, "电商系统");
        assert_eq!(
            template
                .tables
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            ["users", "products", "orders", "order_items"]
        );
        // 关系是派生视图：扫列上的 dependency（不另存清单）
        let relations = panel.scenario_relations();
        assert_eq!(relations.len(), 3);
        assert_eq!(
            relations
                .iter()
                .map(ScenarioRelation::label)
                .collect::<Vec<_>>(),
            [
                "orders.user_id → users.id",
                "order_items.order_id → orders.id",
                "order_items.product_id → products.id",
            ]
        );
    });

    // 未知模板 id：可读错误，不静默
    panel.update(cx, |panel, cx| panel.open_scenario("builtin:nope", cx));
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.error().is_some_and(|e| e.contains("场景模板不存在")),
            "{:?}",
            panel.error()
        );
    });

    // 退出场景态：工作副本丢掉，草稿与结果不受影响
    panel.update(cx, |panel, cx| panel.close_scenario(cx));
    panel.update(cx, |panel, _cx| {
        assert!(panel.scenario().is_none());
        assert!(panel.scenario_relations().is_empty());
    });
}

/// 加关系：写进子列（模型里唯一的关系表达处），并把该列生成器**对齐到父域**。
#[gpui_kit::test]
fn adding_a_relation_writes_it_onto_the_child_column(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        // 先删掉现有的那条，再加一条新的（指向 users）
        panel.remove_relation("items", "order_id", cx);
        *panel.relation_pick.borrow_mut() = RelationPick {
            child_table: Some("items".to_string()),
            child_column: Some("order_id".to_string()),
            parent_table: Some("users".to_string()),
            parent_column: Some("id".to_string()),
        };
        panel.add_relation(cx);
    });
    draw(cx);

    panel.update(cx, |panel, _cx| {
        let template = panel.scenario().expect("工作副本");
        let column = template
            .tables
            .iter()
            .find(|t| t.name == "items")
            .and_then(|t| t.columns.iter().find(|c| c.name == "order_id"))
            .expect("子列");
        let dep = column.dependency.as_ref().expect("关系写在列上");
        assert_eq!(dep.ref_table, "users");
        assert_eq!(dep.ref_column, "id");
        // users 40 行、id 自增 1..40 → 生成器对齐到 1..40（单表生成也不能跑出父域）
        assert!(
            matches!(
                column.generator,
                GeneratorConfig::RandomInt { min: 1, max: 40 }
            ),
            "{:?}",
            column.generator
        );
        assert!(
            panel
                .outcome()
                .unwrap_or_default()
                .contains("items.order_id")
        );
        assert!(panel.error().is_none(), "{:?}", panel.error());
    });

    // 删掉后：列上不再有 dependency（生成器保留，此时它就是个普通随机列）
    panel.update(cx, |panel, cx| {
        panel.remove_relation("items", "order_id", cx)
    });
    panel.update(cx, |panel, _cx| {
        assert!(
            panel
                .scenario_relations()
                .iter()
                .all(|r| !(r.child_table == "items" && r.child_column == "order_id")),
            "删掉的关系不该还在派生视图里"
        );
        assert!(panel.outcome().unwrap_or_default().contains("已删除关系"));
    });
}

/// 父列不是自增 → 加关系时就拒（只支持算得出域的引用目标），并给出可读原因。
#[gpui_kit::test]
fn adding_a_relation_rejects_a_parent_without_a_derivable_domain(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        let mut template = toy_scenario();
        // 把 users 的主键换成 uuid：域算不出来
        if let Some(users) = template.tables.iter_mut().find(|t| t.name == "users") {
            users.columns[0].generator = GeneratorConfig::UuidV4;
        }
        panel.load_scenario(template, cx);
        *panel.relation_pick.borrow_mut() = RelationPick {
            child_table: Some("items".to_string()),
            child_column: Some("order_id".to_string()),
            parent_table: Some("users".to_string()),
            parent_column: Some("id".to_string()),
        };
        panel.add_relation(cx);
    });
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.error().is_some_and(|e| e.contains("不能作引用目标")),
            "{:?}",
            panel.error()
        );
    });

    // 四个位置没选全：拒绝（不半途写坏工作副本）
    panel.update(cx, |panel, cx| {
        *panel.relation_pick.borrow_mut() = RelationPick::default();
        panel.add_relation(cx);
    });
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.error().is_some_and(|e| e.contains("都要选")),
            "{:?}",
            panel.error()
        );
    });
}

/// 改完关系再生成：提交的是**工作副本**（关系随任务走），不是按 id 重取的模板。
#[gpui_kit::test]
fn the_edited_work_copy_is_what_gets_generated(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        // 把 items.order_id 改指到 users.id（原为 orders.id）
        panel.remove_relation("items", "order_id", cx);
        *panel.relation_pick.borrow_mut() = RelationPick {
            child_table: Some("items".to_string()),
            child_column: Some("order_id".to_string()),
            parent_table: Some("users".to_string()),
            parent_column: Some("id".to_string()),
        };
        panel.add_relation(cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);

    panel.update(cx, |panel, _cx| {
        let submitted = rec.relations.borrow();
        let relations = submitted.last().expect("任务应带回关系");
        assert!(
            relations.contains(&"items.order_id→users.id".to_string()),
            "提交的应是编辑后的关系：{relations:?}"
        );
        assert!(
            !relations.contains(&"items.order_id→orders.id".to_string()),
            "旧指向不该还在：{relations:?}"
        );
        assert_eq!(panel.results().len(), 3, "三张表都回来了");
    });
}

/// 出口只作用于当前表，而关系是跨表的——面板必须把「只落这张」的后果说出来。
#[gpui_kit::test]
fn the_panel_spells_out_what_landing_only_the_current_table_means(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.last_relations().len(), 2, "关系快照随结果留存");
        // 当前表 = orders（第 0 张）：它引用了 users
        assert_eq!(
            panel.gen_info().map(|i| i.table_name.as_str()),
            Some("orders")
        );
        let note = panel.current_relation_note().expect("有跨表关系就该有提示");
        assert!(note.contains("引用了 user_id → users.id"), "{note}");
        assert!(note.contains("不会跟着落库"), "{note}");
    });

    // 切到 users（被 orders 引用）：提示换成「被引用」那一侧
    panel.update(cx, |panel, cx| panel.select_result(2, cx));
    draw(cx);
    panel.update(cx, |panel, _cx| {
        let note = panel.current_relation_note().expect("被引用也要提示");
        assert!(note.contains("被 orders.user_id 引用"), "{note}");
        assert!(note.contains("落空"), "{note}");
    });

    // 单表生成：没有任何跨表关系可言，提示必须消失
    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
        panel.run_generate(cx);
    });
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.last_relations().is_empty(),
            "单表结果清掉场景关系快照"
        );
        assert!(panel.current_relation_note().is_none());
    });

    // 切项目（作废结果）：快照也跟着清
    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);
    panel.update(cx, |panel, cx| panel.forget_generated(3, cx));
    panel.update(cx, |panel, _cx| {
        assert!(panel.last_relations().is_empty());
        assert!(panel.current_relation_note().is_none());
    });
}

/// D38 的分权：**状态单点**（单表任务的进度在中央、集合任务的在右 Dock，同时刻只有一处）、
/// 清单状态点（生成 / 落库两套口径）、关系挂在**子表**行下、历史 / 模板默认收起。
#[gpui_kit::test]
fn the_table_list_keeps_status_and_progress_in_one_place(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    // 空闲：没有进度行；折叠段默认收起；草稿还没生成
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.job_row_scope(), None);
        assert!(!panel.fold_open, "历史 / 模板默认收起");
        assert_eq!(panel.table_status("mock_data"), TableStatus::Idle);
    });

    // 单表生成：进度归中央（`Single`）
    panel.update(cx, |panel, cx| {
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
        panel.run_generate(cx);
    });
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.job_row_scope(), Some(JobRowScope::Single));
        assert_eq!(panel.table_status("mock_data"), TableStatus::Generating);
    });
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.job_row_scope(), None);
        assert_eq!(panel.table_status("mock_data"), TableStatus::NotPersisted);
    });

    // 场景生成：进度归右 Dock（`Collection`），关系挂在**子表**行下
    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.job_row_scope(), Some(JobRowScope::Collection));
    });
    poll_job(cx, &panel);
    draw(cx);
    panel.update(cx, |panel, _cx| {
        let orders = panel.outgoing_relations("orders");
        assert_eq!(orders.len(), 1, "orders 有一条出边（挂在它那一行下）");
        assert_eq!(orders[0].parent_table, "users");
        assert!(panel.outgoing_relations("users").is_empty(), "父表没有出边");
        assert_eq!(
            panel.table_status("orders"),
            TableStatus::DanglingParent,
            "它引用的 users 还没落库"
        );
        assert_eq!(
            panel.table_status("items"),
            TableStatus::DanglingParent,
            "它引用的 orders 还没落库"
        );
    });

    // 落库 users（被 orders 引用）：orders 的悬空提示消失，users 自己转「已落库」
    panel.update(cx, |panel, cx| {
        panel.select_result(2, cx);
        panel.persist_table(cx);
    });
    poll_job(cx, &panel);
    draw(cx);
    panel.update(cx, |panel, _cx| {
        assert!(panel.landed_tables().iter().any(|table| table == "users"));
        assert_eq!(panel.table_status("users"), TableStatus::Persisted);
        assert_eq!(panel.table_status("orders"), TableStatus::NotPersisted);
        assert_eq!(
            panel.table_status("items"),
            TableStatus::DanglingParent,
            "items 的父表 orders 还没落库"
        );
    });
}

/// B7：生成器菜单的两条便利——「最近使用」（本会话点过的，最近在前、去重、只留几条）
/// 与「推荐」（与导入结构同一条智能映射推理，只用于标记，不写回配置）。
#[gpui_kit::test]
fn generator_menu_remembers_recent_picks_and_marks_the_recommendation(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.add_column("email".to_string(), ColumnDataType::Integer, cx);
        panel.add_column("amount".to_string(), ColumnDataType::Integer, cx);
    });
    draw(cx);

    panel.update(cx, |panel, _cx| {
        assert!(
            panel.recent_generators().is_empty(),
            "还没点过任何生成器：不给空的「最近使用」分组"
        );
        // 推荐与导入结构同源（`ColumnMapper::infer`）：列名命中 → 那个生成器
        assert_eq!(
            panel.recommended_generator("email", &ColumnDataType::Integer),
            "safe_email"
        );
        // 认不出的名字走类型兜底（整数 → 随机整数），与 schema_map 的用例同一口径
        assert_eq!(
            panel.recommended_generator("xyz_field", &ColumnDataType::Integer),
            "random_int"
        );
    });

    // 点两个不同的生成器，再点回第一个：最近的在前、同一个不重复
    let ids: Vec<u64> = panel.update(cx, |panel, _cx| {
        panel
            .draft()
            .columns
            .iter()
            .map(|column| column.id)
            .collect()
    });
    panel.update(cx, |panel, cx| {
        panel.set_generator(ids[0], "random_int", cx);
        panel.set_generator(ids[1], "digit", cx);
        panel.set_generator(ids[0], "random_int", cx);
    });
    draw(cx);
    panel.update(cx, |panel, _cx| {
        let recent: Vec<&str> = panel
            .recent_generators()
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(recent, ["random_int", "digit"]);
    });
}

/// 场景生成不记生成历史（历史是单表配置的重放来源）——面板上要说明，别让人以为是丢了。
#[gpui_kit::test]
fn scenario_runs_are_not_recorded_in_history(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);

    panel.update(cx, |panel, _cx| {
        assert!(
            panel.history.is_empty(),
            "场景生成不该往历史里塞单表记录：{:?}",
            panel.history.len()
        );
        assert!(
            panel.outcome().unwrap_or_default().contains("生成 3 张表"),
            "{:?}",
            panel.outcome()
        );
    });
}

/// 关系里还没落库的表要摆出来，并能一次依次落库（含当前表；已落库的跳过）。
#[gpui_kit::test]
fn landing_the_whole_relation_closure_in_one_go(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    // 默认录制器里已有一张 `orders`（供「同名已存在」用例）；这条要验全新落库
    *rec.tables.borrow_mut() = Vec::new();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);
    draw(cx);

    // 当前表 orders：关系闭包 = orders / users（引用了 users）+ items（被 items 的 order_id 引用）
    panel.update(cx, |panel, _cx| {
        assert_eq!(
            panel.pending_relation_tables(),
            [
                "orders".to_string(),
                "items".to_string(),
                "users".to_string()
            ],
            "按结果表顺序列出闭包里全部未落库的表"
        );
        assert!(panel.landed_tables().is_empty());
    });

    panel.update(cx, |panel, cx| panel.persist_related(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(
            rec.persisted.borrow().as_slice(),
            [
                "orders".to_string(),
                "items".to_string(),
                "users".to_string()
            ],
            "依次落库的顺序就是结果表顺序"
        );
        assert_eq!(
            panel.landed_tables(),
            [
                "orders".to_string(),
                "items".to_string(),
                "users".to_string()
            ]
        );
        assert!(
            panel.pending_relation_tables().is_empty(),
            "落完了就不再提示"
        );
        let outcome = panel.outcome().unwrap_or_default();
        assert!(outcome.contains("已落库 3 张"), "{outcome}");
        assert!(panel.error().is_none(), "{:?}", panel.error());
    });

    // 再点一次：没有待落库的表 → 可读拒绝（不空跑一个任务）
    let started = rec.started.borrow().len();
    panel.update(cx, |panel, cx| panel.persist_related(cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(rec.started.borrow().len(), started, "不该提交任务");
        assert!(
            panel.error().is_some_and(|e| e.contains("没有待落库")),
            "{:?}",
            panel.error()
        );
    });
}

/// 批量落库里的部分失败：**成功的保留**（不回滚别人的表），失败的原样报出。
#[gpui_kit::test]
fn batch_landing_keeps_the_successes_and_reports_the_failures(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    // 项目库里已经有一张 items：它应当失败，其余照落
    *rec.tables.borrow_mut() = vec!["items".to_string()];
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        panel.run_scenario(cx);
    });
    poll_job(cx, &panel);
    panel.update(cx, |panel, cx| panel.persist_related(cx));
    poll_job(cx, &panel);

    panel.update(cx, |panel, _cx| {
        assert_eq!(
            panel.landed_tables(),
            ["orders".to_string(), "users".to_string()],
            "成功的两张记下来"
        );
        let error = panel.error().unwrap_or_default();
        assert!(error.contains("失败 1 张"), "{error}");
        assert!(error.contains("items"), "{error}");
        assert!(
            panel
                .pending_relation_tables()
                .contains(&"items".to_string()),
            "失败的那张还在待落库清单里（可以修好再落）"
        );
    });
}

/// 自定义多表：把当前草稿加进场景工作副本（导入结构 / 列编辑都在单表态里做完）。
#[gpui_kit::test]
fn a_draft_can_be_added_to_the_scenario_as_another_table(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        // 草稿：一张用户自己调好的表
        panel.draft.table_name = "invoices".to_string();
        panel.draft.options.rows = 750;
        panel.add_column("id".to_string(), ColumnDataType::Integer, cx);
        panel.add_column("amount".to_string(), ColumnDataType::Text, cx);
        panel.add_draft_to_scenario(cx);
    });
    draw(cx);

    panel.update(cx, |panel, _cx| {
        let template = panel.scenario().expect("工作副本");
        assert_eq!(
            template
                .tables
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            ["orders", "items", "users", "invoices"],
            "新表加在末尾"
        );
        let added = template.tables.last().expect("新表");
        assert_eq!(added.row_count, 750, "行数取草稿");
        assert_eq!(
            added
                .columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            ["id", "amount"],
            "列取草稿"
        );
        let outcome = panel.outcome().unwrap_or_default();
        assert!(outcome.contains("已把草稿 invoices"), "{outcome}");
        assert!(outcome.contains("750 行 · 2 列"), "{outcome}");
    });

    // 重名：拒绝（不静默改掉那张表）
    panel.update(cx, |panel, cx| {
        panel.draft.table_name = "orders".to_string();
        panel.add_draft_to_scenario(cx);
    });
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.error().is_some_and(|e| e.contains("已经有表 orders")),
            "{:?}",
            panel.error()
        );
        assert_eq!(panel.scenario().expect("工作副本").tables.len(), 4);
    });

    // 没列的草稿：拒绝（生不出东西的表加进去只会拖累整次生成）
    panel.update(cx, |panel, cx| {
        panel.draft.table_name = "empty_one".to_string();
        panel.draft.columns.clear();
        panel.add_draft_to_scenario(cx);
    });
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.error().is_some_and(|e| e.contains("还没有列")),
            "{:?}",
            panel.error()
        );
    });

    // 加进来的表能一起生成：提交的是含 4 张表的工作副本
    panel.update(cx, |panel, cx| panel.run_scenario(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.results().len(), 4, "4 张表都回来了");
        assert!(
            panel
                .results()
                .iter()
                .any(|info| info.table_name == "invoices")
        );
    });
}

/// 删表会**连带清掉指向它的关系**：否则剩余引用变成「指向模板里没有的表」，
/// 用户到生成前才看到错误，那时已经不知道是谁指向它了。
#[gpui_kit::test]
fn removing_a_scenario_table_drops_the_references_pointing_at_it(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.load_scenario(toy_scenario(), cx);
        // toy：`orders.user_id → users.id`（orders 的**出边**）与
        // `items.order_id → orders.id`（指向 orders 的**入边**）
        assert_eq!(panel.scenario_relations().len(), 2);
        panel.remove_scenario_table("orders", cx);
    });
    draw(cx);

    panel.update(cx, |panel, _cx| {
        let template = panel.scenario().expect("工作副本");
        assert_eq!(
            template
                .tables
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            ["items", "users"]
        );
        assert!(
            panel.scenario_relations().is_empty(),
            "出边随表一起消失，入边必须清掉：{:?}",
            panel.scenario_relations()
        );
        let outcome = panel.outcome().unwrap_or_default();
        assert!(
            outcome.contains("清掉 1 条"),
            "只有指向它的那一条需要清：{outcome}"
        );
        assert!(panel.error().is_none(), "{:?}", panel.error());
    });

    // 删完仍能生成（不再有任何引用，校验通过）
    panel.update(cx, |panel, cx| panel.run_scenario(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.results().len(), 2);
        assert!(panel.error().is_none(), "{:?}", panel.error());
    });

    // 删不存在的表：无声忽略（UI 上不可能发生，测试桥直接调才可能）
    panel.update(cx, |panel, cx| panel.remove_scenario_table("nope", cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.scenario().expect("工作副本").tables.len(), 2);
    });
}

// ==================== 编辑表（改表名 / 行数） ====================

/// 打开「编辑表」对话框并写进两个输入框（对话框状态由面板持有，测试直接写）。
///
/// 先关掉还开着的：校验失败的用例会**故意**把它留着，用例之间不该叠罗汉。
fn open_table_edit_dialog(
    panel: &Entity<MockPanel>,
    table: &str,
    name: &str,
    rows: &str,
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        if window.has_active_dialog(cx) {
            window.close_dialog(cx);
        }
        panel.update(cx, |panel, cx| panel.open_table_dialog(table, window, cx));
        assert!(window.has_active_dialog(cx), "编辑表对话框应打开");
        panel.update(cx, |panel, cx| {
            if let Some(input) = panel.edit_name_input.clone() {
                input.update(cx, |state, cx| state.set_value(name, window, cx));
            }
            if let Some(input) = panel.edit_rows_input.clone() {
                input.update(cx, |state, cx| state.set_value(rows, window, cx));
            }
        });
        window.draw(cx).clear(cx);
    });
}

/// 点「应用」：与按钮同一条件（`table_editing` 清空＝校验通过 → 收起对话框）。
fn click_table_edit_apply(panel: &Entity<MockPanel>, cx: &mut VisualTestContext) -> bool {
    let applied = panel.update(cx, |panel, cx| {
        panel.apply_table_edit(cx);
        panel.table_editing.is_none()
    });
    if applied {
        cx.update(|window, cx| window.close_dialog(cx));
    }
    applied
}

/// `items.order_id → orders.id` 那一条（`relation_range` 的观察对象）。
fn items_reference(panel: &MockPanel) -> ScenarioRelation {
    panel
        .scenario_relations()
        .into_iter()
        .find(|relation| relation.child_table == "items")
        .expect("items 的引用")
}

/// 改行数：工作副本跟着变，引用它的关系的**取值域自动跟着变**——域由父表行数算出，
/// 用户不需要再去同步任何东西（这是这次编辑最直接的可见回报）。
#[gpui_kit::test]
fn editing_a_table_row_count_moves_the_reference_domain(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| panel.load_scenario(toy_scenario(), cx));
    draw(cx);

    // 改前：orders 100 行 → 域 1..100
    panel.update(cx, |panel, _cx| {
        assert_eq!(
            panel.relation_range(&items_reference(panel)).as_deref(),
            Some("1..100")
        );
    });

    open_table_edit_dialog(&panel, "orders", "orders", "750", cx);
    assert!(click_table_edit_apply(&panel, cx), "校验通过就该收起对话框");
    draw(cx);

    panel.update(cx, |panel, _cx| {
        let template = panel.scenario().expect("工作副本");
        let orders = template
            .tables
            .iter()
            .find(|t| t.name == "orders")
            .expect("orders 还在");
        assert_eq!(orders.row_count, 750);
        assert_eq!(
            panel.relation_range(&items_reference(panel)).as_deref(),
            Some("1..750"),
            "域跟着父表行数走"
        );
        let outcome = panel.outcome().unwrap_or_default();
        assert!(outcome.contains("已更新表 orders"), "{outcome}");
        assert!(outcome.contains("750 行"), "{outcome}");
        assert!(panel.error().is_none(), "{:?}", panel.error());
    });

    // 改完照旧能生成（工作副本仍是唯一事实来源）
    panel.update(cx, |panel, cx| panel.run_scenario(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.results().len(), 3);
        assert!(panel.error().is_none(), "{:?}", panel.error());
    });

    // 父列不是自增（域算不出来）→ 不给域文案，面板那一行就不显示域
    panel.update(cx, |panel, _cx| {
        let template = panel.scenario.as_mut().expect("工作副本");
        if let Some(orders) = template.tables.iter_mut().find(|t| t.name == "orders") {
            orders.columns[0].generator = GeneratorConfig::UuidV4;
        }
        assert!(panel.relation_range(&items_reference(panel)).is_none());
    });
}

/// 改表名：指向它的**入边自动跟着改**——否则立刻就变成「指向模板里没有的表」，
/// 用户在生成前才发现，那时已经不知道是谁指向它了。
#[gpui_kit::test]
fn renaming_a_table_retargets_the_references_pointing_at_it(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| panel.load_scenario(toy_scenario(), cx));
    open_table_edit_dialog(&panel, "orders", "sales_orders", "100", cx);
    assert!(click_table_edit_apply(&panel, cx));
    draw(cx);

    panel.update(cx, |panel, _cx| {
        let template = panel.scenario().expect("工作副本");
        assert!(
            template.tables.iter().any(|t| t.name == "sales_orders"),
            "新名字在册"
        );
        assert!(
            !template.tables.iter().any(|t| t.name == "orders"),
            "旧名字不该留下"
        );
        let relation = items_reference(panel);
        assert_eq!(relation.parent_table, "sales_orders", "入边必须跟着改名");
        assert_eq!(
            panel.relation_range(&relation).as_deref(),
            Some("1..100"),
            "改名不动行数，域照旧"
        );
        assert_eq!(panel.scenario_relations().len(), 2, "两条关系都还在");
        let outcome = panel.outcome().unwrap_or_default();
        assert!(outcome.contains("同步 1 条"), "{outcome}");
    });

    // 改完照旧能生成：没有悬空引用
    panel.update(cx, |panel, cx| panel.run_scenario(cx));
    poll_job(cx, &panel);
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.results().len(), 3);
        assert!(
            panel
                .results()
                .iter()
                .any(|info| info.table_name == "sales_orders")
        );
        assert!(panel.error().is_none(), "{:?}", panel.error());
    });
}

/// 校验没过就**不收起对话框**（错误就地显示，改完能接着点「应用」）。
#[gpui_kit::test]
fn table_edit_rejections_keep_the_dialog_open(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| panel.load_scenario(toy_scenario(), cx));

    // 空表名
    open_table_edit_dialog(&panel, "orders", "  ", "100", cx);
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.error().is_none(),
            "刚打开：上一次的错误不该还在（{:?}）",
            panel.error()
        );
    });
    assert!(!click_table_edit_apply(&panel, cx), "空表名不该收起对话框");
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("表名不能为空"));
        assert!(panel.table_editing.is_some(), "还在这张表上接着改");
    });

    // 撞上本次生成里已有的表名
    open_table_edit_dialog(&panel, "orders", "items", "100", cx);
    assert!(!click_table_edit_apply(&panel, cx));
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.error().is_some_and(|e| e.contains("已经有表 items")),
            "{:?}",
            panel.error()
        );
        assert_eq!(panel.scenario().expect("工作副本").tables.len(), 3);
    });

    // 行数非法：工作副本原样不动
    open_table_edit_dialog(&panel, "orders", "orders", "0", cx);
    assert!(!click_table_edit_apply(&panel, cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("行数需为正整数"));
        let orders = panel
            .scenario()
            .expect("工作副本")
            .tables
            .iter()
            .find(|t| t.name == "orders")
            .expect("orders 还在");
        assert_eq!(orders.row_count, 100, "原行数保留");
        assert!(panel.outcome().is_none());
    });

    // 表名不是合法标识符：与单表路径同一条规矩（非法名不留到生成时才报）
    open_table_edit_dialog(&panel, "orders", "orders-1", "100", cx);
    assert!(!click_table_edit_apply(&panel, cx));
    panel.update(cx, |panel, _cx| {
        assert_eq!(panel.error(), Some("表名只能包含字母、数字与下划线"));
        assert!(
            panel
                .scenario()
                .expect("工作副本")
                .tables
                .iter()
                .any(|t| t.name == "orders"),
            "非法名不写进工作副本"
        );
    });

    // 四次都没收起，且渲染不炸（错误行就地显示）
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx), "校验没过不要收起对话框");
        window.draw(cx).clear(cx);
    });
}
