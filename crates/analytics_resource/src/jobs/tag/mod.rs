//! 标签 / 分组域（拆分自 jobs.rs）：标签取数、标签动作与分组动作执行。
//!
//! `load_tag_rows` 取标签词典 + 多选目标各自已挂的标签（交集 = 全有、并集 - 交集 = 部分）；
//! `run_tag_action` / `run_group_action` 逐条改、不做预回滚（部分成功就部分成功）。

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use engine::persistence::project_db::ProjectDatabaseManager;

use crate::dialogs::tag::{TagChoice, TagDialogSeed};
use crate::{AnalyticsResourceStore, TagTarget};

use super::{Job, OpOutcome, SQLITE_POOL_SIZE, jobs};

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


mod action;
mod group;
mod list;

pub(crate) use action::{reload_tag_rows_after, run_tag_action};
pub(crate) use group::run_group_action;
pub(crate) use list::load_tag_rows;

