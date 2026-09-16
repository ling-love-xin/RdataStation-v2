//! 洞察面板的尺寸常量（M8 专用）。
//!
//! 登记约定（对齐 `crates/workbench_shell/src/ui.rs` 与 `crates/mock/src/mock_view.rs`）：
//! 结构尺寸集中在本文件，视图只引用常量、不写字面量——`px(` 会被 workbench 的 UI
//! 契约测试拦下，本 crate 同守该约定。常量取 **rem 倍率**（`f32`），使用处写
//! `rems(ui::…)`；`rems()` 的基准是主题字号（默认 16px），不是 Tailwind 的 4px。

// ===== M8 专用 =====

/// Tab 条高度（1.75rem = 28px；五项 2 字标签在 17.5rem 面板宽内不溢出）
pub const INSIGHT_TAB_HEIGHT: f32 = 1.75;
/// 直方图条高度（0.5rem = 8px）
pub const INSIGHT_HISTOGRAM_BAR_HEIGHT: f32 = 0.5;
/// 单条占比条高度（0.375rem = 6px；布尔 True 占比等「只有一条」的分布与评分卡四维共用）
pub const INSIGHT_RATIO_BAR_HEIGHT: f32 = 0.375;
/// 样本单元格最大字符数（超出截断，长文本不得撑破面板）
pub const INSIGHT_SAMPLE_MAX_CHARS: usize = 200;
/// 行内小图标（0.875rem = 14px；提示行与质量提示前的图标）
pub const INSIGHT_INLINE_ICON_SIZE: f32 = 0.875;
/// 分区标题字号（0.75rem = 12px，对齐面板内的次级信息层级）
pub const INSIGHT_SECTION_TITLE_FONT: f32 = 0.75;
/// 评分卡总分字号（1.75rem = 28px；四档取色）
pub const INSIGHT_SCORE_FONT: f32 = 1.75;
/// Schema 健康分字号（2.25rem = 36px；结构洞察的头号结论）
pub const INSIGHT_HEALTH_SCORE_FONT: f32 = 2.25;
/// 结构报告每行最多给几个表名下钻热点（再多就改成一条链接列表了）
pub const INSIGHT_SCHEMA_DRILLDOWN_LIMIT: usize = 4;
/// 规则管理对话框宽度（40rem = 640px；三层分组 + 路径 + 错误原文要宽度）
pub const INSIGHT_RULES_DIALOG_WIDTH: f32 = 40.0;
/// 表探查：序号列宽（1.25rem = 20px）
pub const INSIGHT_TABLE_INDEX_WIDTH: f32 = 1.25;
/// 表探查：类型列宽（4.5rem = 72px）
pub const INSIGHT_TABLE_TYPE_WIDTH: f32 = 4.5;
/// 表探查：可空列宽（1.75rem = 28px，「YES / NO」）
pub const INSIGHT_TABLE_FLAG_WIDTH: f32 = 1.75;
/// 表探查：质量列宽（3.25rem = 52px，容得下「95 优秀」）
pub const INSIGHT_TABLE_QUALITY_WIDTH: f32 = 3.25;
/// 版本对比：标签列宽（4rem = 64px，容得下「长度范围」四个中文字）
pub const INSIGHT_DIFF_LABEL_WIDTH: f32 = 4.0;

// ===== 与外壳共用的结构尺寸（重导出）=====
//
// 归宿是 `crates/workbench_shell`（"面板状态 + 视图共用资产"）：面板头 / 行高 / 内距 /
// 空态图标必须与外壳一致，镜像两份会在任一侧调整时静默错位。
// 故这里**重导出**而不是再登记一份——视图侧 `ui::PANEL_HEADER_HEIGHT` 等路径不变。
pub use workbench_shell::ui::{
    DIALOG_ROW_HEIGHT, DIALOG_TAB_BODY_HEIGHT, PANEL_HEADER_HEIGHT, PANEL_PADDING, ROW_HEIGHT,
    SCRATCHPAD_EMPTY_ICON_SIZE as EMPTY_ICON_SIZE,
};
