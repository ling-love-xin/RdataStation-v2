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
    HistoryReply, MockColumnSpec, MockDetailView, MockDraft, MockGenInfo, MockHost, MockJobDone,
    MockJobKind, MockJobPhase, MockJobProgress, MockJobState, MockPanel, MockPreview,
    MockRunOptions, SchemaRequest, SchemaSource, focus_detail_tab, param_text, parse_percent_ratio,
    parse_rows, parse_seed, patch_param, search_generators, summarize_params, validate_table_name,
};
use crate::generator_catalog::{self, ParamKind};
use crate::models::{ColumnDataType, ColumnDef, GeneratorConfig, Locale, MockExportFormat};
use crate::persistence::{MockGenerationColumn, MockGenerationDetail, MockGenerationTask};
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
    assert_eq!(hits.len(), 137);
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

// ==================== 测试宿主桥 ====================

#[derive(Default)]
struct Recorder {
    notifies: Cell<usize>,
    opened_detail: Cell<usize>,
    /// 提交的后台任务（种类）
    started: RefCell<Vec<MockJobKind>>,
    /// 取消请求次数
    cancels: Cell<usize>,
    /// (目标表, 行数, 种子, 追加目标)
    generated: RefCell<Vec<(String, u32, Option<u32>, Option<String>)>>,
    persisted: RefCell<Vec<String>>,
    appended: RefCell<Vec<String>>,
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
            temp_table_name: format!("temp_mock_{}", draft.table_name),
            row_count: draft.options.rows,
            elapsed_ms: 7,
            preview: MockPreview {
                columns: draft.columns.iter().map(|c| c.def.name.clone()).collect(),
                rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            },
        }
    }

    /// 出口：新建表（同名已存在 → 报错，与真实装配层同一语义）。
    fn persist(&self, draft: &MockDraft, info: &MockGenInfo) -> Result<MockJobDone, String> {
        self.rec
            .persisted
            .borrow_mut()
            .push(draft.table_name.clone());
        self.rec
            .sink_temps
            .borrow_mut()
            .push(info.temp_table_name.clone());
        if self
            .rec
            .tables
            .borrow()
            .iter()
            .any(|t| t == &draft.table_name)
        {
            return Err(format!(
                "分析库已存在表 {}：请改用「追加到既有表」",
                draft.table_name
            ));
        }
        Ok(MockJobDone::Persisted {
            table: draft.table_name.clone(),
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
            MockJobKind::Persist(info) => self.persist(draft, info),
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

    fn open_detail(&self, _window: &mut Window, _cx: &mut App) {
        self.rec.opened_detail.set(self.rec.opened_detail.get() + 1);
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
        let detail = cx.new(|cx| MockDetailView::new(panel.clone(), cx));
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
        assert!(outcome.contains("已生成 1000 行"), "{outcome}");
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
                .is_some_and(|o| o.contains("已在分析库新建表 mock_data")),
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
                .is_some_and(|o| o.contains("已追加到 orders（表内共 1100 行）")),
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
        assert_eq!(panel.error(), Some("只读模式：不允许写入分析库"));
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
        assert_eq!(list.read(cx).delegate().hits().len(), 137);

        // 与 `List` 搜索框同一入口：改 query 触发 `perform_search`
        list.update(cx, |state, cx| state.set_query("电话", window, cx));
        let hits = list.read(cx).delegate().hits();
        assert!(hits.len() < 137 && !hits.is_empty(), "应过滤到少数项");
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
        let other = cx.new(|cx| MockDetailView::new(panel.clone(), cx));
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

/// 出口（落库）也是后台任务：提交即返回、进行中报「写入分析库」阶段，完成后预览仍可用。
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
            panel.outcome().is_some_and(|o| o.contains("写入分析库中")),
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
                .is_some_and(|o| o.contains("已在分析库新建表 mock_data（5 行）")),
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
        error_message: (status != "success").then(|| "分析库已存在表 orders".to_string()),
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
            Ok(HistoryReply::Listed(vec![
                history_task("t_ok", "success"),
                history_task("t_bad", "failed"),
            ])),
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
            Some("分析库已存在表 orders")
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

/// 删除失败（如未打开项目）时，列表不变且错误可见。
#[gpui_kit::test]
fn delete_history_keeps_the_list_and_reports_failure(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    panel.update(cx, |panel, cx| {
        panel.accept_history(
            Ok(HistoryReply::Listed(vec![history_task("t1", "success")])),
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
