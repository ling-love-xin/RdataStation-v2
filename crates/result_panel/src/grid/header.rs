//! 表头渲染域（`grid/` 第八块）：`TableDelegate::render_th`（截断 + 悬停全文 +
//! 类型小标签 + 排序箭头 / 列头菜单）+ 表头 tooltip 分块文案。

use gpui_kit::*;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{ButtonVariants as _, DropdownButton};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::table::TableState;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;

use crate::rowset::RowRef;
use crate::store::ColumnKind;
use crate::ui;
use workbench_shell::model::ColumnAnnotation;
use super::delegate::ResultGridDelegate;
use super::menu::{ContextAction, ContextTarget, column_menu_items};
use super::stats::{column_stats, stats_lines};
/// 【P1】表头悬停的分块内容：第一行「列名 · 类型」，后续行字段元数据（来源 / 注释 / 可空 / 默认）
///
/// 返回 `(标题行, 元数据行列表)`。注释缺失 = 空列表（与今天的单行 tooltip 等效）。
/// 多行不拼 `\n` 字符串：tooltip 容器是 `whitespace_normal`，换行会被折叠成空格，
/// 由调用方逐行 `child` 渲染成块。
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
            delegate.columns
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
                .text_xs()
                .rounded(gpui_kit::px(3.))
                .flex_none()
                .child(SharedString::from(sym))
        });

        // 【B1】列名主行：badge + 列名(flex_1) + 筛选▽ + 排序↑/↓
        let sort_indicator = delegate.sort_state().and_then(|(sort_col, descending)| {
            if sort_col == column {
                Some(if descending { "↓" } else { "↑" })
            } else {
                None
            }
        });
        let main_row = div()
            .h_flex()
            .items_center()
            .gap_1()
            .size_full()
            .min_w_0()
            .truncate()
            .when_some(type_badge, |row, badge| row.child(badge))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(name.clone())),
            )
            .when_some(sort_indicator, |row, arrow| {
                row.child(
                    div().text_xs().text_color(muted).child(SharedString::from(arrow)),
                )
            })
            .when(delegate.has_column_filter(column), |row| {
                row.child(
                    div().text_xs().text_color(muted).child(SharedString::from("▽")),
                )
            });

        // 【B1】注释第二行：开关开启 + 列有注释时画（muted 小字，截短；hover tooltip 看全）
        let comment_line = if delegate.show_column_comments && !ResultGridDelegate::is_row_number(col_ix) && !delegate.transposed {
            delegate.column_annotations
                .get(col_ix - 1)
                .cloned()
                .flatten()
                .and_then(|a| a.comment)
                .filter(|c| !c.trim().is_empty())
                .map(|comment| {
                    div()
                        .w_full()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .child(SharedString::from(comment))
                })
        } else {
            None
        };

        let head = div()
            .v_flex()
            .size_full()
            .min_w_0()
            .debug_selector(move || format!("editor-result-th-{col_ix}"))
            .child(main_row)
            .when_some(comment_line, |head, line| head.child(line));
        // 转置视图只画列名（类型是混合的，注释表头没有意义）；行号槽 / 空列名同理
        if delegate.transposed || ResultGridDelegate::is_row_number(col_ix) || name.is_empty() {
            return head.into_any_element();
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
        };
        let menu_items = column_menu_items(&column_menu_target);
        let copy_column_text = delegate.column_text(column);
        let stats_text = stats_lines(&column_stats(&delegate.rows, column)).join("\n");
        let column_name_for_menu = name.clone();
        let on_sort_down = delegate.on_sort_down.clone();
        let on_column_stats = delegate.on_column_stats.clone();
        let on_insight_column = delegate.on_insight_column.clone();
        let on_column_filter = delegate.on_column_filter.clone();
        let head = head.child(
            // guard 层带 id：`on_click` 是 StatefulInteractiveElement 方法（裸 div 只有
            // `on_mouse_down`）；点击落在 guard 上时截住冒泡，列头排序收不到
            div()
                .id(("editor-result-col-menu-guard", col_ix))
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
                                                move |_, _window, app| {
                                                    hook(&column, descending, app)
                                                },
                                            ));
                                        }
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
                                        let table = table.clone();
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| {
                                                table.update(app, |state, cx| {
                                                    state.delegate_mut().toggle_freeze(column);
                                                    state.refresh(cx);
                                                });
                                            },
                                        ));
                                    }
                                    ContextAction::HideColumn(column) => {
                                        // 隐藏是**表格自己的事**（改视图列序），与冻结同一条路
                                        let table = table.clone();
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| {
                                                table.update(app, |state, cx| {
                                                    state.delegate_mut().hide_column(column);
                                                    state.refresh(cx);
                                                });
                                            },
                                        ));
                                    }
                                    ContextAction::ShowAllColumns => {
                                        let table = table.clone();
                                        menu = menu.item(PopupMenuItem::new(label).on_click(
                                            move |_, _window, app| {
                                                table.update(app, |state, cx| {
                                                    state.delegate_mut().show_all_columns();
                                                    state.refresh(cx);
                                                });
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
                                            menu = menu.item(PopupMenuItem::new(label).on_click(
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
        // 【P1】悬停给全名 + 类型 + 字段元数据（来源 / 注释 / 可空 / 默认）：
        // 有注释才追加分块行（没有 = 与今天的单行一致）；注释块里的类型是 NavCache 原始名，
        // 与表头小标签（渲染层语义标签）并存不冲突。
        let annotation = delegate.column_annotations.get(col_ix - 1).cloned().flatten();
        let tip_name = name.clone();
        let tip_label = label.clone();
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
