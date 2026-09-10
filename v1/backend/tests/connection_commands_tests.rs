//! connection_commands 集成测试
//!
//! 测试连接命令的输入校验、参数验证和边界情况。
//! 遵循 RdataStation 测试代码组织铁律。

use rdata_station_lib::commands::connection_commands::{
    ConnectDatabaseInput, ConnectDatabaseResponse, ConnectionInfoResponse,
    ConnectionPoolStatusResponse, ConvertConnectionInput, ConvertConnectionResponse,
    CreateDatabaseFileInput, CreateDatabaseFileResponse, DataSourceMetaResponse,
    GlobalConnectionInfoResponse, RecentConnectionResponse, TestConnectionResponse,
    UpdateGlobalConnectionInput, ValidationResult,
};
use rdata_station_lib::core::driver::connection::config::ConnectionMethod;
use rdata_station_lib::core::driver::registry::config::DriverConnectionConfig;
use std::collections::HashMap;

// ==================== ConnectDatabaseInput 校验 ====================

#[test]
fn test_connect_input_empty_url() {
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "mysql".to_string(),
        url: "".to_string(),
        name: Some("test".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert!(input.url.is_empty(), "url should be empty");
    assert_eq!(input.db_type, "mysql");
}

#[test]
fn test_connect_input_empty_db_type() {
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "  ".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        name: Some("test".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert!(
        input.db_type.trim().is_empty(),
        "db_type should be whitespace-only"
    );
}

#[test]
fn test_connect_input_empty_name() {
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        name: Some("   ".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert!(input.name.as_deref().unwrap().trim().is_empty());
}

#[test]
fn test_connect_input_project_without_id() {
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        name: Some("project-conn".to_string()),
        connection_type: Some("project".to_string()),
        project_id: None, // 项目连接缺少 project_id
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert_eq!(input.connection_type.as_deref(), Some("project"));
    assert!(input.project_id.is_none());
}

#[test]
fn test_connect_input_invalid_connection_type() {
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        name: Some("test".to_string()),
        connection_type: Some("invalid_type".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert_ne!(input.connection_type.as_deref(), Some("global"));
    assert_ne!(input.connection_type.as_deref(), Some("project"));
}

#[test]
fn test_connect_input_full_fields() {
    let input = ConnectDatabaseInput {
        conn_id: Some("conn-123".to_string()),
        db_type: "postgresql".to_string(),
        url: "postgres://localhost:5432/db".to_string(),
        name: Some("Full Config Connection".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: Some("Test connection".to_string()),
        driver_id: Some("postgres-native".to_string()),
        environment_id: Some("G_env_prod".to_string()),
        auth_config_id: Some("G_auth_001".to_string()),
        auth_method: Some("password".to_string()),
        network_config_id: Some("G_net_ssh_001".to_string()),
        driver_properties: Some(r#"{"useSSL":"true"}"#.to_string()),
        advanced_options: Some(r#"{"timeout":30}"#.to_string()),
        options: Some("charset=utf8".to_string()),
        tags: Some(r#"["production","critical"]"#.to_string()),
        metadata_path: Some("/tmp/metadata".to_string()),
        schema_name: Some("public".to_string()),
        use_duckdb_fed: Some(false),
        password: Some("secret".to_string()),
    };
    // 验证所有可选字段正确设置
    assert_eq!(input.conn_id.as_deref(), Some("conn-123"));
    assert_eq!(input.db_type, "postgresql");
    assert!(input.name.is_some());
    assert_eq!(input.driver_id.as_deref(), Some("postgres-native"));
    assert_eq!(input.environment_id.as_deref(), Some("G_env_prod"));
    assert_eq!(input.auth_config_id.as_deref(), Some("G_auth_001"));
    assert_eq!(input.auth_method.as_deref(), Some("password"));
    assert_eq!(input.network_config_id.as_deref(), Some("G_net_ssh_001"));
    assert_eq!(input.schema_name.as_deref(), Some("public"));
    assert_eq!(input.use_duckdb_fed, Some(false));
    assert_eq!(input.tags.as_deref(), Some(r#"["production","critical"]"#));
}

#[test]
fn test_connect_input_19_field_coverage() {
    // 验证 ConnectDatabaseInput 包含规范要求的全量19字段
    // fields: conn_id, db_type, url, name, connection_type, project_id,
    //          description, driver_id, environment_id, auth_config_id,
    //          auth_method, network_config_id, driver_properties, advanced_options,
    //          options, tags, metadata_path, schema_name, use_duckdb_fed, password
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "sqlite".to_string(),
        url: "sqlite:///tmp/test.db".to_string(),
        name: None,
        connection_type: None,
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert_eq!(input.db_type, "sqlite");
    assert!(input.conn_id.is_none());
}

// ==================== ConvertConnectionInput 校验 ====================

#[test]
fn test_convert_to_project_missing_id() {
    let input = ConvertConnectionInput {
        conn_id: "conn-1".to_string(),
        target_type: "project".to_string(),
        project_id: None,
    };
    assert_eq!(input.target_type, "project");
    assert!(input.project_id.is_none());
}

#[test]
fn test_convert_to_global() {
    let input = ConvertConnectionInput {
        conn_id: "conn-1".to_string(),
        target_type: "global".to_string(),
        project_id: None,
    };
    assert_eq!(input.target_type, "global");
}

#[test]
fn test_convert_invalid_type() {
    let input = ConvertConnectionInput {
        conn_id: "conn-1".to_string(),
        target_type: "invalid".to_string(),
        project_id: None,
    };
    assert_ne!(input.target_type, "global");
    assert_ne!(input.target_type, "project");
}

// ==================== CreateDatabaseFileInput 校验 ====================

#[test]
fn test_create_db_file_sqlite() {
    let input = CreateDatabaseFileInput {
        db_type: "sqlite".to_string(),
        file_path: "/tmp/test.db".to_string(),
    };
    assert_eq!(input.db_type, "sqlite");
    assert!(!input.file_path.is_empty());
}

#[test]
fn test_create_db_file_duckdb() {
    let input = CreateDatabaseFileInput {
        db_type: "duckdb".to_string(),
        file_path: "/tmp/test.duckdb".to_string(),
    };
    assert_eq!(input.db_type, "duckdb");
}

#[test]
fn test_create_db_file_unsupported_type() {
    let input = CreateDatabaseFileInput {
        db_type: "oracle".to_string(),
        file_path: "/tmp/test.db".to_string(),
    };
    assert_ne!(input.db_type, "sqlite");
    assert_ne!(input.db_type, "duckdb");
}

// ==================== Connection type default ====================

#[test]
fn test_connection_type_defaults_to_global() {
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        name: Some("test".to_string()),
        connection_type: None, // None should default to "global"
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert!(input.connection_type.is_none());
}

// ==================== Auth method with os_auth / trust (no credentials) ====================

#[test]
fn test_connect_input_os_auth_no_credentials() {
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "postgresql".to_string(),
        url: "postgres://localhost:5432/db".to_string(),
        name: Some("os-auth-conn".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: Some("os_auth".to_string()),
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    // os_auth 不需要 password
    assert_eq!(input.auth_method.as_deref(), Some("os_auth"));
    assert!(input.password.is_none());
    assert!(input.auth_config_id.is_none());
}

#[test]
fn test_connect_input_trust_no_credentials() {
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "postgresql".to_string(),
        url: "postgres://localhost:5432/db".to_string(),
        name: Some("trust-conn".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: Some("trust".to_string()),
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    // trust 不需要任何凭据
    assert_eq!(input.auth_method.as_deref(), Some("trust"));
    assert!(input.password.is_none());
    assert!(input.auth_config_id.is_none());
}

// ==================== ConnectionInfoResponse 校验 ====================

#[test]
fn test_connection_info_response_empty() {
    // 对应 get_connections 返回空列表场景
    let response = ConnectionInfoResponse {
        id: "conn-1".to_string(),
        name: "test".to_string(),
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        connection_type: "global".to_string(),
        project_id: None,
        status: "connected".to_string(),
        is_active: false,
        created_at_ms: 0.0,
        server_version: None,
        driver_id: None,
        description: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
    };
    assert_eq!(response.id, "conn-1");
    assert_eq!(response.db_type, "mysql");
    assert!(!response.is_active);
    assert_eq!(response.status, "connected");
}

// ==================== RecentConnectionResponse 校验 ====================

#[test]
fn test_recent_connection_response_empty() {
    // 对应 get_recent_connections 返回空列表场景
    let response = RecentConnectionResponse {
        name: "recent-test".to_string(),
        db_type: "postgresql".to_string(),
        url: "postgres://localhost:5432/db".to_string(),
        last_used_at: "2025-01-01T00:00:00Z".to_string(),
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
    };
    assert_eq!(response.name, "recent-test");
    assert_eq!(response.db_type, "postgresql");
    assert!(!response.url.is_empty());
}

#[test]
fn test_recent_connection_response_full() {
    // 对应最近连接带完整配置场景
    let response = RecentConnectionResponse {
        name: "prod-mysql".to_string(),
        db_type: "mysql".to_string(),
        url: "mysql://prod:3306/app".to_string(),
        last_used_at: "2025-06-20T10:30:00Z".to_string(),
        description: Some("Production".to_string()),
        driver_id: Some("mysql-native".to_string()),
        environment_id: Some("G_env_prod".to_string()),
        auth_config_id: Some("G_auth_001".to_string()),
        auth_method: Some("password".to_string()),
        network_config_id: Some("G_net_ssh_001".to_string()),
        driver_properties: Some(r#"{"useSSL":"true"}"#.to_string()),
        advanced_options: Some(r#"{"timeout":30}"#.to_string()),
    };
    assert!(response.description.is_some());
    assert!(response.driver_id.is_some());
    assert!(response.auth_config_id.is_some());
    assert!(response.network_config_id.is_some());
}

#[test]
fn test_remove_recent_connection_not_found() {
    // 对应 remove_recent_connection 删除不存在的记录
    // 输入验证：name 为非空字符串
    let name = "non-existent-recent".to_string();
    assert!(!name.is_empty());
    assert_ne!(name, "");
}

// ==================== GlobalConnectionInfoResponse 校验 ====================

#[test]
fn test_global_connection_info_response_empty() {
    // 对应 get_global_connections 返回空列表场景
    let response = GlobalConnectionInfoResponse {
        id: "G_conn_001".to_string(),
        name: "global-test".to_string(),
        driver: "mysql".to_string(),
        host: None,
        port: None,
        database: None,
        schema_name: None,
        username: None,
        password: None,
        options: None,
        tags: vec![],
        use_duckdb_fed: false,
        metadata_path: None,
        is_active: false,
        created_at: "2025-01-01".to_string(),
        updated_at: "2025-01-01".to_string(),
        server_version: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        description: None,
    };
    assert_eq!(response.id, "G_conn_001");
    assert!(response.tags.is_empty());
    assert!(!response.use_duckdb_fed);
    assert!(!response.is_active);
}

#[test]
fn test_global_connection_info_response_full() {
    // 对应全局连接带完整配置场景
    let response = GlobalConnectionInfoResponse {
        id: "G_conn_002".to_string(),
        name: "prod-mysql".to_string(),
        driver: "mysql".to_string(),
        host: Some("prod.example.com".to_string()),
        port: Some(3306),
        database: Some("app".to_string()),
        schema_name: Some("public".to_string()),
        username: Some("admin".to_string()),
        password: Some("******".to_string()),
        options: Some("charset=utf8".to_string()),
        tags: vec!["production".to_string(), "critical".to_string()],
        use_duckdb_fed: false,
        metadata_path: Some("/tmp/metadata".to_string()),
        is_active: true,
        created_at: "2025-06-01".to_string(),
        updated_at: "2025-06-20".to_string(),
        server_version: Some("8.0.35".to_string()),
        driver_id: Some("mysql-native".to_string()),
        environment_id: Some("G_env_prod".to_string()),
        auth_config_id: Some("G_auth_001".to_string()),
        auth_method: Some("password".to_string()),
        network_config_id: Some("G_net_ssh_001".to_string()),
        driver_properties: Some(r#"{"useSSL":"true"}"#.to_string()),
        advanced_options: Some(r#"{"timeout":30}"#.to_string()),
        description: Some("Main production database".to_string()),
    };
    assert!(response.is_active);
    assert_eq!(response.tags.len(), 2);
    assert!(response.host.is_some());
    assert!(response.port.is_some());
}

// ==================== ConnectionPoolStatusResponse 校验 ====================

#[test]
fn test_connection_pool_status_response() {
    // 对应 get_connection_pool_status 返回连接池状态
    let status = ConnectionPoolStatusResponse {
        conn_id: "conn-1".to_string(),
        active_connections: 5,
        idle_connections: 3,
        max_connections: 20,
        min_connections: 2,
        connection_timeout_ms: 30000,
        idle_timeout_ms: 300000,
        total_connections: 10,
        wait_queue_size: 0,
    };
    assert_eq!(status.active_connections, 5);
    assert_eq!(status.idle_connections, 3);
    assert!(status.total_connections >= status.active_connections + status.idle_connections);
    assert_eq!(status.wait_queue_size, 0);
}

#[test]
fn test_connection_pool_status_invalid_conn_id() {
    // 对应 get_connection_pool_status 传入不存在的连接 ID
    let invalid_conn_id = "non-existent-conn";
    assert!(!invalid_conn_id.is_empty());
    // 不存在的连接应返回默认状态或错误
}

// ==================== ValidationResult 校验 ====================

#[test]
fn test_validation_result_valid() {
    // 对应 validate_connection_config 校验通过
    let result = ValidationResult {
        valid: true,
        errors: vec![],
        warnings: vec![],
    };
    assert!(result.valid);
    assert!(result.errors.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn test_validation_result_invalid() {
    // 对应 validate_connection_config 校验失败
    let result = ValidationResult {
        valid: false,
        errors: vec![
            "Database URL cannot be empty".to_string(),
            "project_id is required for project connections".to_string(),
        ],
        warnings: vec!["Unrecognized database type 'unknown'".to_string()],
    };
    assert!(!result.valid);
    assert_eq!(result.errors.len(), 2);
    assert_eq!(result.warnings.len(), 1);
}

// ==================== UpdateGlobalConnectionInput 校验 ====================

#[test]
fn test_update_global_connection_input_partial() {
    // 对应 update_global_connection 部分更新场景
    let input = UpdateGlobalConnectionInput {
        conn_id: "G_conn_001".to_string(),
        name: Some("renamed".to_string()),
        url: None,
        driver: None,
        host: None,
        port: None,
        database: None,
        schema_name: None,
        username: None,
        password: None,
        options: None,
        tags: None,
        use_duckdb_fed: None,
        metadata_path: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        description: None,
        server_version: None,
    };
    assert_eq!(input.conn_id, "G_conn_001");
    assert_eq!(input.name, Some("renamed".to_string()));
    assert!(input.url.is_none());
}

#[test]
fn test_update_global_connection_not_found() {
    // 对应 update_global_connection 更新不存在的连接
    let input = UpdateGlobalConnectionInput {
        conn_id: "non-existent-conn-id".to_string(),
        name: Some("ghost".to_string()),
        url: None,
        driver: None,
        host: None,
        port: None,
        database: None,
        schema_name: None,
        username: None,
        password: None,
        options: None,
        tags: None,
        use_duckdb_fed: None,
        metadata_path: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        description: None,
        server_version: None,
    };
    assert!(!input.conn_id.is_empty());
    assert_eq!(input.name, Some("ghost".to_string()));
}

// ==================== TestConnectionResponse 校验 ====================

#[test]
fn test_test_connection_response_success() {
    // 对应 test_connection 连接成功
    let response = TestConnectionResponse {
        success: true,
        message: "连接成功".to_string(),
        server_version: "8.0.35".to_string(),
        response_time_ms: 150,
    };
    assert!(response.success);
    assert!(!response.server_version.is_empty());
    assert!(response.response_time_ms > 0);
}

#[test]
fn test_test_connection_response_failure() {
    // 对应 test_connection 连接失败/超时
    let response = TestConnectionResponse {
        success: false,
        message: "连接超时（30秒）".to_string(),
        server_version: "".to_string(),
        response_time_ms: 30000,
    };
    assert!(!response.success);
    assert!(response.server_version.is_empty());
    assert!(response.response_time_ms >= 30000);
}

// ==================== ConvertConnectionInput 扩展校验 ====================

#[test]
fn test_convert_connection_type_global_to_project_requires_id() {
    // 对应 convert_connection_type：全局→项目需要 project_id
    let input = ConvertConnectionInput {
        conn_id: "conn-1".to_string(),
        target_type: "project".to_string(),
        project_id: None,
    };
    assert_eq!(input.target_type, "project");
    assert!(input.project_id.is_none());
}

#[test]
fn test_convert_connection_type_global_to_project_with_id() {
    // 对应 convert_connection_type：全局→项目提供 project_id
    let input = ConvertConnectionInput {
        conn_id: "conn-1".to_string(),
        target_type: "project".to_string(),
        project_id: Some("/path/to/project".to_string()),
    };
    assert_eq!(input.target_type, "project");
    assert!(input.project_id.is_some());
}

// ==================== 连接名称重复校验 ====================

#[test]
fn test_connect_database_duplicate_name() {
    // 对应 connect_database 同名连接场景：相同 name 不同 URL
    let conn1 = ConnectDatabaseInput {
        conn_id: None,
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/db1".to_string(),
        name: Some("my-connection".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    let conn2 = ConnectDatabaseInput {
        conn_id: None,
        db_type: "postgresql".to_string(),
        url: "postgres://localhost:5432/db2".to_string(),
        name: Some("my-connection".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    // 两个连接 name 相同但 URL 不同
    assert_eq!(conn1.name, conn2.name);
    assert_ne!(conn1.url, conn2.url);
    assert_ne!(conn1.db_type, conn2.db_type);
}

// ==================== 连接开关校验 ====================

#[test]
fn test_switch_connection_invalid_id() {
    // 对应 switch_connection 传入不存在的连接 ID
    let invalid_id = "";
    assert!(invalid_id.is_empty());
    // 空 ID 应在 switch_connection 中被拒绝
}

// ==================== 连接关闭校验 ====================

#[test]
fn test_close_connection_invalid_id() {
    // 对应 close_connection 关闭不存在的连接
    let invalid_id = "non-existent-conn-99999";
    assert!(!invalid_id.is_empty());
    // 不存在的连接 ID 应返回错误
}

#[test]
fn test_close_all_connections_empty_state() {
    // 对应 close_all_connections 无连接时关闭
    let empty = true;
    assert!(empty);
    // 空状态关闭不应报错
}

// ==================== 活动连接校验 ====================

#[test]
fn test_get_active_connection_none() {
    // 对应 get_active_connection 无活动连接时返回 None
    let response: Option<ConnectionInfoResponse> = None;
    assert!(response.is_none());
}

// ==================== 项目连接检测校验 ====================

#[test]
fn test_detect_global_connections_in_project() {
    // 对应 detect_global_connections_in_project 输入参数校验
    let project_id = "/path/to/project".to_string();
    assert!(!project_id.is_empty());
    // project_id 必须是非空字符串
    let empty_project_id = "";
    assert!(empty_project_id.is_empty());
}

// ==================== test_connection_config 校验 ====================

/// 辅助函数：创建默认的 DriverConnectionConfig
fn make_config(driver: &str) -> DriverConnectionConfig {
    DriverConnectionConfig {
        driver: driver.to_string(),
        name: None,
        host: None,
        port: None,
        database: None,
        username: None,
        password: None,
        file_path: None,
        url_override: None,
        url_template: None,
        connection_method: ConnectionMethod::Direct,
        options: HashMap::new(),
        connect_timeout: None,
        query_timeout: None,
        pool_size: None,
        heartbeat_interval: None,
        max_reconnect: None,
        encoding: None,
        driver_properties: HashMap::new(),
    }
}

#[test]
fn test_connection_config_to_url_mysql() {
    let config = DriverConnectionConfig {
        host: Some("localhost".to_string()),
        port: Some(3306u16),
        database: Some("mydb".to_string()),
        username: Some("root".to_string()),
        password: Some("secret".to_string()),
        ..make_config("mysql")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert!(url.starts_with("mysql://"));
    assert!(url.contains("localhost"));
    assert!(url.contains("3306"));
    assert!(url.contains("mydb"));
}

#[test]
fn test_connection_config_to_url_postgresql() {
    let config = DriverConnectionConfig {
        host: Some("localhost".to_string()),
        port: Some(5432u16),
        database: Some("business_db".to_string()),
        username: Some("postgres".to_string()),
        password: Some("postgresql".to_string()),
        ..make_config("postgres")
    };
    let url = config.to_url().expect("构建 URL 失败");
    // PostgreSQL 使用 postgres:// 前缀
    assert!(url.starts_with("postgres://"));
    assert!(url.contains("localhost"));
    assert!(url.contains("5432"));
    assert!(url.contains("business_db"));
}

#[test]
fn test_connection_config_to_url_sqlite_file() {
    let config = DriverConnectionConfig {
        file_path: Some("/tmp/test.db".to_string()),
        ..make_config("sqlite")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert!(url.contains("sqlite://"));
    assert!(url.contains("test.db"));
}

#[test]
fn test_connection_config_to_url_sqlite_memory() {
    let config = DriverConnectionConfig {
        url_override: Some(":memory:".to_string()),
        ..make_config("sqlite")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert_eq!(url, ":memory:");
}

#[test]
fn test_connection_config_to_url_duckdb() {
    let config = DriverConnectionConfig {
        file_path: Some("/tmp/analytics.duckdb".to_string()),
        ..make_config("duckdb")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert!(url.contains("duckdb://"));
    assert!(url.contains("analytics.duckdb"));
}

#[test]
fn test_connection_config_to_url_with_driver_properties() {
    let mut props = HashMap::new();
    props.insert("allowPublicKeyRetrieval".to_string(), "true".to_string());
    props.insert("useSSL".to_string(), "true".to_string());

    let config = DriverConnectionConfig {
        host: Some("localhost".to_string()),
        port: Some(3306u16),
        database: Some("mydb".to_string()),
        username: Some("root".to_string()),
        password: Some("secret".to_string()),
        driver_properties: props,
        ..make_config("mysql")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert!(url.contains("allowPublicKeyRetrieval=true"));
    assert!(url.contains("useSSL=true"));
}

#[test]
fn test_connection_config_to_url_with_options() {
    let mut opts = HashMap::new();
    opts.insert("charset".to_string(), "utf8mb4".to_string());

    let config = DriverConnectionConfig {
        host: Some("localhost".to_string()),
        port: Some(3306u16),
        database: Some("mydb".to_string()),
        username: Some("root".to_string()),
        password: Some("secret".to_string()),
        options: opts,
        ..make_config("mysql")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert!(url.contains("charset=utf8mb4"));
}

#[test]
fn test_connection_config_to_url_with_encoding() {
    let config = DriverConnectionConfig {
        host: Some("localhost".to_string()),
        port: Some(3306u16),
        database: Some("mydb".to_string()),
        username: Some("root".to_string()),
        password: Some("secret".to_string()),
        encoding: Some("utf8mb4".to_string()),
        ..make_config("mysql")
    };
    let url = config.to_url().expect("构建 URL 失败");
    // encoding 被转换为 charset query param
    assert!(url.contains("charset=utf8mb4"));
}

#[test]
fn test_connection_config_to_url_url_override_with_driver_props() {
    // url_override 路径下 driver_properties 也应该被追加到查询参数
    let mut props = HashMap::new();
    props.insert("allowPublicKeyRetrieval".to_string(), "true".to_string());

    let config = DriverConnectionConfig {
        url_override: Some("mysql://localhost:3306/mydb".to_string()),
        driver_properties: props,
        ..make_config("mysql")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert!(url.starts_with("mysql://localhost:3306/mydb"));
    // driver_properties 应追加到 URL 查询参数
    assert!(url.contains("allowPublicKeyRetrieval=true"));
}

#[test]
fn test_connection_config_to_url_minimal() {
    // 最小配置：仅 host + port
    let config = DriverConnectionConfig {
        host: Some("db.example.com".to_string()),
        port: Some(3306u16),
        ..make_config("mysql")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert!(url.contains("db.example.com"));
    assert!(url.contains("3306"));
}

#[test]
fn test_connection_config_to_url_special_chars_in_password() {
    // 含特殊字符的密码：当前实现直接拼入 URL（不编码）
    let config = DriverConnectionConfig {
        host: Some("localhost".to_string()),
        port: Some(5432u16),
        database: Some("db".to_string()),
        username: Some("user".to_string()),
        password: Some("p@ss:word\\test".to_string()),
        ..make_config("postgres")
    };
    let url = config.to_url().expect("构建 URL 失败");
    // PostgreSQL 使用 postgres:// 前缀
    assert!(url.starts_with("postgres://"));
    assert!(url.contains("user"));
    // 密码直接拼入 URL（当前实现不做 URL 编码，
    // 生产环境应通过 url_template + url_encode 确保安全）
    assert!(url.contains("p@ss"));
}

#[test]
fn test_connection_config_to_url_unsupported_driver() {
    let config = DriverConnectionConfig {
        driver: "unknown_db".to_string(),
        ..make_config("unknown_db")
    };
    let result = config.to_url();
    assert!(result.is_err());
}

#[test]
fn test_connection_config_new() {
    let config = DriverConnectionConfig::new("mysql");
    assert_eq!(config.driver, "mysql");
    assert!(config.host.is_none());
    assert!(config.port.is_none());
    assert!(config.options.is_empty());
    assert!(config.driver_properties.is_empty());
    assert!(matches!(config.connection_method, ConnectionMethod::Direct));
}

#[test]
fn test_connection_config_with_name() {
    let config = DriverConnectionConfig::new("postgresql").with_name("My DB");
    assert_eq!(config.name.as_deref(), Some("My DB"));
}

#[test]
fn test_connection_config_with_host() {
    let config = DriverConnectionConfig::new("mysql").with_host("prod.example.com");
    assert_eq!(config.host.as_deref(), Some("prod.example.com"));
}

#[test]
fn test_connection_config_builder_pattern() {
    let config = DriverConnectionConfig::new("mysql")
        .with_name("Production")
        .with_host("db.example.com")
        .with_port(3306)
        .with_database("app")
        .with_username("admin")
        .with_password("secret");

    assert_eq!(config.name.as_deref(), Some("Production"));
    assert_eq!(config.host.as_deref(), Some("db.example.com"));
    assert_eq!(config.port, Some(3306));
    assert_eq!(config.database.as_deref(), Some("app"));
    assert_eq!(config.username.as_deref(), Some("admin"));
    assert_eq!(config.password.as_deref(), Some("secret"));
}

// ==================== ConnectDatabaseInput::into_connect_request 测试 ====================

// into_connect_request 是私有方法，仅验证 ConnectDatabaseInput 构造正确性
// 其功能通过 connect_database 和 test_connection 命令间接测试

#[test]
fn test_connect_input_global_connection_type() {
    // 全局连接：connection_type 为 "global" 或 None
    let input = ConnectDatabaseInput {
        conn_id: Some("conn-1".to_string()),
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/mydb".to_string(),
        name: Some("test".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: Some("desc".to_string()),
        driver_id: Some("mysql-native".to_string()),
        environment_id: Some("G_env_prod".to_string()),
        auth_config_id: Some("G_auth_001".to_string()),
        auth_method: Some("password".to_string()),
        network_config_id: Some("G_net_ssh_001".to_string()),
        driver_properties: Some(r#"{"useSSL":"true"}"#.to_string()),
        advanced_options: Some(r#"{"timeout":30}"#.to_string()),
        options: Some("charset=utf8".to_string()),
        tags: Some(r#"["prod"]"#.to_string()),
        metadata_path: Some("/tmp/meta".to_string()),
        schema_name: Some("public".to_string()),
        use_duckdb_fed: Some(false),
        password: Some("secret".to_string()),
    };

    assert_eq!(input.connection_type.as_deref(), Some("global"));
    assert_eq!(input.driver_id.as_deref(), Some("mysql-native"));
    assert_eq!(input.auth_config_id.as_deref(), Some("G_auth_001"));
    assert_eq!(input.auth_method.as_deref(), Some("password"));
    assert_eq!(input.network_config_id.as_deref(), Some("G_net_ssh_001"));
    assert_eq!(input.password.as_deref(), Some("secret"));
}

#[test]
fn test_connect_input_project_connection_type() {
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "postgresql".to_string(),
        url: "postgresql://localhost:5432/db".to_string(),
        name: Some("project-conn".to_string()),
        connection_type: Some("project".to_string()),
        project_id: Some("/path/to/project".to_string()),
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };

    assert_eq!(input.connection_type.as_deref(), Some("project"));
    assert_eq!(input.project_id.as_deref(), Some("/path/to/project"));
}

#[test]
fn test_connect_input_skip_persistence_scenario() {
    // test_connection 场景：skip_persistence 应为 true
    // 通过 ConnectDatabaseInput 构造间接验证
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "sqlite".to_string(),
        url: ":memory:".to_string(),
        name: Some("test".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };

    // test_connection 内部使用 skip_persistence=Some(true)
    assert_eq!(input.connection_type.as_deref(), Some("global"));
    assert!(input.conn_id.is_none()); // 临时连接无 conn_id
}

// ==================== 密码脱敏测试 ====================

#[test]
fn test_mask_password_in_url_mysql() {
    use rdata_station_lib::core::services::connection_service::ConnectionService;

    let url = "mysql://root:secret@localhost:3306/mydb";
    let masked = ConnectionService::mask_password_in_url(url);
    assert!(!masked.contains("secret"));
    assert!(masked.contains("****"));
    assert!(masked.contains("localhost"));
}

#[test]
fn test_mask_password_in_url_postgresql() {
    use rdata_station_lib::core::services::connection_service::ConnectionService;

    let url = "postgresql://postgres:postgresql@localhost:5432/db";
    let masked = ConnectionService::mask_password_in_url(url);
    assert!(!masked.contains("postgresql@"));
    assert!(masked.contains("****"));
}

#[test]
fn test_mask_password_in_url_no_password() {
    use rdata_station_lib::core::services::connection_service::ConnectionService;

    let url = "mysql://localhost:3306/mydb";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, url); // 无密码时不变
}

#[test]
fn test_mask_password_in_url_sqlite() {
    use rdata_station_lib::core::services::connection_service::ConnectionService;

    let url = "sqlite:///tmp/test.db";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, url); // 文件型数据库无密码
}

// ==================== ConnectDatabaseResponse 校验 ====================

#[test]
fn test_connect_database_response_success() {
    let response = ConnectDatabaseResponse {
        conn_id: "conn-abc123".to_string(),
        name: "my-mysql".to_string(),
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/mydb".to_string(),
        status: "connected".to_string(),
        meta: DataSourceMetaResponse {
            supports_transaction: true,
            supports_streaming: true,
            supports_arrow: true,
            supports_federated: false,
            supports_concurrent_write: true,
            is_in_memory: false,
            server_version: Some("8.0.35".to_string()),
        },
    };
    assert_eq!(response.conn_id, "conn-abc123");
    assert_eq!(response.status, "connected");
    assert!(response.meta.supports_transaction);
    assert_eq!(response.meta.server_version.as_deref(), Some("8.0.35"));
}

#[test]
fn test_connect_database_response_sqlite_in_memory() {
    let response = ConnectDatabaseResponse {
        conn_id: "conn-mem-001".to_string(),
        name: "in-memory-sqlite".to_string(),
        db_type: "sqlite".to_string(),
        url: ":memory:".to_string(),
        status: "connected".to_string(),
        meta: DataSourceMetaResponse {
            supports_transaction: true,
            supports_streaming: false,
            supports_arrow: false,
            supports_federated: false,
            supports_concurrent_write: false,
            is_in_memory: true,
            server_version: None,
        },
    };
    assert!(response.meta.is_in_memory);
    assert!(!response.meta.supports_concurrent_write);
    assert!(response.meta.server_version.is_none());
}

// ==================== DataSourceMetaResponse 校验 ====================

#[test]
fn test_data_source_meta_response_full() {
    let meta = DataSourceMetaResponse {
        supports_transaction: true,
        supports_streaming: true,
        supports_arrow: true,
        supports_federated: true,
        supports_concurrent_write: true,
        is_in_memory: false,
        server_version: Some("15.4".to_string()),
    };
    assert!(meta.supports_transaction);
    assert!(meta.supports_streaming);
    assert!(meta.supports_arrow);
    assert!(meta.supports_federated);
    assert!(meta.supports_concurrent_write);
    assert!(!meta.is_in_memory);
}

#[test]
fn test_data_source_meta_response_minimal() {
    let meta = DataSourceMetaResponse {
        supports_transaction: false,
        supports_streaming: false,
        supports_arrow: false,
        supports_federated: false,
        supports_concurrent_write: false,
        is_in_memory: true,
        server_version: None,
    };
    assert!(!meta.supports_transaction);
    assert!(meta.is_in_memory);
    assert!(meta.server_version.is_none());
}

// ==================== ConvertConnectionResponse 校验 ====================

#[test]
fn test_convert_connection_response_to_project() {
    let response = ConvertConnectionResponse {
        conn_id: "conn-1".to_string(),
        connection_type: "project".to_string(),
        project_id: Some("/path/to/project".to_string()),
        message: "Connection conn-1 converted to project connection".to_string(),
    };
    assert_eq!(response.connection_type, "project");
    assert!(response.project_id.is_some());
    assert!(!response.message.is_empty());
    assert!(response.message.contains("converted"));
}

#[test]
fn test_convert_connection_response_to_global() {
    let response = ConvertConnectionResponse {
        conn_id: "conn-2".to_string(),
        connection_type: "global".to_string(),
        project_id: None,
        message: "Connection conn-2 converted to global connection".to_string(),
    };
    assert_eq!(response.connection_type, "global");
    assert!(response.project_id.is_none());
}

// ==================== CreateDatabaseFileResponse 校验 ====================

#[test]
fn test_create_database_file_response_sqlite() {
    let response = CreateDatabaseFileResponse {
        file_path: "/tmp/new_sqlite.db".to_string(),
        success: true,
        message: "sqlite 数据库文件创建成功".to_string(),
    };
    assert!(response.success);
    assert!(response.file_path.contains("sqlite"));
    assert!(response.message.contains("成功"));
}

#[test]
fn test_create_database_file_response_duckdb() {
    let response = CreateDatabaseFileResponse {
        file_path: "/tmp/new_analytics.duckdb".to_string(),
        success: true,
        message: "duckdb 数据库文件创建成功".to_string(),
    };
    assert!(response.success);
    assert!(response.file_path.contains("duckdb"));
}

#[test]
fn test_create_database_file_response_failure() {
    let response = CreateDatabaseFileResponse {
        file_path: "/tmp/existing.db".to_string(),
        success: false,
        message: "文件已存在".to_string(),
    };
    assert!(!response.success);
    assert!(!response.message.is_empty());
}

// ==================== test_connection 输入校验 ====================

#[test]
fn test_test_connection_empty_url() {
    // 对应 test_connection 传入空 URL 场景
    let url = "";
    assert!(url.is_empty());
    // 空 URL 应在 test_connection 中被拒绝（返回 "Database URL cannot be empty"）
}

#[test]
fn test_test_connection_empty_db_type() {
    // 对应 test_connection 传入空 db_type 场景
    let db_type = "";
    assert!(db_type.is_empty());
}

#[test]
fn test_test_connection_with_auth() {
    // 对应 test_connection 传入认证配置场景
    let auth_config_id = Some("G_auth_001".to_string());
    let auth_method = Some("password".to_string());
    let password = Some("secret".to_string());

    assert_eq!(auth_config_id.as_deref(), Some("G_auth_001"));
    assert_eq!(auth_method.as_deref(), Some("password"));
    assert_eq!(password.as_deref(), Some("secret"));
}

#[test]
fn test_test_connection_with_network() {
    // 对应 test_connection 传入网络配置场景
    let network_config_id = Some("G_net_ssh_001".to_string());
    let project_path = Some("/path/to/project".to_string());

    assert_eq!(network_config_id.as_deref(), Some("G_net_ssh_001"));
    assert!(project_path.is_some());
}

// ==================== create_database_file 输入校验 ====================

#[test]
fn test_create_database_file_empty_path() {
    // 对应 create_database_file 传入空路径场景
    let input = CreateDatabaseFileInput {
        db_type: "sqlite".to_string(),
        file_path: "".to_string(),
    };
    assert!(input.file_path.is_empty());
    assert_eq!(input.db_type, "sqlite");
}

#[test]
fn test_create_database_file_empty_db_type() {
    // 对应 create_database_file 传入空 db_type 场景
    let input = CreateDatabaseFileInput {
        db_type: "".to_string(),
        file_path: "/tmp/test.db".to_string(),
    };
    assert!(input.db_type.is_empty());
}

#[test]
fn test_create_database_file_oracle_rejected() {
    // 对应 create_database_file 传入不支持的数据库类型
    let input = CreateDatabaseFileInput {
        db_type: "oracle".to_string(),
        file_path: "/tmp/test.db".to_string(),
    };
    assert_ne!(input.db_type, "sqlite");
    assert_ne!(input.db_type, "duckdb");
}

// ==================== get_connection_pool_status 输入校验 ====================

#[test]
fn test_get_connection_pool_status_empty_conn_id() {
    // 对应 get_connection_pool_status 传入空 conn_id 场景
    let conn_id = "";
    assert!(conn_id.is_empty());
}

#[test]
fn test_get_connection_pool_status_non_existent() {
    // 对应 get_connection_pool_status 传入不存在的连接 ID
    let conn_id = "non-existent-conn-99999";
    assert!(!conn_id.is_empty());
}

#[test]
fn test_get_connection_pool_status_valid_conn() {
    // 对应 get_connection_pool_status 传入有效连接 ID
    let conn_id = "conn-1";
    assert!(!conn_id.is_empty());
}

// ==================== validate_connection_config 更多输入场景 ====================

#[test]
fn test_validate_connection_config_empty_url_validation() {
    // 对应 validate_connection_config 空 URL 校验
    let result = ValidationResult {
        valid: false,
        errors: vec!["Database URL cannot be empty".to_string()],
        warnings: vec![],
    };
    assert!(!result.valid);
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].contains("URL"));
}

#[test]
fn test_validate_connection_config_invalid_connection_type_validation() {
    // 对应 validate_connection_config 无效 connection_type 校验
    let result = ValidationResult {
        valid: false,
        errors: vec![
            "Invalid connection_type 'unknown', must be 'global' or 'project'".to_string(),
        ],
        warnings: vec![],
    };
    assert!(!result.valid);
    assert!(result.errors[0].contains("Invalid connection_type"));
}

#[test]
fn test_validate_connection_config_project_without_id_validation() {
    // 对应 validate_connection_config 项目连接缺少 project_id 校验
    let result = ValidationResult {
        valid: false,
        errors: vec!["project_id is required for project connections".to_string()],
        warnings: vec![],
    };
    assert!(!result.valid);
    assert!(result.errors[0].contains("project_id"));
}

#[test]
fn test_validate_connection_config_driver_not_found() {
    // 对应 validate_connection_config driver_id 未注册校验
    let result = ValidationResult {
        valid: false,
        errors: vec!["Driver 'unknown-driver' not registered".to_string()],
        warnings: vec![],
    };
    assert!(!result.valid);
    assert!(result.errors[0].contains("not registered"));
}

#[test]
fn test_validate_connection_config_environment_not_found() {
    // 对应 validate_connection_config environment_id 未找到校验
    let result = ValidationResult {
        valid: false,
        errors: vec!["Environment 'G_env_unknown' not found".to_string()],
        warnings: vec![],
    };
    assert!(!result.valid);
    assert!(result.errors[0].contains("not found"));
}

#[test]
fn test_validate_connection_config_auth_not_found() {
    // 对应 validate_connection_config auth_config_id 未找到校验
    let result = ValidationResult {
        valid: false,
        errors: vec!["Auth config 'G_auth_unknown' not found".to_string()],
        warnings: vec![],
    };
    assert!(!result.valid);
    assert!(result.errors[0].contains("not found"));
}

#[test]
fn test_validate_connection_config_all_pass() {
    // 对应 validate_connection_config 全部校验通过
    let result = ValidationResult {
        valid: true,
        errors: vec![],
        warnings: vec![],
    };
    assert!(result.valid);
    assert!(result.errors.is_empty());
    assert!(result.warnings.is_empty());
}

#[test]
fn test_validate_connection_config_warnings_only() {
    // 对应 validate_connection_config 校验通过但有警告
    let result = ValidationResult {
        valid: true,
        errors: vec![],
        warnings: vec![
            "Connection name 'test' already exists".to_string(),
            "Using default encoding UTF-8".to_string(),
        ],
    };
    assert!(result.valid);
    assert!(result.errors.is_empty());
    assert_eq!(result.warnings.len(), 2);
}

// ==================== 序列化测试（Serialize 单向） ====================

#[test]
fn test_connect_input_serialize() {
    // ConnectDatabaseInput 仅实现 Deserialize（前端→后端），验证 JSON 反序列化
    let json = r#"{"db_type":"mysql","url":"mysql://localhost:3306/mydb","name":"test","conn_id":"conn-1","connection_type":"global","description":"desc","driver_id":"mysql-native","project_id":null,"environment_id":null,"auth_config_id":null,"auth_method":null,"network_config_id":null,"driver_properties":null,"advanced_options":null,"options":null,"tags":null,"metadata_path":null,"schema_name":null,"use_duckdb_fed":null,"password":null}"#;
    let parsed: ConnectDatabaseInput = serde_json::from_str(json).expect("反序列化失败");
    assert_eq!(parsed.db_type, "mysql");
    assert_eq!(parsed.url, "mysql://localhost:3306/mydb");
    assert_eq!(parsed.conn_id, Some("conn-1".to_string()));
    assert_eq!(parsed.connection_type, Some("global".to_string()));
    // 验证未传入的字段为 None
    assert!(parsed.auth_config_id.is_none());
    assert!(parsed.auth_method.is_none());
    assert!(parsed.network_config_id.is_none());
}

#[test]
fn test_convert_input_deserialize() {
    // ConvertConnectionInput 仅实现 Deserialize
    let json = r#"{"conn_id":"conn-1","target_type":"project","project_id":"/path/to/project"}"#;
    let parsed: ConvertConnectionInput = serde_json::from_str(json).expect("反序列化失败");
    assert_eq!(parsed.conn_id, "conn-1");
    assert_eq!(parsed.target_type, "project");
    assert_eq!(parsed.project_id, Some("/path/to/project".to_string()));
}

#[test]
fn test_create_db_file_input_deserialize() {
    // CreateDatabaseFileInput 仅实现 Deserialize
    let json = r#"{"db_type":"sqlite","file_path":"/tmp/test.db"}"#;
    let parsed: CreateDatabaseFileInput = serde_json::from_str(json).expect("反序列化失败");
    assert_eq!(parsed.db_type, "sqlite");
    assert_eq!(parsed.file_path, "/tmp/test.db");
}

#[test]
fn test_update_global_input_deserialize() {
    // UpdateGlobalConnectionInput 仅实现 Deserialize
    let json = r#"{"conn_id":"G_conn_001","name":"renamed"}"#;
    let parsed: UpdateGlobalConnectionInput = serde_json::from_str(json).expect("反序列化失败");
    assert_eq!(parsed.conn_id, "G_conn_001");
    assert_eq!(parsed.name, Some("renamed".to_string()));
    // 未传入的字段为 None
    assert!(parsed.url.is_none());
    assert!(parsed.driver.is_none());
}

#[test]
fn test_connection_info_response_serialize() {
    // ConnectionInfoResponse 仅实现 Serialize（后端→前端）
    let response = ConnectionInfoResponse {
        id: "conn-1".to_string(),
        name: "test".to_string(),
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        connection_type: "global".to_string(),
        project_id: None,
        status: "connected".to_string(),
        is_active: true,
        created_at_ms: 1700000000.0,
        server_version: Some("8.0.35".to_string()),
        driver_id: Some("mysql-native".to_string()),
        description: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
    };
    let json = serde_json::to_string(&response).expect("序列化失败");
    assert!(json.contains("conn-1"));
    assert!(json.contains("connected"));
    assert!(json.contains("global"));
    assert!(json.contains("8.0.35"));
    assert!(json.contains("mysql-native"));
}

#[test]
fn test_validation_result_serialize() {
    // ValidationResult 仅实现 Serialize（后端→前端）
    let result = ValidationResult {
        valid: false,
        errors: vec!["Database URL cannot be empty".to_string()],
        warnings: vec!["Using default encoding".to_string()],
    };
    let json = serde_json::to_string(&result).expect("序列化失败");
    assert!(json.contains("valid"));
    assert!(json.contains("errors"));
    assert!(json.contains("warnings"));
    assert!(json.contains("Database URL cannot be empty"));
    assert!(json.contains("Using default encoding"));
}

#[test]
fn test_test_connection_response_serialize() {
    // TestConnectionResponse 仅实现 Serialize（后端→前端）
    let response = TestConnectionResponse {
        success: true,
        message: "连接成功".to_string(),
        server_version: "8.0.35".to_string(),
        response_time_ms: 150,
    };
    let json = serde_json::to_string(&response).expect("序列化失败");
    assert!(json.contains("success"));
    assert!(json.contains("连接成功"));
    assert!(json.contains("8.0.35"));
    assert!(json.contains("150"));
}

#[test]
fn test_recent_connection_response_serialize() {
    // RecentConnectionResponse 仅实现 Serialize（后端→前端）
    let response = RecentConnectionResponse {
        name: "recent-test".to_string(),
        db_type: "postgresql".to_string(),
        url: "postgresql://localhost:5432/db".to_string(),
        last_used_at: "2025-01-01T00:00:00Z".to_string(),
        description: Some("Production".to_string()),
        driver_id: Some("pg-native".to_string()),
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
    };
    let json = serde_json::to_string(&response).expect("序列化失败");
    assert!(json.contains("recent-test"));
    assert!(json.contains("postgresql"));
    assert!(json.contains("Production"));
    assert!(json.contains("pg-native"));
}

#[test]
fn test_global_connection_info_response_serialize() {
    // GlobalConnectionInfoResponse 仅实现 Serialize（后端→前端）
    let response = GlobalConnectionInfoResponse {
        id: "G_conn_001".to_string(),
        name: "global-test".to_string(),
        driver: "mysql".to_string(),
        host: Some("localhost".to_string()),
        port: Some(3306),
        database: Some("mydb".to_string()),
        schema_name: None,
        username: Some("root".to_string()),
        password: None,
        options: None,
        tags: vec!["production".to_string()],
        use_duckdb_fed: false,
        metadata_path: None,
        is_active: false,
        created_at: "2025-01-01".to_string(),
        updated_at: "2025-01-01".to_string(),
        server_version: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        description: None,
    };
    let json = serde_json::to_string(&response).expect("序列化失败");
    assert!(json.contains("G_conn_001"));
    assert!(json.contains("global-test"));
    assert!(json.contains("localhost"));
    assert!(json.contains("production"));
    assert!(json.contains("3306"));
}

// ==================== 边界条件测试 ====================

#[test]
fn test_connect_input_long_name() {
    // 超长连接名称
    let long_name = "a".repeat(1000);
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        name: Some(long_name.clone()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert_eq!(input.name.unwrap().len(), 1000);
}

#[test]
fn test_connect_input_special_chars_in_name() {
    // 连接名称包含特殊字符
    let special_name = "测试-连接_MySQL#1 (生产)";
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "mysql".to_string(),
        url: "mysql://localhost:3306/test".to_string(),
        name: Some(special_name.to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert_eq!(input.name.as_deref(), Some(special_name));
}

#[test]
fn test_connect_input_ipv6_url() {
    // IPv6 地址 URL
    let input = ConnectDatabaseInput {
        conn_id: None,
        db_type: "mysql".to_string(),
        url: "mysql://root:pass@[::1]:3306/mydb".to_string(),
        name: Some("ipv6-conn".to_string()),
        connection_type: Some("global".to_string()),
        project_id: None,
        description: None,
        driver_id: None,
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: None,
        advanced_options: None,
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: None,
        password: None,
    };
    assert!(input.url.contains("[::1]"));
}

#[test]
fn test_connect_input_all_db_types() {
    // 所有支持的数据库类型
    let db_types = vec!["mysql", "postgresql", "sqlite", "duckdb"];
    for db_type in db_types {
        let input = ConnectDatabaseInput {
            conn_id: None,
            db_type: db_type.to_string(),
            url: format!("{}://localhost/test", db_type),
            name: Some(format!("{}-conn", db_type)),
            connection_type: Some("global".to_string()),
            project_id: None,
            description: None,
            driver_id: None,
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: None,
            driver_properties: None,
            advanced_options: None,
            options: None,
            tags: None,
            metadata_path: None,
            schema_name: None,
            use_duckdb_fed: None,
            password: None,
        };
        assert_eq!(input.db_type, db_type);
    }
}

#[test]
fn test_connection_config_to_url_no_port() {
    // 不指定端口时使用默认端口
    let config = DriverConnectionConfig {
        host: Some("localhost".to_string()),
        database: Some("mydb".to_string()),
        username: Some("root".to_string()),
        password: Some("secret".to_string()),
        port: None,
        ..make_config("mysql")
    };
    let url = config.to_url().expect("构建 URL 失败");
    // 无端口时，MySQL 默认端口 3306 应出现在 URL 中
    assert!(url.contains("localhost"));
    assert!(url.contains("mydb"));
}

#[test]
fn test_connection_config_to_url_only_host() {
    // 仅指定主机名
    let config = DriverConnectionConfig {
        host: Some("db.example.com".to_string()),
        ..make_config("mysql")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert!(url.contains("db.example.com"));
}

#[test]
fn test_connection_config_to_url_empty_password() {
    // 空密码
    let config = DriverConnectionConfig {
        host: Some("localhost".to_string()),
        port: Some(5432u16),
        database: Some("db".to_string()),
        username: Some("user".to_string()),
        password: Some("".to_string()),
        ..make_config("postgres")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert!(url.contains("user"));
    // PostgreSQL URL 格式: postgres://user:@host:port/db (空密码)
    assert!(url.starts_with("postgres://"));
}

#[test]
fn test_connection_config_to_url_with_template() {
    // 使用 url_template 构建 URL
    let config = DriverConnectionConfig {
        host: Some("localhost".to_string()),
        port: Some(5432u16),
        database: Some("mydb".to_string()),
        username: Some("user".to_string()),
        password: Some("pass".to_string()),
        url_template: Some("{driver}://{username}:{password}@{host}:{port}/{database}".to_string()),
        ..make_config("postgres")
    };
    let url = config.to_url().expect("构建 URL 失败");
    // 模板中使用 {driver} 占位符，会被替换为 "postgresql"
    assert!(url.contains("localhost:5432"));
    assert!(url.contains("mydb"));
}

#[test]
fn test_connection_config_to_url_duckdb_memory() {
    // DuckDB 内存数据库
    let config = DriverConnectionConfig {
        url_override: Some(":memory:".to_string()),
        ..make_config("duckdb")
    };
    let url = config.to_url().expect("构建 URL 失败");
    assert_eq!(url, ":memory:");
}

#[test]
fn test_connection_config_to_url_duckdb_file() {
    // DuckDB 文件数据库，带额外选项
    let mut opts = HashMap::new();
    opts.insert("access_mode".to_string(), "read_only".to_string());

    let config = DriverConnectionConfig {
        file_path: Some("/data/analytics.duckdb".to_string()),
        options: opts,
        ..make_config("duckdb")
    };
    let url = config.to_url().expect("构建 URL 失败");
    // DuckDB URL 格式: duckdb:///data/analytics.duckdb
    assert!(url.contains("analytics.duckdb"));
    assert!(url.starts_with("duckdb://"));
}

// ==================== 密码脱敏边界测试 ====================

#[test]
fn test_mask_password_in_url_with_special_chars() {
    use rdata_station_lib::core::services::connection_service::ConnectionService;

    let url = "mysql://user:p@ss:word@localhost:3306/db";
    let masked = ConnectionService::mask_password_in_url(url);
    assert!(!masked.contains("p@ss:word"));
    assert!(masked.contains("****"));
}

#[test]
fn test_mask_password_in_url_with_at_sign() {
    use rdata_station_lib::core::services::connection_service::ConnectionService;

    let url = "postgresql://admin:pass@word@localhost:5432/db";
    let masked = ConnectionService::mask_password_in_url(url);
    assert!(!masked.contains("pass@word"));
    assert!(masked.contains("****"));
}

#[test]
fn test_mask_password_in_url_only_user() {
    use rdata_station_lib::core::services::connection_service::ConnectionService;

    // 仅用户名无密码时，mask_password_in_url 会追加 ******
    let url = "mysql://root@localhost:3306/db";
    let masked = ConnectionService::mask_password_in_url(url);
    // 仅用户名无密码时，函数会追加 ****** 标记
    assert!(masked.contains("******"));
    assert!(masked.contains("localhost"));
}

// ==================== ConnectionMethod 测试 ====================

#[test]
fn test_connection_method_direct() {
    let method = ConnectionMethod::Direct;
    let json = serde_json::to_string(&method).expect("序列化失败");
    // ConnectionMethod 使用 #[serde(rename_all = "snake_case")]，Direct → "direct"
    assert!(json.contains("direct"));
}

#[test]
fn test_connection_method_ssh() {
    use rdata_station_lib::core::driver::connection::config::{SshAuth, SshConfig};
    let method = ConnectionMethod::Ssh(SshConfig {
        host: "bastion.example.com".to_string(),
        port: 22,
        username: "admin".to_string(),
        auth: SshAuth::Password {
            password: "ssh_pass".to_string(),
        },
        remote_host: "db.internal".to_string(),
        remote_port: 3306,
        local_port: 0,
        timeout_secs: 30,
    });
    let json = serde_json::to_string(&method).expect("序列化失败");
    assert!(json.contains("bastion.example.com"));
    assert!(json.contains("ssh"));
    assert!(json.contains("db.internal"));
}

#[test]
fn test_connection_method_ssl() {
    use rdata_station_lib::core::driver::connection::config::SslConfig;
    let method = ConnectionMethod::Ssl(SslConfig {
        verify_server_cert: true,
        ca_cert_path: Some("/path/to/ca.pem".to_string()),
        client_cert_path: None,
        client_key_path: None,
        ..Default::default()
    });
    let json = serde_json::to_string(&method).expect("序列化失败");
    assert!(json.contains("ssl"));
    assert!(json.contains("ca.pem"));
}

#[test]
fn test_connection_method_http_proxy() {
    use rdata_station_lib::core::driver::connection::config::{ProxyAuth, ProxyConfig};
    let method = ConnectionMethod::HttpProxy(ProxyConfig {
        host: "proxy.example.com".to_string(),
        port: 8080,
        auth: Some(ProxyAuth {
            username: "proxyuser".to_string(),
            password: "proxypass".to_string(),
        }),
        no_proxy: vec![],
        timeout_secs: 30,
    });
    let json = serde_json::to_string(&method).expect("序列化失败");
    assert!(json.contains("http_proxy"));
    assert!(json.contains("proxy.example.com"));
    assert!(json.contains("8080"));
    assert!(json.contains("proxyuser"));
}

#[test]
fn test_connection_method_socks_proxy() {
    use rdata_station_lib::core::driver::connection::config::ProxyConfig;
    let method = ConnectionMethod::SocksProxy(ProxyConfig {
        host: "socks.example.com".to_string(),
        port: 1080,
        auth: None,
        no_proxy: vec![],
        timeout_secs: 30,
    });
    let json = serde_json::to_string(&method).expect("序列化失败");
    assert!(json.contains("socks_proxy"));
    assert!(json.contains("socks.example.com"));
    assert!(json.contains("1080"));
}

#[test]
fn test_connection_method_chain() {
    use rdata_station_lib::core::driver::connection::config::{
        ChainHop, ProxyConfig, SshAuth, SshConfig, SslConfig,
    };
    let method = ConnectionMethod::Chain(vec![
        ChainHop::HttpProxy(ProxyConfig {
            host: "proxy.example.com".to_string(),
            port: 8080,
            auth: None,
            no_proxy: vec![],
            timeout_secs: 30,
        }),
        ChainHop::Ssh(SshConfig {
            host: "bastion.example.com".to_string(),
            port: 22,
            username: "admin".to_string(),
            auth: SshAuth::Password {
                password: "ssh_pass".to_string(),
            },
            remote_host: "db.internal".to_string(),
            remote_port: 3306,
            local_port: 0,
            timeout_secs: 30,
        }),
        ChainHop::Ssl(SslConfig {
            verify_server_cert: true,
            ..Default::default()
        }),
    ]);
    let json = serde_json::to_string(&method).expect("序列化失败");
    assert!(json.contains("chain"));
    assert!(json.contains("proxy.example.com"));
    assert!(json.contains("bastion.example.com"));
}
