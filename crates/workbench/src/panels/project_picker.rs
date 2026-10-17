//! M1：项目选择器面板（中央 tab 组常驻 tab）。
//!
//! 原设计是无项目时用选择器覆盖中央区（view.rs no_project 分支）；但那样
//! DockArea 在无项目时不挂树，左右 Dock（Mock / 洞察 / 历史面板）整体不可达。
//! Mock 面板「纯生成不受影响」（`docs/architecture/mock/mock-architecture.md`
//! I8「未打开项目也能跑」），无项目时点 Mock 图标必须能打开右 Dock——所以把
//! 选择器迁入 Dock 中央 tab 组：
//! - 无项目：`visible() = true`，装配期激活（打开项目入口 = 中央「项目」tab）；
//! - 有项目：`visible() = false`，Dock 渲染回退第一个可见 tab（编辑器）；
//! - 不可关闭（`closable() = false`）——这是无项目时打开项目的持久入口。

use gpui_kit::EventEmitter;
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::*;

/// 项目选择器（最近 / 全部 / 已移除 + 新建 / 打开），作为中央 tab 组的常驻面板。
pub struct ProjectPickerPanel {
    /// 项目视图宿主（内置 `session`：打开项目后这里转 `Some`，面板自动隐藏）。
    host: project::ui::ProjectUiHost,
    /// 选择器搜索 / 新建 / 删除确认的输入实体（与宿主懒创建同一份）。
    inputs: project::ui::ProjectInputs,
    /// Dock 面板的焦点句柄（`BasePanel: Focusable` 要求）。
    focus_handle: FocusHandle,
}

impl ProjectPickerPanel {
    pub fn new(
        host: project::ui::ProjectUiHost,
        inputs: project::ui::ProjectInputs,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            host,
            inputs,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<BasePanelEvent> for ProjectPickerPanel {}

impl Focusable for ProjectPickerPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ProjectPickerPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 先渲染（可变借用 cx），再挂 id / role——顺序与 property 面板一致。
        let picker = project::ui::render_picker(&self.host, &self.inputs, cx);
        picker
            .id("project-picker-panel")
            .role(gpui::Role::Group)
            .test_support()
    }
}

impl BasePanel for ProjectPickerPanel {
    fn panel_name(&self) -> &'static str {
        "project-picker"
    }

    /// 无项目时显示（打开项目入口），有项目时隐藏——Dock 渲染回退第一个可见 tab。
    fn visible(&self, _cx: &App) -> bool {
        self.host.session.borrow().is_none()
    }

    /// 唯一的无项目入口，不允许关掉。
    fn closable(&self, _cx: &App) -> bool {
        false
    }
}

impl ComponentPanel for ProjectPickerPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        Some("项目".into())
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .child("项目")
    }
}
