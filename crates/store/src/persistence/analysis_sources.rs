//! 分析源登记（`analysis_sources`）：项目库里的**意图侧**。
//!
//! 分工（口径见 `docs/architecture/layout/analysis-sources-spec.md`、铁律第 4 条）：
//!
//! - 本模块只做**登记行**的读写（项目级、持久）：要挂哪些源、用什么别名、怎么挂。
//! - 「挂没挂」是**运行态事实**：`duckdb_databases()` 里有没有这个别名。两侧对账在
//!   上层（面板）做，本模块不缓存状态——写进库必然与实例漂移。
//! - **会话级不入库**：会话级加源切项目即清（铁律第 6 条），存进项目库只会留下
//!   "未挂载的项目级源"这种假记录。事实侧看得见它，登记侧没有它，就是「会话级 · 未登记」。
//! - 表清单同样不缓存：展开时按 `database_name = alias` 现查 `duckdb_tables()` / `duckdb_views()`。
//! - **主源**（未限定名的解析者）也是意图：登记表只管"有哪些源"，主源单值另存
//!   `analysis_primary`（单行表，036）；会话里的 `search_path` 是运行态，由执行侧按意图去切。
//!
//! 迁移：`crates/store/migrations/project_meta/034_analysis_sources.sql`、
//! `036_analysis_primary.sql`；本模块另留 [`ensure_tables`] 作回退修复
//! （与 `network_two_layer` 同一种做法：迁移没跑也能读写）。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::sync::RwLock;

use shared::error::{CoreError, StorageError};

/// 连接 id → （登记别名, 挂载档） 的**进程级快照**（宿主在清单刷新时发布，执行侧读取）。
///
/// 为什么要有这一份：联邦计划的组装在**工作线程**上（`FederatedSourcePort::records`，
/// 不持 `Shared`、拿不到项目根），而登记表在项目库里。让执行侧每次去开项目库是错的
/// （工作线程不能 mutate UI 状态、也不该反复 open SQLite）；所以由面板在刷新清单时
/// 顺带发布一份只读快照（几十条，复制一次比每次查库便宜）。
///
/// 语义：**只是缓存**，权威仍是项目库；发布点必须跟着 [`list_in_project`] 走。
///
/// 带档位（2026-11-27，`use_duckdb_fed` 解耦）：渠道门控要回答"这条连接是不是被登记成
/// 本地加速源"，只存别名答不了。档位是登记行的一部分，跟着别名一起发布才不会有第二处来源。
static ALIASES_BY_CONN: std::sync::LazyLock<RwLock<HashMap<String, (String, AnalysisMountKind)>>> =
    std::sync::LazyLock::new(|| RwLock::new(HashMap::new()));

/// 发布「连接 id → (别名, 档)」快照（清空后重填：切项目必须整体换掉，不能留上个项目的键）。
pub fn publish_aliases(rows: &[AnalysisSourceRow]) {
    let mut map = ALIASES_BY_CONN
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map.clear();
    for row in rows {
        if let Some(conn_id) = row.connection_id.as_deref() {
            map.insert(
                conn_id.to_string(),
                (row.alias.clone(), row.mount_kind),
            );
        }
    }
}

/// 取某条连接在项目里登记的别名（没有登记 / 快照还没发布 → `None`）。
///
/// 调用方据此决定联邦挂载别名：有登记用它（SQL 引用名与面板/登记一致），
/// 没有就退回「连接名 sanitize」的旧规则（会话级源）。
pub fn alias_for_connection(conn_id: &str) -> Option<String> {
    ALIASES_BY_CONN
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(conn_id)
        .map(|(alias, _)| alias.clone())
}

/// 某条连接在项目里登记成哪一档（没有登记 → `None`）。
///
/// 渠道门控的唯一判据（2026-11-27 起不再看连接上的 `use_duckdb_fed`）：
/// 「本地加速」= 登记成 [`AnalysisMountKind::Accel`]；「联邦」= 登记成
/// `Federation` / `AttachFile` 或本文档绑定（能力另由驱动常量判）。
pub fn mount_kind_for_connection(conn_id: &str) -> Option<AnalysisMountKind> {
    ALIASES_BY_CONN
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(conn_id)
        .map(|(_, kind)| *kind)
}

/// 这条连接在**当前项目**里有没有登记（= 快照里有别名）。
///
/// 只对「登记行」作答（`DbFile` 那种没有 `connection_id` 的源天然不在快照里）；
/// 权威仍是项目库，快照由面板刷新 / 进项目时发布。
pub fn is_registered(conn_id: &str) -> bool {
    alias_for_connection(conn_id).is_some()
}

/// 项目登记的主源别名（**进程级快照**，与 [`ALIASES_BY_CONN`] 同一套发布口径）。
///
/// 为什么也要一份缓存：主源在**工作线程**上被读取（联邦计划组装要决定未限定名的解析者），
/// 那里拿不到项目根；缓存由面板刷新 / 进项目时发布（[`publish_project`]），权威仍是项目库。
static PRIMARY_ALIAS: std::sync::LazyLock<RwLock<Option<String>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

/// 当前打开的项目根（随 [`publish_project`] 更新）。
///
/// 为什么需要它：写主源意图的是**执行线程**（`FederatedSourcePort::remember_primary`），
/// 它拿不到 `Shared`（项目根在 UI 状态里），而项目库路径必须现算。**只在发布快照时写**，
/// 别在别处另设第二个来源。
static PROJECT_ROOT: std::sync::LazyLock<RwLock<Option<std::path::PathBuf>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

/// 当前打开的项目根（没有项目 → `None`）。
pub fn current_project_root() -> Option<std::path::PathBuf> {
    PROJECT_ROOT
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// 取项目登记的主源别名（没登记 / 快照还没发布 → `None`）。
pub fn primary_alias_cached() -> Option<String> {
    PRIMARY_ALIAS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn publish_primary(alias: Option<String>) {
    let mut slot = PRIMARY_ALIAS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = alias;
}

fn storage_err(op: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analysis_sources".to_string(),
        operation: op.to_string(),
        reason,
    })
}

/// 源的种类：连接（源库直查）还是数据库文件（`.duckdb` / `.sqlite` 挂载）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisSourceKind {
    /// 既有连接（凭据仍由连接体系持有，登记里只放 `connection_id`）。
    Conn,
    /// 数据库文件（本体路径放 `path`）。
    DbFile,
}

impl AnalysisSourceKind {
    pub const ALL: [Self; 2] = [Self::Conn, Self::DbFile];

    /// 落库值（迁移 034 的 `kind` 列）。
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Conn => "conn",
            Self::DbFile => "dbfile",
        }
    }

    /// 从库值解析：未知值返回 `None`（不猜、不静默改写，由上层决定怎么报）。
    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "conn" => Some(Self::Conn),
            "dbfile" => Some(Self::DbFile),
            _ => None,
        }
    }
}

/// 挂载方式：决定用哪条入口挂、卸载怎么卸、别名自不自由。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisMountKind {
    /// 联邦直查：`federation::session::ensure_session`，别名自由。
    Federation,
    /// 本地加速：`accel::ensure_on_instance`，别名固定 `rds_src`；同实例已有别的源占着就
    /// **报冲突失败**（2026-11-27 起不再让位成 `rds_src_2`；实际别名见 `alias_for_conn`）。
    Accel,
    /// 数据库文件挂载：`file_view::load_database_as` / `load_database_global`，别名自由。
    AttachFile,
}

impl AnalysisMountKind {
    pub const ALL: [Self; 3] = [Self::Federation, Self::Accel, Self::AttachFile];

    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Federation => "federation",
            Self::Accel => "accel",
            Self::AttachFile => "attach_file",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "federation" => Some(Self::Federation),
            "accel" => Some(Self::Accel),
            "attach_file" => Some(Self::AttachFile),
            _ => None,
        }
    }

    /// 给人看的档位名（门控原因、面板文案用；库里仍存 [`Self::as_db_str`]）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Federation => "联邦源",
            Self::Accel => "本地加速",
            Self::AttachFile => "文件库",
        }
    }

    /// 别名对本档是不是**自由**的：加速档的 ATTACH 别名是 `accel::SOURCE_ALIAS`（`rds_src`），
    /// 登记里的 `alias` 对它只是期望名/展示名。界面上要据此决定「能不能改名」。
    pub fn alias_is_free(self) -> bool {
        !matches!(self, Self::Accel)
    }
}

/// 一条登记（= SQLite 的一行）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AnalysisSourceRow {
    pub id: String,
    /// 展示名（连接名 / 文件名，**未经**归一化；别名另有 `alias`）。
    pub name: String,
    pub kind: AnalysisSourceKind,
    /// `kind = Conn` 时的连接 id；文件库为 `None`。
    pub connection_id: Option<String>,
    /// `kind = DbFile` 时的本体绝对路径；连接为 `None`。
    pub path: Option<String>,
    /// SQL 别名 = 挂载后 `duckdb_databases().database_name`。
    pub alias: String,
    pub mount_kind: AnalysisMountKind,
    pub created_at: String,
    pub updated_at: String,
}

/// 确保表存在（回退修复：迁移 034 / 036 没跑时也能读写；与 `network_two_layer` 同一做法）。
pub fn ensure_tables(conn: &Connection) -> Result<(), CoreError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS analysis_sources (
            id            TEXT PRIMARY KEY,
            name          TEXT NOT NULL,
            kind          TEXT NOT NULL,
            connection_id TEXT,
            path          TEXT,
            alias         TEXT NOT NULL,
            mount_kind    TEXT NOT NULL,
            created_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at    TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_analysis_sources_alias
            ON analysis_sources(alias)
            WHERE mount_kind <> 'accel';
        CREATE INDEX IF NOT EXISTS idx_analysis_sources_alias_accel
            ON analysis_sources(alias)
            WHERE mount_kind = 'accel';
        CREATE INDEX IF NOT EXISTS idx_analysis_sources_kind ON analysis_sources(kind);
        CREATE TABLE IF NOT EXISTS analysis_primary (
            singleton  INTEGER PRIMARY KEY CHECK (singleton = 1),
            alias      TEXT NOT NULL,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        );",
    )
    .map_err(|e| storage_err("ensure_tables", e.to_string()))
}

/// 读**某个项目**的登记表：`{project_root}/.RSmeta/project.db`，同步、一次性连接。
///
/// 读路径**不造项目骨架**：文件不在就返回空（还没建项目 / 还没写过），不在这里
/// `create_dir_all` 出一份假项目。面板每次刷新调一次——本地一次 open + 一次全表读，
/// 比多养一个连接池便宜，也不需要 tokio 运行时（UI 线程的事件路径可以直接调）。
pub fn list_in_project(
    project_root: &std::path::Path,
) -> Result<Vec<AnalysisSourceRow>, CoreError> {
    let path = project_root.join(".RSmeta").join("project.db");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open(&path).map_err(|e| storage_err("open_project_db", e.to_string()))?;
    list(&conn)
}

/// 读**某个项目**的登记表并把两张进程级快照一起发布：连接 → 别名、主源。
///
/// 进项目（`view::lifecycle`）与数据面板刷新都走它：执行侧（工作线程）只认这两份快照，
/// 少发布一次就会出现「登记了但执行侧看不见」/「主源设了但重启后回不到它」。
/// 读路径不造项目骨架（与 [`list_in_project`] 同一口径）。
pub fn publish_project(
    project_root: &std::path::Path,
) -> Result<Vec<AnalysisSourceRow>, CoreError> {
    if let Ok(mut slot) = PROJECT_ROOT.write() {
        *slot = Some(project_root.to_path_buf());
    }
    let path = project_root.join(".RSmeta").join("project.db");
    if !path.exists() {
        publish_aliases(&[]);
        publish_primary(None);
        return Ok(Vec::new());
    }
    let conn = Connection::open(&path).map_err(|e| storage_err("open_project_db", e.to_string()))?;
    let rows = list(&conn)?;
    let primary = primary(&conn)?;
    publish_aliases(&rows);
    publish_primary(primary);
    Ok(rows)
}

/// 关掉项目（没有项目了）：清空项目根与两张快照。
///
/// 不清就会拿**上一个项目**的别名 / 主源去开会话——切到空态时最容易踩。
pub fn clear_project() {
    if let Ok(mut slot) = PROJECT_ROOT.write() {
        *slot = None;
    }
    publish_aliases(&[]);
    publish_primary(None);
}

/// 项目登记的主源别名（权威读：项目库里的 `analysis_primary`）。
pub fn primary(conn: &Connection) -> Result<Option<String>, CoreError> {
    ensure_tables(conn)?;
    conn.query_row(
        "SELECT alias FROM analysis_primary WHERE singleton = 1",
        [],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| storage_err("primary", e.to_string()))
}

/// 设主源（未限定名从此在它里面解析）。
///
/// 只接受登记表里**非加速档**的别名：加速档的挂载名固定 `rds_src`，主源就是它自己，
/// 没有"选一个"这回事。主源是**意图**，写这里；会话里的 `search_path` 是运行态，由
/// 执行侧按这条意图去切（`catalog-context-model.md` §5）。
pub fn set_primary(conn: &Connection, alias: &str) -> Result<(), CoreError> {
    ensure_tables(conn)?;
    let mount_kind: Option<String> = conn
        .query_row(
            "SELECT mount_kind FROM analysis_sources WHERE alias = ?1 ORDER BY id LIMIT 1",
            params![alias],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| storage_err("set_primary_lookup", e.to_string()))?;
    let Some(mount_kind) = mount_kind else {
        return Err(storage_err(
            "set_primary",
            format!("登记里没有别名 {alias}：先把这条源登记为项目源，主源只能是登记过的源"),
        ));
    };
    if mount_kind == "accel" {
        return Err(storage_err(
            "set_primary",
            format!("{alias} 是加速档：挂载名固定 rds_src，主源就是它，不需要选"),
        ));
    }
    conn.execute(
        "INSERT INTO analysis_primary (singleton, alias) VALUES (1, ?1)
         ON CONFLICT(singleton) DO UPDATE SET alias = excluded.alias,
             updated_at = CURRENT_TIMESTAMP",
        params![alias],
    )
    .map(|_| ())
    .map_err(|e| storage_err("set_primary", e.to_string()))
}

/// 清掉主源意图（没设过是空操作）。
pub fn clear_primary(conn: &Connection) -> Result<(), CoreError> {
    ensure_tables(conn)?;
    conn.execute("DELETE FROM analysis_primary WHERE singleton = 1", [])
        .map(|_| ())
        .map_err(|e| storage_err("clear_primary", e.to_string()))
}


/// 打开**某个项目**的登记表（**写**路径）：`.RSmeta/project.db` 不在时建出骨架。
///
/// 与 [`list_in_project`] 不同：写登记是用户显式动作（右键「登记为项目源」），
/// 这一下没库就建库是对的——读路径才不该造骨架。
pub fn open_in_project(
    project_root: &std::path::Path,
) -> Result<Connection, CoreError> {
    let meta = project_root.join(".RSmeta");
    std::fs::create_dir_all(&meta).map_err(|e| storage_err("create_RSmeta", e.to_string()))?;
    let conn = Connection::open(meta.join("project.db"))
        .map_err(|e| storage_err("open_project_db", e.to_string()))?;
    ensure_tables(&conn)?;
    Ok(conn)
}

/// 全部登记行，按别名排序（面板树的顺序要稳定，不按写入时间抖）。
pub fn list(conn: &Connection) -> Result<Vec<AnalysisSourceRow>, CoreError> {
    ensure_tables(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, kind, connection_id, path, alias, mount_kind, created_at, updated_at
             FROM analysis_sources ORDER BY alias",
        )
        .map_err(|e| storage_err("list_prepare", e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(|e| storage_err("list_query", e.to_string()))?;

    let mut out = Vec::new();
    for row in rows {
        let (id, name, kind, connection_id, path, alias, mount_kind, created_at, updated_at) =
            row.map_err(|e| storage_err("list_row", e.to_string()))?;
        // 脏值（迁移前手工写坏 / 将来加了新档）不静默改写：跳过并记 warn，
        // 让面板少一行，而不是把一条读不懂的行硬塞进树里。
        let (Some(kind), Some(mount_kind)) = (
            AnalysisSourceKind::from_db_str(&kind),
            AnalysisMountKind::from_db_str(&mount_kind),
        ) else {
            tracing::warn!(
                kind = %kind,
                mount_kind = %mount_kind,
                alias = %alias,
                "analysis_sources 行有未知的 kind / mount_kind，已跳过"
            );
            continue;
        };
        out.push(AnalysisSourceRow {
            id,
            name,
            kind,
            connection_id,
            path,
            alias,
            mount_kind,
            created_at,
            updated_at,
        });
    }
    Ok(out)
}

/// 按 id 取一条。
pub fn find(conn: &Connection, id: &str) -> Result<Option<AnalysisSourceRow>, CoreError> {
    ensure_tables(conn)?;
    conn.query_row(
        "SELECT id, name, kind, connection_id, path, alias, mount_kind, created_at, updated_at
         FROM analysis_sources WHERE id = ?1",
        params![id],
        |row| {
            Ok(AnalysisSourceRow {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: AnalysisSourceKind::from_db_str(&row.get::<_, String>(2)?)
                    .unwrap_or(AnalysisSourceKind::Conn),
                connection_id: row.get(3)?,
                path: row.get(4)?,
                alias: row.get(5)?,
                mount_kind: AnalysisMountKind::from_db_str(&row.get::<_, String>(6)?)
                    .unwrap_or(AnalysisMountKind::Federation),
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(|e| storage_err("find", e.to_string()))
}

/// 已占用的别名（给"重名加 `_2`"用）。`except_id` 用来排除正在改名的那一行。
///
/// 加速档不算"占用"（035 起唯一性分域）：它的 ATTACH 别名固定 `rds_src`，
/// 表里的 `alias` 只是展示名——若把它算进来，第二条加速源的展示名会白白
/// 抢掉联邦/文件档想要的挂载名（`taken_aliases` 的调用方正是要挑挂载名）。
pub fn taken_aliases(conn: &Connection, except_id: Option<&str>) -> Result<Vec<String>, CoreError> {
    ensure_tables(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT alias FROM analysis_sources
             WHERE id IS NOT ?1 AND mount_kind <> 'accel'
             ORDER BY alias",
        )
        .map_err(|e| storage_err("taken_prepare", e.to_string()))?;
    let rows = stmt
        .query_map(params![except_id], |row| row.get::<_, String>(0))
        .map_err(|e| storage_err("taken_query", e.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("taken_row", e.to_string()))
}

/// 写一条登记。别名撞车（唯一索引）翻成人能看懂的错，不把 SQLite 的原文抛给界面。
pub fn insert(conn: &Connection, row: &AnalysisSourceRow) -> Result<(), CoreError> {
    ensure_tables(conn)?;
    // 不变量：连接档必须有 connection_id，文件档必须有 path。宁可在这里报，
    // 也别等到挂载时才发现 `ATTACH NULL`。
    match row.kind {
        AnalysisSourceKind::Conn if row.connection_id.as_deref().unwrap_or("").is_empty() => {
            return Err(storage_err(
                "insert",
                "conn 档的登记必须有 connection_id".to_string(),
            ))
        }
        AnalysisSourceKind::DbFile if row.path.as_deref().unwrap_or("").is_empty() => {
            return Err(storage_err(
                "insert",
                "dbfile 档的登记必须有 path".to_string(),
            ))
        }
        _ => {}
    }
    conn.execute(
        "INSERT INTO analysis_sources
            (id, name, kind, connection_id, path, alias, mount_kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            row.id,
            row.name,
            row.kind.as_db_str(),
            row.connection_id,
            row.path,
            row.alias,
            row.mount_kind.as_db_str(),
        ],
    )
    .map(|_| ())
    .map_err(|e| {
        if is_alias_conflict(&e) {
            storage_err("insert", format!("别名已被占用：{}", row.alias))
        } else {
            storage_err("insert", e.to_string())
        }
    })
}

/// 改别名（已挂载时上层要先 `DETACH` 再 `ATTACH`，并把"正在跑的查询会断"讲清楚）。
pub fn update_alias(conn: &Connection, id: &str, alias: &str) -> Result<bool, CoreError> {
    ensure_tables(conn)?;
    conn.execute(
        "UPDATE analysis_sources SET alias = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id, alias],
    )
    .map(|n| n > 0)
    .map_err(|e| {
        if is_alias_conflict(&e) {
            storage_err("update_alias", format!("别名已被占用：{alias}"))
        } else {
            storage_err("update_alias", e.to_string())
        }
    })
}

/// 删登记（**不碰实例**：卸载是运行态动作，由上层先 `DETACH` 再删行）。
///
/// 顺手清掉指向它的主源意图：主源指向一条不存在的登记就是空悬意图，下次执行会
/// 按它去切一个不存在的 catalog——比"回到文档连接"更糟。
pub fn delete(conn: &Connection, id: &str) -> Result<bool, CoreError> {
    ensure_tables(conn)?;
    let removed = conn
        .execute("DELETE FROM analysis_sources WHERE id = ?1", params![id])
        .map_err(|e| storage_err("delete", e.to_string()))?;
    conn.execute(
        "DELETE FROM analysis_primary
         WHERE singleton = 1
           AND alias NOT IN (SELECT alias FROM analysis_sources)",
        [],
    )
    .map_err(|e| storage_err("delete_primary", e.to_string()))?;
    Ok(removed > 0)
}

/// SQLite 的唯一约束冲突（`alias` 是唯一索引）。按错误文本认，不引入额外依赖。
fn is_alias_conflict(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(_, Some(message)) => {
            message.contains("idx_analysis_sources_alias")
                || (message.contains("UNIQUE") && message.contains("alias"))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().expect("memory db");
        ensure_tables(&c).expect("ensure tables");
        c
    }

    fn row(id: &str, alias: &str) -> AnalysisSourceRow {
        AnalysisSourceRow {
            id: id.to_string(),
            name: "商城库".to_string(),
            kind: AnalysisSourceKind::Conn,
            connection_id: Some("C_1".to_string()),
            path: None,
            alias: alias.to_string(),
            mount_kind: AnalysisMountKind::Federation,
            created_at: "2026-11-25T00:00:00Z".to_string(),
            updated_at: "2026-11-25T00:00:00Z".to_string(),
        }
    }

    /// 快照带档位（2026-11-27 `use_duckdb_fed` 解耦）：渠道门控要能问出「这条连接在
    /// 项目里登记成哪一档」，只存别名答不了；档位必须跟别名同源发布，不许第二处维护。
    #[test]
    fn alias_snapshot_carries_mount_kind() {
        let mut fed = row("s_1", "shop_src");
        fed.connection_id = Some("C_fed".to_string());
        fed.mount_kind = AnalysisMountKind::Federation;
        let mut accel = row("s_2", "rds_src");
        accel.connection_id = Some("C_accel".to_string());
        accel.mount_kind = AnalysisMountKind::Accel;
        publish_aliases(&[fed, accel]);

        assert_eq!(alias_for_connection("C_fed").as_deref(), Some("shop_src"));
        assert_eq!(
            mount_kind_for_connection("C_fed"),
            Some(AnalysisMountKind::Federation),
            "联邦档要读得回来"
        );
        assert_eq!(
            mount_kind_for_connection("C_accel"),
            Some(AnalysisMountKind::Accel),
            "加速档要读得回来"
        );
        assert_eq!(alias_for_connection("C_other"), None, "未登记就是没有");
        assert_eq!(mount_kind_for_connection("C_other"), None);

        publish_aliases(&[]); // 快照是进程级静态，别留给后面的测试
    }

    #[test]
    fn ensure_tables_is_idempotent() {
        let c = conn();
        ensure_tables(&c).expect("第二次 ensure 不该报错");
        assert!(list(&c).expect("list").is_empty());
    }

    #[test]
    fn insert_list_delete_roundtrip() {
        let c = conn();
        let mut b = row("s_2", "users_src");
        b.name = "用户库".to_string();
        b.connection_id = Some("C_2".to_string());
        b.mount_kind = AnalysisMountKind::Accel;
        insert(&c, &row("s_1", "shop_src")).expect("insert 1");
        insert(&c, &b).expect("insert 2");

        let all = list(&c).expect("list");
        assert_eq!(all.len(), 2);
        // 按别名排序：shop_src 在 users_src 前
        assert_eq!(all[0].alias, "shop_src");
        assert_eq!(all[1].mount_kind, AnalysisMountKind::Accel);
        assert!(!all[1].mount_kind.alias_is_free(), "加速档别名不自由");

        assert!(delete(&c, "s_1").expect("delete"));
        assert!(!delete(&c, "s_1").expect("再删一次返回 false"));
        assert_eq!(list(&c).expect("list").len(), 1);
    }

    /// 036：主源是**单值意图**（写进项目库，不是会话内存）——换了就只剩新的；
    /// 没登记过的别名不能当主源（主源只能是登记过的源）。
    #[test]
    fn primary_is_a_single_persisted_intent() {
        let c = conn();
        insert(&c, &row("s_1", "shop_src")).expect("insert 1");
        insert(&c, &row("s_2", "users_src")).expect("insert 2");
        assert_eq!(primary(&c).expect("primary"), None, "没设过就是没有");

        set_primary(&c, "shop_src").expect("设主源");
        assert_eq!(primary(&c).expect("primary").as_deref(), Some("shop_src"));

        set_primary(&c, "users_src").expect("换主源");
        assert_eq!(
            primary(&c).expect("primary").as_deref(),
            Some("users_src"),
            "单值：换了就不该还留着上一条"
        );

        let err = set_primary(&c, "nope_src").expect_err("没登记的别名不能当主源");
        assert!(
            format!("{err:?}").contains("nope_src"),
            "错误要点名那个别名，实际：{err:?}"
        );
    }

    /// 删掉主源那行 → 主源意图跟着清掉（不许留空悬的「解析到不存在的 catalog」）。
    #[test]
    fn deleting_the_primary_row_clears_the_intent() {
        let c = conn();
        insert(&c, &row("s_1", "shop_src")).expect("insert");
        set_primary(&c, "shop_src").expect("设主源");
        delete(&c, "s_1").expect("delete");
        assert_eq!(primary(&c).expect("primary"), None, "空悬主源必须清掉");
    }

    /// 加速档固定挂 `rds_src`，主源就是它自己，没有"选一个"这回事。
    #[test]
    fn accel_source_cannot_be_the_primary() {
        let c = conn();
        let mut accel = row("s_1", "rds_src");
        accel.mount_kind = AnalysisMountKind::Accel;
        insert(&c, &accel).expect("insert");
        let err = set_primary(&c, "rds_src").expect_err("加速档不该可选");
        assert!(format!("{err:?}").contains("accel") || format!("{err:?}").contains("rds_src"));
    }

    /// 035：唯一性按档位分域——加速档的 `alias` 只是展示名（ATTACH 固定 `rds_src`），
    /// 两条加速源可以同名共存，且**不占**联邦/文件档想要的挂载名。
    #[test]
    fn accel_alias_is_not_unique_and_does_not_reserve_the_name() {
        let c = conn();
        let mut a1 = row("s_1", "rds_src");
        a1.mount_kind = AnalysisMountKind::Accel;
        a1.connection_id = Some("C_1".to_string());
        let mut a2 = row("s_2", "rds_src");
        a2.mount_kind = AnalysisMountKind::Accel;
        a2.connection_id = Some("C_2".to_string());
        insert(&c, &a1).expect("第一条加速源");
        insert(&c, &a2).expect("第二条加速源同名必须也能登进来");

        let taken = taken_aliases(&c, None).expect("taken");
        assert!(taken.is_empty(), "加速档不该占用挂载名，实际：{taken:?}");

        // 同一时刻联邦档要叫 rds_src：这是它自己的挂载名，必须放行
        insert(&c, &row("s_3", "rds_src")).expect("联邦档用 rds_src 不该被加速档挡住");
        // 但联邦档之间仍然互斥
        let err = insert(&c, &row("s_4", "rds_src")).expect_err("联邦档撞车必须拒");
        assert!(format!("{err}").contains("别名已被占用"));
    }

    #[test]
    fn duplicate_alias_is_reported_as_alias_conflict() {
        let c = conn();
        insert(&c, &row("s_1", "shop_src")).expect("insert");
        let err = insert(&c, &row("s_2", "shop_src")).expect_err("别名撞车必须拒");
        let text = format!("{err}");
        assert!(text.contains("别名已被占用"), "实际错误：{text}");
    }

    #[test]
    fn update_alias_rejects_collision_and_keeps_old_value() {
        let c = conn();
        insert(&c, &row("s_1", "a_src")).expect("insert 1");
        insert(&c, &row("s_2", "b_src")).expect("insert 2");
        let err = update_alias(&c, "s_2", "a_src").expect_err("改名撞车必须拒");
        assert!(format!("{err}").contains("别名已被占用"));
        assert_eq!(find(&c, "s_2").expect("find").expect("必有").alias, "b_src");

        assert!(update_alias(&c, "s_2", "c_src").expect("改名"));
        assert_eq!(find(&c, "s_2").expect("find").expect("必有").alias, "c_src");
    }

    #[test]
    fn taken_aliases_excludes_the_row_being_renamed() {
        let c = conn();
        insert(&c, &row("s_1", "a_src")).expect("insert 1");
        insert(&c, &row("s_2", "b_src")).expect("insert 2");
        assert_eq!(
            taken_aliases(&c, Some("s_2")).expect("taken"),
            vec!["a_src".to_string()]
        );
        assert_eq!(
            taken_aliases(&c, None).expect("taken"),
            vec!["a_src".to_string(), "b_src".to_string()]
        );
    }

    #[test]
    fn kind_must_carry_its_own_payload() {
        let c = conn();
        let mut no_conn = row("s_1", "shop_src");
        no_conn.connection_id = None;
        let err = insert(&c, &no_conn).expect_err("conn 档缺 connection_id 必须拒");
        assert!(format!("{err}").contains("connection_id"));

        let file = AnalysisSourceRow {
            id: "s_2".to_string(),
            name: "data.duckdb".to_string(),
            kind: AnalysisSourceKind::DbFile,
            connection_id: None,
            path: None,
            alias: "data_duckdb".to_string(),
            mount_kind: AnalysisMountKind::AttachFile,
            created_at: "2026-11-25T00:00:00Z".to_string(),
            updated_at: "2026-11-25T00:00:00Z".to_string(),
        };
        let err = insert(&c, &file).expect_err("dbfile 档缺 path 必须拒");
        assert!(format!("{err}").contains("path"));

        let file = AnalysisSourceRow {
            path: Some("D:/data/123.duckdb".to_string()),
            ..file
        };
        insert(&c, &file).expect("补齐 path 后应能写");
        assert_eq!(list(&c).expect("list")[0].kind, AnalysisSourceKind::DbFile);
    }

    #[test]
    fn unknown_kind_rows_are_skipped_not_guessed() {
        let c = conn();
        c.execute(
            "INSERT INTO analysis_sources (id, name, kind, connection_id, alias, mount_kind)
             VALUES ('s_bad', '坏行', 'httpfs', 'C_1', 'bad_src', 'federation')",
            [],
        )
        .expect("直写到库");
        assert!(
            list(&c).expect("list").is_empty(),
            "未知 kind 不许被猜成 conn"
        );

        let c2 = conn();
        c2.execute(
            "INSERT INTO analysis_sources (id, name, kind, connection_id, alias, mount_kind)
             VALUES ('s_bad', '坏行', 'conn', 'C_1', 'bad_src', 'magic')",
            [],
        )
        .expect("直写到库");
        assert!(
            list(&c2).expect("list").is_empty(),
            "未知 mount_kind 也不许猜"
        );
    }

    /// 迁移 034 自己得能把表建出来——不能只靠 `ensure_tables` 兜底。
    #[test]
    fn migration_034_creates_the_table() {
        let dir = std::env::temp_dir().join(format!("rds_sources_mig_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let db_path = dir.join("project.db");
        let applied = crate::migration::MigrationManager::new()
            .migrate(&db_path, crate::migration::MigrationType::ProjectMeta)
            .expect("项目库迁移应能跑");
        let c = Connection::open(&db_path).expect("open project.db");

        // 先直查表在不在（list 会顺手 ensure_tables，用它验就测不出迁移）
        let n: i64 = c
            .query_row("SELECT count(*) FROM analysis_sources", [], |row| {
                row.get(0)
            })
            .expect("analysis_sources 应由迁移 034 建出");
        assert_eq!(n, 0);
        assert!(
            applied.iter().any(|m| m.version == 34),
            "034 必须在本次应用清单里：{:?}",
            applied.iter().map(|m| m.version).collect::<Vec<_>>()
        );

        // 迁移建的表与实际写入的列必须对得上
        let row = row("s_1", "shop_src");
        insert(&c, &row).expect("迁移建的表应能按同一套列写");
        assert_eq!(list(&c).expect("list").len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 项目根下的读路径：没有 `.RSmeta/project.db` 就是空清单（**不许**顺手建一个假项目），
    /// 有库就按别名读出来。
    #[test]
    fn list_in_project_reads_the_project_db_or_returns_empty() {
        let dir = std::env::temp_dir().join(format!("rds_sources_proj_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        // ① 没有项目库：空，且目录里不会凭空长出 .RSmeta
        assert!(list_in_project(&dir).expect("空项目应返回空").is_empty());
        assert!(
            !dir.join(".RSmeta").exists(),
            "读路径不许造项目骨架"
        );

        // ② 真有库：按别名读出来
        std::fs::create_dir_all(dir.join(".RSmeta")).expect("建目录");
        let db_path = dir.join(".RSmeta").join("project.db");
        {
            let c = Connection::open(&db_path).expect("open");
            ensure_tables(&c).expect("建表");
            insert(&c, &row("s_2", "zeta_src")).expect("写");
            insert(&c, &row("s_1", "alpha_src")).expect("写");
        }
        let rows = list_in_project(&dir).expect("应能读出来");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].alias, "alpha_src", "按别名排序");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
