//! 归档 / 取回 / 撤销 / 回收站域（拆分自 jobs.rs）：作业结构、执行器与入队入口。
//!
//! `worker` 按 `Job` 枚举分发到这里：执行动作（`run_*`）或取数（`load_trash`），
//! 结果由 `worker` 写回 `jobs()` 的结果槽；本域只做执行，不做调度。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use chrono::{DateTime, Utc};

use crate::analysis::AnalysisFacts;
use crate::dialogs::trash::{ForeignTrash, TrashAction, TrashDialogSeed, TrashRow};
use crate::payload::PayloadStore;
use crate::present::{build_trash_snapshot, entity_trash_row, reference_trash_rows};
use crate::{ArchiveKind, ArchiveRequest, ArchiveUndo, CheckoutRequest, KeepVersions};

use super::{free_dest, jobs, open_service, Job, OpOutcome};

/// 一次归档任务。
pub(crate) struct ArchiveJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) request: ArchiveRequest,
    /// 历史内容保留策略（设置项 `resources.keep_versions`，在入队时从主线程读好）。
    pub(crate) keep_versions: KeepVersions,
    /// 归档前是否**探一次数据文件**（`analysis` 档的采集侧）。
    ///
    /// 为什么要过作业：探测要连 DuckDB、要读文件——不能在事件路径上做（那会卡 UI）。
    /// 探测失败**降级为文件型**而不是拒归档：文件先安全落进 `resources/` 比"结构没探到就不让归档"重要。
    pub(crate) analysis_probe: bool,
}

/// 一次取回（检出）任务。
pub(crate) struct CheckoutJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) request: CheckoutRequest,
    /// 是否顺手在编辑器里打开（打开动作由事件路径做——它才有窗口）。
    pub(crate) open_after: bool,
}

/// 一次撤销归档任务。
pub(crate) struct UndoJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) undo: ArchiveUndo,
}

/// 一次移入回收站任务（面板多选可以一次提交多条）。
pub(crate) struct TrashJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) resource_ids: Vec<String>,
}

/// 一次回收站取数任务（条目 + 别人条目的统计）。
pub(crate) struct TrashListJob {
    pub(crate) project_root: PathBuf,
}

/// 一次回收站动作任务（还原 / 永久删除 / 清空自己名下的）。
pub(crate) struct TrashActionJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) action: TrashAction,
}

/// 执行一次归档。
/// 探一个数据文件，拼成 [`AnalysisFacts`]（工作线程上执行）。
///
/// 用全局内存 DuckDB 连接（`DuckDBManager::global`）：它已经配好扩展目录与文件读取器，
/// 不必为一个探测另开库；锁住期间只读文件，不与其它分析争写。
fn probe_analysis_facts(path: &std::path::Path) -> Result<AnalysisFacts, String> {
    let path_text = path.to_string_lossy().to_string();
    let definition = engine::duckdb::file_select_sql(&path_text)
        .ok_or_else(|| format!("没有可用的读取器：{path_text}"))?;
    let conn = engine::duckdb::DuckDBManager::get_or_create_in_memory()
        .map_err(|e| format!("打开分析引擎失败：{e}"))?;
    let guard = conn.lock().map_err(|e| format!("分析引擎繁忙：{e}"))?;
    let probe = engine::duckdb::probe_file(&guard, &path_text).map_err(|e| e.to_string())?;
    Ok(AnalysisFacts::from_probe(
        Some(definition),
        probe.columns,
        probe.row_count,
    ))
}

pub(crate) async fn run_archive(job: &ArchiveJob) -> OpOutcome {
    let name = job.request.name.clone();
    // 采集侧（P4.1 第二半）：数据文件先探一次结构，指纹按“定义 + 结构”走。
    let mut request = job.request.clone();
    if job.analysis_probe {
        match probe_analysis_facts(&request.source_path) {
            Ok(facts) => request.analysis = Some(facts),
            Err(reason) => {
                // 降级而不是拒归档：文件先安全落进 `resources/`，按文件型登记指纹。
                tracing::warn!(error = %reason, path = %request.source_path.display(),
                    "数据文件探测失败，按文件型归档");
                request.kind = ArchiveKind::File;
                request.analysis = None;
            }
        }
    }
    let service = match open_service(job.project_root.clone(), Some(job.keep_versions)).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "归档",
                reason,
            };
        }
    };
    match service.archive(request.clone()).await {
        Ok(outcome) => {
            // 首次归档才给撤销凭据：再归档的"撤销"是版本回退（服务层也会挡），不在这条路上。
            let undo = job
                .request
                .existing_resource_id
                .is_none()
                .then(|| ArchiveUndo {
                    resource_id: outcome.resource_id.clone(),
                    name: name.clone(),
                    source_path: job.request.source_path.clone(),
                });
            OpOutcome::Archived {
                resource_id: outcome.resource_id.clone(),
                name,
                version: outcome.version,
                rel_path: outcome.file_rel_path,
                undo,
                notes: outcome.notes,
            }
        }
        Err(error) => OpOutcome::Failed {
            action: "归档",
            reason: error.to_string(),
        },
    }
}

/// 执行一次取回（检出）。
///
/// 落点在动文件**之前**定死（重名避让），回执里给的就是真实路径：用户看到"取回为 X"
/// 之后去草稿箱找 X，必须找得到。
pub(crate) async fn run_checkout(job: &CheckoutJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone(), None).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "取回",
                reason,
            };
        }
    };
    let dest = free_dest(&job.request.dest_path);
    let request = CheckoutRequest {
        resource_id: job.request.resource_id.clone(),
        dest_path: dest.clone(),
    };
    match service.checkout(request).await {
        Ok(outcome) => OpOutcome::CheckedOut {
            dest: outcome.dest_path,
            version: outcome.version,
            open_after: job.open_after,
        },
        Err(error) => OpOutcome::Failed {
            action: "取回",
            reason: error.to_string(),
        },
    }
}

/// 执行一次撤销归档。
pub(crate) async fn run_undo(job: &UndoJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone(), None).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "撤销归档",
                reason,
            };
        }
    };
    match service.undo_archive(&job.undo).await {
        Ok(()) => OpOutcome::Undone {
            name: job.undo.name.clone(),
        },
        Err(error) => OpOutcome::Failed {
            action: "撤销归档",
            reason: error.to_string(),
        },
    }
}

/// 执行一次移入回收站（批量）。
///
/// 逐条走服务层；服务层不做预回滚（部分成功就部分成功），所以错误文案里会带“已移入 N 项”
/// ——宿主只需原样转述，不要自己算进度。
pub(crate) async fn run_trash(job: &TrashJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone(), None).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "移入回收站",
                reason,
            };
        }
    };
    match service.move_to_trash(&job.resource_ids).await {
        Ok(entries) => OpOutcome::Trashed {
            names: entries.into_iter().map(|entry| entry.name).collect(),
        },
        Err(error) => OpOutcome::Failed {
            action: "移入回收站",
            reason: error.to_string(),
        },
    }
}

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
    let reference: Vec<(DateTime<Utc>, TrashRow)> = match open_service(job.project_root.clone(), None).await {
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
        scratchpad::ORIGIN_SCRATCHPAD => "草稿箱".to_string(),
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

/// 提交一次归档（**事件路径**调用：对话框确认之后）。
pub fn enqueue_archive(
    project_root: PathBuf,
    read_only: bool,
    request: ArchiveRequest,
    keep_versions: KeepVersions,
    analysis_probe: bool,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Archive(ArchiveJob {
        project_root,
        read_only,
        request,
        keep_versions,
        analysis_probe,
    }));
}

/// 提交一次取回（**事件路径**调用：对话框确认之后）。
pub fn enqueue_checkout(
    project_root: PathBuf,
    read_only: bool,
    request: CheckoutRequest,
    open_after: bool,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Checkout(CheckoutJob {
        project_root,
        read_only,
        request,
        open_after,
    }));
}

/// 提交一次撤销归档（**事件路径**调用：撤销栏的「撤销」）。
pub fn enqueue_undo(project_root: PathBuf, read_only: bool, undo: ArchiveUndo) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Undo(UndoJob {
        project_root,
        read_only,
        undo,
    }));
}

/// 提交一次移入回收站（**事件路径**调用：详情危险区 / 多选动作栏）。
pub fn enqueue_trash(project_root: PathBuf, read_only: bool, resource_ids: Vec<String>) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Trash(TrashJob {
        project_root,
        read_only,
        resource_ids,
    }));
}

/// 提交一次回收站取数（**事件路径**调用：面板头「⋯ → 回收站…」）。
pub fn enqueue_trash_list(project_root: PathBuf) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs()
        .tx
        .send(Job::TrashList(TrashListJob { project_root }));
}

/// 提交一次回收站动作（**事件路径**调用：回收站对话框的行内动作与「清空」）。
pub fn enqueue_trash_action(project_root: PathBuf, read_only: bool, action: TrashAction) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::TrashAction(TrashActionJob {
        project_root,
        read_only,
        action,
    }));
}


/// 动作后重取回收站清单（worker 里 TrashAction 分支用；构造在域内，字段不外露）。
pub(crate) async fn reload_trash_after(project_root: PathBuf) -> Result<TrashDialogSeed, String> {
    load_trash(&TrashListJob { project_root }).await
}
