//! 全局实例域（拆分自 jobs.rs）：升级到全局 / 从全局分发。
//!
//! 项目侧写连接：分发需要项目 DuckDB 写锁，在工作线程里 `ProjectDatabaseManager::open`
//! （动作完成后连接即释放，不与宿主主写连接长存并发）。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use engine::persistence::project_db::ProjectDatabaseManager;

use shared::error::{CommonError, CoreError};

use super::{ConflictContext, Job, OpOutcome, SQLITE_POOL_SIZE, jobs};

/// 一次「升级到全局」任务（§13：项目 → 全局，表 + 版本信息进全局 DuckDB）。
pub(crate) struct PromoteJob {
    pub(crate) project_root: PathBuf,
    pub(crate) request: crate::global::PromoteRequest,
    /// true = 同名冲突经「覆盖」确认后重入队（旧表删、版本接续）。
    pub(crate) overwrite: bool,
}

/// 一次「从全局分发」任务（§13：全局 → 项目；跨项目唯一法定路径）。
pub(crate) struct DistributeJob {
    pub(crate) project_root: PathBuf,
    pub(crate) name: String,
    /// 项目侧落库名（冲突询问「改名」分支填入；None = 沿用全局名）。
    pub(crate) target: Option<String>,
    /// true = 同名冲突经「覆盖」确认后重入队（旧本体移除、登记软删）。
    pub(crate) overwrite: bool,
}

/// 一次「从全局实例删除」任务（刀 B；只清全局这一份，已分发副本不动）。
pub(crate) struct DeleteGlobalJob {
    pub(crate) project_root: PathBuf,
    pub(crate) name: String,
}

/// 删除全局资产（工作线程上执行）。
pub(crate) async fn run_delete_global(job: &DeleteGlobalJob) -> OpOutcome {
    use crate::global::GlobalInstanceStore;

    let gstore = match GlobalInstanceStore::production().await {
        Ok(store) => store,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "删除全局资产",
                reason: format!("打开全局系统库失败：{reason}"),
            };
        }
    };
    match gstore.delete_global(&job.name).await {
        Ok(outcome) => OpOutcome::GlobalDeleted {
            name: outcome.name,
            versions: outcome.versions,
            tables: outcome.tables,
            projects: outcome.projects,
        },
        Err(reason) => OpOutcome::Failed {
            action: "删除全局资产",
            reason: reason.to_string(),
        },
    }
}

/// 升级到全局（工作线程上执行；§13）。
///
/// 本体 = `{项目}/resources/<payload_rel_path>`（File 档）；`content_hash` 用
/// 面板详情已算好的指纹（归档时算过 sha256），不在 worker 里重算。
/// 形态判定：白名单扩展名（csv/tsv/txt/parquet/xlsx/xls/json/ndjson）→ 数据文件
/// 映射出表；其余（sql/py/md 等）→ 脚本内容表（方案 A）。
pub(crate) async fn run_promote(job: &PromoteJob) -> OpOutcome {
    use crate::global::{GlobalInstanceStore, InstanceOpError};

    let gstore = match GlobalInstanceStore::production().await {
        Ok(store) => store,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "升级到全局",
                reason: format!("打开全局系统库失败：{reason}"),
            };
        }
    };
    let outcome = if job.request.source_table.is_some() {
        // 表型：本体是项目分析库里的一张表。必须由项目自己的写连接整表导出，
        // 再把导出的 parquet 按数据文件升级（见 `promote_table_from_project`）。
        match open_project_conn(&job.project_root).await {
            Ok(project) => {
                promote_table_from_project(&gstore, &project, &job.request, job.overwrite).await
            }
            Err(reason) => Err(InstanceOpError::Core(CoreError::common(
                CommonError::General(reason),
            ))),
        }
    } else {
        match job.overwrite {
            true => gstore.promote_overwrite(&job.request).await,
            false => gstore.promote(&job.request).await,
        }
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(InstanceOpError::Conflict { name, field }) => {
            return OpOutcome::Conflict {
                context: ConflictContext::Promote(job.request.clone()),
                name,
                field,
            };
        }
        Err(reason) => {
            return OpOutcome::Failed {
                action: "升级到全局",
                reason: reason.to_string(),
            };
        }
    };
    OpOutcome::Promoted {
        name: outcome.name,
        version: outcome.version,
    }
}

/// 打开项目库并借一个写连接（表型升级要把项目表整表导出，必须由项目自己的连接做）。
async fn open_project_conn(
    project_root: &std::path::Path,
) -> Result<crate::global::ProjectDuckConn, String> {
    let manager = ProjectDatabaseManager::open(project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|reason| format!("打开项目库失败：{reason}"))?;
    let Some(duckdb) = manager.duckdb_conn() else {
        return Err("项目 DuckDB 写连接未就绪".to_string());
    };
    let duck_conn = duckdb.acquire().await.map_err(|reason| reason.to_string())?;
    Ok(crate::global::ProjectDuckConn(duck_conn, manager.sqlite_pool()))
}

/// 表型升级：项目表 → 临时 parquet → 全局表（**整表搬运**）。
///
/// 为什么要绕一步 parquet：全局 DuckDB 与项目 DuckDB 都是单写者，Windows 单句柄下
/// 互相 `ATTACH` 会撞「文件被占用」（分发侧已踩过，见 `distribute` 的注释）。
/// 项目连接自己 `COPY ... TO` 是安全的，而且 **`COPY` 不带 LIMIT**——整表搬，
/// 不采样：资产库要跨项目比版本，不允许「这次只有前一万行」。
///
/// 半成品清理：`COPY` 失败留下的是半截 parquet，升级失败留下的是已建的全局表
/// （`promote` 内部按分支清），这里统一负责临时文件那一步。
pub(crate) async fn promote_table_from_project(
    gstore: &crate::global::GlobalInstanceStore,
    project: &crate::global::ProjectDuckConn,
    req: &crate::global::PromoteRequest,
    overwrite: bool,
) -> Result<crate::global::PromoteOutcome, crate::global::InstanceOpError> {
    use crate::global::{InstanceOpError, PromoteKind, quote_ident};

    let Some(table) = req.source_table.as_deref() else {
        return Err(InstanceOpError::Core(CoreError::common(
            CommonError::General("升级请求缺少源表名".to_string()),
        )));
    };

    let tmp_dir = paths::temp_dir();
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| {
            InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                "创建临时目录失败（{}）：{e}",
                tmp_dir.display()
            ))))
        })?;
    let tmp = tmp_dir.join(format!(
        "rds_promote_{}.parquet",
        uuid::Uuid::new_v4().simple()
    ));
    let copy_sql = format!(
        "COPY {} TO '{}' (FORMAT PARQUET);",
        quote_ident(table),
        tmp.to_string_lossy().replace('\\', "/")
    );

    let copy_result = {
        let mut guard = project.0.lock().await;
        match guard.as_mut() {
            Some(conn) => conn.execute_batch(&copy_sql).map_err(|e| {
                InstanceOpError::Core(CoreError::common(CommonError::General(format!(
                    "导出项目表 {table} 失败：{e}"
                ))))
            }),
            None => Err(InstanceOpError::Core(CoreError::common(
                CommonError::General("项目 DuckDB 连接已关闭".to_string()),
            ))),
        }
    };
    if let Err(error) = copy_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(error);
    }

    // 导出的 parquet 就是「数据文件」：形态判定、全量建表、指纹都不必另走一套。
    let mut effective = req.clone();
    effective.kind = PromoteKind::DataFile;
    effective.source_abs_path = tmp.clone();
    // 表已经搬成文件了：源表名与重建目标都不再需要。
    effective.source_table = None;
    effective.rel_path = None;

    let result = match overwrite {
        true => gstore.promote_overwrite(&effective).await,
        false => gstore.promote(&effective).await,
    };
    // 全局表建好之后这份 parquet 就没用了（数据已在全局库里）。
    let _ = std::fs::remove_file(&tmp);
    result
}

/// 从全局分发到当前项目（工作线程上执行；§13 跨项目唯一法定路径）。
///
/// 项目侧写连接：分发需要项目 DuckDB 写锁，按 M6 后台任务惯例在工作线程里
/// `ProjectDatabaseManager::open`（动作完成后连接即释放，不与宿主主写连接长存并发）。
pub(crate) async fn run_distribute(job: &DistributeJob) -> OpOutcome {
    use crate::global::{DistributeShape, GlobalInstanceStore, InstanceOpError, ProjectDuckConn};

    let gstore = match GlobalInstanceStore::production().await {
        Ok(store) => store,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "从全局分发",
                reason: format!("打开全局系统库失败：{reason}"),
            };
        }
    };
    let manager = match ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE).await {
        Ok(manager) => manager,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "从全局分发",
                reason: format!("打开项目库失败：{reason}"),
            };
        }
    };
    let Some(duckdb) = manager.duckdb_conn() else {
        return OpOutcome::Failed {
            action: "从全局分发",
            reason: "项目 DuckDB 写连接未就绪".to_string(),
        };
    };
    let duck_conn = match duckdb.acquire().await {
        Ok(conn) => conn,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "从全局分发",
                reason: reason.to_string(),
            };
        }
    };
    let project = ProjectDuckConn(duck_conn, manager.sqlite_pool());
    let result = match job.overwrite {
        true => {
            gstore
                .distribute_overwrite(
                    &project,
                    &job.project_root,
                    &job.name,
                    job.target.as_deref(),
                )
                .await
        }
        false => {
            gstore
                .distribute(
                    &project,
                    &job.project_root,
                    &job.name,
                    job.target.as_deref(),
                )
                .await
        }
    };
    match result {
        Ok(outcome) => OpOutcome::Distributed {
            name: outcome.name,
            version: outcome.version,
            shape: match outcome.shape {
                DistributeShape::Table => DistributeShape::Table,
                DistributeShape::File => DistributeShape::File,
            },
        },
        Err(InstanceOpError::Conflict { name, field }) => OpOutcome::Conflict {
            context: ConflictContext::Distribute {
                global_name: job.name.clone(),
                target: job.target.clone(),
            },
            name,
            field,
        },
        Err(InstanceOpError::NotFound { name }) => OpOutcome::Failed {
            action: "从全局分发",
            reason: format!("全局实例中没有「{name}」"),
        },
        Err(reason) => OpOutcome::Failed {
            action: "从全局分发",
            reason: reason.to_string(),
        },
    }
}

/// 提交一次「升级到全局」（§13；事件路径调用：右键菜单确认之后）。
///
/// `overwrite = true` 走覆盖式升级（冲突询问「覆盖」确认后重入队）。
pub fn enqueue_promote(
    project_root: PathBuf,
    request: crate::global::PromoteRequest,
    overwrite: bool,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Promote(PromoteJob {
        project_root,
        request,
        overwrite,
    }));
}

/// 提交一次「从全局分发」（§13；事件路径调用：全局实例列表动作之后）。
///
/// `target` = 项目侧落库名（None = 沿用全局名；冲突询问「改名」分支填入）；
/// `overwrite = true` 走覆盖式分发（冲突询问「覆盖」确认后重入队）。
pub fn enqueue_distribute(
    project_root: PathBuf,
    name: String,
    target: Option<String>,
    overwrite: bool,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Distribute(DistributeJob {
        project_root,
        name,
        target,
        overwrite,
    }));
}

/// 提交一次「从全局实例删除」（刀 B；确认框之后）。
pub fn enqueue_delete_global(project_root: PathBuf, name: String) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::DeleteGlobal(DeleteGlobalJob {
        project_root,
        name,
    }));
}
