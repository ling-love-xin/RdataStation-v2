//! 草稿树的**行**：行高预算、单行渲染、内联编辑行、空态。
//!
//! 与面板外壳（`chrome.rs`）的边界：列表本体与「每一种行长什么样」在这里；
//! 面板头 / 工具栏 / 搜索行 / 引用 / 回收站 / 状态行在外壳。
//!
//! 行高是**硬契约**：虚拟列表（`v_virtual_list`）按给定高度定义布局、不回写实测值，
//! 所以 `scratchpad_row_height` 的估算与行渲染成对维护（估小会压行）。

use super::*;

impl ScratchpadRowCtx {
    /// 该显示行是否为内联新建行。
    pub(super) fn is_edit_row(&self, display: usize) -> bool {
        matches!(self.edit_insert, Some((i, _)) if i == display)
    }

    /// 内联新建行的缩进层级（仅当该行是新建行时有意义）。
    pub(super) fn edit_row_depth(&self, display: usize) -> usize {
        match self.edit_insert {
            Some((i, depth)) if i == display => depth,
            _ => 0,
        }
    }

    /// 显示序号 → 真实行序号（内联新建行不占真实行）。
    pub(super) fn real_index(&self, display: usize) -> Option<usize> {
        if self.is_edit_row(display) {
            return None;
        }
        match self.edit_insert {
            Some((i, _)) if display > i => Some(display - 1),
            _ => Some(display),
        }
    }
}

impl ScratchpadView {
    /// 渲染草稿树单行（选中态 / 行内操作 / 右键菜单 / 展开时触发懒加载）。
    ///
    /// 同时被普通渲染与虚拟列表闭包调用，行索引按 `ctx.rows` 全局序号。
    pub(super) fn scratchpad_row(
        &self,
        display: usize,
        ctx: &ScratchpadRowCtx,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // 内联新建行：插在目标文件夹首行位置（未选中文件夹时在模块根）。
        if ctx.is_edit_row(display) {
            return self
                .render_scratchpad_edit_row(ctx.edit_row_depth(display), cx)
                .into_any_element();
        }
        let Some(real) = ctx.real_index(display) else {
            return div().into_any_element();
        };
        let Some((depth, entry)) = ctx.rows.get(real) else {
            return div().into_any_element();
        };
        let colors = ctx.colors;
        let ScratchpadRowColors {
            hover_bg,
            selected_bg,
            fg,
            muted,
            folder_color,
            primary,
            info,
            success,
            active_border,
        } = colors;
        let key = entry.path.to_string_lossy().to_string();

        // 本行正在重命名 → 渲染内联输入。
        if let Some(ScratchpadEdit::Rename { path }) = &ctx.edit {
            if path == &key {
                return self
                    .render_scratchpad_edit_row(*depth, cx)
                    .into_any_element();
            }
        }

        let entity = cx.entity();
        let view_handle = self.scratchpad.clone();
        let is_folder = entry.kind == ScratchpadEntryKind::Folder;
        // 「查看统计」（M8 洞察）：判定问宿主——读取器口径在 `engine`，草稿箱不认识它
        // （也不该认识：依赖只向下）。判定不是真时菜单项根本不出现。
        let can_stats = !is_folder && self.host.can_view_stats(std::path::Path::new(&key));
        let stats_label = entry.name.clone();
        let is_selected = ctx.selected.contains(&key);
        let is_expanded = ctx.expanded.contains(&key);
        // 展开且尚未懒加载过子目录 → 触发加载。
        let needs_load = is_folder && !ctx.loaded.contains_key(&key) && entry.children.is_none();

        let click = {
            let view = view_handle.clone();
            let entity = entity.clone();
            let key = key.clone();
            let load_key = key.clone();
            let keys = ctx.keys.clone();
            let position = real;
            // 双击文件 = 在编辑器中打开（与 Enter 同一语义）。
            let open_host = self.host.clone();
            move |ev: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                let modifiers = ev.modifiers();
                if ev.click_count() >= 2 && !is_folder {
                    open_host.open_in_editor(std::path::PathBuf::from(&key));
                }
                let mut should_load = false;
                {
                    let mut v = view.borrow_mut();
                    if modifiers.shift {
                        if let Some(anchor) = v.anchor.clone() {
                            if let Some(a) = keys.iter().position(|k| k == &anchor) {
                                let (lo, hi) = if a <= position {
                                    (a, position)
                                } else {
                                    (position, a)
                                };
                                v.selected = keys[lo..=hi].iter().cloned().collect();
                            }
                        } else {
                            v.selected.clear();
                            v.selected.insert(key.clone());
                            v.anchor = Some(key.clone());
                        }
                    } else if modifiers.control {
                        if !v.selected.remove(&key) {
                            v.selected.insert(key.clone());
                        }
                        v.anchor = Some(key.clone());
                    } else {
                        v.selected.clear();
                        v.selected.insert(key.clone());
                        v.anchor = Some(key.clone());
                        if is_folder {
                            if v.expanded.contains(&key) {
                                v.expanded.remove(&key);
                            } else {
                                v.expanded.insert(key.clone());
                                should_load = needs_load;
                            }
                        }
                    }
                }
                // 点击即聚焦面板，使 Ctrl+A 等面板快捷键生效。
                entity.update(app, |this, cx| {
                    this.focus_handle.clone().focus(window, cx);
                    if should_load {
                        this.request_scratchpad_dir(load_key.clone(), cx);
                    } else {
                        cx.notify();
                    }
                });
            }
        };

        let icon_color = if is_folder {
            folder_color
        } else {
            scratchpad_icon_color(&entry.name, info, success, primary, muted)
        };

        let mut row = div()
            .id(format!("sp-row-{key}"))
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(ui::ROW_HEIGHT))
            .gap_1()
            .rounded_sm()
            .relative()
            .cursor_pointer()
            .when(is_selected, |this| this.bg(selected_bg))
            // 悬停只在未选中时生效（不得盖掉选中底色）——与导航树、`ui-design-spec` §4 同一口径。
            .when(!is_selected, move |s| s.hover(move |s| s.bg(hover_bg)))
            .on_click(click)
            // 选中左侧 2px 品牌色条（原型 §3）——共用原语，与导航 / 设置页同一套画法。
            .when(is_selected, |this| {
                this.child(tree::active_bar(active_border))
            })
            .child(tree::indent_spacer(ui::TREE_INDENT, *depth))
            .child(if is_folder {
                // 展开指示：槽宽固定（10px），载体与导航统一到图标（见 `tree::disclosure_slot`）。
                tree::disclosure_slot().child(tree::disclosure_icon(is_expanded, muted))
            } else {
                tree::disclosure_slot()
            })
            .child(div().w_2().h_2().flex_none().rounded_sm().bg(icon_color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(fg)
                    .overflow_hidden()
                    .child(entry.name.clone()),
            )
            // 脏点（原型 §2/§3：名字 · ● · 大小时间；只有文件画）
            .when(scratchpad_shows_dirty_dot(&ctx.dirty, entry), |this| {
                this.child(div().w_2().h_2().flex_none().rounded_full().bg(primary))
            })
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(scratchpad_meta_label(entry)),
            );

        // 选中行才显示操作（打开位置 / 重命名 / 删除）。
        if is_selected {
            let open_location = {
                let entity = entity.clone();
                let key = key.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.open_scratchpad_location(key.clone(), cx)
                    });
                }
            };
            let rename = {
                let entity = entity.clone();
                let key = key.clone();
                move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.start_scratchpad_edit(
                            ScratchpadEdit::Rename { path: key.clone() },
                            window,
                            cx,
                        )
                    });
                }
            };
            let delete = {
                let entity = entity.clone();
                let key = key.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.delete_scratchpad_entry(key.clone(), cx)
                    });
                }
            };
            row = row
                .child(
                    div()
                        .id(format!("sp-open-{key}"))
                        .w(rems(1.125))
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(muted)
                        .hover(move |s| s.bg(hover_bg))
                        .child("↗")
                        .on_click(open_location),
                )
                .child(
                    div()
                        .id(format!("sp-ren-{key}"))
                        .w(rems(1.125))
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(muted)
                        .hover(move |s| s.bg(hover_bg))
                        .child("✎")
                        .on_click(rename),
                )
                .child(
                    div()
                        .id(format!("sp-del-{key}"))
                        .w(rems(1.125))
                        .h_flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .text_xs()
                        .text_color(muted)
                        .hover(move |s| s.bg(hover_bg))
                        .child("✕")
                        .on_click(delete),
                );
        }

        // 拖到编辑区 = 插入文件内容（原型 §4.5）。载荷类型住在 `shared`（两侧都看得见），
        // 不靠特性 crate 互相依赖。
        if !is_folder {
            let label = entry.name.clone();
            row = row.on_drag(
                shared::InsertFileDrag {
                    label: label.clone(),
                    path: entry.path.clone(),
                },
                move |payload, _offset, _window, cx| {
                    let label = payload.label.clone();
                    cx.new(|_| ScratchpadDragGhost { label })
                },
            );
        }

        // 右键菜单（打开位置 / 重命名 / 剪切 / 复制 / 删除）。
        {
            let menu_entity = entity.clone();
            let menu_key = key.clone();
            // 文件夹没有「打开」（双击/Enter 的语义是展开）。
            let menu_open = entity.clone();
            let menu_open_key = key.clone();
            row.context_menu(move |menu, _window, _cx| {
                let open_doc_entity = menu_open.clone();
                let open_doc_key = menu_open_key.clone();
                let open_entity = menu_entity.clone();
                let open_key = menu_key.clone();
                let rename_entity = menu_entity.clone();
                let rename_key = menu_key.clone();
                let cut_entity = menu_entity.clone();
                let cut_key = menu_key.clone();
                let copy_entity = menu_entity.clone();
                let copy_key = menu_key.clone();
                let del_entity = menu_entity.clone();
                let del_key = menu_key.clone();
                let mut menu = menu;
                if !is_folder {
                    menu = menu.item(PopupMenuItem::new("打开").on_click(move |_, _, app| {
                        open_doc_entity.update(app, |this, cx| {
                            this.request_open_scratchpad_file(open_doc_key.clone(), cx)
                        });
                    }));
                    if can_stats {
                        // 只把路径与显示名交给宿主：取样来源（DuckDB 读取器 + 路径转义）在那边构造。
                        let stats_entity = menu_entity.clone();
                        let stats_key = menu_key.clone();
                        let stats_label = stats_label.clone();
                        menu =
                            menu.item(PopupMenuItem::new("查看统计").on_click(move |_, _, app| {
                                let entity = stats_entity.clone();
                                let key = stats_key.clone();
                                let label = stats_label.clone();
                                entity.update(app, |this, cx| {
                                    this.request_view_stats(key.clone(), label.clone(), cx)
                                });
                            }));
                    }
                }
                menu.item(PopupMenuItem::new("打开位置").on_click(move |_, _, app| {
                    open_entity.update(app, |this, cx| {
                        this.open_scratchpad_location(open_key.clone(), cx)
                    });
                }))
                .item(
                    PopupMenuItem::new("重命名").on_click(move |_, window, app| {
                        rename_entity.update(app, |this, cx| {
                            this.start_scratchpad_edit(
                                ScratchpadEdit::Rename {
                                    path: rename_key.clone(),
                                },
                                window,
                                cx,
                            )
                        });
                    }),
                )
                .separator()
                .item(PopupMenuItem::new("剪切").on_click(move |_, _, app| {
                    cut_entity.update(app, |this, cx| {
                        this.scratchpad.borrow_mut().selected =
                            std::iter::once(cut_key.clone()).collect();
                        this.set_scratchpad_clipboard(ScratchpadClipboardMode::Cut, cx);
                    });
                }))
                .item(PopupMenuItem::new("复制").on_click(move |_, _, app| {
                    copy_entity.update(app, |this, cx| {
                        this.scratchpad.borrow_mut().selected =
                            std::iter::once(copy_key.clone()).collect();
                        this.set_scratchpad_clipboard(ScratchpadClipboardMode::Copy, cx);
                    });
                }))
                .separator()
                .item(PopupMenuItem::new("删除").on_click(move |_, _, app| {
                    del_entity.update(app, |this, cx| {
                        this.delete_scratchpad_entry(del_key.clone(), cx)
                    });
                }))
            })
            .into_any_element()
        }
    }

    /// 草稿树行高：内联编辑行用控件高，其余用树行高（与虚拟列表 `item_sizes` 保持一致）。
    ///
    /// 行高预算走共用原语（`workbench_shell::tree::RowHeight`：基础高 + 附加块），
    /// 与导航同一套口径；换算成像素交给 `tree::row_sizes`。
    ///
    /// 内联「新建文件」行底下还有一行模板 chip：那一块必须算成附加块——虚拟列表
    /// 按给定高度累计 origin、**不实测回写**，估小会把后一行压上来。
    pub(super) fn scratchpad_row_height(ctx: &ScratchpadRowCtx, display: usize) -> tree::RowHeight {
        let controls_high = if ctx.is_edit_row(display) {
            true
        } else {
            let renaming = match (
                ctx.real_index(display).and_then(|i| ctx.keys.get(i)),
                &ctx.edit,
            ) {
                (Some(key), Some(ScratchpadEdit::Rename { path })) => path == key,
                _ => false,
            };
            renaming
        };
        let base = if controls_high {
            ui::CONTROL_HEIGHT_SM
        } else {
            ui::ROW_HEIGHT
        };
        // chip 只跟着「内联新建文件」行（重命名与新建文件夹都没有那一行）。
        let chips = ctx.is_edit_row(display) && matches!(ctx.edit, Some(ScratchpadEdit::NewFile));
        tree::RowHeight::new(base).add_if(chips, ui::SCRATCHPAD_ROW_CHIPS)
    }

    /// 内联编辑行（新建 / 重命名通用）。
    ///
    /// 新建文件时额外渲染一行模板 chip（原型 §4.1：自动补后缀 + 填充占位内容）。
    pub(super) fn render_scratchpad_edit_row(&self, depth: usize, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let primary = theme.colors.primary;
        let hover_bg = theme.colors.list_hover;
        // 选中底走列表角色（与导航 / 资源库 / Mock 同一口径；`sidebar_accent` 是侧栏容器角色）。
        let selected_bg = theme.colors.list_active;
        let fg = theme.colors.foreground;

        let Some(input) = self.scratchpad.borrow().name_input.clone() else {
            return div();
        };
        let entity = cx.entity();
        let edit = self.scratchpad.borrow().edit.clone();
        let new_template = self.scratchpad.borrow().new_template;

        let commit = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.commit_scratchpad_edit(window, cx));
            }
        };
        let cancel = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.cancel_scratchpad_edit(cx));
            }
        };

        let row = div()
            .h_flex()
            .items_center()
            .w_full()
            .h(rems(1.625))
            .gap_1()
            .px_1()
            .child(div().w(rems(depth as f32 * ui::TREE_INDENT)).flex_none())
            .child(div().w_2p5().flex_none())
            .child(div().flex_1().min_w_0().child(Input::new(&input).w_full()))
            .child(
                div()
                    .id("sp-edit-ok")
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .w_5()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(primary)
                    .child("✓")
                    .on_click(commit),
            )
            .child(
                div()
                    .id("sp-edit-cancel")
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .w_5()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(muted)
                    .child("✕")
                    .on_click(cancel),
            );

        // 仅新建文件时展示模板 chip 行。
        if !matches!(edit, Some(ScratchpadEdit::NewFile)) {
            return row;
        }
        let mut chips = div().h_flex().items_center().gap_1().w_full().pl_1();
        for template in ScratchpadTemplate::ALL {
            let on = template == new_template;
            let handler = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        this.set_scratchpad_template(template, window, cx)
                    });
                }
            };
            chips = chips.child(
                div()
                    .id(format!("sp-tpl-{}", template.label()))
                    .h_flex()
                    .items_center()
                    .justify_center()
                    .px_1()
                    .h_5()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(if on { fg } else { muted })
                    .when(on, |this| this.bg(selected_bg))
                    // 开关态 chip 与行选中同一规矩：悬停不盖当前位置（未开才给悬停底）。
                    .when(!on, move |s| s.hover(move |s| s.bg(hover_bg)))
                    .child(template.label())
                    .on_click(handler),
            );
        }
        div()
            .v_flex()
            .w_full()
            .gap_0p5()
            .py_0p5()
            .child(row)
            .child(chips)
    }

    /// 切换新建文件模板（并把已输入名字的后缀跟着换）。
    pub(super) fn set_scratchpad_template(
        &mut self,
        template: ScratchpadTemplate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.scratchpad.borrow_mut().new_template = template;
        if let Some(input) = self.scratchpad.borrow().name_input.clone() {
            let current = input.read(cx).value().trim().to_string();
            if !current.is_empty() {
                let next = scratchpad_apply_template_ext(&current, template);
                if next != current {
                    input.update(cx, |s, cx| s.set_value(next, window, cx));
                }
            }
        }
        cx.notify();
    }

    /// 草稿树空态（原型 §2.3）：大图标 + 标题 + 说明 + 「新建」「导入」双按钮。
    ///
    /// `filter` 非空表示是「搜索无结果」而不是「真的没有草稿」。
    pub(super) fn render_scratchpad_empty_state(
        &self,
        entity: &Entity<Self>,
        filter: &str,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let fg = theme.colors.foreground;
        let icon_color = theme.colors.border;
        let entity = entity.clone();

        if !filter.is_empty() {
            return div()
                .v_flex()
                .items_center()
                .w_full()
                .pt_6()
                .px_2()
                .text_xs()
                .text_color(muted)
                .child("没有匹配的文件");
        }

        let new_file = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.start_scratchpad_edit(ScratchpadEdit::NewFile, window, cx)
                });
            }
        };
        let new_folder = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| {
                    this.start_scratchpad_edit(ScratchpadEdit::NewFolder, window, cx)
                });
            }
        };
        let import = {
            let entity = entity.clone();
            move |_: &gpui_kit::ClickEvent, window: &mut gpui_kit::Window, app: &mut App| {
                entity.update(app, |this, cx| this.pick_scratchpad_imports(window, cx));
            }
        };

        div()
            .v_flex()
            .items_center()
            .w_full()
            .pt_8()
            .px_2()
            .gap_2()
            .child(
                div()
                    .text_color(icon_color)
                    .text_size(rems(ui::SCRATCHPAD_EMPTY_ICON_SIZE))
                    .child("🗒"),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg)
                    .child("草稿箱是空的"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("随手写点东西，或从外部导入文件"),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("sp-empty-new")
                            .small()
                            .primary()
                            .label("＋ 新建")
                            .on_click(new_file),
                    )
                    .child(
                        Button::new("sp-empty-folder")
                            .small()
                            .label("🗀 文件夹")
                            .on_click(new_folder),
                    )
                    .child(
                        Button::new("sp-empty-import")
                            .small()
                            .label("⬇ 导入")
                            .on_click(import),
                    ),
            )
    }
}
