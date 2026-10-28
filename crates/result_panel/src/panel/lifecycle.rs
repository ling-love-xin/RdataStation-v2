//! 生命周期域（`panel/` 第一块）：构造 / 网格注入 / 投影同步（`sync`）。

use std::cell::RefCell;

use gpui_kit::*;
use gpui_kit::component::list::ListState;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::table::{TableEvent, TableState};

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use crate::column_browser::ColumnBrowserDelegate;
use crate::grid::{self as result_grid, ResultGridDelegate, ResultStatus, ResultToolbar};
use crate::ports::{Presentation, ResultActionQueue, ResultRequest, RightTab};
/// 【类型样式】全局偏好聚合（shell 层：settings 持久化与渲染消费共用）。
use workbench_shell::result_style::TypeStylePrefs;
use super::data::{ResultPanel, ResultSyncInput};

impl ResultPanel {

    /// 构造空面板：建筛选输入框（含 300ms 防抖订阅）。
    ///
    /// 网格实体由编辑器构造期创建（钩子注入依赖 editor 动作），经 [`Self::set_grids`]
    /// 交进来；构造后、首次 sync 前面板不渲染任何结果。
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        queue: ResultActionQueue,
    ) -> Self {
        // 【B15】本地筛选框：输入变化走防抖（原型 §5.5 的 300ms），到点才重算视图行序
        let filter_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("筛选当前结果…", window, cx);
            state
        });
        let filter_sub = cx.subscribe_in(
            &filter_input,
            window,
            |this, state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    this.on_filter_input(text, cx);
                }
            },
        );
        // 订阅句柄仅持有（防重复建立）；面板实体自身不需要额外使用
        let _ = filter_sub;

        // 【DBeaver 对齐】状态栏中段行数输入框：可编辑，输入行号跳转（暂不接跳转逻辑）
        let row_count_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("200", window, cx);
            state
        });

        // 占位网格：编辑器构造真网格（含钩子）后经 `set_grids` 替换；占位仅保证
        // 构造后到接线前这段窗口里实体非空（渲染不会走到——还没 sync 过）
        let grid = result_grid::new_table_state(
            ResultGridDelegate::empty("尚未执行"),
            window,
            cx,
        );

        // 【列浏览器】清单 List（kit 组件承载：搜索框 / 键盘导航 / 滚动条内置）：
        // delegate 只持投影，数据由 `sync` 灌入；占位网格句柄先借用，`set_grids`
        // 替换网格后 `sync` 会重灌（构造后到接线前不会渲染，见 grid 占位注释）
        let browser_list = cx.new(|cx| {
            ListState::new(ColumnBrowserDelegate::new(queue.clone(), grid.clone()), window, cx)
                .searchable(true)
        });

        // 【DBeaver 对齐】订阅 table 事件：双击单元格编辑 / 列序拖拽 / 清除选择
        let _grid_sub = cx.subscribe_in(
            &grid,
            window,
            |this, _table, event: &TableEvent, _window, cx| {
                match event {
                    // 双击单元格：进入编辑态（当前走行级叠加层）
                    TableEvent::DoubleClickedCell(row_ix, col_ix) => {
                        // 选中该单元格（delegate 坐标），让行级模型知道要编辑哪格
                        this.selection_pos = Some((*row_ix, *col_ix));
                        cx.notify();
                    }
                    // 列序拖拽：记下新显示序（delegate 的 `display_order` 快照），
                    // 并推 `ColumnOrderChanged` 让宿主落库（跨会话持久化）
                    TableEvent::MoveColumn(from, to) => {
                        let _ = (from, to);
                        let order = this.grid.read(cx).delegate().display_order();
                        if order != this.column_order {
                            this.column_order = order;
                            this.queue.push(ResultRequest::ColumnOrderChanged {
                                order: this.column_order.clone(),
                            });
                        }
                        cx.notify();
                    }
                    // 清除选择：清空选中格位置
                    TableEvent::ClearSelection => {
                        this.selection_pos = None;
                        this.selected_col = None;
                        cx.notify();
                    }
                    // 其他事件忽略（SelectRow / SelectColumn / SelectCell 已由
                    // delegate 内联处理或后续接线）
                    _ => {}
                }
            },
        );

        Self {
            grid,
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
            edge_bar_visible: true,
            left_sidebar_epoch: 0,
            right_sidebar_epoch: 0,
            sql_dialog_open: false,
            sql_textarea: None,
            filter_expanded: false,
            selection_pos: None,
            // 【结果集 #296】列序默认空 = 数据序；宿主构造期注入持久化列序
            column_order: Vec::new(),
            selected_col: None,
            browser_list,
            row_count_input,
            presentation: Presentation::Grid,
            right_tab: RightTab::Browser,
            compare_active: 0,
            compare_toolbar: None,
            compare_status: None,
            result_sql: None,
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
            pushdown: false,
            renaming: None,
            renaming_input: None,
            show_column_comments: false,
            // 类型样式默认全继承 / 原始值（与渲染现状一致；host 构造时按全局偏好覆盖）
            type_style: TypeStylePrefs::default(),
        }
    }

    /// 编辑器把构造好的网格（含钩子）交进来
    pub fn set_grids(
        &mut self,
        grid: Entity<TableState<ResultGridDelegate>>,
        compare_grid: Option<Entity<TableState<ResultGridDelegate>>>,
    ) {
        self.grid = grid;
        self.compare_grid = compare_grid;
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
                    // 【Q6】类型必须**在 set_data 之后**设：`set_data` 会清空类型
                    state.delegate_mut().set_column_types(payload.column_types);
                    // 【A9】来源表名同理（set_data 会清掉旧的）
                    state.delegate_mut().set_result_table(payload.table);
                    // 【P1】字段注释同理（set_data 会清掉旧的）
                    state.delegate_mut().set_column_annotations(payload.annotations);
                    // 【B5】事实帧同理（set_data 会清掉旧的）；交互派生时才读取，render 不解析
                    state.delegate_mut().set_fact(payload.fact);
                    // 【#28】本地叠加层同理（set_data 会清掉旧的）；未接行级动作时为 None
                    state.delegate_mut().set_local_snapshot(payload.local);
                }
                None => state.delegate_mut().clear(input.empty),
            }
            // 【M8】这份结果能不能洞察（菜单项据此出现 / 消失）
            state.delegate_mut().set_insight_available(input.insight_available);
            // 【B5b】这份结果还能不能再取一段 / 是不是正在取
            state.delegate_mut().set_has_more(input.has_more);
            state.delegate_mut().set_loading_more(input.loading_more);
            // 【T2a-4/6】单元格编辑开关 + 脏格 / 冲突格底色
            state.delegate_mut().set_editable(input.editable);
            state.delegate_mut().set_dirty(input.dirty);
            state.delegate_mut().set_conflict(input.conflict);
            state.refresh(cx);
        });

        // 【列浏览器】清单数据跟随网格投影重灌（换结果集 / 刷新 / 取段都走到这里；
        // 搜索词**跨结果保留**的既有口径不变——`set_data` 只换投影，不清搜索框）。
        // List 内部选中高亮（selected_index）没有窗口句柄可在这里复位，换结果后
        // 可能残留旧行高亮，用户点击 / 搜索 / 网格反推后即纠正（记录在案）。
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
            });
        }
    }


}