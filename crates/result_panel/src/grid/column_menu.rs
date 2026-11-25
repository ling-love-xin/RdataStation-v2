//! 列头菜单（`grid/` 第十块，2026-11-26）：分组计划 + 动作接线 + 菜单构造
//!
//! 真值有**两条**列头菜单入口，形态不同：
//!
//! | 入口 | 真值 | 形态 |
//! | --- | --- | --- |
//! | 列头右端 `drop_down` 图标点击 | `SpreadsheetPresentation.handleColumnIconClick:1585-1591` → `ResultSetViewer.showColumnMenu:2988-2999` | **平铺**：排序三项 + `fillFiltersMenu` 整组 |
//! | 列头右键 | `SpreadsheetPresentation.fillContextMenu:1088-1110`（`COLUMN_HEADER`）→ `ResultSetViewer.fillContextMenu:3151-3300` | **分组**：`复制列名` 顶层 + `筛选▸` + `排序▸` + `导航▸` + `编辑▸` + `面板▸` + `呈现格式▸` + `布局▸` … |
//!
//! 本仓：
//! - 右键用 [`column_menu_plan`] 的**分组**形态（本仓没有导航 / 编辑 / 导出的列级能力，
//!   面板子菜单由 `menu::append_panels_submenu` 仍在数据格右键上挂；登记见文档 §9.72）；
//! - 下拉三角仍是**平铺**（`menu::column_menu_items`），与真值 `showColumnMenu` 同构。
//!
//! 两条入口共用 [`ColumnMenuHooks`] 与 [`apply_item`]：动作分派只有一份，形状差异只在
//! 分组与否。

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::table::TableState;
use gpui_kit::{ClipboardItem, Context, Entity, Window};

use super::delegate::{
    ColumnFilterHook, ColumnStatsHook, InsightColumnHook, ResultGridDelegate, SortDownHook,
};
use super::menu::{
    ContextAction, ContextMenuItem, ContextTarget, column_copy_items, column_filter_value_items,
    column_layout_items, column_sort_items, filters_menu_items,
};
use crate::ports::{ResultActionQueue, ResultRequest};

/// 列头右键菜单的一组（组名 + 子菜单项）
#[derive(Clone)]
pub(crate) struct ColumnMenuGroup {
    /// 子菜单标题（真值里是 `筛选` / `排序` / `面板` / `呈现格式` …，本仓只有前三档白话名）
    pub label: &'static str,
    /// 子菜单图标（真值每个子菜单都带：`UIIcon.FILTER` / `UIIcon.SORT`；本仓的「列操作」是
    /// 增强项、真值无对应图标，给 `None`）
    pub icon: Option<IconName>,
    pub items: Vec<ContextMenuItem>,
}

/// 列头右键的完整计划：顶层项 + 分组子菜单
#[derive(Clone)]
pub(crate) struct ColumnMenuPlan {
    /// 顶层项（真值 `COLUMN_HEADER` 落点唯一多出来的一项 = `复制列名`）
    pub leading: Vec<ContextMenuItem>,
    /// 分组子菜单（按显示顺序）
    pub groups: Vec<ColumnMenuGroup>,
}

/// 列头右键的菜单计划（纯函数，可单测）
///
/// 组序对齐真值 `fillContextMenu:3166-3271`：`筛选` → `排序` →（本仓增强的 `列操作`）。
/// 真值的 `导航` / `编辑` / `面板` / `呈现格式` / `布局` / `导出 / 打开方式` 本仓没有对应
/// 能力，不摆空壳子菜单（摆了就是假控件）。
pub(crate) fn column_menu_plan(target: &ContextTarget) -> ColumnMenuPlan {
    // 「筛选」组：列级值筛选（本仓）在前，真值 `fillFiltersMenu` 整组在后——
    // 值列表是「按不同值筛选」的直接入口，比「保存 / 重置筛选设置」更常用。
    let mut filters = column_filter_value_items(target);
    filters.extend(filters_menu_items());
    // 「列操作」组：复制此列 + 冻结 / 隐藏 / 统计 / 显示全部 / 洞察（本仓增强）
    let mut layout: Vec<ContextMenuItem> = column_copy_items(target)
        .into_iter()
        .filter(|item| !matches!(item.action, ContextAction::CopyColumnName))
        .collect();
    layout.extend(column_layout_items(target));
    ColumnMenuPlan {
        leading: column_copy_items(target)
            .into_iter()
            .filter(|item| matches!(item.action, ContextAction::CopyColumnName))
            .collect(),
        groups: vec![
            ColumnMenuGroup {
                label: "筛选",
                icon: Some(IconName::Funnel),
                items: filters,
            },
            ColumnMenuGroup {
                label: "排序",
                icon: Some(IconName::SortAscending),
                items: column_sort_items(target),
            },
            ColumnMenuGroup {
                label: "列操作",
                icon: None,
                items: layout,
            },
        ],
    }
}

/// 列头菜单动作需要的全部句柄与预计算文本（`Clone`：菜单构造闭包是 `Fn`，可能被调多次）
#[derive(Clone)]
pub(crate) struct ColumnMenuHooks {
    pub table: Entity<TableState<ResultGridDelegate>>,
    /// 列布局落盘（冻结 / 隐藏改完交回宿主存会话）
    pub layout_queue: Option<ResultActionQueue>,
    /// 筛选设置段（保存 / 重置 / 全清 / 编辑设置）
    pub filter_queue: Option<ResultActionQueue>,
    pub sort_down: Option<SortDownHook>,
    pub column_stats: Option<ColumnStatsHook>,
    pub insight_column: Option<InsightColumnHook>,
    pub column_filter: Option<ColumnFilterHook>,
    /// 列名（复制列名 / 统计标题 / 无障碍标签用）
    pub column_name: String,
    /// 「复制此列」的整列文本（闭包外算好，菜单构造时不再遍历整列）
    pub copy_column_text: String,
    /// 「列统计」的弹层正文（同上）
    pub stats_text: String,
}

/// 把一个菜单项挂到 `menu` 上（唯一的动作分派处；下拉三角与右键子菜单共用）
///
/// 没接钩子的动作**不摆**（真值里对应命令在宿主没注册时也不会出现），
/// 值 / 行相关的动作（预览 / 按值筛选 / 复制值 / 复制整行）只属于数据格右键——
/// 列头落点落进 `_` 分支，不渲染。
pub(crate) fn apply_item(
    mut menu: PopupMenu,
    item: ContextMenuItem,
    hooks: &ColumnMenuHooks,
) -> PopupMenu {
    let label = item.label;
    match item.action {
        ContextAction::SortDown { column, descending } => {
            if let Some(hook) = hooks.sort_down.clone() {
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _window, app| {
                    hook(&column, descending, app)
                }));
            }
        }
        ContextAction::CopyColumnName => {
            // 【DBeaver 对齐】列头「复制列名」：DBeaver 侧走 `getQuotedIdentifier`（按方言加引号，
            // `ResultSetHandlerMain.java:367-371`），本仓网格不持有方言（结果可能来自联邦 / 计算列），
            // 复制**裸名**；需要引号时由用户自己加。
            let text = hooks.column_name.clone();
            menu = menu.item(PopupMenuItem::new(label).on_click(
                move |_, _window, app| {
                    app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                },
            ));
        }
        ContextAction::CopyColumn(_) => {
            let text = hooks.copy_column_text.clone();
            menu = menu.item(PopupMenuItem::new(label).on_click(
                move |_, _window, app| {
                    app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                },
            ));
        }
        ContextAction::ToggleFreeze(column) => {
            // 冻结是**表格自己的事**（改 `Column.fixed`）：点击在独立事件里，可安全更新它
            let table = hooks.table.clone();
            let queue = hooks.layout_queue.clone();
            menu = menu.item(PopupMenuItem::new(label).on_click(
                move |_, _window, app| {
                    table.update(app, |state, cx| {
                        state.delegate_mut().toggle_freeze(column);
                        state.refresh(cx);
                    });
                    if let Some(queue) = &queue {
                        queue.request(ResultRequest::ColumnLayoutChanged, app);
                    }
                },
            ));
        }
        ContextAction::HideColumn(column) => {
            let table = hooks.table.clone();
            let queue = hooks.layout_queue.clone();
            menu = menu.item(PopupMenuItem::new(label).on_click(
                move |_, _window, app| {
                    table.update(app, |state, cx| {
                        state.delegate_mut().hide_column(column);
                        state.refresh(cx);
                    });
                    if let Some(queue) = &queue {
                        queue.request(ResultRequest::ColumnLayoutChanged, app);
                    }
                },
            ));
        }
        ContextAction::ShowAllColumns => {
            let table = hooks.table.clone();
            let queue = hooks.layout_queue.clone();
            menu = menu.item(PopupMenuItem::new(label).on_click(
                move |_, _window, app| {
                    table.update(app, |state, cx| {
                        state.delegate_mut().show_all_columns();
                        state.refresh(cx);
                    });
                    if let Some(queue) = &queue {
                        queue.request(ResultRequest::ColumnLayoutChanged, app);
                    }
                },
            ));
        }
        ContextAction::ColumnStats { column: _ } => {
            // 统计是**网格自己算**（纯遍历），弹层由宿主开
            if let Some(hook) = hooks.column_stats.clone() {
                let name = hooks.column_name.clone();
                let text = hooks.stats_text.clone();
                menu = menu.item(PopupMenuItem::new(label).on_click(
                    move |_, _window, app| hook(name.clone(), text.clone(), app),
                ));
            }
        }
        ContextAction::InsightColumn { column } => {
            if let Some(hook) = hooks.insight_column.clone() {
                menu = menu.item(PopupMenuItem::new(label).on_click(
                    move |_, _window, app| hook(&column, app),
                ));
            }
        }
        ContextAction::FilterColumnValue { column, value } => {
            if let Some(hook) = hooks.column_filter.clone() {
                menu = menu.item(PopupMenuItem::new(label).on_click(
                    move |_, _window, app| hook(column, &value, app),
                ));
            }
        }
        ContextAction::ClearColumnFilter { column } => {
            // 清除走同一钩子：值传空串（面板端约定「空 = 清除」）
            if let Some(hook) = hooks.column_filter.clone() {
                menu = menu.item(PopupMenuItem::new(label).on_click(
                    move |_, _window, app| hook(column, "", app),
                ));
            }
        }
        ContextAction::ClearSort => {
            let table = hooks.table.clone();
            menu = menu.item(PopupMenuItem::new(label).on_click(
                move |_, _window, app| {
                    table.update(app, |state, cx| {
                        state.delegate_mut().clear_sort();
                        state.refresh(cx);
                    });
                },
            ));
        }
        ContextAction::OpenFind => {
            if let Some(queue) = hooks.filter_queue.clone() {
                menu = menu.item(PopupMenuItem::new(label).on_click(
                    move |_, _window, app| {
                        queue.request(ResultRequest::OpenFind, app);
                    },
                ));
            }
        }
        // 【DBeaver 对齐】`fillFiltersMenu` 尾部四项（`ResultSetViewer.java:3926-3932`）
        ContextAction::SaveFilterSetting => {
            if let Some(queue) = hooks.filter_queue.clone() {
                menu = menu.item(PopupMenuItem::new(label).on_click(
                    move |_, _window, app| {
                        queue.request(ResultRequest::SaveFilterSetting, app);
                    },
                ));
            }
        }
        ContextAction::ResetFilterSetting => {
            if let Some(queue) = hooks.filter_queue.clone() {
                menu = menu.item(PopupMenuItem::new(label).on_click(
                    move |_, _window, app| {
                        queue.request(ResultRequest::ResetFilterSetting, app);
                    },
                ));
            }
        }
        ContextAction::ClearAllFilters => {
            if let Some(queue) = hooks.filter_queue.clone() {
                menu = menu.item(PopupMenuItem::new(label).on_click(
                    move |_, _window, app| {
                        queue.request(ResultRequest::ClearFilterAndOrdering, app);
                    },
                ));
            }
        }
        ContextAction::OpenFilterSettings => {
            if let Some(queue) = hooks.filter_queue.clone() {
                menu = menu.item(PopupMenuItem::new(label).on_click(
                    move |_, _window, app| {
                        queue.request(ResultRequest::OpenFilterSettings, app);
                    },
                ));
            }
        }
        _ => {
            // 值与行相关的动作只属于数据格右键；列头落点不渲染它们
        }
    }
    menu
}

/// 按计划铺一份**分组**菜单（列头右键用）
///
/// 子菜单用 `PopupMenu::submenu_with_icon`（vendor `popup_menu.rs:725-745`，
/// 内部 `PopupMenu::build` + 父子层级绑定）。空组直接跳过——摆一个空子菜单等于假控件。
pub(crate) fn build_plan_menu(
    mut menu: PopupMenu,
    plan: ColumnMenuPlan,
    hooks: &ColumnMenuHooks,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    for item in plan.leading {
        menu = apply_item(menu, item, hooks);
    }
    for group in plan.groups {
        if group.items.is_empty() {
            continue;
        }
        let hooks = hooks.clone();
        let items = group.items;
        let icon = group.icon.map(Icon::new);
        menu = menu.submenu_with_icon(icon, group.label, window, cx, move |mut sub, _window, _cx| {
            for item in items.clone() {
                sub = apply_item(sub, item, &hooks);
            }
            sub
        });
    }
    menu
}

/// 按平铺列表铺一份菜单（列头下拉三角用；真值 `showColumnMenu` 就是平铺）
pub(crate) fn build_flat_menu(
    mut menu: PopupMenu,
    items: &[ContextMenuItem],
    hooks: &ColumnMenuHooks,
) -> PopupMenu {
    for item in items {
        if item.separator_before {
            menu = menu.separator();
        }
        menu = apply_item(menu, item.clone(), hooks);
    }
    menu
}
