//! 菜单域（`grid/` 第五块）：右键 / 列头 / 列浏览器三组菜单项（纯函数）+ 列去重
//! 取值 + `TableDelegate::context_menu`（网格最近右键单元格 → 菜单动作回钩子）。

use gpui_kit::*;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::table::TableState;

use crate::rowset::RowRef;
use super::delegate::ResultGridDelegate;
use super::stats::{column_stats, stats_lines};
use super::text::preview_of;
/// 右键菜单的动作（界面按它决定点击后干什么）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextAction {
    /// 【遗留 #7】查看完整值（原值原样交给面板，弹层里展示）
    PreviewValue(String),
    /// 按值筛选（值原样交给面板）
    FilterByValue(String),
    /// 复制此值
    CopyValue(String),
    /// 【A9】复制此值为 JSON（`NULL` → `null`，其余 JSON 字符串，与导出同口径）
    CopyValueJson(String),
    /// 复制整列（**数据列**下标）：当前视图行序里这一列的全部取值，一行一个
    /// （与 mock 预览「复制此列」同一口径：只给已抓到的行，不重查源库）
    CopyColumn(usize),
    /// 复制整行（**视图行**下标）：整行拼成 TSV（列之间是制表符，粘进表格就能分列）
    CopyRow(usize),
    /// 【A9】复制整行为 JSON（单对象，与导出 [`encode_json`] 同口径，只是不包数组）
    CopyRowJson(usize),
    /// 【A9】复制整行为 INSERT（表名猜得到才摆这一项；值 / 标识符与导出档同口径）
    CopyRowInsert(usize),
    /// 冻结 / 取消冻结这一**数据列**
    ToggleFreeze(usize),
    /// 【P2】隐藏这一**数据列**（从视图列里去掉；数据不动）
    HideColumn(usize),
    /// 【P2】把隐藏的列全部放回来（显示全部列）
    ShowAllColumns,
    /// 【P2-3】这一**数据列**的统计（count / null / min / max / avg / sum）
    ColumnStats { column: usize },
    /// 按这一列（列名）排序并**下发重查**
    SortDown { column: String, descending: bool },
    /// 【M8】洞察这一列（列名；宿主据此取样 → 列画像）
    InsightColumn { column: String },
    /// 【筛选】此列按给定值过滤（**本地列级筛选**，不动源库——与「按值筛选」的
    /// 全局词不同：它限定在这一个数据列上）
    FilterColumnValue { column: usize, value: String },
    /// 【筛选】清除此列的列级筛选（数据恢复显示）
    ClearColumnFilter { column: usize },
    /// 【排序】取消本地排序（回原顺序；不下发源库重查，对齐 DBeaver 排序 `NONE`）
    ClearSort,
    /// 【#28-B3】在该行下方插入一条本地空行（视图行号；动作由宿主落到叠加层）
    InsertRowBelow(usize),
    /// 【#28-B3】复制该行（视图行号）为一条本地行快照
    DuplicateRow(usize),
    /// 【#28-B3】删除该行（视图行号；底层行打删除标记，本地行移出叠加层）
    DeleteRow(usize),
}
/// 右键菜单里的一项
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuItem {
    pub label: String,
    pub action: ContextAction,
    /// 这项前面要不要加一条分隔线
    pub separator_before: bool,
}
/// 右键落点：菜单项要回答的「这是哪一格、这一列的处境」（[`context_menu_items`] 的输入）
///
/// 打包成一个结构体而不是递一串位置参数：`column` / `row` / `rows` 挨在一起时
/// 很容易传反（三个都是数字），而传反了菜单看上去照样正常。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextTarget {
    /// 目标单元格的取值（原样交给「按值筛选」与「复制此值」，预览只管显示）
    pub value: String,
    /// 目标单元格所在数据列的列名
    pub column_name: String,
    /// 数据列下标（含行号槽的列下标已折算掉）
    pub column: usize,
    /// 目标单元格所在**视图行**（复制整行用它）
    pub row: usize,
    /// 这一列在视图里的取值个数（复制此列的文案要写明会拷走多少）
    pub rows: usize,
    /// 这一列冻结了吗（文案随它反向）
    pub frozen: bool,
    /// 【P2】当前隐藏了多少列（>0 时菜单摆「显示全部列」）
    pub hidden_columns: usize,
    /// 【M8】这份结果能不能洞察（宿主接了端口 + 这份结果可取样）
    pub insight: bool,
    /// 【A9】这份结果的来源表名（面板从结果 SQL 猜好传来；`None` = 猜不到
    /// →「复制整行为 INSERT」不摆——宁可没有也不给一条插错表的语句）
    pub table: Option<String>,
    /// 【筛选】这一列当前有没有列级筛选生效（`true` → 菜单摆「清除此列筛选」）
    pub column_filter_active: bool,
    /// 【排序】这份结果当前是否正按这一列排序（`true` → 菜单摆「取消排序」）
    pub sort_active: bool,
    /// 【筛选】这一列的去重取值（前 `COLUMN_FILTER_VALUE_LIMIT` 个；列头菜单
    /// 「按值筛选此列」的值列表——由网格从已抓行算好传入，纯函数不碰数据）
    pub column_values: Vec<String>,
    /// 【#28-B3】目标行身份（底层行 / 本地行；行级动作菜单按它区分可用性）
    pub row_ref: RowRef,
    /// 【#28-B3】这份结果能不能编辑（`false` 时右键不摆新增 / 复制 / 删除三项）
    pub editable: bool,
    /// 【#28-B3】目标底层行是否已被标记删除（被删行不摆复制 / 删除行级动作）
    pub is_deleted: bool,
}
/// 右键菜单的项（**纯函数**：有什么、叫什么都在这里定，界面只负责画）
///
/// 原型 §5.5 的右键入口：按值筛选（写进筛选框、默认本地）· 复制（此值 / 此列 / 整行）·
/// 冻结 / 取消冻结（原生 `Column.fixed`）·【M8】洞察此列。
///
/// `target.insight` = 【M8】这份结果能不能洞察（宿主接了端口 + 这份结果可取样，见
/// `ResultEntry::can_insight_column`）——**能力没有就不摆入口**。
///
/// 文案与 mock 预览表的右键菜单同一套：两张表是同一类东西，同一种动作不该两个叫法。
pub fn context_menu_items(target: &ContextTarget) -> Vec<ContextMenuItem> {
    let value = target.value.as_str();
    let column_name = target.column_name.as_str();
    let preview = preview_of(value);
    let mut items = vec![
        ContextMenuItem {
            // 【遗留 #7】查看完整值：右键菜单里的「按值筛选「预览」」只给 24 字符截断，
            // 长值（JSON / 文本）要从这里真正看全
            label: "查看完整值…".to_string(),
            action: ContextAction::PreviewValue(value.to_string()),
            separator_before: false,
        },
        ContextMenuItem {
            label: format!("按值筛选「{preview}」"),
            action: ContextAction::FilterByValue(value.to_string()),
            separator_before: false,
        },
        ContextMenuItem {
            label: "复制此值".to_string(),
            action: ContextAction::CopyValue(value.to_string()),
            separator_before: false,
        },
        ContextMenuItem {
            // 【A9】值 JSON：`NULL` → `null`，其余 JSON 字符串（与「复制此值」平行，
            // 口径在 export::value_to_json——同一个值两种格式不该两套转义）
            label: "复制此值为 JSON".to_string(),
            action: ContextAction::CopyValueJson(value.to_string()),
            separator_before: false,
        },
        ContextMenuItem {
            // 个数写明：这一列拷走多少得让用户心里有数（与 mock 预览同一句文案）
            label: format!("复制此列（{} 个取值）", target.rows),
            action: ContextAction::CopyColumn(target.column),
            separator_before: false,
        },
        ContextMenuItem {
            label: "复制整行（TSV）".to_string(),
            action: ContextAction::CopyRow(target.row),
            separator_before: false,
        },
        ContextMenuItem {
            // 【A9】行 JSON：单对象（列名 → 值，`NULL` → `null`），与导出 JSON 档同口径
            label: "复制整行为 JSON".to_string(),
            action: ContextAction::CopyRowJson(target.row),
            separator_before: false,
        },
    ];
    // 【A9】表名猜得到才摆 INSERT：猜不到宁可没有（照着一个不存在的表去插，
    // 比右键里少一项更糟——表名猜测见 export::table_from_sql）
    if let Some(table) = target.table.as_deref() {
        items.push(ContextMenuItem {
            label: format!("复制整行为 INSERT（{table}）"),
            action: ContextAction::CopyRowInsert(target.row),
            separator_before: false,
        });
    }
    // 【#28-B3】行级动作独立分组（对齐 DBeaver 行操作段）：新增 · 复制 · 删除一行。
    // 只读结果集（`!editable`）整组不摆；已标记删除的底层行不再摆复制 / 删除——
    // 新增空行也一并收起（被删行区段不再产生新的叠加动作）。动作只改本地叠加层，
    // 不下发 INSERT / DELETE。
    if target.editable && !target.is_deleted {
        items.push(ContextMenuItem {
            label: "在下方插入新行".to_string(),
            action: ContextAction::InsertRowBelow(target.row),
            separator_before: true,
        });
        items.push(ContextMenuItem {
            label: "复制此行".to_string(),
            action: ContextAction::DuplicateRow(target.row),
            separator_before: false,
        });
        items.push(ContextMenuItem {
            label: "删除此行".to_string(),
            action: ContextAction::DeleteRow(target.row),
            separator_before: false,
        });
    }
    items.push(ContextMenuItem {
        label: format!("按「{column_name}」升序（下发源库）"),
        action: ContextAction::SortDown {
            column: column_name.to_string(),
            descending: false,
        },
        separator_before: true,
    });
    items.push(ContextMenuItem {
        label: format!("按「{column_name}」降序（下发源库）"),
        action: ContextAction::SortDown {
            column: column_name.to_string(),
            descending: true,
        },
        separator_before: false,
    });
    // 【排序】当前正按此列排时补「取消排序」（DBeaver 排序子菜单的 NONE 项）
    if target.sort_active {
        items.push(ContextMenuItem {
            label: format!("取消排序「{column_name}」"),
            action: ContextAction::ClearSort,
            separator_before: true,
        });
    }
    items.push(ContextMenuItem {
        label: if target.frozen {
            "取消冻结此列".to_string()
        } else {
            format!("冻结「{column_name}」")
        },
        action: ContextAction::ToggleFreeze(target.column),
        separator_before: false,
    });
    items.push(ContextMenuItem {
        label: format!("隐藏「{column_name}」"),
        action: ContextAction::HideColumn(target.column),
        separator_before: false,
    });
    items.push(ContextMenuItem {
        label: format!("列统计「{column_name}」"),
        action: ContextAction::ColumnStats { column: target.column },
        separator_before: false,
    });
    if target.hidden_columns > 0 {
        items.push(ContextMenuItem {
            label: "显示全部列".to_string(),
            action: ContextAction::ShowAllColumns,
            separator_before: false,
        });
    }
    if target.insight {
        items.push(ContextMenuItem {
            label: format!("洞察「{column_name}」这一列"),
            action: ContextAction::InsightColumn {
                column: column_name.to_string(),
            },
            separator_before: true,
        });
    }
    items
}
/// 列头下拉菜单的项（**纯函数**；DBeaver 列头箭头菜单 = 右键菜单的列级子集）：
/// 排序升/降 · 复制此列 · 冻结/取消冻结 · 隐藏此列 · 显示全部列 · 列统计 · 洞察此列。
/// 值与行相关的动作（预览 / 按值筛选 / 复制值 / 复制整行）只属于右键——它们依赖
/// “当前格”，列头没有当前格。条件与右键同口径（hidden>0 才摆「显示全部列」、
/// insight 才摆「洞察此列」）。
pub fn column_menu_items(target: &ContextTarget) -> Vec<ContextMenuItem> {
    let column_name = target.column_name.as_str();
    let mut items = vec![
        ContextMenuItem {
            label: format!("按「{column_name}」升序（下发源库）"),
            action: ContextAction::SortDown {
                column: column_name.to_string(),
                descending: false,
            },
            separator_before: false,
        },
        ContextMenuItem {
            label: format!("按「{column_name}」降序（下发源库）"),
            action: ContextAction::SortDown {
                column: column_name.to_string(),
                descending: true,
            },
            separator_before: false,
        },
    ];
    // 【排序】当前正按此列排时补「取消排序」（本地回原顺序，不下发重查）
    if target.sort_active {
        items.push(ContextMenuItem {
            label: format!("取消排序「{column_name}」"),
            action: ContextAction::ClearSort,
            separator_before: true,
        });
    }
    // 【筛选】列级筛选段（DBeaver 列头筛选）：值列表 + 清除项。
    // 值列表来自已抓行的去重取值（`target.column_values`，网格算好传入）；
    // 有值才摆（没数据这一列自然没有可选值）。清除项只在筛选生效时摆——
    // 「没有筛」还摆「清除」是僵尸入口。
    let mut values = target
        .column_values
        .iter()
        .map(|value| ContextMenuItem {
            label: format!("按值筛选「{}」", preview_of(value)),
            action: ContextAction::FilterColumnValue {
                column: target.column,
                value: value.clone(),
            },
            separator_before: false,
        })
        .collect::<Vec<_>>();
    if !values.is_empty() {
        values[0].separator_before = true;
    }
    if target.column_filter_active {
        values.push(ContextMenuItem {
            label: "清除此列筛选".to_string(),
            action: ContextAction::ClearColumnFilter {
                column: target.column,
            },
            separator_before: true,
        });
    }
    items.extend(values);
    items.push(ContextMenuItem {
        label: format!("复制此列（{} 个取值）", target.rows),
        action: ContextAction::CopyColumn(target.column),
        separator_before: false,
    });
    items.push(ContextMenuItem {
        label: if target.frozen {
            "取消冻结此列".to_string()
        } else {
            format!("冻结「{column_name}」")
        },
        action: ContextAction::ToggleFreeze(target.column),
        separator_before: false,
    });
    items.push(ContextMenuItem {
        label: format!("隐藏「{column_name}」"),
        action: ContextAction::HideColumn(target.column),
        separator_before: false,
    });
    items.push(ContextMenuItem {
        label: format!("列统计「{column_name}」"),
        action: ContextAction::ColumnStats { column: target.column },
        separator_before: false,
    });
    if target.hidden_columns > 0 {
        items.push(ContextMenuItem {
            label: "显示全部列".to_string(),
            action: ContextAction::ShowAllColumns,
            separator_before: false,
        });
    }
    if target.insight {
        items.push(ContextMenuItem {
            label: format!("洞察「{column_name}」这一列"),
            action: ContextAction::InsightColumn {
                column: column_name.to_string(),
            },
            separator_before: true,
        });
    }
    items
}
/// 左栏「列浏览器」清单行右键菜单的项（**纯函数**；对齐 DBeaver 元数据面板清单右键口径：
/// 清单不承担排序下发 / 筛选 / 洞察——那些属于列头与单元格，清单只暴露列级本地动作）：
/// 复制此列 · 冻结 / 解冻 · 隐藏此列 · 列统计 · 显示全部列（有隐藏列才摆）· 取消排序（生效才摆）。
///
/// 与列头菜单共用 [`ContextTarget`] 同一份数据语义；排序 / 筛选 / 洞察 / 值与行相关动作
/// 在清单入口不生成（`target.insight` / `column_values` / `column_filter_active` 留空即可）。
pub fn column_browser_menu_items(target: &ContextTarget) -> Vec<ContextMenuItem> {
    let column_name = target.column_name.as_str();
    let mut items = vec![
        ContextMenuItem {
            label: format!("复制此列（{} 个取值）", target.rows),
            action: ContextAction::CopyColumn(target.column),
            separator_before: false,
        },
        ContextMenuItem {
            label: if target.frozen {
                "取消冻结此列".to_string()
            } else {
                format!("冻结「{column_name}」")
            },
            action: ContextAction::ToggleFreeze(target.column),
            separator_before: false,
        },
        ContextMenuItem {
            label: format!("隐藏「{column_name}」"),
            action: ContextAction::HideColumn(target.column),
            separator_before: false,
        },
        ContextMenuItem {
            label: format!("列统计「{column_name}」"),
            action: ContextAction::ColumnStats { column: target.column },
            separator_before: false,
        },
    ];
    if target.sort_active {
        items.push(ContextMenuItem {
            label: format!("取消排序「{column_name}」"),
            action: ContextAction::ClearSort,
            separator_before: true,
        });
    }
    if target.hidden_columns > 0 {
        items.push(ContextMenuItem {
            label: "显示全部列".to_string(),
            action: ContextAction::ShowAllColumns,
            separator_before: false,
        });
    }
    items
}
/// 【筛选】这一列的去重取值（纯函数）：按出现顺序去重，最多 `limit` 个；
/// 列越界 / 无行 = 空切片。`NULL` 是字面量，原样计入（值列表里能筛 NULL）。
pub fn column_distinct_values(
    rows: &[Vec<String>],
    column: usize,
    limit: usize,
) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for row in rows {
        if let Some(value) = row.get(column) {
            if !seen.iter().any(|v| v == value) {
                seen.push(value.clone());
                if seen.len() >= limit {
                    break;
                }
            }
        }
    }
    seen
}
/// 【B14】右键菜单：按值筛选 / 复制（此值 / 此列 / 整行）/ 冻结此列（原型 §5.5 的右键入口）
///
/// 目标单元格是**最近右键的那一个**（[`ResultGridDelegate::context_cell`]）：没有就什么都不摆
/// （不猜一个“大概是想筛这个”）。菜单项由 [`context_menu_items`] 给（纯函数，可断言）。
///
/// 2026-10-29 目录拆分：由 `TableDelegate::context_menu` 薄转发至此（Rust trait impl 不可分块）。
pub(crate) fn context_menu(
    delegate: &mut ResultGridDelegate,
    _row_ix: usize,
    menu: PopupMenu,
    _window: &mut Window,
    cx: &mut Context<TableState<ResultGridDelegate>>,
) -> PopupMenu {
        // 【P2-4】转置是查看视图：右键菜单（筛选 / 排序 / 冻结 / 编辑）对它是错的
        if delegate.transposed {
            return menu;
        }
        let Some((row, col)) = delegate.context_cell else {
            return menu;
        };
        if ResultGridDelegate::is_row_number(col) || row >= delegate.view_len() {
            return menu;
        }
        let column = delegate.data_column(col - 1);
        let column_name = delegate
            .columns
            .get(column)
            .cloned()
            .unwrap_or_else(|| format!("列 {}", column + 1));

        let mut menu = menu;
        let insight_available = delegate.on_insight_column.is_some() && delegate.insight_available;
        let target = ContextTarget {
            value: delegate.cell(row, col),
            // clone 进 target：外部的 column_name 留着给「查看完整值」分支用
            column_name: column_name.clone(),
            column,
            row,
            rows: delegate.view_len(),
            frozen: delegate.is_frozen(column),
            hidden_columns: delegate.hidden.len(),
            insight: insight_available,
            // 【A9】表名给菜单（猜不到 = None → 不摆 INSERT 项）
            table: delegate.result_table.clone(),
            // 右键菜单不摆列级筛选段（右键的「按值筛选」是全局词）：两字段留空
            column_filter_active: false,
            column_values: Vec::new(),
            sort_active: delegate
                .sort_state()
                .is_some_and(|(sort_col, _)| sort_col == column),
            // 【#28-B3】行身份 + 编辑闸口 + 删除标记：行级三项菜单按这三个字段判定
            row_ref: delegate.row_ref(row).unwrap_or(RowRef::default()),
            editable: delegate.editable,
            is_deleted: delegate.is_deleted_row(row),
        };
        for item in context_menu_items(&target) {
            if item.separator_before {
                menu = menu.separator();
            }
            match item.action {
                ContextAction::PreviewValue(value) => {
                    // 【遗留 #7】没接钩子就不摆这一项（能力没有就不给入口）
                    if let Some(hook) = delegate.on_preview_value.clone() {
                        // 分支内克隆副本给闭包（外部的 column_name 后续分支还要用）；
                        // 闭包是 Fn（可多次调用）：取值必须 clone，不能 move 捕获
                        let column_name = column_name.clone();
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| {
                                hook(value.clone(), column_name.clone(), column, row, app)
                            },
                        ));
                    }
                }
                ContextAction::FilterByValue(needle) => {
                    // 没接钩子就不摆这一项（能力没有就不给入口）
                    if let Some(hook) = delegate.on_filter_value.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| hook(&needle, app),
                        ));
                    }
                }
                ContextAction::SortDown { column, descending } => {
                    // 与「按值筛选」同口径：没接钩子就不摆这一项
                    if let Some(hook) = delegate.on_sort_down.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| hook(&column, descending, app),
                        ));
                    }
                }
                ContextAction::CopyValue(copied) => {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
                        },
                    ));
                }
                ContextAction::CopyValueJson(copied) => {
                    // 【A9】值 JSON 在菜单构造时拼好（`NULL` → null；与导出档同口径）
                    let text = crate::export::value_to_json(&copied);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    ));
                }
                ContextAction::CopyRowJson(row) => {
                    // 【A9】整行 JSON（视图行序，与「复制整行（TSV）」同口径）
                    let text = delegate.row_json_text(row);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    ));
                }
                ContextAction::CopyRowInsert(row) => {
                    // 【A9】整行 INSERT（表名已由面板同步；拼好的语句带上）
                    let text = delegate.row_insert_text(row);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    ));
                }
                ContextAction::CopyColumn(column) => {
                    // 文案先拼好再交给点击回调：菜单项活到的比这一次 `context_menu` 调用长，
                    // 而“当时那一列是什么”只有现在知道。取值走**视图行序**（看着的才是拷走的）
                    let text = delegate.column_text(column);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    ));
                }
                ContextAction::CopyRow(row) => {
                    // 同上：拼好的 TSV 带上（整行 = 各列制表符分隔，粘进表格直接分列）
                    let text = delegate.row_text(row);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    ));
                }
                ContextAction::ToggleFreeze(column) => {
                    // 冻结是**表格自己的事**（改的是 `Column.fixed`），不需要面板：
                    // 点击发生在独立事件里（不在表格的更新栈内），可以安全地更新它并重建列组
                    let table = cx.entity().clone();
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            table.update(app, |state, cx| {
                                state.delegate_mut().toggle_freeze(column);
                                state.refresh(cx);
                            });
                        },
                    ));
                }
                ContextAction::InsightColumn { column } => {
                    // 同「按值筛选」：没接钩子（宿主没接洞察端口）就不摆这一项
                    if let Some(hook) = delegate.on_insight_column.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| hook(&column, app),
                        ));
                    }
                }
                ContextAction::ColumnStats { column } => {
                    // 统计是**网格自己算**（纯遍历），弹层由宿主开（复用值预览的对话框通道）
                    if let Some(hook) = delegate.on_column_stats.clone() {
                        let name = delegate
                            .columns
                            .get(column)
                            .cloned()
                            .unwrap_or_default();
                        let text = stats_lines(&column_stats(&delegate.rows, column)).join("\n");
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| hook(name.clone(), text.clone(), app),
                        ));
                    }
                }
                ContextAction::HideColumn(column) => {
                    // 隐藏是**表格自己的事**（改视图列序），与冻结同一条路：点击在独立事件里，
                    // 可以安全地更新 delegate 并重建列组
                    let table = cx.entity().clone();
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            table.update(app, |state, cx| {
                                state.delegate_mut().hide_column(column);
                                state.refresh(cx);
                            });
                        },
                    ));
                }
                ContextAction::ShowAllColumns => {
                    let table = cx.entity().clone();
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            table.update(app, |state, cx| {
                                state.delegate_mut().show_all_columns();
                                state.refresh(cx);
                            });
                        },
                    ));
                }
                ContextAction::ClearSort => {
                    // 取消排序 = 本地回原顺序（与列头菜单同一条路，不下发源库重查）
                    let table = cx.entity().clone();
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            table.update(app, |state, cx| {
                                state.delegate_mut().clear_sort();
                                state.refresh(cx);
                            });
                        },
                    ));
                }
                // 【#28-B3】行级动作：网格不持本地行真值，打包成请求交回宿主分派；
                // 没接钩子（只读结果集）时菜单项已被 `context_menu_items` 收起，这里兜底忽略。
                ContextAction::InsertRowBelow(row) => {
                    if let Some(hook) = delegate.on_row_action.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| {
                                hook(crate::ports::ResultRequest::InsertRowBelow(Some(row)), app);
                            },
                        ));
                    }
                }
                ContextAction::DuplicateRow(row) => {
                    if let Some(hook) = delegate.on_row_action.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| {
                                hook(crate::ports::ResultRequest::DuplicateRow(row), app);
                            },
                        ));
                    }
                }
                ContextAction::DeleteRow(row) => {
                    if let Some(hook) = delegate.on_row_action.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| {
                                hook(crate::ports::ResultRequest::DeleteRow(row), app);
                            },
                        ));
                    }
                }
                // 列级筛选只属于列头菜单（右键的「按值筛选」是全局词）：右键收到即忽略
                ContextAction::FilterColumnValue { .. } | ContextAction::ClearColumnFilter { .. } => {}
            }
        }
        menu
    }
