//! 资产库（M6）后台任务：取存档行 + 索引健康扫描 → 面板快照；**归档 / 取回的执行也在这条线程上**。
//!
//! 为什么必须后台：取数要开项目库（`ProjectDatabaseManager::open`，异步 + 文件 I/O），
//! 而 `IndexRepair::scan` 还会**逐个本体算 sha256**（"内容已变"这一档的代价）——放在
//! 事件路径上会直接冻结 UI 线程。归档还要搬文件（跨设备时是真拷贝），取回要复制整份本体，
//! 同理。形态照 `nav_jobs`：单工作线程 + tokio 运行时 + 结果槽 + `enqueue_*` / `drain_*`。
//!
//! 快照在 worker 侧组装（`present::build_snapshot`）：`Shared` 与 GPUI 实体都不跨线程，
//! 所以入队只带**所有权数据**（项目根 / 只读标志 / 请求结构体），行模型与状态映射都在
//! 工作线程上落地，回传的是可直接渲染的 `ResourcesSnapshot`。
//!
//! 动作完成后**顺手补一次取数**（同一个 worker 里）：用户点完对话框，下一帧列表就该有新行——
//! 不让界面停在"提示说成功了，列表却没变"这种要用户自己按刷新一下的中间态。
//!
//! 扫描成本的现状（刻意）：每次刷新都重算指纹。文件多 / 本体大时是可感知开销，但它发生在
//! 工作线程上；若将来成为瓶颈，先在 `indexer` 加"只查存在性"的快路径，**不要**把指纹比对
//! 删掉——那会让"内容已变"静默失效。未登记的本体（`UntrackedFile`）不进行模型（它不是存档），
//! 由索引修复对话框呈现（本模块只把带 id 的两类差异折进行状态）。

mod archive;
mod global;
mod index;
mod rename;
mod tag;
mod refresh;
mod version;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use engine::persistence::project_db::ProjectDatabaseManager;

use crate::dialogs::trash::TrashDialogSeed;
use crate::payload::PayloadStore;
use crate::present::{ArchiveStatuses, PreviewMap, SnapshotInputs, build_snapshot};
use crate::preview::{self, Preview};
use crate::resource_view::{ArchiveRow, GroupOption, ResourcesSnapshot};
use crate::{
    AnalyticsResource, AnalyticsResourceStore, ArchiveKind, ArchiveService, ArchiveStatus,
    ArchiveUndo, IndexIssue, IndexRepair, KeepVersions,
};

use archive::{
    ArchiveJob, CheckoutJob, TrashActionJob, TrashJob, TrashListJob, UndoJob, load_trash,
    reload_trash_after, run_archive, run_checkout, run_trash, run_trash_action, run_undo,
};
use global::{DistributeJob, PromoteJob, run_distribute, run_promote};
use index::{
    IndexRepairActionJob, IndexScanJob, load_index_report, reload_index_scan_after,
    run_repair_action,
};
use rename::{RenameJob, SetAliasJob, run_rename, run_set_alias};
use tag::{
    GroupActionJob, TagActionJob, TagListJob, load_tag_rows, reload_tag_rows_after,
    run_group_action, run_tag_action,
};
use self::refresh::run_refresh;
use version::{
    VersionActionJob, VersionDiffJob, VersionsJob, load_version_diff, load_versions,
    reload_versions_after, run_version_action,
};

/// 对外（面板 / 宿主）直接引用的行模型类型：由领域文件定义、这里 re-export。
pub use index::IndexScanRows;
pub use tag::TagRows;
pub use version::{VersionDiffRows, VersionRows};

/// 连接池大小：与其它项目库访问点一致（`data_source_service` / `workspace_loader` 同为 4）。
pub(crate) const SQLITE_POOL_SIZE: usize = 4;
pub(crate) struct RefreshJob {
    project_root: PathBuf,
    read_only: bool,
}

pub(crate) enum Job {
    Refresh(RefreshJob),
    Archive(ArchiveJob),
    Checkout(CheckoutJob),
    Undo(UndoJob),
    Trash(TrashJob),
    Versions(VersionsJob),
    VersionAction(VersionActionJob),
    VersionDiff(VersionDiffJob),
    IndexScan(IndexScanJob),
    IndexRepairAction(IndexRepairActionJob),
    TrashList(TrashListJob),
    TrashAction(TrashActionJob),
    TagList(TagListJob),
    TagAction(TagActionJob),
    GroupAction(GroupActionJob),
    Rename(RenameJob),
    SetAlias(SetAliasJob),
    Promote(PromoteJob),
    Distribute(DistributeJob),
}

pub enum OpOutcome {
    /// 已归档：显示名 / 版本 / 最终相对路径（可能因重名避让与用户填的不同）。
    Archived {
        resource_id: String,
        name: String,
        version: i32,
        rel_path: String,
        /// 撤销凭据（**只有首次归档才给**：再归档的"撤销"是版本回退，不在这条路上）。
        undo: Option<ArchiveUndo>,
        /// 附属项（标签 / 分组）未落上的说明（空 = 全部落上；归档本身已成功）。
        notes: Vec<String>,
    },
    /// 已取回：落地路径 / 存档版本 / 是否要顺手打开。
    CheckedOut {
        dest: PathBuf,
        version: i32,
        open_after: bool,
    },
    /// 已重命名：显示名（新）——回执只说结果，旧名由事件路径从面板快照取。
    Renamed { name: String },
    /// 别名已更新（`note` 已是一句有信息量的话：设了 / 清了）。
    AliasDone { note: String },
    /// 已撤销归档：显示名（本体已回原位）。
    Undone { name: String },
    /// 已移入回收站：显示名列表（多条时文案只说数量）。
    Trashed { names: Vec<String> },
    /// 版本历史动作完成（`note` 已是一句有信息量的话，事件路径直接印）。
    VersionActionDone { note: String },
    /// 索引修复动作完成（同上）。
    RepairDone { note: String },
    /// 回收站动作完成（还原 / 永久删除 / 清空，`note` 已是一句有信息量的话）。
    TrashDone { note: String },
    /// 标签动作完成（打标 / 去标 / 新建，`note` 已是一句有信息量的话）。
    TagDone { note: String },
    /// 分组动作完成（建 / 改名 / 删 / 移动，`note` 已是一句有信息量的话）。
    GroupDone { note: String },
    /// 已升级到全局：显示名 / 版本（§13，表 + 版本信息进全局 DuckDB）。
    Promoted { name: String, version: i32 },
    /// 已从全局分发到当前项目：显示名 / 版本 / 形态（表 or 文件）。
    Distributed {
        name: String,
        version: i32,
        shape: crate::global::DistributeShape,
    },
    /// 失败：动作名 + 原因（原因原样来自服务层，已含可操作信息）。
    Failed {
        action: &'static str,
        reason: String,
    },
    /// 同名冲突：事件路径据此弹「覆盖 / 改名 / 取消」（§13.3 / §13.6）。
    ///
    /// 上下文带重试凭据：覆盖 = 带 `overwrite` 重入队；改名 = 换新名后非覆盖重入队；
    /// 取消 = 只回执，不再入队。
    Conflict {
        context: ConflictContext,
        /// 冲突名（对话框标题与回执用；改名后以新名为准）。
        name: String,
    },
}

/// 冲突重试凭据（覆盖 / 改名重入队所需的原始上下文；随回执回事件路径）。
///
/// 拆分 jobs.rs 时曾丢失 `Clone`，workbench 两个调用点（resources.rs / panels.rs）
/// 依赖它，编译回归；此处恢复拆分前契约。
#[derive(Debug, Clone)]
pub enum ConflictContext {
    /// 升级冲突：原始请求（改名 = 复制一份、替换 `name` 后重入队）。
    Promote(crate::global::PromoteRequest),
    /// 分发冲突：全局资源名 + 本次目标名（None = 沿用全局名）。
    Distribute {
        global_name: String,
        target: Option<String>,
    },
}

pub(crate) struct Jobs {
    pub(crate) tx: Sender<Job>,
    /// 排队 + 执行中的作业数（面板据它决定是否继续轮询）。
    pub(crate) pending: AtomicUsize,
    /// 快照槽：**只保留最新一份**——面板呈现的是"当前状态"，旧快照没有回放价值。
    result: Mutex<Option<Result<ResourcesSnapshot, String>>>,
    /// 动作回执槽：同样只留最新一份（同一时刻用户只可能刚做完一个动作）。
    op_result: Mutex<Option<OpOutcome>>,
    /// 版本历史槽：只留最新一份（版本对话框一次只开一个）。
    versions: Mutex<Option<Result<VersionRows, String>>>,
    /// 版本 diff 槽：只留最新一份（diff 对话框一次只开一个）。
    version_diff: Mutex<Option<Result<VersionDiffRows, String>>>,
    /// 索引扫描槽：只留最新一份（索引修复对话框一次只开一个）。
    index_scan: Mutex<Option<Result<IndexScanRows, String>>>,
    /// 回收站槽：只留最新一份（回收站对话框一次只开一个）。
    trash_rows: Mutex<Option<Result<TrashDialogSeed, String>>>,
    /// 标签槽：只留最新一份（标签对话框一次只开一个）。
    tag_rows: Mutex<Option<Result<TagRows, String>>>,
}

static JOBS: OnceLock<Jobs> = OnceLock::new();

pub(crate) fn jobs() -> &'static Jobs {
    JOBS.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("rds-resource-jobs".to_string())
            .spawn(move || worker(rx))
            .expect("failed to spawn resource jobs worker");
        Jobs {
            tx,
            pending: AtomicUsize::new(0),
            result: Mutex::new(None),
            op_result: Mutex::new(None),
            versions: Mutex::new(None),
            version_diff: Mutex::new(None),
            index_scan: Mutex::new(None),
            trash_rows: Mutex::new(None),
            tag_rows: Mutex::new(None),
        }
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn worker(rx: mpsc::Receiver<Job>) {
    // tokio 运行时随工作线程存活；项目库打开与 store 调用都要它。
    let Ok(rt) = tokio::runtime::Runtime::new() else {
        return;
    };
    while let Ok(job) = rx.recv() {
        match job {
            Job::Refresh(job) => {
                let result = rt.block_on(run_refresh(&job));
                *lock(&jobs().result) = Some(result);
            }
            Job::Archive(job) => {
                let outcome = rt.block_on(run_archive(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 动作之后立刻补一次取数（见模块头注释）。
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::Checkout(job) => {
                let outcome = rt.block_on(run_checkout(&job));
                *lock(&jobs().op_result) = Some(outcome);
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::Undo(job) => {
                let outcome = rt.block_on(run_undo(&job));
                *lock(&jobs().op_result) = Some(outcome);
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::Trash(job) => {
                let outcome = rt.block_on(run_trash(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 本体走了、登记行软删了：列表与计数都要重取（回收站对话框是下一批）。
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::Versions(job) => {
                let result = rt.block_on(load_versions(&job));
                *lock(&jobs().versions) = Some(result);
            }
            Job::VersionAction(job) => {
                let outcome = rt.block_on(run_version_action(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 动作改的是版本与本体：既刷新对话框的行，也刷新主列表。
                let result = rt.block_on(reload_versions_after(
                    job.project_root.clone(),
                    job.resource_id.clone(),
                ));
                *lock(&jobs().versions) = Some(result);
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::VersionDiff(job) => {
                // 只读查看：不改任何数据，只回填 diff 对话框。
                let result = rt.block_on(load_version_diff(&job));
                *lock(&jobs().version_diff) = Some(result);
            }
            Job::IndexScan(job) => {
                let result = rt.block_on(load_index_report(&job));
                *lock(&jobs().index_scan) = Some(result);
            }
            Job::IndexRepairAction(job) => {
                let outcome = rt.block_on(run_repair_action(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 修完立刻重扫：对话框的行集合自己就更新了（可能变空）。
                let result = rt.block_on(reload_index_scan_after(job.project_root.clone()));
                *lock(&jobs().index_scan) = Some(result);
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::TrashList(job) => {
                let result = rt.block_on(load_trash(&job));
                *lock(&jobs().trash_rows) = Some(result);
            }
            Job::TrashAction(job) => {
                let outcome = rt.block_on(run_trash_action(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 三件事都会改变回收站自己（还原少一条、删除少一条、清空全没）：
                // 既重取列表，也重取主列表（还原会把行带回列表）。
                let result = rt.block_on(reload_trash_after(job.project_root.clone()));
                *lock(&jobs().trash_rows) = Some(result);
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::TagList(job) => {
                let result = rt.block_on(load_tag_rows(&job));
                *lock(&jobs().tag_rows) = Some(result);
            }
            Job::TagAction(job) => {
                let outcome = rt.block_on(run_tag_action(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 打标改的是关联与标签词典：既刷新已开的标签对话框，也重取主列表
                // （行的 `tag_ids` 与详情的 chips 都在快照里）。
                let result = rt.block_on(reload_tag_rows_after(
                    job.project_root.clone(),
                    job.targets.clone(),
                ));
                *lock(&jobs().tag_rows) = Some(result);
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::GroupAction(job) => {
                let outcome = rt.block_on(run_group_action(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 分组改的是分区与行的归属：重取主列表就够了（字典也在快照里）。
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::Rename(job) => {
                let outcome = rt.block_on(run_rename(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 改的是行上的显示名（排序也看它）：重取主列表。
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::SetAlias(job) => {
                let outcome = rt.block_on(run_set_alias(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 别名进详情头部：重取主列表（详情就在快照里）。
                refresh_after_op(&rt, job.project_root, job.read_only);
            }
            Job::Promote(job) => {
                let outcome = rt.block_on(run_promote(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 升级只动全局库，项目列表不变，但重取无妨（同一 worker，成本可控）。
                refresh_after_op(&rt, job.project_root, false);
            }
            Job::Distribute(job) => {
                let outcome = rt.block_on(run_distribute(&job));
                *lock(&jobs().op_result) = Some(outcome);
                // 分发把全局表落进项目库：项目列表会出现新行，必须重取。
                refresh_after_op(&rt, job.project_root, false);
            }
        }
        jobs().pending.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::collision_name;

    #[test]
    fn collision_name_keeps_extension_at_the_end() {
        assert_eq!(
            collision_name("月报（工作副本）", Some("sql"), 2),
            "月报（工作副本）-2.sql"
        );
        assert_eq!(collision_name("无扩展名", None, 3), "无扩展名-3");
    }
}

pub use archive::{
    enqueue_archive, enqueue_checkout, enqueue_trash, enqueue_trash_action, enqueue_trash_list,
    enqueue_undo,
};
pub use global::{enqueue_distribute, enqueue_promote};
pub use index::enqueue_index_repair_action;
pub use index::enqueue_index_scan;
pub use rename::{enqueue_rename, enqueue_set_alias};
pub use tag::{
    GroupJobAction, TagJobAction, enqueue_group_action, enqueue_tag_action, enqueue_tag_list,
};
pub use version::{enqueue_version_action, enqueue_version_diff, enqueue_versions};

use self::refresh::{free_dest, open_service, refresh_after_op};
#[cfg(test)]
use self::refresh::collision_name;

pub use self::refresh::{
    drain_index_scan, drain_op, drain_snapshot, drain_tag_rows, drain_trash_rows,
    drain_version_diff, drain_versions, enqueue_refresh, has_pending,
};
