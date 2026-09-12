//! 元数据缓存身份指纹（纯计算，未接线）
//!
//! 解决的问题：目前 L2 元数据缓存按**连接 ID** 分文件（`conn_{id}.sqlite`），于是
//! 指向同一个物理库的多条连接（改名、改密码、换驱动实现、加 SSL 参数）会各自重建缓存。
//! 本模块定义「目标身份」并给出规范化串与指纹，让这些连接可以共享同一份缓存。
//!
//! 身份 = 数据库**族** + 规范化地址 + 访问主体（principal）+ 缓存格式版本。边界：
//!
//! - **不含**连接 ID、显示名：否则改名即失配，失去复用意义；
//! - **不含**驱动实现 id（`mysql_native` 等）：同库多驱动内省的是同一台库，驱动差异属于
//!   “对象覆盖面”问题，由能力掩码在命中时校验，不进身份；
//! - **不含**密码 / 密钥 / SSL / 代理 / 超时等连接参数：既不安全（会进文件名与日志），
//!   也会让轮换密码、加参数就整份缓存失效；
//! - **含** principal（用户名）：权限决定可见对象集合（`information_schema` 过滤、schema
//!   可见性、行级安全），不同主体共享缓存会让窄权限用户的缓存污染宽权限用户的树。
//!
//! 指纹只作**缓存键**，不作连接主键与唯一性约束（连接 ID 规则见 `id_prefix`）。
//!
//! 落地状态：本模块只提供纯函数与单测，**不改动任何缓存路径**；路径切换（`meta_{fp}.sqlite`）
//! 与索引表（占用 / 孤儿 / 可读描述）在导航接入 L2 的那一轮进行，
//! 见 `docs/architecture/connection/connection-dialog-architecture.md` §3.6。

use sha2::{Digest, Sha256};

/// 缓存格式版本：缓存结构变更时 +1，指纹随之变化 → 旧缓存自然分池（按约定不删除）。
pub const CACHE_FORMAT_VERSION: u32 = 1;

/// 指纹长度（十六进制字符数）。64 bit，对本机量级的连接数碰撞概率可忽略；
/// 索引表同时保存规范化串，命中时可校验（不一致按“不可复用”处理）。
pub const FINGERPRINT_LEN: usize = 16;

/// 空段占位：principal / database / schema 为空时统一使用，保证规范串定长且可读。
pub const EMPTY_PLACEHOLDER: &str = "-";

/// 规范化后的目标地址。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheAuthority {
    /// 网络型数据库（MySQL / PostgreSQL / …）。
    Network(NetworkAuthority),
    /// 文件型库（SQLite / DuckDB）。
    File {
        /// 原始路径（规范化在指纹计算时进行）。
        path: String,
    },
    /// 内存库 / 临时库：**永不共享**缓存。
    Ephemeral,
}

/// 网络型地址（与目标实例的规范化输入，不含任何连接参数）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkAuthority {
    /// 主机（大小写不敏感；IPv6 可带或不带方括号）。
    pub host: String,
    /// 实际端口；为空时回落 `default_port`。
    pub port: Option<u16>,
    /// 驱动声明的默认端口：仅参与归一，让 `h/db` 与 `h:3306/db` 视为同一身份。
    pub default_port: Option<u16>,
    /// 数据库 / catalog（大小写敏感，不折叠）。
    pub database: Option<String>,
    /// schema / 命名空间（大小写敏感，不折叠；影响可见对象集合）。
    pub schema: Option<String>,
}

/// 连接的目标身份（用于派生元数据缓存键）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionIdentity {
    /// 数据库**族** id（如 `mysql`），**不是**驱动实现 id（`mysql_native`）。
    pub type_id: String,
    /// 规范化目标地址。
    pub authority: CacheAuthority,
    /// 访问主体：用户名（优先）/ 认证档案 id / 认证类型；缺省为匿名。
    pub principal: Option<String>,
    /// 缓存格式版本（默认取 [`CACHE_FORMAT_VERSION`]）。
    pub format_version: u32,
}

impl ConnectionIdentity {
    /// 构造身份（格式版本取当前 [`CACHE_FORMAT_VERSION`]）。
    pub fn new(
        type_id: impl Into<String>,
        authority: CacheAuthority,
        principal: Option<&str>,
    ) -> Self {
        Self {
            type_id: type_id.into(),
            authority,
            principal: principal.map(|p| p.to_string()),
            format_version: CACHE_FORMAT_VERSION,
        }
    }

    /// 覆盖格式版本（测试 / 迁移校验用）。
    pub fn with_format_version(mut self, version: u32) -> Self {
        self.format_version = version;
        self
    }

    /// 规范化身份串（可读，供索引表展示与排障；不含密码 / 密钥）。
    ///
    /// 形如 `v1|mysql|net:db.internal:3306/orders/-|user:app_ro`；
    /// 地址不可用（主机为空 / 文件路径为空 / 内存库）时返回 `None`。
    pub fn canonical_string(&self) -> Option<String> {
        let authority = match &self.authority {
            CacheAuthority::Network(net) => {
                let host = normalize_host(&net.host);
                if host.is_empty() {
                    // 地址未知（驱动属性未回填等）→ 不做共享，宁可各建各的缓存。
                    return None;
                }
                let port = net
                    .port
                    .or(net.default_port)
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| EMPTY_PLACEHOLDER.to_string());
                format!(
                    "net:{host}:{port}/{}/{}",
                    normalize_segment(net.database.as_deref()),
                    normalize_segment(net.schema.as_deref())
                )
            }
            CacheAuthority::File { path } => {
                if is_ephemeral_path(path) {
                    return None;
                }
                let normalized = normalize_file_path(path);
                if normalized.is_empty() {
                    return None;
                }
                format!("file:{normalized}")
            }
            CacheAuthority::Ephemeral => return None,
        };
        Some(format!(
            "v{}|{}|{}|user:{}",
            self.format_version,
            self.type_id.trim().to_ascii_lowercase(),
            authority,
            normalize_segment(self.principal.as_deref())
        ))
    }

    /// 身份指纹（`sha256` 前 16 位十六进制）；不可共享时返回 `None`。
    pub fn fingerprint(&self) -> Option<String> {
        let canonical = self.canonical_string()?;
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        let hex = format!("{:x}", hasher.finalize());
        Some(hex[..FINGERPRINT_LEN].to_string())
    }

    /// 计划中的缓存文件名 `meta_{fp}.sqlite`（**未接线**，切换时机见模块文档）。
    pub fn cache_file_name(&self) -> Option<String> {
        self.fingerprint().as_deref().map(cache_file_name)
    }
}

/// 缓存文件名约定（与 [`ConnectionIdentity::fingerprint`] 配对使用）。
pub fn cache_file_name(fingerprint: &str) -> String {
    format!("meta_{fingerprint}.sqlite")
}

/// 文件型地址规范化（生产入口：Windows 折叠大小写）。
pub fn normalize_file_path(path: &str) -> String {
    normalize_file_path_with(path, cfg!(windows))
}

/// 文件型地址规范化（可显式指定是否折叠大小写，便于单测覆盖两种平台行为）。
///
/// 规则：去 URI scheme（`file:` / `sqlite:` / `duckdb:`）→ 去查询串与片段 →
/// 反斜杠统一为 `/` → 折叠重复分隔符（保留 UNC 前导 `//`）→ 去尾部 `/` →
/// 去 Windows 三斜杠残留的前导 `/`（`/C:/x.db` → `c:/x.db`）→ 可选大小写折叠。
pub fn normalize_file_path_with(path: &str, fold_case: bool) -> String {
    let mut p = path.trim();
    for scheme in ["sqlite:", "duckdb:", "file:"] {
        if let Some(rest) = strip_scheme_ci(p, scheme) {
            p = rest;
            break;
        }
    }
    // 查询串 / 片段只影响打开方式（如 `?mode=ro`），不改变库文件身份。
    p = p.split(['?', '#']).next().unwrap_or("");

    let mut s = collapse_slashes(&p.replace('\\', "/"));
    if s.len() > 1 {
        s = s.trim_end_matches('/').to_string();
    }
    s = strip_drive_leading_slash(&s);
    if fold_case {
        s = s.to_ascii_lowercase();
    }
    s
}

/// 内存库 / 临时库判定：`:memory:`、`file::memory:[?...]`、`sqlite::memory:` 等一律不共享。
pub fn is_ephemeral_path(path: &str) -> bool {
    let mut p = path.trim();
    for scheme in ["sqlite:", "duckdb:", "file:"] {
        if let Some(rest) = strip_scheme_ci(p, scheme) {
            p = rest;
            break;
        }
    }
    let p = p.split(['?', '#']).next().unwrap_or("").trim();
    let p = p.trim_start_matches("//");
    p.is_empty() || p.eq_ignore_ascii_case(":memory:")
}

/// 主机归一：去首尾空白 → 去 IPv6 方括号（统一由本函数补回）→ 小写。
fn normalize_host(host: &str) -> String {
    let h = host.trim();
    let h = h
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(h);
    let h = h.to_ascii_lowercase();
    if h.is_empty() {
        h
    } else if h.contains(':') {
        format!("[{h}]")
    } else {
        h
    }
}

/// 段归一：空 / 全空白 → 占位符；否则保留原样（数据库名与用户名均大小写敏感）。
fn normalize_segment(segment: Option<&str>) -> String {
    match segment.map(str::trim).filter(|s| !s.is_empty()) {
        Some(value) => value.to_string(),
        None => EMPTY_PLACEHOLDER.to_string(),
    }
}

/// 大小写不敏感地剥离 URI scheme（返回 scheme 之后的部分）。
fn strip_scheme_ci<'a>(input: &'a str, scheme: &str) -> Option<&'a str> {
    if input.len() >= scheme.len() && input[..scheme.len()].eq_ignore_ascii_case(scheme) {
        Some(&input[scheme.len()..])
    } else {
        None
    }
}

/// 折叠重复 `/`：保留 UNC 的前导 `//` 与绝对路径的前导 `/`。
///
/// 三个及以上前导 `/` 按「空 authority 的 file URI」处理（`file:///tmp/a.db` ≡ `/tmp/a.db`），
/// 否则 UNC 会被误当成 POSIX 绝对路径。
fn collapse_slashes(input: &str) -> String {
    let (mut lead, mut rest) = if let Some(rest) = input.strip_prefix("//") {
        ("//", rest)
    } else if let Some(rest) = input.strip_prefix('/') {
        ("/", rest)
    } else {
        ("", input)
    };
    if lead == "//" && rest.starts_with('/') {
        lead = "/";
        rest = rest.trim_start_matches('/');
    }
    let mut out = String::with_capacity(input.len());
    out.push_str(lead);
    let mut prev_slash = false;
    for ch in rest.chars() {
        if ch == '/' {
            if !prev_slash {
                out.push('/');
            }
            prev_slash = true;
        } else {
            out.push(ch);
            prev_slash = false;
        }
    }
    out
}

/// 去 Windows 三斜杠残留的前导 `/`（`///C:/x.db` → `C:/x.db`；UNC 与绝对路径不受影响）。
fn strip_drive_leading_slash(path: &str) -> String {
    let trimmed = path.trim_start_matches('/');
    let bytes = trimmed.as_bytes();
    let looks_like_drive = trimmed.len() < path.len()
        && bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':';
    if looks_like_drive {
        trimmed.to_string()
    } else {
        path.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(
        type_id: &str,
        host: &str,
        port: Option<u16>,
        default_port: Option<u16>,
        database: &str,
        schema: &str,
        principal: &str,
    ) -> ConnectionIdentity {
        ConnectionIdentity::new(
            type_id,
            CacheAuthority::Network(NetworkAuthority {
                host: host.to_string(),
                port,
                default_port,
                database: Some(database.to_string()),
                schema: Some(schema.to_string()),
            }),
            Some(principal),
        )
    }

    #[test]
    fn default_port_filled_yields_same_identity() {
        let with_port = network("mysql", "db.internal", Some(3306), None, "orders", "-", "app");
        let implicit = network("mysql", "db.internal", None, Some(3306), "orders", "-", "app");
        assert_eq!(with_port.canonical_string(), implicit.canonical_string());
        assert_eq!(with_port.fingerprint(), implicit.fingerprint());
    }

    #[test]
    fn host_is_case_folded_but_database_is_not() {
        let upper = network("mysql", "DB.Internal", Some(3306), None, "Orders", "PUBLIC", "app");
        let lower = network("mysql", "db.internal", Some(3306), None, "Orders", "PUBLIC", "app");
        assert_eq!(upper.fingerprint(), lower.fingerprint());

        let other_case_db = network("mysql", "db.internal", Some(3306), None, "orders", "PUBLIC", "app");
        assert_ne!(upper.fingerprint(), other_case_db.fingerprint());
    }

    #[test]
    fn ipv6_brackets_are_normalized() {
        let bracketed = network("postgres", "[2001:DB8::1]", Some(5432), None, "app", "-", "svc");
        let bare = network("postgres", "2001:db8::1", Some(5432), None, "app", "-", "svc");
        assert_eq!(bracketed.fingerprint(), bare.fingerprint());
        assert!(bracketed.canonical_string().unwrap().contains("[2001:db8::1]"));
    }

    #[test]
    fn principal_is_part_of_identity_and_case_preserved() {
        let read_only = network("postgres", "db", Some(5432), None, "app", "-", "app_ro");
        let read_write = network("postgres", "db", Some(5432), None, "app", "-", "app_rw");
        assert_ne!(read_only.fingerprint(), read_write.fingerprint());

        let upper = network("postgres", "db", Some(5432), None, "app", "-", "App");
        let lower = network("postgres", "db", Some(5432), None, "app", "-", "app");
        assert_ne!(upper.fingerprint(), lower.fingerprint());
    }

    #[test]
    fn anonymous_principal_uses_placeholder() {
        let identity = ConnectionIdentity::new(
            "mysql",
            CacheAuthority::Network(NetworkAuthority {
                host: "db".to_string(),
                port: None,
                default_port: Some(3306),
                database: None,
                schema: None,
            }),
            None,
        );
        let canonical = identity.canonical_string().unwrap();
        assert_eq!(canonical, "v1|mysql|net:db:3306/-/-|user:-");
    }

    #[test]
    fn type_family_and_format_version_break_reuse() {
        let mysql = network("mysql", "db", Some(3306), None, "app", "-", "svc");
        let mariadb = network("mariadb", "db", Some(3306), None, "app", "-", "svc");
        assert_ne!(mysql.fingerprint(), mariadb.fingerprint());

        let bumped = mysql.clone().with_format_version(CACHE_FORMAT_VERSION + 1);
        assert_ne!(mysql.fingerprint(), bumped.fingerprint());
    }

    #[test]
    fn empty_host_or_path_is_never_shared() {
        let no_host = ConnectionIdentity::new(
            "mysql",
            CacheAuthority::Network(NetworkAuthority {
                host: "  ".to_string(),
                port: Some(3306),
                default_port: None,
                database: Some("app".to_string()),
                schema: None,
            }),
            Some("svc"),
        );
        assert_eq!(no_host.fingerprint(), None);

        let empty_path = ConnectionIdentity::new(
            "sqlite",
            CacheAuthority::File { path: "   ".to_string() },
            None,
        );
        assert_eq!(empty_path.fingerprint(), None);
    }

    #[test]
    fn ephemeral_databases_are_never_shared() {
        for path in [":memory:", "file::memory:", "sqlite::memory:?cache=shared", " file::memory: "] {
            let identity = ConnectionIdentity::new("sqlite", CacheAuthority::File { path: path.to_string() }, None);
            assert!(is_ephemeral_path(path), "应判定为内存库：{path}");
            assert_eq!(identity.fingerprint(), None, "内存库不应共享缓存：{path}");
        }

        let explicit = ConnectionIdentity::new("duckdb", CacheAuthority::Ephemeral, None);
        assert_eq!(explicit.fingerprint(), None);
    }

    #[test]
    fn file_path_is_normalized_across_schemes_and_separators() {
        let from_uri = normalize_file_path_with("file:///C:/Data/Orders.DB", true);
        let from_plain = normalize_file_path_with(r"C:\Data\orders.db", true);
        let from_triple = normalize_file_path_with("sqlite:///C:/Data/orders.db", true);
        assert_eq!(from_uri, "c:/data/orders.db");
        assert_eq!(from_uri, from_plain);
        assert_eq!(from_uri, from_triple);

        let identity_uri = ConnectionIdentity::new("sqlite", CacheAuthority::File { path: "file:///C:/Data/Orders.DB".to_string() }, None);
        let identity_plain = ConnectionIdentity::new("sqlite", CacheAuthority::File { path: r"C:\Data\orders.db".to_string() }, None);
        if cfg!(windows) {
            assert_eq!(identity_uri.fingerprint(), identity_plain.fingerprint());
        }
    }

    #[test]
    fn case_folding_is_platform_specific() {
        assert_eq!(normalize_file_path_with("C:/Data/Orders.db", true), "c:/data/orders.db");
        assert_eq!(normalize_file_path_with("C:/Data/Orders.db", false), "C:/Data/Orders.db");
    }

    #[test]
    fn file_query_string_and_fragment_are_dropped() {
        let with_query = ConnectionIdentity::new(
            "sqlite",
            CacheAuthority::File { path: "file:///tmp/app.db?mode=ro&immutable=1".to_string() },
            Some("svc"),
        );
        let plain = ConnectionIdentity::new(
            "sqlite",
            CacheAuthority::File { path: "/tmp/app.db".to_string() },
            Some("svc"),
        );
        assert_eq!(with_query.fingerprint(), plain.fingerprint());
        assert_eq!(normalize_file_path_with("file:///tmp/app.db?mode=ro", false), "/tmp/app.db");
        assert_eq!(normalize_file_path_with("/tmp/app.db#frag", false), "/tmp/app.db");
    }

    #[test]
    fn unc_path_keeps_leading_double_slash() {
        assert_eq!(normalize_file_path_with(r"\\server\share\app.db", false), "//server/share/app.db");
    }

    #[test]
    fn fingerprint_shape_and_cache_file_name() {
        let identity = network("mysql", "db.internal", Some(3306), None, "orders", "-", "app_ro");
        let fingerprint = identity.fingerprint().unwrap();
        assert_eq!(fingerprint.len(), FINGERPRINT_LEN);
        assert!(fingerprint.chars().all(|c| c.is_ascii_hexdigit()));

        let canonical = identity.canonical_string().unwrap();
        assert_eq!(canonical, "v1|mysql|net:db.internal:3306/orders/-|user:app_ro");
        assert_eq!(
            identity.cache_file_name(),
            Some(cache_file_name(&fingerprint))
        );
        assert_eq!(cache_file_name("0123456789abcdef"), "meta_0123456789abcdef.sqlite");
    }
}
