//! 宿主面板的窗口级测试（GPUI headless 窗口）
//!
//! 覆盖 A2 的验收口径（多文档并存、标题跟随、脏点显示、面板可关闭、渲染不 panic）
//! 与 A10 的动作口径（快捷键真按下 → 动作真生效）。
//!
//! 注意：这里**不使用** `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的 `test`
//! 属性宏带入作用域，而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会因此解析到它自己
//! （recursion limit reached）。所有依赖显式列举。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::dock::{
    BasePanel as _, DockArea, DockPlacement, DockSkin, Panel as _,
};
use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, AppContext as _, Context, Entity, Focusable as _, IntoElement, KeyBinding, ParentElement as _,
    Render, Styled as _, TestAppContext, VisualTestContext, Window,
};

use crate::commands::{ExecuteAll, ExecuteSql, SaveDocument, ToggleComment};
use crate::connection::{ConnectionOption, ConnectionsPort};
use crate::execution::{self, QueryData, QueryRunner};
use crate::mode::CellGranularity;
use crate::model::{DocumentId, EditorMode};
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::view::host::{
    EditorHostPanel, close_document_in_dock, request_close_document, request_save_as,
    resolve_close_choice,
};

/// 测试用最小宿主：一个真 `DockArea` + 挂在里面的编辑面板
///
/// 动作要真正可派发，面板必须**被渲染**（dispatch path 来自已渲染的树），
/// 所以这里得有一个会画出 Dock 的根视图。
struct Harness {
    area: Entity<DockArea>,
    panels: Vec<Entity<EditorHostPanel>>,
}

impl Render for Harness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.area.clone())
    }
}

/// 临时目录（每个用例一个，互不干扰；用完尽力清理）
fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("rds_editor_actions_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 与 `crates/app/src/main.rs` **同一套**编辑器键位
///
/// 测试里重绑一遍是有意的：context 名或按键串写错时，这里会直接失败，
/// 而不是等到用户按下无反应的键（“注册了才宣传”）。
/// `Ctrl+W` 不在这里：它由宿主执行（见 `close_document_in_dock`），
/// 键位注册仍在 app 层，宿主处理逻辑在 workbench。
fn bind_editor_keys(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("ctrl-s", SaveDocument, Some("editor")),
            KeyBinding::new("ctrl-/", ToggleComment, Some("editor")),
            KeyBinding::new("ctrl-enter", ExecuteSql, Some("editor")),
            KeyBinding::new("ctrl-shift-enter", ExecuteAll, Some("editor")),
        ]);
    });
}

/// 建共享状态并打开一份 `.sql` 文档
fn shared_with_document(path: &str, content: &str) -> (EditorShared, DocumentId) {
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(path, content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id)
}

#[gpui_kit::test]
fn tab_name_tracks_document_title(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\report.sql", "select 1");

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    let name = cx.update(|_window, cx| panel.read(cx).tab_name(cx));
    assert_eq!(name.as_deref(), Some("report.sql"), "标签名来自文档标题");

    // 另存为：身份不变、标题跟随（A1 的语义在视图上直接体现）
    shared.update(|service| service.rename(&id, r"D:\sql\renamed.sql").expect("rename"));

    let name = cx.update(|_window, cx| panel.read(cx).tab_name(cx));
    assert_eq!(name.as_deref(), Some("renamed.sql"));
}

#[gpui_kit::test]
fn dirty_dot_appears_only_after_edit(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\report.sql", "select 1");

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    // 刚打开：干净 → 无脏点（脏点由 `title_suffix` 按 `is_dirty()` 渲染）
    assert!(!cx.update(|_window, cx| panel.read(cx).is_dirty()));

    // 内容变了（这里直接走服务层，等价于内核回写后的状态）
    shared.update(|service| {
        service.set_content(&id, "select 1, 2".to_string());
    });

    assert!(
        cx.update(|_window, cx| panel.read(cx).is_dirty()),
        "改了内容必须脏（标签上会出现脏点）"
    );

    // 保存后回到干净 → 脏点消失
    shared.update(|service| {
        service.mark_saved(&id);
    });
    assert!(
        !cx.update(|_window, cx| panel.read(cx).is_dirty()),
        "保存后必须回到干净（脏点消失）"
    );
}

#[gpui_kit::test]
fn three_documents_coexist_as_three_panels(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();

    let ids: Vec<DocumentId> = [
        (r"D:\sql\a.sql", "select 1"),
        (r"D:\sql\b.sql", "select 2"),
        (r"D:\sql\notes.txt", "纯文本"),
    ]
    .into_iter()
    .map(|(path, content)| {
        shared
            .open(OpenRequest::file(path, content, EditorMode::Sql))
            .id()
            .clone()
    })
    .collect();

    // 三个面板挂在同一个窗口里（真实运行时会挂进同一个 Dock tab 组）
    let mut panels = Vec::new();
    for id in &ids {
        let (panel, _) = {
            let shared = shared.clone();
            let id = id.clone();
            cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
        };
        panels.push(panel);
    }

    let names: Vec<String> = panels
        .iter()
        .map(|panel| {
            cx.update(|cx| panel.read(cx).tab_name(cx))
                .map(|name| name.to_string())
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(names, vec!["a.sql", "b.sql", "notes.txt"]);
    assert_eq!(shared.service().len(), 3);

    // 只编辑第二个 → 只有它的脏状态为真
    shared.update(|service| {
        service.set_content(&ids[1], "select 2, 3".to_string());
    });
    let dirty_flags: Vec<bool> = panels
        .iter()
        .map(|panel| cx.update(|cx| panel.read(cx).is_dirty()))
        .collect();
    assert_eq!(dirty_flags, vec![false, true, false]);
}

#[gpui_kit::test]
fn mode_switch_syncs_editor_and_still_renders(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\report.sql", "select 1");

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    // 切到文本模式：不再上色（判定的“计划”在 `mode::plan_switch`，视图只跟随）
    shared.update(|service| {
        service.set_mode(&id, EditorMode::Text);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Text)
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 切回 SQL：恢复着色
    shared.update(|service| {
        service.set_mode(&id, EditorMode::Sql);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui_kit::test]
fn panel_reports_name_closable_and_renders(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\report.sql", "select 1");

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|_window, cx| {
        assert_eq!(panel.read(cx).panel_name(), "editor");
        assert!(
            panel.read(cx).closable(cx),
            "文档标签应可关闭（草稿兜底语义）"
        );
    });

    // 渲染一帧：不 panic 即通过（编辑器内核来自组件库，不在本 crate 手搓）
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

// ===== A10：动作与快捷键 =====

#[gpui_kit::test]
fn ctrl_s_writes_the_file_and_clears_dirty(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let dir = temp_dir("save");
    let path = dir.join("report.sql");
    std::fs::write(&path, "select 1").expect("写盘");

    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    // 改内容 → 脏。脏文档上保存才会真的发出写盘
    shared.update(|service| service.set_content(&id, "select 2".to_string()));
    assert!(cx.update(|_window, cx| panel.read(cx).is_dirty()));

    // 先画一帧建 dispatch tree，再给焦点，然后真按下 Ctrl+S
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-s");

    assert_eq!(
        std::fs::read_to_string(&path).expect("读回"),
        "select 2",
        "Ctrl+S 必须真的写盘"
    );
    assert!(
        !cx.update(|_window, cx| panel.read(cx).is_dirty()),
        "保存后必须清脏"
    );
    assert!(
        cx.update(|_window, cx| panel.read(cx).message.is_none()),
        "成功不应留提示"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui_kit::test]
fn ctrl_s_on_an_untitled_document_reports_the_reason(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-s");

    // 未命名文档写不了盘：**不能静默结束**，原因要落在状态栏上
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    let message = message.expect("未命名文档保存失败要给出原因");
    assert!(message.contains("另存为"), "{message}");
}

#[gpui_kit::test]
fn ctrl_slash_comments_the_selection(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("select 1\nselect 2", EditorMode::Sql))
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    // 光标停在起始位 → 注释第一行
    cx.simulate_keystrokes("ctrl-/");
    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert_eq!(text, "-- select 1\nselect 2");
    // 服务层同步（否则标签脏点与保存都会落后于屏幕上看到的文本）
    assert_eq!(
        cx.update(|_window, _cx| shared
            .service()
            .find(&id)
            .map(|doc| doc.content().to_string())),
        Some("-- select 1\nselect 2".to_string()),
        "注释后的文本必须回写服务层"
    );

    // 再按一次 → 去注释（可逆）
    cx.simulate_keystrokes("ctrl-/");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).text_for_test(cx)),
        "select 1\nselect 2"
    );
}

#[gpui_kit::test]
fn readonly_document_refuses_the_comment_action(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let shared = EditorShared::new();
    let id = shared
        .open(
            OpenRequest::untitled("select 1", EditorMode::Sql)
                .with_read_only(crate::model::ReadOnly::editor_only()),
        )
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-/");

    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).text_for_test(cx)),
        "select 1",
        "只读文档不得被改"
    );
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.expect("只读拒绝要留原因").contains("只读"));
}

#[gpui_kit::test]
fn closing_a_clean_document_on_request_removes_the_panel_and_closes_it(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let shared = EditorShared::new();
    let ids: Vec<DocumentId> = ["select 1", "select 2"]
        .into_iter()
        .map(|text| {
            shared
                .open(OpenRequest::untitled(text, EditorMode::Sql))
                .id()
                .clone()
        })
        .collect();

    let (harness, cx) = {
        let shared = shared.clone();
        let ids = ids.clone();
        cx.add_window_view(move |window, cx| {
            let (area, _skin) = DockSkin::dock_area("editor-actions", Some(1), window, cx);
            let mut panels = Vec::new();
            for id in ids {
                let panel = cx.new(|cx| EditorHostPanel::new(shared.clone(), id, window, cx));
                panels.push(panel.clone());
                area.update(cx, |area, cx| {
                    area.add_panel(panel, DockPlacement::Center, None, window, cx);
                });
            }
            Harness { area, panels }
        })
    };
    assert_eq!(shared.service().len(), 2);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 宿主发起关闭（快捷键由 workbench 的 on_action 调到这里）。
    // 完整链路：`close_document_in_dock` → DockArea 移除面板 → `on_removed` → 文档关闭。
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });
    let closed = cx.update(|window, cx| close_document_in_dock(&area, panel, window, cx));
    assert!(closed, "干净文档应当被关掉");

    assert_eq!(shared.service().len(), 1, "只关掉指定的那份文档");
    assert!(shared.service().find(&ids[0]).is_none(), "被关的是指定文档");
    assert!(
        shared.service().find(&ids[1]).is_some(),
        "另一个文档不受影响"
    );
    assert!(
        cx.update(|_window, cx| harness.read(cx).panels[0].read(cx).is_closed()),
        "面板要被标为已移除（宿主据此清理面板列表）"
    );
}

#[gpui_kit::test]
fn closing_a_dirty_document_is_refused_with_a_reason(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();

    let (harness, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| {
            let (area, _skin) = DockSkin::dock_area("editor-close-dirty", Some(1), window, cx);
            let panel = cx.new(|cx| EditorHostPanel::new(shared.clone(), id, window, cx));
            let handle = panel.clone();
            area.update(cx, |area, cx| {
                area.add_panel(handle, DockPlacement::Center, None, window, cx);
            });
            Harness {
                area,
                panels: vec![panel],
            }
        })
    };
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 改动 → 脏 → 拒绝关闭（Dock 没有“关闭前否决”钩子，只能在这里拦）
    shared.update(|service| service.set_content(&id, "select 2".to_string()));
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });
    let closed = cx.update(|window, cx| close_document_in_dock(&area, panel, window, cx));

    assert!(!closed, "脏文档不得被关掉");
    assert_eq!(shared.service().len(), 1);
    let message = cx.update(|_window, cx| harness.read(cx).panels[0].read(cx).message.clone());
    assert!(message.expect("拒绝关闭要留原因").contains("未保存"));
}

#[gpui_kit::test]
fn focusing_a_tab_activates_its_document(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let shared = EditorShared::new();
    let first = shared
        .open(OpenRequest::untitled("select 1", EditorMode::Sql))
        .id()
        .clone();

    let (harness, cx) = {
        let shared = shared.clone();
        let id = first.clone();
        cx.add_window_view(move |window, cx| {
            let (area, _skin) = DockSkin::dock_area("editor-focus", Some(1), window, cx);
            let panel = cx.new(|cx| EditorHostPanel::new(shared.clone(), id, window, cx));
            let handle = panel.clone();
            area.update(cx, |area, cx| {
                area.add_panel(handle, DockPlacement::Center, None, window, cx);
            });
            Harness {
                area,
                panels: vec![panel],
            }
        })
    };
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 又开一份文档（服务层的“当前文档”转到他身上，但面板还没建）
    let second = shared
        .open(OpenRequest::untitled("select 2", EditorMode::Sql))
        .id()
        .clone();
    assert_eq!(shared.service().active_id(), Some(&second));

    // 把已存在的标签挤到前台（宿主打开已打开文档时走同一条路）。
    // 面板拿到焦点后，服务层的“当前文档”也跟着回来——宿主的文档级动作
    // （`Ctrl+W` 等）靠它定位到正确的那份。
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.focus_self(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert_eq!(
        shared.service().active_id(),
        Some(&first),
        "激活标签后当前文档要跟着回去"
    );
}

// ===== A14：执行（假执行器，真线程）=====

/// 执行器看到的 SQL 序列（A14 断言用）
type SeenSql = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// 执行器看到的连接序列（B1 断言用）
type SeenConnections = std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>;

/// 假执行器：记录收到的连接与 SQL，按 SQL 内容决定成败
struct ScriptRunner {
    seen: SeenSql,
    /// 收到的连接（B1）：绑定的连接必须原样传到这里
    seen_connections: SeenConnections,
}

impl QueryRunner for ScriptRunner {
    fn run(&self, connection: Option<&str>, sql: &str, _options: execution::RunOptions) -> Result<QueryData, String> {
        self.seen.lock().expect("锁").push(sql.to_string());
        self.seen_connections
            .lock()
            .expect("锁")
            .push(connection.map(str::to_string));
        if sql.contains("boom") {
            return Err("驱动报错：boom".to_string());
        }
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            elapsed_ms: 5,
            truncated: false,
            affected_rows: None,
            has_more: false,
        })
    }
}

/// 带假执行器的共享状态 + 一份文档
fn shared_with_runner(
    content: &str,
    mode: EditorMode,
) -> (EditorShared, DocumentId, SeenSql, SeenConnections) {
    let shared = EditorShared::new();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_connections = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    shared.attach_runner(std::sync::Arc::new(ScriptRunner {
        seen: seen.clone(),
        seen_connections: seen_connections.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled(content, mode))
        .id()
        .clone();
    (shared, id, seen, seen_connections)
}

/// 假执行器（B2）：按 SQL 里的 `rows=N` 决定结果行数
///
/// 多结果集的测试必须能区分“网格里现在是哪一份”，否则切过去也看不出来。
struct SizedRunner;

impl QueryRunner for SizedRunner {
    fn run(&self, _connection: Option<&str>, sql: &str, _options: execution::RunOptions) -> Result<QueryData, String> {
        // 与 `ScriptRunner` 同一口径：带 `boom` 的语句失败（批量要能验“失败不中断”）
        if sql.contains("boom") {
            return Err("驱动报错：boom".to_string());
        }
        let rows = sql
            .split("rows=")
            .nth(1)
            .and_then(|tail| {
                let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
                digits.parse::<usize>().ok()
            })
            .unwrap_or(1);
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: (0..rows).map(|index| vec![index.to_string()]).collect(),
            elapsed_ms: 3,
            truncated: false,
            affected_rows: None,
            has_more: false,
        })
    }
}

/// 带 `SizedRunner` 的共享状态 + 一份文档（B2：多结果集）
fn shared_with_sized_runner(content: &str) -> (EditorShared, DocumentId) {
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(SizedRunner));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id)
}

/// 假执行器（B5）：按 SQL 内容给出「写语句 / 被截断 / 普通结果」三种真实形状
struct ToolbarRunner;

impl QueryRunner for ToolbarRunner {
    fn run(&self, _connection: Option<&str>, sql: &str, _options: execution::RunOptions) -> Result<QueryData, String> {
        if sql.contains("insert") {
            return Ok(QueryData {
                columns: Vec::new(),
                rows: Vec::new(),
                elapsed_ms: 7,
                truncated: false,
                affected_rows: Some(3),
                has_more: false,
            });
        }
        if sql.contains("truncated") {
            return Ok(QueryData {
                columns: vec!["n".to_string()],
                rows: (0..3).map(|index| vec![index.to_string()]).collect(),
                elapsed_ms: 4,
                truncated: true,
                affected_rows: None,
                has_more: false,
            });
        }
        Ok(QueryData {
            columns: vec!["n".to_string(), "note".to_string()],
            rows: vec![vec!["1".to_string(), "a".to_string()]],
            elapsed_ms: 2,
            truncated: false,
            affected_rows: None,
            has_more: false,
        })
    }
}

/// 带 `ToolbarRunner` 的共享状态 + 一份文档（B5：结果工具栏）
fn shared_with_toolbar_runner(content: &str) -> (EditorShared, DocumentId) {
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(ToolbarRunner));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id)
}

/// 假执行器（B6）：回一条**带位置**的真实格式错误（SQLite 的 `no such column: x`）
///
/// 文本用真机抓到的形态之一（另一种是 `near "x": syntax error in … at offset N`）。
struct LocatedFailureRunner;

/// 假执行器（B5b）：首段 2 行且**拿满了**（has_more），取下一段再给 2 行
///
/// 记下每次要的 `(offset, limit)`：界面按的 offset 对不对是这条链路的关键。
struct SegmentRunner {
    asked: std::sync::Arc<std::sync::Mutex<Vec<(usize, usize)>>>,
}

impl QueryRunner for SegmentRunner {
    fn run(&self, _connection: Option<&str>, _sql: &str, _options: execution::RunOptions) -> Result<QueryData, String> {
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            elapsed_ms: 2,
            truncated: false,
            affected_rows: None,
            has_more: true,
        })
    }

    fn fetch_next(
        &self,
        _connection: Option<&str>,
        _sql: &str,
        offset: usize,
        limit: usize,
    ) -> Result<QueryData, String> {
        self.asked.lock().expect("锁").push((offset, limit));
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["3".to_string()], vec!["4".to_string()]],
            elapsed_ms: 1,
            truncated: false,
            affected_rows: None,
            has_more: false,
        })
    }
}

/// 带 `SegmentRunner` 的共享状态 + 一份文档（B5b）
fn shared_with_segment_runner(
    content: &str,
) -> (
    EditorShared,
    DocumentId,
    std::sync::Arc<std::sync::Mutex<Vec<(usize, usize)>>>,
) {
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(SegmentRunner {
        asked: asked.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id, asked)
}

/// 假执行器（B5b）：说“还有下一段”但不支持取（默认 `fetch_next` 说实话）
struct MoreUnsupportedRunner;

impl QueryRunner for MoreUnsupportedRunner {
    fn run(&self, _connection: Option<&str>, _sql: &str, _options: execution::RunOptions) -> Result<QueryData, String> {
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()]],
            elapsed_ms: 1,
            truncated: false,
            affected_rows: None,
            has_more: true,
        })
    }
}

impl QueryRunner for LocatedFailureRunner {
    fn run(&self, _connection: Option<&str>, sql: &str, _options: execution::RunOptions) -> Result<QueryData, String> {
        if sql.contains("wheree") {
            return Err(format!(
                "[DB_QUERY] Query failed: no such column: wheree (SQL: {sql})"
            ));
        }
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()]],
            elapsed_ms: 1,
            truncated: false,
            affected_rows: None,
            has_more: false,
        })
    }
}

/// 跑一句话并等回填（B5 用：结果工具栏要真的跟着结果变）
fn run_statement(
    cx: &mut VisualTestContext,
    panel: &Entity<EditorHostPanel>,
    sql: &str,
    placement: execution::ResultPlacement,
) {
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(execution::ExecTarget::Statement(sql.to_string()), placement, cx)
        })
    });
    wait_for_all_pending(cx, panel);
}

/// 等本文档“已提交的语句都回填完”（批量是多条，`wait_for_result` 只等第一条）
fn wait_for_all_pending(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_exec_results(cx);
                panel.pending_for_test() == 0
            })
        });
        if done {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "批量结果迟迟没全部回来");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 等一个条件成立（工作线程把状态摆在内存里时用；超时就直接报出来）
fn wait_until(mut ready: impl FnMut() -> bool, what: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ready() {
        assert!(std::time::Instant::now() < deadline, "等不到：{what}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// 网格当前行数（结果区的真实投影）
fn grid_rows(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) -> usize {
    cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx))
}

/// 结果集标签（文案 + 是否失败）
fn result_tabs(
    cx: &mut VisualTestContext,
    panel: &Entity<EditorHostPanel>,
) -> Vec<(String, bool)> {
    cx.update(|_window, cx| panel.read(cx).result_tabs_for_test())
}

/// 在窗口里建面板（宿主建的窗口第一层视图**
fn open_panel<'a>(
    cx: &'a mut TestAppContext,
    shared: &EditorShared,
    id: &DocumentId,
) -> (Entity<EditorHostPanel>, &'a mut VisualTestContext) {
    let shared = shared.clone();
    let id = id.clone();
    cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
}

/// 等结果回填（真后台线程；测试里手动跑轮询泵的那一步，带超时）
fn wait_for_result(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_exec_results(cx);
                panel.result_summary_for_test().is_some()
            })
        });
        if done {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "执行结果迟迟没回来");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn ctrl_enter_runs_the_statement_under_the_cursor(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id, seen, _seen_conn) = shared_with_runner(
        "select 1;
select 2;
select boom;",
        EditorMode::Sql,
    );
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 光标停在第二句里（键位路径仍走真按键）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_caret_for_test(11, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-enter");
    wait_for_result(cx, &panel);

    assert_eq!(
        seen.lock().expect("锁").as_slice(),
        ["select 2".to_string()],
        "执行的应当是光标所在的那一句"
    );

    // 结果进了权威存储（数字都是真实值）
    let stored = shared
        .results()
        .active(&id)
        .map(|entry| (entry.row_count(), entry.columns.len(), entry.summary()));
    assert_eq!(stored, Some((2, 1, "2 行 × 1 列 · 5 ms".to_string())));

    // 界面：结果行 + 网格里的行 + 不 panic 的一帧
    let summary = cx.update(|_window, cx| {
        panel
            .read(cx)
            .result_summary_for_test()
            .map(|text| text.to_string())
    });
    assert_eq!(summary.as_deref(), Some("行数 2 · 耗时 5 ms"));
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        2,
        "网格要拿到行"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui_kit::test]
fn ctrl_shift_enter_runs_the_whole_script(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (_shared, id, seen, _seen_conn) = shared_with_runner(
        "select 1;
select 2;",
        EditorMode::Sql,
    );
    let (panel, cx) = open_panel(cx, &_shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-shift-enter");
    wait_for_result(cx, &panel);

    assert_eq!(
        seen.lock().expect("锁").as_slice(),
        ["select 1;
select 2;"
            .to_string()],
        "“执行全部”发的是整篇脚本"
    );
}

#[gpui_kit::test]
fn a_failing_execution_gets_an_error_card_instead_of_an_empty_grid(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id, _seen, _seen_conn) = shared_with_runner("select boom;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-enter");
    wait_for_result(cx, &panel);

    // 【B5】工具栏只说“失败”（原型 §2.4 的 `行数 │ 耗时 │ 连接名` 里失败占一段）
    let summary = cx
        .update(|_window, cx| panel.read(cx).result_summary_for_test())
        .expect("失败也要有结果区文案");
    assert!(summary.contains("失败"), "{summary}");
    // 【B6】原因在**错误卡片**里（原型 §2.4「错误呈现」），不是一句灰字
    let card = cx
        .update(|_window, cx| panel.read(cx).result_error_card_for_test())
        .expect("失败要有错误卡片");
    assert!(card.message.contains("boom"), "{}", card.message);
    assert!(
        card.location.is_none(),
        "这条错误认不出位置，就不给定位按钮"
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-error-card"),
        "卡片要真的画出来"
    );
    assert!(
        !dialog_button_rendered(cx, "editor-result-error-locate"),
        "认不出位置就不摆定位按钮"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        0,
        "失败不该有网格行"
    );
    // 失败原因同时写进结果存储与状态栏
    let stored = shared
        .results()
        .active(&id)
        .and_then(|entry| entry.error.clone());
    assert!(stored.is_some(), "错误要进 ResultStore");
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.expect("状态栏也要说原因").contains("boom"));
}

// ===== B2：批量执行与多结果集 =====

/// 批量：三句语句 → 三个结果集，**失败不中断**且失败的那份带标记
#[gpui_kit::test]
fn a_batch_lands_three_statements_in_three_result_sets(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id) = shared_with_sized_runner("select rows=1;\nselect boom;\nselect rows=3;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 走菜单同样的落地入口（菜单点击在 headless 下命中测试不可靠：目标解析已有纯函数单测）
    let target = execution::batch_target("select rows=1;\nselect boom;\nselect rows=3;");
    assert_eq!(target.statements().len(), 3, "先确认切出了三句");
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(target, execution::ResultPlacement::NewSet, cx)
        })
    });
    wait_for_all_pending(cx, &panel);

    let tabs = result_tabs(cx, &panel);
    assert_eq!(
        tabs,
        vec![
            ("结果 1".to_string(), false),
            ("结果 2".to_string(), true),
            ("结果 3".to_string(), false),
        ],
        "每句一个结果集，中间那句失败要标出来"
    );
    assert_eq!(
        shared.results().set_count(&id),
        3,
        "三份结果都在权威存储里"
    );
    // 失败不中断：第三句真的跑了（否则它不会有自己的结果集）
    assert_eq!(grid_rows(cx, &panel), 1, "默认选中第一份");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 点开第三份：网格跟着换（headless 下点击不稳，直接驱动落地入口）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_result_set(2, cx)));
    assert_eq!(grid_rows(cx, &panel), 3, "第三句的结果真的落地了");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_active_for_test()),
        2
    );
    // 界面上真的画出了标签条（两份以上结果才画）
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 「在新结果标签中执行」：新结果放旁边，**原结果集仍选中**（原型 §4.4）
#[gpui_kit::test]
fn running_into_a_new_set_keeps_the_previous_one_selected(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id) = shared_with_sized_runner("select rows=2;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 第一次：普通执行（替换语义）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.execute_preferring_selection(cx))
    });
    wait_for_result(cx, &panel);
    assert_eq!(grid_rows(cx, &panel), 2);
    assert_eq!(result_tabs(cx, &panel).len(), 1);

    // 第二次：同文档、新结果集
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement("select rows=5".to_string()),
                execution::ResultPlacement::NewSet,
                cx,
            )
        })
    });
    wait_for_all_pending(cx, &panel);

    assert_eq!(shared.results().set_count(&id), 2, "新的一份追加在后面");
    assert_eq!(
        result_tabs(cx, &panel).len(),
        2,
        "两份结果 → 标签条该出现"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_active_for_test()),
        0,
        "原结果集保持选中（新结果放旁边，不打扰在看的那份）"
    );
    assert_eq!(grid_rows(cx, &panel), 2, "网格还是第一份的数据");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 切到新的一份：网格与选中态一起跟着走
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_result_set(1, cx)));
    assert_eq!(grid_rows(cx, &panel), 5);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_active_for_test()),
        1
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).result_summary_for_test()),
        Some("行数 5 · 耗时 3 ms".to_string()),
        "状态行跟着选中的结果集走"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

// ===== B3：中断与超时 =====

/// 假执行器（B3）：卡在 `run` 里等中断（模拟慢查询；驱动侧由取消令牌打断）
struct BlockingRunner {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// 假执行器真的进去跑了吗
    ///
    /// “提交”不等于“驱动在跑”：工作线程要先取到作业、再读中断标记，然后才进 `run`。
    /// 中断的用例必须等它真的跑起来，否则测到的是“还没开始的那条被标已取消”
    /// （那是另一条语义，由批量用例钉住）。
    started: std::sync::Arc<std::sync::atomic::AtomicBool>,
    cancels: SeenConnections,
}

impl QueryRunner for BlockingRunner {
    fn run(&self, _connection: Option<&str>, _sql: &str, _options: execution::RunOptions) -> Result<QueryData, String> {
        self.started.store(true, std::sync::atomic::Ordering::SeqCst);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !self.stop.load(std::sync::atomic::Ordering::SeqCst) {
            assert!(std::time::Instant::now() < deadline, "假执行器没等到中断");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Err("Query cancelled".to_string())
    }

    fn cancel(&self, connection: Option<&str>) -> Result<bool, String> {
        self.cancels
            .lock()
            .expect("锁")
            .push(connection.map(str::to_string));
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(true)
    }
}

/// 带可中断假执行器的共享状态 + 一份文档
fn shared_with_blocking_runner(
    content: &str,
) -> (
    EditorShared,
    DocumentId,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    SeenConnections,
) {
    let shared = EditorShared::new();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let started = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancels: SeenConnections = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    shared.attach_runner(std::sync::Arc::new(BlockingRunner {
        stop: stop.clone(),
        started: started.clone(),
        cancels: cancels.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id, stop, started, cancels)
}

/// 中断：慢查询就此结束（结果集报错），中断请求真的到了执行器，跑完不再计时
#[gpui_kit::test]
fn interrupting_a_slow_query_ends_it_with_a_visible_reason(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id, stop, started, cancels) = shared_with_blocking_runner("select slow;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement("select slow".to_string()),
                execution::ResultPlacement::Replace,
                cx,
            )
        })
    });
    assert_eq!(cx.update(|_window, cx| panel.read(cx).pending_for_test()), 1);
    assert!(
        cx.update(|_window, cx| panel.read(cx).elapsed_for_test()).is_some(),
        "执行中就该有耗时（状态栏的“执行中 3.4s…”靠它）"
    );
    // 执行中画一帧：状态栏那个 ■ 中断 按钮真的渲染（布局/借用问题会在这里暴露）
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 等它**真的跑起来**再中断：中断要打断的是“跑着的查询”
    wait_until(
        || started.load(std::sync::atomic::Ordering::SeqCst),
        "假执行器开始跑",
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.interrupt(cx)));
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.expect("中断要有回执").contains("已请求中断"),
        "点了中断必须看得见"
    );

    wait_for_all_pending(cx, &panel);
    assert_eq!(
        cancels.lock().expect("锁").as_slice(),
        [None],
        "未绑定连接的文档取消的就是“当前活动连接”"
    );
    let summary = cx
        .update(|_window, cx| panel.read(cx).result_summary_for_test())
        .expect("中断也要有结果区文案");
    assert!(summary.contains("失败"), "{summary}");
    // 原因在错误卡片里（原型 §2.4）
    let card = cx
        .update(|_window, cx| panel.read(cx).result_error_card_for_test())
        .expect("中断要有错误卡片");
    assert!(card.message.contains("cancel"), "{}", card.message);
    assert!(
        cx.update(|_window, cx| panel.read(cx).elapsed_for_test()).is_none(),
        "跑完就不再计时"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // 释放仍在自旋的假执行器（它已经返回了，这里只是让变量活着）
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// 没在跑时点中断：回绝并说原因（“点了没反应”不可接受）
#[gpui_kit::test]
fn interrupting_when_nothing_runs_says_why(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (_shared, id, _stop, _started, _cancels) = shared_with_blocking_runner("select 1;");
    let (panel, cx) = open_panel(cx, &_shared, &id);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).pending_for_test()),
        0
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.interrupt(cx)));
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.expect("回绝也要说原因").contains("没有执行"),
        "没在跑就该直说"
    );
}

// ===== B4：事务 =====

/// 假执行器（B4）：支持事务，记录执行选项与事务动作
struct TxRunner {
    actions: std::sync::Arc<std::sync::Mutex<Vec<(crate::execution::TxAction, Option<String>)>>>,
    options: std::sync::Arc<std::sync::Mutex<Vec<crate::execution::RunOptions>>>,
    open: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl QueryRunner for TxRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _sql: &str,
        options: crate::execution::RunOptions,
    ) -> Result<QueryData, String> {
        self.options.lock().expect("锁").push(options);
        if options.use_transaction {
            // 模拟引擎：自动提交关掉时，执行会先开一个事务
            self.open
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()]],
            elapsed_ms: 1,
            truncated: false,
            affected_rows: None,
            has_more: false,
        })
    }

    fn transaction_snapshot(&self, _connection: Option<&str>) -> crate::execution::TxSnapshot {
        crate::execution::TxSnapshot {
            in_transaction: self.open.load(std::sync::atomic::Ordering::SeqCst),
        }
    }

    fn transaction(
        &self,
        connection: Option<&str>,
        action: crate::execution::TxAction,
    ) -> Result<(), String> {
        self.actions
            .lock()
            .expect("锁")
            .push((action, connection.map(str::to_string)));
        let open = match action {
            crate::execution::TxAction::Begin => true,
            crate::execution::TxAction::Commit | crate::execution::TxAction::Rollback => false,
        };
        self.open.store(open, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn supports_transactions(&self) -> bool {
        true
    }
}

/// 带事务假执行器的共享状态 + 一份文档（返回记录用的两个句柄）
#[allow(clippy::type_complexity)]
fn shared_with_tx_runner(
    content: &str,
) -> (
    EditorShared,
    DocumentId,
    std::sync::Arc<std::sync::Mutex<Vec<crate::execution::RunOptions>>>,
    std::sync::Arc<std::sync::Mutex<Vec<(crate::execution::TxAction, Option<String>)>>>,
) {
    let shared = EditorShared::new();
    let actions = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let options = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    shared.attach_runner(std::sync::Arc::new(TxRunner {
        actions: actions.clone(),
        options: options.clone(),
        open: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id, options, actions)
}

/// 等事务动作的回执都回来了
fn wait_for_tx_idle(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_exec_results(cx);
                panel.tx_pending_for_test() == 0
            })
        });
        if done {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "事务动作的回执迟迟没回来");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 自动提交关掉时，**下一次执行**要带上“进事务”，执行后的 TX 区也要跟着变
#[gpui_kit::test]
fn toggling_autocommit_puts_the_next_execution_in_a_transaction(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id, options, _actions) = shared_with_tx_runner("insert into t values (1);");
    let (panel, cx) = open_panel(cx, &shared, &id);

    assert!(
        cx.update(|_window, cx| panel.read(cx).autocommit_for_test()),
        "默认是自动提交"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).tx_text_for_test()),
        "TX 未开启"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_autocommit(cx)));
    assert!(
        !cx.update(|_window, cx| panel.read(cx).autocommit_for_test()),
        "点一下就关掉"
    );

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.execute_preferring_selection(cx))
    });
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        options.lock().expect("锁").as_slice(),
        [crate::execution::RunOptions {
            use_transaction: true
        }],
        "自动提交关 → 执行要进事务"
    );
    assert!(
        cx.update(|_window, cx| panel.read(cx).tx_text_for_test())
            .starts_with("TX 已开启"),
        "执行后停在事务里，状态栏要如实说"
    );
    // 画一帧：此时 TX 区应出现提交 / 回滚按钮（渲染路径不 panic）
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 状态栏的事务动作：开始 → 提交，动作真的到执行器，TX 区跟着开合
#[gpui_kit::test]
fn transaction_actions_from_the_status_bar_show_up_in_the_tx_area(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (shared, id, _options, actions) = shared_with_tx_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.tx_action(crate::execution::TxAction::Begin, cx)
        })
    });
    wait_for_tx_idle(cx, &panel);
    assert!(
        cx.update(|_window, cx| panel.read(cx).tx_text_for_test())
            .starts_with("TX 已开启"),
        "开始事务后 TX 区要开"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.tx_action(crate::execution::TxAction::Commit, cx)
        })
    });
    wait_for_tx_idle(cx, &panel);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).tx_text_for_test()),
        "TX 未开启",
        "提交后 TX 区要关"
    );
    assert_eq!(
        actions
            .lock()
            .expect("锁")
            .iter()
            .map(|(action, _)| *action)
            .collect::<Vec<_>>(),
        [
            crate::execution::TxAction::Begin,
            crate::execution::TxAction::Commit
        ],
        "两个动作都要真的送到执行器"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui_kit::test]
fn text_mode_refuses_to_execute(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (_shared, id, seen, _seen_conn) = shared_with_runner("select 1;", EditorMode::Text);
    let (panel, cx) = open_panel(cx, &_shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-enter");

    assert!(
        seen.lock().expect("锁").is_empty(),
        "文本模式不得把 SQL 发给驱动"
    );
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.expect("拒绝要留原因").contains("文本模式"));
}

// ===== A11：查找 / 替换（内核能力，本 crate 不自建）=====

/// 建一份带内容的文档与面板（查找不需要执行器）
fn panel_with_text<'a>(
    cx: &'a mut TestAppContext,
    content: &str,
) -> (Entity<EditorHostPanel>, &'a mut VisualTestContext) {
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };
    (panel, cx)
}

#[gpui_kit::test]
fn the_kernel_find_panel_takes_over_on_ctrl_f(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (panel, cx) = panel_with_text(cx, "select 1;\nselect 2;");
    // 焦点给编辑内核（真机上用户就在这儿打字）
    let editor_handle = cx.update(|_window, cx| panel.read(cx).editor_focus_handle_for_test(cx));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.focus(&editor_handle, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // Ctrl+F 由内核处理（`input::Search` → 组件库的查找面板）。
    // 可观察信号：焦点被面板的查找框拿走 —— 这正是“查找栏打开了”的真实表现。
    cx.simulate_keystrokes("ctrl-f");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let focused = cx.update(|window, cx| window.focused(cx));
    assert!(
        focused.as_ref() != Some(&editor_handle),
        "Ctrl+F 之后焦点应当离开编辑内核（内核把焦点交给查找框）"
    );

    // 再画一帧、再切走再回来都不该 panic（查找面板是组件库的浮层）
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui_kit::test]
fn ctrl_h_opens_the_kernel_replace_panel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let (panel, cx) = panel_with_text(cx, "select 1;");
    let editor_handle = cx.update(|_window, cx| panel.read(cx).editor_focus_handle_for_test(cx));
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.focus(&editor_handle, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.simulate_keystrokes("ctrl-h");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let focused = cx.update(|window, cx| window.focused(cx));
    assert!(
        focused.as_ref() != Some(&editor_handle),
        "Ctrl+H 之后焦点应当离开编辑内核（替换行出现并聚焦）"
    );
}

// ===== A13：文件档位 =====

/// 造一个指定大小的稀疏文件（秒级，不真写满磁盘）
fn sparse_file(dir: &std::path::Path, name: &str, bytes: u64) -> std::path::PathBuf {
    let path = dir.join(name);
    let file = std::fs::File::create(&path).expect("建文件");
    file.set_len(bytes).expect("扩到目标大小");
    drop(file);
    path
}

fn temp_dir_for(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_editor_tier_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

#[gpui_kit::test]
fn a_huge_file_is_not_read_into_memory_and_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir_for("huge");
    let path = sparse_file(&dir, "huge.sql", crate::limits::HUGE_FILE_BYTES + 4096);

    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|_window, cx| {
        let panel = panel.read(cx);
        assert!(
            !panel.is_editable_for_test(),
            "超大文件不做可编辑会话（只读）"
        );
        let notice = panel.tier_notice().expect("要有提示卡");
        assert!(notice.contains("未加载"), "{notice}");
    });

    // 内容确实是空的：200MB 没被读进来
    let content_len = cx.update(|_window, _cx| {
        shared
            .service()
            .find(&id)
            .map(|doc| doc.content().len())
            .unwrap_or(usize::MAX)
    });
    assert_eq!(content_len, 0, "超大文件不该被整份读进内存");

    // 提示卡在渲染路径上（画一帧不 panic）
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui_kit::test]
fn a_large_file_stays_editable_but_warns_about_heavy_features(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir_for("large");
    // 真写 50MiB 太慢；用稀疏文件 + 写一行内容：档位只看大小，不读全文件也能判定
    let path = sparse_file(&dir, "large.sql", crate::limits::LARGE_FILE_BYTES + 1024);

    let shared = EditorShared::new();
    let tier = crate::limits::tier_for_path(&path);
    assert_eq!(tier, crate::limits::FileTier::Large);
    assert!(tier.disables_completion(), "大文件关补全");

    // 用带档位的请求打开（跳过真读 50MiB：这里只验面板对档位的表现）
    let id = shared
        .open(OpenRequest::file(&path, "select 1;", EditorMode::Sql).with_tier(tier))
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|_window, cx| {
        let panel = panel.read(cx);
        assert!(panel.is_editable_for_test(), "大文件仍可编辑");
        assert!(
            panel.tier_notice().expect("要有提示").contains("补全"),
            "提示要说清关了什么"
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui_kit::test]
fn a_normal_file_has_no_notice_card(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir_for("normal");
    let path = dir.join("small.sql");
    std::fs::write(&path, "select 1;").expect("写盘");

    let (shared, id) = shared_with_document(path.to_string_lossy().as_ref(), "select 1;");
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };
    cx.update(|_window, cx| {
        assert!(
            panel.read(cx).tier_notice().is_none(),
            "常规文件不显示提示卡"
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let _ = std::fs::remove_dir_all(dir);
}

// ═══════════════════════════════════════════════════════════════════════
// 对话框流程（A9）：模式切换确认
// ═══════════════════════════════════════════════════════════════════════
//
// 对话框要真弹得出来，窗口的根视图必须是组件库的 `Root`（`Root::update` 找不到 Root 会
// 直接 panic），且宿主 render 里要挂 `Root::render_dialog_layer`——与生产同构。
// 下面这组测试因此不复用上面那个 Harness，而是把 `Root` 包在外面（“走生产入口”）。

/// 带对话框层的宿主（与 `WorkbenchView` 同构：DockArea + 对话框层）
struct DialogHarness {
    area: Entity<DockArea>,
    panels: Vec<Entity<EditorHostPanel>>,
}

impl Render for DialogHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.area.clone())
            .when_some(Root::render_dialog_layer(window, cx), |this, layer| {
                this.child(layer)
            })
    }
}

/// 起一个带对话框层的窗口，里面装一个文档的面板
fn dialog_harness<'a>(
    cx: &'a mut TestAppContext,
    shared: &EditorShared,
    id: &DocumentId,
    tag: &'static str,
) -> (Entity<DialogHarness>, &'a mut VisualTestContext) {
    let shared = shared.clone();
    let id = id.clone();
    let slot = std::rc::Rc::new(std::cell::RefCell::new(None::<Entity<DialogHarness>>));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let (area, _skin) = DockSkin::dock_area(tag, Some(1), window, cx);
        let panel = cx.new(|cx| EditorHostPanel::new(shared.clone(), id, window, cx));
        area.update(cx, |area, cx| {
            area.add_panel(panel.clone(), DockPlacement::Center, None, window, cx);
        });
        let harness = cx.new(|_cx| DialogHarness {
            area,
            panels: vec![panel],
        });
        *slot_in.borrow_mut() = Some(harness.clone());
        Root::new(harness, window, cx)
    });
    let harness = slot.borrow().clone().expect("harness 已创建");
    (harness, cx)
}

/// 真点对话框上的某个按钮：headless 下 `debug_bounds` 的坐标与鼠标命中测试对不上
/// （试过 `simulate_click` + `run_until_parked`：单跑能中、全套跑就中不了），
/// 因此对话框只断言“层真渲染 + 按钮真在”，点击分发不在这里赌。
fn dialog_button_rendered(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.debug_bounds(selector).is_some()
}

/// 需要付出代价的切换：先问，不问就不切（禁止静默切换，原型 §1.3）
#[gpui_kit::test]
fn switching_to_text_asks_before_hiding_results(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\ask.sql", "select 1;");
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-ask");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.request_mode_switch(EditorMode::Text, window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "SQL → 文本要先确认"
    );
    assert!(
        cx.debug_bounds("dialog-layer").is_some(),
        "对话框层要真渲染（不是只有状态）"
    );
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Sql),
        "未确认前模式不得变（静默切换是禁止项）"
    );

    // 取消（Esc / 点遮罩 / 取消按钮走的是同一条路：对话框关掉、状态不变）
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-switch-cancel"),
        "取消按钮要在对话框里"
    );
    cx.update(|window, cx| window.close_dialog(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Sql),
        "取消就是什么都不做"
    );
}

/// 确认后真切：SQL → 分析要带粒度，按钮上的粒度直接决定单元怎么分
#[gpui_kit::test]
fn confirming_the_switch_applies_the_chosen_granularity(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\cells.sql", "select 1; select 2;");
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-confirm");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.request_mode_switch(EditorMode::Analysis, window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    // 粒度二选一：两个动作按钮就是选择本身（不是单选 + 确定）
    assert!(
        dialog_button_rendered(cx, "editor-dialog-editor-switch-per-statement")
            && dialog_button_rendered(cx, "editor-dialog-editor-switch-single"),
        "粒度两个选项都该在对话框里"
    );
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Sql),
        "未确认前模式不得变"
    );

    // 确认（真路径是按钮回调，这里直接走同一个入口）
    cx.update(|window, cx| window.close_dialog(cx));
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.confirm_mode_switch(
                EditorMode::Analysis,
                CellGranularity::PerStatement,
                window,
                cx,
            )
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Analysis),
        "模式落到文档上"
    );
    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(
        text.contains("-- %%"),
        "按语句拆分后文本层要有单元分隔标记：{text}"
    );
}

/// 免确认的切换（文本 → SQL）不弹窗：不是所有切换都要打断用户
#[gpui_kit::test]
fn switching_from_text_to_sql_needs_no_dialog(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\sql\notes.txt",
            "select 1;",
            EditorMode::Text,
        ))
        .id()
        .clone();
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-free-switch");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.request_mode_switch(EditorMode::Sql, window, cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "文本 → SQL 没有代价，不必打断用户"
    );
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Sql)
    );
    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert_eq!(text, "select 1;", "免确认的切换不动内容");
}

/// 确认回调与粒度要能独立驱动（宿主也能走这条路；粒度不参与内容变换时不该改内容）
#[gpui_kit::test]
fn confirmed_switch_recomputes_the_plan_with_the_chosen_granularity(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\single.sql", "select 1; select 2;");
    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-single-cell");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.confirm_mode_switch(
                EditorMode::Analysis,
                CellGranularity::Single,
                window,
                cx,
            )
        });
    });

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(
        !text.contains("-- %%"),
        "整篇一个单元不该出现分隔标记：{text}"
    );
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::Analysis)
    );
}

// ═══════════════════════════════════════════════════════════════════════
// 对话框流程（A9）：关闭三态 / 另存为
// ═══════════════════════════════════════════════════════════════════════

/// 假“系统文件对话框”：把用户选的路径固定下来（真写盘，不碰宿主与 rfd）
fn attach_picker(shared: &EditorShared, picked: Option<PathBuf>) {
    shared.attach_save_path_picker(Rc::new(move |_current, _default_name| picked.clone()));
}

/// 脏文档关闭：先弹三态确认，**确认之前一张纸也不动**
#[gpui_kit::test]
fn closing_a_dirty_document_asks_before_touching_anything(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\dirty.sql", "select 1;");
    shared.update(|service| service.set_content(&id, "select 2;".to_string()));

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-close-ask");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });
    cx.update(|window, cx| request_close_document(&area, panel.clone(), window, cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "脏文档关闭要先问"
    );
    assert!(shared.service().find(&id).is_some(), "未确认前文档不能关");
    assert!(!cx.update(|_window, cx| panel.read(cx).is_closed()));

    // 取消：文档与面板都留着
    let (area, panel) = (area.clone(), panel.clone());
    cx.update(|window, cx| {
        window.close_dialog(cx);
        resolve_close_choice(&area, panel.clone(), crate::view::dialogs::CloseChoice::Cancel, window, cx);
    });
    assert!(shared.service().find(&id).is_some(), "取消后文档还在");
    assert!(!cx.update(|_window, cx| panel.read(cx).is_closed()));
}

/// “不保存”分支：直接丢改动关掉（用户明确选了不保存）
#[gpui_kit::test]
fn discarding_unsaved_changes_closes_without_writing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir("discard");
    let path = dir.join("keep.sql");
    std::fs::write(&path, "select 1;").expect("写盘");

    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(&path, "select 1;", EditorMode::Sql))
        .id()
        .clone();
    shared.update(|service| service.set_content(&id, "select 999;".to_string()));

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-discard");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });

    cx.update(|window, cx| {
        resolve_close_choice(
            &area,
            panel.clone(),
            crate::view::dialogs::CloseChoice::Discard,
            window,
            cx,
        )
    });

    assert!(shared.service().find(&id).is_none(), "不保存 = 文档关掉");
    assert!(cx.update(|_window, cx| panel.read(cx).is_closed()));
    assert_eq!(
        std::fs::read_to_string(&path).expect("读盘"),
        "select 1;",
        "磁盘上的内容不得被未保存的改动覆盖"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// “保存”分支 + 未命名文档：走另存为（路径选择端口）→ 写盘成功才关
#[gpui_kit::test]
fn saving_an_untitled_document_writes_it_before_closing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir("save-then-close");
    let target = dir.join("new.sql");

    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("select 7;", EditorMode::Sql))
        .id()
        .clone();
    shared.update(|service| service.set_content(&id, "select 7;".to_string()));
    attach_picker(&shared, Some(target.clone()));

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-save-close");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });

    cx.update(|window, cx| {
        resolve_close_choice(
            &area,
            panel.clone(),
            crate::view::dialogs::CloseChoice::Save,
            window,
            cx,
        )
    });

    assert_eq!(
        std::fs::read_to_string(&target).expect("另存为应真的写盘"),
        "select 7;"
    );
    assert!(shared.service().find(&id).is_none(), "写完才能关");
    assert!(cx.update(|_window, cx| panel.read(cx).is_closed()));
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "顺利写盘不该弹二次确认"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// 用户在另存为里取消：文档**不关**，而且不能当已经存过
#[gpui_kit::test]
fn cancelling_save_as_keeps_the_document_open(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("select 7;", EditorMode::Sql))
        .id()
        .clone();
    shared.update(|service| service.set_content(&id, "select 8;".to_string()));
    attach_picker(&shared, None);

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-save-cancel");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });

    cx.update(|window, cx| {
        resolve_close_choice(
            &area,
            panel.clone(),
            crate::view::dialogs::CloseChoice::Save,
            window,
            cx,
        )
    });

    assert!(shared.service().find(&id).is_some(), "取消另存为就不能关");
    assert!(!cx.update(|_window, cx| panel.read(cx).is_closed()));
    assert!(
        shared.service().find(&id).expect("文档").is_dirty(),
        "没写盘就仍是脏的"
    );
}

/// 未接入系统文件对话框时，“保存”分支不能静默吞掉：状态栏要说原因
#[gpui_kit::test]
fn saving_without_a_path_picker_reports_why(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::untitled("select 7;", EditorMode::Sql))
        .id()
        .clone();
    // 不注入 picker：模拟宿主未接系统文件对话框

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-no-picker");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });

    cx.update(|window, cx| {
        resolve_close_choice(
            &area,
            panel.clone(),
            crate::view::dialogs::CloseChoice::Save,
            window,
            cx,
        )
    });

    assert!(shared.service().find(&id).is_some(), "没存成就不许关");
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.expect("要留原因").contains("系统文件对话框"),
        "原因要说清是宿主未接入"
    );
}

/// `Ctrl+Shift+S`（另存为，不关文档）：写盘、标题跟随、清脏；取消则什么都不变
#[gpui_kit::test]
fn save_as_rewrites_the_path_and_keeps_the_document_open(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let dir = temp_dir("save-as");
    let target = dir.join("renamed.sql");

    let (shared, id) = shared_with_document(r"D:\sql\original.sql", "select 1;");
    shared.update(|service| service.set_content(&id, "select 2;".to_string()));
    attach_picker(&shared, Some(target.clone()));

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-save-as");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());

    let saved = cx.update(|_window, cx| request_save_as(&panel, cx));
    assert!(saved, "另存为应当写盘成功");

    assert_eq!(std::fs::read_to_string(&target).expect("读盘"), "select 2;");
    let (path, title, dirty) = {
        let service = shared.service();
        let doc = service.find(&id).expect("文档");
        (
            doc.path().map(|path| path.to_path_buf()),
            doc.title().to_string(),
            doc.is_dirty(),
        )
    };
    assert_eq!(path.as_deref(), Some(target.as_path()), "路径改为另存目标");
    assert_eq!(title, "renamed.sql", "标签标题跟随文件名");
    assert!(!dirty, "另存为即已保存");
    assert!(shared.service().find(&id).is_some(), "另存为不关文档");
    let _ = std::fs::remove_dir_all(&dir);
}

// ══════════════════════════════════════════════════════════════════════
// B1：连接绑定
// ══════════════════════════════════════════════════════════════════════

/// 假连接端口：固定选项表 + 可注入的建连失败；建连成功后运行态变真
struct FakeConnections {
    options: Vec<ConnectionOption>,
    fail_with: Option<String>,
    connected: std::cell::RefCell<Vec<String>>,
}

impl FakeConnections {
    fn new(options: Vec<ConnectionOption>) -> Self {
        Self {
            options,
            fail_with: None,
            connected: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl ConnectionsPort for FakeConnections {
    fn options(&self) -> Vec<ConnectionOption> {
        let connected = self.connected.borrow();
        self.options
            .iter()
            .map(|option| ConnectionOption {
                connected: option.connected || connected.contains(&option.id),
                ..option.clone()
            })
            .collect()
    }

    fn ensure_connected(&self, conn_id: &str) -> Result<(), String> {
        if let Some(reason) = &self.fail_with {
            return Err(reason.clone());
        }
        self.connected.borrow_mut().push(conn_id.to_string());
        Ok(())
    }
}

fn option(id: &str, short: &str, name: &str) -> ConnectionOption {
    ConnectionOption {
        id: id.to_string(),
        short: short.to_string(),
        name: name.to_string(),
        connected: false,
    }
}

/// 绑定一个连接：状态栏跟着变，且**绑定是文档属性**（换文档不影响）
#[gpui_kit::test]
fn binding_a_connection_shows_up_in_the_status_bar(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\bound.sql", "select 1;");
    let port = Rc::new(FakeConnections::new(vec![option("P_orders", "P", "orders")]));
    shared.attach_connections(port);

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    // 初始：未绑定（会跟随当前连接，状态栏要如实说明）
    assert_eq!(
        shared.connection_status_text(None),
        "○ 未绑定连接",
        "未绑定要看得懂"
    );

    // 绑定：先自动建连（假端口成功）→ 写回文档 → 状态栏带运行态点
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_orders".to_string()), cx)
        });
    });
    assert_eq!(
        shared.service().connection_for(&id).as_deref(),
        Some("P_orders"),
        "绑定要落到文档上"
    );
    assert_eq!(shared.connection_status_text(Some("P_orders")), "●P·orders");
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(
        message.expect("绑定要有可见反馈").contains("已绑定"),
        "成功也要留痕（否则用户不知道自自动建连了没）"
    );

    // 解绑：回到“跟随当前连接”
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.bind_connection(None, cx));
    });
    assert_eq!(shared.service().connection_for(&id), None);
}

/// 建连失败**不绑定**：半绑定比不绑定更难排查
#[gpui_kit::test]
fn a_failed_connection_leaves_the_document_unbound(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\unbound.sql", "select 1;");
    let mut port = FakeConnections::new(vec![option("P_down", "P", "down")]);
    port.fail_with = Some("端口不可达".to_string());
    shared.attach_connections(Rc::new(port));

    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_down".to_string()), cx)
        });
    });

    assert_eq!(shared.service().connection_for(&id), None, "建连失败不得绑定");
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    let message = message.expect("失败必须留原因");
    assert!(message.contains("连接不可用"), "{message}");
    assert!(message.contains("端口不可达"), "原因要原样带上来：{message}");
}

// ══════════════════════════════════════════════════════════════════════
// B11：宿主发起的执行（导航「查看数据」打开后自动跑一次）
// ══════════════════════════════════════════════════════════════════════

/// 宿主用 `run_all` 触发的执行与 `Ctrl+Shift+Enter` 同一条路（同一套目标解析与回填）
#[gpui_kit::test]
fn the_host_can_trigger_a_full_run(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _seen_conn) = shared_with_runner("select 1;\nselect 2;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 不按键：宿主直接调公开入口（导航「查看数据」路径就是这一条）
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.run_all(cx)));
    wait_for_result(cx, &panel);

    assert_eq!(
        seen.lock().expect("锁").as_slice(),
        ["select 1;\nselect 2;".to_string()],
        "宿主触发的是整篇脚本"
    );

    // 空文档：不打扰执行器（与快捷键路径同一判据）
    let shared_empty = EditorShared::new();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    shared_empty.attach_runner(std::sync::Arc::new(CountingRunner { calls: calls.clone() }));
    let id_empty = shared_empty
        .open(OpenRequest::untitled("-- 只有注释\n", EditorMode::Sql))
        .id()
        .clone();
    let (panel_empty, cx) = open_panel(cx, &shared_empty, &id_empty);
    cx.update(|_window, cx| panel_empty.update(cx, |panel, cx| panel.run_all(cx)));
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "没有可执行内容就该被拒，不该打扰执行器"
    );
}

/// 只数调用次数的执行器
struct CountingRunner {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl QueryRunner for CountingRunner {
    fn run(&self, _connection: Option<&str>, _sql: &str, _options: execution::RunOptions) -> Result<QueryData, String> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(QueryData::default())
    }
}

/// 绑定的连接要**真的**传到执行器（架构 §12 #26：以前只能走“当前活动连接”）
#[gpui_kit::test]
fn the_bound_connection_reaches_the_execution_port(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);
    let (shared, id, _seen, seen_connections) = shared_with_runner("select 1;", EditorMode::Sql);
    shared.attach_connections(Rc::new(FakeConnections::new(vec![option(
        "P_orders",
        "P",
        "orders",
    )])));
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.bind_connection(Some("P_orders".to_string()), cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));

    cx.simulate_keystrokes("ctrl-enter");
    wait_for_result(cx, &panel);

    assert_eq!(
        seen_connections.lock().expect("锁").as_slice(),
        [Some("P_orders".to_string())],
        "执行器必须收到文档绑定的连接"
    );

    // 【B5】结果也要记住它在哪个连接上跑出来的（结果工具栏显示来源）
    let entry_connection = cx.update(|_window, _cx| {
        shared
            .results()
            .active(&id)
            .and_then(|entry| entry.connection.clone())
    });
    assert_eq!(
        entry_connection.as_deref(),
        Some("P_orders"),
        "结果记录要带上来源连接"
    );
    let toolbar = cx
        .update(|_window, cx| panel.read(cx).result_toolbar_for_test())
        .expect("有结果就有工具栏");
    assert_eq!(
        toolbar.connection.as_deref(),
        Some("●P·orders"),
        "工具栏拿到的是可读的连接段"
    );
}

/// 工具栏在 SQL 模式给出连接选择器（没有端口也说清楚，不假装有得选）
#[gpui_kit::test]
fn the_toolbar_shows_the_connection_picker_in_sql_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\picker.sql", "select 1;");
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-connection").is_some(),
        "SQL 模式应当有连接选择器"
    );

    // 未接端口：选项表为空（菜单会直说“未接入连接列表”）
    assert!(shared.connection_options().is_empty());
    assert!(!shared.has_connections());

    // 文本模式：不与数据库通信 → 连不上问题都不该问，选择器不出现
    shared.update(|service| {
        service.set_mode(&id, EditorMode::Text);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("editor-connection").is_none());
}

// ══════════════════════════════════════════════════════════════════════
// B16：关闭口径 + 工具栏分层
// ══════════════════════════════════════════════════════════════════════

/// 脏文档不给标签 ✕：Dock 没有“关闭前否决”钩子，能问用户的入口只留 `Ctrl+W`
#[gpui_kit::test]
fn a_dirty_document_has_no_close_glyph(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\glyph.sql", "select 1;");
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    // 刚打开：干净 → 有 ✕（`closable` 是 Dock 显示 ✕ 的判据）
    assert!(
        cx.update(|_window, cx| panel.read(cx).closable(cx)),
        "干净文档应当可关（标签上有 ✕）"
    );

    // 改动 → 脏 → 收起 ✕（否则 Dock 会直接移除面板，静默丢掉改动）
    shared.update(|service| service.set_content(&id, "select 2;".to_string()));
    assert!(
        !cx.update(|_window, cx| panel.read(cx).closable(cx)),
        "脏文档不得给 ✕（Dock 无否决钩子，只能靠收起入口）"
    );
    // 标签脏点仍然在（用户看得出“有未保存改动”）
    assert!(cx.update(|_window, cx| panel.read(cx).is_dirty()));

    // 保存（清脏）→ ✕ 回来
    shared.update(|service| {
        service.mark_saved(&id);
    });
    assert!(
        cx.update(|_window, cx| panel.read(cx).closable(cx)),
        "保存后 ✕ 应当回来"
    );
}

/// 工具栏按模式分层：文本 / 分析模式不放“执行”（那是 SQL 模式的能力，不在 UI 上假装）
#[gpui_kit::test]
fn the_toolbar_only_offers_execution_in_sql_mode(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\layers.sql", "select 1;");
    let (panel, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, id, window, cx))
    };

    let exec_present = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds("editor-exec-run").is_some()
    };

    // SQL 模式：模式指示器 + 执行
    assert!(exec_present(cx), "SQL 模式应当有执行按钮");
    assert!(
        cx.debug_bounds("editor-mode-indicator").is_some(),
        "模式指示器在所有模式下都在"
    );

    // 文本模式：只剩模式指示器（不与数据库通信）
    shared.update(|service| {
        service.set_mode(&id, EditorMode::Text);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(!exec_present(cx), "文本模式不得出现执行按钮");
    assert!(cx.debug_bounds("editor-mode-indicator").is_some());

    // 分析模式：执行属笔记级动作（1c），现在也不放
    shared.update(|service| {
        service.set_mode(&id, EditorMode::Analysis);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(!exec_present(cx), "分析模式的执行随单元落地（1c）");
}

// ===== B5：结果工具栏（复制 / 刷新 / 影响行数 / 截断 / 分栏）=====

/// 有网格就有复制，剪贴板里是与网格一致的 TSV
#[gpui_kit::test]
fn copying_the_active_result_puts_tsv_on_the_clipboard(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert!(
        dialog_button_rendered(cx, "editor-result-copy"),
        "有网格就该有复制入口"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.copy_active_result(cx)));
    let text = cx.update(|_window, app| {
        app.read_from_clipboard().and_then(|item| item.text())
    });
    assert_eq!(
        text.as_deref(),
        Some("n\tnote\n1\ta"),
        "复制的是网格那份 TSV（表头 + 行）"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("复制要有回执");
    assert!(message.contains("已复制 1 行"), "{message}");
}

/// 刷新重跑的是**当前选中那份**的 SQL，并且原位替换（结果集数不变）
#[gpui_kit::test]
fn refreshing_reruns_the_selected_result_in_place(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, seen, _) = shared_with_runner("select 1;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select first", execution::ResultPlacement::Replace);
    run_statement(cx, &panel, "select second", execution::ResultPlacement::NewSet);
    run_statement(cx, &panel, "select third", execution::ResultPlacement::NewSet);
    assert_eq!(shared.results().set_count(&id), 3);

    // 用户切到最后一份，刷新它
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.select_result_set(2, cx)));
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.refresh_active_result(cx)));
    wait_for_all_pending(cx, &panel);

    let sqls = seen.lock().expect("锁").clone();
    assert_eq!(
        sqls,
        ["select first", "select second", "select third", "select third"],
        "刷新重跑的是选中那份的 SQL"
    );
    assert_eq!(
        shared.results().set_count(&id),
        3,
        "原位替换：刷出不新开一份结果集"
    );
}

/// 写语句：工具栏报影响行数，且不摆复制（没有网格）
#[gpui_kit::test]
fn a_write_statement_reports_affected_rows_and_offers_no_copy(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("insert into t values (1);");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "insert into t values (1)",
        execution::ResultPlacement::Replace,
    );

    let summary = cx
        .update(|_window, cx| panel.read(cx).result_summary_for_test())
        .expect("写语句也要有结果区文案");
    assert!(summary.starts_with("影响 3 行"), "{summary}");
    assert!(
        !dialog_button_rendered(cx, "editor-result-copy"),
        "写语句没有可复制的结果集"
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-refresh"),
        "有 SQL 就能重跑"
    );
}

/// 截断是警告：工具栏拿到**真实行数**，且已抓到的行仍然可复制
#[gpui_kit::test]
fn truncation_is_reported_as_a_warning(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select truncated;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select truncated", execution::ResultPlacement::Replace);

    let toolbar = cx
        .update(|_window, cx| panel.read(cx).result_toolbar_for_test())
        .expect("有结果就有工具栏");
    assert_eq!(toolbar.rows, Some(3), "工具栏报行数");
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test())
        .expect("有网格就有状态行");
    assert_eq!(
        status.truncated_hint.as_deref(),
        Some("已截断至 3 行"),
        "截断提示要说清拿到多少行"
    );
    let (sql, can_copy) = cx.update(|_window, cx| panel.read(cx).result_actions_for_test());
    assert!(can_copy, "被截断的结果集仍能复制已抓到的行");
    assert_eq!(sql.as_deref(), Some("select truncated"));
}

/// 结果区在可拖拽分栏里：有结果时出现在下半区，且编辑区没被挤掉
#[gpui_kit::test]
fn the_result_pane_sits_in_a_split_without_squeezing_the_editor(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 还没有结果：结果区整个不占位置（不显示空壳）
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-pane").is_none(),
        "没结果时不摆空壳"
    );
    let before = cx
        .debug_bounds("editor-code-area")
        .expect("编辑区在")
        .size
        .height;

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let pane = cx
        .debug_bounds("editor-result-pane")
        .expect("有结果时结果区在");
    assert!(pane.size.height.as_f32() > 0.0, "结果区要有高度");
    let after = cx
        .debug_bounds("editor-code-area")
        .expect("编辑区还在")
        .size
        .height;
    assert!(
        after.as_f32() > 0.0,
        "编辑区不能被结果区压成 0（实得 {after:?}）"
    );
    assert!(
        after < before,
        "分栏生效的标志是编辑区让出了一部分高度（{before:?} → {after:?}）"
    );
}

// ===== B6：错误回填（诊断 + 定位 + 聚焦）=====

/// 能定位的失败：诊断画在出错词上、光标跳过去并选中它、状态栏说出位置
#[gpui_kit::test]
fn a_locatable_failure_marks_the_word_and_moves_the_caret(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(LocatedFailureRunner));
    let id = shared
        .open(OpenRequest::untitled(
            "select 1;\nselect * from t wheree x = 1;",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    run_statement(
        cx,
        &panel,
        "select * from t wheree x = 1",
        execution::ResultPlacement::Replace,
    );

    // 诊断是回填里当场画的（真机与 headless 走同一条路）
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).diagnostics_for_test(cx)),
        1,
        "出错词上要有一条诊断"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("失败要有提示");
    assert!(
        message.contains("no such column: wheree") && message.contains("第 2 行 第 17 列"),
        "提示要说清错误 + 位置：{message}"
    );

    // 光标跳过去这一步：真机上回填发生在**后台轮询**（没有窗口）里，走的是面板存下的窗口句柄；
    // headless 里那条路会撞上“不能在窗口更新里再更新窗口”（`update_window` 直接返回 Err），
    // 所以这里直接驱动落地入口——与“对话框按钮的真点击”同一口径（架构 §12 #29）。
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.jump_to_error_site_with(window, cx))
    });
    // 文档：`select 1;\n`（10 字节）+ `select * from t `（16 字节）→ 出错词在第 2 行第 17 列
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).selected_range_for_test(cx)),
        26..32,
        "光标要选中出错的那个词"
    );
}

/// 认不出位置的失败：只提示，不动光标、不画诊断（定位错比不定位更糟）
#[gpui_kit::test]
fn an_unlocatable_failure_leaves_the_caret_alone(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    // `ScriptRunner` 的 `boom` 里没有任何可定位的线索
    let (shared, id, _seen, _connections) = shared_with_runner("select boom;", EditorMode::Sql);
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_caret_for_test(0, cx))
    });
    run_statement(cx, &panel, "select boom", execution::ResultPlacement::Replace);

    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).selected_range_for_test(cx)),
        0..0,
        "没有位置就不动光标"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).diagnostics_for_test(cx)),
        0,
        "没有位置就不画诊断"
    );
    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("失败要有提示");
    assert!(message.contains("boom"), "{message}");
    assert!(!message.contains("第 "), "没有位置就别编一个出来：{message}");
}

/// 下一次成功要把旧的诊断清掉（不留“已经修好了还红着”的假象）
#[gpui_kit::test]
fn a_successful_run_clears_the_error_marks(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(LocatedFailureRunner));
    let id = shared
        .open(OpenRequest::untitled(
            "select * from t wheree x = 1;\nselect 1;",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    run_statement(
        cx,
        &panel,
        "select * from t wheree x = 1",
        execution::ResultPlacement::Replace,
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).diagnostics_for_test(cx)),
        1
    );

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).diagnostics_for_test(cx)),
        0,
        "成功之后不该再留着上次的诊断"
    );
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.is_none(), "成功要清掉旧提示：{message:?}");
}

// ===== 原型对齐：结果区 ⑤⑥⑦（原型 §2.4）=====

/// 竖向顺序按原型：⑤ 标签条 → ⑥ 工具栏 → 网格 → ⑦ 状态行
#[gpui_kit::test]
fn the_result_area_stacks_the_prototype_blocks_in_order(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 两份结果才有标签条（⑤ 是“多结果”的切换器）
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    run_statement(cx, &panel, "select 2", execution::ResultPlacement::NewSet);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let y = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} 没画出来"))
            .origin
            .y
            .as_f32()
    };
    let tabs = y(cx, "editor-result-tabs");
    let toolbar = y(cx, "editor-result-toolbar");
    let grid = y(cx, "editor-result-grid");
    let status = y(cx, "editor-result-status-row");
    assert!(
        tabs < toolbar && toolbar < grid && grid < status,
        "原型 §2.4 的顺序是 ⑤标签条 → ⑥工具栏 → 网格 → ⑦状态行（实得 {tabs} / {toolbar} / {grid} / {status}）"
    );
}

/// ⑦ 的“已选第 N 行”跟着表格的选中走（点行不改结果集，只重画那一行）
#[gpui_kit::test]
fn the_status_row_reports_the_selected_row(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_toolbar_runner("select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);
    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);

    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test())
        .expect("有网格就有状态行");
    assert_eq!(status.selected_row, None, "刚跑完没选中任何行");

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.select_grid_row_for_test(0, cx))
    });
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test())
        .expect("有网格就有状态行");
    assert_eq!(status.selected_row, Some(1), "选中第一行（1 基）");
    assert!(
        dialog_button_rendered(cx, "editor-result-status-row"),
        "状态行要真的画出来"
    );
}

/// 能定位的失败：卡片带「定位到第 N 行」按钮，且它真能把光标送到出错词
#[gpui_kit::test]
fn the_error_card_offers_a_working_locate_button(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(LocatedFailureRunner));
    let id = shared
        .open(OpenRequest::untitled(
            "select * from t wheree x = 1;",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    run_statement(
        cx,
        &panel,
        "select * from t wheree x = 1",
        execution::ResultPlacement::Replace,
    );

    let card = cx
        .update(|_window, cx| panel.read(cx).result_error_card_for_test())
        .expect("失败要有错误卡片");
    assert_eq!(card.location.as_deref(), Some("第 1 行 第 17 列"));
    assert!(
        dialog_button_rendered(cx, "editor-result-error-card"),
        "卡片要真的画出来"
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-error-locate"),
        "能定位就有按钮"
    );
    assert!(
        !dialog_button_rendered(cx, "editor-result-grid"),
        "失败时画卡片，不画空网格"
    );

    // 按钮按下去（headless 里直接驱动落地入口）：光标落到出错词上
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.jump_to_error_site_with(window, cx))
    });
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).selected_range_for_test(cx)),
        16..22,
        "选中 `wheree`"
    );
}

/// 卡片上的「复制」把驱动原文放进剪贴板（拿去搜索 / 报 bug）
#[gpui_kit::test]
fn the_error_card_can_copy_the_driver_message(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(LocatedFailureRunner));
    let id = shared
        .open(OpenRequest::untitled(
            "select * from t wheree x = 1;",
            EditorMode::Sql,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(
        cx,
        &panel,
        "select * from t wheree x = 1",
        execution::ResultPlacement::Replace,
    );
    assert!(
        dialog_button_rendered(cx, "editor-result-error-copy"),
        "卡片上要有复制"
    );

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.copy_error_message(cx)));
    let text = cx.update(|_window, app| {
        app.read_from_clipboard().and_then(|item| item.text())
    });
    let text = text.expect("剪贴板里要有东西");
    assert!(text.contains("no such column: wheree"), "{text}");
    assert!(text.contains("第 1 行 第 17 列"), "位置一起复制走：{text}");
}

// ===== B5b：分段抓取（⑦ 状态行里的分页位）=====

/// 首段拿满：状态行报 `2+` 并摆「取下一段」；取回来接在同一份结果上
#[gpui_kit::test]
fn fetching_the_next_segment_appends_to_the_same_result(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id, asked) = shared_with_segment_runner("select n from t;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select n from t", execution::ResultPlacement::Replace);
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        2,
        "首段两行"
    );
    let summary = cx
        .update(|_window, cx| panel.read(cx).result_summary_for_test())
        .expect("有结果");
    assert!(summary.contains("行数 2"), "{summary}");
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test())
        .expect("有网格就有状态行");
    assert!(status.has_more, "拿满了 → 还有下一段");
    assert!(
        dialog_button_rendered(cx, "editor-result-more"),
        "还有下一段就摆「取下一段」"
    );

    // 按下去（headless 里直接驱动落地入口）：再拿两行，接在同一份结果上
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.fetch_more(cx)));
    wait_for_all_pending(cx, &panel);

    assert_eq!(
        asked.lock().expect("锁").as_slice(),
        [(2, execution::SEGMENT_ROWS)],
        "要的正是“已经拿到的行数”之后那一段"
    );
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        4,
        "两段接起来"
    );
    assert_eq!(
        shared.results().set_count(&id),
        1,
        "取下一段不新开结果集"
    );
    let status = cx
        .update(|_window, cx| panel.read(cx).result_status_for_test())
        .expect("还有状态行");
    assert_eq!(status.total_rows, 4);
    assert!(!status.has_more, "最后一段没拿满 → 到底了");
    assert!(
        !dialog_button_rendered(cx, "editor-result-more"),
        "到底了就不摆按钮"
    );
    let message = cx.update(|_window, cx| panel.read(cx).message.clone());
    assert!(message.is_none(), "一次成功的取段不留提示：{message:?}");
}

/// 执行器不支持取下一段：按钮按下去要留一句可读原因（不静默、不假装成功）
#[gpui_kit::test]
fn fetching_a_segment_without_support_says_so(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(MoreUnsupportedRunner));
    let id = shared
        .open(OpenRequest::untitled("select 1;", EditorMode::Sql))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);

    run_statement(cx, &panel, "select 1", execution::ResultPlacement::Replace);
    assert!(dialog_button_rendered(cx, "editor-result-more"), "有下一段就摆按钮");

    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.fetch_more(cx)));
    wait_for_all_pending(cx, &panel);

    let message = cx
        .update(|_window, cx| panel.read(cx).message.clone())
        .expect("失败要留原因");
    assert!(message.contains("不支持分段抓取"), "{message}");
    assert_eq!(
        cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx)),
        1,
        "取不回来不该动已抓到的行"
    );
}
