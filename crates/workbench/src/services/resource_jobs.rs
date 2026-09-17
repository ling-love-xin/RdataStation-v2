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

use chrono::Utc;
use engine::persistence::project_db::ProjectDatabaseManager;

use analytics_resource::dialogs::index_repair::{RepairAction, RepairRow};
use analytics_resource::dialogs::version::{VersionAction, VersionDialogSeed};
use analytics_resource::payload::PayloadStore;
use analytics_resource::present::{ArchiveStatuses, build_repair_rows, build_snapshot, build_version_rows};
use analytics_resource::resource_view::ResourcesSnapshot;
use analytics_resource::{
    AnalyticsResourceStore, ArchiveKind, ArchiveRequest, ArchiveService, ArchiveStatus, ArchiveUndo,
    CheckoutRequest, IndexIssue, IndexRepair,
};

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

/// 版本历史取数结果（宿主据此开窗，或刷新已经开着的那个窗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRows {
    pub resource_id: String,
    pub seed: VersionDialogSeed,
}

/// 索引扫描结果（宿主据此开窗，或刷新已经开着的那个窗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexScanRows {
    pub rows: Vec<RepairRow>,
}

/// 队列里的作业。
enum Job {
    Refresh(RefreshJob),
    Archive(ArchiveJob),
    Checkout(CheckoutJob),
    Undo(UndoJob),
    Versions(VersionsJob),
    VersionAction(VersionActionJob),
    IndexScan(IndexScanJob),
    IndexRepairAction(IndexRepairActionJob),
}

/// 动作回执（工作线程 → 事件路径）。
///
/// **结果与最终落点在工作线程上定好**（重名避让、版本号都在那里发生），文案由事件路径组装
/// ——它才有项目根可以换算相对路径、才有状态栏可以写。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpOutcome {
    /// 已归档：显示名 / 版本 / 最终相对路径（可能因重名避让与用户填的不同）。
    Archived {
        name: String,
        version: i32,
        rel_path: String,
        /// 撤销凭据（**只有首次归档才给**：再归档的"撤销"是版本回退，不在这条路上）。
        undo: Option<ArchiveUndo>,
    },
    /// 已取回：落地路径 / 存档版本 / 是否要顺手打开。
    CheckedOut {
        dest: PathBuf,
        version: i32,
        open_after: bool,
    },
    /// 已撤销归档：显示名（本体已回原位）。
    Undone { name: String },
    /// 版本历史动作完成（`note` 已是一句有信息量的话，事件路径直接印）。
    VersionActionDone { note: String },
    /// 索引修复动作完成（同上）。
    RepairDone { note: String },
    /// 失败：动作名 + 原因（原因原样来自服务层，已含可操作信息）。
    Failed { action: &'static str, reason: String },
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
    /// 索引扫描槽：只留最新一份（索引修复对话框一次只开一个）。
    index_scan: Mutex<Option<Result<IndexScanRows, String>>>,
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
            index_scan: Mutex::new(None),
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
async fn open_service(project_root: PathBuf) -> Result<ArchiveService, String> {
    let manager = ProjectDatabaseManager::open(&project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|e| format!("打开项目库失败：{e}"))?;
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());
    Ok(ArchiveService::new(project_root, store))
}

/// 执行一次归档。
async fn run_archive(job: &ArchiveJob) -> OpOutcome {
    let name = job.request.name.clone();
    let service = match open_service(job.project_root.clone()).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "归档",
                reason,
            };
        }
    };
    match service.archive(job.request.clone()).await {
        Ok(outcome) => {
            // 首次归档才给撤销凭据：再归档的"撤销"是版本回退（服务层也会挡），不在这条路上。
            let undo = job.request.existing_resource_id.is_none().then(|| ArchiveUndo {
                resource_id: outcome.resource_id.clone(),
                name: name.clone(),
                source_path: job.request.source_path.clone(),
            });
            OpOutcome::Archived {
                name,
                version: outcome.version,
                rel_path: outcome.file_rel_path,
                undo,
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
    let service = match open_service(job.project_root.clone()).await {
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
    let service = match open_service(job.project_root.clone()).await {
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

/// 版本历史取数：当前版本行 + 历史版本行 + 副本清单 → 对话框种子（工作线程上执行）。
///
/// “副本在不在”与“本体在不在”都要问文件系统（`.RSmeta` 与 `resources/`），
/// 所以这一整套也在工作线程上——渲染期零 I/O 的纪律同样适用于对话框。
async fn load_versions(job: &VersionsJob) -> Result<VersionRows, String> {
    let service = open_service(job.project_root.clone()).await?;
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

/// 执行一次版本历史动作（工作线程上执行）。
async fn run_version_action(job: &VersionActionJob) -> OpOutcome {
    let service = match open_service(job.project_root.clone()).await {
        Ok(service) => service,
        Err(reason) => {
            return OpOutcome::Failed {
                action: "版本历史",
                reason,
            };
        }
    };

    match job.action {
        VersionAction::Restore { version } => match service
            .restore_version(&job.resource_id, version)
            .await
        {
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
        },
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
            let dest = free_dest(&dir.join(format!(
                "{}（v{version} 工作副本）{extension}",
                job.name
            )));
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
    let service = match open_service(job.project_root.clone()).await {
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
        // 跳转类动作由宿主在事件路径拦截（不入这个作业）；真走到这里只给一句话。
        RepairAction::OpenVersions { .. } => OpOutcome::RepairDone {
            note: "版本历史已打开".to_string(),
        },
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

    Ok(build_snapshot(
        &rows,
        &statuses,
        &history_counts,
        job.read_only,
        Utc::now(),
    ))
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
pub fn enqueue_refresh(project_root: PathBuf, read_only: bool) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Refresh(RefreshJob {
        project_root,
        read_only,
    }));
}

/// 提交一次归档（**事件路径**调用：对话框确认之后）。
pub fn enqueue_archive(project_root: PathBuf, read_only: bool, request: ArchiveRequest) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Archive(ArchiveJob {
        project_root,
        read_only,
        request,
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
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::VersionAction(VersionActionJob {
        project_root,
        read_only,
        resource_id,
        name,
        action,
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

/// 提交一次索引扫描（**事件路径**调用：状态行「修复…」与面板头「重建索引…」）。
pub fn enqueue_index_scan(project_root: PathBuf) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::IndexScan(IndexScanJob { project_root }));
}

/// 提交一次索引修复动作（**事件路径**调用：索引修复对话框的行内动作）。
pub fn enqueue_index_repair_action(
    project_root: PathBuf,
    read_only: bool,
    action: RepairAction,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::IndexRepairAction(IndexRepairActionJob {
        project_root,
        read_only,
        action,
    }));
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
        assert_eq!(collision_name("月报（工作副本）", Some("sql"), 2), "月报（工作副本）-2.sql");
        assert_eq!(collision_name("无扩展名", None, 3), "无扩展名-3");
    }
}
