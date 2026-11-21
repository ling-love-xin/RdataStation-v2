//! 生命周期域（`panel/` 第一块）：构造 / 网格注入 / 投影同步（`sync`）。

use std::cell::RefCell;

use gpui_kit::base::TreeState;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::list::ListState;
use gpui_kit::component::table::{TableEvent, TableState};
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use super::data::{ResultPanel, ResultSyncInput};
/// 【筛选设置 · 存档】套用判据（纯函数，事件域定义、投影路径复用）
use super::events::saved_filter_applies;
use crate::column_browser::ColumnBrowserDelegate;
use crate::grid::{self as result_grid, ResultGridDelegate, ResultStatus, ResultToolbar};
use crate::ports::{Presentation, ResultActionQueue, ResultRequest, RightTab};
/// 【类型样式】全局偏好聚合（shell 层：settings 持久化与渲染消费共用）。
use workbench_shell::result_style::TypeStylePrefs;

impl ResultPanel {
    /// 构造空面板：建筛选输入框（含 300ms 防抖订阅）。
    ///
    /// 网格实体由编辑器构造期创建（钩子注入依赖 editor 动作），经 [`Self::set_grids`]
    /// 交进来；构造后、首次 sync 前面板不渲染任何结果。
    pub fn new(window: &mut Window, cx: &mut Context<Self>, queue: ResultActionQueue) -> Self {
        // 【B15】本地筛选框：输入变化走防抖（原型 §5.5 的 300ms），到点才重算视图行序
        //
        // 句柄必须存进 `Self`：原来写成 `let filter_sub = …; let _ = filter_sub;`，
        // `Subscription` 是「掉了就解订」的守卫，`let _ =` 等于当场丢弃 ⇒
        // `InputEvent::Change` 从不到达 `on_filter_input`，边打边筛一直没生效
        // （2026-11-18 与网格订阅同一个坑，由 `typing_into_the_filter_box_reaches_the_panel` 钉住）。
        let filter_input = cx.new(|cx| {
            // 【P3】多行控件：DBeaver 的筛选输入是 `TextViewer(SWT.MULTI)`
            // （`ResultSetFilterPanel.java:192`），常态 1 行高、展开 5 行
            // （`togglePanelExpand` 的 `gd.heightHint = getLineHeight() * (expanded ? 5 : 1)`，
            // `:1320-1327`）。本仓对应组件是 `Textarea`，高度由渲染处的 `.h()` 决定
            // （多行分支 `input.rs:794-797` 走 `h_auto()` + `height`）。
            //
            // `submit_on_enter(true)`：多行控件的回车默认插入换行（`state.rs:1618` 的
            // `is_multi_line && !submit_on_enter` 分支），但筛选词是单行语义，换行会让
            // 拼出的 WHERE 带 `\n`。关掉后回车只发 Submit 事件，本仓未订阅该事件，
            // 交互与改造前的单行框一致。
            let mut state = TextareaState::new(window, cx);
            // 【空提示文案】对齐 DBeaver `ResultSetFilterPanel.java:219-223` 的
            // `UIUtils.addEmptyTextHint(filtersText, …)`：支持数据过滤时写
            // `sql_editor_resultset_filter_panel_text_enter_sql_to_filter`
            // （zh `ResultSetMessages_zh.properties:671` =
            // 「输入一个 SQL 表达式来过滤结果 (使用 Ctrl+Space)」），不支持时写
            // 「不支持数据过滤」。本仓本地过滤恒可用，只取第一支。
            //
            // **尾随的「(使用 Ctrl+Space)」截掉**：那半句指 DBeaver 的内容辅助
            // （`ContentAssistUtils.installContentProposalWithPreferences`，`:304-308`
            // 装列名 / 函数补全）。本仓筛选框没有补全能力，照抄等于承诺一个按不出来的
            // 快捷键；按「能力没有就不说」只留前半句。此前写的是「筛选当前结果…」，
            // 既没对齐文案，也没说清这一格接受的是 **SQL 表达式**（`rewrite_with_where`
            // 直接把它拼进 `where`）。
            state.set_placeholder("输入一个 SQL 表达式来过滤结果", window, cx);
            state.set_submit_on_enter(true, cx);
            state
        });
        let _filter_sub = Some(cx.subscribe_in(
            &filter_input,
            window,
            |this, state, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    let text = state.read(cx).value().to_string();
                    this.on_filter_input(text, cx);
                }
                // 【筛选历史】Enter = 提交（DBeaver `setCustomDataFilter()`，
                // `ResultSetFilterPanel.java:296`；展开态里 Enter 是换行，由
                // `commit_filter` 自己判掉）
                InputEvent::PressEnter { .. } => this.commit_filter(window, cx),
                _ => {}
            },
        ));

        // 【筛选设置】自定义排序输入（浮层内唯一的输入源；Enter = 直接应用）
        let filter_order_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("自定义排序（如 created_at DESC, id）", window, cx);
            state
        });

        // 【结果内查找】查找词输入框 + 订阅（Enter = 下一处，与 DBeaver 查找浮层一致：
        // `FindReplaceOverlay` 的文本框回车走 findNext）
        let find_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("在结果里查找…", window, cx);
            state
        });
        let _find_sub = Some(cx.subscribe_in(
            &find_input,
            window,
            |this, state, event: &InputEvent, _window, cx| match event {
                InputEvent::Change => {
                    let text = state.read(cx).value().to_string();
                    this.find_query_changed(text, cx);
                }
                InputEvent::PressEnter { .. } => this.find_step(true, cx),
                _ => {}
            },
        ));

        // 【R6 段大小】状态行「一段」行数输入框（对齐 DBeaver `resultSetSize` Text，
        // `ResultSetViewer.java:2126-2152`：tooltip「Result-set fetch size」，`>0` 才生效）。
        // 占位即默认值；有效输入把输入框规范化成该值（≈ 失焦回填），
        // 无效输入则不动作，等**失焦**时按当前生效值回填（那边的 `focusLost` 行为）。
        let segment_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder(
                crate::ui::RESULT_SEGMENT_ROWS_DEFAULT.to_string(),
                window,
                cx,
            );
            state
        });
        let segment_sub = cx.subscribe_in(
            &segment_input,
            window,
            |this, state, event: &InputEvent, window, cx| {
                // 【失焦回填】对齐 `ResultSetViewer.java:2129-2137`：文本框内容与生效值不同就改写。
                // 作用在「敲了非法值（`0` / `abc` / 空）却不回车」这种路径上——
                // 否则状态行会一直留着一段不生效的文本。headless 不派发该事件（见 events 侧纯函数）。
                if matches!(event, InputEvent::Blur) {
                    let text = state.read(cx).value().to_string();
                    if let Some(display) =
                        super::events::segment_rows_blur_display(&text, this.segment_rows)
                    {
                        state.update(cx, |s, cx| s.set_value(display, window, cx));
                    }
                    return;
                }
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = state.read(cx).value().to_string();
                let Some(rows) = super::events::parse_segment_rows(&text) else {
                    return;
                };
                // 规范化（`007` → `7`）：DBeaver 在失焦时回填当前值，这里在有效提交时即回填
                state.update(cx, |s, cx| s.set_value(rows.to_string(), window, cx));
                if rows == this.segment_rows {
                    return;
                }
                this.queue.push(ResultRequest::SetSegmentRows(rows));
                cx.notify();
            },
        );

        // 【DBeaver 对齐】状态栏中段行数输入框：可编辑，输入行号跳转到该行
        let row_count_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("200", window, cx);
            state
        });
        // 【P1-2】行号跳转订阅：`InputState::set_value` 程序化回填**不发**事件
        // （宿主 sync 时每帧刷新行数文案不会把自己再触发一遍），所以只有用户真敲键
        // 才入队；非数字 / 0 静默忽略（中段一直在编辑，边敲边报错太吵）。
        let row_count_sub = cx.subscribe_in(
            &row_count_input,
            window,
            |this, state, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let text = state.read(cx).value().to_string();
                let Ok(row) = text.trim().parse::<usize>() else {
                    return;
                };
                if row == 0 {
                    return;
                }
                this.queue.push(ResultRequest::GoRow(row));
                cx.notify();
            },
        );

        // 占位网格：编辑器构造真网格（含钩子）后经 `set_grids` 替换；占位仅保证
        // 构造后到接线前这段窗口里实体非空（渲染不会走到——还没 sync 过）
        let grid = result_grid::new_table_state(ResultGridDelegate::empty("尚未执行"), window, cx);

        // 【值查看器】JSON 展开树状态（`tree::Tree` 组件承载）：选中格变化时事件路径
        // `rebuild_value_tree` 解析 + `set_items` 重建；render 只摆 Tree，不解析 JSON
        let json_tree = cx.new(|cx| TreeState::new(cx));

        // 【列浏览器】清单 List（kit 组件承载：搜索框 / 键盘导航 / 滚动条内置）：
        // delegate 只持投影，数据由 `sync` 灌入；占位网格句柄先借用，`set_grids`
        // 替换网格后 `sync` 会重灌（构造后到接线前不会渲染，见 grid 占位注释）
        let browser_list = cx.new(|cx| {
            ListState::new(
                ColumnBrowserDelegate::new(queue.clone(), grid.clone()),
                window,
                cx,
            )
            .searchable(true)
        });

        // 【网格事件订阅】构造时先挂在占位网格上（`set_grids` 换网格后由它重挂）。
        // 句柄必须存进 `Self`：只在 `new` 里 `let _sub` 会在构造返回时解订，
        // 整套事件处理（选格 / 编辑 / 列序 / 列宽 / 清除 / 整行选中）静默失效
        // ——2026-11-18 由 `select_row_event_mirrors_into_the_delegate` 实测确认过。
        let _grid_sub = Some(cx.subscribe_in(
            &grid,
            window,
            |this, _table, event: &TableEvent, window, cx| {
                this.handle_grid_event(event, window, cx);
            },
        ));

        Self {
            grid,
            json_tree,
            compare_grid: None,
            queue,
            tabs: Vec::new(),
            tabs_scroll: ScrollHandle::default(),
            active: 0,
            notice: None,
            toolbar: None,
            status: None,
            error_card: None,
            script_card: None,
            transposed: false,
            transpose_row: 0,
            compare_mode: false,
            show_left_sidebar: true,
            show_value_sidebar: true,
            // 【工具栏配置】默认隐藏集合 = DBeaver 的三项 `defaultVisibility="false"`
            // （undo / redo / fetch.page），其余默认显示
            hidden_status_commands: crate::ports::StatusCommand::ALL
                .into_iter()
                .filter(|command| !command.default_visible())
                .collect(),
            // 【选中统计】出厂三项全关（对齐 DBeaver `DataEditorsPreferencesInitializer:86-88`）；
            // 宿主构造期用 `set_selection_stat_prefs` 注入用户偏好
            selection_stat_prefs: crate::grid::SelectionStatPrefs::default(),
            edge_bar_visible: true,
            left_sidebar_epoch: 0,
            right_sidebar_epoch: 0,
            sql_dialog_open: false,
            sql_textarea: None,
            sql_dialog_title: None,
            filter_expanded: false,
            // 【筛选条显隐】出厂显示（对齐 `DataEditorsPreferencesInitializer.java:67`）；
            // 宿主构造期用 `set_show_filter_panel` 注入用户偏好
            show_filter_panel: true,
            selection_pos: None,
            // 【P2-1】值查看器 hex 页号（纯视图态，换选中格复位）
            hex_page: 0,
            // 【P2-2】值查看器图片（事件路径解码后放这里；render 只摆）
            value_image: None,
            // 【结果集 #296】列序默认空 = 数据序；宿主构造期注入持久化列序
            column_order: Vec::new(),
            // 【P1-3】列宽快照默认空 = 没拖过；宿主恢复会话时注入
            column_widths: Vec::new(),
            // 【列布局落盘】隐藏 / 冻结快照默认空 = 没改过；宿主恢复会话时注入
            column_hidden: Vec::new(),
            column_frozen: Vec::new(),
            selected_col: None,
            // 【P2-3】多选集合镜像（delegate 经 `ColumnSelectionChanged` 回传）
            multi_selected: Vec::new(),
            browser_list,
            row_count_input,
            _row_count_sub: Some(row_count_sub),
            segment_input,
            _segment_sub: Some(segment_sub),
            // 【网格事件订阅】构造期挂在占位网格上，`set_grids` 换网格后重挂
            _grid_sub,
            // 【B15】筛选框订阅（边打边筛的唯一入口）
            _filter_sub,
            // 【结果内查找】查找栏状态（会话级；`find_at` 存视图列（含 `#` 槽））
            find_open: false,
            find_input,
            _find_sub,
            find_query: String::new(),
            find_hits: 0,
            find_at: None,
            find_filter_only: false,
            find_criteria: crate::ports::FindCriteria::default(),
            // 【筛选设置】浮层默认关闭、排序草稿为空（空 = 沿用原查询 ORDER BY）
            filter_settings_open: false,
            filter_order_input,
            filter_order: String::new(),
            // 【筛选设置】默认停在「列约束」页（DBeaver 对话框先建列页并 setSelection(0)）
            filter_settings_tab: crate::ports::FilterSettingsTab::Columns,
            // 【筛选设置 · 存档】没存过就没有；待回填的词同理（首次 sync 才可能产生）
            saved_filter: None,
            filter_backfill: None,
            presentation: Presentation::Grid,
            right_tab: RightTab::Browser,
            // 出厂 = 六页全开（DBeaver `activateDefaultPanels` 会补默认面板；
            // 本仓的六页一直可用，初值取规范序全集）
            open_tabs: super::events::right_tabs().to_vec(),
            // 【引用页】清单未读（切到该页签时宿主才去取外键）、没有选中键
            references: crate::references::ReferencesState::NotLoaded,
            active_reference: None,
            // 【R2 计算页】默认全集（与 DBeaver `loadDefaultFunctions` 同口径：默认函数全上）
            stats_items: crate::ports::StatsItem::DEFAULT_SET.to_vec(),
            stats_mode: crate::ports::StatsMode::Numeric,
            // 【R3 分组页】默认不额外选列（只用当前选中列）、按值序、不筛重复
            group_columns: Vec::new(),
            group_sort: crate::ports::GroupSort::Value,
            group_duplicates_only: false,
            // 【R5 可拖分栏】右栏宽度默认档（拖拽后可改，开合动画以它为满宽）
            right_sidebar_width: px(crate::ui::RESULT_SIDEBAR_RIGHT_WIDTH * 16.0),
            right_sidebar_dragging: false,
            // 【R5 布局方向切换】默认横排（网格左 / 右栏右），与 DBeaver `verticalLayout = false` 一致
            vertical_layout: false,
            // 【R5 面板最大化】默认不最大化（网格正常显示）
            panel_maximized: false,
            // 【计算行数】还没算过总行数（钮显示图标 + 「计算行数」）
            row_count_note: None,
            // 【R6 段大小】默认一段 1000 行（与编辑器 `execution::SEGMENT_ROWS` 同值，
            // 面板接管后编辑器按本值下发 limit）
            segment_rows: crate::ui::RESULT_SEGMENT_ROWS_DEFAULT,
            right_sidebar_height: px(crate::ui::RESULT_SIDEBAR_RIGHT_HEIGHT * 16.0),
            compare_active: 0,
            compare_toolbar: None,
            compare_status: None,
            result_sql: None,
            state_history: Vec::new(),
            history_pos: 0,
            exec_sql: None,
            result_is_analysis: false,
            result_can_copy: false,
            fetching_more: false,
            export_ctx: None,
            analysis_items: Vec::new(),
            pending_changes: 0,
            committing: false,
            can_undo: false,
            can_redo: false,
            editable: false,
            filter_input,
            filter_text: String::new(),
            filter_version: 0,
            filter_debounce: RefCell::new(None),
            filter_history: Vec::new(),
            pushdown: false,
            renaming: None,
            renaming_input: None,
            show_column_comments: false,
            // 类型样式默认全继承 / 原始值（与渲染现状一致；host 构造时按全局偏好覆盖）
            type_style: TypeStylePrefs::default(),
        }
    }

    /// 网格事件统一处理（构造时挂在占位网格；`set_grids` 换网格后重挂同一个处理）。
    ///
    /// 【DBeaver 对齐】双击单元格编辑 / 列序拖拽 / 清除选择 / 列宽拖拽 / 整行选中。
    /// 组件的 emit 是**延迟**派发的（gpui-fast `Context::emit` 只把 `Effect::Emit`
    /// 排进 `pending_effects`，`context.rs:738-753`），所以处理里再 `grid.update`
    /// 不会撞「实体正被更新」。
    fn handle_grid_event(
        &mut self,
        event: &TableEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            // 单击选格：值详情跟随点选（此前只接双击——「点击文字半天才出来」
            // 的根因之一）+ 重建 JSON 展开树（选中值是 JSON/数组时）
            TableEvent::SelectCell(row_ix, col_ix) => {
                self.set_selection_pos(Some((*row_ix, *col_ix)), window, cx);
                self.rebuild_value_tree(window, cx);
                cx.notify();
            }
            // 双击单元格：进入编辑态（当前走行级叠加层）
            TableEvent::DoubleClickedCell(row_ix, col_ix) => {
                // 选中该单元格（delegate 坐标），让行级模型知道要编辑哪格
                self.selection_pos = Some((*row_ix, *col_ix));
                self.rebuild_value_tree(window, cx);
                cx.notify();
            }
            // 列序拖拽：记下新显示序（delegate 的 `display_order` 快照），
            // 并推 `ColumnOrderChanged` 让宿主落库（跨会话持久化）
            TableEvent::MoveColumn(from, to) => {
                let _ = (from, to);
                let order = self.grid.read(cx).delegate().display_order();
                if order != self.column_order {
                    self.column_order = order;
                    self.queue.push(ResultRequest::ColumnOrderChanged {
                        order: self.column_order.clone(),
                    });
                }
                cx.notify();
            }
            // 【选中行】整行选中：把组件报来的行号镜像进 delegate，`render_tr`
            // 据此整行铺色（组件自己不给选中行画色，见 delegate 字段注释）。
            // 编辑器宿主对它注入的网格另有一条自己的订阅（`host/construct.rs`
            // 同款镜像），两处写同一个值、幂等。
            TableEvent::SelectRow(row) => {
                self.grid.update(cx, |state, cx| {
                    state.delegate_mut().set_selected_row(Some(*row));
                    cx.notify();
                });
            }
            // 清除选择：清空选中格位置 + 清空 JSON 树 + 撤掉整行色
            TableEvent::ClearSelection => {
                self.selection_pos = None;
                self.selected_col = None;
                self.grid.update(cx, |state, cx| {
                    state.delegate_mut().set_selected_row(None);
                    cx.notify();
                });
                self.rebuild_value_tree(window, cx);
                cx.notify();
            }
            // 【P1-3】列宽拖拽结束：先把宽度记进 delegate（宿主那份订阅同样会记，
            // 同值幂等；这样不依赖两个订阅者的先后顺序），再取面板快照并推
            // 「该存了」——宿主从 `panel.column_widths()` 读值落会话
            TableEvent::ColumnWidthsChanged(widths) => {
                let widths = widths.clone();
                self.grid.update(cx, |state, _cx| {
                    state.delegate_mut().set_column_widths(&widths)
                });
                let snapshot = self.grid.read(cx).delegate().column_width_snapshot();
                if snapshot != self.column_widths {
                    self.column_widths = snapshot;
                    self.queue.push(ResultRequest::ColumnWidthsChanged);
                }
            }
            // 其他事件忽略（SelectColumn 由组件内联处理，面板不跟）
            _ => {}
        }
    }

    /// 编辑器把构造好的网格（含钩子）交进来
    ///
    /// 【订阅重挂】面板在 `new` 里订阅的是**构造期的占位网格**；换网格后旧订阅指向旧实体，
    /// 不重挂的话这里列出的全部事件处理在新网格上都不触发（列序落库 / 列宽快照 / 整行选中
    /// 都会静默失效）。所以换网格的同时把订阅换到新网格上——这也是本方法要 window 的原因。
    pub fn set_grids(
        &mut self,
        grid: Entity<TableState<ResultGridDelegate>>,
        compare_grid: Option<Entity<TableState<ResultGridDelegate>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.grid = grid;
        self.compare_grid = compare_grid;
        // 【列布局落盘】把动作队列注入两个网格：列头菜单 / 列头右键的隐藏、冻结是在
        // UI 回调里直接改 delegate 的，改完经队列交回宿主——宿主先取面板快照再存会话。
        let queue = self.queue.clone();
        let grid = self.grid.clone();
        grid.update(cx, |state, _cx| {
            state.delegate_mut().set_action_queue(queue.clone())
        });
        if let Some(compare) = self.compare_grid.clone() {
            compare.update(cx, |state, _cx| state.delegate_mut().set_action_queue(queue));
        }
        // 【订阅重挂】把网格事件订阅换到新网格上（旧句柄随赋值解订，不留两条）
        let grid = self.grid.clone();
        self._grid_sub = Some(cx.subscribe_in(
            &grid,
            window,
            |this, _table, event: &TableEvent, window, cx| {
                this.handle_grid_event(event, window, cx);
            },
        ));
    }

    /// 对比栏投影（编辑器 `sync_compare_grid` 时调用；与主栏投影同口径，只读）
    pub fn set_compare_projection(
        &mut self,
        active: usize,
        toolbar: Option<ResultToolbar>,
        status: Option<ResultStatus>,
    ) {
        self.compare_active = active;
        self.compare_toolbar = toolbar;
        self.compare_status = status;
    }

    /// 动作队列（编辑器订阅处理时读；渲染期不读）
    pub fn queue(&self) -> &ResultActionQueue {
        &self.queue
    }

    /// 投影：把编辑器算好的判据写进面板状态 + 主网格（事件路径调用，render 是纯读）
    pub fn sync(&mut self, cx: &mut Context<Self>, input: ResultSyncInput) {
        // 【P2-4】换结果 = 换一批行列：转置视图自动退出（delegate 侧已由 `set_data`
        // 清掉，这里把面板开关同步复位——`toggled` 与网格状态保持同一份真值）
        self.transposed = false;
        self.transpose_row = 0;
        // 【列浏览器】换结果集：选中列与清单高亮复位（渲染兜底第一可见列）；
        // 搜索词沿用全局筛选框同口径跨结果保留，由搜索框自身 ✕ / ESC 清空
        self.selected_col = None;

        self.tabs = input.tabs;
        self.active = input.active;
        self.notice = input.notice;
        self.toolbar = input.toolbar;
        self.status = input.status;
        self.error_card = input.error_card;
        self.script_card = input.script_card;
        self.result_sql = input.sql;
        // 【计算行数】旧数字只对旧结果有意义：换进来一份新结果就清掉
        self.row_count_note = None;
        // 【状态历史】每份同步进来的结果 SQL 记一条状态（去重 / 截断前向由它自己管）
        self.note_state_sql();
        self.result_can_copy = input.can_copy;
        self.result_is_analysis = input.is_analysis;
        self.export_ctx = input.export_ctx;
        self.analysis_items = input.analysis_items;
        self.pending_changes = input.pending_changes;
        self.committing = input.committing;
        self.can_undo = input.can_undo;
        self.can_redo = input.can_redo;
        self.editable = input.editable;
        self.fetching_more = input.loading_more;

        // 【B15】筛选统计的真值在网格那边（视图行序是它算的）——写入 toolbar 前先回填
        self.refresh_filter_hint(cx);

        self.grid.update(cx, |state, cx| {
            match input.grid {
                Some(payload) => {
                    // 【结果集 #296】先按持久化列名序重排（空 = 默认序，原样直通）
                    let (columns, rows) = self.apply_column_order(payload.columns, payload.rows);
                    state.delegate_mut().set_data(columns, rows);
                    // 【P1-3】持久化列宽**在 set_data（清空列宽表）之后**回填：按列名对，
                    // 对不上的列跳过（结果换了表 / 列名变了就回默认档）
                    let widths = self.column_widths.clone();
                    state.delegate_mut().apply_column_width_snapshot(&widths);
                    // 【列布局落盘】隐藏 / 冻结同理：必须排在 set_data 之后（它清空两张清单），
                    // 也排在列宽之后（隐藏会改变可见列序，后续步骤不依赖它，顺序仅需一致）
                    let hidden = self.column_hidden.clone();
                    state.delegate_mut().apply_hidden_columns(&hidden);
                    let frozen = self.column_frozen.clone();
                    state.delegate_mut().apply_frozen_columns(&frozen);
                    // 【Q6】类型必须**在 set_data 之后**设：`set_data` 会清空类型
                    state.delegate_mut().set_column_types(payload.column_types);
                    // 【A9】来源表名同理（set_data 会清掉旧的）
                    state.delegate_mut().set_result_table(payload.table);
                    // 【P1】字段注释同理（set_data 会清掉旧的）
                    state
                        .delegate_mut()
                        .set_column_annotations(payload.annotations);
                    // 【B5】事实帧同理（set_data 会清掉旧的）；交互派生时才读取，render 不解析
                    state.delegate_mut().set_fact(payload.fact);
                    // 【#28】本地叠加层同理（set_data 会清掉旧的）；未接行级动作时为 None
                    state.delegate_mut().set_local_snapshot(payload.local);
                }
                None => state.delegate_mut().clear(input.empty),
            }
            // 【M8】这份结果能不能洞察（菜单项据此出现 / 消失）
            state
                .delegate_mut()
                .set_insight_available(input.insight_available);
            // 【B5b】这份结果还能不能再取一段 / 是不是正在取
            state.delegate_mut().set_has_more(input.has_more);
            state.delegate_mut().set_loading_more(input.loading_more);
            // 【T2a-4/6】单元格编辑开关 + 脏格 / 冲突格底色
            state.delegate_mut().set_editable(input.editable);
            state.delegate_mut().set_dirty(input.dirty);
            state.delegate_mut().set_conflict(input.conflict);
            state.refresh(cx);
        });

        // 【筛选设置 · 存档】换进来的是**存档同一条 SQL** 的结果、且当前没有筛选词时，
        // 自动套用存档——对齐 DBeaver：打开该表的数据编辑器就 `restoreDataFilter`
        // （`ResultSetViewer.java:4266-4284`；只认 `hasFilters()` 的存档，本仓对应
        // 「存档里的筛选词非空」）。三条边界：
        // - `apply_filter_inner(cx, false)` 不 requeue：自动套用**不能**顺手触发「下发源库
        //   重查」，否则每开一次结果就多跑一次数据库；
        // 只恢复**筛选词**；存档里的自定义排序不自动套用——排序必须重查数据库才有意义，
        // 那是用户的显式动作（打开筛选设置 → 应用），自动套用等于替用户下发查询。
        if let Some(saved) = self.saved_filter.clone() {
            if saved_filter_applies(&saved, self.result_sql.as_deref(), &self.filter_text) {
                self.filter_text = saved.filter.clone();
                self.apply_filter_inner(cx, false);
                self.filter_backfill = Some(saved.filter);
            }
        }

        // 【列浏览器】清单数据跟随网格投影重灌（换结果集 / 刷新 / 取段都走到这里；
        // 搜索词**跨结果保留**的既有口径不变——`set_data` 只换投影，不清搜索框）。
        // List 内部选中高亮（selected_index）没有窗口句柄可在这里复位，换结果后
        // 可能残留旧行高亮，用户点击 / 搜索 / 网格反推后即纠正（记录在案）。
        // 【P2-3】多选集合镜像：换投影后先剔除越界列号（旧结果集的同号列不能继续命中），
        // 再把面板镜像回灌给 delegate——`set_data` 会清空 delegate 侧集合，二者要重新对齐，
        // 否则刷新一次（同一份结果重灌）就把用户的多选丢了。
        let mirrored_selection = {
            let column_count = self.grid.read(cx).delegate().columns().len();
            self.multi_selected.retain(|c| *c < column_count);
            self.multi_selected.clone()
        };
        {
            let grid = self.grid.clone();
            let browser = self.browser_list.clone();
            browser.update(cx, |list, cx| {
                // 先换真实网格再灌投影：构造期 delegate 持有的是占位网格（面板建好
                // 表状态前 ListState 就要先建），右键动作（冻结/隐藏/显示全部/清排序）
                // 直接 update 网格，不换会落在空数据上（功能回归）
                list.delegate_mut().set_grid(grid.clone());
                let delegate = grid.read(&*cx).delegate();
                let columns = delegate.columns().to_vec();
                let type_names: Vec<String> = delegate
                    .column_types()
                    .iter()
                    .map(|t| t.as_ref().map(|t| t.name.clone()).unwrap_or_default())
                    .collect();
                let hidden: Vec<bool> = (0..columns.len())
                    .map(|c| delegate.is_hidden_column(c))
                    .collect();
                let comments: Vec<Option<String>> = delegate
                    .column_annotations()
                    .iter()
                    .map(|a| a.as_ref().and_then(|a| a.comment.clone()))
                    .collect();
                list.delegate_mut()
                    .set_data(columns, type_names, hidden, comments);
                list.delegate_mut()
                    .set_selected_columns(mirrored_selection);
            });
        }
    }
}
