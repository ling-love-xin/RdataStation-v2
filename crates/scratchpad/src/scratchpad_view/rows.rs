//! 草稿树的**行**：行高预算、单行渲染、内联编辑行、空态。
//!
//! 与面板外壳（`chrome.rs`）的边界：列表本体与「每一种行长什么样」在这里；
//! 面板头 / 工具栏 / 搜索行 / 引用 / 回收站 / 状态行在外壳。
//!
//! 行高是**硬契约**：虚拟列表（`v_virtual_list`）按给定高度定义布局、不回写实测值，
//! 所以 `scratchpad_row_height` 的估算与行渲染成对维护（估小会压行）。

use super::*;

use gpui_kit::component::Icon;
use content_search::view::hit_line;

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

        // 过滤态三要素（与 ctx 同源）：Content 模式 + 过滤非空时，单击文件行
        // 额外触发内容搜索——中央编辑区显示该次搜索的命中明细（用户点名 2026-10-10：
        // 对齐 VSCode 搜索面板，点击文件显示「行号+命中行」明细，而不是只打开整个文件）。
        let filter_active = !ctx.filter.is_empty();
        let filter_mode = ctx.mode;
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
                    // 过滤态（Content + 非空）单击文件：触发内容搜索 → 中央区命中明细。
                    if !is_folder && filter_active && filter_mode == ScratchpadSearchMode::Content {
                        this.run_scratchpad_content_search(cx);
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
            // 微标（用户点名：zed / VS Code 风格的类型**图标**，不是文本标签）：
            // 文件夹画文件夹图标；文件画扩展名对应的类型图标（sql→database、log→file-terminal…）。
            .child(
                if is_folder {
                    Icon::empty()
                        .path("icons/folder.svg")
                        .size(rems(ui::ICON_SIZE_MD))
                        .flex_none()
                        .text_color(folder_color)
                        .into_any_element()
                } else {
                    Icon::empty()
                        .path(scratchpad_type_icon(&entry.name))
                        .size(rems(ui::ICON_SIZE_MD))
                        .flex_none()
                        .text_color(icon_color)
                        .into_any_element()
                },
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(fg)
                    .overflow_hidden()
                    .debug_selector({
                        let n = entry.name.clone();
                        move || format!("sp-row-text-{n}")
                    })
                    .child(entry.name.clone()),
            )
            // 脏点（原型 §2/§3：名字 · ● · 大小时间；只有文件画）
            .when(scratchpad_shows_dirty_dot(&ctx.dirty, entry), |this| {
                this.child(div().w_2().h_2().flex_none().rounded_full().bg(primary))
            })
            // 命中数角标（用户点名 2026-10-10：过滤态树行显示命中数，对齐 VSCode 搜索面板
            // 左侧每文件命中数）：Content 模式 + 过滤非空 + 文件内容有命中行时才画
            // （文件名命中但内容零命中的文件不显示，避免误导性的「0」）。
            // 口径与搜索任务一致（逐行 is_match，头部 256KB）；当前规模同步读，目录极大时
            // 可在 ScratchpadView 层加内容命中缓存（按 query+mode+case+regex 键控）。
            .when(
                !ctx.filter.is_empty()
                    && ctx.mode == ScratchpadSearchMode::Content
                    && entry.kind == ScratchpadEntryKind::File,
                move |this| {
                    let count = scratchpad_content_hit_count(
                        &entry.path,
                        &ctx.filter,
                        ctx.regex,
                        ctx.case_sensitive,
                    );
                    if count == 0 {
                        this
                    } else {
                        this.child(tree::count_badge(count, primary, hover_bg))
                    }
                },
            )
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
                        .w(rems(ui::ROW_ACTION_SIZE))
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
                        .w(rems(ui::ROW_ACTION_SIZE))
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
                        .w(rems(ui::ROW_ACTION_SIZE))
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

        // 右键「标签…」的目标：绝对路径 → 模块内相对路径（entry_id 语义；目录不是打标对象）。
        let rel_for_tags = if !is_folder {
            self.host
                .project_root()
                .map(|root| {
                    ScratchpadStore::new(root)
                        .relative_path_of(std::path::Path::new(&key))
                        .unwrap_or_default()
                })
                .unwrap_or_default()
        } else {
            String::new()
        };

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
                // 「在文件管理器中显示」：文件与文件夹都可用（定位而不打开）。
                // 语义与「用系统应用打开」分离——前者是 `explorer /select`，
                // 后者是 `opener::open`（用户点名澄清过：两种都要保留）。
                let reveal_entity = menu_entity.clone();
                let reveal_key = menu_key.clone();
                let open_sys_entity = menu_entity.clone();
                let open_sys_key = menu_key.clone();
                menu = menu.item(
                    PopupMenuItem::new("在文件管理器中显示").on_click(move |_, _, app| {
                        reveal_entity.update(app, |this, cx| {
                            this.reveal_scratchpad_location(reveal_key.clone(), cx)
                        });
                    }),
                );
                if !is_folder {
                    // 「用系统应用打开」：只给文件（文件夹“打开”在资源管理器里就是定位）。
                    menu = menu.item(
                        PopupMenuItem::new("用系统应用打开").on_click(move |_, _, app| {
                            open_sys_entity.update(app, |this, cx| {
                                this.open_scratchpad_location(open_sys_key.clone(), cx)
                            });
                        }),
                    );
                }
                // 「属性」：文件与文件夹都可用（用户点名 2026-10-10：简单属性面板，
                // 树行 meta 只留大小；时间与「SQL 文件连接哪个数据库」都放这里）。
                let info_entity = menu_entity.clone();
                let info_key = menu_key.clone();
                menu = menu.item(PopupMenuItem::new("属性").on_click(move |_, _, app| {
                    info_entity.update(app, |this, cx| {
                        this.show_scratchpad_file_info(info_key.clone(), cx)
                    });
                }));
                // 「标签…」（统一规格 §7.3）：只给文件；打开标签编辑弹窗（勾选打标 / 新建）。
                if !is_folder {
                    let tag_entity = menu_entity.clone();
                    let tag_rel = rel_for_tags.clone();
                    menu = menu.item(
                        PopupMenuItem::new("标签…").on_click(move |_, window, app| {
                            if !tag_rel.is_empty() {
                                tag_entity.update(app, |this, cx| {
                                    this.open_tag_dialog(window, &tag_rel, cx)
                                });
                            }
                        }),
                    );
                }
                let menu = menu;
                menu.item(
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
        // 模板 chip 行已随「去模板」移除（用户点名）：新建文件统一空白起步，
        // 不再有附加块，虚拟列表行高回到基础高。
        tree::RowHeight::new(base)
    }

    /// 内联编辑行（新建 / 重命名通用；新建文件统一空白起步，无模板 chip 行）。
    pub(super) fn render_scratchpad_edit_row(&self, depth: usize, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let primary = theme.colors.primary;

        let Some(input) = self.scratchpad.borrow().name_input.clone() else {
            return div();
        };
        let entity = cx.entity();

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
            // 输入框与树行同字号（`xsmall` → `text_xs` 12px，行高 20px 落在 26px 编辑行内），
            // 边框保持 1px 细边框（`bordered` 默认 true：`border_1` + 主题 `input` 色）。
            .child(div().flex_1().min_w_0().child(Input::new(&input).xsmall().w_full()))
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

        // 模板 chip 行已随「去模板」移除（用户点名）：新建文件统一空白起步。
        // `ScratchpadTemplate::Blank` 保留在枚举里，供未来「分析文件」类型引导复用；
        // `set_scratchpad_template` / `commit_scratchpad_edit` 的模板应用逻辑不受影响。
        row
    }

    /// 切换新建文件模板（并把已输入名字的后缀跟着换）。
    /// 注：模板 chip 行已随「去模板」移除（用户点名），本方法与 `commit_scratchpad_edit`
    /// 的模板应用逻辑保留供未来「分析文件」类型引导复用（见上方 render 注释）。
    #[allow(dead_code)]
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
            // 原型 664-668：空态第二行说明（2026-10-11 对齐；措辞同步语义定义：
            // 根 = 项目下的 scratchpad/；导入 = 复制，引用 = 链接）。
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("草稿箱根 = 项目下的 scratchpad/；导入 = 复制，引用 = 链接"),
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

    /// 侧栏搜索结果树（2026-10-11 用户点名：结构与草稿箱树一致）。
    ///
    /// 分组：按命中文件的目录（`scratchpad\` 根 + 子目录），目录节点画 folder 图标；
    /// 文件节点画类型微标 + 命中数角标；命中明细缩进对齐文件节点名（无行号、带高亮）。
    /// 展开语义：`search_expanded` 存「收起」集合——不在集合 = 展开（初始全展开，与原型一致）；
    /// 点击目录 / 文件节点切换展开收起；hover 行尾图标操作：目录 ⇄ 全部替换 / 文件 ⇄ 单文件替换、✕ 收起。
    pub(super) fn render_search_results_tree(
        &self,
        search: &ScratchpadSearchView,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let hover_bg = theme.colors.list_hover;
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let folder_color = theme.colors.warning;
        let primary = theme.colors.primary;
        let info = theme.colors.info;
        let success = theme.colors.success;
        let match_bg = workbench_shell::product_tokens::get(cx).search_match_background(&theme);

        let collapsed = self.scratchpad.borrow().search_expanded.clone();
        let entity = cx.entity();

        // 文件 → 命中行；目录 → 文件（BTreeMap 保证顺序稳定；根目录 = 空串）。
        let mut by_file: std::collections::BTreeMap<String, Vec<&content_search::view::SearchHit>> =
            std::collections::BTreeMap::new();
        for hit in &search.hits {
            by_file.entry(hit.file.clone()).or_default().push(hit);
        }
        let mut groups: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for file in by_file.keys() {
            let dir = std::path::Path::new(file)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            groups.entry(dir).or_default().push(file.clone());
        }

        let mut tree = div().v_flex().w_full().gap_px().mt_1().mb_1();
        for (dir, files) in &groups {
            let dir_key = format!("d:{dir}");
            let dir_collapsed = collapsed.contains(&dir_key);
            let dir_name = if dir.is_empty() {
                "scratchpad\\".to_string()
            } else {
                format!("scratchpad\\{dir}\\")
            };
            // ── 目录行：disclosure + folder 图标 + 目录名 + hover 操作 ──
            let toggle_dir = {
                let entity = entity.clone();
                let k = dir_key.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| {
                        let mut v = this.scratchpad.borrow_mut();
                        if !v.search_expanded.remove(&k) {
                            v.search_expanded.insert(k.clone());
                        }
                        cx.notify();
                    });
                }
            };
            let replace_all = {
                let entity = entity.clone();
                move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                    entity.update(app, |this, cx| this.replace_scratchpad_all(cx));
                }
            };
            let mut dir_ops = div()
                .h_flex()
                .items_center()
                .gap_0p5()
                .flex_none()
                .invisible()
                .group_hover("sp-res-dir", |s| s.visible());
            dir_ops = dir_ops
                .child(
                    Button::new("sp-res-dir-replace")
                        .ghost()
                        .small()
                        .icon(IconName::Replace)
                        .tooltip("替换该目录下全部匹配")
                        .on_click(replace_all),
                )
                .child(
                    Button::new("sp-res-dir-collapse")
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("收起该目录")
                        .on_click(toggle_dir.clone()),
                );
            tree = tree.child(
                div()
                    .id(format!("sp-res-dir-{dir_key}"))
                    .group("sp-res-dir")
                    .h_flex()
                    .items_center()
                    .w_full()
                    .h(rems(ui::ROW_HEIGHT))
                    .gap_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(toggle_dir)
                    .child(tree::disclosure_slot().child(tree::disclosure_icon(
                        !dir_collapsed,
                        muted,
                    )))
                    .child(
                        Icon::empty()
                            .path("icons/folder.svg")
                            .size(rems(ui::ICON_SIZE_MD))
                            .flex_none()
                            .text_color(folder_color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(fg)
                            .overflow_hidden()
                            .child(dir_name),
                    )
                    .child(dir_ops),
            );

            if dir_collapsed {
                continue;
            }
            // ── 文件行 + 命中明细 ──
            for file in files {
                let file_hits = &by_file[file];
                let fname = std::path::Path::new(file)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| file.clone());

                let count = file_hits.len();
                let file_collapsed = collapsed.contains(file);
                let toggle_file = {
                    let entity = entity.clone();
                    let f = file.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| this.toggle_search_file_expanded(&f, cx));
                    }
                };
                let replace_file = {
                    let entity = entity.clone();
                    let f = file.clone();
                    move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                        entity.update(app, |this, cx| this.replace_scratchpad_file(&f, cx));
                    }
                };
                let mut file_ops = div()
                    .h_flex()
                    .items_center()
                    .gap_0p5()
                    .flex_none()
                    .invisible()
                    .group_hover("sp-res-file", |s| s.visible());
                file_ops = file_ops
                    .child(
                        Button::new("sp-res-file-replace")
                            .ghost()
                            .small()
                            .icon(IconName::Replace)
                            .tooltip("替换该文件内全部匹配")
                            .on_click(replace_file),
                    )
                    .child(
                        Button::new("sp-res-file-collapse")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .tooltip("收起命中明细")
                            .on_click(toggle_file.clone()),
                    );
                tree = tree.child(
                    div()
                        .id(format!("sp-res-file-{file}"))
                        .group("sp-res-file")
                        .h_flex()
                        .items_center()
                        .w_full()
                        .h(rems(ui::ROW_HEIGHT))
                        .gap_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(toggle_file)
                        // 缩进层级与草稿箱树文件节点一致（TREE_INDENT × 1）。
                        .child(tree::indent_spacer(ui::TREE_INDENT, 1))
                        .child(tree::disclosure_slot().child(tree::disclosure_icon(
                            !file_collapsed,
                            muted,
                        )))
                        // 文件类型微标（与草稿箱树同一图标与取色）。
                        .child(
                            Icon::empty()
                                .path(scratchpad_type_icon(&fname))
                                .size(rems(ui::ICON_SIZE_MD))
                                .flex_none()
                                .text_color(scratchpad_icon_color(
                                    &fname, info, success, primary, muted,
                                )),
                        )
                        .child(
                            div()
                                // 元素断言锚点：命中明细文本起点必须与此节点名对齐（见 tests.rs 对齐用例）。
                                // debug_selector 供 VisualTestContext::debug_bounds 像素级断言（.id() 不注册）。
                                .debug_selector(|| format!("sp-res-file-text-{file}"))
                                .id(format!("sp-res-file-text-{file}"))
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(fg)
                                .overflow_hidden()
                                .child(fname),
                        )
                        .child(tree::count_badge(count, primary, hover_bg))
                        .child(file_ops),
                );

                if file_collapsed {
                    continue;
                }
                // ── 命中明细：缩进对齐文件节点名（TREE_INDENT×1 + 槽+图标+间距补偿 2.125rem）、
                //    无行号、命中关键词高亮、点击跳转到编辑器对应行 ──
                for hit in file_hits {
                    // 命中明细独立折叠（2026-10-12：工具栏「展开/折叠明细」只作用于 h: 键）。
                    let hit_key = format!("h:{file}#{}", hit.line);
                    if collapsed.contains(&hit_key) {
                        continue;
                    }
                    let open_hit = {
                        let entity = entity.clone();
                        let target = ScratchpadSearchHitTarget {
                            file: file.clone(),
                            line: hit.line as u32,
                            span: hit.spans.first().copied(),
                        };
                        move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
                            // `target` 非 Copy，闭包每次调用 clone 一份（闭包本身保持 Fn）。
                            let target = target.clone();
                            entity.update(app, |this, _cx| this.host.open_search_hit(target));
                        }
                    };
                    tree = tree.child(
                        div()
                            .id(format!("sp-res-hit-{file}-{}", hit.line))
                            .h_flex()
                            .items_center()
                            .w_full()
                            .h(rems(ui::ROW_HEIGHT_COMPACT))
                            .gap_1()
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .on_click(open_hit)
                            // 命中明细：与文件节点行逐元素同构（indent + 槽 + 微标 Icon + gap），
                            // 使命中文本起点与文件节点名完全对齐（2026-10-11 用户点名对齐文件）。
                            .child(tree::indent_spacer(ui::TREE_INDENT, 1))
                            .child(tree::disclosure_slot())
                            // 与文件行同一 Icon 组件渲染路径（空 div 占位实测不占宽度，
                            // 导致命中行靠左 16px；改用同款微标保证像素级对齐）。
                            // 命中明细对齐占位（2026-10-12 用户点名：明细行不应显示类型微标）。
                            .child(
                                div()
                                    .w(rems(ui::ICON_SIZE_MD))
                                    .h(rems(ui::ICON_SIZE_MD))
                                    .flex_none(),
                            )
                            // 命中文本：包裹 div 的 left 即文本起点（无 padding），元素断言锚点。
                            .child(
                                div()
                                    .debug_selector(|| {
                                        format!("sp-res-hit-text-{file}-{}", hit.line)
                                    })
                                    .id(format!("sp-res-hit-text-{file}-{}", hit.line))
                                    .min_w_0()
                                    .child(hit_line(&hit.content, &hit.spans, match_bg, fg)),
                            ),
                    );
                }
            }
        }
        tree
    }
}
