//! 结果区（原型 §2.4 的 ⑤⑥⑦ + 网格）
//!
//! 自上而下：**结果集标签条**（由宿主给）→ **工具栏**（[`ResultToolbar`]：`行数 N` │
//! `耗时 1.2s` │ `连接名` + 动作）→ **网格 或 错误卡片** → **结果状态行**（[`ResultStatus`]：
//! `共 N 行` │ `已选第 M 行` + 截断提示）。
//!
//! 网格本身用组件库的 `DataTable` + `TableState`（虚拟滚动、列宽拖拽、行选择都是现成的，
//! 不手搓）。首列是固定的 `#` 行号槽（原型 §2.4），`NULL` 用 `muted_foreground` 斜体呈现
//! ——它是 SQL 里的一个真实值，与字符串 `"NULL"` 区分开。
//!
//! 单元格与表头都**截断**（结果里的长文本 / 长 JSON / 长列名比列宽宽是常态），截掉的部分
//! 靠**悬停全文**看（自己画 `render_th` / `render_td` 就是为了挂它）。右键菜单除了按值筛选
//! 与冻结，还给三种复制：此值 · 此列 · 整行（TSV，走 [`::shared::string::tsv_cell`] 同一份转义）。
//!
//! 原型 §2.4 的工具栏还包含**筛选 · 下发开关 · 分析 · 导出**——它们各自属 B15 / B14 / B7，
//! **没实现就不摆按钮**；分页/取下一段属 B5b，位置留在 ⑦ 那一行。
//!
//! 数据源是 [`crate::store::ResultStore`]（结果唯一权威）。网格**不持有真值**——
//! delegate 里的行是从权威那里拷来的投影，`set_data` 是唯一的写入点。
//!
//! ## 类型感知渲染（本切片）
//!
//! 列类型（[`crate::store::ColumnKind`]）只影响**怎么画**：数字右对齐、布尔 / 时间族用等宽字体
//! （主题的 `mono_font_family`，不引新依赖）、表头在列名右侧挂一个类型小标签（次级色）。
//! **类型缺失时一个像素都不变**：对齐、字体、表头都回到今天的样子——驱动填上列类型之前，
//! 所有真实结果走的都是这一档。
//!
//! ## 按需取值（本切片）
//!
//! 网格不预计算任何展示文本：组件的虚拟滚动只问可见窗口，delegate 就只回答那一格的取值。
//! 于是三条本来会按**整表**算的事都推到了真要的时候：
//!
//! - 视图行序：没有筛选也没有排序时**不物化**那份映射（`view_rows = None` 即恒等）；
//! - 筛选匹配：两边都是 ASCII 时零分配（回落 `to_lowercase` 只是为了保住口径）；
//! - 类型解析：入库时**每列一次**，不是每格一次。
//!
//! 需要**整表真值**的出口仍然是整表，但它们都是用户动作触发的（复制整列 / 整行 · 导出 ·
//! 筛选 · 排序），不是入库时预支的。

use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::table::{Column, ColumnSort, DataTable, TableDelegate, TableState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

// TSV 转义（剪贴板 / 导出的唯一实现，与 mock 预览表共用）：本 crate 根部有同名模块
// `crate::shared`，所以 `shared` 得从 crate 外路径（`::`）进来
use ::shared::string::{tsv_cell, tsv_row};

use crate::store::{ColumnKind, ColumnType};
use crate::status::elapsed_text;
use crate::ui;
use workbench_shell::model::ColumnAnnotation;

/// 结果网格的 key context（`crates/app` 把 `Ctrl+C` 绑在它上面，两边成对）
///
/// 与编辑器面板根的 `editor` 刻意分开：这个上下文只挂在结果网格那层元素上，所以
/// **编辑区里 `Ctrl+C` 仍是复制 SQL 文本、网格里才是复制选中的一格 / 一整行**。
/// 组件库的表格自己用的是 `DataTable`（方向键 / 翻页 / `Esc` 清选）——两个上下文
/// 会同时出现在焦点路径上，各管各的键。
///
/// 2026-10-23 下移：key context 字符串跨 crate 匹配（editor 的 `commands.rs` 从这里重导出）。
pub const RESULT_GRID_CONTEXT: &str = "editor-result-grid";

/// 【筛选】列头菜单「按值筛选此列」值列表的上限（DBeaver 值列表筛选取前若干去重值，
/// 不把整列几百个值全塞进菜单——要全量用全局筛选框）
pub const COLUMN_FILTER_VALUE_LIMIT: usize = 8;

/// 单元格**展示**文本的字符上限（对齐 dbdelve `result_grid.rs` `CELL_DISPLAY_LIMIT=300`）。
///
/// 只裁「画出来的这一格」：复制 / 导出 / 编辑初值 / 悬停全文 / 按值筛选 / 统计仍走完整值
/// （[`crate::ResultGridDelegate::cell`] 与 `row_cell_text` 均不经过这个裁剪）。超长
/// JSONB / PostGIS 几何 / 大文本因此不参与 GPUI 文本测量，可见格每帧布局成本有硬上界。
pub const CELL_DISPLAY_LIMIT: usize = 300;

/// 滚动到底要更多数据时的回调（面板注入：它才知道当前选中哪份结果、忙不忙）
///
/// 组件库在**可见范围接近末尾**时调 delegate 的 [`TableDelegate::load_more`]，
/// 而 delegate 在 `TableState` 里拿不到面板实体，所以走这个钩子（与执行通道同一个口径：
/// 视图不自己发执行，只把意图交回给知道全局的那一层）。
pub type LoadMoreHook = Rc<dyn Fn(&mut App)>;

/// 【T2a-4】单元格编辑**提交**的回调（面板注入：把（数据行序、数据列序、原值、新值）
/// 交给面板落变更集；面板自己在变更集里决定 upsert 还是移除）
pub type CommitEditHook = Rc<dyn Fn(usize, usize, String, String, &mut App)>;

/// 【T2a-4】正在编辑的单元格（编辑态由网格持有；提交 / 取消走钩子交回面板）
///
/// - 行号存两序：**视图行序**管渲染匹配（组件只懂视图行），**数据行序**管变更集与写回
///   （筛选 / 排序之后视图行与数据行可以不同，而变更集的“行”必须是数据行）。
/// - 列下标是**数据列序**（不含 `#` 行号槽；进入编辑时从组件列序折算掉）。
/// - `settled`：编辑框的初值同步与聚焦是幂等一次性动作（在 render_td 里做），
///   做过了就不再碰——免得每次重渲染都重设值 / 抢焦点。
pub struct CellEdit {
    /// 视图行序（渲染匹配用）
    view_row: usize,
    /// 数据行序（变更集与写回用）
    data_row: usize,
    /// 数据列序
    column: usize,
    /// 进入编辑时的原值（提交时若没变，面板据此不落变更）
    original: String,
    /// 当前输入值（`InputEvent::Change` 时从编辑框同步；提交时读它）
    pub value: String,
    /// 编辑框是否已完成首次聚焦与初值同步
    settled: bool,
    /// 【类型感知】提交校验失败时的错误文案（`Some` = 上次提交被拒，编辑框红框提示；
    /// 输入一变化就清除）。只影响“这次提交能不能发出去”，不落变更集。
    pub error: Option<String>,
}

/// 【B14】「按值筛选」的钩子（面板注入：把值写进筛选框并立刻生效）
pub type FilterValueHook = Rc<dyn Fn(&str, &mut App)>;

/// 【筛选】列级筛选钩子（列头菜单「按值筛选此列 / 清除」交回面板；参数 = 数据列、值）
pub type ColumnFilterHook = Rc<dyn Fn(usize, &str, &mut App)>;

/// 【B14】「排序下发」的钩子（面板注入：按列名重查源库；入参 = （列名，是否降序））
pub type SortDownHook = Rc<dyn Fn(&str, bool, &mut App)>;

/// 【M8】「洞察此列」的钩子（面板注入：把列名交给面板，面板再连同 SQL / 连接交给宿主）
///
/// 网格不知道结果集的 SQL 与连接（那是面板/存储的事），所以这里只传列名——
/// 与「按值筛选」同一种分工：**网格只负责说“用户点了谁”**。
pub type InsightColumnHook = Rc<dyn Fn(&str, &mut App)>;

/// 【遗留 #7】「查看完整值」的钩子（面板注入）：值 / 列名 / 数据列下标 / 视图行。
/// 弹层是宿主的事（对话框生命周期归宿主），网格只负责发意图。
pub type PreviewValueHook = Rc<dyn Fn(String, String, usize, usize, &mut App)>;

/// 【P2-3】列统计弹层的钩子（宿主开弹层；参数 = 列名、统计文本）
pub type ColumnStatsHook = Rc<dyn Fn(String, String, &mut App)>;

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

/// 网格数据（表头 + 行）
#[derive(Default)]
pub struct ResultGridDelegate {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    /// **每列的类型**（与 `columns` 同序；空 = 这份结果没带类型）
    ///
    /// 只影响怎么画（右对齐 / 等宽 / 表头小标签），不影响任何取值；没带类型时与今天完全一致。
    /// 由面板在 `set_data` **之后**同一次同步里给（见 [`ResultGridDelegate::set_column_types`]）。
    column_types: Vec<Option<ColumnType>>,
    /// 【P1】**每列的元数据注释**（与 `columns` 同序；空 = 这份结果没报注释）
    ///
    /// 只影响表头 tooltip（来源表 / 来源列 / 注释 / 类型 / 可空 / 默认值）；没报时与今天完全一致。
    /// 由面板在 `set_data` **之后**同一次同步里给（见 [`ResultGridDelegate::set_column_annotations`]）。
    column_annotations: Vec<Option<ColumnAnnotation>>,
    /// 空态文案（没有结果 / 执行失败都在这里说清楚）
    empty_text: String,
    /// 这份结果**还能不能取下一段**（面板从权威结果同步过来；`load_more` 据此决定要不要真的取）
    has_more: bool,
    /// 正在取下一段（防止“滚动到底”在回填之前反复触发）
    loading_more: bool,
    /// 取下一段的回调（面板构造时注入）
    on_load_more: Option<LoadMoreHook>,
    /// 【B15】本地筛选词（视图层：只影响看见与导出的行，**不重查数据库**）
    filter: String,
    /// 【筛选】列级筛选（`(数据列, 值)`；本地包含匹配，与全局 `filter` 叠加取交集）。
    /// 独立于全局筛选词：列头菜单的「按值筛选此列」走它，清空也不动筛选框。
    column_filter: Option<(usize, String)>,
    /// 【B15】本地排序（列下标按**数据列**算，`0` = 第一数据列；`None` = 原顺序）
    sort: Option<(usize, bool /* 降序 */)>,
    /// 【B14】最近右键的单元格（视图行号，列下标含行号槽）——组件库没给访问器，
    /// 所以在 `render_td` 上自己记（事件从最内层开始派发，不会丢）
    context_cell: Option<(usize, usize)>,
    /// 【B15】冻结的数据列（原生 `Column.fixed`：横向滚动时钉在左侧）
    frozen: Vec<usize>,
    /// 【P2】隐藏的数据列（**数据列序**；显示时跳过它们。只影响视图列序，数据一行不动）
    hidden: Vec<usize>,
    /// 【P1-5】会话内列宽记忆：按**列名**记（隐藏列 / 列序变化后回填仍对得上），
    /// `None` 不算——空表 = 用户没拖过，走默认档。切结果集由 `set_data` 清空。
    /// 行号槽不进表（宽度钉死）；本机制不做落盘持久化（见 backlog）。
    column_widths: HashMap<String, Pixels>,
    /// 【P2-4】单行转置：把选中一行竖排成「# / 字段 / 值」三列（DBeaver 式；只读视图）
    transposed: bool,
    /// 【P2-4】转置当前展示的数据行（越界自动夹取）
    transpose_row: usize,
    /// 【B14】按值筛选的钩子（面板注入）
    on_filter_value: Option<FilterValueHook>,
    /// 【筛选】列级筛选的钩子（面板注入；列头菜单 FilterColumnValue / ClearColumnFilter）
    on_column_filter: Option<ColumnFilterHook>,
    /// 【B14】排序下发的钩子（面板注入）
    on_sort_down: Option<SortDownHook>,
    /// 【M8】「洞察此列」的钩子（宿主接端口时面板才注入）
    on_insight_column: Option<InsightColumnHook>,
    /// 【遗留 #7】「查看完整值」的钩子（面板注入）
    /// 【遗留 #7】「查看完整值」的钩子（面板注入）：值 / 列名 / 数据列下标 / 视图行
    on_preview_value: Option<PreviewValueHook>,
    /// 【P2-3】列统计弹层的钩子（宿主开弹层；参数 = 列名、统计文本）
    on_column_stats: Option<ColumnStatsHook>,
    /// 【A9】这份结果的来源表名（面板从结果 SQL 猜好传来；`None` = 猜不到
    /// → 右键不摆「复制整行为 INSERT」）
    result_table: Option<String>,
    /// 【M8】这份结果能不能洞察（面板同步数据时算：见 `ResultEntry::can_insight_column`）
    insight_available: bool,
    /// 【B15】视图行序：当前看着的这一串行，元素是 `rows` 里的下标
    ///
    /// `None` = **与数据行序相同**（既没筛选也没排序）：这时不物化那份映射——
    /// 10 万行的结果切一次结果集就为它分配一个 10 万元素的表，而绝大多数时候视图
    /// 与数据是同一个顺序，那份表一次也用不上（入库 / 切换结果集时不该做这件事）。
    ///
    /// 筛选与排序都只改这个映射，**数据行一行不动**（所以清除筛选能原样恢复，
    /// 也不会把“已抓到的窗口”搞乱）。行号列显示的是**视图行号**，不是数据行号。
    view_rows: Option<Vec<usize>>,
    /// 【T2a-4】这份结果能不能编辑（面板同步数据时算：`ResultEntry` 编辑判据 +
    /// 面板当前连接 / 项目只读防线；`false` 时双击不进入编辑）
    editable: bool,
    /// 【T2a-4】正在编辑的单元格（有值 = 编辑中；编辑框只在这一格渲染）
    editing: Option<CellEdit>,
    /// 【T2a-4】复用的编辑框实体（宿主构造时创建，同步数据时注入；每次编辑
    /// 复用同一个，不做“每格一个实体”的泄漏）
    edit_input: Option<Entity<InputState>>,
    /// 【T2a-4】提交回调（面板注入：落变更集）
    on_commit_edit: Option<CommitEditHook>,
    /// 【T2a-3】脏格集合（**数据行序**，含列序；面板从权威变更集投影，标底色）
    dirty: Vec<(usize, usize)>,
    /// 【T2a-6】冲突格集合（提交回执标过 `Conflict` 的格子；面板从权威变更集投影）
    ///
    /// 与 `dirty` 分开：冲突格**不是**待提交的变更（提交已结束），它是「这一格没写进去、
    /// 源库里的旧值已被别人改过」的**提示**——底色用 danger 而不是 warning，一眼能分清。
    conflict: Vec<(usize, usize)>,
}

impl ResultGridDelegate {
    /// 空网格：只带一句空态说明
    pub fn empty(text: impl Into<String>) -> Self {
        Self {
            empty_text: text.into(),
            ..Default::default()
        }
    }

    /// 换一批数据（结果集变化时从权威拷贝一次；网格不持有第二份真值）
    pub fn set_data(&mut self, columns: Vec<String>, rows: Vec<Vec<String>>) {
        self.columns = columns;
        self.rows = rows;
        self.empty_text = "查询返回 0 行".to_string();
        // 换了一份结果，旧的排序与筛选不该跟过来（列都可能不是同一批）
        self.sort = None;
        // 冻结也一样：列都换了一批，钉子留在旧列号上没有意义
        self.frozen.clear();
        // 【P2】隐藏列同理：换结果 = 换一批列，旧的隐藏清单不再成立
        self.hidden.clear();
        // 【P1-5】列宽按列名记：新结果列集合变了，旧宽度不成立（与隐藏 / 冻结同口径）
        self.column_widths.clear();
        // 【P2-4】转置同理：行与列都换了，旧转置行没有意义（面板侧开关状态不随数据自动关）
        self.transposed = false;
        self.transpose_row = 0;
        self.context_cell = None;
        // 【T2a-3/4】数据被新结果顶掉：正在编辑的格子、标脏的集合都随旧数据作废
        self.editing = None;
        self.dirty.clear();
        self.conflict.clear();
        // 类型是**这一份结果自己的**：先把上一份的清掉，等面板紧跟着给新的一份
        // （漏给就是“没类型”，不会错套到新数据上）
        self.column_types.clear();
        // 【P1】注释同理（面板随后紧跟 set_column_annotations；漏给 = 表头只画列名）
        self.column_annotations.clear();
        // 【A9】来源表名同理（面板随后紧跟 set_result_table；漏给 = 不摆 INSERT 项）
        self.result_table = None;
        self.rebuild_view_rows();
    }

    /// 同步每列的类型（**与 `set_data` 同一次同步里、在它之后调**）
    ///
    /// 为什么是单独一次而不是 `set_data` 加参数：`set_data` 的签名上挂着面板的现有调用，
    /// 而类型今天还只能从另一条路来（上游填上驱动类型之前它一直是空）。
    /// 顺序反了（先给类型再 `set_data`）会被清掉——**不会**把上一份的类型套到新数据上。
    pub fn set_column_types(&mut self, types: Vec<Option<ColumnType>>) {
        self.column_types = types;
    }

    /// 【P1】同步每列的元数据注释（与 `set_data` 同一次同步里、在它之后调）
    ///
    /// 与 [`Self::set_column_types`] 同款时序：`set_data` 清掉上一份的注释，
    /// 面板紧跟给新的一份；漏给 = 表头只画列名，不套旧注释。
    pub fn set_column_annotations(&mut self, annotations: Vec<Option<ColumnAnnotation>>) {
        self.column_annotations = annotations;
    }

    /// 【P1-5】记下组件拖宽后报来的全部列宽（会话内记忆；不做落盘持久化，见 backlog）。
    ///
    /// `all` 下标是组件视图列序（`0` = 行号槽，钉死不记）；其余先经
    /// [`Self::data_column`] 落到数据列、再按**列名**存——隐藏列调整后回填仍对得上。
    /// 越界 / 列名取不到就跳过那一列，不打断别的列。
    pub fn set_column_widths(&mut self, all: &[Pixels]) {
        for (ix, width) in all.iter().enumerate() {
            let Some(view_col) = ix.checked_sub(1) else {
                continue;
            };
            let data = self.data_column(view_col);
            let Some(name) = self.columns.get(data).cloned() else {
                continue;
            };
            self.column_widths.insert(name, *width);
        }
    }

    /// 【A9】同步结果来源表名（与 `set_data` 同一次同步里、在它之后调）
    ///
    /// `None` = 从来源 SQL 猜不到 → 右键「复制整行为 INSERT」不摆（宁可没有，
    /// 也不给一条插到错误表的语句）。
    pub fn set_result_table(&mut self, table: Option<String>) {
        self.result_table = table;
    }

    /// 清空（切到无结果的模式 / 关闭文档时）
    pub fn clear(&mut self, text: impl Into<String>) {
        self.columns.clear();
        self.rows.clear();
        self.column_types.clear();
        // 【A9】来源表名跟着清（没有结果就没有 INSERT 目标）
        self.result_table = None;
        self.view_rows = None;
        self.empty_text = text.into();
        // 没有结果就没有“下一段”：留着会让滚动到底去取一份已经不存在的结果
        self.has_more = false;
        self.loading_more = false;
        // 【T2a-3/4】结果没了，编辑态与脏格一起作废
        self.editing = None;
        self.dirty.clear();
        self.conflict.clear();
    }

    /// 【B14】记下最近右键的单元格（由 `render_td` 上的右键处理调用）
    pub fn set_context_cell(&mut self, cell: Option<(usize, usize)>) {
        self.context_cell = cell;
    }

    /// 【B14】注入「按值筛选」钩子（面板构造时一次）
    pub fn set_filter_value_hook(&mut self, hook: FilterValueHook) {
        self.on_filter_value = Some(hook);
    }

    /// 【筛选】注入「列级筛选」钩子（面板构造时一次；列头菜单据此把动作交回面板）
    pub fn set_column_filter_hook(&mut self, hook: ColumnFilterHook) {
        self.on_column_filter = Some(hook);
    }

    /// 【M8】注入「洞察此列」钩子（宿主接了端口时面板构造一次）
    pub fn set_insight_column_hook(&mut self, hook: InsightColumnHook) {
        self.on_insight_column = Some(hook);
    }

    /// 【遗留 #7】注入「查看完整值」钩子（面板构造时一次）
    pub fn set_preview_value_hook(&mut self, hook: PreviewValueHook) {
        self.on_preview_value = Some(hook);
    }

    /// 【P2-3】宿主注入列统计弹层的打开钩子（没接 = 菜单不摆这一项）
    pub fn set_column_stats_hook(&mut self, hook: ColumnStatsHook) {
        self.on_column_stats = Some(hook);
    }

    /// 【M8】这份结果能不能洞察（面板每次同步结果时设；钩子没装也等于不能）
    pub fn set_insight_available(&mut self, available: bool) {
        self.insight_available = available;
    }

    /// 【M8】测试用：钩子装上了吗
    pub fn has_insight_column_hook(&self) -> bool {
        self.on_insight_column.is_some()
    }

    /// 【M8】测试用：入口现在该不该出现（钩子在 + 这份结果可洞察）
    pub fn insight_available(&self) -> bool {
        self.on_insight_column.is_some() && self.insight_available
    }

    /// 【B14】注入「排序下发」钩子（面板构造时一次）
    pub fn set_sort_down_hook(&mut self, hook: SortDownHook) {
        self.on_sort_down = Some(hook);
    }

    /// 【B15】这一数据列冻结了吗
    pub fn is_frozen(&self, column: usize) -> bool {
        self.frozen.contains(&column)
    }

    /// 【筛选】这一数据列当前有没有列级筛选生效（列头菜单据此摆「清除此列筛选」）
    pub fn has_column_filter(&self, column: usize) -> bool {
        self.column_filter
            .as_ref()
            .is_some_and(|(filtered, _)| *filtered == column)
    }

    /// 【筛选】生效中的列级筛选（`(数据列, 值)`；`None` = 没有列筛选）
    pub fn column_filter(&self) -> Option<(usize, &str)> {
        self.column_filter
            .as_ref()
            .map(|(column, value)| (*column, value.as_str()))
    }

    /// 【筛选】设置列级筛选（本地包含匹配，只改视图行序；`None` = 清除）
    pub fn set_column_filter(&mut self, column: Option<(usize, String)>) {
        self.column_filter = column;
        self.rebuild_view_rows();
    }

    /// 【筛选】这一列的去重取值（列头菜单值列表）：已抓行里按出现顺序去重，最多
    /// `COLUMN_FILTER_VALUE_LIMIT` 个；列越界 = 空切片。只读行数据，不碰网格状态。
    pub fn column_distinct_values(&self, column: usize) -> Vec<String> {
        column_distinct_values(&self.rows, column, COLUMN_FILTER_VALUE_LIMIT)
    }

    // ---------- 【P2】隐藏列：视图列序 ↔ 数据列序 ----------

    /// 这一数据列被隐藏了吗
    pub fn is_hidden_column(&self, column: usize) -> bool {
        self.hidden.contains(&column)
    }

    /// 当前可见的数据列数（总列数减隐藏；全藏没有意义，hide 会兜底留 1 列）
    pub fn visible_column_count(&self) -> usize {
        self.columns.len().saturating_sub(self.hidden.len())
    }

    /// 已隐藏的数据列数（列浏览器清单菜单「显示全部列」出现条件）
    pub fn hidden_count(&self) -> usize {
        self.hidden.len()
    }

    /// 隐藏此列（**数据列序**）。隐藏到只剩 1 列时拒绝——全藏了表格没有内容。
    pub fn hide_column(&mut self, column: usize) {
        if self.visible_column_count() <= 1 || self.is_hidden_column(column) {
            return;
        }
        self.hidden.push(column);
        self.hidden.sort_unstable();
    }

    /// 单列取消隐藏（**数据列序**；列头菜单「显示全部」是整批，侧栏眼睛要单列粒度）
    pub fn show_column(&mut self, column: usize) {
        self.hidden.retain(|c| *c != column);
    }

    /// 显示全部列（清空隐藏清单）
    pub fn show_all_columns(&mut self) {
        self.hidden.clear();
    }

    /// 列名（**数据列序**；侧栏列结构列表读它，借读不拷）
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// 每列类型（与 [`Self::columns`] 同序；空 = 这份结果没带类型）
    pub fn column_types(&self) -> &[Option<ColumnType>] {
        &self.column_types
    }

    /// 列统计：网格本地算完转交宿主钩子开弹层（列头菜单与列浏览器清单共用同一条路）。
    /// 没接钩子 = 静默返回（与现有「没接钩子就不摆入口」口径一致；清单菜单上这一项
    /// 只在钩子存在时生成，见 [`crate::grid::column_browser_menu_items`] 的使用方）。
    pub fn fire_column_stats(&self, column: usize, app: &mut App) {
        let Some(hook) = self.on_column_stats.clone() else {
            return;
        };
        let name = self.columns.get(column).cloned().unwrap_or_default();
        let text = stats_lines(&column_stats(&self.rows, column)).join("\n");
        hook(name, text, app);
    }

    /// 列统计菜单载荷（列浏览器清单菜单构造期用）：钩子 + 列名 + 统计文本一次取好，
    /// 点击时直接 `hook(name, text, app)`——不经过 `grid_entity.update` 闭包，
    /// 避免闭包内二次可变借用 `app`（与列头菜单 L2241 同一条范式）。没接钩子 = `None`。
    pub fn column_stats_payload(&self, column: usize) -> Option<(ColumnStatsHook, String, String)> {
        let hook = self.on_column_stats.clone()?;
        let name = self.columns.get(column).cloned().unwrap_or_default();
        let text = stats_lines(&column_stats(&self.rows, column)).join("\n");
        Some((hook, name, text))
    }

    // ---------- 【P2-4】单行转置（只读视图，DBeaver 式） ----------

    /// 开 / 关转置视图。`row` 为要竖排的数据行（越界自动夹取）；
    /// 关掉后原网格原样回来（排序 / 冻结 / 隐藏都没动过）。
    pub fn set_transposed(&mut self, transposed: bool, row: usize) {
        self.transposed = transposed;
        if transposed {
            self.transpose_row = row.min(self.rows.len().saturating_sub(1));
        }
    }

    /// 转置行导航：切到另一行（越界夹取）
    pub fn set_transpose_row(&mut self, row: usize) {
        self.transpose_row = row.min(self.rows.len().saturating_sub(1));
    }

    /// 当前是否处于转置视图
    pub fn is_transposed(&self) -> bool {
        self.transposed
    }

    /// 当前转置的数据行（导航 / 状态行读它）
    pub fn transpose_row(&self) -> usize {
        self.transpose_row
    }

    /// 转置投影的数据列（跳过隐藏列——转置也只投影用户看得见的列）
    fn transpose_columns(&self) -> Vec<usize> {
        (0..self.columns.len())
            .filter(|d| !self.is_hidden_column(*d))
            .collect()
    }

    /// 转置视图的单元格：第 1 列 = 原列名，第 2 列 = 选中行该列的值
    /// （`#` 行号槽不是数据，返回 `None`）
    fn transpose_cell(&self, row_ix: usize, col_ix: usize) -> Option<&str> {
        if Self::is_row_number(col_ix) {
            return None;
        }
        let data_col = *self.transpose_columns().get(row_ix)?;
        match col_ix {
            1 => self.columns.get(data_col).map(String::as_str),
            _ => self
                .rows
                .get(self.transpose_row)?
                .get(data_col)
                .map(String::as_str),
        }
    }

    /// 可见数据列序 → 数据列序（跳过隐藏列）。
    ///
    /// 组件给的下标是「用户看着的那一列」（不含 `#` 行号槽），而数据、类型、注释、
    /// 冻结都按**数据列**存——中间隔着隐藏列，换算必须过这里。面板列浏览器联动
    /// （网格选中格 → 清单选中列）同样走它。
    pub fn data_column(&self, view_col: usize) -> usize {
        let mut data = 0;
        let mut visible = 0;
        loop {
            if !self.is_hidden_column(data) {
                if visible == view_col {
                    return data;
                }
                visible += 1;
            }
            data += 1;
        }
    }

    /// 数据列序 → 可见数据列序（该列被隐藏 = `None`；清单双击定位网格用）
    pub fn view_column(&self, data_col: usize) -> Option<usize> {
        if self.is_hidden_column(data_col) {
            return None;
        }
        Some((0..data_col).filter(|d| !self.is_hidden_column(*d)).count())
    }

    /// 【B15】冻结 / 取消冻结（切换；返回切换后的状态）
    pub fn toggle_freeze(&mut self, column: usize) -> bool {
        if let Some(at) = self.frozen.iter().position(|frozen| *frozen == column) {
            self.frozen.remove(at);
            false
        } else {
            self.frozen.push(column);
            true
        }
    }

    /// 【B15】设置本地筛选词（空 = 不筛）；只改视图行序，不重查
    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.rebuild_view_rows();
    }

    /// 【B15】当前筛选词（面板与导出要用同一份真值）
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// 【B15】筛选后的行（导出跟随筛选：原型 §5.5 的口径）
    pub fn visible_rows(&self) -> Vec<Vec<String>> {
        (0..self.view_len())
            .filter_map(|view_ix| self.data_row(view_ix))
            .filter_map(|data_ix| self.rows.get(data_ix).cloned())
            .collect()
    }

    /// 【元数据】列注释访问器（与 `columns` 同序；空 = 这份结果没报注释）。
    /// 面板右栏「元数据」页用它画来源表 / 来源列 / 注释 / 类型 / 可空 / 默认值。
    pub fn column_annotations(&self) -> &[Option<ColumnAnnotation>] {
        &self.column_annotations
    }

    /// 【呈现】文本视图：当前结果拼成 TSV 纯文本（DBeaver「显示方式 → 文本」）。
    ///
    /// 列头 = 可见数据列名；行 = `visible_rows()`（已滤筛选 / 排序）按可见列取。
    /// 约定与导出一致：NULL 原样输出、Tab 分隔、行末不带 `\t`。隐藏列不参与。
    pub fn text_presentation(&self) -> String {
        let visible: Vec<usize> = (0..self.columns.len())
            .filter(|col| !self.hidden.contains(col))
            .collect();
        let header: Vec<&str> = visible
            .iter()
            .map(|col| self.columns[*col].as_str())
            .collect();
        let mut lines = vec![header.join("\t")];
        for row in self.visible_rows() {
            let cells: Vec<&str> = visible
                .iter()
                .filter_map(|col| row.get(*col).map(String::as_str))
                .collect();
            lines.push(cells.join("\t"));
        }
        lines.join("\n")
    }

    /// 【分组】按一列本地分组计数（纯函数，不引 polars——见 result-panel.md §8.8 选型记录）。
    ///
    /// 保留首次出现顺序（与网格行序一致）；NULL 作为独立组值原样输出；
    /// 越界列返回空组（面板侧先取可见列，防御性兜底）。
    pub fn group_by_count(rows: &[Vec<String>], column: usize) -> Vec<(String, usize)> {
        // 越界列：首行都没有这一列，说明列不存在（面板侧先取可见列，防御性兜底）
        let Some(first) = rows.first() else {
            return Vec::new();
        };
        if first.len() <= column {
            return Vec::new();
        }
        let values: Vec<String> = rows
            .iter()
            .map(|row| row.get(column).cloned().unwrap_or_default())
            .collect();
        group_by_values(&values)
    }

    /// 【B15】当前排序（数据列下标，是否降序）
    pub fn sort_state(&self) -> Option<(usize, bool)> {
        self.sort
    }

    /// 【B15】应用本地排序（`None` = 回原顺序）；组件库的列头点击最终走到这里
    pub fn apply_sort(&mut self, sort: Option<(usize, bool)>) {
        self.sort = sort;
        self.rebuild_view_rows();
    }

    /// 【排序】取消当前本地排序（对齐 DBeaver 排序 `NONE`；只回原顺序，不下发源库重查）
    pub fn clear_sort(&mut self) {
        self.apply_sort(None);
    }

    /// 【B15】重算视图行序（筛选 → 排序；数据不动）
    fn rebuild_view_rows(&mut self) {
        let filter = self.filter.trim().to_lowercase();
        // 既没筛选也没排序：视图行序就是数据行序——**不物化**那份映射（`None` 即恒等）。
        // 入库 / 切结果集时这一步占了整一轮逐行工作，而绝大多数时候根本用不上。
        if filter.is_empty() && self.column_filter.is_none() && self.sort.is_none() {
            self.view_rows = None;
            return;
        }
        let mut view: Vec<usize> = (0..self.rows.len())
            .filter(|ix| {
                // 全局筛选词（所有列包含匹配）与列级筛选（单列包含匹配）**叠加取交集**：
                // 一个词卡所有列，一个值卡一列，同时生效时行要两个都过。
                (filter.is_empty() || self.row_matches(*ix, &filter))
                    && self.column_filter_matches(*ix)
            })
            .collect();
        if let Some((column, descending)) = self.sort {
            // 稳定排序：同值时按原顺序（用数据行号做 tie-break，与快排的不稳定性无关）
            view.sort_by(|left, right| {
                let ordering = compare_cells(
                    self.rows.get(*left).and_then(|row| row.get(column)),
                    self.rows.get(*right).and_then(|row| row.get(column)),
                );
                let ordering = if descending { ordering.reverse() } else { ordering };
                ordering.then(left.cmp(right))
            });
        }
        self.view_rows = Some(view);
    }

    /// 这一行有没有单元格命中筛选词（大小写不敏感的子串，与 DBeaver 的本地筛选同类）
    fn row_matches(&self, row_ix: usize, needle: &str) -> bool {
        self.rows
            .get(row_ix)
            .is_some_and(|row| row.iter().any(|cell| cell_matches(cell, needle)))
    }

    /// 【筛选】这一行是否过列级筛选（单列包含匹配；没有列筛选 = 恒过）
    fn column_filter_matches(&self, row_ix: usize) -> bool {
        let Some((column, value)) = &self.column_filter else {
            return true;
        };
        self.rows
            .get(row_ix)
            .and_then(|row| row.get(*column))
            .is_some_and(|cell| cell_matches(cell, value))
    }

    /// 【B15】该数据列在表头上的排序标记
    ///
    /// `Default` = 可排但未排（组件库画的是“上下箭头”图标）；当前排序列才给升/降。
    fn sort_mark(&self, index: usize) -> ColumnSort {
        match self.sort {
            Some((column, true)) if column == index => ColumnSort::Descending,
            Some((column, false)) if column == index => ColumnSort::Ascending,
            _ => ColumnSort::Default,
        }
    }

    /// 视图行号 → 数据行号（`None` = 没有这一行）
    ///
    /// 视图行序没物化时（既没筛选也没排序）就是**恒等**：越界要按数据行数判，
    /// 不能直接把行号当数据行号返回。
    fn data_row(&self, row_ix: usize) -> Option<usize> {
        match &self.view_rows {
            Some(view) => view.get(row_ix).copied(),
            None => (row_ix < self.rows.len()).then_some(row_ix),
        }
    }

    /// 视图行数（没筛选/排序时就是数据行数）
    pub fn view_len(&self) -> usize {
        self.view_rows.as_ref().map_or(self.rows.len(), Vec::len)
    }

    /// 这份结果还能不能取下一段（面板在结果变化时同步；`true` 才启用滚动到底加载）
    pub fn set_has_more(&mut self, has_more: bool) {
        self.has_more = has_more;
    }

    /// 正在取下一段（回填之前不该重复触发）
    pub fn set_loading_more(&mut self, loading_more: bool) {
        self.loading_more = loading_more;
    }

    /// 注入取下一段的回调（面板构造时一次）
    pub fn set_load_more_hook(&mut self, hook: LoadMoreHook) {
        self.on_load_more = Some(hook);
    }

    /// 能不能现在就取下一段（纯函数：面板与测试都是同一套判据）
    pub fn wants_more(&self) -> bool {
        self.has_more && !self.loading_more && self.on_load_more.is_some()
    }

    /// 测试用：右键菜单的两个钩子接上了吗（接线正确性；真点击在窗口里驱动）
    ///
    /// 2026-10-23 去 `#[cfg(test)]`：作为**依赖**编译时 cfg(test) 不生成，而 editor 的
    /// 窗口测试（`view/tests.rs`）也断言它——测试访问器成为本 crate 公开 API。
    pub fn has_filter_value_hook(&self) -> bool {
        self.on_filter_value.is_some()
    }

    /// 测试用：视图行序现在是**物化**的吗（`false` = 恒等，入库 / 切结果集时那份映射被省掉了）
    pub fn view_order_materialized_for_test(&self) -> bool {
        self.view_rows.is_some()
    }

    /// 测试用：见上
    pub fn has_sort_down_hook(&self) -> bool {
        self.on_sort_down.is_some()
    }

    /// 网格里的列名（供测试断言；不复制行数据）
    pub fn columns_for_test(&self) -> &[String] {
        &self.columns
    }

    /// 网格里的行数（供测试断言）
    pub fn row_count_for_test(&self) -> usize {
        self.rows.len()
    }

    /// 筛选后能看见多少行（视图行数）
    pub fn visible_row_count(&self) -> usize {
        self.view_len()
    }

    /// 这份结果一共抓到多少行（数据行数，与筛选无关）
    pub fn data_row_count(&self) -> usize {
        self.rows.len()
    }

    /// 第 `col_ix` 列是不是行号槽（首列；没有数据时不画它）
    fn is_row_number(col_ix: usize) -> bool {
        col_ix == 0
    }

    /// 第 `col_ix` 列（**含行号槽**）的语义类型：行号槽与没带类型的列都是 `Unknown`
    ///
    /// 渲染只读这一条（对齐 / 等宽），每格 O(1)：类型在 `set_column_types` 时就已经解析好了
    /// （每列一次，不是每格一次）。
    fn column_kind(&self, col_ix: usize) -> ColumnKind {
        // 【P2-4】转置视图的列是混合类型（每行对应一个不同的原列），列级类型不成立——
        // 单元格自己的类型由 `render_td` 按行取（那里能拿到原列）
        if self.transposed {
            return ColumnKind::Unknown;
        }
        if Self::is_row_number(col_ix) {
            return ColumnKind::Unknown;
        }
        self.column_types
            .get(self.data_column(col_ix - 1))
            .and_then(Option::as_ref)
            .map_or(ColumnKind::Unknown, |column| column.kind)
    }

    /// 表头的类型小标签：驱动报的**原始名字**（行号槽 / 没带类型 = `None`，那就一个字也不摆）
    fn type_label(&self, col_ix: usize) -> Option<&str> {
        // 【P2-4】转置列是混合类型，不给列级标签
        if self.transposed {
            return None;
        }
        if Self::is_row_number(col_ix) {
            return None;
        }
        self.column_types
            .get(self.data_column(col_ix - 1))
            .and_then(Option::as_ref)
            .map(|column| column.name.as_str())
    }

    // ---------- 【T2a-4】单元格编辑 ----------

    /// 这份结果能否编辑（面板同步数据时算；`false` 时双击不进入编辑）
    pub fn set_editable(&mut self, editable: bool) {
        self.editable = editable;
    }

    /// 编辑框实体（宿主构造时创建、同步数据时注入；编辑框渲染用同一个实体）
    pub fn set_edit_input(&mut self, input: Entity<InputState>) {
        self.edit_input = Some(input);
    }

    /// 提交回调（宿主注入：落变更集）
    pub fn set_commit_edit_hook(&mut self, hook: CommitEditHook) {
        self.on_commit_edit = Some(hook);
    }

    /// 【T2a-3】脏格集合（面板从权威变更集投影；**数据行序**，含数据列序）
    pub fn set_dirty(&mut self, dirty: Vec<(usize, usize)>) {
        self.dirty = dirty;
    }

    /// 【T2a-6】冲突格集合（提交回执标过 `Conflict`；面板从权威变更集投影）
    pub fn set_conflict(&mut self, conflict: Vec<(usize, usize)>) {
        self.conflict = conflict;
    }

    /// 这一格是不是脏的（视图行序 + 组件列序 → 数据行 / 数据列比较）
    fn is_dirty(&self, row_ix: usize, col_ix: usize) -> bool {
        // 【P2-4】转置是只读视图，没有待提交的变更
        if self.transposed {
            return false;
        }
        if Self::is_row_number(col_ix) {
            return false;
        }
        let Some(data_ix) = self.data_row(row_ix) else {
            return false;
        };
        self.dirty.contains(&(data_ix, self.data_column(col_ix - 1)))
    }

    /// 这一格是不是冲突的（提交没写进去；与脏格分开标色）
    fn is_conflict(&self, row_ix: usize, col_ix: usize) -> bool {
        // 【P2-4】转置是只读视图，没有提交冲突
        if self.transposed {
            return false;
        }
        if Self::is_row_number(col_ix) {
            return false;
        }
        let Some(data_ix) = self.data_row(row_ix) else {
            return false;
        };
        self.conflict
            .contains(&(data_ix, self.data_column(col_ix - 1)))
    }

    /// 这一格是否正在编辑（视图行序 + 组件列序匹配）
    pub fn is_editing(&self, row_ix: usize, col_ix: usize) -> bool {
        // 【P2-4】转置是只读视图，不进编辑
        if self.transposed {
            return false;
        }
        if Self::is_row_number(col_ix) {
            return false;
        }
        self.editing.as_ref().is_some_and(|edit| {
            edit.view_row == row_ix && self.view_column(edit.column) == Some(col_ix - 1)
        })
    }

    /// 进入编辑：只设状态（初值同步 / 聚焦由 render_td 首次渲染时一次性做）。
    /// 返回是否真的进入了（不可编辑 / 点行号槽 / 行越界 = `false`）。
    pub fn begin_edit(&mut self, view_row: usize, col_ix: usize) -> bool {
        // 【P2-4】转置是只读视图，双击不进入编辑
        if self.transposed || !self.editable || self.editing.is_some() || Self::is_row_number(col_ix)
        {
            return false;
        }
        let Some(data_row) = self.data_row(view_row) else {
            return false;
        };
        let column = self.data_column(col_ix - 1);
        let original = self
            .row_cell_text(view_row, col_ix)
            .unwrap_or_default()
            .to_string();
        self.editing = Some(CellEdit {
            view_row,
            data_row,
            column,
            value: original.clone(),
            original,
            settled: false,
            error: None,
        });
        true
    }

    /// 取消编辑（Esc）：丢弃输入值回到非编辑态。**不**动变更集。
    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// 提交编辑（Enter / 失焦）：取当前输入值走钩子交回面板。
    /// 这里先清编辑态——面板落变更集后，编辑框不再渲染，InputState 失焦
    /// 触发的 Blur 看到编辑态已空，不会重复提交。
    ///
    /// 【类型感知】提交前按列类型校验（数字列必须数字、布尔列取值受限…）；
    /// 非法 = **拒绝提交**（编辑态保留 + `error` 记录，渲染红框提示），返回 `false`。
    /// 返回 `true` 才真的交了。调用方（宿主 Enter / Blur 订阅）拿返回值决定要不要
    /// 投影刷新：失败时只重绘（显示错误），不去跑整轮结果投影。
    pub fn commit_edit(&mut self, cx: &mut App) -> bool {
        // 取编辑态先只读：校验失败要把错误写回它，不能 take 后再补
        let Some(edit) = &self.editing else {
            return false;
        };
        let kind = self
            .column_types
            .get(edit.column)
            .and_then(|t| t.as_ref())
            .map(|t| t.kind);
        if let Some(kind) = kind {
            if let Some(message) = validate_edit_value(kind, &edit.value) {
                self.editing.as_mut().unwrap().error = Some(message);
                return false;
            }
        }
        let Some(edit) = self.editing.take() else {
            return false;
        };
        if let Some(hook) = &self.on_commit_edit {
            hook(edit.data_row, edit.column, edit.original, edit.value, cx);
        }
        true
    }

    /// 编辑框输入值同步（`InputEvent::Change` 时由宿主调；不区分“谁发的”）。
    /// 输入一变化就清掉上次的提交错误——用户已经动手改，红框该撤。
    pub fn sync_edit_value(&mut self, value: String) {
        if let Some(edit) = &mut self.editing {
            edit.value = value;
            edit.error = None;
        }
    }

    /// 当前编辑态（render_td 画编辑框用；借读不拷贝）
    pub fn editing(&self) -> Option<&CellEdit> {
        self.editing.as_ref()
    }

    /// 编辑框实体（宿主与测试聚焦 / 读值时用；未进入编辑也可能已注入——实体是复用的）
    pub fn edit_input(&self) -> Option<Entity<InputState>> {
        self.edit_input.clone()
    }

    /// 标记编辑框已完成“初值同步 + 聚焦”（render_td 首次渲染时调，防重复）
    fn mark_edit_settled(&mut self) {
        if let Some(edit) = &mut self.editing {
            edit.settled = true;
        }
    }

    /// 【T2a-4】编辑框（正在编辑的那一格）：`Input` 组件 + 初值同步 / 聚焦（幂等一次）+
    /// Escape 取消。渲染位仍是原生的 `render_td`（属于 `TableDelegate` 的调用点）；
    /// Enter 提交走 `InputEvent::PressEnter` 订阅（宿主接），Esc 因输入内核默认
    /// `cx.propagate()`（不消费、不清值）能传到外层这里。
    ///
    /// 为什么放在普通 impl 而不是 trait impl：这是网格**自己的**渲染细节，不是
    /// `TableDelegate` 接口的一部分（组件库不关心编辑框长什么样）。
    fn render_edit_cell(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let state = self.edit_input.clone().expect("宿主必注入编辑框实体");
        // 幂等一次性：首次渲染时同步初值（`set_value` **不发** `InputEvent::Change`，
        // 不会把“用户改了才做”的副作用重跑一遍）。
        // 聚焦**不在这里做**（render 是纯读阶段，`window.focus` 不可靠）：由宿主在
        // 进入编辑的事件路径给（`host.rs` 双击分支），焦点事件在下一帧 draw 时派发。
        if self.editing().is_some_and(|edit| !edit.settled) {
            let initial = self
                .editing()
                .map(|edit| edit.value.clone())
                .unwrap_or_default();
            state.update(cx, |state, cx| state.set_value(initial, window, cx));
            self.mark_edit_settled();
        }
        let theme = cx.theme();
        // 【类型感知】提交校验失败的错误（红框 + 悬停提示）；无错误 = 与今天完全一致
        let error = self.editing().and_then(|edit| edit.error.clone());
        let danger = theme.colors.danger;
        // Escape：输入内核默认 `cx.propagate()`（不消费、不清值），在这里拦下 → 取消编辑。
        // 元素回调没有实体参数，把表格实体抓到闭包里再 update。
        let table = cx.entity().clone();
        let input = Input::new(&state)
            .id(format!("editor-result-cell-input-{row_ix}-{col_ix}"))
            .w_full()
            .h_full()
            .bordered(false)
            .bg(theme.colors.background);
        div()
            .id(format!("editor-result-edit-cell-{row_ix}-{col_ix}"))
            .size_full()
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .on_key_down(move |event: &KeyDownEvent, _window, cx| {
                // gpui 的键名是字符串（`Keystroke.key`），与 keymap 绑定一致：`"escape"`
                if event.keystroke.key == "escape" {
                    table.update(cx, |state, cx| {
                        state.delegate_mut().cancel_edit();
                        cx.notify();
                    });
                }
            })
            .when_some(error, |cell, message| {
                // 提交被拒：1px 红框 + 悬停看错误文案（单元格高度是行高，文本没地方放）
                cell.border_1()
                    .border_color(danger)
                    .tooltip(move |window, cx| Tooltip::new(message.clone()).build(window, cx))
            })
            .child(input)
    }

    /// 单元格文案：首列是行号，其余按 `col_ix - 1` 取数据
    ///
    /// 行号与数据都走**视图行序**（筛选/排序之后看着的那一行）；
    /// 缺列/缺行给空串而不是 panic（结果行由驱动给出，形状不可全信）。
    fn cell(&self, row_ix: usize, col_ix: usize) -> String {
        if Self::is_row_number(col_ix) {
            return (row_ix + 1).to_string();
        }
        self.row_cell_text(row_ix, col_ix)
            .unwrap_or_default()
            .to_string()
    }

    /// 这一格的文本（**只借不拷**；`#` 行号槽与不存在的格都是 `None`）
    ///
    /// “这一格是什么”只此一份：`cell()` 画它、结果内查找搜它、将来类型化导出也读它。
    /// 行号槽不在这里（它不是一个数据格：显示的是视图行号，而视图行号由渲染现算）。
    fn row_cell_text(&self, row_ix: usize, col_ix: usize) -> Option<&str> {
        // 【P2-4】转置视图：单元格是「字段名 / 值」，走转置投影
        if self.transposed {
            return self.transpose_cell(row_ix, col_ix);
        }
        if Self::is_row_number(col_ix) {
            return None;
        }
        let data_ix = self.data_row(row_ix)?;
        self.rows
            .get(data_ix)?
            .get(self.data_column(col_ix - 1))
            .map(String::as_str)
    }

    /// 结果内查找：从 `after` **之后**找下一处命中，找完一圈回到开头（`None` = 整份结果里没有）
    ///
    /// 口径与本地筛选**同一个实现**（[`find_in_cell`]）：大小写不敏感的子串；只看**看得见的行**
    /// （筛选 / 排序之后的视图行序——找到看不见的行等于没找到）；`#` 行号槽不是数据，不参与。
    ///
    /// `after = None` = 从第一格开始。区间只在与词**都是 ASCII** 时给出（折叠不改字节长度）；
    /// 含非 ASCII 时是 `None`，宿主退化成**整格**高亮（`İ` → 两个字符，折叠后的字节偏移在原文里
    /// 不是合法边界，格内高亮的区间映射要单独设计，不在本切片）。
    ///
    /// 为什么是纯读：查找**不改任何状态**——选中哪一格、要不要滚过去、要不要画高亮都是宿主的
    /// 决定（宿主才知道焦点与滚动句柄）。这里只回答“下一处在哪”。
    pub fn find_next(&self, needle: &str, after: Option<(usize, usize)>) -> Option<ResultMatch> {
        let needle = needle.trim().to_lowercase();
        let columns = self.columns.len();
        let rows = self.view_len();
        if needle.is_empty() || columns == 0 || rows == 0 {
            return None;
        }
        // 把（视图行, 列）当成一条带子上的格子：搜一圈就是一条不大不小的扫描
        let per_row = columns + 1;
        let cells = rows * per_row;
        let start = match after {
            Some((row, col)) if col < per_row => {
                row.saturating_mul(per_row).saturating_add(col + 1) % cells
            }
            // 没给过起点，或起点已经不在这份结果里（刷新 / 换结果之后）：从头找
            _ => 0,
        };
        for step in 0..cells {
            let flat = (start + step) % cells;
            let (row, col) = (flat / per_row, flat % per_row);
            let Some(text) = self.row_cell_text(row, col) else {
                continue;
            };
            if let Some(range) = find_in_cell(text, &needle) {
                return Some(ResultMatch { row, col, range });
            }
        }
        None
    }

    /// 这一数据列在**当前视图行序**下的全部取值（一行一个，换行分隔；逐格按 TSV 转义）
    ///
    /// 只给**已抓到的行**、不重查源库（与 mock 预览「复制此列」同一口径）：
    /// 一份十万行的结果整列就是十万个值，粘到哪里都不好用——真要全列就导出。
    /// 列浏览器清单「复制此列」与右键菜单共用这份文本。
    pub fn column_text(&self, column: usize) -> String {
        let mut text = String::new();
        for (index, data_ix) in (0..self.view_len())
            .filter_map(|view_ix| self.data_row(view_ix))
            .enumerate()
        {
            if index > 0 {
                text.push('\n');
            }
            let cell = self
                .rows
                .get(data_ix)
                .and_then(|row| row.get(column))
                .map(String::as_str)
                .map(tsv_cell)
                .unwrap_or_default();
            text.push_str(&cell);
        }
        text
    }

    /// 【P2-2】当前视图行序下，某数据列的全部取值：只 clone 目标列字符串，
    /// 不物化整行矩阵。计算页统计 / 分组页计数共用这份访问器——原先 `visible_rows()`
    /// 会把每行全列都拷一遍，万行 × 多列时是每帧 render 的冗余 O(行×列) 拷贝。
    pub fn column_values(&self, column: usize) -> Vec<String> {
        (0..self.view_len())
            .filter_map(|view_ix| self.data_row(view_ix))
            .map(|data_ix| {
                self.rows
                    .get(data_ix)
                    .and_then(|row| row.get(column))
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }

    /// 【P2-2】计算页：某数据列统计（列级访问器 → 列级纯函数，口径与矩阵版一致）
    pub fn column_stats_for(&self, column: usize) -> ColumnStats {
        column_stats_from_values(&self.column_values(column))
    }

    /// 【P2-2】分组页：某数据列本地分组计数（同上，不物化整行矩阵）
    pub fn group_by_for(&self, column: usize) -> Vec<(String, usize)> {
        group_by_values(&self.column_values(column))
    }

    /// 这一行拼成 TSV（**视图行序**：复制的是用户看着的那一行，不是数据行序）
    fn row_text(&self, row_ix: usize) -> String {
        // 【P2-4】转置一行 = 一个字段：字段名 + 值（菜单不摆复制项，这里是兜底）
        if self.transposed {
            let field = self.transpose_cell(row_ix, 1).unwrap_or_default();
            let value = self.transpose_cell(row_ix, 2).unwrap_or_default();
            return format!("{field}\t{value}");
        }
        let Some(data_ix) = self.data_row(row_ix) else {
            return String::new();
        };
        tsv_row(&self.visible_row_values(data_ix))
    }

    /// 【P2】按可见列投影一行（隐藏列不参与复制 / 导出——用户看不到的不该拷走）
    fn visible_row_values(&self, data_ix: usize) -> Vec<String> {
        let Some(row) = self.rows.get(data_ix) else {
            return Vec::new();
        };
        (0..self.columns.len())
            .filter(|d| !self.is_hidden_column(*d))
            .filter_map(|d| row.get(d).cloned())
            .collect()
    }

    /// 【P2】可见列名（隐藏列不参与整行 JSON / INSERT）
    fn visible_column_names(&self) -> Vec<String> {
        (0..self.columns.len())
            .filter(|d| !self.is_hidden_column(*d))
            .filter_map(|d| self.columns.get(d).cloned())
            .collect()
    }

    /// 【A9】这一行拼成 JSON 对象（视图行序，与 [`Self::row_text`] 同口径）
    fn row_json_text(&self, row_ix: usize) -> String {
        let Some(data_ix) = self.data_row(row_ix) else {
            return String::new();
        };
        crate::export::row_to_json(&self.visible_column_names(), &self.visible_row_values(data_ix))
    }

    /// 【A9】这一行拼成 INSERT（视图行序；表名来自面板同步的 [`Self::result_table`]——
    /// 菜单摆出这一项时表名必然已确定，这里只是按当时的行取值）
    fn row_insert_text(&self, row_ix: usize) -> String {
        let Some(data_ix) = self.data_row(row_ix) else {
            return String::new();
        };
        let Some(table) = self.result_table.as_deref() else {
            return String::new();
        };
        crate::export::row_to_insert(
            table,
            &self.visible_column_names(),
            &self.visible_row_values(data_ix),
        )
    }

    /// 【B14】选中内容的复制文本（`Ctrl+C` 用）：一格给**原文**，一整行给 TSV
    ///
    /// 与右键「复制此值 / 复制整行（TSV）」同一套口径与同一份实现（本方法就是它俩的入口）：
    /// 单格不转义（要的就是那一格的值），整行才拼 TSV。
    /// `None` = 没东西可复制（选择指向的行已经不在视图里了——筛选 / 刷新之后可能如此）。
    pub fn selection_text(&self, selection: GridSelection) -> Option<String> {
        let row = match selection {
            GridSelection::Cell { row, .. } | GridSelection::Row { row } => row,
        };
        // 行不在了就说清“没东西可复制”，而不是安静地拷一个空串（用户会粘出一片空白）
        if self.data_row(row).is_none() {
            return None;
        }
        Some(match selection {
            GridSelection::Cell { row, col } => self.cell(row, col),
            GridSelection::Row { row } => self.row_text(row),
        })
    }
}

/// 单元格 / 表头的悬停全文：取值与列名都可能很宽，`truncate` 之后只剩这一条看全的路。
///
/// 宽度走 `ui::RESULT_TOOLTIP_MAX_WIDTH`（结构尺寸只认常量表），并让它**折行**：
/// 悬停提示跟着鼠标，横向越宽越容易被窗口边缘截掉。
fn result_cell_tooltip(text: String, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, _| {
        div()
            .max_w(rems(ui::RESULT_TOOLTIP_MAX_WIDTH))
            .whitespace_normal()
            .text_xs()
            .child(text.clone())
    })
    .build(window, cx)
}

/// 【P1】表头悬停的分块内容：第一行「列名 · 类型」，后续行字段元数据（来源 / 注释 / 可空 / 默认）
///
/// 返回 `(标题行, 元数据行列表)`。注释缺失 = 空列表（与今天的单行 tooltip 等效）。
/// 多行不拼 `\n` 字符串：tooltip 容器是 `whitespace_normal`，换行会被折叠成空格，
/// 由调用方逐行 `child` 渲染成块。
fn header_tooltip_lines(
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

/// 【B14】网格里**最近一次选中**（`Ctrl+C` 复制谁）
///
/// 为什么由面板记事件而不是读组件状态：组件没公开“当前是选格还是选行”，而
/// `selected_cell()` / `selected_row()` 可以**同时有值**（选过整行再点一个格子）——
/// 猜哪个更新会猜错，所以只认最近一次选择事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSelection {
    /// 选中一格（`col` 是含行号槽的列下标）
    Cell { row: usize, col: usize },
    /// 选中一整行（**视图行序**）
    Row { row: usize },
}

/// 结果内查找的一个命中（[`ResultGridDelegate::find_next`] 的产物）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultMatch {
    /// **视图行号**（筛选 / 排序之后看着的那一行）
    pub row: usize,
    /// 含行号槽的列下标
    pub col: usize,
    /// 格内区间（字节；`None` = 命中但给不出区间，见 [`ResultGridDelegate::find_next`]）
    pub range: Option<Range<usize>>,
}

/// 菜单里的值预览（短、单行；太长的值不该把菜单撑开）
fn preview_of(value: &str) -> String {
    let folded: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.chars().count() > 24 {
        let mut text: String = folded.chars().take(24).collect();
        text.push('…');
        text
    } else {
        folded
    }
}

/// 两个单元格比大小（本地排序用）
///
/// 两边都像数字就按**数值**比（`1, 2, 10` 不该排成 `1, 10, 2`——展示文本是字符串，
/// 但用户看的是数）；否则按字符串比。`None`（缺列）排在最后。
fn compare_cells(left: Option<&String>, right: Option<&String>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (Some(left), Some(right)) = (left, right) else {
        return match (left.is_some(), right.is_some()) {
            (false, false) => Ordering::Equal,
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (true, true) => Ordering::Equal,
        };
    };
    if let (Ok(left), Ok(right)) = (left.parse::<f64>(), right.parse::<f64>()) {
        return left.partial_cmp(&right).unwrap_or(Ordering::Equal);
    }
    left.cmp(right)
}

/// 一格的渲染决策（**纯函数**产出就是这一份；渲染只负责把它落到元素上）
///
/// 为什么单独拿出来：`Unknown` 这一档必须是**今天的样式**，而“没带类型就一个像素不变”
/// 这句话只有被断言过才算数——渲染函数里测不到（gpui 不暴露算出来的对齐与字体）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    /// 文本对齐（数字右对齐：个位对齐才看得出一列的数量级差）
    align: TextAlign,
    /// 用等宽字体吗（布尔 / 时间族；字体族从主题取，不引新依赖）
    monospace: bool,
}

/// 列类型 → 这一列所有格子的样式
fn cell_style(kind: ColumnKind) -> CellStyle {
    CellStyle {
        align: if kind.is_numeric() {
            TextAlign::Right
        } else {
            TextAlign::Left
        },
        monospace: kind.is_monospace(),
    }
}

/// 【P2-3】一列的统计结果（数值字段只在列里有可解析的数值样本时才填）
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColumnStats {
    /// 非 NULL 的行数
    pub count: usize,
    /// `NULL` 的行数（与渲染同一判定）
    pub nulls: usize,
    /// 数值样本的最小值
    pub min: Option<f64>,
    /// 数值样本的最大值
    pub max: Option<f64>,
    /// 数值样本之和
    pub sum: Option<f64>,
    /// 数值样本均值
    pub avg: Option<f64>,
}

/// 【P2-3】整列统计：遍历该列一次，`NULL`（与渲染同款判定）计空值，
/// 其余值能按 f64 解析的参与数值统计；文本 / 日期 / BLOB 只计非空数。
/// 纯函数（输入只有 rows），方便单测。
pub fn column_stats(rows: &[Vec<String>], column: usize) -> ColumnStats {
    let values: Vec<String> = rows
        .iter()
        .map(|row| row.get(column).cloned().unwrap_or_default())
        .collect();
    column_stats_from_values(&values)
}

/// 【P2-3】列级统计内核：输入已经是「一列取值」，不必再套二维矩阵。
/// delegate 的列级访问器（`column_stats_for`）与矩阵版 `column_stats` 共用这份口径。
pub fn column_stats_from_values(values: &[String]) -> ColumnStats {
    let mut stats = ColumnStats::default();
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut sum = 0.0;
    let mut numeric = 0usize;
    for cell in values {
        if cell == "NULL" {
            stats.nulls += 1;
            continue;
        }
        stats.count += 1;
        if let Ok(value) = cell.parse::<f64>() {
            min = min.min(value);
            max = max.max(value);
            sum += value;
            numeric += 1;
        }
    }
    stats.min = (numeric > 0).then_some(min);
    stats.max = (numeric > 0).then_some(max);
    stats.sum = (numeric > 0).then_some(sum);
    stats.avg = (numeric > 0).then_some(sum / numeric as f64);
    stats
}

/// 【分组】列级计数内核：输入已经是「一列取值」；保留首现顺序，`NULL` 独立成组。
pub fn group_by_values(values: &[String]) -> Vec<(String, usize)> {
    let mut groups: Vec<(String, usize)> = Vec::new();
    for value in values {
        if let Some((_, count)) = groups.iter_mut().find(|(v, _)| *v == *value) {
            *count += 1;
        } else {
            groups.push((value.clone(), 1));
        }
    }
    groups
}

/// 【P2-3】统计的展示行（一行一个键值；数值有则给，没有就不摆那一行）
pub fn stats_lines(stats: &ColumnStats) -> Vec<String> {
    let mut lines = vec![
        format!("值 {} 个", stats.count),
        format!("NULL {} 个", stats.nulls),
    ];
    let mut push_num = |label: &str, value: f64| {
        lines.push(format!("{label} {}", fmt_stat_num(value)));
    };
    if let Some(min) = stats.min {
        push_num("最小", min);
    }
    if let Some(max) = stats.max {
        push_num("最大", max);
    }
    if let Some(avg) = stats.avg {
        push_num("均值", avg);
    }
    if let Some(sum) = stats.sum {
        push_num("合计", sum);
    }
    lines
}

/// 统计数字的人读格式：固定 6 位小数截尾，去尾零（`1.500000` → `1.5`）
fn fmt_stat_num(value: f64) -> String {
    let mut text = format!("{value:.6}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// 单元格展示文本字符级裁剪：超过 `limit` 个字符时取前 `limit` 个字符并补省略号 `…`。
///
/// 按 `char` 边界取前若干个，不会裁断中文 / emoji / 组合字符的多字节序列；省略号不占
/// `limit` 额度（与 dbdelve `clip` 同口径：300 字符 + 一个尾巴符号）。未达上限原样返回，
/// 不做换行展平或其它改写——换行由单元格样式自己处理。
fn clip_cell_display(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    let mut clipped: String = value.chars().take(limit).collect();
    clipped.push('…');
    clipped
}

/// 【P1-4】格内 JSON 美化展示：仅当文本以 `{` / `[` 开头且能解析成 JSON 时，
/// 返回缩进格式化串；非 JSON / 解析失败一律 `None`（按普通文本走裁剪，不猜）。
///
/// 只改**展示**：复制 / 悬停 / 右键 / 导出 / 编辑仍取原始 `text`（见 `render_td`）。
/// 裸标量（数字 / 布尔 / 引号串）本来就短，不进这条路径；解析失败不抛错、不 panic。
fn pretty_json_display(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !matches!(trimmed.chars().next(), Some('{') | Some('[')) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    serde_json::to_string_pretty(&value).ok()
}

/// 【P2】BLOB 单元格的摘要文案：`hex → BLOB · N B`
///
/// Arrow 的 display 把二进制序列化成**小写 hex**（每字节两位，见 `arrow-cast` 的
/// `DisplayIndex for GenericBinaryArray`），所以 `len / 2` = 字节数。非偶数 / 含
/// 非 hex 字符的串（驱动 / 上游异常）退回 `None`，按原文本显示，不猜。
fn blob_summary(text: &str) -> Option<String> {
    let hex = text.strip_prefix("0x").unwrap_or(text);
    if hex.len() >= 2
        && hex.len() % 2 == 0
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        Some(format!("BLOB · {} B", hex.len() / 2))
    } else {
        None
    }
}

/// 单元格里找 `needle`（**已小写**）的字节区间：两边都是 ASCII → 零分配
fn ascii_match_range(cell: &str, needle: &str) -> Option<Range<usize>> {
    let (haystack, needle) = (cell.as_bytes(), needle.as_bytes());
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|at| at..at + needle.len())
}

/// 格内命中的唯一实现：本地筛选与结果内查找都走它（一份口径，不许两套）
///
/// - `Some(Some(range))`：命中，且给得出**格内区间**（格与词都是 ASCII —— 折叠不改字节长度）；
/// - `Some(None)`：命中，但给不出区间（含非 ASCII）；
/// - `None`：没命中。
///
/// 为什么要单独一条 ASCII 快路径：筛选框每敲一下都要把**整表**过一遍，原先每格
/// `to_lowercase()` 一次分配（10 万行 × 20 列 = 200 万次），而结果里绝大多数格子是 ASCII。
/// 含非 ASCII 时回落 `to_lowercase`：`str::to_lowercase` 有上下文相关的特例（如希腊文末位
/// sigma），自写的逐字节比较会与它不一致——**口径不能有两种**，宁可慢一点。
fn find_in_cell(cell: &str, needle: &str) -> Option<Option<Range<usize>>> {
    if needle.is_empty() {
        return None;
    }
    if cell.is_ascii() && needle.is_ascii() {
        return ascii_match_range(cell, needle).map(Some);
    }
    cell.to_lowercase().contains(needle).then_some(None)
}

/// 单元格命中判据（筛选用：只看有没有）
fn cell_matches(cell: &str, needle: &str) -> bool {
    find_in_cell(cell, needle).is_some()
}

/// 【类型感知】按列类型校验编辑值（提交前调用；空 / `NULL` 一律放行——清掉
/// 这一格是“删值”语义，不该被类型挡住）。返回错误文案（`None` = 合法）。
pub fn validate_edit_value(kind: ColumnKind, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "NULL" {
        return None;
    }
    match kind {
        ColumnKind::Integer => value
            .parse::<i64>()
            .map(|_| ())
            .map_err(|_| "必须是整数（如 42）".to_string())
            .err(),
        ColumnKind::Float | ColumnKind::Decimal => value
            .parse::<f64>()
            .map(|_| ())
            .map_err(|_| "必须是数字（如 3.14）".to_string())
            .err(),
        ColumnKind::Boolean => {
            if matches!(
                value.to_lowercase().as_str(),
                "true" | "false" | "1" | "0" | "t" | "f" | "yes" | "no"
            ) {
                None
            } else {
                Some("布尔列只能填 true / false".to_string())
            }
        }
        ColumnKind::Uuid => {
            if looks_like_uuid(value) {
                None
            } else {
                Some(
                    "UUID 格式应为 8-4-4-4-12（如 550e8400-e29b-41d4-a716-446655440000）"
                        .to_string(),
                )
            }
        }
        ColumnKind::Date => {
            if looks_like_date(value) {
                None
            } else {
                Some("日期格式应为 YYYY-MM-DD".to_string())
            }
        }
        // Text / Timestamp / Time / Json / Binary / Unknown：不校验（文本自由、
        // 时刻格式各驱动不一、JSON / 二进制自由文本——交给源库自己判断）
        _ => None,
    }
}

/// UUID 格式探针（不引 uuid crate：只查形状——`8-4-4-4-12` hex，大小写皆可）
fn looks_like_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    [8, 13, 18, 23].iter().all(|&at| bytes[at] == b'-')
        && bytes
            .iter()
            .enumerate()
            .filter(|(i, _)| ![8, 13, 18, 23].contains(i))
            .all(|(_, b)| b.is_ascii_hexdigit())
}

/// 日期格式探针（`YYYY-MM-DD`，粗查形状 + 月 / 日范围；不引 chrono）
fn looks_like_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !bytes.iter().enumerate().all(|(i, b)| {
        i == 4 || i == 7 || b.is_ascii_digit()
    }) {
        return false;
    }
    let month = (bytes[5] - b'0') * 10 + (bytes[6] - b'0');
    let day = (bytes[8] - b'0') * 10 + (bytes[9] - b'0');
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

impl TableDelegate for ResultGridDelegate {
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

    /// 表头单元格：组件默认只画列名（`div().size_full().child(name)`），这里补三件事——
    /// **截断**、**悬停全文**、**列类型小标签**。列名常常比默认列宽（128px）长，截断之后
    /// 就没有第二条知道它是什么的路（单元格取值有悬停，表头同样需要）。
    ///
    /// 类型小标签摆在列名**右侧**并且 `flex_none`：先截断的总是列名——反过来的话，
    /// 窄列上被省略号吃掉的恰好是“类型”（那正是想看它的时候）。
    ///
    /// 行号槽（`#`）不挂悬停也不挂标签——它不是数据。`debug_selector` 是给用例找这个表头用的
    /// （`.id(...)` 不登记坐标）：表头的排序箭头就在它右端，直调 `perform_sort` 验不到那段几何。
    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let name = if Self::is_row_number(col_ix) {
            "#".to_string()
        } else if self.transposed {
            // 转置视图固定两列：字段 / 值（列名不跟原列走）
            match col_ix {
                1 => "字段".to_string(),
                _ => "值".to_string(),
            }
        } else {
            self.columns
                .get(self.data_column(col_ix - 1))
                .cloned()
                .unwrap_or_default()
        };
        // 类型缺失时 `label` 是 `None`：这一档与今天一模一样（表头就一个列名）
        let label = self.type_label(col_ix).map(str::to_string);
        // 【筛选】列筛选指示要按数据列查，这里先折算（行号槽 / 转置视图没有列筛选）
        let column = if Self::is_row_number(col_ix) || self.transposed {
            usize::MAX
        } else {
            self.data_column(col_ix - 1)
        };
        let muted = cx.theme().colors.muted_foreground;

        let head = div()
            .h_flex()
            .items_center()
            .gap_1()
            .size_full()
            .min_w_0()
            .truncate()
            .debug_selector(move || format!("editor-result-th-{col_ix}"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(SharedString::from(name.clone())),
            )
            .when(self.has_column_filter(column), |head| {
                // 列级筛选生效：列名后加 muted 小标记（DBeaver 列头筛选指示的轻量版；
                // 不用 emoji——`▽` 是几何字符，与排序箭头的 `▾/▴` 区分）
                head.child(
                    div().text_xs().text_color(muted).child(SharedString::from("▽")),
                )
            })
            .when_some(label.clone(), |head, label| {
                head.child(
                    div()
                        .flex_none()
                        .text_size(rems(ui::RESULT_HEADER_TYPE_FONT_SIZE))
                        .text_color(muted)
                        .child(SharedString::from(label)),
                )
            });
        // 转置视图只画列名（类型是混合的，注释表头没有意义）；行号槽 / 空列名同理
        if self.transposed || Self::is_row_number(col_ix) || name.is_empty() {
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
            rows: self.view_len(),
            frozen: self.is_frozen(column),
            hidden_columns: self.hidden.len(),
            insight: self.insight_available(),
            table: self.result_table.clone(),
            column_filter_active: self.has_column_filter(column),
            column_values: self.column_distinct_values(column),
            sort_active: self
                .sort_state()
                .is_some_and(|(sort_col, _)| sort_col == column),
        };
        let menu_items = column_menu_items(&column_menu_target);
        let copy_column_text = self.column_text(column);
        let stats_text = stats_lines(&column_stats(&self.rows, column)).join("\n");
        let column_name_for_menu = name.clone();
        let on_sort_down = self.on_sort_down.clone();
        let on_column_stats = self.on_column_stats.clone();
        let on_insight_column = self.on_insight_column.clone();
        let on_column_filter = self.on_column_filter.clone();
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
        let annotation = self.column_annotations.get(col_ix - 1).cloned().flatten();
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

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // 【T2a-4】正在编辑的这一格：画编辑框，不画普通文本（渲染位仍是原生 render_td）
        if self.is_editing(row_ix, col_ix) {
            return self.render_edit_cell(row_ix, col_ix, window, cx).into_any_element();
        }
        let text = self.cell(row_ix, col_ix);
        // 【P2-4】转置视图每行对应一个不同的原列，类型按「行」取（字段名列是纯文本）；
        // 普通视图按列取。数值右对齐 / BLOB 摘要 / 等宽因此都随原列走
        let kind = if self.transposed {
            match col_ix {
                1 => ColumnKind::Unknown,
                _ => self
                    .transpose_columns()
                    .get(row_ix)
                    .and_then(|d| self.column_types.get(*d))
                    .and_then(Option::as_ref)
                    .map_or(ColumnKind::Unknown, |column| column.kind),
            }
        } else {
            self.column_kind(col_ix)
        };
        let style = cell_style(kind);
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let foreground = theme.colors.foreground;
        // 字体族从主题取（不引新依赖）：只在真要等宽的那几类上取一次
        let mono_family = style.monospace.then(|| theme.mono_font_family.clone());

        // 行号槽永远是灰的（它不是数据）
        if Self::is_row_number(col_ix) {
            return div()
                .px_2()
                .h_full()
                .truncate()
                .text_xs()
                .text_color(muted)
                // 用例按它量行高（`.id()` 不登记坐标）：这是「表格密度」那条口径的回归哨兵
                .debug_selector(move || format!("editor-result-rowno-{row_ix}"))
                .child(SharedString::from(text))
                .into_any_element();
        }

        // `NULL` 是 SQL 里的一个真实值：灰 + 斜体，与字符串 "NULL" 区分开（原型 §2.4）
        let is_null = text == "NULL";
        // 【P2】二进制列：格子里不铺一长串 hex，给「BLOB · 大小」摘要；
        // 悬停 / 查看完整值仍然给完整 hex（那是看全它的路）
        let is_blob = !is_null && kind == ColumnKind::Binary;
        let display = if is_blob {
            blob_summary(&text).unwrap_or_else(|| text.clone())
        } else {
            // 展示派生顺序：JSON 美化 → 数值列整数分组 → 原文；最后统一字符级裁剪。
            // `text` 原文仍给悬停 tooltip / 右键菜单 / 复制 / 导出 / 编辑（下方 full = text.clone()）。
            let shown = match pretty_json_display(&text) {
                Some(pretty) => pretty,
                // 【P2】数值列整数值加千分位；解析不成整数（小数 / 异常样本）回退原文
                None if kind.is_numeric() => {
                    format_integer_cell(&text).unwrap_or_else(|| text.clone())
                }
                None => text.clone(),
            };
            clip_cell_display(&shown, CELL_DISPLAY_LIMIT)
        };
        // 【B14】右键菜单要用“哪个单元格”：组件库只公开了 `right_clicked_row`，
        // 没有单元格访问器，所以在单元格自己身上记一笔（事件从最内层派发，不会丢）
        let table = cx.entity().clone();
        let mut cell = div()
            // 悬停提示要求元素有 id：gpui 的**流式** `tooltip` 声明在
            // `StatefulInteractiveElement` 上（没 id 的裸 `div` 只有直调 `Interactivity` 的写法），
            // 组件库自己的表头 / 行头单元格也是这么挂的
            .id((
                "editor-result-cell",
                row_ix * (self.columns.len() + 1) + col_ix,
            ))
            .px_2()
            .truncate()
            .text_xs()
            .text_color(if is_null { muted } else { foreground })
            // 【T2a-3】脏格（未提交变更）：浅警告底色，一眼看出哪些格子会被写回
            .when(self.is_dirty(row_ix, col_ix), |cell| {
                cell.bg(theme.colors.warning.opacity(0.16))
            })
            // 【T2a-6】冲突格（提交没写进去）：danger 底色，与脏格区分——优先级高于脏格
            // （冲突格先被标过 dirty 的可能性存在：同一格再编辑后提交，两段底色会叠）
            .when(self.is_conflict(row_ix, col_ix), |cell| {
                cell.bg(theme.colors.danger.opacity(0.16))
            })
            // 【类型感知】数字右对齐；其余左对齐
            .text_align(style.align)
            // 【类型感知】布尔 / 时间族用主题的等宽字体（两列放一起时字形对得齐）
            .when_some(mono_family, |cell, family| cell.font_family(family))
            .when(is_null, |cell| cell.italic())
            // 用例按它点/量真单元格（`Ctrl+C` 复制选中、行距口径都从这里进去）
            .debug_selector(move || format!("editor-result-cell-{row_ix}-{col_ix}"))
            .on_mouse_down(MouseButton::Right, move |_, _window, app| {
                table.update(app, |state, _cx| {
                    state.delegate_mut().set_context_cell(Some((row_ix, col_ix)));
                });
            });
        // 取值可能很宽（JSON / 长文本），`truncate` 之后就只剩悬停这一条看全的路
        if !text.is_empty() {
            let full = text.clone();
            cell = cell.tooltip(move |window, cx| result_cell_tooltip(full.clone(), window, cx));
        }
        cell.child(SharedString::from(display)).into_any_element()
    }

    /// 【B14】右键菜单：按值筛选 / 复制（此值 / 此列 / 整行）/ 冻结此列（原型 §5.5 的右键入口）
    ///
    /// 目标单元格是**最近右键的那一个**（[`Self::context_cell`]）：没有就什么都不摆
    /// （不猜一个“大概是想筛这个”）。菜单项由 [`context_menu_items`] 给（纯函数，可断言）。
    fn context_menu(
        &mut self,
        _row_ix: usize,
        menu: PopupMenu,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        // 【P2-4】转置是查看视图：右键菜单（筛选 / 排序 / 冻结 / 编辑）对它是错的
        if self.transposed {
            return menu;
        }
        let Some((row, col)) = self.context_cell else {
            return menu;
        };
        if Self::is_row_number(col) || row >= self.view_len() {
            return menu;
        }
        let column = self.data_column(col - 1);
        let column_name = self
            .columns
            .get(column)
            .cloned()
            .unwrap_or_else(|| format!("列 {}", column + 1));

        let mut menu = menu;
        let insight_available = self.on_insight_column.is_some() && self.insight_available;
        let target = ContextTarget {
            value: self.cell(row, col),
            // clone 进 target：外部的 column_name 留着给「查看完整值」分支用
            column_name: column_name.clone(),
            column,
            row,
            rows: self.view_len(),
            frozen: self.is_frozen(column),
            hidden_columns: self.hidden.len(),
            insight: insight_available,
            // 【A9】表名给菜单（猜不到 = None → 不摆 INSERT 项）
            table: self.result_table.clone(),
            // 右键菜单不摆列级筛选段（右键的「按值筛选」是全局词）：两字段留空
            column_filter_active: false,
            column_values: Vec::new(),
            sort_active: self
                .sort_state()
                .is_some_and(|(sort_col, _)| sort_col == column),
        };
        for item in context_menu_items(&target) {
            if item.separator_before {
                menu = menu.separator();
            }
            match item.action {
                ContextAction::PreviewValue(value) => {
                    // 【遗留 #7】没接钩子就不摆这一项（能力没有就不给入口）
                    if let Some(hook) = self.on_preview_value.clone() {
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
                    if let Some(hook) = self.on_filter_value.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| hook(&needle, app),
                        ));
                    }
                }
                ContextAction::SortDown { column, descending } => {
                    // 与「按值筛选」同口径：没接钩子就不摆这一项
                    if let Some(hook) = self.on_sort_down.clone() {
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
                    let text = self.row_json_text(row);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    ));
                }
                ContextAction::CopyRowInsert(row) => {
                    // 【A9】整行 INSERT（表名已由面板同步；拼好的语句带上）
                    let text = self.row_insert_text(row);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    ));
                }
                ContextAction::CopyColumn(column) => {
                    // 文案先拼好再交给点击回调：菜单项活到的比这一次 `context_menu` 调用长，
                    // 而“当时那一列是什么”只有现在知道。取值走**视图行序**（看着的才是拷走的）
                    let text = self.column_text(column);
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        },
                    ));
                }
                ContextAction::CopyRow(row) => {
                    // 同上：拼好的 TSV 带上（整行 = 各列制表符分隔，粘进表格直接分列）
                    let text = self.row_text(row);
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
                    if let Some(hook) = self.on_insight_column.clone() {
                        menu = menu.item(PopupMenuItem::new(item.label).on_click(
                            move |_, _window, app| hook(&column, app),
                        ));
                    }
                }
                ContextAction::ColumnStats { column } => {
                    // 统计是**网格自己算**（纯遍历），弹层由宿主开（复用值预览的对话框通道）
                    if let Some(hook) = self.on_column_stats.clone() {
                        let name = self
                            .columns
                            .get(column)
                            .cloned()
                            .unwrap_or_default();
                        let text = stats_lines(&column_stats(&self.rows, column)).join("\n");
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
                // 列级筛选只属于列头菜单（右键的「按值筛选」是全局词）：右键收到即忽略
                ContextAction::FilterColumnValue { .. } | ContextAction::ClearColumnFilter { .. } => {}
            }
        }
        menu
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

/// 结果工具栏（⑥，原型 §2.4）：左段 `行数 N` │ `耗时 1.2s` │ `连接名`
///
/// 全是可选真值：没有的东西不显示，也不用占位文案撑场面。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultToolbar {
    /// 行数（`None` = 这次没有网格：写语句 / 失败）
    pub rows: Option<usize>,
    /// 写语句的影响行数（`Some` = 这次只有影响行数，没有结果集）
    pub affected_rows: Option<u32>,
    /// 这次执行失败了吗（原因在错误卡片里，这里只标一个「失败」）
    pub failed: bool,
    /// 耗时（毫秒；`None` = 还没有结论）
    pub elapsed_ms: Option<u64>,
    /// 来源连接文案（`●P·orders`；`None` = 当时未绑定 / 认不出）
    pub connection: Option<String>,
    /// 【B15】来源摘要（血缘）：`原查询` / `下发筛选` / `排序下发` / `取下一段` / 标题（执行计划 / 分析）
    pub lineage: Option<String>,
    /// 【B15】本地筛选生效时的（视图行数，已抓总行数）——原型 §5.5 要求“统计跟随并明示已筛选”
    pub filtered: Option<(usize, usize)>,
}

impl ResultToolbar {
    /// 左段文案（**纯函数**：哪几段出现、数字怎么写都在这儿定，可逐条断言）
    pub fn segments(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some(affected) = self.affected_rows {
            parts.push(format!("影响 {} 行", thousands(affected as usize)));
        } else if let Some(rows) = self.rows {
            parts.push(format!("行数 {}", thousands(rows)));
        }
        if let Some((visible, total)) = self.filtered {
            parts.push(format!(
                "已筛选 {} / {} 行",
                thousands(visible),
                thousands(total)
            ));
        }
        if self.failed {
            parts.push("失败".to_string());
        }
        if let Some(elapsed_ms) = self.elapsed_ms {
            parts.push(format!("耗时 {}", duration_text(elapsed_ms)));
        }
        // 【B15】血缘（原型 §2.4：结果集要带来源摘要）——排在连接之前：
        // “这份是怎么来的”比“从哪条连接来”更常被问
        if let Some(lineage) = &self.lineage {
            parts.push(lineage.clone());
        }
        if let Some(connection) = &self.connection {
            parts.push(connection.clone());
        }
        parts
    }
}

/// 结果状态行（⑦）的输入：那一行是分页 / 总行数 / 已选 / 截断提示的家
///
/// 分页与「取下一段」属 B5b：接上之后它们也放这一行（原型 §2.4 的 `‹ 1 2 3 … ›`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultStatus {
    /// 已抓到的行数
    pub total_rows: usize,
    /// 当前选中的行（1 基；`None` = 没选中）
    pub selected_row: Option<usize>,
    /// 当前选中的格的值预览（DBeaver 对齐：状态行左下显示当前格；`None` = 没选中格）
    pub selected_cell: Option<String>,
    /// 截断提示（`Some` = 数据不完整）
    pub truncated_hint: Option<String>,
    /// 【B5b】还能取下一段吗（`true` 时总行数显示成 `1,000+`，并摆「取下一段」）
    pub has_more: bool,
}

/// 状态行左右两段（纯函数；截断提示单独渲染——它是警告，要另一种颜色）
pub fn status_segments(status: &ResultStatus) -> (String, String) {
    // 【B5b】还有下一段时总数不是“共”而是“已抓”：`1,000+` 说的就是“至少这么多”
    let left = if status.has_more {
        format!("共 {}+ 行", thousands(status.total_rows))
    } else {
        format!("共 {} 行", thousands(status.total_rows))
    };
    let right = match status.selected_row {
        Some(row) => format!("已选第 {} 行", thousands(row)),
        None => "未选中行".to_string(),
    };
    (left, right)
}

/// 单次查询的行数上限（对齐 engine `services/sql_service.rs` 的 `MAX_QUERY_ROWS = 10_000`；
/// 本 crate 不跨模块搬 engine 常量，仅状态行文案引用同一口径）。
pub const MAX_SINGLE_SEGMENT_ROWS: usize = 10_000;

/// 窗口分段加载的代价说明（落实 engine `sql_service.rs:78-79`「这条取舍要写在界面上，
/// 不能假装它和游标等价」）：每段重跑一次查询，`ORDER BY` 不稳时两段之间可能重复 /
/// 跳过行。只挂在「还有更多」/「截断」提示的 tooltip 上，不常驻占状态行宽度。
pub const WINDOW_FETCH_NOTICE: &str =
    "分段加载：每段重跑查询；无稳定 ORDER BY 时行可能重复或跳过";

/// 状态行「还有更多」提示：这段拿满、可能还有下一段（`has_more`）。
pub fn fetch_more_hint(status: &ResultStatus) -> Option<&'static str> {
    status.has_more.then_some("还有更多 · 滚动到底继续加载")
}

/// 撞单段上限且已到底（`truncated_hint` 有值、`!has_more`）：文案明确是「被截断、结果
/// 不完整」，区别于「正常取完」。分段中同时挂截断信号属防御组合，此处不重复出警告
/// （旧 `truncated_hint` 由渲染层兜底显示）。
pub fn truncation_notice(status: &ResultStatus) -> Option<String> {
    if status.truncated_hint.is_some() && !status.has_more {
        Some(format!(
            "已达单段上限 {} 行 · 结果不完整",
            thousands(MAX_SINGLE_SEGMENT_ROWS)
        ))
    } else {
        None
    }
}

/// 是否需要挂窗口代价 tooltip：分段中或被截断时给。
pub fn window_fetch_active(status: &ResultStatus) -> bool {
    status.has_more || status.truncated_hint.is_some()
}

/// 底部动作状态行的**占位动作钮**（#26 视图骨架：本轮全部禁用灰态）。
///
/// 顺序对齐 DBeaver 底部动作条，三段：
/// - 写操作段（0..7）：保存 · 取消 · 新增行 · 复制行 · 删除行 · 撤销 · 重做；
/// - 翻页段（7..10）：首页 · 上页 · 末页——「下页」是现有取下一段真钮，由
///   `ResultControls.more` 注入，不进占位清单；
/// - 系统段（10..）：导出 · 设置。
///
/// 行级叠加层模型（新增 / 复制 / 删除 + 行级撤销重做）是下一轮任务，见
/// `result-panel.md` §8.10 ④；本轮只摆位、不接线，按钮一律 `disabled(true)`。
pub fn status_action_placeholders() -> Vec<(IconName, &'static str)> {
    vec![
        (IconName::Save, "保存（待行级模型落地）"),
        (IconName::X, "取消（回滚待提交变更）"),
        (IconName::Plus, "新增行（暂未支持下发）"),
        (IconName::CopyPlus, "复制行（暂未支持下发）"),
        (IconName::Trash, "删除行（暂未支持下发）"),
        (IconName::Undo2, "撤销"),
        (IconName::Redo2, "重做"),
        (IconName::SkipBack, "首页（窗口化取数暂不支持）"),
        (IconName::ChevronLeft, "上页（窗口化取数暂不支持）"),
        (IconName::SkipForward, "末页（窗口化取数暂不支持）"),
        (IconName::Download, "导出"),
        (IconName::Settings, "设置"),
    ]
}

/// 占位动作钮分组下标的分界（写操作段 `0..FIRST_PAGER_ACTION`；翻页段 `FIRST_PAGER_ACTION..FIRST_SYSTEM_ACTION`；
/// 系统段 `FIRST_SYSTEM_ACTION..`）。render 层据此在段间插发丝分隔线。
const FIRST_PAGER_ACTION: usize = 7;
const FIRST_SYSTEM_ACTION: usize = 10;

/// 结果工具栏右段的动作控件（由面板构造：它知道点击该干什么）
///
/// 与状态栏的 `StatusControls` 同口径：文案交给可穷举的纯函数，控件交给面板。
#[derive(Default)]
pub struct ResultControls {
    /// 【B15】本地筛选框（原型 §5.5：只作用于视图层，不重查）
    pub filter: Option<AnyElement>,
    /// 【侧栏】右侧辅助栏开关（面板构造：翻转 `show_value_sidebar`；列浏览器 / 值详情 /
    /// 计算 / 分组共用这一栏、页签切换）
    pub value_sidebar: Option<AnyElement>,
    /// 【并排对比】对比模式开关（两份以上结果时才给；开启后结果区双栏并排）
    pub compare: Option<AnyElement>,
    /// 【T2a-7】批量提交/回滚对（有待提交变更才给；参考 DBeaver 的提交/回滚对）
    pub commit: Option<AnyElement>,
    /// 【B15】本地分析（下拉：计数 / 逐列分组计数；数据是桥接过来的行）
    pub analysis: Option<AnyElement>,
    /// 复制当前结果集（TSV）
    pub copy: Option<AnyElement>,
    /// 【B7】导出当前结果集（下拉：格式 × 仅已抓取 / 抓全量）
    pub export: Option<AnyElement>,
    /// 重跑当前结果集的 SQL（结果集换一份新的，不是新开一份）
    pub refresh: Option<AnyElement>,
    /// 【B5b】取下一段（只在这份结果还有下一段时给）
    pub more: Option<AnyElement>,
}

/// 结果区一次要画的东西（面板一次读齐；`render` 只负责画）
#[derive(Default)]
pub struct ResultPane {
    pub toolbar: ResultToolbar,
    /// 结果状态行（只有网格时才给；失败 / 写语句不给）
    pub status: Option<ResultStatus>,
    pub controls: ResultControls,
    /// 失败时的错误卡片（有卡片就不画网格——网格里本来也没东西）
    pub card: Option<AnyElement>,
    /// 结果集标签条（两份以上结果才给）
    pub tabs: Option<AnyElement>,
    /// 【B13】结果区顶部那一行提示（切通道后“旧结果来自 X”）；`None` = 不摆
    pub notice: Option<String>,
}

/// 截断提示文案（达到驱动行数上限时；原型 §2.4 的口径是 `已截断至 N 行`）
///
/// 只报**我们真拿到多少行**；上限是驱动侧的事（`engine` 侧 10000 行），编辑器不猜那个数。
pub fn truncated_hint(rows: usize) -> String {
    format!("已截断至 {} 行", thousands(rows))
}

/// 千分位（原型里的 `1,204`；用于行数 / 统计计数等非负整数）
pub fn thousands(value: usize) -> String {
    group_digits(&value.to_string())
}

/// 纯数字串按三位一组插逗号（分组内核；`thousands` 与单元格整数展示共用一份口径）
fn group_digits(digits: &str) -> String {
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            text.push(',');
        }
        text.push(ch);
    }
    text
}

/// 【P2】整数单元格展示千分位：能解析成 `i64` 的整数值加分组逗号，负数保留负号。
///
/// 只改**展示**：复制 / 悬停 / 导出 / 编辑仍取原始 `text`（见 `render_td`）。浮点 / 文本 /
/// `NULL` / 空串返回 `None`，交回普通展示路径；小数本轮不分组，避免破坏精度与科学计数法口径。
fn format_integer_cell(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let value: i64 = trimmed.parse().ok()?;
    let (sign, abs) = if value < 0 {
        ("-", value.unsigned_abs().to_string())
    } else {
        ("", value.to_string())
    };
    Some(format!("{sign}{}", group_digits(&abs)))
}

/// 耗时文案（原型 §2.4 的 `1.2s`）
///
/// 不到一秒直接报毫秒：`0.0s` 什么也没说（而且首屏结果常常就在这个量级）。
pub fn duration_text(elapsed_ms: u64) -> String {
    if elapsed_ms < 1000 {
        format!("{elapsed_ms} ms")
    } else {
        elapsed_text(std::time::Duration::from_millis(elapsed_ms))
    }
}

/// 建一个结果表状态（面板构造时一次；结果变化用 `refresh` 而不是重建）
pub fn new_table_state(
    delegate: ResultGridDelegate,
    window: &mut Window,
    cx: &mut App,
) -> Entity<TableState<ResultGridDelegate>> {
    cx.new(|cx| {
        TableState::new(delegate, window, cx)
            .col_resizable(true)
            .row_selectable(true)
            .cell_selectable(true)
            // 组件自带的窄行头去掉（`cell_selectable` 下默认会画一条）：我们的 `#` 列
            // 就是行把手，两条并列各能点一半是纯粹的干扰。关掉后仍能选整行——
            // **再点一下已选中的格子**升级为整行（组件自己的口径，`row_selectable` 已开）
            .row_header(false)
    })
}

/// 画结果区（⑤ 标签条 → ⑥ 工具栏 → 网格 / 错误卡片 → ⑦ 状态行）
///
/// **高度不在这里定**：结果区是 `ResizablePanel` 的一个面板（可拖拽），
/// 它的高度由分栏给出——这里只保证自己撑满那个面板。
pub fn render(
    state: &Entity<TableState<ResultGridDelegate>>,
    pane: ResultPane,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;

    // 先把各段拆开：工具栏先拿走 refresh/copy，状态行⑦再拿 more（部分移动会让字段不可用）
    let ResultPane {
        toolbar,
        status,
        controls,
        card,
        tabs,
        notice,
    } = pane;
    // 有卡片就不画网格（网格里本来也没东西，画出来只是一块空白）
    let has_card = card.is_some();

    div()
        .v_flex()
        .w_full()
        .h_full()
        .min_h_0()
        // 测试按选择器断言“结果区在不在、多高”：分栏是结构，不是装饰
        .debug_selector(|| "editor-result-pane".to_string())
        // ⑤ 结果集标签条（原型里在工具栏上面）
        .children(tabs)
        // 【B13】切通道后的那一行提示（原型 §5.7 规则 2）：旧结果不删，但要能看出它不是
        // 现在这档跑的——颜色（标签标灰）只是一个通道，白话在这一行
        .children(notice.map(|text| {
            let theme = cx.theme();
            let warning = theme.colors.warning;
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(warning)
                .bg(warning.opacity(0.18))
                .border_b(ui::HAIRLINE)
                .border_color(warning)
                // 测试按选择器断言“切了通道要说一句”
                .debug_selector(|| "editor-result-notice".to_string())
                .child(SharedString::from(text))
        }))
        // ⑥ 结果工具栏
        .child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                .text_xs()
                .text_color(muted)
                // 测试按选择器断言原型 §2.4 的竖向顺序（⑤ 标签条 → ⑥ 工具栏 → 网格 → ⑦ 状态行）
                .debug_selector(|| "editor-result-toolbar".to_string())
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .children(
                            toolbar
                                .segments()
                                .into_iter()
                                .map(|segment| SharedString::from(segment)),
                        ),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        // 顺序照原型 §2.4 的右段：⌕ 筛选 · 列浏览器 · ⧉ 对比 · 提交变更 · ⚗ 分析 ▾ · ⤓ 导出 ▾ · ⟳ · 复制
                        // （网格 / 文本 / 记录视图切换已迁到左缘竖条，不在顶部工具栏）
                        .children(controls.filter)
                        .children(controls.value_sidebar)
                        .children(controls.compare)
                        .children(controls.commit)
                        .children(controls.analysis)
                        .children(controls.export)
                        .children(controls.refresh)
                        .children(controls.copy),
                ),
        )
        // 有卡片：卡片替掉网格；否则画网格
        .children(card)
        .when(!has_card, |pane| {
            pane.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .debug_selector(|| "editor-result-grid".to_string())
                    // 【B14】`Ctrl+C` 的上下文只挂这一层（键位在 `crates/app` 注册）：
                    // 焦点在网格里时它才在 dispatch path 上，编辑区里按 `Ctrl+C`
                    // 仍是内核的文本复制。焦点本身不用我们管——组件的 `DataTable`
                    // 自己 `track_focus`，点一下就在它身上了（用例从真点击一路验到剪贴板）
                    .key_context(RESULT_GRID_CONTEXT)
                    // 密度档走常量表（`RESULT_TABLE_SIZE` = 组件 XSmall = 26px）——
                    // 不写 size 就落到组件默认档 32px（原型稿写的是 22px，三个数都不一致，见常量注释）
                    .child(
                        DataTable::new(state)
                            .stripe(true)
                            .bordered(false)
                            .with_size(ui::RESULT_TABLE_SIZE),
                    ),
            )
        })
        // ⑦ 结果状态行
        .when_some(status, |pane, status| {
            let (left, right) = status_segments(&status);
            let fetch_hint = fetch_more_hint(&status);
            let truncation = truncation_notice(&status);
            let window_tip = window_fetch_active(&status).then_some(WINDOW_FETCH_NOTICE);
            let window_tip_text = window_tip.map(str::to_string);
            // 防御组合（分段中同时挂截断信号）才留旧 hint；正常到底已由 truncation 接管
            let legacy_hint = if truncation.is_none() {
                status.truncated_hint.clone()
            } else {
                None
            };
            let warning = theme.colors.warning;
            let border = theme.colors.border;

            // 【#26 视图骨架】底部动作状态行三段：左动作组（占位灰钮 + 取下一段真钮）、
            // 中段只读行数框、右段当前格 / 截断 / 选中行元信息。占位钮全部 disabled，
            // 行级叠加层模型落地后逐个接线（result-panel.md §8.10 ④）。
            let placeholders = status_action_placeholders();
            let placeholder_btn = |(icon, tooltip): (IconName, &'static str)| {
                let id = format!("editor-result-action-{:?}", icon);
                Button::new(id)
                    .ghost()
                    .small()
                    .icon(icon)
                    .tooltip(tooltip)
                    .disabled(true)
                    .debug_selector(|| format!("editor-result-action-{:?}", icon))
                    .into_any_element()
            };
            let separator = || {
                div()
                    .w_px()
                    .h_3()
                    .flex_none()
                    .bg(border)
                    .into_any_element()
            };
            // 写操作段：保存 · 取消 · 新增行 · 复制行 · 删除行 · 撤销 · 重做
            let write_ops = placeholders[0..FIRST_PAGER_ACTION]
                .iter()
                .copied()
                .map(placeholder_btn)
                .collect::<Vec<_>>();
            // 翻页段占位：首页 · 上页（下页 = 现有取下一段真钮；末页占位）
            let pager_before = [
                placeholder_btn(placeholders[FIRST_PAGER_ACTION]),
                placeholder_btn(placeholders[FIRST_PAGER_ACTION + 1]),
            ];
            let last_pager = placeholder_btn(placeholders[FIRST_SYSTEM_ACTION - 1]);
            // 系统段：导出 · 设置
            let system_ops = placeholders[FIRST_SYSTEM_ACTION..]
                .iter()
                .copied()
                .map(placeholder_btn)
                .collect::<Vec<_>>();

            pane.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .h(rems(ui::RESULT_STATUS_BAR_HEIGHT))
                    .text_xs()
                    .text_color(muted)
                    .debug_selector(|| "editor-result-status-row".to_string())
                    // 左·动作组（写操作 ‖ 翻页 ‖ 系统，段间发丝分隔）
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .flex_none()
                            .children(write_ops)
                            .child(separator())
                            .children(pager_before)
                            // 【B5b】下页 = 取下一段真钮（只在还有下一段时由面板注入）
                            .children(controls.more)
                            .child(last_pager)
                            .child(separator())
                            .children(system_ops),
                    )
                    // 中段·只读行数框（行号跳转输入是 backlog；本轮只摆槽位与共 N[+] 行文案）
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_center()
                            .flex_none()
                            .w(rems(ui::RESULT_ROW_COUNT_BOX_WIDTH))
                            .border_b(ui::HAIRLINE)
                            .border_color(border)
                            .child(SharedString::from(left)),
                    )
                    // 右·元信息：当前格预览 · 截断警告 · 已选行
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .ml_auto()
                            // 当前格值预览（DBeaver 对齐：状态行显示当前格，长值已由
                            // 编辑器截断；`·` 分隔避免与选中行文案粘连）
                            .when_some(status.selected_cell, |row, value| {
                                row.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .child(SharedString::from(format!("· {}", value))),
                                )
                            })
                            // 【B5b】还有更多：次级说明；悬停看窗口分段代价（不等价于游标）。
                            // `.id(...)` 先把裸 div 转成 Stateful<Div>，tooltip 是交互态方法
                            .when_some(fetch_hint, |row, hint| {
                                row.child(
                                    div()
                                        .id("editor-result-fetch-hint")
                                        .min_w_0()
                                        .truncate()
                                        .when_some(window_tip_text.clone(), |cell, tip| {
                                            let tip = tip.clone();
                                            cell.tooltip(move |window, cx| {
                                                Tooltip::new(tip.clone()).build(window, cx)
                                            })
                                        })
                                        .child(SharedString::from(hint)),
                                )
                            })
                            // 撞单段上限、已到底：警告色（数据不完整）；悬停看窗口代价
                            .when_some(truncation, |row, hint| {
                                row.child(
                                    div()
                                        .id("editor-result-truncation")
                                        .min_w_0()
                                        .truncate()
                                        .text_color(warning)
                                        .when_some(window_tip_text.clone(), |cell, tip| {
                                            let tip = tip.clone();
                                            cell.tooltip(move |window, cx| {
                                                Tooltip::new(tip.clone()).build(window, cx)
                                            })
                                        })
                                        .child(SharedString::from(hint)),
                                )
                            })
                            // 防御组合：分段中同时挂截断信号时旧 hint 兜底显示（正常到底已走上面 notice）
                            .when_some(legacy_hint, |row, hint| {
                                row.child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(warning)
                                        .child(SharedString::from(hint)),
                                )
                            })
                            .child(SharedString::from(right)),
                    ),
            )
        })
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        CELL_DISPLAY_LIMIT, MAX_SINGLE_SEGMENT_ROWS, ContextAction, ContextTarget,
        ResultGridDelegate, ResultMatch, ResultStatus, ResultToolbar, TextAlign, WINDOW_FETCH_NOTICE,
        blob_summary, cell_matches, cell_style, clip_cell_display, column_distinct_values,
        column_stats, column_stats_from_values, compare_cells, context_menu_items, duration_text, fetch_more_hint,
        find_in_cell, fmt_stat_num, group_by_values, header_tooltip_lines, column_menu_items,
        column_browser_menu_items, format_integer_cell, pretty_json_display, preview_of, stats_lines, status_action_placeholders,
        status_segments, thousands, truncation_notice, truncated_hint, validate_edit_value,
        window_fetch_active, FIRST_PAGER_ACTION, FIRST_SYSTEM_ACTION,
    };
    use gpui_kit::assets::IconName;
    use workbench_shell::model::ColumnAnnotation;
    use crate::store::{ColumnKind, ColumnType};
    use gpui_kit::App;
    use gpui_kit::component::table::{ColumnSort, TableDelegate as _};

    fn grid() -> ResultGridDelegate {
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            vec![
                vec!["1".to_string(), "a".to_string()],
                vec!["2".to_string(), "NULL".to_string()],
            ],
        );
        grid
    }

    /// 造一份指定行的网格（B15 的筛选/排序测试用：列固定为 id / name）
    fn grid_with(rows: &[&[&str]]) -> ResultGridDelegate {
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            rows.iter()
                .map(|row| row.iter().map(|cell| cell.to_string()).collect())
                .collect(),
        );
        grid
    }

    /// 右键落点（用例只改关心的那几项）
    fn target(value: &str, name: &str, column: usize) -> ContextTarget {
        ContextTarget {
            value: value.to_string(),
            column_name: name.to_string(),
            column,
            row: 0,
            rows: 2,
            frozen: false,
            hidden_columns: 0,
            insight: false,
            table: None,
            column_filter_active: false,
            column_values: Vec::new(),
            sort_active: false,
        }
    }

    /// 带表名的右键落点（【A9】INSERT 项只在表名猜得到时出现）
    fn target_with_table(value: &str, name: &str, column: usize) -> ContextTarget {
        let mut target = target(value, name, column);
        target.table = Some("orders".to_string());
        target
    }

    /// 【P1】表头悬停分块：无注释 = 单行（与今天一致）；有注释 = 标题 + 元数据行
    #[test]
    fn header_tooltip_lines_build_title_and_meta_blocks() {
        // 无注释：单行「列名 · 类型标签」；无类型 = 只列名
        let (title, meta) = header_tooltip_lines("total", Some("numeric"), None);
        assert_eq!(title, "total · numeric");
        assert!(meta.is_empty());
        let (title, meta) = header_tooltip_lines("id", None, None);
        assert_eq!(title, "id");
        assert!(meta.is_empty());

        // 有注释：标题 + 来源 / 注释 / 类型 / 可空 / 默认（缺的字段不摆那一行）
        let ann = ColumnAnnotation {
            source_table: Some("public.orders".into()),
            source_column: Some("total".into()),
            comment: Some("总额".into()),
            data_type: Some("numeric".into()),
            nullable: Some(true),
            default_value: Some("0".into()),
        };
        let (title, meta) = header_tooltip_lines("total", Some("numeric"), Some(&ann));
        assert_eq!(title, "total · numeric");
        assert_eq!(
            meta,
            vec![
                "来源：public.orders.total",
                "注释：总额",
                "类型：numeric",
                "可空：是",
                "默认：0",
            ]
        );

        // 表达式列（来源两处 None）：标题在，元数据只剩注释 / 可空等
        let ann = ColumnAnnotation {
            source_table: None,
            source_column: None,
            comment: Some("合计".into()),
            data_type: None,
            nullable: None,
            default_value: None,
        };
        let (title, meta) = header_tooltip_lines("cnt", None, Some(&ann));
        assert_eq!(title, "cnt");
        assert_eq!(meta, vec!["注释：合计"]);
    }

    /// 【B14】右键菜单的项：按值筛选 / 复制（此值 · 此列 · 整行）/ 冻结（文案随冻结状态变）
    #[test]
    fn context_menu_offers_filter_copy_and_freeze() {
        let target = target("orders", "name", 1);
        let items = context_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "查看完整值…",
                "按值筛选「orders」",
                "复制此值",
                "复制此值为 JSON",
                "复制此列（2 个取值）",
                "复制整行（TSV）",
                "复制整行为 JSON",
                "按「name」升序（下发源库）",
                "按「name」降序（下发源库）",
                "冻结「name」",
                "隐藏「name」",
                "列统计「name」",
            ]
        );
        assert_eq!(
            items[0].action,
            ContextAction::PreviewValue("orders".to_string()),
            "【遗留 #7】查看完整值：值原样给面板（弹层是宿主的事）"
        );
        assert_eq!(
            items[1].action,
            ContextAction::FilterByValue("orders".to_string()),
            "值原样给面板（预览只管显示）"
        );
        assert_eq!(items[2].action, ContextAction::CopyValue("orders".to_string()));
        assert_eq!(
            items[3].action,
            ContextAction::CopyValueJson("orders".to_string()),
            "【A9】值 JSON 与「复制此值」平行（NULL → null，其余字符串）"
        );
        assert_eq!(
            items[4].action,
            ContextAction::CopyColumn(1),
            "复制整列认的是**数据列**下标（1 列、行号槽已折算掉）"
        );
        assert_eq!(
            items[5].action,
            ContextAction::CopyRow(0),
            "复制整行认的是**视图行**下标，不是列下标（两个都是数字，传反了菜单照样能摆出来）"
        );
        assert_eq!(
            items[6].action,
            ContextAction::CopyRowJson(0),
            "【A9】整行 JSON（单对象，与导出 JSON 档同口径）"
        );
        assert_eq!(
            items[7].action,
            ContextAction::SortDown {
                column: "name".to_string(),
                descending: false
            }
        );
        assert_eq!(
            items[8].action,
            ContextAction::SortDown {
                column: "name".to_string(),
                descending: true
            }
        );
        assert!(
            items[7].separator_before,
            "“对整份结果的操作”那组前面要有分隔线"
        );
        assert!(!items[9].separator_before, "冻结与排序同组");
        assert_eq!(items[9].action, ContextAction::ToggleFreeze(1));
        assert_eq!(
            items[10].action,
            ContextAction::HideColumn(1),
            "【P2】隐藏此列跟在冻结后（都是列级动作）"
        );
        assert_eq!(
            items[11].action,
            ContextAction::ColumnStats { column: 1 },
            "【P2-3】列统计与隐藏同组"
        );
        assert!(
            !items
                .iter()
                .any(|item| item.action == ContextAction::ShowAllColumns),
            "没有隐藏列时不摆「显示全部列」"
        );

        let mut done = target.clone();
        done.frozen = true;
        let items = context_menu_items(&done);
        assert_eq!(items[9].label, "取消冻结此列", "已冻结时给的是反向动作");

        // 【P2】隐藏了列才出现「显示全部列」
        let mut hidden = target.clone();
        hidden.hidden_columns = 2;
        let items = context_menu_items(&hidden);
        assert_eq!(
            items[12].action,
            ContextAction::ShowAllColumns,
            "隐藏非空时给恢复入口（排在「列统计」之后）"
        );
    }

    /// 【A9】「复制整行为 INSERT」只在表名猜得到时出现（面板从来源 SQL 猜好传来）：
    /// 猜不到宁可少一项，也不给一条插到错误表的语句。
    #[test]
    fn context_menu_offers_insert_only_when_table_is_known() {
        let without = context_menu_items(&target("orders", "name", 1));
        assert!(
            !without
                .iter()
                .any(|item| matches!(item.action, ContextAction::CopyRowInsert(_))),
            "表名未知时不摆 INSERT 项"
        );
        let with = context_menu_items(&target_with_table("orders", "name", 1));
        let insert = with
            .iter()
            .find(|item| matches!(item.action, ContextAction::CopyRowInsert(_)))
            .expect("表名已知时要摆 INSERT 项");
        assert_eq!(insert.label, "复制整行为 INSERT（orders）", "表名写进文案");
        assert!(!insert.separator_before, "与复制组同组（紧跟整行 JSON 后）");
    }

    /// 【M8】「洞察此列」只在**能洞察**时出现（宿主接了端口 + 这份结果可取样）：
    /// 不能洞察时连项都不摆（能力没有就不给入口，与「按值筛选」同口径）。
    #[test]
    fn context_menu_only_offers_insight_when_available() {
        let without = context_menu_items(&target("orders", "name", 1));
        assert!(
            !without
                .iter()
                .any(|item| matches!(item.action, ContextAction::InsightColumn { .. })),
            "不能洞察时不该摆这一项：{without:?}"
        );

        let mut with = target("orders", "name", 1);
        with.insight = true;
        let with = context_menu_items(&with);
        let last = with.last().expect("至少有一项");
        assert_eq!(last.label, "洞察「name」这一列");
        assert_eq!(
            last.action,
            ContextAction::InsightColumn {
                column: "name".to_string()
            }
        );
        assert!(last.separator_before, "它自成一段，前面要有分隔线");
    }

    /// 值预览：空白折叠 + 截断（长值不该把菜单撑开）
    #[test]
    fn the_value_preview_folds_and_truncates() {
        assert_eq!(preview_of("a\nb  c"), "a b c");
        let long = "x".repeat(40);
        let preview = preview_of(&long);
        assert_eq!(preview.chars().count(), 25, "24 个字符 + 省略号");
        assert!(preview.ends_with('…'), "{preview}");
        let items = context_menu_items(&target(&long, "c", 0));
        assert!(items[0].label.contains('…'), "{}", items[0].label);
    }

    /// 复制整列 / 整行：走**视图行序**，并按 TSV 规则转义（与 `ResultEntry::to_tsv`
    /// 同一份实现）：带制表符 / 换行 / 引号的值不许把粘出去的形状搞坏。
    #[test]
    fn copying_a_column_and_a_row_keeps_the_tsv_shape() {
        let mut grid = grid_with(&[&["1", "plain"], &["2", "two\tcells"], &["3", "say \"hi\""]]);

        // 整行：列之间是制表符，只有会破坏形状的格子才加引号
        assert_eq!(grid.row_text(0), "1\tplain");
        assert_eq!(grid.row_text(1), "2\t\"two\tcells\"");
        assert_eq!(grid.row_text(2), "3\t\"say \"\"hi\"\"\"");
        assert_eq!(
            grid.row_text(99),
            "",
            "行不存在给空串（不摆这一项的是界面）"
        );

        // 整列：一行一个取值（本身就是单列 TSV）
        assert_eq!(grid.column_text(0), "1\n2\n3");
        assert_eq!(
            grid.column_text(1),
            "plain\n\"two\tcells\"\n\"say \"\"hi\"\"\""
        );

        // 取值里的换行也被包起来：不然它会拆出一行，看上去像多了一行数据
        let with_break = grid_with(&[&["1", "line\nbreak"]]);
        assert_eq!(with_break.column_text(1), "\"line\nbreak\"");
        assert_eq!(with_break.row_text(0), "1\t\"line\nbreak\"");

        // 视图行序：本地排序 / 筛选之后，复制的是**看着的那些行**
        grid.apply_sort(Some((0, true)));
        assert_eq!(grid.column_text(0), "3\n2\n1");
        assert_eq!(grid.row_text(0), "3\t\"say \"\"hi\"\"\"");

        grid.set_filter("two");
        assert_eq!(grid.column_text(0), "2", "筛选生效时只复制看得见的行");
    }

    /// 【A9】整行 JSON / INSERT：与 TSV 同一份视图行序口径；JSON 走导出 JSON 的 NULL /
    /// 字符串规则，INSERT 走导出字面量规则；表名没同步时 INSERT 给空串（菜单不摆，防御）。
    #[test]
    fn row_json_and_insert_follow_the_view_order() {
        let mut grid = grid_with(&[&["1", "a"], &["2", "NULL"]]);
        // JSON：NULL → null；列名与值都是字符串
        assert_eq!(
            grid.row_json_text(0),
            "{\n  \"id\": \"1\",\n  \"name\": \"a\"\n}"
        );
        assert_eq!(
            grid.row_json_text(1),
            "{\n  \"id\": \"2\",\n  \"name\": null\n}"
        );
        // 没同步表名 → INSERT 空串（界面已不摆那一项，这里是防御）
        assert_eq!(grid.row_insert_text(0), "");
        // 同步表名后：值字面量与导出档同口径（NULL 不加引号）
        grid.set_result_table(Some("orders".to_string()));
        assert_eq!(
            grid.row_insert_text(1),
            "INSERT INTO orders (id, name) VALUES (2, NULL);"
        );
        // 视图行序：排序后复制的是看着的那行
        grid.apply_sort(Some((0, true)));
        assert_eq!(grid.row_json_text(0), "{\n  \"id\": \"2\",\n  \"name\": null\n}");
        // 换数据（set_data）→ 表名跟着清（防旧表名插到新结果上）
        grid.set_data(vec!["x".to_string()], vec![vec!["y".to_string()]]);
        assert_eq!(grid.row_insert_text(0), "");
    }

    /// 【B15】冻结列：切换、换数据时清空（列都换了一批，旧列号没意义）
    #[test]
    fn freezing_columns_toggles_and_resets_with_data() {
        let mut grid = grid();
        assert!(!grid.is_frozen(0));
        assert!(grid.toggle_freeze(0), "第一次是冻结");
        assert!(grid.is_frozen(0));
        assert!(grid.toggle_freeze(1));
        assert!(!grid.toggle_freeze(0), "再切一次是取消");
        assert!(!grid.is_frozen(0) && grid.is_frozen(1));

        grid.set_data(
            vec!["a".to_string()],
            vec![vec!["1".to_string()]],
        );
        assert!(!grid.is_frozen(1), "换了一份结果：冻结不跟过来");
    }

    /// 【B15】筛选只改视图行序：命中那些行还在原位，行号跟的是视图
    #[test]
    fn filtering_keeps_matching_rows_in_place() {
        let mut grid = grid_with(&[&["1", "orders"], &["2", "users"], &["3", "orders_archive"]]);
        grid.set_filter("orders");
        assert_eq!(grid.visible_row_count(), 2, "命中两行");
        assert_eq!(grid.data_row_count(), 3, "数据一行没动（清掉筛选就能原样回来）");
        assert_eq!(grid.cell(0, 1), "1", "首列显示的是视图行号");
        assert_eq!(grid.cell(0, 2), "orders");
        assert_eq!(grid.cell(1, 2), "orders_archive");
        assert_eq!(grid.cell(2, 2), "", "越界的视图行给空串（不 panic）");

        // 大小写不敏感（与 DBeaver 的本地筛选同类）
        grid.set_filter("ORDERS");
        assert_eq!(grid.visible_row_count(), 2);

        // 清除筛选：全部回来，顺序不变
        grid.set_filter("");
        assert_eq!(grid.visible_row_count(), 3);
        assert_eq!(grid.visible_rows()[0][1], "orders");
        assert_eq!(grid.visible_rows()[2][1], "orders_archive");
    }

    /// 【B15】排序：数字按数值比（`1, 2, 10` 不该排成 `1, 10, 2`），字符串按字典序
    #[test]
    fn sorting_is_numeric_when_the_values_look_numeric() {
        assert_eq!(
            compare_cells(Some(&"2".to_string()), Some(&"10".to_string())),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_cells(Some(&"b".to_string()), Some(&"a".to_string())),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_cells(None, Some(&"1".to_string())),
            std::cmp::Ordering::Greater,
            "缺列排在最后"
        );

        let mut grid = grid_with(&[&["10", "b"], &["2", "c"], &["1", "a"]]);
        grid.apply_sort(Some((0, false)));
        assert_eq!(
            grid.visible_rows()
                .iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>(),
            ["1", "2", "10"],
            "升序按数值"
        );
        grid.apply_sort(Some((1, true)));
        assert_eq!(
            grid.visible_rows()
                .iter()
                .map(|row| row[1].clone())
                .collect::<Vec<_>>(),
            ["c", "b", "a"],
            "降序按字符串"
        );
        grid.apply_sort(None);
        assert_eq!(
            grid.visible_rows()
                .iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>(),
            ["10", "2", "1"],
            "取消排序回原顺序（不是部分退还）"
        );
    }

    /// 【B15】筛选与排序叠在一起：先筛后排，两者都只动视图
    #[test]
    fn filter_and_sort_stack_on_the_same_view() {
        let mut grid = grid_with(&[&["10", "orders"], &["2", "orders"], &["7", "users"]]);
        grid.set_filter("orders");
        grid.apply_sort(Some((0, false)));
        assert_eq!(grid.visible_row_count(), 2);
        assert_eq!(grid.visible_rows()[0][0], "2");
        assert_eq!(grid.visible_rows()[1][0], "10");
    }

    /// 类型 → 格子样式（表驱动）：数字右对齐、布尔 / 时间族等宽、**未知档就是今天的样式**
    ///
    /// 这条钉的是“类型感知渲染”的最后一跳：模型说“是数字”还不够，渲染得真按右对齐画。
    #[test]
    fn the_cell_style_follows_the_column_type_and_unknown_stays_as_today() {
        let cases: &[(ColumnKind, TextAlign, bool)] = &[
            (ColumnKind::Integer, TextAlign::Right, false),
            (ColumnKind::Float, TextAlign::Right, false),
            (ColumnKind::Decimal, TextAlign::Right, false),
            (ColumnKind::Boolean, TextAlign::Left, true),
            (ColumnKind::Timestamp, TextAlign::Left, true),
            (ColumnKind::Date, TextAlign::Left, true),
            (ColumnKind::Time, TextAlign::Left, true),
            (ColumnKind::Uuid, TextAlign::Left, true),
            (ColumnKind::Text, TextAlign::Left, false),
            (ColumnKind::Json, TextAlign::Left, false),
            (ColumnKind::Binary, TextAlign::Left, false),
            // 没类型 / 认不出：左对齐 + 默认字体 —— 就是今天的样子
            (ColumnKind::Unknown, TextAlign::Left, false),
        ];
        for (kind, align, monospace) in cases {
            let style = cell_style(*kind);
            assert_eq!(style.align, *align, "{kind:?} 的对齐");
            assert_eq!(style.monospace, *monospace, "{kind:?} 的字体");
        }
    }

    /// 表头类型小标签：驱动报了才摆（行号槽永不摆），名字**原样**（方言名照显示）
    ///
    /// 同一条用例兼验“换了一份结果/清空之后类型不跟过来”：漏给就是“没类型”，
    /// 而**没类型这一档就是今天**（表头只有一个列名）。
    #[test]
    fn the_header_shows_the_type_only_when_the_driver_reported_one() {
        let mut grid = grid();
        assert!(grid.type_label(0).is_none(), "行号槽不是数据");
        assert!(grid.type_label(1).is_none(), "没带类型 = 不摆标签");
        assert!(grid.type_label(2).is_none());
        assert_eq!(grid.column_kind(1), ColumnKind::Unknown);

        grid.set_column_types(vec![
            Some(ColumnType::parse("bigint")),
            Some(ColumnType::parse("numeric(38,10)")),
        ]);
        assert_eq!(grid.type_label(1), Some("bigint"));
        assert_eq!(
            grid.type_label(2),
            Some("numeric(38,10)"),
            "表头显示的是驱动报的**原样名字**（带参数也照显）"
        );
        assert_eq!(grid.type_label(0), None, "行号槽没有类型可谈");
        assert_eq!(grid.type_label(9), None, "越界不 panic");
        assert_eq!(grid.column_kind(1), ColumnKind::Integer);
        assert_eq!(grid.column_kind(2), ColumnKind::Decimal);
        assert_eq!(
            grid.column_kind(0),
            ColumnKind::Unknown,
            "行号槽永远是无类型档"
        );

        // 换一份结果：上一份的类型不许跟过来（面板漏给 = 没类型，与今天一致）
        grid.set_data(vec!["a".to_string()], vec![vec!["1".to_string()]]);
        assert_eq!(grid.type_label(1), None);
        assert_eq!(grid.column_kind(1), ColumnKind::Unknown);

        // 一条列报了、另一条没报：没报的那条不摆标签
        grid.set_column_types(vec![Some(ColumnType::parse("uuid")), None]);
        assert_eq!(grid.type_label(1), Some("uuid"));
        assert_eq!(grid.type_label(2), None);

        grid.clear("执行失败：boom");
        assert_eq!(grid.column_kind(1), ColumnKind::Unknown);
        assert!(grid.type_label(1).is_none());
    }

    /// 没有筛选也没有排序时**不物化**视图行序（入库 / 切结果集时省掉的正是这份整表映射）；
    /// 而“不物化”必须与“物化一份恒等映射”给出**一模一样**的行——否则就是省错了
    #[test]
    fn the_view_order_is_not_materialized_until_a_filter_or_sort_needs_it() {
        let mut grid = grid_with(&[&["2", "orders"], &["1", "users"]]);
        assert!(
            !grid.view_order_materialized_for_test(),
            "刚入库：恒等，不建映射"
        );
        let plain = grid.visible_rows();
        assert_eq!(plain.len(), 2);

        // 筛一下：这时才建（本来就要逐行看一遍）
        grid.set_filter("users");
        assert!(grid.view_order_materialized_for_test());
        assert_eq!(grid.visible_row_count(), 1);
        assert_eq!(grid.cell(0, 2), "users");

        // 清掉筛选：又回到恒等（不是“留一份恰好是全集的映射”）
        grid.set_filter("");
        assert!(!grid.view_order_materialized_for_test());
        assert_eq!(grid.visible_rows(), plain, "与物化版给出的行必须一模一样");

        // 排序也是（用户真点了列头才建）
        grid.apply_sort(Some((0, false)));
        assert!(grid.view_order_materialized_for_test());
        assert_eq!(grid.visible_rows()[0][0], "1");
        grid.apply_sort(None);
        assert!(!grid.view_order_materialized_for_test());
        assert_eq!(
            grid.visible_rows(),
            plain,
            "取消排序回原顺序（不是部分退还）"
        );

        grid.clear("执行失败：boom");
        assert!(!grid.view_order_materialized_for_test());
    }

    /// 筛选匹配的口径（表驱动）：与 `to_lowercase().contains` 一致；ASCII 边上零分配
    ///
    /// `needle` 的契约是**已小写**（调用方 `to_lowercase` 一次，不给每格都分一次）；
    /// ASCII 快路径两边大小写都不敏感，非 ASCII 回落路径按小写词比。
    #[test]
    fn the_filter_matcher_keeps_the_lowercase_semantics() {
        let cases: &[(&str, &str, bool)] = &[
            ("orders", "orders", true),
            ("ORDERS", "orders", true),
            ("Orders_Archive", "orders", true),
            ("users", "orders", false),
            ("", "orders", false),
            ("ord", "orders", false),
            ("1", "orders", false),
            // 非 ASCII：回落 `to_lowercase`（大小写仍然不敏感，中文不受影响）
            ("Ärger", "ärger", true),
            ("ärger", "ärger", true),
            ("订单_2026", "订单", true),
            ("订单_2026", "2026", true),
            ("订单", "orders", false),
        ];
        for (cell, needle, expected) in cases {
            assert_eq!(
                cell_matches(cell, needle),
                *expected,
                "格 {cell:?} 找 {needle:?}"
            );
        }

        // 格内区间：两边都是 ASCII 才给得出；含非 ASCII 时是 `None`（命中但无区间）
        assert_eq!(find_in_cell("Orders_Archive", "orders"), Some(Some(0..6)));
        assert_eq!(find_in_cell("订单_2026", "2026"), Some(None));
        assert_eq!(find_in_cell("plain", ""), None, "空词不命中任何格");
        assert_eq!(find_in_cell("plain", "plainly"), None, "词比格长");
    }

    /// 结果内查找：只看**数据列**（`#` 槽不算）、按视图行序、找完一圈回到开头
    #[test]
    fn finding_in_the_result_walks_the_visible_rows_and_wraps() {
        let mut grid = grid_with(&[&["1", "orders"], &["2", "users"], &["3", "orders_archive"]]);
        assert_eq!(
            grid.find_next("orders", None),
            Some(ResultMatch {
                row: 0,
                col: 2,
                range: Some(0..6),
            }),
            "第一处命中的是**数据列**（行号槽不是数据）"
        );
        assert_eq!(
            grid.find_next("orders", Some((0, 2))).map(|hit| hit.row),
            Some(2),
            "从上一处继续：中间那行的第二列（users）不命中，继续往后找"
        );
        assert_eq!(
            grid.find_next("orders", Some((2, 2))).map(|hit| hit.row),
            Some(0),
            "找不到就绕回开头（表是圆的）"
        );

        // 行号槽里的数字不算命中（`#` 显示的是视图行号，不是数据）
        assert_eq!(
            grid.find_next("3", None),
            Some(ResultMatch {
                row: 2,
                col: 1,
                range: Some(0..1),
            })
        );
        assert_eq!(grid.find_next("nope", None), None);
        assert_eq!(grid.find_next("   ", None), None, "空词不找任何东西");

        // 含非 ASCII 的格：命中但不给格内区间（宿主退化成整格高亮）
        let chinese = grid_with(&[&["1", "订单_2026"]]);
        assert_eq!(
            chinese.find_next("2026", None),
            Some(ResultMatch {
                row: 0,
                col: 2,
                range: None,
            })
        );

        // 筛选之后只找**看得见的行**（找到看不见的行等于没找到）
        grid.set_filter("users");
        assert_eq!(grid.find_next("orders", None), None);
        assert_eq!(grid.find_next("users", None).map(|hit| hit.row), Some(0));
    }

    /// 【B15】换一份结果：排序跟不过来（列都可能不是同一批），筛选词留着（用户还在找同样的东西）
    #[test]
    fn switching_data_clears_sort_but_keeps_the_filter() {
        let mut grid = grid_with(&[&["2", "orders"], &["1", "users"]]);
        grid.set_filter("orders");
        grid.apply_sort(Some((0, true)));
        assert_eq!(grid.sort_state(), Some((0, true)));

        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            vec![vec!["5".to_string(), "orders".to_string()]],
        );
        assert_eq!(grid.sort_state(), None, "换结果不该继承旧的排序");
        assert_eq!(grid.filter(), "orders", "筛选词留着");
        assert_eq!(grid.visible_row_count(), 1);
    }

    #[test]
    fn columns_and_rows_follow_the_data() {
        let grid = grid();
        let columns = grid.columns_for_test();
        assert_eq!(columns, ["id".to_string(), "name".to_string()]);
        assert_eq!(grid.row_count_for_test(), 2);
    }

    #[test]
    fn empty_grid_says_why_instead_of_showing_a_blank_panel() {
        let grid = ResultGridDelegate::empty("尚未执行");
        assert_eq!(grid.row_count_for_test(), 0);
        assert!(grid.columns_for_test().is_empty());
    }

    #[test]
    fn clearing_drops_previous_rows() {
        let mut grid = grid();
        grid.clear("执行失败：boom");
        assert_eq!(grid.row_count_for_test(), 0);
        assert!(grid.columns_for_test().is_empty());
    }

    /// 【B15】列头可排序：数据列给 `Some(Default)`（可排但未排），行号槽不给
    ///
    /// 组件库对 `sort: None` 的列**不响应点击**（`perform_sort` 里直接 return），
    /// 所以这条钉的是“列头真的能点下去”。
    #[gpui_kit::test]
    fn data_columns_are_sortable_from_the_header(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(0, cx).sort, None, "行号槽不是数据列");
            assert_eq!(grid.column(1, cx).sort, Some(ColumnSort::Default));
            assert_eq!(grid.column(2, cx).sort, Some(ColumnSort::Default));
        });
        // 排上以后指示器要回得去（组件库 refresh 后会重新问 column）
        grid.apply_sort(Some((0, false)));
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(1, cx).sort, Some(ColumnSort::Ascending));
            assert_eq!(grid.column(2, cx).sort, Some(ColumnSort::Default));
        });
    }

    /// 【P1-5】拖过的列宽按列名记进 delegate，重建表头（`column()` 重取）时恢复；
    /// 行号槽宽度钉死不跟事件；换结果集后旧宽度清空回默认档。
    #[gpui_kit::test]
    fn dragged_column_widths_are_remembered_by_column_name(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut grid = grid();
        // 没拖过：数据列走默认档，行号槽钉死
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(1, cx).width, crate::ui::RESULT_COLUMN_WIDTH);
            assert_eq!(grid.column(2, cx).width, crate::ui::RESULT_COLUMN_WIDTH);
            assert_eq!(grid.column(0, cx).width, crate::ui::RESULT_ROW_NUMBER_WIDTH);
        });

        // 组件拖完 mouse-up 报全部列宽：下标 0 = 行号槽，1 = id，2 = name
        grid.set_column_widths(&[
            crate::ui::RESULT_ROW_NUMBER_WIDTH,
            gpui_kit::px(240.),
            gpui_kit::px(96.),
        ]);
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(1, cx).width, gpui_kit::px(240.), "id 列回填用户宽度");
            assert_eq!(grid.column(2, cx).width, gpui_kit::px(96.), "name 列回填用户宽度");
            assert_eq!(
                grid.column(0, cx).width,
                crate::ui::RESULT_ROW_NUMBER_WIDTH,
                "行号槽宽度钉死，不跟事件跑"
            );
        });

        // 换一份结果集：列集合变了，旧宽度清空，回到默认档
        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            vec![vec!["1".to_string(), "a".to_string()]],
        );
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(1, cx).width, crate::ui::RESULT_COLUMN_WIDTH);
            assert_eq!(grid.column(2, cx).width, crate::ui::RESULT_COLUMN_WIDTH);
        });
    }

    /// 【P1-5】列宽事件下标是视图列序：隐藏列后要经 `data_column` 换算，
    /// 宽度才落到对得上名的数据列上（否则隐藏一列后续列宽度整体错位）。
    #[gpui_kit::test]
    fn column_widths_map_around_hidden_columns(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut grid = grid();
        // 隐藏数据列 0（id）：视图只剩 name（数据列 1），视图列 1 = name
        grid.hide_column(0);
        grid.set_column_widths(&[crate::ui::RESULT_ROW_NUMBER_WIDTH, gpui_kit::px(200.)]);
        cx.update(|cx: &mut App| {
            assert_eq!(
                grid.column(1, cx).width,
                gpui_kit::px(200.),
                "隐藏 id 后，视图第一列 name 拿到对应的用户宽度"
            );
        });
    }

    /// 列数与行数要和真实数据一致（delegate 是网格唯一的取数口）
    ///
    /// **列数比数据列多 1**：首列是行号槽（原型 §2.4）。
    #[gpui_kit::test]
    fn delegate_reports_counts(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.columns_count(cx), 3, "行号槽 + 两列数据");
            assert_eq!(grid.rows_count(cx), 2);
        });
    }

    /// 首列是行号槽：1 基、与数据错开一位、不进导出文本的数据
    #[gpui_kit::test]
    fn the_first_column_is_a_row_number_gutter(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.cell_text(0, 0, cx), "1", "行号是 1 基");
            assert_eq!(grid.cell_text(1, 0, cx), "2");
            assert_eq!(grid.cell_text(0, 1, cx), "1", "数据从第 2 列开始");
            assert_eq!(grid.cell_text(1, 2, cx), "NULL");
        });
    }

    /// 空网格不画行号槽（没数据时那一列是空的）
    #[gpui_kit::test]
    fn an_empty_grid_has_no_gutter(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = ResultGridDelegate::empty("尚未执行");
        cx.update(|cx: &mut App| assert_eq!(grid.columns_count(cx), 0));
    }

    /// 截断提示按原型 §2.4 的口径（`已截断至 N 行`），数字带千分位
    #[test]
    fn truncation_hint_names_the_real_row_count() {
        assert_eq!(truncated_hint(10_000), "已截断至 10,000 行");
    }

    /// 工具栏左段：`行数 N` │ `耗时 1.2s` │ `来源摘要` │ `连接名`，没有的东西不占位
    #[test]
    fn toolbar_segments_follow_the_prototype_order() {
        let full = ResultToolbar {
            rows: Some(1_204),
            affected_rows: None,
            failed: false,
            elapsed_ms: Some(1_200),
            // 【B15】血缘排在连接之前（“这份是怎么来的”比“从哪条连接来”更常被问）
            lineage: Some("下发筛选".to_string()),
            connection: Some("●P·orders".to_string()),
            ..Default::default()
        };
        assert_eq!(
            full.segments(),
            ["行数 1,204", "耗时 1.2s", "下发筛选", "●P·orders"]
        );

        // 写语句：报影响行数，没有行数段
        let write = ResultToolbar {
            rows: None,
            affected_rows: Some(3),
            failed: false,
            elapsed_ms: Some(35),
            connection: None,
            ..Default::default()
        };
        assert_eq!(write.segments(), ["影响 3 行", "耗时 35 ms"]);

        // 失败：只说“失败”（原因在错误卡片里）
        let failed = ResultToolbar {
            failed: true,
            ..Default::default()
        };
        assert_eq!(failed.segments(), ["失败"]);

        // 什么都没执行过：一段都不摆
        assert!(ResultToolbar::default().segments().is_empty());
    }

    /// 状态行：`共 N 行` │ `已选第 M 行`，没选中就直说
    #[test]
    fn status_segments_report_total_and_selection() {        let idle = ResultStatus {
            total_rows: 1_204,
            selected_row: None,
            selected_cell: None,
            truncated_hint: None,
            has_more: false,
        };
        assert_eq!(status_segments(&idle), ("共 1,204 行".to_string(), "未选中行".to_string()));

        let picked = ResultStatus {
            selected_row: Some(12),
            ..idle.clone()
        };
        assert_eq!(status_segments(&picked).1, "已选第 12 行");
    }

    /// 【B5b】还有下一段时总数是 `1,000+`（“至少这么多”，不是“共”）
    #[test]
    fn a_partial_result_shows_a_plus_after_the_count() {
        let partial = ResultStatus {
            total_rows: 1_000,
            selected_row: None,
            selected_cell: None,
            truncated_hint: None,
            has_more: true,
        };
        assert_eq!(status_segments(&partial).0, "共 1,000+ 行");
    }

    /// 【P0-2a】状态行分段语义：分段中提示「还有更多」；撞单段上限且到底才出「不完整」警告；
    /// 正常取完两者都不提示；窗口代价 tooltip 只在分段中 / 截断时挂。
    #[test]
    fn status_fetch_and_truncation_hints_follow_state_combination() {
        // 正常取完：无任何额外提示
        let done = ResultStatus {
            total_rows: 120,
            selected_row: None,
            selected_cell: None,
            truncated_hint: None,
            has_more: false,
        };
        assert_eq!(fetch_more_hint(&done), None);
        assert_eq!(truncation_notice(&done), None);
        assert!(!window_fetch_active(&done));

        // 分段中：「还有更多」，不出截断警告
        let partial = ResultStatus { has_more: true, ..done.clone() };
        assert_eq!(fetch_more_hint(&partial), Some("还有更多 · 滚动到底继续加载"));
        assert_eq!(truncation_notice(&partial), None);
        assert!(window_fetch_active(&partial));

        // 撞上限、已到底：出「不完整」警告，文案点明 10,000 上限口径
        let capped = ResultStatus {
            truncated_hint: Some("已截断至 10,000 行".to_string()),
            has_more: false,
            ..done.clone()
        };
        assert_eq!(fetch_more_hint(&capped), None);
        let notice = truncation_notice(&capped).expect("到底且截断必出警告");
        assert!(notice.contains("10,000"), "文案须点明单段上限口径");
        assert!(notice.contains("不完整"));
        assert!(window_fetch_active(&capped));

        // 防御组合（分段中 + 截断信号）：截断警告让位给「还有更多」，不重复出警告
        let mixed = ResultStatus {
            truncated_hint: Some("已截断至 10,000 行".to_string()),
            has_more: true,
            ..done.clone()
        };
        assert!(truncation_notice(&mixed).is_none());
        assert!(window_fetch_active(&mixed));

        // 窗口代价文案与 engine 分段取舍一致；上限口径对齐 engine MAX_QUERY_ROWS
        assert!(WINDOW_FETCH_NOTICE.contains("ORDER BY"));
        assert_eq!(MAX_SINGLE_SEGMENT_ROWS, 10_000);
    }

    /// 【#26】底部动作状态行占位钮清单：顺序对齐 DBeaver 三段（写操作 ‖ 翻页 ‖ 系统），
    /// 「下页」是现有取下一段真钮，不进占位清单；本轮全部 disabled，接线归下一轮行级模型
    #[test]
    fn status_action_placeholders_follow_dbeaver_group_order() {
        let slots = status_action_placeholders();
        assert_eq!(slots.len(), 12, "占位钮共 12 个（下页真钮另由 controls.more 注入）");

        // 写操作段：保存 · 取消 · 新增行 · 复制行 · 删除行 · 撤销 · 重做
        let write: Vec<IconName> = slots[0..FIRST_PAGER_ACTION]
            .iter()
            .map(|(icon, _)| *icon)
            .collect();
        assert_eq!(
            write,
            vec![
                IconName::Save,
                IconName::X,
                IconName::Plus,
                IconName::CopyPlus,
                IconName::Trash,
                IconName::Undo2,
                IconName::Redo2,
            ]
        );

        // 翻页段：首页 · 上页 · 末页（下页 = 取下一段真钮，占位清单里没有 ChevronRight）
        let pager: Vec<IconName> = slots[FIRST_PAGER_ACTION..FIRST_SYSTEM_ACTION]
            .iter()
            .map(|(icon, _)| *icon)
            .collect();
        assert_eq!(pager, vec![IconName::SkipBack, IconName::ChevronLeft, IconName::SkipForward]);
        assert!(!slots.iter().any(|(icon, _)| *icon == IconName::ChevronRight));

        // 系统段：导出 · 设置
        let system: Vec<IconName> = slots[FIRST_SYSTEM_ACTION..]
            .iter()
            .map(|(icon, _)| *icon)
            .collect();
        assert_eq!(system, vec![IconName::Download, IconName::Settings]);

        // 每个占位钮都带可读 tooltip（说明为何禁用 / 待哪一轮接线）
        assert!(slots.iter().all(|(_, tooltip)| !tooltip.is_empty()));
    }

    /// 列头菜单（DBeaver 箭头）：列级子集——排序升/降、复制此列、冻结、隐藏、
    /// 统计；值与行动作（预览 / 按值筛选 / 复制整行）不属于列头；没隐藏列就不摆
    /// 「显示全部列」
    #[test]
    fn column_menu_is_the_column_level_subset() {
        let target = ContextTarget {
            value: "x".to_string(),
            column_name: "id".to_string(),
            column: 1,
            row: 3,
            rows: 12,
            frozen: false,
            hidden_columns: 0,
            insight: false,
            table: None,
            column_filter_active: false,
            column_values: Vec::new(),
            sort_active: false,
        };
        let items = column_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"按「id」升序（下发源库）"));
        assert!(labels.contains(&"按「id」降序（下发源库）"));
        assert!(labels.contains(&"复制此列（12 个取值）"));
        assert!(labels.contains(&"冻结「id」"));
        assert!(labels.contains(&"隐藏「id」"));
        assert!(labels.contains(&"列统计「id」"));
        // 值与行动作不属于列头
        assert!(!labels.iter().any(|l| l.contains("查看完整值")));
        assert!(!labels.iter().any(|l| l.contains("按值筛选")));
        assert!(!labels.iter().any(|l| l.contains("复制整行")));
        // 没隐藏列就不摆「显示全部列」
        assert!(!labels.contains(&"显示全部列"));
    }

    /// 列浏览器清单菜单 = 列头菜单的本地动作子集：复制 / 冻结 / 隐藏 / 列统计；
    /// 排序下发 / 值筛选 / 洞察 / 查看完整值 / 复制整行不进清单入口（前者依赖宿主
    /// 钩子，后者属于格 / 行交互，见拍板口径）。
    #[test]
    fn column_browser_menu_is_local_column_subset() {
        let target = target("x", "id", 1);
        let items = column_browser_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"复制此列（2 个取值）"));
        assert!(labels.contains(&"冻结「id」"));
        assert!(labels.contains(&"隐藏「id」"));
        assert!(labels.contains(&"列统计「id」"));
        // 不摆排序下发 / 值筛选 / 洞察 / 值与行动作
        assert!(!labels.iter().any(|l| l.contains("升序") || l.contains("降序")));
        assert!(!labels.iter().any(|l| l.contains("按值筛选")));
        assert!(!labels.iter().any(|l| l.contains("洞察")));
        assert!(!labels.iter().any(|l| l.contains("查看完整值")));
        assert!(!labels.iter().any(|l| l.contains("复制整行")));
        // 无隐藏列 / 未排序 → 不摆条件项
        assert!(!labels.contains(&"显示全部列"));
        assert!(!labels.iter().any(|l| l.contains("取消排序")));
    }

    /// 清单菜单条件项：排序生效 → 取消排序；有隐藏列 → 显示全部列；冻结中 → 文案反向。
    #[test]
    fn column_browser_menu_shows_conditional_items() {
        let mut target = target("", "name", 2);
        target.frozen = true;
        target.hidden_columns = 2;
        target.sort_active = true;
        let items = column_browser_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"取消冻结此列"));
        assert!(labels.contains(&"显示全部列"));
        assert!(labels.contains(&"取消排序「name」"));
    }

    /// 清单菜单动作归属：本地动作直接映射到对应 `ContextAction`，列号不串位。
    #[test]
    fn column_browser_menu_actions_map_to_local_column_actions() {
        let target = target("", "name", 3);
        let items = column_browser_menu_items(&target);
        let by_label: std::collections::HashMap<&str, &ContextAction> =
            items.iter().map(|i| (i.label.as_str(), &i.action)).collect();
        assert!(matches!(
            by_label["复制此列（2 个取值）"],
            ContextAction::CopyColumn(3)
        ));
        assert!(matches!(
            by_label["冻结「name」"],
            ContextAction::ToggleFreeze(3)
        ));
        assert!(matches!(
            by_label["隐藏「name」"],
            ContextAction::HideColumn(3)
        ));
        assert!(matches!(
            by_label["列统计「name」"],
            ContextAction::ColumnStats { column: 3 }
        ));
    }

    /// 列头菜单的条件项：隐藏了列 → 摆「显示全部列」；可洞察 → 摆「洞察此列」；
    /// 冻结中 → 文案反向（取消冻结）
    #[test]
    fn column_menu_shows_conditional_items() {
        let target = ContextTarget {
            value: String::new(),
            column_name: "name".to_string(),
            column: 2,
            row: 0,
            rows: 5,
            frozen: true,
            hidden_columns: 2,
            insight: true,
            table: Some("t_user".to_string()),
            column_filter_active: false,
            column_values: Vec::new(),
            sort_active: false,
        };
        let items = column_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"显示全部列"));
        assert!(labels.contains(&"洞察「name」这一列"));
        assert!(labels.contains(&"取消冻结此列"));
        // 表名只喂给 INSERT（右键项）；列头菜单不含它
        assert!(!labels.iter().any(|l| l.contains("INSERT")));
    }

    /// 列头菜单筛选段：值列表（去重取值，按出现顺序）平铺成「按值筛选「值」」项，
    /// 第一项带分隔线；筛选生效时才摆「清除此列筛选」
    #[test]
    fn column_menu_lists_distinct_values_and_clear_only_when_active() {
        let target = ContextTarget {
            value: String::new(),
            column_name: "status".to_string(),
            column: 1,
            row: 0,
            rows: 9,
            frozen: false,
            hidden_columns: 0,
            insight: false,
            table: None,
            column_filter_active: false,
            column_values: vec!["paid".to_string(), "NULL".to_string(), "refunded".to_string()],
            sort_active: false,
        };
        let items = column_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"按值筛选「paid」"));
        assert!(labels.contains(&"按值筛选「NULL」"));
        assert!(labels.contains(&"按值筛选「refunded」"));
        // 值列表第一项带分隔线（与上方排序段隔开）
        let first_value = items
            .iter()
            .find(|i| i.label.contains("按值筛选"))
            .unwrap();
        assert!(first_value.separator_before);
        // 没筛不摆清除项
        assert!(!labels.contains(&"清除此列筛选"));

        // 筛选生效：摆「清除此列筛选」（分隔线隔开），值列表照旧
        let active = ContextTarget {
            column_filter_active: true,
            ..target.clone()
        };
        let items = column_menu_items(&active);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"清除此列筛选"));
        let clear = items.iter().find(|i| i.label == "清除此列筛选").unwrap();
        assert!(clear.separator_before);
        // 空值列表 = 不摆筛选段（没数据这一列自然没有可选值）
        let empty = ContextTarget {
            column_values: Vec::new(),
            ..target
        };
        assert!(!column_menu_items(&empty)
            .iter()
            .any(|i| i.label.contains("按值筛选")));
    }

    /// 【筛选】去重取值：按出现顺序去重、到 `limit` 就停；列越界 / 无行 = 空
    #[test]
    fn column_distinct_values_dedupes_in_order_and_limits() {
        let rows: Vec<Vec<String>> = vec![
            vec!["a".into(), "x".into()],
            vec!["b".into(), "x".into()],
            vec!["c".into(), "y".into()],
            vec!["d".into(), "z".into()],
            vec!["e".into(), "z".into()],
        ];
        // 第 1 列：x, y, z（x 去重）
        let got = column_distinct_values(&rows, 1, 8);
        assert_eq!(got, ["x", "y", "z"].map(String::from));
        // limit 生效：最多 2 个
        let got = column_distinct_values(&rows, 1, 2);
        assert_eq!(got, ["x", "y"].map(String::from));
        // 列越界 / 无行 = 空
        assert!(column_distinct_values(&rows, 9, 8).is_empty());
        assert!(column_distinct_values(&[], 0, 8).is_empty());
        // NULL 是字面量，原样计入（值列表里能筛 NULL）
        let rows = vec![vec!["NULL".to_string()], vec!["NULL".to_string()], vec!["ok".to_string()]];
        let got = column_distinct_values(&rows, 0, 8);
        assert_eq!(got, ["NULL", "ok"].map(String::from));
    }

    /// 【筛选】列级筛选只命中目标列（包含匹配），且与全局筛选词叠加取交集
    #[test]
    fn column_filter_scopes_to_column_and_stacks_with_global() {
        let mut grid = grid_with(&[
            &["1", "alice"],
            &["2", "bob"],
            &["3", "alice"],
            &["4", "carol"],
        ]);
        // 列筛选 = 第 1 列（name）含 "ali" → 行 0、2
        grid.set_column_filter(Some((1, "ali".to_string())));
        assert_eq!(
            grid.visible_rows(),
            vec![
                vec!["1".to_string(), "alice".to_string()],
                vec!["3".to_string(), "alice".to_string()],
            ]
        );
        assert!(grid.has_column_filter(1));
        assert!(!grid.has_column_filter(0));
        assert_eq!(grid.column_filter(), Some((1, "ali")));

        // 叠加全局筛选（id 列含 "3"）→ 只剩行 2
        grid.set_filter("3");
        assert_eq!(grid.visible_rows(), vec![vec!["3".to_string(), "alice".to_string()]]);

        // 清除列筛选 → 全局筛选单独生效
        grid.set_column_filter(None);
        assert_eq!(grid.visible_rows(), vec![vec!["3".to_string(), "alice".to_string()]]);
        assert!(!grid.has_column_filter(1));
        assert!(grid.column_filter().is_none());
    }

    /// 【类型感知】按列类型校验编辑值：数字列必须数字、布尔取值受限、UUID / 日期
    /// 查形状；空与 `NULL` 一律放行（清值是“删值”语义）；文本 / 未知列不校验
    #[test]
    fn edit_validation_is_type_aware_and_allows_clear() {
        use crate::store::ColumnKind as K;
        // 整数：合法 / 非法 / 空与 NULL 放行
        assert!(validate_edit_value(K::Integer, "42").is_none());
        assert!(validate_edit_value(K::Integer, "-7").is_none());
        assert!(validate_edit_value(K::Integer, "3.14").is_some());
        assert!(validate_edit_value(K::Integer, "abc").is_some());
        assert!(validate_edit_value(K::Integer, "").is_none());
        assert!(validate_edit_value(K::Integer, "NULL").is_none());
        // 浮点 / 定点
        assert!(validate_edit_value(K::Float, "3.14").is_none());
        assert!(validate_edit_value(K::Decimal, "1e3").is_none());
        assert!(validate_edit_value(K::Float, "x").is_some());
        // 布尔：true/false/1/0/t/f/yes/no（大小写不限）
        for ok in ["true", "false", "1", "0", "t", "f", "yes", "no", "TRUE"] {
            assert!(validate_edit_value(K::Boolean, ok).is_none(), "{ok}");
        }
        assert!(validate_edit_value(K::Boolean, "maybe").is_some());
        // UUID 形状（8-4-4-4-12）
        assert!(validate_edit_value(K::Uuid, "550e8400-e29b-41d4-a716-446655440000").is_none());
        assert!(validate_edit_value(K::Uuid, "550e8400e29b41d4a716446655440000").is_some());
        assert!(validate_edit_value(K::Uuid, "hello").is_some());
        // 日期（YYYY-MM-DD + 月/日范围）
        assert!(validate_edit_value(K::Date, "2026-10-24").is_none());
        assert!(validate_edit_value(K::Date, "2026-13-01").is_some());
        assert!(validate_edit_value(K::Date, "2026/10/24").is_some());
        assert!(validate_edit_value(K::Date, "2026-10-32").is_some());
        // 文本 / 未知不校验
        assert!(validate_edit_value(K::Text, "任意 内容!@#").is_none());
        assert!(validate_edit_value(K::Unknown, "任意").is_none());
    }

    /// 耗时：不到一秒报毫秒（`0.0s` 什么都没说），过一秒按人读的写法
    #[test]
    fn duration_text_is_readable() {
        assert_eq!(duration_text(0), "0 ms");
        assert_eq!(duration_text(999), "999 ms");
        assert_eq!(duration_text(1_200), "1.2s");
        assert_eq!(duration_text(102_000), "1m42s");
    }

    /// 千分位（原型里的 `1,204`）
    #[test]
    fn thousands_separates_digit_groups() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_204), "1,204");
        assert_eq!(thousands(12_345_678), "12,345,678");
    }

    /// 【P2-2】BLOB 摘要：正常 hex → `BLOB · N B`；异常形态退回 `None`（按原文显示）
    #[test]
    fn blob_summary_is_the_binary_badge() {
        assert_eq!(
            blob_summary("48656c6c6f"),
            Some("BLOB · 5 B".to_string()),
            "10 个 hex 位 = 5 字节"
        );
        assert_eq!(blob_summary(""), None, "空串不是二进制");
        assert_eq!(blob_summary("abc"), None, "奇数位不猜");
        assert_eq!(blob_summary("zzzzzzzz"), None, "非 hex 字符退回原文");
        assert_eq!(
            blob_summary("0x48656c6c6f"),
            Some("BLOB · 5 B".to_string()),
            "带 0x 前缀也认得（宽松处理）"
        );
    }

    /// 【P0-1】展示裁剪只裁画面串：按 `char` 边界不裁断中文 / emoji；完整值另走
    /// `cell()` / `row_cell_text`（复制 / 导出 / 悬停 / 筛选 / 统计），BLOB 优先摘要由
    /// `render_td` 的分支结构保证，不在这个纯函数里。
    #[test]
    fn clip_cell_display_respects_char_boundary_and_limit() {
        // 恰好上限：原样返回，不加省略号
        let exact = "a".repeat(CELL_DISPLAY_LIMIT);
        assert_eq!(clip_cell_display(&exact, CELL_DISPLAY_LIMIT), exact);

        // 超 1 个字符：前 300 字符 + …，省略号不占额度
        let over = "a".repeat(CELL_DISPLAY_LIMIT + 1);
        let clipped = clip_cell_display(&over, CELL_DISPLAY_LIMIT);
        assert_eq!(clipped.chars().count(), CELL_DISPLAY_LIMIT + 1);
        let body = clipped.strip_suffix('…').expect("超长必带省略号");
        assert_eq!(body, "a".repeat(CELL_DISPLAY_LIMIT));

        // 中文 + emoji：按 char 计数，边界落在字符上，超界 emoji 整字符裁掉、不出现半个
        let unicode = "中".repeat(CELL_DISPLAY_LIMIT) + "😀";
        let u = clip_cell_display(&unicode, CELL_DISPLAY_LIMIT);
        assert!(u.ends_with('…'));
        assert!(!u.contains('😀'));
        assert_eq!(u.chars().count(), CELL_DISPLAY_LIMIT + 1);

        // 多行值：中间换行保留，只截断尾部补省略号
        let multiline = "第一行\n第二行\n".repeat(40);
        let m = clip_cell_display(&multiline, CELL_DISPLAY_LIMIT);
        assert!(m.contains('\n'), "中间换行不应被破坏");
        assert!(m.ends_with('…'));

        // 短串 / NULL / 空串原样返回
        assert_eq!(clip_cell_display("NULL", CELL_DISPLAY_LIMIT), "NULL");
        assert_eq!(clip_cell_display("", CELL_DISPLAY_LIMIT), "");

        // 通用小 limit：5 字符取前 5 + …
        assert_eq!(clip_cell_display("abcdef", 5), "abcde…");
    }

    /// 【P1-4】格内 JSON 美化：对象 / 数组压缩串展开成缩进多行；非 JSON 与裸标量原样交回 None，
    /// 由上层按普通文本裁剪；解析失败不抛错。
    #[test]
    fn pretty_json_only_formats_parseable_objects_and_arrays() {
        // 对象：两空格缩进 + 换行展开
        let obj = r#"{"a":1,"b":"x"}"#;
        assert_eq!(
            pretty_json_display(obj).as_deref(),
            Some("{\n  \"a\": 1,\n  \"b\": \"x\"\n}"),
            "压缩对象展开成缩进多行"
        );
        // 数组同样美化
        let arr = r#"[1,2,3]"#;
        assert_eq!(
            pretty_json_display(arr).as_deref(),
            Some("[\n  1,\n  2,\n  3\n]"),
            "压缩数组展开成缩进多行"
        );
        // 前后空白包裹仍识别（trim 后解析）
        assert!(pretty_json_display("  {\"k\": null}  ").is_some());

        // `{` 开头但不是合法 JSON：返回 None，不猜成 JSON
        assert!(pretty_json_display("{oops").is_none());
        // 裸标量 / 普通文本 / NULL / 空串：不进美化路径
        assert!(pretty_json_display("123").is_none());
        assert!(pretty_json_display("true").is_none());
        assert!(pretty_json_display("\"plain\"").is_none());
        assert!(pretty_json_display("plain text").is_none());
        assert!(pretty_json_display("NULL").is_none());
        assert!(pretty_json_display("").is_none());
    }

    /// 【P2】整数单元格千分位：整数值三位一组加逗号、负数保留负号；小数 / 科学计数 /
    /// 文本 / NULL / 空串返回 None（交回普通展示路径，不强行分组）。
    #[test]
    fn integer_cell_values_get_thousands_separators_others_stay_raw() {
        assert_eq!(format_integer_cell("1234567").as_deref(), Some("1,234,567"));
        assert_eq!(format_integer_cell("1234").as_deref(), Some("1,234"));
        assert_eq!(format_integer_cell("42").as_deref(), Some("42"));
        assert_eq!(format_integer_cell("0").as_deref(), Some("0"));
        // 负数：负号保留，绝对值分组
        assert_eq!(format_integer_cell("-1234").as_deref(), Some("-1,234"));
        assert_eq!(format_integer_cell("-42").as_deref(), Some("-42"));
        // 前后空白先 trim 再解析
        assert_eq!(format_integer_cell("  1000  ").as_deref(), Some("1,000"));

        // 小数 / 科学计数 / 文本 / NULL / 空串：不分组
        assert!(format_integer_cell("3.14").is_none());
        assert!(format_integer_cell("1e3").is_none());
        assert!(format_integer_cell("abc").is_none());
        assert!(format_integer_cell("NULL").is_none());
        assert!(format_integer_cell("").is_none());
    }

    /// 【P2-1】隐藏列：视图列序跳过隐藏列，数据 / 类型 / 冻结仍按数据列走；
    /// 复制整行不再带隐藏列；换结果清空隐藏清单
    #[test]
    fn hidden_columns_are_skipped_in_view_but_kept_in_data() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec![
                "id".to_string(),
                "name".to_string(),
                "blob".to_string(),
            ],
            vec![
                vec!["1".to_string(), "alice".to_string(), "aabb".to_string()],
                vec!["2".to_string(), "bob".to_string(), "ccdd".to_string()],
            ],
        );

        // 隐藏第 1 数据列（name）：表格只剩 # + id + blob 三列
        grid.hide_column(1);
        assert_eq!(grid.visible_column_count(), 2, "3 数据列 − 1 隐藏");
        assert_eq!(grid.data_column(0), 0, "可见列 0 = 数据列 0");
        assert_eq!(grid.data_column(1), 2, "可见列 1 跳过隐藏列 → 数据列 2");
        assert_eq!(grid.view_column(0), Some(0));
        assert_eq!(grid.view_column(1), None, "隐藏列没有可见下标");
        assert_eq!(grid.cell(0, 2), "aabb", "取值走数据列（blob 的内容还在）");

        // 复制整行：隐藏列不参与（用户看不到的不该拷走）
        assert_eq!(grid.row_text(0), "1\taabb", "TSV 只含可见列");

        // 换一批数据：隐藏清单作废
        grid.set_data(
            vec!["a".to_string(), "b".to_string()],
            vec![vec!["1".to_string(), "2".to_string()]],
        );
        assert_eq!(grid.visible_column_count(), 2, "新结果回到全部列");
    }

    /// 【侧栏】单列取消隐藏：`show_column` 只放回这一列，其它隐藏列原样保留
    /// （与「显示全部」整批清空的粒度区别开——左栏的眼睛是单列粒度）
    #[test]
    fn show_column_restores_only_that_column() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
            ],
            vec![vec!["1".to_string(), "2".to_string(), "3".to_string()]],
        );
        grid.hide_column(0);
        grid.hide_column(1);
        assert!(grid.is_hidden_column(0) && grid.is_hidden_column(1));
        grid.show_column(0);
        assert!(!grid.is_hidden_column(0), "第 0 列恢复");
        assert!(grid.is_hidden_column(1), "第 1 列仍隐藏");
        assert_eq!(grid.visible_column_count(), 2);
    }

    /// 【呈现】文本视图：可见列头 + 可见行拼 TSV；隐藏列不参与；行序随筛选
    #[test]
    fn text_presentation_skips_hidden_columns_and_follows_view_rows() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec![
                "id".to_string(),
                "name".to_string(),
                "blob".to_string(),
            ],
            vec![
                vec!["1".to_string(), "alice".to_string(), "aabb".to_string()],
                vec!["2".to_string(), "bob".to_string(), "ccdd".to_string()],
            ],
        );
        let text = grid.text_presentation();
        assert_eq!(
            text,
            "id\tname\tblob\n1\talice\taabb\n2\tbob\tccdd",
            "全列 TSV（列头 + 行）"
        );
        grid.hide_column(2);
        assert_eq!(
            grid.text_presentation(),
            "id\tname\n1\talice\n2\tbob",
            "隐藏列不参与文本"
        );
        grid.set_filter("bob");
        assert_eq!(grid.text_presentation(), "id\tname\n2\tbob", "筛选只留命中的行");
    }

    /// 【分组】本地 groupby 计数：保留首次出现顺序，NULL 独立成组
    #[test]
    fn group_by_count_keeps_order_and_null_group() {
        let rows = vec![
            vec!["a".to_string(), "x".to_string()],
            vec!["b".to_string(), "NULL".to_string()],
            vec!["a".to_string(), "x".to_string()],
            vec!["c".to_string(), "y".to_string()],
        ];
        let groups = ResultGridDelegate::group_by_count(&rows, 0);
        assert_eq!(
            groups,
            vec![
                ("a".to_string(), 2),
                ("b".to_string(), 1),
                ("c".to_string(), 1),
            ],
            "首现顺序 + 计数"
        );
        let by_null = ResultGridDelegate::group_by_count(&rows, 1);
        assert_eq!(
            by_null,
            vec![("x".to_string(), 2), ("NULL".to_string(), 1), ("y".to_string(), 1)],
            "NULL 独立成组"
        );
        assert!(ResultGridDelegate::group_by_count(&rows, 9).is_empty(), "越界列空组");
    }

    /// 【P2-3】列统计：NULL 计空值，数值列给 min/max/avg/sum，文本列只给计数
    #[test]
    fn column_stats_count_nulls_and_numeric_bounds() {
        let rows = vec![
            vec!["10".to_string(), "alice".to_string(), "NULL".to_string()],
            vec!["20".to_string(), "bob".to_string(), "x".to_string()],
            vec!["NULL".to_string(), "carol".to_string(), "y".to_string()],
            vec!["30".to_string(), "dave".to_string(), "z".to_string()],
        ];
        let numeric = column_stats(&rows, 0);
        assert_eq!(numeric.count, 3, "10/20/30 三个非空");
        assert_eq!(numeric.nulls, 1, "NULL 单计");
        assert_eq!(numeric.min, Some(10.0));
        assert_eq!(numeric.max, Some(30.0));
        assert_eq!(numeric.avg, Some(20.0));
        assert_eq!(numeric.sum, Some(60.0));

        let text = column_stats(&rows, 1);
        assert_eq!(text.count, 4);
        assert_eq!(text.nulls, 0);
        assert_eq!(text.min, None, "文本列不编数值");

        let mixed = column_stats(&rows, 2);
        assert_eq!(mixed.count, 3, "x/y/z 三个非空");
        assert_eq!(mixed.nulls, 1, "NULL 计入空值");
        assert_eq!(mixed.min, None, "x/y/z 解析不了数字");
    }

    /// 【P2-2】列级纯函数与矩阵版同口径：抽列后统计 / 分组结果逐项相等。
    #[test]
    fn column_level_stats_and_grouping_match_matrix_versions() {
        let rows = vec![
            vec!["10".to_string(), "a".to_string()],
            vec!["20".to_string(), "NULL".to_string()],
            vec!["NULL".to_string(), "a".to_string()],
            vec!["30".to_string(), "b".to_string()],
        ];
        let values: Vec<String> = rows.iter().map(|row| row[0].clone()).collect();
        assert_eq!(column_stats_from_values(&values), column_stats(&rows, 0), "统计列级内核与矩阵版一致");

        let name_values: Vec<String> = rows.iter().map(|row| row[1].clone()).collect();
        assert_eq!(group_by_values(&name_values), ResultGridDelegate::group_by_count(&rows, 1), "分组列级内核与矩阵版一致");
    }

    /// 【P2-2】delegate 列级访问器跟随筛选 / 排序后的**视图行序**，结果与矩阵
    /// `visible_rows()` 现算一致——消除整行物化后口径不能变。
    #[test]
    fn delegate_column_accessors_follow_view_rows_like_matrix_version() {
        let mut grid = grid_with(&[&["10", "a"], &["20", "b"], &["NULL", "a"], &["30", "b"]]);

        // 全量视图：统计 / 分组与矩阵版逐项相等
        let rows = grid.visible_rows();
        assert_eq!(grid.column_stats_for(0), column_stats(&rows, 0));
        assert_eq!(grid.group_by_for(1), ResultGridDelegate::group_by_count(&rows, 1));
        assert_eq!(
            grid.group_by_for(1),
            vec![("a".to_string(), 2), ("b".to_string(), 2)],
            "首现顺序 + 计数"
        );

        // 筛选后只剩 name 含 a 的两行：访问器同样只统计视图行
        grid.set_filter("a");
        let filtered = grid.visible_rows();
        assert_eq!(grid.column_stats_for(0), column_stats(&filtered, 0), "统计跟随筛选行序");
        assert_eq!(grid.group_by_for(1), ResultGridDelegate::group_by_count(&filtered, 1), "分组跟随筛选行序");
        assert_eq!(grid.column_stats_for(0).count, 1, "10 那行非空");
        assert_eq!(grid.column_stats_for(0).nulls, 1, "NULL 那行被筛进来");
    }

    /// 【P2-3】展示行：数值给全，纯文本列只给计数两行
    #[test]
    fn stats_lines_are_key_value_pairs() {
        let stats = column_stats(
            &[vec!["1.5".to_string(), "NULL".to_string()]],
            0,
        );
        assert_eq!(
            stats_lines(&stats),
            ["值 1 个", "NULL 0 个", "最小 1.5", "最大 1.5", "均值 1.5", "合计 1.5"]
        );
        assert_eq!(fmt_stat_num(1.0), "1", "去尾零");
        assert_eq!(fmt_stat_num(1.500000), "1.5");
    }

    /// 【P2-1】隐藏到只剩 1 列就拒绝（全藏了表格没有内容）
    #[test]
    fn hiding_stops_before_the_last_column() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec!["a".to_string(), "b".to_string()],
            vec![vec!["1".to_string(), "2".to_string()]],
        );
        grid.hide_column(0);
        assert_eq!(grid.hidden.len(), 1);
        grid.hide_column(1);
        assert_eq!(grid.hidden.len(), 1, "只剩 1 列时再藏被拒绝");
        grid.show_all_columns();
        assert!(grid.hidden.is_empty(), "显示全部列清空隐藏清单");
    }

    /// 【P2-4】单行转置：选中行竖排成「# / 字段 / 值」，行导航夹取，
    /// 只读（不脏 / 不冲突 / 不进编辑），关闭后原网格原样回来
    #[test]
    fn transpose_shows_one_row_as_field_value_pairs() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec!["id".to_string(), "name".to_string(), "blob".to_string()],
            vec![
                vec!["1".to_string(), "alice".to_string(), "aabb".to_string()],
                vec!["2".to_string(), "bob".to_string(), "ccdd".to_string()],
            ],
        );
        // 竖排第 2 行（数据行序 1）
        grid.set_transposed(true, 1);
        assert!(grid.is_transposed());
        assert_eq!(grid.transpose_row(), 1);
        // 三行 = 三个可见字段
        assert_eq!(grid.transpose_columns(), vec![0, 1, 2]);
        assert_eq!(grid.transpose_cell(0, 1), Some("id"));
        assert_eq!(grid.transpose_cell(0, 2), Some("2"), "第 2 行的 id 值");
        assert_eq!(grid.transpose_cell(1, 1), Some("name"));
        assert_eq!(grid.transpose_cell(1, 2), Some("bob"));
        assert_eq!(grid.transpose_cell(2, 2), Some("ccdd"));
        assert_eq!(grid.row_text(0), "id\t2", "转置一行 = 字段名 + 值");
        // 只读视图：不脏、不冲突、不进编辑（菜单短路在 context_menu 方法）
        assert!(!grid.is_dirty(0, 2));
        assert!(!grid.is_conflict(0, 2));
        assert!(!grid.is_editing(0, 2));
        assert!(!grid.begin_edit(0, 2));
        // 行导航：越界夹回最后一行
        grid.set_transpose_row(99);
        assert_eq!(grid.transpose_row(), 1);
        // 关闭：原网格原样（隐藏列也没被改动）
        grid.set_transposed(false, 0);
        assert!(!grid.is_transposed());
        assert_eq!(grid.row_text(0), "1\talice\taabb", "普通视图回来");
    }

    /// 【P2-4】转置只投影可见列（隐藏列不竖排出来）
    #[test]
    fn transpose_skips_hidden_columns() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec!["id".to_string(), "name".to_string(), "blob".to_string()],
            vec![vec!["1".to_string(), "alice".to_string(), "aabb".to_string()]],
        );
        grid.hide_column(1);
        grid.set_transposed(true, 0);
        assert_eq!(grid.transpose_columns(), vec![0, 2], "隐藏的 name 不参与转置");
        assert_eq!(grid.transpose_cell(1, 1), Some("blob"));
        assert_eq!(grid.transpose_cell(1, 2), Some("aabb"));
    }

    /// 【P2-4】换数据自动退出转置（行与列都换了，旧转置行无意义）
    #[test]
    fn transpose_resets_when_data_changes() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec!["a".to_string(), "b".to_string()],
            vec![vec!["1".to_string(), "2".to_string()]],
        );
        grid.set_transposed(true, 0);
        assert!(grid.is_transposed());
        grid.set_data(vec!["x".to_string()], vec![vec!["9".to_string()]]);
        assert!(!grid.is_transposed(), "换结果自动退出转置");
    }
}
