//! 宿主面板的窗口级测试（GPUI headless 窗口）
//!
//! 覆盖 A2 的验收口径：多文档并存、标题跟随、脏点显示、面板可关闭、渲染不 panic。
//!
//! 注意：这里**不使用** `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的 `test`
//! 属性宏带入作用域，而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会因此解析到它自己
//! （recursion limit reached）。所有依赖显式列举。

use gpui_kit::TestAppContext;
use gpui_kit::component::dock::{BasePanel as _, Panel as _};

use crate::model::{DocumentId, EditorMode};
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::view::host::EditorHostPanel;

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
