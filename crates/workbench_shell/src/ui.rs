//! 工作台 UI 尺寸约束（rem 基准 + 固定描边）。
//!
//! 目的：给视图层提供**唯一尺寸来源**，避免各组件各写 `px(N.)` 导致布局漂移
//! （栏宽、栏高、控件高、Quick Open 尺寸等）。
//!
//! 两套基准：
//! - **rem 倍率常量**（`f32`）：结构化尺寸（栏宽栏高、控件高、Quick Open 尺寸）。
//!   样式场景直接用 gpui 原生 `rems(ui::TITLE_BAR_HEIGHT)`；需要 `Pixels` 的 API
//!   （如 `set_dock_size`）用 `cx.theme().font_size * ui::LEFT_DOCK_WIDTH`。
//!   本模块只声明**设计倍率**（数值来源），换算交给 gpui / 主题字号，随字号 / DPI 缩放。
//! - **固定描边**（`Pixels`）：1px 边框、2px 激活条等不应随字号缩放的细线。
//!
//! 间距 / 内距等局部尺寸直接用 gpui 的 Tailwind 尺度方法（`gap_1` / `px_2` / `h_9`），
//! 其取值天然受限，无需登记常量。
//!
//! 边界：本模块只固定**尺寸与间距**；颜色、圆角、字号一律走主题 token
//! （`cx.theme().colors.*`、`theme.radius`），不在此定义。
//! 规范与数值来源见 `docs/architecture/ui/ui-design-spec.md`。
//!
//! 使用约定：
//! 1. 新增**结构性尺寸**先在本模块登记倍率，再在视图中换算引用；
//! 2. 视图不得出现与布局结构相关的裸 `px(N.)`；局部间距用 Tailwind 尺度方法；
//! 3. 数值调整只改此处，全工作台同步生效。

use gpui_kit::*;

// ===== 五段布局（rem 基准；注释为 16px 字号下的实际值） =====

/// 标题栏高度（2.25rem ≈ 36px）
pub const TITLE_BAR_HEIGHT: f32 = 2.25;
/// 活动栏宽度，左右一致（3rem = 48px）
pub const ACTIVITY_BAR_WIDTH: f32 = 3.0;

// ===== 活动栏（rem 基准） =====

/// 活动栏图标按钮尺寸（1.75rem ≈ 28px）
pub const ACTIVITY_ICON_SIZE: f32 = 1.75;
/// 活动栏单项宽（2.75rem ≈ 44px）
pub const ACTIVITY_ITEM_WIDTH: f32 = 2.75;
/// 活动栏单项高（2.5rem = 40px）
pub const ACTIVITY_ITEM_HEIGHT: f32 = 2.5;

// ===== Dock 边栏（rem 基准） =====

/// 左侧边栏起步宽度（15rem = 240px，设计 §2.3）
pub const LEFT_DOCK_WIDTH: f32 = 15.0;
/// 右侧边栏起步宽度（17.5rem = 280px，设计 §2.3）
pub const RIGHT_DOCK_WIDTH: f32 = 17.5;

// ===== 标题栏元素（rem 基准） =====

/// 软件图标尺寸（1.25rem = 20px）
pub const TITLE_LOGO_SIZE: f32 = 1.25;
/// 标题栏左侧内边距（0.625rem = 10px）
pub const TITLE_BAR_PADDING_X: f32 = 0.625;
/// 挖空项目槽高度（1.625rem ≈ 26px）
pub const TITLE_SLOT_HEIGHT: f32 = 1.625;
/// 挖空项目槽水平内边距（0.875rem ≈ 14px）
pub const TITLE_SLOT_PADDING_X: f32 = 0.875;
/// Quick Open 入口宽度（20rem = 320px）
pub const QUICK_OPEN_ENTRY_WIDTH: f32 = 20.0;
/// Quick Open 入口高度（1.625rem ≈ 26px）
pub const QUICK_OPEN_ENTRY_HEIGHT: f32 = 1.625;
/// Quick Open 入口水平内边距（0.625rem = 10px）
pub const QUICK_OPEN_ENTRY_PADDING_X: f32 = 0.625;
/// Quick Open 弹层宽度（35rem = 560px）
pub const QUICK_OPEN_PANEL_WIDTH: f32 = 35.0;
/// Quick Open 弹层距窗口顶部距离（2.625rem ≈ 42px）
pub const QUICK_OPEN_PANEL_TOP: f32 = 2.625;
/// Quick Open 弹层最大高度（26.25rem ≈ 420px）
pub const QUICK_OPEN_PANEL_MAX_HEIGHT: f32 = 26.25;
/// Quick Open 结果区最大高度（21.25rem ≈ 340px）
pub const QUICK_OPEN_LIST_MAX_HEIGHT: f32 = 21.25;
/// Quick Open 分组头行高（1.125rem = 18px）
pub const QUICK_OPEN_GROUP_HEADER_HEIGHT: f32 = 1.125;
/// Quick Open 底部提示行高（1.5rem = 24px）
pub const QUICK_OPEN_HINT_HEIGHT: f32 = 1.5;
/// Quick Open 单组渲染上限（条；超出显示「还有 N 条」）。
pub const QUICK_OPEN_MAX_ROWS_PER_GROUP: usize = 8;
/// Quick Open 总渲染上限（条）。
pub const QUICK_OPEN_MAX_ROWS: usize = 50;
/// Quick Open 内容档（`#`）的行高：两行式（标题行 + snippet 行；2.75rem = 44px）。
///
/// 高度按档切而不是按行：`List` 只量一个样本行并要求同行同高。
pub const QUICK_OPEN_ROW_HEIGHT_FULLTEXT: f32 = 2.75;
/// Quick Open 异步元数据搜索的防抖窗口（毫秒）。
pub const QUICK_OPEN_SEARCH_DEBOUNCE_MS: u64 = 150;

// ===== 列表与树（rem 基准） =====

/// 列表 / 树单行高度（1.5rem = 24px）
pub const ROW_HEIGHT: f32 = 1.5;
/// 树层级缩进步长（0.875rem ≈ 14px）
pub const TREE_INDENT: f32 = 0.875;
/// 树基础左内边距（0.5rem = 8px）
pub const TREE_BASE_PADDING: f32 = 0.5;
/// 导航树类别文件夹（表/视图等）首批渲染条数；超出时显示「加载更多」（非 rem 尺寸，是条目计数）。
pub const NAV_FOLDER_PAGE_SIZE: usize = 200;
/// 导航搜索结果区最多渲染多少条命中（超出只提示“缩小搜索词”）。
pub const NAV_SEARCH_MAX_ROWS: usize = 100;
/// 连接行徽标尺寸（1.125rem ≈ 18px；颜色=状态 / 形状=类型）。
pub const NAV_BADGE_SIZE: f32 = 1.125;
/// 连接行归属域列宽（短码模式；2.4rem ≈ 38px）。
pub const NAV_SCOPE_COL_SHORT: f32 = 2.4;
/// 连接行归属域列宽（文字模式；3.4rem ≈ 54px）。
pub const NAV_SCOPE_COL_TEXT: f32 = 3.4;
/// 连接行尾「加标签」按钮尺寸（1rem = 16px）。
pub const NAV_ADD_TAG_SIZE: f32 = 1.0;
/// 面板头部高度（2.25rem ≈ 36px）
pub const PANEL_HEADER_HEIGHT: f32 = 2.25;

// ===== 控件与图标（rem 基准） =====

/// 小控件高度（搜索框、工具栏控件；1.625rem ≈ 26px）
pub const CONTROL_HEIGHT_SM: f32 = 1.625;
/// 标准控件高度（输入框、按钮；2rem = 32px）
pub const CONTROL_HEIGHT_MD: f32 = 2.0;
/// 小图标尺寸（0.875rem ≈ 14px）
pub const ICON_SIZE_SM: f32 = 0.875;
/// 标准图标尺寸（1rem = 16px）
pub const ICON_SIZE_MD: f32 = 1.0;

// ===== 间距（rem 倍率） =====
//
// 局部间距 / 内距优先用 gpui 的 Tailwind 尺度方法（`gap_1` / `px_2` 等），
// 以下倍率供需要 `Pixels`（如传给 `set_dock_size` 类 API）或需集中调整的场合。

/// 紧凑间距（0.25rem = 4px）
pub const GAP_SM: f32 = 0.25;
/// 常规间距（0.5rem = 8px）
pub const GAP_MD: f32 = 0.5;
/// 宽松间距（0.75rem = 12px）
pub const GAP_LG: f32 = 0.75;
/// 面板内容内边距（0.5rem = 8px）
pub const PANEL_PADDING: f32 = 0.5;

// ===== 固定描边（不随字号缩放） =====

/// 1px 细线（边框 / 分隔线）
pub const HAIRLINE: Pixels = px(1.);
/// 激活项侧条宽度（2px，左栏在左、右栏在右）
pub const ACTIVITY_ACCENT_BAR: Pixels = px(2.);
/// 属性面板最小宽度（13.75rem = 220px，可拖拽下限）
pub const PROPERTY_PANEL_MIN_WIDTH: f32 = 13.75;
/// 属性面板最大宽度（47.5rem = 760px，可拖拽上限）
pub const PROPERTY_PANEL_MAX_WIDTH: f32 = 47.5;
/// 树 / 列表激活项侧条宽度
pub const TREE_ACTIVE_BAR: Pixels = px(2.);
/// 树 / 列表激活项侧条的上下内缩（3px；避免色条贴满整行高）
pub const TREE_ACTIVE_BAR_INSET: Pixels = px(3.);
/// 导航分组头左侧色条宽度（2px；与激活条同宽，语义独立）
pub const NAV_GROUP_BAR_WIDTH: Pixels = px(2.);

// ===== 草稿箱（M5）专用尺寸（rem 基准；登记在此，视图只引用） =====
/// 外部引用 / 回收站区域最大高度（7.5rem = 120px；超出内部滚动，保证草稿树始终有可用高度）
pub const SCRATCHPAD_GROUP_MAX_HEIGHT: f32 = 7.5;
/// 空态大图标尺寸（2.25rem = 36px）
pub const SCRATCHPAD_EMPTY_ICON_SIZE: f32 = 2.25;

// ===== 连接对话框（M3）专用尺寸（rem 基准；登记在此，视图只引用） =====
//
// 对话框是一个独立模态层，尺寸自成一套（比主界面行高略大、控件更高），
// 因此单独登记而不强行套用主界面常量（数值来源：connection-prototype-design §2/§3.1）。

/// 表单行标签列宽（4.25rem = 68px；再宽会显得“标签离输入框太远”）
pub const DIALOG_FORM_LABEL_WIDTH: f32 = 4.25;
/// 对话框行高（1.75rem ≈ 28px：暂存条目 / 分组标题 / 表单行）
pub const DIALOG_ROW_HEIGHT: f32 = 1.75;
/// 对话框「两列行」高度（32.5rem = 520px；左栏类型树 / 右栏 Tab 内容区共用）。
///
/// 两列**等高且行高确定**：夹住行高后，切 Tab / 加草稿 / 目录变化都不会改变对话框高度
/// （设计 §2「布局恒定」）。取值按本工程侧栏自然高（搜索 + 暂存 7.5rem + 标签行 +
/// 类型树全部可见）取整；类型目录增长时类型树内部滚动，不再撑高对话框。
pub const DIALOG_BODY_HEIGHT: f32 = 32.5;
/// Tab 内容区高度（20.5rem ≈ 328px；**保留作为设计参考值与其它面板的尺寸来源**）。
///
/// 注：连接对话框两列等高改造后，右列实际高度由 [`DIALOG_BODY_HEIGHT`]（两列行高）决定，
/// 本常量在该对话框内不再引用（`insight` 的对比视图仍在用）。
pub const DIALOG_TAB_BODY_HEIGHT: f32 = 20.5;
/// 暂存列表固定高度（7.5rem = 120px；超出内部滚动）
pub const DIALOG_STAGING_HEIGHT: f32 = 7.5;
/// Header 驱动下拉定宽（11rem = 176px）
pub const DIALOG_DRIVER_WIDTH: f32 = 11.0;
/// Header 项目栏定宽（17rem = 272px）
pub const DIALOG_PROJECT_WIDTH: f32 = 17.0;
/// Header 类型徽标宽（1.75rem ≈ 28px，仅图标）
pub const DIALOG_BADGE_WIDTH: f32 = 1.75;
/// Header 类型徽标高（1.5rem = 24px）
pub const DIALOG_BADGE_HEIGHT: f32 = 1.5;
/// 作用域分段项高（1.25rem = 20px）——**已被 `TabBar::segmented` 组件尺寸档取代**（决策 #84）：
/// 保留常量供原型文档 / 契约测试对照，视图不再引用。
pub const DIALOG_SEGMENT_ITEM_HEIGHT: f32 = 1.25;
/// 暂存条目状态点直径（0.4375rem = 7px；无类型信息的旧草稿回退显示）
pub const DIALOG_STATUS_DOT_SIZE: f32 = 0.4375;
/// 短码徽标圆角（0.125rem = 2px）；横向内距用 Tailwind 尺度 `px_1()`（契约禁止 `.px(...)` 字面量）
pub const DIALOG_CHIP_RADIUS: f32 = 0.125;

// ===== 日志对话框（日志系统的读路径）专用尺寸 =====
//
// 日志查看是**只读快照 + 手动刷新**（没有自动轮询），尺寸只需要三件事：
// 列表的滞高上限、以及时间 / 级别两列的定宽（定宽才能让消息列对齐）。
// 形态与口径：`docs/architecture/runtime/logging.md`。

/// 日志列表滚动高度上限（22.5rem = 360px；条目少时按内容高度，不撑空）
pub const DIALOG_LOG_LIST_MAX_HEIGHT: f32 = 22.5;
/// 时间列定宽（3.75rem = 60px：`HH:MM:SS`）
pub const DIALOG_LOG_TIME_WIDTH: f32 = 3.75;
/// 级别列定宽（3.25rem = 52px：`ERROR` 最宽，定宽让消息列对齐）
pub const DIALOG_LOG_LEVEL_WIDTH: f32 = 3.25;
/// 模块列定宽（9.75rem = 156px；超长截断，不抢消息列）
pub const DIALOG_LOG_TARGET_WIDTH: f32 = 9.75;

// ===== 设置页（应用级）专用尺寸（rem 基准；登记在此，视图只引用） =====
//
// 设置页是居中模态弹层，两栏 = 分节导航 + 内容区；尺寸自成一套，不与主界面行高混用。
// 数值来源：`docs/architecture/settings/settings-prototype-design.md` §7。

/// 弹层宽度（61.25rem = 980px；与「新建数据源连接」同档）
pub const SETTINGS_PAGE_WIDTH: f32 = 61.25;
/// 弹层高度（35rem = 560px；**固定高**——切节 / 加行都不跳高）
pub const SETTINGS_PAGE_HEIGHT: f32 = 35.0;
/// 分节导航宽度（12.5rem = 200px；与连接对话框侧栏同宽）
pub const SETTINGS_NAV_WIDTH: f32 = 12.5;
/// 设置行标签列宽（15rem = 240px；再窄中长标签要换行）
pub const SETTINGS_LABEL_WIDTH: f32 = 15.0;
/// 设置行最小高度（2.5rem = 40px；带说明行时自动增高）
pub const SETTINGS_ROW_MIN_HEIGHT: f32 = 2.5;
/// 节标题行高度（1.75rem = 28px）
pub const SETTINGS_SECTION_HEAD_HEIGHT: f32 = 1.75;
/// 分组卡与控件圆角（0.375rem = 6px；弹层圆角走 `theme.radius`）
pub const SETTINGS_CARD_RADIUS: f32 = 0.375;
/// 分组卡内边距（0.75rem = 12px）
pub const SETTINGS_CARD_PADDING: f32 = 0.75;
