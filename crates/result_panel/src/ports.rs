//! 结果面板 ↔ 编辑器宿主的动作端口（Shared 事件队列，彻底解耦）
//!
//! 结果面板（[`crate::panel::ResultPanel`]）渲染期只做纯读 + 入队：工具栏 / 标签条 /
//! 卡片上的按钮一律 [`ResultActionQueue::push`]（不直接调宿主方法）；编辑器宿主在
//! 事件路径 `drain` 后按变体分派到既有动作方法（执行 / 提交 / 导出 / 分析 / 绑定等
//! 仍归编辑器，它们依赖执行链与 engine 端口）。
//!
//! 为什么不用注入闭包：结果面板要在不反向依赖 editor 的前提下自洽（lib.rs 的依赖
//! 方向硬约束 `editor → result_panel`），闭包会把宿主方法名焊进面板渲染；队列把
//! 「动作语义」（枚举）与「动作实现」（宿主）分开，面板可独立复用与测试。
//!
//! 网格内部交互（右键菜单 / 行编辑 / 列统计 / 值预览 / 加载更多）**不走队列**——
//! [`crate::grid::ResultGridDelegate`] 已有跨 crate 安全的注入钩子
//! （`Rc<dyn Fn(..., &mut App)>`），那套机制保持原样，不重复发明。

use std::cell::RefCell;
use std::rc::Rc;

use crate::export::ExportFormat;

/// 【分页】状态栏翻页目标（**纯视图滚动，不重查源库**）
///
/// 为什么是滚动而不是带 OFFSET 重取：`fetch_next` 那套分段取数依赖源库给出**稳定顺序**，
/// 无 `ORDER BY` 的查询上翻页会重复 / 跳行（`WINDOW_FETCH_NOTICE` 已就此提示）；而已抓到的
/// 行本来就在内存里，翻页只是把视口挪过去——零重查、零风险，且「末页」不需要下发 COUNT
/// （末页 = 已抓到的最后一行，语义是"到底了"而不是"库里的第 N 页"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageTarget {
    /// 首页：滚到第 1 行
    First,
    /// 上一页：以当前可见首行为基准上移一屏
    Prev,
    /// 下一页：以当前可见首行为基准下移一屏（到底且还有下一段时顺带取数）
    Next,
    /// 末页：滚到已抓到的最后一行
    Last,
}

/// 结果面板发出的面板级动作请求（编辑器宿主 drain 后分派）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultRequest {
    /// 结果集标签条切选中（下标 = 标签顺序）
    SelectSet(usize),
    /// 对比栏标签条切选中（下标 = 标签顺序；点当前选中 = 取消对比）
    SelectCompare(usize),
    /// 工具栏「并排对比」开关（开 / 关）
    ToggleCompareMode,
    /// 工具栏「转置视图」开关（开 / 关）
    ToggleTranspose,
    /// 转置视图「上一行」导航
    TransposePrev,
    /// 转置视图「下一行」导航
    TransposeNext,
    /// 工具栏筛选框「✕」清除（清空输入框 + 立即应用空词；`window` 由宿主分派时提供）
    ClearFilter,
    /// 下发源库开关翻转后的重查（开着时应用筛选会重查数据库）
    PushdownFilter { value: String },
    /// 【1.2】筛选框运行按钮：用 `select * from (原SQL) where 词` **替换**当前结果集
    /// 重查（词取面板实时筛选词）；生成 SQL 写入面板 `exec_sql`（右栏 ExecSql 页签展示）
    RunFilterAsQuery,
    /// 【1.2】筛选框展开 / 收起（点击展开按钮后出现顶宽筛选条）
    ToggleFilterExpand,
    /// 批量提交待提交变更
    CommitPending,
    /// 批量撤销待提交变更
    RollbackPending,
    /// 分析菜单项（对已抓行跑聚合 SQL）
    Analysis { sql: String },
    /// 分析菜单「自定义」项（弹输入框，模板预填）
    AnalysisCustom,
    /// 复制当前结果（TSV）
    CopyTsv,
    /// 复制来源 SQL（工具栏最左侧 SQL 预览点击）
    CopySql,
    /// 【2026-10-28 微调】SQL 预览：点击工具栏 SQL 文本框弹出**窗口**显示原始 SQL
    /// （kit Dialog + 只读 Textarea；面板在事件路径预建实体——render 期只有 &App，
    /// `Dialog::new` 需 `&mut App`，走端口转手）
    OpenSqlDialog,
    /// 【2026-10-28 微调】关闭 SQL 预览弹窗（Dialog 关闭按钮 / 遮罩点击触发）
    CloseSqlDialog,
    /// 【1.2】复制「执行SQL」页签里的 wrapped SQL（筛选框运行生成的）
    CopyExecSql,
    /// 【R1】复制当前选中单元格的值（值详情页工具条的复制钮；无选中格时为空操作）
    CopyValue,
    /// 【保存值】把当前选中格的值存成文件。
    ///
    /// **对齐的是哪个动作**（实证口径）：DBeaver 值页工具条由
    /// `ValueViewerPanel.fillToolBar`（`ValueViewerPanel.java:409-428`）装三样东西——
    /// `valueManager.contributeActions(...)`（`ContentValueManager.contributeContentActions`
    /// 在此贡献「保存至文件…」`ResultSetMessages.model_jdbc_save_to_file_`，图标 `UIIcon.SAVE_AS`，
    /// 见 `ContentValueManager.java:86-99`）、引用值动作，以及**只在非只读时才加**的
    /// `SaveValueAction`（`:445`，command `…core.resultset.cell.save`，`saveValue()` 于 `:370-385`
    /// 把编辑器里的值**写回结果集**——`previewController.updateValue`，不是存文件）。
    /// 本仓只读预览、无写回通道，故只做「保存至文件」这一支：`ContentValueManager.saveToFile`
    /// （`:291-334`，文本按 UTF-8、二进制直写、用户取消不算错误、失败弹提示）。
    ///
    /// 字节与默认文件名由面板算好（[`ResultPanel::selected_value_payload`]），
    /// 宿主只负责选路径 + 写盘。
    SaveValue,
    /// 刷新（重跑当前结果 SQL）
    Refresh,
    /// 取下一段
    FetchMore,
    /// 取全部行（DBeaver 对齐：抓全量数据）
    FetchAll,
    /// 【分页】状态栏翻页（首页 / 上页 / 下页 / 末页）：**纯视图滚动**，不重查
    /// （下页到底且这份结果还有下一段时，宿主顺带发一次取下一段）
    GoPage(PageTarget),
    /// 【P1-2】状态栏行号跳转（**1 基**，与行号列显示同口径）：滚到该行并选中；
    /// 越界由宿主夹到最后一行。纯视图导航，不重查。
    GoRow(usize),
    /// 【P2-1】值查看器 hex 转储翻页（目标页号，0 基；越界由面板夹到合法页）。
    /// 面板自持页号（纯视图态），所以这条同样走队列回到面板自己处理。
    HexPage(usize),
    /// 导出（格式 × 范围）
    Export { format: ExportFormat, all: bool },
    /// 错误卡片「复制错误」
    CopyError,
    /// 错误卡片「定位到行」
    LocateError,
    /// 下发源库开关（只翻转状态，不重查——与 [`Self::PushdownFilter`] 区别开）
    SetPushdown { open: bool },
    /// 工具栏「辅助栏」开关：结果面板右侧辅助栏（列浏览器 / 值详情 / 计算 / 分组）开合
    ToggleValueSidebar,
    /// 状态栏左端开关：左侧栏（网格 / 文本 / 记录竖条）开合（完全收起 = 不占宽度）
    ToggleLeftSidebar,
    /// 【R5 可拖分栏】拖右栏分栏边改尺寸：`delta` = 本次拖拽沿**分栏法线**的位移
    /// （像素；横排是 x、竖排是 y，正向 = 朝手柄方向即**尺寸变小**，与横排口径同构）。
    /// 面板按方向更新宽度或高度并 clamp 到 ui 的上下限；纯视图状态，不落盘。
    /// 取整像素（`ResultRequest` 需要 `Eq`，故不携带 `f32`）。
    ResizeRightSidebar(i32),
    /// 【R5 比例持久化】拖拽结束定稿：宿主把面板当前右栏宽度写进全局偏好项
    /// （`result_panel.sidebar_width`）。拖拽过程只改内存宽度，落盘只在这一刻做一次。
    /// 竖排（网格下方）时只清拖拽标记、不落盘（高度没有对应偏好项）。
    EndResizeRightSidebar,
    /// 【R5 布局方向切换】右栏在右侧 / 下方之间切换（对齐 DBeaver
    /// `CMD_TOGGLE_LAYOUT` → `toggleVerticalLayout`）：纯视图状态，不落盘。
    ToggleVerticalLayout,
    /// 左栏列浏览器清单行眼睛：切换该列隐藏 / 显示（宿主 update 网格 + 面板）
    ToggleColumnVisibility(usize),
    /// 【P2-3】列浏览器多选批量显隐：把 `columns`（数据列序）整批设为隐藏 / 显示
    /// （逐列复用单列接口，隐藏仍保留「至少 1 列可见」底线）
    SetColumnsHidden { columns: Vec<usize>, hidden: bool },
    /// 【P2-3】清单多选集合变化（Ctrl 点击增删 / 普通点击单选替换）：delegate 是
    /// 选集的权威（`List` 只给 `confirm(secondary)`，不维护集合），面板只镜像它
    /// （标题行「已选 N 列」与批量按钮可见性据此渲染）
    ColumnSelectionChanged(Vec<usize>),
    /// 【P2-3】清空清单多选集合（标题行「清空选择」按钮）
    ClearColumnSelection,
    /// 列浏览器清单单击选中一列（数据列序）：写清单选中态 + 网格列高亮联动
    SelectColumn(usize),
    /// 列浏览器清单双击定位：网格滚到该列首行并选中
    LocateColumn(usize),
    /// 列浏览器搜索框「✕」清除（清空输入框 + 搜索词；`window` 由宿主分派时提供）
    ClearColumnSearch,
    /// 呈现方式切换（DBeaver 左栏「显示方式」：网格 / 文本）。显式目标值——
    /// 网格 / 文本两个入口（左栏按钮组、工具栏按钮）都要能精确切到目标，
    /// 翻转语义在「已在该呈现」时会把用户点反。
    SetPresentation(Presentation),
    /// 右栏标签页切换（列浏览器 / 值详情 / 计算 / 分组；对齐 DBeaver 右侧 metadata /
    /// valueviewer / aggregate / grouping 同栏页签体系）
    SetRightTab(RightTab),
    /// 【#28-B3】在指定视图行下方插入一条本地空行（`None` = 追加到末尾区段）。
    /// 宿主经网格 `RowRef` 翻译身份后落到 `LocalRowSet`；只改叠加层，不下发 INSERT。
    InsertRowBelow(Option<usize>),
    /// 【#28-B3】复制指定视图行（视图行号）为一条本地行，携带源行展示值快照。
    DuplicateRow(usize),
    /// 【#28-B3】删除指定视图行：底层行只打删除标记（可撤销恢复）；本地行直接移出叠加层。
    DeleteRow(usize),
    /// 【#28-B3】撤销（组合语义：格级 `EditHistory` 非空先走格级，栈空再走行级 `RowHistory`）
    UndoAction,
    /// 【#28-B3】重做（组合语义与 [`Self::UndoAction`] 对称）
    RedoAction,
    /// 【C1】重命名第 `index` 份结果集标签。
    /// `Some(name)` = 覆盖自动标签（空白串宿主侧归一为恢复 `结果 N`）；`None` = 恢复自动标签。
    /// 会话级，不落盘。
    RenameSet(usize, Option<String>),
    /// 【结果集 #296】列头拖拽出新列序（列名数组，按显示序）——宿主持久化到会话
    /// （跨会话恢复：同文档重跑按列名匹配重排）。空数组 = 不该发生（拖拽必有新序）。
    ColumnOrderChanged { order: Vec<String> },
    /// 【P1-3】列宽拖拽结束（列名 → 像素宽的权威值已写进面板快照）——宿主收到只做
    /// 「该存了」这件事：值直接从面板 `column_widths()` 读，不带载荷（与列序同通道）。
    ColumnWidthsChanged,
    /// 【列布局落盘】隐藏 / 冻结改了（面板快照已更新到 `column_hidden()` /
    /// `column_frozen()`）——宿主收到只做「该存了」，值从面板读，不带载荷。
    /// 列头菜单 / 列头右键就地改 delegate 的路径也发它：宿主先取面板快照再存会话。
    ColumnLayoutChanged,
    /// 【C1】切换第 `index` 份结果集的钉住态（钉住不被 FIFO 淘汰；会话级）。
    TogglePin(usize),
    /// 【DBeaver 对齐】关闭第 `index` 份结果集（标签上的关闭按钮）：宿主从 store 移除
    /// 该槽位（条目与变更集 / 历史 / 事实档一并丢弃）。只剩一份时 store 拒绝删除
    /// （`remove_set` 的口径，见其注释——与 DBeaver「关掉后留空白页」是一处有意偏离）。
    CloseSet(usize),
    /// 【DBeaver 对齐】拖动标签重排：把第 `from` 份结果集搬到第 `to` 位
    /// （对应 `SQLEditor.java:1518` 的 `TabFolderReorder`；槽位整份搬，选中项跟着走）。
    MoveSet { from: usize, to: usize },
    /// 【C1】进入第 `index` 份结果集的重命名态（面板弹输入框；输入确认后再发
    /// [`Self::RenameSet`]）。单独走一条事件路径是因为 render 闭包拿不到 `&mut App`，
    /// 不能在 render 里直接改面板状态。
    BeginRename(usize),
    /// 【C1】提交重命名（读面板输入框当前值，发 RenameSet 回 store；空串恢复序号）
    CommitRename,
    /// 【C1】取消重命名（不发请求，面板退出 renaming 态）
    CancelRename,
    /// 【B1】面板级临时开关：列头直接显示注释（覆盖全局偏好）
    SetShowColumnComments { open: bool },

    // 【R2 计算页对齐 DBeaver Calc】`AggregateColumnsPanel` 的函数集与口径都是可变的
    // （`+:365` / `-:418` / `Reset:438` / `GroupByColumns:333` / `ValueTypeToggle:348`），
    // 本仓这四项都经队列回面板（render 是纯读路径，不能就地改状态）。
    /// 启用一个统计项（对齐「＋添加函数」）
    AddStatsItem(StatsItem),
    /// 停用一个统计项（对齐「－删除选中函数」；本仓由统计行行尾的 × 触发，
    /// 因为统计行是普通元素、不是可选中 Tree 行，避免为对齐按钮引入整棵 Tree）
    RemoveStatsItem(StatsItem),
    /// 恢复默认函数集（对齐「Reset」）
    ResetStatsItems,
    /// 切换聚合口径（对齐 `aggregateAsStrings` CHECK：影响 MIN/MAX 的比较方式）
    SetStatsMode(StatsMode),
    /// 【R3 分组页】切换分组列集合里的某一列（点选加入 / 再点移除；空集合 = 用当前选中列）
    ToggleGroupColumn(usize),
    /// 【R3 分组页】分组结果排序口径（按值 / 按计数）
    SetGroupSort(GroupSort),
    /// 【R3 分组页】「仅重复」：只保留行数 > 1 的分组（对齐 DBeaver 分组面板同名选项）
    SetGroupDuplicatesOnly(bool),
    /// 【R3 分组页】清空分组列集合（对齐 DBeaver `ClearGroupingAction`：工具条图标动作，
    /// 且只有分组列非空时才可用）。本仓清空后回落到「当前选中列兜底」，DBeaver 是回到
    /// 空态（见 result-panel.md §8.49）
    ClearGroupColumns,
}

/// 【保存值】待落盘的内容：面板算字节与默认文件名，宿主只选路径 + 写盘。
///
/// 为什么由面板算字节：值查看器的三种形态（文本 / JSON / hex & 图片）**同源于格子文本**
/// （二进制是 hex 文本，`hex_to_bytes` 可无损还原；图片也是同一份 hex 文本经魔数判定），
/// 只有面板手里有 `selection_pos` 与列类型，宿主不该再理解一遍值形态。
pub struct ValuePayload {
    /// 要写的字节：文本 / JSON = 展示文本原文；二进制 = hex 还原出的真字节
    /// （图片因此存下来就是可用的 png/jpg/gif/webp）
    pub bytes: Vec<u8>,
    /// 默认文件名（扩展名按值形态给），只给文件名、不含目录
    pub default_name: String,
}

/// 【R2】计算页的统计项（对齐 DBeaver Calc 的可配置聚合函数集
/// `AggregateColumnsPanel.enabledFunctions`，来源 `FunctionsRegistry` 的
/// `AggregateFunctionDescriptor`）。本仓不引注册表，六项固定可选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatsItem {
    /// 非空值数（COUNT，DBeaver `IAggregateFunction` 的 count）
    Count,
    /// NULL 数（本地扩展项：DBeaver 用 COUNT 与行数的差表达，本仓直给）
    Nulls,
    /// 最小值
    Min,
    /// 最大值
    Max,
    /// 均值（仅数值口径有值）
    Avg,
    /// 合计（仅数值口径有值）
    Sum,
}

impl StatsItem {
    /// 默认全集（顺序 = 原「计算」页六行的顺序）
    pub const ALL: [StatsItem; 6] = [
        Self::Count,
        Self::Nulls,
        Self::Min,
        Self::Max,
        Self::Avg,
        Self::Sum,
    ];

    /// 展示名（语义对标 DBeaver 的 COUNT / SUM / AVG / MIN / MAX 函数标签）
    pub fn label(self) -> &'static str {
        match self {
            Self::Count => "非空",
            Self::Nulls => "NULL",
            Self::Min => "最小值",
            Self::Max => "最大值",
            Self::Avg => "均值",
            Self::Sum => "合计",
        }
    }

    /// 数值口径之外是否仍有意义：Min/Max 在字符串口径下按字典序比较，
    /// Avg/Sum 只有数值口径有值（DBeaver 的 SUM/AVG 对字符串累计不成立）。
    pub fn supports_text_mode(self) -> bool {
        matches!(self, Self::Count | Self::Nulls | Self::Min | Self::Max)
    }
}

/// 【R2】计算页聚合口径（对齐 DBeaver Calc 的 `aggregateAsStrings` CHECK）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatsMode {
    /// 数值口径（默认）：Min/Max 按数值比较，Avg/Sum 有值
    #[default]
    Numeric,
    /// 字符串口径：Min/Max 按字典序比较（文本列也能给极值），Avg/Sum 不出值
    Text,
}

impl StatsMode {
    /// 另一个口径（按钮 toggle 用）
    pub fn toggled(self) -> Self {
        match self {
            Self::Numeric => Self::Text,
            Self::Text => Self::Numeric,
        }
    }
}

/// 【R3 分组页】分组结果排序口径（对齐 DBeaver 分组面板的排序配置）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupSort {
    /// 按分组键字典序升序（默认；未排序的首现顺序也接近这个直觉）
    #[default]
    Value,
    /// 按组内行数降序（同数按值序兜底，避免每次重算顺序抖）
    CountDesc,
}

impl GroupSort {
    /// 另一个口径（按钮 toggle 用）
    pub fn toggled(self) -> Self {
        match self {
            Self::Value => Self::CountDesc,
            Self::CountDesc => Self::Value,
        }
    }

    /// 按钮文案（右侧窄栏只放得下 3 个字）
    pub fn label(self) -> &'static str {
        match self {
            Self::Value => "按值",
            Self::CountDesc => "按计数",
        }
    }
}

/// 结果集呈现方式（DBeaver Data Editor 的显示方式：网格 / 文本）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presentation {
    /// 默认网格（gpui-kit DataTable）
    #[default]
    Grid,
    /// 文本视图：当前结果（可见列 + 可见行）渲染为 TSV 纯文本，可选中复制
    Text,
}

/// 结果面板右侧辅助栏的标签页（对齐 DBeaver `resultset/panel/` 右侧面板组：
/// metadata / valueviewer / aggregate / grouping 同栏页签切换；「引用」页留 backlog，
/// 见 result-panel.md §8.12）。顺序：列浏览器 → 值详情 → 计算 → 分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    /// 列浏览器：搜索框 + 列清单 + 底部元数据明细（默认；对齐 DBeaver metadata 面板）
    #[default]
    Browser,
    /// 数值查看器：选中格完整值
    Value,
    /// 计算：选中列的 count / null / min / max / avg / sum（纯函数，不引 polars）
    Stats,
    /// 分组：按一列本地 groupby 计数（纯函数；DBeaver 大表口径是下发 GROUP BY）
    Group,
    /// 引用：外键引用查看（选中列被哪些表引用，对齐 DBeaver 引用页签；backlog 空态）
    Reference,
    /// 【1.2】执行 SQL：筛选框运行后生成的 wrapped SQL（`select * from (原SQL) where …`）
    /// 原文展示（只读 + 复制）。执行完成后自动切到此页签。
    ExecSql,
}

/// 结果动作队列（`Rc<RefCell<Vec<_>>>` 共享句柄）
///
/// 编辑器宿主创建（一次），同一份句柄给两块：
/// - 结果面板（渲染闭包 clone 捕获，按钮 `push`）
/// - 宿主自身（事件路径 `drain` 分派）
#[derive(Clone, Debug, Default)]
pub struct ResultActionQueue {
    inner: Rc<RefCell<Vec<ResultRequest>>>,
    /// 【#8-按钮断链修复】通知目标（结果面板实体弱引用）。按钮 `request` 在
    /// 入队后立即 `notify` 面板 → 宿主 `observe_in` 回调 drain。此前按钮只 `push`
    /// 不通知，宿主 drain 依赖面板"碰巧"的 notify，点击要么延迟生效要么永不生效
    /// （用户实测：记录按钮无反应、文本视图半天才切出来——同一根因）。
    panel: Rc<std::cell::RefCell<Option<gpui_kit::WeakEntity<super::panel::ResultPanel>>>>,
}

impl ResultActionQueue {
    /// 新建空队列
    pub fn new() -> Self {
        Self::default()
    }

    /// 【#8】宿主在面板实体创建后注入通知目标（弱引用，不产生循环）
    pub fn set_panel(&self, panel: gpui_kit::WeakEntity<super::panel::ResultPanel>) {
        *self.panel.borrow_mut() = Some(panel);
    }

    /// 【#8】入队并立即通知面板（按钮 `on_click` 专用：有 `&mut App`）：
    /// `push` + `app.notify(&panel)`。通知触发宿主 `observe_in` 回调 → `drain_result_actions`
    /// 在同一次事件里分派，点击即时生效。面板方法内部仍用纯 `push`（方法自身会 notify）。
    pub fn request(&self, request: ResultRequest, app: &mut gpui_kit::App) {
        self.inner.borrow_mut().push(request);
        if let Some(panel) = self.panel.borrow().clone() {
            app.notify(panel.entity_id());
        }
    }

    /// 入队一条动作（面板方法内部调用：自身随后 notify；纯写，无副作用）
    pub fn push(&self, request: ResultRequest) {
        self.inner.borrow_mut().push(request);
    }

    /// 取走全部在途动作（事件路径消费；取空后队列恢复空）
    pub fn drain(&self) -> Vec<ResultRequest> {
        std::mem::take(&mut *self.inner.borrow_mut())
    }

    /// 是否为空（渲染期判「有没有在途请求」用不到；主要给测试断言）
    pub fn is_empty(&self) -> bool {
        self.inner.borrow().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::ExportFormat;

    /// 队列先入先出：push 顺序 = drain 顺序
    #[gpui_kit::test]
    fn queue_preserves_push_order(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let queue = ResultActionQueue::new();
        assert!(queue.is_empty());
        queue.push(ResultRequest::FetchMore);
        queue.push(ResultRequest::ToggleTranspose);
        queue.push(ResultRequest::Export {
            format: ExportFormat::Csv,
            all: true,
        });
        assert!(!queue.is_empty());
        let drained = queue.drain();
        assert_eq!(
            drained,
            vec![
                ResultRequest::FetchMore,
                ResultRequest::ToggleTranspose,
                ResultRequest::Export {
                    format: ExportFormat::Csv,
                    all: true,
                },
            ]
        );
        // drain 后队列清空
        assert!(queue.is_empty());
        assert!(queue.drain().is_empty());
    }

    /// 多次 drain 之间可继续 push（宿主分派一批后，新动作能进下一批）
    #[gpui_kit::test]
    fn queue_accepts_push_after_drain(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let queue = ResultActionQueue::new();
        queue.push(ResultRequest::CopyTsv);
        let first = queue.drain();
        assert_eq!(first, vec![ResultRequest::CopyTsv]);
        queue.push(ResultRequest::Refresh);
        let second = queue.drain();
        assert_eq!(second, vec![ResultRequest::Refresh]);
    }
}
