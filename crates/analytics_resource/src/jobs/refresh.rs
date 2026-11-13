//! 刷新链路与队列入口：索引扫描、快照刷新、任务入队与结果取回。

use super::*;

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// 动作之后补一次取数（同一个 worker 里，见模块头注释）。
pub(crate) fn refresh_after_op(rt: &tokio::runtime::Runtime, project_root: PathBuf, read_only: bool) {
    let result = rt.block_on(run_refresh(&RefreshJob {
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
    // 迁移账本按 MAX(version) 跳过，老库 / 跳号场景可能漏建 027–030 的表。
    // 用与迁移同源的 IF NOT EXISTS 补建一遍；只读项目写不了 DDL 时只告警，不阻断浏览。
    if let Err(error) = store.ensure_m6_tables().await {
        tracing::warn!(error = %error, "资产库扩展表自检未通过，语义写入可能失败");
    }
    let service = ArchiveService::new(project_root, store);
    Ok(match keep_versions {
        Some(keep) => service.with_keep_versions(keep),
        None => service,
    })
}

/// 业务时间窗是否明显过期：只认文本里出现的四位年份（如 `2026Q3` / `2026-01`）。
///
/// 认不出来就当不过期——主动校验宁可少报，也不能凭猜测把正常结论标成失效。
/// 逐字节滑窗再 `from_utf8`，避免切在中文字符中间 panic。
fn expired_time_window(window: &str, now_year: i32) -> bool {
    let bytes = window.as_bytes();
    if bytes.len() < 4 {
        return false;
    }
    for index in 0..=bytes.len() - 4 {
        let Some(slice) = bytes.get(index..index + 4) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(slice) else {
            continue;
        };
        if let Ok(year) = text.parse::<i32>() {
            if (1900..=2100).contains(&year) {
                return year < now_year;
            }
        }
    }
    false
}

/// 每个项目根只重放一次（刷新在每次动作后都会跑，不能反复覆盖本体）。
static STAGING_REPLAYED: OnceLock<Mutex<HashSet<std::path::PathBuf>>> = OnceLock::new();

/// 重放上次「索引事务已提交、本体替换失败」留下的暂存内容。
///
/// 顺序是「暂存 → 索引提交 → 原子替换」，崩溃或占用会留下 staged 文件：
/// 此时索引已是新指纹、本体还是旧内容，indexer 只会在下次扫描报 `ContentChanged`，
/// 用户得手工处理。这里在刷新前做一次**幂等且保守**的补齐：
/// 只有 staged 的 sha256 与当前登记行的 `content_hash` **完全一致**才替换；
/// 对不上（索引回滚了 / 用户又改了内容）就丢弃暂存并告警，绝不猜着覆盖。
pub(super) async fn replay_staging(project_root: &std::path::Path, rows: &[AnalyticsResource]) {
    {
        let replayed = STAGING_REPLAYED.get_or_init(Default::default);
        let mut seen = replayed.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.insert(project_root.to_path_buf()) {
            return;
        }
    }
    let payload = PayloadStore::new(project_root);
    for item in payload.list_staged() {
        let Some(row) = rows
            .iter()
            .find(|row| row.file_rel_path.as_deref() == Some(item.rel.as_str()))
        else {
            tracing::warn!(rel = %item.rel, path = %item.staged.display(), "暂存找不到对应登记行，丢弃（新内容需重新归档）");
            payload.discard_staged(&item.staged).await;
            continue;
        };
        let hash = match payload.content_hash(&item.staged).await {
            Ok(hash) => hash,
            Err(error) => {
                tracing::error!(error = %error, path = %item.staged.display(), "暂存指纹计算失败，保留待下次处理");
                continue;
            }
        };
        if row.content_hash.as_deref() == Some(hash.as_str()) {
            match payload.commit_staged(&item.staged, &item.rel).await {
                Ok(_) => tracing::info!(rel = %item.rel, "已重放暂存内容，本体与索引恢复一致"),
                Err(error) => {
                    tracing::error!(error = %error, rel = %item.rel, "暂存重放失败，保留文件并交由索引修复")
                }
            }
        } else {
            tracing::warn!(rel = %item.rel, "暂存内容与当前登记指纹不符，丢弃（不做猜测覆盖）");
            payload.discard_staged(&item.staged).await;
        }
    }
}

/// 取行 → 扫描 → 组装快照（工作线程上执行）。
pub(super) async fn run_refresh(job: &RefreshJob) -> Result<ResourcesSnapshot, String> {
    let manager = ProjectDatabaseManager::open(&job.project_root, SQLITE_POOL_SIZE)
        .await
        .map_err(|e| format!("打开项目库失败：{e}"))?;
    let store = AnalyticsResourceStore::new(manager.sqlite_pool());
    let payload = PayloadStore::new(job.project_root.clone());

    let rows = store
        .list_file_archives()
        .await
        .map_err(|e| format!("读取存档列表失败：{e}"))?;
    // 先补齐上次「索引已提交、本体未替换」的暂存内容，再扫描——否则 scan 会把这种
    // 可自动恢复的状态误报成 `ContentChanged` 交给用户手工处理。
    replay_staging(&job.project_root, &rows).await;
    // 版本数**一次查完**：详情面板的"版本"区要按行给历史条数，逐行查会把一次刷新
    // 变成 N+1 次查询（面板展示的是整个列表）。
    let history_counts = store
        .version_counts()
        .await
        .map_err(|e| format!("读取版本历史失败：{e}"))?;
    // 「最近 3 条」明细与版本数**同批**取（同一次刷新的另一张表读，不是一个新 N+1）：
    // 详情面板的版本区既要总数也要三条摘要，分两次逐行查就是 2N 次往返。
    let recent_versions = store
        .recent_versions_by_resource(crate::present::DETAIL_RECENT_VERSIONS)
        .await
        .map_err(|e| format!("读取最近版本失败：{e}"))?;
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

    // 业务断言与效力：一次查完（与标签 / 分组同一纪律，避免详情区 N+1）。
    let assertions = store.all_assertions().await.unwrap_or_else(|error| {
        tracing::warn!(error = %error, "读取业务断言失败，详情与搜索降级为无断言");
        Default::default()
    });
    let lifecycles = store.all_lifecycles().await.unwrap_or_default();

    // 主动校验：业务时间窗明显过期时留一条醒目履历。刷新是高频动作，
    // 所以由 store 用 NOT EXISTS 去重——不会每刷一次就往时间线刷一条重复告警。
    let now_year = Utc::now().format("%Y").to_string().parse::<i32>().unwrap_or(0);
    for (id, assertion) in &assertions {
        let Some(window) = assertion.time_window.as_deref() else {
            continue;
        };
        if !expired_time_window(window, now_year) {
            // 口径已改到未过期的时间窗：撤掉旧告警，别让时间线一直挂着"已过期"。
            if let Err(error) = store.clear_window_validations(id).await {
                tracing::warn!(error = %error, resource_id = %id, "清除过期提示失败");
            }
            continue;
        }
        let version = rows
            .iter()
            .find(|row| row.id == *id)
            .map(|row| row.version)
            .unwrap_or(1);
        let detail = format!("业务时间窗「{window}」已过期，请确认结论是否仍适用");
        if let Err(error) = store.record_validation_if_absent(id, version, &detail).await {
            tracing::warn!(error = %error, resource_id = %id, "记录时间窗校验事件失败");
        }
    }

    Ok(build_snapshot(SnapshotInputs {
        assertions,
        lifecycles,
        resources: &rows,
        global_rows,
        global_resources,
        statuses: &statuses,
        history_counts: &history_counts,
        recent_versions: &recent_versions,
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
pub fn enqueue_refresh(project_root: PathBuf, read_only: bool) {
    jobs().pending.fetch_add(1, Ordering::SeqCst);
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
mod validation_tests {
    use super::expired_time_window;

    #[test]
    fn expired_window_recognizes_year_in_common_formats() {
        assert!(expired_time_window("2020Q3", 2026));
        assert!(expired_time_window("统计区间 2024-01 ~ 2024-12", 2026));
        assert!(!expired_time_window("2026Q3", 2026));
    }

    #[test]
    fn unrecognizable_window_never_reports_expiry() {
        assert!(!expired_time_window("上季度", 2026));
        assert!(!expired_time_window("", 2026));
        assert!(!expired_time_window("中文口径说明", 2026));
    }
}
