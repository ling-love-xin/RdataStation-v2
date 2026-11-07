//! 复制 / 刷新 / 分页（copy_active_result / copy_grid_selection / refresh_active_result / fetch_more）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use crate::execution::{self, ExecTarget};
use crate::store::{Pushdown, ResultEntry, ResultPlacement};
use crate::view::results::grid::{self as result_grid};
use result_panel::ports::PageTarget;

impl EditorHostPanel {
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
    pub(crate) fn toggle_transpose(&mut self, cx: &mut Context<Self>) {
        self.result_panel.update(cx, |p, cx| p.toggle_transpose(cx));
    }

    /// 【P2-4】转置行导航：上一行 / 下一行（越界夹取；数据行数来自 delegate）
    pub(crate) fn transpose_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.result_panel
            .update(cx, |p, cx| p.transpose_step(delta, cx));
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
            Some(Pushdown::Filtered { filter, columns }) => ExecTarget::Filtered {
                sql: entry.sql.clone(),
                filter,
                columns,
            },
            Some(Pushdown::SortedDown { column, descending }) => ExecTarget::SortedDown {
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

    /// 【分页】状态栏翻页（首页 / 上页 / 下页 / 末页）：**纯视图滚动**，不重查源库
    ///
    /// 为什么不是带 OFFSET 重取：分段取数依赖源库给出稳定顺序，无 `ORDER BY` 的查询上
    /// 翻页会重复 / 跳行（`WINDOW_FETCH_NOTICE` 已就此提示）；而已抓到的行本来就在内存里，
    /// 翻页只是把视口挪过去——零重查、零风险，且「末页」不需要下发 COUNT（末页 = 已抓到的
    /// 最后一行，语义是"到底了"而不是"库里的第 N 页"）。
    ///
    /// 目标行以网格回写的**可见首行**为基准，一屏 = `SEGMENT_ROWS`；下页滚到末尾且这份
    /// 结果还有下一段时，顺带发一次取下一段（与「获取数据」真钮同一条路）。
    pub(crate) fn go_page(&mut self, target: PageTarget, cx: &mut Context<Self>) {
        let page = execution::SEGMENT_ROWS;
        let (total, top) = {
            let grid = self.grid.read(cx);
            let delegate = grid.delegate();
            (delegate.view_total(), delegate.visible_top())
        };
        let Some(last) = total.checked_sub(1) else {
            // 空结果集：没有可滚的目标（按钮本就不该出现，这里防御一次）
            return;
        };
        let mut need_more = false;
        let row = match target {
            PageTarget::First => 0,
            PageTarget::Last => last,
            PageTarget::Prev => top.saturating_sub(page),
            PageTarget::Next => {
                let next = top.saturating_add(page);
                if next >= last {
                    // 已经到（或越过）末尾：还有下一段就先取数（新行追加后会留在收尾处），
                    // 否则停在最后一行
                    need_more = self
                        .shared
                        .results_active(&self.document)
                        .is_some_and(|entry| entry.can_fetch_more());
                    last
                } else {
                    next
                }
            }
        };
        let grid = self.grid.clone();
        grid.update(cx, |state, cx| state.scroll_to_row(row, cx));
        if need_more {
            self.fetch_more(cx);
        }
    }

    /// 【P1-2】状态栏行号跳转（**1 基**行号，与行号列显示同口径）：滚到该行并选中。
    ///
    /// 纯视图导航：不重查源库（行已在内存里）。越界不报错而是夹到最后一行——用户敲
    /// `9999` 时想要的是"到底"，弹一句"没有这一行"只会多一次点击。
    pub(crate) fn go_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let grid = self.grid.clone();
        let total = {
            let state = grid.read(cx);
            state.delegate().view_total()
        };
        let Some(last) = total.checked_sub(1) else {
            self.set_message(Some("这份结果还没有行".to_string()), cx);
            return;
        };
        let target = row.saturating_sub(1).min(last);
        grid.update(cx, |state, cx| {
            state.scroll_to_row(target, cx);
            // 选中同一行：状态行「已选第 N 行」随之更新（否则滚过去了但"选中的"还在原处）
            state.set_selected_row(target, cx);
        });
    }

    /// 【B5b】标记“正在取下一段”
    /// 除了面板自己的真值，还要**当场**同步给网格的 delegate：提交到回填之间没有
    /// `sync_result_view`（那是一次全量投影，太重），不补这一下的话“滚动到底”会在
    /// 这段窗口里反复触发、每次都撞上“执行中”的回绝。
    pub(crate) fn mark_fetching_more(&mut self, cx: &mut Context<Self>) {
        self.fetching_more = true;
        let grid = self.grid.clone();
        grid.update(cx, |state, _cx| state.delegate_mut().set_loading_more(true));
    }
}
