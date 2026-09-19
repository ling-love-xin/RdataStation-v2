//! Quick Open 浮层的窗口级测试（GPUI headless 窗口）。
//!
//! 用**简化宿主**（哑端口 + 只挂浮层的 Harness）驱动，不必构造 `WorkbenchView`
//! （那要读项目会话 / 连磁盘 / 起目录监听）；但走的是**生产入口**：
//! 置 `Shared::quick_open` → `on_opened` → 真按键（`simulate_keystrokes`）→
//! 浮层 render → 委托 → 宿主端口。
//!
//! 注意：**不通配导入**（`use gpui_kit::*` / `use super::*` 会把 gpui 的 `test` 宏带进来，
//! 与 `#[gpui_kit::test]` 冲突）。依赖全部显式列举。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
};

use crate::panels::Shared;
use crate::quick_open::model::Action;
use crate::quick_open::palette::{QuickOpenHost, QuickOpenPalette};
use crate::view::ConnectionItem;

/// 哑宿主端口：只记录被执行的动作，不碰工作台。
struct StubHost {
    executed: RefCell<Vec<(Action, bool)>>,
}

impl StubHost {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            executed: RefCell::new(Vec::new()),
        })
    }

    fn taken(&self) -> Vec<(Action, bool)> {
        self.executed.borrow().clone()
    }
}

impl QuickOpenHost for StubHost {
    fn execute(&self, action: Action, keep_open: bool, _window: &mut Window, _cx: &mut App) {
        self.executed.borrow_mut().push((action, keep_open));
    }
}

/// 测试宿主：把浮层挂进元素树（动作要能派发，元素必须真被渲染）。
struct Harness {
    palette: Entity<QuickOpenPalette>,
}

impl Render for Harness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.palette.clone())
    }
}

fn conn(id: &str, driver: &str) -> ConnectionItem {
    ConnectionItem {
        id: id.to_string(),
        name: id.to_string(),
        driver: driver.to_string(),
        connected: true,
        host: None,
        port: None,
        database: None,
        schema: None,
        description: None,
        use_duckdb_fed: false,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// 造一块「已打开」的浮层并渲染一帧（输入框创建 / 落选中 / 聚焦都在这一帧完成）。
///
/// 生产路径是标题栏入口 / `Ctrl+P` → `WorkbenchView::toggle_quick_open` → `on_opened`；
/// 这里直接置权威开关 + 调同一个入口，避免为测试造一个工作台。
fn open_palette(
    cx: &mut TestAppContext,
) -> (
    Rc<StubHost>,
    Shared,
    Entity<Harness>,
    &mut VisualTestContext,
) {
    open_palette_in(cx, None)
}

/// 同上，但带项目根（文件源要真项目目录：草稿箱是项目级能力）。
fn open_palette_in_project(
    cx: &mut TestAppContext,
    project_root: std::path::PathBuf,
) -> (
    Rc<StubHost>,
    Shared,
    Entity<Harness>,
    &mut VisualTestContext,
) {
    open_palette_in(cx, Some(project_root))
}

fn open_palette_in(
    cx: &mut TestAppContext,
    project_root: Option<std::path::PathBuf>,
) -> (
    Rc<StubHost>,
    Shared,
    Entity<Harness>,
    &mut VisualTestContext,
) {
    cx.update(gpui_kit::init);
    let host = StubHost::new();
    let palette_host = host.clone();
    // 两个 ASCII 名的连接：便于用真按键（`s` / `sa`）驱动过滤与漫游
    let shared = Shared::with_connections(
        vec![
            conn("sales", "postgres"),
            conn("sales_archive", "duckdb"),
        ],
        None,
    );
    if let Some(root) = project_root {
        *shared.project.borrow_mut() = Some(project::ui::OpenProject::from_root(root));
    }
    shared.quick_open.set(true);
    let shared_for_palette = shared.clone();
    let (harness, cx) = cx.add_window_view(|_window, cx| {
        let palette = cx
            .new(|cx| QuickOpenPalette::new(shared_for_palette, palette_host, cx));
        palette.update(cx, |palette, cx| palette.on_opened(cx));
        Harness { palette }
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (host, shared, harness, cx)
}

/// 打开即聚焦：真按键要落进输入框（而不是掉在地上）。
#[gpui_kit::test]
fn typing_reaches_the_input_after_open(cx: &mut TestAppContext) {
    let (_host, _shared, harness, cx) = open_palette(cx);

    cx.simulate_keystrokes("s");

    let (query, rows) = cx.update(|_window, cx| {
        let palette = harness.read(cx).palette.read(cx);
        (palette.query_text(cx), palette.row_count(cx))
    });
    assert_eq!(query, "s", "按键必须进输入框（打开即聚焦）");
    assert_eq!(rows, 2, "`s` 应命中两个连接行");
}

/// ↑↓ 漫游 + ↵ 执行：选中按业务键走，确认经宿主端口落地并关面板。
#[gpui_kit::test]
fn arrow_keys_move_selection_and_enter_executes(cx: &mut TestAppContext) {
    let (host, shared, harness, cx) = open_palette(cx);
    cx.simulate_keystrokes("s");

    let first = cx.update(|_window, cx| {
        harness
            .read(cx)
            .palette
            .read(cx)
            .selected_key()
            .expect("打开后应落位到第一行")
    });
    assert!(first.starts_with("conn:0:"), "首行应是 sales：{first}");

    cx.simulate_keystrokes("down");
    let second = cx.update(|_window, cx| {
        harness.read(cx).palette.read(cx).selected_key()
    });
    assert_eq!(
        second.as_deref(),
        Some("conn:1:sales_archive"),
        "↓ 应移到第二行"
    );

    cx.simulate_keystrokes("enter");
    let executed = host.taken();
    assert_eq!(executed.len(), 1, "↵ 应经宿主端口执行一次");
    assert_eq!(executed[0].0, Action::SelectConnection(1));
    assert!(!executed[0].1, "普通 ↵ 不保留面板");
    assert!(
        !shared.quick_open.get(),
        "执行后面板要关（下一次打开是全新的）"
    );
}

/// Esc 关闭（单行 Input 不消费 Esc，会冒泡到浮层根）。
#[gpui_kit::test]
fn escape_closes_the_palette(cx: &mut TestAppContext) {
    let (_host, shared, _harness, cx) = open_palette(cx);
    cx.simulate_keystrokes("s");

    cx.simulate_keystrokes("escape");

    assert!(!shared.quick_open.get(), "Esc 必须关掉浮层");
}

/// 单字符门槛：1 个字符不发元数据搜索，第 2 个字符才发（防全库扫）。
#[gpui_kit::test]
fn metadata_search_starts_only_from_two_chars(cx: &mut TestAppContext) {
    let (_host, _shared, harness, cx) = open_palette(cx);

    cx.simulate_keystrokes("s");
    let (searching, sent) = cx.update(|_window, cx| {
        let palette = harness.read(cx).palette.read(cx);
        (palette.searching(), palette.sent_query())
    });
    assert!(!searching, "单字符不该进「搜索中」");
    assert_eq!(sent, None, "单字符不该发搜索");

    cx.simulate_keystrokes("a");
    let (searching, sent) = cx.update(|_window, cx| {
        let palette = harness.read(cx).palette.read(cx);
        (palette.searching(), palette.sent_query())
    });
    assert!(searching, "两字符应进入「搜索中」");
    assert_eq!(sent.as_deref(), Some("sa"), "发的应是当前词");
}

/// 文件源端到端：打开面板 → 为当前项目排一次扁平清单 → 回填后出「文件」行。
///
/// 这条链路跨了三种时序：宿主端口（项目根）→ `scratchpad::jobs`（**真工作线程 + 真文件系统**）
/// → 浮层的泵（测试时钟驱动）。真线程只能用真实等待配合测试时钟推进，
/// 所以这是本文件里唯一需要 `advance_clock` 的用例。
#[gpui_kit::test]
fn opening_the_palette_loads_scratchpad_files(cx: &mut TestAppContext) {
    use std::time::{Duration, Instant};

    let project = temp_project("qo_files");
    std::fs::create_dir_all(project.join("scratchpad/notes")).unwrap();
    std::fs::write(project.join("scratchpad/notes/a.md"), "# 笔记").unwrap();

    let (_host, _shared, harness, cx) = open_palette_in_project(cx, project.clone());

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut keys: Vec<String> = Vec::new();
    while Instant::now() < deadline {
        // 测试时钟：把泵的 60ms 轮询推到点；真实小睡：等后台线程把清单做出来。
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        keys = cx.update(|_window, cx| harness.read(cx).palette.read(cx).row_keys(cx));
        if keys.iter().any(|key| key == "file:notes/a.md") {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(
        keys.iter().any(|key| key == "file:notes/a.md"),
        "文件清单应在 20 s 内回填成行；实际行：{keys:?}"
    );

    std::fs::remove_dir_all(&project).ok();
}

/// 临时项目目录（每个用例一个；仿 `scratchpad::jobs` 测试的同名助手）。
fn temp_project(tag: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("rds_qo_{tag}_{}_{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp project");
    dir
}
