//! 洞察面板的尺寸常量（M8 专用）。
//!
//! 登记约定（对齐 `crates/workbench_shell/src/ui.rs` 与 `crates/mock/src/mock_view.rs`）：
//! 结构尺寸集中在本文件，视图只引用常量、不写字面量——`px(` 会被 workbench 的 UI
//! 契约测试拦下，本 crate 同守该约定。常量取 **rem 倍率**（`f32`），使用处写
//! `rems(ui::…)`；`rems()` 的基准是主题字号（默认 16px），不是 Tailwind 的 4px。

// ===== M8 专用 =====

/// 直方图条高度（0.5rem = 8px；**已由 `Progress` 的 `Size::Medium` 档消费**，
/// 保留作设计值登记，与 `view/mod.rs::ratio_bar` 的档位映射注释互证）
pub const INSIGHT_HISTOGRAM_BAR_HEIGHT: f32 = 0.5;
/// 单条占比条高度（0.375rem = 6px；布尔 True 占比等「只有一条」的分布事实条共用。
/// **已由 `Progress` 的 `Size::Small` 档消费**，保留作设计值登记）
pub const INSIGHT_RATIO_BAR_HEIGHT: f32 = 0.375;
/// 样本单元格最大字符数（超出截断，长文本不得撑破面板）
pub const INSIGHT_SAMPLE_MAX_CHARS: usize = 200;
/// 行内小图标（0.875rem = 14px；提示行与事实提示前的图标）
pub const INSIGHT_INLINE_ICON_SIZE: f32 = 0.875;
/// 分区标题字号（0.75rem = 12px，对齐面板内的次级信息层级）
pub const INSIGHT_SECTION_TITLE_FONT: f32 = 0.75;
/// 概览五卡主数值字号（1.0625rem = 17px；原型 `.o-card .v`，纯事实数值无评级语义）
///
/// 2026-11-01 对齐原型时收敛：此前是「基准 28px × 卡内缩放 0.75 = 21px」两段式——
/// 比原型重 4px，且真正上屏的 21px 与登记的 28px 分离，契约因此锁了一个从未被
/// 画出来的数字。原型只有单一 17px，故删掉基准与缩放两个中间量。
pub const INSIGHT_CARD_VALUE_REM: f32 = 1.0625;
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
/// 表探查：缺失率列宽（3.25rem = 52px，容得下「12.3%」）
pub const INSIGHT_TABLE_NULL_RATE_WIDTH: f32 = 3.25;
/// 版本对比：标签列宽（4rem = 64px，容得下「长度范围」四个中文字）
pub const INSIGHT_DIFF_LABEL_WIDTH: f32 = 4.0;
/// 中间工作页对比表：数值列宽（行数 / 列数，4.5rem = 72px）
pub const INSIGHT_WS_NUM_WIDTH: f32 = 4.5;
/// 中间工作页对比表：状态词列宽（3.25rem = 52px，命中 / 未命中 / 已评估等短词）
pub const INSIGHT_WS_STATUS_WIDTH: f32 = 3.25;
/// 中间工作页：数据源列宽（3.5rem = 56px，容得下「来源 MySQL …」截断）
pub const INSIGHT_WS_SOURCE_WIDTH: f32 = 3.5;
/// 中间工作页检查器命中环直径（5.5rem = 88px；经 `ProgressCircle::Size::Size` 消费，
/// 环内数字随主题字号缩放；只呈现适用 / 命中计数，无评级语义）
pub const INSIGHT_WS_RING_SIZE: f32 = 5.5;
/// 中间工作页标题栏超长表名最大宽度（15rem = 240px；VS Code 约束：长标题省略 + hover 提示全名）
pub const INSIGHT_WS_TITLE_MAX_WIDTH: f32 = 15.0;
/// 顶部多标签（A 方案，2026-10-30）单个 tab 的最大宽度（18rem = 288px；表名省略）
pub const INSIGHT_WS_TAB_MAX_WIDTH: f32 = 18.0;
/// 右栏摘要 Tab 检查器命中环直径（4.25rem = 68px；适用 / 命中计数，无评级语义）
pub const INSIGHT_OVERVIEW_RING_SIZE: f32 = 4.25;
/// 列多选 / 列对选择浮层宽度（13.5rem = 216px；Checkbox 行 + 中文列名不换行）
pub const INSIGHT_PICKER_WIDTH: f32 = 13.5;
/// 中间工作页最小宽度（42rem = 672px；对齐原型 `body{min-width:1000px}`）
///
/// 原型不做响应式：窗口不够宽就整体横向滚动，栅格保持设计列数（五卡恒 5 列、
/// 列卡恒双列、对比表恒「指标为行」）。本常量是**中央工作页**那一格的对应值——
/// 原型 1000px 减去 activity 48px 与右 dock 280px 即得 672px。低于它整页横向
/// 滚动，而不是折行或压缩栅格。
pub const INSIGHT_WS_MIN_WIDTH: f32 = 42.0;
/// 右栏「已洞察表」集合列表最大高度（18rem = 288px，约 5 行卡）
///
/// 集合是**会话级管理入口**，全量渲染会把下钻区整块推出首屏——而下钻区才是用户
/// 点进洞察要看的东西。超过本高度列表自身纵向滚动。
pub const INSIGHT_SET_LIST_MAX_HEIGHT: f32 = 18.0;
/// 工具栏当前目标 chip（`来源 · 表名`）最大宽度（14rem = 224px）
///
/// chip 在 36px 单行工具栏里：`flex_none` 不该让它把相邻读数挤出可视区。
/// 内部两段各自 ellipsis（来源次要取窄，表名吃余量）。
pub const INSIGHT_WS_CHIP_MAX_WIDTH: f32 = 14.0;
/// 工具栏目标下拉里的表名最多几个字符（超出左侧省略，hover 提示全名）
///
/// 下拉 label 是 Button 的**纯文本槽**（套不了 `max_w` + `text_ellipsis`，那些是
/// 布局层样式），超长表名会一路撑破工具栏 → 在文本层按字符数截断，hover 给全名。
pub const INSIGHT_WS_TITLE_MAX_CHARS: usize = 24;
/// 转置对比表：指标标签列宽（5rem = 80px；容得下「检查器命中」这类五字行标签）
pub const INSIGHT_WS_METRIC_LABEL_WIDTH: f32 = 5.0;
/// 转置对比表：每张勾选表一列的列宽（6.5rem = 104px；容得下「128,450」）
pub const INSIGHT_WS_METRIC_COL_MIN_WIDTH: f32 = 6.5;
/// 对比结果表「表」列最小宽度（6rem = 96px）
pub const INSIGHT_WS_TABLE_COL_MIN_WIDTH: f32 = 6.0;
/// 对比候选表：勾选框占位列宽（1.25rem = 20px，与表头占位同一常量）
pub const INSIGHT_WS_CHECKBOX_WIDTH: f32 = 1.25;
/// 对比**候选**表最小内容宽度（26.5rem = 424px；横向滚动阈值）
///
/// 同上推导：勾选框 1.25 + 表列 6 + 数据源 3.5 + 行数 4.5 + 列数 4.5 + 状态 3.25
/// = 23rem 刚性列宽，加 6 道间距与左右内距。
pub const INSIGHT_WS_CANDIDATE_MIN_WIDTH: f32 = INSIGHT_WS_CHECKBOX_WIDTH
    + INSIGHT_WS_TABLE_COL_MIN_WIDTH
    + INSIGHT_WS_SOURCE_WIDTH
    + INSIGHT_WS_NUM_WIDTH * 2.0
    + INSIGHT_WS_STATUS_WIDTH
    + COMPARE_TABLE_GAP * 6.0
    + COMPARE_TABLE_PAD * 2.0;

/// 对比结果表列间距（`gap_2` 的 rem 倍率）
const COMPARE_TABLE_GAP: f32 = 0.5;
/// 对比结果表行左右内距（`px_1` 的 rem 倍率）
const COMPARE_TABLE_PAD: f32 = 0.25;

/// 转置对比表（指标为行 / 表为列）的最小内容宽度
///
/// 与结果表不同：它的**列数随勾选表数变化**，阈值无法登记成常量，只能按表数算——
/// 指标标签列 + 每张表一列 + 列间距 + 左右内距。
pub fn metric_table_min_width(table_count: usize) -> f32 {
    let n = table_count as f32;
    INSIGHT_WS_METRIC_LABEL_WIDTH
        + INSIGHT_WS_METRIC_COL_MIN_WIDTH * n
        + COMPARE_TABLE_GAP * n
        + COMPARE_TABLE_PAD * 2.0
}

// ===== 与外壳共用的结构尺寸（重导出）=====
//
// 归宿是 `crates/workbench_shell`（"面板状态 + 视图共用资产"）：面板头 / 行高 / 内距 /
// 空态图标必须与外壳一致，镜像两份会在任一侧调整时静默错位。
// 故这里**重导出**而不是再登记一份——视图侧 `ui::PANEL_HEADER_HEIGHT` 等路径不变。
pub use workbench_shell::ui::{
    DIALOG_ROW_HEIGHT, DIALOG_TAB_BODY_HEIGHT, PANEL_HEADER_HEIGHT, PANEL_PADDING, ROW_HEIGHT,
    SCRATCHPAD_EMPTY_ICON_SIZE as EMPTY_ICON_SIZE,
};
