//! Mock 引擎集成测试（`MockEngine` 公开 API 端到端）。
//!
//! 覆盖 v1 `backend/tests/mock_engine_tests.rs` 的设计意图，并补齐它从未覆盖的出口链路
//! （export / persist_as_asset / save_to_scratchpad / 场景模板）。
//!
//! **与 v1 那份测试的关系**：v1 文件停留在旧 API（断言 `response.confidence > 0.0`、
//! `response.generator.type_name()`、`preview.rows` 非空），迁移时已无法编译、也从未通过，
//! 故不照搬，改为按 v2 真实契约重写：
//! - `ColumnMappingResponse::confidence` 是 `"high"/"low"` 字符串，GeneratorConfig 无 `type_name()`；
//! - `read_preview` 只填 Arrow `batches`，`QueryResult::rows` 为空，取值须经
//!   `QueryResult::from_batches` 物化（见 `rows_of` 辅助函数）。
//!
//! 注意：`MockEngine` 的临时表落在 engine 的**进程级全局内存 DuckDB**，测试并行执行共享同一连接，
//! 因此每个用例使用互不相同的表名。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::sql::{ColumnDefInfo, SqlEngine};
use rds_mock::{
    ColumnDataType, ColumnDef, ColumnDependency, GeneratorConfig, Locale, MockConfig, MockEngine,
    MockExportFormat, ScenarioTemplate, TempTableWriteMode, TemplateTable, parse_data_type,
};
use shared::models::QueryResult;

// ==================== 配置构造 ====================

fn col(name: &str, data_type: ColumnDataType, generator: GeneratorConfig) -> ColumnDef {
    ColumnDef {
        name: name.to_string(),
        data_type,
        generator,
        nullable_ratio: 0.0,
        unique: false,
        dependency: None,
    }
}

fn auto_increment(name: &str) -> ColumnDef {
    ColumnDef {
        unique: true,
        ..col(
            name,
            ColumnDataType::Integer,
            GeneratorConfig::AutoIncrement { start: 1, step: 1 },
        )
    }
}

fn basic_config(table_name: &str) -> MockConfig {
    MockConfig {
        table_name: table_name.to_string(),
        row_count: 50,
        seed: Some(42),
        locale: Locale::ZhCn,
        columns: vec![
            auto_increment("id"),
            col(
                "username",
                ColumnDataType::Varchar { length: Some(50) },
                GeneratorConfig::Username,
            ),
            col(
                "email",
                ColumnDataType::Varchar { length: None },
                GeneratorConfig::Email,
            ),
        ],
    }
}

/// 把预览结果物化为行数据（`read_preview` 只填 Arrow `batches`，`rows` 为空）。
fn rows_of(result: &rds_mock::MockGenerateResult) -> Vec<Vec<shared::models::Value>> {
    QueryResult::from_batches(
        result.preview.columns.clone(),
        result.preview.batches.clone(),
    )
    .rows
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_mock_it_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录");
    dir
}

// ==================== 跨库直写（落库新路径） ====================

/// 目标库里的行数（用**新连接**读文件：数据真的落盘才算数）。
fn file_row_count(db: &std::path::Path, table: &str) -> i64 {
    let conn = duckdb::Connection::open(db).expect("打开目标库");
    let sql = SqlEngine::build_select(table, &["COUNT(*)"], None);
    conn.query_row(&sql, [], |row| row.get(0)).expect("计数")
}

/// 目标库里的列名（表不存在时报错）。
fn file_columns(db: &std::path::Path, table: &str) -> Vec<String> {
    let conn = duckdb::Connection::open(db).expect("打开目标库");
    let sql = SqlEngine::build_select_all(table, Some(0));
    let mut stmt = conn.prepare(&sql).expect("prepare");
    let _rows = stmt.query([]).expect("query");
    stmt.column_names().iter().map(|c| c.to_string()).collect()
}

/// 目标库里的表名（排序）。
fn file_tables(db: &std::path::Path) -> Vec<String> {
    let conn = duckdb::Connection::open(db).expect("打开目标库");
    let sql = SqlEngine::build_select(
        "information_schema.tables",
        &["table_name", "table_schema"],
        None,
    );
    let mut stmt = conn.prepare(&sql).expect("prepare");
    let mut names: Vec<String> = Vec::new();
    let mut rows = stmt.query([]).expect("query");
    while let Some(row) = rows.next().expect("next") {
        let schema: String = row.get(1).unwrap_or_default();
        if schema == "main" {
            names.push(row.get::<usize, String>(0).unwrap_or_default());
        }
    }
    names.sort();
    names
}

fn int_column(name: &str, unique: bool, nullable: bool) -> ColumnDefInfo {
    ColumnDefInfo {
        name: name.to_string(),
        data_type: "INTEGER".to_string(),
        unique,
        nullable,
    }
}

/// 直写建表：数据用新连接读回来（含中文列名，锁住标识符加引号）。
#[tokio::test]
async fn write_temp_table_creates_table_in_file_database() {
    let dir = temp_dir("sink_create");
    let db = dir.join("analytics.duckdb");
    let result = MockEngine::generate(MockConfig {
        table_name: "t_sink_create".to_string(),
        row_count: 30,
        seed: Some(7),
        locale: Locale::ZhCn,
        columns: vec![
            auto_increment("id"),
            col(
                "金额",
                ColumnDataType::Integer,
                GeneratorConfig::RandomInt { min: 1, max: 9 },
            ),
        ],
    })
    .await
    .expect("生成应当成功");

    MockEngine::write_temp_table_to_database(
        &db,
        &result.temp_table_name,
        "t_sink_create",
        TempTableWriteMode::Create(vec![
            int_column("id", true, false),
            int_column("金额", false, true),
        ]),
    )
    .expect("直写建表应当成功");

    assert_eq!(file_row_count(&db, "t_sink_create"), 30);
    assert_eq!(file_columns(&db, "t_sink_create"), ["id", "金额"]);

    let _ = std::fs::remove_dir_all(&dir);
}

/// 直写追加：既有行保留，新行接在后面。
///
/// 用非唯一列：自增主键的接续（重算起点）是**装配层**的语义，
/// 已在 `crates/workbench/tests/mock_generator.rs::append_continues_primary_key_sequence` 覆盖。
#[tokio::test]
async fn write_temp_table_appends_to_existing_table() {
    let dir = temp_dir("sink_append");
    let db = dir.join("analytics.duckdb");
    let config = || MockConfig {
        table_name: "t_sink_append".to_string(),
        row_count: 10,
        seed: Some(3),
        locale: Locale::ZhCn,
        columns: vec![col(
            "amount",
            ColumnDataType::Integer,
            GeneratorConfig::RandomInt { min: 1, max: 100 },
        )],
    };

    let first = MockEngine::generate(config()).await.expect("首先生成");
    MockEngine::write_temp_table_to_database(
        &db,
        &first.temp_table_name,
        "t_sink_append",
        TempTableWriteMode::Create(vec![int_column("amount", false, true)]),
    )
    .expect("建表应当成功");
    assert_eq!(file_row_count(&db, "t_sink_append"), 10);

    // 同名目标表再生成一次（临时表被重建为新内容）→ 追加
    let second = MockEngine::generate(config()).await.expect("再次生成");
    MockEngine::write_temp_table_to_database(
        &db,
        &second.temp_table_name,
        "t_sink_append",
        TempTableWriteMode::Append,
    )
    .expect("追加应当成功");
    assert_eq!(
        file_row_count(&db, "t_sink_append"),
        20,
        "既有 10 行不应被覆盖"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 同名表已存在时用 `Create` 模式：报错，且**既有表与数据都留着**（回滚分支只删自己刚建的表）。
#[tokio::test]
async fn write_temp_table_create_on_existing_table_keeps_data() {
    let dir = temp_dir("sink_taken");
    let db = dir.join("analytics.duckdb");
    let config = || MockConfig {
        table_name: "t_sink_taken".to_string(),
        row_count: 12,
        seed: Some(5),
        locale: Locale::ZhCn,
        columns: vec![auto_increment("id")],
    };

    let first = MockEngine::generate(config()).await.expect("首先生成");
    MockEngine::write_temp_table_to_database(
        &db,
        &first.temp_table_name,
        "t_sink_taken",
        TempTableWriteMode::Create(vec![int_column("id", true, false)]),
    )
    .expect("建表应当成功");

    let again = MockEngine::generate(config()).await.expect("再次生成");
    let err = MockEngine::write_temp_table_to_database(
        &db,
        &again.temp_table_name,
        "t_sink_taken",
        TempTableWriteMode::Create(vec![int_column("id", true, false)]),
    )
    .expect_err("同名建表应当失败");
    assert!(!err.to_string().is_empty(), "应有可读错误");
    assert_eq!(
        file_row_count(&db, "t_sink_taken"),
        12,
        "既有表的数据不应被回滚分支删掉"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 写入失败（建表成功但插入失败）→ 回滚刚建的空表，且**失败后仍能继续写同一个库**（证明已解挂）。
#[tokio::test]
async fn write_temp_table_rolls_back_and_detaches_after_failure() {
    let dir = temp_dir("sink_rollback");
    let db = dir.join("analytics.duckdb");
    let result = MockEngine::generate(MockConfig {
        table_name: "t_sink_rollback".to_string(),
        row_count: 8,
        seed: Some(11),
        locale: Locale::ZhCn,
        columns: vec![
            auto_increment("id"),
            col(
                "note",
                ColumnDataType::Varchar { length: Some(16) },
                GeneratorConfig::Word,
            ),
        ],
    })
    .await
    .expect("生成应当成功");

    // 列定义故意少一列：建表能过，INSERT SELECT 会在「列不存在」上失败
    let err = MockEngine::write_temp_table_to_database(
        &db,
        &result.temp_table_name,
        "t_sink_rollback",
        TempTableWriteMode::Create(vec![int_column("id", false, true)]),
    )
    .expect_err("插入列对不上应当失败");
    assert!(!err.to_string().is_empty(), "应有可读错误");
    assert!(
        !file_tables(&db).contains(&"t_sink_rollback".to_string()),
        "半成品空表应被回滚: {:?}",
        file_tables(&db)
    );

    // 失败后同一文件仍可用（说明失败路径也 DETACH 了）
    MockEngine::write_temp_table_to_database(
        &db,
        &result.temp_table_name,
        "t_sink_rollback",
        TempTableWriteMode::Create(vec![
            int_column("id", false, true),
            ColumnDefInfo {
                name: "note".to_string(),
                data_type: "VARCHAR".to_string(),
                unique: false,
                nullable: true,
            },
        ]),
    )
    .expect("失败后应能重新挂载并写入");
    assert_eq!(file_row_count(&db, "t_sink_rollback"), 8);

    let _ = std::fs::remove_dir_all(&dir);
}

// ==================== 生成 ====================

#[tokio::test]
async fn generate_basic_fields() {
    let result = MockEngine::generate(basic_config("t_basic"))
        .await
        .expect("生成应当成功");

    assert_eq!(result.table_name, "t_basic");
    assert_eq!(result.row_count, 50);
    assert_eq!(result.columns.len(), 3);
    assert_eq!(result.temp_table_name, "temp_mock_t_basic");
    // 耗时字段只做合理性断言（毫秒级生成可能为 0，不断言 > 0 以免抖动）。
    assert!(
        result.elapsed_ms <= 60_000,
        "耗时异常: {}",
        result.elapsed_ms
    );
}

#[tokio::test]
async fn generate_preview_has_expected_shape() {
    let result = MockEngine::generate(basic_config("t_preview"))
        .await
        .expect("生成应当成功");

    let preview = &result.preview;
    assert_eq!(preview.columns.len(), 3, "预览列数应与定义一致");
    assert_eq!(preview.batches.len(), 1, "预览应为单批次");
    // 预览取前 10 行（PREVIEW_ROWS），不超过请求行数。
    assert_eq!(preview.batches[0].num_rows(), 10);

    let rows = rows_of(&result);
    assert_eq!(rows.len(), 10);
    // id 列单调递增（AutoIncrement + unique）。
    let ids: Vec<i64> = rows
        .iter()
        .map(|r| r[0].as_int().expect("id 应为整数"))
        .collect();
    assert!(
        ids.windows(2).all(|w| w[0] < w[1]),
        "id 应严格递增: {:?}",
        ids
    );
    // username 列在中文 locale 下应可读为文本而非 NULL。
    assert!(!rows[0][1].is_null(), "username 不应为 NULL");
}

#[tokio::test]
async fn generate_rejects_empty_columns() {
    let config = MockConfig {
        table_name: "t_empty".to_string(),
        row_count: 10,
        seed: None,
        locale: Locale::ZhCn,
        columns: vec![],
    };
    assert!(
        MockEngine::generate(config).await.is_err(),
        "无列定义应报错"
    );
}

#[tokio::test]
async fn generate_rejects_zero_rows() {
    let config = MockConfig {
        table_name: "t_zero".to_string(),
        row_count: 0,
        seed: None,
        locale: Locale::ZhCn,
        columns: vec![auto_increment("id")],
    };
    assert!(MockEngine::generate(config).await.is_err(), "0 行应报错");
}

#[tokio::test]
async fn generate_rejects_nullable_ratio_out_of_range() {
    let mut column = auto_increment("id");
    column.nullable_ratio = 1.5;
    let config = MockConfig {
        table_name: "t_bad_ratio".to_string(),
        row_count: 5,
        seed: None,
        locale: Locale::ZhCn,
        columns: vec![column],
    };
    assert!(
        MockEngine::generate(config).await.is_err(),
        "nullable_ratio 超出 0.0~1.0 应报错"
    );
}

#[tokio::test]
async fn generate_nullable_ratio_produces_nulls() {
    let mut column = col(
        "col",
        ColumnDataType::Integer,
        GeneratorConfig::RandomInt { min: 1, max: 100 },
    );
    column.nullable_ratio = 1.0;
    let config = MockConfig {
        table_name: "t_nullable".to_string(),
        row_count: 100,
        seed: Some(1),
        locale: Locale::ZhCn,
        columns: vec![column],
    };

    let result = MockEngine::generate(config).await.expect("生成应当成功");
    let rows = rows_of(&result);
    assert!(!rows.is_empty());
    assert!(
        rows.iter().all(|r| r[0].is_null()),
        "nullable_ratio = 1.0 时全列应为 NULL"
    );
}

#[tokio::test]
async fn generate_is_reproducible_with_same_seed() {
    let first = MockEngine::generate(basic_config("t_seed_a"))
        .await
        .expect("生成应当成功");
    let second = MockEngine::generate(basic_config("t_seed_b"))
        .await
        .expect("生成应当成功");

    let a: Vec<String> = rows_of(&first)
        .iter()
        .map(|r| r[1].as_text().unwrap_or_default())
        .collect();
    let b: Vec<String> = rows_of(&second)
        .iter()
        .map(|r| r[1].as_text().unwrap_or_default())
        .collect();
    assert_eq!(a, b, "同一 seed 应产生相同序列");
}

// ==================== 预览 ====================

#[tokio::test]
async fn preview_after_generate_returns_rows() {
    let result = MockEngine::generate(basic_config("t_preview_again"))
        .await
        .expect("生成应当成功");

    let preview = MockEngine::preview(&result.temp_table_name, 20).expect("预览应当成功");
    assert_eq!(preview.columns.len(), 3);
    assert_eq!(preview.batches[0].num_rows(), 20, "limit 20 应返回 20 行");
}

#[tokio::test]
async fn preview_unknown_table_errors() {
    assert!(
        MockEngine::preview("temp_mock_does_not_exist", 10).is_err(),
        "预览不存在的表应报错"
    );
}

// ==================== 列映射 ====================

#[test]
fn map_column_matches_by_name() {
    let id = MockEngine::map_column("id", "INTEGER").expect("映射应当成功");
    assert!(matches!(
        id.generator,
        GeneratorConfig::AutoIncrement { .. }
    ));
    assert_eq!(id.confidence, "high");
    assert!(!id.sample_value.is_empty());

    let email = MockEngine::map_column("email", "VARCHAR").expect("映射应当成功");
    assert!(matches!(email.generator, GeneratorConfig::SafeEmail));

    let created_at = MockEngine::map_column("created_at", "TIMESTAMP").expect("映射应当成功");
    assert!(matches!(
        created_at.generator,
        GeneratorConfig::DateTime { .. }
    ));
}

/// 带精度/长度的源库类型必须解析到真实类型，而不是回退到文本生成器。
///
/// 列名刻意选无规则命中的 `zzz_metric`：函数只由类型兜底决定。
/// 若 `DECIMAL(12,2)` 被当成未知类型（旧实现会回退 `Varchar`）则会得到 `Sentence`，
/// 因此本用例是「类型串统一入口」的回归锁。
#[test]
fn map_column_resolves_parameterized_source_types() {
    let metric = MockEngine::map_column("zzz_metric", "DECIMAL(12,2)").expect("映射应当成功");
    assert!(
        matches!(metric.generator, GeneratorConfig::RandomDecimal { .. }),
        "DECIMAL(12,2) 应映射为小数生成器，实际: {:?}",
        metric.generator
    );
    assert_eq!(metric.confidence, "low", "类型兜底应为低置信度");

    let flag = MockEngine::map_column("zzz_flag", "BOOLEAN").expect("映射应当成功");
    assert!(matches!(flag.generator, GeneratorConfig::Boolean { .. }));

    let stamp = MockEngine::map_column("zzz_stamp", "TIMESTAMP").expect("映射应当成功");
    assert!(matches!(stamp.generator, GeneratorConfig::DateTime { .. }));
}

#[test]
fn map_columns_batch_returns_one_response_per_column() {
    let columns = vec![
        ("id".to_string(), "INTEGER".to_string()),
        ("name".to_string(), "VARCHAR".to_string()),
        ("created_at".to_string(), "DATETIME".to_string()),
        ("amount".to_string(), "NUMERIC".to_string()),
    ];
    let responses = MockEngine::map_columns_batch(columns).expect("批量映射应当成功");
    assert_eq!(responses.len(), 4);
    for response in &responses {
        assert!(!response.sample_value.is_empty());
        assert!(!response.confidence.is_empty());
    }
    assert_eq!(responses[0].column_name, "id");
}

// ==================== 取消 ====================

#[tokio::test]
async fn cancel_flag_is_reset_by_next_generate() {
    MockEngine::cancel();
    let result = MockEngine::generate(basic_config("t_cancel")).await;
    assert!(
        result.is_ok(),
        "每次生成前应重置取消标志: {:?}",
        result.err()
    );
}

// ==================== 数据类型 ====================

#[test]
fn column_data_type_to_duckdb_type() {
    assert_eq!(ColumnDataType::Integer.to_duckdb_type(), "INTEGER");
    assert_eq!(ColumnDataType::BigInt.to_duckdb_type(), "BIGINT");
    assert_eq!(ColumnDataType::Boolean.to_duckdb_type(), "BOOLEAN");
    assert_eq!(
        ColumnDataType::Varchar { length: Some(255) }.to_duckdb_type(),
        "VARCHAR(255)"
    );
    assert_eq!(
        ColumnDataType::Varchar { length: None }.to_duckdb_type(),
        "VARCHAR"
    );
    assert_eq!(
        ColumnDataType::Decimal {
            precision: 18,
            scale: 2
        }
        .to_duckdb_type(),
        "DECIMAL(18, 2)"
    );
    assert_eq!(ColumnDataType::Text.to_duckdb_type(), "VARCHAR");
    assert_eq!(ColumnDataType::Uuid.to_duckdb_type(), "VARCHAR");
    assert_eq!(ColumnDataType::Blob.to_duckdb_type(), "BLOB");
}

#[test]
fn parse_data_type_is_public_and_loose() {
    assert!(matches!(
        parse_data_type("VARCHAR(64)"),
        ColumnDataType::Varchar { .. }
    ));
    assert!(matches!(
        parse_data_type("DECIMAL(12,2)"),
        ColumnDataType::Decimal { .. }
    ));
    assert!(matches!(parse_data_type("BIGINT"), ColumnDataType::BigInt));
    assert!(matches!(parse_data_type("WEIRD"), ColumnDataType::Text));
}

// ==================== 导出 ====================

#[tokio::test]
async fn export_sql_insert_writes_insert_statements() {
    let dir = temp_dir("sql");
    let path = dir.join("inserts.sql");

    let result = MockEngine::generate(basic_config("t_export_sql"))
        .await
        .expect("生成应当成功");

    let message = MockEngine::export(
        &result.temp_table_name,
        &MockExportFormat::SqlInsert,
        Some(path.to_str().expect("路径应为 UTF-8")),
        Some("orders_sql"),
    )
    .expect("SQL INSERT 导出应当成功");
    assert!(message.contains("SQL INSERT"), "返回信息: {}", message);

    let text = std::fs::read_to_string(&path).expect("导出文件应当可读");
    assert!(
        text.contains("INSERT INTO \"orders_sql\""),
        "目标表名应去掉临时前缀"
    );
    assert_eq!(text.lines().count(), 50, "每行一条 INSERT");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn export_csv_writes_file_with_header() {
    let dir = temp_dir("csv");
    let path = dir.join("rows.csv");

    let result = MockEngine::generate(basic_config("t_export_csv"))
        .await
        .expect("生成应当成功");
    MockEngine::export(
        &result.temp_table_name,
        &MockExportFormat::Csv,
        Some(path.to_str().expect("路径应为 UTF-8")),
        None,
    )
    .expect("CSV 导出应当成功");

    let text = std::fs::read_to_string(&path).expect("导出文件应当可读");
    assert!(
        text.starts_with("id,"),
        "首行应为列头: {:?}",
        text.lines().next()
    );
    assert_eq!(text.lines().count(), 51, "列头 + 50 行");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn export_sql_insert_requires_output_path() {
    let result = MockEngine::generate(basic_config("t_export_no_path"))
        .await
        .expect("生成应当成功");
    assert!(
        MockEngine::export(
            &result.temp_table_name,
            &MockExportFormat::SqlInsert,
            None,
            None
        )
        .is_err(),
        "缺少 output_path 应报错"
    );
}

#[tokio::test]
async fn export_table_creates_named_table_and_drops_temp() {
    let result = MockEngine::generate(basic_config("t_export_table"))
        .await
        .expect("生成应当成功");

    let message = MockEngine::export(
        &result.temp_table_name,
        &MockExportFormat::Table,
        None,
        Some("orders_snapshot"),
    )
    .expect("Table 导出应当成功");
    assert!(message.contains("orders_snapshot"), "返回信息: {message}");

    // 导出为正式表后，临时表应已释放；新表可查询且列数与定义一致。
    assert!(
        MockEngine::preview(&result.temp_table_name, 5).is_err(),
        "导出后临时表应已释放"
    );
    let snapshot = MockEngine::preview("orders_snapshot", 5).expect("新表应可预览");
    assert_eq!(snapshot.columns.len(), 3);
}

// ==================== 持久化出口 ====================

#[tokio::test]
async fn persist_as_asset_creates_table_and_consumes_temp() {
    let result = MockEngine::generate(basic_config("t_persist"))
        .await
        .expect("生成应当成功");

    let (name, row_count, column_count) =
        MockEngine::persist_as_asset(&result.temp_table_name, "orders_asset")
            .expect("持久化应当成功");

    assert_eq!(name, "orders_asset");
    assert_eq!(row_count, 50);
    assert_eq!(column_count, 3);
    // 临时表已被 drop，预览应报错——这是「持久化为资产」与「临时表」的分界。
    assert!(
        MockEngine::preview(&result.temp_table_name, 10).is_err(),
        "持久化后临时表应已释放"
    );
}

#[tokio::test]
async fn save_to_scratchpad_writes_file_into_dir() {
    let dir = temp_dir("scratchpad");
    let result = MockEngine::generate(basic_config("t_scratchpad"))
        .await
        .expect("生成应当成功");

    MockEngine::save_to_scratchpad(
        &result.temp_table_name,
        &MockExportFormat::Csv,
        dir.to_str().expect("路径应为 UTF-8"),
    )
    .expect("保存到草稿目录应当成功");

    let saved: Vec<_> = std::fs::read_dir(&dir)
        .expect("目录应当可读")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(saved.len(), 1, "应写入 1 个文件: {:?}", saved);
    assert!(
        saved[0].starts_with("mock_t_scratchpad_"),
        "文件名: {}",
        saved[0]
    );
    assert!(saved[0].ends_with(".csv"), "文件名: {}", saved[0]);

    let _ = std::fs::remove_dir_all(&dir);
}

// ==================== 场景模板 ====================

#[test]
fn list_templates_returns_builtins() {
    let templates = MockEngine::list_templates().expect("列出模板应当成功");
    assert_eq!(templates.len(), 6, "内置 6 个场景模板");
    let ids: Vec<&str> = templates.iter().map(|t| t.id.as_str()).collect();
    assert!(ids.contains(&"builtin:ecommerce"));

    let ecommerce = MockEngine::apply_template("builtin:ecommerce").expect("模板应当存在");
    assert_eq!(ecommerce.tables.len(), 4);
    assert!(MockEngine::apply_template("builtin:nope").is_err());
}

#[tokio::test]
async fn generate_scenario_generates_every_table_and_reports_progress() {
    let template = ScenarioTemplate {
        id: "it:scenario".to_string(),
        name: "集成测试场景".to_string(),
        description: "两张表，验证多表生成与进度回调".to_string(),
        category: "测试".to_string(),
        locale: "zh_cn".to_string(),
        tables: vec![
            TemplateTable {
                name: "it_parent".to_string(),
                row_count: 5,
                columns: vec![
                    auto_increment("id"),
                    col(
                        "code",
                        ColumnDataType::Varchar { length: Some(16) },
                        GeneratorConfig::Word,
                    ),
                ],
            },
            TemplateTable {
                name: "it_child".to_string(),
                row_count: 8,
                columns: vec![
                    auto_increment("id"),
                    col(
                        "note",
                        ColumnDataType::Varchar { length: Some(32) },
                        GeneratorConfig::Word,
                    ),
                ],
            },
        ],
    };

    let seen = Arc::new(AtomicUsize::new(0));
    let progress = seen.clone();
    let result = MockEngine::generate_scenario(&template, move |_done, total| {
        assert_eq!(total, 2, "总表数应为 2");
        progress.fetch_add(1, Ordering::SeqCst);
    })
    .await
    .expect("场景生成应当成功");

    assert_eq!(result.template_id, "it:scenario");
    assert_eq!(result.table_count, 2);
    assert_eq!(result.tables.len(), 2);
    assert_eq!(result.total_rows, 13, "5 + 8 行");
    assert_eq!(seen.load(Ordering::SeqCst), 2, "每张表应回调一次进度");
    assert_eq!(result.tables[0].temp_table_name, "temp_mock_it_parent");
    assert_eq!(result.tables[1].temp_table_name, "temp_mock_it_child");
}

// ==================== 表间引用（多表场景） ====================

/// 声明一条跨表引用（挂在列上，是本模块表达关系的唯一处）。
fn reference(parent_table: &str, parent_column: &str) -> ColumnDependency {
    ColumnDependency::foreign_key(parent_table, parent_column)
}

/// 引用列：场景生成时值从父表主键域取；单表生成时按自己的生成器取值。
fn ref_col(
    name: &str,
    parent_table: &str,
    parent_column: &str,
    fallback: GeneratorConfig,
) -> ColumnDef {
    ColumnDef {
        dependency: Some(reference(parent_table, parent_column)),
        ..col(name, ColumnDataType::Integer, fallback)
    }
}

/// 取某张临时表某一列的全部值（字符串化）。
fn column_of(temp_table: &str, column: &str) -> Vec<String> {
    let preview = MockEngine::preview(temp_table, 10_000).expect("读预览");
    let index = preview
        .columns
        .iter()
        .position(|c| c == column)
        .unwrap_or_else(|| panic!("{temp_table} 里没有列 {column}：{:?}", preview.columns));
    QueryResult::from_batches(preview.columns.clone(), preview.batches.clone())
        .rows
        .iter()
        .map(|row| match &row[index] {
            shared::models::Value::Null => "NULL".to_string(),
            other => other.to_string(),
        })
        .collect()
}

/// 引用：子表外键列的每个值都落在**父表主键域**内，且确实用上了父键（不是常数）。
///
/// 域是算出来的（父列自增参数 + 父表行数），所以这里同时验证了“不读已落地数据”也能对齐。
#[tokio::test]
async fn scenario_references_stay_inside_the_parent_key_domain() {
    let template = ScenarioTemplate {
        id: "it:refs".to_string(),
        name: "引用完整性".to_string(),
        description: "父表 5 行、子表 40 行，子表两列引用父表主键".to_string(),
        category: "测试".to_string(),
        locale: "zh_cn".to_string(),
        tables: vec![
            TemplateTable {
                name: "it_ref_parent".to_string(),
                row_count: 5,
                columns: vec![auto_increment("id")],
            },
            TemplateTable {
                name: "it_ref_child".to_string(),
                row_count: 40,
                columns: vec![
                    auto_increment("id"),
                    ref_col(
                        "parent_id",
                        "it_ref_parent",
                        "id",
                        GeneratorConfig::RandomInt { min: 1, max: 5 },
                    ),
                    ref_col(
                        "parent_id_again",
                        "it_ref_parent",
                        "id",
                        GeneratorConfig::RandomInt { min: 1, max: 5 },
                    ),
                ],
            },
        ],
    };

    let result = MockEngine::generate_scenario(&template, |_, _| {})
        .await
        .expect("场景生成应当成功");

    let parent_keys = column_of(&result.tables[0].temp_table_name, "id");
    assert_eq!(parent_keys.len(), 5, "父表应生成 5 行");
    let child_keys = column_of(&result.tables[1].temp_table_name, "parent_id");
    assert_eq!(child_keys.len(), 40, "子表应生成 40 行");

    let bad: Vec<&String> = child_keys
        .iter()
        .filter(|value| !parent_keys.contains(value))
        .collect();
    assert!(
        bad.is_empty(),
        "子表外键应全部落在父表主键域 {parent_keys:?} 内，越界的有 {bad:?}"
    );

    let distinct: std::collections::HashSet<&String> = child_keys.iter().collect();
    assert!(
        distinct.len() > 1,
        "40 行只抽到同一个父键（{}），说明没真的按域采样",
        distinct.len()
    );
    let again = column_of(&result.tables[1].temp_table_name, "parent_id_again");
    assert!(
        again.iter().all(|value| parent_keys.contains(value)),
        "同一张表上的第二条引用同样要在域内"
    );
}

/// 引用**自身**（自关联）合法：父域就是本表主键域；引用**后面**才生成的表也合法
/// （域由模板参数算出，与“哪张表先跑”无关）。
#[tokio::test]
async fn scenario_references_allow_self_and_forward_targets() {
    let template = ScenarioTemplate {
        id: "it:refs-order".to_string(),
        name: "自关联与前向引用".to_string(),
        description: "child 引用自己（parent_id）并引用后生成的 later 表".to_string(),
        category: "测试".to_string(),
        locale: "en".to_string(),
        tables: vec![
            TemplateTable {
                name: "it_selfref_child".to_string(),
                row_count: 12,
                columns: vec![
                    auto_increment("id"),
                    ref_col(
                        "parent_id",
                        "it_selfref_child",
                        "id",
                        GeneratorConfig::RandomInt { min: 1, max: 12 },
                    ),
                    ref_col(
                        "later_id",
                        "it_selfref_later",
                        "id",
                        GeneratorConfig::RandomInt { min: 1, max: 3 },
                    ),
                ],
            },
            TemplateTable {
                name: "it_selfref_later".to_string(),
                row_count: 3,
                columns: vec![auto_increment("id")],
            },
        ],
    };

    let result = MockEngine::generate_scenario(&template, |_, _| {})
        .await
        .expect("自关联与前向引用都应当成功");

    let own = column_of(&result.tables[0].temp_table_name, "id");
    assert!(
        column_of(&result.tables[0].temp_table_name, "parent_id")
            .iter()
            .all(|value| own.contains(value)),
        "自关联列应落在本表主键域内"
    );
    let later = column_of(&result.tables[1].temp_table_name, "id");
    assert_eq!(later.len(), 3);
    assert!(
        column_of(&result.tables[0].temp_table_name, "later_id")
            .iter()
            .all(|value| later.contains(value)),
        "前向引用应落在后生成的表的主键域内"
    );
}

/// 解析不了的引用：**生成前**就报可读错误（不猜、也不去读已落地的数据）。
#[test]
fn scenario_reference_resolution_reports_readable_errors() {
    // 父表总是只有一列 `id`；`parent_column` 是**子表引用时写的列名**
    let build = |parent: &str, parent_column: &str, parent_gen: GeneratorConfig| ScenarioTemplate {
        id: "it:refs-bad".to_string(),
        name: "坏引用".to_string(),
        description: String::new(),
        category: "测试".to_string(),
        locale: "en".to_string(),
        tables: vec![
            TemplateTable {
                name: "it_bad_parent".to_string(),
                row_count: 8,
                columns: vec![ColumnDef {
                    generator: parent_gen,
                    ..auto_increment("id")
                }],
            },
            TemplateTable {
                name: "it_bad_child".to_string(),
                row_count: 4,
                columns: vec![
                    auto_increment("id"),
                    ref_col(
                        "fk",
                        parent,
                        parent_column,
                        GeneratorConfig::RandomInt { min: 1, max: 8 },
                    ),
                ],
            },
        ],
    };

    let missing_table = build(
        "nope",
        "id",
        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
    );
    let err = MockEngine::resolve_reference_domains(&missing_table).expect_err("应报错");
    assert!(err.to_string().contains("没有的表"), "err: {err}");

    let missing_column = build(
        "it_bad_parent",
        "nope",
        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
    );
    let err = MockEngine::resolve_reference_domains(&missing_column).expect_err("应报错");
    assert!(err.to_string().contains("没有这一列"), "err: {err}");

    // 父列不是自增（uuid / 随机）→ 算不出域，明确拒绝
    let not_auto = build("it_bad_parent", "id", GeneratorConfig::UuidV4);
    let err = MockEngine::resolve_reference_domains(&not_auto).expect_err("应报错");
    assert!(err.to_string().contains("自增"), "err: {err}");

    // 全是好引用时：域按 (表, 列) 去重（两条引用指向同一父列）
    let ok = build(
        "it_bad_parent",
        "id",
        GeneratorConfig::AutoIncrement { start: 1, step: 1 },
    );
    let domains = MockEngine::resolve_reference_domains(&ok).expect("应解析成功");
    assert_eq!(domains.len(), 1, "两条引用指向同一父列，域应只有一条");
    assert_eq!(domains[0].first, 1);
    assert_eq!(domains[0].last(), 8);
    assert_eq!(domains[0].count, 8);
}

// ==================== 约束类生成器（集合类参数） ====================

/// 集合为空 / 权重全为 0：**生成前**拦住并给可读错误。
///
/// 不拦的话生成期会直接 panic（空区间取随机索引 / 除零 / 抽 `0.0..0`），
/// 工作线程一挂，该进程后面所有任务都失败。
#[tokio::test]
async fn empty_constraint_collection_is_rejected_before_generating() {
    for (hint, generator) in [
        (
            "外键取值",
            GeneratorConfig::ForeignKey { values: Vec::new() },
        ),
        (
            "序列取值",
            GeneratorConfig::Sequence {
                values: Vec::new(),
                cycle: false,
            },
        ),
        (
            "加权选项",
            GeneratorConfig::Weighted {
                choices: Vec::new(),
            },
        ),
        (
            "加权选项",
            GeneratorConfig::Weighted {
                choices: vec![("a".to_string(), 0.0)],
            },
        ),
    ] {
        let config = MockConfig {
            table_name: "t_empty_set".to_string(),
            row_count: 5,
            seed: Some(1),
            locale: Locale::ZhCn,
            columns: vec![col(
                "v",
                ColumnDataType::Varchar { length: None },
                generator,
            )],
        };
        let err = MockEngine::generate(config).await.expect_err("应当被拦住");
        assert!(err.to_string().contains(hint), "{err}");
        assert!(err.to_string().contains("列 'v'"), "{err}");
    }
}

/// 填上集合后就能正常生成（配对的"绿灯"用例：证明拦的是空集合本身）。
#[tokio::test]
async fn filled_constraint_collection_generates() {
    let config = MockConfig {
        table_name: "t_filled_set".to_string(),
        row_count: 20,
        seed: Some(9),
        locale: Locale::ZhCn,
        columns: vec![col(
            "status",
            ColumnDataType::Varchar { length: None },
            GeneratorConfig::Weighted {
                choices: vec![("已发货".to_string(), 3.0), ("已取消".to_string(), 1.0)],
            },
        )],
    };
    let result = MockEngine::generate(config)
        .await
        .expect("填了集合就应当能生成");
    assert_eq!(result.row_count, 20);

    // 值必须来自集合（不能是空串或别的默认值）
    let rows = rows_of(&result);
    assert!(
        rows.iter().all(|values| values
            .first()
            .is_some_and(|value| value.to_string() == "已发货" || value.to_string() == "已取消")),
        "生成值应来自集合: {rows:?}"
    );
}

/// 时间 / 日期 / 十进制列：预览与 SQL 导出都必须有值（不能是 NULL）。
///
/// 这两条曾经一起坏掉：`row_to_arrow` 只认 5 类变体、`value_to_sql_literal` 只认 9 类，
/// 于是 DateTime / DECIMAL 列在预览里恒为 NULL、在导出的 `.sql` 里也全是 NULL——
/// 用户看到的就是「生成完了但数据是空的」。
#[tokio::test]
async fn temporal_and_decimal_columns_survive_preview_and_sql_export() {
    let dir = temp_dir("value_coverage");
    let path = dir.join("out.sql");

    let config = MockConfig {
        table_name: "value_coverage".to_string(),
        row_count: 3,
        seed: Some(11),
        locale: Locale::ZhCn,
        columns: vec![
            auto_increment("id"),
            col(
                "created_at",
                ColumnDataType::DateTime,
                GeneratorConfig::DateTime {
                    min: "2024-01-01T00:00:00Z".to_string(),
                    max: "2024-12-31T23:59:59Z".to_string(),
                },
            ),
            col(
                "birth_date",
                ColumnDataType::Date,
                GeneratorConfig::Date {
                    min: "1980-01-01".to_string(),
                    max: "2000-12-31".to_string(),
                },
            ),
            col(
                "amount",
                ColumnDataType::Decimal {
                    precision: 10,
                    scale: 2,
                },
                GeneratorConfig::RandomDecimal {
                    min: 1.0,
                    max: 100.0,
                    scale: 2,
                },
            ),
        ],
    };
    let result = MockEngine::generate(config).await.expect("生成");

    // 预览：每一格都得有值
    let rows = rows_of(&result);
    assert_eq!(rows.len(), 3);
    for row in &rows {
        for (index, value) in row.iter().enumerate() {
            assert_ne!(
                value,
                &shared::models::Value::Null,
                "预览第 {index} 列是 NULL：{row:?}"
            );
        }
    }
    assert_eq!(
        result.preview.columns,
        vec!["id", "created_at", "birth_date", "amount"]
    );

    // 导出的 `.sql`：时间戳 / 日期要有类型前缀，且整份脚本不含 NULL
    MockEngine::export(
        &result.temp_table_name,
        &MockExportFormat::SqlInsert,
        Some(path.to_str().expect("路径应为 UTF-8")),
        Some("value_coverage"),
    )
    .expect("SQL INSERT 导出");
    let sql = std::fs::read_to_string(&path).expect("导出文件应当可读");
    assert!(sql.contains("TIMESTAMP '"), "{sql}");
    assert!(sql.contains("DATE '"), "{sql}");
    assert!(!sql.contains("NULL"), "导出里不该有 NULL 占位：{sql}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 非法参数要在**生成前**被拦，而不是在工作线程里 panic。
///
/// 为什么重要：panic 发生在持有内存库连接锁期间会毒化锁，本进程之后每次生成都报
/// `poisoned lock`——用户只能重启应用（v1 的两处兜底就踩过：映射兜底 `Sentence{1,1}`、
/// 反向区间）。这里从公开 API 锁住拦截行为。
#[tokio::test]
async fn unsamplable_generator_params_are_rejected_before_generation() {
    let config = MockConfig {
        table_name: "bad_range".to_string(),
        row_count: 3,
        seed: Some(1),
        locale: Locale::ZhCn,
        columns: vec![
            auto_increment("id"),
            col(
                "n",
                ColumnDataType::Integer,
                GeneratorConfig::RandomInt { min: 10, max: 5 },
            ),
        ],
    };
    let reason = MockEngine::generate(config)
        .await
        .expect_err("反向区间必须被拦")
        .to_string();
    assert!(reason.contains("min 10 > max 5"), "{reason}");
    assert!(reason.contains('n'), "错误里要点出是哪一列：{reason}");

    // 拦下之后内存库仍然可用（没有被 panic 毒化）
    let ok = MockEngine::generate(basic_config("bad_range_after"))
        .await
        .expect("拦截之后照常能生成");
    assert_eq!(ok.row_count, 50);
}
