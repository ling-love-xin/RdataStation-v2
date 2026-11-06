//! 归档请求：草稿箱批量归档与系统文件归档。

use super::*;

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

        let backend = host.backend.clone();
        let read_only = host.read_only();
        // 分组名单与默认分组都在事件路径上取一份（来自面板快照的字典 + 设置；都不查库）；
        // 闭包里只能拿副本。
        let known_groups = host.known_groups(cx);
        let default_group = host.default_group_for_archive(cx);
        // 多选路径的保留策略按设置批带入（闭包外取值：闭包是 `'static`，不能捕获 `host`）。
        let keep_versions = host.backend.keep_versions(cx);
        open_draft_pick_dialog(
            window,
            cx,
            PickDialogSeed { candidates },
            move |picked, window, cx| match picked.as_slice() {
                // 选一个：展开完整的归档确认（与「从本地文件归档…」同一条路）——
                // 能改显示名、能看到"目标被占则改名到哪"。
                [draft] => {
                    // 归档继承（规格 §7.4）：草稿条目标签预填进归档对话框，用户可改可删；
                    // 读标签失败不阻断归档（继承是"能带就带"）。
                    let inherited_tags = scratchpad::ScratchpadStore::new(root.clone())
                        .tags_of_entry(&draft.rel_path)
                        .map(|tags| tags.into_iter().map(|t| t.name).collect())
                        .unwrap_or_default();
                    let kind = archive_kind_for(&draft.abs_path);
                    let source_abs = draft.abs_path.to_string_lossy().to_string();
                    let (seed, resolved_rel) = archive_seed(
                        &root,
                        format!("草稿箱：{}", draft.rel_path),
                        &draft.rel_path,
                        &draft.display_name,
                        draft.connection_id.clone(),
                        kind,
                        known_groups.clone(),
                        default_group.clone(),
                        inherited_tags,
                        Some(source_abs),
                    );
                    let backend = backend.clone();
                    let draft = draft.clone();
                    // 本回调签名是 `Fn`（对话框可能被调用多次的历史原因）：内层 `move` 闭包
                    // 只能拿副本，不能把外层捕获的 `root` 移走。
                    let root = root.clone();
                    open_archive_dialog(window, cx, seed, move |result, cx| {
                        let request = ArchiveRequest {
                            source_path: draft.abs_path.clone(),
                            rel_path: resolved_rel.clone(),
                            name: result.name,
                            alias: result.alias,
                            // 数据文件按分析档登记（P4.5）：探测由作业在工作线程上做。
                            kind,
                            payload_mode: result.payload_mode,
                            analysis: None,
                            binding: ArchiveBinding {
                                // 归档凭证的"出处"：草稿相对路径（带模块前缀，与文档口径一致）
                                promoted_from: Some(format!(
                                    "{}/{}",
                                    scratchpad::MODULE_DIR_NAME,
                                    draft.rel_path
                                )),
                                source_connection_id: draft.connection_id.clone(),
                                source_table: None,
                            },
                            tags: result.tags,
                            group_id: None,
                            keep_versions: result.keep_versions,
                            existing_resource_id: None,
                        };
                        submit_archive(&root, read_only, request, &backend, cx);
                    });
                }
                // 选多个：默认名 + 各自来源，逐个入队（列表里没有位置让用户逐个改名）。
                many => {
                    let payload = PayloadStore::new(root.clone());
                    // 保留策略按设置批带入（多选路径没有逐个覆盖的输入框）。
                    let mut renamed = 0usize;
                    for draft in many {
                        let resolved_rel = payload.free_rel_path(&draft.rel_path);
                        if resolved_rel != draft.rel_path {
                            renamed += 1;
                        }
                        // 归档继承（规格 §7.4）：多选没有标签输入框，草稿条目标签直接带入；
                        // 读标签失败不阻断归档（继承是"能带就带"）。
                        let inherited_tags = scratchpad::ScratchpadStore::new(root.clone())
                            .tags_of_entry(&draft.rel_path)
                            .map(|tags| tags.into_iter().map(|t| t.name).collect())
                            .unwrap_or_default();
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
                                promoted_from: Some(format!(
                                    "{}/{}",
                                    scratchpad::MODULE_DIR_NAME,
                                    draft.rel_path
                                )),
                                source_connection_id: draft.connection_id.clone(),
                                source_table: None,
                            },
                            tags: inherited_tags,
                            group_id: None,
                            keep_versions: None,
                            existing_resource_id: None,
                        };
                        // 探测标志跟着档位走（分析档才连一次 DuckDB）。
                        let analysis_probe =
                            request.kind == ArchiveKind::Analysis && request.analysis.is_none();
                        resource_jobs::enqueue_archive(
                            root.clone(),
                            read_only,
                            request,
                            keep_versions,
                            analysis_probe,
                        );
                    }
                    let mut message = format!("资产库：已提交 {} 个归档…", many.len());
                    if renamed > 0 {
                        message.push_str(&format!("（{renamed} 个目标被占，已自动改名）"));
                    }
                    backend.refresh_resources(cx);
                    say(&backend, message, cx);
                }
            },
        );
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
            submit_archive(&root, read_only, request, &backend, cx);
        });
}
