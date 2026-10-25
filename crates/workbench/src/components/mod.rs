//! 工作台 GPUI 组件（Phase A：数据源连接对话框；M1：项目视图宿主桥；
//! M4：数据库导航宿主桥；M7：Mock 面板宿主桥；M2：日志查看对话框）。

pub mod cache_dialog;
// 连接对话框已下沉 `rds-connection-dialog`（2026-10-15）：此处重导保持
// `crate::components::connection_dialog::*` 路径不变（editor.rs 等引用点零改动）。
pub use connection_dialog::components::connection_dialog;
pub mod group_form_dialog;
pub mod group_manager_dialog;
pub mod tag_manager_dialog;
pub mod saved_view_dialog;
pub mod visible_databases_dialog;
pub mod object_mask_dialog;
pub mod insight_actions;
pub mod log_dialog;
pub mod mock_host;
pub mod nav_host;
pub mod project_host;
pub mod scratchpad_host;
pub mod resource_host;
pub mod type_short_map_dialog;
