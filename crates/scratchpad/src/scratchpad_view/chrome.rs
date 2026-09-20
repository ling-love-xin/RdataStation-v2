//! 面板外壳：面板头 / 工具栏 / 搜索行 / 草稿树装配 / 引用 / 回收站 / 撤销栏 / 状态行。
//!
//! 这是 `render_scratchpad` 的落点——草稿箱唯一的主视图（虚拟列表的装配也在这里，
//! 行本身在 `rows.rs`）。

use super::*;

impl ScratchpadView {
    /// 草稿箱面板（M5）：根 = `{project}/scratchpad/`。
    ///
    /// 闭环：新建（内联）/重命名/删除→回收站+撤销栏/回收站恢复与清空/文件名过滤/外部引用移除。
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
        let (rows, error, external_refs, trash, sort, sort_desc, edit, undo, filter, loading) = {
            let view = self.scratchpad.borrow();
            let filter = view
                .search_input
                .as_ref()
                .map(|i| i.read(cx).value().to_lowercase().trim().to_string())
                .unwrap_or_default();
            let mut flat = Vec::new();
            flatten_scratchpad(
                &view.entries,
                0,
                &view.expanded,
                &view.children,
                view.sort,
                view.sort_desc,
                &filter,
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

        let toolbar = self.render_scratchpad_header(cx);

        let search_row = self.render_scratchpad_search(cx);

        let mut panel = div()
            .v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .child(toolbar)
            .child(search_row);

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
        let drafts = self.render_scratchpad_tree(rows, loading, &filter, window, cx);
        panel = panel.child(drafts);

        // ── 底部固定区（引用 / 回收站 / 撤销栏 / 状态；不随草稿树滚动）──
        // 引用与回收站两块抽成方法（`render_scratchpad_refs_and_trash`）：主视图只留装配顺序。
        let body = self.render_scratchpad_refs_and_trash(cx);

        // 引用 / 回收站限高可滚，保证草稿树始终有可用高度。
        panel = panel.child(
            div()
                .v_flex()
                .w_full()
                .max_h(rems(ui::SCRATCHPAD_GROUP_MAX_HEIGHT))
                .overflow_y_scrollbar()
                .child(body),
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
                        div()
                            .id("sp-undo")
                            .cursor_pointer()
                            .text_xs()
                            .text_color(primary)
                            .child("撤销")
                            .on_click(undo_click),
                    ),
            );
        }

        // ── 底部状态 ──
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
    /// 分组标题行（「草稿 (N)」「外部引用 (N)」共用）：紧凑行高 + 弱文字 + 计数。
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
        // ── 外部引用（链接：改名 / 打开 / 移除；不复制文件）──
        if !external_refs.is_empty() {
            body = body.child(self.scratchpad_group_header("外部引用", external_refs.len(), cx));
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
                body = body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .h(rems(ui::ROW_HEIGHT_COMPACT))
                        .px_1p5()
                        .child(div().w_2().h_2().flex_none().rounded_sm().bg(ref_color))
                        .child(
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
        }

        // ── 回收站（展开 / 还原 / 清空）──
        {
            let toggle_trash = {
                let view = view_handle.clone();
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    {
                        let mut v = view.borrow_mut();
                        v.trash_expanded = !v.trash_expanded;
                    }
                    entity.update(app, |_, cx| cx.notify());
                }
            };
            let mut header = div()
                .id("sp-trash-head")
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .h(rems(ui::ROW_HEIGHT_COMPACT))
                .px_1p5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .hover(move |s| s.bg(hover_bg))
                .on_click(toggle_trash)
                // 展开载体与树行统一（回收站头是同一类可折叠头）。
                .child(tree::disclosure_slot().child(tree::disclosure_icon(trash_expanded, muted)))
                .child("回收站")
                .child(div().flex_1())
                .child(trash.len().to_string());

            if !trash.is_empty() {
                let empty = {
                    let entity = entity.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| this.empty_scratchpad_trash(cx));
                    }
                };
                header = header.child(
                    div()
                        .id("sp-trash-empty")
                        .cursor_pointer()
                        .text_xs()
                        .text_color(primary)
                        .child("清空")
                        .on_click(empty),
                );
            }
            body = body.child(header);

            if trash_expanded {
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
                    body =
                        body.child(
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
            }
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
        let hover_bg = theme.colors.list_hover;
        let muted = theme.colors.muted_foreground;
        let warning = theme.colors.warning;
        let danger = theme.colors.danger;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();
        let (selected, has_clipboard, conflicts) = {
            let view = self.scratchpad.borrow();
            let conflicts: Vec<(String, std::path::PathBuf, bool)> = view
                .conflicts
                .iter()
                .map(|c| (c.relative.clone(), c.absolute.clone(), c.diff.is_some()))
                .collect();
            (view.selected.clone(), view.clipboard.is_some(), conflicts)
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
        let cut_selection = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.set_scratchpad_clipboard(ScratchpadClipboardMode::Cut, cx)
                });
            }
        };
        let copy_selection = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.set_scratchpad_clipboard(ScratchpadClipboardMode::Copy, cx)
                });
            }
        };
        let paste_clipboard = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.paste_scratchpad_clipboard(cx));
            }
        };
        let delete_selection = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.delete_scratchpad_selection(cx));
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
                // 引用 = 只记路径、不复制，因此**文件或目录**均可（区别于导入）。
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

        let tool_btn = |id: &'static str,
                        glyph: &'static str,
                        handler: Box<
            dyn Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut App) + 'static,
        >| {
            div()
                .id(id)
                .h_flex()
                .items_center()
                .justify_center()
                .w_6()
                .h_6()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(muted)
                .hover(move |s| s.bg(hover_bg))
                .on_click(move |ev, window, app| handler(ev, window, app))
                .child(glyph)
        };

        let has_selection = !selected.is_empty();
        let mut toolbar = div().v_flex().w_full().gap_1().px_1p5().py_1();
        toolbar = toolbar.child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .w_full()
                .child(tool_btn("sp-new-file", "＋", Box::new(start_new_file)))
                .child(tool_btn("sp-new-folder", "🗀", Box::new(start_new_folder)))
                .child(tool_btn("sp-import", "⬇", Box::new(import_files)))
                .child(tool_btn("sp-add-ref", "🔗", Box::new(add_reference)))
                .child(div().flex_1())
                .child(tool_btn("sp-sort", "⇅", Box::new(cycle_sort)))
                .child(tool_btn("sp-refresh", "↻", Box::new(refresh))),
        );
        if has_selection || has_clipboard {
            toolbar = toolbar.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .w_full()
                    .child(tool_btn("sp-cut", "✂", Box::new(cut_selection)))
                    .child(tool_btn("sp-copy", "⧉", Box::new(copy_selection)))
                    .child(tool_btn("sp-paste", "📋", Box::new(paste_clipboard)))
                    .child(tool_btn("sp-delete", "🗑", Box::new(delete_selection)))
                    .child(div().flex_1())
                    .child(div().id("sp-sel-count").text_xs().text_color(muted).child(
                        if has_selection {
                            format!("{} 项", selected.len())
                        } else {
                            "剪贴板".to_string()
                        },
                    )),
            );
        }

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
                                    .label(if diff_ready { "差异" } else { "计算中…" })
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
    fn render_scratchpad_search(&self, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let hover_bg = theme.colors.list_hover;
        let selected_bg = theme.colors.list_active;
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();

        // ── 搜索（文件名过滤 / 内容搜索）──
        let (search_mode, search_regex, search_case) = {
            let v = self.scratchpad.borrow();
            (v.search_mode, v.search_regex, v.search_case)
        };
        let toggle_mode = {
            let view = view_handle.clone();
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                {
                    let mut v = view.borrow_mut();
                    v.search_mode = if v.search_mode == ScratchpadSearchMode::Content {
                        ScratchpadSearchMode::Name
                    } else {
                        ScratchpadSearchMode::Content
                    };
                }
                entity.update(app, |_, cx| cx.notify());
            }
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
        let run_search = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.run_scratchpad_content_search(cx));
            }
        };

        let mode_label = if search_mode == ScratchpadSearchMode::Content {
            "内容"
        } else {
            "文件名"
        };
        let mode_on = search_mode == ScratchpadSearchMode::Content;

        let mut search_row = div()
            .h_flex()
            .items_center()
            .gap_1()
            .w_full()
            .px_1p5()
            .pb_1();
        search_row = search_row.child(
            div()
                .id("sp-mode")
                .h_flex()
                .items_center()
                .justify_center()
                .px_1()
                .h_5()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(if mode_on { fg } else { muted })
                .when(mode_on, |this| this.bg(selected_bg))
                // 开关 chip 同一条规矩：悬停不盖当前位置。
                .when(!mode_on, move |s| s.hover(move |s| s.bg(hover_bg)))
                .child(mode_label)
                .on_click(toggle_mode),
        );
        if let Some(input) = self.scratchpad.borrow().search_input.clone() {
            search_row =
                search_row.child(div().flex_1().min_w_0().child(Input::new(&input).w_full()));
        }
        if search_mode == ScratchpadSearchMode::Content {
            search_row = search_row
                .child(
                    div()
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
                        .on_click(toggle_regex),
                )
                .child(
                    div()
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
                        .child("Aa")
                        .on_click(toggle_case),
                )
                .child(
                    div()
                        .id("sp-run")
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .px_1()
                        .h_5()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(muted)
                        .hover(move |s| s.bg(hover_bg))
                        .child("⏎")
                        .on_click(run_search),
                );
        }

        search_row
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
                        .child("加载中…"),
                );
            } else {
                drafts = drafts.child(self.render_scratchpad_empty_state(&entity, &filter, cx));
            }
        } else {
            if row_count > 0 {
                drafts = drafts.child(self.scratchpad_group_header("草稿", row_count, cx));
            }
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
            drafts = drafts.child(div().flex_1().min_h_0().w_full().child(list));
        }

        drafts
    }
}
