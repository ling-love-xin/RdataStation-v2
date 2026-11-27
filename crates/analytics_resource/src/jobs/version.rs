//! 版本历史域（拆分自 jobs.rs）：版本行取数、版本 diff 与版本动作执行。
//!
//! `worker` 按 `Job` 枚举分发到这里；`load_versions` / `load_version_diff` 是只读取数，
//! `run_version_action` 走服务层改版本与副本。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use similar::TextDiff;

use crate::dialogs::version::{VersionAction, VersionDialogSeed};
use crate::dialogs::version_diff::{VersionDiffKind, VersionDiffSeed};
use crate::present::{build_timeline_rows, build_version_rows};
use crate::{AnalyticsResource, ArchiveKind, ArchiveService};

use super::{Job, OpOutcome, free_dest, jobs, open_service, open_service_with_manager};

/// diff 对比的单侧文本大小上限（字节）：超过就不给对比，避免取数线程白算一个大文件。
const DIFF_MAX_TEXT_BYTES: usize = 512 * 1024;

/// 超限时给用户的话（与阈值同处声明：改阈值时这行文案也在这里一起看）。
const DIFF_TOO_LARGE: &str = "文件过大，请用编辑器查看差异";

/// 一次版本历史取数任务。
pub(crate) struct VersionsJob {
    pub(crate) project_root: PathBuf,
    pub(crate) resource_id: String,
}

/// 一次版本历史动作任务（还原 / 取回该版本 / 删副本）。
pub(crate) struct VersionActionJob {
    pub(crate) project_root: PathBuf,
    pub(crate) read_only: bool,
    pub(crate) resource_id: String,
    /// 存档显示名（回执文案用；事件路径不必再查库）。
    pub(crate) name: String,
    pub(crate) action: VersionAction,
}

/// 一次版本 diff 取数任务（**只读查看**：读两版文本 + 生成 unified patch）。
pub(crate) struct VersionDiffJob {
    pub(crate) project_root: PathBuf,
    pub(crate) resource_id: String,
    /// 用户在版本表里选中的那一版。
    pub(crate) version: i32,
    /// 对比基准（版本表里 Shift+点选的那一版）；None = 本体当前版本。
    pub(crate) against: Option<i32>,
}

/// 版本历史取数结果（宿主据此开窗，或刷新已经开着的那个窗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRows {
    pub resource_id: String,
    pub seed: VersionDialogSeed,
    /// 数据来自全局实例（项目库没有这个 id）：对话框动作栏必须收敛为只读，
    /// 不能把全局行当项目行去还原 / 取回。
    pub global: bool,
}

/// 版本 diff 取数结果（宿主据此开 diff 对话框）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionDiffRows {
    pub resource_id: String,
    pub seed: VersionDiffSeed,
}

/// 版本历史取数：当前版本行 + 历史版本行 + 副本清单 → 对话框种子（工作线程上执行）。
///
/// “副本在不在”与“本体在不在”都要问文件系统（`.RSmeta` 与 `resources/`），
/// 所以这一整套也在工作线程上——渲染期零 I/O 的纪律同样适用于对话框。
pub(crate) async fn load_versions(job: &VersionsJob) -> Result<VersionRows, String> {
    match load_project_versions(job).await {
        Ok(rows) => Ok(rows),
        Err(project_error) => match load_global_versions(job).await {
            Ok(rows) => Ok(rows),
            Err(_) => Err(project_error),
        },
    }
}

/// 项目侧版本链（文件读 `analytics_resource_versions` + `file_asset_versions`，
/// 表读 `table_asset_versions`）。
async fn load_project_versions(job: &VersionsJob) -> Result<VersionRows, String> {
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
    let file_versions = service
        .store()
        .list_file_asset_versions(&job.resource_id)
        .await
        .map_err(|e| format!("读取文件版本索引失败：{e}"))?;
    let is_table =
        crate::ArchiveKind::resolve(&current.kind, current.analytics_table.as_deref())
            == crate::ArchiveKind::AnalyticsTable;
    let table_versions = if is_table {
        service
            .store()
            .table_asset_versions(&job.resource_id)
            .await
            .map_err(|e| format!("读取表快照版本失败：{e}"))?
    } else {
        Vec::new()
    };
    // 表型资产的本体是项目分析库里的一张表（不是文件）：没有 `file_rel_path`，
    // 不能因为“路径为空”就把当前版本标成「内容缺失」。
    let payload_ok = if crate::ArchiveKind::resolve(&current.kind, current.analytics_table.as_deref())
        == crate::ArchiveKind::AnalyticsTable
    {
        true
    } else {
        current
            .file_rel_path
            .as_deref()
            .map(|rel| {
                service
                    .payload()
                    .resolve(rel)
                    .map(|path| path.is_file())
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    };

    // 履历事件与版本行一起在工作线程上取（渲染期零 I/O）。
    // 老库没有事件表时不能让整个对话框打不开：履历为空仍可看版本，只是没有动作记录。
    let mut events = service
        .store()
        .get_resource_events(&job.resource_id)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(error = %error, "读取资产履历失败，时间线只显示版本节点");
            Vec::new()
        });
    // 谱系关系并入同一条时间线：内存里归一成事件节点，呈现层无需认识第二种数据源。
    for reference in service
        .store()
        .get_refs(&job.resource_id)
        .await
        .unwrap_or_default()
    {
        events.push(crate::model::TimelineEvent {
            id: format!("ref-{}-{}", reference.from_id, reference.to_id),
            resource_id: job.resource_id.clone(),
            version: None,
            kind: crate::model::TimelineEventKind::Superseded,
            detail: Some(format!(
                "谱系 · {} → {}",
                reference.from_id, reference.to_id
            )),
            content_hash: None,
            occurred_at: reference.created_at,
        });
    }

    // 取回凭据（030）并入时间线：把「从 vN 取回工作副本 → 再归档生成 vM」显式画出来。
    // 与谱系同一手法——内存里归一成事件节点，呈现层仍只认识一种数据源。
    for receipt in service
        .store()
        .checkouts_for_resource(&job.resource_id)
        .await
        .unwrap_or_default()
    {
        let (Some(consumed_at), Some(new_version)) =
            (receipt.consumed_at, receipt.consumed_version)
        else {
            continue;
        };
        let short = Path::new(&receipt.dest_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "工作副本".to_string());
        events.push(crate::model::TimelineEvent {
            id: format!("rearchived-{}", receipt.dest_path),
            resource_id: job.resource_id.clone(),
            version: Some(new_version),
            kind: crate::model::TimelineEventKind::Rearchived,
            detail: Some(format!(
                "从 v{} 取回「{}」后归档，生成 v{}",
                receipt.from_version, short, new_version
            )),
            content_hash: None,
            occurred_at: consumed_at,
        });
    }

    Ok(VersionRows {
        resource_id: job.resource_id.clone(),
        seed: VersionDialogSeed {
            name: current.name.clone(),
            current_version: current.version,
            rows: build_version_rows(
                &current,
                &versions,
                &file_versions,
                &table_versions,
                payload_ok,
            ),
            timeline: build_timeline_rows(&current, &versions, &events),
        },
        global: false,
    })
}

/// 全局侧版本链：全局表按 asset_id，全局文件按 name 聚合（文件每版一行资源）。
///
/// 全局行没有项目标签 / 分组 / 履历事件，时间线只给版本节点；动作栏由宿主按
/// `VersionRows.global` 收敛为只读 + 对比。
async fn load_global_versions(job: &VersionsJob) -> Result<VersionRows, String> {
    let store = crate::global::GlobalInstanceStore::production()
        .await
        .map_err(|error| format!("全局实例未初始化，无法读取全局版本历史：{error}"))?;
    let current = crate::global::get_global_resource(&store, &job.resource_id)
        .map_err(|error| format!("读取全局资产失败：{error}"))?
        .ok_or_else(|| format!("全局资产 {} 不存在或已删除", job.resource_id))?;
    let is_table =
        ArchiveKind::resolve(&current.kind, current.analytics_table.as_deref())
            == ArchiveKind::AnalyticsTable;
    let table_versions = if is_table {
        store
            .global_table_versions(&current.id)
            .map_err(|error| format!("读取全局表版本链失败：{error}"))?
    } else {
        Vec::new()
    };
    let file_versions = if is_table {
        Vec::new()
    } else {
        store
            .global_file_versions(&current.name)
            .map_err(|error| format!("读取全局文件版本链失败：{error}"))?
    };
    Ok(VersionRows {
        resource_id: current.id.clone(),
        seed: VersionDialogSeed {
            name: current.name.clone(),
            current_version: current.version,
            rows: build_version_rows(
                &current,
                &[],
                &file_versions,
                &table_versions,
                true,
            ),
            // 全局库没有项目侧标签 / 分组 / 履历事件；空时间线不会让对话框打不开。
            timeline: Vec::new(),
        },
        global: true,
    })
}

/// 取两版内容并生成 unified patch（只读；文件文本 / 表快照在此分流）。
pub(crate) async fn load_version_diff(job: &VersionDiffJob) -> Result<VersionDiffRows, String> {
    match load_project_version_diff(job).await {
        Ok(rows) => Ok(rows),
        Err(project_error) => match load_global_version_diff(job).await {
            Ok(rows) => Ok(rows),
            Err(_) => Err(project_error),
        },
    }
}

async fn load_project_version_diff(
    job: &VersionDiffJob,
) -> Result<VersionDiffRows, String> {
    let (service, manager) = open_service_with_manager(job.project_root.clone()).await?;
    let current = service
        .store()
        .get_resource_by_id(&job.resource_id)
        .await
        .map_err(|e| format!("读取存档失败：{e}"))?;
    let (old_v, new_v) = diff_sides(job.version, job.against, current.version)?;
    let kind = ArchiveKind::resolve(&current.kind, current.analytics_table.as_deref());
    let name = current.name.clone();

    if kind == ArchiveKind::AnalyticsTable {
        let versions = service
            .store()
            .table_asset_versions(&job.resource_id)
            .await
            .map_err(|e| format!("读取表快照版本失败：{e}"))?;
        let old_row = versions
            .iter()
            .find(|row| row.version == old_v)
            .ok_or_else(|| format!("v{old_v} 不在表版本链里，无法对比"))?;
        let new_row = versions
            .iter()
            .find(|row| row.version == new_v)
            .ok_or_else(|| format!("v{new_v} 不在表版本链里，无法对比"))?;
        let old_manifest = service
            .store()
            .table_manifest(&old_row.manifest_id)
            .await
            .map_err(|e| format!("读取 v{old_v} manifest 失败：{e}"))?
            .ok_or_else(|| format!("v{old_v} 的 manifest 已缺失，无法对比"))?;
        let new_manifest = service
            .store()
            .table_manifest(&new_row.manifest_id)
            .await
            .map_err(|e| format!("读取 v{new_v} manifest 失败：{e}"))?
            .ok_or_else(|| format!("v{new_v} 的 manifest 已缺失，无法对比"))?;
        let old_snapshot = service
            .store()
            .table_snapshot(&old_row.snapshot_id)
            .await
            .map_err(|e| format!("读取 v{old_v} 快照失败：{e}"))?
            .ok_or_else(|| format!("v{old_v} 的物理快照已缺失，无法对比"))?;
        let new_snapshot = service
            .store()
            .table_snapshot(&new_row.snapshot_id)
            .await
            .map_err(|e| format!("读取 v{new_v} 快照失败：{e}"))?
            .ok_or_else(|| format!("v{new_v} 的物理快照已缺失，无法对比"))?;
        let duckdb = manager
            .duckdb_conn()
            .ok_or_else(|| "项目分析库未打开，无法读取表快照".to_string())?;
        let handle = duckdb
            .acquire()
            .await
            .map_err(|e| format!("获取项目分析库连接失败：{e}"))?;
        let guard = handle.lock().await;
        let conn = guard
            .as_ref()
            .ok_or_else(|| "项目分析库连接已关闭，无法读取表快照".to_string())?;
        let report = crate::table_asset::diff::table_version_diff(
            conn,
            &name,
            old_v,
            new_v,
            &old_manifest,
            &new_manifest,
            &old_snapshot,
            &new_snapshot,
        )
        .map_err(|e| format!("生成表版本差异失败：{e}"))?;
        return Ok(VersionDiffRows {
            resource_id: job.resource_id.clone(),
            seed: VersionDiffSeed {
                name,
                old_version: old_v,
                new_version: new_v,
                patch: report.patch,
                kind: VersionDiffKind::Table,
                summary: Some(report.summary),
            },
        });
    }

    // 新侧若是当前版本就读本体（与旧行为一致）；否则从该版 manifest 物化。
    // 旧侧在当前版本位于两版之间时也按 manifest 读，避免把“当前本体”误当成历史内容。
    let new_text = if new_v == current.version {
        read_resource_text(&service, &current).await?
    } else {
        read_manifest_text(&service, &job.resource_id, new_v).await?
    };
    let old_text = read_manifest_text(&service, &job.resource_id, old_v).await?;
    if diff_text_too_large(&old_text, &new_text) {
        return Err(DIFF_TOO_LARGE.to_string());
    }
    let patch = unified_patch(&name, old_v, new_v, &old_text, &new_text);
    Ok(VersionDiffRows {
        resource_id: job.resource_id.clone(),
        seed: VersionDiffSeed {
            name,
            old_version: old_v,
            new_version: new_v,
            patch,
            kind: VersionDiffKind::File,
            summary: None,
        },
    })
}

/// 全局侧版本对比：表读全局快照 + 全局 DuckDB；文件读全局 CAS 的 manifest 内容。
async fn load_global_version_diff(
    job: &VersionDiffJob,
) -> Result<VersionDiffRows, String> {
    let store = crate::global::GlobalInstanceStore::production()
        .await
        .map_err(|error| format!("全局实例未初始化，无法对比全局版本：{error}"))?;
    let current = crate::global::get_global_resource(&store, &job.resource_id)
        .map_err(|error| format!("读取全局资产失败：{error}"))?
        .ok_or_else(|| format!("全局资产 {} 不存在或已删除", job.resource_id))?;
    let (old_v, new_v) = diff_sides(job.version, job.against, current.version)?;
    let kind = ArchiveKind::resolve(&current.kind, current.analytics_table.as_deref());
    let name = current.name.clone();

    if kind == ArchiveKind::AnalyticsTable {
        let versions = store
            .global_table_versions(&current.id)
            .map_err(|error| format!("读取全局表版本链失败：{error}"))?;
        let old_row = versions
            .iter()
            .find(|row| row.version == old_v)
            .ok_or_else(|| format!("v{old_v} 不在全局表版本链里，无法对比"))?;
        let new_row = versions
            .iter()
            .find(|row| row.version == new_v)
            .ok_or_else(|| format!("v{new_v} 不在全局表版本链里，无法对比"))?;
        let old_manifest = store
            .global_table_manifest(&old_row.manifest_id)
            .map_err(|error| format!("读取 v{old_v} 全局 manifest 失败：{error}"))?
            .ok_or_else(|| format!("v{old_v} 的全局 manifest 已缺失，无法对比"))?;
        let new_manifest = store
            .global_table_manifest(&new_row.manifest_id)
            .map_err(|error| format!("读取 v{new_v} 全局 manifest 失败：{error}"))?
            .ok_or_else(|| format!("v{new_v} 的全局 manifest 已缺失，无法对比"))?;
        let old_snapshot = store
            .global_table_snapshot(&old_row.snapshot_id)
            .map_err(|error| format!("读取 v{old_v} 全局快照失败：{error}"))?
            .ok_or_else(|| format!("v{old_v} 的全局物理快照已缺失，无法对比"))?;
        let new_snapshot = store
            .global_table_snapshot(&new_row.snapshot_id)
            .map_err(|error| format!("读取 v{new_v} 全局快照失败：{error}"))?
            .ok_or_else(|| format!("v{new_v} 的全局物理快照已缺失，无法对比"))?;
        let report = store
            .with_duckdb(|conn| {
                crate::table_asset::diff::table_version_diff(
                    conn,
                    &name,
                    old_v,
                    new_v,
                    &old_manifest,
                    &new_manifest,
                    &old_snapshot,
                    &new_snapshot,
                )
            })
            .await
            .map_err(|error| format!("生成全局表版本差异失败：{error}"))?;
        return Ok(VersionDiffRows {
            resource_id: current.id,
            seed: VersionDiffSeed {
                name,
                old_version: old_v,
                new_version: new_v,
                patch: report.patch,
                kind: VersionDiffKind::Table,
                summary: Some(report.summary),
            },
        });
    }

    let versions = store
        .global_file_versions(&name)
        .map_err(|error| format!("读取全局文件版本链失败：{error}"))?;
    let old_row = versions
        .iter()
        .find(|row| row.version == old_v)
        .ok_or_else(|| format!("v{old_v} 不在全局文件版本链里，无法对比"))?;
    let new_row = versions
        .iter()
        .find(|row| row.version == new_v)
        .ok_or_else(|| format!("v{new_v} 不在全局文件版本链里，无法对比"))?;
    let old_text = read_global_manifest_text(&store, &old_row.manifest_id).await?;
    let new_text = read_global_manifest_text(&store, &new_row.manifest_id).await?;
    if diff_text_too_large(&old_text, &new_text) {
        return Err(DIFF_TOO_LARGE.to_string());
    }
    let patch = unified_patch(&name, old_v, new_v, &old_text, &new_text);
    Ok(VersionDiffRows {
        resource_id: current.id,
        seed: VersionDiffSeed {
            name,
            old_version: old_v,
            new_version: new_v,
            patch,
            kind: VersionDiffKind::File,
            summary: None,
        },
    })
}

/// 从全局 CAS 读某一版的首个内容条目（与项目侧 `read_manifest_text` 同口径）。
async fn read_global_manifest_text(
    store: &crate::global::GlobalInstanceStore,
    manifest_id: &str,
) -> Result<String, String> {
    let cas = store.file_cas();
    let manifest = cas
        .read_manifest(manifest_id)
        .await
        .map_err(|error| format!("读取全局 v{manifest_id} manifest 失败：{error}"))?;
    let entry = manifest
        .content_entry()
        .ok_or_else(|| "该全局版本没有内容条目，无法对比".to_string())?;
    let path = cas
        .blob_path(&entry.hash)
        .map_err(|error| format!("解析全局内容对象失败：{error}"))?;
    if !path.is_file() {
        return Err(format!("全局内容对象 {} 不在位，无法对比", path.display()));
    }
    read_text_file(&path)
}

/// 选中的两版按版本号排成「旧侧 → 新侧」：diff 永远读旧→新，跟点选顺序无关。
///
/// 同一版不弹 diff（用户 Shift+点了当前选中行，或两次点到同一行）。
fn diff_sides(selected: i32, against: Option<i32>, current: i32) -> Result<(i32, i32), String> {
    let other = against.unwrap_or(current);
    if selected == other {
        return Err("两版相同，没有可对比的差异".to_string());
    }
    Ok(if selected < other {
        (selected, other)
    } else {
        (other, selected)
    })
}

/// 从某一版的 manifest 物化出首个内容条目并读成文本（无 manifest = 旧资产，拒绝对比）。
async fn read_manifest_text(
    service: &ArchiveService,
    resource_id: &str,
    version: i32,
) -> Result<String, String> {
    let path = service
        .version_file(resource_id, version)
        .await
        .map_err(|e| format!("读取历史 manifest 失败：{e}"))?
        .ok_or_else(|| {
            format!("v{version} 没有 manifest（旧文件资产不再支持对比），无法对比")
        })?;
    read_text_file(&path)
}

/// 单侧文本是否超出对比上限（两侧都查：旧副本可能比本体大得多）。
///
/// 抽成纯函数是为了让"多大算大"这条挡板能被单测锁住——它在消息里对用户承诺了
/// 「请用编辑器查看差异」，阈值一改，文案与行为要一起变。
fn diff_text_too_large(old_text: &str, new_text: &str) -> bool {
    old_text.len() > DIFF_MAX_TEXT_BYTES || new_text.len() > DIFF_MAX_TEXT_BYTES
}

/// 两版文本 → unified patch（头部带 `a/{name}@{旧版本}` / `b/{name}@{新版本}`，上下各 3 行）。
///
/// 纯函数：不碰文件系统、不查库，diff 对话框拿到的就是这段文本（`DiffFile::parse` 再解析）。
fn unified_patch(name: &str, old_version: i32, new_version: i32, old_text: &str, new_text: &str) -> String {
    TextDiff::from_lines(old_text, new_text)
        .unified_diff()
        .context_radius(3)
        .header(
            &format!("a/{name}@{old_version}"),
            &format!("b/{name}@{new_version}"),
        )
        .to_string()
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
pub(crate) async fn run_version_action(job: &VersionActionJob) -> OpOutcome {
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
        // 对比是只读查看，已在 `request_version_action` 分流（`enqueue_version_diff`），
        // 不会走到动作执行——保留 arm 只为 match 完整。
        VersionAction::Diff { .. } => {
            unreachable!("diff 不走版本动作执行（对比在宿主已分流）")
        }
        // 「用 vN 分析」同理：是只读查看，宿主要先取副本路径再挂视图，
        // 在 `request_version_action` 已分流，不会送到这里。
        VersionAction::Analyze { .. } => {
            unreachable!("analyze 不走版本动作执行（宿主已分流）")
        }
        // 固化只对全局引用资产，宿主分流到 `enqueue_solidify`；普通版本作业不会收到。
        VersionAction::Solidify { .. } => {
            unreachable!("solidify 不走版本动作执行（宿主已分流到引用固化作业）")
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
                    note: format!("「{}」已经指向 v{version} 的内容，无需还原", job.name),
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
            let dir = job.project_root.join(crate::resource_host::SCRATCHPAD_DIR);
            let dest =
                free_dest(&dir.join(format!("{}（v{version} 工作副本）{extension}", job.name)));
            match service
                .checkout_version(&job.resource_id, version, &dest)
                .await
            {
                Ok(_) => {
                    // 取回历史版本同样是「取回」：凭此凭据改完再归档也会回到原存档升版，
                    // 回溯起点就是用户选中的那个历史版本号。
                    if let Err(error) = service
                        .store()
                        .record_checkout(&dest, &job.resource_id, version)
                        .await
                    {
                        tracing::error!(error = %error, "历史版本取回凭据写入失败");
                    }
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
    }
}

/// 提交一次版本历史取数（**事件路径**调用：右键「版本历史…」/ 详情「查看全部…」）。
pub fn enqueue_versions(project_root: PathBuf, resource_id: String) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::Versions(VersionsJob {
        project_root,
        resource_id,
    }));
}

/// 历史版本 manifest 的只读物化文件（供「用 vN 分析」使用；`None` = 该版本没有 manifest）。
pub async fn version_file(
    project_root: PathBuf,
    resource_id: &str,
    version: i32,
) -> Result<Option<PathBuf>, shared::error::CoreError> {
    let service = open_service(project_root).await.map_err(|reason| {
        shared::error::CoreError::common(shared::error::CommonError::General(reason))
    })?;
    service.version_file(resource_id, version).await
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

/// 提交一次版本 diff 取数（**事件路径**调用：版本对话框动作栏「与…对比…」）。
///
/// 只读查看：不改任何数据，job 线程读两版文本 + 生成 unified patch 后回填 `version_diff` 槽。
/// `against=None` = 与本体当前版本比；`Some(v)` = 与指定版本比（Shift+点选的基准）。
pub fn enqueue_version_diff(
    project_root: PathBuf,
    resource_id: String,
    version: i32,
    against: Option<i32>,
) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
    let _ = jobs().tx.send(Job::VersionDiff(VersionDiffJob {
        project_root,
        resource_id,
        version,
        against,
    }));
}

/// 动作后重取版本历史行（worker 里 VersionAction 分支用；构造在域内，字段不外露）。
pub(crate) async fn reload_versions_after(
    project_root: PathBuf,
    resource_id: String,
) -> Result<VersionRows, String> {
    load_versions(&VersionsJob {
        project_root,
        resource_id,
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::{DIFF_MAX_TEXT_BYTES, diff_sides, diff_text_too_large, unified_patch};

    /// 任意两版都按版本号排成旧→新：点选顺序不影响 diff 方向。
    #[test]
    fn diff_sides_orders_versions_regardless_of_click_order() {
        assert_eq!(diff_sides(5, Some(2), 9).unwrap(), (2, 5), "先点新版再点旧版");
        assert_eq!(diff_sides(2, Some(5), 9).unwrap(), (2, 5), "先点旧版再点新版");
        assert_eq!(diff_sides(4, None, 7).unwrap(), (4, 7), "不设基准 = 与当前比");
    }

    /// 同一版不弹 diff；Shift+点当前选中行也走这条。
    #[test]
    fn diff_sides_rejects_same_version() {
        assert!(diff_sides(3, Some(3), 7).is_err());
        assert!(diff_sides(7, None, 7).is_err(), "当前版本不能自己跟自己比");
    }

    /// 头部两行是 diff 对话框标题之外**唯一**的版本出处：
    /// `a/名字@旧版`、`b/名字@新版`（`DiffFile::parse` 靠它认出文件名与两侧版本）。
    #[test]
    fn patch_header_carries_name_and_both_versions() {
        let patch = unified_patch("dau.sql", 3, 5, "select 1;\n", "select 2;\n");
        let lines: Vec<&str> = patch.lines().collect();
        assert_eq!(lines[0], "--- a/dau.sql@3", "旧侧头：名字 + 旧版本号");
        assert_eq!(lines[1], "+++ b/dau.sql@5", "新侧头：名字 + 当前版本号");
        // hunk 头形如 `@@ -1,1 +1,1 @@`（不同版本可能省略 `,1`）：只断两侧行号起点。
        assert!(
            lines[2].starts_with("@@ -1") && lines[2].contains("+1"),
            "改一行 → 一个从第 1 行起的 hunk：{}",
            lines[2]
        );
        assert_eq!(lines[3], "-select 1;");
        assert_eq!(lines[4], "+select 2;");
    }

    /// 上下各 3 行上下文：改在第 10 行、其余没动，patch 里只出现 3 行前文 / 3 行后文。
    #[test]
    fn patch_keeps_three_lines_of_context() {
        let old: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let mut new: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        new = new.replace("line 10\n", "line 10 changed\n");
        let patch = unified_patch("x.sql", 1, 2, &old, &new);
        assert!(patch.contains(" line 7\n"), "第 10 行前第 3 行在上下文里");
        assert!(!patch.contains(" line 6\n"), "再往前就被裁掉（不是全文件展开）");
        assert!(patch.contains(" line 13\n"), "第 10 行后第 3 行在上下文里");
        assert!(!patch.contains(" line 14\n"), "再往后就被裁掉");
        assert!(patch.contains("-line 10\n"), "被删的那一行");
        assert!(patch.contains("+line 10 changed\n"), "换上的那一行");
        assert_eq!(
            patch.matches("@@").count(),
            2,
            "只有改动附近一个 hunk（两处 `@@` 是同一行的首尾）"
        );
    }

    /// 内容一样 → 只有两行头、零 hunk：对话框据此不会画出一个“空 diff”（没有 +/- 行）。
    #[test]
    fn identical_text_has_header_but_no_hunks() {
        let patch = unified_patch("same.sql", 1, 1, "select 1;\n", "select 1;\n");
        assert!(!patch.contains("@@"), "内容一致就没有 hunk");
        assert_eq!(
            patch.lines().skip(2).count(),
            0,
            "头两行之后没有任何正文行（没有空的 +/-）"
        );
    }

    /// 大小挡板：两侧任一超限都要挡（旧副本可能比本体大得多）。
    #[test]
    fn size_guard_covers_either_side() {
        let small = "select 1;\n";
        let big = "x".repeat(DIFF_MAX_TEXT_BYTES + 1);
        assert!(!diff_text_too_large(small, small), "都小 → 放行");
        assert!(diff_text_too_large(&big, small), "旧侧超限也要挡");
        assert!(diff_text_too_large(small, &big), "新侧超限要挡");
        // 恰好等于上限仍放行（边界值不误伤）。
        let at_limit = "x".repeat(DIFF_MAX_TEXT_BYTES);
        assert!(!diff_text_too_large(&at_limit, small));
    }
}
