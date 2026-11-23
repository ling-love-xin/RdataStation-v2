//! 工作台 GPUI 组件（Phase A：数据源连接对话框；M1：项目视图宿主桥；
//! M4：数据库导航宿主桥；M7：Mock 面板宿主桥；M2：日志查看对话框）。

pub mod cache_dialog;
/// 数据对象清单（面板与来源选择器共用的一层取数与身份规则）
pub mod data_objects;
pub mod data_objects_load;
// 连接对话框已下沉 `rds-connection-dialog`（2026-10-15）：此处重导保持
// `crate::components::connection_dialog::*` 路径不变（editor.rs 等引用点零改动）。
pub use connection_dialog::components::connection_dialog;
pub mod dialog_helpers;
/// 编辑器宿主端口实现（P2-a：`editor::host::EditorHost` 的 workbench 侧实现）
pub mod editor_host;
/// 数据对象文件段的动作宿主（外部文件 → 归档成快照，铁律 D 组）
pub mod file_actions;
/// 数据对象「导出」窗口（目的地 / 格式 / 文件名一次问清；格式不再是工具栏状态）
pub mod export_dialog;
pub mod group_form_dialog;
pub mod group_manager_dialog;
pub mod insight_actions;
pub mod insight_new_dialog;
pub mod log_dialog;
pub mod mock_host;
pub mod nav_host;
/// 数据对象「属性」窗口（身份键 / 通道 / 寿命 / 列结构；右键入口）
pub mod object_props;
pub mod object_mask_dialog;
pub mod project_host;
pub mod resource_host;
pub mod result_set_actions;
pub mod saved_view_dialog;
pub mod scratchpad_host;
/// 来源选择器通用内核（洞察 / 联邦 / 结果集二次分析共用；当前洞察首个实装）
pub mod source_picker;
pub mod tag_assign_dialog;
pub mod tag_manager_dialog;
pub mod type_short_map_dialog;
/// 版本信息对话框（真机验证前核对提交 / 工作树，数据见 `crate::version`）
pub mod version_dialog;
pub mod visible_databases_dialog;
