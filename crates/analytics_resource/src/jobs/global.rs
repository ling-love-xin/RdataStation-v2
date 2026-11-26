//! 全局实例域（拆分自 jobs.rs）：升级到全局 / 从全局分发。
//!
//! 项目侧写连接：分发需要项目 DuckDB 写锁，在工作线程里 `ProjectDatabaseManager::open`
//! （动作完成后连接即释放，不与宿主主写连接长存并发）。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use engine::persistence::project_db::ProjectDatabaseManager;

use shared::error::{CommonError, CoreError};

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

/// 一次「从全局实例删除」任务（刀 B；只清全局这一份，已分发副本不动）。
pub(crate) struct DeleteGlobalJob {
    pub(crate) project_root: PathBuf,
    pub(crate) name: String,
}

/// 一次「引用到本项目」任务（全局 → 项目引用资产；不落数据）。
pub(crate) struct ReferenceGlobalJob {
    pub(crate) project_root: PathBuf,
    pub(crate) name: String,
    /// 引用的全局版本（列表/详情当前看到的那一版）。
    pub(crate) global_version: i32,
    /// 项目内别名（None = 不设）。
    pub(crate) alias: Option<String>,
}

/// 一次「固化引用版本」任务（引用资产 vN → 独立本地表资产）。
pub(crate) struct SolidifyRefJob {
    pub(crate) project_root: PathBuf,
    pub(crate) resource_id: String,
    pub(crate) project_version: i32,
    /// 固化成的新本地资产名 / 物理表名（调用方保证唯一，冲突走 Conflict）。
    pub(crate) target: String,
}

/// 一次「恢复全局历史版本」任务：表型复制旧表，文件型复制旧副本，都生成新版本。
pub(crate) struct RestoreGlobalVersionJob {
    pub(crate) project_root: PathBuf,
    pub(crate) name: String,
    /// 要恢复的历史版本号。
    pub(crate) version: i32,
    /// true = 表型（`restore_global_table_version`），false = 文件型。
    pub(crate) table: bool,
}

/// 把全局资产移入回收站（工作线程上执行）。
pub(crate) async fn run_delete_global(job: &DeleteGlobalJob) -> OpOutcome {
    use crate::global::GlobalInstanceStore;

    let gstore = match GlobalInstanceStore::production().await {
        Ok(store) => store,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "移入全局回收站",
                reason: format!("打开全局系统库失败：{reason}"),
            };
        }
    };
    match gstore.trash_global(&job.name).await {
        Ok(outcome) => OpOutcome::GlobalTrashed {
            name: outcome.name,
            version: outcome.version,
        },
        Err(reason) => OpOutcome::Failed {
            action: "移入全局回收站",
            reason: reason.to_string(),
        },
    }
}

/// 从全局历史版本恢复（工作线程上执行）：表型复制旧表，文件型校验旧副本。
pub(crate) async fn run_restore_global_version(job: &RestoreGlobalVersionJob) -> OpOutcome {
    use crate::global::GlobalInstanceStore;

    let action = if job.table {
        "恢复全局表版本"
    } else {
        "恢复全局文件版本"
    };
    let gstore = match GlobalInstanceStore::production().await {
        Ok(store) => store,
        Err(reason) => {
            return OpOutcome::Failed {
                action,
                reason: format!("打开全局系统库失败：{reason}"),
            };
        }
    };
    let result = if job.table {
        gstore.restore_global_table_version(&job.name, job.version).await
    } else {
        gstore.restore_global_file_version(&job.name, job.version).await
    };
    match result {
        Ok(outcome) => OpOutcome::GlobalVersionRestored {
            name: outcome.name,
            from_version: job.version,
            version: outcome.version,
        },
        Err(reason) => OpOutcome::Failed {
            action,
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
        // 表型：本体是项目分析库里的一张表。全局库与项目库同处一条根连接，
        // 项目会话直接 CTAS 到 `rds_global.<版本表>`（见 `promote_table_from_project`）。
        match open_project_conn(&job.project_root).await {
            Ok(project) => {
                gstore
                    .promote_table_from_project(&project, &job.request, job.overwrite)
                    .await
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

/// 打开项目库并借一个写连接（表型升级的 CTAS 要在项目会话上执行）。
async fn open_project_conn(
    project_root: &std::path::Path,
) -> Result<crate::global::ProjectDuckConn, String> {
    let manager = ProjectDatabaseManager::open(project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|reason| format!("打开项目库失败：{reason}"))?;
    let Some(duckdb) = manager.duckdb_conn() else {
        return Err("项目 DuckDB 写连接未就绪".to_string());
    };
    let duck_conn = duckdb
        .acquire()
        .await
        .map_err(|reason| reason.to_string())?;
    Ok(crate::global::ProjectDuckConn(
        duck_conn,
        manager.sqlite_pool(),
    ))
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
    // 刀 C：分发成功后记一条 `derived_from`（项目产物 → 全局上游）。
    // 跨库只存 id 字符串：时间线按 from_id 命中即可见；上游被删由全局侧自己回答。
    // 写失败只告警：分发本身已成功，不能因为谱系补记不上就回滚一份能用的数据。
    if let Ok(outcome) = &result {
        match gstore.latest_global_id(&job.name).await {
            Ok(Some(global_id)) => {
                let store = crate::store::AnalyticsResourceStore::new(manager.sqlite_pool());
                if let Err(error) = store
                    .record_derived_from(&outcome.resource_id, &global_id)
                    .await
                {
                    tracing::warn!(error = %error, "分发落库后补记 derived_from 失败");
                }
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(error = %error, "读全局上游 id 失败，跳过 derived_from");
            }
        }
    }
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

/// 引用到本项目（工作线程上执行）：项目侧建 `table_ref` 行 + pin 全局版本，不复制数据。
pub(crate) async fn run_reference(job: &ReferenceGlobalJob) -> OpOutcome {
    use crate::global::{GlobalInstanceStore, InstanceOpError};

    let gstore = match GlobalInstanceStore::production().await {
        Ok(store) => store,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "引用全局资产",
                reason: format!("打开全局系统库失败：{reason}"),
            };
        }
    };
    let project = match open_project_conn(&job.project_root).await {
        Ok(project) => project,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "引用全局资产",
                reason,
            };
        }
    };
    match gstore
        .reference_to_project(
            &project,
            &job.project_root,
            &job.name,
            job.global_version,
            job.alias.as_deref(),
        )
        .await
    {
        Ok(outcome) => OpOutcome::Referenced {
            name: outcome.name,
            version: outcome.version,
            global_version: outcome.global_version,
        },
        Err(InstanceOpError::NotFound { name }) => OpOutcome::Failed {
            action: "引用全局资产",
            reason: format!("全局实例中没有「{name}」v{}", job.global_version),
        },
        Err(reason) => OpOutcome::Failed {
            action: "引用全局资产",
            reason: reason.to_string(),
        },
    }
}

/// 固化引用版本（工作线程上执行）：复制指定全局版本 → 独立本地表资产。
pub(crate) async fn run_solidify(job: &SolidifyRefJob) -> OpOutcome {
    use crate::global::{GlobalInstanceStore, InstanceOpError};

    let gstore = match GlobalInstanceStore::production().await {
        Ok(store) => store,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "固化引用版本",
                reason: format!("打开全局系统库失败：{reason}"),
            };
        }
    };
    let project = match open_project_conn(&job.project_root).await {
        Ok(project) => project,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "固化引用版本",
                reason,
            };
        }
    };
    match gstore
        .solidify_ref_version(
            &project,
            &job.project_root,
            &job.resource_id,
            job.project_version,
            &job.target,
        )
        .await
    {
        Ok(outcome) => OpOutcome::Solidified {
            name: outcome.name,
            version: outcome.version,
        },
        Err(InstanceOpError::Conflict { name, field }) => OpOutcome::Failed {
            action: "固化引用版本",
            reason: format!(
                "目标名「{name}」已存在（冲突字段：{}），请换一个名字",
                field.label()
            ),
        },
        Err(reason) => OpOutcome::Failed {
            action: "固化引用版本",
            reason: reason.to_string(),
        },
    }
}

/// 提交一次「引用到本项目」（引用 = pin 全局版本，不写本地数据）。
pub fn enqueue_reference(
    project_root: PathBuf,
    name: String,
    global_version: i32,
    alias: Option<String>,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Reference(ReferenceGlobalJob {
        project_root,
        name,
        global_version,
        alias,
    }));
}

/// 提交一次「固化引用版本」（把选中的引用版本复制成本地独立表资产）。
pub fn enqueue_solidify(
    project_root: PathBuf,
    resource_id: String,
    project_version: i32,
    target: String,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Solidify(SolidifyRefJob {
        project_root,
        resource_id,
        project_version,
        target,
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

/// 提交一次「恢复全局历史版本」。
pub fn enqueue_restore_global_version(
    project_root: PathBuf,
    name: String,
    version: i32,
    table: bool,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::RestoreGlobalVersion(RestoreGlobalVersionJob {
        project_root,
        name,
        version,
        table,
    }));
}

/// 提交一次「从全局实例删除」（刀 B；确认框之后）。
pub fn enqueue_delete_global(project_root: PathBuf, name: String) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs()
        .tx
        .send(Job::DeleteGlobal(DeleteGlobalJob { project_root, name }));
}
