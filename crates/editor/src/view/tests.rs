//! 宿主面板的窗口级测试（GPUI headless 窗口）
//!
//! 覆盖 A2 的验收口径（多文档并存、标题跟随、脏点显示、面板可关闭、渲染不 panic）
//! 与 A10 的动作口径（快捷键真按下 → 动作真生效）。
//!
//! 注意：这里**不使用** `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的 `test`
//! 属性宏带入作用域，而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会因此解析到它自己
//! （recursion limit reached）。所有依赖显式列举。

use std::path::PathBuf;

use gpui_kit::component::dock::{
    BasePanel as _, DockArea, DockPlacement, DockSkin, Panel as _,
};
use gpui_kit::{
    AppContext as _, Context, Entity, Focusable as _, IntoElement, KeyBinding, ParentElement as _,
    Render, Styled as _, TestAppContext, Window, div,
};

use crate::commands::{SaveDocument, ToggleComment};
use crate::model::{DocumentId, EditorMode};
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::view::host::{EditorHostPanel, close_document_in_dock};

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
    let dir = std::env::temp_dir().join(format!("rds_editor_actions_{name}_{}", std::process::id()));
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
    assert!(shared.service().find(&ids[1]).is_some(), "另一个文档不受影响");
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
