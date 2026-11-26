use super::*;
use crate::store::resource::{RESOURCE_COLUMNS, map_resource_row};
use chrono::Utc;
use rusqlite::OptionalExtension;
use shared::error::{CommonError, CoreError, StorageError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

// =====================================================================
// M6 双实例（原型 §13）：项目 → 全局升级 / 全局 → 项目分发
//
// 拓扑（2026-10-22 用户锁定）：
//   * 全局分析库 `{system}/analytics.duckdb`（连接前缀 G_）+ 全局登记
//     `{system}/global.db`（engine 全局迁移 026 建表，scope = 'global'）；
//   * 项目分析库 `{项目}/.RSmeta/analytics.duckdb` + 项目登记
//     （project_meta 020/023/024，scope = 'project'）；
//   * 升级 = 表 + 版本信息进全局 DuckDB；跨项目唯一法定路径 = 项目 → 全局 → 项目，
//     项目 ↔ 项目禁止直传。
//
// 连接纪律（重要，2026-11-26 所有权收口后）：
//   * 全局 / 项目分析库共用进程级 DuckDB 根连接（`shared::duckdb_root`），
//     `rds_global` / `analytics` 两个 ATTACH 别名同根；本模块不另开文件连接，
//     只借全局会话句柄执行 SQL（DuckDB 单句柄限制）；
//   * 全局 SQLite 复用 `TagTable::open_global` 先例（直接打开文件；WAL 下低频写
//     可并发）；测试注入独立临时路径，不碰产品数据根。
// =====================================================================

/// 全局实例存储（全局 SQLite 登记 + 全局 DuckDB 表读写）。
///
/// 连接懒加载：`sqlite` 首次操作时按 `get_global_db_path()` 打开（或 `open_at`
/// 注入的路径）；`duckdb` 由 `production()` 从 `GlobalDatabaseManager` 接管，
/// 测试由 `open_at` 直开临时文件。
#[derive(Clone)]
pub struct GlobalInstanceStore {
    sqlite: Arc<Mutex<Option<rusqlite::Connection>>>,
    duckdb: Arc<AsyncMutex<Option<duckdb::Connection>>>,
    /// 全局系统目录（`data/system`）：文件资产本体与历史副本都落在它下面。
    system_dir: PathBuf,
}

/// 升级请求（项目 → 全局）。
#[derive(Debug, Clone)]
pub struct PromoteRequest {
    /// 升级后的显示名 = 全局 DuckDB 表名（冲突检测 / 分发定位都靠它）。
    pub name: String,
    /// 本体种类：数据文件（映射出表）/ 脚本（内容表）。
    pub kind: PromoteKind,
    /// 本体绝对路径（数据文件 / 脚本在项目 `resources/` 下的位置）。
    pub source_abs_path: PathBuf,
    /// 项目侧相对路径（脚本内容表分发时重建为目标文件；数据文件无此概念）。
    pub rel_path: Option<String>,
    /// 内容指纹（sha256）。
    pub content_hash: String,
    /// 本体大小（字节）。
    pub file_size: Option<i64>,
    /// 来源：项目侧那条存档的 id（**A 的 id**）。
    ///
    /// 跨项目只允许「项目 A → 全局 → 项目 B」，所以全局行要能回指被搬走的那条 A 存档。
    /// 早期调用点可以给 `None`（那时没有这两项），全局行照旧不记来源。
    pub source_resource_id: Option<String>,
    /// 来源：A 的版本号。**各层版本各自编号**——A 的 v5 升级成全局的 v1，
    /// 这里记的是被搬走的那一版 A，不是全局的版本号。
    pub source_version: Option<i32>,
    /// 本体是项目分析库里的**一张表**时给表名（表型存档升级）。
    ///
    /// `Some` 时这份请求不由全局连接直接建表：表在项目库里，必须由项目自己的
    /// 写连接整表导出（Windows 单句柄，全局连接 `ATTACH` 项目库会撞「文件被占用」，
    /// 与分发侧同一条限制）。这时 `source_abs_path` 指的是项目分析库文件，
    /// 只作说明与日志用——真正搬运在 `jobs::global::promote_table_from_project`。
    pub source_table: Option<String>,
    /// 本次升级的历史版本表保留策略（`None` = 不裁剪）。
    ///
    /// 一版一张表，全局库随版本数线性增长；这里给 `Some(KeepVersions)` 时，
    /// 登记成功后按同名链裁掉更早版本的表（**只删内容表，登记与版本快照都留**）。
    /// 由宿主的设置项驱动（`ResourceHostBackend::keep_versions`）。
    pub keep_versions: Option<crate::model::KeepVersions>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromoteKind {
    /// 数据文件：CSV / TSV / Parquet / Excel / JSON → 全局表（read_xxx 映射）。
    DataFile,
    /// 脚本 / 文本：内容表进全局（方案 A，§13.5），分发时重建为文件。
    Script,
}

/// 脚本型资源提升到全局的体积上限（16 MiB）。
///
/// 脚本在全局库里是一张 `filename/content/content_hash` 三列表的**单行 TEXT**：
/// 它承载的是定义而不是数据，16 MiB 已是正常脚本（几百 KB）的几十倍。超过就拒绝，
/// 并指引按数据文件归档——那条路走 `CREATE TABLE AS read_xxx`，不进进程内存。
pub const MAX_SCRIPT_BYTES: u64 = 16 * 1024 * 1024;

/// 升级结果。
#[derive(Debug, Clone)]
pub struct PromoteOutcome {
    pub resource_id: String,
    pub name: String,
    pub version: i32,
    pub row_count: Option<i32>,
    pub column_count: Option<i32>,
    /// true = 同名冲突经「覆盖」处理（旧表已删、版本接续）；false = 全新升级。
    pub overwritten: bool,
}

/// 分发结果（全局 → 项目）。
#[derive(Debug, Clone)]
pub struct DistributeOutcome {
    pub resource_id: String,
    pub name: String,
    pub version: i32,
    /// 分发形态：表（Analysis 档，建项目表）/ 文件（File 档，脚本重建）。
    pub shape: DistributeShape,
    pub row_count: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributeShape {
    Table,
    File,
}

/// 一条分发回执（全局侧视角：这个全局存档被哪个项目用过、用到第几版）。
///
/// 项目侧登记只回答「我来自哪」；「我被谁用过」只能由全局侧记，见迁移 033。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchRecord {
    /// 目标项目根（规范化：`\` → `/`）。
    pub project_root: String,
    /// 目标侧名（改名分发时与全局名不同）。
    pub target_name: String,
    /// 最近一次分发时的全局版本。
    pub version: i32,
    /// 分发形态。
    pub shape: DistributeShape,
    /// 累计分发次数（同一目标重复覆盖分发会累加，不另起行）。
    pub dispatch_count: i64,
    pub first_distributed_at: String,
    pub last_distributed_at: String,
}

/// 一个全局存档的**使用面**（回执按全局名聚合后的结果，供详情区显示一行摘要）。
///
/// 只回答四个值：几个项目在用、最近分发到第几版、累计分发多少次、最近一次什么时候。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DispatchUsage {
    /// 用它的项目数（回执行数，同一项目的改名分发算不同目标）。
    pub project_count: usize,
    /// 最近一次分发时的全局版本。
    pub latest_version: i32,
    /// 累计分发次数。
    pub total_dispatches: i64,
    /// 最近一次分发时间（SQLite 原始串 `YYYY-MM-DD HH:MM:SS`）。
    pub last_distributed_at: String,
}

/// 冲突撞在目标实例的**哪一层**（三查的结论：登记 / 物理表 / 原件）。
///
/// 这个结论必须带进冲突框：三查都可能命中，但用户要做的事不一样——
/// 撞在登记层是同名存档（可改名；覆盖要软删登记），撞在表层是库里已有同名表
/// （改名也要看表名是否被别的东西用着），撞在文件层是 `resources/` 下已有同名原件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictField {
    /// SQLite 登记表里已有同名（`deleted_at IS NULL`）。
    Registry,
    /// 目标 DuckDB 里已有同名物理表。
    Table,
    /// 目标项目 `resources/` 下已有同名原件（脚本档分发）。
    File,
    /// 登记与物理表都在（升级侧双保险同时命中）。
    RegistryAndTable,
}

impl ConflictField {
    /// 中文层名（冲突框的一句话说明用；不含"撞在"这类状语，调用方拼）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Registry => "登记表",
            Self::Table => "数据表",
            Self::File => "本体文件",
            Self::RegistryAndTable => "登记表和数据表",
        }
    }
}

/// 双实例操作错误。
#[derive(Debug, thiserror::Error)]
pub enum InstanceOpError {
    /// 同名冲突（升级目标全局表 / 分发目标项目表已存在）——UI 应弹「覆盖 / 改名 / 取消」。
    #[error("同名冲突：{name}（撞在{}）", field.label())]
    Conflict {
        name: String,
        /// 撞在哪一层：让冲突框说清"是登记、是表、还是文件"，而不是笼统一句"已有同名"。
        field: ConflictField,
    },
    /// 资源不存在（分发引用了未登记的全局资源）。
    #[error("资源不存在：{name}")]
    NotFound { name: String },
    /// 全局资产仍被项目引用（按版本 pin），不能整条删除。
    #[error("全局资产「{name}」仍有 {refs} 个项目引用版本在使用，不能删除；先固化或解除引用")]
    Referenced { name: String, refs: usize },
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// 项目侧写连接句柄（分发落项目用）：项目 DuckDB 写连接 + 项目 SQLite 池。
///
/// 由调用方（workbench 后台 job）先 `ProjectDatabaseManager::open(project_root)` 取得
/// 并传入——本模块不自行打开项目写连接（DuckDB 单写者，避免与宿主主写连接并发）。
pub struct ProjectDuckConn(
    pub Arc<AsyncMutex<Option<duckdb::Connection>>>,
    pub Arc<engine::persistence::ProjectSqlitePool>,
);

/// 把脚本内容重建为项目 `resources/<rel>` 文件（§13.5 方案 A 分发侧）。
///
/// 目录自动创建；文件写入后即归档本体（File 档 copy 语义）。
pub fn write_project_file(
    project_root: &Path,
    rel: &str,
    content: &str,
    name: &str,
) -> Result<(), CoreError> {
    let target = project_root.join("resources").join(rel);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources".to_string(),
                operation: "create_resources_dir".to_string(),
                reason: format!("{}: {}", target.display(), e),
            })
        })?;
    }
    std::fs::write(&target, content).map_err(|e| {
        CoreError::storage(StorageError::Persistence {
            store: "analytics_resources".to_string(),
            operation: "write_project_file".to_string(),
            reason: format!("{}（分发「{name}」）: {}", target.display(), e),
        })
    })?;
    Ok(())
}

/// 项目侧重名双保险探测（§13.6 冲突询问）：SQLite 未删登记 + DuckDB 表 + resources 文件，
/// 任一命中即视为冲突。与升级侧 `promote` 的"登记层 + 本体层"口径一致，不静默覆盖。
///
/// 返回**命中哪一层**（`None` = 没撞），让冲突框能说清是登记、表还是文件。
///
/// `rel_path` 只用于脚本档文件存在性探测（文件路径固定、不随改名变化）；分析表档传 `None`。
async fn project_name_taken(
    project: &ProjectDuckConn,
    project_root: &Path,
    name: &str,
    rel_path: Option<&str>,
) -> Result<Option<ConflictField>, InstanceOpError> {
    // 1. SQLite 登记层（scope = 'project'，未软删）。
    let pool = project.1.clone();
    let conn = pool.acquire().await.map_err(InstanceOpError::Core)?;
    let inner = conn.inner().map_err(InstanceOpError::Core)?;
    let registered: i64 = inner
        .query_row(
            "SELECT count(*) FROM analytics_resources \
             WHERE name = ?1 AND scope = 'project' AND deleted_at IS NULL",
            rusqlite::params![name],
            |row| row.get(0),
        )
        .map_err(sqlite_err("check_project_conflict"))?;
    if registered > 0 {
        return Ok(Some(ConflictField::Registry));
    }

    // 2. DuckDB 表存在（分析表档；标识符规范化为小写，与 `table_exists` 同口径）。
    let mut guard = project.0.lock().await;
    let Some(duck) = guard.as_mut() else {
        return Err(InstanceOpError::Core(CoreError::common(
            CommonError::General("项目 DuckDB 连接已关闭".to_string()),
        )));
    };
    let table_exists = duck
        .prepare("SELECT count(*) FROM information_schema.tables WHERE table_name = ?1")
        .map_err(|e| duckdb_err("prepare_project_table_exists", &e.to_string()))?
        .query_row(duckdb::params![name.to_lowercase()], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| duckdb_err("query_project_table_exists", &e.to_string()))?
        > 0;
    drop(guard);
    if table_exists {
        return Ok(Some(ConflictField::Table));
    }

    // 3. resources 文件存在（脚本档；路径固定，不随显示名变化）。
    if let Some(rel) = rel_path {
        if project_root.join("resources").join(rel).is_file() {
            return Ok(Some(ConflictField::File));
        }
    }
    Ok(None)
}

/// 项目 DuckDB 表行数 / 列数统计（分发建表后）。
async fn project_table_stats(
    project: &ProjectDuckConn,
    name: &str,
) -> Result<(Option<i32>, Option<i32>), CoreError> {
    let mut guard = project.0.lock().await;
    let conn = guard.as_mut().ok_or_else(|| {
        CoreError::common(CommonError::General("项目 DuckDB 连接已关闭".to_string()))
    })?;
    let mut stmt = conn
        .prepare(&format!("SELECT count(*) FROM {};", quote_ident(name)))
        .map_err(|e| duckdb_err("prepare_project_count", &e.to_string()))?;
    let rows: i64 = stmt
        .query_row([], |row| row.get(0))
        .map_err(|e| duckdb_err("query_project_count", &e.to_string()))?;
    let mut stmt = conn
        .prepare(&format!("DESCRIBE {};", quote_ident(name)))
        .map_err(|e| duckdb_err("prepare_project_describe", &e.to_string()))?;
    let columns = stmt
        .query_map([], |_| Ok(()))
        .map_err(|e| duckdb_err("query_project_describe", &e.to_string()))?
        .count();
    Ok((
        Some(rows.clamp(0, i32::MAX as i64) as i32),
        Some(columns as i32),
    ))
}

impl GlobalInstanceStore {
    /// 生产入口：全局 DuckDB 复用 `GlobalDatabaseManager` 持有的写连接。
    ///
    /// 调用前提：`engine::migration::initialize_global_system()` 已执行
    /// （workbench 启动即做，见 `app/main.rs`）；未初始化时返回明确错误，不静默降级。
    pub async fn production() -> Result<Self, CoreError> {
        let manager = engine::migration::get_global_db_manager().ok_or_else(|| {
            CoreError::common(CommonError::General(
                "全局系统库未初始化（initialize_global_system 未调用）".to_string(),
            ))
        })?;
        let duckdb = manager.duckdb_conn().acquire().await?;
        let system_dir = engine::migration::get_system_dir()?;
        Ok(Self {
            sqlite: Arc::new(Mutex::new(None)),
            duckdb,
            system_dir,
        })
    }

    /// 指定路径直开（测试注入 / 多环境）：不依赖全局单例，独立临时库。
    ///
    /// 建表幂等：跑 026 迁移 SQL（与 engine 全局迁移同一份），保证测试库结构
    /// 与生产一致（`map_resource_row` 引用的列全在）。
    pub async fn open_at(sqlite_path: PathBuf, duckdb_path: PathBuf) -> Result<Self, CoreError> {
        let sqlite = open_registry(&sqlite_path)?;
        // 格式守门（2026-11-01）：DuckDB 能打开 SQLite 格式的同名文件（内置 SQLite 存储后端），
        // 所以必须先断格式，不能只靠 open 成功与否判断。
        shared::duckdb_open::assert_native_format(&duckdb_path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_duckdb_global".to_string(),
                operation: "assert_native_format".to_string(),
                reason: e.to_string(),
            })
        })?;
        let duckdb = duckdb::Connection::open(&duckdb_path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_duckdb_global".to_string(),
                operation: "open".to_string(),
                reason: e.to_string(),
            })
        })?;
        let system_dir = sqlite_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            sqlite: Arc::new(Mutex::new(Some(sqlite))),
            duckdb: Arc::new(AsyncMutex::new(Some(duckdb))),
            system_dir,
        })
    }

    /// 测试专用：登记库走临时 SQLite，DuckDB 用调用方给的会话。
    ///
    /// 同根改造后 M6 测试必须让全局库与项目库挂在同一条根连接上；调用方用
    /// `shared::duckdb_root::isolated` 建根，再把 `handle(GLOBAL_ALIAS)` 传进来。
    #[cfg(test)]
    pub(crate) fn open_at_handle(
        sqlite_path: PathBuf,
        duckdb: duckdb::Connection,
    ) -> Result<Self, CoreError> {
        let sqlite = open_registry(&sqlite_path)?;
        let system_dir = sqlite_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self {
            sqlite: Arc::new(Mutex::new(Some(sqlite))),
            duckdb: Arc::new(AsyncMutex::new(Some(duckdb))),
            system_dir,
        })
    }
}

/// 打开全局登记库并跑全局迁移。
fn open_registry(sqlite_path: &Path) -> Result<rusqlite::Connection, CoreError> {
    if let Some(parent) = sqlite_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources_global".to_string(),
                operation: "create_global_dir".to_string(),
                reason: e.to_string(),
            })
        })?;
    }
    let sqlite = rusqlite::Connection::open(sqlite_path).map_err(|e| {
        CoreError::storage(StorageError::Persistence {
            store: "analytics_resources_global".to_string(),
            operation: "open_sqlite".to_string(),
            reason: e.to_string(),
        })
    })?;
    // 全局库建表 SQL 统一从 `schema_sql` 取：此前 026/033/034 三段在这里手抄，
    // 加一个迁移就得记着补这一处，见 `store::schema_sql` 的注释。
    sqlite
        .execute_batch(super::store::schema_sql::GLOBAL_MIGRATIONS_SQL)
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources_global".to_string(),
                operation: "migrate_global_schema".to_string(),
                reason: e.to_string(),
            })
        })?;
    Ok(sqlite)
}

pub mod delete;
pub mod distribute;
pub mod list;
mod payload;
pub mod promote;
pub mod refs;
pub mod register;
pub mod sql;
pub mod table_name;

pub use self::delete::{
    GlobalDeleteOutcome, GlobalRestoreOutcome, GlobalTrashEntry, GlobalTrashOutcome,
};
pub use self::list::{list_global_resources, to_global_rows};
pub use self::refs::{GlobalRefOutcome, GlobalRefPin};
pub(crate) use self::sql::{duckdb_err, file_read_expr, quote_ident, sqlite_err};

#[cfg(test)]
mod tests;
