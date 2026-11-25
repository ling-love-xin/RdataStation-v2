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
//!
//! 迁移：`crates/store/migrations/project_meta/034_analysis_sources.sql`；本模块另留
//! [`ensure_tables`] 作回退修复（与 `network_two_layer` 同一种做法：迁移没跑也能读写）。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use specta::Type;

use shared::error::{CoreError, StorageError};

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
    /// 本地加速：`accel::ensure_session`，**ATTACH 别名是固定 `rds_src`**（不按源命名）。
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

/// 确保表存在（回退修复：迁移 034 没跑时也能读写；与 `network_two_layer` 同一做法）。
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
        CREATE UNIQUE INDEX IF NOT EXISTS idx_analysis_sources_alias ON analysis_sources(alias);
        CREATE INDEX IF NOT EXISTS idx_analysis_sources_kind ON analysis_sources(kind);",
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
pub fn taken_aliases(conn: &Connection, except_id: Option<&str>) -> Result<Vec<String>, CoreError> {
    ensure_tables(conn)?;
    let mut stmt = conn
        .prepare("SELECT alias FROM analysis_sources WHERE id IS NOT ?1 ORDER BY alias")
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
pub fn delete(conn: &Connection, id: &str) -> Result<bool, CoreError> {
    ensure_tables(conn)?;
    conn.execute("DELETE FROM analysis_sources WHERE id = ?1", params![id])
        .map(|n| n > 0)
        .map_err(|e| storage_err("delete", e.to_string()))
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
