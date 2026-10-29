//! 标签 / 分组域（拆分自 jobs.rs）：标签取数、标签动作与分组动作执行。
//!
//! `load_tag_rows` 取标签词典 + 多选目标各自已挂的标签（交集 = 全有、并集 - 交集 = 部分）；
//! `run_tag_action` / `run_group_action` 逐条改、不做预回滚（部分成功就部分成功）。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use engine::persistence::project_db::ProjectDatabaseManager;

use crate::dialogs::tag::{TagChoice, TagDialogSeed};
use crate::{AnalyticsResourceStore, TagTarget};

use super::{jobs, Job, OpOutcome, SQLITE_POOL_SIZE};

/// 一次标签取数任务（全部标签 + 这批目标各自已挂的）。
pub(crate) struct TagListJob {
    pub(crate) project_root: PathBuf,
    pub(crate) targets: Vec<TagTarget>,
}

/// 标签动作（作业侧的形态：对话框的 `TagDialogEvent` 多一种“去掉单个”）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagJobAction {
    /// 提交勾选差集。
    Apply {
        add: Vec<String>,
        remove: Vec<String>,
    },
    /// 新建一个标签并直接打上。
    CreateAndTag { name: String },
    /// 重命名一个标签（只改显示名）。
    RenameTag { id: String, name: String },
    /// 删除一个标签（关联一并清除）。
    DeleteTag { id: String },
    /// 去掉一个标签（详情面板 chip 上的 ×）。
    RemoveOne { tag_id: String },
}

/// 一次标签动作任务。
///
/// 收**目标切片**（单击一元、多选 N 元）：`Apply` 对每个目标各跑一遍差集，
/// 字典级动作（改名 / 删标签）不需要目标，但列表型动作（新建并打上）要全打上。
pub(crate) struct TagActionJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) targets: Vec<TagTarget>,
    pub(crate) action: TagJobAction,
}

/// 分组动作（建 / 改名 / 删 / 移动）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupJobAction {
    Create {
        name: String,
    },
    Rename {
        id: String,
        name: String,
    },
    Delete {
        id: String,
    },
    /// 移动（`folder_id = None` = 移回未分组）；`resource_ids` 收切片，多选也走这一条。
    Move {
        resource_ids: Vec<String>,
        folder_id: Option<String>,
    },
}

/// 一次分组动作任务（不需要对话框取数：分组字典随主快照下发）。
pub(crate) struct GroupActionJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) action: GroupJobAction,
}

/// 标签对话框取数结果（宿主据此开窗，或刷新已经开着的那个窗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRows {
    /// 本次编辑的目标（单选一元、多选 N 元）——动作回来后会话靠它认自己。
    pub targets: Vec<TagTarget>,
    pub seed: TagDialogSeed,
}

/// 取标签词典 + 这批目标各自已挂的 → 对话框取数结果（工作线程上执行）。
///
/// 多选的两份集合：**全有**（交集，显示为勾上）与**部分**（并集 − 交集，显示为「部分」）。
/// 交集为空说明这批目标没有共同标签；并集为空说明都没打过标签——两种都不算错误。
pub(crate) async fn load_tag_rows(job: &TagListJob) -> Result<TagRows, String> {
    let manager = ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|e| format!("打开项目库失败：{e}"))?;
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());

    let tags = store
        .list_tags(Some("project"))
        .await
        .map_err(|e| format!("读取标签失败：{e}"))?;
    let counts = store.tag_usage_counts().await.unwrap_or_default();

    // 逐目标取已挂标签：并集入 seen，交集用“出现过几次 = 目标数”判定。
    let mut first: Option<Vec<String>> = None;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut hit_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let total = job.targets.len();
    for target in &job.targets {
        let ids: Vec<String> = store
            .get_tags_for_resource(&target.id)
            .await
            .map_err(|e| format!("读取存档标签失败：{e}"))?
            .into_iter()
            .map(|tag| tag.id)
            .collect();
        for id in &ids {
            seen.insert(id.clone());
            *hit_counts.entry(id.clone()).or_insert(0) += 1;
        }
        let set: std::collections::HashSet<String> = ids.into_iter().collect();
        first = Some(match first {
            Some(prev) => prev.into_iter().filter(|id| set.contains(id)).collect(),
            None => set.into_iter().collect(),
        });
    }
    let selected: Vec<String> = first.unwrap_or_default();
    let partial: Vec<String> = seen
        .into_iter()
        .filter(|id| hit_counts.get(id).copied().unwrap_or(0) < total && !selected.contains(id))
        .collect();
    let resource_name = match job.targets.as_slice() {
        [one] => one.name.clone(),
        many => format!("{} 项", many.len()),
    };

    Ok(TagRows {
        targets: job.targets.clone(),
        seed: TagDialogSeed {
            resource_name,
            target_count: total,
            options: crate::present::tag_options(&tags, &counts)
                .into_iter()
                .map(|option| TagChoice {
                    id: option.id,
                    name: option.name,
                    count: option.count,
                })
                .collect(),
            selected,
            partial,
        },
    })
}

/// 执行一次标签动作（工作线程上执行）。
///
/// 不静默降级：删一个不存在的标签、建一个重名标签都会报错（服务层的错误文案已可读）；
/// 逐条打/去标**不做预回滚**（部分成功就部分成功，错误里说清做到哪一步）。
pub(crate) async fn run_tag_action(job: &TagActionJob) -> OpOutcome {
    let manager = match ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE).await {
        Ok(manager) => manager,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "标签",
                reason: format!("打开项目库失败：{reason}"),
            };
        }
    };
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());

    match &job.action {
        TagJobAction::Apply { add, remove } => {
            // 目标 × 标签两层循环：逐条改、**不做预回滚**（部分成功就部分成功，
            // 错误里报“改了 N 处”），与删除 / 移动同一条口径。
            let mut done = 0usize;
            for target in &job.targets {
                for tag_id in remove {
                    if let Err(error) = store.remove_tag_from_resource(&target.id, tag_id).await {
                        return OpOutcome::Failed {
                            action: "去标签",
                            reason: format!(
                                "「{}」没改成：{error}（本次已改 {done} 处）",
                                target.name
                            ),
                        };
                    }
                    done += 1;
                }
                for tag_id in add {
                    if let Err(error) = store.add_tag_to_resource(&target.id, tag_id).await {
                        return OpOutcome::Failed {
                            action: "打标签",
                            reason: format!(
                                "「{}」没打成：{error}（本次已改 {done} 处）",
                                target.name
                            ),
                        };
                    }
                    done += 1;
                }
            }
            let scope = match job.targets.as_slice() {
                [one] => format!("「{}」", one.name),
                many => format!("{} 项", many.len()),
            };
            OpOutcome::TagDone {
                note: format!("已更新{scope}的标签（改了 {done} 处）"),
            }
        }
        TagJobAction::CreateAndTag { name } => {
            let tag = match store
                .create_tag(crate::models::CreateTagRequest {
                    name: name.clone(),
                    scope: "project".to_string(),
                    color: None,
                    icon: None,
                })
                .await
            {
                Ok(tag) => tag,
                Err(error) => {
                    return OpOutcome::Failed {
                        action: "新建标签",
                        reason: error.to_string(),
                    };
                }
            };
            // 新建并打上：**所有目标**都打上（多选时“新建并打上”就是给全选集的）。
            for target in &job.targets {
                if let Err(error) = store.add_tag_to_resource(&target.id, &tag.id).await {
                    return OpOutcome::Failed {
                        action: "打标签",
                        reason: format!(
                            "标签「{}」已建好，但「{}」没打上：{error}",
                            tag.name, target.name
                        ),
                    };
                }
            }
            let scope = match job.targets.as_slice() {
                [one] => one.name.clone(),
                many => format!("{} 项", many.len()),
            };
            OpOutcome::TagDone {
                note: format!("已新建标签「{}」并打上（{scope}）", tag.name),
            }
        }
        TagJobAction::RenameTag { id, name } => match store.rename_tag(id, name).await {
            Ok(tag) => OpOutcome::TagDone {
                note: format!("已把标签改名为「{}」", tag.name),
            },
            Err(error) => OpOutcome::Failed {
                action: "重命名标签",
                reason: error.to_string(),
            },
        },
        TagJobAction::DeleteTag { id } => match store.delete_tag(id).await {
            Ok(()) => OpOutcome::TagDone {
                note: "已删除该标签（定义软删，已挂关联不再显示）".to_string(),
            },
            Err(error) => OpOutcome::Failed {
                action: "删除标签",
                reason: error.to_string(),
            },
        },
        TagJobAction::RemoveOne { tag_id } => {
            let Some(target) = job.targets.first() else {
                return OpOutcome::Failed {
                    action: "去标签",
                    reason: "没有指定要改的存档".to_string(),
                };
            };
            match store.remove_tag_from_resource(&target.id, tag_id).await {
                Ok(()) => OpOutcome::TagDone {
                    note: format!("已去掉「{}」的一个标签", target.name),
                },
                Err(error) => OpOutcome::Failed {
                    action: "去标签",
                    reason: error.to_string(),
                },
            }
        }
    }
}

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
                .create_folder(crate::models::CreateFolderRequest {
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

/// 提交一次标签取数（**事件路径**调用：详情面板「＋ 标签」、行右键「编辑标签…」）。
pub fn enqueue_tag_list(project_root: PathBuf, targets: Vec<TagTarget>) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::TagList(TagListJob {
        project_root,
        targets,
    }));
}

/// 提交一次标签动作（**事件路径**调用：标签对话框的提交、详情 chip 的 ×）。
pub fn enqueue_tag_action(
    project_root: PathBuf,
    read_only: bool,
    targets: Vec<TagTarget>,
    action: TagJobAction,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::TagAction(TagActionJob {
        project_root,
        read_only,
        targets,
        action,
    }));
}

/// 提交一次分组动作（**事件路径**调用：行菜单「移动到分组」、分组头菜单与分组名对话框）。
pub fn enqueue_group_action(project_root: PathBuf, read_only: bool, action: GroupJobAction) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::GroupAction(GroupActionJob {
        project_root,
        read_only,
        action,
    }));
}


/// 动作后重取标签取数（worker 里 TagAction 分支用；构造在域内，字段不外露）。
pub(crate) async fn reload_tag_rows_after(
    project_root: PathBuf,
    targets: Vec<TagTarget>,
) -> Result<TagRows, String> {
    load_tag_rows(&TagListJob {
        project_root,
        targets,
    })
    .await
}
