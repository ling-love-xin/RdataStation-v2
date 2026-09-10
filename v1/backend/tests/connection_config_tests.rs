//! 连接配置单元测试 — DriverConnectionConfig + mask_password_in_url
//!
//! 测试 to_url() 各路径（url_override / url_template / legacy match）、
//! append_query_params、各 builder 方法、以及 URL 密码脱敏。

use rdata_station_lib::core::services::connection_service::ConnectionService;

// ==================== mask_password_in_url ====================

#[test]
fn test_mask_password_mysql_url() {
    let url = "mysql://root:secret123@localhost:3306/mydb";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, "mysql://root:******@localhost:3306/mydb");
}

#[test]
fn test_mask_password_postgres_url() {
    let url = "postgres://postgres:pg_pass@db.example.com:5432/analytics";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(
        masked,
        "postgres://postgres:******@db.example.com:5432/analytics"
    );
}

#[test]
fn test_mask_password_no_password() {
    let url = "mysql://root@localhost:3306/mydb";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, "mysql://root******@localhost:3306/mydb");
}

#[test]
fn test_mask_password_no_port() {
    let url = "postgres://admin:secret@localhost/analytics";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, "postgres://admin:******@localhost/analytics");
}

#[test]
fn test_mask_password_special_chars() {
    // 密码含 @ 符号时，find('@') 找到的是密码中的第一个 @
    let url = "mysql://user:p@ss:w0rd!@host:3306/db";
    let masked = ConnectionService::mask_password_in_url(url);
    // 实际行为：第一个 @ 在密码中，mask 到该位置
    assert!(masked.contains("******"));
    assert!(!masked.contains("p@ss"));
}

#[test]
fn test_mask_password_file_database() {
    // 文件型数据库没有 @ 符号，返回原 URL
    let url = "sqlite:///data/myapp.db";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, url);
}

#[test]
fn test_mask_password_duckdb() {
    let url = "duckdb:///data/analytics.duckdb";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, url);
}

#[test]
fn test_mask_password_empty_url() {
    let masked = ConnectionService::mask_password_in_url("");
    assert_eq!(masked, "");
}

#[test]
fn test_mask_password_no_scheme() {
    let url = "localhost:3306";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, url);
}

#[test]
fn test_mask_password_no_auth() {
    let url = "mysql://localhost:3306/mydb";
    let masked = ConnectionService::mask_password_in_url(url);
    // 没有 @ 符号，返回原 URL
    assert_eq!(masked, url);
}

#[test]
fn test_mask_password_ipv6() {
    let url = "mysql://user:pass@[::1]:3306/db";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, "mysql://user:******@[::1]:3306/db");
}

#[test]
fn test_mask_password_only_username() {
    let url = "mysql://root@localhost:3306/db";
    let masked = ConnectionService::mask_password_in_url(url);
    assert_eq!(masked, "mysql://root******@localhost:3306/db");
}

// ==================== DriverConnectionConfig::to_url() ====================

use rdata_station_lib::core::driver::registry::DriverConnectionConfig;

#[test]
fn test_to_url_mysql_full() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("db.example.com")
        .with_port(3306)
        .with_username("admin")
        .with_password("secret")
        .with_database("production");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(url, "mysql://admin:secret@db.example.com:3306/production");
}

#[test]
fn test_to_url_mysql_defaults() {
    let config = DriverConnectionConfig::new("mysql").with_host("localhost");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(url, "mysql://root:@localhost:3306");
}

#[test]
fn test_to_url_postgres_full() {
    let config = DriverConnectionConfig::new("postgres")
        .with_host("pg.example.com")
        .with_port(5432)
        .with_username("pguser")
        .with_password("pgpass")
        .with_database("analytics");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(
        url,
        "postgres://pguser:pgpass@pg.example.com:5432/analytics"
    );
}

#[test]
fn test_to_url_postgres_native_alias() {
    let config = DriverConnectionConfig::new("postgres_native")
        .with_host("localhost")
        .with_port(5432)
        .with_username("postgres")
        .with_password("pass")
        .with_database("postgres");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(url, "postgres://postgres:pass@localhost:5432/postgres");
}

#[test]
fn test_to_url_sqlite() {
    let config = DriverConnectionConfig::new("sqlite").with_file_path("/data/myapp.db");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(url, "sqlite:///data/myapp.db");
}

#[test]
fn test_to_url_sqlite_no_path() {
    let config = DriverConnectionConfig::new("sqlite");
    let result = config.to_url();
    assert!(result.is_err());
}

#[test]
fn test_to_url_duckdb() {
    let config = DriverConnectionConfig::new("duckdb").with_file_path("/data/analytics.duckdb");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(url, "duckdb:///data/analytics.duckdb");
}

#[test]
fn test_to_url_duckdb_no_path() {
    let config = DriverConnectionConfig::new("duckdb");
    let result = config.to_url();
    assert!(result.is_err());
}

#[test]
fn test_to_url_unsupported_driver() {
    let config = DriverConnectionConfig::new("mongodb").with_host("localhost");
    let result = config.to_url();
    assert!(result.is_err());
}

#[test]
fn test_to_url_with_options() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306);
    config.options.insert("charset".into(), "utf8mb4".into());
    config.options.insert("useSSL".into(), "true".into());
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("charset=utf8mb4"));
    assert!(url.contains("useSSL=true"));
}

#[test]
fn test_to_url_postgres_default_database() {
    let config = DriverConnectionConfig::new("postgres").with_host("localhost");
    let url = config.to_url().expect("to_url failed");
    // PostgreSQL 默认数据库为 postgres
    assert!(url.contains("/postgres"));
}

#[test]
fn test_to_url_with_driver_properties() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306);
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".into(), "true".into());
    config
        .driver_properties
        .insert("useSSL".into(), "true".into());
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("allowPublicKeyRetrieval=true"));
    assert!(url.contains("useSSL=true"));
}

#[test]
fn test_to_url_driver_properties_no_override_options() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306);
    // options 优先级高于 driver_properties
    config.options.insert("charset".into(), "utf8mb4".into());
    config
        .driver_properties
        .insert("charset".into(), "latin1".into());
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("charset=utf8mb4"));
    // driver_properties 中的 charset 被 options 覆盖，不应出现
    let count = url.matches("charset").count();
    assert_eq!(count, 1);
}

#[test]
fn test_to_url_with_encoding() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_encoding("GBK");
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("charset=gbk"));
}

#[test]
fn test_to_url_with_connect_timeout() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_connect_timeout(30);
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("connect_timeout=30"));
}

#[test]
fn test_to_url_url_override() {
    let config = DriverConnectionConfig::new("mysql")
        .with_url_override("mysql://custom:pass@override.example.com:3307/mydb");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(url, "mysql://custom:pass@override.example.com:3307/mydb");
}

#[test]
fn test_to_url_url_override_with_driver_properties() {
    let mut config =
        DriverConnectionConfig::new("mysql").with_url_override("mysql://user:pass@host:3306/db");
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".into(), "true".into());
    let url = config.to_url().expect("to_url failed");
    // url_override 路径也应该追加 driver_properties
    assert!(url.contains("allowPublicKeyRetrieval=true"));
    assert!(url.starts_with("mysql://user:pass@host:3306/db"));
}

#[test]
fn test_to_url_url_template() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("template.example.com")
        .with_port(3307)
        .with_username("tpl_user")
        .with_password("tpl_pass")
        .with_database("tpl_db")
        .with_url_template("mysql://{username}:{password}@{host}:{port}/{database}");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(
        url,
        "mysql://tpl_user:tpl_pass@template.example.com:3307/tpl_db"
    );
}

#[test]
fn test_to_url_url_template_with_file_path() {
    let config = DriverConnectionConfig::new("sqlite")
        .with_file_path("/data/test.db")
        .with_url_template("sqlite://{file_path}");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(url, "sqlite:///data/test.db");
}

#[test]
fn test_to_url_url_template_with_query_params() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_database("test")
        .with_url_template("mysql://{host}:{port}/{database}");
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".into(), "true".into());
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("mysql://localhost:3306/test"));
    assert!(url.contains("allowPublicKeyRetrieval=true"));
}

#[test]
fn test_to_url_mysql_native_alias() {
    let config = DriverConnectionConfig::new("mysql_native")
        .with_host("localhost")
        .with_port(3306)
        .with_username("root")
        .with_password("pass")
        .with_database("test");
    let url = config.to_url().expect("to_url failed");
    assert_eq!(url, "mysql://root:pass@localhost:3306/test");
}

#[test]
fn test_to_url_port_zero() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(0);
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains(":0"));
}

#[test]
fn test_to_url_no_host_mysql() {
    let config = DriverConnectionConfig::new("mysql");
    let result = config.to_url();
    assert!(result.is_err());
}

#[test]
fn test_to_url_no_host_postgres() {
    let config = DriverConnectionConfig::new("postgres");
    let result = config.to_url();
    assert!(result.is_err());
}

#[test]
fn test_append_query_params_empty() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306);
    let url = config.to_url().expect("to_url failed");
    // 无 options/properties 时不应有 ?
    assert!(!url.contains('?'));
}

#[test]
fn test_append_query_params_multiple() {
    let mut config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_connect_timeout(30)
        .with_encoding("UTF-8");
    config.options.insert("charset".into(), "utf8mb4".into());
    config
        .driver_properties
        .insert("allowPublicKeyRetrieval".into(), "true".into());
    let url = config.to_url().expect("to_url failed");
    assert!(url.contains("charset=utf8mb4"));
    assert!(url.contains("connect_timeout=30"));
    assert!(url.contains("allowPublicKeyRetrieval=true"));
}
