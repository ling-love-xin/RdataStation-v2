//! 复制 / 刷新 / 分页（copy_active_result / copy_grid_selection / refresh_active_result / fetch_more）
//!
//! 自 host/results.rs 拆分（2026-10-29 Step 7）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use crate::execution::{self, ExecTarget};
use crate::store::{Pushdown, ResultEntry, ResultPlacement};
use crate::view::results::grid::{self as result_grid};
use result_panel::ports::PageTarget;

/// 【第 71 批】「取全部」的安全阀：最多提交多少段
///
/// DBeaver `readAllData:4426-4459` 是「一直取到 `isHasMoreData()` 为假」，没有自带上限
/// ——那边每次取段是同步阻塞的，用户还能按取消。本仓是异步多轮、没有中断入口，
/// 源库若每段都报「还有」（比如视图在别的会话里持续增长），没有上限就是一个停不下来的循环。
/// 100 段 × 默认 1000 行 = 10 万行；到顶就停并如实说一句，用户可以改段大小再来。
/// 这是**本仓差异**（DBeaver 无此上限），记档在 `result-panel.md` §9.54.5。
pub(crate) const FETCH_ALL_MAX_SEGMENTS: usize = 100;

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

    /// 【D10-5】`Ctrl+C`（网格里）：把选中的**一格**、**一整行**或**一整列**拷到剪贴板
    ///
    /// 与右键菜单同一套口径：一格给**原文**（要的就是那一格的值），整行 / 整列拼 TSV。
    /// 选择自身是组件的真值，面板只记“最近一次选的是格还是行还是列”（见 `result_selection`），
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
            Some(result_grid::GridSelection::Column { .. }) => "已复制整列（TSV）".to_string(),
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
    ///
    /// 【第 71 批】两条互斥：正在「取全部」时不许再单独取段（同一个 offset 会被提交两次、
    /// 行重复）；导出正在抓全量时也不许（两条都是 `Append`，会互相打乱 offset）。
    pub(crate) fn fetch_more(&mut self, cx: &mut Context<Self>) {
        if let Some(progress) = self.fetching_all.as_ref() {
            let segments = progress.segments;
            self.set_message(
                Some(format!("正在取全部数据（已取 {segments} 段），稍等")),
                cx,
            );
            return;
        }
        if self.pending_export.is_some() {
            self.set_message(Some("导出正在取数，先等它结束".to_string()), cx);
            return;
        }
        self.submit_segment(cx);
    }

    /// 【第 72 批】「取全部」先问一声（DBeaver `readAllData` 的确认环节，`:4433-4440`）
    ///
    /// 按钮与 `CTRL+SHIFT+=` 都先落到这里。判定只做一份，因为「还有没有下一段 /
    /// 有没有执行错误」的真值在结果条目（`ResultEntry::can_fetch_more`）；DBeaver 也是
    /// 先 `!isHasMoreData()` 静默 return、再弹确认（`:4430-4432`）。
    ///
    /// 两种「不弹」的提示分开说：执行出错时错因比「取完了」更该讲，别让用户误以为
    /// 数据已经取干净了。
    pub(crate) fn ask_fetch_all(&mut self, cx: &mut Context<Self>) {
        if let Some(progress) = self.fetching_all.as_ref() {
            let segments = progress.segments;
            self.set_message(
                Some(format!("正在取全部数据（已取 {segments} 段），稍等")),
                cx,
            );
            return;
        }
        // 【第 73 批】导出正在抓全量：它已经占着取段，这个问题问下去只会白走一趟浮层
        // （「继续」照样会被 `fetch_all` 的 `pending_export` 判定拒）。判在 `fetching_more`
        // 之前：导出的在途段也会置 `fetching_more`，说「正在取下一段」不如说清是谁在取。
        if self.pending_export.is_some() {
            self.set_message(Some("导出正在取数，先等它结束".to_string()), cx);
            return;
        }
        // 取下一段还在途：这会儿问「要不要取全部」没有意义——`fetch_all` 用的 offset
        // 是「已抓到的行数」，在途那一段还没回来，一问一答之间行数就变了
        if self.fetching_more {
            self.set_message(Some("正在取下一段，等它回来再取全部".to_string()), cx);
            return;
        }
        let (has_error, has_more, rows) = match self.shared.results_active(&self.document) {
            Some(entry) => (entry.error.is_some(), entry.has_more, entry.row_count()),
            None => {
                self.set_message(Some("这份结果没有可取的数据".to_string()), cx);
                return;
            }
        };
        if has_error {
            self.set_message(
                Some("这份结果执行出错了；修好再试取全部".to_string()),
                cx,
            );
            return;
        }
        if !has_more {
            self.set_message(Some(format!("这份结果已经取完了（共 {rows} 行）")), cx);
            return;
        }
        self.result_panel
            .update(cx, |panel, cx| panel.open_fetch_all_confirm(cx));
    }

    /// 【第 71 批】「取全部数据」（DBeaver `readAllData:4426-4459`）
    ///
    /// DBeaver 那边是 `while (isHasMoreData()) readNextSegment()` 的一个循环；本仓的执行
    /// 是异步的（提交 → 回填 → 下一轮），所以循环不能写在函数体内，只能拆成
    /// 「发起」+「每段回填后推进」（[`Self::advance_fetch_all`]，由执行回填处调）。
    ///
    /// 与「取下一段」共用 [`Self::submit_segment`]：判定（无 SQL / 已取完）与提示同源，
    /// 不为取全部另写一套真值表。
    pub(crate) fn fetch_all(&mut self, cx: &mut Context<Self>) {
        if let Some(progress) = self.fetching_all.as_ref() {
            self.set_message(
                Some(format!("正在取全部数据（已取 {} 段）", progress.segments)),
                cx,
            );
            return;
        }
        // 【第 72 批】取下一段还在途时不许开「取全部」：`submit_segment` 的 offset
        // 取「已抓到的行数」，在途那一段还没回填 —— 两条会拿同一个 offset 各提交一次，
        // 行重复。第 71 批只在「取全部 / 导出」方向上做了互斥，这一条是反向补口
        // （DBeaver 用 `nextSegmentReadingBlocked` 挡同一件事，`ResultSetViewer:4384`）。
        if self.fetching_more {
            self.set_message(Some("正在取下一段，等它回来再取全部".to_string()), cx);
            return;
        }
        if self.pending_export.is_some() {
            self.set_message(Some("导出正在取数，先等它结束".to_string()), cx);
            return;
        }
        let set = self.result_panel.read(cx).active();
        if self.submit_segment(cx) {
            self.fetching_all = Some(FetchAllProgress { set, segments: 1 });
            self.set_message(Some("正在取全部数据…".to_string()), cx);
        }
    }

    /// 【第 71 批】每段回填后推进「取全部」：还有下一段就再提交一段，否则收口
    ///
    /// 调用点必须在 `sync_result_view` **之后**（`pump.rs` 的回填块）：下一段的 `offset`
    /// 取的是「已抓到的行数」，只有同步过才是新值。
    pub(crate) fn advance_fetch_all(
        &mut self,
        last_segment_failed: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(progress) = self.fetching_all.take() else {
            return;
        };
        if last_segment_failed {
            // 失败原因由回填块统一说（这一段抓到的行是用户的成果，不入存储也不清）
            return;
        }
        if self.pending_export.is_some() {
            // 导出接管了取段（两条互斥，理论上不会同时存在）：让出去
            return;
        }
        // 面板切了结果集：`Append` 会接到另一份结果的尾巴上，必须停
        if self.result_panel.read(cx).active() != progress.set {
            self.set_message(Some("取全部取消：结果集已切换".to_string()), cx);
            return;
        }
        // 先把要用的两个值复制出来：`entry` 借的是 `self.shared`，后面 `submit_segment`
        // 要 `&mut self`，borrow 必须在那之前结束
        let (can_more, rows) = match self.shared.results_active(&self.document) {
            Some(entry) => (entry.can_fetch_more(), entry.row_count()),
            None => return,
        };
        if !can_more {
            self.set_message(Some(format!("已取全部 {rows} 行")), cx);
            return;
        }
        if progress.segments >= FETCH_ALL_MAX_SEGMENTS {
            self.set_message(
                Some(format!(
                    "已取 {} 段（{rows} 行），先停在这里；要更多就把段大小调大再来",
                    progress.segments
                )),
                cx,
            );
            return;
        }
        if self.submit_segment(cx) {
            self.fetching_all = Some(FetchAllProgress {
                set: progress.set,
                segments: progress.segments + 1,
            });
        }
    }

    /// 提交一次「取下一段」（**不做**取全部 / 导出的互斥判定：调用方负责）
    ///
    /// 返回是否提交成功。成功后当场 [`Self::mark_fetching_more`]——提交到回填之间
    /// 滚动到底不该再触发一次。
    fn submit_segment(&mut self, cx: &mut Context<Self>) -> bool {
        let sql = self.result_panel.read(cx).sql().map(str::to_string);
        let Some(sql) = sql else {
            self.set_message(Some("这份结果没有可重取的 SQL".to_string()), cx);
            return false;
        };
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::can_fetch_more)
        else {
            self.set_message(Some("这份结果已经取完了".to_string()), cx);
            return false;
        };
        let offset = entry.row_count();
        // 【R6 段大小】段大小由面板持有（状态行输入框可改，对齐 DBeaver
        // `resultSetSize` → `RESULT_SET_MAX_ROWS`）：这里按面板当前值下发 limit，
        // 不再写死编辑器常量。
        let limit = self.result_panel.read(cx).segment_rows();
        let submitted = self.execute(
            ExecTarget::Segment {
                sql,
                offset,
                limit,
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
        submitted
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
