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
use workbench::commands::{
    CloseProject, DraftNext, DraftPrev, FocusNavSearch, NavCollapse, NavDown, NavExpand,
    NavOpenProperties, NavUp, SaveConnection, ScratchpadCancelEdit, ScratchpadDelete,
    ScratchpadDown, ScratchpadNewFile, ScratchpadOpen, ScratchpadRename, ScratchpadSelectAll,
    ScratchpadUp, SwitchProject, TestConnection, ToggleQuickOpen,
};

fn main() {
    // Windows 主线程默认 1 MiB 栈，而 GPUI 的视图树构建 / 布局 / 事件派发在 debug
    // 构建下递归较深，会在运行期以 `thread 'main' has overflowed its stack` 崩溃
    // （窗口短暂出现后消失／点入口无反应）。把应用主循环放到专用大栈线程执行。
    let handle = std::thread::Builder::new()
        .name("rds-app-main".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(run_app)
        .expect("failed to spawn app main thread");
    if let Err(e) = handle.join() {
        eprintln!("[startup] 应用主线程异常退出: {e:?}");
        std::process::exit(1);
    }
}

/// 应用主循环（运行于大栈线程，见 `main`）。
fn run_app() {
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
            // 2b. 产品语义 token（gpui-kit 固定语义面之外的角色）。
            attach_product_tokens(&themes_dir, cx);

            // 3. 主题目录监听（热更新）：文件变更后重新接入并刷新窗口。
            let themes_dir_watch = themes_dir.clone();
            let _ = ThemeRegistry::watch_dir(themes_dir, cx, move |cx| {
                attach_rds_theme(cx);
                attach_product_tokens(&themes_dir_watch, cx);
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
                // M4 数据源导航：聚焦搜索框（先切到数据源面板并展开左侧 Dock）。
                KeyBinding::new("ctrl-f", FocusNavSearch, Some("workbench")),
                // M4 导航树键盘导航（仅当焦点在导航面板内时生效）。
                KeyBinding::new("up", NavUp, Some("database-nav")),
                KeyBinding::new("down", NavDown, Some("database-nav")),
                KeyBinding::new("right", NavExpand, Some("database-nav")),
                KeyBinding::new("left", NavCollapse, Some("database-nav")),
                KeyBinding::new("f4", NavOpenProperties, Some("database-nav")),
                KeyBinding::new("enter", NavOpenProperties, Some("database-nav")),
                // M5 草稿箱：全选 / 重命名 / 删除 / 取消编辑 / 树内导航（仅当焦点在草稿箱面板内时生效）。
                KeyBinding::new("ctrl-a", ScratchpadSelectAll, Some("scratchpad")),
                KeyBinding::new("f2", ScratchpadRename, Some("scratchpad")),
                KeyBinding::new("delete", ScratchpadDelete, Some("scratchpad")),
                KeyBinding::new("escape", ScratchpadCancelEdit, Some("scratchpad")),
                KeyBinding::new("up", ScratchpadUp, Some("scratchpad")),
                KeyBinding::new("down", ScratchpadDown, Some("scratchpad")),
                KeyBinding::new("enter", ScratchpadOpen, Some("scratchpad")),
                KeyBinding::new("ctrl-n", ScratchpadNewFile, Some("scratchpad")),
                // M1 项目管理：切换项目（回选择器）/ 关闭项目。
                KeyBinding::new("ctrl-shift-p", SwitchProject, Some("workbench")),
                KeyBinding::new("ctrl-shift-w", CloseProject, Some("workbench")),
                // M3 连接对话框（独立于 workbench 的元素层）：
                // 仅在对话框容器的 key_context("connection-dialog") 内生效；
                // secondary-enter 在 Windows/Linux 即 Ctrl+Enter。
                KeyBinding::new("secondary-enter", SaveConnection, Some("connection-dialog")),
                KeyBinding::new("ctrl-t", TestConnection, Some("connection-dialog")),
                KeyBinding::new("up", DraftPrev, Some("connection-dialog")),
                KeyBinding::new("down", DraftNext, Some("connection-dialog")),
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
        // 产品语义 token 不是主题文件（结构不同），由 `attach_product_tokens` 单独加载。
        if path.file_name().and_then(|s| s.to_str()) == Some("product-tokens.json") {
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

/// 加载产品语义 token（`assets/themes/product-tokens.json`）为 GPUI global。
fn attach_product_tokens(themes_dir: &std::path::Path, cx: &mut App) {
    let path = themes_dir.join("product-tokens.json");
    if let Err(e) = settings::product_tokens::apply_from_path(&path, cx) {
        eprintln!("[startup] {e}（回退标准字段）");
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
