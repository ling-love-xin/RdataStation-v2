//! 外部源登记：谁参与了联邦、叫什么别名、现在什么状态
//!
//! 设计见 `docs/architecture/federation/`（模块入口 + 原型 + 架构）。
//!
//! ## 这一层管什么
//!
//! - **源的描述**（[`FederatedSource`]）：连接 id + SQL 别名 + 种类 + 连接串。
//!   由**宿主组装**（引擎不读连接库、不解密口令）——与 [`super::super::accel::AccelSource`]
//!   同一分工，只是多了一个别名（加速档只有一条源，别名固定）。
//! - **别名规则**（[`sanitize_alias`] / [`unique_alias`] / [`validate_alias`]）：纯函数，
//!   界面给个默认名、会话建之前先拦下重名。
//! - **状态形状**（[`MountState`] / [`MountedSource`] / [`SessionSnapshot`]）：给界面看的
//!   **内存快照**（渲染路径不 I/O）。
//!
//! ## 不在这一层
//!
//! 挂载与执行在 [`super::session`]；连接与凭据在连接体系；持久化的"哪些连接用作联邦源"
//! 是宿主侧的事（第一期后半接）。

use super::super::accel::{AccelKind, normalize_file_path, normalize_scheme, quote_literal};

/// 别名里不能用的名字：DuckDB 自己的 catalog、加速档的固定别名，以及**最容易被当成
/// 连接名的 SQL 关键字**（`left` / `order` / `table` …——真机踩到过：`ATTACH … AS left`
/// 直接语法错，而且报错信息里只有 `syntax error at or near "left"`，看不出是别名的问题）
const RESERVED_ALIASES: &[&str] = &[
    // catalog / 会话内固定名
    "memory",
    "system",
    "temp",
    "main",
    "rds_src",
    // join 族
    "join",
    "inner",
    "outer",
    "left",
    "right",
    "full",
    "cross",
    "natural",
    "using",
    "on",
    // 查询族
    "select",
    "from",
    "where",
    "group",
    "order",
    "by",
    "having",
    "limit",
    "offset",
    "qualify",
    "window",
    "with",
    "recursive",
    "union",
    "intersect",
    "except",
    "all",
    "distinct",
    // DML / DDL 族
    "insert",
    "update",
    "delete",
    "create",
    "drop",
    "alter",
    "table",
    "view",
    "index",
    "values",
    "into",
    "as",
    "set",
    "primary",
    "foreign",
    "key",
    "default",
    "constraint",
    // 表达式族
    "case",
    "when",
    "then",
    "else",
    "end",
    "and",
    "or",
    "not",
    "null",
    "true",
    "false",
    "asc",
    "desc",
];

/// 是不是保留名（大小写不敏感）
fn is_reserved(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    RESERVED_ALIASES.contains(&lowered.as_str())
}

/// 一个 L2 源的凭据：**会话级 Secret**（不落盘）
///
/// 真机台账（架构 §2.1）：Oracle 的社区扩展只认 `ATTACH '<secret 名>'`，不接受连接串；
/// 而凭据（host / port / user / password / service_name）只能在会话里建 Secret 给上。
///
/// **口令在里面**：这个结构不能 `{:?}`（手写了 `Debug`）、错误文本要过 [`super::super::accel::scrub_credentials`]。
#[derive(Clone, PartialEq, Eq)]
pub struct SourceSecret {
    /// Secret 名（`ATTACH '<名>' AS <别名>` 用的就是它）
    pub name: String,
    /// 建它的 SQL（`CREATE OR REPLACE SECRET …`；**含口令**，只在本进程里用）
    pub create_sql: String,
    /// 口令（脱敏用；空串 = 没设密码）
    pub password: String,
}

impl std::fmt::Debug for SourceSecret {
    /// 手写 `Debug`：建 Secret 的 SQL 里有口令，不能随 `{:?}` 出去
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceSecret")
            .field("name", &self.name)
            .field("create_sql", &"<含凭据：已略>")
            .finish()
    }
}

/// `oracle://user:pass@host:port/service` → 会话级 Secret 的建法（**纯函数**）
///
/// 为什么要吃 URL：宿主只需要把凭据按一个形状递过来（连接记录里的字段也是这么进 URL 的），
/// 引擎再把它拆成 Oracle 要的那几个参数——这样“L2 与 L1 的差异”只在引擎侧一处。
///
/// 服务名取 URL 的路径段（`/XEPDB1`）；没写就说清楚（它不能猜：写成 `XE` 会得到 `ORA-01017`，
/// 真机踩过）。口令 / 用户名会先做百分号解码（与 [`connection::url`] 的编码对称）。
pub fn oracle_secret_from_url(alias: &str, url: &str) -> Result<SourceSecret, String> {
    let trimmed = url.trim();
    let rest = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .ok_or_else(|| format!("Oracle 连接串形状不对（要 `oracle://user:pass@host:port/服务名`）：{trimmed}"))?;
    let (cred, host_part) = rest
        .split_once('@')
        .ok_or_else(|| "Oracle 连接串缺少凭据（要 `user:pass@host`）".to_string())?;
    let (user, password) = cred.split_once(':').unwrap_or((cred, ""));
    let (host_port, service) = host_part
        .split_once('/')
        .ok_or_else(|| "Oracle 连接串缺少服务名（`…:1521/XEPDB1`）".to_string())?;
    let (host, port) = match host_port.split_once(':') {
        Some((host, port)) => (
            host.trim(),
            port.trim()
                .parse::<u16>()
                .map_err(|_| format!("Oracle 端口不是数字：{port}"))?,
        ),
        None => (host_port.trim(), 1521),
    };
    if host.is_empty() {
        return Err("Oracle 连接串缺少主机".to_string());
    }
    let service = decode(service.trim());
    if service.is_empty() {
        return Err(
            "Oracle 连接串缺少服务名（`…:1521/XEPDB1`）；写错服务名会得到 ORA-01017，不能猜"
                .to_string(),
        );
    }
    let user = decode(user.trim());
    let password = decode(password);
    // Secret 名跟别名走（别名已经过保留字 / 字符集检查且会话内唯一）
    let name = format!("rds_{alias}");
    let create_sql = format!(
        "CREATE OR REPLACE SECRET {name} (TYPE ORACLE, HOST {}, PORT {port}, USER {}, PASSWORD {}, SERVICE_NAME {})",
        quote_literal(host),
        quote_literal(&user),
        quote_literal(&password),
        quote_literal(&service),
    );
    Ok(SourceSecret {
        name,
        create_sql,
        password,
    })
}

/// 百分号解码（与 URL 组装侧的编码对称；解不开就原样用，不报错）
fn decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .to_string()
}

/// 一个联邦源（**宿主组装**：引擎不读连接库、不解密口令）
#[derive(Clone, PartialEq, Eq)]
pub struct FederatedSource {
    /// 源连接 id（结果集 / 历史 / 界面都拿它对上号）
    pub conn_id: String,
    /// SQL 里的别名（跨源查询写 `<别名>.<schema>.<表>`；L2 是两段 `<别名>.<表>`）
    pub alias: String,
    pub kind: AccelKind,
    /// L1 = 带凭据的连接串（网络型）或裸路径（文件型）；L2 = 原样留着的 URL（凭据解析用）
    pub connection_string: String,
    /// L2 的会话级 Secret（L1 为 `None`）
    pub secret: Option<SourceSecret>,
}

impl std::fmt::Debug for FederatedSource {
    /// 手写 `Debug`：连接串与 Secret 里都有口令，不能随 `{:?}` 出去
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FederatedSource")
            .field("conn_id", &self.conn_id)
            .field("alias", &self.alias)
            .field("kind", &self.kind)
            .field(
                "connection_string",
                &connection::url::mask_password_in_url(&self.connection_string),
            )
            .field("secret", &self.secret)
            .finish()
    }
}

impl FederatedSource {
    /// 从宿主的连接信息组装
    ///
    /// `url` 是这条连接的**运行时连接串**（含凭据）：L1 = `mysql://user:pass@host:port/db`
    /// （或文件型路径），L2 = `oracle://user:pass@host:port/服务名`。
    ///
    /// 文件型的 URL 可能是 `sqlite://D:\x.db` 这种带 scheme 的写法，而 DuckDB 要裸路径——
    /// 与加速档共用同一条剥 scheme 规则（[`normalize_file_path`]），不各写一份。
    pub fn new(conn_id: &str, alias: &str, db_type: &str, url: &str) -> Result<Self, String> {
        // 认不出驱动就说出来（不猜：猜错会把语句发到不相干的库上）
        let kind = AccelKind::from_db_type(db_type)
            .map_err(|_| format!("这个驱动（{db_type}）暂时不能做联邦源"))?;
        validate_alias(alias)?;
        if kind.needs_secret() {
            // L2：凭据走会话级 Secret，`ATTACH` 的目标是 **Secret 名**（不是连接串）
            let trimmed = url.trim();
            if trimmed.is_empty() {
                return Err("这个连接没有可用的连接串".to_string());
            }
            let secret = oracle_secret_from_url(alias, trimmed)?;
            return Ok(Self {
                conn_id: conn_id.to_string(),
                alias: alias.to_string(),
                kind,
                connection_string: trimmed.to_string(),
                secret: Some(secret),
            });
        }
        let connection_string = match kind {
            AccelKind::Sqlite | AccelKind::DuckDb => normalize_file_path(url)?,
            _ => {
                let trimmed = url.trim();
                if trimmed.is_empty() {
                    return Err("这个连接没有可用的连接串".to_string());
                }
                // 驱动 id 不能当 scheme 用（`mysql_native://` DuckDB 不认）——换成扫描器那份
                normalize_scheme(kind, trimmed)
            }
        };
        Ok(Self {
            conn_id: conn_id.to_string(),
            alias: alias.to_string(),
            kind,
            connection_string,
            secret: None,
        })
    }

    /// 挂载语句：L1 **一律只读**；L2 走 Secret（**它不支持 `READ_ONLY`**，写保护靠会话层 + 账号）
    pub fn attach_sql(&self) -> String {
        match &self.secret {
            Some(secret) => format!(
                "ATTACH {} AS {} (TYPE {})",
                quote_literal(&secret.name),
                self.alias,
                self.kind.attach_type().unwrap_or("oracle_scanner")
            ),
            None => {
                let literal = quote_literal(&self.connection_string);
                match self.kind.attach_type() {
                    Some(kind) => format!(
                        "ATTACH {literal} AS {} (TYPE {kind}, READ_ONLY)",
                        self.alias
                    ),
                    None => format!("ATTACH {literal} AS {} (READ_ONLY)", self.alias),
                }
            }
        }
    }
}

/// 展示名 → 可用的 SQL 别名（纯函数）
///
/// 规则：转小写；字母 / 数字 / 下划线之外一律换成 `_`（中文连接名会整串变成 `_`，
/// 由 [`unique_alias`] 兜成 `db` 这类可读名）；首字符必须是字母或下划线，否则加前缀 `s`。
pub fn sanitize_alias(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    // 合并连续下划线、去掉首尾下划线（`订单 库` → `_` 这种最难看的形状先规整掉）
    let mut collapsed = String::with_capacity(out.len());
    let mut last_underscore = false;
    for ch in out.chars() {
        if ch == '_' {
            if !last_underscore {
                collapsed.push('_');
            }
            last_underscore = true;
        } else {
            collapsed.push(ch);
            last_underscore = false;
        }
    }
    let trimmed = collapsed.trim_matches('_').to_string();
    if trimmed.is_empty() {
        return "db".to_string();
    }
    let first = trimmed.chars().next().unwrap_or('d');
    let candidate = if first.is_ascii_alphabetic() || first == '_' {
        trimmed
    } else {
        format!("s{trimmed}")
    };
    // 生成的默认名要**真的能用**：撞保留字就缀 `_src`（`left` → `left_src`）
    if is_reserved(&candidate) {
        format!("{candidate}_src")
    } else {
        candidate
    }
}

/// 在**已占用**的别名里取一个不重名的（纯函数）：`orders` → `orders_2` → `orders_3`…
pub fn unique_alias(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|alias| alias == base) {
        return base.to_string();
    }
    for index in 2..=9999 {
        let candidate = format!("{base}_{index}");
        if !taken.iter().any(|alias| &candidate == alias) {
            return candidate;
        }
    }
    format!("{base}_x")
}

/// 别名能不能用（会话建之前就拦下，别让 DuckDB 报一句看不懂的语法错）
pub fn validate_alias(alias: &str) -> Result<(), String> {
    let trimmed = alias.trim();
    if trimmed.is_empty() {
        return Err("别名不能为空".to_string());
    }
    if trimmed != alias {
        return Err(format!("别名不能有首尾空白（{alias:?}）"));
    }
    let mut chars = trimmed.chars();
    let first = chars.next().unwrap_or('_');
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(format!("别名要以字母或下划线开头（{alias}）"));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return Err(format!("别名只能用字母 / 数字 / 下划线（{alias}）"));
    }
    if is_reserved(trimmed) {
        return Err(format!("别名 {alias} 是 SQL 关键字或 DuckDB 保留名，换一个"));
    }
    Ok(())
}

/// 一个源的挂载状态（界面上就是"可用 / 失败 + 原话"两态）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountState {
    /// 挂上了（`tables` 是挂载时的表数量，纯展示）
    Ready { tables: usize },
    /// 挂不上 / 挂了又掉（原话在这里，不翻译不改写）
    Failed(String),
}

/// 会话里的一条源（含**重建所需的全部信息**：刷新时按同一份描述重挂）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountedSource {
    pub source: FederatedSource,
    pub state: MountState,
}

impl MountedSource {
    pub fn alias(&self) -> &str {
        &self.source.alias
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.state, MountState::Ready { .. })
    }
}

/// 会话快照（**界面读这个**：内存快照，渲染路径可调）
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionSnapshot {
    /// 主源别名（未限定名只在它里面解析）
    pub primary: Option<String>,
    pub sources: Vec<MountedSource>,
    /// 请求的主源不可用时的回退说明（要在界面上说出来，不能悄悄换）
    pub primary_note: Option<String>,
}

impl SessionSnapshot {
    /// 可用的源数（门控与文案都用它，别各自 `filter` 一遍）
    pub fn ready_count(&self) -> usize {
        self.sources.iter().filter(|source| source.is_ready()).count()
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{FederatedSource, oracle_secret_from_url, sanitize_alias, unique_alias, validate_alias};

    #[test]
    fn sanitize_turns_display_names_into_usable_aliases() {
        assert_eq!(sanitize_alias("Orders"), "orders");
        assert_eq!(sanitize_alias("orders-db"), "orders_db");
        assert_eq!(sanitize_alias("  my  db  "), "my_db");
        // 中文 / 符号整串 → 兜底名（用户可改）
        assert_eq!(sanitize_alias("订单库"), "db");
        assert_eq!(sanitize_alias("!!!"), "db");
        // 数字开头要加前缀（不加引号的标识符不能以数字开头）
        assert_eq!(sanitize_alias("2orders"), "s2orders");
        // 撞保留字要缀 `_src`（`ATTACH … AS left` 是语法错，真机踩到）
        assert_eq!(sanitize_alias("left"), "left_src");
        assert_eq!(sanitize_alias("Order"), "order_src");
    }

    #[test]
    fn unique_alias_avoids_taken_names() {
        let taken = vec!["orders".to_string(), "orders_2".to_string()];
        assert_eq!(unique_alias("orders", &taken), "orders_3");
        assert_eq!(unique_alias("fresh", &taken), "fresh");
    }

    #[test]
    fn alias_validation_refuses_unusable_names() {
        assert!(validate_alias("orders_db").is_ok());
        assert!(validate_alias("_x1").is_ok());
        assert!(validate_alias("").is_err());
        assert!(validate_alias("1orders").is_err());
        assert!(validate_alias("orders-db").is_err(), "横线要拼不出来就拒");
        assert!(validate_alias(" orders").is_err(), "首尾空白直接拒");
        for reserved in ["memory", "system", "rds_src", "Memory", "left", "order", "table"] {
            assert!(validate_alias(reserved).is_err(), "{reserved} 是保留名");
        }
    }

    #[test]
    fn a_file_source_gets_its_scheme_stripped_and_an_alias_checked() {
        let source = FederatedSource::new("c1", "orders", "duckdb", "duckdb://D:/x/y.duckdb")
            .expect("文件源");
        assert_eq!(source.connection_string, "D:/x/y.duckdb");
        assert!(source.attach_sql().contains("READ_ONLY"), "一律只读");
        assert!(source.attach_sql().contains("AS orders"));

        // 保留别名在组装这一层就被拦下（别等到 ATTACH 报语法错）
        assert!(FederatedSource::new("c1", "memory", "duckdb", "D:/x.duckdb").is_err());
        // 认不出的驱动如实说
        let err = FederatedSource::new("c1", "x", "clickhouse", "http://x").unwrap_err();
        assert!(err.contains("clickhouse"), "{err}");
    }

    /// 网络源：驱动 id（`mysql_native`）不能当 scheme，交给 DuckDB 前要换成扫描器那份
    #[test]
    fn a_network_source_gets_the_scanners_scheme() {
        let source = FederatedSource::new(
            "c1",
            "orders",
            "mysql_native",
            "mysql_native://root:pw@h:3306/db",
        )
        .expect("网络源");
        assert_eq!(source.connection_string, "mysql://root:pw@h:3306/db");
        assert!(source.attach_sql().contains("TYPE mysql"), "{}", source.attach_sql());
        assert!(source.attach_sql().contains("READ_ONLY"));

        let pg = FederatedSource::new(
            "c2",
            "wh",
            "postgres_native",
            "postgres_native://u:p@h:5432/w",
        )
        .expect("网络源");
        assert_eq!(pg.connection_string, "postgres://u:p@h:5432/w");
        assert!(pg.attach_sql().contains("TYPE postgres"));
    }

    /// L2（Oracle）：凭据拆成会话级 Secret，`ATTACH` 指 Secret 名、**不带 `READ_ONLY`**
    #[test]
    fn an_oracle_source_attaches_through_a_secret() {
        let source = FederatedSource::new(
            "ora1",
            "ora",
            "oracle",
            "oracle://devuser:Dev2026123@192.168.3.138:1521/XEPDB1",
        )
        .expect("L2 源");
        assert_eq!(source.kind.attach_type(), Some("oracle_scanner"));
        assert!(source.kind.needs_secret());
        // 连接串原样留着（只用来解析凭据 / 脱敏，不递进 ATTACH）
        assert_eq!(
            source.connection_string,
            "oracle://devuser:Dev2026123@192.168.3.138:1521/XEPDB1"
        );
        let secret = source.secret.as_ref().expect("该有 Secret");
        assert_eq!(secret.name, "rds_ora");
        assert!(secret.create_sql.contains("CREATE OR REPLACE SECRET rds_ora"), "{}", secret.create_sql);
        assert!(secret.create_sql.contains("TYPE ORACLE"));
        assert!(secret.create_sql.contains("HOST '192.168.3.138'"));
        assert!(secret.create_sql.contains("PORT 1521"));
        assert!(secret.create_sql.contains("USER 'devuser'"));
        assert!(secret.create_sql.contains("PASSWORD 'Dev2026123'"));
        assert!(secret.create_sql.contains("SERVICE_NAME 'XEPDB1'"));

        let attach = source.attach_sql();
        assert!(attach.contains("ATTACH 'rds_ora' AS ora"), "{attach}");
        assert!(attach.contains("TYPE oracle_scanner"), "{attach}");
        assert!(
            !attach.contains("READ_ONLY"),
            "Oracle 的扫描器不接受 READ_ONLY（真机台账）：{attach}"
        );

        // 凭据不能随 Debug 出去（日志 / 快照都可能打它）
        let shown = format!("{source:?}");
        assert!(!shown.contains("Dev2026123"), "{shown}");
        assert!(!format!("{secret:?}").contains("Dev2026123"));
    }

    /// Oracle 连接串的拆解：正常、缺件、端口非数字、百分号编码
    #[test]
    fn oracle_secret_parsing_refuses_incomplete_urls() {
        // 没写端口 → 1521（Oracle 的默认监听口）
        let default_port = oracle_secret_from_url("o", "oracle://u:p@h/XEPDB1").expect("默认端口");
        assert!(default_port.create_sql.contains("PORT 1521"), "{}", default_port.create_sql);

        // 百分号编码的凭据要解回原样（与 URL 组装侧对称）
        let encoded =
            oracle_secret_from_url("o", "oracle://de%76user:p%40ss@h:1521/XEPDB1").expect("编码凭据");
        assert!(encoded.create_sql.contains("USER 'devuser'"), "{}", encoded.create_sql);
        assert!(encoded.create_sql.contains("PASSWORD 'p@ss'"), "{}", encoded.create_sql);

        // 缺件要如实说（猜服务名会得到 ORA-01017，真机踩过）
        let no_service = oracle_secret_from_url("o", "oracle://u:p@h:1521").unwrap_err();
        assert!(no_service.contains("服务名"), "{no_service}");
        let empty_service = oracle_secret_from_url("o", "oracle://u:p@h:1521/").unwrap_err();
        assert!(empty_service.contains("服务名"), "{empty_service}");
        let no_host = oracle_secret_from_url("o", "oracle://u:p@:1521/XEPDB1").unwrap_err();
        assert!(no_host.contains("主机"), "{no_host}");
        let bad_port = oracle_secret_from_url("o", "oracle://u:p@h:abc/XEPDB1").unwrap_err();
        assert!(bad_port.contains("端口"), "{bad_port}");
        let no_credentials = oracle_secret_from_url("o", "oracle://h:1521/XEPDB1").unwrap_err();
        assert!(no_credentials.contains("凭据"), "{no_credentials}");
        let wrong_shape = oracle_secret_from_url("o", "192.168.3.138:1521/XEPDB1").unwrap_err();
        assert!(wrong_shape.contains("形状"), "{wrong_shape}");
    }

    /// L2 不能做本地加速（`AccelSource` 走的是另一条路，这里把边界钉在 `kind` 上）
    #[test]
    fn oracle_is_l2_only() {
        let source = FederatedSource::new("c", "ora", "oracle", "oracle://u:p@h:1521/S")
            .expect("联邦源可以");
        assert!(!source.kind.supports_read_only_attach());
        let err = crate::duckdb::accel::AccelSource::new("c", "oracle", "oracle://u:p@h:1521/S")
            .expect_err("本地加速该拒");
        assert!(err.contains("联邦源"), "{err}");
    }
}
