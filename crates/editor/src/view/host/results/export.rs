//! 导出（export_active_result / start_export / write_export / advance_pending_export）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use crate::execution::{self, ExecTarget};
use crate::export::{self, ExportFormat, ExportScope};
use crate::store::{ResultEntry, ResultPlacement};

impl EditorHostPanel {
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
    pub(crate) fn start_export(&mut self, format: ExportFormat, scope: ExportScope, cx: &mut Context<Self>) {
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
    pub(crate) fn visible_rows_for_export(&self, cx: &mut Context<Self>) -> Vec<Vec<String>> {
        self.grid.read(cx).delegate().visible_rows_for_export()
    }

    /// 【B7 切片二】把行交给 DuckDB 落成 Parquet / XLSX（**后台线程**，不占执行位）
    ///
    /// 为什么不在这里同步写：XLSX 首次要 `INSTALL excel`（联网，探针实测本机 3–11 秒），
    /// 在 UI 线程上等它就是界面假死；回执走 [`Self::drain_exec_results`] 的同一条轮询泵。
    pub(crate) fn start_duckdb_export(
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
    pub(crate) fn write_export(
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
