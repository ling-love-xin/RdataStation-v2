//! 回收站取数、动作执行与动作后重取。

use super::*;

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
        match open_service(job.project_root.clone(), None).await {
            Ok(service) => match service.deleted_reference_archives().await {
                Ok(rows) => reference_trash_rows(&rows),
                Err(error) => {
                    tracing::warn!(error = %error, "读取软删 reference 回收站条目失败");
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        };

    let entity: Vec<(DateTime<Utc>, TrashRow)> =
        entries.iter().filter_map(entity_trash_row).collect();
    let mut entity_i = 0;
    let mut ref_i = 0;
    let mut rows: Vec<TrashRow> = Vec::with_capacity(entity.len() + reference.len());
    while entity_i < entity.len() || ref_i < reference.len() {
        let take_entity = match (entity.get(entity_i), reference.get(ref_i)) {
            (Some(e), Some(r)) => e.0 >= r.0,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            // while 条件已保证至少一项存在，此臂不可达；补齐穷尽性。
            (None, None) => false,
        };
        if take_entity {
            rows.push(entity[entity_i].1.clone());
            entity_i += 1;
        } else {
            rows.push(reference[ref_i].1.clone());
            ref_i += 1;
        }
    }

    Ok(TrashDialogSeed {
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
    let service = match open_service(job.project_root.clone(), None).await {
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
        TrashAction::Purge { trash_id } => match service.purge_archive(trash_id).await {
            Ok(()) => OpOutcome::TrashDone {
                note: "已永久删除该条目（本体不再保留）".to_string(),
            },
            Err(error) => OpOutcome::Failed {
                action: "永久删除",
                reason: error.to_string(),
            },
        },
        TrashAction::Empty => match service.empty_trash().await {
            Ok(0) => OpOutcome::TrashDone {
                note: "回收站里没有资产库的条目".to_string(),
            },
            Ok(count) => OpOutcome::TrashDone {
                note: format!("已清空资产库的回收站（{count} 项，本体不再保留）"),
            },
            Err(error) => OpOutcome::Failed {
                action: "清空回收站",
                reason: error.to_string(),
            },
        },
    }
}

/// 动作后重取回收站清单（worker 里 TrashAction 分支用；构造在域内，字段不外露）。
pub(crate) async fn reload_trash_after(project_root: PathBuf) -> Result<TrashDialogSeed, String> {
    load_trash(&TrashListJob { project_root }).await
}
