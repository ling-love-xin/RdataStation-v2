//! 面板外壳：面板头 / 工具�?/ 搜索�?/ 草�?树�?�?/ 引用 / 回收站/ 撤销�?/ 状�?��?�?//!
//! 这是 `render_scratchpad` 的落点�?��?�草稿�?�?��的主视图（虚拟列表的装配也在这里�?//! 行本�?�� `rows.rs`）�??
use super::*;

use gpui_kit::component::accordion::Accordion;
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::button::DropdownButton;
use gpui_kit::component::label::Label;
// `Size` �?gpui 泛型 `Size<Pixels>` 冲突 �?�?�� `KitSize`（gpui-component 枚举）�
use gpui_kit::component::Size as KitSize;
use gpui_kit::component::Sizable;

impl ScratchpadView {
    /// 草�?箱面板（M5）：�?= `{project}/scratchpad/`�?    ///
    /// �?��：新建（内联�?重命�?删除→回收站+撤销�?回收站恢复与清空/文件名过�?外部引用移除�
    pub fn render_scratchpad(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
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
        // �?�? 搜索态整体切�?��2026-10-11 用户点名，VSCode 对齐）：面板打开时侧栏只显示
        // 搜索面板（普通工具栏 / �?���?/ 底部引用回收站整体消失）�?�?
        if self.scratchpad.borrow().search_panel_open {
            return self.render_scratchpad_search_pane(window, cx);
        }
        let (rows, error, external_refs, trash, sort, sort_desc, edit, undo, filter, loading) = {
            let view = self.scratchpad.borrow();
            let filter = view
                .search_input
                .as_ref()
                .map(|i| i.read(cx).value().trim().to_string())
                .unwrap_or_default();
            // �??��?图的文件搜索 = 恒文件名过滤（VSCode：普通搜索�?=搜文件，工具�?�?搜内容）�
            let (search_mode, search_regex, search_case) =
                (ScratchpadSearchMode::Name, view.search_regex, view.search_case);
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
        // OS 文件拖入状�?�（Phase C）：进入面板时置位并计数，drop / exit 复位�
        let drag_hint = {
            let view = self.scratchpad.borrow();
            (view.external_drag_over, view.external_drag_count)
        };

        let toolbar = self.render_scratchpad_header(cx);

        // �?�? �??��?图文件搜索�?（VSCode 形�?�，2026-10-11 用户点名）：输入�?+ 框�?右侧 �?�?�?
        // �?��：按文件名过滤树（实时）；⌕ = 聚焦输入框（Enter 等效，过滤实时无�?��执�?）；
        // 内�?搜索入口在普通工具栏 ⌕（`open_search_panel`），两�?图标区分（搜文件 vs 搜内容）�
        let search_row = {
            let theme = cx.theme();
            let filter_input = self.scratchpad.borrow().search_input.clone();
            let filter_input_for_btn = filter_input.clone();
            let run_file_filter = move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                // 点击 ⌕：聚焦输入框（实时过滤已生效；�?Enter 同�?义）�
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
                .pb_1();
            if let Some(input) = filter_input {
                row = row
                    .child(div().flex_1().min_w_0().child(Input::new(&input).w_full()))
                    .child(
                        div()
                            .id("sp-file-search-run")
                            .h_flex()
                            .items_center()
                            .justify_center()
                            .px_1()
                            .h_5()
                            .rounded_sm()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .hover(move |s| s.bg(theme.colors.list_hover))
                            .child(IconName::Search)
                            .tooltip(|window, cx| {
                                Tooltip::element(move |_, _| {
                                    div().text_xs().child("搜索文件（按文件名过滤目录树）")
                                })
                                .build(window, cx)
                            })
                            .on_click(run_file_filter),
                    );
            } else {
                row = row.child(div().flex_1());
            }
            row
        };

        let mut panel = div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .child(toolbar)
            .child(search_row);

        // OS 文件拖入提示条（Phase C：进入面板即提示，松�?落盘导入）�?
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
            return panel.child(
                div()
                    .flex_1()
                    .w_full()
                    .px_2p5()
                    .py_3()
                    .text_xs()
                    .text_color(muted)
                    .child(err.clone()),
            );
        }

        // 顶部内联新建（仅「新建引用�?�：文件/文件夹的内联行插在目标文件夹下）�
        if matches!(edit.as_ref(), Some(ScratchpadEdit::NewReference { .. })) {
            panel = panel.child(div().px_1().child(self.render_scratchpad_edit_row(0, cx)));
        }

        // �?�? 草�?树（面板�?��滚动区）�?�? �??�?`render_scratchpad_tree` 里�?
        let row_count = rows.len();
        let file_count = rows
            .iter()
            .filter(|(_, e)| e.kind == ScratchpadEntryKind::File)
            .count();
        let folder_count = row_count - file_count;
        // 过滤三�?素（mode/regex/case）与 flatten 同一来源，供树�?命中数�?标用�?        // �??��?图恒为文件名过滤（Name；内容搜索只在搜索�?�面板，2026-10-11 对齐）�?
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

        // �?�? 底部固定区（引用 / 回收站/ 撤销�?/ 状�?�；不随草�?树滚�?���?�?
        // 引用与回收站两块抽成方法（`render_scratchpad_refs_and_trash`）：主�?图只留�?配顺序�?
        let body = self.render_scratchpad_refs_and_trash(cx);

        // 引用 / 回收站限高可滚，保证草�?树�?终有�?��高度�
        panel = panel.child(
            div()
                .v_flex()
                .w_full()
                .max_h(rems(ui::SCRATCHPAD_GROUP_MAX_HEIGHT))
                .overflow_y_scrollbar()
                .child(body),
        );

        // �?�? 撤销�?�?�?
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
                        // K9：撤�?动作按钮（自绘链�?�?gpui-kit `Button` ghost）�
                        Button::new("sp-undo")
                            .ghost()
                            .small()
                            .label("撤销")
                            .on_click(undo_click),
                    ),
            );
        }

        // �?�? 底部状�??�?�?
        panel = panel.child(
            div()
                .w_full()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(muted)
                .child(format!(
                    "{}{file_count} 个文件 · {folder_count} 个文件夹 · {} 项引用 · {} 项回收站 · 排序 {}",
                    if loading { "加载中… · " } else { "" },
                    external_refs.len(),
                    trash.len(),
                    scratchpad_sort_label(sort, sort_desc)
                )),
        );

        panel
            .key_context("scratchpad")
            .track_focus(&self.focus_handle)
            // 拖放残留兜底（工程�?�，用户实测）：`on_file_drop_exit` �?��元素�?            // hover 时派发（gpui-pre `div.rs` �?`is_hovered_ignoring_last_input`
            // 门控），鼠标拖出窗口�?hover 先丢，Exited 不再送达。鼠标�?�?面板
            // （移到窗口其它区域或移出窗口）都会触�?`MouseExited`，在这里统一复位�?
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
            // M5 Phase C：OS 文件拖入面板 �?导入。平台把 `FileDropEvent::Entered` �?��
            // `active_drag(ExternalPaths)` + 鼠标移动，`Submit` �?�� mouse-up �?drop�?
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
                        // 记录�?��时刻：`on_file_drop_exit` 在鼠标拖出窗口时
                        // �?���?hover 先丢而不派发（�?字�?注释），�??�??兜底�?
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
    }
}

impl ScratchpadView {
    /// 分组标�?行（「草�?(N)」�?��?部引�?(N)」共�?��：紧凑�?�?+ 弱文�?+ 计数�
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

    /// 底部固定区的「�?部引�?+ 回收站�?�两块（不随草�?树滚�?���?    ///
    /// 为什么单�?��方法：`render_scratchpad` �?��板唯�?主�?图，接近千�?—�?�把与�?�数�?���?���?    /// 无关的两块搬出来，主视图�?��装配顺序（�?为不变：同一元素树，�?��了落点）�
    fn render_scratchpad_refs_and_trash(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let primary = theme.colors.primary;
        let hover_bg = theme.colors.list_hover;
        let ref_color = theme.colors.info;
        let info = theme.colors.info;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();
        let (external_refs, trash, trash_expanded, edit) = {
            let view = self.scratchpad.borrow();
            (
                view.external_refs.clone(),
                view.trash.clone(),
                view.trash_expanded,
                view.edit.clone(),
            )
        };

        let mut body = div().v_flex().w_full().gap_1().px_1().pb_1();
        // �?�? 外部引用（原�?415-424 acc-item「引用�?�；2026-10-11 对齐：常驻可折叠
        // Accordion + 胶囊 badge；改�?/ 打开 / 移除 / 重新引用操作保留）─�?
        let mut ref_rows = div().v_flex().w_full().gap_1();
        for r in &external_refs {
                // �?��用�?在改�?�?行内输入�
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
                // 失效引用提供「重新引用�?�（选择新路径后�?���?��）�?
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
                            div()
                                .id(format!("sp-ref-open-{}", r.alias))
                                .w(rems(1.125))
                                .h_flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(muted)
                                .hover(move |s| s.bg(hover_bg))
                                .child("↗")
                                .on_click(open_ref),
                        )
                        .child(
                            div()
                                .id(format!("sp-ref-ren-{}", r.alias))
                                .w(rems(1.125))
                                .h_flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(muted)
                                .hover(move |s| s.bg(hover_bg))
                                .child("✎")
                                .on_click(rename_ref),
                        )
                        .when(!r.exists, |this| {
                            this.child(
                                div()
                                    .id(format!("sp-ref-relink-{}", r.alias))
                                    .w(rems(1.125))
                                    .h_flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .text_xs()
                                    .text_color(info)
                                    .hover(move |s| s.bg(hover_bg))
                                .child("⟲")
                                    .on_click(relink),
                            )
                        })
                        .child(
                            div()
                                .id(format!("sp-ref-{}", r.alias))
                                .w(rems(1.125))
                                .h_flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .text_xs()
                                .text_color(muted)
                                .hover(move |s| s.bg(hover_bg))
                                .child("✕")
                                .on_click(remove),
                        ),
                );
            }
        // 引用区块 = 常驻 Accordion（原�?415-424：acc-item「引用�?? badge；空列表也显示头）�?
        let ref_count = external_refs.len();
        let ref_badge = div()
            .flex_none()
            .px_1()
            .rounded_full()
            .text_xs()
            .text_color(muted)
            .bg(hover_bg)
            .child(ref_count.to_string());
        body = body.child(
            Accordion::new("sp-acc-refs")
                .multiple(false)
                .bordered(true)
                .with_size(KitSize::XSmall)
                .item(move |item| {
                    item.open(true)
                        .title(
                            div()
                                .debug_selector(|| "sp-refs-head".into())
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .child("引用")
                                .child(div().flex_1())
                                .child(ref_badge),
                        )
                        .content_style(StyleRefinement::default().px_1().pb_1())
                        .child(ref_rows)
                }),
        );

        // �?�? 回收站（原型 425-432 acc-item「垃圾�?」；2026-10-11 对齐：可折叠 Accordion +
        // 胶囊 badge + 标�?右侧清空按钮；还原�?保留）─�?
        {
            // 回收站�?（还原）—�?�Accordion 面板内�?；`trash_expanded` �?��初�?展开态，
            // 折叠状�?�由 Accordion �??理（`on_toggle_click` 回写字�?保状态一致）�
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
                                div()
                                    .id(format!("sp-trash-{}", t.manifest.id))
                                    .cursor_pointer()
                                    .text_xs()
                                    .text_color(primary)
                                    .child("还原")
                                    .on_click(restore),
                            ),
                    );
            }
            // 清空按钮（标题右侧；原型 431 acc-foot「清空垃圾�?」收敛到头内右侧�?            // 2026-10-11 对齐：垃圾桶非空才显示）�
            let empty_btn = {
                let entity = entity.clone();
                if trash.is_empty() {
                    None
                } else {
                    Some(
                        div()
                            .id("sp-trash-empty")
                            .cursor_pointer()
                            .text_xs()
                            .text_color(primary)
                            .child("清空")
                            .on_click(move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                                entity.update(app, |this, cx| this.empty_scratchpad_trash(cx));
                            }),
                    )
                }
            };
            let trash_badge = div()
                .flex_none()
                .px_1()
                .rounded_full()
                .text_xs()
                .text_color(muted)
                .bg(hover_bg)
                .child(trash.len().to_string());
            body = body.child(
                Accordion::new("sp-acc-trash")
                    .multiple(false)
                    .bordered(true)
                    .with_size(KitSize::XSmall)
                    .item(move |item| {
                        item.open(trash_expanded)
                            .title(
                                div()
                                    .debug_selector(|| "sp-trash-head".into())
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .child("回收站")
                                    .child(div().flex_1())
                                    .child(trash_badge)
                                    .when_some(empty_btn, |this, btn| this.child(btn)),
                            )
                            .content_style(StyleRefinement::default().px_1().pb_1())
                            .child(trash_rows)
                    })
                    // 折叠状�?�回�?`trash_expanded`（保持面板状态与组件�?致；�
                    // 折叠状态回写 trash_expanded（保持面板状态与组件一致；单 item 索引 0）。
                    .on_toggle_click({
                        let view = view_handle.clone();
                        let entity = entity.clone();
                        move |open: &[usize], _: &mut gpui_kit::Window, app: &mut App| {
                            view.borrow_mut().trash_expanded = open.contains(&0);
                            entity.update(app, |_, cx| cx.notify());
                        }
                    }),
            );
        }
        body
    }
}

impl ScratchpadView {
    /// 面板头与工具栏：新建 / 新建文件�?/ 导入 / 引用 / 排序 / 刷新 + 选中时的�?��板�?�?    /// 以及 C-4 的冲突条（同�?份草稿在编辑器里有未保存�?��、�?盘上又�?外部改了）�??    ///
    /// 为什么单�?��方法：`render_scratchpad` �?��板唯�?主�?图�?��?�把「一行按�??�么摆�?�这�?    /// �?部细节搬出来，主视图�?��装配顺序（�?为不变：同一元素树，�?��了落点）�
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

        // �?�? 工具栏（新建文件 / 新建文件�?/ 刷新）─�?
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
        // 内�?搜索入口（VSCode 对齐�?026-10-11 用户点名）：进入搜索态�?��?�侧栏整体切�?        // 为搜索面板（�??�工具栏/�?���?底部区隐藏）；与�??��?图�?�搜文件」输入�?区分�
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
                // 引用 = �??�?��、不复制，因�?*文件或目�?*均可（区�?��导入）�?
                let receiver = app.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: true,
                    multiple: false,
                    prompt: Some("选择要引用的文件或目录".into()),
                });
                window
                    .spawn(app, async move |cx| {
                        if let Ok(Ok(Some(paths))) = receiver.await {
                            if let Some(path) = paths.into_iter().next() {
                                let _ = cx.update(|window, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.start_scratchpad_edit(
                                            ScratchpadEdit::NewReference { path },
                                            window,
                                            cx,
                                        )
                                    });
                                });
                            }
                        }
                    })
                    .detach();
            }
        };

        // K9：工具栏图标按钮由自�?div 换成 gpui-kit `Button`（ghost 变体透明�?+
        // hover 出背�?��与原 24×24 �?��样式同尺寸；`small` = 24px �?`w_6 h_6` 对齐）�?
        let tool_btn = |id: &'static str,
                        glyph: &'static str,
                        handler: Box<
            dyn Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
        >| {
            Button::new(id)
                .ghost()
                .small()
                .label(glyph)
                .on_click(move |ev, window, app| handler(ev, window, app))
        };
        // 图标版（2026-10-11 对齐）：内�?搜索入口与普通�?图文件搜索�?统一�?        // `IconName::Search`（用户�?「两处图标建�?��分，没有就可用同�?图标」→ 统一 SVG 图标）�?
        let tool_icon_btn = |id: &'static str,
                             icon: IconName,
                             handler: Box<
            dyn Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
        >| {
            Button::new(id)
                .ghost()
                .small()
                .icon(icon)
                .on_click(move |ev, window, app| handler(ev, window, app))
        };

        let mut toolbar = div().v_flex().w_full().gap_1().px_1p5().py_1();
        // �?�? �?更�?设置�?026-10-11 原型 644-658 对齐 + 持久化接线）：菜单勾选显�?隐藏
        // 各入�?+ �?右位�?���?+ 重置为默认布局；配�?���?`settings.scratchpad.toolbar`
        // （空�?= 全部显示 + 默�?位置�?5 �?2）�?�状态快照在渲染时捕获进�?��：菜单每�?        // 打开重建�?��，点击后 notify 重渲染刷新快照，因�? checked/位置初�?��?终与�?近渲染一致�?
        let (tb_cfg, more_entity) = {
            let view = self.scratchpad.borrow();
            (view.toolbar_cfg.clone(), entity.clone())
        };
        // 菜单�?��捕获外部 clone（闭包体�?`clone()` �
// 菜单闭包需捕获外部 clone（闭包体内的 `clone()` 与 move 捕获整个 tb_cfg。
        // 与下方过滤判断的借用冲突）
        let menu_cfg = tb_cfg.clone();
        let more_btn = DropdownButton::new("sp-more")
            .button(
                Button::new("sp-more-trigger")
                    .ghost()
                    .small()
                    .label("更多设置")
                    .tooltip("更多设置（显示/隐藏工具栏按钮 + 左/右位置）"),
            )
            .dropdown_menu(move |menu, _, _| {
                let entity = more_entity.clone();
                let cfg = menu_cfg.clone();
                let mut menu = menu;
                menu = menu.item(PopupMenuItem::label("工具栏按钮"));
                for key in ToolbarButtonKey::ALL {
                    let entity = entity.clone();
                    let current = cfg.get(&key).cloned().unwrap_or_default();
                    let checked = current.visible;
                    // �?��存在（用户没改过）→ 按钮默�?侧；存在 �?落盘 side（非 right 兜底左）�
                    let cur_side = cfg
                        .get(&key)
                        .map(|c| if c.side == "right" { "right" } else { "left" })
                        .unwrap_or_else(|| key.default_side_text());
                    // 行点�?= 切换显示/隐藏（保留既有交互）；�?内�?�左」�?�右」按�?                    // `app.stop_propagation()` 后只切位�?��不触发�?的显�?隐藏�
                    let row_entity = entity.clone();
                    let row = PopupMenuItem::element(move |_window, app| {
                        let entity = entity.clone();
                        let key = key;
                        let cur_side = cur_side;
                        let side_btn = |target: &'static str, active: bool| {
                            let entity = entity.clone();
                            let key = key;
                            let target = target;
                            Button::new(format!("sp-side-{target}-{}", key.as_str()))
                                .ghost()
                                .small()
                                .label(target)
                                .toggled(active)
                                .on_click(move |_, _, app| {
                                    app.stop_propagation();
                                    entity.update(app, |this, cx| {
                                        let mut v = this.scratchpad.borrow_mut();
                                        let cfg = v.toolbar_cfg.entry(key).or_insert_with(|| {
                                            ToolbarButtonCfg {
                                                visible: true,
                                                side: key.default_side_text().to_string(),
                                            }
                                        });
                                        cfg.side = target.to_string();
                                        drop(v);
                                        this.persist_toolbar_cfg(cx);
                                        cx.notify();
                                    });
                                })
                        };
                        div()
                            .h_flex()
                            .w_full()
                            .gap_1()
                            .items_center()
                            .child(Label::new(key.label()))
                            .child(div().flex_1())
                            .child(side_btn("left", cur_side == "left"))
                            .child(side_btn("right", cur_side == "right"))
                    })
                    .checked(checked)
                    .on_click(move |_, _, app| {
                        row_entity.update(app, |this, cx| {
                            let mut v = this.scratchpad.borrow_mut();
                            let cfg = v.toolbar_cfg.entry(key).or_insert_with(|| {
                                ToolbarButtonCfg {
                                    visible: true,
                                    side: key.default_side_text().to_string(),
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

        // �??�?+ �?在侧（缺�?= 显示 + 默�?侧；side �?right 兜底左）�
        let tb_visible = |key: ToolbarButtonKey| tb_cfg.get(&key).map(|c| c.visible).unwrap_or(true);
        let tb_side = |key: ToolbarButtonKey| {
            tb_cfg
                .get(&key)
                .map(|c| if c.side == "right" { "right" } else { "left" })
                .unwrap_or_else(|| key.default_side_text())
        };
        // 按钮按�?�可�?+ �?在侧」归组；左右分组�?debug_selector（元素级对齐测试锚点�?        // �?`sp-res-file-text-*` 同�?先例）：空组宽度�?0，可像素级断�?「按�??隐藏」�?
        let mut left = div()
            .h_flex()
            .items_center()
            .gap_1()
            .debug_selector(|| "sp-toolbar-left".into());
        let mut right = div()
            .h_flex()
            .items_center()
            .gap_1()
            .debug_selector(|| "sp-toolbar-right".into());
        let buttons = [
            (
                ToolbarButtonKey::Search,
                tool_icon_btn("sp-content-search", IconName::Search, Box::new(open_search_panel)),
            ),
            (
                ToolbarButtonKey::NewFile,
                tool_btn("sp-new-file", "＋", Box::new(start_new_file)),
            ),
            (
                ToolbarButtonKey::NewFolder,
                tool_btn("sp-new-folder", "🗀", Box::new(start_new_folder)),
            ),
            (
                ToolbarButtonKey::Import,
                tool_btn("sp-import", "⬇", Box::new(import_files)),
            ),
            (
                ToolbarButtonKey::Reference,
                tool_btn("sp-add-ref", "🔗", Box::new(add_reference)),
            ),
            (ToolbarButtonKey::Sort, tool_btn("sp-sort", "⇅", Box::new(cycle_sort))),
            (
                ToolbarButtonKey::Refresh,
                tool_btn("sp-refresh", "↻", Box::new(refresh)),
            ),
        ];
        for (key, btn) in buttons {
            if !tb_visible(key) {
                continue;
            }
            if tb_side(key) == "right" {
                right = right.child(btn);
            } else {
                left = left.child(btn);
            }
        }
        toolbar = toolbar.child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .child(left)
                .child(div().flex_1())
                .child(right)
                .child(more_btn),
        );


        // �?�? 冲突条（C-4）：同一份草稿在编辑器里有未保存�?��，�?盘上又�?外部改了 �?�?
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
    /// 搜索行：文件名过�?/ 内�?搜索的模�?chip + 输入�?+ 内�?模式下的 `.*` �?`Aa`�?    ///
    /// 搜索状�?�在点击回调里改（`view.search_mode` 等），这里只做投影与渲染�?    /// 搜索态完整面板（2026-10-11 VSCode 对齐；由�??��?图工具栏 �?进入，← 返回�?出）�?    /// 顶部工具栏（�?/ 刷新 / 清除 / 新建编辑器查�?/ 展开折叠明细）→ 垂直表单
    /// （搜�?/ 替换 / 包含 / 排除）→ 统�?�?�?搜索结果树�?
    fn render_scratchpad_search_pane(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let hover_bg = theme.colors.list_hover;
        let selected_bg = theme.colors.list_active;
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();

        // �?�? 顶部工具栏（VSCode 搜索面板形�?�；原型 437-445）─�?
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
        let clear_results = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
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
        // �?展开/折叠下方匹配明细：点击切�??�全部收�?�?全部展开」�?
        let toggle_details = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                let mut v = view.borrow_mut();
                if v.search_expanded.is_empty() {
                    // 全部收起：按当前结果收集�?�� / 文件 key�
                    let mut keys: Vec<String> = Vec::new();
                    let mut files_seen: Vec<&String> = Vec::new();
                    if let Some(sv) = &v.search_view {
                        for hit in &sv.hits {
                            let file = &hit.file;
                            if !files_seen.contains(&file) {
                                let dir = std::path::Path::new(file)
                                    .parent()
                                    .map(|p| p.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                keys.push(format!("d:{dir}"));
                                keys.push(format!("f:{file}"));
                                files_seen.push(file);
                            }
                        }
                    }
                    v.search_expanded = keys.into_iter().collect();
                } else {
                    v.search_expanded.clear();
                }
                drop(v);
                entity.update(app, |_, cx| cx.notify());
            }
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
                    .tooltip("清除结果")
                    .on_click(clear_results),
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
                // 原型 445 sp-details：文�??�▸」（2026-10-11 对齐；展�?/折叠下方匹配明细）�
                Button::new("sp-search-details")
                    .ghost()
                    .small()
                    .label("▸")
                    .tooltip("展开/折叠下方匹配明细")
                    .on_click(toggle_details),
            );

        // �?�? 搜索（内容搜�?���??��?图文件搜索入口已�?��为普通工具栏下方的过滤�?）─�?
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
        // �?�? 搜索行（VSCode 式，2026-10-11 用户点名）：�?�?Aa/ab/.* 嵌在输入框内右侧 �?�?
        // 运�?�?��：输入�? Enter 触发（`ensure_scratchpad_inputs` �?PressEnter 订阅）；
        // 不再有模�?chip / 运�?按钮（原�?449-460，用户�?决确认删除）�
        let mut search_row = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_1p5()
            .pb_1();
        if let Some(input) = self.scratchpad.borrow().search_input.clone() {
            let search_word = self.scratchpad.borrow().search_word;
            let word_btn = div()
                .id("sp-word")
                .h_flex()
                .items_center()
                .justify_center()
                .px_1()
                .h_5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(if search_word { fg } else { muted })
                .when(search_word, |this| this.bg(selected_bg))
                .when(!search_word, move |s| s.hover(move |s| s.bg(hover_bg)))
                .child("ab")
                .tooltip(|window, cx| {
                    Tooltip::element(move |_, _| {
                        div().text_xs().child("全字匹配（ab 开关：仅匹配完整单词）")
                    })
                    .build(window, cx)
                })
                .on_click(toggle_word);
            let reg_btn = div()
                .id("sp-regex")
                .h_flex()
                .items_center()
                .justify_center()
                .px_1()
                .h_5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(if search_regex { fg } else { muted })
                .when(search_regex, |this| this.bg(selected_bg))
                .when(!search_regex, move |s| s.hover(move |s| s.bg(hover_bg)))
                .child(".*")
                .tooltip(|window, cx| {
                    Tooltip::element(move |_, _| {
                                div().text_xs().child("使用正则表达式（如 \\d+ 匹配数字；关掉则按纯文本匹配）")
                    })
                    .build(window, cx)
                })
                .on_click(toggle_regex);
            let case_btn = div()
                .id("sp-case")
                .h_flex()
                .items_center()
                .justify_center()
                .px_1()
                .h_5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(if search_case { fg } else { muted })
                .when(search_case, |this| this.bg(selected_bg))
                .when(!search_case, move |s| s.hover(move |s| s.bg(hover_bg)))
                .child(IconName::CaseSensitive)
                .tooltip(|window, cx| {
                    Tooltip::element(move |_, _| {
                        div().text_xs().child("区分大小写（Aa 开关）")
                    })
                    .build(window, cx)
                })
                .on_click(toggle_case);
            search_row = search_row.child(
                div().flex_1().min_w_0().child(
                    Input::new(&input)
                        .w_full()
                        .suffix(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_0p5()
                                // 顺序对齐原型 450-452：Aa / ab / .*�?026-10-11 全面对齐�?                                // 与中�?es-bar �?致）�?                                .child(case_btn)
                                .child(word_btn)
                                .child(reg_btn),
                        ),
                ),
            );
        }

        // �?�? VSCode 搜索面板形�?�（2026-10-10 对齐）：搜索框下�?= 替换�?+ 高级选项 �?�?
        // 替换输入/包含/排除输入�?`ensure_scratchpad_inputs` 懒建；搜索�?�内恒为内�?搜索�
        let mut search_block = div().v_flex().w_full().gap_1().px_1p5().pb_1();
        search_block = search_block.child(toolbar).child(search_row);
        // 替换行：`[输入(placeholder=替换, 内嵌 AB)] [�?全部替换] [�?高级选项�?关]`
        // �?026-10-11 用户点名：搜�?替换�?��入�?的提示�?明；AB=保留大小写开关嵌框内�?            //   �?控制包含/排除文件 glob 的展�?与收起，默�?收起。）
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
                // AB �?关嵌在替换输入�?内右侧（VSCode 式；与搜索�?�?Aa/.* 同形态）�
                let ab_btn = div()
                    .id("sp-replace-case")
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .px_1()
                    .h_5()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(if replace_case { fg } else { muted })
                    .when(replace_case, |this| this.bg(selected_bg))
                    .when(!replace_case, move |s| s.hover(move |s| s.bg(hover_bg)))
                    .child("AB")
                    .tooltip(|window, cx| {
                        Tooltip::element(move |_, _| {
                            div().text_xs().child("保留大小写（已接入：替换时保持命中词的原有大小写格式，如全大写/首字母大写/全小写）")
                        })
                        .build(window, cx)
                    })
                    .on_click(toggle_replace_case);
                replace_row = replace_row.child(
                    div().flex_1().min_w_0().child(
                        Input::new(&input)
                            .w_full()
                            .suffix(div().h_flex().items_center().gap_0p5().child(ab_btn)),
                    ),
                );
            } else {
                replace_row = replace_row.child(div().flex_1());
            }
            replace_row = replace_row.child(
                // 用户点名�?026-10-10）：查找/替换这类动作按钮用图�?+ tooltip �?��提示
                // （gpui-kit 组件；图�?`IconName::Replace`，�?义与 VSCode 替换入口�?致）�
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

            // 高级选项：`包含的文�?/ 排除的文件`（VSCode glob；由替换行右�?�?展开收起）�??            // 2026-10-11 用户点名对齐原型�?*无�?�?label**（placeholder 即提示）�?            // 行尾操作按钮（包�?�?编辑 / 👁 查看；排�?�?设置）�??            // 输入框聚焦时显示 glob 样例 hint�
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
                        .child(div().flex_1().min_w_0().child(Input::new(&input).w_full()))
                        .child(
                            Button::new("sp-glob-edit")
                                .ghost()
                                .small()
                                .label("编辑")
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
                        .child(div().flex_1().min_w_0().child(Input::new(&input).w_full()))
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

                // glob 样例 hint�?026-10-11 用户点名：点击输入�?时给出包�?排除样例）：
                // 聚焦输入框或点操作按�?��显示，浅色小�?+ 斜杠分隔�
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

            // 统�?行（VSCode 搜索面板「N 文件�?�� M �?��果�?�）：最近一次内容搜索的去重文件数�??            // 命中行数与扫描文件数；`search_stats = None`（未搜索 / 已清空）时不显示�?            // 右侧「在编辑器中打开」链接（原型 484�?026-10-11 用户点名新�?）�?
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
                        .child(div().text_xs().text_color(muted).child(stats_text))
                        .child(div().flex_1())
                        .child(
                            Button::new("sp-stats-open")
                                .ghost()
                                .small()
                                .label("在编辑器中打开")
                                .on_click(open_in_editor),
                        ),
                );
            }

            // 侧栏搜索结果树（2026-10-11 用户点名：结构与草�?箱树�?致�?��?�目�?folder 图标�?            // 文件类型�?��、层级缩进�?�命�?��细缩进�?齐文件节点名）�?
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
    /// 草�?树：内联新建行的落点、�?高表、虚拟列表与空�?�（面板�?��滚动区）�?    ///
    /// 为什么单�?��方法：这�?��板的主体，`render_scratchpad` 里它�?长的�?段脚手架�?    /// �?��来之后主视图�?��「状态投�?+ 装配顺序」（行为不变：同�?元素树，�?��了落点）�
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

        // 内联新建的文�?文件夹�?定位：在�?��文件夹下�?行（�??�中文件夹则列表首�?）�?
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
            // 加载�?��显示空�?�引导，避免「草稿�?�?��的�?�闪现�?
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
                .cursor_pointer()
                .h_flex()
                .items_center()
                .gap_1()
                .child(tree::disclosure_slot().child(tree::disclosure_icon(drafts_expanded, muted)))
                .child("草稿")
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .px_1()
                        .rounded_full()
                        .text_xs()
                        .text_color(muted)
                        .bg(hover_bg)
                        .child(row_count.to_string()),
                )
                .on_click(toggle_drafts);
            drafts = drafts.child(
                div()
                    .v_flex()
                    .w_full()
                    .min_h_0()
                    .flex_1()
                    .child(drafts_head)
                    .child(Collapsible::new().open(drafts_expanded).h_full().content(
                        div().flex_1().min_h_0().w_full().child(list),
                    )),
            );
        }

        drafts
    }

    /// �?更�?设置持久化（2026-10-11 接线）：把面�?`toolbar_cfg` 全量写回
    /// `settings.scratchpad.toolbar`（与默�?�?致的�?��落盘，保持�?�空�?= 全部显示 +
    /// 默认位置」定义），再原子写盘。落盘失败不改本进程已生效。
    /// （`settings::save_settings` 会记录原因，见上面的 `last_save_error`）。
    pub(super) fn persist_toolbar_cfg(&self, cx: &mut Context<Self>) {
        let map = {
            let v = self.scratchpad.borrow();
            let mut out = std::collections::BTreeMap::new();
            for (key, cfg) in &v.toolbar_cfg {
                // 与默认完全一致（显示 + 默�?侧）的键不落盘：空表 = 全默认�?义�?
                if cfg.visible && cfg.side == key.default_side_text() {
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
}

