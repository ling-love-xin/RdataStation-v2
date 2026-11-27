//! 回收站取数、动作执行与动作后重取。

use super::*;
use crate::present::GLOBAL_TRASH_ID_PREFIX;
use crate::global::GlobalInstanceStore;

/// 取回收站条目 → 对话框种子（工作线程上执行）。
///
/// 两个来源（§12.1.1 ③）：
/// - **实体条目**：`ProjectTrash` 目录里的 copy 型本体（回收站是文件系统目录，不碰项目库）；
/// - **软删登记行**：reference 型没有本体可搬，回收站清单要把软删登记行一并列出。
/// 两路都按删除时间倒序返回，这里双指针归并成一份统一倒序清单。
pub(crate) async fn load_trash(job: &TrashListJob) -> Result<TrashDialogSeed, String> {
    let entries = PayloadStore::new(job.project_root.clone())
        .trash()
        .list()
        .await
        .map_err(|e| format!("读取回收站失败：{e}"))?;
    let snapshot = build_trash_snapshot(&entries);

    // 项目库读不到时不弄砸实体清单：第二来源缺失只表现为 reference 条目不显示。
    let reference: Vec<(DateTime<Utc>, TrashRow)> =
        match open_service(job.project_root.clone()).await {
            Ok(service) => match service.deleted_registry_archives().await {
                Ok(rows) => registry_trash_rows(&rows),
                Err(error) => {
                    tracing::warn!(error = %error, "读取软删 reference 回收站条目失败");
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        };

    let entity: Vec<(DateTime<Utc>, TrashRow)> =
        entries.iter().filter_map(entity_trash_row).collect();
    // 全局回收站条目并入同一个清单：回收站只有一个入口，用户不该为了找一条
    // 全局表再换一个对话框。读取失败只降级（项目条目照常列出）。
    let global: Vec<(DateTime<Utc>, TrashRow)> = match crate::global::GlobalInstanceStore::production().await
    {
        Ok(store) => match store.list_global_trash() {
            Ok(entries) => entries.iter().map(global_trash_row).collect(),
            Err(error) => {
                tracing::warn!(error = %error, "读取全局回收站条目失败");
                Vec::new()
            }
        },
        Err(error) => {
            tracing::warn!(error = %error, "打开全局实例失败，全局回收站条目不列出");
            Vec::new()
        }
    };
    let mut rows: Vec<(DateTime<Utc>, TrashRow)> =
        Vec::with_capacity(entity.len() + reference.len() + global.len());
    rows.extend(entity);
    rows.extend(reference);
    rows.extend(global);
    // 三路来源统一按删除时间倒序（同一时刻的先后不重要）。
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let rows: Vec<TrashRow> = rows.into_iter().map(|(_, row)| row).collect();

    Ok(TrashDialogSeed {
        scope: crate::dialogs::trash::TrashScope::Project,
        rows,
        foreign: snapshot
            .foreign
            .into_iter()
            .map(|(origin, count)| ForeignTrash {
                module_label: module_label(&origin),
                count,
            })
            .collect(),
    })
}

/// 回收站条目来源 → 模块显示名。
///
/// 回收站是项目级的：说清“这条在谁那儿”比单给一个内部代号有用；
/// 认不出的代号原样显示（宁可见生，不可编一个名字）。
fn module_label(origin: &str) -> String {
    match origin {
        crate::resource_host::ORIGIN_SCRATCHPAD => "草稿箱".to_string(),
        other => other.to_string(),
    }
}

/// 执行一次回收站动作（工作线程上执行）。
pub(crate) async fn run_trash_action(job: &TrashActionJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone()).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "回收站",
                reason,
            };
        }
    };
    match &job.action {
        TrashAction::Restore { trash_id } => {
            if let Some(name) = trash_id.strip_prefix(GLOBAL_TRASH_ID_PREFIX) {
                let store = match GlobalInstanceStore::production().await {
                    Ok(store) => store,
                    Err(error) => {
                        return OpOutcome::Failed {
                            action: "从全局回收站还原",
                            reason: format!("打开全局实例失败：{error}"),
                        };
                    }
                };
                return match store.restore_global(name).await {
                    Ok(outcome) => OpOutcome::TrashDone {
                        note: format!("已从全局回收站还原「{name}」（v{}）", outcome.version),
                    },
                    Err(error) => OpOutcome::Failed {
                        action: "从全局回收站还原",
                        reason: error.to_string(),
                    },
                };
            }
            match service.restore_archive_from_trash(trash_id).await {
                Ok(resource) => OpOutcome::TrashDone {
                    note: format!("已从回收站还原「{}」", resource.name),
                },
                Err(error) => OpOutcome::Failed {
                    action: "从回收站还原",
                    reason: error.to_string(),
                },
            }
        }
        TrashAction::Purge { trash_id } => {
            if let Some(name) = trash_id.strip_prefix(GLOBAL_TRASH_ID_PREFIX) {
                let store = match GlobalInstanceStore::production().await {
                    Ok(store) => store,
                    Err(error) => {
                        return OpOutcome::Failed {
                            action: "永久删除全局条目",
                            reason: format!("打开全局实例失败：{error}"),
                        };
                    }
                };
                return match store.purge_global(name).await {
                    Ok(_) => OpOutcome::TrashDone {
                        note: "已永久删除该全局条目（表与版本快照不再保留）".to_string(),
                    },
                    Err(error) => OpOutcome::Failed {
                        action: "永久删除全局条目",
                        reason: error.to_string(),
                    },
                };
            }
            match service.purge_archive(trash_id).await {
                Ok(()) => OpOutcome::TrashDone {
                    note: "已永久删除该条目（本体不再保留）".to_string(),
                },
                Err(error) => OpOutcome::Failed {
                    action: "永久删除",
                    reason: error.to_string(),
                },
            }
        }
        TrashAction::Empty => {
            let project = service.empty_trash().await;
            let global: Result<(usize, usize), String> =
                match GlobalInstanceStore::production().await {
                    Ok(store) => store
                        .empty_global_trash()
                        .await
                        .map_err(|error| error.to_string()),
                    Err(error) => Err(format!("打开全局实例失败：{error}")),
                };
            match (project, global) {
                (Ok(project_count), Ok((global_count, skipped))) => {
                    let mut note = format!(
                        "已清空回收站（项目 {project_count} 项、全局 {global_count} 项，本体不再保留）"
                    );
                    if skipped > 0 {
                        note.push_str(&format!("；{skipped} 项仍被项目引用，已跳过"));
                    }
                    OpOutcome::TrashDone { note }
                }
                (Err(error), _) => OpOutcome::Failed {
                    action: "清空回收站",
                    reason: error.to_string(),
                },
                (_, Err(error)) => OpOutcome::Failed {
                    action: "清空全局回收站",
                    reason: format!("{error}"),
                },
            }
        }
    }
}

/// 动作后重取回收站清单（worker 里 TrashAction 分支用；构造在域内，字段不外露）。
pub(crate) async fn reload_trash_after(project_root: PathBuf) -> Result<TrashDialogSeed, String> {
    load_trash(&TrashListJob { project_root }).await
}

