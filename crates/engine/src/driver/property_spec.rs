//! 驱动属性规格：`driver_properties` 里那个键**到底会去哪**。
//!
//! ## 为什么要有这份规格
//!
//! 连接行与驱动行里的 `driver_properties` 在建连时被**原样追加到连接串**
//! （`DriverConnectionConfig::append_query_params`），而这条串最终由**客户端库**解析
//! （源码依据见 `docs/architecture/driver-capability-matrix.md` §2.1）。同一个键在四个实现上
//! 有四种命运：
//!
//! | 客户端库（驱动） | 未知参数的行为 |
//! | --- | --- |
//! | sqlx 0.9（`mysql` / `postgres`） | **静默忽略**（PG 打一条 `ignoring unrecognized connect parameter`） |
//! | mysql_async 0.37（`mysql_native`） | **报错** `UrlError::UnknownParameter` |
//! | tokio-postgres 0.7（`postgres_native`） | **报错** `UnknownOption` |
//! | rusqlite / duckdb-rs（`sqlite` / `duckdb`） | 查询串被工厂剥掉 → **全部不下发** |
//!
//! 于是「属性页写了不生效」只是四种命运里最轻的一种，重的是**连接直接失败**。本模块把命运
//! 固定成一处（库认的键清单 + 中文标签 + 未知键的效果），供三处消费：
//! 界面（属性行下方的「去向」提示）、声明自检（`declaration` 的测试：声明里写的键必须在清单内）、
//! 以及以后真要加「由驱动侧落实」的键时（见下）。
//!
//! ## 不做的事（有意）
//!
//! - **不在下发路径上过滤**：用户写的键照原样进连接串——静默丢弃用户的配置比报错更糟
//!   （本仓的规矩是「做不到就报错，不静默降级」）。本模块只回答「它会怎样」，由界面说清楚。
//! - **不提供 `Builder`（驱动侧落实）这一档**：文件型驱动的 PRAGMA / 会话参数目前**一个都不下发**，
//!   真要做需要先在各驱动里实现「打开连接后应用属性」，属于新能力而非规格；届时在这里加一档
//!   `DriverSide`，而不是先摆一个没人走的分支（§5「空壳与死路」）。

/// 键不在库的接受清单里时，客户端库的实际行为（源码依据见模块头表格）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownEffect {
    /// 静默忽略（sqlx）：写了不生效，不报错
    SilentlyIgnored,
    /// 直接报错（mysql_async / tokio-postgres）：连接失败
    ConnectionError,
    /// 不下发（文件型：查询串被剥掉）
    NotDelivered,
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
    /// 当前实现不会用它（文件型驱动不解析属性）。
    Unsupported {
        label: Option<&'static str>,
        reason: &'static str,
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
    /// 下发时的参数名；`None` = 与 `key` 同名。
    pub param: Option<&'static str>,
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
    /// 当前实现不解析属性（文件型）时的原因；`Some` 时所有键都判 `Unsupported`。
    not_consumed: Option<&'static str>,
}

/// 各驱动的规格（依据：模块头引用的依赖源码 + 能力矩阵 §2.1）。
const DRIVERS: [DriverSpec; 6] = [
    DriverSpec {
        driver: "mysql",
        unknown: UnknownEffect::SilentlyIgnored,
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
                param: None,
                note: Some("sqlx 默认已是 utf8mb4"),
                caution: None,
            },
            PropertySpec {
                key: "collation",
                label: "排序规则",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "timezone",
                label: "连接时区",
                param: None,
                note: Some("等价写法 time-zone 也认"),
                caution: None,
            },
            PropertySpec {
                key: "socket",
                label: "Unix socket 路径",
                param: None,
                note: Some("Windows 上无效"),
                caution: None,
            },
            PropertySpec {
                key: "statement-cache-capacity",
                label: "语句缓存条数",
                param: None,
                note: Some("sqlx 默认 100"),
                caution: None,
            },
            PropertySpec {
                key: "ssl-mode",
                label: "TLS 模式",
                param: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
        ],
        not_consumed: None,
    },
    DriverSpec {
        driver: "postgres",
        unknown: UnknownEffect::SilentlyIgnored,
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
                param: None,
                note: Some("PG 服务端会话标识（pg_stat_activity 里可见）"),
                caution: None,
            },
            PropertySpec {
                key: "options",
                label: "服务端命令行选项",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "statement-cache-capacity",
                label: "语句缓存条数",
                param: None,
                note: Some("sqlx 默认 100"),
                caution: None,
            },
            PropertySpec {
                key: "sslmode",
                label: "TLS 模式",
                param: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
            PropertySpec {
                key: "host",
                label: "主机（覆盖连接串）",
                param: None,
                note: None,
                caution: Some("会覆盖连接串里的地址，谨慎使用"),
            },
            PropertySpec {
                key: "port",
                label: "端口（覆盖连接串）",
                param: None,
                note: None,
                caution: Some("会覆盖连接串里的端口，谨慎使用"),
            },
            PropertySpec {
                key: "dbname",
                label: "数据库（覆盖连接串）",
                param: None,
                note: None,
                caution: Some("会覆盖连接串里的数据库，谨慎使用"),
            },
        ],
        not_consumed: None,
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
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "wait_timeout",
                label: "服务端空闲超时（秒）",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "tcp_keepalive",
                label: "TCP keepalive（毫秒）",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "tcp_nodelay",
                label: "禁用 Nagle（true/false）",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "compression",
                label: "协议压缩",
                param: None,
                note: Some("on/fast/best 或 0-9"),
                caution: None,
            },
            PropertySpec {
                key: "stmt_cache_size",
                label: "语句缓存条数",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "prefer_socket",
                label: "优先 unix socket",
                param: None,
                note: Some("工厂建连时默认写 false（避免走 localhost 的 socket）"),
                caution: None,
            },
            PropertySpec {
                key: "pool_min",
                label: "连接池下限",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "pool_max",
                label: "连接池上限",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "require_ssl",
                label: "要求加密",
                param: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
            PropertySpec {
                key: "verify_ca",
                label: "校验证书链",
                param: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
            PropertySpec {
                key: "verify_identity",
                label: "校验主机名",
                param: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
        ],
        not_consumed: None,
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
                param: None,
                note: Some("PG 服务端会话标识（pg_stat_activity 里可见）"),
                caution: None,
            },
            PropertySpec {
                key: "connect_timeout",
                label: "连接超时（秒）",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "tcp_user_timeout",
                label: "TCP 未确认超时（秒）",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "keepalives",
                label: "启用 TCP keepalive（0/1）",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "keepalives_idle",
                label: "keepalive 空闲（秒）",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "target_session_attrs",
                label: "会话要求",
                param: None,
                note: Some("read-write = 只要主库"),
                caution: None,
            },
            PropertySpec {
                key: "channel_binding",
                label: "通道绑定",
                param: None,
                note: Some("disable/prefer/require"),
                caution: None,
            },
            PropertySpec {
                key: "options",
                label: "服务端命令行选项",
                param: None,
                note: None,
                caution: None,
            },
            PropertySpec {
                key: "sslmode",
                label: "TLS 模式",
                param: None,
                note: None,
                caution: Some("会覆盖「连接安全」里的档位；建议在那里配"),
            },
            PropertySpec {
                key: "host",
                label: "主机（覆盖连接串）",
                param: None,
                note: None,
                caution: Some("会覆盖连接串里的地址，谨慎使用"),
            },
        ],
        not_consumed: None,
    },
    DriverSpec {
        driver: "sqlite",
        unknown: UnknownEffect::NotDelivered,
        // 文件型：属性不会下发（查询串被工厂剥掉，见 `driver/factory.rs::sqlite_path_from_config`）
        params: &[],
        curated: &[],
        not_consumed: Some(
            "文件型驱动的地址由工厂从连接串直接取（查询串会被剥掉），属性不会下发；\
             PRAGMA 型设置（journalMode / busyTimeout…）当前实现不应用",
        ),
    },
    DriverSpec {
        driver: "duckdb",
        unknown: UnknownEffect::NotDelivered,
        // 同上（`factory.rs` 的 duckdb 分支）
        params: &[],
        curated: &[],
        not_consumed: Some(
            "文件型驱动的地址由工厂从连接串直接取（查询串会被剥掉），属性不会下发；\
             会话级设置由 `duckdb::manager` 统一钉（memory_limit / temp_directory…）",
        ),
    },
];

/// 查驱动的规格（未收录 → `None`，可能是将来接进来的插件驱动）。
fn spec_of(driver: &str) -> Option<&'static DriverSpec> {
    DRIVERS.iter().find(|d| d.driver == driver)
}

/// 驱动里**有中文标签**的键（界面提示「常用键」用；未收录的驱动 → 空）。
pub fn known_keys(driver: &str) -> &'static [PropertySpec] {
    spec_of(driver).map(|d| d.curated).unwrap_or(&[])
}

/// 客户端库是否认这个键（`declaration` 的自检用它盯住「声明的默认属性必须落在清单内」）。
///
/// 未收录的驱动 → `false`（自检会因此报错，提醒把新驱动补进 [`DRIVERS`]）。
pub fn accepts(driver: &str, key: &str) -> bool {
    let key = key.trim();
    spec_of(driver)
        .map(|d| d.params.contains(&key) || d.curated.iter().any(|c| c.key == key))
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

    let curated = d.curated.iter().find(|c| c.key == key);
    let label = curated.map(|c| c.label);

    // 当前实现压根不解析属性（文件型）：无论键是什么都不下发。
    if let Some(reason) = d.not_consumed {
        return Verdict::Unsupported { label, reason };
    }

    if let Some(c) = curated {
        return Verdict::Delivered {
            param: c.param.unwrap_or(c.key).to_string(),
            label: Some(c.label),
            note: c.note,
            caution: c.caution,
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

    /// 有中文标签的键必须是「库认的键」的子集——标签写着能用、实际被库拒下是最糟的组合。
    #[test]
    fn curated_keys_are_accepted_by_the_library() {
        for d in &DRIVERS {
            if d.not_consumed.is_some() {
                assert!(
                    d.curated.is_empty(),
                    "{}：不下发属性的驱动不该有「常用键」标签",
                    d.driver
                );
                continue;
            }
            for c in d.curated {
                assert!(
                    d.params.contains(&c.key),
                    "{}：标签键 {} 不在库的接受清单里（清单：{:?}）",
                    d.driver,
                    c.key,
                    d.params
                );
                if let Some(param) = c.param {
                    assert!(
                        d.params.contains(&param),
                        "{}：{} 的下发参数名 {} 不在清单里",
                        d.driver,
                        c.key,
                        param
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
                effect: UnknownEffect::SilentlyIgnored
            }
        );
        assert_eq!(
            verdict("postgres", "applicationName"), // 旧种子里的驼峰写法
            Verdict::Unknown {
                effect: UnknownEffect::SilentlyIgnored
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
        // 文件型：不下发
        assert!(matches!(
            verdict("sqlite", "journalMode"),
            Verdict::Unsupported { .. }
        ));
        assert!(matches!(
            verdict("duckdb", "memoryLimit"),
            Verdict::Unsupported { .. }
        ));
    }

    /// 库认的键一律判「会下发」，并带上标签 / 提醒。
    #[test]
    fn accepted_keys_are_delivered() {
        for d in &DRIVERS {
            if d.not_consumed.is_some() {
                continue;
            }
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
        assert!(known_keys("sqlite").is_empty(), "文件型不下发属性，无常用键");
        assert!(known_keys("some_plugin_driver").is_empty());
    }
}
