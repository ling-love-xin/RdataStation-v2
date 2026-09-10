//! Mock 引擎集成测试
//!
//! 测试 MockEngine 的公共 API：生成、预览、导出、列映射、依赖解析。

use rdata_station_lib::mock::{
    ColumnDataType, ColumnDef, ColumnDependency, DependencyType, GeneratorConfig, Locale,
    MockConfig, MockEngine,
};

// ==================== 配置构造 ====================

fn basic_config() -> MockConfig {
    MockConfig {
        table_name: "test_users".to_string(),
        row_count: 50,
        seed: Some(42),
        locale: Locale::ZhCn,
        columns: vec![
            ColumnDef {
                name: "id".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                nullable_ratio: 0.0,
                unique: true,
                dependency: None,
            },
            ColumnDef {
                name: "username".to_string(),
                data_type: ColumnDataType::Varchar { length: Some(50) },
                generator: GeneratorConfig::Username,
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
            ColumnDef {
                name: "email".to_string(),
                data_type: ColumnDataType::Varchar { length: Some(100) },
                generator: GeneratorConfig::Email,
                nullable_ratio: 0.1,
                unique: false,
                dependency: None,
            },
        ],
    }
}

// ==================== 生成测试 ====================

#[tokio::test]
async fn test_generate_basic() {
    let config = basic_config();
    let result = MockEngine::generate(config).await.expect("generate should succeed");

    assert_eq!(result.table_name, "test_users");
    assert_eq!(result.row_count, 50);
    assert_eq!(result.columns.len(), 3);
    assert!(!result.temp_table_name.is_empty());
    assert!(result.temp_table_name.starts_with("temp_mock_"));
    assert!(result.elapsed_ms > 0);
}

#[tokio::test]
async fn test_generate_produces_preview() {
    let config = basic_config();
    let result = MockEngine::generate(config).await.expect("generate should succeed");

    let preview = result.preview;
    assert!(!preview.rows.is_empty(), "preview should have rows");
    assert_eq!(preview.columns.len(), 3, "preview should have 3 columns");
}

#[tokio::test]
async fn test_generate_empty_columns_errors() {
    let config = MockConfig {
        table_name: "empty".to_string(),
        row_count: 10,
        seed: None,
        locale: Locale::ZhCn,
        columns: vec![],
    };
    let result = MockEngine::generate(config).await;
    assert!(result.is_err(), "should error on empty columns");
}

#[tokio::test]
async fn test_generate_zero_rows_errors() {
    let config = MockConfig {
        table_name: "zero".to_string(),
        row_count: 0,
        seed: None,
        locale: Locale::ZhCn,
        columns: vec![ColumnDef {
            name: "col".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        }],
    };
    let result = MockEngine::generate(config).await;
    assert!(result.is_err(), "should error on zero rows");
}

#[tokio::test]
async fn test_generate_with_nullable_ratio() {
    let config = MockConfig {
        table_name: "nullable_test".to_string(),
        row_count: 100,
        seed: Some(1),
        locale: Locale::ZhCn,
        columns: vec![ColumnDef {
            name: "col".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::RandomInt { min: 1, max: 100 },
            nullable_ratio: 1.0,
            unique: false,
            dependency: None,
        }],
    };
    let result = MockEngine::generate(config).await.expect("generate should succeed");
    let preview = result.preview;
    for row in &preview.rows {
        assert!(row[0].is_null(), "all values should be null with ratio 1.0");
    }
}

#[tokio::test]
async fn test_generate_with_unique_column() {
    let config = MockConfig {
        table_name: "unique_test".to_string(),
        row_count: 30,
        seed: Some(7),
        locale: Locale::ZhCn,
        columns: vec![ColumnDef {
            name: "id".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            nullable_ratio: 0.0,
            unique: true,
            dependency: None,
        }],
    };
    let result = MockEngine::generate(config).await.expect("generate should succeed");
    let preview = result.preview;
    let values: Vec<Option<i64>> = preview.rows.iter().map(|r| {
        r[0].as_ref().and_then(|v| v.parse::<i64>().ok())
    }).collect();
    let mut prev = 0;
    for v in values.iter().flatten() {
        assert!(*v > prev, "values should be strictly increasing: {} <= {}", v, prev);
        prev = *v;
    }
}

// ==================== 预览测试 ====================

#[tokio::test]
async fn test_preview_after_generate() {
    let config = basic_config();
    let result = MockEngine::generate(config).await.expect("generate should succeed");

    let _preview = MockEngine::preview(&result.temp_table_name, 20)
        .expect("preview should succeed");
}

#[tokio::test]
async fn test_preview_nonexistent_table_errors() {
    let result = MockEngine::preview("nonexistent_table", 10);
    assert!(result.is_err(), "should error on nonexistent table");
}

// ==================== 列映射测试 ====================

#[test]
fn test_map_column_numeric() {
    let response = MockEngine::map_column("age", "integer")
        .expect("map_column should succeed");
    assert!(!response.generator.type_name().is_empty());
    assert!(response.confidence > 0.0);
}

#[test]
fn test_map_column_text() {
    let response = MockEngine::map_column("email", "varchar")
        .expect("map_column should succeed");
    assert!(!response.generator.type_name().is_empty());
}

#[test]
fn test_map_columns_batch() {
    let columns = vec![
        ("id".to_string(), "integer".to_string()),
        ("name".to_string(), "varchar".to_string()),
        ("created_at".to_string(), "datetime".to_string()),
    ];
    let responses = MockEngine::map_columns_batch(columns)
        .expect("map_columns_batch should succeed");
    assert_eq!(responses.len(), 3);
    for r in &responses {
        assert!(!r.generator.type_name().is_empty());
    }
}

// ==================== 依赖解析测试 ====================

#[test]
fn test_resolve_dependencies_no_deps() {
    let columns = vec![
        ColumnDef {
            name: "a".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        },
        ColumnDef {
            name: "b".to_string(),
            data_type: ColumnDataType::Varchar { length: None },
            generator: GeneratorConfig::Words { min: 1, max: 3 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        },
    ];
    let config = MockEngine::resolve_dependencies(&columns);
    assert_eq!(config.sorted_columns.len(), 2);
    assert!(config.dependencies.is_empty());
}

#[test]
fn test_resolve_dependencies_linear() {
    let columns = vec![
        ColumnDef {
            name: "a".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        },
        ColumnDef {
            name: "b".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::RandomInt { min: 1, max: 100 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: Some(ColumnDependency {
                dep_type: DependencyType::Expression,
                source_columns: vec!["a".to_string()],
                expression: Some("a * 2".to_string()),
                ref_table: None,
                ref_column: None,
                weights: None,
            }),
        },
    ];
    let config = MockEngine::resolve_dependencies(&columns);
    let a_idx = config.sorted_columns.iter().position(|n| n == "a").unwrap();
    let b_idx = config.sorted_columns.iter().position(|n| n == "b").unwrap();
    assert!(a_idx < b_idx, "a must come before b in dependency order");
    assert_eq!(config.dependencies.len(), 1);
}

#[test]
fn test_resolve_dependencies_chain() {
    let columns = vec![
        ColumnDef {
            name: "a".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        },
        ColumnDef {
            name: "b".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::RandomInt { min: 1, max: 100 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: Some(ColumnDependency {
                dep_type: DependencyType::Expression,
                source_columns: vec!["a".to_string()],
                expression: Some("a + 1".to_string()),
                ref_table: None,
                ref_column: None,
                weights: None,
            }),
        },
        ColumnDef {
            name: "c".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::RandomInt { min: 1, max: 100 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: Some(ColumnDependency {
                dep_type: DependencyType::Expression,
                source_columns: vec!["b".to_string()],
                expression: Some("b + 1".to_string()),
                ref_table: None,
                ref_column: None,
                weights: None,
            }),
        },
    ];
    let config = MockEngine::resolve_dependencies(&columns);
    let a_idx = config.sorted_columns.iter().position(|n| n == "a").unwrap();
    let b_idx = config.sorted_columns.iter().position(|n| n == "b").unwrap();
    let c_idx = config.sorted_columns.iter().position(|n| n == "c").unwrap();
    assert!(a_idx < b_idx, "a before b");
    assert!(b_idx < c_idx, "b before c");
}

// ==================== 取消测试 ====================

#[tokio::test]
async fn test_cancel_flag_reset() {
    let config = MockConfig {
        table_name: "cancel_test".to_string(),
        row_count: 10,
        seed: Some(1),
        locale: Locale::ZhCn,
        columns: vec![ColumnDef {
            name: "col".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        }],
    };
    let result = MockEngine::generate(config).await;
    assert!(result.is_ok(), "generate should succeed after cancel_reset");
}

// ==================== 数据类型映射测试 ====================

#[test]
fn test_column_data_type_to_duckdb() {
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
        ColumnDataType::Decimal { precision: 18, scale: 2 }.to_duckdb_type(),
        "DECIMAL(18, 2)"
    );
    assert_eq!(ColumnDataType::Text.to_duckdb_type(), "VARCHAR");
    assert_eq!(ColumnDataType::Uuid.to_duckdb_type(), "VARCHAR");
    assert_eq!(ColumnDataType::Blob.to_duckdb_type(), "BLOB");
}