//! 设置页尺寸常量（rem 基准 + 固定描边）。
//!
//! 为什么本 crate 自持一份：页面住在 `settings`，而尺寸常量原在 `crates/workbench`（工作台侧），
//! 依赖方向是 `workbench → settings`，**下面不能引用上面**。
//! 订正（2026-09-16）：常量已下沉到外壳 crate `crates/workbench_shell`（只依 gpui-kit），
//! 上述理由**已不成立**——本 crate 可以依赖外壳并直接复用这份常量。自持是现状，合并待拍板
//! （见 `docs/architecture/layout/panels-coupling-plan.md` §9）。
//!
//! 与 workbench `ui.rs` 同名常量**保持同值**，调整需两边同步；
//! `ui_contract` 的尺寸契约扫描覆盖本文件（见 `crates/workbench/tests/ui_contract.rs`）。
//!
//! 数值来源：`docs/architecture/settings/settings-prototype-design.md` §7。

use gpui_kit::*;

/// 弹层宽度（61.25rem = 980px；与「新建数据源连接」同档）
pub const PAGE_WIDTH: f32 = 61.25;
/// 弹层高度（35rem = 560px；固定高——切节 / 加行都不跳高）
pub const PAGE_HEIGHT: f32 = 35.0;
/// 分节导航宽度（12.5rem = 200px；与连接对话框侧栏同宽）
pub const NAV_WIDTH: f32 = 12.5;
/// 行标签列宽（15rem = 240px；再窄中长标签要换行）
pub const LABEL_WIDTH: f32 = 15.0;
/// 设置行最小高度（2.5rem = 40px；带说明行时自动增高）
pub const ROW_MIN_HEIGHT: f32 = 2.5;
/// 列表行高度（1.5rem = 24px；与 workbench `ROW_HEIGHT` 同值）——分节导航行用。
pub const ROW_HEIGHT: f32 = 1.5;
/// 节标题行高度（1.75rem = 28px）
pub const SECTION_HEAD_HEIGHT: f32 = 1.75;
/// 标题行高度（2.25rem = 36px）
pub const HEADER_HEIGHT: f32 = 2.25;
/// 分组卡与控件圆角（0.375rem = 6px；弹层圆角走 `theme.radius`）
pub const CARD_RADIUS: f32 = 0.375;
/// 分组卡内边距（0.75rem = 12px）
pub const CARD_PADDING: f32 = 0.75;

// ===== 固定描边（不随字号缩放） =====

/// 1px 细线（边框 / 分隔线）
pub const HAIRLINE: Pixels = px(1.);
/// 分节导航激活项侧条宽度（2px）
pub const ACTIVE_BAR: Pixels = px(2.);
