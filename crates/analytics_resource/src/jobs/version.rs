//! 版本历史域（拆分自 jobs.rs）：版本行取数、版本 diff 与版本动作执行。
//!
//! `worker` 按 `Job` 枚举分发到这里；`load_versions` / `load_version_diff` 是只读取数，
//! `run_version_action` 走服务层改版本与副本。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use similar::TextDiff;

use crate::dialogs::version::{VersionAction, VersionDialogSeed};
use crate::dialogs::version_diff::VersionDiffSeed;
use crate::present::build_version_rows;
use crate::{AnalyticsResource, ArchiveService, KeepVersions};

use super::{free_dest, jobs, open_service, Job, OpOutcome};

/// diff 对比的单侧文本大小上限（字节）：超过就不给对比，避免取数线程白算一个大文件。
const DIFF_MAX_TEXT_BYTES: usize = 512 * 1024;

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
    /// 历史内容保留策略（还原会把当前内容留成副本，之后要按策略裁剪）。
    pub(crate) keep_versions: KeepVersions,
}

/// 一次版本 diff 取数任务（**只读查看**：读两版文本 + 生成 unified patch）。
pub(crate) struct VersionDiffJob {
    pub(crate) project_root: PathBuf,
    pub(crate) resource_id: String,
    /// 对比的旧版本号（版本历史里选的那一版；新侧 = 本体当前版本）。
    pub(crate) old_version: i32,
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

/// 版本历史取数：当前版本行 + 历史版本行 + 副本清单 → 对话框种子（工作线程上执行）。
///
/// “副本在不在”与“本体在不在”都要问文件系统（`.RSmeta` 与 `resources/`），
/// 所以这一整套也在工作线程上——渲染期零 I/O 的纪律同样适用于对话框。
pub(crate) async fn load_versions(job: &VersionsJob) -> Result<VersionRows, String> {
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
pub(crate) async fn load_version_diff(job: &VersionDiffJob) -> Result<VersionDiffRows, String> {
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
pub(crate) async fn run_version_action(job: &VersionActionJob) -> OpOutcome {
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
