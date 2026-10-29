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
    reload_trash_after, run_archive, run_checkout, run_trash, run_undo, ArchiveJob, CheckoutJob,
    TrashActionJob, TrashJob, TrashListJob, UndoJob, load_trash, run_trash_action,
};
use global::{run_distribute, run_promote, DistributeJob, PromoteJob};
use index::{
    load_index_report, reload_index_scan_after, run_repair_action, IndexRepairActionJob,
    IndexScanJob,
};
use rename::{run_rename, run_set_alias, RenameJob, SetAliasJob};
use tag::{
    load_tag_rows, reload_tag_rows_after, run_group_action, run_tag_action, GroupActionJob,
    TagActionJob, TagListJob,
};
use version::{
    load_version_diff, load_versions, reload_versions_after, run_version_action, VersionActionJob,
    VersionDiffJob, VersionsJob,
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
pub(crate) async fn open_service(
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
pub(crate) fn collision_name(stem: &str, ext: Option<&str>, index: usize) -> String {
    match ext {
        Some(ext) => format!("{stem}-{index}.{ext}"),
        None => format!("{stem}-{index}"),
    }
}

/// 找一个不冲突的落地路径：目标不存在就原样用，否则依次试 `-2`…`-999`。
///
/// 试到上限仍冲突就退回原名——让服务层给出**明确的失败**，而不是无限找一个名字。
pub(crate) fn free_dest(dest: &Path) -> PathBuf {
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

/// 取走最新标签取数结果（未就绪时 `None`）。
pub fn drain_tag_rows() -> Option<Result<TagRows, String>> {
    lock(&jobs().tag_rows).take()
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


pub use archive::{
    enqueue_archive, enqueue_checkout, enqueue_trash, enqueue_trash_action, enqueue_trash_list,
    enqueue_undo,
};
pub use global::{enqueue_distribute, enqueue_promote};
pub use index::enqueue_index_repair_action;
pub use index::enqueue_index_scan;
pub use rename::{enqueue_rename, enqueue_set_alias};
pub use tag::{
    enqueue_group_action, enqueue_tag_action, enqueue_tag_list, GroupJobAction, TagJobAction,
};
pub use version::{
    enqueue_version_action, enqueue_version_diff, enqueue_versions,
};
