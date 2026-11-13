//! 归档请求：草稿箱批量归档、草稿行点名归档与系统文件归档。

use super::*;
use crate::dialogs::archive::ArchiveDialogResult;

/// 面板头 `＋▾` 第一项：把草稿箱里**全部**可归档文件摆进挑选对话框（默认一条不勾）。
pub(super) fn request_archive_from_drafts(host: &ResourceHost, window: &mut Window, cx: &mut App) {
    let Some(root) = host.require_project("无法归档", cx) else {
        return;
    };
    if host.read_only() {
        host.notice("资产库：项目为只读模式，不能归档", cx);
        return;
    }
    let Some(candidates) = host.draft_candidates(cx) else {
        return;
    };
    if candidates.is_empty() {
        host.notice("资产库：草稿箱里还没有可归档的文件", cx);
        return;
    }

    let handler = pick_handler(
        root,
        host.known_groups(cx),
        host.default_group_for_archive(cx),
        // 多选路径的保留策略按设置批带入（闭包外取值：闭包是 `'static`，不能捕获 `host`）。
        host.backend.keep_versions(cx),
        host.backend.clone(),
        host.read_only(),
    );
    open_draft_pick_dialog(window, cx, PickDialogSeed { candidates }, handler);
}

/// 草稿箱行右键「归档为存档…」：候选由**调用方点名**，且预勾上。
///
/// 与 [`request_archive_from_drafts`] 同一套流程（同一个挑选对话框、同一个确认对话框、
/// 同一份批量入队），差别只有两点：
///
/// - 候选不是"草稿箱里全部"，而是点中的那几行（多选时是整个选择集）；
/// - 落到挑选对话框时**预先勾上**——点名就是"我要归档这些"，对话框只是留个剔除的机会。
///
/// 一条时直接展开确认对话框（与挑选对话框里只勾一条的落点一致，少一次点确认）。
pub(super) fn request_archive_drafts(
    host: &ResourceHost,
    paths: Vec<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(root) = host.require_project("无法归档", cx) else {
        return;
    };
    if host.read_only() {
        host.notice("资产库：项目为只读模式，不能归档", cx);
        return;
    }
    let Some(candidates) = host.draft_candidates(cx) else {
        return;
    };
    // 只认候选里确实有的：点名的是草稿箱的行，而候选是刚读的一次目录快照——
    // 期间被移走 / 删除的路径不能凭旧名单硬归档（那样会把"文件已不在"报成归档失败）。
    let picked = named_candidates(candidates, &paths);

    let known_groups = host.known_groups(cx);
    let default_group = host.default_group_for_archive(cx);
    match picked.as_slice() {
        [] => host.notice(
            "资产库：这些草稿已不在草稿箱里（可能刚被移走或删除）",
            cx,
        ),
        // 一条：直接进完整的归档确认（能改显示名、能看到"目标被占则改名到哪"）。
        [draft] => open_archive_for_draft(
            &root,
            draft,
            &known_groups,
            default_group.as_deref(),
            &host.backend,
            host.read_only(),
            window,
            cx,
        ),
        // 多条：预勾上再摆挑选对话框（列表里没有位置让用户逐个改名，批量用默认名）。
        many => {
            let state = PickDialogState::new(many.len());
            for index in 0..many.len() {
                state.set_selected(index, true);
            }
            open_draft_pick_dialog_with(
                window,
                cx,
                PickDialogSeed {
                    candidates: many.to_vec(),
                },
                state,
                pick_handler(
                    root,
                    known_groups,
                    default_group,
                    host.backend.keep_versions(cx),
                    host.backend.clone(),
                    host.read_only(),
                ),
            );
        }
    }
}

/// 按点名路径从候选里挑（顺序跟候选走，不跟点名走；重复点名只算一次）。
///
/// 抽成纯函数是为了能单测「已不在草稿箱里的路径不会凭旧名单硬归档」这条纪律——
/// 它拦的是"文件刚被移走，归档却按旧快照报成功/失败"这类难复现的坑。
fn named_candidates(candidates: Vec<DraftCandidate>, paths: &[PathBuf]) -> Vec<DraftCandidate> {
    candidates
        .into_iter()
        .filter(|candidate| paths.iter().any(|path| path == &candidate.abs_path))
        .collect()
}

/// 挑选对话框的提交处理：两条入口（面板头全量 / 草稿行点名）共用。
fn pick_handler(
    root: PathBuf,
    known_groups: Vec<GroupOption>,
    default_group: Option<String>,
    keep_versions: KeepVersions,
    backend: Rc<dyn ResourceHostBackend>,
    read_only: bool,
) -> impl Fn(Vec<DraftCandidate>, &mut Window, &mut App) + 'static {
    move |picked, window, cx| match picked.as_slice() {
        // 选一个：展开完整的归档确认（与「从本地文件归档…」同一条路）——
        // 能改显示名、能看到"目标被占则改名到哪"。
        [draft] => open_archive_for_draft(
            &root,
            draft,
            &known_groups,
            default_group.as_deref(),
            &backend,
            read_only,
            window,
            cx,
        ),
        // 选多个：默认名 + 各自来源，逐个入队（列表里没有位置让用户逐个改名）。
        many => archive_many(&root, many, keep_versions, &backend, read_only, cx),
    }
}

/// 单条草稿 → 归档请求（纯函数：对话框结果逐字段透传）。
///
/// 抽成纯函数是为了钉住「对话框里改的东西要落库」：这条路径此前把 `group_id` 写死
/// `None`，用户在归档对话框里选的分组被静默丢掉；服务层 `archive` 是会落
/// `apply_labels(..., req.group_id)` 的，所以丢的是**用户输入**而不是某个默认值。
fn draft_archive_request(
    draft: &DraftCandidate,
    kind: ArchiveKind,
    rel_path: String,
    result: ArchiveDialogResult,
) -> ArchiveRequest {
    ArchiveRequest {
        source_path: draft.abs_path.clone(),
        rel_path,
        name: result.name,
        alias: result.alias,
        kind,
        payload_mode: result.payload_mode,
        analysis: None,
        binding: ArchiveBinding {
            // 归档凭证的"出处"：草稿相对路径（带模块前缀，与文档口径一致）
            promoted_from: Some(format!("{}/{}", SCRATCHPAD_DIR, draft.rel_path)),
            source_connection_id: draft.connection_id.clone(),
            source_table: None,
        },
        tags: result.tags,
        group_id: result.group_id,
        keep_versions: result.keep_versions,
        existing_resource_id: None,
    }
}

/// 单条草稿的完整归档确认（挑选对话框单选 / 草稿行点名单选共用）。
#[allow(clippy::too_many_arguments)]
fn open_archive_for_draft(
    root: &Path,
    draft: &DraftCandidate,
    known_groups: &[GroupOption],
    default_group: Option<&str>,
    backend: &Rc<dyn ResourceHostBackend>,
    read_only: bool,
    window: &mut Window,
    cx: &mut App,
) {
    // 归档继承（规格 §7.4）：标签已由宿主在扫描草稿时预取，用户可改可删。
    let inherited_tags = draft.tags.clone();
    let kind = archive_kind_for(&draft.abs_path);
    let source_abs = draft.abs_path.to_string_lossy().to_string();
    let (seed, resolved_rel) = archive_seed(
        root,
        format!("草稿箱：{}", draft.rel_path),
        &draft.rel_path,
        &draft.display_name,
        draft.connection_id.clone(),
        kind,
        known_groups.to_vec(),
        default_group.map(str::to_string),
        inherited_tags,
        Some(source_abs),
    );
    let backend = backend.clone();
    let draft = draft.clone();
    let root = root.to_path_buf();
    open_archive_dialog(window, cx, seed, move |result, cx| {
        // 数据文件按分析档登记（P4.5）：探测由作业在工作线程上做。
        let assertion = result.assertion.clone();
        let request = draft_archive_request(&draft, kind, resolved_rel.clone(), result);
        submit_archive(&root, read_only, request, assertion, &backend, cx);
    });
}

/// 多条草稿：默认名 + 各自来源，逐个入队。
fn archive_many(
    root: &Path,
    many: &[DraftCandidate],
    keep_versions: KeepVersions,
    backend: &Rc<dyn ResourceHostBackend>,
    read_only: bool,
    cx: &mut App,
) {
    let payload = PayloadStore::new(root.to_path_buf());
    // 保留策略按设置批带入（多选路径没有逐个覆盖的输入框）。
    let mut renamed = 0usize;
    for draft in many {
        let resolved_rel = payload.free_rel_path(&draft.rel_path);
        if resolved_rel != draft.rel_path {
            renamed += 1;
        }
        // 归档继承（规格 §7.4）：多选没有标签输入框，宿主预取的标签直接带入。
        let inherited_tags = draft.tags.clone();
        let request = ArchiveRequest {
            source_path: draft.abs_path.clone(),
            rel_path: resolved_rel,
            name: draft.display_name.clone(),
            alias: None,
            // 数据文件按分析档登记（P4.5）：探测由作业在工作线程上做。
            kind: archive_kind_for(&draft.abs_path),
            payload_mode: PayloadMode::Copy,
            analysis: None,
            binding: ArchiveBinding {
                promoted_from: Some(format!("{}/{}", SCRATCHPAD_DIR, draft.rel_path)),
                source_connection_id: draft.connection_id.clone(),
                source_table: None,
            },
            tags: inherited_tags,
            group_id: None,
            keep_versions: None,
            existing_resource_id: None,
        };
        // 探测标志跟着档位走（分析档才连一次 DuckDB）。
        let analysis_probe = request.kind == ArchiveKind::Analysis && request.analysis.is_none();
        resource_jobs::enqueue_archive(
            root.to_path_buf(),
            read_only,
            request,
            keep_versions,
            analysis_probe,
            None,
        );
    }
    let mut message = format!("资产库：已提交 {} 个归档…", many.len());
    if renamed > 0 {
        message.push_str(&format!("（{renamed} 个目标被占，已自动改名）"));
    }
    backend.refresh_resources(cx);
    say(backend, message, cx);
}

pub(super) fn request_archive_from_file(host: &ResourceHost, window: &mut Window, cx: &mut App) {
    let Some(root) = host.require_project("无法归档", cx) else {
        return;
    };
    if host.read_only() {
        host.notice("资产库：项目为只读模式，不能归档", cx);
        return;
    }
    // 来源：系统文件选择（事件路径上的模态框）。**取消不是错误**：不提示、不入队。
    let Some(source) = host.backend.pick_path() else {
        return;
    };
    let Some(file_name) = source
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
    else {
        host.notice("资产库：归档失败（文件名无法解析）", cx);
        return;
    };
    let default_name = std::path::Path::new(&file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(&file_name)
        .to_string();
    // 目标相对路径：`rel_path` **相对 `resources/`**（`PayloadStore::resolve` 的口径），
    // 这里只给文件名——本地文件没有草稿相对路径可保留。
    let kind = archive_kind_for(&source);
    let source_abs = source.to_string_lossy().to_string();
    let (seed, resolved_rel) = archive_seed(
        &root,
        format!("本地文件：{}", source.display()),
        &file_name,
        &default_name,
        // 本地文件没有"来源草稿的连接"可带出：**不猜**当前活动连接（那不一定是它的来路）。
        None,
        kind,
        host.known_groups(cx),
        host.default_group_for_archive(cx),
        // 本地文件没有草稿标签可继承。
        Vec::new(),
        Some(source_abs),
    );

    let backend = host.backend.clone();
    let read_only = host.read_only();
    open_archive_dialog(window, cx, seed, move |result, cx| {
        let request = ArchiveRequest {
            source_path: source.clone(),
            rel_path: resolved_rel.clone(),
            name: result.name,
            alias: result.alias,
            // 同上：数据文件走分析档（探测在工作线程上做）。
            kind,
            payload_mode: result.payload_mode,
            analysis: None,
            binding: ArchiveBinding {
                promoted_from: None,
                source_connection_id: None,
                source_table: None,
            },
            tags: result.tags,
            group_id: result.group_id,
            keep_versions: result.keep_versions,
            existing_resource_id: None,
        };
        submit_archive(&root, read_only, request, result.assertion.clone(), &backend, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(abs: &str, rel: &str) -> DraftCandidate {
        DraftCandidate {
            abs_path: PathBuf::from(abs),
            rel_path: rel.to_string(),
            display_name: rel.trim_end_matches(".sql").to_string(),
            connection_id: None,
            tags: Vec::new(),
        }
    }

    /// 点名归档只认候选里还在的：草稿箱点的是行，候选是刚读的目录快照——中间被移走 /
    /// 删除的路径不能凭旧名单硬归档（否则会把"文件已不在"报成归档失败）。
    #[test]
    fn named_candidates_keeps_only_drafts_still_in_the_snapshot() {
        let all = vec![
            candidate(r"D:\p\scratchpad\a.sql", "a.sql"),
            candidate(r"D:\p\scratchpad\b.sql", "b.sql"),
        ];
        let picked = named_candidates(
            all,
            &[
                PathBuf::from(r"D:\p\scratchpad\b.sql"),
                // 这条已经不在候选里（刚被移走）：丢掉，不硬归档。
                PathBuf::from(r"D:\p\scratchpad\gone.sql"),
            ],
        );
        assert_eq!(picked.len(), 1, "只留还在草稿箱里的那一条");
        assert_eq!(picked[0].rel_path, "b.sql");
    }

    /// 顺序跟候选走（而不是跟点名的顺序走），重复点名只算一次。
    #[test]
    fn named_candidates_follows_snapshot_order_and_dedups() {
        let all = vec![
            candidate(r"D:\p\scratchpad\a.sql", "a.sql"),
            candidate(r"D:\p\scratchpad\b.sql", "b.sql"),
        ];
        let picked = named_candidates(
            all,
            &[
                PathBuf::from(r"D:\p\scratchpad\b.sql"),
                PathBuf::from(r"D:\p\scratchpad\a.sql"),
                PathBuf::from(r"D:\p\scratchpad\a.sql"),
            ],
        );
        assert_eq!(
            picked
                .iter()
                .map(|c| c.rel_path.as_str())
                .collect::<Vec<_>>(),
            vec!["a.sql", "b.sql"],
            "批量归档的顺序要可复现"
        );
    }

    #[test]
    fn named_candidates_is_empty_when_nothing_matches() {
        let all = vec![candidate(r"D:\p\scratchpad\a.sql", "a.sql")];
        let picked = named_candidates(all, &[PathBuf::from(r"D:\p\scratchpad\zzz.sql")]);
        assert!(picked.is_empty(), "调用方据此给一句「已不在草稿箱里」");
    }

    /// 归档对话框里选的分组要真的落库：这条路径此前把它写死 `None`（用户选了也丢）。
    #[test]
    fn draft_request_keeps_the_group_chosen_in_the_dialog() {
        let draft = candidate(
            r"D:\p\scratchpad\reports\dau_report.sql",
            "reports/dau_report.sql",
        );
        let request = draft_archive_request(
            &draft,
            ArchiveKind::File,
            "reports/dau_report-2.sql".to_string(),
            ArchiveDialogResult {
                name: "dau_report".to_string(),
                alias: Some("日活".to_string()),
                tags: vec!["财务".to_string()],
                group_id: Some("af_reports".to_string()),
                keep_versions: Some(KeepVersions::Keep(3)),
                payload_mode: PayloadMode::Copy,
                assertion: None,
            },
        );
        assert_eq!(
            request.group_id.as_deref(),
            Some("af_reports"),
            "选的分组不能丢"
        );
        assert_eq!(request.alias.as_deref(), Some("日活"));
        assert_eq!(request.tags, vec!["财务".to_string()]);
        assert_eq!(request.keep_versions, Some(KeepVersions::Keep(3)));
        assert_eq!(request.payload_mode, PayloadMode::Copy);
        assert_eq!(request.rel_path, "reports/dau_report-2.sql", "用已避让的落点");
        assert_eq!(request.kind, ArchiveKind::File);
        assert_eq!(
            request.binding.promoted_from.as_deref(),
            Some("scratchpad/reports/dau_report.sql")
        );
    }

    /// 「未分组」也是用户的选择（空 = 不归组），不能顺手补成默认分组；
    /// 「保留历史内容」留空同理（= 跟随设置），不是拍一个数字。
    #[test]
    fn draft_request_leaves_unset_fields_unset() {
        let draft = candidate(r"D:\p\scratchpad\a.sql", "a.sql");
        let request = draft_archive_request(
            &draft,
            ArchiveKind::Analysis,
            "a.sql".to_string(),
            ArchiveDialogResult {
                name: "a".to_string(),
                alias: None,
                tags: Vec::new(),
                group_id: None,
                keep_versions: None,
                payload_mode: PayloadMode::Reference,
                assertion: None,
            },
        );
        assert_eq!(request.group_id, None, "未分组就是不归组");
        assert_eq!(request.keep_versions, None, "留空 = 跟随设置");
        assert_eq!(request.payload_mode, PayloadMode::Reference);
        assert_eq!(request.kind, ArchiveKind::Analysis);
    }
}