//! 请求分发与窗口辅助（dispatch_result_request / drain_result_actions / clip_cell_preview）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use result_panel::ports::ResultRequest;

impl EditorHostPanel {
    /// 处理中可能**再入队**（如 `SetPushdown` 打开且有词 → 面板入队 `PushdownFilter`），
    /// 所以循环 drain 直到队列空——新请求落进下一轮，同一次事件里一并消化。
    pub(crate) fn drain_result_actions(&mut self, cx: &mut Context<Self>) {
        let mut pending = self.result_action_queue.drain();
        while !pending.is_empty() {
            for request in std::mem::take(&mut pending) {
                self.dispatch_result_request(request, cx);
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

    /// 单条结果动作的分派（语义与渲染区删除前的按钮回调一一对应）
    pub(crate) fn dispatch_result_request(&mut self, request: ResultRequest, cx: &mut Context<Self>) {
        match request {
            ResultRequest::SelectSet(index) => self.select_result_set(index, cx),
            ResultRequest::SelectCompare(index) => self.select_compare_set(index, cx),
            ResultRequest::ToggleCompareMode => self.toggle_compare_mode(cx),
            ResultRequest::ToggleTranspose => self.toggle_transpose(cx),
            ResultRequest::TransposePrev => self.transpose_step(-1, cx),
            ResultRequest::TransposeNext => self.transpose_step(1, cx),
            ResultRequest::ClearFilter => self.clear_filter_via_window(cx),
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
            ResultRequest::CommitPending => {
                if let Err(reason) = self.submit_pending_changes(cx) {
                    self.set_message(Some(reason), cx);
                }
            }
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
            ResultRequest::CloseSqlDialog => {
                self.result_panel.update(cx, |p, cx| p.close_sql_dialog(cx));
            }
            // 【1.2】筛选框运行：wrapped SQL（select * from (原SQL) where 词）替换重查，
            // 生成 SQL 写入面板 exec_sql（右栏「执行SQL」页签）
            ResultRequest::RunFilterAsQuery => self.run_filter_as_query(cx),
            // 【1.2】筛选框展开 / 收起（顶宽筛选条）
            ResultRequest::ToggleFilterExpand => {
                self.result_panel.update(cx, |p, cx| p.toggle_filter_expand(cx))
            }
            // 【1.2】复制「执行SQL」页签里的 wrapped SQL
            ResultRequest::CopyExecSql => {
                let sql = self.result_panel.read(cx).exec_sql().unwrap_or("").to_string();
                if !sql.is_empty() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(sql));
                }
            }
            ResultRequest::Refresh => self.refresh_active_result(cx),
            ResultRequest::FetchMore => self.fetch_more(cx),
            ResultRequest::FetchAll => {
                // 【DBeaver 对齐】取全部行：循环 fetch_more 直到没有下一段
                // （当前 fetch_more 是单次取下一段，FetchAll 是循环取全量；
                //  实现方式：调用 fetch_more 的循环版本，直到 has_more = false）
                self.fetch_more(cx);
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
            ResultRequest::ToggleValueSidebar => {
                self.result_panel.update(cx, |p, cx| p.toggle_value_sidebar(cx))
            }
            // 【侧栏】状态栏左端开关：左侧栏（网格 / 文本 / 记录竖条）开合
            ResultRequest::ToggleLeftSidebar => {
                self.result_panel.update(cx, |p, cx| p.toggle_left_sidebar(cx))
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
                self.result_panel.update(cx, |_, cx| cx.notify());
            }
            // 【呈现】显示方式（网格 / 文本）：纯面板状态，宿主只转发
            ResultRequest::SetPresentation(presentation) => {
                self.result_panel
                    .update(cx, |p, cx| p.set_presentation(presentation, cx));
            }
            // 【右栏】标签页切换（列浏览器 / 值详情 / 计算 / 分组）：纯面板状态
            ResultRequest::SetRightTab(tab) => {
                self.result_panel.update(cx, |p, cx| p.set_right_tab(tab, cx));
            }
            // 【列浏览器】清单单击选中列：面板写选中态 + 网格列高亮联动（方法内部已联动网格）
            ResultRequest::SelectColumn(column) => {
                self.result_panel
                    .update(cx, |p, cx| p.select_column(column, cx));
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
            // 【C1】切换第 index 份结果集的钉住态（钉住不被 FIFO 淘汰）
            ResultRequest::TogglePin(index) => {
                self.shared
                    .update_results(|store| store.toggle_pin(&self.document, index));
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
            // 【B1】列头直接显示注释（面板级临时开关）
            ResultRequest::SetShowColumnComments { open } => {
                self.result_panel.update(cx, |p, cx| p.set_show_column_comments(open, cx));
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

    /// 需要 `&mut Window` 的动作走窗口句柄转手（headless / 窗口已关时静默跳过；
    /// 测试走直接入口 [`Self::clear_filter`]）
    pub(crate) fn clear_filter_via_window(&mut self, cx: &mut Context<Self>) {
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.clear_filter(window, cx));
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
