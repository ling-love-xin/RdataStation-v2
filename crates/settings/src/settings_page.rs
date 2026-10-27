//! rds-settings — 设置页（gpui-kit `setting` 骨架）。
//!
//! 自研两栏布局（List 导航 / 顶部搜索 / 行卡片 / 命中高亮）已退役：分节导航、
//! 搜索过滤、分组卡片、页面级 reset-all 全部由 `gpui_kit::component::setting`
//! 组件接管（见 `skeleton.rs`）。本文件只保留**壳层**：实体、宿主桥、顶栏、
//! 底栏、落盘失败横幅、焦点 / 键上下文。
//!
//! ## 三条硬约束（改这个文件前先读）
//!
//! 1. **行只能来自登记表**：分节用 `registry::sections()`、行用 `registry::page_rows()`，
//!    页面不自己排顺序、不自己加设置行（见 `docs/architecture/settings/settings-architecture.md` §7.1）。
//! 2. **写只走 `SettingsService::apply_by_key`**（唯一写入者）：页面 / `bridge` 不直接改 model、不落盘。
//! 3. **render 是纯读路径**：不读盘、不写盘、不改状态；副作用只在事件路径。
//!
//! 「缓存管理…」「查看日志…」等是**动作行**（不是设置项：没有 key / 默认值 / 生效方式），
//! 由 `skeleton.rs` 按节硬编码，副作用经宿主桥 `SettingsHost` 回调。

use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::status_bar::StatusBar;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::skeleton;
// 尺寸常量在**壳层**（单一来源）：workbench 侧经 `crate::ui` 重导，直接依赖 shell 的 crate 走此路径。
use workbench_shell::ui;

/// 宿主桥：页面的副作用由宿主提供。
///
/// 页面**不**依赖 engine / 具体对话框（依赖只向下）：缓存对话框要 engine（rusqlite +
/// 元数据缓存清理），引入它会把数据库依赖拖进视图 crate。
pub struct SettingsHost {
    /// 关闭设置页（宿主把 overlay 关掉并重绘）。
    pub on_close: Rc<dyn Fn(&mut App)>,
    /// 打开「缓存管理」对话框（宿主实现）。
    pub on_open_cache: Rc<dyn Fn(&mut Window, &mut App)>,
    /// 打开日志目录（宿主实现；设置层不依赖 engine / opener，只负责回调）。
    pub on_open_log_dir: Rc<dyn Fn(&mut Window, &mut App)>,
    /// 打开日志查看对话框（宿主实现）。
    pub on_open_log_view: Rc<dyn Fn(&mut Window, &mut App)>,
    /// 打开「类型简称映射」对话框（宿主实现；维护数据库类型 → 简称表）。
    pub on_open_type_short_map: Rc<dyn Fn(&mut Window, &mut App)>,
    /// 任一设置项落盘后通知宿主（workbench 用它把新值装配到 DockSkin 等消费方；
    /// 设置项自身有副作用时走 setter，此回调只负责「外部消费方重应用」）。
    pub on_settings_changed: Rc<dyn Fn(&mut App)>,
}

/// 设置页实体（宿主把本实体放在 overlay 层渲染）。
pub struct SettingsPage {
    host: SettingsHost,
    /// 页面焦点句柄：`Esc` 关闭靠它路由（`key_context("settings")`）。
    focus_handle: FocusHandle,
    /// 首次渲染时把焦点接到页面上（一次性；否则刚打开时按 `Esc` 没反应）。
    focus_pending: bool,
    /// 写后通知本实体重读（重渲染）的闭包；由 `skeleton` 的字段写路径调用。
    notify: Rc<dyn Fn(&mut App)>,
}

impl SettingsPage {
    pub fn new(_window: &mut Window, host: SettingsHost, cx: &mut Context<Self>) -> Self {
        // 通知闭包：任一设置项写入成功后通知页面实体重渲染。
        // 页面不再维护第二份快照——骨架字段在 render 时直接读 global，重渲染即重读。
        let page = cx.entity().downgrade();
        let notify: Rc<dyn Fn(&mut App)> = Rc::new(move |app| {
            if let Some(p) = page.upgrade() {
                p.update(app, |_, cx| cx.notify());
            }
        });
        Self {
            host,
            focus_handle: cx.focus_handle(),
            focus_pending: true,
            notify,
        }
    }

    // ===== 顶栏 / 底栏 =====

    fn render_header(&self, cx: &Context<Self>) -> Div {
        let theme = cx.theme().clone();
        div()
            .h_flex()
            .w_full()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .flex_none()
            .pl_3()
            .pr_2()
            .gap_2()
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
                Button::new("settings-page-close")
                    .ghost()
                    .xsmall()
                    .icon(IconName::CircleX)
                    .on_click({
                        let on_close = self.host.on_close.clone();
                        move |_, _window, app| on_close(app)
                    }),
            )
    }

    fn render_footer(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        StatusBar::new().left(
            div()
                .pl_3()
                .text_xs()
                .text_color(theme.colors.primary_foreground)
                .child("修改即时生效并持久化到 settings.json"),
        )
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        // 打开后接管焦点：否则 `Esc` 要等用户先点一下页面才生效（一次性）。
        if self.focus_pending {
            self.focus_pending = false;
            self.focus_handle.focus(window, cx);
        }
        // 落盘失败：底栏上方给一条危险色提示（否则"改了没存住"是静默的，K2）。
        // 从全局 `last_save_error()` 纯读（成功落盘会清空该槽，见 lib.rs）。
        let notice: Option<Div> = crate::last_save_error().map(|err| {
            div()
                .w_full()
                .flex_none()
                .px_3()
                .py_1()
                .bg(theme.colors.danger)
                .text_xs()
                .text_color(theme.colors.danger_foreground)
                .child(format!("未能写入 settings.json：{err}（本次改动只在本进程生效）"))
        });
        // 骨架构造（纯函数）：节 → 页，行 → 分组项；值读写经闭包接 global。
        let skeleton = skeleton::build(&self.host, &self.notify);
        div()
            .v_flex()
            .w(rems(ui::SETTINGS_PAGE_WIDTH))
            .h(rems(ui::SETTINGS_PAGE_HEIGHT))
            .max_h_full()
            .rounded_lg()
            .bg(theme.colors.popover)
            .border_1()
            .border_color(theme.colors.border)
            .overflow_hidden()
            .child(self.render_header(cx))
            .child(
                div()
                    .w_full()
                    .h(ui::HAIRLINE)
                    .flex_none()
                    .bg(theme.colors.border),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(skeleton),
            )
            .when_some(notice, |d, notice| d.child(notice))
            .child(self.render_footer(cx))
            .key_context("settings")
            .track_focus(&self.focus_handle)
            .id(ElementId::Name("settings-page".into()))
            .role(gpui::Role::Group)
            .debug_selector(|| "settings-page".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use gpui_kit::TestAppContext;

    use super::{SettingsHost, SettingsPage};

    /// 测试用宿主桥：全部回调为空操作（只记录调用，不接真实宿主）。
    fn dummy_host() -> SettingsHost {
        SettingsHost {
            on_close: Rc::new(|_| {}),
            on_open_cache: Rc::new(|_, _| {}),
            on_open_log_dir: Rc::new(|_, _| {}),
            on_open_log_view: Rc::new(|_, _| {}),
            on_open_type_short_map: Rc::new(|_, _| {}),
            on_settings_changed: Rc::new(|_| {}),
        }
    }

    /// 窗口冒烟：设置页整体（含 gpui-kit `setting` 骨架）渲染不 panic。
    ///
    /// 骨架构造与字段闭包在 render 期展开；任何 id 冲突 / 元素嵌套错误
    /// 会在 draw 时暴露，这里连画两帧（让骨架内部状态落定）。
    #[gpui_kit::test]
    fn settings_page_renders_with_skeleton(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        // 字段渲染经 `SettingsService::get` 读全局；先注册默认 Settings。
        cx.update(|cx| crate::SettingsService::init(cx));
        let (_, cx) = cx.add_window_view(|window, cx| SettingsPage::new(window, dummy_host(), cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}
