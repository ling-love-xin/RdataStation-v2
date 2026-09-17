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
    let date = GeneratorConfig::DateTime {
        min: "2020-01-01".to_string(),
        max: "2025-12-31".to_string(),
    };
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
