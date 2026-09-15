//! 编辑器尺寸常量（**rem 倍率**；视图里用 `rems(...)` 换算）
//!
//! 为什么不复用 workbench 的 `ui.rs`：依赖方向是 `workbench → editor`，editor 不能反向依赖。
//! 因此编辑器自己的结构尺寸在这里登记（架构 §12 #6），跨模块共用的常量将来再上提到 shared。
//!
//! 规则同全仓：**结构尺寸进常量表 → 视图换算引用**；局部间距直接用 Tailwind 尺度方法
//! （`gap_1` / `px_2` …）；视图里不得出现裸 `px(N.)`。
//! 注意 `rems(x)` 的基准是主题字号（默认 16px），与 `gap_1`（=4px）不是一套单位。

/// 编辑器工具栏高（2.25rem = 36px，与面板头同档）
pub const EDITOR_TOOLBAR_HEIGHT: f32 = 2.25;

/// 编辑器状态栏高（1.5rem = 24px，与列表行同档）
pub const EDITOR_STATUS_BAR_HEIGHT: f32 = 1.5;

/// 编辑区最小高度（防止被上方工具栏压成 0）
pub const EDITOR_MIN_HEIGHT: f32 = 6.0;

/// 标签脏点直径（0.375rem = 6px）
pub const DIRTY_DOT_SIZE: f32 = 0.375;

/// 编辑区左右内距（0.5rem = 8px）
pub const EDITOR_BODY_PADDING_X: f32 = 0.5;
