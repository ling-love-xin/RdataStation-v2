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
pub use workbench_shell::ui::ROW_HEIGHT;

// ===== 本 crate 自有尺寸 =====

/// 行数 / 种子输入框宽度（5rem = 80px）
pub const NUM_INPUT_WIDTH: f32 = 5.0;
/// 对话框内输入框宽度（9rem = 144px）
pub const PARAM_INPUT_WIDTH: f32 = 9.0;
/// 字段清单滚动区最大高度（16rem = 256px）
pub const FIELD_LIST_MAX_HEIGHT: f32 = 16.0;
/// 字段区列搜索框宽度（9rem = 144px；与对话框内输入框同档）
pub const COLUMN_FILTER_WIDTH: f32 = 9.0;
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
