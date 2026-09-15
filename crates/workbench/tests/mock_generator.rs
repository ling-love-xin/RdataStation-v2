//! M7 Mock 装配层集成测试：生成 → 落库（新建 / 追加）/ 导出的端到端语义。
//!
//! 全部走显式路径入口（`*_at`）：生产入口恒取全局分析库，测试用临时库文件隔离。

use std::path::PathBuf;

use mock::mock_view::{MockColumnSpec, MockDraft, MockRunOptions};
use mock::models::{ColumnDataType, ColumnDef, GeneratorConfig, Locale, MockExportFormat};
use mock::parse_data_type;
use rds_workbench::services::mock_generator::{
    append_table_at, existing_tables_at, export_file, generate_at, persist_table_at,
    save_scratchpad,
};

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rds_mg_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn column(name: &str, generator: GeneratorConfig) -> MockColumnSpec {
    MockColumnSpec {
        id: 0,
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

/// 三列草稿：自增主键 + 随机整数 + 文本（覆盖自增接续这条关键语义）。
fn draft(table: &str, rows: u32) -> MockDraft {
    MockDraft {
        table_name: table.to_string(),
        columns: vec![
            column("id", GeneratorConfig::AutoIncrement { start: 1, step: 1 }),
            column("amount", GeneratorConfig::RandomInt { min: 1, max: 100 }),
            MockColumnSpec {
                def: ColumnDef {
                    name: "status".to_string(),
                    data_type: ColumnDataType::Varchar { length: None },
                    generator: GeneratorConfig::Constant {
                        value: "new".to_string(),
                    },
                    nullable_ratio: 0.0,
                    unique: false,
                    dependency: None,
                },
                ..column("status", GeneratorConfig::Constant {
                    value: "new".to_string(),
                })
            },
        ],
        options: MockRunOptions::new(rows, Some(42), Locale::ZhCn),
    }
}

fn count_rows(db: &PathBuf, table: &str) -> i64 {
    let conn = duckdb::Connection::open(db).expect("open duckdb");
    conn.query_row(&format!("SELECT COUNT(*) FROM \"{table}\""), [], |r| {
        r.get(0)
    })
    .expect("count rows")
}

#[test]
fn generate_does_not_write_analysis_db() {
    let dir = temp_dir("gen_only");
    let db = dir.join("analytics.duckdb");

    let draft = draft("t_gen_only", 50);
    let info = generate_at(&db, &draft, None).expect("generate");
    assert_eq!(info.row_count, 50);
    assert_eq!(info.temp_table_name, "temp_mock_t_gen_only");
    assert_eq!(info.preview.columns, ["id", "amount", "status"]);
    assert!(!info.preview.rows.is_empty(), "应带预览");

    // 关键语义：生成只产临时表，分析库文件里不应出现该表
    assert!(
        existing_tables_at(&db).is_empty(),
        "生成不应在分析库建表: {:?}",
        existing_tables_at(&db)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn persist_creates_table_and_rejects_second_run() {
    let dir = temp_dir("persist");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_persist", 50);

    let info = generate_at(&db, &draft, None).expect("generate");
    let rows = persist_table_at(&db, &draft, &info).expect("persist");
    assert_eq!(rows, 50);
    assert_eq!(count_rows(&db, "t_persist"), 50);
    assert_eq!(
        existing_tables_at(&db),
        vec!["t_persist".to_string()],
        "落库后应出现在既有表清单里"
    );

    // 二次落库：不覆盖，报错并给出「追加」引导
    let err = persist_table_at(&db, &draft, &info).expect_err("应拒绝同名建表");
    assert!(err.contains("已存在"), "err: {err}");
    assert!(err.contains("追加"), "err: {err}");
    assert_eq!(count_rows(&db, "t_persist"), 50, "既有数据不应被覆盖");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn append_continues_primary_key_sequence() {
    let dir = temp_dir("append");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_append", 50);

    // 首次建表
    let first = generate_at(&db, &draft, None).expect("generate");
    persist_table_at(&db, &draft, &first).expect("persist");

    // 追加：生成时按表内行数接续自增起点，再插入
    let second = generate_at(&db, &draft, Some("t_append")).expect("regenerate for append");
    let total = append_table_at(&db, &draft, &second, "t_append").expect("append");
    assert_eq!(total, 100, "追加后表内应累计 100 行");

    let conn = duckdb::Connection::open(&db).expect("open duckdb");
    let distinct: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT id) FROM t_append",
            [],
            |r| r.get(0),
        )
        .expect("count distinct ids");
    let max_id: i64 = conn
        .query_row("SELECT MAX(id) FROM t_append", [], |r| r.get(0))
        .expect("max id");
    assert_eq!(distinct, 100, "主键不应重复（自增起点接续表内行数）");
    assert_eq!(max_id, 100, "最大主键应为 100");
    drop(conn);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn append_to_unknown_table_errors() {
    let dir = temp_dir("append_missing");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_append_missing", 10);

    // 生成需要目标表现有行数：表不存在 → 可读报错，不静默写入
    let err = generate_at(&db, &draft, Some("nope")).expect_err("应报错");
    assert!(err.contains("分析库没有表 nope"), "err: {err}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn append_rejects_column_mismatch() {
    let dir = temp_dir("append_mismatch");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_mismatch", 10);

    let info = generate_at(&db, &draft, None).expect("generate");
    persist_table_at(&db, &draft, &info).expect("persist");

    // 目标表缺少草稿里的列 → 报缺失列名（不让 DuckDB 原始错误冒到界面）
    let mut narrowed = draft.clone();
    narrowed.columns.push(column("extra", GeneratorConfig::Digit));
    let info = generate_at(&db, &narrowed, Some("t_mismatch")).expect("regenerate");
    let err = append_table_at(&db, &narrowed, &info, "t_mismatch").expect_err("应报缺列");
    assert!(err.contains("缺少列"), "err: {err}");
    assert!(err.contains("extra"), "err: {err}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generate_without_columns_errors() {
    let dir = temp_dir("empty");
    let db = dir.join("analytics.duckdb");

    let empty = MockDraft {
        table_name: "empty_t".to_string(),
        columns: Vec::new(),
        options: MockRunOptions::new(10, Some(42), Locale::ZhCn),
    };
    let err = generate_at(&db, &empty, None).expect_err("应报错");
    assert!(err.contains("没有可用的列"), "err: {err}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_writes_csv_with_header() {
    let dir = temp_dir("export");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_export", 20);
    let info = generate_at(&db, &draft, None).expect("generate");

    let csv = dir.join("t_export.csv");
    let message = export_file(
        &draft,
        &info,
        &MockExportFormat::Csv,
        csv.to_str().expect("utf-8 path"),
    )
    .expect("export csv");
    assert!(message.contains("已导出"), "{message}");

    let text = std::fs::read_to_string(&csv).expect("read csv");
    assert!(text.starts_with("id,amount,status"), "首行应为表头: {text}");
    assert_eq!(text.lines().count(), 21, "表头 + 20 行");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scratchpad_without_project_errors() {
    let dir = temp_dir("scratch_none");
    let db = dir.join("analytics.duckdb");
    let draft = draft("t_scratch", 5);
    let info = generate_at(&db, &draft, None).expect("generate");

    let err = save_scratchpad(&draft, &info, &MockExportFormat::Csv, None).expect_err("应报错");
    assert!(err.contains("未打开项目"), "err: {err}");

    // 有项目根：写入 {项目}/mock/mock_<表>_<时间戳>.csv
    let message = save_scratchpad(&draft, &info, &MockExportFormat::Csv, Some(&dir))
        .expect("save to scratchpad");
    assert!(message.contains("已保存到草稿箱"), "{message}");
    let mock_dir = dir.join("mock");
    let saved: Vec<String> = std::fs::read_dir(&mock_dir)
        .expect("mock dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(saved.len(), 1, "{saved:?}");
    assert!(saved[0].starts_with("mock_t_scratch_"), "{saved:?}");
    assert!(saved[0].ends_with(".csv"), "{saved:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn schema_sources_carry_connection_defaults() {
    let item = rds_workbench::view::ConnectionItem {
        id: "G_pg".to_string(),
        name: "生产库".to_string(),
        driver: "postgresql".to_string(),
        connected: true,
        host: Some("db.local".to_string()),
        port: Some(5432),
        database: Some("shop".to_string()),
        schema: Some("public".to_string()),
        description: None,
        use_duckdb_fed: false,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let sources = rds_workbench::services::mock_generator::schema_sources(&[item]);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].conn_id, "G_pg");
    assert_eq!(sources[0].label, "生产库");
    assert_eq!(sources[0].catalog, "shop", "对话框预填库名");
    assert_eq!(sources[0].schema, "public", "对话框预填 schema");
}

#[test]
fn parse_data_type_maps_common_types() {
    // 类型串解析的权威在 mock crate（`schema_map`）；本用例锁住它对导航树类型的表现。
    assert!(matches!(
        parse_data_type("INTEGER"),
        ColumnDataType::Integer
    ));
    assert!(matches!(
        parse_data_type("VARCHAR(64)"),
        ColumnDataType::Varchar { .. }
    ));
    assert!(matches!(
        parse_data_type("DECIMAL(12,2)"),
        ColumnDataType::Decimal { .. }
    ));
    assert!(matches!(
        parse_data_type("BOOLEAN"),
        ColumnDataType::Boolean
    ));
    assert!(matches!(
        parse_data_type("TIMESTAMP"),
        ColumnDataType::Timestamp
    ));
    assert!(matches!(parse_data_type("WEIRD"), ColumnDataType::Text));
}
