//! 改名 / 别名域（拆分自 jobs.rs）：改显示名与别名。
//!
//! 走 `rename_resource` / `set_alias`：只改名字，不涨版本、不写版本快照。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use engine::persistence::project_db::ProjectDatabaseManager;

use crate::AnalyticsResourceStore;

use super::{jobs, Job, OpOutcome, SQLITE_POOL_SIZE};

/// 改显示名（不需要对话框取数：目标与当前名字都在事件路径上拿到）。
pub(crate) struct RenameJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) resource_id: String,
    pub(crate) name: String,
}

/// 改别名（同上；空串 = 清除）。
pub(crate) struct SetAliasJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) resource_id: String,
    pub(crate) alias: String,
}

/// 改一条存档的显示名（工作线程上执行）。
///
/// 走 `rename_resource`（不是 `update_resource`）：**只改名字**，不涨版本、不写版本快照
/// ——改名不是内容变更（见 `resource.rs::rename_resource` 的注释）。
pub(crate) async fn run_rename(job: &RenameJob) -> OpOutcome {
    let manager = match ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE).await {
        Ok(manager) => manager,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "重命名",
                reason: format!("打开项目库失败：{reason}"),
            };
        }
    };
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());
    match store.rename_resource(&job.resource_id, &job.name).await {
        Ok(resource) => OpOutcome::Renamed {
            name: resource.name,
        },
        Err(error) => OpOutcome::Failed {
            action: "重命名",
            reason: error.to_string(),
        },
    }
}

/// 改一条存档的别名（工作线程上执行）：空串 = 清除（存 NULL）。
pub(crate) async fn run_set_alias(job: &SetAliasJob) -> OpOutcome {
    let manager = match ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE).await {
        Ok(manager) => manager,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "编辑别名",
                reason: format!("打开项目库失败：{reason}"),
            };
        }
    };
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());
    let alias = (!job.alias.trim().is_empty()).then(|| job.alias.trim().to_string());
    match store.set_alias(&job.resource_id, alias.as_deref()).await {
        Ok(resource) => OpOutcome::AliasDone {
            note: match resource.alias.as_deref() {
                Some(alias) => format!("已把「{}」的别名设为「{alias}」", resource.name),
                None => format!("已清除「{}」的别名", resource.name),
            },
        },
        Err(error) => OpOutcome::Failed {
            action: "编辑别名",
            reason: error.to_string(),
        },
    }
}

/// 提交一次改名（**事件路径**调用：重命名对话框提交）。
///
/// 不需要取数作业：目标是**已存在**的那一条，新名字就是用户在对话框里填的。
pub fn enqueue_rename(project_root: PathBuf, read_only: bool, resource_id: String, name: String) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Rename(RenameJob {
        project_root,
        read_only,
        resource_id,
        name,
    }));
}

/// 提交一次别名编辑（**事件路径**调用：详情头部点别名）。空串 = 清除。
pub fn enqueue_set_alias(
    project_root: PathBuf,
    read_only: bool,
    resource_id: String,
    alias: String,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::SetAlias(SetAliasJob {
        project_root,
        read_only,
        resource_id,
        alias,
    }));
}
