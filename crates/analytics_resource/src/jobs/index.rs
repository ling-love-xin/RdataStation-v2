//! 索引健康域（拆分自 jobs.rs）：索引扫描取数与修复动作执行。
//!
//! `load_index_report` 逐个本体算 sha256（"指纹不匹配"那一档的代价），必须在工作线程；
//! `run_repair_action` 走 `IndexRepair` 的四类修复动作。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use engine::persistence::project_db::ProjectDatabaseManager;

use crate::dialogs::index_repair::{RepairAction, RepairRow};
use crate::payload::PayloadStore;
use crate::present::build_repair_rows;
use crate::{AnalyticsResourceStore, ArchiveKind, IndexRepair};

use super::{jobs, open_service, Job, OpOutcome, SQLITE_POOL_SIZE};

/// 一次索引扫描任务（扫描不改任何状态，所以不带只读标志）。
pub(crate) struct IndexScanJob {
    pub(crate) project_root: PathBuf,
}

/// 一次索引修复动作任务（补登 / 删孤儿记录 / 接受当前内容）。
pub(crate) struct IndexRepairActionJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) action: RepairAction,
}

/// 索引扫描结果（宿主据此开窗，或刷新已经开着的那个窗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexScanRows {
    pub rows: Vec<RepairRow>,
}

/// 索引扫描 → 修复行（工作线程上执行）。
///
/// 要逐个本体算 sha256（“指纹不匹配”那一档的代价），所以必须离 UI 线程；
/// 与 `refresh` 各自扫一次（那一份扫是为了行状态与计数），二者都不改任何状态。
pub(crate) async fn load_index_report(job: &IndexScanJob) -> Result<IndexScanRows, String> {
    let manager = ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|e| format!("打开项目库失败：{e}"))?;
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());
    let payload = PayloadStore::new(job.project_root.clone());
    let report = IndexRepair::new(&payload, &store)
        .scan()
        .await
        .map_err(|e| format!("扫描本体失败：{e}"))?;

    Ok(IndexScanRows {
        rows: build_repair_rows(&report),
    })
}

/// 执行一次索引修复动作（工作线程上执行）。
pub(crate) async fn run_repair_action(job: &IndexRepairActionJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone(), None).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "索引修复",
                reason,
            };
        }
    };
    let repair = IndexRepair::new(service.payload(), service.store());

    match job.action.clone() {
        RepairAction::Adopt { rel_path } => {
            // `resources/` 里只可能是文件，所以固定文件型；来源无从得知，留空（原型 §4.5）。
            match repair.adopt_file(&rel_path, None, ArchiveKind::File).await {
                Ok(resource) => OpOutcome::RepairDone {
                    note: format!("已补登「{}」（resources/{rel_path}）", resource.name),
                },
                Err(error) => OpOutcome::Failed {
                    action: "补登存档",
                    reason: error.to_string(),
                },
            }
        }
        RepairAction::DeleteRecord { resource_id } => {
            match repair.remove_orphan_record(&resource_id).await {
                Ok(()) => OpOutcome::RepairDone {
                    note: "已删除孤儿登记记录".to_string(),
                },
                Err(error) => OpOutcome::Failed {
                    action: "删除记录",
                    reason: error.to_string(),
                },
            }
        }
        RepairAction::AcceptContent { resource_id } => {
            match repair.accept_current_content(&resource_id).await {
                Ok(resource) => OpOutcome::RepairDone {
                    note: format!(
                        "已接受当前内容（「{}」现为 v{}；旧内容不可得，那一版只留元数据）",
                        resource.name, resource.version
                    ),
                },
                Err(error) => OpOutcome::Failed {
                    action: "接受当前内容",
                    reason: error.to_string(),
                },
            }
        }
        RepairAction::RestoreFromTrash { rel_path } => {
            match service.restore_archive_by_rel_path(&rel_path).await {
                Ok(resource) => OpOutcome::RepairDone {
                    note: format!(
                        "已从回收站还原「{}」（resources/{rel_path}）",
                        resource.name
                    ),
                },
                Err(error) => OpOutcome::Failed {
                    action: "从回收站还原",
                    reason: error.to_string(),
                },
            }
        }
        // 跳转类动作由宿主在事件路径拦截（不入这个作业）；真走到这里只给一句话。
        RepairAction::OpenVersions { .. } => OpOutcome::RepairDone {
            note: "版本历史已打开".to_string(),
        },
    }
}

/// 提交一次索引扫描（**事件路径**调用：状态行「修复…」与面板头「重建索引…」）。
pub fn enqueue_index_scan(project_root: PathBuf) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs()
        .tx
        .send(Job::IndexScan(IndexScanJob { project_root }));
}

/// 提交一次索引修复动作（**事件路径**调用：索引修复对话框的行内动作）。
pub fn enqueue_index_repair_action(project_root: PathBuf, read_only: bool, action: RepairAction) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::IndexRepairAction(IndexRepairActionJob {
        project_root,
        read_only,
        action,
    }));
}


/// 动作后重扫索引（worker 里 IndexRepairAction 分支用；构造在域内，字段不外露）。
pub(crate) async fn reload_index_scan_after(
    project_root: PathBuf,
) -> Result<IndexScanRows, String> {
    load_index_report(&IndexScanJob { project_root }).await
}
