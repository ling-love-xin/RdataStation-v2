//! `EditorHostPanel::new` 的独立 impl 块：一个面板 = 一个标签 = 一份文档的装配。
//!
//! 为什么单独成文件：装配要按顺序建内核 / 装 provider / 订阅 / 恢复会话 / 应用设置默认，
//! 每一步都有顺序约束（注释里逐条写着），拆成小函数反而看不清次序。

use super::*;

impl EditorHostPanel {
    pub fn new(
        shared: EditorShared,
        document: DocumentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        // 【B4】语言编辑规则全局注册（幂等：重复注册同键覆盖）。面板首次创建时
        // 完成一次即可——`set_language_config` 是叠加注册，多面板重复调用无害。
        crate::lsp::language_config::install(cx);

        let (content, editor_read_only, mode) = {
            let service = shared.service();
            match service.find(&document) {
                Some(doc) => (
                    doc.content().to_string(),
                    !doc.read_only().can_edit(),
                    doc.mode(),
                ),
                None => (String::new(), true, EditorMode::General),
            }
        };
        // 分析模式的 `.rdsnote` 解析要读正文，而 `content` 稍后被 move 进编辑内核闭包——先 clone
        let notebook_content = content.clone();
        // 语义着色类型在闭包外算好：SQL 模式与未识别扩展名走 SQL 词法，通用模式按
        // 脚本扩展名分派（CSV 走表格展示不上色）。`service.find` 拿文档路径做判定。
        let highlight_kind = {
            let service = shared.service();
            match service.find(&document) {
                Some(doc) => highlight::kind_for_document(doc.path(), doc.mode()),
                None => highlight::kind_for_document(None, mode),
            }
        };

        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx);
            // 【B4】语言编辑规则：SQL 走注册表（`set_language_config("sql", …)`），
            // 脚本类型走 gpui-component 默认（text / json / python 已有 config）。
            // language 键决定自动闭合 / 智能缩进的规则来源，与高亮类型一致。
            if let Some(language) = editor_language_key(highlight_kind) {
                state = state.language(language);
            }
            state.set_value(content, window, cx);
            state.set_readonly(editor_read_only, cx);
            // 去掉编辑器默认左边 padding（让文字顶到最左边）
            state.set_editor_paddings(gpui::Edges {
                top: Pixels::ZERO,
                right: Pixels::from(8.),
                bottom: Pixels::ZERO,
                left: Pixels::ZERO,
            });
            // A4：语义着色（SQL 词法；通用模式按脚本类型分派；CSV / 分析模式不上色）
            if highlight::is_enabled(mode) {
                highlight::install(&mut state, cx, highlight_kind);
            }
            // B9：SQL 补全（候选来自宿主端口；文本模式 / 大文件档位不装）
            if shared.completion_enabled(&document) {
                completion::install(&mut state, shared.clone(), document.clone());
            }
            // B17：折叠（内核只认 highlighter 给的候选，我们自己算，见 `fold` / `view::fold`）。
            // 装不上就真关开关，不只留一个判据。
            sync_fold(&shared, &document, &mut state, window, cx);
            state
        });

        let sub = cx.subscribe_in(
            &editor,
            window,
            |this, state, event: &InputEvent, window, cx| {
                // 【书签】gutter 书签列单击：内核只给行号与 Shift，语义在这里定
                if let InputEvent::GutterMarkerClick { line, shift } = event {
                    this.on_gutter_marker_click(*line, *shift, window, cx);
                    return;
                }
                if matches!(event, InputEvent::Change) {
                    // 【T2c-3 收尾】行编辑模式强制单光标（列编辑 = 多光标）：文本变更时
                    // 把多光标收拢到活动光标。订阅闭包里的 `state` 是内核实体的引用，
                    // 先 `read` 判断、再 `update` 收拢（避免与后面的 `this.editor_text` 争用）。
                    if this.edit_mode == EditMode::Row && state.read(cx).cursor_count() > 1 {
                        state.update(cx, |state, cx| state.collapse_cursors(window, cx));
                    }
                    // 内核 → 服务：内容只落到 `EditorService`，脏状态由它比较基线得出
                    let text = this.editor_text(cx);
                    // 【#54-C4 二期】输入 `(` 自动弹签名帮助：光标前一字符是 `(` 且
                    // `auto_call` 识别出函数调用才弹（打字中光标紧随参数/逗号天然不触发）；
                    // 已有模态 Dialog 时不抢（保险：签名帮助打开期间编辑器已失焦）
                    if !window.has_active_dialog(cx) {
                        let cursor = state
                            .read(cx)
                            .cursor_offsets()
                            .first()
                            .copied()
                            .unwrap_or(0);
                        if let Some(view) = crate::view::signature_help::auto_call(&text, cursor) {
                            crate::view::signature_help::open_signature_help(window, cx, view);
                        }
                    }
                    // 【LSP】全文同步给语言服务（版本自增；会话健康才发，死了不写）
                    if this.lsp_alive {
                        if let Some(session) = this.lsp.as_ref() {
                            this.lsp_version += 1;
                            let _ =
                                session
                                    .client
                                    .did_change(&session.uri, this.lsp_version, &text);
                        }
                    }
                    // 【B18】语句数 / 折叠候选 / 打字期诊断一律**不在事件路径上算**：
                    // 提交一份带修订号的扫描（正文要两份——服务层一份、扫描器一份，所以克隆一次），
                    // 回执由结果轮询泵回填（`drain_exec_results`）——陈旧的那份会被丢掉。
                    // 动作与补全 / 格式化共用这一条事件路径，所以 `replace_all`
                    // （模板 / 注释 / 格式化）也覆盖到。
                    this.submit_scan(text.clone(), cx);
                    this.shared
                        .update(|service| service.set_content(&this.document, text));
                    // 【遗留 #14】CSV 表格展示：编辑 CSV 文本时实时重解析进网格
                    //（每次击键全量解析对大数据文件有开销，但 CSV 预览场景数据量
                    //  通常可控；卡顿再换防抖泵）
                    this.sync_csv_grid(cx);
                    // 【方案 A】书签偏移校正：取内核本次编辑的行增量，平移 shared 书签并回写
                    this.adjust_bookmarks_for_edit(state, cx);
                    // 【2c】显示隐藏字符开启时：正文变了，标注区间要跟着重算（区间是字节偏移，
                    // 文字一动就全失效；没开显示则集合为空 / 未建，直接跳过）
                    if this.shared.document_show_invisible(&this.document) {
                        let ranges = this.invisible_ranges_for(cx);
                        this.refresh_invisible_decorations(ranges, cx);
                    }
                    // 重绘以刷新标签脏点与状态栏
                    cx.notify();
                    // 状态栏内容变了（行 / 列 / 脏点 / 语句数）：通知宿主刷新全局状态栏段
                    cx.emit(EditorHostEvent::StatusChanged);
                }
            },
        );

        let statements = count_statements(
            &shared
                .service()
                .find(&document)
                .map(|doc| doc.content().to_string())
                .unwrap_or_default(),
        );

        // 【B15】本地筛选框已随 Dock 化迁入面板实体（`ResultPanel::new` 自建含防抖订阅）；
        // 编辑器只保留 右键「按值筛选」→ 面板 `apply_filter_value` 的转发（见钩子注入区）。

        // 结果网格：构造时一次建成，以后结果变化只 `refresh`（不重建，避免丢掉列宽 / 滚动位置）
        let grid = result_grid::new_table_state(
            ResultGridDelegate::empty("尚未执行——按 Ctrl+Enter 执行当前语句"),
            window,
            cx,
        );
        // 【并排对比】对比栏网格：与主网格同处一次建成（复用一个实体，结果变化只 refresh）。
        // 只在对比模式渲染；数据经 `sync_compare_grid` 从 ResultStore 拷入。
        let compare_grid =
            result_grid::new_table_state(ResultGridDelegate::empty("未选择对比结果"), window, cx);

        // Dock 化接线：面板实体 + 动作队列（渲染期 push / 事件路径 drain 分派）。
        // 网格经 `set_grids` 注入**同一实体**——编辑器动作与面板渲染读写同一份 TableState。
        let result_action_queue = ResultActionQueue::new();
        let result_panel = cx.new(|cx| {
            let mut panel = ResultPanel::new(window, cx, result_action_queue.clone());
            panel.set_grids(grid.clone(), Some(compare_grid.clone()), cx);
            // 【B2】全局偏好 `result_panel.show_column_comments` 启动时同步到面板（面板级临时开关
            // 覆盖此默认值；用户在面板工具栏切换后不再回写全局——全局只控"默认开不开"）
            if let Some(settings) = cx.try_global::<settings::model::Settings>() {
                // 先拷贝偏好值、释放 `settings` 借用，再对 panel 可变借用 `cx`
                //（try_global 的借用与 `cx` 可变借用冲突 → E0502，2026-10-28 修复）。
                let on = settings.result_panel.show_column_comments;
                let type_style = settings.result_panel.type_style();
                if on {
                    panel.set_show_column_comments(true, cx);
                }
                // 【类型样式】全局偏好启动同步（数字格式 / NULL 占位 / 布尔映射 / 按族对齐与前景色）；
                // 设置页后续变更走 `apply_result_style`（workbench 钩子）即时生效
                panel.set_type_style(type_style, cx);
            }
            panel
        });
        // 【#8-按钮断链修复】队列通知目标 = 结果面板实体（弱引用）。此后按钮 `request`
        // 入队即通知面板 → 下方 observe_in 回调 drain，点击不再依赖"碰巧的 notify"。
        result_action_queue.set_panel(result_panel.downgrade());
        // Dock 化 §8.5（用户已确认「editor 宿主订阅自 drain」）：面板按钮 push 队列 →
        // 面板 notify → 本宿主观察回调 drain 分派回动作方法。gpui 的通知延迟到事务
        // 边界派发（app.rs notify → pending_effects），回调执行时面板已不在 update 中，
        // 不会「cannot update while already being updated」。
        let result_panel_sub = cx.observe_in(&result_panel, window, |this, _panel, _window, cx| {
            this.drain_result_actions(cx);
        });
        // 行选中 → 状态行那一段（⑦ 的“已选第 N 行”）要重画（表格自己不通知宿主）
        let grid_sub = cx.subscribe_in(
            &grid,
            window,
            |panel, grid, event: &TableEvent, window, cx| {
                let selected_row = grid.read(cx).selected_row().map(|row| row + 1);
                // 状态行「当前格值」段走面板增量更新（DBeaver 对齐）：先取一次面板实体，
                // 各分支复用（`result_panel()` 返回 clone，不借用 panel，可随后写 result_selection）
                let result_panel = panel.result_panel();
                // 【B14】`Ctrl+C` 复制谁：只认**最近一次选择事件**（组件没公开“当前是选格还是选行”，
                // 而它两个字段可以同时有值）。键位失效之后再选也就自动作废了，不用额外清理。
                panel.result_selection = match event {
                    TableEvent::SelectCell(row, col) => {
                        // 编辑器在事件里读网格 delegate 算好值传给面板（面板不碰网格；
                        // 长值截断防状态行被撑爆）
                        let text = grid.read(cx).delegate().cell_text(*row, *col, cx);
                        result_panel.update(cx, |panel, cx| {
                            panel.set_selected_cell(Some(Self::clip_cell_preview(&text)), cx);
                            // 【侧栏】右侧辅助栏（值详情 / 列浏览器）要位置：选中格的原生事件就带着
                            // window 同步列浏览器 List 内部选中高亮（网格反推路径）
                            panel.set_selection_pos(Some((*row, *col)), window, cx);
                        });
                        Some(result_grid::GridSelection::Cell {
                            row: *row,
                            col: *col,
                        })
                    }
                    TableEvent::SelectRow(row) => {
                        result_panel.update(cx, |panel, cx| {
                            panel.set_selected_cell(None, cx);
                            // 【侧栏】选整行不投影单格值详情
                            panel.set_selection_pos(None, window, cx);
                        });
                        Some(result_grid::GridSelection::Row { row: *row })
                    }
                    TableEvent::ClearSelection => {
                        result_panel.update(cx, |panel, cx| {
                            panel.set_selected_cell(None, cx);
                            panel.set_selection_pos(None, window, cx);
                        });
                        None
                    }
                    TableEvent::ColumnWidthsChanged(widths) => {
                        // 【P1-5】拖宽把列宽记进 delegate：排序 / 改行数重建表头时不被打回默认档。
                        // 组件发完事件已自刷新，这里不再 notify；只记宽度，不改动选中状态。
                        grid.update(cx, |state, _cx| {
                            state.delegate_mut().set_column_widths(widths)
                        });
                        panel.result_selection
                    }
                    _ => panel.result_selection,
                };
                // 【T2a-4】双击单元格 → 进入编辑（原生 `DoubleClickedCell` 事件；判据一次
                // 问清：通道 / 项目 / 文档 / 连接 / 行标识 / 列。不过就不进编辑，顶部说明原因。
                if let TableEvent::DoubleClickedCell(row, col) = event {
                    let gate = {
                        let store = panel.shared.results();
                        let entry = store.active(&panel.document);
                        crate::editability::cell_editable(
                            panel.channel(),
                            crate::project::ProjectState {
                                read_only: panel.shared.project_read_only(),
                            },
                            panel.read_only_flags(),
                            entry.and_then(|entry| entry.row_key()).as_ref(),
                            entry.map(|entry| entry.columns.len()).unwrap_or(0),
                            col.saturating_sub(1),
                        )
                    };
                    match gate {
                        Ok(()) => {
                            grid.update(cx, |state, cx| {
                                if state.delegate_mut().begin_edit(*row, *col) {
                                    cx.notify();
                                }
                            });
                            // 【T2a-4】进入编辑后**立即**把焦点交给编辑框（事件路径，不是 render——
                            // render 是纯读阶段，那里的 `window.focus` 不可靠）。焦点事件在下一帧
                            // draw 时派发，输入内核据此激活文本输入，用户才能直接打字。
                            if let Some(input) = grid.read(cx).delegate().edit_input() {
                                let focus = input.read(cx).focus_handle(cx);
                                window.focus(&focus, cx);
                            }
                        }
                        Err(reason) => {
                            // 编辑被拦提示迁入面板（增量写入；sync 全量覆写前保持显示）
                            panel.result_panel.update(cx, |p, cx| {
                                p.set_notice(Some(format!("编辑被拦：{}", reason.reason())), cx)
                            });
                        }
                    }
                }
                // 行选中 → 状态行「已选第 N 行」增量更新（迁入面板实体：`set_selected_row`
                // 只补 status.selected_row，sync 全量覆写前的中间态保持一致）
                panel
                    .result_panel
                    .update(cx, |p, cx| p.set_selected_row(selected_row, cx));
                cx.notify();
            },
        );

        // 【T2a-4】结果表格的单元格编辑框：**一个复用实体**（每次编辑复用同一个，
        // 不做“每格一个实体”的泄漏）。初值同步与聚焦在网格渲染路径做（幂等一次）。
        let edit_input = cx.new(|cx| InputState::new(window, cx));
        // 【类型感知】日期列的日历实体：与编辑框同一个「复用一份」原则（不给每格建实体）。
        // 显示格式按 `YYYY-MM-DD`——与 `Date` 列在校验（`looks_like_date`）和落库里的字符串
        // 口径一致，避免日历上看到 `2026/11/08`、提交成 `2026-11-08` 的“两副面孔”。
        let edit_calendar = cx
            .new(|cx| DatePickerState::new(window, cx).date_format(SharedString::from("%Y-%m-%d")));
        // 日历选中即提交：`Change` 在弹层里每选一次就发（时间精度的场景会连发，
        // 这里只认日期列，选一次定一次）。提交仍走 `commit_edit_as` → 同一条校验 / 钩子通道。
        let _calendar_sub = cx.subscribe_in(
            &edit_calendar,
            window,
            |panel, _state, event: &DatePickerEvent, window, cx| {
                let DatePickerEvent::Change(value) = event;
                let Some(text) = value.format("%Y-%m-%d") else {
                    return;
                };
                let committed = panel.grid.update(cx, |table, _cx| {
                    table.delegate_mut().commit_edit_as(text.to_string())
                });
                if committed {
                    panel.sync_result_view(cx);
                } else {
                    window.refresh();
                }
            },
        );
        // 编辑框事件（原生 `InputEvent`）：输入变化 → 同步到编辑态；Enter / 失焦 → 提交。
        // 提交幂等：`commit_edit` 先清编辑态，失焦带起的第二次 Blur 是空转。
        let _edit_sub =
            cx.subscribe_in(
                &edit_input,
                window,
                |panel, state, event, window, cx| match event {
                    InputEvent::Change => {
                        let value = state.read(cx).value().to_string();
                        panel.grid.update(cx, |table, _cx| {
                            table.delegate_mut().sync_edit_value(value);
                        });
                    }
                    InputEvent::PressEnter { .. } | InputEvent::Blur => {
                        // 【类型感知】`commit_edit` 返回是否真的交了：类型校验失败 = 编辑态
                        // 保留 + 红框提示，此时**不能**跑整轮投影（`sync_result_view` 重建网格
                        // 会把编辑态冲掉）——只请求重绘让红框 / 错误提示上屏。
                        let committed = panel
                            .grid
                            .update(cx, |table, _cx| table.delegate_mut().commit_edit());
                        if committed {
                            // 【T2a-4】提交完成后投影刷新（脏格 + 待提交计数 + 网格数据一起走
                            // `sync_result_view`；grid 已退出上面的 update，这里可安全 update 面板）。
                            // Dock 化后提交按钮组 / 脏格底色读面板投影，不刷新就不会出现。
                            panel.sync_result_view(cx);
                        } else {
                            window.refresh();
                        }
                    }
                    _ => {}
                },
            );
        // 编辑框实体 + 提交钩子注入网格（提交走钩子交回面板落变更集）
        {
            let edit_weak = edit_input.clone();
            grid.update(cx, |state, _cx| {
                state.delegate_mut().set_edit_input(edit_weak);
                state
                    .delegate_mut()
                    .set_edit_calendar(edit_calendar.clone());
            });
            let shared_for_commit = shared.clone();
            let document_for_commit = document.clone();
            grid.update(cx, |state, _cx| {
                state.delegate_mut().set_commit_edit_hook(std::rc::Rc::new(
                    move |row, column, original, edited| {
                        // 【T2a-4】提交落权威变更集：只动 Shared（独立实体，可安全 update）。
                        // 钩子是在网格 delegate 内部（`commit_edit`）被调用的——这里再
                        // update 网格就是重入 panic；脏格重投影由订阅闭包（PressEnter /
                        // Blur 分支）在 commit 完成后做，那里 grid 已退出 update。
                        shared_for_commit.update_results(|store| {
                            let active = store.active_index(&document_for_commit).unwrap_or(0);
                            let changed = edited != original;
                            store.mutate_change_set(&document_for_commit, active, |changes| {
                                if !changed {
                                    // 值没变（改回来）：清掉这条变更
                                    changes.remove(row, column);
                                } else {
                                    changes.upsert(CellChange::pending(
                                        row,
                                        column,
                                        original.clone(),
                                        edited.clone(),
                                    ));
                                }
                            });
                            // 【撤销】值真的变了才记一步历史（改回原值不是新编辑，不入栈；
                            // 新编辑入栈时 EditHistory 自动清空重做栈——标准撤销语义）
                            if changed {
                                store.mutate_history(&document_for_commit, active, |history| {
                                    history
                                        .push(CellChange::pending(row, column, original, edited));
                                });
                            }
                        });
                    },
                ));
            });
            // 【#28-B4】本地行格编辑提交：只写回本地行快照，不进变更集、不进格级 /
            // 行级撤销栈（本地行格编辑提交即落快照，拍板口径；重投影复用 Enter / Blur 后
            // 的 `sync_result_view`，与底层格编辑同一条刷新路径）。
            let shared_for_local = shared.clone();
            let document_for_local = document.clone();
            grid.update(cx, |state, _cx| {
                state
                    .delegate_mut()
                    .set_commit_local_edit_hook(std::rc::Rc::new(
                        move |local_id, column, edited| {
                            shared_for_local.update_results(|store| {
                                let active = store.active_index(&document_for_local).unwrap_or(0);
                                store.mutate_local_rows(&document_for_local, active, |set| {
                                    if let Some(row) = set.local_row_mut(local_id) {
                                        if let Some(cell) = row.cells.get_mut(column) {
                                            *cell = edited;
                                        }
                                    }
                                });
                            });
                        },
                    ));
            });
        }

        // 【B5b】滚动到底自动加载：组件库在可见范围接近末尾时调 delegate 的 `load_more`，
        // 而“该不该再取一段”只有面板知道（选中哪份结果、忙不忙）——走钩子交回给面板。
        // 面板自己的 `fetch_more` 仍是**显式按钮**与自动加载共用的同一条路。
        // 【B14】右键「按值筛选」也走同一套：值写进筛选框并立刻应用（滤镜 UI 在面板这边）。
        {
            let load_weak = cx.entity().downgrade();
            let filter_weak = cx.entity().downgrade();
            // 【筛选】列头筛选钩子与「按值筛选」共用宿主弱引用：两个闭包各要一份（WeakEntity 非 Copy）
            let filter_weak_col = filter_weak.clone();
            let sort_weak = cx.entity().downgrade();
            let insight_weak = cx.entity().downgrade();
            let preview_weak = cx.entity().downgrade();
            // 【#28-B3】右键行级动作（新增 / 复制 / 删除）：delegate 已打包成 ResultRequest，
            // 这里转投结果动作队列并让面板 notify 一下，由 observe 回调统一 drain 分派落库。
            let row_action_weak = cx.entity().downgrade();
            // 【M8】宿主没接「洞察此列」端口就不装钩子——菜单据此不摆那一项（能力没有就不给入口）
            let insight_port = shared.insight_column_port();
            grid.update(cx, |state, _cx| {
                state
                    .delegate_mut()
                    .set_load_more_hook(std::rc::Rc::new(move |app: &mut App| {
                        _ = load_weak.update(app, |panel, cx| panel.fetch_more(cx));
                    }));
                state.delegate_mut().set_filter_value_hook(std::rc::Rc::new(
                    move |value: &str, app: &mut App| {
                        let value = value.to_string();
                        _ = filter_weak
                            .update(app, |panel, cx| panel.apply_filter_value(&value, cx));
                    },
                ));
                // 【筛选】列头菜单「按值筛选此列 / 清除」交回面板（空串 = 清除）
                let filter_weak = filter_weak_col;
                state
                    .delegate_mut()
                    .set_column_filter_hook(std::rc::Rc::new(
                        move |column: usize, value: &str, app: &mut App| {
                            let value = value.to_string();
                            _ = filter_weak.update(app, |panel, cx| {
                                panel.apply_column_filter(column, &value, cx)
                            });
                        },
                    ));
                let sort_weak = sort_weak;
                state.delegate_mut().set_sort_down_hook(std::rc::Rc::new(
                    move |column: &str, descending: bool, app: &mut App| {
                        let column = column.to_string();
                        _ = sort_weak
                            .update(app, |panel, cx| panel.sort_down(&column, descending, cx));
                    },
                ));
                if insight_port.is_some() {
                    state
                        .delegate_mut()
                        .set_insight_column_hook(std::rc::Rc::new(
                            move |column: &str, app: &mut App| {
                                let column = column.to_string();
                                _ = insight_weak
                                    .update(app, |panel, cx| panel.insight_column(&column, cx));
                            },
                        ));
                }
                // 【遗留 #7】「查看完整值」：弹层是宿主的事（对话框生命周期归宿主），
                // 钩子只把 值 / 列名 / 视图行 带回面板
                let preview_for_value = preview_weak.clone();
                state
                    .delegate_mut()
                    .set_preview_value_hook(std::rc::Rc::new(
                        move |value: String,
                              column: String,
                              _column_ix: usize,
                              row: usize,
                              app: &mut App| {
                            _ = preview_for_value.update(app, |panel, cx| {
                                panel.open_value_preview(&value, &column, row, cx)
                            });
                        },
                    ));
                // 【P2-3】列统计：统计文本已由网格算好，弹层复用「查看完整值」的对话框
                // （标题 = 列名，正文 = 统计行）
                let preview_for_stats = preview_weak.clone();
                state.delegate_mut().set_column_stats_hook(std::rc::Rc::new(
                    move |column: String, text: String, app: &mut App| {
                        _ = preview_for_stats.update(app, |panel, cx| {
                            panel.open_value_preview(&text, &column, 0, cx)
                        });
                    },
                ));
                // 【#28-B3】右键新增 / 复制 / 删除行：动作已在 delegate 打包，转投队列后 notify，
                // 由面板 observe 回调 drain 落库（与状态栏按钮同一条分派路径）。
                state
                    .delegate_mut()
                    .set_row_action_hook(std::rc::Rc::new(move |request, app| {
                        _ = row_action_weak.update(app, |host, cx| {
                            host.result_action_queue.push(request);
                            host.result_panel.update(cx, |_, cx| cx.notify());
                        });
                    }));
            });
        }

        // 【2d/T2d-5】分析模式：正文是 `.rdsnote` JSON——解析进笔记本持有器（渲染读它）。
        // 空正文 = 新建空笔记；解析失败如实报（状态栏提示，不假装打开成功）。
        if mode == EditorMode::Analysis {
            if notebook_content.trim().is_empty() {
                shared.set_notebook_state(&document, crate::cell::NotebookState::new());
            } else {
                match crate::cell::NotebookFile::from_json(&notebook_content) {
                    Ok(file) => {
                        let (session, cells) = file.into_state();
                        shared.set_notebook_state(
                            &document,
                            crate::cell::NotebookState { session, cells },
                        );
                    }
                    Err(_reason) => {
                        // 提示卡由面板的消息位承接（构造期没有 cx.notify 目标，先落空态）
                        shared.set_notebook_state(&document, crate::cell::NotebookState::new());
                    }
                }
            }
        }

        let mut panel = Self {
            shared,
            document,
            focus_handle,
            editor,
            statements,
            group: None,
            area: None,
            message: None,
            edit_mode: EditMode::Row,
            closed: false,
            // #48 面板创建即取设置当前值（与内核默认一致；后续变化由 render 同步）
            show_line_numbers: editor_show_line_numbers_or_default(cx),
            // 书签悬停说明：创建时取设置值，render 权威同步点比对后重新投影标记
            show_bookmark_hover_note: editor_bookmark_hover_note_or_default(cx),
            // 常显书签列：缓存初值取内核默认（true），render 首帧即比对设置并同步，
            // 免得「设置为关」时因初值等于设置值而永远不调用内核 setter。
            show_bookmark_column: true,
            // #64 自动换行缓存：创建时取设置值，render 权威同步点比对后同步内核
            soft_wrap: editor_soft_wrap_or_default(cx),
            // #65 自动闭合 / 智能缩进缓存：同 soft_wrap（创建时取设置值）
            auto_close: editor_auto_close_or_default(cx),
            smart_indent: editor_smart_indent_or_default(cx),
            // 用户只读开关默认关（可编辑）；roLock 点击后翻转
            user_ro_lock: false,
            title_hovered: std::cell::Cell::new(false),
            grid,
            pending: 0,
            running_since: None,
            kernel_running: None,
            kernel_pump: RefCell::new(None),
            notebook_pending: Vec::new(),
            autocommit: true,
            tx_open: false,
            tx_since: None,
            tx_pending: 0,
            refresh_pending: 0,
            export_pending: 0,
            pending_labels: std::collections::VecDeque::new(),
            result_panel,
            result_height: std::rc::Rc::new(Cell::new(ui::RESULT_PANE_HEIGHT)),
            result_action_queue,
            compare_grid: Some(compare_grid),
            pending_bind: None,
            error_site: None,
            script_card: None,
            error_site_key: None,
            window: window.window_handle(),
            value_preview: None,
            csv_active: false,
            pending_export: None,
            fetching_more: false,
            result_selection: None,
            exec_pump: RefCell::new(None),
            scanner: crate::scan::Scanner::new(),
            scan_revision: 0,
            scan_pending: false,
            scan_diagnostics: Vec::new(),
            error_reason: None,
            cell_error_marks: std::collections::HashMap::new(),
            _editor_sub: Some(sub),
            _grid_sub: Some(grid_sub),
            _result_panel_sub: Some(result_panel_sub),
            _edit_sub: Some(_edit_sub),
            commit_in_flight: None,
            toolbar: crate::toolbar::ToolbarLayout::default(),
            invisible_decorations: None,
            bookmark_decorations: None,
            lsp: None,
            lsp_version: 0,
            lsp_alive: false,
            lsp_diagnostics: Vec::new(),
            cell_editors: std::collections::HashMap::new(),
            _cell_editor_subs: std::collections::HashMap::new(),
            markdown_preview: false,
        };
        // 【LSP】建面板即向宿主要一次服务（有 → didOpen + 挂 provider；无 → 如实不接）
        panel.sync_lsp_session(cx);
        // 【LSP】新会话的 provider 装配：`new` 里的内联 install 只按 SQL 能力装，
        // 通用模式的 LSP provider 走这里（`sync_mode` 只在切模式时跑，新建不经过它）
        panel
            .editor
            .update(cx, |state, _cx| panel.sync_completion_provider(state));
        // 【遗留 #14】CSV 表格展示：打开即把内容解析进结果网格（空文本也是合法 CSV，
        // 给一个空表而非不显示）
        panel.sync_csv_grid(cx);
        // #48 设置默认值落库：新建 / 未动过格式的文档应用（会话恢复在 show_document
        // 里于面板创建**之后**执行，会话值会覆盖这里的默认——文档级优先）。
        panel.apply_editor_defaults(cx);
        // 【遗留 #10】单元编辑器**不在构造期创建**：构造上下文里 `cx.new` + 订阅会
        // 触发「面板正在被更新」重入 panic。建实例走事件路径——`on_added_to`
        // （面板进 Dock）与单元增删方法。
        panel
    }
}
