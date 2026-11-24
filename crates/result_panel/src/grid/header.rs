//! 表头渲染域（`grid/` 第八块）：`TableDelegate::render_th`（截断 + 悬停全文 +
//! 类型小标签 + 筛选图标 + 列头菜单）+ 表头 tooltip 分块文案。
//! 注：排序三角由组件自己的图标槽画（`table/state.rs` 的 `render_sort_icon`），本文件不再自绘。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{ButtonVariants as _, DropdownButton};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::table::TableState;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::delegate::ResultGridDelegate;
use super::menu::{ContextAction, ContextTarget, column_menu_items};
use super::stats::{column_stats, stats_lines};
use crate::ports::ResultRequest;
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
            ColumnKind::Text | ColumnKind::Uuid | ColumnKind::Json => theme.colors.success,
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

    // 【B1】列名主行：类型 badge + 列名(flex_1) + 筛选图标
    //
    // 排序指示不再自绘（2026-11-18）：组件在每个可排序列的右端留了 20px 图标槽
    // （`state.rs:1849-1871` 的 `render_sort_icon`：`SortAscending` / `SortDescending` /
    // 未排时 `ChevronsUpDown`），图标本身就是**可点的排序控件**（`:1863-1865` 调
    // `perform_sort` → 我们的 delegate `perform_sort` → `apply_sort`），状态由
    // `table_impl::column()` 的 `.sort(self.sort_mark(index))` 回灌。原先另画的一套
    // `↑/↓` 文字是重复指示器，而且是不可点的假控件，本批删掉。
    //
    // 【列头菜单 2026-11-08】悬停表头才出现的组 key：菜单默认**不占列宽**（绝对定位
    // 浮在列名右端，`invisible`），只有整格 hover 才显形。列头菜单恒吃 24px 会把窄列的
    // 列名挤到读不出（布尔列 96px 时只剩 37px），而它其实一直是"想起来了才点"的控件。
    let header_hover_group: SharedString = format!("editor-result-th-hover-{col_ix}").into();
    let main_row = div()
        .h_flex()
        .items_center()
        .gap_1()
        .w_full()
        .min_w_0()
        .truncate()
        // 菜单用 `absolute` 贴右端，需要本行当定位父
        .relative()
        // 整格 hover 组（菜单显形的触发源）；先例见 vendor `attachment.rs` 的移除按钮
        .group(header_hover_group.clone())
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
                // 筛选指示用图标而不是 `▽` 字符：与组件排序图标同尺寸（`size_3` = 12px），
                // 语义上比一个三角字符明确。DBeaver 这里画的是 `IMAGE_FILTER`
                // （`GridColumnRenderer.java:46`，`UIIcon.DROP_DOWN`）——它是下拉入口的
                // 指示；我们的筛选入口是列头菜单，「此列已筛选」用漏斗更准确。
                Icon::new(IconName::Funnel)
                    .size_3()
                    .flex_none()
                    .text_color(muted),
            )
        });

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
            .justify_center()
            // 左右内距 6px：与单元格同一个口径（DBeaver `GridColumnRenderer.LEFT/RIGHT_MARGIN = 6`），
            // 列名与下面格子的文字才落在同一条竖线上；来源是 `ui::RESULT_CELL_PADDING_X`
            .px(ui::RESULT_CELL_PADDING_X)
            // 列边界线画在**左**边：表头格的盒子右端被组件排序图标槽占去 20px
            // （`state.rs:1910` 的 `render_sort_icon`，图标 + 2px 内距），右边界线会
            // 落在列内侧；左边与列左边界重合（列级内距已置零，`table_impl::cell_paddings`）。
            // 首列（行号槽 / 转置首列）的左边界就是表格左边缘，不画（DBeaver 同口径，
            // `LightGrid.java:2426-2437` 跳过首个边界）。色与表头底边线同源（`theme.border`）。
            .when(col_ix > 0, |this| {
                this.border_l(ui::HAIRLINE).border_color(header_sep)
            })
            .debug_selector(move || format!("editor-result-th-{col_ix}"))
            .child(main_row)
            .when_some(extra_line, |head, line| head.child(line))
            .into_any_element();
    }
    // 【列头菜单】DBeaver 列头箭头：排序 / 复制此列 / 冻结 / 隐藏 / 统计 / 洞察
    // （= 右键菜单的列级子集，见 `column_menu_items`）。外包一层 `stop_propagation`：
    // 组件列头点击会排一次序（on_col_head_click），点箭头菜单不能顺带排。
    // 菜单闭包是 move，依赖 self 的文本（复制整列 / 统计）在闭包外算好再捕获。
    let table = cx.entity().clone();
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
    let menu_items = column_menu_items(&column_menu_target);
    let copy_column_text = delegate.column_text(column);
    let stats_text = stats_lines(&column_stats(&delegate.rows, column)).join("\n");
    let column_name_for_menu = name.clone();
    let on_sort_down = delegate.on_sort_down.clone();
    let on_column_stats = delegate.on_column_stats.clone();
    let on_insight_column = delegate.on_insight_column.clone();
    let on_column_filter = delegate.on_column_filter.clone();
    // 【列布局落盘】动作队列在闭包外取出（owned）：builder 要求 `'static`，闭包里
    // 不能再借 `delegate`
    let layout_queue = delegate.action_queue.clone();
    // 【列头筛选段尾部】四项筛选命令（保存 / 重置 / 全清 / 编辑筛选设置）走同一个动作队列：
    // 它们都由面板执行（存档、清空筛选、打开设置浮层都不是网格自己能做的）
    let filter_queue = delegate.action_queue.clone();
    // 【列头菜单】贴在主行**右端之上**（绝对定位，不参与流）：既不当第二行 child
    // （表头带只有 22px 内容高，任何"第二行"都会把主行挤成 0 高、列名被裁——2026-11-08
    // 截图实证：字只有半截残线），也不占列宽（恒吃 24px 会把窄列列名挤到读不出）。
    // 默认 `invisible`，整格 hover 才显形（`header_hover_group`），
    // 先例：vendor `attachment.rs` 的悬停删除按钮。
    let menu_hover_group = header_hover_group.clone();
    let main_row = main_row.child(
        // guard 层带 id：`on_click` 是 StatefulInteractiveElement 方法（裸 div 只有
        // `on_mouse_down`）；点击落在 guard 上时截住冒泡，列头排序收不到
        div()
            .id(("editor-result-col-menu-guard", col_ix))
            .absolute()
            .top_0()
            .right_0()
            // 同 `editor-result-th-main-{col_ix}`：给用例钉"菜单贴右端、与列名重叠"
            .debug_selector(move || format!("editor-result-col-menu-{col_ix}"))
            .invisible()
            .group_hover(menu_hover_group, |this| this.visible())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                DropdownButton::new(("editor-result-col-menu", col_ix))
                    .ghost()
                    .small()
                    .menu_accessibility_label(format!("列菜单：{column_name_for_menu}"))
                    .dropdown_menu(move |mut menu, _window, _app| {
                        // builder 是 Fn（可能被调多次）：items 只能 clone 不能消费
                        for item in menu_items.clone() {
                            let label = item.label.clone();
                            match item.action {
                                ContextAction::SortDown { column, descending } => {
                                    // 与右键「按此列排序」同口径：没接钩子就不摆
                                    if let Some(hook) = on_sort_down.clone() {
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| hook(&column, descending, app),
                                        ));
                                    }
                                }
                                ContextAction::CopyColumnName => {
                                    // 【DBeaver 对齐】列头「复制列名」：DBeaver 侧走
                                    // `getQuotedIdentifier`（按方言加引号，`ResultSetHandlerMain.java:367-371`），
                                    // 本仓网格不持有方言（结果可能来自联邦 / 计算列），复制**裸名**；
                                    // 需要引号时由用户自己加——宁可少猜，不给出一个错的引号。
                                    let text = column_name_for_menu.clone();
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, _window, app| {
                                            app.write_to_clipboard(ClipboardItem::new_string(
                                                text.clone(),
                                            ));
                                        },
                                    ));
                                }
                                ContextAction::CopyColumn(_) => {
                                    // 列头菜单只针对当前列：复制整列文本在闭包外已算好
                                    let text = copy_column_text.clone();
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, _window, app| {
                                            app.write_to_clipboard(ClipboardItem::new_string(
                                                text.clone(),
                                            ));
                                        },
                                    ));
                                }
                                ContextAction::ToggleFreeze(column) => {
                                    // 冻结是**表格自己的事**（改 `Column.fixed`）：点击在
                                    // 独立事件里，可安全更新它并重建列组
                                    // 【列布局落盘】改完交回「该存了」（宿主先取快照再存）
                                    let table = table.clone();
                                    let queue = layout_queue.clone();
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, _window, app| {
                                            table.update(app, |state, cx| {
                                                state.delegate_mut().toggle_freeze(column);
                                                state.refresh(cx);
                                            });
                                            if let Some(queue) = &queue {
                                                queue.request(
                                                    ResultRequest::ColumnLayoutChanged,
                                                    app,
                                                );
                                            }
                                        },
                                    ));
                                }
                                ContextAction::HideColumn(column) => {
                                    // 隐藏是**表格自己的事**（改视图列序），与冻结同一条路
                                    let table = table.clone();
                                    let queue = layout_queue.clone();
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, _window, app| {
                                            table.update(app, |state, cx| {
                                                state.delegate_mut().hide_column(column);
                                                state.refresh(cx);
                                            });
                                            if let Some(queue) = &queue {
                                                queue.request(
                                                    ResultRequest::ColumnLayoutChanged,
                                                    app,
                                                );
                                            }
                                        },
                                    ));
                                }
                                ContextAction::ShowAllColumns => {
                                    let table = table.clone();
                                    let queue = layout_queue.clone();
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, _window, app| {
                                            table.update(app, |state, cx| {
                                                state.delegate_mut().show_all_columns();
                                                state.refresh(cx);
                                            });
                                            if let Some(queue) = &queue {
                                                queue.request(
                                                    ResultRequest::ColumnLayoutChanged,
                                                    app,
                                                );
                                            }
                                        },
                                    ));
                                }
                                ContextAction::ColumnStats { column: _ } => {
                                    // 统计是**网格自己算**（纯遍历），弹层由宿主开
                                    if let Some(hook) = on_column_stats.clone() {
                                        let name = column_name_for_menu.clone();
                                        let text = stats_text.clone();
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| {
                                                hook(name.clone(), text.clone(), app)
                                            },
                                        ));
                                    }
                                }
                                ContextAction::InsightColumn { column } => {
                                    // 同「按值筛选」：没接钩子（宿主没接洞察端口）就不摆
                                    if let Some(hook) = on_insight_column.clone() {
                                        menu =
                                            menu.item(PopupMenuItem::new(label).on_click(
                                                move |_, _window, app| hook(&column, app),
                                            ));
                                    }
                                }
                                ContextAction::FilterColumnValue { column, value } => {
                                    // 列级筛选交回面板（统一 set + 刷新筛选统计 + notify）；
                                    // 没接钩子就不摆（面板构造时一定接，防御性分支）
                                    if let Some(hook) = on_column_filter.clone() {
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| hook(column, &value, app),
                                        ));
                                    }
                                }
                                ContextAction::ClearColumnFilter { column } => {
                                    // 清除走同一钩子：值传空串（面板端约定「空 = 清除」）
                                    if let Some(hook) = on_column_filter.clone() {
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| hook(column, "", app),
                                        ));
                                    }
                                }
                                ContextAction::ClearSort => {
                                    // 取消排序是本地动作（回原顺序，不下发源库重查）：
                                    // 与冻结 / 隐藏同一条独立事件路径，可安全更新 delegate 并刷新
                                    let table = table.clone();
                                    menu = menu.item(PopupMenuItem::new(label).on_click(
                                        move |_, _window, app| {
                                            table.update(app, |state, cx| {
                                                state.delegate_mut().clear_sort();
                                                state.refresh(cx);
                                            });
                                        },
                                    ));
                                }
                                // 【filters 段首】「在结果中查找…」：命令语义（只开不关），
                                // 与数据格右键同一组请求（`fillFiltersMenu:3869`）
                                ContextAction::OpenFind => {
                                    if let Some(queue) = filter_queue.clone() {
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| {
                                                queue.request(ResultRequest::OpenFind, app);
                                            },
                                        ));
                                    }
                                }
                                // 【DBeaver 对齐】`fillFiltersMenu` 尾部四项
                                // （`ResultSetViewer.java:3926-3932`）：保存 / 重置筛选设置、
                                // 删除所有过滤 / 排序、编辑筛选设置——全部转交面板执行
                                ContextAction::SaveFilterSetting => {
                                    if let Some(queue) = filter_queue.clone() {
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| {
                                                queue.request(ResultRequest::SaveFilterSetting, app);
                                            },
                                        ));
                                    }
                                }
                                ContextAction::ResetFilterSetting => {
                                    if let Some(queue) = filter_queue.clone() {
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| {
                                                queue.request(ResultRequest::ResetFilterSetting, app);
                                            },
                                        ));
                                    }
                                }
                                ContextAction::ClearAllFilters => {
                                    if let Some(queue) = filter_queue.clone() {
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| {
                                                queue.request(
                                                    ResultRequest::ClearFilterAndOrdering,
                                                    app,
                                                );
                                            },
                                        ));
                                    }
                                }
                                ContextAction::OpenFilterSettings => {
                                    if let Some(queue) = filter_queue.clone() {
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| {
                                                queue.request(ResultRequest::OpenFilterSettings, app);
                                            },
                                        ));
                                    }
                                }
                                _ => {
                                    // 值与行相关的动作（预览 / 按值筛选 / 复制值 / 复制行）
                                    // 只属于右键菜单；列头菜单不渲染它们
                                }
                            }
                        }
                        menu
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
    head.id(("editor-result-th", col_ix))
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
        .into_any_element()
}
