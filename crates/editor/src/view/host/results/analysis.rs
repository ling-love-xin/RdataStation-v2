//! 自定义分析（request_custom_analysis / run_analysis）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use crate::execution::ExecTarget;
use crate::store::ResultPlacement;
use crate::view::dialogs;

impl EditorHostPanel {
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

}
