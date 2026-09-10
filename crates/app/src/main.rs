//! RdataStation v2 应用入口（App Shell）。
//!
//! 按 GPUI-kit 编码规范「初始化与 Root 所有权」：
//! 1. `gpui_kit::application().run` 启动；
//! 2. `app.run` 回调内先 `gpui_kit::init(cx)`，再加载设置 / 主题 / 快捷键；
//! 3. `open_window` 后，窗口第一层必须是 `Root::new(workspace, window, cx)`；
//! 4. App Shell 只组合 Feature crate 装配窗口，不承载业务逻辑；
//! 5. 工作台视图由 `rds-workbench::WorkbenchView` 提供。
//!
//! 启动装配顺序（与 `docs/architecture/settings/settings-crate-design.md` 对齐）：
//!   SettingsService::init → 主题目录 watch → 应用已保存主题模式 → 快捷键绑定。

use gpui_kit::component::{Root, Theme, ThemeRegistry, TitleBar};
use gpui_kit::*;
use settings::commands::OpenSettings;
use settings::SettingsService;
use workbench::commands::ToggleQuickOpen;
use workbench::WorkbenchView;

fn main() {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);

        // 1. 设置：加载 %APPDATA%/RdataStation/settings.json 为 global。
        SettingsService::init(cx);

        // 2. 主题资产目录监听（assets/themes/rds-theme.json，热更新）。
        let themes_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/themes");
        let _ = ThemeRegistry::watch_dir(themes_dir, cx, |_| {});

        // 3. 应用已保存的主题模式（明 / 暗），与 settings 外观节一致。
        let mode = SettingsService::theme_mode(cx);
        Theme::change(mode, None, cx);

        // 4. 快捷键：Quick Open（Ctrl+P）、设置（Ctrl+,）。
        //    Quick Open 触发后由 workbench 的 key_context("workbench") on_action 处理。
        cx.bind_keys([
            KeyBinding::new("ctrl-p", ToggleQuickOpen, Some("workbench")),
            KeyBinding::new("ctrl-,", OpenSettings, Some("workbench")),
        ]);

        cx.spawn(async move |cx| {
            // 自绘标题栏窗口：隐藏系统标题栏（appears_transparent），
            // 拖拽与窗口控件（最小化/最大化/关闭）由 workbench 标题栏的
            // window_control_area 自绘机制接管（Windows/macOS）。
            let mut options = TitleBar::window_options();
            if let Some(titlebar) = options.titlebar.as_mut() {
                titlebar.title = Some("RdataStation".into());
            }
            cx.open_window(options, |window, cx| {
                let workspace = cx.new(|_| WorkbenchView::new());
                // 窗口第一层必须是 Root
                cx.new(|cx| Root::new(workspace, window, cx))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
