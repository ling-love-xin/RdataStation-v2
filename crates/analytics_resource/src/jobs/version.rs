//! 版本历史域（拆分自 jobs.rs）：版本行取数、版本 diff 与版本动作执行。
//!
//! `worker` 按 `Job` 枚举分发到这里；`load_versions` / `load_version_diff` 是只读取数，
//! `run_version_action` 走服务层改版本与副本。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use similar::TextDiff;

use crate::dialogs::version::{VersionAction, VersionDialogSeed};
use crate::dialogs::version_diff::VersionDiffSeed;
use crate::present::{build_timeline_rows, build_version_rows};
use crate::{AnalyticsResource, ArchiveService, KeepVersions};

use super::{Job, OpOutcome, free_dest, jobs, open_service};

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
            rows: build_version_rows(&current, &versions, &copies, payload_ok),
            timeline: build_timeline_rows(&current, &versions, &events),
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
    if diff_text_too_large(&old_text, &new_text) {
        return Err(DIFF_TOO_LARGE.to_string());
    }
    let name = current.name.clone();
    let (old_v, new_v) = (job.old_version, current.version);
    let patch = unified_patch(&name, old_v, new_v, &old_text, &new_text);
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

#[cfg(test)]
mod tests {
    use super::{DIFF_MAX_TEXT_BYTES, diff_text_too_large, unified_patch};

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
