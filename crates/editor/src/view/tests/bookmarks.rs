//! 书签 → 行号栏（gutter）标记：投影口径与单击 / Shift 语义
//!
//! 覆盖用户口径的三条：①已标记行常显图标（无说明 = 书签轮廓，有说明 = 带横线书签）
//! ②单击切换书签 ④有说明的折叠行箭头常显 + 下方一道横线（是否折叠候选由内核判定）。
//! 悬停淡色 / 悬停说明气泡属渲染层，headless 下只验证"标记与说明文本确实推给了内核"，
//! 像素效果靠人工验收。

use gpui_kit::base::input::{GutterMarker, GutterMarkerKind};
use gpui_kit::{Focusable as _, TestAppContext};

use crate::model::EditorMode;
use crate::shared::EditorShared;

use super::bind_editor_keys;
use super::dialogs::dialog_harness;
use super::temp_dir;

/// 书签投影成 gutter 标记：行号 1 起 → 内核 0 起；无说明 = `Bookmark`（说明留空），
/// 有说明 = `BookmarkNote` + `FoldNote`（两者都带说明文本，供内核画悬停提示）。
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
                kind: GutterMarkerKind::Bookmark,
                note: "".into(),
            },
            GutterMarker {
                line: 3,
                kind: GutterMarkerKind::BookmarkNote,
                note: "起点".into(),
            },
            GutterMarker {
                line: 3,
                kind: GutterMarkerKind::FoldNote,
                note: "起点".into(),
            },
        ],
        "无说明书签只画书签且说明留空；有说明书签附带折叠说明（同一条说明文本）"
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
    let _ = std::fs::remove_dir_all(&dir);
}

/// 设置关掉「悬停显示书签说明」后：标记照旧投影（图标还在），但不再附带说明文本，
/// 内核因此没有可弹的内容——「设置里能关」这条固定在这里。
#[gpui_kit::test]
fn bookmark_hover_note_setting_off_strips_note(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let dir = temp_dir("gutter_note_off");
    let path = dir.join("query.sql");
    std::fs::write(&path, "select 1\nselect 2").expect("写盘");
    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();

    // 面板构造前就把开关改成关闭：构造时读到的初始值即 false
    cx.update(|cx| {
        if cx.has_global::<settings::model::Settings>() {
            cx.global_mut::<settings::model::Settings>()
                .editor
                .bookmark_hover_note = false;
        } else {
            let mut settings = settings::model::Settings::default();
            settings.editor.bookmark_hover_note = false;
            cx.set_global(settings);
        }
    });

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-gutter-note-off");
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 第 1 行写说明：标记仍应有两条（书签 + 折叠说明），但说明文本不下发
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.apply_bookmark_label(1, "起点".to_string(), window, cx)
        });
    });

    let markers = cx.update(|_window, cx| panel.read(cx).editor.read(cx).gutter_markers().to_vec());
    assert_eq!(
        markers.len(),
        2,
        "有说明书签仍投影两条标记（书签 + 折叠说明）"
    );
    assert!(
        markers.iter().all(|marker| marker.note.is_empty()),
        "开关关闭后不下发说明文本"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 跳转到有说明的书签后，说明应常显（不必再把鼠标移到行号栏）。
///
/// 钉的是接线：`jump_to_line` 要把它自己算出的行 pin 给内核（`set_gutter_note_pin`）。
/// 只 `set_cursor_position` 的话内核不知道「刚跳到哪行」，说明就只能靠鼠标悬停——
/// 这正是遗留清单里「`jump_to_line` 不回传行号」那一段。
/// 反向断言：纯书签（`label == 行号`，没有说明文本）不 pin，pin 了也弹不出东西。
#[gpui_kit::test]
fn jumping_to_a_bookmarked_line_pins_its_note(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let dir = temp_dir("bookmark_pin");
    let path = dir.join("pin.sql");
    std::fs::write(&path, "select 1\nselect 2\nselect 3").expect("写盘");
    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-bookmark-pin");
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 第 3 行：带说明书签；第 2 行：gutter 单击加出来的纯书签（说明 = 行号）
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.apply_bookmark_label(3, "第三行说明".to_string(), window, cx);
            p.on_gutter_marker_click(1, false, window, cx);
        });
    });
    let bookmarks = shared.bookmarks(&id);
    assert!(
        bookmarks
            .iter()
            .any(|b| b.line == 3 && b.label == "第三行说明"),
        "前提：第 3 行带说明"
    );
    assert!(
        bookmarks
            .iter()
            .any(|b| b.line == 2 && b.label == "2"),
        "前提：第 2 行只有书签，没有说明，实际 {bookmarks:?}"
    );

    // 跳到有说明的行：pin 到该行（1 起 3 → 内核 0 基 2）
    cx.update(|window, cx| panel.update(cx, |p, cx| p.jump_to_line(3, window, cx)));
    let pin = cx.update(|_window, cx| panel.read(cx).editor.read(cx).gutter_note_pin());
    assert_eq!(pin, Some(2), "跳到第 3 行应把该行 pin 给内核");

    // 跳到纯书签行：没有说明文本，不 pin
    cx.update(|window, cx| panel.update(cx, |p, cx| p.jump_to_line(2, window, cx)));
    let pin = cx.update(|_window, cx| panel.read(cx).editor.read(cx).gutter_note_pin());
    assert_eq!(pin, None, "纯书签行没有说明可弹，不应 pin");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 文本编辑后书签行号跟随平移（方案 A：内核编辑行增量）。
///
/// 钉的是**接线**：`support/bookmark.rs::adjust_for_edit` 的用例只证明算法，
/// 这里从真实编辑（行首 Enter 插一行）走到 `InputEvent::Change` →
/// `adjust_bookmarks_for_edit` → shared 书签 → sidecar 回写，证明书签真的跟着走。
#[gpui_kit::test]
fn bookmarks_follow_an_inserted_line(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let dir = temp_dir("bookmark_shift");
    let path = dir.join("shift.sql");
    std::fs::write(&path, "select 1\nselect 2\nselect 3").expect("写盘");
    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();

    let (harness, cx) = dialog_harness(cx, &shared, &id, "editor-bookmark-shift");
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 第 3 行放一枚带说明书签
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.apply_bookmark_label(3, "第三行".to_string(), window, cx)
        });
    });
    assert_eq!(shared.bookmarks(&id)[0].line, 3);

    // 打开后光标在文首；聚焦内核编辑器后插一个换行（原第 3 行 → 第 4 行）
    let handle = cx.update(|_window, cx| panel.read(cx).editor.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    let end = cx.update(|_window, cx| panel.read(cx).editor.read(cx).selected_range().end);
    assert_eq!(end, 0, "前提：打开后光标在文首");
    cx.simulate_input("\n");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert!(
        text.starts_with('\n'),
        "前提：换行确实插在文首，实际 {text:?}"
    );
    assert_eq!(
        shared.bookmarks(&id)[0].line,
        4,
        "书签应随插入整体下移一行"
    );
    // 说明文本不随行号变（说明是用户写的，不是行号）
    assert_eq!(shared.bookmarks(&id)[0].label, "第三行");

    // 再删掉这行（Backspace 在行首合并空行）→ 书签回到第 3 行
    cx.simulate_keystrokes("backspace");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert_eq!(text, "select 1\nselect 2\nselect 3", "前提：删行回到原文");
    assert_eq!(
        shared.bookmarks(&id)[0].line,
        3,
        "书签应随删行整体上移一行"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
