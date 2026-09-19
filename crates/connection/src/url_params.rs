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
/// `driver` 是驱动 id（供 [`append_ssl_params`] 分派参数写法）。
pub fn inject_chain_ssl_params(
    url: &str,
    hops: &[ChainHop],
    driver: &str,
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
            // 档案只有 `verify_server_cert` 布尔（没有模式字段）：沿用收敛前的映射。
            let mode = SslMode::from_ssl_config(ssl_config);
            return append_ssl_params(url, driver, mode, ssl_config);
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

/// SSL/TLS 模式（UI 与连接对话框的词汇，**不是**任何客户端库的字面量）。
///
/// 各驱动在 [`append_ssl_params`] 里映射到自己的写法：sqlx MySQL 用
/// `PREFERRED/REQUIRED/VERIFY_CA/VERIFY_IDENTITY`，sqlx PostgreSQL 用
/// `prefer/require/verify-ca/verify-full`，mysql_async 用
/// `require_ssl/verify_ca/verify_identity` 布尔，tokio-postgres 只有三档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslMode {
    /// 明确不要 TLS
    Disable,
    /// 能协商就加密，服务端不支持则明文
    Prefer,
    /// 必须加密，但不校验证书
    Require,
    /// 必须加密 + 校验证书链（`verify-ca`）
    VerifyCa,
    /// 必须加密 + 校验证书链与主机名（`verify-full` / `verify-identity`）
    VerifyFull,
}

impl SslMode {
    /// 解析模式字符串：大小写、连字符与下划线均不敏感（`VERIFY_CA` / `verify-ca` 同义）。
    pub fn parse(s: &str) -> Option<Self> {
        let key = s.trim().to_ascii_lowercase().replace('_', "-");
        match key.as_str() {
            "disable" | "disabled" => Some(SslMode::Disable),
            "prefer" | "preferred" | "allow" => Some(SslMode::Prefer),
            "require" | "required" => Some(SslMode::Require),
            "verify-ca" => Some(SslMode::VerifyCa),
            "verify-full" | "verify-identity" | "verifyidentity" => Some(SslMode::VerifyFull),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SslMode::Disable => "disable",
            SslMode::Prefer => "prefer",
            SslMode::Require => "require",
            SslMode::VerifyCa => "verify-ca",
            SslMode::VerifyFull => "verify-full",
        }
    }

    /// 网络档案（只有 `verify_server_cert` 布尔 + 可选 CA 路径）→ 模式。
    ///
    /// 语义与收敛前保持一致：要校验证书且有 CA 路径 → `VERIFY_CA`；
    /// 其余（含「要校验但没给 CA」）→ `REQUIRE`。
    pub fn from_ssl_config(ssl: &SslConfig) -> Self {
        match (ssl.verify_server_cert, ssl.ca_cert_path.is_some()) {
            (true, true) => SslMode::VerifyCa,
            _ => SslMode::Require,
        }
    }

    /// 该模式是否要求「校验证书」（要证书材料或要主机名校验）。
    pub fn requires_verification(&self) -> bool {
        matches!(self, SslMode::VerifyCa | SslMode::VerifyFull)
    }
}

/// 把 URL 的 scheme 换成**客户端库认的**那一个。
///
/// 为什么需要：连接记录里的 `db_type` 是**驱动 id**（`mysql_native` / `postgres_native`），
/// 而 `mysql_async` 只认 `mysql://`（其余 scheme 报 `UrlError::UnsupportedScheme`）、
/// `tokio-postgres` 只认 `postgres://` / `postgresql://`（两个前缀剥离都不命中时，
/// 整串会被当成 `key=value` 连接串去解析而报错）。
/// 与 federation 侧 `accel::normalize_scheme` 同一回事（那里是给 DuckDB scanner 换）。
pub fn normalize_url_scheme(url: &str, scheme: &str) -> String {
    match url.split_once("://") {
        Some((_, rest)) => format!("{}://{}", scheme, rest),
        None => url.to_string(),
    }
}

/// 追加 SSL 连接参数（**按驱动分派**，见 [`SslMode`] 的说明）。
///
/// `driver` 是驱动 id（`drivers.id`，如 `mysql` / `mysql_native`）——
/// 不能用数据库族 id：解析这条 URL 的是**具体客户端库**，两者的参数词汇并不相同。
///
/// 各驱动的能力边界（依据依赖源码，改动前请复读）：
/// - `mysql` / `mariadb`（sqlx）：`ssl-mode` + `ssl-ca` / `ssl-cert` / `ssl-key`；
/// - `postgres` / `pgsql`（sqlx）：`sslmode` + `sslrootcert` / `sslcert` / `sslkey`；
/// - `mysql_native`（mysql_async）：只有 `require_ssl` / `verify_ca` / `verify_identity`，
///   未知参数会直接报错（不是忽略）；证书文件写不进 URL，由驱动从
///   [`crate::config::TlsRequest`] 落到 `SslOpts`；
/// - `postgres_native`（tokio-postgres）：只有 `sslmode=disable|prefer|require`
///   （未知键报错），**verify 档与证书路径都靠驱动的 TLS 连接器**；
///   所以 verify-ca / verify-full 在 URL 上退化为 `sslmode=require`（加密），
///   校验意图随 `TlsRequest` 一起交给驱动——**两边必须成对使用**（唯一派生点是
///   `workbench::services::connection_service::tls_request_of`）；
/// - 文件型（sqlite / duckdb）与未实现族：不注入，返回原 URL（记日志）。
pub fn append_ssl_params(
    url: &str,
    driver: &str,
    mode: SslMode,
    ssl_config: &SslConfig,
) -> Result<String, CoreError> {
    let mut params = String::new();

    match driver {
        "mysql" | "mariadb" => {
            let literal = match mode {
                SslMode::Disable => "DISABLED",
                SslMode::Prefer => "PREFERRED",
                SslMode::Require => "REQUIRED",
                SslMode::VerifyCa => "VERIFY_CA",
                SslMode::VerifyFull => "VERIFY_IDENTITY",
            };
            push_param(&mut params, "ssl-mode", literal);
            push_path(&mut params, "ssl-ca", ssl_config.ca_cert_path.as_deref());
            push_path(&mut params, "ssl-cert", ssl_config.client_cert_path.as_deref());
            push_path(&mut params, "ssl-key", ssl_config.client_key_path.as_deref());
        }
        "postgres" | "postgresql" | "pgsql" => {
            let literal = match mode {
                SslMode::Disable => "disable",
                SslMode::Prefer => "prefer",
                SslMode::Require => "require",
                SslMode::VerifyCa => "verify-ca",
                SslMode::VerifyFull => "verify-full",
            };
            push_param(&mut params, "sslmode", literal);
            push_path(&mut params, "sslrootcert", ssl_config.ca_cert_path.as_deref());
            push_path(&mut params, "sslcert", ssl_config.client_cert_path.as_deref());
            push_path(&mut params, "sslkey", ssl_config.client_key_path.as_deref());
        }
        "mysql_native" => {
            // 三个布尔都要写全：只给 `require_ssl=true` 时 mysql_async 的默认值是
            // **校验证书链 + 校验主机名**（`danger_*` 均 false），对自签服务端会直接连不上；
            // 它的「加密不校验」对应显式 `verify_ca=false`。
            match mode {
                SslMode::Disable => return Ok(url.to_string()),
                // 无 prefer 档：要加密就只能 require
                SslMode::Prefer | SslMode::Require => {
                    push_param(&mut params, "require_ssl", "true");
                    push_param(&mut params, "verify_ca", "false");
                }
                SslMode::VerifyCa | SslMode::VerifyFull => {
                    push_param(&mut params, "require_ssl", "true");
                    push_param(&mut params, "verify_ca", "true");
                    push_param(
                        &mut params,
                        "verify_identity",
                        if mode == SslMode::VerifyFull { "true" } else { "false" },
                    );
                }
            }
        }
        "postgres_native" => {
            let literal = match mode {
                SslMode::Disable => "disable",
                SslMode::Prefer => "prefer",
                // verify 档在 URL 上只能到 require；校验由驱动侧 TlsConnector 落实
                SslMode::Require | SslMode::VerifyCa | SslMode::VerifyFull => "require",
            };
            push_param(&mut params, "sslmode", literal);
        }
        "sqlite" | "duckdb" => {
            tracing::warn!(driver = %driver, "文件型驱动没有 TLS 语义，忽略 SSL 配置");
            return Ok(url.to_string());
        }
        _ => {
            tracing::warn!(
                driver = %driver,
                "该驱动未声明 SSL 参数写法（JDBC/ODBC/插件驱动待接），跳过 SSL 参数注入"
            );
            return Ok(url.to_string());
        }
    }

    // 原生驱动的证书文件不在 URL 里表达（见函数头注释）：这里只提醒，不算失败。
    if matches!(driver, "mysql_native" | "postgres_native") {
        if let Some(what) = first_cert_path(ssl_config) {
            tracing::debug!(
                driver = %driver,
                field = what,
                "证书路径不在 URL 中表达，由驱动从 TlsRequest 落到 TLS 连接器"
            );
        }
    }

    if params.is_empty() {
        return Ok(url.to_string());
    }
    Ok(append_url_params(url, &params))
}

/// 第一个非空的证书路径字段（仅用于日志/测试断言，不参与 URL 拼装）。
fn first_cert_path(ssl: &SslConfig) -> Option<&'static str> {
    let non_empty = |p: &Option<String>| p.as_deref().is_some_and(|v| !v.trim().is_empty());
    if non_empty(&ssl.ca_cert_path) {
        return Some("CA 证书");
    }
    if non_empty(&ssl.client_cert_path) {
        return Some("客户端证书");
    }
    if non_empty(&ssl.client_key_path) {
        return Some("客户端私钥");
    }
    None
}

fn push_param(params: &mut String, key: &str, value: &str) {
    if !params.is_empty() {
        params.push('&');
    }
    params.push_str(key);
    params.push('=');
    params.push_str(value);
}

fn push_path(params: &mut String, key: &str, path: Option<&str>) {
    if let Some(p) = path.map(str::trim).filter(|p| !p.is_empty()) {
        push_param(params, key, p);
    }
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
        // 档案路径（`verify_server_cert` + CA）→ 各驱动自己的词汇
        let cfg = ssl(true, Some("/ca"), Some("/cert"), Some("/key"));
        let mode = SslMode::from_ssl_config(&cfg);

        let url = append_ssl_params("mysql://h/db", "mysql", mode, &cfg).unwrap();
        assert_eq!(
            url,
            "mysql://h/db?ssl-mode=VERIFY_CA&ssl-ca=/ca&ssl-cert=/cert&ssl-key=/key"
        );

        let url = append_ssl_params("postgres://h/db", "postgres", mode, &cfg).unwrap();
        assert_eq!(
            url,
            "postgres://h/db?sslmode=verify-ca&sslrootcert=/ca&sslcert=/cert&sslkey=/key"
        );

        // 非 SQL 库跳过；已有 query 时用 & 追加
        assert_eq!(
            append_ssl_params("duckdb:///x", "duckdb", mode, &cfg).unwrap(),
            "duckdb:///x"
        );
        assert_eq!(
            append_url_params("mysql://h/db?x=1", "y=2"),
            "mysql://h/db?x=1&y=2"
        );
    }

    /// 驱动 id 不是数据库族 id：同族两个实现的 SSL 参数词汇完全不同。
    ///
    /// 依据（依赖源码，改前复读）：`mysql_async 0.37` 的 `opts/mod.rs` 只认
    /// `require_ssl` / `verify_ca` / `verify_identity`，未知参数报 `UnknownParameter`；
    /// `tokio-postgres 0.7` 的 `config.rs` 只认 `sslmode=disable|prefer|require`，
    /// 未知键报 `UnknownOption`。
    #[test]
    fn native_drivers_use_their_own_tls_vocabulary() {
        let no_cert = ssl(false, None, None, None);

        // mysql_native：不能收 sqlx 的 `ssl-mode`（会报未知参数）；
        // 加密不校验必须显式写 `verify_ca=false`（mysql_async 默认是校验的）
        assert_eq!(
            append_ssl_params("mysql://h/db", "mysql_native", SslMode::Require, &no_cert).unwrap(),
            "mysql://h/db?require_ssl=true&verify_ca=false"
        );
        assert_eq!(
            append_ssl_params("mysql://h/db", "mysql_native", SslMode::Disable, &no_cert).unwrap(),
            "mysql://h/db"
        );
        assert_eq!(
            append_ssl_params("mysql://h/db", "mysql_native", SslMode::VerifyFull, &no_cert)
                .unwrap(),
            "mysql://h/db?require_ssl=true&verify_ca=true&verify_identity=true"
        );

        // postgres_native：只有三档；verify 档在 URL 上落到 require（校验靠连接器）
        assert_eq!(
            append_ssl_params(
                "postgres://h/db",
                "postgres_native",
                SslMode::Disable,
                &no_cert
            )
            .unwrap(),
            "postgres://h/db?sslmode=disable"
        );
        assert_eq!(
            append_ssl_params(
                "postgres://h/db",
                "postgres_native",
                SslMode::Prefer,
                &no_cert
            )
            .unwrap(),
            "postgres://h/db?sslmode=prefer"
        );
        assert_eq!(
            append_ssl_params(
                "postgres://h/db",
                "postgres_native",
                SslMode::VerifyFull,
                &no_cert
            )
            .unwrap(),
            "postgres://h/db?sslmode=require"
        );
    }

    /// 证书文件**不进 URL**（两个原生驱动都没有这类参数，乱写会被当未知参数报错）；
    /// 它们由驱动从 `TlsRequest` 落到 TLS 连接器（见 `engine::driver::native::*`）。
    #[test]
    fn native_urls_never_carry_cert_paths() {
        let with_certs = ssl(true, Some("/ca"), Some("/cert"), Some("/key"));

        for (driver, url) in [
            ("mysql_native", "mysql://h/db"),
            ("postgres_native", "postgres://h/db"),
        ] {
            let out = append_ssl_params(url, driver, SslMode::VerifyCa, &with_certs)
                .unwrap_or_else(|e| panic!("{driver} 不应报错（证书改走 TlsRequest）: {e}"));
            // 判据：不带 sqlx 那套证书键，也不出现证书路径本身
            // （不能只看 "ca"：`verify_ca` 里就含这两个字母）
            assert!(
                !out.contains("ssl-ca")
                    && !out.contains("sslrootcert")
                    && !out.contains("/ca")
                    && !out.contains("/cert")
                    && !out.contains("/key"),
                "{driver} 的 URL 不得带证书路径：{out}"
            );
        }

        // sqlx 驱动相反：证书就在 URL 上（这是 sqlx 的唯一通道）
        let out = append_ssl_params("mysql://h/db", "mysql", SslMode::VerifyCa, &with_certs).unwrap();
        assert!(out.contains("ssl-ca=/ca") && out.contains("ssl-cert=/cert"), "{out}");
    }

    #[test]
    fn ssl_mode_parse_is_case_and_separator_insensitive() {
        assert_eq!(SslMode::parse("DISABLED"), Some(SslMode::Disable));
        assert_eq!(SslMode::parse("verify_ca"), Some(SslMode::VerifyCa));
        assert_eq!(SslMode::parse("VERIFY-IDENTITY"), Some(SslMode::VerifyFull));
        assert_eq!(SslMode::parse(" verify-full "), Some(SslMode::VerifyFull));
        assert_eq!(SslMode::parse("maybe"), None);
        assert!(!SslMode::Require.requires_verification());
        assert!(SslMode::VerifyFull.requires_verification());
    }

    /// 驱动 id 不能当 scheme：客户端库只认自己的那一个。
    #[test]
    fn scheme_normalization_keeps_the_rest_of_the_url() {
        assert_eq!(
            normalize_url_scheme("mysql_native://root:pw@h:3306/db?x=1", "mysql"),
            "mysql://root:pw@h:3306/db?x=1"
        );
        assert_eq!(
            normalize_url_scheme("postgres_native://u:p@h:5432/db", "postgres"),
            "postgres://u:p@h:5432/db"
        );
        // 已是目标 scheme：幂等
        assert_eq!(normalize_url_scheme("mysql://h/db", "mysql"), "mysql://h/db");
        // 没有 scheme：原样返回，不猜
        assert_eq!(normalize_url_scheme("h:3306/db", "mysql"), "h:3306/db");
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
