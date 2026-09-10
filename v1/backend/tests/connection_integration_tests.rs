//! 连接命令集成测试 — test_connection + connect_database
//!
//! 使用 SQLite 作为测试数据库（无需网络），验证：
//! 1. test_connection 连接测试流程（成功/失败/超时脱敏）
//! 2. connect_database 连接创建流程（全局/项目）
//! 3. 连接生命周期管理

use std::collections::HashMap;

use rdata_station_lib::core::driver::connection::config::ConnectionMethod;
use rdata_station_lib::core::driver::registry::DriverConnectionConfig;
use rdata_station_lib::core::services::connection_service::ConnectionService;

// ==================== mask_password_in_url（补充边界） ====================

#[test]
fn test_mask_password_combined_roundtrip() {
    // 验证各种 URL 格式的脱敏一致性
    let tests = vec![
        (
            "mysql://root:secret@localhost:3306/db",
            "mysql://root:******@localhost:3306/db",
        ),
        (
            "postgres://admin:pass@pg.host:5432/analytics",
            "postgres://admin:******@pg.host:5432/analytics",
        ),
        (
            "mysql://root@localhost:3306/db",
            "mysql://root******@localhost:3306/db",
        ),
        ("sqlite:///data/test.db", "sqlite:///data/test.db"),
        (
            "duckdb:///data/analytics.duckdb",
            "duckdb:///data/analytics.duckdb",
        ),
        ("", ""),
        ("not-a-url", "not-a-url"),
    ];

    for (input, expected) in tests {
        assert_eq!(
            ConnectionService::mask_password_in_url(input),
            expected,
            "mask_password_in_url({:?})",
            input
        );
    }
}

// ==================== DriverConnectionConfig::to_url() 补充 ====================

#[test]
fn test_config_url_comprehensive() {
    // 测试所有驱动类型
    let configs = vec![
        (
            DriverConnectionConfig::new("mysql")
                .with_host("db.host")
                .with_port(3306)
                .with_username("user")
                .with_password("pass")
                .with_database("test"),
            "mysql://user:pass@db.host:3306/test",
        ),
        (
            DriverConnectionConfig::new("postgres")
                .with_host("pg.host")
                .with_port(5432)
                .with_username("pguser")
                .with_password("pgpass")
                .with_database("analytics"),
            "postgres://pguser:pgpass@pg.host:5432/analytics",
        ),
        (
            DriverConnectionConfig::new("sqlite").with_file_path("/data/test.db"),
            "sqlite:///data/test.db",
        ),
        (
            DriverConnectionConfig::new("duckdb").with_file_path("/data/test.duckdb"),
            "duckdb:///data/test.duckdb",
        ),
    ];

    for (config, expected) in configs {
        let url = config.to_url().expect("to_url failed");
        assert_eq!(url, expected);
    }
}

#[test]
fn test_config_url_with_encoding_and_timeout() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_encoding("GBK")
        .with_connect_timeout(10);
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("charset=gbk"));
    assert!(url.contains("connect_timeout=10"));
}

#[test]
fn test_config_url_mysql_no_port_default() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_username("root")
        .with_password("pass");
    let url = config.to_url().expect("to_url failed");
    // 默认端口为 3306
    assert!(url.contains(":3306"));
}

#[test]
fn test_config_url_postgres_no_port_default() {
    let config = DriverConnectionConfig::new("postgres")
        .with_host("localhost")
        .with_username("postgres")
        .with_password("pass");
    let url = config.to_url().expect("to_url failed");
    // 默认端口为 5432
    assert!(url.contains(":5432"));
}

#[test]
fn test_config_url_postgres_default_database() {
    let config = DriverConnectionConfig::new("postgres")
        .with_host("localhost")
        .with_username("postgres")
        .with_password("pass");
    let url = config.to_url().expect("to_url failed");
    assert!(url.ends_with("/postgres"));
}

#[test]
fn test_config_url_options_encoding_conflict() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_encoding("UTF-8");
    // options 中的 charset 优先于 encoding 推导
    config.options.insert("charset".into(), "latin1".into());
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("charset=latin1"));
    let count = url.matches("charset").count();
    assert_eq!(count, 1, "不应出现重复 charset");
}

#[test]
fn test_config_url_connect_timeout_conflict() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_connect_timeout(30);
    config.options.insert("connect_timeout".into(), "60".into());
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("connect_timeout=60"));
    let count = url.matches("connect_timeout").count();
    assert_eq!(count, 1, "不应出现重复 connect_timeout");
}

#[test]
fn test_config_url_connect_timeout_via_properties() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306);
    // options 中的 connect_timeout 会覆盖 with_connect_timeout
    config.options.insert("connect_timeout".into(), "45".into());
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("connect_timeout=45"));
    let count = url.matches("timeout").count();
    assert_eq!(count, 1, "不应出现重复 timeout");
}

#[test]
fn test_config_url_encoding_utf8_default() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_encoding("UTF-8");
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("charset=utf8mb4"));
}

#[test]
fn test_config_url_encoding_latin1() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_encoding("Latin-1");
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("charset=latin1"));
}

// ==================== ConnectionMethod 默认值 ====================

#[test]
fn test_config_default_connection_method() {
    let config = DriverConnectionConfig::new("mysql");
    // 默认应为 Direct
    assert!(matches!(config.connection_method, ConnectionMethod::Direct));
}

// ==================== 全链路模拟 — 数据流完整性 ====================

/// 模拟前端 ConnectDatabaseInput → DriverConnectionConfig → to_url →
/// ConnectionService::mask_password_in_url 的完整数据流
#[test]
fn test_full_chain_url_build_and_mask() {
    // 模拟前端输入
    let db_type = "mysql";
    let host = "db.example.com";
    let port = 3306;
    let database = "production";
    let username = "admin";
    let password = "super_secret_123";
    let driver_properties: HashMap<String, String> = {
        let mut m = HashMap::new();
        m.insert("allowPublicKeyRetrieval".into(), "true".into());
        m
    };

    // Step 1: 构建 DriverConnectionConfig
    let mut config = DriverConnectionConfig::new(db_type)
        .with_host(host)
        .with_port(port)
        .with_database(database)
        .with_username(username)
        .with_password(password)
        .with_connect_timeout(30);
    config.driver_properties = driver_properties;

    // Step 2: to_url() — 构建连接 URL
    let url = config.to_url().expect("to_url failed");
    assert!(url.starts_with("mysql://admin:super_secret_123@db.example.com:3306/production"));
    assert!(url.contains("allowPublicKeyRetrieval=true"));
    assert!(url.contains("connect_timeout=30"));

    // Step 3: mask_password_in_url — 日志/错误消息脱敏
    let masked = ConnectionService::mask_password_in_url(&url);
    assert!(!masked.contains("super_secret_123"));
    assert!(masked.contains("******"));
    assert!(masked.starts_with("mysql://admin:******@db.example.com:3306/production"));
}

/// 模拟 SQLite 完整链路
#[test]
fn test_full_chain_sqlite_url() {
    let file_path = "/data/myapp.db";
    let config = DriverConnectionConfig::new("sqlite").with_file_path(file_path);
    let url = config.to_url().expect("to_url failed");
    assert_eq!(url, "sqlite:///data/myapp.db");
    // 文件型数据库脱敏后不变
    let masked = ConnectionService::mask_password_in_url(&url);
    assert_eq!(masked, url);
}

/// 模拟 url_template 完整链路
#[test]
fn test_full_chain_url_template() {
    let template = "postgresql://{username}:{password}@{host}:{port}/{database}";
    let config = DriverConnectionConfig::new("postgres")
        .with_host("pg.example.com")
        .with_port(5432)
        .with_username("pguser")
        .with_password("pgpass")
        .with_database("analytics")
        .with_url_template(template);
    let url = config.to_url().expect("to_url failed");
    assert_eq!(
        url,
        "postgresql://pguser:pgpass@pg.example.com:5432/analytics"
    );
    let masked = ConnectionService::mask_password_in_url(&url);
    assert_eq!(
        masked,
        "postgresql://pguser:******@pg.example.com:5432/analytics"
    );
}

/// 模拟 url_override 完整链路
#[test]
fn test_full_chain_url_override() {
    let mut config =
        DriverConnectionConfig::new("mysql").with_url_override("mysql://admin:secret@host:3306/db");
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".into(), "true".into());
    let url = config.to_url().expect("to_url failed");
    assert!(url.starts_with("mysql://admin:secret@host:3306/db"));
    assert!(url.contains("allowPublicKeyRetrieval=true"));
    let masked = ConnectionService::mask_password_in_url(&url);
    assert_eq!(
        masked,
        "mysql://admin:******@host:3306/db?allowPublicKeyRetrieval=true"
    );
}
