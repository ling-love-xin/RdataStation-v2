//! 编辑器宿主面板 —— 结果域（诊断访问器/sync_csv_grid/sync_result_view/sync_compare_grid/dispatch_result_request/apply_error_marks/jump_to_error_site）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。

use result_panel::panel::{ExportCtx, GridPayload, ResultPanel, ResultSyncInput};
use result_panel::ports::ResultRequest;

use gpui_kit::base::input::{Diagnostic, DiagnosticSeverity, Position};
// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法


use crate::channel::{self};
use crate::diagnostics;
use crate::execution::{self, ExecTarget};
use crate::export::{self, ExportFormat, ExportScope};
use crate::shared::InsightColumnRequest;
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）
use crate::store::{Pushdown, ResultEntry, ResultPlacement};
use crate::view::dialogs;
use crate::view::results::error_card::ErrorCard;
use crate::view::results::grid::{
    self as result_grid, ResultGridDelegate, ResultStatus, ResultToolbar,
};
use crate::view::results::sets::{self as result_sets};


use super::*;
impl EditorHostPanel {

    /// 【B6】编辑内核里的诊断条数（错误回填画了没有）
    pub fn diagnostics_for_test(&self, cx: &App) -> usize {
        self.editor
            .read(cx)
            .diagnostics()
            .map(|set| set.len())
            .unwrap_or(0)
    }

    /// 结果工具栏（⑥）的左段文案（供测试断言；`None` = 还没有结论）
    ///
    /// 就是 `ResultToolbar::segments()` 拼起来的那串，原型 §2.4 的 `行数 1,204 │ 耗时 1.2s │ orders`。
    /// Dock 化后状态在面板实体，经只读访问器转发。
    pub fn result_summary_for_test(&self, cx: &App) -> Option<String> {
        self.result_panel
            .read(cx)
            .toolbar()
            .map(|toolbar| toolbar.segments().join(" · "))
    }

    /// 结果工具栏（⑥）的真实输入（供测试断言各段与显隐）
    pub fn result_toolbar_for_test(&self, cx: &App) -> Option<ResultToolbar> {
        self.result_panel.read(cx).toolbar().cloned()
    }

    /// 结果状态行（⑦）的真实输入（供测试断言总行数 / 截断提示）
    pub fn result_status_for_test(&self, cx: &App) -> Option<ResultStatus> {
        self.result_panel.read(cx).status().cloned()
    }

    /// 测试用：当前呈现方式（网格 / 文本）——验证左竖条按钮真实点击链路
    pub fn presentation_for_test(&self, cx: &App) -> result_panel::ports::Presentation {
        self.result_panel.read(cx).presentation()
    }

    /// 当前选中结果集的可重跑 SQL / 可复制（供测试断言按钮显隐）
    pub fn result_actions_for_test(&self, cx: &App) -> (Option<String>, bool) {
        let panel = self.result_panel.read(cx);
        (panel.sql().map(str::to_string), panel.can_copy())
    }

    /// 【B9 切片二】补全弹层状态（供测试断言“真的弹了”、查询词与候选）
    ///
    /// 读的是内核的 `completion_menu_state()`（`#[doc(hidden)] pub`）——**不是我们的副本**：
    /// 断言的就是内核眼里“弹层开着”这件事。
    pub fn completion_menu_for_test(&self, cx: &App) -> (bool, String, Vec<String>) {
        let state = self.editor.read(cx);
        let menu = state.completion_menu_state();
        (
            menu.open,
            menu.query.clone(),
            menu.items.iter().map(|item| item.label.clone()).collect(),
        )
    }

    /// 【B6】错误卡片的内容（供测试断言摘要与定位文案）
    pub fn result_error_card_for_test(&self, cx: &App) -> Option<ErrorCard> {
        self.result_panel.read(cx).error_card().cloned()
    }

    /// 【2b】脚本输出卡片（供测试断言脚本执行的结果形态；`None` = 还没有结论）
    pub fn script_card_for_test(
        &self,
        _cx: &App,
    ) -> Option<crate::view::results::script_card::ScriptCard> {
        // 脚本卡真值留在宿主（回执写入；sync 经输入透传面板），这里直接读宿主
        self.script_card.clone()
    }

    /// 【B5】选中网格里的某一行（供测试断言状态行⑦的“已选第 N 行”；真机上点行就是这条路）
    pub fn select_grid_row_for_test(&mut self, row: usize, cx: &mut Context<Self>) {
        self.grid
            .update(cx, |state, cx| state.set_selected_row(row, cx));
    }

    /// 结果集标签（供测试断言“批量跑出三个结果集”与标签文案）
    pub fn result_tabs_for_test(&self, cx: &App) -> Vec<(String, bool)> {
        self.result_panel
            .read(cx)
            .tabs()
            .iter()
            .map(|tab| (tab.label.clone(), tab.failed))
            .collect()
    }

    /// 【B13】结果集标签的通道徽标（供测试断言“每份结果记住了自己是哪档跑的”）
    pub fn result_badges_for_test(&self, cx: &App) -> Vec<String> {
        self.result_panel.read(cx).tabs().iter().map(|tab| tab.badge()).collect()
    }

    /// 【B13】结果区顶部那一行提示（切通道后旧结果那一条）
    pub fn result_notice_for_test(&self, cx: &App) -> Option<String> {
        self.result_panel.read(cx).notice().map(str::to_string)
    }

    /// 当前选中的结果集下标（供测试断言点标签真的切了）
    pub fn result_active_for_test(&self, cx: &App) -> usize {
        self.result_panel.read(cx).active()
    }

    /// 还有几条语句没回填（供测试断言“批量执行至少知道自己在跑什么”）
    pub fn pending_for_test(&self) -> usize {
        self.pending
    }

    /// 结果网格当前行数（供测试断言网格真的拿到了数据）
    pub fn grid_row_count_for_test(&self, cx: &App) -> usize {
        // 【B15】看的是**视图行数**：筛选生效时网格里就是命中那些行
        self.grid.read(cx).delegate().visible_row_count()
    }

    /// 【并排对比】对比模式开关（供测试断言开关真的翻转）
    pub fn compare_mode_for_test(&self, cx: &App) -> bool {
        self.result_panel.read(cx).compare_mode()
    }

    /// 【并排对比】对比栏网格当前行数（供测试断言右栏真的拿到了数据）
    pub fn compare_grid_rows_for_test(&self, cx: &App) -> usize {
        self.compare_grid
            .as_ref()
            .map(|grid| grid.read(cx).delegate().visible_row_count())
            .unwrap_or(0)
    }

    /// 网格实体（测试用：滚动到底自动加载得真在表格上调 `load_more` 才算验到链路）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn grid_for_test(&self) -> Entity<TableState<ResultGridDelegate>> {
        self.grid.clone()
    }

    /// 结果面板实体（测试用：筛选防抖等面板内行为直接驱动面板方法；生产不经此路径）
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn result_panel_for_test(&self) -> Entity<ResultPanel> {
        self.result_panel.clone()
    }

    /// 结果面板实体（编辑器宿主内嵌：`v_resizable` 分栏渲染活动文档的结果区；2026-10-27 起
    /// 不再挂全局 dock 薄壳，此入口供编辑器内部与测试取面板实体）
    pub fn result_panel(&self) -> Entity<ResultPanel> {
        self.result_panel.clone()
    }

    /// 【类型样式】全局偏好即时生效入口：读当前 Settings 的 `result_panel.type_style()`
    /// 推给结果面板（面板再推 delegate 重绘）。workbench 设置页变更钩子
    /// （`on_settings_changed` 遍历 `editor_hosts`）调用；不随 `set_data` 清掉。
    pub fn apply_result_style(&self, cx: &mut Context<Self>) {
        if let Some(settings) = cx.try_global::<settings::model::Settings>() {
            let prefs = settings.result_panel.type_style();
            self.result_panel.update(cx, |p, cx| p.set_type_style(prefs, cx));
        }
    }

    /// 把光标放到指定位移（供测试断言“执行的是光标所在那句”；键位路径仍走真按键）
    pub fn set_caret_for_test(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.set_selected_range(offset..offset, cx));
    }

    /// 设置选区（供测试断言“有选区只格式化选区”；键位路径仍走真按键）
    pub fn set_selection_for_test(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |state, cx| state.set_selected_range(range, cx));
    }

    /// 【T2c-3 收尾】光标数（多光标 = 列编辑；经 vendor 公开的 `cursor_count` 读内核真实值）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cursor_count_for_test(&self, cx: &App) -> usize {
        self.editor.read(cx).cursor_count()
    }

    /// 【T2c-3 收尾】给活动光标加一个下方光标（列编辑多光标；调内核公开的 `add_cursor_below_line`）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn add_cursor_below_for_test(&mut self, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.add_cursor_below_line(cx));
    }

    /// 【T2c-3 收尾】直调内核公开的 `select_columnar_block` 框选一块（列编辑鼠标框选）——
    /// 同 Alt+Shift 拖拽产出的矩形选区，免鼠标捕获，供窗口用例验证框选路径真到了内核。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn select_columnar_block_for_test(
        &mut self,
        start_offset: usize,
        end_offset: usize,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |state, cx| {
            state.select_columnar_block(start_offset, end_offset, cx)
        });
    }

    /// 【T2c-3 收尾】所有光标字节偏移（活动在前；经 vendor 公开的 `cursor_offsets` 读内核真实值）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn cursor_offsets_for_test(&self, cx: &App) -> Vec<usize> {
        self.editor.read(cx).cursor_offsets()
    }

    /// 【T2c-3 收尾】当前编辑模式（行 / 列）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn edit_mode_for_test(&self) -> EditMode {
        self.edit_mode
    }

    /// 【T2c-3 收尾】设置编辑模式（走生产代码同一条 `set_edit_mode`）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn set_edit_mode_for_test(&mut self, mode: EditMode, cx: &mut Context<Self>) {
        self.set_edit_mode(mode, cx);
    }

    /// 【T2c-3 收尾】按当前模式对齐内核光标（走生产代码同一条 `enforce_edit_mode`）
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn enforce_edit_mode_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.enforce_edit_mode(window, cx);
    }

    /// 按「当前选中的结果集」刷新结果区（网格数据 + 状态行 + 标签条）
    ///
    /// **结果区唯一的读点**：结果集可能因一次执行回填、点标签、批量推进而变，但界面只从
    /// 这里读一次——否则会出现“网格还是上一份、摘要已经换了”的错位（多结果集之后这种错位
    /// 很难靠看界面发现）。
    /// 【遗留 #14】CSV 表格展示：把编辑器文本解析成表格填进结果网格
    ///
    /// 只对 CSV 文档生效（扩展名判定，口径与状态栏文件类型一致）；非 CSV 文档
    /// 把 `csv_active` 复位——**不动网格**，那是 SQL 执行结果的区域。
    /// 表头取第一行（CSV 常见约定）；列类型未知，全部 `None`（不做类型推断）。
    pub fn sync_csv_grid(&mut self, cx: &mut Context<Self>) {
        let is_csv = self
            .document_path()
            .as_deref()
            .and_then(crate::mode::script_file_type)
            == Some(crate::mode::ScriptFileType::Csv);
        if !is_csv {
            self.csv_active = false;
            return;
        }
        let text = self.editor_text(cx);
        let rows = engine::csv::parse(&text);
        let (columns, body) = match rows.split_first() {
            Some((header, rest)) => (header.clone(), rest.to_vec()),
            None => (Vec::new(), Vec::new()),
        };
        let column_count = columns.len();
        self.grid.update(cx, |state, cx| {
            state.delegate_mut().set_data(columns, body);
            // 类型必须**在 set_data 之后**设（set_data 会清空类型，防串台）
            state.delegate_mut().set_column_types(vec![None; column_count]);
            state.refresh(cx);
        });
        self.csv_active = true;
    }

    /// 【遗留 #14】CSV 表格展示：复制整个 CSV 表格（TSV，与结果集复制同口径）
    pub fn copy_csv_table(&mut self, cx: &mut Context<Self>) {
        let (headers, rows) = self.grid.read(cx).dump(cx);
        if headers.is_empty() {
            self.set_message(Some("CSV 没有内容可复制".to_string()), cx);
            return;
        }
        let mut out = String::new();
        out.push_str(&headers.join("\t"));
        for row in &rows {
            out.push('\n');
            out.push_str(&row.join("\t"));
        }
        cx.write_to_clipboard(ClipboardItem::new_string(out));
        self.set_message(Some(format!("已复制 {} 行（TSV）", rows.len())), cx);
    }

    pub(crate) fn sync_result_view(&mut self, cx: &mut Context<Self>) {
        // 先把要用的数据从权威存储里拷出来（不把 `Ref` 带进下面的 `grid.update`）
        let current_channel = self.channel();        let (
            tabs,
            active,
            grid_data,
            empty,
            toolbar,
            status,
            failure,
            extra,
            extra_analysis,
            notice,
            insight_available,
        ) = {
            let store = self.shared.results();
            let active = store.active_index(&self.document).unwrap_or(0);
            let entry = store.active(&self.document);
            // 【B5】当前槽位事实档快照：帧句柄随网格投影进 delegate（列式统计 / 原生数值排序
            // 交互派生时读取）；档位 + 估算字节进工具栏载体段。表档帧 batches 已释放，
            // 这里只给句柄（schema / 表名 / 行数可读），数值取数在 delegate 侧回退矩阵。
            let fact_state = store.fact_at(&self.document, active);
            let fact_snapshot = fact_state.and_then(|state| state.fact().cloned());
            let fact_tier = fact_state.map(|state| state.tier());
            let fact_bytes = fact_state.map_or(0, |state| state.estimated_bytes());
            // 【#28】本地叠加层快照（本地行 + 删除标记；行级动作未接时槽位为空集合）
            let local_snapshot = store.local_rows(&self.document, active).cloned();
            tracing::info!(
                target = "rds_editor::exec",
                doc = %self.document,
                active,
                has_entry = entry.is_some(),
                has_grid = entry.as_ref().is_some_and(|e| e.has_grid()),
                failed = entry.as_ref().is_some_and(|e| e.failed()),
                cols = entry.as_ref().map(|e| e.columns.len()).unwrap_or(0),
                rows = entry.as_ref().map(|e| e.rows.len()).unwrap_or(0),
                channel = ?current_channel,
                entry_channel = ?entry.as_ref().map(|e| e.channel),
                "结果投影：当前选中结果集形状"
            );
            let connection = entry.and_then(|entry| {
                entry
                    .connection
                    .as_deref()
                    .map(|id| self.shared.connection_status_text(Some(id)))
            });
            (
                result_sets::tabs(store.sets(&self.document), current_channel, store.pinned(&self.document)),
                active,
                entry.filter(|entry| entry.has_grid()).map(|entry| {
                    (
                        entry.columns.clone(),
                        entry.rows.clone(),
                        // 【Q6】列类型跟数据一起交给网格
                        entry.column_types.clone(),
                        // 【A9】来源表名（右键「为 INSERT」的目标；猜不到 = None → 不摆那一项）
                        crate::export::table_from_sql(&entry.sql),
                        // 【P1】字段注释（表头 tooltip；与列同序，空 vec = 没报）
                        entry.column_annotations.clone(),
                        // 【B5】事实帧只读句柄（与列 / 行同一次投影；无事实档 = None）
                        fact_snapshot.clone(),
                        // 【#28】本地叠加层快照（与列 / 行同一次投影；空集合 = 纯底层结果集）
                        local_snapshot.clone(),
                    )
                }),
                entry
                    .filter(|entry| !entry.has_grid())
                    .map(empty_text)
                    .unwrap_or_default(),
                // 【B5】工具栏（⑥）：行数 / 影响行数 / 失败 / 耗时 / 连接名
                entry.map(|entry| ResultToolbar {
                    rows: entry.has_grid().then(|| entry.row_count()),
                    affected_rows: entry.affected_rows,
                    failed: entry.failed(),
                    elapsed_ms: Some(entry.elapsed_ms),
                    connection: connection.clone(),
                    // 【B15】来源摘要：有自定义标题的（执行计划 / 分析）用标题，其余用血缘
                    lineage: entry.title.clone().or_else(|| entry.lineage.clone()),
                    // 【B15】筛选统计随后由 `refresh_filter_hint` 从网格真值填上
                    filtered: None,
                    // 【B5】事实档载体段（内存档报列式占用；表档只标载体；无事实档不摆）
                    fact_tier,
                    fact_bytes,
                    // 来源 SQL 全文（工具栏最左侧 SQL 按钮点击显示）
                    sql: Some(entry.sql.clone()),
                }),
                // 【B5】状态行（⑦）：只有网格才给（写语句的“共 0 行”、失败时的“共 0 行”都是噪音）
                entry
                    .filter(|entry| entry.has_grid())
                    .map(|entry| ResultStatus {
                        // 【#28-B3】状态行总行数口径 = 底层未删行 + 本地行（被删底层行不计入；
                        // 本地行始终纳入）。删除标记只从计数里剔除，不参与下推 / 统计链。
                        total_rows: entry.row_count()
                            - local_snapshot.as_ref().map_or(0, |local| local.deleted_count())
                            + local_snapshot.as_ref().map_or(0, |local| local.local_count()),
                        // 选中行在渲染时现读（点行不改结果集，没必要每帧回写状态）
                        selected_row: None,
                        // sync 时没有格选中：当前格值由 `SelectCell` 事件增量更新（见 grid_sub）
                        selected_cell: None,
                        truncated_hint: entry
                            .truncated
                            .then(|| result_grid::truncated_hint(entry.row_count())),
                        has_more: entry.can_fetch_more(),
                    }),
                // 【B6】失败才谈得上定位：把「哪条 SQL + 什么错误」一起带出去
                entry.and_then(|entry| {
                    entry
                        .error
                        .as_ref()
                        .map(|error| (entry.sql.clone(), error.clone()))
                }),
                entry.map(|entry| (entry.sql.clone(), entry.has_grid())),
                // 【B15】这份是不是本地分析的产物（工具栏据此不摆“刷新”）
                entry.is_some_and(|entry| entry.analysis),
                // 【B13】选中这份来自别的通道 → 顶部一行说清“旧结果来自 X”（原型 §5.7 规则 2）；
                // 同一档就不提示（切换本身已经写在工具栏与状态栏上了）
                entry
                    .filter(|entry| entry.channel != current_channel)
                    .map(|entry| channel::stale_notice(entry.channel, current_channel)),
                // 【M8】「洞察此列」能不能给：条目本身（成功 / 有列 / 只读）+ 连接解析得出来
                entry.is_some_and(|entry| self.insight_available(entry)),
            )
        };

        // 【B5b】滚动到底自动加载的两个真值要先取出来（`status` 接着就被移动了）
        let has_more = status.as_ref().is_some_and(|status| status.has_more);
        let loading_more = self.fetching_more;
        let (sql, can_copy) = extra.unwrap_or_default();
        let sql = Some(sql).filter(|sql| !sql.trim().is_empty());
        // 【T2a-3/4/6】这份结果能不能编辑（入口级判据）+ 脏格 / 冲突格集合（投影自权威变更集）
        //
        // 冲突格与脏格分开投影：冲突格（提交没写进去）不是待提交变更，底色用 danger，
        // 网格里 `set_conflict` 与 `set_dirty` 分开标，渲染时互不覆盖。
        let (editable, dirty, conflict) = {
            let store = self.shared.results();
            let editable = store.active(&self.document).is_some_and(|entry| {
                crate::editability::result_editable(
                    self.channel(),
                    crate::project::ProjectState {
                        read_only: self.shared.project_read_only(),
                    },
                    self.read_only_flags(),
                    entry.row_key().as_ref(),
                )
                .is_ok()
            });
            let change_set = store.change_set(&self.document, active);
            let dirty = change_set
                .map(|changes| {
                    changes
                        .pending()
                        .into_iter()
                        .map(|change| (change.row, change.column))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let conflict = change_set
                .map(|changes| changes.conflicts())
                .unwrap_or_default();
            (editable, dirty, conflict)
        };

        // 【B6】定位：结论没变就不重算（要拿整篇文档做匹配，而这里会被反复调用）
        let site = match &failure {
            Some((sql, error)) => {
                let key = (sql.clone(), error.clone());
                if self.error_site_key.as_ref() == Some(&key) {
                    self.error_site.clone()
                } else {
                    let document = self.editor_text(cx);
                    let site = diagnostics::site_in_document(&document, sql, error);
                    self.error_site_key = Some(key);
                    site
                }
            }
            None => {
                self.error_site_key = None;
                None
            }
        };
        self.error_site = site;
        let reason = failure.map(|(_, error)| error);
        // 【B6】错误卡片的两个真值：驱动原话 + “定位到第 N 行”（认不出位置就不给后者）
        let error_card = reason.as_ref().map(|error| ErrorCard {
            message: error.clone(),
            location: self.error_site.as_ref().map(|site| site.location_text()),
        });
        self.apply_error_marks(reason.clone(), cx);

        // 失败原因同时进状态栏（结果区可能被滚出视野）；选中成功的那份则清掉旧提示。
        // 【B6】能定位的失败把位置一并说出来（“第 3 行 第 15 列”——用户能直接去那儿看）
        let message = reason.map(|error| match &self.error_site {
            Some(site) => format!("{error}（{}）", site.location_text()),
            None => error,
        });
        self.set_message(message, cx);

        // 【T2a-7】批量提交/回滚对（渲染期计算迁到事件路径：`render` 是纯读路径）
        let pending_changes = self
            .shared
            .results()
            .change_set(&self.document, active)
            .map(|changes| changes.pending_count())
            .unwrap_or(0);
        let committing = self.commit_pending();
        // 【#28-B3】组合撤销 / 重做栈状态：格级 `EditHistory` 与行级 `RowHistory` 任一非空即可撤销；
        // 分派时同样格级优先、空走行级（见 [`Self::undo_row_action`]）。借用收窄在块内，
        // 避免 Ref 活到函数末尾挡住后续 `self` 可变借用。
        let (can_undo, can_redo) = {
            let store = self.shared.results();
            let cell_undo = store
                .history(&self.document, active)
                .is_some_and(|history| history.can_undo());
            let cell_redo = store
                .history(&self.document, active)
                .is_some_and(|history| history.can_redo());
            let row_undo = store
                .row_history(&self.document, active)
                .is_some_and(|history| history.can_undo());
            let row_redo = store
                .row_history(&self.document, active)
                .is_some_and(|history| history.can_redo());
            (cell_undo || row_undo, cell_redo || row_redo)
        };
        // 【B15】分析：对选中那份结果的**已抓行**跑聚合 SQL（本地 DuckDB，不碰源库）。
        // 菜单项由 `analysis::menu_items` 给（纯函数，可逐项断言）；没网格就不摆。
        let analysis_items = self
            .shared
            .results_active(&self.document)
            .map(|entry| crate::analysis::menu_items(&entry))
            .unwrap_or_default();
        // 【B7】导出上下文：连接 + SQL/序号 + 已抓行文案 + 还有没有下一段 + 键列。
        // 键列查询（端口读导航 L2 缓存）在 sync 期算好，render 期纯净。
        let export_ctx = can_copy.then(|| {
            let conn = self.bound_connection();
            let entry_sql = self
                .shared
                .results_active(&self.document)
                .map(|entry| (entry.sql.clone(), active + 1));
            let rows_text = status
                .as_ref()
                .map(|status| result_grid::thousands(status.total_rows))
                .unwrap_or_else(|| "0".to_string());
            let has_more = status.as_ref().is_some_and(|status| status.has_more);
            let key = entry_sql.as_ref().and_then(|(sql, index)| {
                let table = export::default_table_name(Some(sql), *index);
                conn.as_deref()
                    .and_then(|conn| self.shared.table_key_columns(Some(conn), &table))
                    .and_then(|columns| export::RowKey::new(columns))
            });
            ExportCtx {
                connection: conn,
                entry_sql,
                rows_text,
                has_more,
                key,
            }
        });

        // Dock 化投影：全部真值组装成 `ResultSyncInput` 交给面板实体（渲染 / 状态都在面板侧；
        // 这里不再写宿主结果字段——宿主只留 网格 / 选中事件 / 诊断 等动作侧状态）
        let input = ResultSyncInput {
            tabs,
            active,
            notice,
            toolbar,
            status,
            grid: grid_data.map(
                |(columns, rows, column_types, table, annotations, fact, local)| GridPayload {
                    columns,
                    rows,
                    column_types,
                    table,
                    annotations,
                    fact,
                    local,
                },
            ),
            empty,
            error_card,
            // 脚本卡真值留在宿主（回执写入）；sync 全量覆写面板该字段，必须透传
            script_card: self.script_card.clone(),
            sql,
            can_copy,
            is_analysis: extra_analysis,
            editable,
            dirty,
            conflict,
            has_more,
            loading_more,
            insight_available,
            pending_changes,
            committing,
            can_undo,
            can_redo,
            analysis_items,
            export_ctx,
        };
        self.result_panel.update(cx, |p, cx| p.sync(cx, input));

        // 【并排对比】结果变化（替换 / 追加 / 选中切换）后，对比栏网格跟着同步
        self.sync_compare_grid(cx);
    }

    /// 【并排对比】把对比选中的那份结果拷进对比栏网格（只读对照：不接编辑 / 取段 /
    /// 筛选 / 洞察——那些动作都作用于主栏，对比栏只负责把数据摆出来比）
    fn sync_compare_grid(&mut self, cx: &mut Context<Self>) {
        let Some(grid) = self.compare_grid.clone() else {
            return;
        };
        let payload = {
            let store = self.shared.results();
            store.compare_entry(&self.document).map(|entry| {
                (
                    entry.has_grid(),
                    entry.columns.clone(),
                    entry.rows.clone(),
                    entry.column_types.clone(),
                    entry.column_annotations.clone(),
                )
            })
        };
        grid.update(cx, |state, cx| {
            match payload {
                Some((true, columns, rows, types, annotations)) => {
                    state.delegate_mut().set_data(columns, rows);
                    state.delegate_mut().set_column_types(types);
                    state.delegate_mut().set_column_annotations(annotations);
                    // 对比栏只读：类型照摆（表头要看），但编辑 / 取段 / 洞察的语义
                    // 都归主栏，这里一律不给
                    state.delegate_mut().set_editable(false);
                    state.delegate_mut().set_has_more(false);
                    state.delegate_mut().set_loading_more(false);
                    state.delegate_mut().set_insight_available(false);
                }
                // 对比条目还没有网格数据（未执行 / 已失效）：空态提示，不假装有内容
                Some((false, _, _, _, _)) => state.delegate_mut().clear("未选择对比结果"),
                None => state.delegate_mut().clear("未选择对比结果"),
            }
            state.refresh(cx);
        });
        // 【并排对比】对比选中的下标同步给面板（右栏标签选中态；对比栏不摆独立
        // toolbar / status——只读对照，数据与选中态在网格 + 标签上）
        let compare_active = self
            .shared
            .results()
            .compare_index(&self.document)
            .unwrap_or(0);
        self.result_panel
            .update(cx, |p, _| p.set_compare_projection(compare_active, None, None));
    }

    /// 【并排对比】工具栏开关：开 = 结果区双栏并排（左主选中 / 右对比选中）
    ///
    /// 开启时若还没有对比对象，自动选「除主选中外最近一份」——用户开开关就想看到
    /// 两个东西并排，不让它开在一个空右栏上。关闭则右栏消失、对比选中清掉
    /// （下次开对比重新选）。
    ///
    /// Dock 化后开关状态归面板实体（`ResultPanel::compare_mode`）；这里的后续
    /// （默认对比对象 / 数据同步 / 清理）由编辑器接着做。
    pub fn toggle_compare_mode(&mut self, cx: &mut Context<Self>) {
        self.result_panel.update(cx, |p, cx| p.toggle_compare_mode(cx));
        let compare_mode = self.result_panel.read(cx).compare_mode();
        if compare_mode {
            let needs_default = {
                let store = self.shared.results();
                store.compare_index(&self.document).is_none()
                    && store.set_count(&self.document) >= 2
            };
            if needs_default {
                // fallback 计算包进块：store 的只读 Ref 在块内 drop，
                // 下面的 update_results（borrow_mut）才不会撞 borrow
                let fallback = {
                    let store = self.shared.results();
                    let active = store.active_index(&self.document).unwrap_or(0);
                    let sets = store.sets(&self.document);
                    (0..sets.len()).rev().find(|index| *index != active)
                };
                if let Some(index) = fallback {
                    self.shared.update_results(|store| {
                        store.select_compare(&self.document, index);
                    });
                }
            }
            // 对比模式开着就得有第二个网格（构造期已建好；数据同步走 sync_compare_grid）
            self.sync_compare_grid(cx);
        } else {
            // 关闭对比：右栏不画了，对比选中一并清掉
            let index = self.shared.results().compare_index(&self.document);
            if let Some(index) = index {
                self.shared.update_results(|store| {
                    store.toggle_compare(&self.document, index);
                });
            }
        }
        cx.notify();
    }

    /// 【并排对比】右栏标签条点选：换对比对象（点当前对比项 = 取消对比）
    pub fn select_compare_set(&mut self, index: usize, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .update_results(|store| store.toggle_compare(&self.document, index));
        if changed {
            self.sync_compare_grid(cx);
            cx.notify();
        }
    }

    /// 【Dock 化 §8.4】取走在途的结果面板动作并按变体分派到编辑器动作方法
    ///
    /// 消费方 = 结果面板按钮（[`ResultActionQueue::request`]：入队即 notify 面板，面板不反向依赖
    /// 编辑器）+ 宿主 `observe_in(&result_panel)` 回调（面板 notify 时本函数被调用）。
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
    fn dispatch_result_request(&mut self, request: ResultRequest, cx: &mut Context<Self>) {
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
    fn clear_column_search_via_window(&mut self, cx: &mut Context<Self>) {
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.clear_column_search(window, cx));
        });
    }

    /// 需要 `&mut Window` 的动作走窗口句柄转手（headless / 窗口已关时静默跳过；
    /// 测试走直接入口 [`Self::clear_filter`]）
    fn clear_filter_via_window(&mut self, cx: &mut Context<Self>) {
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.clear_filter(window, cx));
        });
    }

    /// 分析「自定义」弹输入框同样需要窗口；headless 下跳过（测试走
    /// [`Self::request_custom_analysis`] 直接入口）
    fn request_custom_analysis_via_window(&mut self, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let _ = self.window.update(cx, move |_panel, window, app| {
            entity.update(app, |this, cx| this.request_custom_analysis(window, cx));
        });
    }

    /// 【B6】把出错范围画进编辑内核（行内高亮 + 悬停弹层；没有位置就只清旧的）
    ///
    /// 【B18】它同时负责**三个来源的合并**：执行错误回填 + 打字期词法诊断 + LSP 语言服务
    /// 诊断。三源必须走同一条重建流程：内核的诊断集合只有 `clear` + `push`，分开各写一份
    /// 就会互相擦掉（“执行出错了，词法诊断没了”那种抳不到的怪象）。
    pub(crate) fn apply_error_marks(&mut self, reason: Option<String>, cx: &mut Context<Self>) {
        self.error_reason = reason;
        let site = self.error_site.clone();
        let reason = self.error_reason.clone();
        let lexical = self.scan_diagnostics.clone();
        let lsp_diags = self.lsp_diagnostics.clone();
        self.editor.update(cx, |state, cx| {
            // 文本先取：`diagnostics_mut` 借的就是同一份状态，两个借用不能同时活着
            let text = state.value().to_string();
            let Some(diagnostics) = state.diagnostics_mut() else {
                return;
            };
            diagnostics.clear();
            // 打字期诊断（词法层能证明的那些；来源标“语法”以与执行错误区分）
            for one in &lexical {
                let ((start_line, start_column), (end_line, end_column)) = one.positions(&text);
                diagnostics.push(
                    Diagnostic::new(
                        Position::new(start_line as u32, start_column as u32)
                            ..Position::new(end_line as u32, end_column as u32),
                        one.message.clone(),
                    )
                    .with_severity(DiagnosticSeverity::Error)
                    .with_source("语法")
                    .with_code(one.code),
                );
            }
            // LSP 语言服务诊断（内核有 `From<lsp_types::Diagnostic>`，source 一并带过来）
            for one in &lsp_diags {
                diagnostics.push(Diagnostic::from(one.clone()));
            }
            if let (Some(site), Some(reason)) = (site, reason) {
                let (end_line, end_column) = site.end();
                let range = Position::new(
                    site.line.saturating_sub(1) as u32,
                    site.column.saturating_sub(1) as u32,
                )
                    ..Position::new(
                        end_line.saturating_sub(1) as u32,
                        end_column.saturating_sub(1) as u32,
                    );
                diagnostics.push(
                    Diagnostic::new(range, reason)
                        .with_severity(DiagnosticSeverity::Error)
                        .with_source("执行"),
                );
            }
            cx.notify();
        });
    }

    /// 【B6】把光标送到出错处并聚焦
    ///
    /// 回填发生在没有窗口的轮询里，所以借构造时存下的窗口句柄。窗口已经没了、或者这次更新
    /// 落在另一个窗口更新里面（headless 测试就是这样）时，这一步只是没做成——不 panic：
    /// 位置已经画在诊断与状态栏里了。
    pub(crate) fn jump_to_error_site(&mut self, cx: &mut Context<Self>) {
        let Some(site) = self.error_site.clone() else {
            return;
        };
        let range = site.range();
        let editor = self.editor.clone();
        let _ = self.window.update(cx, move |_view, window, app| {
            editor.update(app, |state, cx| {
                state.set_selected_range(range, cx);
                state.focus(window, cx);
            });
        });
    }

    /// 跳转的**落地入口**：手上有窗口时直接做（错误卡片上那个按钮，以及测试）
    ///
    /// 真机的自动跳转走上面的窗口句柄路径（回填发生在没有窗口的后台轮询里）；这条入口是
    /// 按下去就有窗口的情况，也是测试能驱动的那一半（与对话框“真点击”同一口径，架构 §12 #29）。
    #[cfg_attr(not(test), allow(dead_code))] // 生产错误卡按钮走队列 LocateError→jump_to_error_site；本入口测试直接调
    pub(crate) fn jump_to_error_site_with(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(site) = self.error_site.clone() else {
            return;
        };
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(site.range(), cx);
            state.focus(window, cx);
        });
    }

    /// M5 命中跳行：把光标/选区移到 `line`（1 基行号）行，并聚焦面板。
    ///
    /// `span`（行内命中字节区间，来自草稿箱内容搜索）有值时**选中命中文本**；
    /// 无值时光标落在行首。行号越界 / 区间非法时静默退化为「光标放行首」，再不行就只聚焦。
    /// 与 `jump_to_error_site_with` 同口径（`set_selected_range` 会滚到可见处）。
    pub fn reveal_search_target(
        &mut self,
        line: u32,
        span: Option<(usize, usize)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let full = self.editor.read(cx).text().to_string();
        let Some(line_start) = diagnostics::offset_for_line_col(&full, line as usize, 1) else {
            return;
        };
        let range = match span {
            Some((s, e)) if s < e => line_start + s..line_start + e,
            _ => line_start..line_start,
        };
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(range, cx);
            state.focus(window, cx);
        });
    }

    /// 【B5】复制当前结果集（TSV）
    ///
    /// 复制的是**已抓到的行**（被截断的那份就只有那些行）——不假装能拿到没抓的部分。
    pub(crate) fn copy_active_result(&mut self, cx: &mut Context<Self>) {
        // 【#28-B1 拍板 D7】整份复制纳入本地行、排除被删行：走 delegate 视图序列拼全列 TSV，
        // 不再用 `entry.to_tsv()`（那条路是全量底层行，漏本地行、把被删行也拷走）。
        let has_grid = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
            .is_some();
        if !has_grid {
            self.set_message(Some("没有可复制的结果集".to_string()), cx);
            return;
        }
        let (text, rows) = self.grid.read(cx).delegate().active_result_tsv();
        if rows == 0 {
            self.set_message(Some("没有可复制的结果集".to_string()), cx);
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.set_message(Some(format!("已复制 {rows} 行（TSV）")), cx);
    }

    /// 【P2-4】转置视图开关（单行转置，DBeaver 式）：状态与网格同步都在面板实体
    /// （`drain_result_actions` 分派回来时调用；面板内直接改 delegate + refresh）
    fn toggle_transpose(&mut self, cx: &mut Context<Self>) {
        self.result_panel.update(cx, |p, cx| p.toggle_transpose(cx));
    }

    /// 【P2-4】转置行导航：上一行 / 下一行（越界夹取；数据行数来自 delegate）
    fn transpose_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.result_panel.update(cx, |p, cx| p.transpose_step(delta, cx));
    }

    /// 【B14】`Ctrl+C`（网格里）：把选中的**一格**或**一整行**拷到剪贴板
    ///
    /// 与右键菜单同一套口径：一格给**原文**（要的就是那一格的值），一整行拼 TSV。
    /// 选择自身是组件的真值，面板只记“最近一次选的是格还是行”（见 `result_selection`），
    /// 文本由 delegate 按**视图行序**拼（筛选 / 排序之后看着的那一行）。
    ///
    /// 设了键位就不能静默失败：没选中 / 选中的行已经不在视图里时给一句可读的提示。
    pub(crate) fn copy_grid_selection(&mut self, cx: &mut Context<Self>) {
        let selection = self.result_selection;
        let text =
            selection.and_then(|selection| self.grid.read(cx).delegate().selection_text(selection));
        let Some(text) = text else {
            self.set_message(Some("先在结果网格里选一格或一行再复制".to_string()), cx);
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let message = match selection {
            Some(result_grid::GridSelection::Row { .. }) => "已复制整行（TSV）".to_string(),
            _ => "已复制单元格".to_string(),
        };
        self.set_message(Some(message), cx);
    }

    /// 【B5】重跑当前结果集的 SQL（结果**换掉选中那份**，不是新开一份——原位刷新）
    pub(crate) fn refresh_active_result(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("这份结果没有可重跑的 SQL".to_string()), cx);
            return;
        };
        // 【A2】保持形态：下发筛选 / 排序下发的结果集带当初的原料重查（不退回全量）；
        // 普通执行 = 原 SQL 重跑。
        let target = match entry.pushdown.clone() {
            Some(Pushdown::Filtered { filter, columns }) => {
                ExecTarget::Filtered {
                    sql: entry.sql.clone(),
                    filter,
                    columns,
                }
            }
            Some(Pushdown::SortedDown {
                column,
                descending,
            }) => ExecTarget::SortedDown {
                sql: entry.sql.clone(),
                column,
                descending,
            },
            None => ExecTarget::Statement(entry.sql.clone()),
        };
        self.execute(target, ResultPlacement::Replace, cx);
    }

    /// 【B5b】取下一段（状态行⑦上的那个按钮）
    ///
    /// 提交一次 `Segment` 目标（落位 `Append`）：不动用户在看的那份的位置，新抓到的行
    /// **接在后面**。忙 / 没结果 / 没有下一段都回绝得可读（不静默）。
    /// 与滚动到底自动加载走的是**同一条路**（那个只负责把意图送到这里）。
    pub(crate) fn fetch_more(&mut self, cx: &mut Context<Self>) {
        let sql = self.result_panel.read(cx).sql().map(str::to_string);
        let Some(sql) = sql else {
            self.set_message(Some("这份结果没有可重取的 SQL".to_string()), cx);
            return;
        };
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::can_fetch_more)
        else {
            self.set_message(Some("这份结果已经取完了".to_string()), cx);
            return;
        };
        let offset = entry.row_count();
        let submitted = self.execute(
            ExecTarget::Segment {
                sql,
                offset,
                limit: execution::SEGMENT_ROWS,
            },
            ResultPlacement::Append,
            cx,
        );
        // 提交成功才算“正在取”：回填之前滚动到底不该再触发一次（防重入的真值在这边）
        if submitted {
            self.mark_fetching_more(cx);
        } else {
            // 没提交成功：把 delegate 的乐观置位撤回去，否则滚动到底会一直不再触发
            let grid = self.grid.clone();
            grid.update(cx, |state, _cx| {
                state.delegate_mut().set_loading_more(false)
            });
        }
    }

    /// 【B5b】标记“正在取下一段”
    ///
    /// 除了面板自己的真值，还要**当场**同步给网格的 delegate：提交到回填之间没有
    /// `sync_result_view`（那是一次全量投影，太重），不补这一下的话“滚动到底”会在
    /// 这段窗口里反复触发、每次都撞上“执行中”的回绝。
    fn mark_fetching_more(&mut self, cx: &mut Context<Self>) {
        self.fetching_more = true;
        let grid = self.grid.clone();
        grid.update(cx, |state, _cx| state.delegate_mut().set_loading_more(true));
    }

    /// 【1.2】筛选框运行：用 `select * from (原SQL) where 词` **替换**当前结果集重查
    /// （用户口径：刷新结果集）。词取面板**实时**筛选词（运行按钮点击时面板已定稿）。
    /// 生成的 wrapped SQL 写入面板 `exec_sql`（右栏「执行SQL」页签展示）。
    pub(crate) fn run_filter_as_query(&mut self, cx: &mut Context<Self>) {
        let value = self.result_panel.read(cx).filter_text().trim().to_string();
        if value.is_empty() {
            self.set_message(Some("先填一个筛选词再运行".to_string()), cx);
            return;
        }
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可执行的结果集".to_string()), cx);
            return;
        };
        let wrapped = format!("select * from ({}) as _q where {}", entry.sql, value);
        self.result_panel
            .update(cx, |p, cx| p.set_exec_sql(Some(wrapped.clone()), cx));
        self.execute(ExecTarget::Statement(wrapped), ResultPlacement::Replace, cx);
    }

    /// 【B15】下发源库：把当前筛选词交给执行器拼 `WHERE` 重查（结果落**新结果集**）
    pub(crate) fn pushdown_filter(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可下发的结果集".to_string()), cx);
            return;
        };
        let filter = self.result_panel.read(cx).filter_text().trim().to_string();
        if filter.is_empty() {
            self.set_message(Some("先填一个筛选词再下发".to_string()), cx);
            return;
        }
        self.execute(
            ExecTarget::Filtered {
                sql: entry.sql.clone(),
                filter,
                columns: entry.columns.clone(),
            },
            // 原型 §5.5：下发**产生新结果集**，原结果保留
            ResultPlacement::NewSet,
            cx,
        );
    }

    /// 【B14】按值筛选（右键菜单来的）：把值写进筛选框并立刻应用（**本地**那档）
    ///
    /// 输入框要跟着变（用户得看到自己被填了什么）；窗口句柄是构造时存的——
    /// 菜单点击发生在独立事件里，但没有 `window` 参数（headless 里甚至真的没有窗口），
    /// 拿不到就只改真值，筛选照样生效。
    pub(crate) fn apply_filter_value(&mut self, value: &str, cx: &mut Context<Self>) {
        let value = value.to_string();
        // 真值定稿（掐在途防抖 + 立即应用）不依赖窗口
        self.result_panel.update(cx, |p, cx| p.set_filter_value(&value, cx));
        // 输入框回填在窗口可用时做；拿不到（headless / 已关）就跳过，真值已生效
        let panel = self.result_panel.clone();
        let _ = self.window.update(cx, |_panel, window, app| {
            panel.update(app, |p, cx| p.sync_filter_input(&value, window, cx));
        });
    }

    /// 【筛选】列头菜单「按值筛选此列 / 清除」落点：代理到结果面板（空串 = 清除）
    pub(crate) fn apply_column_filter(&mut self, column: usize, value: &str, cx: &mut Context<Self>) {
        let value = value.to_string();
        self.result_panel
            .update(cx, |p, cx| p.apply_column_filter(column, &value, cx));
    }

    /// 【遗留 #7】右键「查看完整值」：记录目标并弹值查看器（对话框由 dialogs 层开）
    ///
    /// 钩子给的是 `&mut App`（拿不到窗口），对话框要 `&mut Window`——所以经
    /// `self.window.update` 转手调 [`Self::open_value_preview_with_window`]。
    /// headless 测试里窗口不在 `App::windows`（测试上下文持有），转手会报
    /// `window not found` 而弹不出——这是测试环境限制，真实窗口下正常。
    pub(crate) fn open_value_preview(
        &mut self,
        value: &str,
        column: &str,
        row: usize,
        cx: &mut Context<Self>,
    ) {
        self.value_preview = Some(ValuePreview {
            value: value.to_string(),
            column: column.to_string(),
            row,
        });
        let target = self.value_preview.clone().expect("刚写入");
        let result = self.window.update(cx, move |_panel, window, app| {
            // 转手闭包里 window 是真实窗口：直接弹
            Self::open_value_preview_with_window_impl(
                window,
                app,
                target.value.clone(),
                target.column.clone(),
                target.row,
            );
        });
        // 窗口句柄不可用（headless / 窗口已关）→ 留一句可读原因，不静默
        if let Err(error) = result {
            self.set_message(Some(format!("值查看器弹不出：{error}")), cx);
        }
    }

    /// 【遗留 #7】带窗口的值查看器入口（测试走它做窗口级验收；生产钩子经
    /// [`Self::open_value_preview`] 转手调同一实现）
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn open_value_preview_with_window(        &mut self,
        value: &str,
        column: &str,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.value_preview = Some(ValuePreview {
            value: value.to_string(),
            column: column.to_string(),
            row,
        });
        Self::open_value_preview_with_window_impl(
            window,
            cx,
            value.to_string(),
            column.to_string(),
            row,
        );
    }

    /// 弹层实现（与窗口来源无关）：`dialogs::open_value_preview` 是模态弹层，
    /// 复制不关弹层（用户复制完可能还要接着看）
    fn open_value_preview_with_window_impl(
        window: &mut Window,
        cx: &mut App,
        value: String,
        column: String,
        row: usize,
    ) {
        dialogs::open_value_preview(
            window,
            cx,
            value.clone(),
            column.clone(),
            row,
            move |app| app.write_to_clipboard(ClipboardItem::new_string(value.clone())),
        );
    }

    /// 【遗留 #7】只读访问器：值查看器目标是否已记录（测试断言用；`ValuePreview` 是私有类型，只暴露 bool）
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn value_preview_for_test(&self) -> bool {
        self.value_preview.is_some()
    }

    /// 【B16】弹参数表单（生产入口；headless 下 `self.window.update` 转手会报
    /// `window not found`——与 [`Self::open_value_preview`] 同款测试环境限制，留可读原因）
    pub(crate) fn open_bind_dialog(&mut self, names: Vec<String>, cx: &mut Context<Self>) {
        let entity = cx.entity();
        let result = self.window.update(cx, move |_panel, window, app| {
            let run_entity = entity.clone();
            let cancel_entity = entity.clone();
            dialogs::open_bind_dialog(
                window,
                app,
                names,
                move |values, _window, cx| {
                    run_entity.update(cx, |panel, cx| panel.finish_bind(values, cx));
                },
                move |cx| {
                    cancel_entity.update(cx, |panel, cx| panel.cancel_bind(cx));
                },
            );
        });
        if let Err(error) = result {
            self.set_message(Some(format!("参数表单弹不出：{error}")), cx);
        }
    }

    /// 【B16】参数表单提交：替换 `:name` 后继续原提交链（替换后无参数，直接走执行）
    pub(crate) fn finish_bind(
        &mut self,
        values: std::collections::HashMap<String, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(pending) = self.pending_bind.take() else {
            return;
        };
        let target = pending.substitute(&values);
        self.execute_labeled(target, pending.placement, pending.title, cx);
    }

    /// 【B16】取消参数表单：清掉挂起（「这次不跑了」不是只关个窗）
    pub(crate) fn cancel_bind(&mut self, _cx: &mut Context<Self>) {
        self.pending_bind = None;
    }

    /// 【B16】只读访问器：参数表单是否在挂起（测试断言用）
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn pending_bind_for_test(&self) -> bool {
        self.pending_bind.is_some()
    }

    /// 【B14】排序下发（右键菜单来的）：按这一列重查源库，结果落**新结果集**
    pub(crate) fn sort_down(&mut self, column: &str, descending: bool, cx: &mut Context<Self>) {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可下发的结果集".to_string()), cx);
            return;
        };
        self.execute(
            ExecTarget::SortedDown {
                sql: entry.sql.clone(),
                column: column.to_string(),
                descending,
            },
            // 与筛选下发同口径：**产生新结果集**，原结果保留
            ResultPlacement::NewSet,
            cx,
        );
    }

    /// 【M8】这份结果**实际**跑在哪条连接上：绑定优先，未绑定回退**当前活动连接**
    ///
    /// 口径与执行完全一致（`QueryRunner::active_connection` ⇒ 与 `resolve_conn_id` 同一处
    /// 语义）：否则会出现「执行走了 A、洞察取样走了 B」这种最难查的偏差。
    fn effective_connection(&self, entry: &ResultEntry) -> Option<String> {
        entry
            .connection
            .clone()
            .or_else(|| self.shared.active_connection())
    }

    /// 【M8】这份结果能不能给「洞察此列」入口（菜单项据此出现 / 消失）
    fn insight_available(&self, entry: &ResultEntry) -> bool {
        entry.can_insight_column() && self.effective_connection(entry).is_some()
    }

    /// 【M8】「洞察此列」（右键菜单来的）：把**列名 + 产生它的 SQL + 连接**交给宿主端口
    ///
    /// 不物化结果集（决策在案）：洞察侧拿这段 SQL 重跑一句带 `LIMIT` 的取样。
    /// 拿不到当前结果 / 解析不出连接 / 宿主没接端口都只是**不动作**——入口本来就不该出现
    /// （`insight_available` 已经挡住了），所以不需要再给用户弹一句话。
    pub(crate) fn insight_column(&mut self, column: &str, cx: &mut Context<Self>) {
        let Some(port) = self.shared.insight_column_port() else {
            return;
        };
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            return;
        };
        let Some(connection) = self.effective_connection(&entry) else {
            return;
        };
        let label = self.result_set_label();
        port(
            InsightColumnRequest {
                column: column.to_string(),
                sql: entry.sql.clone(),
                connection,
                label,
            },
            cx,
        );
    }

    /// 当前结果集在界面上的名字（「洞察此列」的来源标签：面板副标题 / 快照来源用）
    fn result_set_label(&self) -> String {
        // 选中的下标从权威 store 读（不再经面板——纯读路径不借实体）
        let index = self.shared.results().active_index(&self.document).unwrap_or(0);
        let titled = self
            .shared
            .results()
            .active(&self.document)
            .and_then(|entry| entry.title.clone());
        match titled {
            Some(title) => title,
            None => format!("结果 {}", index + 1),
        }
    }

    /// 【B14】开关切换：面板内翻转状态；打开时若已有筛选词，面板会入队
    /// `PushdownFilter`（`drain_result_actions` 分派回来时走 [`Self::pushdown_filter`] 重查）
    pub(crate) fn set_pushdown(&mut self, open: bool, cx: &mut Context<Self>) {
        self.result_panel.update(cx, |p, _| p.set_pushdown(open));
        cx.notify();
    }

    /// 【B15】测试用：直接应用筛选词（跳过 300ms 防抖，验“应用之后怎样”）
    #[cfg(test)]
    pub(crate) fn set_filter_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        self.result_panel.update(cx, |p, cx| p.set_filter_value(text, cx));
    }

    /// 切换结果集（结果集标签条点击）：选中项只有 `ResultStore` 能改，界面按它重画
    pub(crate) fn select_result_set(&mut self, index: usize, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .update_results(|store| store.select(&self.document, index));
        if changed {
            self.sync_result_view(cx);
        }
    }

    /// 【B15 切片二】自定义分析 SQL：先问一条 SQL，再把选中那份结果的已抓行桥接过去跑
    ///
    /// 与预置菜单同一个落点（[`Self::run_analysis`]），只是 SQL 由用户给。
    /// 说明里的行数取自 `request_for` 的桥接口径（截断与计数只有那处），不在界面侧另算。
    pub(crate) fn request_custom_analysis(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(entry) = self.shared.results_active(&self.document) else {
            self.set_message(Some("当前没有可分析的结果".to_string()), cx);
            return;
        };
        if !entry.has_grid() || entry.columns.is_empty() {
            // 与菜单同一判据：没有网格就没有行可桥接
            self.set_message(Some("这份结果没有行可分析".to_string()), cx);
            return;
        }
        // 用模板先算一遍桥接，只为把“多少行参与”说准（真的那次在 `run_analysis` 里重算）。
        // fact / pref / source_set 一并取快照：纯函数签名要求传入，这里不参与分流决策。
        let fact = active_fact_snapshot(&self.shared, &self.document);
        let pref = fact_storage_pref_or_default(cx);
        let source_set = active_set_index(&self.shared, &self.document);
        let bridged = crate::analysis::request_for(
            &entry,
            crate::analysis::CUSTOM_DEFAULT_SQL.to_string(),
            fact,
            pref,
            source_set,
        );
        let entity = cx.entity();
        dialogs::open_analysis_sql(
            window,
            cx,
            crate::analysis::custom_hint(bridged.bridged_rows(), bridged.dropped_rows),
            crate::analysis::CUSTOM_DEFAULT_SQL,
            move |sql, _window, cx| {
                entity.update(cx, |panel, cx| panel.run_analysis(sql, cx));
            },
        );
    }

    /// 【B15】本地分析：对**选中那份结果的已抓行**跑一条聚合 SQL（落新结果集）
    ///
    /// 三条口径：
    /// - 数据是桥接过来的行（把当前结果集已抓到的行建成本地 DuckDB 临时表），**不碰源库**——
    ///   所以不走通道闸 / 项目只读闸（那两道管的是“写源库对象”）；
    /// - 结果落**新结果集**并贴「分析」标题（原结果一行不动）；
    /// - 基于多少行、有没有被上限截掉，由执行器写进结果的说明（`analysis::notice`）。
    pub(crate) fn run_analysis(&mut self, sql: String, cx: &mut Context<Self>) {
        // 空的不提交：提交了也只会拿到一句驱动报错（“没写东西”在本地就能说清）
        if !crate::analysis::is_runnable(&sql) {
            self.set_message(
                Some("分析 SQL 为空（写一条 SELECT 再运行）".to_string()),
                cx,
            );
            return;
        }
        let Some(entry) = self.shared.results_active(&self.document) else {
            self.set_message(Some("当前没有可分析的结果".to_string()), cx);
            return;
        };
        // 【B3】事实档快照 + 偏好随请求带入工作线程：执行器据此选 batches 直灌保真档，
        // 无事实档才回退显示矩阵猜型降级档；工作线程无 `cx`，不能自己读 global。
        // 【B4】source_set = 当前选中槽位下标：物化回执据此回挂原结果集。
        let fact = active_fact_snapshot(&self.shared, &self.document);
        let pref = fact_storage_pref_or_default(cx);
        let source_set = active_set_index(&self.shared, &self.document);
        let request = crate::analysis::request_for(&entry, sql, fact, pref, source_set);
        self.execute_labeled(
            ExecTarget::Analysis(request),
            ResultPlacement::NewSet,
            Some(crate::analysis::ANALYSIS_TITLE.to_string()),
            cx,
        );
    }

    /// 【B7】导出选中结果集（**仅已抓取的行**，不发新查询）
    pub(crate) fn export_active_result(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        self.start_export(format, ExportScope::Fetched, cx);
    }

    /// 【B7】抓全量后导出（多轮取段；中途失败 / 换结果集就作废，不落半截文件）
    pub(crate) fn export_active_result_all(
        &mut self,
        format: ExportFormat,
        cx: &mut Context<Self>,
    ) {
        self.start_export(format, ExportScope::All, cx);
    }

    /// 导出的共同开头：选路径（用户取消 = 什么都不做）→ 直接落盘 / 先抓全量
    fn start_export(&mut self, format: ExportFormat, scope: ExportScope, cx: &mut Context<Self>) {
        tracing::info!(doc = %self.document, format = format.label(), scope = ?scope, "【探针】start_export：导出当前结果集进入");
        if !self.shared.has_export_path_picker() {
            self.set_message(Some("导出未接入（宿主未注入路径选择器）".to_string()), cx);
            return;
        }
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("没有可导出的结果集".to_string()), cx);
            return;
        };
        // 抓到一半再发现“执行器忙”更糟（路径都选完了），先问清楚（通道的忙是全局的：别的文档也在跑）
        if self.shared.is_executing() {
            self.set_message(Some("执行中，稍候再导出".to_string()), cx);
            return;
        }
        // 结果集序号（1 基）当默认名的兜底：SQL 里能猜出表名就用表名
        let index = self.result_panel.read(cx).active() + 1;
        let default_name = export::default_file_name(Some(&entry.sql), index, format);
        let Some(path) = self.shared.pick_export_path(format, default_name) else {
            // 用户取消：不是错误，不弹提示（与另存为同口径）
            return;
        };
        // 「抓全量」只在**真还有下一段**时才需要取段；已经抓完了就是普通导出
        if scope == ExportScope::Fetched || !entry.can_fetch_more() {
            self.write_export(&entry, format, index, &path, cx);
            return;
        }
        self.pending_export = Some(PendingExport {
            format,
            path,
            set: self.result_panel.read(cx).active(),
        });
        // 不另外摆一条“正在抓取”提示：状态栏已经在报「执行中 3.4s…」，
        // 而且 `execute` 成功时会清掉提示（设了也会被清）
        let submitted = self.execute(
            ExecTarget::Segment {
                sql: entry.sql.clone(),
                offset: entry.row_count(),
                limit: execution::SEGMENT_ROWS,
            },
            ResultPlacement::Append,
            cx,
        );
        if !submitted {
            // 没提交成功（通道拒了）：在途状态得撒回去，否则会残留到下一次取段
            self.pending_export = None;
        } else {
            self.mark_fetching_more(cx);
        }
    }

    /// 【B7】筛选中时导出的是**筛后的行集**（原型 §5.5）。
    ///
    /// 【#28-B1 排除矩阵】导出序列 = 底层未删行：本地行不进任何导出档、被删行排除；
    /// 筛选态与恒等态同一口径，不再回退 `entry.rows` 全量底层行（那条路会把被删行导走）。
    fn visible_rows_for_export(&self, cx: &mut Context<Self>) -> Vec<Vec<String>> {
        self.grid.read(cx).delegate().visible_rows_for_export()
    }

    /// 【B7 切片二】把行交给 DuckDB 落成 Parquet / XLSX（**后台线程**，不占执行位）
    ///
    /// 为什么不在这里同步写：XLSX 首次要 `INSTALL excel`（联网，探针实测本机 3–11 秒），
    /// 在 UI 线程上等它就是界面假死；回执走 [`Self::drain_exec_results`] 的同一条轮询泵。
    fn start_duckdb_export(
        &mut self,
        entry: &ResultEntry,
        format: ExportFormat,
        index: usize,
        path: &std::path::Path,
        rows: Vec<Vec<String>>,
        cx: &mut Context<Self>,
    ) {
        // 筛选中导出的是**筛后的行集**（原型 §5.5）：这个事实要跟着请求走，回执里要明示
        let filtered = !self.grid.read(cx).delegate().filter().trim().is_empty();
        // 【B3】按槽位下标取事实档快照：带类型档（Parquet / XLSX）优先 batches 直灌建表 COPY，
        // 无事实档回退显示矩阵猜型降级档；index 即结果集槽位，不取 active（与筛选口径一致）。
        let fact = self
            .shared
            .results()
            .fact_at(&self.document, index)
            .and_then(|state| state.fact().map(std::sync::Arc::clone));
        // 【D1】提交侧纯判定降级档口径（与 workbench 执行分流共用 `fact_export_mode`，
        // 不依赖工作线程回执回传口径）；成功回执据此追加降级说明文案。
        let degraded = ::shared::fact_export_mode(fact.as_deref()).is_degraded();
        let request = execution::DuckDbExportRequest {
            document: self.document.clone(),
            set: index,
            format,
            path: path.to_path_buf(),
            columns: entry.columns.clone(),
            rows,
            filtered,
            fact,
            degraded,
        };
        match self.shared.request_duckdb_export(request) {
            // 说一句“在跑”：导出可能几秒（首次装扩展），不能点完什么都不说
            Ok(()) => {
                self.export_pending += 1;
                self.set_message(
                    Some(format!(
                        "正在导出 {}（首次装 excel 扩展可能要几秒）…",
                        format.label()
                    )),
                    cx,
                );
                self.ensure_exec_pump(cx);
            }
            Err(reason) => self.set_message(Some(format!("导出未提交：{reason}")), cx),
        }
    }

    /// 编码并落盘（事件路径上同步写：与另存为同一口径）
    ///
    /// 【B7 切片二】**Parquet / XLSX 不走这里**：它们要 DuckDB `COPY`（XLSX 首次还要联网装扩展），
    /// 交给 [`Self::start_duckdb_export`] 到后台线程上做。
    fn write_export(
        &mut self,
        entry: &ResultEntry,
        format: ExportFormat,
        index: usize,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        // 【B7 切片二】筛选中就导出筛后的行（原型 §5.5：导出的就是当前筛选后的行集，并且要明示）
        let rows = self.visible_rows_for_export(cx);
        if format.is_duckdb_backed() {
            self.start_duckdb_export(entry, format, index, path, rows, cx);
            return;
        }
        let table = export::default_table_name(Some(&entry.sql), index);
        // 【B7 切片三】`SQL Updates` 的键列：与菜单同一来源（表结构端口），
        // 给了键就真编码成 UPDATE（`encode_rows_with_key`）；没键走普通编码
        // （`WHERE 子句` 不需要键，其余档不受影响）
        let key = self
            .bound_connection()
            .as_deref()
            .and_then(|conn| {
                self.shared
                    .table_key_columns(Some(conn), &table)
            })
            .and_then(|columns| export::RowKey::new(columns));
        let text = export::encode_rows_with_key(entry, format, &table, key.as_ref(), &rows);
        // INSERT 对空结果给空串（没有行就没有语句）；CSV / JSON 空结果仍有表头 / `[]`
        if text.is_empty() {
            self.set_message(Some("这份结果没有可导出的行".to_string()), cx);
            return;
        }
        match std::fs::write(path, text) {
            Ok(()) => {
                let rows_text = format!("{} 行（已筛选）", rows.len());
                self.set_message(
                    Some(format!(
                        "已导出 {rows_text}（{}）→ {}",
                        format.label(),
                        path.display()
                    )),
                    cx,
                )
            }
            Err(error) => self.set_message(Some(format!("导出失败：{error}")), cx),
        }
    }

    /// 【B7】推进「抓全量后导出」：每段回填后决定“再抓一段”还是“落盘”
    ///
    /// `last_segment_failed` 来自回填循环（取段失败时那句失败**不入存储**，得在这里了断）：
    /// 失败 / 结果集被切换都作废，**不落半截文件**。
    pub(crate) fn advance_pending_export(&mut self, last_segment_failed: bool, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_export.as_ref() else {
            return;
        };
        if last_segment_failed {
            self.pending_export = None;
            return;
        }
        let set = pending.set;
        if self.result_panel.read(cx).active() != set {
            self.pending_export = None;
            self.set_message(Some("导出取消：结果集已切换".to_string()), cx);
            return;
        }
        let Some(entry) = self.shared.results_active(&self.document) else {
            self.pending_export = None;
            return;
        };
        if !entry.can_fetch_more() {
            let Some(pending) = self.pending_export.take() else {
                return;
            };
            self.write_export(&entry, pending.format, set + 1, &pending.path, cx);
            return;
        }
        // 还有下一段：接着抓（每次一段，等它回来再推进）
        let sql = self.result_panel.read(cx).sql().map(str::to_string);
        let Some(sql) = sql else {
            self.pending_export = None;
            return;
        };
        let offset = entry.row_count();
        let submitted = self.execute(
            ExecTarget::Segment {
                sql,
                offset,
                limit: execution::SEGMENT_ROWS,
            },
            ResultPlacement::Append,
            cx,
        );
        if !submitted {
            self.pending_export = None;
        } else {
            self.mark_fetching_more(cx);
        }
    }

    /// 【B6】复制错误原文（错误卡片上的「复制」）：便于拿去搜索 / 报 bug
    pub(crate) fn copy_error_message(&mut self, cx: &mut Context<Self>) {
        // 错误卡真值在面板（sync 投影）；这里只复制文案
        let Some(card) = self.result_panel.read(cx).error_card().cloned() else {
            self.set_message(Some("这次没有错误可复制".to_string()), cx);
            return;
        };
        let location = card
            .location
            .as_ref()
            .map(|location| format!("（{location}）"))
            .unwrap_or_default();
        cx.write_to_clipboard(ClipboardItem::new_string(format!(
            "{}{location}",
            card.message
        )));
        self.set_message(Some("已复制错误原文".to_string()), cx);
    }

    /// `Ctrl+W`：请求关闭当前文档
    ///
    /// **不由面板自己执行**：`DockArea` 移除面板会读面板本体（可见性 / 可关闭性），
    /// 从面板自己的 `update`（动作处理器就在其中）里发起就是重入，GPUI 直接 panic。
    /// 所以关闭由宿主调用 [`close_document_in_dock`]，面板只负责回答“能不能关”。
    /// 脏文档在这里拦下并说明原因（Dock 没有“关闭前否决”钩子，架构 §12 #18）；
    /// 三态确认（保存 / 不保存 / 取消）由宿主弹 [`dialogs::open_close_confirm`]。
    pub(crate) fn refuse_close_when_dirty(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.is_dirty() {
            return false;
        }
        self.set_message(Some("有未保存的改动，先保存再关闭".to_string()), cx);
        true
    }
}
