//! 排序 / 洞察 / 结果集选择（sort_down / insight_column / select_result_set / set_pushdown）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use crate::ports::editor_exec::convert::cell_json;
use crate::shared::InsightColumnRequest;
use crate::store::ResultEntry;

impl EditorHostPanel {
    ///
    /// 口径与执行完全一致（`QueryRunner::active_connection` ⇒ 与 `resolve_conn_id` 同一处
    /// 语义）：否则会出现「执行走了 A、洞察取样走了 B」这种最难查的偏差。
    pub(crate) fn effective_connection(&self, entry: &ResultEntry) -> Option<String> {
        entry
            .connection
            .clone()
            .or_else(|| self.shared.active_connection())
    }

    /// 【M8】这份结果能不能给「洞察此列」入口（菜单项据此出现 / 消失）
    pub(crate) fn insight_available(&self, entry: &ResultEntry) -> bool {
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

    /// 【结果集二次分析】「分析此结果集」（右键菜单来的）：**先把整份结果物化成
    /// `tmp_q_` 临时表**，再把表名交给宿主端口。
    ///
    /// 为什么在编辑器里物化而不是宿主里：行数据在结果条目里，编辑器是它的持有者；
    /// 宿主拿不到 10 万行，让它去读也不可能（结果没落过盘）。
    ///
    /// 物化走既有的 `ResultService::create_duckdb_temp_table`（与导出 / 结果集库同一处
    /// 写路径）：建出来就是**登记在案的 `tmp_q_` 表**，受临时表 TTL 与上限管辖。
    /// 链式成立的前提：`tmp_q_` 与洞察取样的 `tmp_i_` 同在进程内存单例，同连接可见。
    pub(crate) fn analyze_result_set(&mut self, cx: &mut Context<Self>) {
        // 先判「有没有东西可分析」：空结果集不该先弹框再报错
        let Some(facts) = self.active_result_set_facts("分析", cx) else {
            return;
        };
        let window = self.window;
        let run_entity = cx.entity();
        // **不能在这里 update window**：此刻正在 update 本实体，重入同一个更新栈会 panic
        // （GUI 进程没有控制台，表现就是「点了没反应」）。`cx.defer` 推到本次更新结束后，
        // 那时已经脱离栈，弹框与错误回写都安全。同款理由见 `view/host/notebook.rs:105`。
        cx.defer(move |cx| {
            let entity = run_entity.clone();
            let cancel_entity = run_entity.clone();
            let result = window.update(cx, move |_root, window, app| {
                crate::view::dialogs::open_result_analyze(
                    window,
                    app,
                    facts,
                    move |name, destination, _window, cx| {
                        _ = entity.update(cx, |panel, cx| {
                            panel.finish_result_analyze(name, destination, cx)
                        });
                    },
                    move |cx| {
                        _ = cancel_entity.update(cx, |panel, cx| {
                            panel.set_message(None, cx);
                        });
                    },
                );
            });
            if let Err(error) = result {
                _ = run_entity.update(cx, |panel, cx| {
                    panel.set_message(Some(format!("分析框弹不出：{error}")), cx);
                });
            }
        });
    }

    /// 把当前结果集的事实（名字 / 行数 / 列数）取出来给对话框；空结果集写消息并返回 `None`
    fn active_result_set_facts(
        &mut self,
        purpose: &str,
        cx: &mut Context<Self>,
    ) -> Option<crate::view::dialogs::ResultSetFacts> {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(
                Some(format!("{purpose}：当前没有可{purpose}的结果集")),
                cx,
            );
            return None;
        };
        if entry.rows.is_empty() {
            self.set_message(Some(format!("{purpose}：这份结果集没有行")), cx);
            return None;
        }
        let label = self.result_set_label();
        let columns = entry.columns.len();
        Some(crate::view::dialogs::ResultSetFacts {
            label,
            rows: entry.rows.len(),
            columns,
        })
    }

    /// 对话框提交：物化 + 按去向分派（临时 = 只入会话；持久 = 交给宿主播进项目库）
    pub(crate) fn finish_result_analyze(
        &mut self,
        name: String,
        destination: crate::view::dialogs::ResultSetDestination,
        cx: &mut Context<Self>,
    ) {
        use crate::view::dialogs::ResultSetDestination;

        let Some((table, _label, rows)) =
            self.materialize_active_result_set("分析", Some(&name), cx)
        else {
            return;
        };
        match destination {
            ResultSetDestination::Temporary => {
                // 不写盘：表已经在进程单例里，宿主要做的只是把数据对象面板的缓存重取一次
                match self.shared.materialized_result_set_port() {
                    Some(port) => port(
                        crate::shared::MaterializedResultSetNotice {
                            table,
                            label: name,
                            rows,
                        },
                        cx,
                    ),
                    None => self.set_message(
                        Some(format!(
                            "已物化为 {table}（{rows} 行）；在数据对象面板刷新后可见"
                        )),
                        cx,
                    ),
                }
            }
            ResultSetDestination::Persistent => {
                let Some(port) = self.shared.persist_result_set_port() else {
                    self.set_message(
                        Some(format!("已物化为 {table}，但宿主没接落库出口（未打开项目？）")),
                        cx,
                    );
                    return;
                };
                self.set_message(
                    Some(format!("已物化为 {table}（{rows} 行），正在写入项目分析库…")),
                    cx,
                );
                port(
                    crate::shared::PersistResultSetRequest {
                        table,
                        label: name,
                        rows,
                    },
                    cx,
                );
            }
        }
    }

    /// 【结果集持久化】把当前结果集落成**项目分析库的正式表**。
    ///
    /// 与 [`Self::analyze_result_set`] 共用同一步物化（同一个 `tmp_q_`），差别只在去处：
    /// 那边拿去当洞察取样来源（临时、随 TTL 走），这边交给宿主播进项目库（持久）。
    /// 分两步的好处是：真落库在宿主侧的后台线程，编辑器这里只负责「把结果交出去」。
    pub(crate) fn persist_result_set(&mut self, cx: &mut Context<Self>) {
        let Some(port) = self.shared.persist_result_set_port() else {
            return;
        };
        let Some((table, label, rows)) = self.materialize_active_result_set("持久化", None, cx) else {
            return;
        };
        self.set_message(
            Some(format!("已物化结果集为 {table}（{rows} 行），正在写入项目分析库…")),
            cx,
        );
        port(
            crate::shared::PersistResultSetRequest { table, label, rows },
            cx,
        );
    }

    /// 把当前结果集物化成登记的 `tmp_q_` 表；失败/无结果时就写消息并返回 `None`。
    ///
    /// 抽出来给「分析此结果集」与「持久化为项目表」共用：两次实现会让两条链在「空结果 /
    /// 无行 / 物化失败」上的口径慢慢分叉，而这两件事本来就是同一份结果的两个去向。
    fn materialize_active_result_set(
        &mut self,
        purpose: &str,
        // 用户起的名字进表名（`None` = 沿用历史口径 `result`）；临时表名只留 ASCII
        description: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Option<(String, String, usize)> {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(
                Some(format!("{purpose}：当前没有可{purpose}的结果集")),
                cx,
            );
            return None;
        };
        if entry.rows.is_empty() {
            self.set_message(Some(format!("{purpose}：这份结果集没有行")), cx);
            return None;
        }
        let label = self.result_set_label();
        // 网格里存的是字符串行（`NULL` 是字面量）；桥接口径与执行链一致（`cell_json`）
        let json_rows: Vec<Vec<serde_json::Value>> = entry
            .rows
            .iter()
            .map(|row| row.iter().map(|cell| cell_json(cell)).collect())
            .collect();
        let rows = entry.rows.len();
        match crate::ports::result_service::ResultService::create_duckdb_temp_table_named(
            &entry.columns,
            &json_rows,
            description.unwrap_or("result"),
            // owner = 本文档：结果集物化的表跟着文档走，关文档一并收掉
            self.document.as_str(),
        ) {
            Ok(table) => Some((table, label, rows)),
            Err(error) => {
                self.set_message(
                    Some(format!("{purpose}：物化结果集失败（{error}）")),
                    cx,
                );
                None
            }
        }
    }

    /// 当前结果集在界面上的名字（「洞察此列」的来源标签：面板副标题 / 快照来源用）
    pub(crate) fn result_set_label(&self) -> String {
        // 选中的下标从权威 store 读（不再经面板——纯读路径不借实体）
        let index = self
            .shared
            .results()
            .active_index(&self.document)
            .unwrap_or(0);
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
        self.result_panel
            .update(cx, |p, cx| p.set_filter_value(text, cx));
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
}
