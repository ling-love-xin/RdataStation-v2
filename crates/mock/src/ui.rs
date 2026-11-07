//! Mock（M7）视图尺寸常量。
//!
//! 结构尺寸的唯一来源是 `crates/workbench_shell/src/ui.rs`：与外壳语义相同的**重导出**
//! （视图侧仍写 `ui::X`），本 crate 自有的（输入宽 / 列表高上限 / 行号槽宽）在此声明。
//!
//! **为什么补上这个文件**（2026-09-20）：本 crate 此前是唯一「自持视图却不依赖外壳」的特性
//! crate，七个常量各自为政、`mock_view.rs` 也不在 `ui_contract` 的两份扫描清单里——同族的
//! 面板尺寸（行高 / 激活条 / 面板内距）在任一侧调整都会静默错位，且裸尺寸 / 裸色值没有回归
//! 哨兵。口径现与 `analytics_resource/src/ui.rs` / `insight/src/ui.rs` 一致。
//!
//! 值 = 设计倍率（`rems(x)` 的基准是主题字号，默认 16px，见 `rds-ui-spec`；
//! 组件库表格的尺寸是 px 基准，那些常量直接声明 `Pixels`，不做 rem 换算）。

use gpui_kit::component::Size as ComponentSize;
use gpui_kit::{Pixels, px};

// ===== 与外壳共用的结构尺寸（重导出；单一来源 = `workbench_shell::ui`） =====
//
// 清单行行高必须与 M4 导航 / M5 草稿箱的列表行同值（同一份 `ROW_HEIGHT`）；
// 选中行的左侧标识条走共用原语 `workbench_shell::tree::active_bar`（不在本 crate 手搓）。
/// 工具栏行高（26px，与数据导航筛选行同档；对齐原型 30px 行的视觉密度）
pub use workbench_shell::ui::CONTROL_HEIGHT_SM;
/// 固定描边（工具栏底部分隔线）
pub use workbench_shell::ui::HAIRLINE;
/// 面板头高度（36px）
pub use workbench_shell::ui::PANEL_HEADER_HEIGHT;
/// 面板内距（8px）
pub use workbench_shell::ui::PANEL_PADDING;
pub use workbench_shell::ui::ROW_HEIGHT;

// ===== 本 crate 自有尺寸 =====

/// 行数 / 种子输入框宽度（5rem = 80px）
pub const NUM_INPUT_WIDTH: f32 = 5.0;
/// 对话框内输入框宽度（9rem = 144px）
pub const PARAM_INPUT_WIDTH: f32 = 9.0;
/// 字段清单滚动区最大高度（16rem = 256px）
///
/// A 方案（单行列卡片）落地后**不再使用**：列区段（第 2 段）自己有 `overflow_y_scrollbar`，
/// 这里再限 256px 会把紧凑单行省下的高度又还回去；常量保留给历史引用点。
pub const FIELD_LIST_MAX_HEIGHT: f32 = 16.0;
/// 列卡片（紧凑单行）的最小行高（34px：xsmall 按钮 24px + 上下各 5px 呼吸）
pub const COLUMN_ROW_HEIGHT: Pixels = px(34.);
/// 列名条宽（11rem = 176px）：固定宽度是「竖着扫能对齐」的前提，短列名留白换取四段信息同一基线。
///
/// **只放列名**：类型 / 置信度 / 引用徽标另起一段（此前徽标挤在这一格里，10rem 分给列名
/// 只剩约 90px，`customer_email_address` 这种名字一律被截成 `customer_…`）。
pub const COLUMN_NAME_CELL_WIDTH: f32 = 11.0;
/// 生成器按钮格最大宽（9rem = 144px）：超长自定义名截断，宽度让给参数摘要
pub const COLUMN_GENERATOR_CELL_WIDTH: f32 = 9.0;
/// 动作行右侧状态格里的迷你进度条宽度（6rem = 96px：够读出走向，又不和读数抢宽度）
pub const ACTION_STATUS_BAR_WIDTH: f32 = 6.0;
/// 字段区列搜索框宽度（9rem = 144px；与对话框内输入框同档）
pub const COLUMN_FILTER_WIDTH: f32 = 9.0;
/// 新表配置自带的那一列的列名（沿用「＋ 加列」的命名口径 `column_{index}`）
pub const DEFAULT_FIRST_COLUMN_NAME: &str = "column_1";

/// 清单行右端操作区的固定宽度（5rem = 80px：「编辑」+ 图标删除 + 两个间距）
///
/// 行内操作也是悬停才显形，没有占位容器时，行数那一格会跟着按钮的有无左右跳。
pub const ROW_OPS_WIDTH: f32 = 5.0;
/// 详情 tab 预览区最小高度（10rem = 160px）
pub const PREVIEW_MIN_HEIGHT: f32 = 10.0;
/// 集合类参数多行输入的高度（5rem = 80px ≈ 5 行）
pub const COMPLEX_INPUT_HEIGHT: f32 = 5.0;
/// 生成器搜索列表高度（16rem = 256px；143 项靠 `List` 虚拟化 + 自带滚动）
pub const SEARCH_LIST_HEIGHT: f32 = 16.0;

// ===== 表格原语（预览表 = 组件库 `DataTable`） =====
//
// 数据列宽**不在这里登记**：走组件库 `Column` 的默认档（100px）+ 可拖宽（`resizable`）——
// 预览要的正是「看不全就拖」，而手搓版把列宽钉在 9rem 上，值一长就永远看不到。
// 只有行号槽要定宽（组件默认 100px 对 `#` 太宽），取值与结果集网格同档。

/// 行号槽宽（48px；`#` 列，钉在左、不可拖宽 / 移动 / 选中）。
///
/// 与结果集网格同值（`crates/editor/src/ui.rs` 的 `RESULT_ROW_NUMBER_WIDTH`）：
/// 两张表是同一类东西，行号槽宽不一致会在并列对照时显形。
/// 真正的单一来源要等 `editor` 也依赖 `workbench_shell`（它目前不依赖，故此处镜像一份）。
pub const PREVIEW_ROW_NUMBER_WIDTH: Pixels = px(48.);

/// 预览表的**密度档**（组件尺寸，不是 rem 倍率）。
///
/// 与结果集网格同档（`crates/editor/src/ui.rs` 的 `RESULT_TABLE_SIZE`：组件 `XSmall` = 26px）——
/// 两张表是同一类东西，密度不一致会在并列对照时显形；两侧各持一份的原因与行号槽宽相同
/// （`mock` 不依赖 `editor`，依赖方向上也不该为了一个常量去引）。
pub const PREVIEW_TABLE_SIZE: ComponentSize = ComponentSize::XSmall;

/// 预览单元格悬停全文的最大宽度（24rem = 384px）。
///
/// 悬停提示跟着鼠标走，比这更宽就会被窗口边缘截掉；超出的部分**折行**（长 JSON / 长文本
/// 要能从头读到尾，截尾就失去看全的意义）。
pub const PREVIEW_TOOLTIP_MAX_WIDTH: f32 = 24.0;

// ===== 图标尺寸（视图层统一走 rem 常量，不写裸 `px(N.)`） =====

/// 清单行右端状态点图标（0.75rem = 12px）
pub const STATUS_ICON_SIZE: f32 = 0.75;
/// 面板树 / 收件箱图标（1.25rem = 20px）
pub const PANEL_ICON_SIZE: f32 = 1.25;

/// 详情页三段分栏：基本信息段初始高度（100px；resizable_panel 的 size 只收 Pixels，故用 px 常量）
pub const DETAIL_HEADER_HEIGHT: Pixels = px(100.);
/// 基本信息段可缩最小高度（64px：表名行 + 动作行两行，再小就装不下按钮）。
///
/// 下界必须**小于**初始高度（100px）：先前下界与初始值同为 100px，
/// 分隔条往下拖立刻被 `min_h` 卡住，表现为「这条分隔条拖不动」。
pub const DETAIL_HEADER_MIN_HEIGHT: Pixels = px(64.);
/// 基本信息段可拉最大高度（220px；看全表名行 / 动作行 / 落库目标）
pub const DETAIL_HEADER_MAX_HEIGHT: Pixels = px(220.);
/// 预览段初始高度下限（200px；resizable_panel 的 size_range 只收 Pixels）
/// 预览此前没有显式尺寸、吃掉全部剩余高度，表格被撑得过高
pub const DETAIL_PREVIEW_MIN_HEIGHT: Pixels = px(200.);
/// 预览段高度上限（640px；需要看更多行时拖大，默认按面板 `detail_preview_rem` 收住）
pub const DETAIL_PREVIEW_MAX_HEIGHT: Pixels = px(640.);
/// 列区最小高度（120px：只做拖拽下限；高度由 flex 分给剩余，窗口再小也不会溢出）
pub const DETAIL_COLUMNS_MIN_HEIGHT: Pixels = px(120.);

/// 列编辑对话框总宽（840px）
pub const COLUMN_DIALOG_WIDTH: Pixels = px(840.);
/// 列编辑对话框主体高度（620px：固定高度，行 3 两列在内部滚动，
/// 不再由内容把对话框撑高；展开分组 / 切 Tab 只影响块内滚动）
pub const COLUMN_DIALOG_HEIGHT: Pixels = px(620.);
/// 自定义生成器（精调层）对话框总宽（560px：只装一个表单，不需要两栏）
pub const GENERATOR_DIALOG_WIDTH: Pixels = px(560.);
/// 列编辑对话框左栏生成器列表宽（280px）
pub const COLUMN_GENERATOR_LIST_WIDTH: Pixels = px(280.);
/// 列编辑对话框列名输入框宽（200px）
pub const COLUMN_NAME_INPUT_WIDTH: Pixels = px(200.);
/// 状态卡片 tooltip 内容最大宽度（260px）
pub const STATUS_TOOLTIP_MAX_WIDTH: Pixels = px(260.);

// ===== 进度条（中央表头 / 清单行 / 状态栏三处同一套原语） =====

/// 清单行下缘进度线高度（2px：只留一条线，不占行内布局、不与选中底 / 悬停底叠色）
pub const ROW_PROGRESS_LINE_HEIGHT: Pixels = px(2.);
/// 状态栏迷你进度条宽度（64px：够读出走向，又不挤掉连接名 / LSP / 三个开关）
pub const STATUS_BAR_BAR_WIDTH: Pixels = px(64.);

// ===== 「当前编辑列」标识条 =====

/// 标识条宽度（3px）。
///
/// 比导航树的 `TREE_ACTIVE_BAR`（2px）宽一档：树里同时选中多行是常态（列表密度高，
/// 2px 足够），而这里**同时只可能有一列在编辑**——它是唯一焦点，可以更有存在感。
pub const CURRENT_BAR_WIDTH: Pixels = px(3.);
/// 标识条上下内缩（2px）：卡片有圆角，条贴满上下会露出直角。
pub const CURRENT_BAR_INSET: Pixels = px(2.);

// ===== 拆分补齐的形态尺寸 =====
//
// 以下常量原先作为裸 `px(N.)` 手写在 `detail.rs` / `panel::render.rs` /
// `generator_library.rs` 单文件内部；拆成目录后按契约提回本文件单一来源
// （`ui_guard` 的 `no_new_hardcoded_color_or_pixel` 守卫要求视图层不出现裸 px）。

/// chip / 列名条圆角（3px；与 `style::chip` 同档）
pub const CHIP_RADIUS: Pixels = px(3.);
/// 清单行上下的内距（2px）
pub const ROW_CARD_PY: Pixels = px(2.);
/// 列区可拖最大高度（4000px：`resizable_panel` 的 size_range 只收 Pixels）
pub const DETAIL_COLUMNS_MAX_HEIGHT: Pixels = px(4000.);
/// 弹性零基准（`flex_basis(0)` / `size_range(.., 0)` 的零边）
pub const FLEX_BASIS_ZERO: Pixels = px(0.);
/// 自定义生成器对话框表单主体高度（420px）
pub const GENERATOR_BODY_HEIGHT: Pixels = px(420.);
/// 生成器下拉菜单宽度（260px）
pub const GENERATOR_MENU_WIDTH: Pixels = px(260.);
/// 生成器下拉菜单最大高度（220px）
pub const GENERATOR_MENU_MAX_HEIGHT: Pixels = px(220.);
/// 生成器列表项内缩（20px）
pub const GENERATOR_PICK_INDENT: Pixels = px(20.);
/// 列编辑对话框列清单宽度下限（220px，与 `column_list_px.clamp` 同一处）
pub const COLUMN_LIST_MIN: f32 = 220.;
/// 列编辑对话框列清单宽度上限（520px）
pub const COLUMN_LIST_MAX: f32 = 520.;
