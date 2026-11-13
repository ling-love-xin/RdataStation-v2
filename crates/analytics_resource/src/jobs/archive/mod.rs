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
use crate::model::ArchiveAssertion;
use crate::{ArchiveKind, ArchiveRequest, ArchiveUndo, CheckoutRequest, KeepVersions};

use super::{Job, OpOutcome, free_dest, jobs, open_service};

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
    /// 用户在归档对话框里填的业务断言（选填；`None` = 一项没填）。
    pub(crate) assertion: Option<ArchiveAssertion>,
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

/// 提交一次归档（**事件路径**调用：对话框确认之后）。
pub fn enqueue_archive(
    project_root: PathBuf,
    read_only: bool,
    request: ArchiveRequest,
    keep_versions: KeepVersions,
    analysis_probe: bool,
    assertion: Option<ArchiveAssertion>,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Archive(ArchiveJob {
        project_root,
        read_only,
        request,
        keep_versions,
        analysis_probe,
        assertion,
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


mod run;
mod trash;

pub(crate) use run::{run_archive, run_checkout, run_undo, run_trash};
pub(crate) use trash::{load_trash, reload_trash_after, run_trash_action};

