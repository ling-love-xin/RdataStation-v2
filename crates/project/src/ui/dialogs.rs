//! 语义对话框：新建 / 打开文件夹 / 空目录询问 / 锁占用 / 未保存拦截 / 重定位 / 删除数据。
//!
//! 校验提示（`location_hint` / `folder_hint` / `target_preview`）与其缓存（`memo_hint`）
//! 也在这里：它们是对话框的行内提示，判据与提交同源。

use super::*;

// ==================== 语义对话框 ====================

/// 表单字段标签。
pub(super) fn label(theme: &gpui_kit::component::Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 对话框内联错误（校验失败时显示）。
pub(super) fn dialog_error_line(theme: &gpui_kit::component::Theme, message: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.danger)
        .child(message.to_string())
}

/// 打开系统目录选择器，把选中的目录回填到输入框（取消则保持原值）。
///
/// `App::prompt_for_paths` 通过 oneshot 异步回传结果；这里用 `Window::spawn` 在窗口上下文
/// 等待，再用 `AsyncWindowContext::update` 取得 `&mut Window` 回填（`set_value` 需要窗口）。
pub(super) fn pick_directory(target: Entity<InputState>, window: &mut Window, cx: &mut App) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("选择目录".into()),
    });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let value = path.to_string_lossy().to_string();
            let _ = cx.update(|window, cx| {
                target.update(cx, |state, cx| state.set_value(value, window, cx));
            });
        })
        .detach();
}

/// 目录输入行：文本框 + 「浏览…」按钮（系统目录选择器）。
pub(super) fn directory_row(target: &Entity<InputState>, browse_id: &'static str) -> Div {
    let picker_target = target.clone();
    div()
        .h_flex()
        .gap_2()
        .child(div().flex_1().child(Input::new(target)))
        .child(
            Button::new(browse_id)
                .secondary()
                .label("浏览…")
                .on_click(move |_, window, cx| pick_directory(picker_target.clone(), window, cx)),
        )
}

/// 「位置」字段的行内提示（判据与 `submit_create` → `service::create` 完全同源）。
///
/// - 位置为空 → 必填提示；
/// - 位置不存在 → 说清哪个路径；
/// - 名称还不合法（空 / 超长 / 含 `\ / : * ? " < > |`）→ 目标路径还不成立，不预判目录状态
///   （否则一打开对话框就红一片，且含分隔符的名称会把 `join` 拼成子路径、判出错误结论）；
/// - 目标已是项目 / 目标非空 → 直接说清该怎么改（前者应改走「打开」）。
pub(super) fn location_hint(location: &str, name: &str) -> Option<String> {
    let location = location.trim();
    if location.is_empty() {
        return Some("请选择项目位置".to_string());
    }
    let root = PathBuf::from(location);
    if !root.exists() {
        return Some(format!("位置不存在：{}", root.display()));
    }
    let name = name.trim();
    if project_service::validate_project_name(name).is_err() {
        return None;
    }
    match project_service::inspect_target(&root.join(name)) {
        TargetDirState::ExistingProject => Some("该目标目录已是项目，请直接打开".to_string()),
        TargetDirState::NonEmpty => Some("目标目录非空，请换一个名称或选择空目录".to_string()),
        TargetDirState::Missing | TargetDirState::Empty => None,
    }
}

/// 新建项目的目标路径预览（原型要求展示 `位置/名称`）。
pub(super) fn target_preview(
    theme: &gpui_kit::component::Theme,
    location: &str,
    name: &str,
) -> Div {
    let (location, name) = (location.trim(), name.trim());
    // 名称不合法（含分隔符等）时 `join` 会拼出可信度为零的路径，宁可退回占位文案。
    let name_ok = project_service::validate_project_name(name).is_ok();
    let text = if location.is_empty() || !name_ok {
        "目标：填写位置与名称后在此预览".to_string()
    } else {
        format!("目标：{}", PathBuf::from(location).join(name).display())
    };
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text)
}

/// 「打开现有项目」对话框的行内提示（判据与 `submit_open_folder` 同源，文案直接复用）。
///
/// - 位置为空 → 必填；
/// - 目录不存在 → 说清哪个路径；
/// - 非空且无 `.RSmeta` → 说明为什么打不开；
/// - 空目录 → **不提示**（提交时会问「在空目录创建项目？」，不是错误）；
/// - 已是项目 → 不提示（可以打开）。
pub(super) fn folder_hint(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return Some("请输入项目目录".to_string());
    }
    let path = PathBuf::from(path);
    match project_service::inspect_target(&path) {
        TargetDirState::Missing => Some(format!("目录不存在：{}", path.display())),
        TargetDirState::NonEmpty => {
            Some("目录不是项目（缺少 .RSmeta），且非空，无法作为项目打开".to_string())
        }
        TargetDirState::Empty | TargetDirState::ExistingProject => None,
    }
}

/// 渲染期提示缓存（`memo_hint` 的载体）：`Rc` 让每帧重跑的闭包共享，`RefCell` 供渲染期写回。
pub(super) type HintCache<K> = Rc<RefCell<Option<(K, Option<String>)>>>;

/// 渲染闭包里的「目录探测」memo：key 不变就复用上次结果。
///
/// `open_dialog` 的 builder 是每帧重跑的渲染闭包，而 `location_hint` / `folder_hint` 要
/// `exists()` + `read_dir`；把探测结果按输入快照缓存，只有输入变化才真的碰磁盘。
pub(super) fn memo_hint<K: PartialEq>(
    cell: &RefCell<Option<(K, Option<String>)>>,
    key: K,
    compute: impl FnOnce() -> Option<String>,
) -> Option<String> {
    let stale = match cell.borrow().as_ref() {
        Some((cached, _)) => cached != &key,
        None => true,
    };
    if stale {
        let hint = compute();
        *cell.borrow_mut() = Some((key, hint.clone()));
        hint
    } else {
        cell.borrow().as_ref().and_then(|(_, hint)| hint.clone())
    }
}

/// 对话框底部按钮组（右对齐）：取消 + 主操作。
///
/// 关闭时机由回调自行决定（`window.close_dialog`），不使用 `Dialog::on_ok` 的返回值——
/// 这样「提交成功才关闭」与「先关掉本对话框再执行可能另开对话框的动作」两种路径行为一致。
pub(super) fn dialog_footer(
    cancel_id: &'static str,
    ok_id: &'static str,
    ok_label: &'static str,
    ok_variant: ButtonVariant,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
) -> DialogFooter {
    DialogFooter::new()
        .child(
            Button::new(cancel_id)
                .secondary()
                .label("取消")
                .on_click(move |_, window, cx| on_cancel(window, cx)),
        )
        .child(
            Button::new(ok_id)
                .with_variant(ok_variant)
                .label(ok_label)
                .on_click(move |_, window, cx| on_ok(window, cx)),
        )
}

/// 三选一底部按钮组（锁逃生口 / 未保存拦截）：取消 + 次要逃生 + 主操作。
pub(super) fn dialog_footer_three(
    cancel_id: &'static str,
    alt_id: &'static str,
    alt_label: &'static str,
    ok_id: &'static str,
    ok_label: &'static str,
    on_alt: impl Fn(&mut Window, &mut App) + 'static,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
) -> DialogFooter {
    DialogFooter::new()
        .child(
            Button::new(cancel_id)
                .secondary()
                .label("取消")
                .on_click(move |_, window, cx| on_cancel(window, cx)),
        )
        .child(
            Button::new(alt_id)
                .secondary()
                .label(alt_label)
                .on_click(move |_, window, cx| on_alt(window, cx)),
        )
        .child(
            Button::new(ok_id)
                .primary()
                .label(ok_label)
                .on_click(move |_, window, cx| on_ok(window, cx)),
        )
}

/// 打开新建项目对话框（重置表单）。
pub fn open_create_dialog(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .create_name
        .update(cx, |s, cx| s.set_value("", window, cx));
    // 位置默认取「上次新建用的父目录」；没记过才回退用户主目录。
    let initial_location = (host.last_create_location)(cx).unwrap_or_else(default_location);
    inputs
        .create_location
        .update(cx, |s, cx| s.set_value(initial_location, window, cx));
    inputs
        .create_desc
        .update(cx, |s, cx| s.set_value("", window, cx));
    host.state.borrow_mut().dialog_error = None;

    let host = host.clone();
    let inputs = inputs.clone();
    // 行内提示缓存：builder 每帧重跑，`location_hint` 要探测目录（I/O），交给 `memo_hint`
    // 只在「位置 / 名称」变化时重算一次。
    let hint_cache: HintCache<(String, String)> = Rc::new(RefCell::new(None));
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        let location = inputs.create_location.read(cx).value().to_string();
        let name = inputs.create_name.read(cx).value().to_string();
        // 行内提示：与 `submit_create` 同一判据（都用 service 的函数），
        // 不会出现「界面说可以、点创建被拒」。名称没填时不提示（不红一片）。
        let name_hint = (!name.trim().is_empty())
            .then(|| project_service::validate_project_name(&name).err())
            .flatten();
        let location_hint = memo_hint(&hint_cache, (location.clone(), name.clone()), || {
            location_hint(&location, &name)
        });
        dialog
            .title("新建项目")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(label(theme, "项目名称 *"))
                    .child(Input::new(&inputs.create_name))
                    .when_some(name_hint, |d, hint| {
                        d.child(
                            dialog_error_line(theme, &hint)
                                .debug_selector(|| "create-name-hint".into()),
                        )
                    })
                    .child(label(theme, "位置 *"))
                    .child(directory_row(&inputs.create_location, "create-browse"))
                    .when_some(location_hint, |d, hint| {
                        d.child(
                            dialog_error_line(theme, &hint)
                                .debug_selector(|| "create-location-hint".into()),
                        )
                    })
                    .child(target_preview(theme, &location, &name))
                    .child(label(theme, "描述（可选）"))
                    .child(Input::new(&inputs.create_desc))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("当前版本仅支持本地目录；DuckLake 远程项目待后续版本。"),
                    )
                    .when_some(error, |d, e| d.child(dialog_error_line(theme, &e))),
            )
            .footer(dialog_footer(
                "create-cancel",
                "create-ok",
                "创建项目",
                ButtonVariant::Primary,
                {
                    let host = host.clone();
                    let inputs = inputs.clone();
                    move |window, cx| {
                        if submit_create(&host, &inputs, window, cx) {
                            window.close_dialog(cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
            .on_ok({
                let host = host.clone();
                let inputs = inputs.clone();
                move |_, window, cx| {
                    // Enter 与「创建项目」按钮同路径；关闭由提交成功分支显式执行。
                    if submit_create(&host, &inputs, window, cx) {
                        window.close_dialog(cx);
                    }
                    false
                }
            })
    });
}

/// 打开「打开现有目录」对话框。
pub fn open_folder_dialog(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .create_location
        .update(cx, |s, cx| s.set_value(default_location(), window, cx));
    host.state.borrow_mut().dialog_error = None;

    let host = host.clone();
    let inputs = inputs.clone();
    // 与新建对话框同理：builder 每帧重跑，目录探测交给 `memo_hint` 按「路径」缓存。
    let hint_cache: HintCache<String> = Rc::new(RefCell::new(None));
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        let path = inputs.create_location.read(cx).value().to_string();
        let hint = memo_hint(&hint_cache, path.clone(), || folder_hint(&path));
        dialog
            .title("打开现有项目")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(label(theme, "项目目录（含 .RSmeta）"))
                    .child(directory_row(&inputs.create_location, "of-browse"))
                    .when_some(hint, |d, h| {
                        d.child(
                            dialog_error_line(theme, &h)
                                .debug_selector(|| "open-folder-hint".into()),
                        )
                    })
                    .when_some(error, |d, e| d.child(dialog_error_line(theme, &e))),
            )
            .footer(dialog_footer(
                "of-cancel",
                "of-ok",
                "打开",
                ButtonVariant::Primary,
                {
                    let host = host.clone();
                    let inputs = inputs.clone();
                    move |window, cx| {
                        if submit_open_folder(&host, &inputs, window, cx) {
                            window.close_dialog(cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
            .on_ok({
                let host = host.clone();
                let inputs = inputs.clone();
                move |_, window, cx| {
                    if submit_open_folder(&host, &inputs, window, cx) {
                        window.close_dialog(cx);
                    }
                    false
                }
            })
    });
}

/// 从输入读取并创建项目；成功返回 `true`（由调用方关闭对话框）。
pub fn submit_create(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let name = inputs.create_name.read(cx).value().trim().to_string();
    let location = inputs.create_location.read(cx).value().trim().to_string();
    let description = {
        let d = inputs.create_desc.read(cx).value().trim().to_string();
        (!d.is_empty()).then_some(d)
    };

    if let Err(e) = project_service::validate_project_name(&name) {
        set_dialog_error(host, e, cx);
        return false;
    }
    if location.is_empty() {
        set_dialog_error(host, "请选择项目位置".to_string(), cx);
        return false;
    }
    let path = PathBuf::from(&location).join(&name);

    let input = project_service::CreateProjectInput::new(name, path).with_description(description);
    match project_service::create(input) {
        Ok(summary) => {
            // 记住这次选的父目录，下次新建默认带出来（失败只是回退主目录，不阻断）。
            (host.save_create_location)(&location, cx);
            // 创建成功后直接打开（走统一打开路径，取得锁）。
            open_path(host, &summary.path, window, cx);
            true
        }
        Err(e) => {
            set_dialog_error(host, e, cx);
            false
        }
    }
}

/// 从输入读取并打开目录；成功返回 `true`（由调用方关闭对话框）。
pub fn submit_open_folder(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let path = PathBuf::from(inputs.create_location.read(cx).value().trim().to_string());
    if path.as_os_str().is_empty() {
        set_dialog_error(host, "请输入项目目录".to_string(), cx);
        return false;
    }
    match project_service::inspect_target(&path) {
        TargetDirState::Missing => {
            set_dialog_error(host, format!("目录不存在：{}", path.display()), cx);
            false
        }
        TargetDirState::NonEmpty => {
            set_dialog_error(
                host,
                "目录不是项目（缺少 .RSmeta），且非空，无法作为项目打开".to_string(),
                cx,
            );
            false
        }
        TargetDirState::Empty => {
            // 原型 C2：空目录不是项目，但应**询问**是否在此创建，而不是直接拒绝。
            // 先关掉本对话框（此刻栈顶即它自己），再弹确认框——避开「后开的框被 pop 掉」。
            // 返回 `false` 以免调用方重复关一次。
            window.close_dialog(cx);
            open_empty_dir_dialog(host, path, window, cx);
            false
        }
        TargetDirState::ExistingProject => {
            open_path(host, &path, window, cx);
            true
        }
    }
}

/// 空目录确认（原型 C2）：确认后在该目录创建项目并打开（项目名取目录末级名）。
pub fn open_empty_dir_dialog(
    host: &ProjectUiHost,
    path: PathBuf,
    window: &mut Window,
    cx: &mut App,
) {
    host.state.borrow_mut().dialog_error = None;
    let host = host.clone();
    window.open_alert_dialog(cx, move |alert, _window, cx| {
        let theme = cx.theme();
        let name = OpenProject::from_root(path.clone()).name;
        let host_create = host.clone();
        let path_create = path.clone();
        let name_create = name.clone();
        alert
            .icon(Icon::new(IconName::FolderOpen).text_color(theme.colors.info))
            .title("在空目录创建项目？")
            .description(format!(
                "目录 {} 为空。确认后将在其中创建名为「{name}」的项目（建 .RSmeta 并打开）。",
                path.display()
            ))
            .on_ok(move |_, window, cx| {
                window.close_dialog(cx);
                create_in_dir(&host_create, &path_create, &name_create, window, cx);
                false
            })
    });
}

/// 在指定目录创建项目并打开（C2 的「在此创建项目」分支）。
pub(super) fn create_in_dir(
    host: &ProjectUiHost,
    path: &std::path::Path,
    name: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let input = project_service::CreateProjectInput::new(name, path.to_path_buf());
    match project_service::create(input) {
        Ok(summary) => open_path(host, &summary.path, window, cx),
        Err(e) => {
            host.state.borrow_mut().notice = Some(e);
            host.notify(cx);
        }
    }
}

/// 内置示例项目的落点：数据目录旁的 `samples/示例项目`。
///
/// 解析不到系统目录时回退 `paths::data_dir()`——改造前这里退到 `%TEMP%`，
/// 系统清理一跑示例项目就没了。
pub(super) fn sample_project_dir() -> PathBuf {
    let base = engine::migration::get_system_dir()
        .ok()
        .and_then(|system| system.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(paths::data_dir);
    base.join("samples").join("示例项目")
}

/// 未保存拦截的「准备」阶段：按选择另存草稿，并清空编辑区。
///
/// 返回 `true` 表示可以推进后续动作（保存成功或选择放弃）；`false` 时已写入
/// 对话框错误，拦截对话框保持打开。
pub(super) fn prepare_unsaved(
    host: &ProjectUiHost,
    save: bool,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    if save {
        match save_draft(host) {
            Ok(path) => {
                host.state.borrow_mut().notice = Some(format!("草稿已保存：{}", path.display()));
            }
            Err(e) => {
                set_dialog_error(host, e, cx);
                return false;
            }
        }
    }
    // 保存或放弃后都要清空编辑区（命令式，事件上下文；避免下一轮 render 再次判脏）。
    host.editor.clear(window, cx);
    host.editor.mark_clean();
    true
}

/// 未保存拦截的「推进」阶段：执行被拦截的打开 / 关闭动作。
///
/// 必须在拦截对话框已关闭之后调用——推进可能另开对话框（如锁占用）。
pub(super) fn advance_pending(
    host: &ProjectUiHost,
    pending: &PendingAction,
    window: &mut Window,
    cx: &mut App,
) {
    match pending {
        PendingAction::OpenPath(path) => open_path(host, path, window, cx),
        // 切换到另一个项目：确认「放弃 / 保存」后先释放当前项目再打开目标。
        PendingAction::SwitchTo(path) => switch_now(host, path, window, cx),
        PendingAction::Close => do_close(host, cx),
        // 「＋ 新增项目」/「打开现有目录…」：确认后直接开目标对话框（#4）。
        PendingAction::CreateProject(inputs) => open_create_dialog(host, inputs, window, cx),
        PendingAction::OpenFolder(inputs) => open_folder_dialog(host, inputs, window, cx),
    }
}

/// 记录对话框校验错误并重绘（语义对话框 builder 每帧重读）。
pub(super) fn set_dialog_error(host: &ProjectUiHost, message: String, cx: &mut App) {
    host.state.borrow_mut().dialog_error = Some(message);
    host.notify(cx);
}

/// 固定 / 取消固定。
pub fn toggle_pin(host: &ProjectUiHost, item: &ProjectSummary, cx: &mut App) {
    if let Err(e) = project_service::set_pinned(&item.id, !item.is_pinned) {
        host.state.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(host, cx);
}

/// 软删（移出名册）。
pub fn soft_remove(host: &ProjectUiHost, id: &str, cx: &mut App) {
    if let Err(e) = project_service::soft_remove(id) {
        host.state.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(host, cx);
}

/// 恢复已移除项目。
pub fn restore(host: &ProjectUiHost, id: &str, cx: &mut App) {
    if let Err(e) = project_service::restore(id) {
        host.state.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(host, cx);
}

/// 失效路径项目：移出名册（不动磁盘）。
pub fn forget(host: &ProjectUiHost, id: &str, cx: &mut App) {
    if let Err(e) = project_service::forget(id) {
        host.state.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(host, cx);
}

/// 打开「重新定位」对话框。
pub fn open_relocate_dialog(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .create_location
        .update(cx, |s, cx| s.set_value(String::new(), window, cx));
    host.state.borrow_mut().dialog_error = None;

    let id = item.id.clone();
    let name = item.name.clone();
    let host = host.clone();
    let inputs = inputs.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        dialog
            .title("重新定位项目")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("项目「{name}」原路径已失效，请指定新目录。")),
                    )
                    .child(label(theme, "新的项目目录（含 .RSmeta）"))
                    .child(directory_row(&inputs.create_location, "rl-browse"))
                    .when_some(error, |d, e| d.child(dialog_error_line(theme, &e))),
            )
            .footer(dialog_footer(
                "rl-cancel",
                "rl-ok",
                "重新定位",
                ButtonVariant::Primary,
                {
                    let host = host.clone();
                    let inputs = inputs.clone();
                    let id = id.clone();
                    move |window, cx| {
                        if submit_relocate(&host, &inputs, &id, cx) {
                            window.close_dialog(cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
            .on_ok({
                let host = host.clone();
                let inputs = inputs.clone();
                let id = id.clone();
                move |_, window, cx| {
                    if submit_relocate(&host, &inputs, &id, cx) {
                        window.close_dialog(cx);
                    }
                    false
                }
            })
    });
}

/// 提交重新定位：校验新目录后改写名册与项目路径；成功返回 `true`。
pub fn submit_relocate(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    id: &str,
    cx: &mut App,
) -> bool {
    let new_root = PathBuf::from(inputs.create_location.read(cx).value().trim().to_string());
    if new_root.as_os_str().is_empty() {
        set_dialog_error(host, "请输入新的项目目录".to_string(), cx);
        return false;
    }
    match project_service::relocate(id, &new_root) {
        Ok(()) => {
            refresh_picker(host, cx);
            true
        }
        Err(e) => {
            set_dialog_error(host, e, cx);
            false
        }
    }
}

/// 打开删除确认（输入项目名）。
pub fn open_delete_dialog(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .delete_confirm
        .update(cx, |s, cx| s.set_value("", window, cx));
    host.state.borrow_mut().dialog_error = None;

    let id = item.id.clone();
    let name = item.name.clone();
    let root = item.path.clone();
    let host = host.clone();
    let inputs = inputs.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        dialog
            .title("删除项目数据")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.danger)
                            .child(format!(
                                "将删除项目内部元数据（{}），用户文件保留。",
                                project_service::RS_META_DIR_NAME
                            )),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(root.to_string_lossy().to_string()),
                    )
                    .child(label(theme, &format!("输入项目名「{name}」以确认")))
                    .child(Input::new(&inputs.delete_confirm))
                    .when_some(error, |d, e| d.child(dialog_error_line(theme, &e))),
            )
            .footer(dialog_footer(
                "del-cancel",
                "del-ok",
                "删除数据",
                ButtonVariant::Danger,
                {
                    let host = host.clone();
                    let inputs = inputs.clone();
                    let id = id.clone();
                    let name = name.clone();
                    let root = root.clone();
                    move |window, cx| {
                        if confirm_delete(&host, &inputs, &id, &name, &root, cx) {
                            window.close_dialog(cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
            .on_ok({
                let host = host.clone();
                let inputs = inputs.clone();
                let id = id.clone();
                let name = name.clone();
                let root = root.clone();
                move |_, window, cx| {
                    if confirm_delete(&host, &inputs, &id, &name, &root, cx) {
                        window.close_dialog(cx);
                    }
                    false
                }
            })
    });
}

/// 确认删除磁盘数据（需输入项目名匹配）；成功返回 `true`。
pub fn confirm_delete(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    id: &str,
    name: &str,
    root: &std::path::Path,
    cx: &mut App,
) -> bool {
    let typed = inputs.delete_confirm.read(cx).value().trim().to_string();
    if typed != name {
        set_dialog_error(host, "输入的项目名不匹配".to_string(), cx);
        return false;
    }
    // 若删除的是当前打开项目，先关闭以释放锁。
    let is_current = host.root().map(|r| r == root).unwrap_or(false);
    if is_current {
        do_close(host, cx);
    }
    match project_service::delete_disk(id, root) {
        Ok(()) => {
            refresh_picker(host, cx);
            true
        }
        Err(e) => {
            set_dialog_error(host, e, cx);
            false
        }
    }
}

/// 移出当前项目（软删）：名册反查 id 后调用 `soft_remove`
///（设置面板的「危险区」与卡片菜单共用；避免在 render 里查名册）。
pub(super) fn soft_remove_current(host: &ProjectUiHost, cx: &mut App) {
    let Some(item) = current_summary(host) else {
        return;
    };
    soft_remove(host, &item.id, cx);
}

/// 打开当前项目的「删除数据」确认对话框（危险区入口）。
pub(super) fn open_delete_dialog_for_current(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(item) = current_summary(host) else {
        return;
    };
    open_delete_dialog(host, inputs, &item, window, cx);
}
