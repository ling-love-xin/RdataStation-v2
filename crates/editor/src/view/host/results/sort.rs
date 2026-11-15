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
        let Some(port) = self.shared.analyze_result_set_port() else {
            return;
        };
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("分析：当前没有可分析的结果集".to_string()), cx);
            return;
        };
        if entry.rows.is_empty() {
            self.set_message(Some("分析：这份结果集没有行".to_string()), cx);
            return;
        }
        let label = self.result_set_label();
        // 网格里存的是字符串行（`NULL` 是字面量）；桥接口径与执行链一致（`cell_json`）
        let json_rows: Vec<Vec<serde_json::Value>> = entry
            .rows
            .iter()
            .map(|row| row.iter().map(|cell| cell_json(cell)).collect())
            .collect();
        let table = match crate::ports::result_service::ResultService::create_duckdb_temp_table(
            &entry.columns,
            &json_rows,
        ) {
            Ok(table) => table,
            Err(error) => {
                self.set_message(Some(format!("分析：物化结果集失败（{error}）")), cx);
                return;
            }
        };
        let rows = entry.rows.len();
        self.set_message(
            Some(format!("已物化结果集为 {table}（{rows} 行），正在打开洞察…")),
            cx,
        );
        port(
            crate::shared::AnalyzeResultSetRequest { table, label, rows },
            cx,
        );
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
