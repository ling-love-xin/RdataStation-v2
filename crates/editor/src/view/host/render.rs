//! 编辑器主机面板 —— Render 渲染（impl Render + render_* 辅助）
//!
//! 自 host/mod.rs 拆分（2026-10-29 Step 5）：`impl EditorHostPanel` 尾部与 trait 实现按职责迁出，mod.rs 收敛为核心状态与调度。
//!
use super::*;

impl Render for EditorHostPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // #48 行号开关：设置 → 面板缓存 → 内核（render 是权威同步点，同 apply_left_mode）。
        // 只在值变化时调 `set_line_number`（内核值相同内部直接 return，不 notify → 无重绘循环）。
        let want_line_numbers = editor_show_line_numbers_or_default(cx);
        if self.show_line_numbers != want_line_numbers {
            self.show_line_numbers = want_line_numbers;
            self.editor.update(cx, |state, cx| {
                state.set_line_number(want_line_numbers, window, cx)
            });
        }
        // 【2026-10-27】折叠开关：设置 → shared.show_folds → 内核（render 权威同步点，
        // 同 #48 行号）。只开关在设置页（工具栏按钮已移除）；仅在值变化时重跑
        // `sync_fold`（install / set_folding），不 notify → 无重绘循环。
        let want_folds = editor_show_folds_or_default(cx);
        if self.shared.show_folds() != want_folds {
            self.shared.set_show_folds(want_folds);
            let shared = &self.shared;
            let document = self.document.clone();
            self.editor.update(cx, |state, cx| {
                sync_fold(shared, &document, state, window, cx);
            });
        }
        // 【2026-10-28】基于缩进的折叠开关：设置 → shared.indent_folding → 重新计算折叠候选。
        // 与 show_folds 同款同步逻辑。
        let want_indent_folding = editor_indent_folding_or_default(cx);
        if self.shared.indent_folding() != want_indent_folding {
            self.shared.set_indent_folding(want_indent_folding);
            let shared = &self.shared;
            let document = self.document.clone();
            self.editor.update(cx, |state, cx| {
                sync_fold(shared, &document, state, window, cx);
            });
        }
        // 【2026-10-28】自定义折叠关键字：设置 → shared → 重新计算折叠候选。
        let want_open_keywords = editor_fold_open_keywords_or_default(cx);
        let want_close_keywords = editor_fold_close_keywords_or_default(cx);
        if self.shared.fold_open_keywords() != want_open_keywords || self.shared.fold_close_keywords() != want_close_keywords {
            self.shared.set_fold_open_keywords(want_open_keywords);
            self.shared.set_fold_close_keywords(want_close_keywords);
            let shared = &self.shared;
            let document = self.document.clone();
            self.editor.update(cx, |state, cx| {
                sync_fold(shared, &document, state, window, cx);
            });
        }
        // #64 自动换行：设置 → 面板缓存 → 内核（render 权威同步点，同 #48 行号）。
        // 内核 `set_soft_wrap` 值相同内部直接 return，不 notify → 无重绘循环。
        let want_soft_wrap = editor_soft_wrap_or_default(cx);
        if self.soft_wrap != want_soft_wrap {
            self.soft_wrap = want_soft_wrap;
            self.editor.update(cx, |state, cx| {
                state.set_soft_wrap(want_soft_wrap, window, cx);
            });
        }
        // #65 自动闭合括号 / 智能缩进：与 soft_wrap 同款同步（render 权威同步点）。
        let want_auto_close = editor_auto_close_or_default(cx);
        if self.auto_close != want_auto_close {
            self.auto_close = want_auto_close;
            self.editor.update(cx, |state, cx| {
                state.set_auto_close(want_auto_close, window, cx);
            });
        }
        let want_smart_indent = editor_smart_indent_or_default(cx);
        if self.smart_indent != want_smart_indent {
            self.smart_indent = want_smart_indent;
            self.editor.update(cx, |state, cx| {
                state.set_smart_indent(want_smart_indent, window, cx);
            });
        }
        let (bg, primary, muted) = {
            let theme = cx.theme();
            (
                theme.colors.background,
                theme.colors.primary,
                theme.colors.muted_foreground,
            )
        };
        let _ = (primary, muted);

        let mut root = div()
            .id("editor-host-panel")
            .role(gpui::Role::Group)
            .v_flex()
            .size_full()
            .min_h_0()
            .bg(bg)
            // A10：键位都绑在这个 context 上（`crates/app` 注册）。
            // 焦点在内核里时内核的 `Input` context 更深、先被尝试；它没处理的键再冒泡到这里。
            // `track_focus` 不能省：没有它面板不在 dispatch path 上，快捷键就落不到动作（已踩）。
            .key_context("editor")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_format))
            .on_action(cx.listener(Self::on_toggle_comment))
            .on_action(cx.listener(Self::on_trigger_completion))
            .on_action(cx.listener(Self::on_execute_sql))
            .on_action(cx.listener(Self::on_execute_all))
            // 【#54-C5】函数手册：`Ctrl+Shift+M`（键位在 crates/app 注册，context 同为 editor）
            .on_action(cx.listener(Self::on_function_manual))
            // 【#54-C4】签名帮助：`Ctrl+Shift+Space`
            .on_action(cx.listener(Self::on_signature_help))
            // 【B14】网格里的 `Ctrl+C`：键位绑在 `view/results/grid.rs` 那一层的上下文上，
            // 所以只有焦点在网格里时才会走到这里（编辑区里 `Ctrl+C` 仍归内核的文本复制）
            .on_action(cx.listener(Self::on_copy_grid_selection))
            // 【撤销/重做】网格里的 `Ctrl+Z` / `Ctrl+Shift+Z`：同挂在网格上下文上；
            // 编辑框内 `Ctrl+Z` 由输入内核处理，只有焦点在网格（非编辑框）时才到这两个
            .on_action(cx.listener(Self::on_undo_result_edit))
            .on_action(cx.listener(Self::on_redo_result_edit))
            // 【书签】`Ctrl+Shift+B` 切换 / `F8` 下一 / `Shift+F8` 上一（键位在 crates/app 注册）
            .on_action(cx.listener(Self::on_toggle_bookmark))
            .on_action(cx.listener(Self::on_next_bookmark))
            .on_action(cx.listener(Self::on_prev_bookmark))
            // 【书签说明】右键「添加书签/说明…」（菜单项派发的动作；弹说明输入框）
            .on_action(cx.listener(Self::on_add_bookmark_note));
        // 工具栏（②）：模式指示器在这里，模式不再是只能从状态栏读到的短标签
        let mode = self
            .with_document(|doc| doc.mode())
            .unwrap_or(EditorMode::General);
        root = root.child(self.render_toolbar(mode, window, cx));
        // 提示卡（A13）：档位带来的限制要在界面上说清，而不是让用户自己撞上（“能编辑却改不了”）
        if let Some(notice) = self.tier_notice() {
            let theme = cx.theme();
            let warning = theme.colors.warning;
            let warning_fill = warning.opacity(0.18);
            root = root.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(warning)
                    .bg(warning_fill)
                    .border_b(ui::HAIRLINE)
                    .border_color(warning)
                    .child(SharedString::from(notice)),
            );
        }
        // 编辑区 + 结果区：v_resizable 上下分栏（DBeaver 口径，结果集跟编辑器 tab 走，
        // 不再挂全局底部 dock）。结果区有内容时才加第二个 panel（无内容时高度为 0 不占位）。
        // 2026-10-28 修复「结果区不可拉伸」：此前被换成固定高度 div（去 Resizable 修遮挡
        // 的中间态），现恢复组件库 `v_resizable / resizable_panel`（不手搓拖动条），
        // 拖到哪记到哪（on_resize 回写 `result_height`，结果区关闭重开仍沿用）。
        let result_has_content = self.result_panel.read(cx).has_content();
        let font_size = cx.theme().font_size;
        let edit_area = div()
            .flex_1()
            .min_h_0()
            .debug_selector(|| "editor-code-area".to_string())
            // 行号区域左侧对齐 Dock 面板左边：去掉左边 padding，只保留右边
            .pl_0()
            .pr_2()
            .on_drop({
                let entity = cx.entity();
                move |payload: &::shared::InsertFileDrag, window, app| {
                    entity.update(app, |this, cx| {
                        this.insert_file_contents(payload, window, cx)
                    });
                }
            })
            .child(self.render_editor_body(cx));

        // 编辑区 + 结果区：上下分栏（结果区有内容时才显示）
        let split = if result_has_content {
            let result_height = self.result_height.clone();
            v_resizable("editor-result-split")
                .child(
                    resizable_panel()
                        .child(edit_area),
                )
                .child(
                    resizable_panel()
                        .child(
                            div()
                                .size_full()
                                .min_h_0()
                                .child(self.result_panel.read(cx).render(cx)),
                        )
                        .size(font_size * self.result_height.get())
                        .size_range(
                            font_size * ui::RESULT_PANE_MIN_HEIGHT..font_size * ui::RESULT_MAX_HEIGHT,
                        ),
                )
                .on_resize(move |state, _window, _cx| {
                    // 拖到哪记到哪：上下分栏末位 panel 即结果区，换算回 rem 存面板字段
                    //（Pixels Div → f32；`px.0` 字段是 pub(crate) 不可外读）
                    if let Some(px) = state.read(_cx).sizes().last().copied() {
                        result_height.set(px / font_size);
                    }
                })
                .into_any_element()
        } else {
            edit_area.into_any_element()
        };
        root = root.child(
            div()
                .flex_1()
                .min_h_0()
                // 中间层必须是 flex_col：否则 edit_area 的 flex_1 在 block 布局里无效，
                // 无结果集时编辑器高度退化为内容高度（第 3 行被裁切、下面大片空白）。
                // 与 gpui-kit tab_group.rs / tab_panel.rs 注释里的 block 忽略 flex_grow 同因。
                .flex()
                .flex_col()
                .child(split),
        );
        // 编辑器自身状态栏（§2.5 定稿规格；需求③/去杂乱）：挂到编辑器 tab 底部。
        // 栏体由 `status_bar::render` 自绘（侧栏底 + 顶分隔线 + 固定高），
        // 文档级上下文（面包屑/方言/事务/微标/控件）都在其内，不再重复包一层。
        if let Some(status_bar) = render_editor_status_bar(self, &cx.entity(), cx) {
            root = root.child(status_bar);
        }
        root
    }
}



/// 编辑器自身状态栏（需求③ / 状态栏去杂乱）：把文档级上下文（连接·通道·事务 /
/// 环境族·未保存·执行中·消息 + 右段 行-列·编码·换行·缩进·模式·脏 + 编辑模式 /
/// 重设 / 事务 / 中断控件）迁到编辑器 tab 底部自己的状态栏，不再塞进工作台全局栏。
///
/// 由编辑器宿主自身的 `render` 挂载（把 `&self` 与自身实体句柄传入）：纯读构造
/// （不写 Shared / 不做 I/O），控件事件回调统一走 `panel.update`，不会重入正在
/// 渲染的宿主。返回 `None` = 没有活动文档 / 文档已关。
pub(crate) fn render_editor_status_bar(
    host: &EditorHostPanel,
    panel_entity: &Entity<EditorHostPanel>,
    cx: &App,
) -> Option<AnyElement> {
    if host.closed {
        return None;
    }
    let snapshot = host.status_snapshot(cx)?;
    let panel_entity = panel_entity.clone();

    // §2.5 右段微标补全：文档字节数 + 选区（字符 / 行数）+ 光标全文偏移从编辑器内核取（render 只读）。
    let (bytes, selected_chars, selected_rows, cursor_offset) = {
        let state = host.editor.read(cx);
        let value = state.value();
        let range = state.selected_range();
        let start = range.start.min(value.len());
        let end = range.end.min(value.len());
        let selected_chars = value[start..end].chars().count();
        let selected_rows = if start == end {
            0
        } else {
            value[start..end].chars().filter(|c| *c == '\n').count() + 1
        };
        // 光标全文偏移：内核给的是**字节**偏移，VSCode「Ch」微标是**字符**偏移——
        // 经 Rope `byte_to_char_idx` 换算（按 chunk 定位，大文件也是 O(log n)，不比 Ln/Col 贵）。
        let cursor_offset = {
            let bytes = state.cursor_offsets().first().copied().unwrap_or(0);
            state.text().byte_to_char_idx(bytes) + 1
        };
        (value.len(), selected_chars, selected_rows, cursor_offset)
    };

    // 状态栏输入（全部来自真实状态；无数据源的字段不放进结构体）
    let inputs = status_bar::StatusInputs {
        mode: snapshot.mode,
        dirty: snapshot.dirty,
        read_only: snapshot.read_only,
        statements: snapshot.statements,
        line: snapshot.line,
        column: snapshot.column,
        cursor_offset,
        cursor_count: snapshot.cursor_count,
        tab_size: snapshot.tab_size,
        hard_tabs: snapshot.hard_tabs,
        format: snapshot.format,
        bytes,
        selected_chars,
        selected_rows,
        message: snapshot.message.as_deref(),
        executing: snapshot.executing,
        elapsed: snapshot.elapsed,
        // 连接定位（短码·名称 / schema@db）已迁工具栏右区固定槽（render_conn_slots）；
        // 状态栏左段不再显示连接路径文本（labels().connection 保留给纯函数测试）
        connection: None,
        dialect: snapshot.dialect.as_deref(),
        // 执行方式由左段 channel 控件表达（可点切换）；文本段不再重复显示
        channel: None,
        env: snapshot.env.as_deref(),
        file_type: snapshot.file_type.as_deref(),
        lsp: snapshot.lsp.as_deref(),
        tx: snapshot.tx.as_deref(),
        session: snapshot.session.as_deref(),
        kernel: snapshot.kernel.as_deref(),
        units: snapshot.units.as_deref(),
    };

    // 执行方式（通道）由工具栏右区连接选择器旁的通道 pill 表达；状态栏不再占它
    let channel_ctrl = None;

    // ── 交互控件：编辑模式 / 缩进 / 换行 / 编码 / 不可见字符 / 事务 / 中断
    //（事件走 panel.update，不重入宿主）──
    // 编辑模式（行 / 列编辑；**循环按钮**——2026-10-27 去掉下拉框，点击在 行编辑/列编辑 间循环）
    let edit_mode_ctrl = {
        let mode = host.edit_mode;
        let row_entity = panel_entity.clone();
        let col_entity = panel_entity.clone();
        Button::new("editor-edit-mode")
            .ghost()
            .small()
            .debug_selector(|| "editor-edit-mode".to_string())
            .label(mode.short_label())
            .tooltip(match mode {
                EditMode::Row => "行编辑（点击切换为列编辑）",
                EditMode::Column => "列编辑（点击切换为行编辑）",
            })
            .on_click(move |_, window, app| {
                if mode == EditMode::Row {
                    col_entity.update(app, |panel, cx| {
                        panel.set_edit_mode(EditMode::Column, cx);
                    });
                } else {
                    // 切回行编辑：立即收拢多光标（走生产代码同一条 `enforce_edit_mode`）
                    row_entity.update(app, |panel, cx| {
                        panel.set_edit_mode(EditMode::Row, cx);
                        panel.enforce_edit_mode(window, cx);
                    });
                }
            })
            .into_any_element()
    };
    // 换行（LF / CRLF；2026-10-27 由下拉菜单改回**纯按钮循环切换**：点击 LF↔CRLF 翻转，
    // 无下拉箭头；宽度固定（以 CRLF 为基准）——切换时状态栏布局不乱动）
    let eol_ctrl = {
        let eol = snapshot.format.eol;
        let entity = panel_entity.clone();
        Button::new("editor-eol")
            .ghost()
            .small()
            .debug_selector(|| "editor-eol".to_string())
            .label(eol.label())
            .w(rems(ui::EOL_SLOT_WIDTH))
            .flex_none()
            .tooltip("换行符（点击切换 LF / CRLF）")
            .on_click(move |_, _window, app| {
                let next = match eol {
                    crate::encoding::LineEnding::Lf => crate::encoding::LineEnding::Crlf,
                    crate::encoding::LineEnding::Crlf => crate::encoding::LineEnding::Lf,
                };
                entity.update(app, |panel, cx| {
                    panel.set_eol(next, cx);
                });
            })
            .into_any_element()
    };
    // 编码（默认显示当前编码短名，点击弹出选择对话框；2026-10-27 拆出——见 encoding_dialog.rs）
    let encoding_ctrl = {
        let current = snapshot.format.encoding;
        let entity = panel_entity.clone();
        Button::new("editor-encoding")
            .ghost()
            .small()
            .debug_selector(|| "editor-encoding".to_string())
            .label(current.short_label())
            .tooltip("编码（点击选择）")
            .on_click(move |_, window, app| {
                let entity = entity.clone();
                crate::view::encoding_dialog::open_encoding_dialog(
                    window,
                    app,
                    current,
                    move |encoding, _window, app| {
                        entity.update(app, |panel, cx| panel.set_encoding(encoding, cx));
                    },
                );
            })
            .into_any_element()
    };
    // 缩进（VSCode 式菜单：`Tab` / `空格: N`，点击弹菜单选择 制表符 / 空格宽度；
    // 2026-10-27 合并 indent_kind + tab_size 两维度到一个入口，替换原「重设 ▾」里的缩进族）
    let indent_ctrl = {
        let current_tab = snapshot.tab_size;
        let hard_tabs = snapshot.hard_tabs;
        let entity = panel_entity.clone();
        // 对齐 VSCode：点击弹出缩进命令面板（双语项 + 分组标签 + tab_size 自定义输入框）
        Button::new("editor-indent")
            .ghost()
            .small()
            .debug_selector(|| "editor-indent".to_string())
            .label(if hard_tabs {
                "Tab".to_string()
            } else {
                format!("空格: {current_tab}")
            })
            .tooltip("缩进设置（点击打开）")
            .on_click(move |_, window, app| {
                let entity = entity.clone();
                let hard = hard_tabs;
                let tab = current_tab;
                crate::view::indent_dialog::open_indent_dialog(
                    window,
                    app,
                    hard,
                    tab,
                    move |action, _window, app| {
                        let entity = entity.clone();
                        entity.update(app, |panel, cx| match action {
                            crate::view::indent_dialog::IndentAction::UseSpaces => {
                                panel.set_hard_tabs(false, cx);
                            }
                            crate::view::indent_dialog::IndentAction::UseTabs => {
                                panel.set_hard_tabs(true, cx);
                            }
                            crate::view::indent_dialog::IndentAction::ChangeTabDisplaySize(size) => {
                                panel.set_hard_tabs(false, cx);
                                panel.set_tab_size(size.max(1), cx);
                            }
                            crate::view::indent_dialog::IndentAction::DetectFromContent => {
                                panel.detect_indent_from_content(cx);
                            }
                            crate::view::indent_dialog::IndentAction::ConvertToSpaces => {
                                panel.convert_indent(false, _window, cx);
                            }
                            crate::view::indent_dialog::IndentAction::ConvertToTabs => {
                                panel.convert_indent(true, _window, cx);
                            }
                            crate::view::indent_dialog::IndentAction::TrimTrailingWhitespace => {
                                panel.trim_trailing_whitespace(_window, cx);
                            }
                        });
                    },
                );
            })
            .into_any_element()
    };
    // 不可见字符开关（2026-10-27 升级：点击弹出**双栏配置面板**，自定义每类符号显示/隐藏；
    // ¶ 按钮 toggled 表示「当前有类别在标注」；面板见 `view/invisible_dialog.rs`）
    let invisible_ctrl = {
        let show_invisible = host.shared.document_show_invisible(&host.document);
        let mask = host.shared.document_invisible_mask(&host.document);
        let entity = panel_entity.clone();
        Button::new("editor-invisible")
            .ghost()
            .small()
            .debug_selector(|| "editor-invisible".to_string())
            .label("¶")
            .toggled(show_invisible)
            .tooltip("不可见字符标注（点击自定义：零宽 / 特殊空格 / 双向控制 / 控制字符）")
            .on_click(move |_, window, app| {
                let entity = entity.clone();
                let current_mask = mask;
                crate::view::invisible_dialog::open_invisible_dialog(
                    window,
                    app,
                    current_mask,
                    move |new_mask, _window, app| {
                        entity.update(app, |panel, cx| {
                            panel.set_invisible_categories(new_mask, cx);
                        });
                    },
                );
            })
            .into_any_element()
    };
    // 事务（提交 / 回滚；只有会通信 + 执行器支持事务 + 通道支持事务才摆）
    let tx_available = snapshot.connection.is_some()
        && host.shared.has_transactions()
        && host.channel().allows_transactions();
    let tx_ctrl = tx_available.then(|| {
        let mut row = div().h_flex().items_center().gap_1();
        if host.tx_open {
            for (action, id) in [
                (execution::TxAction::Commit, "editor-tx-commit"),
                (execution::TxAction::Rollback, "editor-tx-rollback"),
            ] {
                let entity = panel_entity.clone();
                row = row.child(
                    Button::new(id)
                        .ghost()
                        .small()
                        .debug_selector(move || id.to_string())
                        .label(action.label())
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.tx_action(action, cx));
                        }),
                );
            }
        }
        row.into_any_element()
    });
    // 中断（B3）：只在本文档真有语句没回填时出现
    let interrupt = host.can_interrupt().then(|| {
        let entity = panel_entity.clone();
        Button::new("editor-interrupt")
            .ghost()
            .small()
            .debug_selector(|| "editor-interrupt".to_string())
            .label("■ 中断")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.interrupt(cx));
            })
            .into_any_element()
    });

    let controls = status_bar::StatusControls {
        // 2026-10-27：连接 ▸ 库 ▸ Schema 定位整体迁工具栏右区固定槽（render_conn_slots），
        // 状态栏不再摆面包屑（左段只留方言 + 执行方式 + 事务等当前上下文）
        edit_mode: Some(edit_mode_ctrl),
        indent: Some(indent_ctrl),
        encoding: Some(encoding_ctrl),
        eol: Some(eol_ctrl),
        invisible: Some(invisible_ctrl),
        tx: tx_ctrl,
        channel: channel_ctrl,
        interrupt,
    };
    Some(status_bar::render(&inputs, controls, cx).into_any_element())
}

/// 工具栏右区「执行通道」pill（源库 / 加速 / 联邦可切；原型 §2.2 通道级+连接层）。
///
/// 2026-10-27：pill 固定宽（`CHANNEL_SLOT_WIDTH`）——与连接名称 / schema@db 并成
/// 工具栏右区三固定区，不随标签长短推挤布局；超长由容器裁掉（源库/加速/联邦本就很短，
/// 兜底用）。菜单构建发生在点击时（事件路径）——`channel_menu` 的门控真值来自宿主注入的
/// `ChannelsPort`；未注入端口时如实报「尚未接入」，不假装有通道可选。
pub(crate) fn render_channel_control(
    host: &EditorHostPanel,
    panel_entity: &Entity<EditorHostPanel>,
) -> Option<AnyElement> {
    let entity = panel_entity.clone();
    let current = host.channel();
    let items = host.channel_menu();
    let refresh_label = channel::refresh_item_label(current);
    Some(
        div()
            .flex_none()
            .overflow_hidden()
            .child(
                Button::new("editor-channel")
                    // 执行通道 pill 带背景（2026-10-27）：与连接/schema 两个 ghost 槽区分，
                    // 一眼看出「执行发往哪一档」
                    .with_variant(ButtonVariant::Secondary)
                    .small()
                    .debug_selector(|| "editor-channel".to_string())
                    // 执行位置图标化：图标（Locate = 定位执行到哪里）+ 通道短标签，去「执行位置：」文字前缀
                    .icon(IconName::Locate)
                    .label(format!("{} ▾", current.label()))
                    .dropdown_menu(move |menu, _window, _cx| {
                        let mut menu = menu;
                        // 菜单回调是 `Fn`：不能把 `items` 搬进去，逐项 clone 字段名与下标（少而短）
                        for item in items.iter() {
                            let entity = entity.clone();
                            let channel = item.channel;
                            menu = menu.item(
                                PopupMenuItem::new(item.label.clone())
                                    .checked(item.current)
                                    .disabled(!item.available)
                                    .on_click(move |_, _window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.set_channel(channel, cx);
                                        });
                                    }),
                            );
                        }
                        if let Some(label) = refresh_label {
                            let entity = entity.clone();
                            menu = menu.separator().item(PopupMenuItem::new(label).on_click(
                                move |_, _window, app| {
                                    entity.update(app, |panel, cx| panel.refresh_source(cx));
                                },
                            ));
                        }
                        menu
                    })
                    .into_any_element(),
            )
            .into_any_element(),
    )
}

/// 工具栏右区槽位长名截断（> `max` 字符 → 前 `max-1` + …；完整名靠菜单项 / 工具提示）
pub(crate) fn truncate_label(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut shortened: String = text.chars().take(max.saturating_sub(1)).collect();
        shortened.push('…');
        shortened
    }
}

/// 工具栏右区「连接定位」固定槽：运行点 + 连接名称（短码·名称） ▾ schema@库
///
/// 2026-10-27 按用户要求：连接定位从状态栏移工具栏右区，且**固定大小占用区域**——
/// 执行通道 pill / 连接名称 / schema@db 三区各自固定宽（`w(rem N).flex_none`），
/// 超长截断（`truncate_label` + 容器 `overflow_hidden` 兜底），不随内容推挤布局。
/// 状态栏左段不再占连接路径（`status_bar::render` 同步改，只留方言/事务等上下文）。
///
/// - **连接名称**：菜单只列**实时在线**连接（事件路径 `online_ids`）+「跟随当前连接」
///   （默认态）；运行点是**静态绑定指示**（绑定即亮；在线判定留给菜单打开时）。
/// - **schema@库**：schema 可切（schemas + 使用连接默认）；显示 `schema@库`（库 = catalog，
///   从 schemas 列表按当前 schema 反查）；绑定但未选时给可点 `schema ▾` 占位；
///   未绑定连接（跟随当前连接态）时无槽——schema 概念还没出现，不假装。
pub(crate) fn render_conn_slots(
    host: &EditorHostPanel,
    panel_entity: &Entity<EditorHostPanel>,
    theme: &gpui_kit::component::Theme,
) -> Option<AnyElement> {
    let entity = panel_entity.clone();
    let shared = host.shared.clone();
    let options = host.shared.connection_options();
    let has_port = host.shared.has_connections();
    let bound = host
        .with_document(|doc| doc.connection().map(str::to_string))
        .flatten();
    let schema = host
        .with_document(|doc| doc.schema().map(str::to_string))
        .flatten();
    let has_bound = bound.is_some();
    let schemas = host.shared.schemas(bound.as_deref());

    // ── 连接名称槽（固定宽；短码·名称 截断）──
    let conn_label = bound
        .as_deref()
        .and_then(|id| {
            options
                .iter()
                .find(|o| o.id == id)
                .map(|o| format!("{}·{}", o.short, o.name))
        })
        .unwrap_or_else(|| "跟随当前连接".to_string());
    let dot_color = if bound.is_some() {
        theme.colors.success
    } else {
        theme.colors.muted_foreground
    };
    let conn_entity = entity.clone();
    // 连接名称槽（2026-10-28 重构）：使用 DropdownButton 组件，左边是名称按钮，
    // 右边是下拉箭头，合并成一个整体，避免两个按钮分开的视觉问题。
    let conn_name_button = Button::new("editor-conn-name")
        .debug_selector(|| "editor-conn-name".to_string())
        .icon(
            Icon::empty()
                .path("icons/dot.svg")
                .size(theme.font_size * ui::STATUS_DOT_SIZE)
                .text_color(dot_color),
        )
        .label(truncate_label(&conn_label, 14))
        .on_click({
            let entity = entity.clone();
            let shared = shared.clone();
            let bound = bound.clone();
            move |_, window, app| {
                let options = shared.connection_options();
                let current = bound.clone();
                let entity = entity.clone();
                crate::view::dialogs::open_connection_picker(
                    window,
                    app,
                    options,
                    current,
                    move |id, _window, app| {
                        entity.update(app, |panel, cx| panel.bind_connection(id, cx));
                    },
                );
            }
        });
    let conn_slot = DropdownButton::new("editor-conn-dropdown")
        .button(conn_name_button)
        .dropdown_menu(move |menu, _window, _cx| {
            let mut menu = menu;
            if !has_port {
                return menu
                    .item(PopupMenuItem::new("（未接入连接列表）").disabled(true));
            }
            let follow_entity = conn_entity.clone();
            let unbound = bound.is_none();
            menu = menu.item(
                PopupMenuItem::new("跟随当前连接")
                    .checked(unbound)
                    .on_click(move |_, _window, app| {
                        follow_entity.update(app, |panel, cx| {
                            panel.bind_connection(None, cx);
                        });
                    }),
            );
            menu = menu.separator();
            // 实时在线（菜单打开时查，事件路径）：只列真在线连接
            let online = shared.online_ids();
            let online_conns: Vec<(String, String)> = options
                .iter()
                .filter(|o| online.contains(&o.id))
                .map(|o| (format!("{}·{}", o.short, o.name), o.id.clone()))
                .collect();
            if online_conns.is_empty() {
                return menu
                    .item(PopupMenuItem::new("（暂无在线连接）").disabled(true));
            }
            for (label, id) in &online_conns {
                let entity = conn_entity.clone();
                let is_current = bound.as_deref() == Some(id.as_str());
                let label = label.clone();
                let id = id.clone();
                menu = menu.item(
                    PopupMenuItem::new(label)
                        .checked(is_current)
                        .on_click(move |_, _window, app| {
                            let id = id.clone();
                            entity.update(app, |panel, cx| {
                                panel.bind_connection(Some(id), cx);
                            });
                        }),
                );
            }
            menu
        })
        .with_variant(ButtonVariant::Secondary)
        .small()
        .into_any_element();

    // ── schema@库 槽（固定宽；有绑定连接才摆；`schema@catalog` 截断）──
    // 2026-10-27 拆两段：名称区点击弹「选择 Database/Schema」窗（左库右 Schema 选绑定），
    // ▾ 区点击弹现有 schema 下拉（含「使用连接默认」）。显示规则：有 schema 时 `schema@库`，
    // 库即 Schema（catalog 为空）时只显示库名——与弹窗同一口径。
    let schema_slot = has_bound.then(|| {
        let schemas_menu = schemas.clone();
        let current_schema = schema.clone();
        let entity = entity.clone();
        let label = match &schema {
            Some(s) => {
                let catalog = schemas
                    .iter()
                    .find(|item| item.schema == *s)
                    .and_then(|item| item.catalog.clone());
                match catalog {
                    Some(c) => format!("{s}@{c}"),
                    None => s.clone(),
                }
            }
            None => "schema".to_string(),
        };
        let schema_name_button = Button::new("editor-schema-name")
            .debug_selector(|| "editor-schema-name".to_string())
            .label(truncate_label(&label, 12))
            .on_click({
                let entity = entity.clone();
                let schemas = schemas.clone();
                let schema = schema.clone();
                move |_, window, app| {
                    let schemas = schemas.clone();
                    let schema = schema.clone();
                    let entity = entity.clone();
                    crate::view::dialogs::open_schema_picker(
                        window,
                        app,
                        schemas,
                        schema,
                        move |sch, _window, app| {
                            entity.update(app, |panel, cx| {
                                panel.set_schema(Some(sch), cx);
                            });
                        },
                    );
                }
            });
        DropdownButton::new("editor-schema-dropdown")
            .button(schema_name_button)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if schemas_menu.is_empty() {
                    return menu
                        .item(PopupMenuItem::new("（暂无可切 Schema）").disabled(true));
                }
                for item in &schemas_menu {
                    let entity = entity.clone();
                    let schema_name = item.schema.clone();
                    let is_current =
                        current_schema.as_deref() == Some(schema_name.as_str());
                    menu = menu.item(
                        PopupMenuItem::new(schema_name.clone())
                            .checked(is_current)
                            .on_click(move |_, _window, app| {
                                entity.update(app, |panel, cx| {
                                    panel.set_schema(Some(schema_name.clone()), cx);
                                });
                            }),
                    );
                }
                let entity = entity.clone();
                let using_default = current_schema.is_none();
                menu = menu.item(
                    PopupMenuItem::new("使用连接默认")
                        .checked(using_default)
                        .on_click(move |_, _window, app| {
                            entity.update(app, |panel, cx| panel.set_schema(None, cx));
                        }),
                );
                menu
            })
            .with_variant(ButtonVariant::Secondary)
            .small()
            .into_any_element()
    });

    Some(
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .child(conn_slot)
            .children(schema_slot)
            .into_any_element(),
    )
}
