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

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::DriverRegistry;
use crate::registry::{DriverDescriptor, DriverFieldType};
use rds_store::persistence::GlobalDatabaseManager;
use rds_store::persistence::driver_operation_store::DriverOperation;
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
    /// 【D8】写了几行操作留痕（只在真变了 / 首次出现 / 能力位变化时才写）。
    pub recorded: usize,
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
    // 先读旧值：留痕的 `before` 必须是真读到的（读完再 upsert，才有可比的两份）
    let before_snapshots = snapshot_before(conn, decls)?;
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

        // 【D8】留痕：只在**真的变了**（或首次出现）时写一行 —— 幂等的每次启动写 4 行
        // 会把操作历史冲成噪声，用户翻不到真正的变更。
        if let Some(after) = DeclSnapshot::read(conn, &d.id)? {
            let before = before_snapshots.get(d.id.as_str());
            let changed: Vec<&'static str> = match before {
                Some(before) => before.changed_columns(&after),
                None => after.changed_columns(&DeclSnapshot::empty()),
            };
            if before.is_none() || !changed.is_empty() {
                let before_json = before.map(|b| b.to_json(&[]));
                let after_json = after.to_json(&changed);
                let op = DriverOperation::ok("runtime", "declare_sync")
                    .driver(d.id.clone())
                    .after(after_json);
                record_audit(conn, if let Some(b) = before_json { op.before(b) } else { op })?;
                report.recorded += 1;
            }
            // 能力键单独再记一行：能力面变了是**用户可见**的（属性模块会跟着置灰/放开）。
            // 首次出现不算「变化」——那条 declare_sync 已经把全部能力位写进 after 了。
            if before.is_some() && changed.contains(&"capabilities") {
                let mut op = DriverOperation::ok("runtime", "capability_change")
                    .driver(d.id.clone())
                    .after(after.capabilities_json());
                if let Some(before) = before {
                    op = op.before(before.capabilities_json());
                }
                record_audit(conn, op)?;
                report.recorded += 1;
            }
        }
    }

    Ok(report)
}

/// 同步前把每行状态读一遍：留痕的 `before` 必须是**真读到的旧值**，不能编。
fn snapshot_before(conn: &Connection, decls: &[DriverDeclaration]) -> Result<std::collections::HashMap<String, DeclSnapshot>, CoreError> {
    let mut map = std::collections::HashMap::new();
    for d in decls {
        if let Some(snapshot) = DeclSnapshot::read(conn, &d.id)? {
            map.insert(d.id.clone(), snapshot);
        }
    }
    Ok(map)
}

/// 写一行操作留痕（表不存在时只告警一次，不影响声明同步本身）
fn record_audit(conn: &Connection, op: DriverOperation) -> Result<(), CoreError> {
    use rds_store::persistence::driver_operation_store::{record_operation, table_exists};
    if !table_exists(conn)? {
        tracing::warn!(
            action = %op.action,
            driver = ?op.driver_id,
            "driver_operations 表不存在（迁移 030 未跑），本次留痕跳过"
        );
        return Ok(());
    }
    record_operation(conn, &op)?;
    Ok(())
}

/// 一行驱动声明的状态快照（只含判定「变没变」需要的列 + 可读摘要）
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeclSnapshot {
    type_id: String,
    name: String,
    driver_kind: String,
    is_file: bool,
    default_port: Option<i64>,
    url_template: Option<String>,
    version: String,
    config_schema: String,
    supported_auth_types: Option<String>,
    capabilities: String,
    driver_properties: String,
    enabled: bool,
}

impl DeclSnapshot {
    fn empty() -> Self {
        Self {
            type_id: String::new(),
            name: String::new(),
            driver_kind: String::new(),
            is_file: false,
            default_port: None,
            url_template: None,
            version: String::new(),
            config_schema: String::new(),
            supported_auth_types: None,
            capabilities: String::new(),
            driver_properties: String::new(),
            enabled: true,
        }
    }

    fn read(conn: &Connection, id: &str) -> Result<Option<Self>, CoreError> {
        let mut stmt = conn
            .prepare(
                "SELECT type_id, name, driver_kind, is_file, default_port, url_template,
                        version, config_schema, supported_auth_types, capabilities,
                        driver_properties, enabled
                   FROM drivers WHERE id = ?1",
            )
            .map_err(|e| storage_err("prepare_decl_snapshot", e.to_string()))?;
        let row = stmt
            .query_row(params![id], |row| {
                Ok(Self {
                    type_id: row.get(0)?,
                    name: row.get(1)?,
                    driver_kind: row.get(2)?,
                    is_file: row.get(3)?,
                    default_port: row.get(4)?,
                    url_template: row.get(5)?,
                    version: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                    config_schema: row.get(7)?,
                    supported_auth_types: row.get(8)?,
                    capabilities: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                    driver_properties: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                    enabled: row.get(11)?,
                })
            })
            .optional()
            .map_err(|e| storage_err("read_decl_snapshot", format!("{}: {}", id, e)))?;
        Ok(row)
    }

    /// 哪些列变了（列名即 `drivers` 的列名，用户能对上）
    fn changed_columns(&self, other: &Self) -> Vec<&'static str> {
        let mut changed = Vec::new();
        if self.type_id != other.type_id {
            changed.push("type_id");
        }
        if self.name != other.name {
            changed.push("name");
        }
        if self.driver_kind != other.driver_kind {
            changed.push("driver_kind");
        }
        if self.is_file != other.is_file {
            changed.push("is_file");
        }
        if self.default_port != other.default_port {
            changed.push("default_port");
        }
        if self.url_template != other.url_template {
            changed.push("url_template");
        }
        if self.version != other.version {
            changed.push("version");
        }
        if self.config_schema != other.config_schema {
            changed.push("config_schema");
        }
        if self.supported_auth_types != other.supported_auth_types {
            changed.push("supported_auth_types");
        }
        if self.capabilities != other.capabilities {
            changed.push("capabilities");
        }
        if self.driver_properties != other.driver_properties {
            changed.push("driver_properties");
        }
        // `enabled` 归库 / 用户所有，声明同步永远不改它，因此不参与差异判定
        changed
    }

    fn capabilities_json(&self) -> String {
        format!("{{\"capabilities\": {}}}", json_or_string(&self.capabilities))
    }

    /// 差异摘要（只放可读字段：**不放 config_schema 全文**，那会把行撑成几 KB）
    fn to_json(&self, changed: &[&str]) -> String {
        let changed_json = serde_json::to_string(changed).unwrap_or_else(|_| "[]".to_string());
        format!(
            "{{\"version\": {}, \"driver_kind\": {}, \"enabled\": {}, \"capabilities\": {}, \"changed\": {}}}",
            serde_json::to_string(&self.version).unwrap_or_else(|_| "\"\"".to_string()),
            serde_json::to_string(&self.driver_kind).unwrap_or_else(|_| "\"\"".to_string()),
            self.enabled,
            json_or_string(&self.capabilities),
            changed_json
        )
    }
}

/// 列存的是 JSON 文本时原样嵌入，不是合法 JSON 时按字符串包裹（不让留痕自己写坏）
fn json_or_string(raw: &str) -> String {
    let trimmed = raw.trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        trimmed.to_string()
    } else {
        serde_json::to_string(trimmed).unwrap_or_else(|_| "\"\"".to_string())
    }
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
    let report = upsert_all(conn, &decls)?;
    // 保留策略（方案 §3.5：1000 条 / 90 天取先到者）随启动清一次；清理失败不影响同步
    match rds_store::persistence::driver_operation_store::prune_operations(conn, 1000, 90) {
        Ok(0) => {}
        Ok(removed) => tracing::info!(removed, "驱动操作留痕超出保留策略，已清理"),
        Err(error) => tracing::warn!(error = %error, "驱动操作留痕清理失败（不影响声明同步）"),
    }
    Ok(report)
}

/// 一个数据库族（`data_source_types` 行）的代码声明。
///
/// 与 `DriverDeclaration` 同源思路：**族目录的权威也在代码**（本表），
/// 启动时由 [`ensure_data_source_types`] 幂等 upsert 进 `data_source_types`，
/// 迁移 008 的种子退居「首装兜底」。
///
/// ## 为什么驱动之外还要一份族目录
///
/// `drivers.type_id` 是 `data_source_types.id` 的外键——**族必须先存在，驱动声明才落得了库**。
/// 历史做法是「族只靠迁移种子」，新增库族（hive / gaussdb / excel…）必须手写一条迁移，
/// 而驱动只需在 `descriptors.rs` 加一行——不对称，且写错族 id 会静默丢驱动行
/// （见 [`sync_driver_declarations`] 的 `skipped` 留痕）。这份声明让「新增族 + 新增驱动」
/// 都能纯代码声明，不必为加一个库族去动迁移（迁移只在建表 / 补列时修改）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSourceTypeDeclaration {
    pub id: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    /// 目录里的图标（em 字符，界面类型树用）；`None` 则不改动已存在行的图标。
    pub icon: Option<&'static str>,
}

/// 内置数据库族目录（权威）。
///
/// 分类取值与迁移 008 一致（`relational` / `file-based` / `analytics` / `nosql`）。
/// **新增一个库族在这里加一行即可**，再在 `descriptors.rs` 声明指向它的驱动。
/// 注意：`enabled` 列归库（用户可禁用某个族），本声明不拥有——upsert 不写它。
pub const BUILTIN_DATA_SOURCE_TYPES: &[DataSourceTypeDeclaration] = &[
    DataSourceTypeDeclaration {
        id: "mysql",
        name: "MySQL",
        category: "relational",
        icon: Some("🐬"),
    },
    DataSourceTypeDeclaration {
        id: "postgresql",
        name: "PostgreSQL",
        category: "relational",
        icon: Some("🐘"),
    },
    DataSourceTypeDeclaration {
        id: "sqlite",
        name: "SQLite",
        category: "file-based",
        icon: Some("🪶"),
    },
    DataSourceTypeDeclaration {
        id: "duckdb",
        name: "DuckDB",
        category: "analytics",
        icon: Some("🦆"),
    },
    DataSourceTypeDeclaration {
        id: "mariadb",
        name: "MariaDB",
        category: "relational",
        icon: Some("🦭"),
    },
    DataSourceTypeDeclaration {
        id: "oracle",
        name: "Oracle",
        category: "relational",
        icon: Some("🔴"),
    },
    DataSourceTypeDeclaration {
        id: "mssql",
        name: "SQL Server",
        category: "relational",
        icon: Some("🟢"),
    },
    DataSourceTypeDeclaration {
        id: "clickhouse",
        name: "ClickHouse",
        category: "analytics",
        icon: Some("⚡"),
    },
    DataSourceTypeDeclaration {
        id: "mongodb",
        name: "MongoDB",
        category: "nosql",
        icon: Some("🍃"),
    },
    DataSourceTypeDeclaration {
        id: "redis",
        name: "Redis",
        category: "nosql",
        icon: Some("🔶"),
    },
];

/// 幂等 upsert 数据库族目录（`sync_startup_state` 在驱动声明**之前**调用）。
///
/// 返回写入 / 更新的条数。覆盖的列 = 声明拥有：`name` / `category` / `icon`；
/// `enabled`（库 / 用户所有）与 `created_at`（库）不写——用户禁用的族不会被启动同步重新打开。
pub fn ensure_data_source_types(conn: &Connection) -> Result<usize, CoreError> {
    let mut stmt = conn
        .prepare(
            "INSERT INTO data_source_types (id, name, category, icon, enabled)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 category = excluded.category,
                 icon = excluded.icon",
        )
        .map_err(|e| storage_err("prepare_upsert_data_source_types", e.to_string()))?;

    let mut written = 0usize;
    for t in BUILTIN_DATA_SOURCE_TYPES {
        stmt.execute(params![t.id, t.name, t.category, t.icon])
            .map_err(|e| storage_err("upsert_data_source_type", format!("{}: {}", t.id, e)))?;
        written += 1;
    }
    Ok(written)
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
            if let crate::registry::DriverOptionType::Select { options } = &o.option_type {
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

fn option_type_str(t: &crate::registry::DriverOptionType) -> &'static str {
    use crate::registry::DriverOptionType as T;
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

    /// 测试库 + `driver_operations`（留痕用例要真表；其余用例只有 `drivers` 表，
    /// 顺带守着「留痕表不存在也不影响声明同步」这条）
    fn test_db_with_audit() -> Connection {
        let conn = test_db();
        conn.execute_batch(include_str!(
            "../../store/migrations/global/030_driver_operations.sql"
        ))
        .expect("create driver_operations");
        conn
    }

    /// D8 / A-4：首次同步留一行，**幂等的第二次不再写**（否则每次启动灌 4 行噪声）。
    #[test]
    fn declare_sync_is_recorded_only_when_something_changed() {
        use rds_store::persistence::driver_operation_store::{
            DriverOperationFilter, list_operations,
        };

        let conn = test_db_with_audit();
        let decls = declarations();

        let first = upsert_all(&conn, &decls).expect("first upsert");
        assert_eq!(first.written, decls.len());
        assert_eq!(
            first.recorded,
            decls.len(),
            "首次同步每个驱动都该留一行「新增」：{first:?}"
        );

        let second = upsert_all(&conn, &decls).expect("second upsert");
        assert_eq!(second.written, decls.len(), "upsert 本身仍幂等写入");
        assert_eq!(second.recorded, 0, "没变就不该再写留痕：{second:?}");

        let rows = list_operations(
            &conn,
            &DriverOperationFilter {
                action: Some("declare_sync".to_string()),
                limit: Some(100),
                ..Default::default()
            },
        )
        .expect("list declare_sync");
        assert_eq!(rows.len(), decls.len());
        for row in &rows {
            assert_eq!(row.operation.actor, "runtime");
            assert_eq!(row.operation.result, "ok");
            assert!(row.operation.driver_id.is_some());
            // 首次出现的行没有 before（之前表里没有这条驱动）
            assert!(row.operation.before_json.is_none());
            assert!(row.operation.after_json.is_some());
        }
        assert!(
            rows.iter().all(|r| r.operation.after_json.as_deref().unwrap_or("").contains("\"changed\"")),
            "差异摘要要写清哪些列变了"
        );
    }

    /// D8 / A-4：能力键变化额外留一行 `capability_change`（属性模块的置灰/放开跟着它）。
    #[test]
    fn capability_change_is_recorded_separately() {
        use rds_store::persistence::driver_operation_store::{
            DriverOperationFilter, list_operations,
        };

        let conn = test_db_with_audit();
        let decls = declarations();
        upsert_all(&conn, &decls).expect("seed");

        // 改一个驱动声明的能力串（模拟代码里新增 / 去掉一个能力位）
        let mut changed = decls
            .iter()
            .find(|d| d.id == "sqlite")
            .cloned()
            .expect("sqlite");
        changed.capabilities = "[\"transactions\",\"meta_ddl_compose\"]".to_string();
        let report = upsert_all(&conn, &[changed]).expect("capability upsert");
        assert!(report.recorded >= 2, "declare_sync + capability_change 各一行：{report:?}");

        let cap_rows = list_operations(
            &conn,
            &DriverOperationFilter {
                action: Some("capability_change".to_string()),
                ..Default::default()
            },
        )
        .expect("list capability_change");
        assert_eq!(cap_rows.len(), 1);
        let row = &cap_rows[0];
        assert_eq!(row.operation.driver_id.as_deref(), Some("sqlite"));
        assert!(
            row.operation.before_json.as_deref().unwrap_or("").contains("transactions"),
            "要有改动前的能力位：{:?}",
            row.operation.before_json
        );
        assert!(
            row.operation.after_json.as_deref().unwrap_or("").contains("meta_ddl_compose"),
            "要有改动后的能力位：{:?}",
            row.operation.after_json
        );
    }

    fn declarations() -> Vec<DriverDeclaration> {        crate::AutoDriverRegistrar::register_builtin_drivers();
        DriverDeclaration::all()
    }

    /// 声明必须挂到**真实存在的族**上：`postgres` 系列曾把族写成 `postgres`
    /// （`data_source_types.id` 是 `postgresql`），upsert 会因外键挂不上而静默少两行。
    #[test]
    fn every_declaration_targets_a_seeded_family() {
        let conn = test_db();
        let decls = declarations();
        assert!(
            decls.len() >= 4,
            "内置驱动声明应有 4 条（每族一条官方实现）：{decls:#?}"
        );

        // `mysql` / `postgres` 不在表内：这两个 id 由 sidecar 插件（`rds.driver.sqlx`）
        // 贡献声明，内核声明器不再 upsert 它们（见 sqlx-driver-extraction-plan.md D3/D4）。
        let expected: HashMap<&str, &str> = [
            ("mysql_native", "mysql"),
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
        // `recorded` 首跑与二跑**按设计不同**（留痕只记真变化），其余字段必须一致
        assert_eq!(second.written, first.written, "第二次写入行数应与第一次一致（幂等）");
        assert_eq!(second.skipped, first.skipped, "第二次跳过集合应与第一次一致");
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

    /// 族目录的 upsert 必须幂等，且**不越权**：`enabled` 归库（用户可禁用某个族）。
    #[test]
    fn data_source_types_upsert_is_idempotent_and_keeps_enabled() {
        let conn = test_db();

        // 首跑：写满代码声明的族目录（test_db 只预置了 4 个族，其余由 ensure 补上）。
        let first = ensure_data_source_types(&conn).expect("ensure first");
        assert_eq!(first, BUILTIN_DATA_SOURCE_TYPES.len(), "应写满内置族目录");

        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).expect("count") };
        let rows_after_first = count("SELECT COUNT(*) FROM data_source_types");
        assert_eq!(
            rows_after_first,
            BUILTIN_DATA_SOURCE_TYPES.len() as i64,
            "行数 = 声明条数"
        );

        // 预置：用户禁用了 mariadb（不该被后续同步重新打开）。
        conn.execute(
            "UPDATE data_source_types SET enabled = 0 WHERE id = 'mariadb'",
            [],
        )
        .expect("disable mariadb");

        // 再跑：幂等，行数不变，且用户禁用的族不被打开。
        let second = ensure_data_source_types(&conn).expect("ensure second");
        assert_eq!(second, first, "第二次应与第一次一致（幂等）");
        assert_eq!(
            count("SELECT COUNT(*) FROM data_source_types"),
            rows_after_first,
            "upsert 不得新增行"
        );

        let enabled: i64 = conn
            .query_row(
                "SELECT enabled FROM data_source_types WHERE id = 'mariadb'",
                [],
                |r| r.get(0),
            )
            .expect("read back");
        assert_eq!(enabled, 0, "用户禁用的族不得被启动同步打开");

        // 声明侧列被写入（新增库族的 name / category / icon 从这里来）。
        let (name, category, icon): (String, String, Option<String>) = conn
            .query_row(
                "SELECT name, category, icon FROM data_source_types WHERE id = 'clickhouse'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("read back");
        assert_eq!(name, "ClickHouse");
        assert_eq!(category, "analytics");
        assert_eq!(icon.as_deref(), Some("⚡"));
    }

    /// 族目录声明必须覆盖每个内置驱动引用的族：`drivers.type_id` 是外键，
    /// 驱动声明的族若不在目录里，upsert 会静默丢行。
    #[test]
    fn every_driver_family_is_in_builtin_data_source_types() {
        let known: std::collections::HashSet<&str> =
            BUILTIN_DATA_SOURCE_TYPES.iter().map(|t| t.id).collect();
        for d in declarations() {
            assert!(
                known.contains(d.type_id.as_str()),
                "驱动 {} 引用的族 {} 不在内置族目录里（去 BUILTIN_DATA_SOURCE_TYPES 补一行）",
                d.id,
                d.type_id
            );
        }
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
    /// 清单的唯一来源是 [`crate::property_spec`]（依据依赖源码，见那里的模块头）；
    /// 本测试只盯住声明与清单不漂：sqlx 会静默忽略未知键、`mysql_async` 与 `tokio-postgres`
    /// 会直接报错——所以一个不认的键要么无声失效、要么弄坏连接，两种都不能出现在默认值里。
    #[test]
    fn declared_property_keys_are_accepted_by_the_client_library() {
        use crate::property_spec;

        for d in declarations() {
            let keys: serde_json::Map<String, Value> = serde_json::from_str(&d.driver_properties)
                .expect("driver_properties 应是 JSON 对象");
            for key in keys.keys() {
                assert!(
                    property_spec::accepts(&d.id, key),
                    "{} 声明了客户端库不认的属性键 {key}（该驱动认的键：{:?}）",
                    d.id,
                    property_spec::known_keys(&d.id)
                        .iter()
                        .map(|k| k.key)
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    /// `config_schema` 的形状要与视图的解析范围一致（视图只读 `fields[]` 的
    /// `key/label/type/required/placeholder`，`options[]` 目前无消费者）。
    #[test]
    fn config_schema_carries_what_the_view_reads() {
        use crate::registry::{duckdb_driver, mysql_native_driver, sqlite_driver};

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
                let kind = f
                    .get("type")
                    .and_then(|v| v.as_str())
                    .expect("type 是字符串");
                assert!(
                    ["text", "password", "number", "file", "select"].contains(&kind),
                    "{} 的字段类型 {} 不是视图认识的形态",
                    d.id,
                    kind
                );
            }
            assert!(json.get("options").and_then(|o| o.as_array()).is_some());
        };

        check(&mysql_native_driver());
        check(&sqlite_driver());
        check(&duckdb_driver());

        // 有下拉选项的驱动：`options[].values` 要带上，否则选项就丢了。
        // （D3 之后带 Select 选项的内置驱动只剩 sqlite 的「打开模式」——mysql 的 ssl-mode
        //  下拉随 sqlx 实现一起进了插件清单。）
        let json: Value = serde_json::from_str(&config_schema_json(&sqlite_driver())).expect("json");
        let values = json["options"][0]["values"]
            .as_array()
            .expect("select 的 values");
        assert!(values.iter().any(|v| v == "rwc"), "{json}");
    }
}
