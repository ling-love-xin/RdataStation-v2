//! Quick Open 浮层的窗口级测试（GPUI headless 窗口）——夹具与按档分组的用例目录。
//!
//! 用**简化宿主**（哑端口 + 只挂浮层的 Harness）驱动，不必构造 `WorkbenchView`
//! （那要读项目会话 / 连磁盘 / 起目录监听）；但走的是**生产入口**：
//! 置 `Shared::quick_open` → `on_opened` → 真按键（`simulate_keystrokes`）→
//! 浮层 render → 委托 → 宿主端口。
//!
//! 用例分组：[`palette`]（输入 / 选中 / 确认 / 遮罩 / 门槛 / 搜索回填）、
//! [`goto`]（Goto 档三例）、[`host`]（文件源端到端）。
//!
//! 注意：**不通配导入**（`use gpui_kit::*` / `use super::*` 会把 gpui 的 `test` 宏带进来，
//! 与 `#[gpui_kit::test]` 冲突）。依赖全部显式列举。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
};

use crate::panels::Shared;
use crate::quick_open::model::{Action, Mode};
use crate::quick_open::palette::{QuickOpenHost, QuickOpenPalette};
use crate::view::ConnectionItem;

mod goto;
mod host;
mod palette;

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
        // 与生产同形：工作台根带 `workbench` 键上下文（`WorkbenchView::render` 里那个），
        // 工作台级键位（如 `alt-enter`）才会沿焦点链生效。
        div()
            .size_full()
            .key_context("workbench")
            .child(self.palette.clone())
    }
}

/// 造一条命中：`orders` 表（带 catalog / schema，这两种信息行与定位都要用）。
fn table_hit() -> database::nav_jobs::SearchHit {
    database::nav_jobs::SearchHit {
        conn_id: "P_1".into(),
        conn_label: "营销".into(),
        driver: "mysql".into(),
        object_type: "table".into(),
        object_name: "orders".into(),
        parent_name: None,
        catalog: Some("shop".into()),
        schema: Some("public".into()),
        snippet: None,
        hit_source: database::nav_jobs::HitSource::Name,
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
    open_palette_in(cx, None, Mode::Default)
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
    open_palette_in(cx, Some(project_root), Mode::Default)
}

/// 「跳转到对象」档打开（`Ctrl+Shift+O` 路径）：仅元数据、主动作 = 回树定位。
fn open_palette_goto(
    cx: &mut TestAppContext,
) -> (
    Rc<StubHost>,
    Shared,
    Entity<Harness>,
    &mut VisualTestContext,
) {
    open_palette_in(cx, None, Mode::Goto)
}

fn open_palette_in(
    cx: &mut TestAppContext,
    project_root: Option<std::path::PathBuf>,
    mode: Mode,
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
        vec![conn("sales", "postgres"), conn("sales_archive", "duckdb")],
        None,
    );
    if let Some(root) = project_root {
        *shared.project.borrow_mut() = Some(project::ui::OpenProject::from_root(root));
    }
    shared.quick_open.set(true);
    let shared_for_palette = shared.clone();
    let (harness, cx) = cx.add_window_view(|_window, cx| {
        let palette = cx.new(|cx| QuickOpenPalette::new(shared_for_palette, palette_host, cx));
        palette.update(cx, |palette, cx| palette.on_opened(mode, cx));
        Harness { palette }
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (host, shared, harness, cx)
}

/// Goto 档：空词不发后台搜索（`search_seq` 仍为 0），伪造一条空词命中并推泵上列，返回业务键。
fn push_goto_meta_row(cx: &mut VisualTestContext, palette: Entity<QuickOpenPalette>) -> String {
    database::nav_jobs::push_search_results_for_test(
        database::nav_jobs::SearchConsumer::QuickOpen,
        database::nav_jobs::SearchResult {
            consumer: database::nav_jobs::SearchConsumer::QuickOpen,
            seq: 0,
            query: String::new(),
            searched: 1,
            hits: vec![table_hit()],
        },
    );
    cx.update(|_window, cx| palette.update(cx, |palette, cx| palette.pump_results(cx)));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|_window, cx| {
        palette
            .read(cx)
            .row_keys(cx)
            .into_iter()
            .find(|key| key.starts_with("meta:"))
            .expect("元数据行应已上列")
    })
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
