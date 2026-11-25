//! 请求分发与窗口辅助（dispatch_result_request / drain_result_actions / clip_cell_preview）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use result_panel::ports::ResultRequest;

impl EditorHostPanel {
    /// 处理中可能**再入队**（如 `SetPushdown` 打开且有词 → 面板入队 `PushdownFilter`），
    /// 所以循环 drain 直到队列空——新请求落进下一轮，同一次事件里一并消化。
    ///
    /// 【第 60 批】带 `window`：切表示要移交焦点（面板层拿不到 window）。
    pub(crate) fn drain_result_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut pending = self.result_action_queue.drain();
        while !pending.is_empty() {
            for request in std::mem::take(&mut pending) {
                self.dispatch_result_request(request, window, cx);
            }
            pending = self.result_action_queue.drain();
        }
    }

    /// 状态行「当前格值」截断：长值只留一截（按字符截，避免把多字节字符拦腰切断），
    /// 防状态行被大字段撑爆（DBeaver 对齐：状态行只给预览，看全值走值预览）
    pub(crate) fn clip_cell_preview(text: &str) -> String {
        const MAX: usize = 48;
        let mut count = 0usize;
        let mut end = 0usize;
        for (i, ch) in text.char_indices() {
            if count >= MAX {
                break;
            }
            count += 1;
            end = i + ch.len_utf8();
        }
        if count >= MAX {
            format!("{}…", &text[..end])
        } else {
            text.to_string()
        }
    }

    /// 【第 60 批】切呈现后移交焦点：目标由面板按当前呈现给出（文本态 → 面板根，
    /// 网格态 → 网格）。面板层拿不到 window（`render` 是 `&App`、事件方法用
    /// `Context<Self>`），所以这一步落在宿主这侧。
    fn focus_after_presentation_switch(&self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.result_panel.read(cx).presentation_focus_target(cx);
        window.focus(&target, cx);
    }

    /// 单条结果动作的分派（语义与渲染区删除前的按钮回调一一对应）
    pub(crate) fn dispatch_result_request(
        &mut self,
        request: ResultRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match request {
            ResultRequest::SelectSet(index) => self.select_result_set(index, cx),
            ResultRequest::SelectCompare(index) => self.select_compare_set(index, cx),
            ResultRequest::ToggleCompareMode => self.toggle_compare_mode(cx),
            ResultRequest::ToggleTranspose => self.toggle_transpose(cx),
            ResultRequest::TransposePrev => self.transpose_step(-1, cx),
            ResultRequest::TransposeNext => self.transpose_step(1, cx),
            ResultRequest::ClearFilter => self.clear_filter_via_window(cx),
            // 【筛选条 ✕】「删除所有过滤 / 排序」（DBeaver `clearDataFilter(true)`）：
            // 同样要回填输入框，走同一条窗口转手路子
            ResultRequest::ClearFilterAndOrdering => {
                self.clear_filter_and_ordering_via_window(cx)
            }
            ResultRequest::ApplyFilter(value) => self.apply_filter_via_window(value, cx),
            // 【筛选历史】开 / 关历史表浮层：纯状态切换，面板自己 notify
            ResultRequest::SetFilterHistoryPopup(open) => self
                .result_panel
                .update(cx, |p, cx| p.set_filter_history_popup(open, cx)),
            ResultRequest::ToggleFind => self.toggle_find_via_window(cx),
            // 【结果内查找·命令语义】右键 filters 段首的「在结果中查找…」：只开不关
            ResultRequest::OpenFind => self.open_find_via_window(cx),
            ResultRequest::FindNext => self
                .result_panel
                .update(cx, |p, cx| p.find_step(true, cx)),
            ResultRequest::FindPrev => self
                .result_panel
                .update(cx, |p, cx| p.find_step(false, cx)),
            ResultRequest::CloseFind => self.result_panel.update(cx, |p, cx| p.close_find(cx)),
            ResultRequest::SetFindFilterOnly(on) => self
                .result_panel
                .update(cx, |p, cx| p.set_find_filter_only(on, cx)),
            // 【结果内查找·右键】边输边过滤能力开关（勾选框右键菜单的两项）
            ResultRequest::SetFindInteractiveFilter(enabled) => self
                .result_panel
                .update(cx, |p, cx| p.set_find_interactive_filter(enabled, cx)),
            // 【结果内查找】三颗口径开关（大小写 / 正则 / 全词）
            ResultRequest::SetFindOption(option, on) => self
                .result_panel
                .update(cx, |p, cx| p.set_find_option(option, on, cx)),
            // 【状态历史】浏览器式后退 / 前进（DBeaver `navigateHistory`，
            // `ResultSetViewer.java:4050-4063`：按该状态**重跑数据泵**）。
            // 面板只给「目标下标 + 该状态 SQL」，重查走与「筛选框运行」同一条通道
            // （`ExecTarget::Statement` + `ResultPlacement::Replace`），执行完 `sync` 会把
            // 同一份 SQL 再记一次历史——`note_state_sql` 的去重正好把位置落回那一条。
            ResultRequest::NavigateHistory(delta) => {
                let target = self.result_panel.read(cx).history_target(delta);
                if let Some((pos, sql)) = target {
                    self.result_panel
                        .update(cx, |p, cx| p.set_history_pos(pos, cx));
                    self.execute(
                        crate::execution::ExecTarget::Statement(sql),
                        crate::store::ResultPlacement::Replace,
                        cx,
                    );
                }
            }
            // 【状态历史】箭头下拉里点了一条具体状态（不限于走一格）
            ResultRequest::NavigateHistoryTo(pos) => {
                let sql = self.result_panel.read(cx).history_sql_at(pos);
                if let Some(sql) = sql {
                    self.result_panel
                        .update(cx, |p, cx| p.set_history_pos(pos, cx));
                    self.execute(
                        crate::execution::ExecTarget::Statement(sql),
                        crate::store::ResultPlacement::Replace,
                        cx,
                    );
                }
            }
            ResultRequest::PushdownFilter { value } => {
                // 值以队列快照为准（drain 时面板筛选词可能已被用户改掉）；
                // 定稿后重查走 `pushdown_filter`（读面板的 filter_text = 刚定稿的值）。
                // 用「静默定稿」：drain 正分派这条下发本身，若再走 `set_filter_value`
                // 的 apply_filter（开着下发源库）会再入队同款请求，跟 while 循环互相
                // 喂，队列无限增长（曾实测 4.4GB 分配失败）。
                self.result_panel
                    .update(cx, |p, cx| p.set_filter_value_dispatch(&value, cx));
                self.pushdown_filter(cx);
            }
            ResultRequest::CommitPending => self.commit_pending_from_ui(cx),
            ResultRequest::RollbackPending => self.discard_pending_changes(cx),
            ResultRequest::Analysis { sql } => self.run_analysis(sql, cx),
            ResultRequest::AnalysisCustom => self.request_custom_analysis_via_window(cx),
            ResultRequest::CopyTsv => {
                // 【遗留 #14】CSV 预览按模式分派：CSV 文档复制的是表格视图那份
                if self.csv_active {
                    self.copy_csv_table(cx);
                } else {
                    self.copy_active_result(cx);
                }
            }
            ResultRequest::CopySql => {
                // 复制来源 SQL（工具栏最左侧 SQL 预览点击）
                let sql = self.result_panel.read(cx).sql().unwrap_or("").to_string();
                if !sql.is_empty() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(sql));
                }
            }
            // 【2026-10-28 微调】SQL 预览：弹出窗口（kit Dialog + 只读 Textarea）。
            // `Dialog::new` 需 `&mut App`，面板 render 只有 `&App`——走窗口句柄转手，
            // 面板方法在事件路径预建 Dialog 元素 / Textarea 实体（与 BeginRename 同模式）
            ResultRequest::OpenSqlDialog => {
                let panel = self.result_panel.clone();
                let _ = self.window.update(cx, |_panel, window, app| {
                    panel.update(app, |p, cx| p.open_sql_dialog(window, cx));
                });
            }
            // 【Metadata · 生成 DDL】列浏览器页内工具条：按结果集列出建表 DDL 并弹预览浮层
            // （对齐 DBeaver `MetaDataPanel.contributeActions` 的那条命令；文本生成在面板内
            // 用纯函数算，预览需要 `&mut Window` 建 Textarea，走窗口句柄转手）。
            ResultRequest::GenerateResultSetDdl => {
                let panel = self.result_panel.clone();
                let _ = self.window.update(cx, |_panel, window, app| {
                    panel.update(app, |p, cx| p.open_ddl_dialog(window, cx));
                });
            }
            ResultRequest::CloseSqlDialog => {
                self.result_panel.update(cx, |p, cx| p.close_sql_dialog(cx));
            }
            // 【对象格浮层】开 / 关筛选框第 1 格的非模态浮层：纯状态切换
            ResultRequest::SetSqlPopup(open) => self
                .result_panel
                .update(cx, |p, cx| p.set_sql_popup(open, cx)),
            // 【1.2】筛选框运行：wrapped SQL（select * from (原SQL) where 词）替换重查，
            // 生成 SQL 写入面板 exec_sql（右栏「执行SQL」页签）
            ResultRequest::RunFilterAsQuery => {
                // 【筛选历史】▶ 执行也算一次显式提交（DBeaver 那边执行钮走同一条
                // 定稿路径），先记历史再重查
                self.result_panel
                    .update(cx, |p, _cx| p.note_filter_commit());
                self.run_filter_as_query(cx)
            }
            // 【1.2】筛选框展开 / 收起（顶宽筛选条）
            ResultRequest::ToggleFilterExpand => self
                .result_panel
                .update(cx, |p, cx| p.toggle_filter_expand(cx)),
            // 【筛选设置】打开浮层（要 &mut Window 回填 + 交焦点给排序输入框）
            ResultRequest::OpenFilterSettings => {
                let panel = self.result_panel.clone();
                let _ = self.window.update(cx, |_panel, window, app| {
                    panel.update(app, |p, cx| p.open_filter_settings(window, cx));
                });
            }
            ResultRequest::CloseFilterSettings => {
                self.result_panel
                    .update(cx, |p, cx| p.close_filter_settings(cx));
            }
            // 「应用」按钮只入队无参请求：读输入框值要 &mut Context，由面板自己读
            // 并入队带参的 `ApplyFilterSettings`
            ResultRequest::CommitFilterSettings => {
                self.result_panel
                    .update(cx, |p, cx| p.apply_filter_settings(cx));
            }
            ResultRequest::ApplyFilterSettings {
                where_text,
                order_text,
            } => self.run_custom_filter(where_text, order_text, cx),
            // 【筛选设置 · 存档】「保存 / 重置过滤器设置」：整份状态都在面板里
            // （读当前筛选词与排序，不碰宿主任何字段），宿主只转发
            ResultRequest::SaveFilterSetting => self
                .result_panel
                .update(cx, |p, cx| p.save_filter_setting(cx)),
            ResultRequest::ResetFilterSetting => self
                .result_panel
                .update(cx, |p, cx| p.reset_filter_setting(cx)),
            // 【筛选设置】切页签（列约束 / 自定义条件）：纯 UI 状态，宿主只转发
            ResultRequest::SetFilterSettingsTab(tab) => self
                .result_panel
                .update(cx, |p, cx| p.set_filter_settings_tab(tab, cx)),
            // 【筛选设置 · 列约束】页内工具条三颗（全部显示 / 全部不显示 / 重置列布局）
            ResultRequest::ColumnsShowAll => self
                .result_panel
                .update(cx, |p, cx| p.show_all_columns(cx)),
            ResultRequest::ColumnsHideAll => self
                .result_panel
                .update(cx, |p, cx| p.hide_all_columns(cx)),
            ResultRequest::ColumnsResetLayout => self
                .result_panel
                .update(cx, |p, cx| p.reset_column_layout(cx)),
            // 【筛选设置 · 列约束】列序移动四钮 + 列名排序（都作用在面板的选中列 / 全表列序）
            ResultRequest::MoveSelectedColumn(direction) => self
                .result_panel
                .update(cx, |p, cx| p.move_selected_column(direction, cx)),
            ResultRequest::SortColumnsByName => self
                .result_panel
                .update(cx, |p, cx| p.sort_columns_by_name(cx)),
            // 【工具栏配置】状态栏命令项显隐：先改面板（即时重绘），再把**面板的权威集合**
            // 序列化回全局偏好 `result_panel.hidden_status_commands`（DBeaver 的工具条配置
            // 是 workbench 级全局状态，本仓用 settings 存同一份语义）。
            ResultRequest::SetStatusCommandVisible { command, visible } => {
                self.result_panel
                    .update(cx, |p, cx| p.set_status_command_visible(command, visible, cx));
                let hidden = self
                    .result_panel
                    .read(cx)
                    .hidden_status_commands()
                    .clone();
                let text = result_panel::ports::StatusCommand::hidden_list_text(&hidden);
                settings::SettingsService::set_result_panel_hidden_status_commands(&text, cx);
            }
            // 【结果集设置】状态行齿轮：派发「打开设置并定位到结果集节」的动作
            // （对齐 DBeaver `ConfigAction` → `PrefPageResultSetMain.PAGE_ID`）。
            // 设置窗口归 workbench 所有，面板这一层只发动作（与菜单栏 / 快捷键同机制）；
            // 需要 `&mut Window`，走窗口句柄转手（headless / 窗口已关时静默跳过）。
            ResultRequest::OpenResultPanelSettings => {
                let _ = self.window.update(cx, |_panel, window, app| {
                    window.dispatch_action(
                        Box::new(settings::commands::OpenResultPanelSettings),
                        app,
                    );
                });
            }
            // 【1.2】复制「执行SQL」页签里的 wrapped SQL
            ResultRequest::CopyExecSql => {
                let sql = self
                    .result_panel
                    .read(cx)
                    .exec_sql()
                    .unwrap_or("")
                    .to_string();
                if !sql.is_empty() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(sql));
                }
            }
            // 【R1】复制值详情页当前单元格的值（工具条复制钮；无选中格时为空操作）
            ResultRequest::CopyValue => {
                let value = self
                    .result_panel
                    .read(cx)
                    .selected_value_text(cx)
                    .unwrap_or_default();
                if !value.is_empty() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(value));
                }
            }
            // 【保存值】把值详情页当前格存成文件（对齐 DBeaver「保存至文件…」）：
            // 字节与默认文件名由面板按值形态算好，这里只走「选路径 + 写盘」
            ResultRequest::SaveValue => self.save_selected_value(cx),
            ResultRequest::Refresh => self.refresh_active_result(cx),
            // 【自动刷新】刷新钮右箭头菜单：开启态/间隔记回面板（DBeaver
            // `AutoRefreshControl.enableAutoRefresh` / `runPreset` 也只改标志 + 换图标）。
            // 定时重跑在**面板侧**（`panel/events.rs::set_auto_refresh` 的代际定时任务，
            // 2026-11-25 接入）：它到点只入队 `ResultRequest::Refresh`，最终仍走下面那条
            // `Refresh => refresh_active_result`，所以宿主这里不必自己排程。
            ResultRequest::SetAutoRefresh {
                enabled,
                interval_secs,
            } => {
                self.result_panel
                    .update(cx, |p, cx| p.set_auto_refresh(enabled, interval_secs, cx));
            }
            // 【自动刷新】「自定义...」：弹面板内配置对话框（真值 `AutoRefreshConfigDialog`，
            // 两项：间隔 + 「错误时停止」）。建输入框、交焦点要 `&mut Window`，与
            // `SubmitGotoColumn` 同一条「窗口参数直传面板」的路。
            ResultRequest::ConfigureAutoRefresh => self
                .result_panel
                .update(cx, |p, cx| p.open_auto_refresh_config(window, cx)),
            // 【自动刷新】对话框里两项各自的出口：勾选点即写；「确定」读输入 + 开启
            ResultRequest::SetAutoRefreshStopOnError(on) => self
                .result_panel
                .update(cx, |p, cx| p.set_auto_refresh_stop_on_error(on, cx)),
            ResultRequest::ApplyAutoRefreshSettings => self
                .result_panel
                .update(cx, |p, cx| p.apply_auto_refresh_settings(cx)),
            ResultRequest::CloseAutoRefreshConfig => self
                .result_panel
                .update(cx, |p, cx| p.close_auto_refresh_config(cx)),
            // 【复制列名】列浏览器清单的多选集合：与 `ALT+SHIFT+C` 同一分流（单列直拷 /
            // 多列开「选项」浮层）。状态行不另报（清单自己就在用户眼前）
            ResultRequest::CopyColumnNames { names } => {
                let _ = self.result_panel.update(cx, |p, cx| {
                    p.copy_column_names_from_list(names, window, cx)
                });
            }
            // 【复制列名】「选项」对话框（真值 `ResultSetHandlerCopySpecial.CopyConfigDialog`）：
            // 「确定」读三行输入框（不需要 window，`InputState::value()` 走 `&App`）/「取消」
            ResultRequest::ApplyCopyNamesOptions => self
                .result_panel
                .update(cx, |p, cx| p.apply_copy_names_options(cx)),
            ResultRequest::CloseCopyNamesOptions => self
                .result_panel
                .update(cx, |p, cx| p.close_copy_names_options(cx)),
            // 【计算行数】状态行那颗钮：对原 SQL 跑 count(*)，数字就地回填到钮上
            ResultRequest::CountRows => self.count_result_rows(cx),
            ResultRequest::FetchMore => self.fetch_more(cx),
            // 【第 72 批 · DBeaver 对齐】取全部的**第一跳**：按钮 / `CTRL+SHIFT+=`
            // 只送到这里，由宿主判「还有没有下一段 + 有没有错」，真该取才开确认浮层
            // （DBeaver `readAllData:4430-4440`：先静默判、再 `CONFIRM_RS_FETCH_ALL`）
            ResultRequest::AskFetchAll => self.ask_fetch_all(cx),
            ResultRequest::CloseFetchAllConfirm => {
                self.result_panel
                    .update(cx, |p, cx| p.close_fetch_all_confirm(cx));
            }
            // 【第 71 批 · DBeaver 对齐】取全部行：`readAllData:4426-4459` 是
            // 「一直取到 `isHasMoreData()` 为假」。本仓执行是异步的，循环拆成
            // 发起（`fetch_all`）+ 每段回填后推进（`advance_fetch_all`，在 `pump.rs`）。
            // 第 70 批之前这里只调一次 `fetch_more`，注释承诺与实现不符，本批补齐。
            // 【第 72 批】这条现在只由确认浮层的「继续」发出：进来先收浮层（幂等）
            ResultRequest::FetchAll => {
                self.result_panel
                    .update(cx, |p, cx| p.close_fetch_all_confirm(cx));
                self.fetch_all(cx)
            }
            ResultRequest::GoPage(target) => self.go_page(target, cx),
            ResultRequest::GoRow(row) => self.go_row(row, cx),
            // 【第 75 批】跳列浮层的两个出口（render 只有 `&App`，焦点回收要 window）：
            // 提交 = 解析 + 跳 + 收浮层；关闭 = 只收浮层。键位那条不走这里
            // （`on_goto_column` 直接调 `open_goto_column`，与 `CTRL+G` 同构）。
            ResultRequest::SubmitGotoColumn => self
                .result_panel
                .update(cx, |p, cx| p.submit_goto_column(window, cx)),
            ResultRequest::CloseGotoColumn => self
                .result_panel
                .update(cx, |p, cx| p.close_goto_column(window, cx)),
            // 【P2-1】值查看器 hex 翻页：页号是面板自持的纯视图态，宿主只把请求还回面板
            ResultRequest::HexPage(page) => {
                self.result_panel
                    .update(cx, |panel, cx| panel.set_hex_page(page, cx));
            }
            ResultRequest::Export { format, all } => {
                if all {
                    self.export_active_result_all(format, cx);
                } else {
                    self.export_active_result(format, cx);
                }
            }
            ResultRequest::CopyError => self.copy_error_message(cx),
            ResultRequest::LocateError => self.jump_to_error_site(cx),
            ResultRequest::SetPushdown { open } => self.set_pushdown(open, cx),
            // 【侧栏】右侧辅助栏开合：只翻面板状态
            ResultRequest::ToggleValueSidebar => self
                .result_panel
                .update(cx, |p, cx| p.toggle_value_sidebar(cx)),
            // 【侧栏】右缘竖条顶部的面板总开关（DBeaver `panelsButton`）：纯面板视图态
            ResultRequest::SetPanelsVisible(visible) => self
                .result_panel
                .update(cx, |p, cx| p.set_panels_visible(visible, cx)),
            // 【侧栏】状态栏左端开关：左侧栏（网格 / 文本 / 记录竖条）开合
            ResultRequest::ToggleLeftSidebar => self
                .result_panel
                .update(cx, |p, cx| p.toggle_left_sidebar(cx)),
            // 【R5 可拖分栏】右栏左边缘拖拽改宽度：纯面板视图状态（不落盘、不影响取数）
            ResultRequest::ResizeRightSidebar(dx) => self
                .result_panel
                .update(cx, |p, cx| p.resize_right_sidebar(dx as f32, cx)),
            // 【R5 比例持久化】松手定稿：面板交回当前宽度（像素），写进全局偏好项
            // `result_panel.sidebar_width`（DBeaver 的 `panelRatio` 对应项，本仓按 px 存）。
            ResultRequest::EndResizeRightSidebar => {
                let width = self
                    .result_panel
                    .update(cx, |p, cx| p.end_right_sidebar_resize(cx));
                if let Some(width) = width {
                    settings::SettingsService::set_result_panel_sidebar_width(
                        width.as_f32(),
                        cx,
                    );
                }
            }
            // 【R5 布局方向切换】右栏在右侧 / 下方之间切换（DBeaver `CMD_TOGGLE_LAYOUT`）：
            // 纯面板视图状态，不落盘、不动网格数据
            ResultRequest::ToggleVerticalLayout => self
                .result_panel
                .update(cx, |p, cx| p.toggle_vertical_layout(cx)),
            // 【R5 面板最大化】收起 / 还原结果网格（DBeaver `CMD_TOGGLE_MAXIMIZE`）：
            // 纯面板视图状态，不落盘、不动网格数据
            ResultRequest::TogglePanelMaximize => self
                .result_panel
                .update(cx, |p, cx| p.toggle_panel_maximize(cx)),
            // 【R6 段大小】状态行输入框提交的「一段」行数（对齐 DBeaver `resultSetSize`）：
            // 面板视图态立即生效（下一次「取下一段」按它下发 limit，不重取已加载行），
            // 同时写回全局偏好 `result_panel.segment_rows`，下次启动沿用。
            ResultRequest::SetSegmentRows(rows) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_segment_rows(rows, cx));
                let effective = self.result_panel.read(cx).segment_rows();
                settings::SettingsService::set_result_panel_segment_rows(effective, cx);
            }
            // 【侧栏】清单行眼睛：切换该列隐藏 / 显示（网格是另一实体，
            // update 它安全；面板再 notify 重画列浏览器的隐藏标记）
            ResultRequest::ToggleColumnVisibility(column) => {
                let grid = self.grid.clone();
                grid.update(cx, |table, cx| {
                    let delegate = table.delegate_mut();
                    if delegate.is_hidden_column(column) {
                        delegate.show_column(column);
                    } else {
                        delegate.hide_column(column);
                    }
                    table.refresh(cx);
                });
                // 【列布局落盘】面板取快照（有变化才推 `ColumnLayoutChanged`）：
                // drain 循环随后会处理那条请求并落库——这里不再显式存一次，
                // 否则同一操作要写两遍会话
                self.result_panel
                    .update(cx, |panel, cx| panel.snapshot_column_layout(cx));
                self.result_panel.update(cx, |_, cx| cx.notify());
            }
            // 【筛选条 ✕ 箭头菜单】「显示全部列」（对齐 DBeaver `EraseItemListener`
            // `ResultSetViewer.java:1525-1545` 里「显示隐藏列」那一项）：委托放回全部列，
            // 再按 `ToggleColumnVisibility` 同一条路子刷新网格 + 快照落盘
            ResultRequest::ShowAllColumns => {
                let grid = self.grid.clone();
                grid.update(cx, |table, cx| {
                    table.delegate_mut().show_all_columns();
                    table.refresh(cx);
                });
                self.result_panel
                    .update(cx, |panel, cx| panel.snapshot_column_layout(cx));
                self.result_panel.update(cx, |_, cx| cx.notify());
            }
            // 【筛选条 ✕ 箭头菜单】「取消所有冻结列」（DBeaver「重置所有 pins」的对应物：
            // 逐约束 `removeOption(ATTR_OPTION_PINNED)`，`FilterResetAllPinsAction.java:34-44`）：
            // 清 delegate 的冻结集合，再照 `ShowAllColumns` 同一条路子刷新 + 快照落盘
            ResultRequest::UnfreezeAllColumns => {
                let grid = self.grid.clone();
                let changed = grid.update(cx, |table, cx| {
                    let changed = table.delegate_mut().unfreeze_all_columns();
                    if changed {
                        table.refresh(cx);
                    }
                    changed
                });
                if changed {
                    self.result_panel
                        .update(cx, |panel, cx| panel.snapshot_column_layout(cx));
                    self.result_panel.update(cx, |_, cx| cx.notify());
                }
            }
            // 【筛选条 ✕ 箭头菜单】「重置所有筛选与列设置」：对齐
            // `FilterResetAllSettingsAction.run`（`actions/FilterResetAllSettingsAction.java:44-63`）
            // ——清筛选 → 有隐藏列就显示全部列 → 有 pins 就重置 pins（颜色 / 转换器本仓没有）。
            // 清筛选这一步带输入框回填，走宿主窗口转手；列级两步直接改 delegate
            ResultRequest::ResetAllFilterSettings => {
                self.clear_filter_and_ordering_via_window(cx);
                let grid = self.grid.clone();
                grid.update(cx, |table, cx| {
                    let delegate = table.delegate_mut();
                    delegate.show_all_columns();
                    delegate.unfreeze_all_columns();
                    table.refresh(cx);
                });
                self.result_panel
                    .update(cx, |panel, cx| panel.snapshot_column_layout(cx));
                self.result_panel.update(cx, |_, cx| cx.notify());
            }
            // 【呈现】显示方式（网格 / 文本）：纯面板状态，宿主只转发。
            // 【第 60 批】切完移交焦点：文本态下网格卸载、焦点会退化到窗口根，
            // 面板级 `Ctrl+反引号` 就切不回来（目标句柄见 `presentation_focus_target`）。
            ResultRequest::SetPresentation(presentation) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_presentation(presentation, cx));
                self.focus_after_presentation_switch(window, cx);
            }
            // 【呈现·布局段】切到下一档（DBeaver `CMD_SWITCH_PRESENTATION`）：同为面板状态
            ResultRequest::CyclePresentation => {
                self.result_panel.update(cx, |p, cx| p.cycle_presentation(cx));
                self.focus_after_presentation_switch(window, cx);
            }
            // 【右栏】标签页切换（列浏览器 / 值详情 / 计算 / 分组）：纯面板状态
            ResultRequest::SetRightTab(tab) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_right_tab(tab, cx));
                // 【引用页 · 已置灰 2026-11-24】原实现在切到该页时置 `Loading` + 入队
                // `LoadReferences`（对齐 `ReferencesPanel.java:133-136` 的 `refresh(false)`）。
                // 该页当前是占位（结果集面板 §25 / Q36-A：引用行要内嵌结果集才成立），
                // 取回来也看不到，所以这里不再触发——省掉一次白跑的元数据查询。
                // 接线时把下面这段恢复即可（`begin_references_load` 与 `load_references` 都还在）：
                //   if tab == RightTab::Reference { p.begin_references_load(cx); }
            }
            // 【右栏】面板开/关语义（右缘竖条图标 / 视图菜单页项）：该页**开着且整栏可见**就
            // 关掉这一页，否则打开并切到该页。对齐 DBeaver `ResultSetHandlerTogglePanel`
            // （`CMD_TOGGLE_PANEL` → `toggleResultsPanel:64-69` → `closePanel` / `activatePanel`）。
            // 面板状态本身与 `SetRightTab` 同口径（纯面板状态，宿主只转发）；
            // 只有「真的切到了引用页」才需要读外键，与 `SetRightTab` 分支同一条刷新路径。
            ResultRequest::ToggleRightTab(tab) => {
                // toggle 是「开/关该页」（`ResultSetHandlerTogglePanel:64-81`）：关掉一页同样改
                // `open_tabs`，所以要按集合变化落盘，不能只看「是否切到了该页」。
                let before = self.result_panel.read(cx).open_tabs().to_vec();
                let switched = self
                    .result_panel
                    .update(cx, |p, cx| p.toggle_right_tab(tab, cx));
                let changed = self.result_panel.read(cx).open_tabs() != before.as_slice();
                // 【右栏页签持久化】竖条也能关掉 / 重新启用某页 → 同一次落盘
                if switched || changed {
                    self.persist_result_open_tabs(cx);
                }
                // 【引用页 · 已置灰】切到该页不触发取数（理由同 `SetRightTab` 分支）
            }
            // 【右栏】关掉这一页（其余页保留）：纯面板状态，宿主只转发。
            // 对齐 DBeaver `CTabItem(panelFolder, SWT.CLOSE)` + `enabledPanelIds` 移除语义。
            ResultRequest::CloseRightTab(tab) => {
                self.result_panel
                    .update(cx, |p, cx| p.close_right_tab(tab, cx));
                // 【右栏页签持久化】对齐 `enabledPanelIds` 的移除语义：关一次落一次盘
                self.persist_result_open_tabs(cx);
            }
            // 【页签重排】拖拽换位（对齐 DBeaver `TabFolderReorder(panelFolder)`）：
            // 只改显示顺序，顺序随 `result_panel.open_tabs` 偏好一起落盘。
            ResultRequest::MoveRightTab { from, to } => {
                self.result_panel
                    .update(cx, |p, cx| p.move_right_tab(from, to, cx));
                self.persist_result_open_tabs(cx);
            }
            // 【右栏】重新打开一页（视图菜单勾选项）：与 `SetRightTab` 同口径，
            // 切到引用页时同样要触发外键读取。
            ResultRequest::OpenRightTab(tab) => {
                self.result_panel
                    .update(cx, |p, cx| p.open_right_tab(tab, cx));
                // 【右栏页签持久化】重开一页同样落盘（与 × 的移除共用同一份集合）
                self.persist_result_open_tabs(cx);
                // 【引用页 · 已置灰】切到该页不触发取数（理由同 `SetRightTab` 分支）
            }
            // 【值详情页】在外部程序中打开当前值（临时文件 + 系统默认程序）
            ResultRequest::OpenValueExternally => {
                self.open_selected_value_externally(cx);
            }
            // 【洞察心智提醒 2026-11-24】列头右键「洞察此列」先确认再动手（拍板 Q37-A）。
            //
            // 钩子侧只有 `&mut App`（开不了对话框），所以那一按只是把请求入队到这里；
            // 这里握着 window，先把「数据会取样进分析库临时表」说清，确认后才走原路径
            // （[`Self::insight_column`] → 宿主端口 → 洞察面板）。取消 = 什么都不做。
            ResultRequest::RequestInsightColumn(column) => {
                let entity = cx.entity();
                let for_confirm = column.clone();
                dialogs::open_insight_confirm(window, cx, for_confirm.clone(), move |_window, cx| {
                    entity.update(cx, |panel, cx| panel.insight_column(&for_confirm, cx));
                });
            }
            // 【引用页】宿主侧取外键（面板只交 schema / 表 / 可见列）
            ResultRequest::LoadReferences {
                schema,
                table,
                visible_columns,
            } => self.load_references(schema, table, visible_columns, cx),
            // 【引用页】键下拉选中（纯面板状态）
            ResultRequest::SetActiveReference(index) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_active_reference(index, cx));
            }
            // 【引用页】「打开目标对象」：导航树定位目标表
            ResultRequest::OpenReferenceTarget { schema, table } => {
                self.open_reference_target(schema, table, cx)
            }
            // 【引用页】「查看引用行」：按当前行的外键值另开一份结果集（方案 B′）。
            // 落位用 `NewSet`——DBeaver 在面板内嵌一份结果视图互不干扰，
            // 本仓换成「原结果保持选中、引用行放旁边」，语义见 references-plan §5。
            ResultRequest::OpenReferenceRows { sql } => {
                self.execute(
                    crate::execution::ExecTarget::Statement(sql),
                    crate::store::ResultPlacement::NewSet,
                    cx,
                );
            }
            // 【列浏览器】清单单击选中列：面板写选中态 + 网格列高亮联动（方法内部已联动网格）
            ResultRequest::SelectColumn(column) => {
                self.result_panel
                    .update(cx, |p, cx| p.select_column(column, cx));
            }
            // 【P2-3】列浏览器多选批量显隐 / 选集镜像 / 清空选集：都是纯面板 + 网格
            // delegate 的本地状态，宿主只转发（选集权威在 delegate，面板存镜像）
            ResultRequest::SetColumnsHidden { columns, hidden } => {
                self.result_panel.update(cx, |p, cx| {
                    p.set_columns_hidden(columns, hidden, cx);
                });
            }
            ResultRequest::ColumnSelectionChanged(columns) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_multi_selected(columns, cx));
            }
            ResultRequest::ClearColumnSelection => {
                self.result_panel
                    .update(cx, |p, cx| p.clear_multi_selected(cx));
            }
            // 【列浏览器】清单双击定位：网格滚到该列首行并选中
            ResultRequest::LocateColumn(column) => {
                self.result_panel
                    .update(cx, |p, cx| p.locate_column(column, cx));
            }
            // 【列浏览器】搜索框 ✕ 清除：需要 window（`InputState::set_value` 签名），走窗口句柄转手
            ResultRequest::ClearColumnSearch => self.clear_column_search_via_window(cx),
            // 【#28-B3】行级动作：新增 / 复制 / 删除一行（只改本地叠加层，不下发 SQL）
            ResultRequest::InsertRowBelow(anchor_view) => {
                self.insert_local_row(anchor_view, cx);
            }
            ResultRequest::DuplicateRow(view) => self.duplicate_local_row(view, cx),
            ResultRequest::DeleteRow(view) => self.delete_local_row(view, cx),
            // 【第 67 批】右键「重置单元格」：两支（行已删 → 撤销删除；否则 → 清掉这一格的
            // 变更，显示回落数据原值），见 `reset_result_cell`
            ResultRequest::ResetCell { row, col } => self.reset_result_cell(row, col, cx),
            // 【第 57 批】状态栏「修改」钮：进入行内编辑（与双击同一条事件链路）
            ResultRequest::BeginEditCell => {
                self.result_panel.update(cx, |p, cx| p.begin_edit_cell(cx));
            }
            // 【第 61 批】右键「修改单元格」：编辑的是**右键落点那一格**（不是焦点格），
            // 与 DBeaver `fillEditMenu:3524-3525` 的 `CMD_ROW_EDIT_INLINE` 同义。
            ResultRequest::BeginEditCellAt { row, col } => {
                self.result_panel
                    .update(cx, |p, cx| p.begin_edit_cell_at(row, col, cx));
            }
            // 【D10-6】方向键 / Home / End 挪焦点格（DBeaver `LightGrid.java:3738-3841`）：
            // 位移与状态行 / 值详情投影都在面板里（`set_selection_pos` 要 window 驱动列浏览
            // 器选中行，面板层拿不到 window，故走 `self.window` 转手）；宿主只剩一件事——
            // 把「最近一次选的是格」换成新格，`Ctrl+C` 的复制目标读的是这份缓存。
            ResultRequest::StepFocus { step, extend } => {
                let panel = self.result_panel.clone();
                let moved = self
                    .window
                    .update(cx, |_panel, window, app| {
                        panel.update(app, |p, cx| p.step_grid_focus(step, extend, window, cx))
                    })
                    .ok()
                    .flatten();
                if let Some((row, col)) = moved {
                    self.result_selection = Some(result_grid::GridSelection::Cell { row, col });
                    cx.notify();
                }
            }
            // 【第 55 批】「从上一行 / 下一行复制」：逐格改写现有行（不是插新行）
            ResultRequest::CopyRowValuesFrom { row, above } => {
                self.copy_row_values_from_adjacent(row, above, cx);
            }
            // 组合撤销 / 重做：格级栈优先，空走行级栈（两个栈互不混栈）
            ResultRequest::UndoAction => self.undo_row_action(cx),
            ResultRequest::RedoAction => self.redo_row_action(cx),
            // 【C1】重命名第 index 份结果集标签（会话级；空白串宿主侧归一为恢复序号）
            ResultRequest::RenameSet(index, title) => {
                self.shared
                    .update_results(|store| store.set_title(&self.document, index, title));
                cx.notify();
            }
            // 【结果集 #296】列序拖拽：面板订阅里已更新 `column_order`（权威值），
            // 这里只需触发落库（与 Ctrl+S 同一条快照链路，改动即存）
            ResultRequest::ColumnOrderChanged { .. } => {
                self.save_session_now(cx);
                cx.notify();
            }
            // 【P1-3】列宽拖拽结束：面板快照已是权威值，同样只做「落库」这一件事
            ResultRequest::ColumnWidthsChanged => {
                self.save_session_now(cx);
                cx.notify();
            }
            // 【列布局落盘】隐藏 / 冻结改了：列头菜单 / 列头右键是就地改 delegate 的，
            // 面板快照还没跟上——先取快照，再落库（面板方法内部推的这条请求不会重复存）
            ResultRequest::ColumnLayoutChanged => {
                self.result_panel
                    .update(cx, |panel, cx| panel.snapshot_column_layout(cx));
                self.save_session_now(cx);
                cx.notify();
            }
            // 【筛选存档落盘】存档被保存 / 重置：面板已是权威值（链接口都从面板方法发出），
            // 宿主只落库——与 `ColumnWidthsChanged` 同款，不再额外取快照
            ResultRequest::SavedFilterChanged => {
                self.save_session_now(cx);
                cx.notify();
            }
            // 【C1】切换第 index 份结果集的钉住态（钉住不被 FIFO 淘汰）
            ResultRequest::TogglePin(index) => {
                self.shared
                    .update_results(|store| store.toggle_pin(&self.document, index));
                cx.notify();
            }
            // 【DBeaver 对齐】关闭第 index 份结果集：从 store 移除该槽位（条目 + 变更集 /
            // 历史 / 事实档一并丢弃）。只剩一份时 store 拒绝删除，界面不额外报错——
            // 关闭按钮那次点击什么也不会发生，与「至少留一份」的口径一致。
            ResultRequest::CloseSet(index) => {
                self.shared
                    .update_results(|store| store.remove_set(&self.document, index));
                cx.notify();
            }
            // 【DBeaver 对齐】标签拖拽重排：整份槽位（条目 + 伴随状态）搬到新位置，
            // 选中项跟着那份走（`store::reorder_index`）。
            ResultRequest::MoveSet { from, to } => {
                self.shared
                    .update_results(|store| store.move_set(&self.document, from, to));
                cx.notify();
            }
            // 【C1】进入重命名态（需要 window 创建/回填输入框，走窗口句柄转手）
            ResultRequest::BeginRename(index) => {
                let panel = self.result_panel.clone();
                let _ = self.window.update(cx, |_panel, window, app| {
                    panel.update(app, |p, cx| p.begin_rename(index, window, cx));
                });
            }
            // 【C1】提交 / 取消重命名（纯面板态，host 只转发）
            ResultRequest::CommitRename => {
                self.result_panel.update(cx, |p, cx| p.commit_rename(cx));
            }
            ResultRequest::CancelRename => {
                self.result_panel.update(cx, |p, cx| p.cancel_rename(cx));
            }
            // 【选中统计】数据格右键 layout 段切换三项之一：面板改本地态（状态行重画），
            // 随后宿主把三项一起落盘（与「计算页统计项」同一条分工：面板推、宿主存）
            ResultRequest::ToggleSelectionStat { kind, on } => {
                self.result_panel
                    .update(cx, |p, cx| p.toggle_selection_stat(kind, on, cx));
                self.persist_result_selection_stats(cx);
            }
            // 【筛选条显隐】数据格右键 layout 段的「显示过滤器面板」（DBeaver
            // `ToggleFilterPanelAction.run:5434-5441`：改偏好 → setVisible）：面板收掉
            // 展开态并重画，宿主随后落盘（与选中统计同一分工）。
            ResultRequest::ToggleFilterPanel { open } => {
                self.result_panel
                    .update(cx, |p, cx| p.toggle_filter_panel(open, cx));
                self.persist_result_show_filter_panel(cx);
            }
            // 【R2 计算页】函数集增删 / 复位 / 口径切换：面板本地视图状态（不影响取数），
            // 宿主只转发（与 SetRightTab 同口径）。**启用集要落盘**（对齐 DBeaver
            // `AggregateColumnsPanel.saveSettings` 记住 `enabledFunctions`）：三条改动路径
            // 生效后都按面板当前值回写一次，口径切换不落盘（DBeaver 也不存）。
            ResultRequest::AddStatsItem(item) => {
                self.result_panel
                    .update(cx, |p, cx| p.add_stats_item(item, cx));
                self.persist_result_stats_items(cx);
            }
            ResultRequest::RemoveStatsItem(item) => {
                self.result_panel
                    .update(cx, |p, cx| p.remove_stats_item(item, cx));
                self.persist_result_stats_items(cx);
            }
            ResultRequest::ResetStatsItems => {
                self.result_panel
                    .update(cx, |p, cx| p.reset_stats_items(cx));
                self.persist_result_stats_items(cx);
            }
            ResultRequest::SetStatsMode(mode) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_stats_mode(mode, cx));
            }
            // 【D11】「按列分组」开关：纯展示态，不落盘（DBeaver 存在 `PARAM_GROUP_BY_COLUMNS`，
            // 本仓沿用「口径切换不落盘」这条既有取舍，差异记在基线 §31）
            ResultRequest::SetStatsGroupBy(on) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_stats_group_by(on, cx));
            }
            // 【D12】统计行选中态：工具条「−」的作用对象，纯视图态
            ResultRequest::SelectStatsItem(item) => {
                self.result_panel
                    .update(cx, |p, cx| p.select_stats_item(item, cx));
            }
            // 【R3 分组页】分组键增删 / 清空是纯面板视图状态（不落盘、不影响取数）；
            // 【分组页持久化】排序口径与「仅重复」要落盘（对齐 DBeaver `ChangeSortingAction`
            // 写 `RS_GROUPING_DEFAULT_SORTING` + `persistConfiguration()`、`DuplicatesOnlyAction`
            // 写 `RS_GROUPING_SHOW_DUPLICATES_ONLY`），两条都按面板当前值回写一次。
            ResultRequest::ToggleGroupColumn(column) => {
                self.result_panel
                    .update(cx, |p, cx| p.toggle_group_column(column, cx));
            }
            ResultRequest::MoveGroupColumn { column, delta } => {
                self.result_panel
                    .update(cx, |p, cx| p.move_group_column(column, delta, cx));
            }
            ResultRequest::SetGroupSort(sort) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_group_sort(sort, cx));
                self.persist_result_group_sort(cx);
            }
            ResultRequest::SetGroupDuplicatesOnly(on) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_group_duplicates_only(on, cx));
                self.persist_result_group_duplicates_only(cx);
                // 【D14】对齐 DBeaver `DuplicatesOnlyAction`：改完即 `refresh(true)` 重建
                // groupingQuery（`GroupingPanel.refresh:139-157` → `rebuildGrouping`），
                // 不要求用户再点一次运行。只在已经查过、且有分组键时补一次，免得
                // 开个开关就凭空发查询（那一页还是空态时由「运行」负责首查）。
                let already_ran = self
                    .result_panel
                    .read(cx)
                    .group_query()
                    .is_some();
                if already_ran {
                    let columns = self.result_panel.read(cx).group_key_names(cx);
                    if !columns.is_empty() {
                        self.run_grouping_query(columns, on, cx);
                    }
                }
            }
            ResultRequest::ClearGroupColumns => {
                self.result_panel
                    .update(cx, |p, cx| p.clear_group_columns(cx));
            }
            // 【D14】分组页重查：生成 `GROUP BY` 查询下发，结果落分组页的内嵌结果集
            // （`ResultPlacement::Grouping`，见 `run_grouping_query`）
            ResultRequest::RunGroupingQuery {
                columns,
                duplicates_only,
            } => {
                self.run_grouping_query(columns, duplicates_only, cx);
            }
        }
    }

    /// 列浏览器搜索框清除同样需要窗口句柄（headless / 窗口已关时静默跳过；
    /// 与 [`Self::clear_filter_via_window`] 同口径）
    pub(crate) fn clear_column_search_via_window(&mut self, cx: &mut Context<Self>) {
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.clear_column_search(window, cx));
        });
    }

    /// 【计算页持久化】把面板当前的启用统计项写回全局偏好
    /// （对齐 DBeaver `AggregateColumnsPanel.saveSettings` 记 `enabledFunctions`）。
    ///
    /// 三条改动路径（＋ / 行尾 × / Reset）生效后各回写一次：面板已把值夹成合法集合，
    /// 宿主只负责读走那一串 id 交给设置服务（同值不写盘由设置侧判定）。
    pub(crate) fn persist_result_stats_items(&mut self, cx: &mut Context<Self>) {
        let ids = self.result_panel.read(cx).stats_items_ids();
        settings::SettingsService::set_result_panel_stats_items(&ids, cx);
    }

    /// 【选中统计】把面板当前的三项开关写回全局偏好
    /// （对齐 DBeaver `ToggleSelectionStatAction.run` 改 `resultset.show.selected.*`）。
    ///
    /// 三项一次写：改一个开关不该触发三次写盘；面板已把值改稳，宿主只读走交设置服务。
    pub(crate) fn persist_result_selection_stats(&mut self, cx: &mut Context<Self>) {
        let prefs = self.result_panel.read(cx).selection_stat_prefs();
        settings::SettingsService::set_result_panel_selection_stats(
            prefs.rows,
            prefs.columns,
            prefs.cells,
            cx,
        );
    }

    /// 【筛选条显隐】把面板当前的显隐写回全局偏好
    /// （对齐 DBeaver `ToggleFilterPanelAction.run:5436-5439` 改 `resultset.show.filterPanel`）。
    pub(crate) fn persist_result_show_filter_panel(&mut self, cx: &mut Context<Self>) {
        let show = self.result_panel.read(cx).show_filter_panel();
        settings::SettingsService::set_result_panel_show_filter_panel(show, cx);
    }

    /// 【分组页持久化】把面板当前的排序口径写回全局偏好
    /// （对齐 DBeaver `ChangeSortingAction.run` 写 `RS_GROUPING_DEFAULT_SORTING`）。
    pub(crate) fn persist_result_group_sort(&mut self, cx: &mut Context<Self>) {
        let id = self.result_panel.read(cx).group_sort_id();
        settings::SettingsService::set_result_panel_group_sort(id, cx);
    }

    /// 【分组页持久化】把面板当前的「仅重复」开关写回全局偏好
    /// （对齐 DBeaver `DuplicatesOnlyAction.run` 写 `RS_GROUPING_SHOW_DUPLICATES_ONLY`）。
    pub(crate) fn persist_result_group_duplicates_only(&mut self, cx: &mut Context<Self>) {
        let on = self.result_panel.read(cx).group_duplicates_only_value();
        settings::SettingsService::set_result_panel_group_duplicates_only(on, cx);
    }

    /// 【右栏页签持久化】把面板当前启用的页写回全局偏好
    /// （对齐 DBeaver `ResultSetViewer.addPanel:1388` 的 `enabledPanelIds.add(id)` 与
    /// 关闭时的移除：那边这份集合随 `PresentationSettings` 存，下次打开仍是这几页）。
    ///
    /// 两条改动路径（页签条行尾 × / 视图菜单勾选项）生效后各回写一次；面板已把集合
    /// 归成规范序，宿主只负责拼成 id 串交给设置服务（同值不写盘由设置侧判定）。
    ///
    /// 没有 `Settings` 全局时静默跳过：headless（用例里的 `TestAppContext`）不装这个
    /// 全局，而 `SettingsService::set_result_panel_open_tabs` 内部 `cx.global::<Settings>()`
    /// 会直接 panic，且它拿到全局后还会真写用户目录的 `settings.json`——用例不该有这种
    /// 副作用。生产路径 `rds-app` 启动即注册该全局，所以这一步永远走得通。同文件的
    /// 窗口转手入口（[`Self::clear_filter_via_window`]）也是「缺句柄就静默跳过」的口径。
    pub(crate) fn persist_result_open_tabs(&mut self, cx: &mut Context<Self>) {
        if !cx.has_global::<settings::model::Settings>() {
            return;
        }
        let ids = self
            .result_panel
            .read(cx)
            .open_tabs()
            .iter()
            .map(|tab| tab.id())
            .collect::<Vec<_>>()
            .join(",");
        settings::SettingsService::set_result_panel_open_tabs(&ids, cx);
    }

    /// 需要 `&mut Window` 的动作走窗口句柄转手（headless / 窗口已关时静默跳过；
    /// 测试走直接入口 [`Self::clear_filter`]）
    pub(crate) fn clear_filter_via_window(&mut self, cx: &mut Context<Self>) {
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.clear_filter(window, cx));
        });
    }

    /// 【筛选条 ✕】「删除所有过滤 / 排序」的窗口转手（同
    /// [`Self::clear_filter_via_window`] 的口径；测试走直接入口
    /// [`result_panel::ResultPanel::clear_filter_and_ordering`]）
    pub(crate) fn clear_filter_and_ordering_via_window(&mut self, cx: &mut Context<Self>) {
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.clear_filter_and_ordering(window, cx));
        });
    }

    /// 【筛选历史】选中一条历史筛选词：回填输入框 + 立即应用（同
    /// [`Self::clear_filter_via_window`] 的窗口转手口径）
    pub(crate) fn apply_filter_via_window(&mut self, value: String, cx: &mut Context<Self>) {
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, move |_panel, window, app| {
            panel.update(app, |p, cx| p.apply_filter_from_history(&value, window, cx));
        });
    }

    /// 【结果内查找】开关查找栏：打开时要把焦点交给查找框，需窗口句柄
    /// （同 [`Self::clear_filter_via_window`] 的转手口径）
    pub(crate) fn toggle_find_via_window(&mut self, cx: &mut Context<Self>) {
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.toggle_find(window, cx));
        });
    }

    /// 【结果内查找·命令语义】打开并聚焦查找栏（幂等）：与 `toggle_find_via_window`
    /// 同一条窗口转手口径，只是面板侧走 `open_find` 而不是 `toggle_find`。
    pub(crate) fn open_find_via_window(&mut self, cx: &mut Context<Self>) {
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.open_find(window, cx));
        });
    }

    /// 分析「自定义」弹输入框同样需要窗口；headless 下跳过（测试走
    /// [`Self::request_custom_analysis`] 直接入口）
    pub(crate) fn request_custom_analysis_via_window(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let _ = self.window.update(cx, move |_panel, window, app| {
            entity.update(app, |this, cx| this.request_custom_analysis(window, cx));
        });
    }
}
