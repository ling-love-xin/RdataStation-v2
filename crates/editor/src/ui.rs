//! 编辑器尺寸常量（**rem 倍率**；视图里用 `rems(...)` 换算）
//!
//! 为什么不复用 workbench 的 `ui.rs`：依赖方向是 `workbench → editor`，editor 不能反向依赖。
//! 因此编辑器自己的结构尺寸在这里登记（架构 §12 #6），跨模块共用的常量将来再上提到 shared。
//!
//! 规则同全仓：**结构尺寸进常量表 → 视图换算引用**；局部间距直接用 Tailwind 尺度方法
//! （`gap_1` / `px_2` …）；视图里不得出现裸 `px(N.)`。
//! 注意 `rems(x)` 的基准是主题字号（默认 16px），与 `gap_1`（=4px）不是一套单位。

use gpui_kit::component::Size as ComponentSize;
use gpui_kit::{Pixels, px};

/// 固定描边（1px，不随字号缩放）
pub const HAIRLINE: Pixels = px(1.);

/// 编辑器工具栏高（2.25rem = 36px，与面板头同档）
pub const EDITOR_TOOLBAR_HEIGHT: f32 = 2.25;

/// 编辑器状态栏高（1.5rem = 24px，与列表行同档）
pub const EDITOR_STATUS_BAR_HEIGHT: f32 = 1.5;

/// 编辑区最小高度（防止被上方工具栏压成 0）
pub const EDITOR_MIN_HEIGHT: f32 = 6.0;

/// 状态点直径（0.375rem = 6px）：标签脏点与结果集失败点共用一档
pub const STATUS_DOT_SIZE: f32 = 0.375;

/// 结果区默认高度（18rem = 288px；可拖拽后以用户拖到的高度为准）
///
/// 结果区出现/消失时编辑区高度会跳一下，但比“按比例分栏”在无结果时不占位更符合
/// “只显示真实内容”。拖拽改高走组件库的 `ResizablePanel`（不手搓拖动条）。
pub const RESULT_PANE_HEIGHT: f32 = 18.0;

/// 结果区可拖拽的最大高度（48rem = 768px：再高编辑区就没地方了）
pub const RESULT_MAX_HEIGHT: f32 = 48.0;

/// 结果区状态行高（1.5rem = 24px，与编辑器状态栏同档；结果工具栏同高）
pub const RESULT_STATUS_BAR_HEIGHT: f32 = 1.5;

/// 结果集标签条高（1.5rem = 24px，与状态行同档；TabBar 的 Small 档就是这个高）
pub const RESULT_TABS_HEIGHT: f32 = 1.5;

/// 结果工具栏的本地筛选框宽（11.25rem = 180px；原型 §5.5 的 `⌕ 筛选`，够放下十来个字符的词）
pub const RESULT_FILTER_WIDTH: f32 = 11.25;

/// 自定义分析 SQL 输入框高（9rem = 144px：约八行，写一条聚合 SQL 够用）
pub const ANALYSIS_SQL_HEIGHT: f32 = 9.0;

/// 结果网格列宽（128px；`Column::width` 只收 `Pixels`，故这里不做 rem 换算）
pub const RESULT_COLUMN_WIDTH: Pixels = px(128.);

/// 结果网格首列的行号槽宽（48px，原型 §2.4 的固定 `#` 列）
pub const RESULT_ROW_NUMBER_WIDTH: Pixels = px(48.);

/// 结果网格的**密度档**（组件尺寸，不是 rem 倍率）。
///
/// 组件给的表行高只有 26 / 30 / 32 / 40 四档（`XSmall` / `Small` / `Medium`（默认）/ `Large`）；
/// 取 `XSmall` = 26px：这是数据网格应有的密度，也与 Mock 预览表（`crates/mock/src/ui.rs` 的
/// `PREVIEW_TABLE_SIZE`）同档。
///
/// 订正（2026-09-20）：原型稿 §8 写的「行高 22px（V1 AG Grid）」被实现默默落到了组件**默认档**
/// 32px——三方面三个数（稿子 22 / 实现 32 / 预览表 26），而这一档此前根本没有常量。
/// 精确给 22px 虽然可行（`Size::Size(px(22.))`），但组件会按 `size * 0.875` 反推字号（19px），
/// 不划算；所以口径统一到组件的 `XSmall`。
/// 两个表格共用这一档：改这里就是改「表格类组件的行高」这一条口径。
pub const RESULT_TABLE_SIZE: ComponentSize = ComponentSize::XSmall;

/// 结果网格列最小宽（64px）
pub const RESULT_COLUMN_MIN_WIDTH: Pixels = px(64.);

/// 结果网格单元格 / 表头悬停全文的最大宽度（24rem = 384px）。
///
/// 悬停提示跟着鼠标走，比这更宽就会被窗口边缘截掉；超出的部分**折行**（长 JSON / 长文本
/// 要能从头读到尾，截尾就失去看全的意义）。与 Mock 预览表同值同口径
/// （`crates/mock/src/ui.rs` 的 `PREVIEW_TOOLTIP_MAX_WIDTH`）：两张表是同一类东西，
/// 悬停提示的宽度不一致会在并列对照时显形。
pub const RESULT_TOOLTIP_MAX_WIDTH: f32 = 24.0;

/// 结果网格列名行高（2rem = 32px）
pub const RESULT_HEADER_HEIGHT: f32 = 2.0;

/// 表头**类型小标签**的字号倍率（0.6875rem = 11px）
///
/// 比单元格正文（`text_xs` = 12px）再小一档：规格要求它“小字、次级色”，而表头行只有
/// 一行高度（组件按密度档给 26px），两行排不下——所以做成同行内、比列名小一档的标签，
/// 层级靠**字号 + `muted_foreground`**两条分开。
pub const RESULT_HEADER_TYPE_FONT_SIZE: f32 = 0.6875;

/// 结果区最小高度（7rem = 112px：工具栏 24 + 多结果时的标签条 24 + 至少两行网格）
///
/// 拖拽不许把它压得比这更小——再小就只剩一条工具栏，看不出自己看的是哪份结果。
pub const RESULT_MIN_HEIGHT: f32 = 7.0;
