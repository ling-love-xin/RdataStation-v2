//! rds-settings — 设置面板视图。
//!
//! 以居中面板呈现（overlay 由宿主 workbench 提供）：
//! 分节渲染 `外观 / 引擎 / 连接默认值`，外观节含明暗主题切换。
//! 本视图只做展示与触发，所有写操作经 `SettingsService`（即时生效 + 持久化）。

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _, Theme, ThemeMode};
use gpui_kit::*;

use crate::model::Settings;
use crate::SettingsService;

/// 设置面板（宿主把本实体放在 overlay 层渲染）。
pub struct SettingsView {
    settings: Settings,
    /// 关闭回调（由宿主 workbench 提供，关闭后宿主重渲染）。
    on_close: std::rc::Rc<dyn Fn(&mut App)>,
    /// 打开「缓存管理」回调（缓存 UI 依赖 engine，故由宿主 workbench 提供）。
    on_open_cache: std::rc::Rc<dyn Fn(&mut Window, &mut App)>,
}

impl SettingsView {
    pub fn new(
        cx: &mut App,
        on_close: std::rc::Rc<dyn Fn(&mut App)>,
        on_open_cache: std::rc::Rc<dyn Fn(&mut Window, &mut App)>,
    ) -> Self {
        Self {
            settings: SettingsService::get(cx),
            on_close,
            on_open_cache,
        }
    }

    fn section_title(theme: &Theme, title: impl Into<SharedString>) -> Div {
        let title: SharedString = title.into();
        div()
            .h_7()
            .pl_3()
            .pr_3()
            .items_center()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.colors.foreground)
            .child(title)
    }

    fn row(theme: &Theme, label: impl Into<SharedString>, control: impl IntoElement) -> Div {
        let label: SharedString = label.into();
        div()
            .h_flex()
            .h_9()
            .pl_3()
            .pr_3()
            .gap_2()
            .items_center()
            .text_xs()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(theme.colors.muted_foreground)
                    .child(label),
            )
            .child(control)
    }

    fn value_text(&self, theme: &Theme, text: String) -> Div {
        div()
            .text_xs()
            .text_color(theme.colors.secondary_foreground)
            .child(text)
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
                    .size(rems(1.625))
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
                    .size(rems(1.625))
                    .label("深色")
                    .on_click(move |_, window, app| {
                        SettingsService::set_theme_mode(ThemeMode::Dark, Some(window), app);
                        entity_dark.update(app, |_, cx| cx.notify());
                    }),
            )
    }

    fn content(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let engine_dir = if self.settings.engine.workspace_dir.is_empty() {
            "默认位置".to_string()
        } else {
            self.settings.engine.workspace_dir.clone()
        };
        div()
            .v_flex()
            .w_full()
            .gap_1()
            .child(div().h_1p5())
            .child(Self::section_title(theme, "外观"))
            .child(Self::row(theme, "主题模式", self.theme_switcher(cx)))
            .child(Self::row(
                theme,
                "界面语言",
                self.value_text(theme, "中文（简体）".to_string()),
            ))
            .child(div().h_1p5())
            .child(Self::section_title(theme, "数据源导航"))
            .child(Self::row(theme, "来源标识", self.source_code_switcher(cx)))
            .child(Self::row(
                theme,
                "缓存",
                Button::new("settings-open-cache")
                    .ghost()
                    .small()
                    .label("缓存管理…")
                    .on_click({
                        let on_open_cache = self.on_open_cache.clone();
                        move |_, window, app| (on_open_cache)(window, app)
                    }),
            ))
            .child(div().h_1p5())
            .child(Self::section_title(theme, "引擎"))
            .child(Self::row(
                theme,
                "工作区目录",
                self.value_text(theme, engine_dir),
            ))
            .child(div().h_1p5())
            .child(Self::section_title(theme, "连接默认值"))
            .child(Self::row(
                theme,
                "默认数据源",
                self.value_text(
                    theme,
                    self.settings.connection_defaults.default_driver.clone(),
                ),
            ))
            .child(Self::row(
                theme,
                "建连超时",
                self.connect_timeout_switcher(cx),
            ))
            .child(Self::row(theme, "LAN 直连 TLS", self.lan_tls_switcher(cx)))
            .child(Self::row(
                theme,
                "查询超时",
                self.value_text(
                    theme,
                    format!("{} ms", self.settings.connection_defaults.query_timeout_ms),
                ),
            ))
    }

    /// 建连超时预设（毫秒）；超时后会自动重试一次。
    fn connect_timeout_switcher(&self, cx: &Context<Self>) -> Div {
        let current = self.settings.connection_defaults.connect_timeout_ms;
        let presets: [(u64, &'static str, &'static str); 4] = [
            (5_000, "5s", "conn-timeout-5"),
            (15_000, "15s", "conn-timeout-15"),
            (30_000, "30s", "conn-timeout-30"),
            (60_000, "60s", "conn-timeout-60"),
        ];
        let mut row = div().h_flex().gap_1();
        for (ms, label, id) in presets {
            let entity = cx.entity();
            row = row.child(
                Button::new(id)
                    .ghost()
                    .toggled(current == ms)
                    .size(rems(1.625))
                    .label(label)
                    .on_click(move |_, _, app| {
                        SettingsService::set_connect_timeout_ms(ms, app);
                        entity.update(app, |this, cx| {
                            this.settings = SettingsService::get(cx);
                            cx.notify();
                        });
                    }),
            );
        }
        row
    }

    /// LAN / 本机直连是否显式关闭 TLS（公网 / 需要 TLS 时选「保留 TLS」）。
    fn lan_tls_switcher(&self, cx: &Context<Self>) -> Div {
        let on = self.settings.connection_defaults.lan_disable_tls;
        let entity_off = cx.entity();
        let entity_keep = entity_off.clone();
        div()
            .h_flex()
            .gap_1()
            .child(
                Button::new("lan-tls-off")
                    .ghost()
                    .toggled(on)
                    .size(rems(1.625))
                    .label("关 TLS")
                    .on_click(move |_, _, app| {
                        SettingsService::set_lan_disable_tls(true, app);
                        entity_off.update(app, |this, cx| {
                            this.settings = SettingsService::get(cx);
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("lan-tls-keep")
                    .ghost()
                    .toggled(!on)
                    .size(rems(1.625))
                    .label("保留 TLS")
                    .on_click(move |_, _, app| {
                        SettingsService::set_lan_disable_tls(false, app);
                        entity_keep.update(app, |this, cx| {
                            this.settings = SettingsService::get(cx);
                            cx.notify();
                        });
                    }),
            )
    }

    /// 来源标识：短码 `P/G/GP` ⇄ 文字（项目 / 全局 / 共享）。
    fn source_code_switcher(&self, cx: &Context<Self>) -> Div {
        let short = self.settings.navigator.source_short_code;
        let entity_short = cx.entity();
        let entity_text = entity_short.clone();
        div()
            .h_flex()
            .gap_1()
            .child(
                Button::new("source-code-short")
                    .ghost()
                    .toggled(short)
                    .size(rems(1.625))
                    .label("短码")
                    .on_click(move |_, _, app| {
                        SettingsService::set_source_short_code(true, app);
                        entity_short.update(app, |this, cx| {
                            this.settings = SettingsService::get(cx);
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::new("source-code-text")
                    .ghost()
                    .toggled(!short)
                    .size(rems(1.625))
                    .label("文字")
                    .on_click(move |_, _, app| {
                        SettingsService::set_source_short_code(false, app);
                        entity_text.update(app, |this, cx| {
                            this.settings = SettingsService::get(cx);
                            cx.notify();
                        });
                    }),
            )
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div()
            .v_flex()
            .w(rems(35.))
            .max_h_full()
            .rounded_lg()
            .bg(theme.colors.background)
            .border_1()
            .border_color(theme.colors.border)
            .pt_2()
            .pb_2p5()
            .child(
                div()
                    .h_flex()
                    .h_9()
                    .pl_3p5()
                    .pr_3p5()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.colors.foreground)
                            .child("设置"),
                    )
                    .child(Icon::new(IconName::Settings).size_4())
                    .child(
                        Button::new("settings-close")
                            .ghost()
                            .icon(IconName::CircleX)
                            .size_6()
                            .on_click({
                                let on_close = self.on_close.clone();
                                move |_, _, app| (on_close)(app)
                            }),
                    ),
            )
            .child(div().h_px().w_full().bg(theme.colors.border))
            .child(self.content(cx))
            .child(div().h_1())
            .child(
                StatusBar::new().left(
                    div()
                        .pl_3()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("修改即时生效并持久化到 settings.json"),
                ),
            )
    }
}
