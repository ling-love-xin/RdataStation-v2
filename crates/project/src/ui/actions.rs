//! 动作编排：打开 / 关闭 / 创建 / 重定位 / 删除 / 移出 / 归档 / 项目信息 / 默认连接，
//! 以及选择器的加载与筛选动作。

use super::*;

/// 从示例项目开始（内置最小示例：示例 SQL 草稿）。
pub fn create_sample(host: &ProjectUiHost, window: &mut Window, cx: &mut App) {
    let path = sample_project_dir();
    tracing::debug!(path = %path.display(), "项目选择器「从示例项目开始」触发");
    if !project_service::is_valid_project(&path) {
        let input = project_service::CreateProjectInput::new("示例项目", path.clone())
            .with_description(Some("RdataStation 内置示例".to_string()))
            .with_sample_drafts(true);
        if let Err(e) = project_service::create(input) {
            host.state.borrow_mut().picker.error = Some(e);
            host.notify(cx);
            return;
        }
        // 写入示例草稿（失败不阻断打开）。
        let _ = std::fs::write(
            path.join("welcome.sql"),
            "-- RdataStation 示例项目\n-- 在此编写并执行 SQL。\nSELECT 1 AS hello;\n",
        );
    }
    open_path(host, &path, window, cx);
}

/// 打开项目（含未保存拦截 + 锁占用分支）。
pub fn request_open(host: &ProjectUiHost, path: &Path, window: &mut Window, cx: &mut App) {
    if host.editor.is_dirty() {
        open_unsaved_dialog(
            host,
            PendingAction::OpenPath(path.to_path_buf()),
            window,
            cx,
        );
        return;
    }
    open_path(host, path, window, cx);
}

/// 直接打开（不再拦截）。
pub fn open_path(host: &ProjectUiHost, path: &Path, window: &mut Window, cx: &mut App) {
    tracing::debug!(path = %path.display(), "项目选择器 open_path 打开项目");
    match project_service::open(path) {
        Ok(OpenOutcome::Opened(opened)) => apply_opened(host, opened, cx),
        Ok(OpenOutcome::Busy(info)) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "项目".to_string());
            open_lock_busy_dialog(host, name, path.to_path_buf(), info.pid, window, cx);
        }
        Err(e) => {
            host.state.borrow_mut().picker.error = Some(e);
            host.notify(cx);
        }
    }
}

/// 锁占用逃生口：只读打开 / 仍要打开 / 取消。
pub fn open_lock_busy_dialog(
    host: &ProjectUiHost,
    name: String,
    root: PathBuf,
    pid: u32,
    window: &mut Window,
    cx: &mut App,
) {
    host.state.borrow_mut().dialog_error = None;
    let host = host.clone();
    window.open_alert_dialog(cx, move |alert, _window, cx| {
        let theme = cx.theme();
        let host_ro = host.clone();
        let root_ro = root.clone();
        let host_force = host.clone();
        let root_force = root.clone();
        alert
            .icon(Icon::new(IconName::TriangleAlert).text_color(theme.colors.warning))
            .title(format!("项目「{name}」已在另一实例打开"))
            .description(format!(
                "另一进程（pid {pid}）持有写锁。可只读打开；仅在确认对方已退出时，才选择「仍要打开」。\n\
                 若该进程已经退出（比如上次崩溃 / 断电留下的陈旧锁），锁会被系统自动释放，此时选「仍要打开」即可接管。"
            ))
            .footer(dialog_footer_three(
                "lock-cancel",
                "lock-force",
                "仍要打开",
                "lock-ro",
                "只读打开",
                move |window, cx| {
                    window.close_dialog(cx);
                    // 「仍要打开」：清除陈旧锁文件后重新抢锁（对方已退出的场景）。
                    let _ = std::fs::remove_file(crate::ProjectLock::lock_path(&root_force));
                    open_path(&host_force, &root_force, window, cx);
                },
                move |window, cx| {
                    window.close_dialog(cx);
                    open_read_only(&host_ro, &root_ro, cx);
                },
                |window, cx| window.close_dialog(cx),
            ))
    });
}

/// 未保存草稿拦截：放弃 / 保存并继续 / 取消。
pub fn open_unsaved_dialog(
    host: &ProjectUiHost,
    pending: PendingAction,
    window: &mut Window,
    cx: &mut App,
) {
    host.state.borrow_mut().dialog_error = None;
    let host = host.clone();
    window.open_alert_dialog(cx, move |alert, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        let pending_discard = pending.clone();
        let pending_save = pending.clone();
        alert
            .icon(Icon::new(IconName::TriangleAlert).text_color(theme.colors.warning))
            .title("未保存的草稿")
            .description(
                "编辑区存在未保存的 SQL 草稿。「保存并继续」会另存为项目草稿文件，「放弃更改」则直接执行切换 / 关闭 / 所选项目操作。",
            )
            .when_some(error, |a, e| a.child(dialog_error_line(theme, &e)))
            .footer(dialog_footer_three(
                "unsaved-cancel",
                "unsaved-discard",
                "放弃更改并继续",
                "unsaved-save",
                "保存并继续",
                {
                    let host = host.clone();
                    move |window, cx| {
                        // 先关本对话框再推进动作：推进可能另开对话框（如锁占用），
                        // 否则栈顶变化会让 pop 关错对象。
                        if prepare_unsaved(&host, false, window, cx) {
                            window.close_dialog(cx);
                            advance_pending(&host, &pending_discard, window, cx);
                        }
                    }
                },
                {
                    let host = host.clone();
                    move |window, cx| {
                        if prepare_unsaved(&host, true, window, cx) {
                            window.close_dialog(cx);
                            advance_pending(&host, &pending_save, window, cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
    });
}

/// 只读打开（逃生口）。
pub fn open_read_only(host: &ProjectUiHost, path: &Path, cx: &mut App) {
    match project_service::open_read_only(path) {
        Ok(opened) => apply_opened(host, opened, cx),
        Err(e) => {
            host.state.borrow_mut().picker.error = Some(e);
            host.notify(cx);
        }
    }
}

/// 应用打开结果：写会话 + 持锁 + 自增世代 + 交宿主刷新。
pub(super) fn apply_opened(
    host: &ProjectUiHost,
    opened: project_service::OpenedProject,
    cx: &mut App,
) {
    let (mut store, lock, read_only, summary) = opened.into_parts();

    // 记录「本项目最近由哪个构建打开」——只写不展示（原因见 `BUILD_TAG_KEY` 的文档）。
    // 用打开时已加载好的 store，不额外读盘；值未变则跳过写盘，免得每次打开都改
    // settings.json 的 mtime。只读打开绝不写盘（那也是别人的项目目录）。
    if !read_only && let Some(tag_of) = &host.build_tag {
        let tag = tag_of();
        let unchanged = store
            .load_config()
            .ok()
            .and_then(|config| {
                config
                    .extensions
                    .get(crate::store::BUILD_TAG_KEY)
                    .and_then(|value| value.as_str())
                    .map(|current| current == tag)
            })
            .unwrap_or(false);
        if !unchanged
            && let Err(e) = store.update_config(|config| {
                config.extensions.insert(
                    crate::store::BUILD_TAG_KEY.to_string(),
                    serde_json::Value::String(tag),
                );
            })
        {
            // 配置写不进去不影响会话，记一行即可。
            tracing::warn!("记录打开构建失败: {e}");
        }
    }

    // store 用于确认加载成功并记录构建标识；会话只保留根与名（与既有 P0 会话一致）。
    drop(store);

    host.set_current(Some(OpenProject::new(
        summary.path.clone(),
        summary.name.clone(),
    )));

    {
        let mut ui = host.state.borrow_mut();
        ui.lock = lock;
        ui.read_only = read_only;
        ui.settings_open = false;
        ui.dialog_error = None;
        ui.notice = read_only.then(|| "只读打开：该项目已被另一实例占用".to_string());
        ui.picker.error = None;
    }

    // 切换项目 → 宿主刷新连接列表 / 导航缓存 / 结果归属，并清掉编辑区脏标记。
    (host.on_opened)(cx);
    host.editor.mark_clean();

    host.notify(cx);
}

/// 请求「＋ 新增项目」（含未保存拦截）。
///
/// 有未保存草稿时先走确认对话框，**确认后直接打开新建项目对话框**（不再要求用户回选择器
/// 重新点一次）——连接对话框项目栏的「＋ 新增项目」与标题栏项目菜单共用本入口。
pub fn request_create_project(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) {
    if host.editor.is_dirty() {
        open_unsaved_dialog(
            host,
            PendingAction::CreateProject(inputs.clone()),
            window,
            cx,
        );
        return;
    }
    open_create_dialog(host, inputs, window, cx);
}

/// 请求「打开现有目录…」（含未保存拦截；确认后直接打开目录选择对话框）。
pub fn request_open_folder(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) {
    if host.editor.is_dirty() {
        open_unsaved_dialog(host, PendingAction::OpenFolder(inputs.clone()), window, cx);
        return;
    }
    open_folder_dialog(host, inputs, window, cx);
}

/// 请求关闭项目（含未保存拦截）。
pub fn request_close(host: &ProjectUiHost, window: &mut Window, cx: &mut App) {
    if host.editor.is_dirty() {
        open_unsaved_dialog(host, PendingAction::Close, window, cx);
        return;
    }
    do_close(host, cx);
}

/// 执行关闭：释放锁 + 清会话 + 回选择器。
pub fn do_close(host: &ProjectUiHost, cx: &mut App) {
    let lock = host.state.borrow_mut().lock.take();
    if let Some(lock) = lock {
        let _ = lock.release();
    }
    *host.session.borrow_mut() = None;
    {
        let mut ui = host.state.borrow_mut();
        ui.read_only = false;
        ui.settings_open = false;
        ui.dialog_error = None;
        ui.notice = None;
    }
    refresh_picker(host, cx);
    // #21：关闭项目后清掉该项目的 P_/GP_ 连接（残留行点“连接/编辑”会因
    // `project_root=None` 必失败）；宿主在此按无项目作用域刷新连接列表。
    (host.on_closed)(cx);
}

/// 保存编辑区 SQL 草稿到项目根（唯一命名）。
pub(super) fn save_draft(host: &ProjectUiHost) -> Result<std::path::PathBuf, String> {
    let root = host.root().ok_or_else(|| "未打开项目".to_string())?;
    let sql = host.editor.sql();
    if sql.trim().is_empty() {
        return Err("没有可保存的内容".to_string());
    }
    let mut name = "未命名.sql".to_string();
    let mut i = 1;
    while root.join(&name).exists() {
        name = format!("未命名_{i}.sql");
        i += 1;
    }
    let path = root.join(&name);
    std::fs::write(&path, sql).map_err(|e| format!("保存草稿失败: {e}"))?;
    Ok(path)
}

/// 保存项目设置中的名称与描述（U1 + U2）。
///
/// 会话只存根 / 名，描述与 id 经名册反查；空描述写入 `None`（与创建语义一致）。
pub fn save_project_info(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) {
    if read_only_blocked(host, cx, "修改项目信息") {
        return;
    }
    let name = inputs.rename.read(cx).value().trim().to_string();
    let raw_desc = inputs.description.read(cx).value().trim().to_string();
    let description = (!raw_desc.is_empty()).then_some(raw_desc);
    if let Err(e) = project_service::validate_project_name(&name) {
        notify_error(host, window, cx, e);
        return;
    }
    // 通过名册查当前项目（会话只存根 / 名）。
    let Some(root) = host.root() else { return };
    let Some(id) = current_summary(host).map(|p| p.id) else {
        return;
    };
    match project_service::update_meta(&id, &root, &name, description.as_deref()) {
        Ok(()) => {
            if let Some(session) = host.session.borrow_mut().as_mut() {
                session.name = name;
            }
            notify_success(host, window, cx, "已保存项目信息");
        }
        Err(e) => notify_error(host, window, cx, e),
    }
    // 名册改名 / 描述变更后刷新列表与卡片，并更新设置面板快照（概览要显示新描述）。
    refresh_picker(host, cx);
    load_settings_snapshot(host, cx);
}

/// 归档 / 取消归档当前项目。
pub fn toggle_archive(host: &ProjectUiHost, window: &mut Window, cx: &mut App) {
    if read_only_blocked(host, cx, "归档") {
        return;
    }
    let Some(item) = current_summary(host) else {
        return;
    };
    let archived = item.status == "archived";
    match project_service::set_archived(&item.id, &item.path, !archived) {
        Ok(()) => notify_success(
            host,
            window,
            cx,
            format!("已{}项目", if archived { "取消归档" } else { "归档" }),
        ),
        Err(e) => notify_error(host, window, cx, e),
    }
    host.notify(cx);
}

/// 设置默认连接（U3）：`None` = 清除默认。
///
/// 只动项目本体的 `settings.json`；不写名册（默认连接是项目内偏好）。
pub fn set_default_connection(
    host: &ProjectUiHost,
    connection_id: Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    if read_only_blocked(host, cx, "修改默认连接") {
        return;
    }
    let Some(root) = host.root() else {
        return;
    };
    match project_service::save_default_connection(&root, connection_id.as_deref()) {
        Ok(()) => {
            host.state.borrow_mut().settings.default_connection = connection_id;
            notify_success(host, window, cx, "已更新默认连接");
        }
        Err(e) => notify_error(host, window, cx, e),
    }
    host.notify(cx);
}
