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
//!   运行时数据根（TEMP 重定向 + 旧布局迁移）→ 全局系统库初始化 + 日志接线 →
//!   SettingsService::init → 主题目录 watch → 应用已保存主题模式 → 快捷键绑定。
//!
//! 数据根位置见 `docs/architecture/runtime/data-paths.md`：默认 = 可执行文件所在目录
//! （安装目录），`RDS_HOME` 可覆盖；日志见 `docs/architecture/runtime/logging.md`。

use analytics_resource::commands::{
    ClearSearch, DeleteSelected, FocusSearch, RenameSelected, SelectAllRows,
};
use editor::commands::{
    CloseDocument, CopyGridSelection, ExecuteAll, ExecuteSql, FormatDocument, OpenDocument,
    SaveDocument, SaveDocumentAs, ToggleComment, TriggerCompletion,
};
use gpui_kit::component::{Root, Theme, ThemeRegistry, TitleBar};
use gpui_kit::*;
use insight::commands::InsightRefresh;
use settings::SettingsService;
use settings::commands::{CloseSettings, FocusSettingsSearch, OpenSettings};
use workbench::WorkbenchView;
use workbench::commands::{
    CloseProject, DraftNext, DraftPrev, FocusNavSearch, GenerateMock, NavClearSearch, NavCollapse,
    NavDown, NavExpand, NavOpenProperties, NavReorderDown, NavReorderUp, NavUp, QuickOpenLocate,
    SaveConnection, ScratchpadCancelEdit, ScratchpadDelete, ScratchpadDown, ScratchpadNewFile,
    ScratchpadOpen, ScratchpadRename, ScratchpadSelectAll, ScratchpadUp, SwitchProject,
    TestConnection, ToggleQuickOpen,
};

mod assets;

fn main() {
    // 0. panic 落盘：真机 GUI 崩溃（尤其 `0xc0000409` 这类 fastfail）在终端之外什么都不留，
    //    事后只能靠“点了就没了”发梦。先装钩子，后面任何 panic 都有消息 + 回溯可查。
    install_panic_logger();
    // 0. 运行时数据根（设计见 docs/architecture/runtime/data-paths.md）：
    //    把进程的 TEMP / TMP / TMPDIR 指到 <RDS_HOME>/tmp。只改这一处，
    //    所有 `std::env::temp_dir()` 调用点（DuckDB spill / 联邦临时库 / 各处 scratch）
    //    自动落到数据根下，不必逐个改代码。
    //    必须在任何线程 / 运行时启动前调用，所以放在 main 的第一条语句。
    if let Err(e) = paths::install_process_temp_dir() {
        eprintln!("[startup] 临时目录重定向失败，继续用系统临时目录: {e}");
    }
    if let Err(e) = paths::ensure_dirs() {
        eprintln!("[startup] 数据目录创建失败: {e}");
    }
    // 0b. 旧布局（%APPDATA%/RdataStation、%LOCALAPPDATA%/RdataStation、~/.rdatastation）
    //     的数据一次性搬进数据根：只补不盖，一次过（含密钥库，否则存量连接密码解不开）。
    let migration = paths::migrate_legacy_layout();
    if !migration.is_empty() {
        eprintln!("[startup] {}", migration.summary());
    }
    eprintln!(
        "[startup] 数据根 {}（来源：{}）",
        paths::home().display(),
        paths::home_origin().label()
    );

    // Windows 主线程默认 1 MiB 栈，而 GPUI 的视图树构建 / 布局 / 事件派发在 debug
    // 构建下递归较深，会在运行期以 `thread 'main' has overflowed its stack` 崩溃
    // （窗口短暂出现后消失／点入口无反应）。把应用主循环放到专用大栈线程执行。
    //
    // `RDS_UI_STACK_MB` 可覆盖（诊断用）：真机“点一下就崩”时，先用它验证
    // “是不是栈不够”（调大就好 = 深度问题；调大照样崩 = 递归/其它原因），
    // 改一行环境变量即可，不必重编。
    let stack_mb = std::env::var("RDS_UI_STACK_MB")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|mb| *mb >= 16)
        .unwrap_or(64);
    let handle = std::thread::Builder::new()
        .name("rds-app-main".to_string())
        .stack_size(stack_mb * 1024 * 1024)
        .spawn(run_app)
        .expect("failed to spawn app main thread");
    if let Err(e) = handle.join() {
        eprintln!("[startup] 应用主线程异常退出: {e:?}");
        std::process::exit(1);
    }
}

/// 把 panic 消息 + 回溯落到 `<RDS_HOME>/logs/panic-<unix时间>.log`。
///
/// 为何需要：GUI 进程被点着把窗口关掉时，终端里那一屏 panic 很容易随会话一起丢；
/// 而 `0xc0000409`（fastfail）连“进程退出原因”都不给。落一份磁盘日志，
/// 用户只要复现一次，开发者就能直接拿到栈（本仓已有这个口径：真机报错要能附日志）。
///
/// 失败不阻断（best-effort）：写日志本身出错时只往 stderr 说一句，不能因为写日志
/// 而把原本的 panic 信息也弄丢。
fn install_panic_logger() {
    // 保留默认输出（stderr）：终端里仍然能看到熟悉的 panic 段。
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
        let dir = paths::home().join("logs");
        let body = format!(
            "thread: {}\n{info}\n\n{}",
            std::thread::current().name().unwrap_or("<unnamed>"),
            std::backtrace::Backtrace::force_capture(),
        );
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("[panic] 建日志目录失败（只走 stderr）: {e}");
            return;
        }
        if let Err(e) = std::fs::write(dir.join(format!("panic-{secs}.log")), body) {
            eprintln!("[panic] 写 panic 日志失败（只走 stderr）: {e}");
        }
    }));
}

/// 应用主循环（运行于大栈线程，见 `main`）。
fn run_app() {
    // 注册资产源：gpui-kit 组件与 IconName 的 SVG 均从 AssetSource 加载，
    // 未注册时所有图标静默渲染为空（元素在但看不到）。
    // 本仓的 [`assets::AppAssets`] 在内置资产前多查一层**运行时品牌包**
    // （`<RDS_HOME>/icons/db/<type_id>.svg`）：数据库品牌标是厂商注册商标、不随包发布，
    // 用户丢文件即生效，没放就回落到内置通用图标（见 `docs/architecture/ui/db-icons.md`）。
    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(move |cx| {
            // 日志级别变更的出口：设置层与 engine 互不依赖（依赖只向下），装配点在这里。
            // 设置页改级别 → 落盘 + 调这个 sink → 日志系统 reload（即时生效，不用重启）。
            settings::install_log_level_sink(|level| {
                if let Err(e) = engine::reload_log_level(level.as_str()) {
                    eprintln!("[startup] 日志级别切换失败: {e}");
                }
            });
            // 退出前把最后一批日志落库：库侧是"每 100 条或每 1 秒"批量提交，
            // 不等这一下，关窗口前最后 1 秒的记录就没了。
            // `Subscription` **一 drop 就退订**（gpui 语义），所以必须 forget —— 钩子要活到进程结束。
            std::mem::forget(cx.on_app_quit(|_cx| async {
                let _ = engine::flush_logs().await;
            }));

            // 0. 全局系统库（global.db / analytics.duckdb）：M3 连接、M4 元数据
            //    与工作台列表的共同持久化根，必须在任何 Feature 读取前完成初始化。
            init_global_system();

            gpui_kit::init(cx);

            // 1. 设置：加载 <RDS_HOME>/config/settings.json 为 global。
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
            //    A10：编辑器键位绑在 key_context("editor")（面板根元素）上——
            //    只有焦点在工作区编辑面板内才生效，不抢导航树/草稿箱的同名键。
            cx.bind_keys([
                KeyBinding::new("ctrl-p", ToggleQuickOpen, Some("workbench")),
                // 浮层里的 `⌥↵`：在导航树中定位选中的元数据命中（只有元数据行可定位）。
                // 绑在 workbench 而不是浮层自己的 context：浮层不单独占 key_context，
                // 它渲染在工作台根下，焦点链会穿过工作台根到这个 on_action。
                KeyBinding::new("alt-enter", QuickOpenLocate, Some("workbench")),
                KeyBinding::new("ctrl-, ", OpenSettings, Some("workbench")),
                // 设置页内（更深的 key_context 先拿到键）：`Esc` 关闭、`Ctrl+F` 聚焦搜索。
                // 与 workbench 的 `Ctrl+F`（聚焦数据源导航搜索）不冲突——那只在设置页外生效。
                KeyBinding::new("escape", CloseSettings, Some("settings")),
                KeyBinding::new("ctrl-f", FocusSettingsSearch, Some("settings")),
                // A10 编辑器：保存 / 行注释开关 / 关闭当前文档。
                // `Ctrl+F` 不在其中：内核（Input context）已绑 `input::Search`，键位先由内核拿到；
                // 编辑器查找（A11）落地后再定归属，**没实现就不宣传**。
                KeyBinding::new("ctrl-s", SaveDocument, Some("editor")),
                KeyBinding::new("ctrl-/", ToggleComment, Some("editor")),
                KeyBinding::new("ctrl-w", CloseDocument, Some("editor")),
                // A9 文档级：`Ctrl+O` 打开文件 / `Ctrl+Shift+S` 另存为（都弹系统文件对话框，
                // 由 `WorkbenchView` 处理）。两键内核都没占用（已核对 `input/base/state.rs`；
                // `Ctrl+O` 只被组件库的命令面板绑在 `Command` context 上，与本 context 不冲突）。
                KeyBinding::new("ctrl-o", OpenDocument, Some("editor")),
                KeyBinding::new("ctrl-shift-s", SaveDocumentAs, Some("editor")),
                // A14 执行：`Ctrl+Enter` = 选区优先 / 否则当前语句；`Ctrl+Shift+Enter` = 全部。
                // 这两个键内核没有占用（已核对 `input/base/state.rs` 的 `init`）。
                KeyBinding::new("ctrl-enter", ExecuteSql, Some("editor")),
                KeyBinding::new("ctrl-shift-enter", ExecuteAll, Some("editor")),
                // B10 格式化：`Ctrl+Shift+F`（A10 表里预留给它的键，已核对内核未占用）
                KeyBinding::new("ctrl-shift-f", FormatDocument, Some("editor")),
                // 【B9 切片二】手动补全：`Ctrl+Space`（内核没占用：grep 过 `input/` 下的 `ctrl-space` /
                // `"space"` 均为空；打字触发那条路由内核自己管，这条只是“再请一次候选”）
                KeyBinding::new("ctrl-space", TriggerCompletion, Some("editor")),
                // 【B14】结果网格里的 `Ctrl+C` = 复制选中的一格 / 一整行。**另一个 context**：
                // 它只挂在结果网格那层元素上，所以编辑区里按 `Ctrl+C` 仍是内核的文本复制
                // ——两个 context 互不覆盖（配对处见 `commands::RESULT_GRID_CONTEXT` 的注释）
                KeyBinding::new(
                    "ctrl-c",
                    CopyGridSelection,
                    Some(editor::commands::RESULT_GRID_CONTEXT),
                ),
                // M8 洞察：`Ctrl+Shift+R` = 重算当前目标的画像。键位绑在 `insight` context 上
                // （面板根元素的 `key_context`），只有焦点在洞察面板内才生效，
                // 不抢其它面板的同名键。
                KeyBinding::new("ctrl-shift-r", InsightRefresh, Some("insight")),
                // M7 Mock：`Ctrl+Enter` = 生成当前草稿（中央「Mock · {表}」tab 的 key_context）。
                // 与编辑器同在 Ctrl+Enter 上不冲突：两者的 context 不同（那里是「执行 SQL」），
                // 且结果表 tab 上该动作无意义（D38：要改就回草稿改完再生成）。
                KeyBinding::new("ctrl-enter", GenerateMock, Some("mock-detail")),
                // A11 查找 / 替换**不注册键位**：`Ctrl+F` / `Ctrl+H` 是编辑器内核自己的能力
                // （`input::Search` / `input::Replace` → 组件库的查找面板），内核在 `Input`
                // context 里先拿到按键，应用层再绑只会重复。焦点不在编辑器内时，
                // `Ctrl+F` 仍落到工作台的“聚焦数据源导航搜索”（另一个 context）。
                // M4 数据源导航：聚焦搜索框（先切到数据源面板并展开左侧 Dock）。
                KeyBinding::new("ctrl-f", FocusNavSearch, Some("workbench")),
                // M4 导航树键盘导航（仅当焦点在导航面板内时生效）。
                KeyBinding::new("up", NavUp, Some("database-nav")),
                KeyBinding::new("down", NavDown, Some("database-nav")),
                KeyBinding::new("right", NavExpand, Some("database-nav")),
                KeyBinding::new("left", NavCollapse, Some("database-nav")),
                KeyBinding::new("f4", NavOpenProperties, Some("database-nav")),
                KeyBinding::new("enter", NavOpenProperties, Some("database-nav")),
                // 清空搜索框（只清搜索词，facet 筛选不动；语义与资产库的 `Esc` 一致）。
                KeyBinding::new("escape", NavClearSearch, Some("database-nav")),
                // 条目重排（对齐 VS Code 的 Alt+↑/↓）：只改顺序，不改光标位置。
                KeyBinding::new("alt-up", NavReorderUp, Some("database-nav")),
                KeyBinding::new("alt-down", NavReorderDown, Some("database-nav")),
                // M5 草稿箱：全选 / 重命名 / 删除 / 取消编辑 / 树内导航（仅当焦点在草稿箱面板内时生效）。
                KeyBinding::new("ctrl-a", ScratchpadSelectAll, Some("scratchpad")),
                KeyBinding::new("f2", ScratchpadRename, Some("scratchpad")),
                KeyBinding::new("delete", ScratchpadDelete, Some("scratchpad")),
                KeyBinding::new("escape", ScratchpadCancelEdit, Some("scratchpad")),
                KeyBinding::new("up", ScratchpadUp, Some("scratchpad")),
                KeyBinding::new("down", ScratchpadDown, Some("scratchpad")),
                KeyBinding::new("enter", ScratchpadOpen, Some("scratchpad")),
                KeyBinding::new("ctrl-n", ScratchpadNewFile, Some("scratchpad")),
                // M6 资产库（仅当焦点在资产库面板内时生效）：`Ctrl+F` 聚焦搜索 /
                // `Esc` 清搜索词 / `Delete` 移入回收站。`Ctrl+F` 与 workbench 的
                // 「聚焦数据源导航搜索」、设置页的同名键均不冲突——各自绑在更深的
                // context 上，只在对应面板内生效（与设置页的注释同一口径）。
                // 行漫游（`↑↓`）与打开（`Enter`）由列表组件自己的选中 / 确认通道处理。
                KeyBinding::new("ctrl-f", FocusSearch, Some("analytics-resource")),
                KeyBinding::new("escape", ClearSearch, Some("analytics-resource")),
                KeyBinding::new("delete", DeleteSelected, Some("analytics-resource")),
                // 全选可见行（多选态；行的多选手势见 `classify_row_click`）。
                KeyBinding::new("ctrl-a", SelectAllRows, Some("analytics-resource")),
                // 重命名显示名（原型 §2.3 / §9）：只改 `project.db` 里的名字，`resources/`
                // 下的文件名与路径不动——想改文件名要先取回再归档（原型 §1 原则 2）。
                KeyBinding::new("f2", RenameSelected, Some("analytics-resource")),
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
                    // E3【窗口退出草稿兜底】：平台关闭路径（点 ✕ / `Alt+F4`）没有任何钩子，
                    // 会话只在 `Ctrl+S` 与「关掉这份文档」时写——不补这一刀，敲了一半的 SQL 直接丢。
                    // 它把**每一份**打开文档的会话落库，然后**返回 true**：兜底是「存了再走」，
                    // 不是拦住不让走（拦人那是「关单个文档」那条路的事，那里有三态确认）。
                    let session_saver = workspace.clone();
                    window.on_window_should_close(cx, move |_window, cx| {
                        session_saver.update(cx, |view, cx| view.save_all_editor_sessions(cx));
                        true
                    });
                    // 窗口第一层必须是 Root
                    cx.new(|cx| Root::new(workspace, window, cx))
                })
                .expect("failed to open window");
            })
            .detach();
        });
}

/// 初始化全局系统库（执行全局迁移 + 建立连接池单例）并接上日志。
///
/// 运行时全部常驻：sqlx 连接池的后台维护任务依托其存活，不可随初始化结束而销毁。
/// 失败不阻断启动：工作台会以降级模式显示空列表与错误提示。
fn init_global_system() {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    /// 日志消费者任务：存住它才叫"应用级"生命周期清晰（丢弃也不会中止任务，
    /// 但留存句柄后将来要做优雅退出时有东西可 abort）。
    static LOG_CONSUMER: std::sync::OnceLock<tokio::task::JoinHandle<()>> =
        std::sync::OnceLock::new();

    let runtime = RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("rds-global-db")
            .build()
            .expect("failed to build global-db runtime")
    });
    let outcome = runtime.block_on(engine::migration::initialize_global_system());
    if let Err(e) = &outcome {
        // 启动期一次性错误：stderr 供开发/诊断查看，UI 侧由工作台降级提示补充。
        eprintln!("[startup] 全局系统库初始化失败: {e}");
    }

    // 日志接线：库层要写全局库的 `app_logs` 表、并起一个异步消费者任务，
    // 所以只能排在全局库建立之后。`runtime.enter()` 给当前线程挂上运行时上下文。
    //
    // 级别/保留期从**设置**里读（此刻 `SettingsService::init` 还没跑——设置页尚未创建，
    // 直接读盘即可；之后用户在设置页改级别时由装配层注册的 sink 走 reload，不走这里）。
    let saved = settings::load_settings();
    let log_config = engine::LogConfig {
        min_level: engine::LogLevel::parse_level(saved.logging.min_level.as_str())
            .unwrap_or(engine::LogLevel::Info),
        ..engine::LogConfig::default()
    };
    let _in_runtime = runtime.enter();
    match engine::init_app_logging(&log_config) {
        Ok(handle) => {
            let _ = LOG_CONSUMER.set(handle);
            // 订阅者是刚挂上的：这之前的日志（含全局库初始化的结果）在这里补记一条，
            // 否则最早的失败只在 stderr，事后翻文件/库都查不到。
            match outcome {
                Ok(()) => tracing::info!("全局系统库初始化完成"),
                Err(e) => tracing::error!(error = %e, "全局系统库初始化失败"),
            }
            tracing::info!(
                data_root = %paths::home().display(),
                origin = paths::home_origin().label(),
                level = log_config.min_level.as_str(),
                "日志系统已启用"
            );
        }
        // 不阻断启动：拿不到订阅者时日志只走 stderr（代码里大量 eprintln! 仍在）。
        Err(e) => eprintln!("[startup] 日志系统未启用（只输出 stderr）: {e}"),
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
