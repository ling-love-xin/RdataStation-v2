//! 表头渲染域（`grid/` 第八块）：`TableDelegate::render_th`（截断 + 悬停全文 +
//! 类型小标签 + 筛选图标 + **排序三态图标** + 列头菜单）+ 表头 tooltip 分块文案。
//! 排序图标**自绘**（`grid/sort_icon.rs` 内嵌 DBeaver 真值 SVG）：组件那个 12px 双 chevron
//! 槽既不匹配真值图形也没有定制入口，见 `table_impl::column()` 里「故意不设 `sort`」那段。
//! 焦点列额外画四边框（`focus_border_overlay`，真值 `GridColumnRenderer.java:96-104`）。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{ButtonVariants as _, DropdownButton};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::table::TableState;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::column_menu::{
    ColumnMenuHooks, build_flat_menu, build_plan_menu, column_menu_plan,
};
use super::delegate::ResultGridDelegate;
use super::menu::{ContextTarget, column_menu_items};
use super::stats::{column_stats, stats_lines};
use crate::rowset::RowRef;
use crate::store::ColumnKind;
use crate::ui;
use workbench_shell::model::ColumnAnnotation;
use workbench_shell::result_style::ColumnHeaderExtra;
/// 表头悬停的分块内容：第一行「列名 · 类型」，后续行字段元数据（来源 / 注释 / 可空 / 默认）。
///
/// 返回 `(标题行, 元数据行列表)`。注释缺失 = 空列表（与单行 tooltip 等效）。
/// 多行不拼 `\n` 字符串：tooltip 容器是 `whitespace_normal`，换行会被折叠成空格，
/// 由调用方逐行 `child` 渲染成块。
///
/// 2026-10-30：已接线至 `render_th`（悬停 tooltip）并补测试锁定（`grid/tests.rs`
/// `header_tooltip_lines_build_title_and_meta_blocks`），原【P1】标记移除。
pub(crate) fn header_tooltip_lines(
    name: &str,
    label: Option<&str>,
    annotation: Option<&ColumnAnnotation>,
) -> (String, Vec<String>) {
    let title = match label {
        Some(label) => format!("{name} · {label}"),
        None => name.to_string(),
    };
    let Some(annotation) = annotation else {
        return (title, Vec::new());
    };
    let mut meta = Vec::new();
    // 来源：表达式列两处都是 `None` → 不追加这一行
    match (&annotation.source_table, &annotation.source_column) {
        (Some(table), Some(column)) => meta.push(format!("来源：{table}.{column}")),
        (Some(table), None) => meta.push(format!("来源表：{table}")),
        _ => {}
    }
    if let Some(comment) = annotation.comment.as_deref().filter(|c| !c.is_empty()) {
        meta.push(format!("注释：{comment}"));
    }
    if let Some(data_type) = annotation.data_type.as_deref().filter(|d| !d.is_empty()) {
        meta.push(format!("类型：{data_type}"));
    }
    if let Some(nullable) = annotation.nullable {
        meta.push(if nullable {
            "可空：是".to_string()
        } else {
            "可空：否".to_string()
        });
    }
    if let Some(default_value) = annotation.default_value.as_deref() {
        meta.push(format!("默认：{default_value}"));
    }
    (title, meta)
}

/// 列头第二行该画的文本（`columnHeaderExtra` × 列注解）。
///
/// 对齐 DBeaver `SpreadsheetPresentation.java:3422-3436` 的 `getDescription`：
/// - `Nothing` → 不画（该处 `return null`）；
/// - `Description` → 字段注释（`attributeBinding.getDescription()`）；
/// - `DataType` → 完整类型名（`columnTypeDescriptions` = `DBUtils.getFullTypeName`）。
///
/// 空 / 纯空白的文本按「没有」处理（DBeaver 侧 `null` 与空串在渲染上都是不占位）。
pub(crate) fn column_header_extra_text(
    extra: ColumnHeaderExtra,
    annotation: Option<&ColumnAnnotation>,
) -> Option<String> {
    let text = match extra {
        ColumnHeaderExtra::Nothing => return None,
        ColumnHeaderExtra::Description => annotation?.comment.as_deref(),
        ColumnHeaderExtra::DataType => annotation?.data_type.as_deref(),
    };
    text.filter(|text| !text.trim().is_empty())
        .map(str::to_string)
}
/// 焦点列表头格的四边框（真值 `GridColumnRenderer.java:96-104`，
/// `PAINT_COLUMN_FOCUS_BORDER = true`，四边全画）。
///
/// 用**绝对定位覆盖层**而不是 `.border_t/b/r`：GPUI 的 border 占盒内空间
/// （上下各 1px 会把表头文字顶偏，切到另一列时整条表头抖一下）；真值也是把线画在
/// 格边界上、不挤内容（`gc.drawLine` 直接描在 `bounds` 上）。
///
/// 右边界落在「列宽 − 组件表头右内距 4px」处：组件给表头内容区留了 `offset_pr`
/// （`vendor/gpui-component/src/table/state.rs:2171-2176`，`XSmall` 档 = 4px），
/// 这 4px 只有改 vendor 才能消掉（见 `ui::RESULT_FIT_HEADER_CHROME` 的逐项构成）。
fn focus_border_overlay(col_ix: usize, color: Hsla) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .border_1()
        .border_color(color)
        .debug_selector(move || format!("editor-result-th-focus-{col_ix}"))
}

/// 表头单元格：组件默认只画列名（`div().size_full().child(name)`），这里补三件事——
/// **截断**、**悬停全文**、**列类型小标签**。列名常常比默认列宽（128px）长，截断之后
/// 就没有第二条知道它是什么的路（单元格取值有悬停，表头同样需要）。
///
/// 类型小标签摆在列名**右侧**并且 `flex_none`：先截断的总是列名——反过来的话，
/// 窄列上被省略号吃掉的恰好是“类型”（那正是想看它的时候）。
///
/// 行号槽（`#`）不挂悬停也不挂标签——它不是数据。`debug_selector` 是给用例找这个表头用的
/// （`.id(...)` 不登记坐标）：表头的排序箭头就在它右端，直调 `perform_sort` 验不到那段几何。
///
/// 2026-10-29 目录拆分：由 `TableDelegate::render_th` 薄转发至此（Rust trait impl 不可分块）。
pub(crate) fn render_th(
    delegate: &mut ResultGridDelegate,
    col_ix: usize,
    _window: &mut Window,
    cx: &mut Context<TableState<ResultGridDelegate>>,
) -> impl IntoElement {
    let name = if ResultGridDelegate::is_row_number(col_ix) {
        "#".to_string()
    } else if delegate.transposed {
        // 转置视图固定两列：字段 / 值（列名不跟原列走）
        match col_ix {
            1 => "字段".to_string(),
            _ => "值".to_string(),
        }
    } else {
        delegate
            .columns
            .get(delegate.data_column(col_ix - 1))
            .cloned()
            .unwrap_or_default()
    };
    // 类型缺失时 `label` 是 `None`：这一档与今天一模一样（表头就一个列名）
    let label = delegate.type_label(col_ix).map(str::to_string);
    // 【筛选】列筛选指示要按数据列查，这里先折算（行号槽 / 转置视图没有列筛选）
    let column = if ResultGridDelegate::is_row_number(col_ix) || delegate.transposed {
        usize::MAX
    } else {
        delegate.data_column(col_ix - 1)
    };
    let muted = cx.theme().colors.muted_foreground;
    // 表头列分隔线色：与表头底边线同源（`state.rs:2114-2115` 用 `theme.border`），
    // 对应 DBeaver 的 `getHeaderBorder(element)`（同一色画列右边界与底边，
    // `GridColumnRenderer.java:106-108`）。先取出来：后面构造列头菜单要用 `cx` 的可变借用。
    let header_sep = cx.theme().colors.border;

    // 【焦点列 2026-11-27】真值焦点列表头格画四边框（`GridColumnRenderer.java:96-104`）。
    // 焦点列 = 选区**当前格**所在列：读 delegate 自持的 `CellRange::cursor()`（含行号槽的
    // 组件列下标，与 `col_ix` 同口径）。不能走 `cx.entity().read(cx)` 拿
    // `TableState::selected_cell`——这里 `cx` 是 `Context<TableState<Self>>`、同实体已被
    // `&mut` 借出，再读会撞借用。
    // 行号槽不是 `GridColumn`，真值的 `getFocusColumn()` 永远不会指到它，所以排除 col 0。
    let focus_col = delegate.range_focus().map(|(_, col)| col);
    let is_focused = focus_col == Some(col_ix) && !ResultGridDelegate::is_row_number(col_ix);
    // 真值线色是 SWT 系统色 `COLOR_WIDGET_NORMAL_SHADOW`（平台给值，dbeaver-ref 里取不到
    // 具体 RGB）；本仓取语义最近的 `table_active_border`——组件画单元格焦点框用的同一颗
    // （`vendor/gpui-component/src/table/state.rs:2695-2714`），不写死色值。
    let focus_border = cx.theme().table_active_border;

    // 【A3】类型族 badge：浅底 + 彩色符号（123/Aa/◷/✓/01）。
    // 位置在列名前；类型族未知不画 badge。浅底 = 族色 18% 透明度，文字 = 族色。
    let kind = if ResultGridDelegate::is_row_number(col_ix) || delegate.transposed {
        ColumnKind::Unknown
    } else {
        delegate.column_kind(col_ix)
    };
    let theme = cx.theme();
    let type_badge = ResultGridDelegate::type_kind_sym(kind).map(|sym| {
        // 族色 → 浅底 + 文字色
        let color = match kind {
            ColumnKind::Integer | ColumnKind::Float | ColumnKind::Decimal => theme.colors.primary,
            ColumnKind::Text | ColumnKind::Uuid | ColumnKind::Json | ColumnKind::Array => {
                theme.colors.success
            }
            ColumnKind::Timestamp | ColumnKind::Date | ColumnKind::Time => theme.colors.info,
            ColumnKind::Boolean => theme.colors.warning,
            ColumnKind::Binary => theme.colors.secondary,
            ColumnKind::Unknown => theme.colors.muted_foreground,
        };
        div()
            .bg(color.opacity(0.18))
            .text_color(color)
            .px_1()
            // 字号走常量表：比列名（`text_xs` 12px）小一档，`grid/fit.rs` 量 badge 符号宽
            // 用的是同一个字号（两边不一致会让双击自适应算出的宽度和实际渲染差一两个字符）
            .text_size(gpui_kit::rems(crate::ui::RESULT_HEADER_TYPE_FONT_SIZE))
            .rounded(gpui_kit::px(3.))
            .flex_none()
            .child(SharedString::from(sym))
    });

    // 【B1】列名主行：类型 badge + 列名(flex_1) + 筛选图标 + 排序三态 + 三角菜单
    //
    // 【排序图标 2026-11-27】**改回自绘**：此前（2026-11-18）为去掉不可点的假指示器，
    // 交给组件 `render_sort_icon`（`table/state.rs:2087-2131`）。复核真值后确认那条路
    // 画的是 12px 的 Lucide「一实一虚双 chevron」，而真值是 16px「单支带杆箭头」三态
    // （`GridColumnRenderer.java:43-50`、`:212-226`，三张 SVG 见 `grid/sort_icon.rs`），
    // 且组件没有图标字段 / builder 可换。现在组件槽已关（`column()` 不设 `sort`），
    // 这里自绘真值图标并挂点击（真值 `LightGrid.java:3404-3414` 点排序图标 →
    // `Event_ChangeSort` → `changeSorting` → `toggleSortOrder`，即升→降→清空）。
    // 元素间距取真值 `IMAGE_SPACING = 3`（此前 `gap_1()` 是 4）。
    let sort_control = if ResultGridDelegate::is_row_number(col_ix) || delegate.transposed {
        // 行号槽 / 转置视图没有列级排序（真值同样只对 `isElementSupportsSort` 的列画）
        None
    } else {
        let sort = delegate.sort_mark(column);
        Some(
            div()
                .id(("editor-result-col-sort", col_ix))
                .debug_selector(move || format!("editor-result-sort-icon-{col_ix}"))
                .flex_none()
                // 截住冒泡：组件列头 `on_click` 会选整列，点排序图标不该顺带选列
                // （真值这两个事件也是分开的：`Event_ChangeSort` vs 列头按下）
                .on_click(cx.listener(move |table, _, _, cx| {
                    cx.stop_propagation();
                    // 真值 `toggleSortOrder`（`SpreadsheetPresentation.java:1567-1579`）：
                    // 未排 → 升 → 降 → 清空
                    let next = match table.delegate().sort_state() {
                        Some((sorted, false)) if sorted == column => Some((column, true)),
                        Some((sorted, true)) if sorted == column => None,
                        _ => Some((column, false)),
                    };
                    table.delegate_mut().apply_sort(next);
                    cx.notify();
                }))
                .child(super::sort_icon::sort_icon(sort)),
        )
    };
    let main_row = div()
        .h_flex()
        .items_center()
        // 元素间距 3px = 真值 `GridColumnRenderer.IMAGE_SPACING`（类型图标↔列名、
        // 列名↔排序图标、排序图标↔筛选三角三处同值）；此前 `gap_1()` 是 4px
        .gap(ui::RESULT_HEADER_ICON_SPACING)
        .w_full()
        .min_w_0()
        .truncate()
        // 给用例读主行几何（表头带 22px 内容高，主行被挤成 0 高时列名整行看不见——
        // 2026-11-08 的"字段被遮挡"就是这个；断言见 `grid/tests.rs`
        // `header_main_row_keeps_a_readable_height`）
        .debug_selector(move || format!("editor-result-th-main-{col_ix}"))
        .when_some(type_badge, |row, badge| row.child(badge))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                // 给用例量「列名拿到多少宽度」：菜单若回到流内会直接吃掉这一块
                .debug_selector(move || format!("editor-result-th-name-{col_ix}"))
                .child(SharedString::from(name.clone())),
        )
        .when(delegate.has_column_filter(column), |row| {
            row.child(
                // 筛选指示用图标而不是 `▽` 字符：与排序图标同尺寸（16px 槽内 12px 字形），
                // 语义上比一个三角字符明确。DBeaver 这里画的是 `IMAGE_FILTER`
                // （`GridColumnRenderer.java:46`，`UIIcon.DROP_DOWN`）——它是下拉入口的
                // 指示；我们的筛选入口是列头菜单，「此列已筛选」用漏斗更准确。
                Icon::new(IconName::Funnel)
                    .size_3()
                    .flex_none()
                    .text_color(muted),
            )
        })
        // 排序三态（真值 16px，见上方 `sort_control` 的取值）
        .when_some(sort_control, |row, sort| row.child(sort));

    // 【DBeaver 对齐】列头附加第二行：`columnHeaderExtra` 三档决定画什么——
    // `Nothing` 不画 / `Description` 画字段注释 / `DataType` 画完整类型名
    // （`SpreadsheetPresentation.java:3422-3436` 的 `getDescription`）。
    // 文字为空（没报注释 / 没报类型）的列不占位，与该处返回 `null` 一致。
    // 两行要装得下，表头带由 `ResultGridDelegate::header_row_height()` 抬到 40px
    // （内容高 36 = 两行各 18px）；没抬之前这行会被组件的 `overflow_hidden` 裁掉。
    let extra_text = if ResultGridDelegate::is_row_number(col_ix) || delegate.transposed {
        // 行号槽没有列注解；转置视图的类型是混合的（DBeaver 侧转置列同样不挂
        // `DBDAttributeBinding`，`getDescription` 返回 null）
        None
    } else {
        column_header_extra_text(
            delegate.column_header_extra(),
            delegate
                .column_annotations
                .get(col_ix - 1)
                .cloned()
                .flatten()
                .as_ref(),
        )
    };
    let extra_line = extra_text.map(|text| {
        div()
            .w_full()
            .min_w_0()
            .truncate()
            .text_xs()
            .text_color(muted)
            // 给用例读「第二行真的上屏了」：表头没抬高时这一行会被组件裁掉
            .debug_selector(move || format!("editor-result-th-extra-{col_ix}"))
            .child(SharedString::from(text))
    });

    // 【D10-3】行号槽表头格（组件第 0 列）= DBeaver 的「左上交角」：点一下全选所有格
    // （`LightGrid.java:3174-3184` → `selectAllCellsInternal` `:4272-4296`）。
    //
    // 它同样不挂列头菜单（行号槽没有列级动作），所以在接菜单之前就返回。
    // 组件把 `on_col_head_click` 挂在列头格**外层**（vendor `state.rs:2139-2141`），
    // 那条路会报 `SelectColumn(0)`，面板收到就是「按此列排序」——行号槽没有可排的键，
    // 所以这里 `stop_propagation` 把冒泡截住（与列头菜单 guard 同一手法，见 `:311-322`；
    // `on_click` 是 `StatefulInteractiveElement` 方法，元素必须先有 `.id()`）。
    if ResultGridDelegate::is_row_number(col_ix) {
        let table = cx.entity().clone();
        return div()
            .id(("editor-result-th-corner", col_ix))
            .v_flex()
            .size_full()
            .min_w_0()
            .justify_center()
            // 左右内距 6px：与单元格同一个口径（DBeaver `GridColumnRenderer.LEFT/RIGHT_MARGIN = 6`）
            .px(ui::RESULT_CELL_PADDING_X)
            // 首列的左边界就是表格左边缘，不画（DBeaver 同口径，`LightGrid.java:2426-2437`
            // 跳过首个边界）。
            .debug_selector(move || format!("editor-result-th-{col_ix}"))
            .child(main_row)
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                table.update(cx, |state, cx| {
                    // 【D10-10】当前格 = 视口首行 × 首个数据列（DBeaver `:3183-3184`：
                    // `focusColumn = getColumn(new Point(rowHeaderWidth + 1, 1))` +
                    // `focusItem = getTopIndex()`）。`getTopIndex()` 就是视口首行，
                    // **不滚动**——所以走不带滚动的 setter，否则 `Center` 会把首行拽到正中。
                    let top_row = state.visible_range().rows().start;
                    // 空表（没有行或没有列）什么都不选，与 DBeaver 那条 `return null` 一致
                    if state.delegate_mut().select_all_cells(top_row) {
                        // 读回 delegate 的当前格（它已把 top_row 夹到有效行内），保证
                        // 组件 / 矩形 / 状态行三份是同一个值。
                        if let Some(range) = state.delegate().cell_range() {
                            let (row, col) = range.cursor();
                            state.set_selected_cell_without_scroll(row, col, cx);
                        }
                        cx.notify();
                    }
                });
            })
            .into_any_element();
    }

    // 转置视图只画列名（类型是混合的，注释表头没有意义）；空列名同理。
    // 这几档不挂列头菜单，所以在接菜单之前就返回（否则会白构造一份菜单项）。
    if delegate.transposed || name.is_empty() {
        return div()
            .v_flex()
            .size_full()
            .min_w_0()
            .relative()
            .justify_center()
            // 左右内距 6px：与单元格同一个口径（DBeaver `GridColumnRenderer.LEFT/RIGHT_MARGIN = 6`），
            // 列名与下面格子的文字才落在同一条竖线上；来源是 `ui::RESULT_CELL_PADDING_X`
            .px(ui::RESULT_CELL_PADDING_X)
            // 列边界线画在**左**边：表头内容区的右端被组件 `offset_pr` 占去 4px
            // （`state.rs:2171-2176`，`XSmall` 档 `table_cell_padding().right = 4`），
            // 右边界线会落在列内侧；左边与列左边界重合（列级内距已置零，`table_impl::cell_paddings`）。
            // 首列（行号槽 / 转置首列）的左边界就是表格左边缘，不画（DBeaver 同口径，
            // `LightGrid.java:2426-2437` 跳过首个边界）。色与表头底边线同源（`theme.border`）。
            .when(col_ix > 0, |this| {
                this.border_l(ui::HAIRLINE).border_color(header_sep)
            })
            .debug_selector(move || format!("editor-result-th-{col_ix}"))
            .child(main_row)
            .when_some(extra_line, |head, line| head.child(line))
            .when_some(
                is_focused.then(|| focus_border_overlay(col_ix, focus_border)),
                |head, overlay| head.child(overlay),
            )
            .into_any_element();
    }
    // 【列头菜单 2026-11-26】真值有**两条**列头菜单入口，形态不同：
    // ① 列头右端 `drop_down` 图标 → `showColumnMenu:2988-2999`（排序三项 + `fillFiltersMenu`，**平铺**）
    // ② 列头右键 → `fillContextMenu(COLUMN_HEADER):3151-3300`（`复制列名` 顶层 + `筛选▸` + `排序▸` … **分组**）
    // 两条入口共用 `column_menu::ColumnMenuHooks` 与 `apply_item`：动作分派只有一份，
    // 形状差异只在「平铺 / 分组」。
    let column_menu_target = ContextTarget {
        value: String::new(),
        column_name: name.clone(),
        column,
        row: 0,
        rows: delegate.view_len(),
        frozen: delegate.is_frozen(column),
        hidden_columns: delegate.hidden.len(),
        insight: delegate.insight_available(),
        // 列头菜单只做列级动作：整表「分析此结果集」不在这一组（见 `column_menu_items`）
        analyze: false,
        persist: false,
        table: delegate.result_table.clone(),
        column_filter_active: delegate.has_column_filter(column),
        column_values: delegate.column_distinct_values(column),
        sort_active: delegate
            .sort_state()
            .is_some_and(|(sort_col, _)| sort_col == column),
        // 列头菜单没有行身份：行级三项只出现在单元格右键菜单（`context_menu_items`），
        // 这里占位即可，不参与行级菜单项判定。
        row_ref: RowRef::default(),
        editable: delegate.editable,
        is_deleted: false,
        // 【第 67 批】列头落点没有「这一格」：重置单元格只在单元格右键（`context_menu_items`）
        cell_changed: false,
        // 列头菜单不摆选中统计三颗（它们只在单元格右键的 layout 段）
        selection_stat_rows: false,
        selection_stat_columns: false,
        selection_stat_cells: false,
        // 列头菜单同样不摆布局段（呈现方式组只在单元格右键）
        presentation: delegate.presentation_view(),
        record_mode: delegate.transposed,
        // 同上：筛选条显隐项也不在列头菜单里
        show_filter_panel: delegate.show_filter_panel(),
    };
    // 动作句柄 + 预计算文本（复制整列 / 统计正文都在闭包外算好：菜单 builder 是 `Fn`，
    // 可能被调多次，且不能再借 `delegate`）
    let column_menu_hooks = ColumnMenuHooks {
        table: cx.entity().clone(),
        layout_queue: delegate.action_queue.clone(),
        filter_queue: delegate.action_queue.clone(),
        sort_down: delegate.on_sort_down.clone(),
        column_stats: delegate.on_column_stats.clone(),
        insight_column: delegate.on_insight_column.clone(),
        column_filter: delegate.on_column_filter.clone(),
        column_name: name.clone(),
        copy_column_text: delegate.column_text(column),
        stats_text: stats_lines(&column_stats(&delegate.rows, column)).join("\n"),
        // `面板▸`：与数据格右键共用同一份注入构造器（真值三个落点共用 `fillPanelsMenu()`，
        // `ResultSetViewer.java:3224-3234`）；`None` 时列头也不摆
        panels: delegate.panels_menu_builder.clone(),
    };
    // ① 下拉三角内容：平铺（真值 `showColumnMenu` 就是「排序 + 筛选」平铺；
    // 本仓在此之上还带列级增强项：复制此列 / 冻结 / 隐藏 / 列统计 / 显示全部 / 洞察）
    let flat_items = column_menu_items(&column_menu_target);
    let flat_hooks = column_menu_hooks.clone();
    // ② 右键计划：分组（`复制列名` 顶层 + `筛选▸` / `排序▸` / `列操作▸`）
    let plan = column_menu_plan(&column_menu_target);
    let plan_hooks = column_menu_hooks.clone();
    // 【DBeaver 对齐】列头右端的 `drop_down` 图标**常显**：真值 `IMAGE_FILTER = UIIcon.DROP_DOWN`
    // （`GridColumnRenderer.java:46`），`icons/misc/drop_down.svg` 是 `#3387C5` 蓝色实心三角、
    // 16×16，不随 hover 显隐（旧版 `invisible + group_hover` 是本仓自造，已去掉）。
    // 外层带 id 的 div 截住冒泡：组件列头点击会排一次序（`on_col_head_click`），
    // 点箭头菜单不能顺带把列排了。
    let main_row = main_row.child(
        div()
            .id(("editor-result-col-menu-guard", col_ix))
            .flex_none()
            .debug_selector(move || format!("editor-result-col-menu-{col_ix}"))
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                DropdownButton::new(("editor-result-col-menu", col_ix))
                    .ghost()
                    // 【DBeaver 对齐 2026-11-27】真值槽宽 19px（16 图标 + 3 间距，
                    // `GridColumnRenderer.java:44-46`）；组件 `.small()` 实测 28px（白吃 9px 列名宽）。
                    // 这个按钮只有 caret（无 label / children）→ 走 icon-only 分支，`Size::Size(v)`
                    // 直接决定 19×19 的盒子（`button.rs:693-700`），里面的 `Caret` 恒 16px 居中，
                    // 左右各余 1.5 就是真值那 3px 间距。推导见 `ui::RESULT_HEADER_TRIANGLE_SLOT`。
                    .with_size(ui::RESULT_HEADER_TRIANGLE_SLOT)
                    .menu_accessibility_label(format!("列菜单：{name}"))
                    .dropdown_menu(move |menu, _window, _cx| {
                        build_flat_menu(menu, &flat_items, &flat_hooks)
                    }),
            ),
    );
    // 悬停给全名 + 类型 + 字段元数据（来源 / 注释 / 可空 / 默认）：
    // 有注释才追加分块行（没有 = 与单行一致）；注释块里的类型是 NavCache 原始名，
    // 与表头小标签（渲染层语义标签）并存不冲突。
    let annotation = delegate
        .column_annotations
        .get(col_ix - 1)
        .cloned()
        .flatten();
    let tip_name = name.clone();
    let tip_label = label.clone();
    let head = div()
        .v_flex()
        .size_full()
        .min_w_0()
        .justify_center()
        // 左右内距 6px：与单元格同口径（DBeaver `GridColumnRenderer.LEFT/RIGHT_MARGIN = 6`），
        // 表头文字与格子文字左边缘对齐；组件层内距已置零（`table_impl::cell_paddings`），
        // 单侧宽度取 `ui::RESULT_CELL_PADDING_X`（列宽自适应用的是同一个来源）
        .px(ui::RESULT_CELL_PADDING_X)
        // 列边界线：同 early-return 分支（画左边、跳过首列），数据区的竖分隔线
        // 因此从表头一直贯到末行
        .when(col_ix > 0, |this| {
            this.border_l(ui::HAIRLINE).border_color(header_sep)
        })
        // 与early-return 分支同一个探针选择器：数据列（带列名 / 带列头菜单）也要能被用例
        // 读到整格几何——接列头菜单时这里漏挂过，`editor/src/view/tests/results_a.rs`
        // 的「表头右端真点击」随即取不到坐标（`editor-result-th-1` 恒为 None）。
        .debug_selector(move || format!("editor-result-th-{col_ix}"))
        .child(main_row)
        .when_some(extra_line, |head, line| head.child(line));
    // 【列头右键】挂在整个表头格上：`ContextMenu` 在**冒泡**阶段 `stop_propagation`
    // （`vendor/gpui-component/src/menu/context_menu.rs:404-410`），内层先命中 →
    // 压掉 `table-inner` 的数据行菜单，右键落进列头就只出列头菜单。
    // 【DBeaver 对齐】列头 hover 用**选中色**（`GridColumnRenderer.java:96-97`：底色取
    // `getHeaderBackground(element, selected || hovering)`，hover 与选中同一档色）。
    // 组件自己的列头没有 hover 分支（`table/state.rs:2120` 那个是行号槽分割线的 hover），
    // 所以在内容层自绘一层背景；色先取出来，闭包是 `Fn` 不能被借的 `cx` 反复用。
    let hover_bg = cx.theme().tokens.table_active;
    let head = head
        .id(("editor-result-th", col_ix))
        .hover(move |this| this.bg(hover_bg))
        .tooltip(move |window, cx| {
            let (title, meta) =
                header_tooltip_lines(&tip_name, tip_label.as_deref(), annotation.as_ref());
            // `Tooltip::element` 的 F 是 Fn（可能被调多次）：body 必须**每次现建**，
            // 只 clone 捕获的 title / meta，不能把构建好的 body move 进闭包
            Tooltip::element(move |_, _| {
                let mut body = div()
                    .v_flex()
                    .gap_1()
                    .max_w(rems(ui::RESULT_TOOLTIP_MAX_WIDTH))
                    .text_xs()
                    .child(SharedString::from(title.clone()));
                for line in meta.clone() {
                    body = body.child(div().text_color(muted).child(SharedString::from(line)));
                }
                body
            })
            .build(window, cx)
        })
        .context_menu(move |menu, window, cx| {
            build_plan_menu(menu, plan.clone(), &plan_hooks, window, cx)
        });
    // `ContextMenu<Div>` 在 gpui-fast 下只实现 `Element`（`IntoElement` 的 impl 被
    // `#[cfg(not(feature = "gpui-fast"))]` 门掉），再包一层 `div` 回到 `Div`：
    // `ContextMenu` 自身以绝对定位挂子元素、不参与流，视觉与布局不变。
    // 返回类型被上面的 corner 早退钉成 `AnyElement`，这里同样 `into_any_element`。
    // 外层 `.relative()` 是焦点列四边框的定位锚（覆盖层 `absolute().inset_0()`）。
    div()
        .size_full()
        .relative()
        .child(head)
        .when_some(
            is_focused.then(|| focus_border_overlay(col_ix, focus_border)),
            |this, overlay| this.child(overlay),
        )
        .into_any_element()
}
