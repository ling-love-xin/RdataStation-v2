//! rds-settings — 设置面板视图。
//!
//! 以居中面板呈现（overlay 由宿主 workbench 提供）：
//! 分节渲染 `外观 / 引擎 / 连接默认值`，外观节含明暗主题切换。
//! 本视图只做展示与触发，所有写操作经 `SettingsService`（即时生效 + 持久化）。

use gpui_kit::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme, Icon, IconName, ThemeMode};
use gpui_kit::base::StyledExt;

use crate::model::Settings;
use crate::SettingsService;

/// 设置面板（宿主把本实体放在 overlay 层渲染）。
pub struct SettingsView {
    settings: Settings,
    /// 关闭回调（由宿主 workbench 提供，关闭后宿主重渲染）。
    on_close: std::rc::Rc<dyn Fn(&mut App)>,
}

impl SettingsView {
    pub fn new(cx: &mut App, on_close: std::rc::Rc<dyn Fn(&mut App)>) -> Self {
        Self {
            settings: SettingsService::get(cx),
            on_close,
        }
    }

    fn section_title(title: impl Into<SharedString>) -> Div {
        let title: SharedString = title.into();
        div()
            .h(px(28.))
            .pl(px(12.))
            .pr(px(12.))
            .items_center()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(title_color())
            .child(title)
    }

    fn row(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
        let label: SharedString = label.into();
        div()
            .h_flex()
            .h(px(36.))
            .pl(px(12.))
            .pr(px(12.))
            .gap_2()
            .items_center()
            .text_xs()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(muted_fg())
                    .child(label),
            )
            .child(control)
    }

    fn value_text(&self, text: String) -> Div {
        div().text_xs().text_color(secondary_fg()).child(text)
    }

    fn theme_switcher(&self, cx: &Context<Self>) -> Div {
        let current = self.settings.appearance.theme_mode;
        let entity_light = cx.entity();
        let entity_dark = entity_light.clone();
        div()
            .h_flex()
            .gap_1()
            .child(
                Button::new("theme-light")
                    .ghost()
                    .toggled(current == ThemeMode::Light)
                    .size(px(26.))
                    .label("浅色")
                    .on_click(move |_, window, app| {
                        SettingsService::set_theme_mode(ThemeMode::Light, Some(window), app);
                        entity_light.update(app, |_, cx| cx.notify());
                    }),
            )
            .child(
                Button::new("theme-dark")
                    .ghost()
                    .toggled(current == ThemeMode::Dark)
                    .size(px(26.))
                    .label("深色")
                    .on_click(move |_, window, app| {
                        SettingsService::set_theme_mode(ThemeMode::Dark, Some(window), app);
                        entity_dark.update(app, |_, cx| cx.notify());
                    }),
            )
    }

    fn content(&self, cx: &Context<Self>) -> Div {
        let engine_dir = if self.settings.engine.workspace_dir.is_empty() {
            "默认位置".to_string()
        } else {
            self.settings.engine.workspace_dir.clone()
        };
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .child(div().h(px(6.)))
            .child(Self::section_title("外观"))
            .child(Self::row("主题模式", self.theme_switcher(cx)))
            .child(Self::row("界面语言", self.value_text("中文（简体）".to_string())))
            .child(div().h(px(6.)))
            .child(Self::section_title("引擎"))
            .child(Self::row("工作区目录", self.value_text(engine_dir)))
            .child(div().h(px(6.)))
            .child(Self::section_title("连接默认值"))
            .child(Self::row(
                "默认数据源",
                self.value_text(self.settings.connection_defaults.default_driver.clone()),
            ))
            .child(Self::row(
                "查询超时",
                self.value_text(format!("{} ms", self.settings.connection_defaults.query_timeout_ms)),
            ))
    }
}

fn title_color() -> gpui_kit::Hsla {
    gpui_kit::Hsla::from(gpui_kit::rgb(0x6E747E))
}
fn muted_fg() -> gpui_kit::Hsla {
    gpui_kit::Hsla::from(gpui_kit::rgb(0x8A8F98))
}
fn secondary_fg() -> gpui_kit::Hsla {
    gpui_kit::Hsla::from(gpui_kit::rgb(0xB8BDC6))
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div()
            .v_flex()
            .w(px(560.))
            .max_h_full()
            .rounded_lg()
            .bg(theme.colors.background)
            .border_1()
            .border_color(theme.colors.border)
            .pt(px(8.))
            .pb(px(10.))
            .child(
                div()
                    .h_flex()
                    .h(px(36.))
                    .pl(px(14.))
                    .pr(px(14.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.colors.foreground)
                            .child("设置"),
                    )
                    .child(Icon::new(IconName::Settings).size(px(16.)))
                    .child(
                        Button::new("settings-close")
                            .ghost()
                            .icon(IconName::CircleX)
                            .size(px(24.))
                            .on_click({
                                let on_close = self.on_close.clone();
                                move |_, _, app| (on_close)(app)
                            }),
                    ),
            )
            .child(div().h(px(1.)).w_full().bg(theme.colors.border))
            .child(self.content(cx))
            .child(div().h(px(4.)))
            .child(
                StatusBar::new().left(
                    div()
                        .pl(px(12.))
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("修改即时生效并持久化到 settings.json"),
                ),
            )
    }
}
