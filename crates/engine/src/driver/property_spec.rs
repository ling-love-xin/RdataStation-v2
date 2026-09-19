//! 驱动属性规格：`driver_properties` 里那个键**到底会去哪**。
//!
//! ## 为什么要有这份规格
//!
//! 连接行与驱动行里的 `driver_properties` 在建连时被**原样追加到连接串**
//! （`DriverConnectionConfig::append_query_params`），而这条串最终由**客户端库**解析
//! （源码依据见 `docs/architecture/driver-capability-matrix.md` §2.1）。同一个键在不同实现上命运不同：
//!
//! | 客户端库（驱动） | 未知参数的行为 |
//! | --- | --- |
//! | sqlx 0.9（`mysql` / `postgres`） | **静默忽略**（PG 打一条 `ignoring unrecognized connect parameter`） |
//! | mysql_async 0.37（`mysql_native`） | **报错** `UrlError::UnknownParameter` |
//! | tokio-postgres 0.7（`postgres_native`） | **报错** `UnknownOption` |
//! | rusqlite / duckdb-rs（`sqlite` / `duckdb`） | 查询串被剥掉 → URL 上不下发；改由本仓驱动侧应用（PRAGMA / SET），**清单外的键不应用**（记 warn） |
//!
//! 于是「属性页写了不生效」只是最轻的一种命运，重的是**连接直接失败**。本模块把命运
//! 固定成一处（库认的键清单 + 中文标签 + 未知键的效果），供三处消费：
//! 界面（属性行下方的「去向」提示）、声明自检（`declaration` 的测试：声明里写的键必须在清单内）、
//! 以及驱动侧落实（文件型驱动按这里的键名逐条应用：SQLite PRAGMA / DuckDB SET）。
//!
//! ## 两档路线
//!
//! - [`Route::Url`]：写进连接串，由客户端库自己解析（网络型驱动）；
//! - [`Route::DriverSide`]：由**本仓驱动**在开库时应用（文件型的 PRAGMA / SET）——
//!   URL 上不出现，实现分别在 `driver/native/{sqlite,duckdb}.rs`。
//!
//! ## 不做的事（有意）
//!
//! - **不在下发路径上过滤**：用户写的键照原样进连接串——静默丢弃用户的配置比报错更糟
//!   （本仓的规矩是「做不到就报错，不静默降级」）。本模块只回答「它会怎样」，由界面说清楚。
//! - **不把“未知键”当 PRAGMA 名去试**：文件型驱动只应用清单里的键，其余键**记 warn 且不执行**
//!   （用户键值直接拼 SQL 既危险又会把只读型 PRAGMA 当配置用）。

/// 键不在接受清单里时的实际行为（依据见模块头表格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownEffect {
    /// sqlx 静默忽略；文件型驱动侧不应用（记 warn）——两种都是“写了不生效、不报错”。
    Ignored,
    /// `mysql_async` / `tokio-postgres`：报错（连接失败）。
    ConnectionError,
}

/// 一个键靠什么落实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// 写进连接串，由客户端库解析（网络型驱动）。
    Url,
    /// 由本仓驱动在开库时应用（文件型的 PRAGMA / SET）——URL 上不出现。
    DriverSide,
}

/// 一个键在某个驱动上的去向（界面按此如实标注）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// 会写进连接串。`param` 是客户端库认的参数名（可能与用户写的键不同），
    /// `label` / `note` / `caution` 是给界面看的中文说明（可能没有）。
    Delivered {
        param: String,
        label: Option<&'static str>,
        note: Option<&'static str>,
        caution: Option<&'static str>,
    },
    /// 由本仓驱动侧应用（文件型的 PRAGMA / SET）。`applied_as` 是实际应用的名字。
    DriverSide {
        applied_as: &'static str,
        label: Option<&'static str>,
        note: Option<&'static str>,
        caution: Option<&'static str>,
    },
    /// 库不认这个键（后果由 `effect` 给）。
    Unknown { effect: UnknownEffect },
    /// 该驱动不在本模块收录范围内（未来的插件驱动）：去向未知，按原样下发。
    Unclassified,
}

impl Verdict {
    /// 是否「会真的下发」（界面用一句话概括）。
    pub fn is_delivered(&self) -> bool {
        matches!(self, Verdict::Delivered { .. })
    }
}

/// 一个（有中文标签的）属性键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertySpec {
    /// 键（用户在属性页里写的名字）。
    pub key: &'static str,
    /// 中文标签（界面提示用）。
    pub label: &'static str,
    /// 等价写法（旧种子的驼峰名 / 客户端库的另一种写法）：判定时与 `key` 等价，
    /// 界面会提示「下发为 `key`」。
    pub aliases: &'static [&'static str],
    /// 靠什么落实。
    pub route: Route,
    /// 下发 / 应用时用的名字；`None` = 与 `key` 同名。
    pub as_name: Option<&'static str>,
    /// 额外说明（中性：解释默认值 / 等价写法 / 平台限制）。
    pub note: Option<&'static str>,
    /// 需要注意的副作用（如「会覆盖「连接安全」里的档位」）——界面按警告色标注。
    pub caution: Option<&'static str>,
}

/// 一个驱动的规格：库认的键清单 + 未知键的效果 +（可选）「属性根本不下发」的原因。
struct DriverSpec {
    /// 驱动 id（`drivers.id`）——**不能用数据库族 id**：解析连接串的是具体客户端库
    /// （`mysql` 与 `mysql_native` 同一族，词表完全不同）。
    driver: &'static str,
    /// 未知键的效果。
    unknown: UnknownEffect,
    /// 客户端库认的**全部**参数名（含没有中文标签的；改依赖版本时按模块头引用的源码复查）。
    params: &'static [&'static str],
    /// 有中文标签 / 提醒的键（必须是 `params` 的子集，有测试盯住）。
    curated: &'static [PropertySpec],
}

/// 各驱动的规格（依据：模块头引用的依赖源码 + 能力矩阵 §2.1）。
const DRIVERS: [DriverSpec; 6] = [
    DriverSpec {
        driver: "mysql",
        unknown: UnknownEffect::Ignored,
        // `sqlx-mysql-0.9.0/src/options/parse.rs:50-79`（`_ => {}` 分支 = 静默忽略）
        params: &[
            "sslmode",
            "ssl-mode",
            "sslca",
            "ssl-ca",
            "sslcert",
            "ssl-cert",
            "sslkey",
            "ssl-key",
            "charset",
            "collation",
            "statement-cache-capacity",
            "socket",
            "timezone",
            "time-zone",
        ],
        curated: &[
            PropertySpec {
                key: "charset",
                label: "字符集",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("sqlx 默认已是 utf8mb4"),
                caution: None,
            },
            PropertySpec {
                key: "collation",
                label: "排序规则",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "timezone",
                label: "连接时区",
                aliases: &["time-zone"],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "socket",
                label: "Unix socket 路径",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("Windows 上无效"),
                caution: None,
            },
            PropertySpec {
                key: "statement-cache-capacity",
                label: "语句缓存条数",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("sqlx 默认 100"),
                caution: None,
            },
            PropertySpec {
                key: "ssl-mode",
                label: "TLS 模式",
                aliases: &["sslmode"],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
        ],
    },
    DriverSpec {
        driver: "postgres",
        unknown: UnknownEffect::Ignored,
        // `sqlx-postgres-0.9.0/src/options/parse.rs:52-100`（未知键 warn 后忽略）
        params: &[
            "sslmode",
            "ssl-mode",
            "sslrootcert",
            "ssl-root-cert",
            "ssl-ca",
            "sslcert",
            "ssl-cert",
            "sslkey",
            "ssl-key",
            "statement-cache-capacity",
            "host",
            "hostaddr",
            "port",
            "dbname",
            "user",
            "password",
            "application_name",
            "options",
        ],
        curated: &[
            PropertySpec {
                key: "application_name",
                label: "应用名",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("PG 服务端会话标识（pg_stat_activity 里可见）"),
                caution: None,
            },
            PropertySpec {
                key: "options",
                label: "服务端命令行选项",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "statement-cache-capacity",
                label: "语句缓存条数",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("sqlx 默认 100"),
                caution: None,
            },
            PropertySpec {
                key: "sslmode",
                label: "TLS 模式",
                aliases: &["ssl-mode"],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
            PropertySpec {
                key: "host",
                label: "主机（覆盖连接串）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖连接串里的地址，谨慎使用"),
            },
            PropertySpec {
                key: "port",
                label: "端口（覆盖连接串）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖连接串里的端口，谨慎使用"),
            },
            PropertySpec {
                key: "dbname",
                label: "数据库（覆盖连接串）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖连接串里的数据库，谨慎使用"),
            },
        ],
    },
    DriverSpec {
        driver: "mysql_native",
        unknown: UnknownEffect::ConnectionError,
        // `mysql_async-0.37.1/src/opts/mod.rs:1782-2048`（末尾 `return Err(UnknownParameter)`）
        params: &[
            "pool_min",
            "pool_max",
            "inactive_connection_ttl",
            "ttl_check_interval",
            "conn_ttl",
            "abs_conn_ttl",
            "abs_conn_ttl_jitter",
            "tcp_keepalive",
            "max_allowed_packet",
            "wait_timeout",
            "enable_cleartext_plugin",
            "reset_connection",
            "tcp_nodelay",
            "stmt_cache_size",
            "prefer_socket",
            "secure_auth",
            "client_found_rows",
            "socket",
            "compression",
            "require_ssl",
            "verify_ca",
            "verify_identity",
            "built_in_roots",
        ],
        curated: &[
            PropertySpec {
                key: "max_allowed_packet",
                label: "客户端最大包（字节）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "wait_timeout",
                label: "服务端空闲超时（秒）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "tcp_keepalive",
                label: "TCP keepalive（毫秒）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "tcp_nodelay",
                label: "禁用 Nagle（true/false）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "compression",
                label: "协议压缩",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("on/fast/best 或 0-9"),
                caution: None,
            },
            PropertySpec {
                key: "stmt_cache_size",
                label: "语句缓存条数",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "prefer_socket",
                label: "优先 unix socket",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("工厂建连时默认写 false（避免走 localhost 的 socket）"),
                caution: None,
            },
            PropertySpec {
                key: "pool_min",
                label: "连接池下限",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "pool_max",
                label: "连接池上限",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "require_ssl",
                label: "要求加密",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
            PropertySpec {
                key: "verify_ca",
                label: "校验证书链",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
            PropertySpec {
                key: "verify_identity",
                label: "校验主机名",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
        ],
    },
    DriverSpec {
        driver: "postgres_native",
        unknown: UnknownEffect::ConnectionError,
        // `tokio-postgres-0.7.17/src/config.rs:560-720`（末尾 `Err(UnknownOption)`）
        params: &[
            "user",
            "password",
            "dbname",
            "options",
            "application_name",
            "sslmode",
            "host",
            "sslnegotiation",
            "hostaddr",
            "port",
            "connect_timeout",
            "tcp_user_timeout",
            "keepalives",
            "keepalives_idle",
            "keepalives_interval",
            "keepalives_retries",
            "target_session_attrs",
            "channel_binding",
            "load_balance_hosts",
        ],
        curated: &[
            PropertySpec {
                key: "application_name",
                label: "应用名",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("PG 服务端会话标识（pg_stat_activity 里可见）"),
                caution: None,
            },
            PropertySpec {
                key: "connect_timeout",
                label: "连接超时（秒）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "tcp_user_timeout",
                label: "TCP 未确认超时（秒）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "keepalives",
                label: "启用 TCP keepalive（0/1）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "keepalives_idle",
                label: "keepalive 空闲（秒）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "target_session_attrs",
                label: "会话要求",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("read-write = 只要主库"),
                caution: None,
            },
            PropertySpec {
                key: "channel_binding",
                label: "通道绑定",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: Some("disable/prefer/require"),
                caution: None,
            },
            PropertySpec {
                key: "options",
                label: "服务端命令行选项",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "sslmode",
                label: "TLS 模式",
                aliases: &["ssl-mode"],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
            PropertySpec {
                key: "host",
                label: "主机（覆盖连接串）",
                aliases: &[],
                route: Route::Url,
                as_name: None,
                note: None,
                caution: Some("会覆盖连接串里的地址，谨慎使用"),
            },
        ],
    },
    DriverSpec {
        driver: "sqlite",
        unknown: UnknownEffect::Ignored,
        // URL 上没有参数：文件型驱动的地址由工厂从连接串直接取，查询串会被剥掉。
        // 属性靠**驱动侧**应用（`native/sqlite.rs` 的 PRAGMA 规划器）。
        params: &[],
        curated: &[
            PropertySpec {
                key: "journal_mode",
                label: "日志模式",
                aliases: &["journalMode"],
                route: Route::DriverSide,
                as_name: None,
                note: Some("WAL/DELETE/TRUNCATE/PERSIST/MEMORY/OFF；:memory: 库不支持 WAL"),
                caution: None,
            },
            PropertySpec {
                key: "synchronous",
                label: "同步级别",
                aliases: &[],
                route: Route::DriverSide,
                as_name: None,
                note: Some("OFF/NORMAL/FULL/EXTRA；WAL 下常用 NORMAL"),
                caution: None,
            },
            PropertySpec {
                key: "busy_timeout",
                label: "忙等超时（毫秒）",
                aliases: &["busyTimeout"],
                route: Route::DriverSide,
                as_name: None,
                note: Some("并发写时等待锁的时间（默认 0 = 立即报 database is locked）"),
                caution: None,
            },
            PropertySpec {
                key: "foreign_keys",
                label: "外键约束",
                aliases: &["foreignKeys"],
                route: Route::DriverSide,
                as_name: None,
                note: None,
                caution: Some("SQLite 默认关；开启后既有的违规写入会开始报错"),
            },
            PropertySpec {
                key: "cache_size",
                label: "页缓存（负数 = KB）",
                aliases: &["cacheSize"],
                route: Route::DriverSide,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "temp_store",
                label: "临时表位置",
                aliases: &["tempStore"],
                route: Route::DriverSide,
                as_name: None,
                note: Some("DEFAULT/FILE/MEMORY"),
                caution: None,
            },
            PropertySpec {
                key: "mode",
                label: "打开模式",
                aliases: &[],
                route: Route::DriverSide,
                as_name: None,
                note: Some("ro/rw/rwc（开库时定，之后改不了）"),
                caution: Some("ro 会拒绝一切写入（含 PRAGMA 写），连表都建不了"),
            },
        ],
    },
    DriverSpec {
        driver: "duckdb",
        unknown: UnknownEffect::Ignored,
        // URL 同上；属性靠驱动侧应用（`native/duckdb.rs` 的 SET 规划器）。
        params: &[],
        curated: &[
            PropertySpec {
                key: "access_mode",
                label: "打开模式",
                aliases: &["accessMode"],
                route: Route::DriverSide,
                as_name: None,
                note: Some("automatic/read_only/read_write（开库时定，之后改不了）"),
                caution: None,
            },
            PropertySpec {
                key: "threads",
                label: "线程数",
                aliases: &[],
                route: Route::DriverSide,
                as_name: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "memory_limit",
                label: "内存上限",
                aliases: &["memoryLimit"],
                route: Route::DriverSide,
                as_name: None,
                note: Some("如 1GB / 512MB"),
                caution: Some("覆盖应用默认的内存闸（`duckdb::manager`）"),
            },
            PropertySpec {
                key: "temp_directory",
                label: "溢写目录",
                aliases: &["tempDirectory"],
                route: Route::DriverSide,
                as_name: None,
                note: None,
                caution: Some("覆盖应用默认的溢写目录（仓库内 .rds/tmp）"),
            },
            PropertySpec {
                key: "max_temp_directory_size",
                label: "溢写上限",
                aliases: &[],
                route: Route::DriverSide,
                as_name: None,
                note: Some("如 10GB"),
                caution: None,
            },
            PropertySpec {
                key: "preserve_insertion_order",
                label: "保持插入顺序",
                aliases: &["preserveInsertionOrder"],
                route: Route::DriverSide,
                as_name: None,
                note: Some("true/false"),
                caution: None,
            },
        ],
    },
];

/// 查驱动的规格（未收录 → `None`，可能是将来接进来的插件驱动）。
fn spec_of(driver: &str) -> Option<&'static DriverSpec> {
    DRIVERS.iter().find(|d| d.driver == driver)
}

/// 这个键（含别名）在该驱动上有没有标签条目。
fn curated_of(d: &DriverSpec, key: &str) -> Option<&'static PropertySpec> {
    d.curated
        .iter()
        .find(|c| c.key == key || c.aliases.contains(&key))
}

/// 驱动里**有中文标签**的键（界面提示「常用键」用；未收录的驱动 → 空）。
///
/// 只列规范写法：别名（旧驼峰名 / 客户端库的另一种写法）在条目的 `aliases` 里，不另占一行。
pub fn known_keys(driver: &str) -> &'static [PropertySpec] {
    spec_of(driver).map(|d| d.curated).unwrap_or(&[])
}

/// 客户端库 / 驱动侧是否认这个键（`declaration` 的自检用它盯住「声明的默认属性必须落在清单内」）。
///
/// 未收录的驱动 → `false`（自检会因此报错，提醒把新驱动补进 [`DRIVERS`]）。
pub fn accepts(driver: &str, key: &str) -> bool {
    let key = key.trim();
    spec_of(driver)
        .map(|d| d.params.contains(&key) || curated_of(d, key).is_some())
        .unwrap_or(false)
}

/// 这个键在该驱动上的去向（界面唯一判据）。
pub fn verdict(driver: &str, key: &str) -> Verdict {
    let key = key.trim();
    if key.is_empty() {
        return Verdict::Unclassified;
    }
    let Some(d) = spec_of(driver) else {
        return Verdict::Unclassified;
    };

    if let Some(c) = curated_of(d, key) {
        let applies_as = c.as_name.unwrap_or(c.key);
        return match c.route {
            Route::Url => Verdict::Delivered {
                param: applies_as.to_string(),
                label: Some(c.label),
                note: c.note,
                caution: c.caution,
            },
            Route::DriverSide => Verdict::DriverSide {
                applied_as: applies_as,
                label: Some(c.label),
                note: c.note,
                caution: c.caution,
            },
        };
    }
    if d.params.contains(&key) {
        return Verdict::Delivered {
            param: key.to_string(),
            label: None,
            note: None,
            caution: None,
        };
    }
    // 既不在 URL 参数里、也没在驱动侧落实（文件型的不认键走这里）
    Verdict::Unknown { effect: d.unknown }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin_drivers() -> Vec<String> {
        crate::driver::AutoDriverRegistrar::register_builtin_drivers();
        let mut ids = crate::driver::DriverRegistry::all_driver_ids();
        ids.sort();
        ids
    }

    /// 每个内置驱动都要有规格：新增驱动忘了表态时在这里红，而不是等用户连不上才发觉。
    #[test]
    fn every_builtin_driver_is_covered() {
        let ids = builtin_drivers();
        assert!(ids.len() >= 6, "内置驱动应至少 6 个：{ids:?}");
        for id in &ids {
            assert!(
                spec_of(id).is_some(),
                "驱动 {id} 没有属性规格（往 property_spec::DRIVERS 补一条）"
            );
        }
    }

    /// 标签键与它们的别名都得有出处：
    /// - `Route::Url` 的键必须在库的接受清单里；
    /// - `Route::DriverSide` 的键由本仓驱动落实（该驱动必须不是「不解析属性」那一类）。
    #[test]
    fn curated_keys_have_a_known_home() {
        for d in &DRIVERS {
            for c in d.curated {
                match c.route {
                    Route::Url => {
                        assert!(
                            d.params.contains(&c.key),
                            "{}：标签键 {} 不在库的接受清单里（清单：{:?}）",
                            d.driver,
                            c.key,
                            d.params
                        );
                        if let Some(name) = c.as_name {
                            assert!(
                                d.params.contains(&name),
                                "{}：{} 的下发参数名 {} 不在清单里",
                                d.driver,
                                c.key,
                                name
                            );
                        }
                    }
                    Route::DriverSide => assert!(
                        d.params.is_empty(),
                        "{}：驱动侧落实的键不该同时声明 URL 参数 {:?}",
                        d.driver,
                        d.params
                    ),
                }
            }
        }
    }

    /// 别名（旧驼峰名 / 客户端库的另一种写法）不能与规范名或别的键撞车，
    /// 否则属性页两行等价、或一个键有两个去向。
    #[test]
    fn aliases_do_not_collide() {
        for d in &DRIVERS {
            for c in d.curated {
                for alias in c.aliases {
                    assert_ne!(alias, &c.key, "{}：{} 的别名与规范名相同", d.driver, c.key);
                    let hits = d
                        .curated
                        .iter()
                        .filter(|other| other.key == *alias || other.aliases.contains(alias))
                        .count();
                    assert_eq!(
                        hits, 1,
                        "{}：别名 {alias} 命中 {hits} 个条目（应恰好 1 个）",
                        d.driver
                    );
                    assert!(
                        accepts(d.driver, alias),
                        "{}：别名 {alias} 应被判为可接受",
                        d.driver
                    );
                }
            }
        }
    }

    /// 四种命运各有代表（否则「如实标注」就无从谈起）。
    #[test]
    fn each_driver_reports_its_real_unknown_key_effect() {
        // sqlx：静默忽略
        assert_eq!(
            verdict("mysql", "connectTimeout"),
            Verdict::Unknown {
                effect: UnknownEffect::Ignored
            }
        );
        assert_eq!(
            verdict("postgres", "applicationName"), // 旧种子里的驼峰写法
            Verdict::Unknown {
                effect: UnknownEffect::Ignored
            }
        );
        // native：直接报错
        assert_eq!(
            verdict("mysql_native", "connectTimeout"),
            Verdict::Unknown {
                effect: UnknownEffect::ConnectionError
            }
        );
        assert_eq!(
            verdict("postgres_native", "ssl_mode"), // 对话框旧初值就是这个名字
            Verdict::Unknown {
                effect: UnknownEffect::ConnectionError
            }
        );
        // 文件型：不该出现在 URL 参数里，但**头部键由驱动侧落实**（PRAGMA / SET）
        assert_eq!(
            verdict("sqlite", "journalMode").clone(),
            Verdict::DriverSide {
                applied_as: "journal_mode",
                label: Some("日志模式"),
                note: Some("WAL/DELETE/TRUNCATE/PERSIST/MEMORY/OFF；:memory: 库不支持 WAL"),
                caution: None,
            }
        );
        assert!(matches!(
            verdict("duckdb", "memoryLimit"),
            Verdict::DriverSide { .. }
        ));
        // 不在清单里的键：驱动侧不执行（记 warn），界面也不能说它会生效
        assert_eq!(
            verdict("sqlite", "magic_setting"),
            Verdict::Unknown {
                effect: UnknownEffect::Ignored
            }
        );
    }

    /// 库认的键一律判「会下发」，并带上标签 / 提醒。
    #[test]
    fn accepted_keys_are_delivered() {
        for d in &DRIVERS {
            for p in d.params {
                let v = verdict(d.driver, p);
                assert!(
                    v.is_delivered(),
                    "{}：库认的参数 {p} 却被判成 {v:?}",
                    d.driver
                );
            }
        }

        // 标签与提醒：声明里真正写的那两个键
        match verdict("postgres_native", "application_name") {
            Verdict::Delivered { label, note, .. } => {
                assert_eq!(label, Some("应用名"));
                assert!(note.is_some(), "PG 应用名应解释它对服务端可见");
            }
            other => panic!("application_name 应判 Delivered：{other:?}"),
        }
        match verdict("mysql_native", "max_allowed_packet") {
            Verdict::Delivered { param, .. } => assert_eq!(param, "max_allowed_packet"),
            other => panic!("max_allowed_packet 应判 Delivered：{other:?}"),
        }
        // 需要注意的副作用与中性说明分开（界面按不同颜色标注）
        match verdict("mysql_native", "require_ssl") {
            Verdict::Delivered { note, caution, .. } => {
                assert!(note.is_none(), "会覆盖别的设置属于 caution 而非 note");
                assert!(caution.is_some_and(|c| c.contains("连接安全")));
            }
            other => panic!("require_ssl 应判 Delivered：{other:?}"),
        }
        match verdict("mysql", "charset") {
            Verdict::Delivered { note, caution, .. } => {
                assert!(note.is_some(), "charset 应解释「sqlx 默认已是 utf8mb4」");
                assert!(caution.is_none());
            }
            other => panic!("charset 应判 Delivered：{other:?}"),
        }
    }

    /// 空键 / 未收录的驱动不装作知道。
    #[test]
    fn unknown_driver_and_blank_key_are_unclassified() {
        assert_eq!(verdict("mysql", "   "), Verdict::Unclassified);
        assert_eq!(verdict("some_plugin_driver", "charset"), Verdict::Unclassified);
        assert!(!accepts("some_plugin_driver", "charset"));
        assert!(accepts("mysql", "charset"));
    }

    /// 「常用键」提示要能给出中文标签（属性页靠它解释这个驱动能配什么）。
    #[test]
    fn known_keys_carry_labels() {
        let keys = known_keys("mysql_native");
        assert!(keys.len() >= 6, "已声明默认属性的驱动应给足常用键");
        assert!(keys.iter().any(|k| k.key == "max_allowed_packet"));
        for k in keys {
            assert!(!k.label.trim().is_empty(), "{} 缺中文标签", k.key);
        }
        // 文件型也有常用键（驱动侧落实的 PRAGMA / SET）
        let sqlite = known_keys("sqlite");
        assert!(sqlite.iter().any(|k| k.key == "journal_mode"));
        assert!(sqlite.iter().any(|k| k.key == "busy_timeout"));
        let duck = known_keys("duckdb");
        assert!(duck.iter().any(|k| k.key == "access_mode"));
        assert!(duck.iter().any(|k| k.key == "memory_limit"));

        assert!(known_keys("some_plugin_driver").is_empty());
    }
}
