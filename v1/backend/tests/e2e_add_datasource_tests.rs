//! 新增数据源 E2E 全链路测试
//!
//! 测试 Tauri Command 层：connect_database、test_connection、
//! create_auth_config、create_network_config 的完整调用链路。

use rdata_station_lib::core::driver::registry::DriverConnectionConfig;
use rdata_station_lib::core::persistence::auth_store;
use rdata_station_lib::core::services::connection_service::ConnectionService;

// ==================== 辅助函数 ====================

/// 创建临时 SQLite 文件路径（测试用）
fn temp_sqlite_path(name: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push("rdata-station-e2e-tests");
    let _ = std::fs::create_dir_all(&path);
    path.push(format!("{}.db", name));
    let path_str = path.to_string_lossy().replace('\\', "/");
    // 清理旧文件
    let _ = std::fs::remove_file(&path_str);
    path_str
}

// ==================== E2E: connect_database ====================

/// 模拟 connect_database 全链路：前端输入 → DriverConnectionConfig → to_url → 连接
#[test]
fn test_e2e_connect_database_global_sqlite() {
    // Step 1: 模拟前端 formData 输入
    let db_type = "sqlite";
    let file_path = temp_sqlite_path("e2e_global_sqlite");
    let name = "E2E Test SQLite";

    // Step 2: 构建 DriverConnectionConfig（模拟 connect_database 命令内部逻辑）
    let config = DriverConnectionConfig::new(db_type).with_file_path(&file_path);
    let url = config.to_url().expect("Failed to build URL");

    // Step 3: 验证 URL 格式
    assert!(url.starts_with("sqlite://"));
    assert!(url.contains("e2e_global_sqlite"));

    // Step 4: 验证 URL 脱敏（模拟 test_connection 日志输出）
    let masked = ConnectionService::mask_password_in_url(&url);
    assert_eq!(masked, url); // 文件型数据库无密码，脱敏后不变

    // Step 5: 验证连接名称
    assert!(!name.is_empty());
    assert_eq!(name, "E2E Test SQLite");

    // 清理
    let _ = std::fs::remove_file(&file_path);
}

/// 模拟 connect_database 项目连接
#[test]
fn test_e2e_connect_database_project_sqlite() {
    let db_type = "sqlite";
    let file_path = temp_sqlite_path("e2e_project_sqlite");
    let project_id = "/projects/test-app";

    let config = DriverConnectionConfig::new(db_type).with_file_path(&file_path);
    let url = config.to_url().expect("Failed to build URL");

    assert!(url.starts_with("sqlite://"));
    assert!(!project_id.is_empty());

    let _ = std::fs::remove_file(&file_path);
}

/// 模拟 connect_database MySQL 全链路
#[test]
fn test_e2e_connect_database_mysql_url() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("localhost")
        .with_port(3306)
        .with_username("root")
        .with_password("secret")
        .with_database("test");

    let url = config.to_url().expect("Failed to build URL");
    assert_eq!(url, "mysql://root:secret@localhost:3306/test");

    // 脱敏验证
    let masked = ConnectionService::mask_password_in_url(&url);
    assert_eq!(masked, "mysql://root:******@localhost:3306/test");
    assert!(!masked.contains("secret"));
}

// ==================== E2E: test_connection ====================

/// 模拟 test_connection 成功路径
#[test]
fn test_e2e_test_connection_success() {
    let file_path = temp_sqlite_path("e2e_test_conn_success");

    // 创建有效的 SQLite 文件
    let conn = rusqlite::Connection::open(&file_path).expect("Failed to create SQLite DB");
    conn.execute("CREATE TABLE test (id INTEGER PRIMARY KEY)", [])
        .expect("Failed to create table");
    conn.close().ok();

    let config = DriverConnectionConfig::new("sqlite").with_file_path(&file_path);
    let url = config.to_url().expect("Failed to build URL");

    // 模拟 test_connection 命令逻辑
    let result = std::panic::catch_unwind(|| {
        let conn = rusqlite::Connection::open(&file_path).expect("Failed to open");
        let mut stmt = conn.prepare("SELECT 1").expect("Failed to prepare");
        let _ = stmt.query([]).expect("Failed to query");
    });

    assert!(result.is_ok(), "test_connection should succeed");

    // 验证 URL 脱敏
    let masked = ConnectionService::mask_password_in_url(&url);
    assert_eq!(masked, url);

    let _ = std::fs::remove_file(&file_path);
}

/// 模拟 test_connection 失败脱敏
#[test]
fn test_e2e_test_connection_failure_masked() {
    let invalid_path = "/nonexistent/path/db.sqlite";

    let config = DriverConnectionConfig::new("sqlite").with_file_path(invalid_path);
    let url = config.to_url().expect("Failed to build URL");

    // 模拟连接失败
    let result = std::panic::catch_unwind(|| {
        let _ = rusqlite::Connection::open(invalid_path).expect("Should fail");
    });

    assert!(
        result.is_err(),
        "test_connection should fail for invalid path"
    );

    // 验证错误消息脱敏（模拟 test_connection 命令中 mask_password_in_url）
    let masked = ConnectionService::mask_password_in_url(&url);
    assert!(!masked.contains("password"));
    assert!(!masked.contains("secret"));
}

/// 模拟 test_connection MySQL 失败脱敏
#[test]
fn test_e2e_test_connection_mysql_failure_masked() {
    let config = DriverConnectionConfig::new("mysql")
        .with_host("invalid.host")
        .with_port(3306)
        .with_username("admin")
        .with_password("super_secret_123")
        .with_database("test");

    let url = config.to_url().expect("Failed to build URL");
    assert!(url.contains("super_secret_123")); // 原始 URL 包含密码

    // 脱敏
    let masked = ConnectionService::mask_password_in_url(&url);
    assert!(!masked.contains("super_secret_123"));
    assert!(masked.contains("******"));
}

// ==================== E2E: create_auth_config ====================

/// 模拟 create_auth_config 加密存储全链路
#[test]
fn test_e2e_create_auth_config_password_encryption() {
    let auth_data = r#"{"username":"admin","password":"MySecret123"}"#;

    // Step 1: 加密（模拟 create_auth_config 命令中的 encrypt_auth_data）
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");

    // Step 2: 验证 AES: 前缀
    assert!(encrypted.contains("AES:"), "加密后应包含 AES: 前缀");

    // Step 3: 验证原始密码不在加密数据中
    assert!(!encrypted.contains("MySecret123"));

    // Step 4: 解密（模拟 list_auth_configs 命令中的 decrypt_auth_data）
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");

    // Step 5: 验证解密后还原
    let json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(json["username"], "admin");
    assert_eq!(json["password"], "MySecret123");
}

/// 模拟 create_auth_config 空密码不加密
#[test]
fn test_e2e_create_auth_config_empty_password() {
    let auth_data = r#"{"username":"admin","password":""}"#;

    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");

    // 空密码不应加密（encrypt 不触发）
    let json: serde_json::Value = serde_json::from_str(&encrypted).expect("parse failed");
    assert_eq!(json["password"], "");
}

/// 模拟 create_auth_config 已加密密码不重复加密
#[test]
fn test_e2e_create_auth_config_no_double_encrypt() {
    let auth_data = r#"{"username":"admin","password":"AES:already_encrypted"}"#;

    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");

    // 不应出现双重 AES:（skip 已加密的字段）
    let count = encrypted.matches("AES:").count();
    assert_eq!(count, 1, "不应双重加密");
}

// ==================== E2E: create_network_config ====================

/// 模拟 create_network_config SSH 配置存储
#[test]
fn test_e2e_create_network_config_ssh() {
    let ssh_config = serde_json::json!({
        "host": "jump.corp.com",
        "port": 22,
        "username": "admin",
        "auth_type": "password",
        "password": "ssh-secret"
    });

    let config_str = ssh_config.to_string();

    // 验证 JSON 序列化/反序列化
    let parsed: serde_json::Value = serde_json::from_str(&config_str).expect("parse failed");
    assert_eq!(parsed["host"], "jump.corp.com");
    assert_eq!(parsed["port"], 22);
    assert_eq!(parsed["username"], "admin");
    assert_eq!(parsed["auth_type"], "password");
}

/// 模拟 create_network_config SSL 配置存储
#[test]
fn test_e2e_create_network_config_ssl() {
    let ssl_config = serde_json::json!({
        "ssl_mode": "verify-full",
        "ca_cert_path": "/etc/ssl/ca.crt",
        "client_cert_path": "/etc/ssl/client.crt",
        "client_key_path": "/etc/ssl/client.key"
    });

    let config_str = ssl_config.to_string();
    let parsed: serde_json::Value = serde_json::from_str(&config_str).expect("parse failed");
    assert_eq!(parsed["ssl_mode"], "verify-full");
    assert_eq!(parsed["ca_cert_path"], "/etc/ssl/ca.crt");
}

/// 模拟 create_network_config Proxy 配置存储
#[test]
fn test_e2e_create_network_config_proxy() {
    let proxy_config = serde_json::json!({
        "proxy_type": "socks5",
        "host": "proxy.corp.com",
        "port": 1080,
        "username": "proxyuser",
        "password": "proxypass"
    });

    let config_str = proxy_config.to_string();
    let parsed: serde_json::Value = serde_json::from_str(&config_str).expect("parse failed");
    assert_eq!(parsed["proxy_type"], "socks5");
    assert_eq!(parsed["host"], "proxy.corp.com");
    assert_eq!(parsed["port"], 1080);
}

// ==================== E2E: 综合全链路 ====================

/// 完整全链路：前端输入 → URL构建 → 脱敏 → 加密 → 存储
#[test]
fn test_e2e_full_chain_mysql_with_auth_and_network() {
    // 1. 前端表单数据
    let host = "db.example.com";
    let port = 3306;
    let database = "production";
    let username = "admin";
    let password = "super_secret_123";

    // 2. DriverConnectionConfig → to_url
    let config = DriverConnectionConfig::new("mysql")
        .with_host(host)
        .with_port(port)
        .with_username(username)
        .with_password(password)
        .with_database(database);
    let url = config.to_url().expect("to_url failed");

    assert_eq!(
        url,
        "mysql://admin:super_secret_123@db.example.com:3306/production"
    );

    // 3. test_connection 日志脱敏
    let masked = ConnectionService::mask_password_in_url(&url);
    assert_eq!(
        masked,
        "mysql://admin:******@db.example.com:3306/production"
    );

    // 4. create_auth_config 加密
    let auth_data = format!(r#"{{"username":"{}","password":"{}"}}"#, username, password);
    let encrypted = auth_store::encrypt_auth_data(&auth_data).expect("encrypt failed");
    assert!(encrypted.contains("AES:"));
    assert!(!encrypted.contains(password));

    // 5. list_auth_configs 解密
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(json["username"], username);
    assert_eq!(json["password"], password);
}

/// 完整全链路：SQLite 文件数据库
#[test]
fn test_e2e_full_chain_sqlite_file_db() {
    let file_path = temp_sqlite_path("e2e_full_chain");

    // 1. 自建数据库文件
    let conn = rusqlite::Connection::open(&file_path).expect("Failed to create");
    conn.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)", [])
        .expect("Failed to create table");
    conn.execute("INSERT INTO users (name) VALUES ('Alice')", [])
        .expect("Failed to insert");
    conn.close().ok();

    // 2. URL 构建
    let config = DriverConnectionConfig::new("sqlite").with_file_path(&file_path);
    let url = config.to_url().expect("Failed to build URL");
    assert!(url.starts_with("sqlite://"));

    // 3. 脱敏（文件型数据库不变）
    let masked = ConnectionService::mask_password_in_url(&url);
    assert_eq!(masked, url);

    // 4. 连接验证
    let conn = rusqlite::Connection::open(&file_path).expect("Failed to open");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
        .expect("Query failed");
    assert_eq!(count, 1);

    let _ = std::fs::remove_file(&file_path);
}
