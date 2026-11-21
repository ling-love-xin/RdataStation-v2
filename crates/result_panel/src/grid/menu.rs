//! 菜单域（`grid/` 第五块）：右键 / 列头 / 列浏览器三组菜单项（纯函数）+ 列去重
//! 取值 + `TableDelegate::context_menu`（网格最近右键单元格 → 菜单动作回钩子）。

use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::table::TableState;
use gpui_kit::*;

use super::delegate::ResultGridDelegate;
use super::edit::can_edit_cell;
use super::stats::{column_stats, stats_lines};
use super::text::preview_of;
use crate::ports::{Presentation, ResultRequest};
use crate::rowset::RowRef;
/// 【选中统计】状态行三项开关的种类（DBeaver layout 菜单三颗
/// `ToggleSelectionStatAction`，`ResultSetViewer.java:3580-3588`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionStatKind {
    /// `resultset.show.selected.rows`
    Rows,
    /// `resultset.show.selected.columns`
    Columns,
    /// `resultset.show.selected.cells`
    Cells,
}

impl SelectionStatKind {
    /// 菜单文案里的量词（对齐 `ResultSetMessages_zh.properties:125/127/129`）
    pub fn noun(self) -> &'static str {
        match self {
            SelectionStatKind::Rows => "行数",
            SelectionStatKind::Columns => "列数",
            SelectionStatKind::Cells => "单元格数",
        }
    }
}
/// 【第 64 批】行级三钮的可用态（DBeaver `ResultSetPropertyTester.PROP_EDITABLE` 的三个
/// 行级 `expectedValue` 分支，`ResultSetPropertyTester.java:137-158`）。
///
/// | 字段 | DBeaver 条件 | 本仓条件 |
/// | --- | --- | --- |
/// | `add` | `rsv.isInsertable()`（`:137-138`，**不**要求 currentRow） | `editable` |
/// | `copy` | `currentRow != null && rsv.isInsertable()`（`:139-141`） | `editable && has_current_row` |
/// | `delete` | `currentRow != null && (isDeletable` 或 `行是 STATE_ADDED)`（`:142-157`） | `editable && has_current_row && !row_deleted` |
///
/// 本仓没有 `isDeletable` 能力位（`editable` 已是通道 / 项目 / 只读 / 行标识的合一投影，
/// 见 `editor::exec::editability`），所以 `delete` 退化成「可编辑 + 行没被标记删除」：
/// 已删行上再点删除是空操作（宿主 `delete_local_row` 对已删底层行直接返回），置灰就是
/// 为了不让人点了没反应。本地新增行（`RowRef::Local`）的删除 = 移出叠加层，
/// `row_deleted` 恒 false ⇒ 照常可用，与 DBeaver 的 `STATE_ADDED` 分支同向。
///
/// 与 [`can_edit_cell`] 的分工：那个管「编辑某一格」（`PROP_EDITABLE` 的 `edit` / `inline`
/// 分支），这个管「行级增删复制」（`add` / `copy` / `delete` 分支）。两者都只是 UI 侧的
/// 投影，权威判据仍在宿主（`insert_local_row` / `delete_local_row` / `duplicate_local_row`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowActionState {
    pub add: bool,
    pub copy: bool,
    pub delete: bool,
}

pub fn row_action_state(
    editable: bool,
    has_current_row: bool,
    row_deleted: bool,
) -> RowActionState {
    RowActionState {
        add: editable,
        copy: editable && has_current_row,
        delete: editable && has_current_row && !row_deleted,
    }
}

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
    /// 【结果集二次分析】整份结果集进洞察（右侧面板）：宿主先物化成 `tmp_q_` 再取样。
    /// 与 `InsightColumn` 的区别是**范围**：那个只看一列，这个看整张表。
    AnalyzeResultSet,
    /// 【结果集持久化】把整份结果落成**项目分析库的正式表**（`res_<描述>_<时间戳>`）。
    /// 与 `AnalyzeResultSet` 同一份输入（已物化的临时表），差别是去向：临时 → 持久。
    PersistResultSet,
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
    /// 【第 55 批】把相邻行的值复制到**这一行**（对齐 DBeaver `CMD_ROW_COPY_FROM_ABOVE` /
    /// `_BELOW`，`fillContextMenu:3213-3214`）：逐格改写现有行，不插新行
    CopyRowValuesFrom { row: usize, above: bool },
    /// 【第 61 批】编辑**右键落点这一格**（对齐 DBeaver `fillEditMenu:3518-3525` 的
    /// `CMD_ROW_EDIT_INLINE`；`row` = 视图行，`column` = 数据列下标）。
    ///
    /// 与状态栏「修改单元格」（[`ContextAction`] 之外那条 `ResultRequest::BeginEditCell`）
    /// 的区别：那条读网格选中格，这条用右键定位——右键的格与选中格可以不同。
    /// 落点走内嵌编辑（本仓无外部值编辑器通道）。
    EditCell { row: usize, column: usize },
    /// 【列浏览器】复制选中列的**列名**（对齐 DBeaver 元数据面板清单右键
    /// `meta_data_panel_action_copy_column_text`：多选时逐行拼、换行分隔）。
    ///
    /// 不带载荷：选哪些列是清单自己的多选集合（委托 `selected_set`），
    /// 由清单在点击时展开成列名——菜单纯函数不该知道界面状态。
    CopyColumnNames,
    /// 【列头菜单】复制**这一列**的列名（对齐 DBeaver 列头 `CMD_COPY_COLUMN_NAMES`，
    /// `ResultSetViewer.java:3151-3161`，handler 在 `ResultSetHandlerMain.java:333-376`；
    /// zh 文案是「复制字段名」，本仓术语统一叫「列」）。
    ///
    /// 不带载荷：列头菜单天然只针对一列，列名在渲染闭包里已算好
    /// （`header.rs` 的 `column_name_for_menu`）。与列浏览器的 [`Self::CopyColumnNames`]
    /// 分开：那个拷的是多选集合、换行分隔；这个是单列名。
    CopyColumnName,
    /// 【列头菜单·筛选段尾部】保存当前筛选设置（对齐 DBeaver `CMD_FILTER_SAVE_SETTING`，
    /// `ResultSetViewer.java:3928`）。与筛选设置箭头里的「保存过滤器设置」是同一条请求。
    SaveFilterSetting,
    /// 【列头菜单·筛选段尾部】重置已保存的筛选设置（`CMD_FILTER_RESET_SETTING`，`:3929`）
    ResetFilterSetting,
    /// 【列头菜单·筛选段尾部】删除所有过滤 / 排序（`CMD_FILTER_CLEAR_SETTING`，`:3931`；
    /// DBeaver 侧恒摆，无门控）
    ClearAllFilters,
    /// 【列头菜单·筛选段尾部】打开筛选设置浮层（`CMD_FILTER_EDIT_SETTINGS`，`:3932`）
    OpenFilterSettings,
    /// 【行号槽】复制这一行的**显示行号**（对齐 DBeaver `ROW_HEADER` 落点的
    /// `CMD_COPY_ROW_NAMES`，`ResultSetViewer.java:3162-3164`；handler
    /// `ResultSetHandlerMain.java:377-396` 取 `getVisualNumber() + 1`，多行时弹配置对话框）。
    ///
    /// 载荷是显示行号文本（`delegate.cell(row, 0)` 现算的 `row + 1`）：本仓行号槽只按
    /// 单行右键（没有多选集合），直接拷这一行的号，不做配置对话框。
    CopyRowNumber(String),
    /// 【选中统计】切换状态行三项开关之一（`on` = 点击后的目标态；
    /// DBeaver 的 `ToggleSelectionStatAction` 是勾选项，`ResultSetViewer.java:3580-3588`）
    ToggleSelectionStat { kind: SelectionStatKind, on: bool },
    /// 【布局段·记录模式】开 / 关记录（转置）视图（对齐 DBeaver `ToggleModeAction`，
    /// `ResultSetViewer.java:3550-3552`；动作 `actions/ToggleModeAction.java`，
    /// 文案 `dialog_text_check_box_record` = 记录）。文本态不摆（DBeaver 的
    /// `supportsRecordMode()` 门控，`:3550`）。
    ToggleRecordMode,
    /// 【布局段】切到**下一个**呈现方式（对齐 DBeaver `CMD_SWITCH_PRESENTATION`，
    /// `ResultSetViewer.java:3553-3555`；循环逻辑 `switchPresentation():1161-1172`，
    /// 命令名 `command.org.jkiss.dbeaver.core.resultset.switchPresentation.name` = 切换表示）
    CyclePresentation,
    /// 【布局段】显式切到某一档（对齐 DBeaver 给每个 presentation 摆的勾选项，
    /// `ResultSetViewer.java:3558-3577`）
    SetPresentation(Presentation),
    /// 【布局段·筛选条】显示 / 隐藏整条筛选区（对齐 DBeaver `ToggleFilterPanelAction`，
    /// `ResultSetViewer.java:3589-3592`；`open` = 点击后的目标态）
    ToggleFilterPanel { open: bool },
    /// 【filters 段首】「在结果中查找…」（对齐 DBeaver `EDIT_FIND_AND_REPLACE`，
    /// `ResultSetViewer.fillFiltersMenu:3869`）：命令语义，打开查找栏并聚焦，已开不反关。
    OpenFind,
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
    /// 【结果集二次分析】这份结果能不能整表进洞察（宿主接了钩子 + 有行）
    ///
    /// 与 `insight` 分开：`insight` 是「能取一列的样本」（还要求绑定连接），
    /// 这个是「能把整份结果物化成临时表再分析」——不绑定连接的本地结果同样可以。
    pub analyze: bool,
    /// 【结果集持久化】这份结果能不能落成项目表（宿主接了钩子 + 有行 + 已打开项目）
    pub persist: bool,
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
    /// 【选中统计】三项开关当前值（右键菜单项据此标勾选态；
    /// 对齐 DBeaver `ToggleSelectionStatAction` 的 `isChecked`）
    pub selection_stat_rows: bool,
    pub selection_stat_columns: bool,
    pub selection_stat_cells: bool,
    /// 【布局段】当前呈现方式（网格 / 文本）。面板态镜像进网格 delegate，
    /// 菜单据此给「网格 / 文本」两项标勾选态（DBeaver 各 presentation 的 check box）
    pub presentation: Presentation,
    /// 【布局段】记录（转置）视图是否开着（= delegate 的 `transposed`）
    pub record_mode: bool,
    /// 【布局段】筛选条当前是否显示（DBeaver `resultset.show.filterPanel` 的勾选态）
    pub show_filter_panel: bool,
}
/// 行号槽（`#` 列）右键的项（**纯函数**）：DBeaver 的 `ROW_HEADER` 落点只挂「复制行号」
/// （`ResultSetViewer.java:3162-3164`；handler `ResultSetHandlerMain.java:377-396`）。
/// 本仓同口径：行号不是数据，列级动作（筛选 / 排序 / 冻结 / 统计）对它都不成立，
/// 所以这个落点只有这一项。
///
/// 文案把要拷的号写进去（`复制行号「3」`）：一眼能核对是不是那一行，
/// 与列头「复制列名「…」」同一写法。
pub fn row_number_menu_items(row_number: &str) -> Vec<ContextMenuItem> {
    vec![ContextMenuItem {
        label: format!("复制行号「{row_number}」"),
        action: ContextAction::CopyRowNumber(row_number.to_string()),
        separator_before: false,
    }]
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
    // 【DBeaver 对齐】`fillContextMenu:3166-3177`：复制段之后先一条 Separator，再挂 filters
    // 子菜单（对 DATA / COLUMN_HEADER / ROW_HEADER 三个落点都摆）；本仓菜单扁平，把
    // `fillFiltersMenu` 的「在结果中查找…」+ 四项设置直接平铺在这里。
    items.extend(filters_menu_items());
    // 【DBeaver 对齐】排序段（order 子菜单，`fillContextMenu:3179-3187`）：紧接 filters 之后、
    // 行级动作之前。DBeaver 的 `fillOrderingsMenu:3939-3946` 内部只有 ASC / DESC / NONE 三项、
    // 彼此无分隔线，所以本仓这三项同组、首项也不带分隔线。
    items.push(ContextMenuItem {
        label: format!("按「{column_name}」升序（下发源库）"),
        action: ContextAction::SortDown {
            column: column_name.to_string(),
            descending: false,
        },
        separator_before: false,
    });
    items.push(ContextMenuItem {
        label: format!("按「{column_name}」降序（下发源库）"),
        action: ContextAction::SortDown {
            column: column_name.to_string(),
            descending: true,
        },
        separator_before: false,
    });
    // 【排序】当前正按此列排时补「取消排序」（DBeaver 排序子菜单的 NONE 项，同组无分隔线）
    if target.sort_active {
        items.push(ContextMenuItem {
            label: format!("取消排序「{column_name}」"),
            action: ContextAction::ClearSort,
            separator_before: false,
        });
    }
    // 【#28-B3】行级动作独立分组（对齐 DBeaver 行操作段 edit 子菜单，`fillContextMenu:3197-3215`）：
    // 新增 · 复制 · 删除一行。DBeaver 在 edit 子菜单之前先 `add(new Separator())`（`:3217`），
    // 本仓同样给整组一条前导分隔线。
    // 只读结果集（`!editable`）整组不摆；已标记删除的底层行不再摆复制 / 删除——
    // 新增空行也一并收起（被删行区段不再产生新的叠加动作）。动作只改本地叠加层，
    // 不下发 INSERT / DELETE。
    //
    // 【第 62 批】可编辑 / 记录视图这两条不再手写，统一走 [`can_edit_cell`]
    // （对齐 DBeaver `PROP_EDITABLE` 的三条条件，见该函数注释）。`is_deleted` 是本仓
    // 独有的第四个维度：DBeaver 的 `STATE_REMOVED` 只影响 `CMD_CELL_RESET`（`fillEditMenu:3536`），
    // 不影响编辑项；本仓 `begin_edit` 直接拒绝已删行，所以整段一起收起（§9.44.3）。
    if can_edit_cell(target.editable, target.record_mode, true) && !target.is_deleted {
        // 【第 61 批】edit 段首「修改单元格」：对齐 DBeaver `fillEditMenu:3518-3525`——
        // 子菜单内最先摆 `CMD_ROW_EDIT` / `CMD_ROW_EDIT_INLINE`（门控 `valueController != null`
        // 且 `!isReadOnly()`），之后 `:3208` 再来一条 Separator 才接行级的 add/copy/delete。
        // 本仓没有外部值编辑器通道（`ports.rs:214-215` 只读预览、无写回），只接 inline 那条；
        // 文案沿用状态栏那颗（「修改单元格」，§9.24）而不照 `bundle_zh` 的「内嵌编辑」，
        // 免得同一条链路在界面上有两个名字。
        // 行号槽落点不进本函数（`context_menu:786` 早退），所以这里 `column` 一定是数据列。
        items.push(ContextMenuItem {
            label: "修改单元格".to_string(),
            action: ContextAction::EditCell {
                row: target.row,
                column: target.column,
            },
            separator_before: true,
        });
        items.push(ContextMenuItem {
            label: "在下方插入新行".to_string(),
            action: ContextAction::InsertRowBelow(target.row),
            // 组内第二条分隔线：对应 DBeaver `line 3208` 的 `editMenu.add(new Separator())`
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
        // 【第 55 批】DBeaver 在 ADD / COPY / DELETE 之后又一条 Separator（`fillContextMenu:3212`），
        // 再接「从上一行复制 / 从下一行复制」两个命令（`:3213-3214`）——本仓扁平化后
        // 就是行级动作段内的一条分隔线，把「改行本身」与「借用邻行的值」两组分开。
        // 文案照 bundle_zh 的 name（`:128` / `:130`），不加「值到当前行」（那是 description）。
        // 越界由宿主判定（纯函数看不到视图行数）：源行不存在 = 点击无动作，与 DBeaver 一致。
        items.push(ContextMenuItem {
            label: "从上一行复制".to_string(),
            action: ContextAction::CopyRowValuesFrom {
                row: target.row,
                above: true,
            },
            separator_before: true,
        });
        items.push(ContextMenuItem {
            label: "从下一行复制".to_string(),
            action: ContextAction::CopyRowValuesFrom {
                row: target.row,
                above: false,
            },
            separator_before: false,
        });
    }
    // 【DBeaver 对齐】列级视图动作段（冻结 / 隐藏 / 统计）：对应 DBeaver 在 edit 之后
    // `add(new Separator())`（`:3219`）再挂的 view 子菜单（`:3237-3243`），所以本仓整组
    // 带一条前导分隔线。
    items.push(ContextMenuItem {
        label: if target.frozen {
            "取消冻结此列".to_string()
        } else {
            format!("冻结「{column_name}」")
        },
        action: ContextAction::ToggleFreeze(target.column),
        separator_before: true,
    });
    items.push(ContextMenuItem {
        label: format!("隐藏「{column_name}」"),
        action: ContextAction::HideColumn(target.column),
        separator_before: false,
    });
    items.push(ContextMenuItem {
        label: format!("列统计「{column_name}」"),
        action: ContextAction::ColumnStats {
            column: target.column,
        },
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
    // 【结果集二次分析】整表入口：与「洞察此列」同组（都是进洞察），范围是整份结果
    if target.analyze {
        items.push(ContextMenuItem {
            label: format!("分析这份结果集…（{} 行）", target.rows),
            action: ContextAction::AnalyzeResultSet,
            separator_before: !target.insight,
        });
    }
    // 【结果集持久化】同一份结果的另一个去向（临时 → 持久），紧跟「分析」
    if target.persist {
        items.push(ContextMenuItem {
            label: format!("持久化为项目表（{} 行）", target.rows),
            action: ContextAction::PersistResultSet,
            separator_before: false,
        });
    }
    // 【DBeaver 对齐】右键菜单尾部三段（`ResultSetViewer.fillLayoutMenu:3549-3597` 的顺序）：
    // 呈现方式组（记录模式 / 切换表示 / 网格·文本勾选，`:3550-3577`）→ 选中统计三颗
    // （`:3580-3588`）→ 筛选条显隐（`:3589-3592`，中间还隔着一条 `Separator`，
    // 所以该项自带 `separator_before`）。本仓菜单是扁平结构（没有 layout 子菜单），
    // 三段直接平铺在尾部；「✓」前缀表示当前已选中（DBeaver 用 check box 表达同一件事）。
    items.extend(layout_presentation_menu_items(target));
    items.extend(selection_stat_menu_items(target));
    items.push(filter_panel_menu_item(target));
    items
}

/// 【DBeaver 对齐】右键 layout 段的「呈现方式」组（**纯函数**，`ResultSetViewer.java:3549-3578`）：
///
/// 顺序照 DBeaver：记录模式（`ToggleModeAction`，`:3550-3552`，仅 `supportsRecordMode()`）→
/// 切换表示（`CMD_SWITCH_PRESENTATION`，`:3553-3555`）→ 各呈现方式的勾选项（`:3558-3577`）。
///
/// 本仓菜单扁平（没有 layout 子菜单），这一段平铺在数据格右键尾部、选中统计三颗之前；
/// 组首项带分隔线，把它与上方命令隔开（DBeaver 的 presentation 段前面也是一条 separator）。
pub fn layout_presentation_menu_items(target: &ContextTarget) -> Vec<ContextMenuItem> {
    let mut items = Vec::new();
    // 文本态没有「记录」语义（DBeaver 同样按 `activePresentationDescriptor.supportsRecordMode()`
    // 决定摆不摆，`:3550`）——本仓用「不摆」表达，不引入菜单禁用态。
    if target.presentation != Presentation::Text {
        items.push(ContextMenuItem {
            label: format!("{}记录", if target.record_mode { "✓ " } else { "" }),
            action: ContextAction::ToggleRecordMode,
            separator_before: true,
        });
    }
    items.push(ContextMenuItem {
        // DBeaver 只在 presentation 数 ≥2 时摆切换项（`canSwitchPresentation`）；
        // 本仓恒有两档（网格 / 文本），所以恒摆
        label: "切换表示".to_string(),
        action: ContextAction::CyclePresentation,
        separator_before: items.is_empty(),
    });
    for (presentation, label) in [(Presentation::Grid, "网格"), (Presentation::Text, "文本")] {
        items.push(ContextMenuItem {
            label: format!(
                "{}{label}",
                if target.presentation == presentation {
                    "✓ "
                } else {
                    ""
                }
            ),
            action: ContextAction::SetPresentation(presentation),
            separator_before: false,
        });
    }
    items
}

/// 【DBeaver 对齐】右键布局段的「显示过滤器面板」项（**纯函数**，
/// `ResultSetViewer.fillLayoutMenu:3589-3592` 的 `ToggleFilterPanelAction`）：
///
/// 文案照中文包 `controls_resultset_viewer_action_show_filter_panel`
/// （`ResultSetMessages_zh.properties:131` = 「显示过滤器面板」）；DBeaver 是
/// `AS_CHECK_BOX`，`isChecked()` 直接读偏好 `resultset.show.filterPanel`
/// （`:5429-5431`），本仓用 `✓ ` 前缀 + `open` 载荷表达同一件事。
///
/// 位置：统计三颗**之后**，且带分隔线（DBeaver 在这项前面先 `add(new Separator())`）。
pub fn filter_panel_menu_item(target: &ContextTarget) -> ContextMenuItem {
    ContextMenuItem {
        label: format!(
            "{}显示过滤器面板",
            if target.show_filter_panel { "✓ " } else { "" }
        ),
        action: ContextAction::ToggleFilterPanel {
            open: !target.show_filter_panel,
        },
        separator_before: true,
    }
}

/// 【DBeaver 对齐】选中统计三项开关（**纯函数**，`ResultSetViewer.java:3580-3588`）：
/// 顺序固定 行 → 列 → 单元格，文案照中文包
/// `controls_resultset_viewer_action_show_selected_{row,column,cell}_count`
/// （`ResultSetMessages_zh.properties:125/127/129`）；`on` 载荷是点击后的**目标态**。
pub fn selection_stat_menu_items(target: &ContextTarget) -> Vec<ContextMenuItem> {
    let trio = [
        (SelectionStatKind::Rows, target.selection_stat_rows),
        (SelectionStatKind::Columns, target.selection_stat_columns),
        (SelectionStatKind::Cells, target.selection_stat_cells),
    ];
    trio.into_iter()
        .enumerate()
        .map(|(ix, (kind, on))| ContextMenuItem {
            label: format!("{}显示选定的{}", if on { "✓ " } else { "" }, kind.noun()),
            action: ContextAction::ToggleSelectionStat { kind, on: !on },
            separator_before: ix == 0,
        })
        .collect()
}
/// 【DBeaver 对齐】filters 子菜单的整组（**纯函数**，`ResultSetViewer.fillFiltersMenu:3864-3933`）：
/// 首项「在结果中查找…」（`EDIT_FIND_AND_REPLACE`，`:3869`），随后是本仓能落地的四项设置
/// （[`filter_settings_menu_items`]）。DBeaver 中间还有「按不同值过滤 / 按操作符过滤 /
/// 清除本列条件」几组，本仓没有列级约束模型，不做（见 `result-panel.md` §9.35.3 / §9.37.3）。
///
/// 分隔线：本项自带 `separator_before`（filters 段与上方复制段隔开，`:3166`）；
/// 四项那一组也自带一条（`:3926` 的 Separator），所以两小组之间仍有线。
pub fn filters_menu_items() -> Vec<ContextMenuItem> {
    let mut items = vec![ContextMenuItem {
        label: "在结果中查找…".to_string(),
        action: ContextAction::OpenFind,
        separator_before: true,
    }];
    items.extend(filter_settings_menu_items());
    items
}

/// 【DBeaver 对齐】`fillFiltersMenu` 尾部四项（**纯函数**，`ResultSetViewer.java:3926-3932`）：
/// 一条 Separator 之后连摆「保存过滤器设置 / 重置过滤器设置 / 删除所有过滤·排序 /
/// 编辑筛选设置」。
///
/// 门控差异：DBeaver 只在容器是 `DBSEntity` 时摆前两项（`:3927-3930`）；本仓存档键是
/// **结果 SQL**（§9.26.4 第 1 条），不存在「非实体容器」这一层——join / 计算列结果同样能存能取，
/// 故四项恒摆。清空与编辑设置 DBeaver 侧也恒摆（`:3931-3932`）。
///
/// 落点：DBeaver 的 filters 子菜单挂在 `fillContextMenu:3169-3177`，对
/// DATA / COLUMN_HEADER / ROW_HEADER **三个落点都摆**（该方法无落点门控）；本仓菜单扁平，
/// 数据格右键与列头菜单共用这四项（段位置照 `fillContextMenu`：复制段之后、排序段之前）。
pub fn filter_settings_menu_items() -> Vec<ContextMenuItem> {
    vec![
        ContextMenuItem {
            label: "保存过滤器设置".to_string(),
            action: ContextAction::SaveFilterSetting,
            separator_before: true,
        },
        ContextMenuItem {
            label: "重置过滤器设置".to_string(),
            action: ContextAction::ResetFilterSetting,
            separator_before: false,
        },
        ContextMenuItem {
            label: "删除所有过滤 / 排序".to_string(),
            action: ContextAction::ClearAllFilters,
            separator_before: false,
        },
        ContextMenuItem {
            label: "编辑筛选设置…".to_string(),
            action: ContextAction::OpenFilterSettings,
            separator_before: false,
        },
    ]
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
    // 【DBeaver 对齐】列头菜单第一件复制动作是「复制列名」（`ResultSetViewer.java:3151-3161`
    // 在 DATA 落点之外单独给 COLUMN_HEADER 挂 `CMD_COPY_COLUMN_NAMES`）。本仓列头菜单是
    // 扁平结构（DBeaver 是「过滤 / 排序 / 编辑」子菜单），放在复制段首位：
    // 用户拿列名是为了拼 SQL，比「复制整列取值」更靠前。
    items.push(ContextMenuItem {
        label: format!("复制列名「{column_name}」"),
        action: ContextAction::CopyColumnName,
        separator_before: false,
    });
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
        action: ContextAction::ColumnStats {
            column: target.column,
        },
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
    // 【DBeaver 对齐】`fillFiltersMenu` 整组（`ResultSetViewer.java:3864-3933`）：
    // 与数据格右键共用同一组（DBeaver 的 filters 子菜单对三个落点都摆，见
    // `fillContextMenu:3169-3177`），文案 / 顺序 / 分隔线口径只在 `filters_menu_items` 一处。
    items.extend(filters_menu_items());
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
        // 【DBeaver 对齐】清单右键第一项就是「复制列名」（`MetaDataPanel.fillCustomActions`
        // 的 `meta_data_panel_action_copy_column_text`，文案 "Copy Column Names"）；
        // 多选时按选中集合逐行拼、换行分隔，本仓同口径。
        ContextMenuItem {
            label: "复制列名".to_string(),
            action: ContextAction::CopyColumnNames,
            separator_before: false,
        },
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
            action: ContextAction::ColumnStats {
                column: target.column,
            },
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
pub fn column_distinct_values(rows: &[Vec<String>], column: usize, limit: usize) -> Vec<String> {
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
    if row >= delegate.view_len() {
        return menu;
    }
    // 【DBeaver 对齐】行号槽落点只给「复制行号」（`ROW_HEADER` → `CMD_COPY_ROW_NAMES`，
    // `ResultSetViewer.java:3162-3164`）：行号不是数据，列级动作对它不成立。这里是独立的
    // 早退路径，不进 [`context_menu_items`]——那个纯函数回答的是「这一格数据能干什么」。
    if ResultGridDelegate::is_row_number(col) {
        let number = delegate.cell(row, col);
        let mut menu = menu;
        for item in row_number_menu_items(&number) {
            if item.separator_before {
                menu = menu.separator();
            }
            if let ContextAction::CopyRowNumber(text) = item.action {
                menu = menu.item(PopupMenuItem::new(item.label).on_click(
                    move |_, _window, app| {
                        app.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    },
                ));
            }
        }
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
        // 【结果集二次分析】整表入口独立判定（钩子在 + 有行；不要求绑定连接）
        analyze: delegate.analyze_result_set_available(),
        // 【结果集持久化】同理，另要求宿主接了持久化出口（未打开项目时宿主不接）
        persist: delegate.persist_result_set_available(),
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
        // 【选中统计】三项开关当前值（菜单标「✓」用；镜像是面板同步进来的）
        selection_stat_rows: delegate.selection_stat_prefs.rows,
        selection_stat_columns: delegate.selection_stat_prefs.columns,
        selection_stat_cells: delegate.selection_stat_prefs.cells,
        // 【布局段】呈现方式与记录模式（delegate 侧镜像 / 直读 transposed）
        presentation: delegate.presentation_view(),
        record_mode: delegate.transposed,
        // 【布局段】筛选条显隐（delegate 侧镜像；反向布尔见字段注释）
        show_filter_panel: delegate.show_filter_panel(),
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
                    menu = menu.item(
                        PopupMenuItem::new(item.label)
                            .on_click(move |_, _window, app| hook(&needle, app)),
                    );
                }
            }
            ContextAction::SortDown { column, descending } => {
                // 与「按值筛选」同口径：没接钩子就不摆这一项
                if let Some(hook) = delegate.on_sort_down.clone() {
                    menu = menu.item(
                        PopupMenuItem::new(item.label)
                            .on_click(move |_, _window, app| hook(&column, descending, app)),
                    );
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
                // 【列布局落盘】改完交回「该存了」：宿主那边先取面板快照再存会话
                let table = cx.entity().clone();
                let queue = delegate.action_queue.clone();
                menu = menu.item(PopupMenuItem::new(item.label).on_click(
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
            ContextAction::AnalyzeResultSet => {
                // 同「洞察此列」：没接钩子（宿主没接二次分析出口）就不摆这一项
                if let Some(hook) = delegate.on_analyze_result_set.clone() {
                    menu = menu.item(
                        PopupMenuItem::new(item.label)
                            .on_click(move |_, _window, app| hook(app)),
                    );
                }
            }
            ContextAction::PersistResultSet => {
                // 同上：宿主没接持久化出口（未打开项目等情况）就不摆这一项
                if let Some(hook) = delegate.on_persist_result_set.clone() {
                    menu = menu.item(
                        PopupMenuItem::new(item.label)
                            .on_click(move |_, _window, app| hook(app)),
                    );
                }
            }
            ContextAction::InsightColumn { column } => {
                // 同「按值筛选」：没接钩子（宿主没接洞察端口）就不摆这一项
                if let Some(hook) = delegate.on_insight_column.clone() {
                    menu = menu.item(
                        PopupMenuItem::new(item.label)
                            .on_click(move |_, _window, app| hook(&column, app)),
                    );
                }
            }
            ContextAction::ColumnStats { column } => {
                // 统计是**网格自己算**（纯遍历），弹层由宿主开（复用值预览的对话框通道）
                if let Some(hook) = delegate.on_column_stats.clone() {
                    let name = delegate.columns.get(column).cloned().unwrap_or_default();
                    let text = stats_lines(&column_stats(&delegate.rows, column)).join("\n");
                    menu = menu.item(
                        PopupMenuItem::new(item.label)
                            .on_click(move |_, _window, app| hook(name.clone(), text.clone(), app)),
                    );
                }
            }
            ContextAction::HideColumn(column) => {
                // 隐藏是**表格自己的事**（改视图列序），与冻结同一条路：点击在独立事件里，
                // 可以安全地更新 delegate 并重建列组
                // 【列布局落盘】改完交回「该存了」（口径同冻结）
                let table = cx.entity().clone();
                let queue = delegate.action_queue.clone();
                menu = menu.item(PopupMenuItem::new(item.label).on_click(
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
                let table = cx.entity().clone();
                let queue = delegate.action_queue.clone();
                menu = menu.item(PopupMenuItem::new(item.label).on_click(
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
            // 【第 61 批】右键「修改单元格」：把右键落点坐标折成**组件列**（+1 补行号槽）
            // 后交回宿主，走与双击同一条 `TableEvent::DoubleClickedCell` 链路。
            // 用 `on_row_action` 钩子（同一份队列），与行级动作一致；没接钩子（只读网格 /
            // 未挂宿主的单测）时菜单项已在 `context_menu_items` 收起，这里兜底忽略。
            ContextAction::EditCell { row, column } => {
                if let Some(hook) = delegate.on_row_action.clone() {
                    let col = column + 1;
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            hook(crate::ports::ResultRequest::BeginEditCellAt { row, col }, app);
                        },
                    ));
                }
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
            // 【第 55 批】「从上一行 / 下一行复制」：逐格改写现有行，走同一条行级动作钩子
            // 交回宿主（底层行落变更集、本地行落快照）；没接钩子的只读网格不摆。
            ContextAction::CopyRowValuesFrom { row, above } => {
                if let Some(hook) = delegate.on_row_action.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            hook(
                                crate::ports::ResultRequest::CopyRowValuesFrom { row, above },
                                app,
                            );
                        },
                    ));
                }
            }
            // 列级筛选只属于列头菜单（右键的「按值筛选」是全局词）：右键收到即忽略
            ContextAction::FilterColumnValue { .. } | ContextAction::ClearColumnFilter { .. } => {}
            // 【filters 段首】「在结果中查找…」= DBeaver `EDIT_FIND_AND_REPLACE` 命令
            // （`fillFiltersMenu:3869`）：只打开并聚焦查找栏，已开不反关（与工具栏那颗
            // CHECK 型开关 `ToggleFind` 的语义分开，见 `ports.rs` 两变体的注释）。
            ContextAction::OpenFind => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            queue.request(ResultRequest::OpenFind, app);
                        },
                    ));
                }
            }
            // 【DBeaver 对齐】筛选段尾部四项（`fillFiltersMenu:3926-3932`）：DBeaver 的
            // filters 子菜单对 DATA / COLUMN_HEADER / ROW_HEADER 三个落点都摆
            // （`fillContextMenu:3169-3177` 无落点门控），所以数据格右键同样摆这四项——
            // 它们按「当前结果的筛选状态」动手，与落点在哪个单元格无关。四项都只把意图
            // 交回面板（与列头菜单 `grid/header.rs` 发同一组请求）。
            ContextAction::SaveFilterSetting => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            queue.request(ResultRequest::SaveFilterSetting, app);
                        },
                    ));
                }
            }
            ContextAction::ResetFilterSetting => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            queue.request(ResultRequest::ResetFilterSetting, app);
                        },
                    ));
                }
            }
            ContextAction::ClearAllFilters => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            queue.request(ResultRequest::ClearFilterAndOrdering, app);
                        },
                    ));
                }
            }
            ContextAction::OpenFilterSettings => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            queue.request(ResultRequest::OpenFilterSettings, app);
                        },
                    ));
                }
            }
            // 「复制列名」只属于列浏览器清单（它要多选集合，网格右键没有这个概念）
            ContextAction::CopyColumnNames => {}
            // 列头的「复制列名」也只属于列头：右键的「复制此列」复制的是**取值**，不是名字
            ContextAction::CopyColumnName => {}
            // 行号槽的「复制行号」走本函数开头的独立早退路径，这里只做穷尽占位
            ContextAction::CopyRowNumber(_) => {}
            // 【选中统计】三项开关是**面板状态**（状态行由面板画），网格只把意图交回宿主：
            // 宿主改面板状态 + 落盘偏好 → 状态行重画。没有行动队列（独立网格）就不摆。
            ContextAction::ToggleSelectionStat { kind, on } => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            queue.request(ResultRequest::ToggleSelectionStat { kind, on }, app);
                        },
                    ));
                }
            }
            // 【布局段】记录模式 / 切换表示 / 显式切档：全是**面板状态**（呈现方式由面板
            // 决定、记录视图由面板转置），网格只交回请求；没有队列（独立网格）就不摆。
            ContextAction::ToggleRecordMode => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| queue.request(ResultRequest::ToggleTranspose, app),
                    ));
                }
            }
            ContextAction::CyclePresentation => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            queue.request(ResultRequest::CyclePresentation, app)
                        },
                    ));
                }
            }
            ContextAction::SetPresentation(presentation) => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            queue.request(ResultRequest::SetPresentation(presentation), app)
                        },
                    ));
                }
            }
            // 【布局段·筛选条】显隐同样是面板态：网格只交回请求，面板收掉展开态后重画
            ContextAction::ToggleFilterPanel { open } => {
                if let Some(queue) = delegate.action_queue.clone() {
                    menu = menu.item(PopupMenuItem::new(item.label).on_click(
                        move |_, _window, app| {
                            queue.request(ResultRequest::ToggleFilterPanel { open }, app)
                        },
                    ));
                }
            }
        }
    }
    menu
}
