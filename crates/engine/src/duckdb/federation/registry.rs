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

/// 一个联邦源（**宿主组装**：引擎不读连接库、不解密口令）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedSource {
    /// 源连接 id（结果集 / 历史 / 界面都拿它对上号）
    pub conn_id: String,
    /// SQL 里的别名（跨源查询写 `<别名>.<schema>.<表>`）
    pub alias: String,
    pub kind: AccelKind,
    /// 网络型 = 带凭据的 URL；文件型 = **裸路径**（[`FederatedSource::new`] 会剥 scheme）
    pub connection_string: String,
}

impl FederatedSource {
    /// 从宿主的连接信息组装
    ///
    /// 文件型的 URL 可能是 `sqlite://D:\x.db` 这种带 scheme 的写法，而 DuckDB 要裸路径——
    /// 与加速档共用同一条剥 scheme 规则（[`normalize_file_path`]），不各写一份。
    pub fn new(conn_id: &str, alias: &str, db_type: &str, url: &str) -> Result<Self, String> {
        // 认不出驱动就说出来（不猜：猜错会把语句发到不相干的库上）
        let kind = AccelKind::from_db_type(db_type)
            .map_err(|_| format!("这个驱动（{db_type}）暂时不能做联邦源"))?;
        validate_alias(alias)?;
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
        })
    }

    /// 挂载语句：**一律只读**（写语义未定义就不开放；本地临时对象不受影响）
    pub fn attach_sql(&self) -> String {
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
    use super::{FederatedSource, sanitize_alias, unique_alias, validate_alias};

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
}
