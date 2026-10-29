//! 网格同步与比较模式（sync_csv_grid / sync_result_view / sync_compare_grid / toggle_compare_mode）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use result_panel::panel::{ExportCtx, GridPayload, ResultSyncInput};
use crate::channel::{self};
use crate::diagnostics;
use crate::export::{self};
use crate::view::results::error_card::ErrorCard;
use crate::view::results::grid::{self as result_grid, ResultStatus, ResultToolbar};
use crate::view::results::sets::{self as result_sets};

impl EditorHostPanel {
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
    pub(crate) fn sync_compare_grid(&mut self, cx: &mut Context<Self>) {
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

}
