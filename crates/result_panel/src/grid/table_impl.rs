//! `TableDelegate` 主体实现（`grid/` 第九块）：列数 / 行数 / 拖列 / 排序 / 取下一段 /
//! 列描述 / 空态 / 导出取文本。
//!
//! Rust 不允许同一 trait 对同一类型拆多个 impl 块（E0119），所以 `render_th` /
//! `render_td` / `context_menu` 的实际逻辑在 [`header`] / [`cell`] / [`menu`] 的
//! **顶层自由函数**中，这里只做薄转发（2026-10-29 拆分）。

use gpui_kit::*;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::prelude::FluentBuilder as _;

use crate::ui;
use super::delegate::ResultGridDelegate;
impl TableDelegate for ResultGridDelegate {
    /// 表头单元格（转发到 [`crate::grid::header::render_th`]）
    fn render_th(
        &mut self,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        crate::grid::header::render_th(self, col_ix, window, cx)
    }

    /// 单元格渲染（转发到 [`crate::grid::cell::render_td`]）
    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        crate::grid::cell::render_td(self, row_ix, col_ix, window, cx)
    }

    /// 右键菜单（转发到 [`crate::grid::menu::context_menu`]）
    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        crate::grid::menu::context_menu(self, row_ix, menu, window, cx)
    }

    fn columns_count(&self, _cx: &App) -> usize {
        // 首列是行号槽（原型 §2.4）：没有结果时不画它，空态就交回给 `render_empty`；
        // 有结果时 = 数据列 + 行号槽 − 隐藏列
        if self.columns.is_empty() {
            0
        } else if self.transposed {
            // 转置视图固定三列：`#` 行号槽 · 字段 · 值
            3
        } else {
            self.columns.len() + 1 - self.hidden.len()
        }
    }

    fn rows_count(&self, _cx: &App) -> usize {
        // 转置视图一行 = 一个可见字段（行数 = 可见列数）
        if self.transposed {
            return self.transpose_columns().len();
        }
        self.view_len()
    }

    /// 【结果集 #296】列头拖拽：组件已让渲染按新序走（内部 `col_groups` 重排），
    /// 这里只需把 `display_order` 同步成用户拖出来的显示序，供面板读取持久化。
    /// `col_ix` / `to_ix` 是含行号槽的列下标（0 = 行号槽；跟 perform_sort 同一口径）。
    fn move_column(
        &mut self,
        col_ix: usize,
        to_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        if self.transposed || Self::is_row_number(col_ix) || Self::is_row_number(to_ix) {
            return;
        }
        let from = self.data_column(col_ix - 1);
        let to = self.data_column(to_ix - 1);
        if from >= self.display_order.len() || to >= self.display_order.len() {
            return;
        }
        let moved = self.display_order.remove(from);
        self.display_order.insert(to, moved);
    }

    /// 【B15】列头点击排序：组件库已在它那边循环 `Default → Descending → Ascending`，
    /// 交到这里的已经是**新值**；本 delegate 只负责记住它并重算视图行序（本地排序，不重查）。
    /// `col_ix` 是含行号槽的列下标，折算成数据列。
    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        // 【P2-4】转置视图排序无意义（值是各原列，列级排序会打乱字段行），整表禁用
        if self.transposed {
            return;
        }
        if Self::is_row_number(col_ix) {
            return;
        }
        let column = self.data_column(col_ix - 1);
        self.apply_sort(match sort {
            ColumnSort::Default => None,
            ColumnSort::Ascending => Some((column, false)),
            ColumnSort::Descending => Some((column, true)),
        });
        // 视图行序变了，重画（表格自己也会 notify，但这里是真值变了）
        cx.notify();
    }

    /// 滚动到底自动加载（B5b）：组件库在可见范围距末尾不足 `load_more_threshold` 行时
    /// 反复调 [`Self::load_more`]，所以真实条件与防重入都在这里判——
    /// “还有没有下一段”是结果的真值（面板同步），不是表格猜的。
    fn has_more(&self, _cx: &App) -> bool {
        self.has_more
    }

    fn load_more(&mut self, _window: &mut Window, cx: &mut Context<TableState<Self>>) {
        if !self.wants_more() {
            return;
        }
        let Some(hook) = self.on_load_more.clone() else {
            return;
        };
        // 乐观置位：组件库会在“接近末尾”的每一帧都调进来，而回调是异步的（下面那行
        // `spawn_in`）——不先拦住的话同一帧里的后续调用会再发一次。真实值由面板回填时同步
        // （面板提交通道上失败会把这一下撤回去）。
        self.loading_more = true;
        //
        // 面板会在提交成功时置 `loading_more`（下一次同步回这里），
        // 所以这里不必自己改状态——避免“面板以为没在取、delegate 以为在取”的两份真值。
        //
        // 回调要**挪出当前更新栈**：面板会回头把 `loading_more` 写回本 delegate，
        // 同步调用就是“更新一个正在更新的实体”→ panic（组件库自己的
        // `load_more_if_need` 也是用 `spawn_in` 把它挪出去的）。
        cx.spawn_in(_window, async move |_view, window| {
            _ = window.update(|_window, cx| hook(cx));
        })
        .detach();
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        // 【P2-4】转置视图：行号槽照旧（is_row_number(0) 命中下方分支），
        // 第 1 / 2 列固定是「字段 / 值」——列名就是这两个词，不跟原列走
        if self.transposed && !Self::is_row_number(col_ix) {
            return match col_ix {
                1 => Column::new("transpose-field", "字段")
                    .width(ui::RESULT_COLUMN_WIDTH)
                    .min_width(ui::RESULT_COLUMN_MIN_WIDTH)
                    .resizable(true),
                _ => Column::new("transpose-value", "值")
                    .width(ui::RESULT_COLUMN_WIDTH)
                    .min_width(ui::RESULT_COLUMN_MIN_WIDTH)
                    .resizable(true),
            };
        }
        if Self::is_row_number(col_ix) {
            // 行号槽：窄、钉在左边、不可拖宽/移动——它是视图辅助，不是数据列
            return Column::new("row-no", "#")
                .width(ui::RESULT_ROW_NUMBER_WIDTH)
                .min_width(ui::RESULT_ROW_NUMBER_WIDTH)
                .fixed_left()
                .resizable(false)
                .movable(false)
                .selectable(false);
        }
        let index = self.data_column(col_ix - 1);
        let name = self
            .columns
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("列 {}", index + 1));
        // 【P1-5】拖过的列按列名回填用户宽度；没拖过走默认档（行号槽 / 转置列在上面分支已返回）
        let width = self
            .column_widths
            .get(&name)
            .copied()
            .unwrap_or(ui::RESULT_COLUMN_WIDTH);
        Column::new(format!("col-{index}"), name)
            .width(width)
            .min_width(ui::RESULT_COLUMN_MIN_WIDTH)
            // 【B15】可本地排序：组件库点列头时会在 Default → Descending → Ascending 里循环，
            // 并回调 `perform_sort`（排序状态存在它那边，这里只把当前态回去，refresh 后指示器不丢）
            .sort(self.sort_mark(index))
            // 【B15】冻结列用原生 `fixed_left`（横向滚动钉在左侧；行号槽一直是钉住的）
            .when(self.is_frozen(index), |column| column.fixed_left())
    }
    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(muted)
            .child(self.empty_text.clone())
    }

    /// 导出用（B7 的 CSV/JSON 导出取这里；复制走 `store::ResultEntry` 的 TSV）
    fn cell_text(&self, row_ix: usize, col_ix: usize, _cx: &App) -> String {
        self.cell(row_ix, col_ix)
    }
}
