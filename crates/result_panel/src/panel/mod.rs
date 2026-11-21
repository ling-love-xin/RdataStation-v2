//! 结果面板实体（`ResultPanel`）——`panel.rs`（3338 行）目录拆分后的门面。
//!
//! 拆分（2026-10-29）：一个巨型 `impl ResultPanel` 块按域切成多个 `impl` 块——
//! Rust 允许同一 struct 多 impl 块，跨文件私有方法以 `pub(crate)` 暴露；
//! 本面板无标准 `Render` trait impl（自定义 `render(&self, cx)`，host 手动调），
//! 故不存在 trait impl 不可分块问题。
//!
//! 子域：
//! - `data`：pub 类型（`GridPayload` / `ExportCtx` / `ResultSyncInput` / `ResultPanel`）
//! - `lifecycle`：构造 / 网格注入 / `sync` 投影同步
//! - `render`：主渲染组装
//! - `events`：视图层动作 + 过滤器 API
//! - `sidebars`：文本视图 / 竖条 / 列浏览器 / 右栏 / 右缘 / 展开筛选条渲染
//! - `dialog`：SQL 预览弹层渲染
//! - `tabs`：右栏各页签渲染 + 顶层辅助（`kv_row` / `fmt_stat` / `right_tab_from_index`）
//! - `tests`：全量单元测试（窗口测试仿 host.rs 包一层 `Render` 实体）

pub(crate) mod data;
pub(crate) mod dialog;
pub(crate) mod events;
pub(crate) mod lifecycle;
pub(crate) mod render;
pub(crate) mod sidebars;
pub(crate) mod tabs;
#[cfg(test)]
mod tests;

// —— 对外 pub 面（editor 侧 use result_panel::panel::{ExportCtx, GridPayload, ResultPanel, ResultSyncInput}）——
pub use data::{AnalysisItem, ExportCtx, GridPayload, ResultPanel, ResultSyncInput, SavedFilterSetting};

/// 【第 60 批】面板级键位上下文：挂在面板根元素上（`render` 根 div 的 `.key_context`）。
///
/// 与 [`crate::grid::RESULT_GRID_CONTEXT`] 是**父子**关系：网格层是子（更深，键位优先命中），
/// 面板层是父。文本态下网格整块不渲染、`RESULT_GRID_CONTEXT` 不在焦点路径上，
/// 切表示这类面板级键位只能靠这一层命中（对齐 DBeaver 把 `CMD_SWITCH_PRESENTATION`
/// 挂在 resultset focused context 上：焦点在 resultset 内即可用，与具体呈现器无关）。
pub const RESULT_PANEL_CONTEXT: &str = "editor-result-panel";

// —— crate 内部顶层辅助（tests 以 use super::X 定位；生产编译不引用故整体 cfg(test)）——
#[cfg(test)]
pub(crate) use events::reorder_by;
#[cfg(test)]
pub(crate) use sidebars::{
    clamp_right_sidebar_height, clamp_right_sidebar_width, next_right_edge_state,
    sidebar_width_anim,
};
