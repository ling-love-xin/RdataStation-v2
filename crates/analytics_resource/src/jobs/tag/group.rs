//! 分组动作执行（建 / 改名 / 删 / 移动）。

use engine::persistence::project_db::ProjectDatabaseManager;

use super::*;

/// 执行一次分组动作（工作线程上执行）。
pub(crate) async fn run_group_action(job: &GroupActionJob) -> OpOutcome {
    let manager = match ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE).await {
        Ok(manager) => manager,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "分组",
                reason: format!("打开项目库失败：{reason}"),
            };
        }
    };
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());

    match &job.action {
        GroupJobAction::Create { name } => {
            match store
                .create_folder(crate::store::rows::CreateFolderRequest {
                    name: name.clone(),
                    scope: "project".to_string(),
                    // 单层分组（架构 D8）：父分组恒为空。
                    parent_folder_id: None,
                    color: None,
                    icon: None,
                })
                .await
            {
                Ok(folder) => OpOutcome::GroupDone {
                    note: format!("已新建分组「{}」", folder.name),
                },
                Err(error) => OpOutcome::Failed {
                    action: "新建分组",
                    reason: error.to_string(),
                },
            }
        }
        GroupJobAction::Rename { id, name } => match store.rename_folder(id, name).await {
            Ok(folder) => OpOutcome::GroupDone {
                note: format!("已把分组改名为「{}」", folder.name),
            },
            Err(error) => OpOutcome::Failed {
                action: "重命名分组",
                reason: error.to_string(),
            },
        },
        GroupJobAction::Delete { id } => match store.delete_folder(id).await {
            Ok(0) => OpOutcome::GroupDone {
                note: "分组已删除（里面没有存档）".to_string(),
            },
            Ok(ungrouped) => OpOutcome::GroupDone {
                note: format!("分组已删除，{ungrouped} 条存档回到未分组"),
            },
            Err(error) => OpOutcome::Failed {
                action: "删除分组",
                reason: error.to_string(),
            },
        },
        GroupJobAction::Move {
            resource_ids,
            folder_id,
        } => {
            // 移动到同一分组是无意义操作但不算错；逐条走，中途失败不做预回滚
            // （与移入回收站同一取舍），错误里说清做到哪一条。
            let mut done = 0usize;
            for resource_id in resource_ids {
                let result = match folder_id.as_deref() {
                    Some(folder) => store.add_resource_to_folder(resource_id, folder).await,
                    None => store.clear_resource_folder(resource_id).await,
                };
                if let Err(error) = result {
                    return OpOutcome::Failed {
                        action: "移动到分组",
                        reason: format!("{error}（已移动 {done} 条）"),
                    };
                }
                done += 1;
            }
            OpOutcome::GroupDone {
                note: match folder_id.as_deref() {
                    Some(_) => format!("已移动 {done} 条存档到分组"),
                    None => format!("已把 {done} 条存档移回未分组"),
                },
            }
        }
    }
}
