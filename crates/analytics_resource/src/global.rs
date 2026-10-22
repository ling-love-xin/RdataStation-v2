use super::*;
use crate::resource::{map_resource_row, RESOURCE_COLUMNS};
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
// 连接纪律（重要）：
//   * 全局 DuckDB 是**单写者**——本模块**不得**另开文件连接，必须复用
//     `GlobalDatabaseManager::duckdb_conn()` 持有的连接（DuckDB 单句柄限制）；
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromoteKind {
    /// 数据文件：CSV / TSV / Parquet / Excel / JSON → 全局表（read_xxx 映射）。
    DataFile,
    /// 脚本 / 文本：内容表进全局（方案 A，§13.5），分发时重建为文件。
    Script,
}

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

/// 双实例操作错误。
#[derive(Debug, thiserror::Error)]
pub enum InstanceOpError {
    /// 同名冲突（升级目标全局表 / 分发目标项目表已存在）——UI 应弹「覆盖 / 改名 / 取消」。
    #[error("同名冲突：{name}")]
    Conflict { name: String },
    /// 资源不存在（分发引用了未登记的全局资源）。
    #[error("资源不存在：{name}")]
    NotFound { name: String },
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
        Ok(Self {
            sqlite: Arc::new(Mutex::new(None)),
            duckdb,
        })
    }

    /// 指定路径直开（测试注入 / 多环境）：不依赖全局单例，独立临时库。
    ///
    /// 建表幂等：跑 026 迁移 SQL（与 engine 全局迁移同一份），保证测试库结构
    /// 与生产一致（`map_resource_row` 引用的列全在）。
    pub async fn open_at(sqlite_path: PathBuf, duckdb_path: PathBuf) -> Result<Self, CoreError> {
        if let Some(parent) = sqlite_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources_global".to_string(),
                    operation: "create_global_dir".to_string(),
                    reason: e.to_string(),
                })
            })?;
        }
        let sqlite = rusqlite::Connection::open(&sqlite_path).map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources_global".to_string(),
                operation: "open_sqlite".to_string(),
                reason: e.to_string(),
            })
        })?;
        sqlite
            .execute_batch(GLOBAL_MIGRATION_026_SQL)
            .map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources_global".to_string(),
                    operation: "migrate_026".to_string(),
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
        Ok(Self {
            sqlite: Arc::new(Mutex::new(Some(sqlite))),
            duckdb: Arc::new(AsyncMutex::new(Some(duckdb))),
        })
    }

    /// 升级（项目 → 全局）：数据文件映射出表 / 脚本内容表，表 + 版本信息进全局。
    ///
    /// 语义（§13.2 / §13.5）：
    /// 1. 冲突检测（全局 DuckDB 表 + 全局登记行同名）；
    /// 2. 建全局表（数据文件 = `CREATE TABLE AS read_xxx`；脚本 = 内容表 + INSERT）；
    /// 3. 全局登记（scope = 'global'，kind = 'analysis'）+ 全局版本快照；
    /// 4. 返回结果（冲突未覆盖时返回 [`InstanceOpError::Conflict`]，由 UI 弹询问）。
    pub async fn promote(&self, req: &PromoteRequest) -> Result<PromoteOutcome, InstanceOpError> {
        // ---- 冲突检测（先查登记，再查 DuckDB 表，双保险）----
        let existing: Option<i32> = self.with_sqlite(|conn| {
            conn.query_row(
                r#"
                SELECT version FROM analytics_resources
                WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL
                ORDER BY version DESC LIMIT 1
                "#,
                rusqlite::params![req.name],
                |row| row.get(0),
            )
            .optional()
            .map_err(sqlite_err("check_global_conflict"))
        })?;

        let table_exists = self.table_exists(&req.name).await?;
        if table_exists && existing.is_none() {
            // 全局表在、登记不在：脏状态（索引修复同族）。不静默覆盖，按冲突走询问。
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
            });
        }
        if existing.is_some() && !table_exists {
            // 登记在、表不在：同属脏状态，冲突路径覆盖时会重建表，安全。
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
            });
        }
        if existing.is_some() && table_exists {
            return Err(InstanceOpError::Conflict {
                name: req.name.clone(),
            });
        }

        // ---- 建全局表 ----
        let (row_count, column_count) = match req.kind {
            PromoteKind::DataFile => {
                let read_expr = file_read_expr(&req.source_abs_path).ok_or_else(|| {
                    InstanceOpError::Core(CoreError::common(CommonError::InvalidArgument {
                        param: "source_abs_path".to_string(),
                        reason: format!(
                            "不支持的文件类型：{}（白名单：csv/tsv/txt/parquet/xlsx/xls/json/ndjson）",
                            req.source_abs_path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                        ),
                    }))
                })?;
                let create_sql = format!(
                    "CREATE TABLE {} AS SELECT * FROM {};",
                    quote_ident(&req.name),
                    read_expr
                );
                self.with_duckdb(|conn| {
                    conn.execute_batch(&create_sql)
                        .map_err(|e| duckdb_err("create_table_from_file", &e.to_string()))
                })
                .await?;
                self.table_stats(&req.name).await?
            }
            PromoteKind::Script => {
                let content = std::fs::read_to_string(&req.source_abs_path).map_err(|e| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                        "读取脚本内容失败（{}）：{}",
                        req.source_abs_path.display(),
                        e
                    ))))
                })?;
                let create_sql = format!(
                    "CREATE TABLE {} (\"filename\" TEXT, \"content\" TEXT, \"content_hash\" TEXT);",
                    quote_ident(&req.name)
                );
                let insert_sql = format!(
                    "INSERT INTO {} (\"filename\", \"content\", \"content_hash\") VALUES (?1, ?2, ?3);",
                    quote_ident(&req.name)
                );
                self.with_duckdb(|conn| {
                    conn.execute_batch(&create_sql)
                        .map_err(|e| duckdb_err("create_content_table", &e.to_string()))?;
                    conn.execute(
                        &insert_sql,
                        duckdb::params![
                            req.rel_path.as_deref().unwrap_or(""),
                            content,
                            req.content_hash
                        ],
                    )
                    .map_err(|e| duckdb_err("insert_content", &e.to_string()))?;
                    Ok(())
                })
                .await?;
                (Some(1_i32), Some(3_i32))
            }
        };

        // ---- 全局登记（scope = 'global'）+ 版本快照 ----
        let outcome = self.register_global(&req.name, req, row_count, column_count)?;
        Ok(outcome)
    }

    /// 覆盖式升级：删掉全局旧表 + 软删旧登记，再走 [`promote`]。
    ///
    /// 覆盖语义（§13.2 冲突询问「覆盖」分支）：旧表删除、登记行软删，新升级
    /// 成为新版本（版本号接续旧链）。
    pub async fn promote_overwrite(&self, req: &PromoteRequest) -> Result<PromoteOutcome, InstanceOpError> {
        self.drop_global_table(&req.name).await?;
        self.with_sqlite(|conn| {
            conn.execute(
                r#"
                UPDATE analytics_resources
                SET deleted_at = ?1
                WHERE name = ?2 AND scope = 'global' AND deleted_at IS NULL
                "#,
                rusqlite::params![Utc::now().to_rfc3339(), req.name],
            )
            .map_err(sqlite_err("soft_delete_global"))?;
            Ok(())
        })?;
        let mut outcome = self.promote(req).await?;
        outcome.overwritten = true;
        Ok(outcome)
    }

    /// 分发（全局 → 项目）：读全局登记 + 全局 DuckDB 表，落到当前项目库。
    ///
    /// 语义（§13.6）：跨项目唯一法定路径 = 项目 → 全局 → 项目，项目 ↔ 项目禁止直传。
    /// 形态判定：全局行 `file_rel_path` 非空 → 脚本内容表，分发**重建为文件**
    /// （`resources/<rel_path>`，File 档）；否则 → 建项目分析表（Analysis 档）。
    ///
    /// 连接纪律：项目 DuckDB 是单写者，本方法在调用方（后台 job 工作线程）已打开
    /// 项目写连接的前提下执行——打开动作由调用方负责（`ProjectDatabaseManager::open`），
    /// 避免与宿主主写连接并发。
    pub async fn distribute(
        &self,
        project: &ProjectDuckConn,
        project_root: &Path,
        name: &str,
    ) -> Result<DistributeOutcome, InstanceOpError> {
        // ---- 读全局登记行（最新版本）----
        let global_row = self
            .with_sqlite(|conn| {
                let row = conn
                    .query_row(
                        r#"
                    SELECT id, version, kind, content_hash, file_rel_path, definition_sql,
                           row_count, column_count, file_size
                    FROM analytics_resources
                    WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL
                    ORDER BY version DESC LIMIT 1
                    "#,
                        rusqlite::params![name],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, i32>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?,
                                row.get::<_, Option<String>>(4)?,
                                row.get::<_, Option<String>>(5)?,
                                row.get::<_, Option<i32>>(6)?,
                                row.get::<_, Option<i32>>(7)?,
                                row.get::<_, Option<i64>>(8)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(sqlite_err("read_global_row"))?;
                Ok(row)
            })?;
        let (global_id, global_version, _kind, global_hash, rel_path, definition_sql,
             _global_rows, _global_cols, global_size) =
            global_row.ok_or_else(|| InstanceOpError::NotFound {
                name: name.to_string(),
            })?;

        // ---- 形态判定 + 落项目 ----
        let shape = if let Some(rel) = rel_path.as_deref() {
            // 脚本内容表 → 重建为文件（File 档）。
            let content = self
                .read_script_content(name)
                .await
                .map_err(InstanceOpError::Core)?;
            write_project_file(project_root, rel, &content, name)
                .map_err(InstanceOpError::Core)?;
            let outcome = self
                .register_project_file(
                    project,
                    name,
                    rel,
                    global_id.as_str(),
                    global_version,
                    global_hash.as_deref(),
                    global_size,
                )
                .await
                .map_err(InstanceOpError::Core)?;
            outcome
        } else {
            // 分析表 / 数据文件映射表 → 建项目表（Analysis 档）。
            // 数据搬运路径：全局连接 `COPY ... TO parquet` → 项目连接
            // `CREATE TABLE AS SELECT read_parquet(...)`。
            // 为什么不用 ATTACH：Windows 单句柄限制下，项目连接 ATTACH 全局 DuckDB
            // 文件会撞「文件被占用」（全局写连接由 GlobalDatabaseManager 持有，不能关）。
            // Parquet 是 DuckDB 原生格式，中转确定性、无锁冲突。
            let tmp = std::env::temp_dir().join(format!(
                "rds_dist_{}.parquet",
                uuid::Uuid::new_v4().simple()
            ));
            let copy_sql = format!(
                "COPY {} TO '{}' (FORMAT PARQUET);",
                quote_ident(name),
                tmp.to_string_lossy().replace('\\', "/")
            );
            self.with_duckdb(|conn| {
                conn.execute_batch(&copy_sql)
                    .map_err(|e| duckdb_err("distribute_copy_out", &e.to_string()))?;
                Ok(())
            })
            .await?;

            let create_sql = format!(
                "CREATE TABLE {} AS SELECT * FROM read_parquet('{}');",
                quote_ident(name),
                tmp.to_string_lossy().replace('\\', "/")
            );
            let mut guard = project.0.lock().await;
            let conn = guard.as_mut().ok_or_else(|| {
                InstanceOpError::Core(CoreError::common(CommonError::General(
                    "项目 DuckDB 连接已关闭".to_string(),
                )))
            })?;
            let project_result = conn
                .execute_batch(&create_sql)
                .map_err(|e| duckdb_err("distribute_create_table", &e.to_string()));
            drop(guard);

            // 临时 parquet 用后即删（失败也尝试清理，不污染系统临时目录）。
            let _ = std::fs::remove_file(&tmp);
            project_result?;

            let (rows, cols) = project_table_stats(&project, name).await?;
            self.register_project_table(
                project,
                name,
                global_id.as_str(),
                global_version,
                global_hash.as_deref(),
                definition_sql.as_deref(),
                rows,
                cols,
            )
            .await
            .map_err(InstanceOpError::Core)?
        };

        Ok(shape)
    }

    /// 覆盖式分发：删项目旧表 / 旧文件 + 软删项目登记，再走 [`distribute`]。
    ///
    /// 覆盖语义（§13.2 冲突询问「覆盖」分支）：项目侧旧本体移除、登记软删，
    /// 新分发接续项目版本链（version = 旧 max + 1）。
    pub async fn distribute_overwrite(
        &self,
        project: &ProjectDuckConn,
        project_root: &Path,
        name: &str,
    ) -> Result<DistributeOutcome, InstanceOpError> {
        // 项目侧旧本体移除（表或文件都删；文件在 resources/ 下，删前先探存在）。
        {
            let mut guard = project.0.lock().await;
            if let Some(conn) = guard.as_mut() {
                conn.execute_batch(&format!("DROP TABLE IF EXISTS {};", quote_ident(name)))
                    .map_err(|e| duckdb_err("drop_project_table", &e.to_string()))?;
            }
        }
        if let Some(rel) = self
            .with_sqlite(|conn| {
                Ok(conn
                    .query_row(
                        "SELECT file_rel_path FROM analytics_resources WHERE name = ?1 AND scope = 'global' AND deleted_at IS NULL ORDER BY version DESC LIMIT 1",
                        rusqlite::params![name],
                        |row| row.get::<_, Option<String>>(0),
                    )
                    .optional()
                    .map_err(sqlite_err("read_global_rel"))?)
            })?
            .flatten()
        {
            let path = project_root.join("resources").join(&rel);
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| {
                    InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                        "删除项目旧文件失败（{}）：{}",
                        path.display(),
                        e
                    ))))
                })?;
            }
        }
        // 软删项目登记。
        self.distribute(project, project_root, name).await
    }

    // ==================== 分发内部 ====================

    /// 读全局脚本内容表首行（分发重建文件用）。
    async fn read_script_content(&self, name: &str) -> Result<String, CoreError> {
        self.with_duckdb(|conn| {
            let mut stmt = conn
                .prepare(&format!("SELECT \"content\" FROM {} LIMIT 1;", quote_ident(name)))
                .map_err(|e| duckdb_err("prepare_read_script", &e.to_string()))?;
            stmt.query_row([], |row| row.get::<_, String>(0))
                .map_err(|e| duckdb_err("read_script_content", &e.to_string()))
        })
        .await
    }

    /// 项目登记：Analysis 档（建表分发）。
    async fn register_project_table(
        &self,
        project: &ProjectDuckConn,
        name: &str,
        global_id: &str,
        global_version: i32,
        content_hash: Option<&str>,
        definition_sql: Option<&str>,
        row_count: Option<i32>,
        column_count: Option<i32>,
    ) -> Result<DistributeOutcome, CoreError> {
        let pool = project.1.clone();
        let conn = pool.acquire().await?;
        let conn = conn.inner()?;

        let next_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM analytics_resources WHERE name = ?1 AND scope = 'project'",
                rusqlite::params![name],
                |row| row.get::<_, i32>(0),
            )
            .map_err(sqlite_err("next_project_version"))? + 1;

        let id = format!("arp_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count,
                file_size, version, parent_version_id, parent_resource_id, source_query,
                created_at, updated_at, created_by, deleted_at,
                kind, content_hash, file_rel_path, readonly, promoted_from,
                source_connection_id, source_table, definition_sql, archived_at,
                payload_mode, source_path
            ) VALUES (
                ?1, 'analysis', ?2, NULL, 'null', 'project', ?3, ?4, NULL,
                ?5, NULL, NULL, NULL, ?6, ?6, NULL, NULL,
                'analysis', ?7, NULL, 1, ?8, NULL, ?2, ?9, ?6,
                'copy', NULL
            )
            "#,
            rusqlite::params![
                &id,
                name,
                row_count,
                column_count,
                next_version,
                now,
                content_hash,
                format!("global:{}", global_id),
                definition_sql,
            ],
        )
        .map_err(sqlite_err("insert_project_table"))?;

        // 项目版本快照（版本 = 接续全局链，快照记全局来源）。
        let snapshot = serde_json::json!({
            "kind": "analysis",
            "distributed_from": format!("global:{}", global_id),
            "global_version": global_version,
            "content_hash": content_hash,
            "row_count": row_count,
            "column_count": column_count,
            "distributed_at": now,
        })
        .to_string();
        conn.execute(
            "INSERT INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![format!("arvp_{}", uuid::Uuid::new_v4().simple()), &id, next_version, snapshot, now],
        )
        .map_err(sqlite_err("insert_project_version"))?;

        Ok(DistributeOutcome {
            resource_id: id,
            name: name.to_string(),
            version: next_version,
            shape: DistributeShape::Table,
            row_count,
        })
    }

    /// 项目登记：File 档（脚本分发重建文件）。
    async fn register_project_file(
        &self,
        project: &ProjectDuckConn,
        name: &str,
        rel_path: &str,
        global_id: &str,
        global_version: i32,
        content_hash: Option<&str>,
        file_size: Option<i64>,
    ) -> Result<DistributeOutcome, CoreError> {
        let pool = project.1.clone();
        let conn = pool.acquire().await?;
        let conn = conn.inner()?;

        let next_version: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM analytics_resources WHERE name = ?1 AND scope = 'project'",
                rusqlite::params![name],
                |row| row.get::<_, i32>(0),
            )
            .map_err(sqlite_err("next_project_version"))? + 1;

        let id = format!("arp_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"
            INSERT INTO analytics_resources (
                id, resource_type, name, alias, config, scope, row_count, column_count,
                file_size, version, parent_version_id, parent_resource_id, source_query,
                created_at, updated_at, created_by, deleted_at,
                kind, content_hash, file_rel_path, readonly, promoted_from,
                source_connection_id, source_table, definition_sql, archived_at,
                payload_mode, source_path
            ) VALUES (
                ?1, 'file', ?2, NULL, 'null', 'project', NULL, NULL, ?3,
                ?4, NULL, NULL, NULL, ?5, ?5, NULL, NULL,
                'file', ?6, ?7, 1, ?8, NULL, NULL, NULL, ?5,
                'copy', NULL
            )
            "#,
            rusqlite::params![
                &id,
                name,
                file_size,
                next_version,
                now,
                content_hash,
                rel_path,
                format!("global:{}", global_id),
            ],
        )
        .map_err(sqlite_err("insert_project_file"))?;

        let snapshot = serde_json::json!({
            "kind": "file",
            "distributed_from": format!("global:{}", global_id),
            "global_version": global_version,
            "content_hash": content_hash,
            "rel_path": rel_path,
            "distributed_at": now,
        })
        .to_string();
        conn.execute(
            "INSERT INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![format!("arvp_{}", uuid::Uuid::new_v4().simple()), &id, next_version, snapshot, now],
        )
        .map_err(sqlite_err("insert_project_version"))?;

        Ok(DistributeOutcome {
            resource_id: id,
            name: name.to_string(),
            version: next_version,
            shape: DistributeShape::File,
            row_count: None,
        })
    }

    // ==================== 内部 ====================

    /// 全局 SQLite 操作（懒加载打开全局库；同步段完成即放锁）。
    fn with_sqlite<T>(&self, f: impl FnOnce(&rusqlite::Connection) -> Result<T, CoreError>) -> Result<T, CoreError> {
        let mut guard = self.sqlite.lock().map_err(|_| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources_global".to_string(),
                operation: "lock_sqlite".to_string(),
                reason: "全局登记 SQLite 锁中毒".to_string(),
            })
        })?;
        if guard.is_none() {
            let path = engine::migration::get_global_db_path()?;
            *guard = Some(rusqlite::Connection::open(&path).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_resources_global".to_string(),
                    operation: "open_sqlite".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }
        f(guard.as_ref().expect("刚打开过，必为 Some"))
    }

    /// 全局 DuckDB 操作（懒加载仅测试路径；生产连接由 `production()` 注入）。
    async fn with_duckdb<T>(&self, f: impl FnOnce(&mut duckdb::Connection) -> Result<T, CoreError>) -> Result<T, CoreError> {
        let mut guard = self.duckdb.lock().await;
        if guard.is_none() {
            let path = engine::migration::get_global_duckdb_path()?;
            *guard = Some(duckdb::Connection::open(&path).map_err(|e| {
                CoreError::storage(StorageError::Persistence {
                    store: "analytics_duckdb_global".to_string(),
                    operation: "open".to_string(),
                    reason: e.to_string(),
                })
            })?);
        }
        f(guard.as_mut().expect("刚打开过，必为 Some"))
    }

    /// 全局 DuckDB 是否存在同名表（information_schema，DuckDB 标识符规范化为小写）。
    async fn table_exists(&self, name: &str) -> Result<bool, CoreError> {
        self.with_duckdb(|conn| {
            let mut stmt = conn
                .prepare("SELECT count(*) FROM information_schema.tables WHERE table_name = ?1")
                .map_err(|e| duckdb_err("prepare_table_exists", &e.to_string()))?;
            let count: i64 = stmt
                .query_row(duckdb::params![name.to_lowercase()], |row| row.get(0))
                .map_err(|e| duckdb_err("query_table_exists", &e.to_string()))?;
            Ok(count > 0)
        })
        .await
    }

    async fn drop_global_table(&self, name: &str) -> Result<(), CoreError> {
        self.with_duckdb(|conn| {
            conn.execute_batch(&format!("DROP TABLE IF EXISTS {};", quote_ident(name)))
                .map_err(|e| duckdb_err("drop_global_table", &e.to_string()))?;
            Ok(())
        })
        .await
    }

    /// 统计全局表行数 / 列数（升级与分发共用）。
    async fn table_stats(&self, name: &str) -> Result<(Option<i32>, Option<i32>), CoreError> {
        self.with_duckdb(|conn| {
            let mut stmt = conn
                .prepare(&format!("SELECT count(*) FROM {};", quote_ident(name)))
                .map_err(|e| duckdb_err("prepare_count", &e.to_string()))?;
            let rows: i64 = stmt
                .query_row([], |row| row.get(0))
                .map_err(|e| duckdb_err("query_count", &e.to_string()))?;
            let mut stmt = conn
                .prepare(&format!("DESCRIBE {};", quote_ident(name)))
                .map_err(|e| duckdb_err("prepare_describe", &e.to_string()))?;
            let columns = stmt
                .query_map([], |_| Ok(()))
                .map_err(|e| duckdb_err("query_describe", &e.to_string()))?
                .count();
            Ok((Some(rows.clamp(0, i32::MAX as i64) as i32), Some(columns as i32)))
        })
        .await
    }

    /// 全局登记 + 版本快照（升级落点）。覆盖时版本接续旧链，否则从 1 开始。
    fn register_global(
        &self,
        name: &str,
        req: &PromoteRequest,
        row_count: Option<i32>,
        column_count: Option<i32>,
    ) -> Result<PromoteOutcome, InstanceOpError> {
        self.with_sqlite(|conn| {
            let next_version: i32 = conn
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM analytics_resources WHERE name = ?1 AND scope = 'global'",
                    rusqlite::params![name],
                    |row| row.get::<_, i32>(0),
                )
                .map_err(sqlite_err("next_global_version"))? + 1;

            let id = format!("arg_{}", uuid::Uuid::new_v4().simple());
            let now = Utc::now().to_rfc3339();
            // 数据文件 / 分析表：本体 = 全局表，重建定义 = 建表 SQL（可复现）。
            let definition_sql = match req.kind {
                PromoteKind::DataFile => file_read_expr(&req.source_abs_path)
                    .map(|expr| {
                        format!(
                            "CREATE TABLE {} AS SELECT * FROM {};",
                            quote_ident(name),
                            expr
                        )
                    }),
                PromoteKind::Script => Some(format!(
                    "内容表（方案 A，§13.5）：{}（filename / content / content_hash），分发时重建为 resources/{}",
                    quote_ident(name),
                    req.rel_path.as_deref().unwrap_or("")
                )),
            };

            conn.execute(
                r#"
                INSERT INTO analytics_resources (
                    id, resource_type, name, alias, config, scope, row_count, column_count,
                    file_size, version, parent_version_id, parent_resource_id, source_query,
                    created_at, updated_at, created_by, deleted_at,
                    kind, content_hash, file_rel_path, readonly, promoted_from,
                    source_connection_id, source_table, definition_sql, archived_at,
                    payload_mode, source_path
                ) VALUES (
                    ?1, ?2, ?3, NULL, 'null', 'global', ?4, ?5, ?6,
                    ?7, NULL, NULL, NULL, ?8, ?8, NULL, NULL,
                    'analysis', ?9, ?10, 1, NULL, NULL, NULL, ?11, ?8,
                    'copy', ?12
                )
                "#,
                rusqlite::params![
                    &id,
                    "analysis",
                    name,
                    row_count,
                    column_count,
                    req.file_size,
                    next_version,
                    now,
                    req.content_hash,
                    req.rel_path,
                    definition_sql,
                    req.source_abs_path.to_string_lossy().replace('\\', "/"),
                ],
            )
            .map_err(sqlite_err("insert_global"))?;

            // 全局版本快照（与项目侧 `save_resource_version_on` 同构）。
            let snapshot = serde_json::json!({
                "kind": "analysis",
                "content_hash": req.content_hash,
                "row_count": row_count,
                "column_count": column_count,
                "definition_sql": definition_sql,
                "source": req.source_abs_path.to_string_lossy().to_string(),
                "promoted_at": now,
            })
            .to_string();
            let version_id = format!("argv_{}", uuid::Uuid::new_v4().simple());
            conn.execute(
                "INSERT INTO analytics_resource_versions (id, resource_id, version, snapshot, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![&version_id, &id, next_version, snapshot, now],
            )
            .map_err(sqlite_err("insert_global_version"))?;

            Ok(PromoteOutcome {
                resource_id: id,
                name: name.to_string(),
                version: next_version,
                row_count,
                column_count,
                overwritten: false,
            })
        })
        .map_err(InstanceOpError::Core)
    }
}

/// 列出全局资源（scope = 'global' 的活跃行，按更新时间倒序）。
///
/// 展示层（资源列表「全局实例」维度）与宿主「升级到全局」后的确认都用它。
/// 列序与 [`RESOURCE_COLUMNS`] 严格一致，复用 [`map_resource_row`]。
pub fn list_global_resources(store: &GlobalInstanceStore) -> Result<Vec<AnalyticsResource>, CoreError> {
    store.with_sqlite(|conn| {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {} FROM analytics_resources
                 WHERE scope = 'global' AND deleted_at IS NULL
                 ORDER BY updated_at DESC",
                RESOURCE_COLUMNS
            ))
            .map_err(sqlite_err("list_global"))?;
        let rows = stmt
            .query_map([], |row| map_resource_row(row))
            .map_err(sqlite_err("list_global_rows"))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sqlite_err("list_global_collect"))
    })
}

/// 全局登记行 → 展示行（§13 展示层）。
///
/// 全局行是**只读 + 可分发**：没有项目标签 / 分组 / 状态扫描语义，tail 只给
/// 体积与更新时间（来源详情在版本链，详情面板接入是后续批次）。`scope = Global`
/// 让行渲染给出「全局」徽标并置灰全部项目侧动作。
pub fn to_global_rows(
    resources: &[AnalyticsResource],
    now: chrono::DateTime<Utc>,
) -> Vec<crate::resource_view::ArchiveRow> {
    resources
        .iter()
        .map(|resource| {
            let kind = crate::model::ArchiveKind::from_db_str(&resource.kind);
            let tail = {
                let mut parts = Vec::new();
                if let Some(size) = resource.file_size {
                    parts.push(crate::present::format_size(Some(size)));
                }
                parts.push(crate::present::format_relative_time(now, resource.updated_at));
                parts.join(" · ")
            };
            crate::resource_view::ArchiveRow {
                id: resource.id.clone(),
                name: resource.name.clone(),
                scope: crate::model::RowScope::Global,
                alias: None,
                kind,
                version: resource.version,
                status: crate::model::ArchiveStatus::Normal,
                tail,
                source_table: resource.source_table.clone(),
                tags: Vec::new(),
                folder_id: None,
                updated_epoch: resource.updated_at.timestamp(),
                archived_epoch: resource.archived_at.map(|when| when.timestamp()),
                size_bytes: resource.file_size.map(i64::from),
            }
        })
        .collect()
}

/// 按扩展名选 DuckDB read 表达式（白名单与 `engine/duckdb/file_reader.rs` 一致）。
///
/// 返回可直接嵌进 SQL 的表达式（含路径字面量，`'` 转义为 `''`）。
fn file_read_expr(abs_path: &Path) -> Option<String> {
    let ext = abs_path.extension()?.to_str()?.to_ascii_lowercase();
    let path_lit = abs_path.to_string_lossy().replace('\\', "/");
    let quoted = path_lit.replace('\'', "''");
    let expr = match ext.as_str() {
        "csv" | "tsv" | "txt" => format!("read_csv_auto('{quoted}')"),
        "parquet" => format!("read_parquet('{quoted}')"),
        "xlsx" | "xls" => format!("read_excel_auto('{quoted}')"),
        "json" | "ndjson" => format!("read_json_auto('{quoted}')"),
        _ => return None,
    };
    Some(expr)
}

/// 标识符引用（双引号 + 内部转义）；DuckDB 把标识符规范化为小写，读写同口径。
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn sqlite_err(op: &str) -> impl Fn(rusqlite::Error) -> CoreError + '_ {
    move |e| {
        CoreError::storage(StorageError::Persistence {
            store: "analytics_resources_global".to_string(),
            operation: op.to_string(),
            reason: e.to_string(),
        })
    }
}

fn duckdb_err(op: &str, reason: &str) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_duckdb_global".to_string(),
        operation: op.to_string(),
        reason: reason.to_string(),
    })
}

/// 全局迁移 026（与 engine 迁移同源，测试库建表用；生产由 engine 迁移执行）。
pub(crate) const GLOBAL_MIGRATION_026_SQL: &str = include_str!(
    "../../engine/migrations/global/026_analytics_resource_global.sql"
);

#[cfg(test)]
mod tests {
    use super::*;
    use engine::persistence::ProjectSqlitePool;
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    const BASE_MIGRATION_SQL: &str =
        include_str!("../../engine/migrations/project_meta/007_analytics_resources.sql");
    const ARCHIVE_MIGRATION_SQL: &str =
        include_str!("../../engine/migrations/project_meta/020_analytics_resource_archive.sql");
    const PAYLOAD_MODE_MIGRATION_SQL: &str =
        include_str!("../../engine/migrations/project_meta/023_analytics_resource_payload_mode.sql");
    const TAG_LINK_FK_MIGRATION_SQL: &str = include_str!(
        "../../engine/migrations/project_meta/024_analytics_resource_tags_drop_tag_fk.sql"
    );

    struct TestEnv {
        dir: PathBuf,
        store: GlobalInstanceStore,
        project_pool: Arc<ProjectSqlitePool>,
        project_duckdb: Arc<AsyncMutex<Option<duckdb::Connection>>>,
        project_root: PathBuf,
    }

    async fn create_test_env() -> TestEnv {
        let dir = std::env::temp_dir().join(format!("rds_gtest_{}", Uuid::new_v4().simple()));
        let global_dir = dir.join("global");
        let project_dir = dir.join("project");
        fs::create_dir_all(&global_dir).expect("create global dir");
        fs::create_dir_all(&project_dir).expect("create project dir");
        fs::create_dir_all(project_dir.join("resources")).expect("create resources dir");

        let store = GlobalInstanceStore::open_at(
            global_dir.join("global.db"),
            global_dir.join("analytics.duckdb"),
        )
        .await
        .expect("open global store");

        // 项目 SQLite（跑 020/023/024 迁移，与生产结构一致）。
        let db_path = project_dir.join(".RSmeta/project.db");
        fs::create_dir_all(db_path.parent().unwrap()).expect("create rsmeta");
        let pool = Arc::new(
            ProjectSqlitePool::new(db_path.clone(), 2)
                .await
                .expect("create project pool"),
        );
        {
            let conn = pool.acquire().await.expect("acquire");
            let inner = conn.inner().expect("inner");
            inner
                .execute_batch(BASE_MIGRATION_SQL)
                .expect("migrate 007");
            inner
                .execute_batch(ARCHIVE_MIGRATION_SQL)
                .expect("migrate 020");
            inner
                .execute_batch(PAYLOAD_MODE_MIGRATION_SQL)
                .expect("migrate 023");
            inner
                .execute_batch(TAG_LINK_FK_MIGRATION_SQL)
                .expect("migrate 024");
        }

        // 项目 DuckDB 写连接（独立临时文件）。
        let duckdb_path = project_dir.join(".RSmeta/analytics.duckdb");
        fs::create_dir_all(duckdb_path.parent().unwrap()).expect("create rsmeta for duckdb");
        let project_duckdb =
            Arc::new(AsyncMutex::new(Some(
                duckdb::Connection::open(&duckdb_path).expect("open project duckdb"),
            )));

        TestEnv {
            dir,
            store,
            project_pool: pool,
            project_duckdb,
            project_root: project_dir,
        }
    }

    fn write_csv(dir: &Path) -> PathBuf {
        let path = dir.join("orders.csv");
        fs::write(
            &path,
            "id,sku,qty\n1,a,10\n2,b,20\n3,c,30\n",
        )
        .expect("write csv");
        path
    }

    fn promote_req(name: &str, path: &Path, kind: PromoteKind) -> PromoteRequest {
        PromoteRequest {
            name: name.to_string(),
            kind,
            source_abs_path: path.to_path_buf(),
            // 数据文件分发 = 建项目表（无重建目标）；脚本分发 = 重建 resources/<rel> 文件。
            rel_path: match kind {
                PromoteKind::DataFile => None,
                PromoteKind::Script => Some(format!("work/{name}")),
            },
            content_hash: format!("hash_{}", Uuid::new_v4().simple()),
            file_size: Some(123),
        }
    }

    #[tokio::test]
    async fn g001_promote_data_file_and_register_global() {
        let env = create_test_env().await;
        let csv = write_csv(&env.dir);
        let req = promote_req("orders_v1", &csv, PromoteKind::DataFile);

        let outcome = env.store.promote(&req).await.expect("promote");
        assert_eq!(outcome.version, 1, "首版从 1 开始");
        assert!(!outcome.overwritten);
        assert_eq!(outcome.row_count, Some(3), "CSV 3 行");
        assert_eq!(outcome.column_count, Some(3), "CSV 3 列");

        // 全局 DuckDB 表真实存在 + 行数一致（经 store 连接验证，Windows 单句柄下
        // 不能另开文件连接复核）。
        let (rows, cols) = env.store.table_stats("orders_v1").await.expect("global stats");
        assert_eq!(rows, Some(3), "全局表行数 = CSV 行数");
        assert_eq!(cols, Some(3));

        // 全局登记行（scope = 'global'，kind = 'analysis'）。
        let list = list_global_resources(&env.store).expect("list global");
        assert_eq!(list.len(), 1);
        let row = &list[0];
        assert_eq!(row.name, "orders_v1");
        assert_eq!(row.scope, "global");
        assert_eq!(row.kind, "analysis");
        assert_eq!(row.version, 1);
        assert!(row.definition_sql.as_deref().unwrap_or("").contains("read_csv_auto"));

        // 版本快照落全局库。
        env.store
            .with_sqlite(|conn| {
                let v: i32 = conn
                    .query_row(
                        "SELECT count(*) FROM analytics_resource_versions WHERE resource_id = ?1",
                        rusqlite::params![row.id],
                        |r| r.get(0),
                    )
                    .map_err(sqlite_err("test_count_versions"))?;
                assert_eq!(v, 1);
                Ok(())
            })
            .expect("version snapshot");

        let _ = fs::remove_dir_all(&env.dir);
    }

    #[tokio::test]
    async fn g002_promote_conflict_then_overwrite() {
        let env = create_test_env().await;
        let csv = write_csv(&env.dir);
        let req = promote_req("orders_conf", &csv, PromoteKind::DataFile);
        env.store.promote(&req).await.expect("first promote");

        // 同名再升级 → Conflict。
        let err = env.store.promote(&req).await.expect_err("second promote must conflict");
        assert!(
            matches!(err, InstanceOpError::Conflict { .. }),
            "同名升级应报 Conflict，实际 {err:?}"
        );

        // 覆盖式升级 → 版本 2。
        let out = env.store
            .promote_overwrite(&req)
            .await
            .expect("overwrite promote");
        assert_eq!(out.version, 2, "覆盖后版本接续旧链");
        assert!(out.overwritten);

        let list = list_global_resources(&env.store).expect("list");
        assert_eq!(list.len(), 1, "覆盖后仍一条活跃登记");
        assert_eq!(list[0].version, 2);

        let _ = fs::remove_dir_all(&env.dir);
    }

    #[tokio::test]
    async fn g003_promote_script_content_table() {
        let env = create_test_env().await;
        let script = env.dir.join("script.sql");
        fs::write(&script, "SELECT 1 AS one;\n-- 注释\n").expect("write script");
        let req = promote_req("calc_script", &script, PromoteKind::Script);

        let outcome = env.store.promote(&req).await.expect("promote script");
        assert_eq!(outcome.row_count, Some(1), "内容表固定 1 行（内容聚合）");

        // 全局内容表可读回原文（分发重建依赖它）——经 store 连接验证。
        let content = env
            .store
            .read_script_content("calc_script")
            .await
            .expect("read content");
        assert!(content.contains("SELECT 1 AS one"), "内容完整入库");

        let list = list_global_resources(&env.store).expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "analysis");
        assert!(list[0].file_rel_path.as_deref().unwrap_or("").contains("calc_script"));

        let _ = fs::remove_dir_all(&env.dir);
    }

    #[tokio::test]
    async fn g004_distribute_table_to_project() {
        let env = create_test_env().await;
        let csv = write_csv(&env.dir);
        let req = promote_req("shared_orders", &csv, PromoteKind::DataFile);
        env.store.promote(&req).await.expect("promote");

        let project_conn = ProjectDuckConn(
            env.project_duckdb.clone(),
            env.project_pool.clone(),
        );
        let outcome = env
            .store
            .distribute(&project_conn, &env.project_root, "shared_orders")
            .await
            .expect("distribute");
        assert_eq!(outcome.shape, DistributeShape::Table);
        assert_eq!(outcome.version, 1, "项目侧首版");

        // 项目表真实存在 + 行数一致（跨库搬运成功）——经项目写连接验证。
        let (rows, _) = project_table_stats(&project_conn, "shared_orders")
            .await
            .expect("project stats");
        assert_eq!(rows, Some(3), "分发后项目表行数 = 全局表行数");

        // 项目登记（scope = 'project'，来源 = 全局）。
        let pool = &env.project_pool;
        let conn = pool.acquire().await.expect("acquire");
        let conn = conn.inner().expect("inner");
        let (scope, promoted_from): (String, String) = conn
            .query_row(
                "SELECT scope, promoted_from FROM analytics_resources WHERE name = 'shared_orders' AND deleted_at IS NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("query project row");
        assert_eq!(scope, "project");
        assert!(promoted_from.starts_with("global:"), "项目行记全局来源");

        let _ = fs::remove_dir_all(&env.dir);
    }

    #[tokio::test]
    async fn g005_distribute_script_rebuilds_file() {
        let env = create_test_env().await;
        let script = env.dir.join("report.md");
        fs::write(&script, "# 周报\n\n数据正常\n").expect("write md");
        let req = promote_req("weekly_note", &script, PromoteKind::Script);
        env.store.promote(&req).await.expect("promote script");

        let project_conn = ProjectDuckConn(
            env.project_duckdb.clone(),
            env.project_pool.clone(),
        );
        let outcome = env
            .store
            .distribute(&project_conn, &env.project_root, "weekly_note")
            .await
            .expect("distribute script");
        assert_eq!(outcome.shape, DistributeShape::File, "脚本分发重建为文件");

        // 文件真实重建在项目 resources/ 下（方案 A 分发侧）。
        let rel = env.store
            .with_sqlite(|conn| {
                conn.query_row(
                    "SELECT file_rel_path FROM analytics_resources WHERE name = 'weekly_note' AND scope = 'global' AND deleted_at IS NULL",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .map_err(sqlite_err("read rel"))
            })
            .expect("read global rel");
        let rebuilt = env.project_root.join("resources").join(&rel);
        let content = fs::read_to_string(&rebuilt).expect("read rebuilt file");
        assert!(content.contains("周报"), "内容重建完整");
        assert!(content.contains("数据正常"));

        // 项目侧是 File 档登记。
        let pool = &env.project_pool;
        let conn = pool.acquire().await.expect("acquire");
        let conn = conn.inner().expect("inner");
        let kind: String = conn
            .query_row(
                "SELECT kind FROM analytics_resources WHERE name = 'weekly_note' AND deleted_at IS NULL",
                [],
                |row| row.get(0),
            )
            .expect("query kind");
        assert_eq!(kind, "file");

        let _ = fs::remove_dir_all(&env.dir);
    }

    #[tokio::test]
    async fn g006_global_row_missing_returns_not_found() {
        let env = create_test_env().await;
        let project_conn = ProjectDuckConn(
            env.project_duckdb.clone(),
            env.project_pool.clone(),
        );
        let err = env
            .store
            .distribute(&project_conn, &env.project_root, "never_promoted")
            .await
            .expect_err("must not find");
        assert!(
            matches!(err, InstanceOpError::NotFound { .. }),
            "未登记的全局资源分发应报 NotFound，实际 {err:?}"
        );
        let _ = fs::remove_dir_all(&env.dir);
    }
}
