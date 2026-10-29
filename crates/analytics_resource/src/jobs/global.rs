//! 全局实例域（拆分自 jobs.rs）：升级到全局 / 从全局分发。
//!
//! 项目侧写连接：分发需要项目 DuckDB 写锁，在工作线程里 `ProjectDatabaseManager::open`
//! （动作完成后连接即释放，不与宿主主写连接长存并发）。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use engine::persistence::project_db::ProjectDatabaseManager;

use super::{jobs, ConflictContext, Job, OpOutcome, SQLITE_POOL_SIZE};

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
    let outcome = match job.overwrite {
        true => gstore.promote_overwrite(&job.request).await,
        false => gstore.promote(&job.request).await,
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(InstanceOpError::Conflict { name }) => {
            return OpOutcome::Conflict {
                context: ConflictContext::Promote(job.request.clone()),
                name,
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
        true => gstore.distribute_overwrite(
            &project,
            &job.project_root,
            &job.name,
            job.target.as_deref(),
        )
        .await,
        false => gstore.distribute(
            &project,
            &job.project_root,
            &job.name,
            job.target.as_deref(),
        )
        .await,
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
        Err(InstanceOpError::Conflict { name }) => OpOutcome::Conflict {
            context: ConflictContext::Distribute {
                global_name: job.name.clone(),
                target: job.target.clone(),
            },
            name,
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
    let _ = jobs()
        .tx
        .send(Job::Distribute(DistributeJob {
            project_root,
            name,
            target,
            overwrite,
        }));
}
