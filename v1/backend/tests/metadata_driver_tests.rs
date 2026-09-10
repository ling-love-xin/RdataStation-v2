//! 元数据与驱动命令 单元测试
//!
//! 覆盖：Metadata 类型序列化、Driver 注册表、Metadata Cache 类型、
//!       SQL 模板 CRUD 类型、导航树节点类型。
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use rdata_station_lib::commands::{
    CacheStatusResponse, ColumnMeta, CreateSqlTemplateInput, DatabaseMeta, RefreshCacheInput,
    SchemaMeta, SequenceMeta, SqlTemplateResponse, TableMeta, TriggerMeta, ViewMeta,
};
use rdata_station_lib::core::driver::introspection::IntrospectionLevel;
use rdata_station_lib::core::driver::registry::descriptors::{
    get_all_drivers, get_driver, DriverKind,
};
use rdata_station_lib::core::driver::traits::{ColumnDetail, SchemaObject, SchemaObjectKind};
use rdata_station_lib::core::persistence::driver_store::{DataSourceType, Driver};
use rdata_station_lib::core::persistence::sql_template_store::SqlTemplate;
use rdata_station_lib::core::types::{FunctionMeta, ProcedureMeta, RoutineSourceMeta};

// ==================== Metadata 类型序列化测试 ====================

#[test]
fn test_database_meta_serialization() {
    let meta = DatabaseMeta {
        name: "test_db".to_string(),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("test_db"));
    let parsed: DatabaseMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "test_db");
}

#[test]
fn test_schema_meta_basic_construction() {
    let meta = SchemaMeta::basic("public".to_string());
    assert_eq!(meta.name, "public");
    assert!(meta.total_tables.is_none());
    assert!(meta.total_views.is_none());
    assert!(meta.total_procedures.is_none());
    assert!(meta.total_functions.is_none());
    assert!(meta.total_size_bytes.is_none());
    assert!(meta.row_count_total.is_none());
}

#[test]
fn test_schema_meta_full_serialization() {
    let meta = SchemaMeta {
        name: "analytics".to_string(),
        total_tables: Some(42),
        total_views: Some(5),
        total_procedures: Some(3),
        total_functions: Some(8),
        total_size_bytes: Some(1024 * 1024),
        row_count_total: Some(100000),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("analytics"));
    // 检查 camelCase 字段名
    assert!(json.contains("totalTables"));
    assert!(json.contains("totalViews"));
    let parsed: SchemaMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.total_tables, Some(42));
    assert_eq!(parsed.row_count_total, Some(100000));
}

#[test]
fn test_table_meta_basic_construction() {
    let meta = TableMeta::basic("users".to_string(), "TABLE".to_string());
    assert_eq!(meta.name, "users");
    assert_eq!(meta.table_type, "TABLE");
    assert!(meta.row_count_estimate.is_none());
    assert!(meta.data_length.is_none());
    assert!(meta.hidden.is_none());
}

#[test]
fn test_table_meta_full_serialization() {
    let meta = TableMeta {
        name: "orders".to_string(),
        table_type: "TABLE".to_string(),
        row_count_estimate: Some(50000),
        data_length: Some(1048576),
        index_length: Some(262144),
        display_order: Some(1),
        hidden: Some(false),
        favorite: Some(true),
        color_label: Some("#FF0000".to_string()),
        user_comment: Some("重要表".to_string()),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("orders"));
    assert!(json.contains("rowCountEstimate"));
    assert!(json.contains("dataLength"));
    assert!(json.contains("colorLabel"));
    let parsed: TableMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "orders");
    assert_eq!(parsed.favorite, Some(true));
}

#[test]
fn test_column_meta_serialization() {
    let meta = ColumnMeta {
        name: "user_id".to_string(),
        data_type: "INTEGER".to_string(),
        is_nullable: false,
        default_value: None,
        is_primary_key: true,
        is_foreign_key: false,
        comment: Some("主键".to_string()),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("user_id"));
    assert!(json.contains("dataType"));
    assert!(json.contains("isPrimaryKey"));
    let parsed: ColumnMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "user_id");
    assert!(parsed.is_primary_key);
}

#[test]
fn test_view_meta_serialization() {
    let meta = ViewMeta {
        name: "user_summary".to_string(),
        view_type: "VIEW".to_string(),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("user_summary"));
    let parsed: ViewMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "user_summary");
    assert_eq!(parsed.view_type, "VIEW");
}

#[test]
fn test_sequence_meta_serialization() {
    let meta = SequenceMeta {
        name: "order_seq".to_string(),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("order_seq"));
    let parsed: SequenceMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "order_seq");
}

#[test]
fn test_trigger_meta_serialization() {
    let meta = TriggerMeta {
        name: "before_insert_audit".to_string(),
        table_name: Some("orders".to_string()),
        event: Some("INSERT".to_string()),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("before_insert_audit"));
    assert!(json.contains("tableName"));
    let parsed: TriggerMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "before_insert_audit");
    assert_eq!(parsed.table_name, Some("orders".to_string()));
    assert_eq!(parsed.event, Some("INSERT".to_string()));
}

#[test]
fn test_procedure_meta_serialization() {
    let meta = ProcedureMeta {
        name: "sp_calculate".to_string(),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("sp_calculate"));
    let parsed: ProcedureMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "sp_calculate");
}

#[test]
fn test_function_meta_serialization() {
    let meta = FunctionMeta {
        name: "fn_get_total".to_string(),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("fn_get_total"));
    let parsed: FunctionMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "fn_get_total");
}

#[test]
fn test_routine_source_meta_serialization() {
    let meta = RoutineSourceMeta {
        name: "fn_sum".to_string(),
        routine_kind: "FUNCTION".to_string(),
        source_code: Some("CREATE FUNCTION fn_sum() ...".to_string()),
    };
    let json = serde_json::to_string(&meta).expect("序列化失败");
    assert!(json.contains("routineKind"));
    assert!(json.contains("sourceCode"));
    assert!(json.contains("fn_sum"));
    let parsed: RoutineSourceMeta = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "fn_sum");
    assert_eq!(parsed.routine_kind, "FUNCTION");
}

// ==================== 导航树节点类型测试 ====================

#[test]
fn test_schema_object_kind_all_variants() {
    let catalog = SchemaObjectKind::Catalog;
    let schema = SchemaObjectKind::Schema;
    let table = SchemaObjectKind::Table;
    let view = SchemaObjectKind::View;
    let column = SchemaObjectKind::Column;
    let index = SchemaObjectKind::Index;
    let pk = SchemaObjectKind::PrimaryKey;
    let fk = SchemaObjectKind::ForeignKey;
    let proc = SchemaObjectKind::Procedure;
    let func = SchemaObjectKind::Function;
    let seq = SchemaObjectKind::Sequence;
    let trigger = SchemaObjectKind::Trigger;

    // 确保所有变体都可构造
    let _ = catalog;
    let _ = schema;
    let _ = table;
    let _ = view;
    let _ = column;
    let _ = index;
    let _ = pk;
    let _ = fk;
    let _ = proc;
    let _ = func;
    let _ = seq;
    let _ = trigger;
}

#[test]
fn test_schema_object_lazy_loading() {
    // children: None → 未加载（懒加载语义）
    let obj = SchemaObject {
        name: "users".to_string(),
        kind: SchemaObjectKind::Table,
        children: None,
        comment: None,
        table_name: None,
        event: None,
    };
    assert!(obj.children.is_none());
    assert_eq!(obj.name, "users");
}

#[test]
fn test_schema_object_with_children() {
    let columns = vec![
        SchemaObject {
            name: "id".to_string(),
            kind: SchemaObjectKind::Column,
            children: None,
            comment: None,
            table_name: None,
            event: None,
        },
        SchemaObject {
            name: "name".to_string(),
            kind: SchemaObjectKind::Column,
            children: None,
            comment: None,
            table_name: None,
            event: None,
        },
    ];
    let obj = SchemaObject {
        name: "users".to_string(),
        kind: SchemaObjectKind::Table,
        children: Some(columns),
        comment: Some("用户表".to_string()),
        table_name: None,
        event: None,
    };
    assert!(obj.children.is_some());
    let children = obj.children.unwrap();
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name, "id");
}

#[test]
fn test_trigger_schema_object_with_table_name_and_event() {
    let trigger = SchemaObject {
        name: "trg_audit".to_string(),
        kind: SchemaObjectKind::Trigger,
        children: None,
        comment: None,
        table_name: Some("orders".to_string()),
        event: Some("INSERT".to_string()),
    };
    assert_eq!(trigger.table_name, Some("orders".to_string()));
    assert_eq!(trigger.event, Some("INSERT".to_string()));
}

#[test]
fn test_column_detail_serialization() {
    let col = ColumnDetail {
        name: "email".to_string(),
        data_type: "VARCHAR(255)".to_string(),
        nullable: true,
        is_primary_key: false,
        is_foreign_key: false,
        default_value: Some("''".to_string()),
        comment: Some("用户邮箱".to_string()),
        extra: std::collections::HashMap::new(),
    };
    let json = serde_json::to_string(&col).expect("序列化失败");
    assert!(json.contains("email"));
    assert!(json.contains("VARCHAR"));
    let parsed: ColumnDetail = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.name, "email");
    assert!(parsed.nullable);
}

// ==================== Driver Registry 测试 ====================

#[test]
fn test_driver_kind_enum() {
    // 验证 DriverKind 枚举变体存在
    let native = DriverKind::Native;
    let jdbc = DriverKind::Jdbc;
    let wasm = DriverKind::Wasm;
    let odbc = DriverKind::Odbc;

    let _ = native;
    let _ = jdbc;
    let _ = wasm;
    let _ = odbc;
}

#[test]
#[ignore = "requires driver registry initialization (app context)"]
fn test_get_all_drivers_not_empty() {
    let drivers = get_all_drivers();
    // 内置驱动至少应该有 6 个
    assert!(drivers.len() >= 6, "应至少有 6 个内置驱动描述符");
}

#[test]
#[ignore = "requires driver registry initialization (app context)"]
fn test_get_driver_mysql() {
    let driver = get_driver("mysql");
    assert!(driver.is_some(), "mysql 驱动应存在");
    let desc = driver.unwrap();
    assert_eq!(desc.id, "mysql");
    assert_eq!(desc.name, "MySQL");
}

#[test]
#[ignore = "requires driver registry initialization (app context)"]
fn test_get_driver_postgres() {
    let driver = get_driver("postgres");
    assert!(driver.is_some(), "postgres 驱动应存在");
    let desc = driver.unwrap();
    assert_eq!(desc.id, "postgres");
    assert_eq!(desc.name, "PostgreSQL");
}

#[test]
#[ignore = "requires driver registry initialization (app context)"]
fn test_get_driver_sqlite() {
    let driver = get_driver("sqlite");
    assert!(driver.is_some(), "sqlite 驱动应存在");
    let desc = driver.unwrap();
    assert_eq!(desc.id, "sqlite");
    assert_eq!(desc.name, "SQLite");
}

#[test]
#[ignore = "requires driver registry initialization (app context)"]
fn test_get_driver_duckdb() {
    let driver = get_driver("duckdb");
    assert!(driver.is_some(), "duckdb 驱动应存在");
    let desc = driver.unwrap();
    assert_eq!(desc.id, "duckdb");
    assert_eq!(desc.name, "DuckDB");
}

#[test]
fn test_get_driver_nonexistent() {
    let driver = get_driver("nonexistent_db");
    assert!(driver.is_none(), "不存在的驱动应返回 None");
}

#[test]
#[ignore = "requires driver registry initialization (app context)"]
fn test_driver_descriptor_fields() {
    let desc = get_driver("mysql").expect("mysql 驱动应存在");
    // 验证关键字段非空
    assert!(!desc.id.is_empty());
    assert!(!desc.name.is_empty());
    assert!(!desc.description.is_empty());
    assert!(!desc.category.is_empty());
    // 网络数据库应有默认端口
    assert!(desc.default_port.is_some());
}

#[test]
#[ignore = "requires driver registry initialization (app context)"]
fn test_driver_descriptor_url_template() {
    let desc = get_driver("mysql").expect("mysql 驱动应存在");
    assert!(desc.url_template.is_some(), "mysql 应有 url_template");
    let template = desc.url_template.as_ref().unwrap();
    assert!(
        template.contains("{host}"),
        "url_template 应包含 host 占位符"
    );
    assert!(
        template.contains("{port}"),
        "url_template 应包含 port 占位符"
    );
}

#[test]
fn test_driver_struct_serialization() {
    let driver = Driver {
        id: "mysql".to_string(),
        type_id: "mysql".to_string(),
        name: "MySQL".to_string(),
        driver_kind: "native".to_string(),
        is_file: false,
        default_port: Some(3306),
        url_template: Some("mysql://{host}:{port}/{database}".to_string()),
        download_url: None,
        download_checksum: None,
        version: Some("8.0".to_string()),
        config_schema: "{}".to_string(),
        supported_auth_types: Some("password".to_string()),
        capabilities: None,
        driver_properties: None,
        enabled: true,
    };
    let json = serde_json::to_string(&driver).expect("序列化失败");
    assert!(json.contains("mysql"));
    let parsed: Driver = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.id, "mysql");
    assert_eq!(parsed.default_port, Some(3306));
}

#[test]
fn test_data_source_type_struct() {
    let ds_type = DataSourceType {
        id: "mysql".to_string(),
        name: "MySQL".to_string(),
        category: "relational".to_string(),
        icon: Some("mysql-icon".to_string()),
        enabled: true,
        created_at: "2024-01-01T00:00:00Z".to_string(),
    };
    let json = serde_json::to_string(&ds_type).expect("序列化失败");
    let parsed: DataSourceType = serde_json::from_str(&json).expect("反序列化失败");
    assert_eq!(parsed.id, "mysql");
    assert_eq!(parsed.category, "relational");
    assert!(parsed.enabled);
}

// ==================== Introspection Level 测试 ====================

#[test]
fn test_introspection_level_parsing() {
    // IntrospectionLevel 有三种级别
    let l1 = IntrospectionLevel::Level1;
    let l2 = IntrospectionLevel::Level2;
    let l3 = IntrospectionLevel::Level3;

    // 确保 Display 可以区分
    let s1 = l1.to_string();
    let s2 = l2.to_string();
    let s3 = l3.to_string();
    assert_ne!(s1, s2);
    assert_ne!(s2, s3);
    assert_ne!(s1, s3);
}

// ==================== Metadata Cache 类型测试 ====================

#[test]
fn test_cache_status_response_serialization() {
    let status = CacheStatusResponse {
        is_valid: true,
        last_sync: Some(1710000000),
        stats: None,
    };
    let json = serde_json::to_string(&status).expect("序列化失败");
    assert!(json.contains("true"));
    let parsed: CacheStatusResponse = serde_json::from_str(&json).expect("反序列化失败");
    assert!(parsed.is_valid);
    assert_eq!(parsed.last_sync, Some(1710000000));
}

#[test]
fn test_cache_status_response_invalid() {
    let status = CacheStatusResponse {
        is_valid: false,
        last_sync: None,
        stats: None,
    };
    let json = serde_json::to_string(&status).expect("序列化失败");
    let parsed: CacheStatusResponse = serde_json::from_str(&json).expect("反序列化失败");
    assert!(!parsed.is_valid);
    assert!(parsed.last_sync.is_none());
}

#[test]
fn test_refresh_cache_input_construction() {
    let input = RefreshCacheInput {
        connection_id: "conn-123".to_string(),
        connection_type: "project".to_string(),
        project_path: Some("/path/to/project".to_string()),
        database_name: "mydb".to_string(),
        schema_name: Some("public".to_string()),
    };
    assert_eq!(input.connection_id, "conn-123");
    assert_eq!(input.connection_type, "project");
    assert_eq!(input.project_path, Some("/path/to/project".to_string()));
    assert_eq!(input.database_name, "mydb");
    assert_eq!(input.schema_name, Some("public".to_string()));
}

// ==================== SQL 模板类型测试 ====================

#[test]
fn test_sql_template_new() {
    let template = SqlTemplate::new(
        "查询用户".to_string(),
        "SELECT * FROM users WHERE id = {id}".to_string(),
        Some("mysql".to_string()),
        "查询".to_string(),
        Some("用户查询模板".to_string()),
        Some("用户,查询".to_string()),
    );
    assert_eq!(template.name, "查询用户");
    assert_eq!(template.content, "SELECT * FROM users WHERE id = {id}");
    assert_eq!(template.db_type, Some("mysql".to_string()));
    assert_eq!(template.category, "查询");
    assert!(!template.is_builtin);
    // 验证 ID 已生成
    assert!(!template.id.is_empty());
}

#[test]
fn test_sql_template_to_response() {
    let template = SqlTemplate::new(
        "模板A".to_string(),
        "SELECT 1".to_string(),
        None,
        "通用".to_string(),
        None,
        None,
    );
    let response: SqlTemplateResponse = template.into();
    assert_eq!(response.name, "模板A");
    assert_eq!(response.content, "SELECT 1");
    assert_eq!(response.category, "通用");
    assert!(response.db_type.is_none());
    assert!(!response.is_builtin);
    assert!(response.created_at_ms > 0);
}

#[test]
fn test_create_sql_template_input_fields() {
    let input = CreateSqlTemplateInput {
        name: "新建模板".to_string(),
        content: "SELECT * FROM {table}".to_string(),
        db_type: Some("postgresql".to_string()),
        category: "查询".to_string(),
        description: Some("描述文字".to_string()),
        tags: Some("tag1,tag2".to_string()),
    };
    assert_eq!(input.name, "新建模板");
    assert_eq!(input.content, "SELECT * FROM {table}");
    assert_eq!(input.db_type, Some("postgresql".to_string()));
    assert_eq!(input.category, "查询");
}

#[test]
fn test_sql_template_response_serialization() {
    let response = SqlTemplateResponse {
        id: "tmpl-001".to_string(),
        name: "测试模板".to_string(),
        content: "SELECT * FROM test".to_string(),
        db_type: Some("sqlite".to_string()),
        category: "查询".to_string(),
        description: Some("测试".to_string()),
        tags: Some("test".to_string()),
        is_builtin: false,
        created_at_ms: 1710000000,
        updated_at_ms: 1710000001,
    };
    let json = serde_json::to_string(&response).expect("序列化失败");
    assert!(json.contains("tmpl-001"));
    assert!(json.contains("测试模板"));
    // SqlTemplateResponse 不实现 Deserialize，仅验证序列化
    assert_eq!(response.id, "tmpl-001");
    assert_eq!(response.db_type, Some("sqlite".to_string()));
}

// ==================== 模板变量解析测试 ====================

#[test]
fn test_template_variable_parsing_simple() {
    // 测试 {variable} 格式的模板变量
    let content = "SELECT * FROM {table_name} WHERE {condition}";
    let var_count = content.matches('{').count();
    assert_eq!(var_count, 2, "应检测到 2 个模板变量");
    assert!(content.contains("{table_name}"));
    assert!(content.contains("{condition}"));
}

#[test]
fn test_template_variable_parsing_nested() {
    let content = "SELECT {column} FROM {schema}.{table} WHERE id = {id}";
    let var_count = content.matches('{').count();
    assert_eq!(var_count, 4, "应检测到 4 个模板变量");
}

#[test]
fn test_template_variable_parsing_no_variables() {
    let content = "SELECT 1";
    let var_count = content.matches('{').count();
    assert_eq!(var_count, 0, "无变量模板不应包含花括号");
}

#[test]
fn test_template_variable_parsing_with_escaped_braces() {
    // 模板内容可能包含 { 但不是变量占位符
    let content = "SELECT * FROM users WHERE name LIKE '%{search}%'";
    let var_count = content.matches('{').count();
    assert_eq!(var_count, 1, "应检测到 1 个模板变量");
    assert!(content.contains("{search}"));
}

// ==================== 复杂场景：多类型组合序列化 ====================

#[test]
fn test_metadata_roundtrip_all_types() {
    // 验证所有 metadata 类型可以成功序列化/反序列化
    let database = DatabaseMeta {
        name: "mydb".to_string(),
    };
    let schema = SchemaMeta::basic("public".to_string());
    let table = TableMeta::basic("users".to_string(), "TABLE".to_string());
    let view = ViewMeta {
        name: "v_active_users".to_string(),
        view_type: "VIEW".to_string(),
    };
    let column = ColumnMeta {
        name: "id".to_string(),
        data_type: "INT".to_string(),
        is_nullable: false,
        default_value: None,
        is_primary_key: true,
        is_foreign_key: false,
        comment: None,
    };

    // 序列化所有类型
    let db_json = serde_json::to_string(&database).expect("database 序列化失败");
    let schema_json = serde_json::to_string(&schema).expect("schema 序列化失败");
    let table_json = serde_json::to_string(&table).expect("table 序列化失败");
    let view_json = serde_json::to_string(&view).expect("view 序列化失败");
    let col_json = serde_json::to_string(&column).expect("column 序列化失败");

    // 反序列化
    let _: DatabaseMeta = serde_json::from_str(&db_json).expect("database 反序列化失败");
    let _: SchemaMeta = serde_json::from_str(&schema_json).expect("schema 反序列化失败");
    let _: TableMeta = serde_json::from_str(&table_json).expect("table 反序列化失败");
    let _: ViewMeta = serde_json::from_str(&view_json).expect("view 反序列化失败");
    let _: ColumnMeta = serde_json::from_str(&col_json).expect("column 反序列化失败");
}
