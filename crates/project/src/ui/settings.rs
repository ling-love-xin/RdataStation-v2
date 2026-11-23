//! 项目设置：磁盘 / 名册 / 版本台账快照（`SettingsSnapshot` 与 `load_settings_snapshot`）
//! 与设置面板渲染（含版本台账的行内两段式删除）。

use shared::i18n::tr;
use super::*;

// ==================== 设置面板的磁盘 / 名册快照（B2 / B3） ====================

/// `.RSmeta` 结构树的一行（排障用：只读，不写）。
///
/// 树在渲染前展平成行，渲染只按 `depth` 缩进——这样树的**内容**可以由纯函数给出
/// （`meta_tree_rows`），窗口测试直接对着临时目录断言，不必经过窗口。
pub struct MetaTreeRow {
    /// 相对 `.RSmeta` 的层级（根下的文件 / 目录为 0）。
    pub depth: usize,
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// 目录不给大小（不递归求和，避免把「哪个文件大」掩盖掉）；`None` 渲染成「—」。
    pub size: Option<u64>,
}

/// 结构树上限：排障视图，异常目录（巨大 / 循环）不该把界面拖死。
pub(super) const META_TREE_MAX_DEPTH: usize = 3;
pub(super) const META_TREE_MAX_ROWS: usize = 200;

/// 枚举 `.RSmeta` 结构：目录在前、同级按名称排序、深度优先。
///
/// 只读文件系统的纯函数（不碰 UI / 名册），因此测试直接给临时目录就能断言。
pub fn meta_tree_rows(meta_dir: &std::path::Path) -> Vec<MetaTreeRow> {
    fn walk(dir: &std::path::Path, depth: usize, out: &mut Vec<MetaTreeRow>) {
        if depth >= META_TREE_MAX_DEPTH || out.len() >= META_TREE_MAX_ROWS {
            return;
        }
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<(bool, std::ffi::OsString, PathBuf)> = Vec::new();
        // 读不了的项直接丢掉：排障视图不该因为一条杂项崩掉。
        for entry in read_dir.filter_map(Result::ok) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // 符号链接等其他类型跳过：`.RSmeta` 里不该有，排障时也不追。
            if file_type.is_dir() || file_type.is_file() {
                entries.push((file_type.is_dir(), entry.file_name(), entry.path()));
            }
        }
        // 目录在前、同级按名称排序：行的位置只跟磁盘结构走，不断跳动才好排障。
        entries.sort_by(|a, b| (!a.0, &a.1).cmp(&(!b.0, &b.1)));
        for (is_dir, name, path) in entries {
            if out.len() >= META_TREE_MAX_ROWS {
                return;
            }
            let name = name.to_string_lossy().to_string();
            if is_dir {
                out.push(MetaTreeRow {
                    depth,
                    name,
                    path: path.clone(),
                    is_dir: true,
                    size: None,
                });
                walk(&path, depth + 1, out);
            } else {
                out.push(MetaTreeRow {
                    depth,
                    name,
                    size: std::fs::metadata(&path).ok().map(|m| m.len()),
                    path,
                    is_dir: false,
                });
            }
        }
    }

    let mut rows = Vec::new();
    walk(meta_dir, 0, &mut rows);
    rows
}

/// 人类可读大小（存储树 / 概览共用）。
pub(super) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// 设置面板的「磁盘 + 名册」快照。
///
/// 在**事件路径**（打开设置 / 点刷新）一次性取，render 只纯读：`.RSmeta` 目录遍历、名册
/// 查询与版本台账都不该每帧做（与「render 不做 I/O」的约定一致）。
#[derive(Default)]
pub struct SettingsSnapshot {
    pub meta_rows: Vec<MetaTreeRow>,
    /// 名册里的描述（概览显示；编辑后由保存路径刷新）。
    pub description: Option<String>,
    /// 名册里的状态 key（`active` / `archived` …）：概览据此显示归档状态。
    pub status: Option<String>,
    pub missing_drivers: Vec<String>,
    /// 默认连接（U3）：`None` = 不设默认。
    pub default_connection: Option<String>,
    /// 默认连接候选（同一快照取一次，render 里不再问宿主）。
    pub connection_options: Vec<ConnectionOption>,
    /// 版本台账快照（同一快照取一次；render 不查库）。
    pub versions: Vec<ProjectVersionRow>,
    /// 版本台账取数失败信息（成功为 `None`，render 显示告警行）。
    pub versions_error: Option<String>,
    /// 是否取过（未取时显示「—」，不当成「没有」）。
    pub loaded: bool,
}

/// 重新取设置面板快照（打开设置 / 点「刷新列表」时调）。
pub fn load_settings_snapshot(host: &ProjectUiHost, cx: &mut App) {
    let Some(root) = host.root() else {
        return;
    };
    let summary = project_service::list_all()
        .ok()
        .and_then(|items| items.into_iter().find(|p| p.path == root));
    // 版本台账与磁盘/名册同一拍取齐：render 不查库（曾每帧开 SQLite 列版本表）。
    let (versions, versions_error) = match project_service::list_versions(&root) {
        Ok(versions) => (versions, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let snapshot = SettingsSnapshot {
        meta_rows: meta_tree_rows(&root.join(project_service::RS_META_DIR_NAME)),
        description: summary.as_ref().and_then(|p| p.description.clone()),
        status: summary.as_ref().map(|p| p.status.clone()),
        missing_drivers: summary.map(|p| p.missing_drivers).unwrap_or_default(),
        default_connection: project_service::load_default_connection(&root)
            .ok()
            .flatten(),
        connection_options: (host.connections)(cx),
        versions,
        versions_error,
        loaded: true,
    };
    host.state.borrow_mut().settings = snapshot;
    host.notify(cx);
}

// ==================== 项目设置 ====================

/// 项目设置（覆盖中央区，Tab 式）。
pub fn render_settings(host: &ProjectUiHost, inputs: &ProjectInputs, cx: &mut App) -> Option<Div> {
    if !host.state.borrow().settings_open {
        return None;
    }
    let (name, root) = {
        let p = host.session.borrow();
        let s = p.as_ref()?;
        (s.name.clone(), s.root.clone())
    };
    let theme = cx.theme();
    // 快照在事件路径填充（打开设置 / 刷新 / 保存），render 只读：`.RSmeta` 目录遍历与
    // 名册查询都不在这里做。
    let ui = host.state.borrow();
    let read_only = ui.read_only;

    // 概览：描述来自名册快照（R7：元信息包含描述）
    let description = snapshot_description(&ui.settings);
    // 状态 = 名册归档状态 + 当前读写状态（两件事分开看：归档不等于只读）
    let status_text = settings_status_text(ui.settings.status.as_deref(), read_only);
    // 存储（B2）：`.RSmeta` 结构树 + 合计，只读排障视图
    let (meta_rows, meta_total) = (
        &ui.settings.meta_rows,
        ui.settings.meta_rows.iter().filter_map(|r| r.size).sum(),
    );
    let mut tree = div().v_flex().gap_1().text_xs();
    if meta_rows.is_empty() {
        tree = tree.child(div().text_color(theme.colors.muted_foreground).child(
            if ui.settings.loaded {
                "（目录为空或不可读）"
            } else {
                "正在读取…"
            },
        ));
    } else {
        tree = tree
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.colors.foreground)
                    .child(format!(
                        "{}/ · 合计 {}",
                        project_service::RS_META_DIR_NAME,
                        human_size(meta_total)
                    )),
            )
            .children(meta_rows.iter().map(|row| meta_tree_row(row, theme)));
    }

    let host_close = host.clone();
    let host_reveal = root.clone();
    let host_reload = host.clone();
    let inputs_rename = inputs.clone();
    let host_rename = host.clone();
    let inputs_version = inputs.clone();
    let host_version = host.clone();
    let host_archive = host.clone();
    let host_remove = host.clone();
    let host_delete = host.clone();
    let inputs_delete = inputs.clone();

    let deps = if !ui.settings.loaded {
        div()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child(tr("依赖自检：正在读取…"))
    } else if ui.settings.missing_drivers.is_empty() {
        div()
            .text_xs()
            .text_color(theme.colors.success)
            .child(tr("依赖自检：未发现缺失驱动"))
    } else {
        div()
            .text_xs()
            .text_color(theme.colors.warning)
            .child(format!(
                "⚠ 缺失驱动：{}",
                ui.settings.missing_drivers.join("、")
            ))
    };

    Some(
        div()
            .absolute()
            .inset_0()
            .v_flex()
            .bg(theme.colors.background)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.colors.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("项目设置 · {name}")),
                    )
                    .when(read_only, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.warning)
                                .child(tr("只读（写操作已禁用）")),
                        )
                    })
                    .child(
                        div().ml_auto().child(
                            Button::new("proj-settings-close")
                                .ghost()
                                .small()
                                .label(tr("关闭"))
                                .on_click(move |_, _, app| {
                                    host_close.state.borrow_mut().settings_open = false;
                                    host_close.notify(app);
                                }),
                        ),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .p_4()
                    // 结构树 / 版本列表可以很长：滚动区在自己身上（不要拖到整个面板）
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(section(theme, "概览"))
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .text_xs()
                            .child(kv(theme, "名称", &name))
                            .child(kv(theme, "路径", &root.to_string_lossy()))
                            .child(kv(theme, "描述", &description))
                            .child(kv(theme, "状态", &status_text)),
                    )
                    .child(section(theme, "基本信息"))
                    .child(label(theme, "名称（重命名，仅改显示名）"))
                    .child(Input::new(&inputs.rename).disabled(read_only))
                    .child(label(theme, "描述"))
                    .child(Input::new(&inputs.description).disabled(read_only))
                    .child(
                        Button::new("proj-info-save")
                            .secondary()
                            .disabled(read_only)
                            .label(tr("保存项目信息"))
                            .on_click(move |_, window, app| {
                                save_project_info(&host_rename, &inputs_rename, window, app)
                            }),
                    )
                    .child(section(theme, "默认项"))
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(default_connection_picker(&ui.settings, host, read_only))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(
                                        "新建查询 / 打开项目时的预选连接；不设则用系统当前连接。",
                                    ),
                            ),
                    )
                    .child(section(theme, "存储（.RSmeta）"))
                    .child(tree)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(
                                "只读的结构视图；用系统文件管理器打开会暴露内部结构（仅排障用）。",
                            ),
                    )
                    .child(
                        Button::new("proj-open-meta")
                            .secondary()
                            .icon(IconName::FolderOpen)
                            .label(tr("打开 .RSmeta"))
                            .on_click(move |_, _, _| reveal_in_explorer(&host_reveal)),
                    )
                    .child(section(theme, "依赖"))
                    .child(deps)
                    .child(section(theme, "版本"))
                    .child(versions_view(&ui.settings, &ui, host, theme))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(&inputs.version_msg).disabled(read_only)),
                            )
                            .child(
                                Button::new("proj-version-add")
                                    .secondary()
                                    .disabled(read_only)
                                    .label(tr("创建版本快照"))
                                    .on_click(move |_, window, app| {
                                        create_version_action(
                                            &host_version,
                                            &inputs_version,
                                            window,
                                            app,
                                        )
                                    }),
                            ),
                    )
                    .child(section(theme, "危险区"))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(
                                Button::new("proj-archive")
                                    .secondary()
                                    .disabled(read_only)
                                    .label(tr("归档 / 取消归档"))
                                    .on_click(move |_, window, app| {
                                        toggle_archive(&host_archive, window, app)
                                    }),
                            )
                            .child(
                                Button::new("proj-remove")
                                    .secondary()
                                    .disabled(read_only)
                                    .label(tr("移出列表"))
                                    .on_click(move |_, _, app| {
                                        soft_remove_current(&host_remove, app)
                                    }),
                            )
                            .child(
                                Button::new("proj-delete")
                                    .danger()
                                    .disabled(read_only)
                                    .label(tr("删除数据…"))
                                    .on_click(move |_, window, app| {
                                        open_delete_dialog_for_current(
                                            &host_delete,
                                            &inputs_delete,
                                            window,
                                            app,
                                        )
                                    }),
                            ),
                    )
                    .child(
                        Button::new("proj-reload")
                            .secondary()
                            .label(tr("刷新列表与结构树"))
                            .on_click(move |_, _, app| {
                                refresh_picker(&host_reload, app);
                                load_settings_snapshot(&host_reload, app);
                            }),
                    ),
            ),
    )
}

/// 概览里的描述文案：未写描述 / 未取快照都显示「—」（不显示成空行）。
pub(super) fn snapshot_description(snapshot: &SettingsSnapshot) -> String {
    snapshot
        .description
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or("—")
        .to_string()
}

/// 概览里的「状态」：名册归档状态 + 读写状态。
///
/// 两件事要分开看：归档（名册标记）与只读（实例锁被占用）互不因果，所以并排写。
/// 快照没取到状态时用「—」，不要默认成「活跃」（那是编数据）。
pub(super) fn settings_status_text(status: Option<&str>, read_only: bool) -> String {
    let registry = status.map(status_label).unwrap_or("—");
    let access = if read_only { "只读" } else { "可写" };
    format!("{registry} · {access}")
}

/// 结构树的一行：缩进 + 图标 + 名称 + 大小 + 「复制路径」。
///
/// 目录不给大小（渲染成「目录」）；复制路径用 toast 反馈（面板本身没有 notice 行）。
pub(super) fn meta_tree_row(row: &MetaTreeRow, theme: &gpui_kit::component::Theme) -> Div {
    let path_copy = row.path.to_string_lossy().to_string();
    let size = match row.size {
        Some(size) => human_size(size),
        None if row.is_dir => "目录".to_string(),
        None => "—".to_string(),
    };
    div()
        .h_flex()
        .items_center()
        .gap_2()
        // 缩进走树/列表共用原语（0.5rem + 0.875rem × depth），不再各写各的倍率
        .pl(rems(tree::indent_rem(
            ui::TREE_BASE_PADDING,
            ui::TREE_INDENT,
            row.depth,
        )))
        .child(
            Icon::new(if row.is_dir {
                IconName::FolderOpen
            } else {
                IconName::FileText
            })
            .size_3p5()
            .text_color(theme.colors.muted_foreground),
        )
        .child(
            div()
                .text_color(if row.is_dir {
                    theme.colors.foreground
                } else {
                    theme.colors.muted_foreground
                })
                .child(row.name.clone()),
        )
        .child(
            div()
                .ml_auto()
                .text_color(theme.colors.muted_foreground)
                .child(size),
        )
        .child(
            // id 用路径：同一帧内唯一，且不随列表顺序变（行间不会串焦点）
            Button::new(ElementId::Name(SharedString::from(format!(
                "meta-copy-{}",
                row.path.display()
            ))))
            .ghost()
            .xsmall()
            .icon(IconName::Copy)
            .on_click(move |_, window, app| {
                app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(path_copy.clone()));
                window.push_notification(
                    gpui_kit::component::notification::Notification::info(format!(
                        "已复制路径：{path_copy}"
                    )),
                    app,
                );
            }),
        )
}

/// 默认连接按钮文案（纯函数，可断言）。
///
/// 记录值指向已不在候选里的连接（比如它被删了）时**如实显示 id**并标「已不可用」，
/// 不静默变成「未设置」——那会让用户以为默认没丢。
pub fn default_connection_label(snapshot: &SettingsSnapshot) -> String {
    match snapshot.default_connection.as_ref() {
        None => "默认连接：未设置".to_string(),
        Some(id) => match snapshot.connection_options.iter().find(|c| &c.id == id) {
            Some(option) => format!("默认连接：{}", option.name),
            None => format!("默认连接：{id}（已不可用）"),
        },
    }
}

/// 默认连接选择（U3）：`Button::dropdown_menu` + 勾选项，空 = 不设默认。
pub(super) fn default_connection_picker(
    snapshot: &SettingsSnapshot,
    host: &ProjectUiHost,
    read_only: bool,
) -> impl IntoElement {
    let current = snapshot.default_connection.clone();
    let label = default_connection_label(snapshot);
    let missing_current = current
        .as_ref()
        .is_some_and(|id| !snapshot.connection_options.iter().any(|c| &c.id == id));
    let options = snapshot.connection_options.clone();
    let current_for_menu = current.clone();
    // 显式 UFCS：`host` 是引用，`host.clone()` 会解析到 `&T: Clone`（拿到引用，进不了
    // `'static` 回调），这里要的是宿主的拥有型副本。
    let host_clear = ProjectUiHost::clone(host);
    let host_items = ProjectUiHost::clone(host);
    let current_id = current.clone();

    Button::new("proj-default-connection")
        .secondary()
        .disabled(read_only)
        .label(label)
        // 下拉指示用组件自带的 caret（替代字符 ▾，随档位缩放）
        .dropdown_caret(true)
        .dropdown_menu(move |menu, _window, _cx| {
            let mut menu = menu.item(
                PopupMenuItem::new(tr("（不设默认）"))
                    .checked(current_for_menu.is_none())
                    .disabled(current_for_menu.is_none())
                    .on_click({
                        let host = host_clear.clone();
                        move |_, window, app| set_default_connection(&host, None, window, app)
                    }),
            );
            // 记录值已不在候选里：摆一项“当前记录”并置灰，让用户看得见默认还在
            if missing_current {
                let id = current_id.clone().unwrap_or_default();
                menu = menu.item(
                    PopupMenuItem::new(format!("{id}（已不可用）"))
                        .checked(true)
                        .disabled(true),
                );
            }
            for option in options.iter() {
                let host = ProjectUiHost::clone(&host_items);
                let id = option.id.clone();
                let is_current = Some(id.clone()) == current_id;
                menu = menu.item(
                    PopupMenuItem::new(option.name.clone())
                        .icon(IconName::HardDrive)
                        .checked(is_current)
                        .disabled(is_current)
                        .on_click(move |_, window, app| {
                            set_default_connection(&host, Some(id.clone()), window, app)
                        }),
                );
            }
            menu
        })
}

pub(super) fn section(theme: &gpui_kit::component::Theme, title: &str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.colors.muted_foreground)
        .child(title.to_string())
}

/// 项目版本列表（只读数据来自快照，render 不查库；失败降级为提示行）。
///
/// 删除是**两段式**：先点「删除」把该行切成「确认删除 / 取消」，再点确认才真删——
/// 版本行没有弹窗，这一行就是确认区（`state.version_delete_confirm` 记录待删 id）。
pub(super) fn versions_view(
    snapshot: &SettingsSnapshot,
    state: &ProjectUiState,
    host: &ProjectUiHost,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let mut list = div().v_flex().gap_1().text_xs();
    if !snapshot.loaded {
        list = list.child(
            div()
                .text_color(theme.colors.muted_foreground)
                .child(tr("正在读取…")),
        );
    } else if let Some(e) = &snapshot.versions_error {
        list = list.child(div().text_color(theme.colors.warning).child(e.clone()));
    } else if snapshot.versions.is_empty() {
        list = list.child(
            div()
                .text_color(theme.colors.muted_foreground)
                .child(tr("暂无版本记录")),
        );
    } else {
        for v in &snapshot.versions {
            let confirming = state.version_delete_confirm.as_deref() == Some(v.id.as_str());
            let mut row = div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_color(theme.colors.muted_foreground)
                        .child(v.created_at.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .text_color(theme.colors.foreground)
                        .child(v.message.clone()),
                );
            if confirming {
                let host_confirm = host.clone();
                let id_confirm = v.id.clone();
                let host_cancel = host.clone();
                let id_cancel = v.id.clone();
                row = row
                    .child(
                        Button::new(format!("ver-del-confirm-{id_confirm}"))
                            .danger()
                            .small()
                            .label(tr("确认删除"))
                            .on_click(move |_, window, app| {
                                confirm_version_delete(&host_confirm, &id_confirm, window, app)
                            }),
                    )
                    .child(
                        Button::new(format!("ver-del-cancel-{id_cancel}"))
                            .ghost()
                            .small()
                            .label(tr("取消"))
                            .on_click(move |_, _, app| cancel_version_delete(&host_cancel, app)),
                    );
            } else {
                let host_request = host.clone();
                let id_request = v.id.clone();
                row = row.child(
                    Button::new(format!("ver-del-{id_request}"))
                        .ghost()
                        .small()
                        .disabled(state.read_only)
                        .label(tr("删除"))
                        .on_click(move |_, _, app| {
                            request_version_delete(&host_request, &id_request, app)
                        }),
                );
            }
            list = list.child(row);
        }
    }
    list
}

/// 版本行内删除第一步：把该行切进二次确认态（不删数据）。
pub(super) fn request_version_delete(host: &ProjectUiHost, id: &str, cx: &mut App) {
    if read_only_blocked(host, cx, "删除版本") {
        return;
    }
    host.state.borrow_mut().version_delete_confirm = Some(id.to_string());
    host.notify(cx);
}

/// 版本行内删除第二步的「取消」：退出二次确认态。
pub(super) fn cancel_version_delete(host: &ProjectUiHost, cx: &mut App) {
    host.state.borrow_mut().version_delete_confirm = None;
    host.notify(cx);
}

/// 版本行内删除的确认：删台账行 → 重取版本快照 → 清确认态。
///
/// 失败时也清确认态：留着确认按钮只会让人反复点同一条已删/不可删的记录。
pub(super) fn confirm_version_delete(
    host: &ProjectUiHost,
    id: &str,
    window: &mut Window,
    cx: &mut App,
) {
    if read_only_blocked(host, cx, "删除版本") {
        return;
    }
    let root = match host.session.borrow().as_ref().map(|s| s.root.clone()) {
        Some(r) => r,
        None => return,
    };
    match project_service::delete_version(&root, id) {
        Ok(()) => {
            let (versions, versions_error) = match project_service::list_versions(&root) {
                Ok(versions) => (versions, None),
                Err(e) => (Vec::new(), Some(e)),
            };
            {
                let mut ui = host.state.borrow_mut();
                ui.settings.versions = versions;
                ui.settings.versions_error = versions_error;
                ui.version_delete_confirm = None;
            }
            notify_success(host, window, cx, "已删除版本记录");
        }
        Err(e) => {
            host.state.borrow_mut().version_delete_confirm = None;
            notify_error(host, window, cx, e);
        }
    }
    host.notify(cx);
}

/// 创建版本快照（只读模式拦截）。
pub fn create_version_action(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) {
    if read_only_blocked(host, cx, "创建版本") {
        return;
    }
    let root = match host.session.borrow().as_ref().map(|s| s.root.clone()) {
        Some(r) => r,
        None => return,
    };
    let message = inputs.version_msg.read(cx).value().trim().to_string();
    match project_service::create_version(&root, &message) {
        Ok(()) => {
            // 台账变了：只刷新版本快照（不重走目录遍历；render 仍不查库）。
            let (versions, versions_error) = match project_service::list_versions(&root) {
                Ok(versions) => (versions, None),
                Err(e) => (Vec::new(), Some(e)),
            };
            {
                let mut ui = host.state.borrow_mut();
                ui.settings.versions = versions;
                ui.settings.versions_error = versions_error;
            }
            // 清掉备注：否则连点两次会写下两条一模一样的快照。
            inputs
                .version_msg
                .update(cx, |s, cx| s.set_value("", window, cx));
            notify_success(host, window, cx, "已创建版本快照");
        }
        Err(e) => notify_error(host, window, cx, e),
    }
    host.notify(cx);
}

pub(super) fn kv(theme: &gpui_kit::component::Theme, key: &str, value: &str) -> Div {
    div()
        .h_flex()
        .gap_2()
        .child(
            div()
                .text_color(theme.colors.muted_foreground)
                .child(key.to_string()),
        )
        .child(
            div()
                .text_color(theme.colors.foreground)
                .child(value.to_string()),
        )
}
