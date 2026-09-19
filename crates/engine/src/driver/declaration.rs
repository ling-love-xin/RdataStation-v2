//! 驱动声明落库：**代码是权威，`drivers` 表是读模型**。
//!
//! ## 解决什么
//!
//! 驱动声明曾经有两处写：Rust 侧 `DriverDescriptor`（只有 `id` 被读）与 `drivers` 表
//! （界面与连接链路真读的那份，靠迁移 008/013/014/016/017 手工对齐）。后果是
//! 「新增驱动只改一行」的承诺不成立，且两份声明必然漂——已经漂出过三处
//! （`postgres` 的族 id 写成 `postgres`、DuckDB / SQLite 的 `transactions`、
//! 网络能力键只在 017 的 SQL 里）。
//!
//! 现在：**声明在代码里（`registry/descriptors.rs`），启动时幂等 upsert 进表**，
//! 迁移里的种子退居「首装兜底」（表结构仍由迁移建，见 §列归属）。
//!
//! ## 列归属（upsert 只覆盖声明拥有的列）
//!
//! | 列 | 归属 | 理由 |
//! | --- | --- | --- |
//! | `type_id` / `name` / `driver_kind` / `is_file` / `default_port` / `url_template` / `version` / `config_schema` / `supported_auth_types` / `capabilities` / `driver_properties` | **代码声明** | 是驱动实现的属性，随代码发布 |
//! | `enabled` | **库 / 用户** | 用户可以关掉一个驱动；启动不能把它打开（`DO UPDATE` 里不出现该列） |
//! | `download_url` / `download_checksum` | **库 / 外部驱动** | 外部驱动的下载信息，内置驱动的声明不拥有 |
//! | `driver_files` 表 | **本机安装状态** | 「装了什么」不是声明能推导的 |
//! | `created_at` | 库 | 审计时间 |
//!
//! ## 一致性
//!
//! `capabilities` 由 [`DriverDescriptor::capability_keys`] 给出（显式声明 + 网络布尔位派生），
//! `config_schema` 由 [`DriverDescriptor::fields`] / `extra_options` 组装成
//! **界面实际读的那几个键**（`key/label/type/required/default/placeholder`、`options[].values`）；
//! `driver_properties` 只含**该客户端库真认**的键（见 `descriptors.rs` 里各驱动的注释与
//! `docs/architecture/driver-capability-matrix.md` §2.1「未知参数」列）。
//!
//! 声明与表的漂移由三处测试盯住：本模块的 upsert 幂等/不越权测试、
//! `capability::tests::declared_capabilities_and_runtime_bits_agree`（键 ↔ 运行时位）。

use rusqlite::{params, Connection};
use serde_json::Value;

use crate::driver::registry::{DriverDescriptor, DriverFieldType};
use crate::driver::DriverRegistry;
use crate::persistence::GlobalDatabaseManager;
use shared::error::{CoreError, StorageError};

/// `drivers.version` 的声明值。
///
/// 内置驱动的版本随应用发布走，不是用户可管理的东西；保留这一列是为了外部驱动
/// （JDBC jar / WASM 包）能记自己那份版本，内置驱动给个恒定值即可。
const DECLARED_DRIVER_VERSION: &str = "1.0.0";

/// 一行驱动声明（`drivers` 表里「声明拥有」的那些列的完整内容）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverDeclaration {
    pub id: String,
    /// 数据库族 id（`data_source_types.id`）——`drivers.type_id` 是它的外键。
    pub type_id: String,
    pub name: String,
    pub driver_kind: String,
    pub is_file: bool,
    pub default_port: Option<i64>,
    pub url_template: Option<String>,
    pub version: String,
    pub config_schema: String,
    pub supported_auth_types: String,
    pub capabilities: String,
    pub driver_properties: String,
}

impl DriverDeclaration {
    /// 由描述符派生一行声明。
    ///
    /// `target_database` 缺失 → `None`：`drivers.type_id` 是外键且非空，没有族就落不了库，
    /// 与其猜一个族（历史上就是猜错才把 `postgres` 当族）不如让调用方显式报出来。
    pub fn of(d: &DriverDescriptor) -> Option<Self> {
        let type_id = d.target_database.clone()?;
        Some(Self {
            id: d.id.clone(),
            type_id,
            name: d.name.clone(),
            driver_kind: d.driver_kind.as_str().to_string(),
            is_file: d.require_file,
            default_port: d.default_port.map(i64::from),
            url_template: d.url_template.clone(),
            version: DECLARED_DRIVER_VERSION.to_string(),
            config_schema: config_schema_json(d),
            supported_auth_types: json_string_array(&d.supported_auth_types),
            capabilities: json_string_array(&d.capability_keys()),
            driver_properties: json_string_map(&d.driver_properties),
        })
    }

    /// 全部内置驱动声明（**按 id 排序**：注册表是 HashMap，迭代顺序不固定，
    /// 排序让 upsert 的顺序与日志稳定、测试可复现）。
    pub fn all() -> Vec<Self> {
        let mut decls: Vec<Self> = DriverRegistry::all_descriptors()
            .iter()
            .filter_map(Self::of)
            .collect();
        decls.sort_by(|a, b| a.id.cmp(&b.id));
        decls
    }
}

/// 声明落库的结果（启动日志与测试都看它）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclarationSync {
    /// 写入（插入或按声明列更新）的行数。
    pub written: usize,
    /// 跳过的驱动：`(驱动 id, 原因)`。跳过必留痕——静默少写一行就是这次要消灭的毛病。
    pub skipped: Vec<(String, String)>,
}

/// 幂等 upsert 全部内置驱动声明（`initialize_global_system` 启动时调用）。
///
/// 返回写入条数；单条声明落不了库（族不在 `data_source_types`）记入 `skipped` 而不中断
/// ——一个坏声明不该让整个启动同步失败。
pub fn upsert_all(
    conn: &Connection,
    decls: &[DriverDeclaration],
) -> Result<DeclarationSync, CoreError> {
    let mut report = DeclarationSync::default();
    let mut stmt = conn
        .prepare(
            "INSERT INTO drivers
                 (id, type_id, name, driver_kind, is_file, default_port, url_template,
                  version, config_schema, supported_auth_types, capabilities, driver_properties, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1)
             ON CONFLICT(id) DO UPDATE SET
                 type_id = excluded.type_id,
                 name = excluded.name,
                 driver_kind = excluded.driver_kind,
                 is_file = excluded.is_file,
                 default_port = excluded.default_port,
                 url_template = excluded.url_template,
                 version = excluded.version,
                 config_schema = excluded.config_schema,
                 supported_auth_types = excluded.supported_auth_types,
                 capabilities = excluded.capabilities,
                 driver_properties = excluded.driver_properties",
        )
        .map_err(|e| storage_err("prepare_upsert_driver_declarations", e.to_string()))?;

    for d in decls {
        // 外键：族必须先在 `data_source_types` 里（由迁移 008 建表 + 播种）。
        // 先查一次而不是靠 FK 报错：错误信息里能带上驱动 id 与缺哪个族。
        match family_exists(conn, &d.type_id) {
            Ok(true) => {}
            Ok(false) => {
                report.skipped.push((
                    d.id.clone(),
                    format!("数据库族 '{}' 不在 data_source_types 里", d.type_id),
                ));
                continue;
            }
            Err(e) => return Err(e),
        }

        stmt.execute(params![
            d.id,
            d.type_id,
            d.name,
            d.driver_kind,
            d.is_file,
            d.default_port,
            d.url_template,
            d.version,
            d.config_schema,
            d.supported_auth_types,
            d.capabilities,
            d.driver_properties,
        ])
        .map_err(|e| storage_err("upsert_driver_declaration", format!("{}: {}", d.id, e)))?;
        report.written += 1;
    }

    Ok(report)
}

/// 启动路径：把代码里的驱动声明写回全局库（幂等）。
///
/// 注册表为空时**报错**而不是静默写 0 行：调用方（启动）按「失败只告警」处理，
/// 日志里能看见，不会出现「同步成功了但一行没写」。
pub async fn sync_driver_declarations(
    manager: &GlobalDatabaseManager,
) -> Result<DeclarationSync, CoreError> {
    let decls = DriverDeclaration::all();
    if decls.is_empty() {
        return Err(storage_err(
            "driver_declarations_empty",
            "驱动注册表为空（启动前应先调用 AutoDriverRegistrar::auto_register）".to_string(),
        ));
    }

    let sqlite = manager
        .sqlite_pool()
        .acquire()
        .await
        .map_err(|e| storage_err("acquire_global_sqlite", e.to_string()))?;
    let conn = sqlite
        .inner()
        .map_err(|e| storage_err("global_sqlite_conn", e.to_string()))?;
    upsert_all(conn, &decls)
}

/// 该族是否已在 `data_source_types` 里。
fn family_exists(conn: &Connection, type_id: &str) -> Result<bool, CoreError> {
    conn.query_row(
        "SELECT 1 FROM data_source_types WHERE id = ?1",
        params![type_id],
        |_| Ok(()),
    )
    .map(|_| true)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(false),
        other => Err(storage_err("family_exists", other.to_string())),
    })
}

/// `config_schema`（界面读的那几个键，见 `connection_dialog::helpers::driver_form_fields`）。
///
/// 字段名与迁移种子逐字一致：`fields[].{key,label,type,required,default,placeholder}`、
/// `options[].{key,label,type,default,values,description}`。`type` 用**字符串**而不是枚举的
/// serde 形态——消费方（视图）读的是字符串，带参数的枚举形态它认不出来（会静默退化成 `text`）。
///
/// `options[]` 当前 crates 内无消费者（视图只读 `fields[]`），保留是为了不丢种子里已有的
/// 选项描述（如 MySQL 的五档 SSL 模式），也给后续「高级选项」表单留好数据。
fn config_schema_json(d: &DriverDescriptor) -> String {
    let fields: Vec<Value> = d
        .fields
        .iter()
        .map(|f| {
            let mut o = serde_json::Map::new();
            o.insert("key".into(), Value::from(f.key.clone()));
            o.insert("label".into(), Value::from(f.label.clone()));
            o.insert("type".into(), Value::from(field_type_str(&f.field_type)));
            o.insert("required".into(), Value::from(f.required));
            if let Some(v) = &f.default_value {
                o.insert("default".into(), Value::from(v.clone()));
            }
            if let Some(v) = &f.placeholder {
                o.insert("placeholder".into(), Value::from(v.clone()));
            }
            Value::Object(o)
        })
        .collect();

    let options: Vec<Value> = d
        .extra_options
        .iter()
        .map(|o| {
            let mut m = serde_json::Map::new();
            m.insert("key".into(), Value::from(o.key.clone()));
            m.insert("label".into(), Value::from(o.label.clone()));
            m.insert("type".into(), Value::from(option_type_str(&o.option_type)));
            m.insert("default".into(), Value::from(o.default_value.clone()));
            if let crate::driver::registry::DriverOptionType::Select { options } = &o.option_type {
                m.insert(
                    "values".into(),
                    Value::Array(options.iter().cloned().map(Value::from).collect()),
                );
            }
            if let Some(desc) = &o.description {
                m.insert("description".into(), Value::from(desc.clone()));
            }
            Value::Object(m)
        })
        .collect();

    serde_json::json!({ "fields": fields, "options": options }).to_string()
}

/// 视图侧的字段类型名（`FormField.kind` 认这几个）。
fn field_type_str(t: &DriverFieldType) -> &'static str {
    match t {
        DriverFieldType::Text => "text",
        DriverFieldType::Password => "password",
        DriverFieldType::Number => "number",
        DriverFieldType::File => "file",
        DriverFieldType::Select { .. } => "select",
    }
}

fn option_type_str(t: &crate::driver::registry::DriverOptionType) -> &'static str {
    use crate::driver::registry::DriverOptionType as T;
    // 词汇与 `fields[].type` 同一套（历史前端 `mapOptionType` 认的就是这几个：
    // `text` / `password` / `number` / `select` / `checkbox`）——布尔写 `checkbox` 而不是 `bool`。
    match t {
        T::String => "text",
        T::Number => "number",
        T::Boolean => "checkbox",
        T::Select { .. } => "select",
        T::File => "file",
    }
}

/// `["a","b"]`（字符串数组列：`supported_auth_types` / `capabilities`）。
fn json_string_array(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string())
}

/// `{"k":"v"}`（`driver_properties`；`BTreeMap` 保证键序稳定 → upsert 写出的字节可复现）。
fn json_string_map(map: &std::collections::BTreeMap<String, String>) -> String {
    serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string())
}

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "driver_declaration".to_string(),
        operation: op.to_string(),
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 建两张表（列与迁移 008 一致，够 upsert 用）。
    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE data_source_types (
                 id TEXT PRIMARY KEY, name TEXT NOT NULL, category TEXT NOT NULL,
                 icon TEXT, enabled BOOLEAN DEFAULT 1,
                 created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP);
             CREATE TABLE drivers (
                 id TEXT PRIMARY KEY,
                 type_id TEXT NOT NULL REFERENCES data_source_types(id),
                 name TEXT NOT NULL,
                 driver_kind TEXT DEFAULT 'native',
                 is_file BOOLEAN DEFAULT 0,
                 default_port INTEGER,
                 url_template TEXT,
                 download_url TEXT,
                 download_checksum TEXT,
                 version TEXT,
                 config_schema TEXT NOT NULL,
                 supported_auth_types TEXT,
                 capabilities TEXT,
                 driver_properties TEXT,
                 enabled BOOLEAN DEFAULT 1,
                 created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP);",
        )
        .expect("create tables");
        for (id, name, cat) in [
            ("mysql", "MySQL", "relational"),
            ("postgresql", "PostgreSQL", "relational"),
            ("sqlite", "SQLite", "file-based"),
            ("duckdb", "DuckDB", "file-based"),
        ] {
            conn.execute(
                "INSERT INTO data_source_types (id, name, category) VALUES (?1, ?2, ?3)",
                params![id, name, cat],
            )
            .expect("seed family");
        }
        conn
    }

    fn declarations() -> Vec<DriverDeclaration> {
        crate::driver::AutoDriverRegistrar::register_builtin_drivers();
        DriverDeclaration::all()
    }

    /// 声明必须挂到**真实存在的族**上：`postgres` 系列曾把族写成 `postgres`
    /// （`data_source_types.id` 是 `postgresql`），upsert 会因外键挂不上而静默少两行。
    #[test]
    fn every_declaration_targets_a_seeded_family() {
        let conn = test_db();
        let decls = declarations();
        assert!(decls.len() >= 6, "内置驱动声明应有 6 条：{decls:#?}");

        let expected: HashMap<&str, &str> = [
            ("mysql", "mysql"),
            ("mysql_native", "mysql"),
            ("postgres", "postgresql"),
            ("postgres_native", "postgresql"),
            ("sqlite", "sqlite"),
            ("duckdb", "duckdb"),
        ]
        .into_iter()
        .collect();

        for d in &decls {
            let want = expected
                .get(d.id.as_str())
                .unwrap_or_else(|| panic!("未预期的内置驱动 {}（新增驱动请同步本测试）", d.id));
            assert_eq!(&d.type_id, want, "{} 的族 id", d.id);
            assert!(
                family_exists(&conn, &d.type_id).expect("family_exists"),
                "{} 的族 {} 不在 data_source_types 里",
                d.id,
                d.type_id
            );
        }
    }

    /// upsert 必须幂等，且**不越权**：`enabled` / `download_*` 是库（用户 / 外部驱动）的列。
    #[test]
    fn upsert_is_idempotent_and_keeps_library_owned_columns() {
        let conn = test_db();
        let decls = declarations();

        // 预置一行：用户关掉了驱动、且有一份外部下载信息（不该被声明覆盖）。
        conn.execute(
            "INSERT INTO drivers (id, type_id, name, config_schema, enabled, download_url)
             VALUES ('duckdb', 'duckdb', '旧的显示名', '{}', 0, 'https://example.invalid/d.zip')",
            [],
        )
        .expect("preseed row");

        let first = upsert_all(&conn, &decls).expect("first upsert");
        assert_eq!(first.written, decls.len(), "首跑应写满：{first:?}");
        assert!(first.skipped.is_empty(), "不应跳过：{first:?}");

        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).expect("count") };
        let rows_after_first = count("SELECT COUNT(*) FROM drivers");

        let second = upsert_all(&conn, &decls).expect("second upsert");
        assert_eq!(second, first, "第二次结果应与第一次一致（幂等）");
        assert_eq!(
            count("SELECT COUNT(*) FROM drivers"),
            rows_after_first,
            "upsert 不得新增行"
        );
        assert_eq!(rows_after_first, decls.len() as i64, "行数 = 声明条数");

        // 库侧列原样保留
        let (enabled, download): (i64, Option<String>) = conn
            .query_row(
                "SELECT enabled, download_url FROM drivers WHERE id = 'duckdb'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("read back");
        assert_eq!(enabled, 0, "用户关掉的驱动不得被启动同步打开");
        assert_eq!(
            download.as_deref(),
            Some("https://example.invalid/d.zip"),
            "download_url 属库侧，声明不覆盖"
        );

        // 声明侧列被写入
        let duck = decls.iter().find(|d| d.id == "duckdb").expect("duckdb");
        let (name, capabilities, props, version): (String, String, String, String) = conn
            .query_row(
                "SELECT name, capabilities, driver_properties, version \
                 FROM drivers WHERE id = 'duckdb'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("read back");
        assert_eq!(name, duck.name);
        assert_eq!(capabilities, duck.capabilities);
        assert_eq!(props, duck.driver_properties);
        assert_eq!(version, DECLARED_DRIVER_VERSION);
        // 派生键确实落库（文件型驱动没有网络键）
        assert!(
            !capabilities.contains("ssl_tls"),
            "文件型不该有网络键：{capabilities}"
        );
    }

    /// 声明了族、但族不在类型目录里 → 跳过并留痕（不中断整批）。
    #[test]
    fn unknown_family_is_skipped_with_a_reason() {
        let conn = test_db();
        let mut decl = declarations()
            .into_iter()
            .find(|d| d.id == "sqlite")
            .expect("sqlite");
        decl.type_id = "no_such_family".to_string();

        let report = upsert_all(&conn, &[decl]).expect("upsert");
        assert_eq!(report.written, 0);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].0, "sqlite");
        assert!(
            report.skipped[0].1.contains("no_such_family"),
            "跳过原因要能看出缺哪个族：{:?}",
            report.skipped
        );
    }

    /// `driver_properties` 的键必须是**该客户端库真认**的参数名。
    ///
    /// 清单来自依赖源码（读到哪一行写在 `descriptors.rs` 对应驱动的注释里），
    /// **升依赖后要复查**：sqlx 会静默忽略未知键、`mysql_async` 与 `tokio-postgres` 会直接报错——
    /// 所以一个不认的键要么无声失效、要么弄坏连接，两种都不能留在这里。
    #[test]
    fn declared_property_keys_are_accepted_by_the_client_library() {
        // 各库 URL 解析器认的键（详见 `docs/architecture/driver-capability-matrix.md` §2.1）
        let accepted: HashMap<&str, &[&str]> = [
            (
                "mysql",
                &[
                    "sslmode", "ssl-mode", "sslca", "ssl-ca", "sslcert", "ssl-cert", "sslkey",
                    "ssl-key", "charset", "collation", "statement-cache-capacity", "socket",
                    "timezone", "time-zone",
                ][..],
            ),
            (
                "postgres",
                &[
                    "sslmode", "ssl-mode", "sslrootcert", "ssl-root-cert", "ssl-ca", "sslcert",
                    "ssl-cert", "sslkey", "ssl-key", "statement-cache-capacity", "host",
                    "hostaddr", "port", "dbname", "user", "password", "application_name", "options",
                ][..],
            ),
            (
                "mysql_native",
                &[
                    "pool_min", "pool_max", "inactive_connection_ttl", "ttl_check_interval",
                    "conn_ttl", "abs_conn_ttl", "abs_conn_ttl_jitter", "tcp_keepalive",
                    "max_allowed_packet", "wait_timeout", "enable_cleartext_plugin",
                    "reset_connection", "tcp_nodelay", "stmt_cache_size", "prefer_socket",
                    "secure_auth", "client_found_rows", "socket", "compression", "require_ssl",
                    "verify_ca", "verify_identity", "built_in_roots",
                ][..],
            ),
            (
                "postgres_native",
                &[
                    "user", "password", "dbname", "options", "application_name", "sslmode", "host",
                    "sslnegotiation", "hostaddr", "port", "connect_timeout", "tcp_user_timeout",
                    "keepalives", "keepalives_idle", "keepalives_interval", "keepalives_retries",
                    "target_session_attrs", "channel_binding", "load_balance_hosts",
                ][..],
            ),
            // 文件型：路径由工厂取，查询串会被剥掉 → 一个键都不该声明
            ("sqlite", &[][..]),
            ("duckdb", &[][..]),
        ]
        .into_iter()
        .collect();

        for d in declarations() {
            let keys: serde_json::Map<String, Value> =
                serde_json::from_str(&d.driver_properties).expect("driver_properties 应是 JSON 对象");
            let allowed = accepted
                .get(d.id.as_str())
                .unwrap_or_else(|| panic!("未预期的驱动 {}（新增驱动请同步本测试）", d.id));
            for key in keys.keys() {
                assert!(
                    allowed.contains(&key.as_str()),
                    "{} 声明了客户端库不认的属性键 {key}（认的键：{allowed:?}）",
                    d.id
                );
            }
        }
    }

    /// `config_schema` 的形状要与视图的解析范围一致（视图只读 `fields[]` 的
    /// `key/label/type/required/placeholder`，`options[]` 目前无消费者）。
    #[test]
    fn config_schema_carries_what_the_view_reads() {
        use crate::driver::registry::{duckdb_driver, mysql_driver, sqlite_driver};

        let check = |d: &DriverDescriptor| {
            let json: Value =
                serde_json::from_str(&config_schema_json(d)).expect("config_schema 应是 JSON");
            let fields = json
                .get("fields")
                .and_then(|f| f.as_array())
                .unwrap_or_else(|| panic!("{} 的 config_schema 缺 fields[]", d.id));
            assert_eq!(fields.len(), d.fields.len(), "{} 的字段数", d.id);
            for f in fields {
                for k in ["key", "label", "type", "required"] {
                    assert!(f.get(k).is_some(), "{} 的字段缺 {k}：{f}", d.id);
                }
                let kind = f.get("type").and_then(|v| v.as_str()).expect("type 是字符串");
                assert!(
                    ["text", "password", "number", "file", "select"].contains(&kind),
                    "{} 的字段类型 {} 不是视图认识的形态",
                    d.id,
                    kind
                );
            }
            assert!(json.get("options").and_then(|o| o.as_array()).is_some());
        };

        check(&mysql_driver());
        check(&sqlite_driver());
        check(&duckdb_driver());

        // 有下拉选项的驱动：`options[].values` 要带上，否则选项就丢了
        let json: Value =
            serde_json::from_str(&config_schema_json(&mysql_driver())).expect("json");
        let values = json["options"][0]["values"].as_array().expect("select 的 values");
        assert!(values.iter().any(|v| v == "VERIFY_IDENTITY"), "{json}");
    }
}
