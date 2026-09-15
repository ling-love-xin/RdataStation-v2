//! `mock_view` 的测试：纯逻辑（无窗口）+ 窗口级（GPUI headless）。
//!
//! 宿主能力全部用测试桥（记录调用，不接 workbench 与真实分析库）。
//!
//! 注意：这里**不使用** `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的
//! `test` 属性宏带入作用域，`#[gpui_kit::test]` 展开出的裸 `#[test]` 会解析到它自己，
//! 造成无限递归（recursion limit reached）。所有依赖显式列举。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::{
    App, AppContext as _, Entity, IntoElement, ParentElement, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};

use super::{
    MockColumnSpec, MockDetailView, MockDraft, MockGenInfo, MockHost, MockPanel, MockPreview,
    MockRunOptions, SchemaRequest, SchemaSource, param_text, parse_percent_ratio, parse_rows,
    parse_seed, patch_param, summarize_params, validate_table_name,
};
use crate::generator_catalog::ParamKind;
use crate::models::{ColumnDataType, ColumnDef, GeneratorConfig, Locale, MockExportFormat};
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
    assert_eq!(validate_table_name("mock_data2"), Ok("mock_data2".to_string()));
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

// ==================== 测试宿主桥 ====================

#[derive(Default)]
struct Recorder {
    notifies: Cell<usize>,
    opened_detail: Cell<usize>,
    /// (目标表, 行数, 种子, 追加目标)
    generated: RefCell<Vec<(String, u32, Option<u32>, Option<String>)>>,
    persisted: RefCell<Vec<String>>,
    appended: RefCell<Vec<String>>,
    exported: RefCell<Vec<(String, String)>>,
    scratchpads: RefCell<Vec<String>>,
    imports: RefCell<Vec<SchemaRequest>>,
    read_only: Cell<bool>,
    tables: RefCell<Vec<String>>,
    sources: RefCell<Vec<SchemaSource>>,
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

impl MockHost for TestHost {
    fn generate(
        &self,
        draft: &MockDraft,
        append_to: Option<&str>,
    ) -> Result<MockGenInfo, String> {
        self.rec.generated.borrow_mut().push((
            draft.table_name.clone(),
            draft.options.rows,
            draft.options.seed,
            append_to.map(|s| s.to_string()),
        ));
        Ok(MockGenInfo {
            temp_table_name: format!("temp_mock_{}", draft.table_name),
            row_count: draft.options.rows,
            elapsed_ms: 7,
            preview: MockPreview {
                columns: draft.columns.iter().map(|c| c.def.name.clone()).collect(),
                rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            },
        })
    }

    fn persist_table(&self, draft: &MockDraft, _info: &MockGenInfo) -> Result<i64, String> {
        self.rec.persisted.borrow_mut().push(draft.table_name.clone());
        if self.rec.tables.borrow().iter().any(|t| t == &draft.table_name) {
            return Err(format!(
                "分析库已存在表 {}：请改用「追加到既有表」",
                draft.table_name
            ));
        }
        Ok(5)
    }

    fn append_table(
        &self,
        _draft: &MockDraft,
        info: &MockGenInfo,
        table: &str,
    ) -> Result<i64, String> {
        self.rec.appended.borrow_mut().push(table.to_string());
        Ok(100 + info.row_count as i64)
    }

    fn export_file(
        &self,
        _draft: &MockDraft,
        _info: &MockGenInfo,
        format: &MockExportFormat,
        path: &str,
    ) -> Result<String, String> {
        self.rec
            .exported
            .borrow_mut()
            .push((format!("{format:?}"), path.to_string()));
        Ok(format!("已导出：{path}"))
    }

    fn save_scratchpad(
        &self,
        _draft: &MockDraft,
        _info: &MockGenInfo,
        format: &MockExportFormat,
    ) -> Result<String, String> {
        self.rec
            .scratchpads
            .borrow_mut()
            .push(format!("{format:?}"));
        Ok("已保存到草稿箱：/proj/mock/mock_x.csv".to_string())
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
    draw(cx);

    panel.update(cx, |panel, _cx| {
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
        assert_eq!(
            panel.error(),
            Some("请先添加列：导入源库结构，或手工加列")
        );
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

    // 生成后落库 → 新建表
    panel.update(cx, |panel, cx| panel.run_generate(cx));
    panel.update(cx, |panel, cx| panel.persist_table(cx));
    panel.update(cx, |panel, _cx| {
        assert!(
            panel.outcome().is_some_and(|o| o.contains("已在分析库新建表 mock_data")),
            "{:?}",
            panel.outcome()
        );
        assert_eq!(panel.landed(), Some("mock_data"));
    });

    // 同名已存在 → 报错并引导「追加」
    *rec.tables.borrow_mut() = vec!["mock_data".to_string()];
    panel.update(cx, |panel, cx| panel.persist_table(cx));
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
}

#[gpui_kit::test]
fn column_edits_track_draft_and_invalidate_result(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (panel, _detail, cx) = open_harness(cx, test_host(&rec));

    draw(cx);
    panel.update(cx, |panel, cx| {
        panel.add_column("c1".to_string(), ColumnDataType::Integer, cx);
        panel.add_column("c2".to_string(), ColumnDataType::Varchar { length: None }, cx);
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
        panel.add_column("email".to_string(), ColumnDataType::Varchar { length: None }, cx);
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
        panel.add_column("email".to_string(), ColumnDataType::Varchar { length: None }, cx);
    });
    panel.update(cx, |panel, cx| panel.run_generate(cx));
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

/// `focus_tab` 未加入 Dock 时应静默返回（不 panic）。
#[gpui_kit::test]
fn focus_tab_without_group_is_noop(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let rec = recorder();
    let (_panel, detail, cx) = open_harness(cx, test_host(&rec));

    cx.update(|window, cx| {
        detail.update(cx, |view, cx| view.focus_tab(window, cx));
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
