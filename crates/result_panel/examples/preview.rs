//! 结果集面板的**独立预览窗口**（不依赖 workbench / editor / settings）。
//!
//! 为什么要它：结果集 UI（筛选条 · 结果工具条 · 网格 · 状态栏 · 右侧辅助栏）的调校
//! 主要靠肉眼比对 DBeaver，而整台工作台启动慢、还要过解锁窗与设置装配。这个 example
//! 只装一块面板 + 一份造出来的结果，`cargo run` 直接出窗口，改完重编即可。
//!
//! 装配路径与 `src/panel/tests.rs::panel_harness` 同源（生产入口，不是另造一套）：
//! `ResultPanel::new` → `set_grids` → `sync`（宿主投影入口）→ `render`。
//!
//! 运行：
//! ```powershell
//! cargo run -p rds-result-panel --example preview
//! ```
//!
//! 宿主职责的占位：真实宿主会把 [`ResultActionQueue`] 里的动作分派给执行引擎（重查 /
//! 取段 / 导出 / 打开设置…）。预览里没有引擎，只 drain 后打到 stderr —— 这样能确认
//! 「按钮点得动、请求真的入队」，但数据不会变。

use gpui_kit::component::Root;
use gpui_kit::component::TitleBar;
use gpui_kit::component::table::TableState;
// 与 lib 内同范式：glob 引入 `div` / 尺寸助手 / `AnyView` 等（非测试模块，不会与
// `#[gpui_kit::test]` 的宏冲突）
use gpui_kit::*;
use rds_result_panel::grid::{
    ResultGridDelegate, ResultStatus, ResultToolbar, SelectionStatPrefs,
};
use rds_result_panel::panel::{ExportCtx, GridPayload, ResultPanel, ResultSyncInput};
use rds_result_panel::ports::{ResultActionQueue, ResultRequest};
use rds_result_panel::sets::ResultSetTab;
use rds_result_panel::store::ColumnType;
use workbench_shell::model::ExecChannel;

/// 预览窗口的宿主：只做两件事——把面板渲染出来、把面板发出的动作从队列里取走。
struct PreviewHost {
    panel: Entity<ResultPanel>,
    queue: ResultActionQueue,
    /// 面板动作订阅（`observe` 的守卫：掉了就解订，动作不再被消费）
    _actions_sub: Option<Subscription>,
}

impl PreviewHost {
    fn new(panel: Entity<ResultPanel>, queue: ResultActionQueue, cx: &mut gpui_kit::Context<Self>) -> Self {
        // 真实宿主在同一处 drain + 分派；预览里只对「纯 UI 态」的请求做本地落地
        // （刷新钮箭头菜单），其余只打印——证明动作链没断。
        // 注意：回调里**不要** `cx.notify()`——面板 notify → 宿主 notify → 重渲染 →
        // 面板再 notify 会立刻在同一 effect cycle 里滚成同步递归（实测直接爆栈）。
        let actions_sub = cx.observe(&panel, |host, _, cx| {
            let actions = host.queue.drain();
            if actions.is_empty() {
                return;
            }
            for action in &actions {
                match action {
                    // 自动刷新菜单：只改面板态（真机里定时重跑是宿主的活，预览没有引擎）
                    ResultRequest::SetAutoRefresh {
                        enabled,
                        interval_secs,
                    } => {
                        let (enabled, secs) = (*enabled, *interval_secs);
                        host.panel
                            .update(cx, |p, cx| p.set_auto_refresh(enabled, secs, cx));
                    }
                    ResultRequest::ConfigureAutoRefresh => {
                        eprintln!(
                            "[preview] 「自定义...」需要 AutoRefreshConfigDialog（本仓未实现，见基线 §56）"
                        );
                    }
                    _ => {}
                }
            }
            eprintln!("[preview] 宿主收到 {} 个动作：{actions:?}", actions.len());
        });
        Self {
            panel,
            queue,
            _actions_sub: Some(actions_sub),
        }
    }
}

impl Render for PreviewHost {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        // `ResultPanel` 没有标准 `Render`（自定义 `render(&self, cx)`，宿主手动调）
        self.panel.read(cx).render(cx)
    }
}

/// 造一份结果：8 行 5 列，覆盖数字 / 文本 / NULL / 时间 / JSON 几种渲染档
fn demo_payload() -> GridPayload {
    let columns = vec![
        "id".to_string(),
        "customer".to_string(),
        "amount".to_string(),
        "created_at".to_string(),
        "payload".to_string(),
    ];
    let rows = vec![
        vec!["1".into(), "张三".into(), "1234.50".into(), "2026-11-25 09:12:03".into(), r#"{"k":"v"}"#.into()],
        vec!["2".into(), "李四".into(), "NULL".into(), "2026-11-24 18:40:11".into(), "{}".into()],
        vec!["3".into(), "王五".into(), "99.00".into(), "2026-11-24 11:02:55".into(), "NULL".into()],
        vec!["4".into(), "赵六".into(), "0.00".into(), "2026-11-23 22:31:07".into(), r#"{"a":[1,2,3]}"#.into()],
        vec!["5".into(), "孙七".into(), "-12.30".into(), "2026-11-23 08:15:42".into(), "{}".into()],
        vec!["6".into(), "周八".into(), "88888.88".into(), "2026-11-22 14:09:30".into(), "NULL".into()],
        vec!["7".into(), "吴九".into(), "12.00".into(), "2026-11-21 16:55:18".into(), r#"{"k":"长文本"}"#.into()],
        vec!["8".into(), "郑十".into(), "3.14".into(), "2026-11-20 07:03:59".into(), "{}".into()],
    ];
    GridPayload {
        columns,
        rows,
        column_types: vec![
            Some(ColumnType::parse("bigint")),
            Some(ColumnType::parse("text")),
            Some(ColumnType::parse("numeric(10,2)")),
            Some(ColumnType::parse("timestamp")),
            Some(ColumnType::parse("jsonb")),
        ],
        table: Some("public.orders".to_string()),
        annotations: vec![None; 5],
        // 事实帧 / 本地叠加层属可选加速档，预览不摆
        fact: None,
        local: None,
    }
}

/// 造一份宿主投影（真值：生产里由 editor 的 sync 路径算好后交进来）
fn demo_input() -> ResultSyncInput {
    let sql = "select id, customer, amount, created_at, payload from public.orders order by id".to_string();
    ResultSyncInput {
        tabs: vec![
            ResultSetTab {
                label: "结果 1".to_string(),
                failed: false,
                channel: ExecChannel::Source,
                stale: false,
                pinned: false,
                sql: sql.clone(),
            },
            ResultSetTab {
                label: "结果 2".to_string(),
                failed: false,
                channel: ExecChannel::Accelerated,
                stale: true,
                pinned: true,
                sql: "select count(*) from public.orders".to_string(),
            },
        ],
        active: 0,
        notice: Some("预览窗口：数据是造的，动作用不到执行引擎（只打印到控制台）".to_string()),
        toolbar: Some(ResultToolbar {
            rows: Some(8),
            affected_rows: None,
            failed: false,
            elapsed_ms: Some(1200),
            connection: Some("●P·orders".to_string()),
            lineage: Some("原查询".to_string()),
            filtered: None,
            fact_tier: None,
            fact_bytes: 0,
            sql: Some(sql.clone()),
        }),
        status: Some(ResultStatus {
            total_rows: 8,
            selected_row: Some(3),
            selected_cell: Some("李四".to_string()),
            truncated_hint: None,
            // 「还有下一段」：状态栏摆「取下一段 / 取全部」，总数显示成 `8+`
            has_more: true,
            selected_rows: 1,
            selected_columns: 1,
            selected_cells: 1,
        }),
        grid: Some(demo_payload()),
        empty: String::new(),
        error_card: None,
        script_card: None,
        sql: Some(sql.clone()),
        can_copy: true,
        is_analysis: false,
        editable: false,
        dirty: Vec::new(),
        conflict: Vec::new(),
        has_more: true,
        loading_more: false,
        insight_available: true,
        pending_changes: 0,
        committing: false,
        can_undo: false,
        can_redo: false,
        analysis_items: Vec::new(),
        export_ctx: Some(ExportCtx {
            connection: Some("P·orders".to_string()),
            entry_sql: Some((sql, 0)),
            rows_text: "已抓取 8 行".to_string(),
            has_more: true,
            key: None,
        }),
    }
}

/// 分段开关（爆栈诊断用，正常跑不需要设）：`1` = 空窗口，`2` = 只渲染面板（不订阅），
/// 其余 = 完整宿主。三种形态一次编译、三次运行即可判「问题在框架层还是面板层」。
fn stage() -> u8 {
    std::env::var("PREVIEW_STAGE")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(3)
}

/// 空窗口：只验 `application + Root + TitleBar` 这条框架链
struct Blank;

impl Render for Blank {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        gpui_kit::div().size_full().child("blank")
    }
}

/// 只转发面板渲染、**不**订阅面板：验「面板自身渲染」是否可收敛
struct PanelOnly {
    panel: Entity<ResultPanel>,
}

impl Render for PanelOnly {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        self.panel.read(cx).render(cx)
    }
}

fn main() {
    // Windows 主线程默认 1 MiB 栈，而这条渲染链在 debug 构建下递归很深（与 app 的
    // 现象完全一致，见 `crates/app/src/main.rs:109-125` 的同类注释）：直接在主线程
    // 跑会以 `thread 'main' has overflowed its stack` 崩掉。照 app 的做法把 UI 主循环
    // 放到专用大栈线程；`RDS_UI_STACK_MB` 可覆盖（诊断用）。
    let stack_mb = std::env::var("RDS_UI_STACK_MB")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|mb| *mb >= 16)
        .unwrap_or(64);
    let handle = std::thread::Builder::new()
        .name("rds-preview-main".to_string())
        .stack_size(stack_mb * 1024 * 1024)
        .spawn(run_preview)
        .expect("failed to spawn preview main thread");
    if let Err(e) = handle.join() {
        eprintln!("[preview] 主线程异常退出: {e:?}");
        std::process::exit(1);
    }
}

fn run_preview() {
    gpui_kit::application()
        // 图标 / 字体资产：组件库内置全量（Lucide 图标 + 默认字体）
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            // 语言：与工作台同一套（`shared::i18n` 默认即 Zh，`tr()` 直出中文原文）。
            // 组件库自带文案（Empty / Dialog / Select 占位…）走另一套 rust-i18n，
            // 不设会一直停在 fallback `en`，中文界面里蹦出 "No data"，故一并同步。
            shared::i18n::set_locale(shared::i18n::Locale::Zh);
            gpui_kit::component::set_locale(shared::i18n::Locale::Zh.component_code());
            // 主题：`gpui_kit::init` 已装默认主题；明暗跟随系统外观（预览不接设置服务）
            let mut options = TitleBar::window_options();
            if let Some(titlebar) = options.titlebar.as_mut() {
                titlebar.title = Some("RdataStation · 结果集预览".into());
            }
            let stage = stage();
            eprintln!("[preview] stage={stage}");
            cx.open_window(options, |window, cx| {
                let view: gpui_kit::AnyView = if stage == 1 {
                    cx.new(|_| Blank).into()
                } else {
                    let queue = ResultActionQueue::new();
                    let panel = cx.new(|cx| ResultPanel::new(window, cx, queue.clone()));
                    // 网格与分组页网格：生产里由编辑器构造期建好交进来（面板只持有渲染 / 投影）
                    let grid: Entity<TableState<ResultGridDelegate>> =
                        cx.new(|cx| TableState::new(ResultGridDelegate::default(), window, cx));
                    let group_grid: Entity<TableState<ResultGridDelegate>> = cx.new(|cx| {
                        TableState::new(ResultGridDelegate::empty("尚未分组重查"), window, cx)
                    });
                    // 宿主投影：走生产入口 `sync`（不是逐个字段塞），保证预览与真机同一条路径
                    let input = demo_input();
                    panel.update(cx, |p, cx| {
                        p.set_grids(grid, None, Some(group_grid), window, cx);
                        p.sync(cx, input);
                        // 选中统计三项出厂全关（DBeaver 默认），预览打开「行数 + 单元格数」
                        // 以便看到状态栏那一段的排布
                        p.set_selection_stat_prefs(
                            SelectionStatPrefs {
                                rows: true,
                                columns: false,
                                cells: true,
                            },
                            cx,
                        );
                    });
                    // 队列的宿主侧：拿到面板弱引用，按钮 notify 才能传到宿主
                    panel.read(cx).queue().set_panel(panel.downgrade());
                    if stage == 2 {
                        cx.new(|_| PanelOnly { panel: panel.clone() }).into()
                    } else {
                        let host = cx.new(|cx| PreviewHost::new(panel, queue, cx));
                        host.into()
                    }
                };
                // 窗口第一层必须是 Root（`Root::new` 的第 3 参是 `Context<Root>`，故再包一层 `cx.new`）
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("预览窗口创建失败");
        });
}
