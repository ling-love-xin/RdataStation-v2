//! 资产库（M6）视图尺寸常量。
//!
//! 与 `crates/workbench/src/ui.rs` 的设计值同源（同一套 rem 基准与 Tailwind 对照口径），
//! 但**常量在本 crate 内声明**：视图随能力同 crate（与 `project::ui` 同例），
//! 依赖方向不允许 feature 反向读 workbench 的常量。
//!
//! 值 = 设计倍率（`rems(x)` 的基准是主题字号，默认 16px，见 `rds-ui-spec`）。

/// 面板头高（2.25rem = 36px）。
pub const PANEL_HEADER_HEIGHT: f32 = 2.25;
/// 行高（1.5rem = 24px，与 M4 树 / M5 列表一致）。
pub const ROW_HEIGHT: f32 = 1.5;
/// 小图标尺寸（0.875rem = 14px）。
pub const ICON_SIZE_SM: f32 = 0.875;
/// 空态大图标（3.0rem = 48px）。
pub const ARCHIVE_EMPTY_ICON_SIZE: f32 = 3.0;
/// 徽标高（1.125rem = 18px；小字号 + 上下留白，不撑破行高）。
pub const ARCHIVE_BADGE_HEIGHT: f32 = 1.125;
