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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use engine::persistence::project_db::ProjectDatabaseManager;

use crate::analysis::AnalysisFacts;
use crate::dialogs::index_repair::{RepairAction, RepairRow};
use crate::dialogs::tag::{TagChoice, TagDialogSeed};
use crate::dialogs::trash::{ForeignTrash, TrashAction, TrashDialogSeed, TrashRow};
use crate::dialogs::version::{VersionAction, VersionDialogSeed};
use crate::dialogs::version_diff::VersionDiffSeed;
use crate::payload::PayloadStore;
use crate::present::{
    ArchiveStatuses, PreviewMap, SnapshotInputs, build_repair_rows, build_snapshot,
    build_trash_snapshot, build_version_rows, entity_trash_row, reference_trash_rows,
};
use crate::preview::{self, Preview};
use crate::resource_view::{ArchiveRow, GroupOption, ResourcesSnapshot};
use crate::{
    AnalyticsResource, AnalyticsResourceStore, ArchiveKind, ArchiveRequest, ArchiveService,
    ArchiveStatus, ArchiveUndo, CheckoutRequest, IndexIssue, IndexRepair, KeepVersions, TagTarget,
};

use similar::TextDiff;

/// 连接池大小：与其它项目库访问点一致（`data_source_service` / `workspace_loader` 同为 4）。
const SQLITE_POOL_SIZE: usize = 4;

/// 一次刷新任务（全部是所有权数据：worker 碰不到 `Shared`）。
struct RefreshJob {
    project_root: PathBuf,
    read_only: bool,
}

/// 一次归档任务。
struct ArchiveJob {
    project_root: PathBuf,
    read_only: bool,
    request: ArchiveRequest,
    /// 历史内容保留策略（设置项 `resources.keep_versions`，在入队时从主线程读好）。
    keep_versions: KeepVersions,
    /// 归档前是否**探一次数据文件**（`analysis` 档的采集侧）。
    ///
    /// 为什么要过作业：探测要连 DuckDB、要读文件——不能在事件路径上做（那会卡 UI）。
    /// 探测失败**降级为文件型**而不是拒归档：文件先安全落进 `resources/` 比"结构没探到就不让归档"重要。
    analysis_probe: bool,
}

/// 一次取回（检出）任务。
struct CheckoutJob {
    project_root: PathBuf,
    read_only: bool,
    request: CheckoutRequest,
    /// 是否顺手在编辑器里打开（打开动作由事件路径做——它才有窗口）。
    open_after: bool,
}

/// 一次撤销归档任务。
struct UndoJob {
    project_root: PathBuf,
    read_only: bool,
    undo: ArchiveUndo,
}

/// 一次移入回收站任务（面板多选可以一次提交多条）。
struct TrashJob {
    project_root: PathBuf,
    read_only: bool,
    resource_ids: Vec<String>,
}

/// 一次版本历史取数任务。
struct VersionsJob {
    project_root: PathBuf,
    resource_id: String,
}

/// 一次版本历史动作任务（还原 / 取回该版本 / 删副本）。
struct VersionActionJob {
    project_root: PathBuf,
    read_only: bool,
    resource_id: String,
    /// 存档显示名（回执文案用；事件路径不必再查库）。
    name: String,
    action: VersionAction,
    /// 历史内容保留策略（还原会把当前内容留成副本，之后要按策略裁剪）。
    keep_versions: KeepVersions,
}

/// 一次版本 diff 取数任务（**只读查看**：读两版文本 + 生成 unified patch）。
struct VersionDiffJob {
    project_root: PathBuf,
    resource_id: String,
    /// 对比的旧版本号（版本历史里选的那一版；新侧 = 本体当前版本）。
    old_version: i32,
}

/// 一次索引扫描任务（扫描不改任何状态，所以不带只读标志）。
struct IndexScanJob {
    project_root: PathBuf,
}

/// 一次索引修复动作任务（补登 / 删孤儿记录 / 接受当前内容）。
struct IndexRepairActionJob {
    project_root: PathBuf,
    read_only: bool,
    action: RepairAction,
}

/// 一次回收站取数任务（条目 + 别人条目的统计）。
struct TrashListJob {
    project_root: PathBuf,
}

/// 一次回收站动作任务（还原 / 永久删除 / 清空自己名下的）。
struct TrashActionJob {
    project_root: PathBuf,
    read_only: bool,
    action: TrashAction,
}

/// 一次标签取数任务（全部标签 + 这批目标各自已挂的）。
struct TagListJob {
    project_root: PathBuf,
    targets: Vec<TagTarget>,
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
struct TagActionJob {
    project_root: PathBuf,
    read_only: bool,
    targets: Vec<TagTarget>,
    action: TagJobAction,
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
struct GroupActionJob {
    project_root: PathBuf,
    read_only: bool,
    action: GroupJobAction,
}

/// 版本历史取数结果（宿主据此开窗，或刷新已经开着的那个窗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRows {
    pub resource_id: String,
    pub seed: VersionDialogSeed,
}

/// 版本 diff 取数结果（宿主据此开 diff 对话框）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionDiffRows {
    pub resource_id: String,
    pub seed: VersionDiffSeed,
}

/// 索引扫描结果（宿主据此开窗，或刷新已经开着的那个窗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexScanRows {
    pub rows: Vec<RepairRow>,
}

/// 标签对话框取数结果（宿主据此开窗，或刷新已经开着的那个窗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRows {
    /// 本次编辑的目标（单选一元、多选 N 元）——动作回来后会话靠它认自己。
    pub targets: Vec<TagTarget>,
    pub seed: TagDialogSeed,
}

/// 队列里的作业。
enum Job {
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

/// 一次「升级到全局」任务（§13：项目 → 全局，表 + 版本信息进全局 DuckDB）。
struct PromoteJob {
    project_root: PathBuf,
    request: crate::global::PromoteRequest,
    /// true = 同名冲突经「覆盖」确认后重入队（旧表删、版本接续）。
    overwrite: bool,
}

/// 一次「从全局分发」任务（§13：全局 → 项目；跨项目唯一法定路径）。
struct DistributeJob {
    project_root: PathBuf,
    name: String,
    /// 项目侧落库名（冲突询问「改名」分支填入；None = 沿用全局名）。
    target: Option<String>,
    /// true = 同名冲突经「覆盖」确认后重入队（旧本体移除、登记软删）。
    overwrite: bool,
}

/// 改显示名（不需要对话框取数：目标与当前名字都在事件路径上拿到）。
struct RenameJob {
    project_root: PathBuf,
    read_only: bool,
    resource_id: String,
    name: String,
}

/// 改别名（同上；空串 = 清除）。
struct SetAliasJob {
    project_root: PathBuf,
    read_only: bool,
    resource_id: String,
    alias: String,
}

/// 动作回执（工作线程 → 事件路径）。
///
/// **结果与最终落点在工作线程上定好**（重名避让、版本号都在那里发生），文案由事件路径组装
/// ——它才有项目根可以换算相对路径、才有状态栏可以写。
#[derive(Debug, Clone)]
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
    Promoted {
        name: String,
        version: i32,
    },
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

struct Jobs {
    tx: Sender<Job>,
    /// 排队 + 执行中的作业数（面板据它决定是否继续轮询）。
    pending: AtomicUsize,
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

fn jobs() -> &'static Jobs {
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
                let result = rt.block_on(refresh(&job));
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
                let result = rt.block_on(load_versions(&VersionsJob {
                    project_root: job.project_root.clone(),
                    resource_id: job.resource_id.clone(),
                }));
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
                let result = rt.block_on(load_index_report(&IndexScanJob {
                    project_root: job.project_root.clone(),
                }));
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
                let result = rt.block_on(load_trash(&TrashListJob {
                    project_root: job.project_root.clone(),
                }));
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
                let result = rt.block_on(load_tag_rows(&TagListJob {
                    project_root: job.project_root.clone(),
                    targets: job.targets.clone(),
                }));
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

/// 改一条存档的显示名（工作线程上执行）。
///
/// 走 `rename_resource`（不是 `update_resource`）：**只改名字**，不涨版本、不写版本快照
/// ——改名不是内容变更（见 `resource.rs::rename_resource` 的注释）。
async fn run_rename(job: &RenameJob) -> OpOutcome {
    let manager = match ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE).await {
        Ok(manager) => manager,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "重命名",
                reason: format!("打开项目库失败：{reason}"),
            };
        }
    };
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());
    match store.rename_resource(&job.resource_id, &job.name).await {
        Ok(resource) => OpOutcome::Renamed {
            name: resource.name,
        },
        Err(error) => OpOutcome::Failed {
            action: "重命名",
            reason: error.to_string(),
        },
    }
}

/// 改一条存档的别名（工作线程上执行）：空串 = 清除（存 NULL）。
async fn run_set_alias(job: &SetAliasJob) -> OpOutcome {
    let manager = match ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE).await {
        Ok(manager) => manager,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "编辑别名",
                reason: format!("打开项目库失败：{reason}"),
            };
        }
    };
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());
    let alias = (!job.alias.trim().is_empty()).then(|| job.alias.trim().to_string());
    match store.set_alias(&job.resource_id, alias.as_deref()).await {
        Ok(resource) => OpOutcome::AliasDone {
            note: match resource.alias.as_deref() {
                Some(alias) => format!("已把「{}」的别名设为「{alias}」", resource.name),
                None => format!("已清除「{}」的别名", resource.name),
            },
        },
        Err(error) => OpOutcome::Failed {
            action: "编辑别名",
            reason: error.to_string(),
        },
    }
}

/// 升级到全局（工作线程上执行；§13）。
///
/// 本体 = `{项目}/resources/<payload_rel_path>`（File 档）；`content_hash` 用
/// 面板详情已算好的指纹（归档时算过 sha256），不在 worker 里重算。
/// 形态判定：白名单扩展名（csv/tsv/txt/parquet/xlsx/xls/json/ndjson）→ 数据文件
/// 映射出表；其余（sql/py/md 等）→ 脚本内容表（方案 A）。
async fn run_promote(job: &PromoteJob) -> OpOutcome {
    use crate::global::{GlobalInstanceStore, InstanceOpError};

    let gstore = match GlobalInstanceStore::production().await {
        Ok(store) => store,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "升级到全局",
                reason: format!("打开全局系统库失败：{reason}"),
            };
        }
    };
    let outcome = match job.overwrite {
        true => gstore.promote_overwrite(&job.request).await,
        false => gstore.promote(&job.request).await,
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(InstanceOpError::Conflict { name }) => {
            return OpOutcome::Conflict {
                context: ConflictContext::Promote(job.request.clone()),
                name,
            };
        }
        Err(reason) => {
            return OpOutcome::Failed {
                action: "升级到全局",
                reason: reason.to_string(),
            };
        }
    };
    OpOutcome::Promoted {
        name: outcome.name,
        version: outcome.version,
    }
}

/// 从全局分发到当前项目（工作线程上执行；§13 跨项目唯一法定路径）。
///
/// 项目侧写连接：分发需要项目 DuckDB 写锁，按 M6 后台任务惯例在工作线程里
/// `ProjectDatabaseManager::open`（动作完成后连接即释放，不与宿主主写连接长存并发）。
async fn run_distribute(job: &DistributeJob) -> OpOutcome {
    use crate::global::{DistributeShape, GlobalInstanceStore, InstanceOpError, ProjectDuckConn};

    let gstore = match GlobalInstanceStore::production().await {
        Ok(store) => store,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "从全局分发",
                reason: format!("打开全局系统库失败：{reason}"),
            };
        }
    };
    let manager = match ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE).await {
        Ok(manager) => manager,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "从全局分发",
                reason: format!("打开项目库失败：{reason}"),
            };
        }
    };
    let Some(duckdb) = manager.duckdb_conn() else {
        return OpOutcome::Failed {
            action: "从全局分发",
            reason: "项目 DuckDB 写连接未就绪".to_string(),
        };
    };
    let duck_conn = match duckdb.acquire().await {
        Ok(conn) => conn,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "从全局分发",
                reason: reason.to_string(),
            };
        }
    };
    let project = ProjectDuckConn(duck_conn, manager.sqlite_pool());
    let result = match job.overwrite {
        true => gstore.distribute_overwrite(
            &project,
            &job.project_root,
            &job.name,
            job.target.as_deref(),
        )
        .await,
        false => gstore.distribute(
            &project,
            &job.project_root,
            &job.name,
            job.target.as_deref(),
        )
        .await,
    };
    match result {
        Ok(outcome) => OpOutcome::Distributed {
            name: outcome.name,
            version: outcome.version,
            shape: match outcome.shape {
                DistributeShape::Table => DistributeShape::Table,
                DistributeShape::File => DistributeShape::File,
            },
        },
        Err(InstanceOpError::Conflict { name }) => OpOutcome::Conflict {
            context: ConflictContext::Distribute {
                global_name: job.name.clone(),
                target: job.target.clone(),
            },
            name,
        },
        Err(InstanceOpError::NotFound { name }) => OpOutcome::Failed {
            action: "从全局分发",
            reason: format!("全局实例中没有「{name}」"),
        },
        Err(reason) => OpOutcome::Failed {
            action: "从全局分发",
            reason: reason.to_string(),
        },
    }
}

/// 动作之后补一次取数（同一个 worker 里，见模块头注释）。
fn refresh_after_op(rt: &tokio::runtime::Runtime, project_root: PathBuf, read_only: bool) {
    let result = rt.block_on(refresh(&RefreshJob {
        project_root,
        read_only,
    }));
    *lock(&jobs().result) = Some(result);
}

/// 开一条项目库连接并组装归档服务（工作线程上执行）。
/// 打开项目库并装配归档服务。
///
/// `keep_versions` 为 `None` 时用服务自带的默认（5 份）：**只有会裁剪的作业**才需要把
/// 设置项传进来（归档 / 版本还原），其余作业拿默认值就够。
async fn open_service(
    project_root: PathBuf,
    keep_versions: Option<KeepVersions>,
) -> Result<ArchiveService, String> {
    let manager = ProjectDatabaseManager::open(&project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|e| format!("打开项目库失败：{e}"))?;
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());
    let service = ArchiveService::new(project_root, store);
    Ok(match keep_versions {
        Some(keep) => service.with_keep_versions(keep),
        None => service,
    })
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

async fn run_archive(job: &ArchiveJob) -> OpOutcome {
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
async fn run_checkout(job: &CheckoutJob) -> OpOutcome {
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
async fn run_undo(job: &UndoJob) -> OpOutcome {
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
async fn run_trash(job: &TrashJob) -> OpOutcome {
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

/// 版本历史取数：当前版本行 + 历史版本行 + 副本清单 → 对话框种子（工作线程上执行）。
///
/// “副本在不在”与“本体在不在”都要问文件系统（`.RSmeta` 与 `resources/`），
/// 所以这一整套也在工作线程上——渲染期零 I/O 的纪律同样适用于对话框。
async fn load_versions(job: &VersionsJob) -> Result<VersionRows, String> {
    let service = open_service(job.project_root.clone(), None).await?;
    let current = service
        .store()
        .get_resource_by_id(&job.resource_id)
        .await
        .map_err(|e| format!("读取存档失败：{e}"))?;
    let versions = service
        .store()
        .get_resource_versions(&job.resource_id)
        .await
        .map_err(|e| format!("读取版本历史失败：{e}"))?;
    let copies = service
        .payload()
        .version_copies(&job.resource_id)
        .await
        .map_err(|e| format!("读取历史副本失败：{e}"))?;
    let payload_ok = current
        .file_rel_path
        .as_deref()
        .map(|rel| {
            service
                .payload()
                .resolve(rel)
                .map(|path| path.is_file())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    Ok(VersionRows {
        resource_id: job.resource_id.clone(),
        seed: VersionDialogSeed {
            name: current.name.clone(),
            current_version: current.version,
            rows: build_version_rows(&current, &versions, &copies, payload_ok),
        },
    })
}

/// 取两版文本并生成 unified patch（只读；文本类 / 大小上限在此挡）。
async fn load_version_diff(job: &VersionDiffJob) -> Result<VersionDiffRows, String> {
    let service = open_service(job.project_root.clone(), None).await?;
    let current = service
        .store()
        .get_resource_by_id(&job.resource_id)
        .await
        .map_err(|e| format!("读取存档失败：{e}"))?;
    // 新侧 = 本体当前文本；旧侧 = 选中版本的副本（缺失 = 保留策略已裁剪）。
    let new_text = read_resource_text(&service, &current).await?;
    let old_path = service
        .payload()
        .version_copy_file(&job.resource_id, job.old_version)
        .await
        .map_err(|e| format!("读取历史副本失败：{e}"))?
        .ok_or_else(|| "该版本的内容副本不在（已被保留策略裁剪），无法对比".to_string())?;
    let old_text = read_text_file(&old_path)?;
    if new_text.len() > DIFF_MAX_TEXT_BYTES || old_text.len() > DIFF_MAX_TEXT_BYTES {
        return Err("文件过大，请用编辑器查看差异".to_string());
    }
    let name = current.name.clone();
    let (old_v, new_v) = (job.old_version, current.version);
    let patch = TextDiff::from_lines(&old_text, &new_text)
        .unified_diff()
        .context_radius(3)
        .header(
            &format!("a/{name}@{old_v}"),
            &format!("b/{name}@{new_v}"),
        )
        .to_string();
    Ok(VersionDiffRows {
        resource_id: job.resource_id.clone(),
        seed: VersionDiffSeed {
            name,
            old_version: old_v,
            current_version: new_v,
            patch,
        },
    })
}

/// 读本体的 UTF-8 文本（diff 只对文本类有意义；非 UTF-8 / 本体缺失直接报错）。
async fn read_resource_text(
    service: &ArchiveService,
    current: &AnalyticsResource,
) -> Result<String, String> {
    let rel = current
        .file_rel_path
        .as_deref()
        .ok_or_else(|| "该存档没有本体路径（可能是分析表档），无法对比".to_string())?;
    let path = service
        .payload()
        .resolve(rel)
        .map_err(|e| format!("解析本体路径失败：{e}"))?;
    if !path.is_file() {
        return Err("本体不在位（可能是缺失存档），无法对比".to_string());
    }
    read_text_file(&path)
}

/// 读一个文件为 UTF-8 文本（二进制判为不可读，diff 对二进制无意义）。
fn read_text_file(path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("读取文件失败（{}）：{e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| "该文件不是文本（可能是二进制），无法对比".to_string())
}

/// 执行一次版本历史动作（工作线程上执行）。
async fn run_version_action(job: &VersionActionJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone(), Some(job.keep_versions)).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "版本历史",
                reason,
            };
        }
    };

    match job.action {
        // 对比是只读查看，已在 `request_version_action` 分流（`enqueue_version_diff`），
        // 不会走到动作执行——保留 arm 只为 match 完整。
        VersionAction::Diff { .. } => {
            unreachable!("diff 不走版本动作执行（对比在宿主已分流）")
        }
        VersionAction::Restore { version } => {
            match service.restore_version(&job.resource_id, version).await {
                Ok(outcome) if outcome.created_new_version => OpOutcome::VersionActionDone {
                    note: format!(
                        "已还原「{}」到 v{version}（生成 v{}；历史未改动）",
                        job.name, outcome.version
                    ),
                },
                Ok(_) => OpOutcome::VersionActionDone {
                    note: format!("「{}」v{version} 的内容与当前一致，无需还原", job.name),
                },
                Err(error) => OpOutcome::Failed {
                    action: "还原版本",
                    reason: error.to_string(),
                },
            }
        }
        VersionAction::CheckoutDraft { version } => {
            // 扩展名取自本体路径（显示名可以是中文，扩展名不行）；落点定在草稿箱目录，
            // 重名避让与常规取回同一套（动文件之前定死）。
            let current = match service.store().get_resource_by_id(&job.resource_id).await {
                Ok(current) => current,
                Err(error) => {
                    return OpOutcome::Failed {
                        action: "取回版本",
                        reason: error.to_string(),
                    };
                }
            };
            let extension = current
                .file_rel_path
                .as_deref()
                .and_then(|rel| Path::new(rel).extension())
                .and_then(|ext| ext.to_str())
                .map(|ext| format!(".{ext}"))
                .unwrap_or_default();
            let dir = job.project_root.join(scratchpad::MODULE_DIR_NAME);
            let dest =
                free_dest(&dir.join(format!("{}（v{version} 工作副本）{extension}", job.name)));
            match service
                .checkout_version(&job.resource_id, version, &dest)
                .await
            {
                Ok(_) => {
                    let shown = dest
                        .strip_prefix(&job.project_root)
                        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| dest.to_string_lossy().to_string());
                    OpOutcome::VersionActionDone {
                        note: format!("已取回「{}」v{version} 到 {shown}", job.name),
                    }
                }
                Err(error) => OpOutcome::Failed {
                    action: "取回版本",
                    reason: error.to_string(),
                },
            }
        }
        VersionAction::DeleteCopy { version } => {
            match service
                .payload()
                .delete_version_copy(&job.resource_id, version)
                .await
            {
                Ok(true) => OpOutcome::VersionActionDone {
                    note: format!(
                        "已删除「{}」v{version} 的内容副本（版本记录保留）",
                        job.name
                    ),
                },
                Ok(false) => OpOutcome::VersionActionDone {
                    note: format!("「{}」v{version} 已经没有内容副本", job.name),
                },
                Err(error) => OpOutcome::Failed {
                    action: "删除内容副本",
                    reason: error.to_string(),
                },
            }
        }
    }
}

/// 索引扫描 → 修复行（工作线程上执行）。
///
/// 要逐个本体算 sha256（“指纹不匹配”那一档的代价），所以必须离 UI 线程；
/// 与 `refresh` 各自扫一次（那一份扫是为了行状态与计数），二者都不改任何状态。
async fn load_index_report(job: &IndexScanJob) -> Result<IndexScanRows, String> {
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
async fn run_repair_action(job: &IndexRepairActionJob) -> OpOutcome {
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

/// 取回收站条目 → 对话框种子（工作线程上执行）。
///
/// 两个来源（§12.1.1 ③）：
/// - **实体条目**：`ProjectTrash` 目录里的 copy 型本体（回收站是文件系统目录，不碰项目库）；
/// - **软删登记行**：reference 型没有本体可搬，回收站清单要把软删登记行一并列出。
/// 两路都按删除时间倒序返回，这里双指针归并成一份统一倒序清单。
async fn load_trash(job: &TrashListJob) -> Result<TrashDialogSeed, String> {
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
async fn run_trash_action(job: &TrashActionJob) -> OpOutcome {
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

/// 取标签词典 + 这批目标各自已挂的 → 对话框取数结果（工作线程上执行）。
///
/// 多选的两份集合：**全有**（交集，显示为勾上）与**部分**（并集 − 交集，显示为「部分」）。
/// 交集为空说明这批目标没有共同标签；并集为空说明都没打过标签——两种都不算错误。
async fn load_tag_rows(job: &TagListJob) -> Result<TagRows, String> {
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
async fn run_tag_action(job: &TagActionJob) -> OpOutcome {
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
async fn run_group_action(job: &GroupActionJob) -> OpOutcome {
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

/// 取行 → 扫描 → 组装快照（工作线程上执行）。
async fn refresh(job: &RefreshJob) -> Result<ResourcesSnapshot, String> {
    let manager = ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|e| format!("打开项目库失败：{e}"))?;
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());
    let payload = PayloadStore::new(job.project_root.clone());

    let rows = store
        .list_file_archives()
        .await
        .map_err(|e| format!("读取存档列表失败：{e}"))?;
    // 版本数**一次查完**：详情面板的"版本"区要按行给历史条数，逐行查会把一次刷新
    // 变成 N+1 次查询（面板展示的是整个列表）。
    let history_counts = store
        .version_counts()
        .await
        .map_err(|e| format!("读取版本历史失败：{e}"))?;
    // 标签两件事都**一次查完**（与版本数同理）：按行的映射给详情 chips 与筛选维，
    // 词典给筛选菜单；两者都做成逐行查询就是 2N 次往返。
    let tags_by_resource = store
        .tags_by_resource()
        .await
        .map_err(|e| format!("读取存档标签失败：{e}"))?;
    let tag_dictionary = {
        let tags = store
            .list_tags(Some("project"))
            .await
            .map_err(|e| format!("读取标签失败：{e}"))?;
        let counts = store.tag_usage_counts().await.unwrap_or_default();
        crate::present::tag_options(&tags, &counts)
    };
    // 分组同理（一次查完）：按行的映射给分区渲染，字典给分组头与「移动到分组」菜单。
    let folders_by_resource = store
        .folders_by_resource()
        .await
        .map_err(|e| format!("读取存档分组失败：{e}"))?;
    let group_dictionary: Vec<GroupOption> = store
        .list_folders(Some("project"), None)
        .await
        .map_err(|e| format!("读取分组失败：{e}"))?
        .into_iter()
        .map(|folder| GroupOption {
            id: folder.id,
            name: folder.name,
        })
        .collect();
    // 扫描只报告、不改状态（`IndexRepair` 的硬原则），可安全地反复调用。
    let report = IndexRepair::new(&payload, &store)
        .scan()
        .await
        .map_err(|e| format!("扫描本体失败：{e}"))?;

    let mut statuses: ArchiveStatuses = ArchiveStatuses::new();
    for issue in &report.issues {
        let (Some(id), status) = (
            issue.resource_id(),
            match issue {
                // 有记录无本体 / 指纹不匹配：两类都落在行状态上。
                IndexIssue::MissingPayload { .. } => ArchiveStatus::Missing,
                IndexIssue::ContentChanged { .. } => ArchiveStatus::ContentChanged,
                // 未登记的文件没有行可标（见模块头注释）。
                IndexIssue::UntrackedFile { .. } => continue,
            },
        ) else {
            continue;
        };
        statuses.insert(id.to_string(), status);
    }

    // 内容预览（原型 §3.1）：只给**可预览的那些行**读一次开头（文本扩展名 + 体积不超阀），
    // 读量上限由 `preview` 模块的常量定——面板说“前 20 行”，就由同一处决定读多少。
    let previews = load_previews(&payload, &rows).await;

    // 全局实例行（§13 展示层）：全局登记读失败 / 全局库未初始化都不弄砸项目刷新
    // （全局列表是附加视图，不是项目刷新能否成功的前提）。
    let (global_rows, global_resources) = load_global_rows().await;

    Ok(build_snapshot(SnapshotInputs {
        resources: &rows,
        global_rows,
        global_resources,
        statuses: &statuses,
        history_counts: &history_counts,
        tags: &tags_by_resource,
        tag_dictionary,
        folders: &folders_by_resource,
        group_dictionary,
        previews: &previews,
        read_only: job.read_only,
        now: Utc::now(),
    }))
}

/// 读全局实例行（§13 展示层；工作线程上执行）。
///
/// 全局登记读失败 / 全局系统库未初始化（`get_global_db_manager` 为空）时返回空表：
/// 全局列表是**附加视图**，不参与项目刷新成败。
async fn load_global_rows() -> (Vec<ArchiveRow>, Vec<AnalyticsResource>) {
    use crate::global::{GlobalInstanceStore, list_global_resources, to_global_rows};
    let Ok(store) = GlobalInstanceStore::production().await else {
        return (Vec::new(), Vec::new());
    };
    match list_global_resources(&store) {
        Ok(list) => (to_global_rows(&list, Utc::now()), list),
        Err(_) => (Vec::new(), Vec::new()),
    }
}

/// 读内容预览（工作线程上执行）：只给可预览的行读开头。
///
/// 三条“不做”：**不读不可预览的行**（二进制扩展名 / 大文件 / 没登记路径）、
/// **不读整文件**（只读头 `PREVIEW_READ_BYTES`）、**不因预览失败而弄砸刷新**
/// （读不到只是这个档案没有预览——它是附加信息，不是刷新能否成功的前提）。
async fn load_previews(payload: &PayloadStore, rows: &[AnalyticsResource]) -> PreviewMap {
    let mut previews = PreviewMap::new();
    for row in rows {
        if ArchiveKind::from_db_str(&row.kind) != ArchiveKind::File {
            continue;
        }
        let Some(rel) = row.file_rel_path.as_deref() else {
            continue;
        };
        // 扩展名以**本体路径**为准（显示名改过可能就不再带扩展名了，而路径不会）。
        if !preview::is_textual(rel) || !preview::size_allows_preview(row.file_size.map(i64::from))
        {
            continue;
        }
        match payload.read_head(rel, preview::PREVIEW_READ_BYTES).await {
            Ok(Some((head, complete))) => {
                previews.insert(row.id.clone(), preview::from_head(&head, complete));
            }
            // 开头就有 NUL：不是给人看的文本，明说“不预览”（与缺省同句，但这里是有依据的）。
            Ok(None) => {
                previews.insert(row.id.clone(), Preview::OnlyMeta(preview::ONLY_META_NOTE));
            }
            Err(error) => {
                tracing::warn!(error = %error, rel = rel, "读内容预览失败，该行按仅元信息展示");
            }
        }
    }
    previews
}

/// 重名避让后的文件名：主名加 `-2` / `-3`… 后缀（原型 §4.2：重复取回自动改名，不静默覆盖）。
///
/// 纯函数（便于单测）：只拼名字，不碰文件系统。
fn collision_name(stem: &str, ext: Option<&str>, index: usize) -> String {
    match ext {
        Some(ext) => format!("{stem}-{index}.{ext}"),
        None => format!("{stem}-{index}"),
    }
}

/// 找一个不冲突的落地路径：目标不存在就原样用，否则依次试 `-2`…`-999`。
///
/// 试到上限仍冲突就退回原名——让服务层给出**明确的失败**，而不是无限找一个名字。
fn free_dest(dest: &Path) -> PathBuf {
    if !dest.exists() {
        return dest.to_path_buf();
    }
    let dir = dest.parent().unwrap_or_else(|| Path::new(""));
    let stem = dest
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("work");
    let ext = dest.extension().and_then(|ext| ext.to_str());
    for index in 2..1000 {
        let candidate = dir.join(collision_name(stem, ext, index));
        if !candidate.exists() {
            return candidate;
        }
    }
    dest.to_path_buf()
}

/// 提交一次刷新（**事件路径**调用：激活面板 / 打开或切换项目 / 归档等变更之后）。
pub fn enqueue_refresh(project_root: PathBuf, read_only: bool) {    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Refresh(RefreshJob {
        project_root,
        read_only,
    }));
}

/// 提交一次「升级到全局」（§13；事件路径调用：右键菜单确认之后）。
///
/// `overwrite = true` 走覆盖式升级（冲突询问「覆盖」确认后重入队）。
pub fn enqueue_promote(
    project_root: PathBuf,
    request: crate::global::PromoteRequest,
    overwrite: bool,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Promote(PromoteJob {
        project_root,
        request,
        overwrite,
    }));
}

/// 提交一次「从全局分发」（§13；事件路径调用：全局实例列表动作之后）。
///
/// `target` = 项目侧落库名（None = 沿用全局名；冲突询问「改名」分支填入）；
/// `overwrite = true` 走覆盖式分发（冲突询问「覆盖」确认后重入队）。
pub fn enqueue_distribute(
    project_root: PathBuf,
    name: String,
    target: Option<String>,
    overwrite: bool,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs()
        .tx
        .send(Job::Distribute(DistributeJob {
            project_root,
            name,
            target,
            overwrite,
        }));
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

/// 提交一次版本历史取数（**事件路径**调用：右键「版本历史…」/ 详情「查看全部…」）。
pub fn enqueue_versions(project_root: PathBuf, resource_id: String) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Versions(VersionsJob {
        project_root,
        resource_id,
    }));
}

/// 提交一次版本历史动作（**事件路径**调用：版本历史对话框的动作栏）。
pub fn enqueue_version_action(
    project_root: PathBuf,
    read_only: bool,
    resource_id: String,
    name: String,
    action: VersionAction,
    keep_versions: KeepVersions,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::VersionAction(VersionActionJob {
        project_root,
        read_only,
        resource_id,
        name,
        action,
        keep_versions,
    }));
}

/// diff 对比的单侧文本大小上限（字节）：超过就不给对比，避免取数线程白算一个大文件。
const DIFF_MAX_TEXT_BYTES: usize = 512 * 1024;

/// 提交一次版本 diff 取数（**事件路径**调用：版本对话框动作栏「与当前版本对比…」）。
///
/// 只读查看：不改任何数据，job 线程读两版文本 + 生成 unified patch 后回填 `version_diff` 槽。
pub fn enqueue_version_diff(project_root: PathBuf, resource_id: String, old_version: i32) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::VersionDiff(VersionDiffJob {
        project_root,
        resource_id,
        old_version,
    }));
}

/// 还有排队或执行中的作业。
pub fn has_pending() -> bool {
    jobs().pending.load(Ordering::SeqCst) > 0
}

/// 取走最新快照（未就绪时 `None`）。
pub fn drain_snapshot() -> Option<Result<ResourcesSnapshot, String>> {
    lock(&jobs().result).take()
}

/// 取走最新动作回执（未就绪时 `None`）。
pub fn drain_op() -> Option<OpOutcome> {
    lock(&jobs().op_result).take()
}

/// 取走最新版本历史取数结果（未就绪时 `None`）。
pub fn drain_versions() -> Option<Result<VersionRows, String>> {
    lock(&jobs().versions).take()
}

/// 取走最新版本 diff 取数结果（未就绪时 `None`）。
pub fn drain_version_diff() -> Option<Result<VersionDiffRows, String>> {
    lock(&jobs().version_diff).take()
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

/// 提交一次分组动作（**事件路径**调用：行菜单「移动到分组」、分组头菜单与分组名对话框）。
pub fn enqueue_group_action(project_root: PathBuf, read_only: bool, action: GroupJobAction) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::GroupAction(GroupActionJob {
        project_root,
        read_only,
        action,
    }));
}

/// 提交一次改名（**事件路径**调用：重命名对话框提交）。
///
/// 不需要取数作业：目标是**已存在**的那一条，新名字就是用户在对话框里填的。
pub fn enqueue_rename(project_root: PathBuf, read_only: bool, resource_id: String, name: String) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Rename(RenameJob {
        project_root,
        read_only,
        resource_id,
        name,
    }));
}

/// 提交一次别名编辑（**事件路径**调用：详情头部点别名）。空串 = 清除。
pub fn enqueue_set_alias(
    project_root: PathBuf,
    read_only: bool,
    resource_id: String,
    alias: String,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::SetAlias(SetAliasJob {
        project_root,
        read_only,
        resource_id,
        alias,
    }));
}

/// 取走最新标签取数结果（未就绪时 `None`）。
pub fn drain_tag_rows() -> Option<Result<TagRows, String>> {
    lock(&jobs().tag_rows).take()
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

/// 取走最新回收站取数结果（未就绪时 `None`）。
pub fn drain_trash_rows() -> Option<Result<TrashDialogSeed, String>> {
    lock(&jobs().trash_rows).take()
}

/// 取走最新索引扫描结果（未就绪时 `None`）。
pub fn drain_index_scan() -> Option<Result<IndexScanRows, String>> {
    lock(&jobs().index_scan).take()
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
