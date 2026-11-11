//! 插件 tab：已装插件清单（名称 / 版本 / 状态）+ 刷新 + 启用 / 停用 / 卸载。
//!
//! 数据面 = 全局 `PluginManager`（`plugin::manager`，app 启动时初始化）；会话态清单
//! 由事件路径刷新（render 纯读）：首次 `new` 与「刷新 / 启停 / 卸载」操作后重新拉取。
//! 启停与卸载是异步管理操作（依赖 tokio 上下文），经全局 runtime 句柄在后台执行，不阻塞 UI。
//!
//! 管理页与本面板共用同一份数据：只读快照 + 动作转发都在 `PluginPanel` 上。
//! 文件按职责拆分：`delegate` 行模型、`panel` 状态与动作、`pure` 纯函数、`render` 渲染与面板协议。

mod delegate;
mod panel;
mod pure;
mod render;

pub use panel::PluginPanel;
pub(crate) use pure::uninstall_confirm_description;
pub(super) use pure::{
    bulk_retry_includes, bulk_toggle_includes, contribution_breakdown, event_touches_plugin_list,
    install_receipt, panel_busy, plugin_row_subtitle, plugin_state_label,
};
