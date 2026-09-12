//! URL 参数注入与改写（C3 第二批收敛：自 workbench `connection_service` 下沉）。
//!
//! 纯字符串逻辑，不依赖 engine：认证凭据注入、SSL / Kerberos 参数追加、
//! host:port 改写与解析、`no_proxy` 匹配、协议链 SSL 参数提取。
//! 供 workbench 连接服务与传输层共用。

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use shared::error::{ConnectionError, CoreError};

/// userinfo 段需转义的字符集：从 `NON_ALPHANUMERIC` 去掉 RFC 3986 的
/// unreserved（`- . _ ~`）与 sub-delims（`! $ & ' ( ) * + , ; =`），
/// 保留 `: @ / ? # % [ ]` 与空白/控制字符被编码——这些字符一旦裸写会破坏 URL 结构
/// （典型：密码含 `@` 时解析出的 host 被截断）。
const USERINFO_ESCAPE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'!')
    .remove(b'$')
    .remove(b'&')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b'+')
    .remove(b',')
    .remove(b';')
    .remove(b'=');

use crate::config::{ChainHop, SslConfig};

/// 将认证凭据注入到 URL 中。
///
/// 支持的 `auth_method` 与 `auth_data_json` 字段：
/// - `password` / `ldap`：`username` / `password`
/// - `pg_class`：`certPath` / `certKeyPath`
/// - `kerberos`：`principal` / `keytabPath`
///
/// 认证凭据优先级高于 URL 中已有的凭据。
pub fn inject_auth_into_url(
    url: &str,
    auth_method: &str,
    auth_data_json: &str,
) -> Result<String, CoreError> {
    let auth_data: serde_json::Value = serde_json::from_str(auth_data_json)
        .map_err(|e| CoreError::from(format!("解析 auth_data 失败: {}", e)))?;

    let obj = auth_data
        .as_object()
        .ok_or_else(|| CoreError::from("auth_data 不是 JSON 对象".to_string()))?;

    match auth_method {
        "password" | "ldap" => {
            let username = obj
                .get("username")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let password = obj
                .get("password")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if username.is_empty() && password.is_empty() {
                return Ok(url.to_string());
            }

            inject_username_password(url, username, password)
        }
        "pg_class" => {
            let cert_path = obj
                .get("certPath")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let cert_key_path = obj
                .get("certKeyPath")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if cert_path.is_empty() {
                return Ok(url.to_string());
            }

            inject_ssl_cert(url, cert_path, cert_key_path)
        }
        "kerberos" => {
            let principal = obj
                .get("principal")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let keytab_path = obj
                .get("keytabPath")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if principal.is_empty() {
                return Ok(url.to_string());
            }

            inject_kerberos(url, principal, keytab_path)
        }
        _ => {
            tracing::warn!("未支持的 auth_method: {}，跳过凭据注入", auth_method);
            Ok(url.to_string())
        }
    }
}

/// 向 URL 中注入用户名和密码（已有凭据则替换）。
pub fn inject_username_password(
    url: &str,
    username: &str,
    password: &str,
) -> Result<String, CoreError> {
    if username.is_empty() {
        return Ok(url.to_string());
    }

    if let Some(scheme_end) = url.find("://") {
        let prefix = &url[..scheme_end + 3];
        let rest = &url[scheme_end + 3..];

        if let Some(at_pos) = rest.find('@') {
            // 跳过 @ 符号，只取主机部分（否则 format 会再拼接一个 @ 导致双 @）
            let host_part = &rest[at_pos + 1..];
            return Ok(format!("{}{}:{}@{}", prefix, username, password, host_part));
        } else if let Some(path_start) = rest.find('/') {
            let host_port = &rest[..path_start];
            let path = &rest[path_start..];
            return Ok(format!(
                "{}{}:{}@{}{}",
                prefix, username, password, host_port, path
            ));
        } else {
            return Ok(format!("{}{}:{}@{}", prefix, username, password, rest));
        }
    }

    Ok(url.to_string())
}

/// 把用户名 / 密码合并进 URL 的 userinfo 段（**带百分号编码**）。
///
/// 与 [`inject_username_password`] 的差异：
/// - 该函数只做「无凭据 → 注入」，URL 已含 userinfo（`@`）时**原样返回**（尊重 URL 作者意图，
///   也避免重复注入）；
/// - 凭据统一走 RFC 3986 userinfo 转义（`p@ss:w/rd` → `p%40ss%3Aw%2Frd`），
///   含特殊字符的密码不会再截断 host；
/// - 无 `://` 的裸串（文件路径）原样返回。
///
/// 供保存 / 更新 / 测试连接的有效 URL 构建共用，保证三处凭据写法一致。
pub fn merge_credentials(url: &str, username: &str, password: &str) -> String {
    if username.is_empty() && password.is_empty() {
        return url.to_string();
    }

    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let prefix = &url[..scheme_end + 3];
    let rest = &url[scheme_end + 3..];

    if rest.contains('@') {
        return url.to_string();
    }

    let user = utf8_percent_encode(username, USERINFO_ESCAPE);
    let userinfo = if password.is_empty() {
        user.to_string()
    } else {
        format!(
            "{}:{}",
            user,
            utf8_percent_encode(password, USERINFO_ESCAPE)
        )
    };

    format!("{}{}@{}", prefix, userinfo, rest)
}

/// 向 PostgreSQL URL 中注入 SSL 证书参数。
pub fn inject_ssl_cert(
    url: &str,
    cert_path: &str,
    cert_key_path: &str,
) -> Result<String, CoreError> {
    let mut result = url.to_string();

    if !cert_path.is_empty() {
        let encoded_cert = utf8_percent_encode(cert_path, NON_ALPHANUMERIC).to_string();
        if result.contains('?') {
            result.push_str(&format!("&sslmode=verify-ca&sslcert={}", encoded_cert));
        } else {
            result.push_str(&format!("?sslmode=verify-ca&sslcert={}", encoded_cert));
        }
    }

    if !cert_key_path.is_empty() {
        let encoded_key = utf8_percent_encode(cert_key_path, NON_ALPHANUMERIC).to_string();
        result.push_str(&format!("&sslkey={}", encoded_key));
    }

    Ok(result)
}

/// 向 Kerberos URL 中注入认证参数。
pub fn inject_kerberos(
    url: &str,
    principal: &str,
    _keytab_path: &str,
) -> Result<String, CoreError> {
    let mut result = url.to_string();

    if !principal.is_empty() {
        let encoded_principal = utf8_percent_encode(principal, NON_ALPHANUMERIC).to_string();
        if result.contains('?') {
            result.push_str(&format!("&krbrprincipal={}", encoded_principal));
        } else {
            result.push_str(&format!("?krbrprincipal={}", encoded_principal));
        }
    }

    Ok(result)
}

/// 从链路 hops 中提取 SSL 配置并注入 URL 参数。
///
/// 跳过已被 Proxy→SSL 嵌套层处理的 SSL hop（前一个 hop 是代理）。
pub fn inject_chain_ssl_params(
    url: &str,
    hops: &[ChainHop],
    db_type: &str,
) -> Result<String, CoreError> {
    for (i, hop) in hops.iter().enumerate() {
        if let ChainHop::Ssl(ssl_config) = hop {
            let previous_is_proxy = i > 0
                && matches!(
                    hops[i - 1],
                    ChainHop::HttpProxy(_) | ChainHop::SocksProxy(_)
                );
            if previous_is_proxy {
                tracing::info!(
                    target: "chain",
                    hop = i,
                    "SSL 跳已由 Proxy→SSL 嵌套层处理，跳过 URL 参数注入"
                );
                continue;
            }
            return append_ssl_params(url, db_type, ssl_config);
        }
    }
    Ok(url.to_string())
}

/// 将 URL 中的 host:port 改写为新的 host:port。
///
/// 用「主机 / 端口 / 数据库」字段重建网络型 URL：**scheme 无关**（mysql / postgres / 任意插件协议），
/// 保留用户名密码与查询串。
///
/// 语义（与对话框“字段 ⇄ URL”双向同步对齐）：
/// - `host = None` 或空 → 不动主机；
/// - `port = None` → 不动端口；
/// - `database = Some("")` → 显式清空路径段（`None` = 保持原值）；
/// - 非 `scheme://` 形式（文件型裸路径）原样返回。
///
/// 与 `rewrite_url_host_port` 的区别：后者只认 mysql / postgres 两个协议且总是重写端口，
/// 供连接链路使用；本函数是 UI 字段回写 URL 的通用入口。
pub fn rewrite_url_authority(
    url: &str,
    host: Option<&str>,
    port: Option<u16>,
    database: Option<&str>,
) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let (scheme, rest) = url.split_at(scheme_end + 3);
    let (authority, tail) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    // 认证前缀（user[:pass]@）原样保留；host:port 部分拆分时只认最后一个 ':'（不处理 IPv6 字面量）。
    let (cred, hostport) = match authority.rfind('@') {
        Some(i) => (&authority[..i + 1], &authority[i + 1..]),
        None => ("", authority),
    };
    let (host_part, port_part) = match hostport.rfind(':') {
        Some(i) => (&hostport[..i], &hostport[i + 1..]),
        None => (hostport, ""),
    };
    // 端口必须是纯数字；否则整个 hostport 当作主机（如 IPv6 字面量 / 异常输入），不丢信息。
    let (current_host, current_port) = if !port_part.is_empty()
        && port_part.chars().all(|c| c.is_ascii_digit())
    {
        (host_part, port_part)
    } else {
        (hostport, "")
    };
    let new_host = match host {
        Some(h) if !h.trim().is_empty() => h.trim(),
        _ => current_host,
    };
    let new_port = match port {
        Some(p) => p.to_string(),
        None => current_port.to_string(),
    };
    let authority_new = if new_port.is_empty() {
        format!("{cred}{new_host}")
    } else {
        format!("{cred}{new_host}:{new_port}")
    };
    let query = tail.find('?').map(|i| &tail[i..]).unwrap_or("");
    let path = match database {
        Some(db) => db.trim().to_string(),
        None => tail.split('?').next().unwrap_or("").to_string(),
    };
    if path.is_empty() {
        format!("{scheme}{authority_new}{query}")
    } else {
        format!("{scheme}{authority_new}/{path}{query}")
    }
}

/// 支持 `mysql://`、`postgres://`（`sqlite://` / `duckdb://` 原样返回）。
pub fn rewrite_url_host_port(
    url: &str,
    new_host: &str,
    new_port: u16,
) -> Result<String, CoreError> {
    if url.starts_with("mysql://") || url.starts_with("postgres://") {
        let (prefix, rest) = url
            .split_once("://")
            .ok_or_else(|| CoreError::from("Invalid connection URL format"))?;
        let after_auth = if let Some(at_pos) = rest.find('@') {
            let (auth, _host_part) = rest.split_at(at_pos + 1);
            format!("{}{}:{}", auth, new_host, new_port)
        } else {
            format!("{}:{}", new_host, new_port)
        };
        let last_part = rest
            .find('@')
            .map(|p| {
                let host_section = &rest[p + 1..];
                host_section
                    .find('/')
                    .map(|s| &host_section[s..])
                    .unwrap_or("")
            })
            .unwrap_or("");
        Ok(format!("{}://{}{}", prefix, after_auth, last_part))
    } else if url.starts_with("sqlite://") || url.starts_with("duckdb://") {
        Ok(url.to_string())
    } else {
        Err(CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: url.to_string(),
            reason: "无法改写 URL，不支持的协议".to_string(),
        }))
    }
}

/// 从数据库 URL 中解析目标主机和端口。
///
/// 支持 `mysql://user:pass@host:port/db` 与 `postgres://...`；
/// 无端口时按协议给默认值（postgres 5432 / 其他 3306）。
pub fn parse_host_port_from_url(url: &str) -> Result<(String, u16), CoreError> {
    let after_scheme = if let Some(pos) = url.find("://") {
        &url[pos + 3..]
    } else {
        return Err(CoreError::connection(ConnectionError::InvalidConfig {
            conn_id: url.to_string(),
            reason: "URL 中没有找到协议前缀".to_string(),
        }));
    };

    let host_part = if let Some(at_pos) = after_scheme.find('@') {
        &after_scheme[at_pos + 1..]
    } else {
        after_scheme
    };

    let host = if let Some(slash_pos) = host_part.find('/') {
        &host_part[..slash_pos]
    } else {
        host_part
    };

    let (hostname, port) = if let Some(colon_pos) = host.rfind(':') {
        let h = &host[..colon_pos];
        let p_str = &host[colon_pos + 1..];
        let p = p_str.parse::<u16>().map_err(|_| {
            CoreError::connection(ConnectionError::InvalidConfig {
                conn_id: url.to_string(),
                reason: format!("无效端口号: {}", p_str),
            })
        })?;
        (h.to_string(), p)
    } else {
        let default_port = if url.starts_with("postgres://") {
            5432
        } else {
            3306
        };
        (host.to_string(), default_port)
    };

    Ok((hostname, port))
}

/// 根据 SslConfig 和数据库类型向 URL 追加 SSL 连接参数。
///
/// MySQL: `ssl-mode` / `ssl-ca` / `ssl-cert` / `ssl-key`；
/// PostgreSQL: `sslmode` / `sslrootcert` / `sslcert` / `sslkey`。
pub fn append_ssl_params(
    url: &str,
    db_type: &str,
    ssl_config: &SslConfig,
) -> Result<String, CoreError> {
    let params = match db_type {
        "mysql" | "mariadb" => {
            let mode = if ssl_config.verify_server_cert {
                if ssl_config.ca_cert_path.is_some() {
                    "VERIFY_CA"
                } else {
                    "REQUIRED"
                }
            } else {
                "REQUIRED"
            };
            let mut p = format!("ssl-mode={}", mode);
            if let Some(ref ca) = ssl_config.ca_cert_path {
                p.push_str(&format!("&ssl-ca={}", ca));
            }
            if let Some(ref cert) = ssl_config.client_cert_path {
                p.push_str(&format!("&ssl-cert={}", cert));
            }
            if let Some(ref key) = ssl_config.client_key_path {
                p.push_str(&format!("&ssl-key={}", key));
            }
            p
        }
        "postgres" | "postgresql" | "pgsql" => {
            let mode = if ssl_config.verify_server_cert {
                if ssl_config.ca_cert_path.is_some() {
                    "verify-ca"
                } else {
                    "require"
                }
            } else {
                "require"
            };
            let mut p = format!("sslmode={}", mode);
            if let Some(ref ca) = ssl_config.ca_cert_path {
                p.push_str(&format!("&sslrootcert={}", ca));
            }
            if let Some(ref cert) = ssl_config.client_cert_path {
                p.push_str(&format!("&sslcert={}", cert));
            }
            if let Some(ref key) = ssl_config.client_key_path {
                p.push_str(&format!("&sslkey={}", key));
            }
            p
        }
        _ => {
            tracing::info!(db_type=%db_type, "非 SQL 数据库，跳过 SSL 参数注入");
            return Ok(url.to_string());
        }
    };

    Ok(append_url_params(url, &params))
}

/// 向 URL 追加 `&params` 或 `?params`。
pub fn append_url_params(url: &str, params: &str) -> String {
    if url.contains('?') {
        format!("{}&{}", url, params)
    } else {
        format!("{}?{}", url, params)
    }
}

/// 检查目标主机是否匹配 `no_proxy` 规则列表。
///
/// 支持格式：精确主机名、IP 地址、`.domain` 后缀通配（匹配 `*.domain`）；
/// `localhost` 与 `127.0.0.1` 互相等价。
pub fn matches_no_proxy(host: &str, rules: &[String]) -> bool {
    if rules.is_empty() {
        return false;
    }
    let host_lower = host.to_lowercase();
    for rule in rules {
        let rule = rule.trim().to_lowercase();
        if rule.is_empty() {
            continue;
        }
        if rule == host_lower {
            return true;
        }
        if rule == "localhost" && (host_lower == "127.0.0.1" || host_lower == "::1") {
            return true;
        }
        if rule == "127.0.0.1" && host_lower == "localhost" {
            return true;
        }
        if let Some(suffix) = rule.strip_prefix('.') {
            if host_lower == suffix || host_lower.ends_with(&format!(".{}", suffix)) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SslConfig;

    fn ssl(verify: bool, ca: Option<&str>, cert: Option<&str>, key: Option<&str>) -> SslConfig {
        SslConfig {
            verify_server_cert: verify,
            ca_cert_path: ca.map(str::to_string),
            client_cert_path: cert.map(str::to_string),
            client_key_path: key.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn inject_credentials_replaces_existing() {
        let url = inject_username_password("mysql://old:old@h:3306/db", "new", "pw").unwrap();
        assert_eq!(url, "mysql://new:pw@h:3306/db");
        // 无 @ 但有路径
        let url = inject_username_password("postgres://h:5432/db", "u", "p").unwrap();
        assert_eq!(url, "postgres://u:p@h:5432/db");
        // 空用户名原样返回
        assert_eq!(
            inject_username_password("mysql://h/db", "", "p").unwrap(),
            "mysql://h/db"
        );
    }

    #[test]
    fn merge_credentials_encodes_userinfo() {
        // 普通凭据与既有写法等价
        assert_eq!(
            merge_credentials("mysql://h:3306/db", "root", "pwd"),
            "mysql://root:pwd@h:3306/db"
        );
        // 特殊字符必须转义，否则 host 被 `@` 截断、`: ` 造成端口歧义
        assert_eq!(
            merge_credentials("mysql://h:3306/db", "u", "p@ss:w/rd"),
            "mysql://u:p%40ss%3Aw%2Frd@h:3306/db"
        );
        // 仅密码也允许（用户名留空 → `:pass@`）
        assert_eq!(
            merge_credentials("postgres://h/db", "", "p wd"),
            "postgres://:p%20wd@h/db"
        );
        // 已含 userinfo / 空凭据 / 非 URL：原样返回
        assert_eq!(
            merge_credentials("mysql://u:p@h/db", "new", "pw"),
            "mysql://u:p@h/db"
        );
        assert_eq!(merge_credentials("mysql://h/db", "", ""), "mysql://h/db");
        assert_eq!(merge_credentials("C:/data/a.db", "u", "p"), "C:/data/a.db");
    }

    #[test]
    fn inject_auth_from_json() {
        let url = inject_auth_into_url(
            "mysql://h:3306/db",
            "password",
            r#"{"username":"u","password":"p"}"#,
        )
        .unwrap();
        assert_eq!(url, "mysql://u:p@h:3306/db");

        // 未知 method：原样返回
        let url = inject_auth_into_url("mysql://h/db", "unknown", "{}").unwrap();
        assert_eq!(url, "mysql://h/db");

        // 非 JSON 对象报错
        assert!(inject_auth_into_url("mysql://h/db", "password", "[]").is_err());
    }

    #[test]
    fn ssl_and_kerberos_params_appended() {
        let url = inject_ssl_cert("postgres://h/db", "/etc/ca.pem", "/etc/key.pem").unwrap();
        assert!(url.starts_with("postgres://h/db?sslmode=verify-ca&sslcert="));
        assert!(url.contains("&sslkey="));

        let url = inject_kerberos("postgres://h/db", "user@REALM", "").unwrap();
        assert!(url.starts_with("postgres://h/db?krbrprincipal="));
    }

    #[test]
    fn rewrite_url_authority_is_scheme_agnostic_and_keeps_credentials() {
        // 改主机 + 端口：凭据与路径保持
        assert_eq!(
            rewrite_url_authority("postgres://u:p@db:5432/warehouse", Some("127.0.0.1"), Some(6543), None),
            "postgres://u:p@127.0.0.1:6543/warehouse"
        );
        // 改数据库：主机 / 凭据保持，只换路径
        assert_eq!(
            rewrite_url_authority("mysql://root:pw@h:3306/old", None, None, Some("newdb")),
            "mysql://root:pw@h:3306/newdb"
        );
        // 显式清空数据库 → 去掉路径段
        assert_eq!(
            rewrite_url_authority("mysql://h:3306/old", None, None, Some("")),
            "mysql://h:3306"
        );
        // 保留查询串
        assert_eq!(
            rewrite_url_authority("mysql://h:3306/old?ssl-mode=REQUIRED", None, None, Some("d2")),
            "mysql://h:3306/d2?ssl-mode=REQUIRED"
        );
        // 未提供主机且端口为 None：端口保持原值（不默默丢掉）
        assert_eq!(
            rewrite_url_authority("oracle://h:1521/orcl", None, None, Some("orcl")),
            "oracle://h:1521/orcl"
        );
        // 无端口 URL + 给端口 → 补上
        assert_eq!(
            rewrite_url_authority("mysql://h/db", Some("h"), Some(3307), None),
            "mysql://h:3307/db"
        );
        // 文件型裸路径原样返回（不参与字段重建）
        assert_eq!(rewrite_url_authority("C:/data/a.db", Some("h"), Some(1), None), "C:/data/a.db");
    }

    #[test]
    fn rewrite_host_port_keeps_credentials_and_path() {
        let url = rewrite_url_host_port("postgres://u:p@db:5432/warehouse", "127.0.0.1", 45678)
            .unwrap();
        assert_eq!(url, "postgres://u:p@127.0.0.1:45678/warehouse");

        // 文件库原样返回；不支持协议报错
        assert_eq!(
            rewrite_url_host_port("sqlite:///tmp/x.db", "h", 1).unwrap(),
            "sqlite:///tmp/x.db"
        );
        assert!(rewrite_url_host_port("oracle://h/db", "h", 1).is_err());
    }

    #[test]
    fn parse_host_port_with_defaults_and_errors() {
        assert_eq!(
            parse_host_port_from_url("postgres://u:p@h/db").unwrap(),
            ("h".to_string(), 5432)
        );
        assert_eq!(
            parse_host_port_from_url("mysql://h:3307/db").unwrap(),
            ("h".to_string(), 3307)
        );
        assert!(parse_host_port_from_url("h:1/db").is_err());
        assert!(parse_host_port_from_url("mysql://h:notaport/db").is_err());
    }

    #[test]
    fn ssl_params_per_dialect() {
        let cfg = ssl(true, Some("/ca"), Some("/cert"), Some("/key"));
        let url = append_ssl_params("mysql://h/db", "mysql", &cfg).unwrap();
        assert_eq!(
            url,
            "mysql://h/db?ssl-mode=VERIFY_CA&ssl-ca=/ca&ssl-cert=/cert&ssl-key=/key"
        );

        let url = append_ssl_params("postgres://h/db", "postgres", &cfg).unwrap();
        assert_eq!(
            url,
            "postgres://h/db?sslmode=verify-ca&sslrootcert=/ca&sslcert=/cert&sslkey=/key"
        );

        // 非 SQL 库跳过；已有 query 时用 & 追加
        assert_eq!(append_ssl_params("duckdb:///x", "duckdb", &cfg).unwrap(), "duckdb:///x");
        assert_eq!(
            append_url_params("mysql://h/db?x=1", "y=2"),
            "mysql://h/db?x=1&y=2"
        );
    }

    #[test]
    fn chain_ssl_params_skip_after_proxy() {
        let hops = vec![
            ChainHop::SocksProxy(crate::config::ProxyConfig {
                host: "127.0.0.1".into(),
                port: 1080,
                auth: None,
                no_proxy: Vec::new(),
                timeout_secs: 10,
            }),
            ChainHop::Ssl(ssl(true, None, None, None)),
        ];
        // 前一个 hop 是代理 → SSL 参数跳过
        assert_eq!(
            inject_chain_ssl_params("postgres://h/db", &hops, "postgres").unwrap(),
            "postgres://h/db"
        );

        let hops = vec![ChainHop::Ssl(ssl(true, None, None, None))];
        assert_eq!(
            inject_chain_ssl_params("postgres://h/db", &hops, "postgres").unwrap(),
            "postgres://h/db?sslmode=require"
        );
    }

    #[test]
    fn no_proxy_matching() {
        let rules = vec!["localhost".to_string(), ".corp.example".to_string()];
        assert!(matches_no_proxy("127.0.0.1", &rules));
        assert!(matches_no_proxy("db.corp.example", &rules));
        assert!(!matches_no_proxy("external.com", &rules));
        assert!(!matches_no_proxy("external.com", &[]));
    }
}
