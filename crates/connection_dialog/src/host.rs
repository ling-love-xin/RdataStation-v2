//! 连接对话框宿主解耦接口。
//!
//! connection_dialog 不直接依赖 workbench 的 `panels::Shared`（聚合层，依赖整个工作台），
//! 只依赖本 trait 暴露的最小能力：重绘通知 + 当前项目快照 + 两个项目入口请求标记 +
//! 保存成功后的宿主副作用（连接列表 / Mock 刷新 / 状态栏提示）。
//! workbench 的 `Shared` 实现本 trait（见 `rds-workbench` `panels/shared.rs`）。

use gpui_kit::App;
use project::ui::OpenProject;
use workbench_shell::model::ConnectionItem;

/// 连接对话框的宿主能力（由工作台面板状态 `Shared` 实现）。
///
/// 方法与 workbench `Shared` 的对应面一一对应：
/// - [`Self::dialog_redraw`] ← `Shared::notify_host`（经 host_redraw 桥触发宿主重绘）；
/// - [`Self::current_project`] ← `Shared.project`（当前项目会话快照，未打开为 `None`）；
/// - [`Self::request_new_project`] / [`Self::request_open_project`] ← `Shared.project_new_request` /
///   `project_open_request` 置位 + 通知宿主（宿主 render 消费并打开对应入口）。
pub trait ConnectionDialogHost {
    /// 通知宿主重绘（对话框内容挂在宿主 render 上，状态变更后必须调）。
    fn dialog_redraw(&self, cx: &mut App);

    /// 当前项目会话快照（名称 + 根路径；未打开项目为 `None`）。
    fn current_project(&self) -> Option<OpenProject>;

    /// 请求宿主打开「＋ 新增项目」入口（置位标记 + 通知宿主重绘）。
    fn request_new_project(&self, cx: &mut App);

    /// 请求宿主打开「打开现有目录…」入口（置位标记 + 通知宿主重绘）。
    fn request_open_project(&self, cx: &mut App);

    /// 批量保存成功后的宿主副作用（对应 workbench `Shared` 的
    /// `connections` 替换 + `refresh_mock_connections` + `notice` 提示）。
    ///
    /// 对话框不直接写宿主状态，由宿主实现按自身结构落盘。
    fn apply_saved_connections(&self, items: Vec<ConnectionItem>, notice: String, cx: &mut App);
}
