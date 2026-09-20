//! 资产库（M6）视图尺寸常量。
//!
//! **与外壳同源的那几个改为重导出**（2026-09-20）：尺寸常量的唯一来源是
//! `crates/workbench_shell/src/ui.rs`（只依 gpui-kit，特性 crate 可直接依赖）。此前本文件
//! 自持 `PANEL_HEADER_HEIGHT` / `ROW_HEIGHT` / `ICON_SIZE_SM` / `CONTROL_HEIGHT_SM` /
//! `GROUP_BAR_WIDTH` 五份**同值副本**，任一侧调整就会静默错位（文件头原文也写着
//! 「是否合并到外壳仍未拍板」——现按 `docs/architecture/layout/panels-coupling-plan.md` §9
//! 的 A 步口径合并，与 `insight/src/ui.rs` 同例：**重导出，视图侧 `ui::X` 路径不变**）。
//!
//! 语义与外壳相同的走重导出；**本 crate 自有**的（对话框宽 / 表格列宽 / 列表上限 / 徽标高）
//! 仍在此声明。
//!
//! 值 = 设计倍率（`rems(x)` 的基准是主题字号，默认 16px，见 `rds-ui-spec`）。

// ===== 与外壳共用的结构尺寸（重导出；单一来源 = `workbench_shell::ui`） =====
//
// 为什么重导出而不是再登记一份：面板头 / 行高 / 图标档必须与 M4 导航、M5 草稿箱一致，
// 镜像两份会在任一侧调整时静默错位（连接对话框、洞察、设置都已在用外壳那一份）。
pub use workbench_shell::ui::{
    CONTROL_HEIGHT_SM, ICON_SIZE_SM, NAV_GROUP_BAR_WIDTH as GROUP_BAR_WIDTH, PANEL_HEADER_HEIGHT,
    ROW_HEIGHT,
};

// ===== 本 crate 自有尺寸 =====

/// 空态大图标（3.0rem = 48px）。
pub const ARCHIVE_EMPTY_ICON_SIZE: f32 = 3.0;
/// 徽标高（1.125rem = 18px；小字号 + 上下留白，不撑破行高）。
pub const ARCHIVE_BADGE_HEIGHT: f32 = 1.125;
/// 详情面板的标签列宽（5.5rem = 88px；固定宽 + 值列弹性，避免长短标签抖动布局）。
pub const DETAIL_LABEL_WIDTH: f32 = 5.5;
/// 工具栏行高（2.0rem = 32px；原型 §2.2）。
pub const TOOLBAR_HEIGHT: f32 = 2.0;
/// 归档确认对话框宽（34.0rem = 544px；原型 §7.1——表单型六字段）。
pub const ARCHIVE_DIALOG_WIDTH: f32 = 34.0;
/// 取回（检出）对话框宽（30.0rem = 480px；原型 §7.1——三字段 + 预览）。
pub const CHECKOUT_DIALOG_WIDTH: f32 = 30.0;
/// 草稿选择对话框宽（34.0rem = 544px）。
///
/// 原型 §7.1 只列了五个对话框，本栅复用归档确认那一档：同族的表单 / 列表对话框，
/// 宽度口径一致比另起一个数字更稳。
pub const PICK_DIALOG_WIDTH: f32 = 34.0;
/// 草稿选择列表的最大高度（18.0rem = 288px）：再长就滚动，不靠加高解决。
pub const PICK_LIST_MAX_HEIGHT: f32 = 18.0;
/// 版本历史对话框宽（48.0rem = 768px；原型 §7.1——版本表格多列）。
pub const VERSION_DIALOG_WIDTH: f32 = 48.0;
/// 版本列表最大高度（20.0rem = 320px）：版本行可积上百条，靠滚动而不是加高对话框。
pub const VERSION_LIST_MAX_HEIGHT: f32 = 20.0;
/// 版本表格的列宽（rem）：固定宽 + 差异列弹性，避免长短值抽动列位。
pub const VERSION_COL_VERSION: f32 = 3.5;
/// 见 [`VERSION_COL_VERSION`]。
pub const VERSION_COL_TIME: f32 = 8.0;
/// 见 [`VERSION_COL_VERSION`]。
pub const VERSION_COL_SIZE: f32 = 5.0;
/// 见 [`VERSION_COL_VERSION`]。
pub const VERSION_COL_HASH: f32 = 9.0;
/// 见 [`VERSION_COL_VERSION`]。
pub const VERSION_COL_COPY: f32 = 5.0;
/// 索引修复对话框宽（54.0rem = 864px；原型 §7.1——三分组 + 表格 + 动作列）。
pub const REPAIR_DIALOG_WIDTH: f32 = 54.0;
/// 修复列表最大高度（24.0rem = 384px）：未登记文件可能一次报出几十条，靠滚动。
pub const REPAIR_LIST_MAX_HEIGHT: f32 = 24.0;
/// 回收站对话框宽（40.0rem = 640px；原型 §7.1——列表 + 操作列）。
pub const TRASH_DIALOG_WIDTH: f32 = 40.0;
/// 回收站列表最大高度（20.0rem = 320px）：条目只增不减，靠滚动而不是加高对话框。
pub const TRASH_LIST_MAX_HEIGHT: f32 = 20.0;
/// 回收站表格的列宽（rem）：固定宽 + 原位置列弹性，避免长短路径抽动列位。
pub const TRASH_COL_NAME: f32 = 9.0;
/// 见 [`TRASH_COL_NAME`]。
pub const TRASH_COL_TIME: f32 = 7.5;
/// 见 [`TRASH_COL_NAME`]。
pub const TRASH_COL_SIZE: f32 = 4.0;
/// 见 [`TRASH_COL_NAME`]（两个小按钮：还原 / 永久删除）。
pub const TRASH_COL_ACTION: f32 = 9.0;
/// 标签对话框宽（26.0rem = 416px；表单型小对话框：列表 + 一个输入行）。
pub const TAG_DIALOG_WIDTH: f32 = 26.0;
/// 分组名对话框宽（24.0rem = 384px；只有一个输入框）。
pub const GROUP_DIALOG_WIDTH: f32 = 24.0;
/// 标签列表最大高度（16.0rem = 256px）：标签可能几十个，靠滚动而不是加高对话框。
pub const TAG_LIST_MAX_HEIGHT: f32 = 16.0;
