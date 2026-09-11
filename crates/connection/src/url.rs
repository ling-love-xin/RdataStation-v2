//! 连接 URL 处理（C3 收敛：自 workbench `connection_service` / `nav_runtime` 下沉）。
//!
//! 职责：记录 → URL 组装（userinfo 百分号编码）、URL 脱敏、URL 凭据提取。
//! 只依赖 `shared`（加密）与本 crate 的 `model::DataSource`，不依赖 engine，
//! 因此可被传输层与 UI 层同时复用。

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use crate::model::DataSource;

/// userinfo 段允许的字符集（RFC 3986 unreserved）；其余（含 `:` `@` `%` `?` `/` `#`）一律编码。
const USERINFO: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// userinfo 百分号编码（用户名 / 密码中的特殊字符不得原样进入 URL）。
pub fn encode_userinfo(s: &str) -> String {
    utf8_percent_encode(s, USERINFO).to_string()
}

/// 组装运行时连接 URL。
///
/// - 网络库：`{driver}://{user[:pass]@}{host}[:port]/[database]`
/// - 文件库（sqlite / duckdb）：`{driver}://{path}`
///
/// 密码取自 `password_encrypted`（AES 解密后百分号编码）；
/// 组装失败（密文损坏 / 文件库缺路径）返回中文错误。
pub fn build_connection_url(ds: &DataSource) -> Result<String, String> {
    let driver = ds.db_type.as_str();
    if matches!(driver, "sqlite" | "duckdb") {
        let path = ds
            .database
            .clone()
            .or_else(|| ds.host.clone())
            .filter(|p| !p.is_empty())
            .ok_or_else(|| "文件型连接缺少数据库路径".to_string())?;
        return Ok(format!("{driver}://{path}"));
    }

    let password = match ds.password_encrypted.as_deref() {
        Some(enc) if !enc.is_empty() => {
            Some(shared::crypto::decrypt_password(enc).map_err(|e| format!("解密密码失败: {e}"))?)
        }
        _ => None,
    };

    let host = ds.host.clone().unwrap_or_else(|| "127.0.0.1".to_string());
    let port = ds.port.map(|p| format!(":{p}")).unwrap_or_default();
    let database = ds.database.clone().unwrap_or_default();
    let cred = match (ds.username.as_deref(), password.as_deref()) {
        (Some(u), Some(p)) if !u.is_empty() => {
            format!("{}:{}@", encode_userinfo(u), encode_userinfo(p))
        }
        (Some(u), None) if !u.is_empty() => format!("{}@", encode_userinfo(u)),
        _ => String::new(),
    };

    Ok(format!("{driver}://{cred}{host}{port}/{database}"))
}

/// 脱敏 URL 中的密码（`******`），用于日志与展示。
pub fn mask_password_in_url(url: &str) -> String {
    if let Some(scheme_end) = url.find("://") {
        let prefix = &url[..scheme_end + 3];
        let rest = &url[scheme_end + 3..];
        if let Some(at_pos) = rest.find('@') {
            let auth = &rest[..at_pos];
            let host_part = &rest[at_pos..];
            if let Some(colon_pos) = auth.find(':') {
                let username = &auth[..colon_pos];
                return format!("{}{}:******{}", prefix, username, host_part);
            }
            return format!("{}{}******{}", prefix, auth, host_part);
        }
    }
    url.to_string()
}

/// 从 URL 提取 `(username, password)`（旧数据迁移与持久化补齐用）。
///
/// 支持 `mysql://user:pass@host:port/database` 形式；文件库 URL 返回 `(None, None)`。
pub fn extract_credentials_from_url(url: &str) -> (Option<String>, Option<String>) {
    let clean_url = if let Some(pos) = url.find("://") {
        &url[pos + 3..]
    } else {
        return (None, None);
    };

    if let Some(at_pos) = clean_url.find('@') {
        let auth_part = &clean_url[..at_pos];
        if let Some(colon_pos) = auth_part.find(':') {
            let username = auth_part[..colon_pos].to_string();
            let password = auth_part[colon_pos + 1..].to_string();
            (Some(username), Some(password))
        } else {
            (Some(auth_part.to_string()), None)
        }
    } else {
        (None, None)
    }
}

/// URL 是否含明文密码（旧数据迁移检测：存在 `user:pass@` 且未脱敏）。
pub fn url_has_plaintext_password(url: &str) -> bool {
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        if let Some(at_pos) = rest.find('@') {
            let auth = &rest[..at_pos];
            return auth.contains(':') && !auth.contains("******");
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConnectionScope, DataSource};

    fn ds(db_type: &str, host: Option<&str>, port: Option<u16>, database: Option<&str>) -> DataSource {
        DataSource {
            id: "G_conn_demo".to_string(),
            name: "demo".to_string(),
            db_type: db_type.to_string(),
            host: host.map(str::to_string),
            port,
            database: database.map(str::to_string),
            schema_name: None,
            username: None,
            password_encrypted: None,
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
            use_duckdb_fed: false,
            metadata_path: None,
            server_version: None,
            scope: ConnectionScope::Global,
            is_active: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn test_build_url_network_with_encoded_credentials() {
        let mut d = ds("postgres", Some("10.0.0.9"), Some(5433), Some("warehouse"));
        d.username = Some("alice@corp".to_string());
        d.password_encrypted = Some(shared::crypto::encrypt_password("p@ss:w/rd%").expect("encrypt"));

        let url = build_connection_url(&d).expect("build");
        assert_eq!(url, "postgres://alice%40corp:p%40ss%3Aw%2Frd%25@10.0.0.9:5433/warehouse");
        // 脱敏保留用户名，隐藏密码。
        assert_eq!(
            mask_password_in_url(&url),
            "postgres://alice%40corp:******@10.0.0.9:5433/warehouse"
        );
    }

    #[test]
    fn test_build_url_without_password() {
        let mut d = ds("mysql", Some("127.0.0.1"), Some(3306), Some("orders"));
        d.username = Some("root".to_string());
        assert_eq!(
            build_connection_url(&d).expect("build"),
            "mysql://root@127.0.0.1:3306/orders"
        );
    }

    #[test]
    fn test_build_url_file_db_uses_database_field() {
        let d = ds("duckdb", None, None, Some("/data/analytics.duckdb"));
        assert_eq!(
            build_connection_url(&d).expect("build"),
            "duckdb:///data/analytics.duckdb"
        );

        let empty = ds("sqlite", None, None, None);
        assert!(build_connection_url(&empty).is_err(), "缺路径应报错");
    }

    #[test]
    fn test_build_url_broken_ciphertext_errors() {
        let mut d = ds("postgres", Some("h"), Some(5432), Some("db"));
        d.username = Some("u".to_string());
        d.password_encrypted = Some("not-a-valid-cipher".to_string());
        assert!(build_connection_url(&d).is_err(), "密文损坏应报错");
    }

    #[test]
    fn test_extract_credentials() {
        assert_eq!(
            extract_credentials_from_url("mysql://root:secret@h:3306/db"),
            (Some("root".to_string()), Some("secret".to_string()))
        );
        assert_eq!(
            extract_credentials_from_url("postgres://onlyuser@h/db"),
            (Some("onlyuser".to_string()), None)
        );
        assert_eq!(
            extract_credentials_from_url("sqlite:///tmp/x.db"),
            (None, None)
        );
        assert_eq!(extract_credentials_from_url("not-a-url"), (None, None));
    }

    #[test]
    fn test_mask_password_without_auth_is_identity() {
        assert_eq!(
            mask_password_in_url("duckdb:///data/analytics.duckdb"),
            "duckdb:///data/analytics.duckdb"
        );
        // 无密码但有用户名：保留用户名（修复旧实现丢失用户名的问题）。
        assert_eq!(
            mask_password_in_url("mysql://root@h:3306/db"),
            "mysql://root******@h:3306/db"
        );
    }

    #[test]
    fn test_plaintext_password_detection() {
        assert!(url_has_plaintext_password("mysql://root:secret@h:3306/db"));
        assert!(!url_has_plaintext_password("mysql://root:******@h:3306/db"));
        assert!(!url_has_plaintext_password("mysql://root@h:3306/db"));
        assert!(!url_has_plaintext_password("duckdb:///data/x.duckdb"));
    }
}
