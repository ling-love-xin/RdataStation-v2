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
use gpui_kit::assets::IconName as CatalogIcon;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::PanelEvent as BasePanelEvent;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel};
use gpui_kit::component::{ActiveTheme, Icon, IconName};
use gpui_kit::*;

/// 项目选择器（最近 / 全部 / 已移除 + 新建 / 打开），作为中央 tab 组的常驻面板。
pub struct ProjectPickerPanel {
    /// 项目视图宿主（内置 `session`：打开项目后这里转 `Some`，面板自动隐藏）。
    host: project::ui::ProjectUiHost,
    /// 选择器搜索 / 新建 / 删除确认的输入实体（与宿主懒创建同一份）。
    inputs: project::ui::ProjectInputs,
    /// Dock 面板的焦点句柄（`BasePanel: Focusable` 要求）。
    focus_handle: FocusHandle,
    /// 宿主共享状态（欢迎页「新建连接」入口经 `Shared::new_connection` 走 EditorBridge）。
    shared: super::Shared,
}

impl ProjectPickerPanel {
    pub fn new(
        shared: super::Shared,
        host: project::ui::ProjectUiHost,
        inputs: project::ui::ProjectInputs,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            host,
            inputs,
            focus_handle: cx.focus_handle(),
            shared,
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
        // 欢迎横幅（#17：无项目时的三入口）：
        // 新建连接 → EditorBridge；打开项目 → 文件夹选择对话框；最近使用 → 下方列表
        //（TabBar 默认停在「最近」）。横幅之下保留完整选择器（搜索 / 状态筛选 / 列表）。
        let shared = self.shared.clone();
        let host_new = self.host.clone();
        let inputs_new = self.inputs.clone();
        let host_recent = self.host.clone();
        let theme = cx.theme();
        let banner = div()
            .v_flex()
            .items_center()
            .gap_3()
            .pt_10()
            .pb_2()
            .child(
                Icon::new(CatalogIcon::Database)
                    .size(rems(2.5))
                    .text_color(theme.colors.primary),
            )
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.colors.foreground)
                    .child("欢迎使用 RdataStation"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.colors.muted_foreground)
                    .child("新建连接、打开项目，或从最近使用继续"),
            )
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("welcome-new-connection")
                            .primary()
                            .icon(IconName::Plus)
                            .label("新建连接")
                            .on_click(move |_, window, app| {
                                shared.new_connection(window, app);
                            }),
                    )
                    .child(
                        Button::new("welcome-open-project")
                            .secondary()
                            .icon(IconName::FolderOpen)
                            .label("打开项目")
                            .on_click(move |_, window, app| {
                                project::ui::open_folder_dialog(
                                    &host_new,
                                    &inputs_new,
                                    window,
                                    app,
                                );
                            }),
                    )
                    .child(
                        Button::new("welcome-recent")
                            .ghost()
                            .icon(CatalogIcon::Clock)
                            .label("最近使用")
                            .on_click(move |_, _, app| {
                                // 刷新最近列表并停留（TabBar 默认「最近」，重绘即回到最近项目）。
                                project::ui::refresh_picker(&host_recent, app);
                            }),
                    ),
            );

        // 先渲染（可变借用 cx），再挂 id / role——顺序与 property 面板一致。
        let picker = project::ui::render_picker(&self.host, &self.inputs, cx);
        div()
            .v_flex()
            .size_full()
            .min_h_0()
            .child(banner)
            .child(
                picker
                    .id("project-picker-panel")
                    .role(gpui::Role::Group)
                    .test_support(),
            )
            .into_any_element()
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
