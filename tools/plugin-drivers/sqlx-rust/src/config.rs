//! 连接参数（宿主 `DriverConnectionConfig` 的 JSON 原样，宿主不做有损翻译）。
//!
//! 宿主 `crates/plugin/src/sidecar/factory.rs` 把整个 `DriverConnectionConfig`
//! `serde_json` 序列化后塞进 `session.open.params`，所以这里按同一形状反序列化：
//! 认不出的字段一律忽略（`tls` 本来就 `#[serde(skip)]`，TLS 决策归宿主
//! `connection::url_params::append_ssl_params` / `apply_lan_tls_default`，
//! 插件只用拿到的 URL 与 options，不自己发明 TLS 默认）。
//!
//! 这一层只做「配置 → DSN」的纯函数，好单测、也好在错误消息里复述。

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

/// 查询超时默认 15 秒（与内核 `MYSQL_QUERY_TIMEOUT` / PG 同口径）。
pub const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(15);
/// 建连超时默认 10 秒（DBeaver 的 connect timeout 默认也是 10s 档）。
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 宿主 `DriverConnectionConfig` 的插件侧投影（只留拼 DSN 与超时需要的字段）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ConnConfig {
    pub driver: String,
    pub name: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub url_override: Option<String>,
    pub options: HashMap<String, String>,
    pub connect_timeout: Option<u32>,
    pub query_timeout: Option<u32>,
}

impl ConnConfig {
    pub fn parse(params: &Value) -> Result<Self, String> {
        serde_json::from_value(params.clone())
            .map_err(|err| format!("连接参数不是 DriverConnectionConfig 形状的 JSON：{err}"))
    }

    pub fn mysql_url(&self) -> Result<String, String> {
        self.assemble("mysql", 3306, "root", "")
    }

    pub fn postgres_url(&self) -> Result<String, String> {
        let url = self.assemble("postgres", 5432, "postgres", "postgres")?;
        // sqlx 的 PG 后端严格校验 `ssl-mode` 枚举值（大写直接报
        // `unknown value "DISABLED" for ssl_mode`）；宿主注入的是 `DISABLED` 这类大写。
        // 归一放在插件侧，与内核 `native/postgres.rs::normalize_ssl_mode` 同语义。
        Ok(normalize_ssl_mode(&url))
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
            .filter(|secs| *secs > 0)
            .map(|secs| Duration::from_secs(u64::from(secs)))
            .unwrap_or(DEFAULT_CONNECT_TIMEOUT)
    }

    pub fn query_timeout(&self) -> Duration {
        self.query_timeout
            .filter(|secs| *secs > 0)
            .map(|secs| Duration::from_secs(u64::from(secs)))
            .unwrap_or(DEFAULT_QUERY_TIMEOUT)
    }

    /// `url_override` 优先（宿主/用户手填的整条连接串），否则按字段拼；
    /// 两条路径都要把 `options` 追加成 URL query 参数。
    fn assemble(
        &self,
        scheme: &str,
        default_port: u16,
        default_user: &str,
        default_database: &str,
    ) -> Result<String, String> {
        if let Some(override_url) = self
            .url_override
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            return Ok(append_options(override_url, &self.options));
        }

        let host = self
            .host
            .as_deref()
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .ok_or_else(|| format!("{scheme} 连接缺少 host"))?;
        let port = self.port.unwrap_or(default_port);
        let user = encode_userinfo(self.username.as_deref().unwrap_or(default_user));
        let password = self.password.as_deref().unwrap_or("");
        let database = self.database.as_deref().unwrap_or(default_database);

        let mut url = if password.is_empty() {
            format!("{scheme}://{user}@{host}:{port}")
        } else {
            format!("{scheme}://{user}:{}@{host}:{port}", encode_userinfo(password))
        };
        if !database.is_empty() {
            url.push('/');
            url.push_str(&encode_path_segment(database));
        }
        Ok(append_options(&url, &self.options))
    }
}

/// 追加 options 为 query 参数；同名（忽略大小写）已存在的不覆盖 —— 显式写进
/// `url_override` 的算数，字典里的只是补缺。
fn append_options(url: &str, options: &HashMap<String, String>) -> String {
    let mut extra: Vec<(String, String)> = Vec::new();
    let existing = url
        .split_once('?')
        .map(|(_, query)| {
            query
                .split('&')
                .filter_map(|pair| pair.split_once('=').map(|(key, _)| key.to_ascii_lowercase()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut keys: Vec<&String> = options.keys().collect();
    // 稳定顺序：HashMap 的迭代序随机，同一份配置两次拼出的 URL 不该不一样。
    keys.sort();
    for key in keys {
        let value = &options[key];
        let trimmed = key.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        // `ssl-mode` 与 `ssl_mode` 是同一条参数的两个写法，不能各写一遍。
        let canonical = lower.replace('_', "-");
        if existing.iter().any(|name| name.replace('_', "-") == canonical) {
            continue;
        }
        extra.push((trimmed.to_string(), value.clone()));
    }

    if extra.is_empty() {
        return url.to_string();
    }
    let mut out = url.to_string();
    out.push(if out.contains('?') { '&' } else { '?' });
    out.push_str(
        &extra
            .iter()
            .map(|(key, value)| format!("{key}={}", encode_query_value(value)))
            .collect::<Vec<_>>()
            .join("&"),
    );
    out
}

/// `ssl-mode` 的取值归一成 sqlx 认识的小写枚举。
pub fn normalize_ssl_mode(url: &str) -> String {
    let Some((head, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let rewritten: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) if key.replace('_', "-").eq_ignore_ascii_case("ssl-mode") => {
                format!("{key}={}", ssl_mode_value(value))
            }
            _ => pair.to_string(),
        })
        .collect();
    format!("{head}?{}", rewritten.join("&"))
}

fn ssl_mode_value(raw: &str) -> String {
    match raw.trim().to_ascii_uppercase().as_str() {
        "DISABLED" | "DISABLE" => "disable".to_string(),
        "ALLOW" => "allow".to_string(),
        "PREFERRED" | "PREFER" => "prefer".to_string(),
        "REQUIRED" | "REQUIRE" => "require".to_string(),
        "VERIFY_CA" | "VERIFY-CA" => "verify-ca".to_string(),
        "VERIFY_IDENTITY" | "VERIFY_FULL" | "VERIFY-FULL" => "verify-full".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

/// 错误消息里出现的连接串必须先脱敏：口令不能进日志 / 界面报错。
pub fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let Some((userinfo, tail)) = rest.split_once('@') else {
        return url.to_string();
    };
    match userinfo.split_once(':') {
        Some((user, _)) => format!("{scheme}://{user}:***@{tail}"),
        None => url.to_string(),
    }
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn encode_userinfo(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        if is_unreserved(*byte) {
            out.push(char::from(*byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// 库名进 path 段：`/` 与空白必须转义，其余保持原样（PG 库名允许 `-` 与大小写）。
fn encode_path_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        if is_unreserved(*byte) {
            out.push(char::from(*byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn encode_query_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        if is_unreserved(*byte) {
            out.push(char::from(*byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn mysql_url_percent_encodes_userinfo_and_database() {
        let config = ConnConfig::parse(&json!({
            "driver": "mysql",
            "host": "10.0.0.5",
            "port": 3307,
            "database": "biz db",
            "username": "app@ops",
            "password": "p@ss:w/rd?#1"
        }))
        .expect("解析配置");
        let url = config.mysql_url().unwrap();
        assert_eq!(
            url,
            "mysql://app%40ops:p%40ss%3Aw%2Frd%3F%231@10.0.0.5:3307/biz%20db",
            "口令里的 @ : / ? # 空格必须转义，否则 DSN 会被截断或解析错"
        );
    }

    #[test]
    fn url_override_wins_and_options_fill_gaps() {
        let config = ConnConfig::parse(&json!({
            "driver": "postgres",
            "url_override": "postgres://u@h:5432/db?sslmode=require",
            "options": {"sslmode": "disable", "application_name": "rds"},
            "host": "ignored"
        }))
        .unwrap();
        let url = config.postgres_url().unwrap();
        assert!(url.starts_with("postgres://u@h:5432/db?sslmode=require"), "覆盖优先：{url}");
        assert!(url.contains("application_name=rds"), "缺的参数要补上：{url}");
        assert_eq!(url.matches("sslmode=").count(), 1, "已有的不能重复追加：{url}");
    }

    #[test]
    fn ssl_mode_values_are_lowercased_for_pg() {
        // 宿主注入的是大写 `DISABLED`，sqlx PG 只认小写枚举。
        let url = normalize_ssl_mode("postgres://u:p@h:5432/db?ssl-mode=DISABLED&application_name=x");
        assert_eq!(
            url,
            "postgres://u:p@h:5432/db?ssl-mode=disable&application_name=x"
        );
        // 下划线写法同样归一。
        let url = normalize_ssl_mode("postgres://u@h/db?ssl_mode=VERIFY_FULL");
        assert_eq!(url, "postgres://u@h/db?ssl_mode=verify-full");
        // 没有 query 的原样返回。
        assert_eq!(
            normalize_ssl_mode("postgres://u@h/db"),
            "postgres://u@h/db"
        );
    }

    #[test]
    fn error_messages_never_carry_the_password() {
        let url = "mysql://root:s3cret@127.0.0.1:3306/db";
        let redacted = redact_url(url);
        assert!(!redacted.contains("s3cret"), "脱敏后不该还有口令：{redacted}");
        assert_eq!(redacted, "mysql://root:***@127.0.0.1:3306/db");
        // 没有口令的连接串不动。
        assert_eq!(
            redact_url("mysql://root@127.0.0.1:3306/db"),
            "mysql://root@127.0.0.1:3306/db"
        );
    }

    #[test]
    fn timeouts_fall_back_to_defaults() {
        let config = ConnConfig::default();
        assert_eq!(config.connect_timeout(), DEFAULT_CONNECT_TIMEOUT);
        assert_eq!(config.query_timeout(), DEFAULT_QUERY_TIMEOUT);

        let config = ConnConfig::parse(&json!({
            "driver": "mysql", "host": "h", "connect_timeout": 3, "query_timeout": 0
        }))
        .unwrap();
        assert_eq!(config.connect_timeout(), Duration::from_secs(3));
        assert_eq!(config.query_timeout(), DEFAULT_QUERY_TIMEOUT, "0 视为没设");
    }
}
