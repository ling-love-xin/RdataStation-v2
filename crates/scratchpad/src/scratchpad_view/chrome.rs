//! 面板外壳：面板头 / 工具栏 / 搜索行 / 草稿树装配 / 引用 / 回收站 / 撤销栏 / 状态行。
//!
//! 这是 `render_scratchpad` 的落点——草稿箱唯一的主视图（虚拟列表的装配也在这里，
//! 行本身在 `rows.rs`）。
use super::*;

use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::button::Toggle;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::label::Label;
use gpui_kit::component::Sizable;
use gpui_kit::component::dialog::{Dialog, DialogButtonProps, DialogContent};

/// 拖拽期间的浮层视图（gpui `on_drag` 要求 W: Render；内容透明，仅承接拖拽会话）。
pub(super) struct ToolbarDragOverlay;
impl gpui_kit::Render for ToolbarDragOverlay {
    fn render(&mut self, _: &mut gpui_kit::Window, _: &mut gpui_kit::Context<Self>) -> impl gpui_kit::IntoElement {
        gpui_kit::div().w_full().h_full()
    }
}


impl ScratchpadView {
    /// 草稿箱面板（M5）：根 = `{project}/scratchpad/`。
    ///
    /// 闭环：新建（内联）/重命名/删除→回收站+撤销栏/回收站恢复与清空/文件名过滤/外部引用移除。
    pub fn render_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.scratchpad.borrow().loaded {
            self.request_scratchpad_load(cx);
        }
        self.ensure_scratchpad_inputs(window, cx);

        let theme = cx.theme();
        let hover_bg = theme.colors.list_hover;
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let primary = theme.colors.primary;

        let entity = cx.entity();
        // ── 搜索态整体切换（2026-10-11 用户点名，VSCode 对齐）：面板打开时侧栏只显示
        // 搜索面板（普通工具栏 / 撤销栏 / 底部引用回收站整体消失）──
        if self.scratchpad.borrow().search_panel_open {
            return self
                .render_scratchpad_search_pane(window, cx)
                .id("scratchpad-search-pane")
                .role(gpui::Role::Group);
        }
        let (rows, error, external_refs, trash, sort, sort_desc, edit, undo, filter, loading) = {
            let view = self.scratchpad.borrow();
            let filter = view
                .search_input
                .as_ref()
                .map(|i| i.read(cx).value().trim().to_string())
                .unwrap_or_default();
            // 草稿图的文件搜索 = 恒文件名过滤（VSCode：普通搜索 = 搜文件，工具栏 = 搜内容）。
            let (search_mode, search_regex, search_case) =
                (ScratchpadSearchMode::Name, view.search_regex, view.search_case);
            // 标签筛选（§7.3）：模块根换算相对路径；`tag_filter` 与文件名过滤可叠加。
            let module_root = self
                .host
                .project_root()
                .map(|root| crate::store::ScratchpadStore::new(root).scratchpad_dir().to_path_buf());
            let tag_filter = view.tag_filter.clone();
            let entry_tags = view.entry_tags.clone();
            let mut flat = Vec::new();
            flatten_scratchpad(
                &view.entries,
                0,
                &view.expanded,
                &view.children,
                view.sort,
                view.sort_desc,
                &filter,
                search_mode,
                search_regex,
                search_case,
                module_root.as_deref(),
                tag_filter.as_deref(),
                &entry_tags,
                &mut flat,
            );
            (
                flat,
                view.error.clone(),
                view.external_refs.clone(),
                view.trash.clone(),
                view.sort,
                view.sort_desc,
                view.edit.clone(),
                view.undo.clone(),
                filter,
                view.loading,
            )
        };
        // OS 文件拖入状态（Phase C）：进入面板时置位并计数，drop / exit 复位。
        let drag_hint = {
            let view = self.scratchpad.borrow();
            (view.external_drag_over, view.external_drag_count)
        };

        let toolbar = self.render_scratchpad_header(cx);

        // ── 草稿图文件搜索行（VSCode 形态，2026-10-11 用户点名）：输入框 + 框内右侧 ⌕ ──
        // 作用：按文件名过滤树（实时）；⌕ = 聚焦输入框（Enter 等效，过滤实时无需额外执行）；
        // 内容搜索入口在普通工具栏 ⌕（`open_search_panel`），两处图标区分（搜文件 vs 搜内容）。
        let search_row = {
            let _theme = cx.theme();
            let filter_input = self.scratchpad.borrow().search_input.clone();
            let filter_input_for_btn = filter_input.clone();
            let run_file_filter = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                // 点击 ⌕：聚焦输入框（实时过滤已生效；与 Enter 同语义）。
                if let Some(input) = &filter_input_for_btn {
                    let _ = input
                        .read(app)
                        .presentation()
                        .focus_handle()
                        .focus(window, app);
                }
            };
            let mut row = div()
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .px_1p5()
                .py_0p5();
            if let Some(input) = filter_input {
                row = row
                    .child(div().flex_1().min_w_0().child(
                        Input::new(&input).bordered(false).focus_bordered(true).w_full(),
                    ))
                    .child(
                        Button::new("sp-file-search-run")
                            .ghost()
                            .small()
                            .icon(IconName::Search)
                            .tooltip("搜索文件（按文件名过滤目录树）")
                            .on_click(run_file_filter),
                    );
            } else {
                row = row.child(div().flex_1());
            }
            // 标签筛选下拉（统一规格 §7.3）：选中 = 只显示打了该标签的文件（平铺），
            // 与文件名过滤可叠加；「全部」清除。菜单每次打开重建（checked 与最近渲染一致）。
            let tag_menu_entity = entity.clone();
            let tag_menu_tags = self.scratchpad.borrow().project_tags.clone();
            let tag_menu_current = self.scratchpad.borrow().tag_filter.clone();
            row = row.child(
                Button::new("sp-tag-filter")
                    .ghost()
                    .small()
                    .icon(Icon::empty().path("icons/tag.svg"))
                    .tooltip("按标签筛选（可叠加文件名过滤）")
                    .dropdown_menu(move |menu, _, _| {
                        let tags = tag_menu_tags.clone();
                        let current = tag_menu_current.clone();
                        let mut menu = menu;
                        let all_entity = tag_menu_entity.clone();
                        menu = menu.item(
                            PopupMenuItem::new("全部")
                                .checked(current.is_none())
                                .on_click(move |_, _, app| {
                                    all_entity.update(app, |this, cx| this.clear_tag_filter(cx))
                                }),
                        );
                        menu = menu.item(PopupMenuItem::separator());
                        for tag in &tags {
                            let tag_id = tag.id.clone();
                            let checked = current.as_deref() == Some(tag_id.as_str());
                            let entity = tag_menu_entity.clone();
                            menu = menu.item(
                                PopupMenuItem::new(tag.name.clone())
                                    .checked(checked)
                                    .on_click(move |_, _, app| {
                                        entity.update(app, |this, cx| {
                                            this.set_tag_filter(&tag_id, cx)
                                        })
                                    }),
                            );
                        }
                        menu
                    }),
            );
            row
        };

        let mut panel = div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .child(toolbar)
            .child(search_row);

        // OS 文件拖入提示条（Phase C：进入面板即提示，松开落盘导入）。
        if drag_hint.0 {
            panel = panel.child(
                div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(primary)
                    .bg(hover_bg)
                    .child(format!("松开以{}个文件到草稿箱", drag_hint.1)),
            );
        }

        if let Some(err) = &error {
            return panel
                .child(
                    div()
                        .flex_1()
                        .w_full()
                        .px_2()
                        .py_3()
                        .text_xs()
                        .text_color(muted)
                        .child(err.clone()),
                )
                .id("scratchpad-panel")
                .role(gpui::Role::Group);
        }

        // 顶部内联新建（仅「新建引用」：文件/文件夹的内联行插在目标文件夹下）。
        if matches!(edit.as_ref(), Some(ScratchpadEdit::NewReference { .. })) {
            panel = panel.child(div().px_1().child(self.render_scratchpad_edit_row(0, cx)));
        }

        // ── 草稿树（面板唯一滚动区）── 本段在 `render_scratchpad_tree` 里。
        let row_count = rows.len();
        let file_count = rows
            .iter()
            .filter(|(_, e)| e.kind == ScratchpadEntryKind::File)
            .count();
        let folder_count = row_count - file_count;
        // 过滤三要素（mode/regex/case）与 flatten 同一来源，供树行命中数标用。
        // 草稿图恒为文件名过滤（Name；内容搜索只在搜索态面板，2026-10-11 对齐）。
        let (filter_mode, filter_regex, filter_case) = {
            let view = self.scratchpad.borrow();
            (ScratchpadSearchMode::Name, view.search_regex, view.search_case)
        };
        let drafts = self.render_scratchpad_tree(
            rows,
            loading,
            &filter,
            filter_mode,
            filter_regex,
            filter_case,
            window,
            cx,
        );
        panel = panel.child(drafts);

        // ── 底部固定区（引用 / 回收站 / 撤销栏 / 状态；不随草稿树滚动）──
        // 引用与回收站两块抽成方法（`render_scratchpad_refs_and_trash`）：主视图只留装配顺序。
        let body = self.render_scratchpad_refs_and_trash(cx);

        // 引用 / 回收站限高可滚，保证草稿树始终有可用高度。
        // 拖拽分区 handle（2026-10-12 用户点名：草稿 | 引用+回收站 高度可拖拽调整，进程内）。
        let group_max_h = self.scratchpad.borrow().group_max_h;
        let split_handle = {
            let entity = entity.clone();
            div()
                .id("sp-split-handle")
                .w_full()
                .h(rems(0.375))
                .flex_none()
                .cursor_row_resize()
                .hover(move |s| s.bg(hover_bg))
                .on_drag(0u8, |_, _, _, cx| cx.new(|_| ToolbarDragOverlay))
                .on_drag_move::<u8>(move |event, _, app| {
                    let delta_px = event.event.position.y.as_f32() - event.bounds.top().as_f32();
                    if delta_px.abs() > 1.0 {
                        entity.update(app, |this, cx| {
                            let mut v = this.scratchpad.borrow_mut();
                            v.group_max_h = (v.group_max_h + delta_px / 16.0).clamp(2.0, 18.0);
                            drop(v);
                            // B3（2026-10-28）：拖拽后的分区高度立即持久化（settings），
                            // 重启后恢复；落盘失败不改本进程已生效。
                            this.persist_group_max_h(cx);
                            cx.notify();
                        });
                    }
                })
        };
        panel = panel.child(
            div()
                .v_flex()
                .w_full()
                .child(split_handle)
                .child(
                    div()
                        .v_flex()
                        .w_full()
                        .max_h(rems(group_max_h))
                        .overflow_y_scrollbar()
                        .child(body),
                ),
        );

        // ── 撤销栏 ──
        if let Some(undo) = &undo {
            let undo_click = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| this.undo_scratchpad_delete(cx));
                }
            };
            panel = panel.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .px_2()
                    .py(rems(1.25))
                    .bg(hover_bg)
                    .rounded_sm()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(fg)
                            .child(format!("已删除 {}", undo.label)),
                    )
                    .child(
                        // K9：撤销动作按钮（自绘链路 → gpui-kit `Button` ghost）。
                        Button::new("sp-undo")
                            .ghost()
                            .small()
                            .label("撤销")
                            .on_click(undo_click),
                    ),
            );
        }

        // ── 底部状态 ──
        // 底部状态行（2026-10-12 图标化：文件/文件夹/引用/回收站 + 排序方向，无中文）。
        panel = panel.child(
            div()
                .w_full()
                .px_2()
                .py_1()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(muted)
                .child(if loading {
                    Icon::empty()
                        .text_color(fg)
                        .path("icons/loader.svg")
                        .size(rems(ui::ICON_SIZE_MD))
                        .flex_none()
                } else {
                    Icon::empty()
                        .text_color(fg)
                        .path("icons/file.svg")
                        .size(rems(ui::ICON_SIZE_MD))
                        .flex_none()
                })
                .child(file_count.to_string())
                .child(
                    Icon::empty()
                        .text_color(fg)
                        .path("icons/folder.svg")
                        .size(rems(ui::ICON_SIZE_MD))
                        .flex_none(),
                )
                .child(folder_count.to_string())
                .child(
                    Icon::empty()
                        .text_color(fg)
                        .path("icons/link.svg")
                        .size(rems(ui::ICON_SIZE_MD))
                        .flex_none(),
                )
                .child(external_refs.len().to_string())
                .child(
                    Icon::empty()
                        .text_color(fg)
                        .path("icons/delete.svg")
                        .size(rems(ui::ICON_SIZE_MD))
                        .flex_none(),
                )
                .child(trash.len().to_string())
                .child(div().flex_1())
                .child(
                    Icon::empty()
                        .text_color(fg)
                        .path(if sort_desc {
                            "icons/sort-descending.svg"
                        } else {
                            "icons/sort-ascending.svg"
                        })
                        .size(rems(ui::ICON_SIZE_MD))
                        .flex_none(),
                )
                .child(match sort {
                    ScratchpadSort::Name => div().flex_none().text_xs().child("名称"),
                    ScratchpadSort::Size => div().flex_none().text_xs().child("大小"),
                    ScratchpadSort::Modified => div().flex_none().text_xs().child("时间"),
                }),
        );

        // 添加外部引用弹窗（2026-10-12 原型「弹窗输入路径+别名」对齐）：打开时渲染 Dialog。
        let ref_dialog_el = if self.scratchpad.borrow().ref_dialog_open {
            Some(self.render_ref_dialog(window, cx))
        } else {
            None
        };

        // 标签编辑弹窗（统一规格 §7.3）：右键「标签…」打开，勾选打标 / 新建标签。
        let tag_dialog_el = if self.scratchpad.borrow().tag_dialog.is_some() {
            Some(self.render_tag_dialog(window, cx))
        } else {
            None
        };

        panel
            .key_context("scratchpad")
            .track_focus(&self.focus_handle)
            // 拖放残留兜底（工程坑，用户实测）：`on_file_drop_exit` 在元素
            // hover 时派发（gpui-pre `div.rs` 的 `is_hovered_ignoring_last_input`
            // 门控），鼠标拖出窗口后 hover 先丢，Exited 不再送达。鼠标移出面板
            // （移到窗口其它区域或移出窗口）都会触发 `MouseExited`，在这里统一复位。
            .on_mouse_exit({
                let entity = entity.clone();
                move |_: &gpui_kit::MouseExitEvent,
                      _window: &mut gpui_kit::Window,
                      app: &mut App| {
                    entity.update(app, |this, cx| {
                        let mut state = this.scratchpad.borrow_mut();
                        if state.external_drag_over {
                            state.external_drag_over = false;
                            state.external_drag_count = 0;
                            state.external_drag_since = None;
                            cx.notify();
                        }
                    });
                }
            })
            // M5 Phase C：OS 文件拖入面板 → 导入。平台把 `FileDropEvent::Entered` 转成
            // `active_drag(ExternalPaths)` + 鼠标移动，`Submit` 在 mouse-up 时 drop。
            .on_drag_move::<gpui_kit::ExternalPaths>({
                let entity = entity.clone();
                move |event: &gpui_kit::DragMoveEvent<gpui_kit::ExternalPaths>,
                      _window: &mut gpui_kit::Window,
                      app: &mut App| {
                    let count = event.drag(app).0.len();
                    entity.update(app, |this, cx| {
                        let mut state = this.scratchpad.borrow_mut();
                        state.external_drag_over = true;
                        state.external_drag_count = count;
                        // 记录拖入时刻：`on_file_drop_exit` 在鼠标拖出窗口时
                        // 鼠标先丢而不派发（见上面注释），用它做兜底。
                        state.external_drag_since = Some(std::time::Instant::now());
                        cx.notify();
                    });
                }
            })
            .on_drop::<gpui_kit::ExternalPaths>({
                let entity = entity.clone();
                move |paths: &gpui_kit::ExternalPaths,
                      _window: &mut gpui_kit::Window,
                      app: &mut App| {
                    let dropped: Vec<std::path::PathBuf> = paths.0.iter().cloned().collect();
                    entity.update(app, |this, cx| {
                        {
                            let mut state = this.scratchpad.borrow_mut();
                            state.external_drag_over = false;
                            state.external_drag_count = 0;
                            state.external_drag_since = None;
                        }
                        let folders: Vec<_> =
                            dropped.iter().filter(|p| !p.is_file()).cloned().collect();
                        let files: Vec<_> =
                            dropped.into_iter().filter(|p| p.is_file()).collect();
                        if !folders.is_empty() && files.is_empty() {
                            this.scratchpad.borrow_mut().error =
                                Some("暂不支持拖入文件夹：请用工具栏的导入或粘贴导入".to_string());
                        } else if !folders.is_empty() {
                            this.scratchpad.borrow_mut().error = Some(format!(
                                "已忽略 {} 个文件夹，仅导入文件",
                                folders.len()
                            ));
                            this.import_scratchpad_files(files, cx);
                        } else {
                            this.import_scratchpad_files(files, cx);
                        }
                        cx.notify();
                    });
                }
            })
            .on_file_drop_exit({
                let entity = entity.clone();
                move |_: &gpui_kit::FileDropEvent,
                      _window: &mut gpui_kit::Window,
                      app: &mut App| {
                    entity.update(app, |this, cx| {
                        let mut state = this.scratchpad.borrow_mut();
                        state.external_drag_over = false;
                        state.external_drag_count = 0;
                        state.external_drag_since = None;
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadSelectAll, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.select_all_scratchpad(cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadRename, window: &mut gpui_kit::Window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.rename_scratchpad_selection(window, cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadDelete, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.delete_scratchpad_selection(cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadCancelEdit, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.cancel_scratchpad_edit(cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadUp, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.scratchpad_move(-1, cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadDown, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.scratchpad_move(1, cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadOpen, _window, cx: &mut App| {
                    entity.update(cx, |this, cx| this.scratchpad_open_selection(cx));
                }
            })
            .on_action({
                let entity = entity.clone();
                move |_: &ScratchpadNewFile, window: &mut gpui_kit::Window, cx: &mut App| {
                    entity.update(cx, |this, cx| {
                        this.start_scratchpad_edit(ScratchpadEdit::NewFile, window, cx)
                    });
                }
            })
            .when_some(ref_dialog_el, |this, el| this.child(el))
            .when_some(tag_dialog_el, |this, el| this.child(el))
            .id("scratchpad-panel")
            .role(gpui::Role::Group)
    }
}


impl ScratchpadView {
    /// 分组标题行（「草稿(N)」/「外部引用(N)」共用：紧凑高度 + 弱文字 + 计数）。
    /// 注：分组标题当前由行渲染内联实现，本方法暂无调用方 → 预留。
    #[allow(dead_code)]
    fn scratchpad_group_header(&self, label: &str, count: usize, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .h(rems(ui::ROW_HEIGHT_COMPACT))
            .px_1p5()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(muted)
            .child(label.to_string())
            .child(div().flex_1())
            .child(count.to_string())
    }

    /// 底部固定区的「外部引用 + 回收站」两块（不随草稿树滚动）。
    ///
    /// 为什么单独成方法：`render_scratchpad` 是面板唯一主视图，接近千行——把与「数据从哪来」
    /// 无关的两块搬出来，主视图只剩装配顺序（行为不变：同一元素树，只换了落点）。
    fn render_scratchpad_refs_and_trash(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let _primary = theme.colors.primary;
        let _hover_bg = theme.colors.list_hover;
        let ref_color = theme.colors.info;
        let _info = theme.colors.info;

        let entity = cx.entity();
        let (external_refs, trash, trash_expanded, refs_expanded, edit) = {
            let view = self.scratchpad.borrow();
            (
                view.external_refs.clone(),
                view.trash.clone(),
                view.trash_expanded,
                view.refs_expanded,
                view.edit.clone(),
            )
        };

        let mut body = div().v_flex().w_full().gap_1().px_1().pb_1();
        // ── 外部引用（原型 415-424 acc-item「引用」；2026-10-11 对齐：常驻可折叠
        // Accordion + 胶囊 badge；改名 / 打开 / 移除 / 重新引用操作保留）──
        let mut ref_rows = div().v_flex().w_full().gap_1();
        for r in &external_refs {
                // 本引用正在改名 → 行内输入。
                if let Some(ScratchpadEdit::RenameReference { alias }) = &edit {
                    if alias == &r.alias {
                        body = body.child(self.render_scratchpad_edit_row(0, cx));
                        continue;
                    }
                }

                let rename_ref = {
                    let entity = entity.clone();
                    let alias = r.alias.clone();
                    move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.start_scratchpad_edit(
                                ScratchpadEdit::RenameReference {
                                    alias: alias.clone(),
                                },
                                window,
                                cx,
                            )
                        });
                    }
                };
                let open_ref = {
                    let entity = entity.clone();
                    let path = r.path.to_string_lossy().to_string();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.open_scratchpad_location(path.clone(), cx)
                        });
                    }
                };
                let remove = {
                    let entity = entity.clone();
                    let alias = r.alias.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.remove_scratchpad_reference(alias.clone(), cx)
                        });
                    }
                };
                // 失效引用提供「重新引用」（选择新路径后只改路径）。
                let relink = {
                    let entity = entity.clone();
                    let alias = r.alias.clone();
                    move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.relink_scratchpad_reference(alias.clone(), window, cx)
                        });
                    }
                };
                ref_rows = ref_rows.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .h(rems(ui::ROW_HEIGHT_COMPACT))
                        .px_1p5()
                        .child(div().w_2().h_2().flex_none().rounded_sm().bg(ref_color))                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(if r.exists { fg } else { muted })
                                .child(if r.exists {
                                    r.alias.clone()
                                } else {
                                    format!("{}（丢失）", r.alias)
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(r.path.to_string_lossy().to_string()),
                        )
                        .child(
                            Button::new(format!("sp-ref-open-{}", r.alias))
                                .ghost()
                                .small()
                                .icon(IconName::ExternalLink)
                                .tooltip("打开引用目标")
                                .on_click(open_ref),
                        )
                        .child(
                            Button::new(format!("sp-ref-ren-{}", r.alias))
                                .ghost()
                                .small()
                                .icon(Icon::empty().path("icons/pencil.svg"))
                                .tooltip("重命名引用")
                                .on_click(rename_ref),
                        )
                        .when(!r.exists, |this| {
                            this.child(
                                Button::new(format!("sp-ref-relink-{}", r.alias))
                                    .ghost()
                                    .small()
                                    .icon(Icon::empty().path("icons/rotate-ccw.svg"))
                                    .tooltip("重新链接引用（原路径失效）")
                                    .on_click(relink),
                            )
                        })
                        .child(
                            Button::new(format!("sp-ref-{}", r.alias))
                                .ghost()
                                .small()
                                .icon(IconName::Close)
                                .tooltip("移除引用")
                                .on_click(remove),
                        ),
                );
            }
        // 引用区块 = Collapsible（2026-10-12 与草稿区统一：标题 chevron + badge + 点击折叠）
        let hdr_add_ref = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.open_ref_dialog(window, cx));
            }
        };
        let ref_count = external_refs.len();
        let ref_badge = Badge::new().count(ref_count);
        let toggle_refs = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    let mut v = this.scratchpad.borrow_mut();
                    v.refs_expanded = !v.refs_expanded;
                    cx.notify();
                });
            }
        };
        let ref_head = div()
            .debug_selector(|| "sp-refs-head".into())
            .id("sp-refs-head")
            .group("sp-refs-head")
            .cursor_pointer()
            .h_flex()
            .items_center()
            .gap_1()
            .text_xs()
            .child(tree::disclosure_slot().child(tree::disclosure_icon(refs_expanded, muted)))
            .child("引用")
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .invisible()
                    .group_hover("sp-refs-head", |s| s.visible())
                    .h_flex()
                    .items_center()
                    .gap_0p5()
                    .child(
                        Button::new("sp-refs-head-add")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .tooltip("添加外部引用（软连接）")
                            .on_click(hdr_add_ref),
                    ),
            )
            .child(ref_badge)
            .on_click(toggle_refs);
        body = body.child(
            div()
                .v_flex()
                .w_full()
                .child(ref_head)
                .child(Collapsible::new().open(refs_expanded).content(
                    div().w_full().px_1().pb_1().child(ref_rows),
                )),
        );

        // ── 回收站（原型 425-432 acc-item「垃圾箱」；2026-10-11 对齐：可折叠 Accordion +
        // 胶囊 badge + 标题右侧清空按钮；还原不保留）──
        {
            // 回收站行（还原）── Accordion 面板内行；`trash_expanded` 记录初始展开态，
            // 折叠状态由 Accordion 管理（`on_toggle_click` 回写字段保状态一致）。
            let mut trash_rows = div().v_flex().w_full().gap_1();
            for t in &trash {
                let restore = {
                    let entity = entity.clone();
                    let id = t.manifest.id.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            this.restore_scratchpad_trash(id.clone(), cx)
                        });
                    }
                };
                trash_rows =
                    trash_rows.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .h(rems(ui::ROW_HEIGHT_COMPACT))
                            .pl(rems(1.125))
                            .pr_1p5()
                            .child(
                                div().flex_1().min_w_0().text_xs().text_color(muted).child(
                                    format!("{} · {}", t.manifest.name, t.manifest.origin),
                                ),
                            )
                            .child(
                                Button::new(format!("sp-trash-{}", t.manifest.id))
                                    .ghost()
                                    .small()
                                    .label("还原")
                                    .on_click(restore),
                            ),
                    );
            }
            // 清空按钮（标题右侧；原型 431 acc-foot「清空垃圾箱」收敛到头内右侧；
            // 2026-10-11 对齐：垃圾桶非空才显示）。
            let empty_btn = {
                let entity = entity.clone();
                if trash.is_empty() {
                    None
                } else {
                    Some(
                        div()
                            .invisible()
                            .group_hover("sp-trash-head", |s| s.visible())
                            .child(
                                Button::new("sp-trash-empty")
                                    .ghost()
                                    .small()
                                    .label("清空")
                                    .on_click(move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                                        entity.update(app, |this, cx| this.empty_scratchpad_trash(cx));
                                    }),
                            ),
                    )
                }
            };
            let trash_badge = Badge::new().count(trash.len());
            let toggle_trash = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        let mut v = this.scratchpad.borrow_mut();
                        v.trash_expanded = !v.trash_expanded;
                        cx.notify();
                    });
                }
            };
            let trash_head = div()
                .debug_selector(|| "sp-trash-head".into())
                .id("sp-trash-head")
                .group("sp-trash-head")
                .cursor_pointer()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .child(tree::disclosure_slot().child(tree::disclosure_icon(trash_expanded, muted)))
                .child("回收站")
                .child(div().flex_1())
                .child(trash_badge)
                .when_some(empty_btn, |this, btn| this.child(btn))
                .on_click(toggle_trash);
            body = body.child(
                div()
                    .v_flex()
                    .w_full()
                    .child(trash_head)
                    .child(Collapsible::new().open(trash_expanded).content(
                        div().w_full().px_1().pb_1().child(trash_rows),
                    )),
            );
        }
        body
    }
}


impl ScratchpadView {
    /// 面板头与工具栏：新建 / 新建文件夹 / 导入 / 引用 / 排序 / 刷新 + 选中时的剪贴板行，
    /// 以及 C-4 的冲突条（同一份草稿在编辑器里有未保存修改、磁盘上又被外部改了）。
    ///
    /// 为什么单独成方法：`render_scratchpad` 是面板唯一主视图——把「一行按钮怎么摆」这类
    /// 局部细节搬出来，主视图只剩装配顺序（行为不变：同一元素树，只换了落点）。
    fn render_scratchpad_header(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let warning = theme.colors.warning;
        let danger = theme.colors.danger;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();
        let conflicts = {
            let view = self.scratchpad.borrow();
            view.conflicts
                .iter()
                .map(|c| (c.relative.clone(), c.absolute.clone(), c.diff.is_some()))
                .collect::<Vec<_>>()
        };

        // ── 工具栏（新建文件 / 新建文件夹 / 刷新）──
        let start_new_file = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.start_scratchpad_edit(ScratchpadEdit::NewFile, window, cx)
                });
            }
        };
        let start_new_folder = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.start_scratchpad_edit(ScratchpadEdit::NewFolder, window, cx)
                });
            }
        };
        let refresh = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                view.borrow_mut().loaded = false;
                entity.update(app, |_, cx| cx.notify());
            }
        };
        // 内容搜索入口（VSCode 对齐，2026-10-11 用户点名）：进入搜索态时侧栏整体切换
        // 为搜索面板（隐藏工具栏/撤销栏/底部区隐藏）；与草稿图「搜文件」输入区分。
        let open_search_panel = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    v.search_panel_open = true;
                    v.search_mode = ScratchpadSearchMode::Content;
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };
        let cycle_sort = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    let (mut next_sort, mut next_desc) = (v.sort, v.sort_desc);
                    scratchpad_cycle_sort(&mut next_sort, &mut next_desc);
                    v.sort = next_sort;
                    v.sort_desc = next_desc;
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };

        let import_files = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.pick_scratchpad_imports(window, cx));
            }
        };
        let add_reference = {
            let entity_template = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                let entity = entity_template.clone();
                // 2026-10-12 对齐原型「弹窗输入路径+别名」：改弹应用内 Dialog 输入。
                entity.update(app, |this, cx| this.open_ref_dialog(window, cx));
            }
        };

        // K9：工具栏图标按钮由自绘 div 换成 gpui-kit `Button`（ghost 变体透明底）＋
        // hover 出背景；与原 24×24 按钮样式同尺寸；`small` = 24px 与 `w_6 h_6` 对齐）。
        let tool_icon_btn = |id: &'static str,
                             tip: &'static str,
                             icon: IconName,
                             handler: Box<
            dyn Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
        >| {
            Button::new(id)
                .ghost()
                .small()
                .icon(icon)
                .tooltip(tip)
                .on_click(move |ev, window, app| handler(ev, window, app))
        };
        // path 版图标按钮（assets 全量 SVG 不经 106 子集枚举）：如引用图标换用 link.svg。
        let tool_path_btn = |id: &'static str,
                             tip: &'static str,
                             icon_path: &'static str,
                             handler: Box<
            dyn Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
        >| {
            Button::new(id)
                .ghost()
                .small()
                .icon(Icon::empty().path(icon_path))
                .tooltip(tip)
                .on_click(move |ev, window, app| handler(ev, window, app))
        };

        let mut toolbar = div().v_flex().w_full();
        // ── 更多设置（2026-10-11 原型 644-658 对齐 + 持久化接线）：菜单勾选显示/隐藏
        // 各入口 + 左右位置调整 + 重置为默认布局；配置项 `settings.scratchpad.toolbar`
        // （空表 = 全部显示 + 默认位置（5 左 2 右））。状态快照在渲染时捕获进菜单：菜单每次
        // 打开重建菜单，点击后 notify 重渲染刷新快照，因此 checked/位置初始化始终与最近渲染一致。
        let (tb_cfg, more_entity) = {
            let view = self.scratchpad.borrow();
            (view.toolbar_cfg.clone(), entity.clone())
        };
        // 菜单每次打开重建：闭包需捕获外部 clone（闭包体内的 `clone()` 与
// 菜单闭包需捕获外部 clone（闭包体内的 `clone()` 与 move 捕获整个 tb_cfg。
        // 与下方过滤判断的借用冲突）
        let menu_cfg = tb_cfg.clone();
        let more_btn = Button::new("sp-more-trigger")
            .ghost()
            .small()
            .icon(Icon::empty().path("icons/ellipsis-vertical.svg"))
            .tooltip("更多设置（显示/隐藏/左右位置/拖拽排序）")
            .dropdown_menu(move |menu, _, _| {
                let entity = more_entity.clone();
                let cfg = menu_cfg.clone();
                let mut menu = menu;
                menu = menu.item(PopupMenuItem::label("工具栏按钮"));
                for key in ToolbarButtonKey::ALL {
                    let entity = entity.clone();
                    let current = cfg.get(&key).cloned().unwrap_or_default();
                    let checked = current.visible;
                    // 键不存在（用户没改过）→ 按钮默认侧；存在则取落盘 side（非 right 兜底左）。
                    let cur_side = cfg
                        .get(&key)
                        .map(|c| if c.side == "right" { "right" } else { "left" })
                        .unwrap_or_else(|| key.default_side_text());
                    // 行点击 = 切换显示/隐藏（保留既有交互）；「放在左」「放在右」按钮
                    // `app.stop_propagation()` 后只切位置，不触发行显隐。
                    let row_entity = entity.clone();
                    let row = PopupMenuItem::element(move |_window, _app| {
                        let entity = entity.clone();
                        let key = key;
                        let cur_side = cur_side;
                        let checked = checked;
                        let key_icon = match key {
                            ToolbarButtonKey::Search => Icon::new(IconName::Search),
                            ToolbarButtonKey::NewFile => Icon::new(IconName::Plus),
                            ToolbarButtonKey::NewFolder => Icon::new(IconName::Folder),
                            ToolbarButtonKey::Reference => Icon::empty().path("icons/link.svg"),
                            ToolbarButtonKey::Import => Icon::new(IconName::Inbox),
                            ToolbarButtonKey::Sort => Icon::empty().path("icons/sort-ascending.svg"),
                            ToolbarButtonKey::Refresh => Icon::new(IconName::RefreshCw),
                        }
                        .size(rems(1.));
                        // 可见：眼睛图标（eye=显示 / eye-off=隐藏），点击切换。
                        let eye_btn = {
                            let entity = entity.clone();
                            let key = key;
                            Button::new(format!("sp-eye-{}", key.as_str()))
                                .ghost()
                                .small()
                                .icon(if checked { IconName::Eye } else { IconName::EyeOff })
                                .tooltip(if checked { "隐藏该按钮" } else { "显示该按钮" })
                                .on_click(move |_, _, app| {
                                    app.stop_propagation();
                                    entity.update(app, |this, cx| {
                                        let mut v = this.scratchpad.borrow_mut();
                                        let cfg = v.toolbar_cfg.entry(key).or_insert_with(|| {
                                            ToolbarButtonCfg {
                                                visible: true,
                                                side: key.default_side_text().to_string(),
                                                order: -1,
                                                ..Default::default()
                                            }
                                        });
                                        cfg.visible = !cfg.visible;
                                        drop(v);
                                        this.persist_toolbar_cfg(cx);
                                        cx.notify();
                                    });
                                })
                        };
                        // 左右：Toggle 滑动开关（panel-left / panel-right），当前侧 checked。
                        let side_toggle = |target: &'static str, active: bool| {
                            let entity = entity.clone();
                            let key = key;
                            let target = target;
                            Toggle::new(format!("sp-st-{target}-{}", key.as_str()))
                                .icon(
                                    Icon::empty()
                                        .path(if target == "left" {
                                            "icons/panel-left.svg"
                                        } else {
                                            "icons/panel-right.svg"
                                        })
                                        .size(rems(1.)),
                                )
                                .checked(active)
                                .tooltip(if target == "left" {
                                    "放在左侧"
                                } else {
                                    "放在右侧"
                                })
                                .on_click(move |_, _, app| {
                                    app.stop_propagation();
                                    entity.update(app, |this, cx| {
                                        let mut v = this.scratchpad.borrow_mut();
                                        let cfg = v.toolbar_cfg.entry(key).or_insert_with(|| {
                                            ToolbarButtonCfg {
                                                visible: true,
                                                side: key.default_side_text().to_string(),
                                                order: -1,
                                                ..Default::default()
                                            }
                                        });
                                        cfg.side = target.to_string();
                                        drop(v);
                                        this.persist_toolbar_cfg(cx);
                                        cx.notify();
                                    });
                                })
                        };
                        // 顺序调整 ↑↓（2026-10-12 用户补充：工具栏按钮顺序可调；order 持久化）。
                        // 拖拽把手：on_drag 开始（携带 ToolbarButtonKey），on_drag_move 按位移重排。
                        let grip = {
                            let entity = entity.clone();
                            let key = key;
                            div()
                                .id(format!("sp-grip-{}", key.as_str()))
                                .h_flex()
                                .items_center()
                                .justify_center()
                                .w(rems(1.5))
                                .h(rems(1.5))
                                .rounded_sm()
                                .cursor_grab()
                                .child(
                                    Icon::empty()
                                        .path("icons/grip-vertical.svg")
                                        .size(rems(1.)),
                                )
                                .tooltip(move |window, cx| {
                                    Tooltip::element(move |_, _| div().text_xs().child("拖动调整顺序"))
                                        .build(window, cx)
                                })
                                .on_drag(key, |_, _, _, cx| cx.new(|_| ToolbarDragOverlay))
                                .on_drag_move::<ToolbarButtonKey>(move |event, _, app| {
                                    let pos_y = event.event.position.y.as_f32();
                                    let row_h = event.bounds.size.height.as_f32();
                                    let row_top = event.bounds.top().as_f32() - row_h / 2.0;
                                    let delta = ((pos_y - row_top) / row_h).round() as i32;
                                    if delta != 0 {
                                        entity.update(app, |this, cx| {
                                            this.reorder_toolbar(key, delta, cx);
                                            cx.notify();
                                        });
                                    }
                                })
                        };
                        div()
                            .h_flex()
                            .w_full()
                            .gap_1()
                            .items_center()
                            .child(key_icon)
                            .child(Label::new(key.label()))
                            .child(div().flex_1())
                            .child(eye_btn)
                            .child(side_toggle("left", cur_side == "left"))
                            .child(side_toggle("right", cur_side == "right"))
                            .child(grip)
                    })
                    .checked(checked)
                    .on_click(move |_, _, app| {
                        row_entity.update(app, |this, cx| {
                            let mut v = this.scratchpad.borrow_mut();
                            let cfg = v.toolbar_cfg.entry(key).or_insert_with(|| {
                                ToolbarButtonCfg {
                                    visible: true,
                                    side: key.default_side_text().to_string(),
 order: -1,
                                    ..Default::default()
                                }
                            });
                            cfg.visible = !cfg.visible;
                            drop(v);
                            this.persist_toolbar_cfg(cx);
                            cx.notify();
                        });
                    });
                    menu = menu.item(row);
                }
                menu = menu.item(PopupMenuItem::separator());
                menu = menu.item(
                    PopupMenuItem::new("重置为默认布局").on_click(move |_, _, app| {
                        entity.update(app, |this, cx| {
                            this.scratchpad.borrow_mut().toolbar_cfg.clear();
                            this.persist_toolbar_cfg(cx);
                            cx.notify();
                        });
                    }),
                );
                menu
            });

        // 可见性 + 所在侧（缺失 = 显示 + 默认侧；side 非 right 兜底左）。
        let tb_visible = |key: ToolbarButtonKey| tb_cfg.get(&key).map(|c| c.visible).unwrap_or(true);
        let tb_side = |key: ToolbarButtonKey| {
            tb_cfg
                .get(&key)
                .map(|c| if c.side == "right" { "right" } else { "left" })
                .unwrap_or_else(|| key.default_side_text())
        };
        // 按钮按「可见性 + 所在侧」归组；左右分组挂 debug_selector（元素级对齐测试锚点，
        // 与 `sp-res-file-text-*` 同为先例）：空组宽度为 0，可像素级断言「按钮已隐藏」。
        // B2（2026-10-28）：按钮进 `Toolbar`/`ToolbarGroup`（roving 键盘焦点 + a11y + 统一
        // size 托管），左右分组保留为 debug 锚点包装（测试断言语义不变）。
        let mut left_btns: Vec<Button> = Vec::new();
        let mut right_btns: Vec<Button> = Vec::new();
        let (hdr_sort, hdr_sort_desc) = {
            let v = self.scratchpad.borrow();
            (v.sort, v.sort_desc)
        };
        let buttons = [
            (
                ToolbarButtonKey::Search,
                tool_icon_btn("sp-content-search", "内容搜索（全文查找/替换）", IconName::Search, Box::new(open_search_panel)),
            ),
            (
                ToolbarButtonKey::NewFile,
                tool_icon_btn("sp-new-file", "新建文件", IconName::Plus, Box::new(start_new_file)),
            ),
            (
                ToolbarButtonKey::NewFolder,
                tool_icon_btn("sp-new-folder", "新建目录", IconName::Folder, Box::new(start_new_folder)),
            ),
            (
                ToolbarButtonKey::Import,
                tool_icon_btn("sp-import", "导入文件（复制到草稿箱）", IconName::Inbox, Box::new(import_files)),
            ),
            (
                ToolbarButtonKey::Reference,
                tool_path_btn("sp-add-ref", "添加外部引用（软连接）", "icons/link.svg", Box::new(add_reference)),
            ),
            (
                ToolbarButtonKey::Sort,
                tool_icon_btn(
                    "sp-sort",
                    scratchpad_sort_label(hdr_sort, hdr_sort_desc),
                    if hdr_sort_desc { IconName::SortDescending } else { IconName::SortAscending },
                    Box::new(cycle_sort),
                ),
            ),
            (
                ToolbarButtonKey::Refresh,
                tool_icon_btn("sp-refresh", "刷新草稿箱", IconName::RefreshCw, Box::new(refresh)),
            ),
        ];
        let mut ordered: Vec<_> = buttons.into_iter().collect();
        // 工具栏顺序（2026-10-12 用户补充：⋯ 菜单 ↑↓ 调整）：已调整（order ≥ 0）按 order 升序；
        // 未调整（order = -1）按原型默认序（ALL 枚举序）。
        let default_order = |k: ToolbarButtonKey| {
            ToolbarButtonKey::ALL
                .iter()
                .position(|x| *x == k)
                .unwrap_or(usize::MAX) as i32
        };
        ordered.sort_by_key(|(k, _)| {
            tb_cfg
                .get(k)
                .map(|c| c.order)
                .filter(|o| *o >= 0)
                .unwrap_or_else(|| default_order(*k))
        });
        for (key, btn) in ordered {
            if !tb_visible(key) {
                continue;
            }
            if tb_side(key) == "right" {
                right_btns.push(btn);
            } else {
                left_btns.push(btn);
            }
        }
        toolbar = toolbar.child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .h(rems(ui::PANEL_HEADER_HEIGHT))
                .px(rems(ui::PANEL_PADDING))
                .border_b(ui::HAIRLINE)
                .border_color(theme.colors.border)
                .child(
                    // Toolbar 容器：自带 roving 方向键焦点 + toolbar a11y 语义 + 统一 size；
                    // 背景/边框/高度由外层画（0.8.1 vendor toolbar.rs 文档同口径）。
                    Toolbar::new("sp-toolbar")
                        .content(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .debug_selector(|| "sp-toolbar-left".into())
                                .child(
                                    ToolbarGroup::new("sp-toolbar-left-group")
                                        .children(left_btns),
                                ),
                        )
                        .content(div().flex_1())
                        .content(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .debug_selector(|| "sp-toolbar-right".into())
                                .child(
                                    ToolbarGroup::new("sp-toolbar-right-group")
                                        .children(right_btns),
                                ),
                        )
                        .content(more_btn),
                ),
        );


        // ── 冲突条（C-4）：同一份草稿在编辑器里有未保存修改，磁盘上又被外部改了 ──
        for (relative, absolute, diff_ready) in conflicts {
            let show_diff = {
                let entity = entity.clone();
                let relative = relative.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.show_scratchpad_conflict_diff(relative.clone(), cx)
                    });
                }
            };
            let reload = {
                let entity = entity.clone();
                let absolute = absolute.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.reload_scratchpad_conflict(absolute.clone(), cx)
                    });
                }
            };
            let ignore = {
                let entity = entity.clone();
                let relative = relative.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.ignore_scratchpad_conflict(relative.clone(), cx)
                    });
                }
            };
            toolbar = toolbar.child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .p_1()
                    .rounded_sm()
                    .border_1()
                    .border_color(warning)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(danger)
                                    .child(format!("冲突：{relative}")),
                            )
                            .child(
                                Button::new(format!("sp-conflict-diff-{relative}"))
                                    .small()
                                    .label(if diff_ready { "查看差异" } else { "计算中…" })
                                    .disabled(!diff_ready)
                                    .on_click(show_diff),
                            )
                            .child(
                                Button::new(format!("sp-conflict-reload-{relative}"))
                                    .small()
                                    .label("重载")
                                    .on_click(reload),
                            )
                            .child(
                                Button::new(format!("sp-conflict-ignore-{relative}"))
                                    .small()
                                    .ghost()
                                    .label("忽略")
                                    .on_click(ignore),
                            ),
                    ),
            );
        }

        toolbar
    }
}


impl ScratchpadView {
    /// 搜索行：文件名过滤 / 内容搜索的模式 chip + 输入框 + 内容模式下的 `.*` 与 `Aa`。
    ///
    /// 搜索状态在点击回调里改（`view.search_mode` 等），这里只做投影与渲染。
    /// 搜索态完整面板（2026-10-11 VSCode 对齐；由草稿图工具栏 ⌕ 进入，← 返回退出）。
    /// 顶部工具栏（返回 / 刷新 / 清除 / 新建编辑器查询 / 展开折叠明细）→ 垂直表单
    /// （搜索 / 替换 / 包含 / 排除）→ 统一搜索结果树。
    fn render_scratchpad_search_pane(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let _hover_bg = theme.colors.list_hover;
        let _selected_bg = theme.colors.list_active;
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();

        // ── 顶部工具栏（VSCode 搜索面板形态；原型 437-445）──
        let close_panel = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    v.search_panel_open = false;
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };
        let refresh_search = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.run_scratchpad_content_search(cx));
            }
        };
        let clear_search_box = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                // 用户点名（2026-10-14）：工具栏「清除」清的是**搜索框内容**而不是结果。
                // `InputState::set_value` 不发 `Change` 事件（程序化回填不触发副作用），
                // 所以结果要在这里同步清掉，界面不会残留旧结果。
                if let Some(input) = view.borrow().search_input.clone() {
                    input.update(app, |s, cx| s.set_value("", window, cx));
                }
                {
                    let mut v = view.borrow_mut();
                    v.search_view = None;
                    v.search_stats = None;
                    v.search_expanded.clear();
                }
                entity.update(app, |this, cx| {
                    this.host.show_search_results(None, cx);
                    this.host.notify_host(cx);
                    cx.notify();
                });
            }
        };
        let open_in_editor_tab = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    let view = this.scratchpad.borrow().search_view.clone();
                    this.host.show_search_results(view, cx);
                    this.host.notify_host(cx);
                });
            }
        };
        // ─ 展开/折叠下方匹配明细：点击切换「全部收起 / 全部展开」。
        let toggle_details = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                let mut v = view.borrow_mut();
                let has_hit_collapsed = v.search_expanded.iter().any(|k| k.starts_with("h:"));
                if !has_hit_collapsed {
                    // 全部收起：按当前结果收集命中 / 文件 key。
                    // 只收集命中明细键（h:{file}#{line}），目录/文件行折叠状态不受影响（2026-10-12）。
                    let mut keys: Vec<String> = Vec::new();
                    if let Some(sv) = &v.search_view {
                        for hit in &sv.hits {
                            keys.push(format!("h:{}#{}", hit.file, hit.line));
                        }
                    }
                    v.search_expanded.extend(keys);
                } else {
                    v.search_expanded.retain(|k| !k.starts_with("h:"));
                }
                drop(v);
                entity.update(app, |_, cx| cx.notify());
            }
        };
        // 展开/折叠明细按钮图标两态（2026-10-12）：h: 键存在（明细已收起）→ ChevronDown（点击展开）；
        // 无 h: 键（明细已展开）→ ChevronUp（点击收起）。
        let details_icon = if self
            .scratchpad
            .borrow()
            .search_expanded
            .iter()
            .any(|k| k.starts_with("h:"))
        {
            IconName::ChevronDown
        } else {
            IconName::ChevronUp
        };
        let toolbar = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_1p5()
            .py_1()
            .border_b_1()
            .border_color(theme.colors.border)
            .child(
                Button::new("sp-search-back")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowLeft)
                    .tooltip("返回文件树（退出搜索状态）")
                    .on_click(close_panel),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("搜索"),
            )
            .child(div().flex_1())
            .child(
                Button::new("sp-search-refresh")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .tooltip("重新执行当前搜索")
                    .on_click(refresh_search),
            )
            .child(
                Button::new("sp-search-clear")
                    .ghost()
                    .small()
                    .icon(IconName::Close)
                    .tooltip("清除搜索内容")
                    .on_click(clear_search_box),
            )
            .child(
                Button::new("sp-search-newtab")
                    .ghost()
                    .small()
                    .icon(IconName::Search)
                    .tooltip("新建编辑器查询（打开「搜索 query」独立 Tab）")
                    .on_click(open_in_editor_tab),
            )
            .child(
                // 原型 445 sp-details：「文件 ▸」（2026-10-11 对齐；展开/折叠下方匹配明细）。
                Button::new("sp-search-details")
                    .ghost()
                    .small()
                    .icon(details_icon)
                    .tooltip("展开/折叠下方匹配明细")
                    .on_click(toggle_details),
            );

        // ── 搜索（内容搜索；草稿图文件搜索入口已收敛为普通工具栏下方的过滤行）──
        let (search_regex, search_case) = {
            let v = self.scratchpad.borrow();
            (v.search_regex, v.search_case)
        };
        let toggle_regex = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    v.search_regex = !v.search_regex;
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };
        let toggle_case = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    v.search_case = !v.search_case;
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };
        let toggle_word = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    v.search_word = !v.search_word;
                }
                entity.update(app, |_, cx| cx.notify());
            }
        };
        // ── 搜索行（VSCode 式，2026-10-11 用户点名）：Aa/ab/.* 嵌在输入框内右侧 ──
        // 运行方式：输入框 Enter 触发（`ensure_scratchpad_inputs` 的 PressEnter 订阅）；
        // 不再有模式 chip / 运行按钮（原型 449-460，用户拍板确认删除）。
        let mut search_row = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_1p5()
            .pb_1();
        if let Some(input) = self.scratchpad.borrow().content_search_input.clone() {
            let search_word = self.scratchpad.borrow().search_word;
            let word_btn = Button::new("sp-word")
                .ghost()
                .small()
                .label("ab")
                .toggled(search_word)
                .tooltip("全字匹配（ab 开关：仅匹配完整单词）")
                .on_click(toggle_word);
            let reg_btn = Button::new("sp-regex")
                .ghost()
                .small()
                .label(".*")
                .toggled(search_regex)
                .tooltip("使用正则表达式（如 \\d+ 匹配数字；关掉则按纯文本匹配）")
                .on_click(toggle_regex);
            let case_btn = Button::new("sp-case")
                .ghost()
                .small()
                .icon(IconName::CaseSensitive)
                .toggled(search_case)
                .tooltip("区分大小写（Aa 开关）")
                .on_click(toggle_case);
            search_row = search_row.child(
                div().flex_1().min_w_0().child(
                    Input::new(&input)
                        .bordered(false)
                        .focus_bordered(true)
                        .w_full()
                        .suffix(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_0p5()
                                // 顺序对齐原型 450-452：Aa / ab / .*（2026-10-11 全面对齐：与中间 es-bar 一致）。
                                .child(case_btn)
                                .child(word_btn)
                                .child(reg_btn),
                        ),
                ),
            );


        }

        // ── VSCode 搜索面板形态（2026-10-10 对齐）：搜索框下方 = 替换行 + 高级选项 ──
        // 替换输入/包含/排除输入由 `ensure_scratchpad_inputs` 懒建；搜索态内恒为内容搜索。
        let mut search_block = div().v_flex().w_full().gap_1().px_1p5().pb_1();
        search_block = search_block.child(toolbar).child(search_row);
        // 替换行：`[输入(placeholder=替换, 内嵌 AB)] [⬇ 全部替换] [⋯ 高级选项开关]`
        // （2026-10-11 用户点名：搜索/替换输入框的提示已明；AB=保留大小写开关嵌框内；
        //    ⋯ 控制包含/排除文件 glob 的展开与收起，默认收起。）
        let replace_filled = self
                .scratchpad
                .borrow()
                .search_replace_input
                .as_ref()
                .map(|i| !i.read(cx).value().trim().is_empty())
                .unwrap_or(false);
            let replace_all = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| this.replace_scratchpad_all(cx));
                }
            };
            let advanced = self.scratchpad.borrow().search_advanced;
            let toggle_advanced = {
                let view = view_handle.clone();
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    {
                        let mut v = view.borrow_mut();
                        v.search_advanced = !v.search_advanced;
                    }
                    entity.update(app, |_, cx| cx.notify());
                }
            };
            let toggle_replace_case = {
                let view = view_handle.clone();
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    {
                        let mut v = view.borrow_mut();
                        v.search_replace_case = !v.search_replace_case;
                    }
                    entity.update(app, |_, cx| cx.notify());
                }
            };
            let replace_case = self.scratchpad.borrow().search_replace_case;
            let mut replace_row = div().h_flex().items_center().gap_1().w_full();
            if let Some(input) = self.scratchpad.borrow().search_replace_input.clone() {
                // AB 开关嵌在替换输入框内右侧（VSCode 式；与搜索行 Aa/.* 同形态）。
                let ab_btn = Button::new("sp-replace-case")
                    .ghost()
                    .small()
                    .label("AB")
                    .toggled(replace_case)
                    .tooltip("保留大小写（已接入：替换时保持命中词的原有大小写格式，如全大写/首字母大写/全小写）")
                    .on_click(toggle_replace_case);
                replace_row = replace_row.child(
                    div().flex_1().min_w_0().child(
                        Input::new(&input)
                            .bordered(false)
                            .focus_bordered(true)
                            .w_full()
                            .suffix(div().h_flex().items_center().gap_0p5().child(ab_btn)),
                    ),
                );
            } else {
                replace_row = replace_row.child(div().flex_1());
            }
            replace_row = replace_row.child(
                // 用户点名（2026-10-10）：查找/替换这类动作按钮用图标 + tooltip 做提示
                // （gpui-kit 组件；图标 `IconName::Replace`，语义与 VSCode 替换入口一致）。
                Button::new("sp-replace-all")
                    .small()
                    .icon(IconName::Replace)
                                .tooltip("全部替换（替换输入框内按 Enter 等效）")
                    .disabled(!replace_filled)
                    .on_click(replace_all),
            );
            // 三个竖点：包含/排除文件 glob 的展开/收起开关（默认收起，2026-10-11 用户点名）。
            replace_row = replace_row.child(
                Button::new("sp-advanced-toggle")
                    .small()
                    .icon(IconName::Ellipsis)
                    .tooltip(if advanced {
                            "收起高级选项（包含/排除文件）"
                    } else {
                            "展开高级选项（包含/排除文件）"
                    })
                    .on_click(toggle_advanced),
            );
            search_block = search_block.child(replace_row);

            // 高级选项：`包含的文件 / 排除的文件`（VSCode glob；由替换行右侧 ⋯ 展开收起）。
            // 2026-10-11 用户点名对齐原型：**无显式 label**（placeholder 即提示）。
            // 行尾操作按钮（包含：编辑 / 👁 查看；排除：设置）。
            // 输入框聚焦时显示 glob 样例 hint。
            if advanced {
                let include_input =
                    self.scratchpad.borrow().search_include_input.clone();
                let exclude_input =
                    self.scratchpad.borrow().search_exclude_input.clone();
                let hint_include = {
                    let view = view_handle.clone();
                    let entity = entity.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        let mut v = view.borrow_mut();
                        v.show_include_hint = !v.show_include_hint;
                        drop(v);
                        entity.update(app, |_, cx| cx.notify());
                    }
                };
                let hint_exclude = {
                    let view = view_handle.clone();
                    let entity = entity.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        let mut v = view.borrow_mut();
                        v.show_exclude_hint = !v.show_exclude_hint;
                        drop(v);
                        entity.update(app, |_, cx| cx.notify());
                    }
                };
                let mut include_row = div().h_flex().w_full().items_center().gap_1();
                if let Some(input) = include_input {
                    include_row = include_row
                        .child(div().flex_1().min_w_0().child(Input::new(&input).bordered(false).focus_bordered(true).w_full()))
                        .child(
                            Button::new("sp-glob-edit")
                                .ghost()
                                .small()
                                .icon(IconName::Settings2)
                                .tooltip("编辑包含文件规则（glob）")
                                .on_click(hint_include.clone()),
                        )
                        .child(
                            Button::new("sp-glob-view")
                                .ghost()
                                .small()
                                .icon(IconName::Eye)
                                .tooltip("查看 glob 样例")
                                .on_click(hint_include),
                        );
                }
                let mut exclude_row = div().h_flex().w_full().items_center().gap_1();
                if let Some(input) = exclude_input {
                    exclude_row = exclude_row
                        .child(div().flex_1().min_w_0().child(Input::new(&input).bordered(false).focus_bordered(true).w_full()))
                        .child(
                            Button::new("sp-glob-settings")
                                .ghost()
                                .small()
                                .icon(IconName::Settings)
                                .tooltip("设置排除文件规则（glob）")
                                .on_click(hint_exclude),
                        );
                }
                search_block = search_block.child(include_row).child(exclude_row);

                // glob 样例 hint（2026-10-11 用户点名：点击输入框时给出包含/排除样例）：
                // 聚焦输入框或点操作按钮时显示，浅色小字 + 斜杠分隔。
                let (include_focused, exclude_focused) = {
                    let v = self.scratchpad.borrow();
                    (
                        v.show_include_hint,
                        v.show_exclude_hint,
                    )
                };
                let include_focused = include_focused
                    || self
                        .scratchpad
                        .borrow()
                        .search_include_input
                        .as_ref()
                        .map(|i| i.read(cx).presentation().focus_handle().is_focused(window))
                        .unwrap_or(false);
                let exclude_focused = exclude_focused
                    || self
                        .scratchpad
                        .borrow()
                        .search_exclude_input
                        .as_ref()
                        .map(|i| i.read(cx).presentation().focus_handle().is_focused(window))
                        .unwrap_or(false);
                if include_focused {
                    search_block = search_block.child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .px_1()
                            .child("样例：*.sql, src/**/include（留空搜索全部文件）"),
                    );
                }
                if exclude_focused {
                    search_block = search_block.child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .px_1()
                            .child("样例：*.log, .git/, target/（留空不排除）"),
                    );
                }
            }

            // 统计行（VSCode 搜索面板「N 文件中的 M 个结果」）：最近一次内容搜索的去重文件数、
            // 命中行数与扫描文件数；`search_stats = None`（未搜索 / 已清空）时不显示。
            // 右侧「在编辑器中打开」链接（原型 484，2026-10-11 用户点名新增）。
            if let Some((files, hits, scanned)) = self.scratchpad.borrow().search_stats {
                let stats_text = if hits == 0 {
                    format!("无结果 · 已扫描 {scanned} 个文件")
                } else {
                    format!("{files} 个文件中 {hits} 个结果 · 已扫描 {scanned} 个文件")
                };
                let open_in_editor = {
                    let entity = entity.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| {
                            let view = this.scratchpad.borrow().search_view.clone();
                            this.host.show_search_results(view, cx);
                            this.host.notify_host(cx);
                        });
                    }
                };
                search_block = search_block.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .w_full()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(muted)
                                .child(stats_text),
                        )
                        .child(
                            Button::new("sp-stats-open")
                                .ghost()
                                .small()
                                .label("在编辑器中打开")
                                .on_click(open_in_editor),
                        ),
                );
            }

            // 侧栏搜索结果树（2026-10-11 用户点名：结构与草稿箱树一致，目录用 folder 图标、
            // 文件类型图标、层级缩进，命中明细缩进对齐文件节点名）。
            if let Some(search) = self.scratchpad.borrow().search_view.clone() {
                if !search.hits.is_empty() {
                    search_block =
                        search_block.child(self.render_search_results_tree(&search, cx));
                }
            }

        search_block
    }
}


impl ScratchpadView {
    /// 草稿树：内联新建行的落点、行高表、虚拟列表与空态（面板唯一滚动区）。
    ///
    /// 为什么单独成方法：这是面板的主体，`render_scratchpad` 里它最长的一段脚手架；
    /// 搬出来之后主视图只剩「状态投影 + 装配顺序」（行为不变：同一元素树，只换了落点）。
    fn render_scratchpad_tree(
        &self,
        rows: Vec<(usize, ScratchpadEntry)>,
        loading: bool,
        filter: &str,
        mode: ScratchpadSearchMode,
        regex: bool,
        case_sensitive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let hover_bg = theme.colors.list_hover;
        let selected_bg = theme.colors.list_active;
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let folder_color = theme.colors.warning;
        let primary = theme.colors.primary;
        let info = theme.colors.info;
        let success = theme.colors.success;
        let active_border = theme.colors.list_active_border;

        let entity = cx.entity();
        let row_count = rows.len();
        let (selected, expanded, loaded_children, edit) = {
            let view = self.scratchpad.borrow();
            (
                view.selected.clone(),
                view.expanded.clone(),
                view.children.clone(),
                view.edit.clone(),
            )
        };

        // 内联新建的文件/文件夹行定位：在目标文件夹下一行（未选中文件夹则列表首行）。
        let new_target = self.scratchpad.borrow().new_target.clone();
        let edit_insert: Option<(usize, usize)> = match edit.as_ref() {
            Some(ScratchpadEdit::NewFile) | Some(ScratchpadEdit::NewFolder) => {
                if new_target.is_empty() {
                    Some((0, 0))
                } else {
                    rows.iter()
                        .position(|(_, e)| e.path.to_string_lossy() == new_target)
                        .map(|i| (i + 1, rows[i].0 + 1))
                        .or(Some((0, 0)))
                }
            }
            _ => None,
        };
        let display_count = row_count + usize::from(edit_insert.is_some());
        let row_ctx = ScratchpadRowCtx {
            keys: Rc::new(
                rows.iter()
                    .map(|(_, e)| e.path.to_string_lossy().to_string())
                    .collect(),
            ),
            rows: Rc::new(rows),
            dirty: self.dirty_seen.borrow().clone(),
            edit: edit.clone(),
            edit_insert,
            selected: selected.clone(),
            expanded: expanded.clone(),
            loaded: loaded_children.clone(),
            colors: ScratchpadRowColors {
                hover_bg,
                selected_bg,
                fg,
                muted,
                folder_color,
                primary,
                info,
                success,
                active_border,
            },
            filter: Rc::new(filter.to_string()),
            mode,
            regex,
            case_sensitive,
        };

        let mut drafts = div().v_flex().flex_1().min_h_0().w_full().gap_1().px_1();
        if display_count == 0 {
            // 加载中不显示空态引导，避免「草稿箱是空的」闪现。
            if loading {
                drafts = drafts.child(
                    div()
                        .v_flex()
                        .items_center()
                        .w_full()
                        .pt_6()
                        .px_2()
                        .text_xs()
                        .text_color(muted)
                        .child("加载中…")
                );
            } else {
                drafts = drafts.child(self.render_scratchpad_empty_state(&entity, &filter, cx));
            }
        } else {
            let sizes: Rc<Vec<Size<Pixels>>> =
                tree::row_sizes(display_count, window.rem_size(), |i| {
                    Self::scratchpad_row_height(&row_ctx, i)
                });
            let list_ctx = row_ctx.clone();
            let scroll = self.scratchpad.borrow().list_scroll.clone();
            let list = v_virtual_list(
                entity.clone(),
                "sp-drafts",
                sizes,
                move |this, range: std::ops::Range<usize>, _window, cx| {
                    range
                        .map(|i| this.scratchpad_row(i, &list_ctx, cx))
                        .collect::<Vec<AnyElement>>()
                },
            )
            .track_scroll(scroll.handle());
            // 原型 387-411（2026-10-11 用户点名「主体 Tree 也用 Accordion，显示
            // 项目·草稿 或 草稿」）：主体草稿区块 = 单个折叠容器，默认展开。
            // 标题 = 「草稿」，右侧圆形胶囊 badge（条数），折叠头自带 chevron。
            // 2026-10-12 用户方向：Accordion 内嵌虚拟列表高度测量异常（树空白），
            // 改用 Collapsible（gpui_base 揭盖容器，content 收起时不渲染）承载草稿主体。
            let drafts_expanded = self.scratchpad.borrow().drafts_expanded;
            let hdr_new_file = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.start_scratchpad_edit(ScratchpadEdit::NewFile, window, cx)
                    });
                }
            };
            let hdr_new_folder = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.start_scratchpad_edit(ScratchpadEdit::NewFolder, window, cx)
                    });
                }
            };
            let hdr_refresh = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.scratchpad.borrow_mut().loaded = false;
                        cx.notify();
                    });
                }
            };
            let toggle_drafts = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        let mut v = this.scratchpad.borrow_mut();
                        v.drafts_expanded = !v.drafts_expanded;
                        cx.notify();
                    });
                }
            };
            let drafts_head = div()
                .debug_selector(|| "sp-drafts-head".into())
                .id("sp-drafts-head")
                .group("sp-drafts-head")
                .cursor_pointer()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .child(tree::disclosure_slot().child(tree::disclosure_icon(drafts_expanded, muted)))
                .child("草稿")
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .invisible()
                        .group_hover("sp-drafts-head", |s| s.visible())
                        .h_flex()
                        .items_center()
                        .gap_0p5()
                        .child(
                            Button::new("sp-drafts-head-new-file")
                                .ghost()
                                .small()
                                .icon(IconName::Plus)
                                .tooltip("新建文件")
                                .on_click(hdr_new_file),
                        )
                        .child(
                            Button::new("sp-drafts-head-new-folder")
                                .ghost()
                                .small()
                                .icon(IconName::Folder)
                                .tooltip("新建目录")
                                .on_click(hdr_new_folder),
                        )
                        .child(
                            Button::new("sp-drafts-head-refresh")
                                .ghost()
                                .small()
                                .icon(IconName::RefreshCw)
                                .tooltip("刷新草稿箱")
                                .on_click(hdr_refresh),
                        ),
                )
                .child(Badge::new().count(row_count))
                .on_click(toggle_drafts);
            drafts = drafts.child(
                div()
                    .v_flex()
                    .w_full()
                    .min_h_0()
                    .when(drafts_expanded, |s| s.flex_1())
                    .when(!drafts_expanded, |s| s.flex_none())
                    .child(drafts_head)
                    .child(Collapsible::new().open(drafts_expanded).h_full().content(
                        div().flex_1().min_h_0().w_full().child(list),
                    )),
            );
        }

        drafts
    }

    /// 更多设置持久化（2026-10-11 接线）：把面板 `toolbar_cfg` 全量写回
    /// `settings.scratchpad.toolbar`（与默认一致的键不落盘，保持「空表 = 全部显示 +
    /// 默认位置」定义），再原子写盘。落盘失败不改本进程已生效。
    /// （`settings::save_settings` 会记录原因，见上面的 `last_save_error`）。
    pub(super) fn persist_toolbar_cfg(&self, cx: &mut Context<Self>) {
        let map = {
            let v = self.scratchpad.borrow();
            let mut out = std::collections::BTreeMap::new();
            for (key, cfg) in &v.toolbar_cfg {
                // 与默认完全一致（显示 + 默认侧 + 未调整顺序）的键不落盘：空表 = 全默认语义。
                // B3（2026-10-28）：补齐 `order` 维度——用户拖拽重排过（order ≥ 0）的按钮
                // 必须落盘，否则重启后顺序回退到原型默认序。
                if cfg.visible && cfg.side == key.default_side_text() && cfg.order < 0 {
                    continue;
                }
                out.insert(key.as_str().to_string(), cfg.clone());
            }
            out
        };
        if cx.has_global::<settings::model::Settings>() {
            cx.global_mut::<settings::model::Settings>()
                .scratchpad
                .toolbar = map;
            let snapshot = cx.global::<settings::model::Settings>().clone();
            let _ = settings::save_settings(&snapshot);
        }
    }

    /// 分区高度持久化（B3，2026-10-28）：把拖拽后的 `group_max_h` 写回
    /// `settings.scratchpad.group_max_h` 并落盘（模型默认 7.5 = 面板默认）。
    /// 落盘失败不改本进程已生效（与 `persist_toolbar_cfg` 同口径）。
    pub(super) fn persist_group_max_h(&self, cx: &mut Context<Self>) {
        let h = self.scratchpad.borrow().group_max_h;
        if cx.has_global::<settings::model::Settings>() {
            cx.global_mut::<settings::model::Settings>()
                .scratchpad
                .group_max_h = h;
            let snapshot = cx.global::<settings::model::Settings>().clone();
            let _ = settings::save_settings(&snapshot);
        }
    }

    /// 添加外部引用弹窗（2026-10-12 原型「弹窗输入路径+别名」对齐）：
    /// gpui-kit Dialog + 路径/别名两个 TextField，确认后创建引用（on_ok 返回 true 关闭）。
    fn render_ref_dialog(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let path_input = self.scratchpad.borrow().ref_path_input.clone();
        let alias_input = self.scratchpad.borrow().ref_alias_input.clone();
        let close = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.scratchpad.borrow_mut().ref_dialog_open = false;
                    cx.notify();
                });
            }
        };
        let confirm = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| -> bool {
                entity.update(app, |this, cx| this.confirm_ref_dialog(window, cx))
            }
        };
        Dialog::new(cx)
            .title(div().child("添加外部引用"))
            .content({
                let path_input = path_input.clone();
                let alias_input = alias_input.clone();
                move |content: DialogContent, window: &mut Window, cx: &mut App| {
                    let _ = (window, cx);
                    let mut body = div().v_flex().w_full().gap_1().px_3().py_1();
                    if let Some(input) = &path_input {
                        body = body
                            .child(Label::new("路径（文件或目录）"))
                            .child(Input::new(input).w_full());
                    }
                    if let Some(input) = &alias_input {
                        body = body
                            .child(Label::new("别名（默认取名称）"))
                            .child(Input::new(input).w_full());
                    }
                    content.child(body)
                }
            })
            .button_props(
                DialogButtonProps::default()
                    .show_cancel(true)
                    .ok_text("添加")
                    .on_ok(confirm),
            )
            .on_close(close)
            .into_any_element()
    }

    /// 标签编辑弹窗（统一规格 §7.3）：项目标签勾选列表（勾 = 打标）+ 新建标签输入行。
    ///
    /// 内容每帧从 `scratchpad` 状态重读（勾选态随 toggle 更新）；勾选 / 新建走事件路径
    /// （`toggle_entry_tag` / `create_entry_tag`），本方法只装配。
    fn render_tag_dialog(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let scratchpad = self.scratchpad.clone();
        let new_input = self.scratchpad.borrow().tag_new_input.clone();
        let close = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.close_tag_dialog(cx));
            }
        };
        let done = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| -> bool {
                entity.update(app, |this, cx| this.close_tag_dialog(cx));
                true
            }
        };
        Dialog::new(cx)
            .title(div().child("标签"))
            .content({
                let entity = entity.clone();
                let scratchpad = scratchpad.clone();
                let new_input = new_input.clone();
                move |content: DialogContent, _window: &mut Window, cx: &mut App| {
                    let theme = cx.theme();
                    let (rel_path, tags, entry) = {
                        let view = scratchpad.borrow();
                        let Some(rel) = view.tag_dialog.clone() else {
                            return content.child(div());
                        };
                        let tags = view.project_tags.clone();
                        let entry = view.entry_tags.get(&rel).cloned().unwrap_or_default();
                        (rel, tags, entry)
                    };
                    let mut body = div().v_flex().w_full().gap_1().px_3().py_1();
                    if tags.is_empty() {
                        body = body.child(
                            div()
                                .w_full()
                                .py_1()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("还没有标签——在下面输入一个名字即可新建"),
                        );
                    }
                    for tag in &tags {
                        let checked = entry.iter().any(|t| t.id == tag.id);
                        let toggle_entity = entity.clone();
                        let toggle_id = tag.id.clone();
                        let toggle_rel = rel_path.clone();
                        let toggle =
                            move |_checked: &bool, _: &mut gpui_kit::Window, app: &mut App| {
                                toggle_entity.update(app, |this, cx| {
                                    this.toggle_entry_tag(&toggle_rel, &toggle_id, cx)
                                });
                            };
                        body = body.child(
                            div()
                                .id(format!("sp-tag-{}", tag.id))
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .w_full()
                                .px_2()
                                .py_1()
                                .rounded_sm()
                                .when(checked, |row| row.bg(theme.colors.list_active))
                                .hover(|s| s.bg(theme.colors.list_hover))
                                .child(
                                    Checkbox::new(SharedString::from(format!(
                                        "sp-tag-check-{}",
                                        tag.id
                                    )))
                                    .checked(checked)
                                    .on_click(toggle),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_ellipsis()
                                        .text_color(theme.colors.foreground)
                                        .child(tag.name.clone()),
                                ),
                        );
                    }
                    if let Some(input) = &new_input {
                        let add_entity = entity.clone();
                        let add_rel = rel_path.clone();
                        let add =
                            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                                add_entity.update(app, |this, cx| this.create_entry_tag(&add_rel, cx));
                            };
                        body = body.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .w_full()
                                .pt_1()
                                .child(div().flex_1().min_w_0().child(Input::new(input).w_full()))
                                .child(Button::new("sp-tag-add").small().label("添加").on_click(add)),
                        );
                    }
                    content.child(body)
                }
            })
            .button_props(
                DialogButtonProps::default()
                    .show_cancel(false)
                    .ok_text("完成")
                    .on_ok(done),
            )
            .on_close(close)
            .into_any_element()
    }
}
