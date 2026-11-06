//! 标签动作执行与动作后重取。

use engine::persistence::project_db::ProjectDatabaseManager;

use super::*;

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
                .create_tag(crate::store::rows::CreateTagRequest {
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
