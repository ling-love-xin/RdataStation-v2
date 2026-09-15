//! 洞察面板的尺寸常量（M8 专用）。
//!
//! 登记约定（对齐 `crates/workbench/src/ui.rs` 与 `crates/mock/src/mock_view.rs`）：
//! 结构尺寸集中在本文件，视图只引用常量、不写字面量——`px(` 会被 workbench 的 UI
//! 契约测试拦下，本 crate 同守该约定。常量取 **rem 倍率**（`f32`），使用处写
//! `rems(ui::…)`；`rems()` 的基准是主题字号（默认 16px），不是 Tailwind 的 4px。

// ===== M8 专用 =====

/// Tab 条高度（1.75rem = 28px；五项 2 字标签在 17.5rem 面板宽内不溢出）
pub const INSIGHT_TAB_HEIGHT: f32 = 1.75;
/// 直方图条高度（0.5rem = 8px）
pub const INSIGHT_HISTOGRAM_BAR_HEIGHT: f32 = 0.5;
/// 单条占比条高度（0.375rem = 6px；布尔 True 占比等「只有一条」的分布）
pub const INSIGHT_RATIO_BAR_HEIGHT: f32 = 0.375;
/// 样本单元格最大字符数（超出截断，长文本不得撑破面板）
pub const INSIGHT_SAMPLE_MAX_CHARS: usize = 200;
/// 行内小图标（0.875rem = 14px；提示行与质量提示前的图标）
pub const INSIGHT_INLINE_ICON_SIZE: f32 = 0.875;
/// 分区标题字号（0.75rem = 12px，对齐面板内的次级信息层级）
pub const INSIGHT_SECTION_TITLE_FONT: f32 = 0.75;

// ===== 与 workbench 外壳共用的结构尺寸（镜像，见下）=====
//
// `shared` 目前不依赖 gpui-kit，跨 crate 共享尺寸常量还没有归宿：这几个值是面板
// 与外壳必须一致的（否则面板头会与 Dock tab 条错位），故在两侧各登记一份。
// 出现第三处使用方时，再统一上收到一个带 gpui-kit 的公共位置。

/// 面板头高度（2.25rem = 36px；镜像 `workbench::ui::PANEL_HEADER_HEIGHT`）
pub const PANEL_HEADER_HEIGHT: f32 = 2.25;
/// 列表 / 树行高（1.5rem = 24px；镜像 `workbench::ui::ROW_HEIGHT`）
pub const ROW_HEIGHT: f32 = 1.5;
/// 面板内容内距（0.5rem = 8px；镜像 `workbench::ui::PANEL_PADDING`）
pub const PANEL_PADDING: f32 = 0.5;
/// 空态图标尺寸（2.25rem = 36px；镜像 `workbench::ui::SCRATCHPAD_EMPTY_ICON_SIZE`）
pub const EMPTY_ICON_SIZE: f32 = 2.25;
