//! 书签 → 行号栏（gutter）标记：投影口径与单击 / Shift 语义
//!
//! 覆盖用户口径的三条：①已标记行常显图标（无说明 = 书签轮廓，有说明 = 带横线书签）
//! ②单击切换书签 ④有说明的折叠行在折叠箭头旁出角标。hover 淡色属渲染层，headless
//! 下只验证"标记确实推给了内核"，像素效果靠人工验收。

use gpui_kit::base::input::{GutterMarker, GutterMarkerKind};
use gpui_kit::component::WindowExt as _;
use gpui_kit::TestAppContext;

use crate::model::EditorMode;
use crate::shared::EditorShared;

use super::bind_editor_keys;
use super::dialogs::dialog_harness;
use super::temp_dir;

/// 书签投影成 gutter 标记：行号 1 起 → 内核 0 起；无说明 = `Bookmark`，
/// 有说明 = `BookmarkNote` + `FoldNote`（角标是否真的画出来由内核按折叠候选判定）。
#[gpui_kit::test]
fn bookmarks_project_into_gutter_markers(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let dir = temp_dir("gutter_markers");
    let path = dir.join("query.sql");
    std::fs::write(&path, "select 1\nselect 2\nselect 3\nselect 4").expect("写盘");
    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-gutter-markers");
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    // 画一帧：面板进 Dock 即 `load_bookmarks`（初始 sidecar 缺失 = 空）
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(shared.bookmarks(&id).is_empty(), "初始无书签");

    // 第 2 行：说明就是行号本身 = 无说明；第 4 行：写了说明
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.apply_bookmark_label(2, "2".to_string(), window, cx);
            p.apply_bookmark_label(4, "起点".to_string(), window, cx);
        });
    });

    let markers = cx.update(|_window, cx| panel.read(cx).editor.read(cx).gutter_markers().to_vec());
    assert_eq!(
        markers,
        vec![
            GutterMarker {
                line: 1,
                kind: GutterMarkerKind::Bookmark
            },
            GutterMarker {
                line: 3,
                kind: GutterMarkerKind::BookmarkNote
            },
            GutterMarker {
                line: 3,
                kind: GutterMarkerKind::FoldNote
            },
        ],
        "无说明书签只画书签；有说明书签再附带折叠角标"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// gutter 单击（无 Shift）= 切换该书签行；Shift+单击 = 只弹说明框，不动书签集合。
#[gpui_kit::test]
fn gutter_click_toggles_bookmark_and_shift_opens_note(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let dir = temp_dir("gutter_click");
    let path = dir.join("query.sql");
    std::fs::write(&path, "select 1\nselect 2\nselect 3").expect("写盘");
    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-gutter-click");
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 单击第 1 行（内核 0 起）→ 新增 1 起第 1 行书签
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| p.on_gutter_marker_click(0, false, window, cx));
    });
    assert_eq!(shared.bookmarks(&id).len(), 1, "单击应新增书签");
    assert_eq!(shared.bookmarks(&id)[0].line, 1, "书签行号 1 起");

    // 再单击同一行 → 移除
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| p.on_gutter_marker_click(0, false, window, cx));
    });
    assert!(shared.bookmarks(&id).is_empty(), "再单击应移除书签");

    // Shift+单击第 3 行：不切换，只弹说明输入框
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| p.on_gutter_marker_click(2, true, window, cx));
    });
    assert!(
        shared.bookmarks(&id).is_empty(),
        "Shift 单击不应切换书签"
    );
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "Shift 单击应弹出说明输入框"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
