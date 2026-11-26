//! `TableDelegate` 主体实现（`grid/` 第九块）：列数 / 行数 / 拖列 / 排序 / 取下一段 /
//! 列描述 / 空态 / 导出取文本。
//!
//! Rust 不允许同一 trait 对同一类型拆多个 impl 块（E0119），所以 `render_th` /
//! `render_td` / `context_menu` 的实际逻辑在 [`header`] / [`cell`] / [`menu`] 的
//! **顶层自由函数**中，这里只做薄转发（2026-10-29 拆分）。

use std::ops::Range;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::delegate::ResultGridDelegate;
use crate::store::ColumnKind;
use crate::ui;
use workbench_shell::product_tokens;

/// 单元格内距（对齐 DBeaver `GridCellRenderer.LEFT_MARGIN / RIGHT_MARGIN = 6`）。
///
/// **左右置零**，由单元格自己画（`grid/cell.rs` 的 `.px(ui::RESULT_CELL_PADDING_X)`）：这样那条 1px
/// 竖分隔线正好落在列边界上。若左右内距留在组件层，单元格的右边线会画在离边界 6px 的
/// 位置——列与列之间就没有分界线了（DBeaver `LightGrid.linesVisible = true` 是逐列画线，
/// `LightGrid.java:2425-2439`）。上下仍留 2px：与组件 XSmall 档原值一致，垂直位置不变。
///
/// `fit_col` 也读这份 padding（`table/state.rs:1299-1303`），所以列宽自适应的
/// 「内容宽 + 内距」与本文件渲染完全同一口径。
fn cell_paddings() -> Edges<Pixels> {
    Edges {
        top: ui::RESULT_CELL_VERTICAL_PADDING,
        bottom: ui::RESULT_CELL_VERTICAL_PADDING,
        left: Pixels::ZERO,
        right: Pixels::ZERO,
    }
}

/// 列类型 → 默认列宽（用户没拖过、也没双击自适应过的列用哪一档）
///
/// 口径：按 [`ColumnKind`]（归一化语义类型）而不是驱动原始类型名——MySQL 报 `int`、
/// PG 报 `int4`，语义都是"数字列"，宽该一样。未知类型回到 [`ui::RESULT_COLUMN_WIDTH`]
/// （128），守住「类型缺失时一个像素都不变」的既有原则（驱动没填类型之前，所有真实结果
/// 走的都是这一档）。
///
/// 取值与推导：tusk 给过的四档照搬（numeric 130 / timestamptz 210 / uuid 300 / json 260）；
/// 竞品没给的（boolean / date·time / text / binary）按「典型展示串宽 + 内边距」推：
/// 字号 12px，西文数字平均宽 ≈ 0.55 × 字号 ≈ 6.6px/字符，单元格左右内边距合计 24px
/// （自身 `px_2` 16 + 组件 `RESULT_TABLE_SIZE` = XSmall 档 8）。明细见
/// `docs/architecture/editor/result-panel.md` §8.24。
pub(crate) fn default_column_width(kind: ColumnKind) -> Pixels {
    match kind {
        ColumnKind::Integer | ColumnKind::Float | ColumnKind::Decimal => {
            ui::RESULT_COLUMN_WIDTH_NUMERIC
        }
        ColumnKind::Boolean => ui::RESULT_COLUMN_WIDTH_BOOLEAN,
        ColumnKind::Timestamp => ui::RESULT_COLUMN_WIDTH_TIMESTAMP,
        ColumnKind::Date | ColumnKind::Time => ui::RESULT_COLUMN_WIDTH_DATE_TIME,
        ColumnKind::Uuid => ui::RESULT_COLUMN_WIDTH_UUID,
        ColumnKind::Json => ui::RESULT_COLUMN_WIDTH_JSON,
        ColumnKind::Binary => ui::RESULT_COLUMN_WIDTH_BINARY,
        ColumnKind::Text => ui::RESULT_COLUMN_WIDTH_TEXT,
        // 数组：展示是 `{a,b}` 文本，宽按文本档（真值对数组列没有专属默认宽）
        ColumnKind::Array => ui::RESULT_COLUMN_WIDTH_TEXT,
        ColumnKind::Unknown => ui::RESULT_COLUMN_WIDTH,
    }
}
impl TableDelegate for ResultGridDelegate {
    /// 行容器：铺整行选中色 / 奇数行斑马底色（本 crate 唯一一处 delegate 原生绘制）
    ///
    /// 【斑马线：跟随主题，不设产品槽】2026-11-21 用户拍板 A 方案——斑马底色一律取主题
    /// `tokens.table_even`，与组件 `.stripe(true)`（`status.rs` 开）同值同源，换主题包时
    /// 斑马线自动跟着变。DBeaver 那颗结果集专属的 `cell.odd.background`（235,245,255）
    /// 不再照抄：组件的 `.stripe(true)` 已按 `table_even` 铺奇数行，本仓原先再叠一层产品槽
    /// 色值只会让同一件事有两个来源。
    ///
    /// 【选中行】组件**不给**选中行铺色（`table/state.rs:2249` 的 `is_selected` 只决定
    /// 行底线与 hover 抑制），而 DBeaver 的选中行是整行色（`LightGrid` 逐格按
    /// `getCellBackground(selected)` 取色），所以整行选中在这里铺：先判选中、再判斑马
    /// （同一行只能是其中一个分支——选中色压掉斑马纹）。色槽与「选中单元格」同源
    /// （`tokens.table_active`），两者不会各说各话。
    fn render_tr(
        &mut self,
        row_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let tr = div().id(("row", row_ix));
        if self.selected_row == Some(row_ix) {
            tr.bg(theme.tokens.table_active)
                // 回归哨兵：颜色在 headless 下读不到，用例只能坐实「选中行分支真的在跑」
                .debug_selector(|| "editor-result-row-selected".to_string())
        } else if row_ix % 2 == 1 {
            // 显式铺一遍组件同款色：delegate 的样式会在组件链末尾经 `refine_style` 重放
            // （vendor `table/state.rs:2258` 取快照、`:2278` 重放），铺的是同一个
            // `table_even`，视觉与组件 stripe 完全一致；保留哨兵是为了坐实这个分支真在跑。
            tr.bg(theme.tokens.table_even)
                // 回归哨兵：颜色在 headless 下读不到，用例只能坐实「奇数行分支真的在跑」
                //（否则 delegate 的 `render_tr` 没被调用时静默失效）
                .debug_selector(|| "editor-result-row-odd".to_string())
        } else {
            tr
        }
    }

    /// 表头行容器：补一条下边线（对齐 DBeaver `header.border`）
    ///
    /// DBeaver 结果集的表头与数据区之间有一条 `resultset.color.header.border` 画的分隔线
    /// （默认 `COLOR_WIDGET_DARK_SHADOW`，比每行的 `lines.normal`(240,240,240) 深得多，
    /// `plugin.xml:1258`）；本仓组件只给**数据行**画下边（vendor `table/state.rs:2274-2276`），
    /// 表头行自身只有铺底 + 文字色、没有边框（`:2059-2069`），所以表头底部原本是断开的。
    ///
    /// 为什么这里能改：组件先铺默认样式，再 `.refine_style(&style)` 重放 delegate 返回元素的
    /// 样式快照（`table/state.rs:2056` 取快照、`:2069` 重放），故 delegate 给的边框落在最上层。
    /// 取 `colors.border`（不是 `table.row.border`）：表头线要比行底线深一档才起到分隔作用，
    /// 而本仓没有 `COLOR_WIDGET_DARK_SHADOW` 这个语义面，`border` 是唯一正确的描边槽。
    fn render_header(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let border = product_tokens::get(cx).result_header_border(cx.theme());
        div()
            .id("editor-result-header-band")
            // 回归哨兵：边框宽度在 headless 下读不到，只能坐实「这个 hook 真被组件调用了」
            //（不被调用时边框静默缺失，与「组件没这入口」无法区分）
            .debug_selector(|| "editor-result-header-band".to_string())
            .border_b(ui::HAIRLINE)
            .border_color(border)
    }

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
        let menu = crate::grid::menu::context_menu(self, row_ix, menu, window, cx);
        // 【DBeaver 对齐】「面板」子菜单挂在这里（真值 `ResultSetViewer.java:3231-3233`）：
        // `context_menu` 内部有转置 / 行号槽两处早退，只有这一层是唯一出口。
        crate::grid::menu::append_panels_submenu(self, menu, window, cx)
    }

    /// 列宽自适应（转发到 [`crate::grid::fit`]）：组件库双击 resize handle 时问这里要
    /// 这一列的内容宽度（默认 trait 实现返回 `None` = 双击没反应）
    fn fit_column(
        &mut self,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Option<Pixels> {
        crate::grid::fit::fit_column(self, col_ix, window, cx)
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
        // 【列序】拖拽改了显示序 → 折算缓存跟上（否则下一次 `refresh` 重建
        // `col_groups` 时会退回隐藏列序，把用户刚拖出来的顺序抹掉、且表头与
        // 单元格错位——这是本批修掉的既有隐患）
        self.rebuild_column_order();
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

    /// 【分页】视口首行回写：状态栏「上页 / 下页」按它算目标行（组件库在可见范围
    /// 变化时频繁调用，这里只赋一个 `usize`，不碰数据）。
    fn visible_rows_changed(
        &mut self,
        visible_range: Range<usize>,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) {
        // 【D10-7 翻页键】首行与容量一起回写：`PAGE_UP` / `PAGE_DOWN` 的「一页」
        // 就是视口容量（DBeaver `getVisibleGridHeight()`），所以 `len` 必须跟着存。
        self.visible_top = visible_range.start;
        self.visible_len = visible_range.len();
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
                    .paddings(cell_paddings())
                    .resizable(true),
                _ => Column::new("transpose-value", "值")
                    .width(ui::RESULT_COLUMN_WIDTH)
                    .min_width(ui::RESULT_COLUMN_MIN_WIDTH)
                    .paddings(cell_paddings())
                    .resizable(true),
            };
        }
        if Self::is_row_number(col_ix) {
            // 行号槽：窄、钉在左边、不可拖宽/移动——它是视图辅助，不是数据列
            return Column::new("row-no", "#")
                .width(ui::RESULT_ROW_NUMBER_WIDTH)
                .min_width(ui::RESULT_ROW_NUMBER_WIDTH)
                .paddings(cell_paddings())
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
        // 【类型默认宽】这一档按**列类型**取，不再一律 128：数字窄、时间戳宽、uuid 最宽；
        // 驱动没给类型时 `column_kind` 是 `Unknown`，仍是 128（与既有口径一致）
        let width = self
            .column_widths
            .get(&name)
            .copied()
            .unwrap_or_else(|| default_column_width(self.column_kind(col_ix)));
        Column::new(format!("col-{index}"), name)
            .width(width)
            .min_width(ui::RESULT_COLUMN_MIN_WIDTH)
            .paddings(cell_paddings())
            // 【排序图标 2026-11-27】**故意不设 `sort`**：组件 `render_sort_icon`
            // （`vendor/gpui-component/src/table/state.rs:2087-2131`）画的是 12px 的 Lucide
            // 「一实一虚双 chevron」，而真值是 16px 的「单支带杆箭头」（三态各一张 SVG）。
            // 组件没有图标字段也没有对应 builder，delegate 也改不了那个独立 slot，
            // 所以这里关掉组件槽（`sort: None` → `render_sort_icon` 早退返回 `None`），
            // 由 `grid/header.rs` 自绘真值图标（`grid/sort_icon.rs` 内嵌 SVG）。
            // 排序态本身仍在 delegate 自持（`sort_mark` / `apply_sort`），点击入口是自绘图标
            // 与两条列头菜单；真值点列名只选整列（`on_col_head_click` 同样只 `set_selected_col`）。
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
