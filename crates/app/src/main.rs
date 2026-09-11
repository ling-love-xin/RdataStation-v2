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
//!   全局系统库初始化 → SettingsService::init → 主题目录 watch →
//!   应用已保存主题模式 → 快捷键绑定。

use gpui_kit::component::{Root, Theme, ThemeRegistry, TitleBar};
use gpui_kit::*;
use settings::SettingsService;
use settings::commands::OpenSettings;
use workbench::WorkbenchView;
use workbench::commands::{CloseProject, SwitchProject, ToggleQuickOpen};

fn main() {
    // 注册内置图标资产源：gpui-kit 组件与 IconName 的 SVG 均从 AssetSource 加载，
    // 未注册时所有图标静默渲染为空（元素在但看不到）。
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            // 0. 全局系统库（global.db / analytics.duckdb）：M3 连接、M4 元数据
            //    与工作台列表的共同持久化根，必须在任何 Feature 读取前完成初始化。
            init_global_system();

            gpui_kit::init(cx);

            // 1. 设置：加载 %APPDATA%/RdataStation/settings.json 为 global。
            SettingsService::init(cx);

            // 2. 主题资产：先同步加载目录内主题并接入 `Theme`，保证首帧即为 RDS 配色。
            //    （`watch_dir` 为异步加载，若仅依赖它，窗口创建早于加载完成时会停留在
            //    gpui-kit 默认主题，表现为全局颜色与设计不符。）
            let themes_dir =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes");
            load_theme_assets(&themes_dir, cx);
            attach_rds_theme(cx);

            // 3. 主题目录监听（热更新）：文件变更后重新接入并刷新窗口。
            let _ = ThemeRegistry::watch_dir(themes_dir, cx, |cx| {
                attach_rds_theme(cx);
                let mode = SettingsService::theme_mode(cx);
                Theme::change(mode, None, cx);
                cx.refresh_windows();
            });

            // 4. 应用已保存的主题模式（明 / 暗），与 settings 外观节一致。
            let mode = SettingsService::theme_mode(cx);
            Theme::change(mode, None, cx);

            // 5. 快捷键：Quick Open（Ctrl+P）、设置（Ctrl+,）。
            //    Quick Open 触发后由 workbench 的 key_context("workbench") on_action 处理。
            cx.bind_keys([
                KeyBinding::new("ctrl-p", ToggleQuickOpen, Some("workbench")),
                KeyBinding::new("ctrl-,", OpenSettings, Some("workbench")),
                // M1 项目管理：切换项目（回选择器）/ 关闭项目。
                KeyBinding::new("ctrl-shift-p", SwitchProject, Some("workbench")),
                KeyBinding::new("ctrl-shift-w", CloseProject, Some("workbench")),
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
                    let workspace = cx.new(|cx| WorkbenchView::new(cx));
                    // 窗口第一层必须是 Root
                    cx.new(|cx| Root::new(workspace, window, cx))
                })
                .expect("failed to open window");
            })
            .detach();
        });
}

/// 初始化全局系统库（执行全局迁移 + 建立连接池单例）。
///
/// 运行时全部常驻：sqlx 连接池的后台维护任务依托其存活，不可随初始化结束而销毁。
/// 失败不阻断启动：工作台会以降级模式显示空列表与错误提示。
fn init_global_system() {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("rds-global-db")
            .build()
            .expect("failed to build global-db runtime")
    });
    if let Err(e) = runtime.block_on(engine::migration::initialize_global_system()) {
        // 启动期一次性错误：stderr 供开发/诊断查看，UI 侧由工作台降级提示补充。
        eprintln!("[startup] 全局系统库初始化失败: {e}");
    }
}

/// 同步读取主题目录内的 JSON 资产并注册到 `ThemeRegistry`。
///
/// `watch_dir` 的首次加载是异步的，若仅依赖它，窗口可能在加载完成前创建并以
/// gpui-kit 默认主题渲染；此处先同步注册，保证 RDS 主题立即可用。
fn load_theme_assets(themes_dir: &std::path::Path, cx: &mut App) {
    let registry = ThemeRegistry::global_mut(cx);
    let Ok(entries) = std::fs::read_dir(themes_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if let Err(e) = registry.load_themes_from_str(&content) {
                    eprintln!("[startup] 主题文件解析失败 {}: {e}", path.display());
                }
            }
            Err(e) => eprintln!("[startup] 主题文件读取失败 {}: {e}", path.display()),
        }
    }
}

/// 把 RDS 明暗主题接入全局 `Theme`。
///
/// `Theme::change` 只应用 `light_theme` / `dark_theme` 字段所指配置，而主题目录中
/// 的自定义主题不会自动写入这两个字段，需按名字显式接入；未注册时保持当前主题。
fn attach_rds_theme(cx: &mut App) {
    let (light, dark) = {
        let registry = ThemeRegistry::global(cx);
        (
            registry.themes().get("RDS Light").cloned(),
            registry.themes().get("RDS Dark").cloned(),
        )
    };
    if let (Some(light), Some(dark)) = (light, dark) {
        let theme = cx.global_mut::<Theme>();
        theme.light_theme = light;
        theme.dark_theme = dark;
    }
}
