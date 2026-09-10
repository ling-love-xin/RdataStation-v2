//! 认证存储测试 — encrypt_auth_data / decrypt_auth_data
//!
//! 测试 AES-256-GCM 加密/解密 auth_data 的完整流程，
//! 包括 password 字段加密、AES: 前缀标记、passphrase/clientSecret 加密、
//! 空数据、非 JSON 数据、已加密数据跳过等场景。

use rdata_station_lib::core::persistence::auth_store;

// ==================== encrypt_auth_data ====================

#[test]
fn test_encrypt_decrypt_password_roundtrip() {
    let auth_data = r#"{"username":"admin","password":"MySecret123"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    // 加密后应包含 AES: 前缀
    assert!(
        encrypted.contains("AES:"),
        "加密后应包含 AES: 前缀，实际: {}",
        encrypted
    );
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let decrypted_json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(decrypted_json["username"], "admin");
    assert_eq!(decrypted_json["password"], "MySecret123");
}

#[test]
fn test_encrypt_empty_auth_data() {
    let result = auth_store::encrypt_auth_data("").expect("encrypt failed");
    assert_eq!(result, "");
}

#[test]
fn test_encrypt_empty_password() {
    let auth_data = r#"{"username":"admin","password":""}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    // 空密码不加密，不包含 AES: 前缀
    assert!(!encrypted.contains("AES:"));
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let decrypted_json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(decrypted_json["password"], "");
}

#[test]
fn test_encrypt_already_encrypted_password() {
    // 已经带有 AES: 前缀的密码不应再次加密
    let auth_data = r#"{"username":"admin","password":"AES:encrypted_data"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    assert!(encrypted.contains("AES:encrypted_data"));
    // 不应该出现双重 AES:
    let count = encrypted.matches("AES:").count();
    assert_eq!(count, 1, "不应双重加密，实际: {}", encrypted);
}

#[test]
fn test_encrypt_passphrase_field() {
    let auth_data = r#"{"username":"admin","password":"pass123","passphrase":"my-ssh-passphrase"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let decrypted_json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(decrypted_json["passphrase"], "my-ssh-passphrase");
}

#[test]
fn test_encrypt_client_secret_field() {
    let auth_data = r#"{"username":"admin","password":"pass123","clientSecret":"oauth-secret"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let decrypted_json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(decrypted_json["clientSecret"], "oauth-secret");
}

#[test]
fn test_encrypt_unicode_password() {
    let auth_data = r#"{"username":"admin","password":"密码测试🔐"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    assert!(encrypted.contains("AES:"));
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let decrypted_json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(decrypted_json["password"], "密码测试🔐");
}

#[test]
fn test_encrypt_special_chars_password() {
    let auth_data = r#"{"username":"admin","password":"p@ss:w0rd!@#$%^&*()"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let decrypted_json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(decrypted_json["password"], "p@ss:w0rd!@#$%^&*()");
}

#[test]
fn test_encrypt_long_password() {
    let long_password = "A".repeat(1024);
    let auth_data = format!(r#"{{"username":"admin","password":"{}"}}"#, long_password);
    let encrypted = auth_store::encrypt_auth_data(&auth_data).expect("encrypt failed");
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let decrypted_json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(decrypted_json["password"], long_password);
}

// ==================== decrypt_auth_data ====================

#[test]
fn test_decrypt_empty_auth_data() {
    let result = auth_store::decrypt_auth_data("").expect("decrypt failed");
    assert_eq!(result, "");
}

#[test]
fn test_decrypt_non_json() {
    // 非 JSON 字符串：serde_json::from_str 失败后 unwrap_or 返回空 Object，序列化为 "{}"
    let result = auth_store::decrypt_auth_data("not-json").expect("decrypt failed");
    assert_eq!(result, "{}");
}

#[test]
fn test_decrypt_no_password_field() {
    let auth_data = r#"{"username":"admin","host":"localhost"}"#;
    let result = auth_store::decrypt_auth_data(auth_data).expect("decrypt failed");
    let json: serde_json::Value = serde_json::from_str(&result).expect("parse failed");
    assert_eq!(json["username"], "admin");
    assert_eq!(json["host"], "localhost");
}

#[test]
fn test_encrypt_additional_fields_preserved() {
    // 加密不应丢失非密码字段
    let auth_data = r#"{"username":"admin","password":"secret","host":"db.example.com","port":"3306","database":"mydb"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(json["username"], "admin");
    assert_eq!(json["password"], "secret");
    assert_eq!(json["host"], "db.example.com");
    assert_eq!(json["port"], "3306");
    assert_eq!(json["database"], "mydb");
}

#[test]
fn test_encrypt_no_password_at_all() {
    // 没有 password/passphrase/clientSecret 字段的数据
    let auth_data = r#"{"username":"admin","host":"localhost"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    assert!(!encrypted.contains("AES:"));
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    let json: serde_json::Value = serde_json::from_str(&decrypted).expect("parse failed");
    assert_eq!(json["username"], "admin");
}

#[test]
fn test_encrypt_non_object_json() {
    // 非 JSON 对象的 auth_data 应返回错误
    let result = auth_store::encrypt_auth_data(r#"["array","not","object"]"#);
    assert!(result.is_err());
}

#[test]
fn test_encrypt_invalid_json() {
    let result = auth_store::encrypt_auth_data("{invalid json");
    assert!(result.is_err());
}

// ==================== AES 前缀规则 ====================

#[test]
fn test_encrypt_adds_aes_prefix() {
    let auth_data = r#"{"password":"test"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    assert!(
        encrypted.contains(r#""password":"AES:"#),
        "加密后密码应包含 AES: 前缀, 实际: {}",
        encrypted
    );
}

#[test]
fn test_decrypt_removes_aes_prefix() {
    let auth_data = r#"{"password":"test"}"#;
    let encrypted = auth_store::encrypt_auth_data(auth_data).expect("encrypt failed");
    let decrypted = auth_store::decrypt_auth_data(&encrypted).expect("decrypt failed");
    assert!(
        !decrypted.contains("AES:"),
        "解密后不应包含 AES: 前缀, 实际: {}",
        decrypted
    );
    assert!(decrypted.contains(r#""password":"test""#));
}
