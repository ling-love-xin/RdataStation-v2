//! 编辑器会话的真机验证（A12）：关闭文档 → 重开 → 光标与模式都回来
//!
//! 链路：面板 `on_removed` → `EditorShared::save_session` → `WorkbenchSessionStore`
//! → 全局库的 `editor_contexts` 表（真临时 SQLite 文件）→ 重启后 `load_latest_session`
//! → `OpenRequest::file`（**用会话内容，不重读磁盘**）→ `restore_session`（光标/选区回内核）。
//!
//! 不碰全局单例：`WorkbenchSessionStore::over(store)` 直接接一份临时库的 store
//! （生产走 `open()` → `get_global_db_manager()`）。

use std::rc::Rc;

use editor::model::EditorMode;
use editor::service::OpenRequest;
use editor::shared::EditorShared;
use editor::view::host::{EditorHostPanel, close_document_in_dock};
use engine::persistence::global_db::GlobalSqlitePool;
use engine::persistence::workbench_context_store::WorkbenchContextStore;
use gpui_kit::component::dock::{DockArea, DockPlacement, DockSkin};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    TestAppContext, Window, div,
};
use rds_workbench::services::editor_session::WorkbenchSessionStore;

/// 测试用最小宿主（同 `editor` crate 内部的那套：动作要能派发就得有已渲染的树）
struct Harness {
    area: Entity<DockArea>,
    panels: Vec<Entity<EditorHostPanel>>,
}

impl Render for Harness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.area.clone())
    }
}

/// 建一个真临时 SQLite 上的 store
fn temp_store(tag: &str) -> (std::path::PathBuf, WorkbenchContextStore) {
    let dir = std::env::temp_dir().join(format!("rds_editor_session_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let db = dir.join("global.db");

    // 建池要 async；建完立刻离开 async 上下文——存储层是同步 API（自建 runtime 驱动），
    // 不能在 tokio 运行时里调用（`acquire_sync` 会返回可读错误）。
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let pool = runtime.block_on(async {
        std::sync::Arc::new(
            GlobalSqlitePool::new(db.clone(), 1)
                .await
                .expect("建池"),
        )
    });
    drop(runtime);

    let store = WorkbenchContextStore::new(pool).expect("打开 store");
    (dir, store)
}

#[gpui_kit::test]
fn closing_a_document_saves_its_session_and_reopening_restores_cursor_and_mode(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (dir, store) = temp_store("roundtrip");
    let store = Rc::new(WorkbenchSessionStore::over(store));

    // ===== 第一次运行：打开 → 移动光标 / 切模式 → 关闭 =====
    let first = EditorShared::new();
    first.attach_session_store(store.clone());
    let path = dir.join("report.sql");
    std::fs::write(&path, "select 1;\nselect 2;").expect("写盘");
    let document = first
        .open(OpenRequest::file(&path, "select 1;\nselect 2;", EditorMode::Sql))
        .id()
        .clone();

    let (harness, cx) = {
        let first = first.clone();
        let document = document.clone();
        cx.add_window_view(move |window, cx| {
            let (area, _skin) = DockSkin::dock_area("editor-session", Some(1), window, cx);
            let panel = cx.new(|cx| EditorHostPanel::new(first.clone(), document, window, cx));
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

    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    // 光标停在第 2 句里，并切到分析模式（模式是文档属性）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_caret_for_test(12, cx));
    });
    first.update(|service| service.set_mode(&document, EditorMode::Analysis));

    // 关闭 → `on_removed` 落会话
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });
    let closed = cx.update(|window, cx| close_document_in_dock(&area, panel, window, cx));
    assert!(closed, "干净文档应当被关掉");
    assert!(first.service().find(&document).is_none(), "文档已关闭");

    // ===== 第二次运行（新进程的等价物）：新一份共享状态，读回会话 =====
    let second = EditorShared::new();
    second.attach_session_store(store.clone());
    let session = second
        .load_latest_session()
        .expect("读会话")
        .expect("应当有可恢复的会话");

    assert_eq!(session.mode, EditorMode::Analysis, "模式要跨重启保留");
    assert_eq!(session.content, "select 1;\nselect 2;");
    assert_eq!(session.cursor, 12, "光标要跨重启保留");

    // 用会话内容重开（不读磁盘：用户上次可能没保存）
    let reopened = second
        .open(OpenRequest::file(
            std::path::PathBuf::from(session.path.clone().expect("有路径")),
            session.content.clone(),
            session.mode,
        ))
        .id()
        .clone();
    let (panel2, cx) = {
        let second = second.clone();
        let reopened = reopened.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(second, reopened, window, cx))
    };
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        panel2.update(cx, |panel, cx| panel.restore_session(&session, window, cx));
    });

    let (cursor_after, mode_after) = cx.update(|_window, cx| {
        (
            panel2.read(cx).selected_range_for_test(cx).start,
            second
                .service()
                .find(&reopened)
                .map(|doc| doc.mode())
                .unwrap_or(EditorMode::Text),
        )
    });
    assert_eq!(cursor_after, 12, "重开后光标回到原处");
    assert_eq!(mode_after, EditorMode::Analysis, "重开后模式回到分析模式");

    // 渲染一帧确认恢复后界面可画
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui_kit::test]
fn an_untitled_document_has_no_session_to_save(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (dir, store) = temp_store("untitled");
    let store = Rc::new(WorkbenchSessionStore::over(store));

    let shared = EditorShared::new();
    shared.attach_session_store(store.clone());
    let document = shared
        .open(OpenRequest::untitled("select 1;", EditorMode::Sql))
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let document = document.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, document, window, cx))
    };

    // 未命名文档没有稳定标识（重启后认不回来）→ 面板不生成会话，也不该写库
    let snapshot = cx.update(|_window, cx| panel.read(cx).session_snapshot(cx));
    assert!(snapshot.is_none(), "未命名文档不该产生会话");
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.save_session_now(cx)));
    assert!(
        shared.load_latest_session().expect("读").is_none(),
        "库裡不该多出一条未命名会话"
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// 有路径的文档才会生成会话（会话标识就是路径键）
#[gpui_kit::test]
fn a_saved_document_produces_a_session_keyed_by_its_path(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (dir, store) = temp_store("keyed");
    let store = Rc::new(WorkbenchSessionStore::over(store));

    let shared = EditorShared::new();
    shared.attach_session_store(store.clone());
    let path = dir.join("keyed.sql");
    std::fs::write(&path, "select 1;").expect("写盘");
    let document = shared
        .open(OpenRequest::file(&path, "select 1;", EditorMode::Sql))
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let document = document.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(shared, document, window, cx))
    };

    let session = cx
        .update(|_window, cx| panel.read(cx).session_snapshot(cx))
        .expect("有路径就有会话");
    assert!(
        session.id.ends_with("keyed.sql"),
        "会话标识应当是路径键：{}",
        session.id
    );
    assert_eq!(session.mode, EditorMode::Sql);
    assert_eq!(session.content, "select 1;");

    let _ = std::fs::remove_dir_all(dir);
}

/// 【B1 余项】连接绑定跟会话一起跨重启：真 SQLite 写进 `editor_contexts.connection_id` 再读回来
///
/// 这里**不接连接端口**，走的正是“未接端口不校验也不丢”那一条（校验分支由 editor 的
/// 窗口用例带假端口覆盖）：真库往返 + 恢复时写回文档，两件事都在本用例里。
#[gpui_kit::test]
fn the_connection_binding_survives_a_restart(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (dir, store) = temp_store("binding");
    let store = Rc::new(WorkbenchSessionStore::over(store));

    // ===== 第一次运行：打开 → 绑定连接 → 关（落会话）=====
    let first = EditorShared::new();
    first.attach_session_store(store.clone());
    let path = dir.join("bind.sql");
    std::fs::write(&path, "select 1;").expect("写盘");
    let document = first
        .open(OpenRequest::file(&path, "select 1;", EditorMode::Sql))
        .id()
        .clone();
    first.update(|service| service.set_connection(&document, Some("P_orders".to_string())));

    let (harness, cx) = {
        let first = first.clone();
        let document = document.clone();
        cx.add_window_view(move |window, cx| {
            let (area, _skin) = DockSkin::dock_area("editor-session", Some(1), window, cx);
            let panel = cx.new(|cx| EditorHostPanel::new(first.clone(), document, window, cx));
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
    let (area, panel) = cx.update(|_window, cx| {
        (
            harness.read(cx).area.clone(),
            harness.read(cx).panels[0].clone(),
        )
    });
    assert!(
        cx.update(|window, cx| close_document_in_dock(&area, panel, window, cx)),
        "干净文档应当被关掉（关闭时落会话）"
    );

    // ===== 第二次运行：读会话 → 绑定回来 =====
    let second = EditorShared::new();
    second.attach_session_store(store.clone());
    let session = second
        .load_latest_session()
        .expect("读会话")
        .expect("应当有可恢复的会话");
    assert_eq!(
        session.connection.as_deref(),
        Some("P_orders"),
        "绑定要跨重启保留（不然用户以为还连着原来那个库）"
    );

    let reopened = second
        .open(OpenRequest::file(
            std::path::PathBuf::from(session.path.clone().expect("有路径")),
            session.content.clone(),
            session.mode,
        ))
        .id()
        .clone();
    let (panel2, cx) = {
        let second = second.clone();
        let reopened = reopened.clone();
        cx.add_window_view(move |window, cx| EditorHostPanel::new(second, reopened, window, cx))
    };
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| {
        panel2.update(cx, |panel, cx| panel.restore_session(&session, window, cx));
    });
    assert_eq!(
        second.service().connection_for(&reopened).as_deref(),
        Some("P_orders"),
        "恢复会话时绑定要真的写回文档"
    );

    let _ = std::fs::remove_dir_all(dir);
}
