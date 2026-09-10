//! Driver 注册表 集成测试
//!
//! 测试 DriverConnectionConfig 构建器 和 DriverDescriptor 的公共 API
//!
//! 本文件位于 src-tauri/tests/（集成测试），
//! 遵循 RdataStation 测试代码组织铁律。

use rdata_station_lib::core::driver::auto_register::AutoDriverRegistrar;
use rdata_station_lib::core::driver::registry::DriverConnectionConfig;
use rdata_station_lib::core::driver::registry::{descriptors, DriverDescriptor};

fn ensure_registry_initialized() {
    AutoDriverRegistrar::register_builtin_drivers();
}

#[test]
fn test_connection_config_new() {
    let config = DriverConnectionConfig::new("mysql");
    assert_eq!(config.driver, "mysql");
    assert!(config.name.is_none());
    assert!(config.host.is_none());
    assert!(config.options.is_empty());
}

#[test]
fn test_connection_config_builder() {
    let config = DriverConnectionConfig::new("mysql")
        .with_name("test-connection")
        .with_host("localhost")
        .with_port(3306)
        .with_database("testdb")
        .with_username("root")
        .with_password("secret")
        .with_option("ssl_mode", "PREFERRED");

    assert_eq!(config.name, Some("test-connection".to_string()));
    assert_eq!(config.host, Some("localhost".to_string()));
    assert_eq!(config.port, Some(3306));
    assert_eq!(config.database, Some("testdb".to_string()));
    assert_eq!(config.username, Some("root".to_string()));
    assert_eq!(config.password, Some("secret".to_string()));
    assert_eq!(
        config.options.get("ssl_mode"),
        Some(&"PREFERRED".to_string())
    );
}

#[test]
fn test_mysql_url_building() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_database("testdb")
        .with_username("root")
        .with_password("secret");

    let url = config.to_url().unwrap();
    assert_eq!(url, "mysql://root:secret@localhost:3306/testdb");
}

#[test]
fn test_mysql_url_with_options() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_database("testdb")
        .with_username("root")
        .with_password("secret")
        .with_option("ssl_mode", "PREFERRED")
        .with_option("charset", "utf8mb4");

    let url = config.to_url().unwrap();
    assert!(url.contains("ssl_mode=PREFERRED"));
    assert!(url.contains("charset=utf8mb4"));
    assert!(url.starts_with("mysql://root:secret@localhost:3306/testdb?"));
}

#[test]
fn test_postgres_url_building() {
    let config = DriverConnectionConfig::new("postgres")
        .with_host("localhost")
        .with_port(5432)
        .with_database("testdb")
        .with_username("postgres")
        .with_password("secret");

    let url = config.to_url().unwrap();
    assert_eq!(url, "postgres://postgres:secret@localhost:5432/testdb");
}

#[test]
fn test_sqlite_url_building() {
    let config = DriverConnectionConfig::new("sqlite").with_file_path("/path/to/db.sqlite");

    let url = config.to_url().unwrap();
    assert_eq!(url, "sqlite:///path/to/db.sqlite");
}

#[test]
fn test_duckdb_url_building() {
    let config = DriverConnectionConfig::new("duckdb").with_file_path("/path/to/db.duckdb");

    let url = config.to_url().unwrap();
    assert_eq!(url, "duckdb:///path/to/db.duckdb");
}

#[test]
fn test_mysql_url_missing_host() {
    let config = DriverConnectionConfig::new("mysql");
    let result = config.to_url();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Host is required"));
}

#[test]
fn test_sqlite_url_missing_file_path() {
    let config = DriverConnectionConfig::new("sqlite");
    let result = config.to_url();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("File path is required"));
}

#[test]
fn test_unsupported_driver() {
    let config = DriverConnectionConfig::new("oracle");
    let result = config.to_url();
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unsupported driver"));
}

// ========== DriverDescriptor 测试 ==========

#[test]
fn test_get_all_drivers() {
    ensure_registry_initialized();
    let drivers = descriptors::get_all_drivers();
    assert!(!drivers.is_empty());

    let driver_ids: Vec<_> = drivers.iter().map(|d| d.id.as_str()).collect();
    assert!(driver_ids.contains(&"mysql"));
    assert!(driver_ids.contains(&"postgres"));
    assert!(driver_ids.contains(&"sqlite"));
    assert!(driver_ids.contains(&"duckdb"));
}

#[test]
fn test_get_driver_mysql() {
    ensure_registry_initialized();
    let driver = descriptors::get_driver("mysql");
    assert!(driver.is_some());

    let driver = driver.unwrap();
    assert_eq!(driver.id, "mysql");
    assert_eq!(driver.default_port, Some(3306));
    assert!(!driver.require_file);
}

#[test]
fn test_get_driver_postgres() {
    ensure_registry_initialized();
    let driver = descriptors::get_driver("postgres");
    assert!(driver.is_some());

    let driver = driver.unwrap();
    assert_eq!(driver.id, "postgres");
    assert_eq!(driver.default_port, Some(5432));
}

#[test]
fn test_get_driver_sqlite() {
    ensure_registry_initialized();
    let driver = descriptors::get_driver("sqlite");
    assert!(driver.is_some());

    let driver = driver.unwrap();
    assert_eq!(driver.id, "sqlite");
    assert!(driver.require_file);
    assert!(!driver.require_database);
}

#[test]
fn test_get_driver_not_found() {
    ensure_registry_initialized();
    let driver = descriptors::get_driver("oracle");
    assert!(driver.is_none());
}

#[test]
fn test_mysql_driver_fields() {
    ensure_registry_initialized();
    let driver = descriptors::get_driver("mysql").unwrap();

    let field_keys: Vec<_> = driver.fields.iter().map(|f| f.key.as_str()).collect();
    assert!(field_keys.contains(&"host"));
    assert!(field_keys.contains(&"port"));
    assert!(field_keys.contains(&"username"));
    assert!(field_keys.contains(&"password"));
}

#[test]
fn test_driver_descriptor_serialization() {
    ensure_registry_initialized();
    let driver = descriptors::get_driver("mysql").unwrap();

    let json = serde_json::to_string(&driver).unwrap();
    assert!(json.contains("mysql"));
    assert!(json.contains("3306"));

    let deserialized: DriverDescriptor = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.id, "mysql");
}

// ========== url_override + driver_properties 回归测试（Round 22 Bug 修复）==========

#[test]
fn test_url_override_preserves_driver_properties() {
    // Bug: url_override 路径直接返回 URL，跳过 append_query_params()
    // 修复: url_override 路径也调用 append_query_params() 追加 driver_properties
    let mut config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb");
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".to_string(), "TRUE".to_string());
    config
        .driver_properties
        .insert("useSSL".to_string(), "false".to_string());

    let url = config.to_url().unwrap();
    assert!(
        url.contains("allowPublicKeyRetrieval=TRUE"),
        "url_override 路径应包含 driver_properties: {}",
        url
    );
    assert!(
        url.contains("useSSL=false"),
        "url_override 路径应包含 driver_properties: {}",
        url
    );
    assert!(
        url.starts_with("mysql://root:root@localhost:3306/testdb?"),
        "URL 应以问号分隔 query params: {}",
        url
    );
}

#[test]
fn test_url_override_with_options_and_properties() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb")
        .with_option("charset", "utf8mb4");
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".to_string(), "TRUE".to_string());

    let url = config.to_url().unwrap();
    assert!(url.contains("charset=utf8mb4"));
    assert!(url.contains("allowPublicKeyRetrieval=TRUE"));
}

#[test]
fn test_url_override_with_encoding() {
    let config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb")
        .with_encoding("GBK");

    let url = config.to_url().unwrap();
    assert!(
        url.contains("charset=gbk"),
        "encoding GBK 应映射为 charset=gbk: {}",
        url
    );
}

#[test]
fn test_url_override_with_connect_timeout() {
    let config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb")
        .with_connect_timeout(30);

    let url = config.to_url().unwrap();
    assert!(
        url.contains("connect_timeout=30"),
        "应包含 connect_timeout: {}",
        url
    );
}

#[test]
fn test_url_override_no_duplicate_params() {
    // 确保 options 中已有的 key 不被 driver_properties 覆盖
    let mut config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb")
        .with_option("allowPublicKeyRetrieval", "false");
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".to_string(), "TRUE".to_string());

    let url = config.to_url().unwrap();
    // options 优先，driver_properties 不覆盖
    assert!(
        url.contains("allowPublicKeyRetrieval=false"),
        "options 中的值应优先于 driver_properties: {}",
        url
    );
}

#[test]
fn test_url_override_empty_driver_properties() {
    // 空 driver_properties 不应导致 crash 或多余 ?
    let config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://root:root@localhost:3306/testdb");

    let url = config.to_url().unwrap();
    assert_eq!(
        url, "mysql://root:root@localhost:3306/testdb",
        "空 driver_properties 时 URL 不应改变: {}",
        url
    );
    assert!(!url.contains('?'), "空 params 时不应有问号: {}", url);
}
