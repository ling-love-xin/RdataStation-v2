//! 编辑器尺寸常量（**rem 倍率**；视图里用 `rems(...)` 换算）
//!
//! 为什么不复用 workbench 的 `ui.rs`：依赖方向是 `workbench → editor`，editor 不能反向依赖。
//! 因此编辑器自己的结构尺寸在这里登记（架构 §12 #6），跨模块共用的常量将来再上提到 shared。
//!
//! 规则同全仓：**结构尺寸进常量表 → 视图换算引用**；局部间距直接用 Tailwind 尺度方法
//! （`gap_1` / `px_2` …）；视图里不得出现裸 `px(N.)`。
//! 注意 `rems(x)` 的基准是主题字号（默认 16px），与 `gap_1`（=4px）不是一套单位。

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

/// 结果区高度（18rem = 288px）
///
/// 1a 固定高度（可拖拽分栏属 1b）：结果区出现/消失时编辑区高度会跳一下，
/// 但比“按比例分栏”在无结果时不占位更符合“只显示真实内容”。
pub const RESULT_PANE_HEIGHT: f32 = 18.0;

/// 结果区状态行高（1.5rem = 24px，与编辑器状态栏同档）
pub const RESULT_STATUS_BAR_HEIGHT: f32 = 1.5;

/// 结果集标签条高（1.5rem = 24px，与状态行同档；TabBar 的 Small 档就是这个高）
pub const RESULT_TABS_HEIGHT: f32 = 1.5;

/// 结果网格列宽（128px；`Column::width` 只收 `Pixels`，故这里不做 rem 换算）
pub const RESULT_COLUMN_WIDTH: Pixels = px(128.);

/// 结果网格列最小宽（64px）
pub const RESULT_COLUMN_MIN_WIDTH: Pixels = px(64.);

/// 结果网格列名行高（2rem = 32px）
pub const RESULT_HEADER_HEIGHT: f32 = 2.0;

/// 结果区最小高度（防止被压成 0）
pub const RESULT_MIN_HEIGHT: f32 = 4.0;
