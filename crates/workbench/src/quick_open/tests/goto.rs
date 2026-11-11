//! Goto 档（`Ctrl+Shift+O`）窗口级用例：锁档、↵ 回树定位、⌥↵ 属性面板。

use gpui_kit::{KeyBinding, TestAppContext};

use crate::commands::QuickOpenLocate;
use crate::quick_open::model::{Action, Mode};

use super::{open_palette_goto, push_goto_meta_row};

/// Goto 档接线：打开即锁定 `Goto` 模式，且不出连接行（默认档连接行由既有输入用例覆盖）。
#[gpui_kit::test]
fn goto_palette_locks_metadata_only_without_connection_rows(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (_host, _shared, harness, cx) = open_palette_goto(cx);
    let palette = cx.update(|_window, cx| harness.read(cx).palette.clone());
    let (mode, keys) = cx.update(|_window, cx| {
        let palette = palette.read(cx);
        (palette.current_mode(), palette.row_keys(cx))
    });
    assert_eq!(mode, Some(Mode::Goto), "Ctrl+Shift+O 打开应锁定 Goto 档");
    assert!(
        keys.iter().all(|key| !key.starts_with("conn:")),
        "Goto 档不出连接行：{keys:?}"
    );
}

/// Goto 档 `↵`：主动作 = 在导航树中定位（`RevealMetadata`），执行后面板关闭。
#[gpui_kit::test]
fn goto_enter_reveals_metadata_into_tree(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (host, shared, harness, cx) = open_palette_goto(cx);
    let palette = cx.update(|_window, cx| harness.read(cx).palette.clone());
    let key = push_goto_meta_row(cx, palette.clone());
    cx.update(|_window, cx| palette.update(cx, |palette, cx| palette.set_selection(Some(key), cx)));

    cx.simulate_keystrokes("enter");

    let executed = host.taken();
    assert_eq!(executed.len(), 1, "↵ 应经宿主端口执行一次：{executed:?}");
    match &executed[0].0 {
        Action::RevealMetadata(object) => {
            assert_eq!(object.name, "orders");
            assert_eq!(object.schema, "public");
            assert_eq!(object.catalog, "shop");
        }
        other => panic!("Goto ↵ 主动作应是回树定位，实际 {other:?}"),
    }
    assert!(!executed[0].1, "主动作不保留面板");
    assert!(
        !shared.quick_open.get(),
        "执行后面板要关（下一拍才看得到树）"
    );
}

/// Goto 档 `⌥↵`：次级动作 = 打开属性面板（`ShowProperties`），与默认档「⌥↵ 定位」方向相反。
#[gpui_kit::test]
fn goto_alt_enter_opens_properties_secondary_action(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (host, shared, harness, cx) = open_palette_goto(cx);
    let palette = cx.update(|_window, cx| harness.read(cx).palette.clone());
    let key = push_goto_meta_row(cx, palette.clone());
    cx.update(|_, cx| {
        cx.bind_keys([KeyBinding::new(
            "alt-enter",
            QuickOpenLocate,
            Some("workbench"),
        )])
    });
    cx.update(|_window, cx| palette.update(cx, |palette, cx| palette.set_selection(Some(key), cx)));

    cx.simulate_keystrokes("alt-enter");

    let executed = host.taken();
    assert_eq!(executed.len(), 1, "⌥↵ 应经宿主端口执行一次：{executed:?}");
    match &executed[0].0 {
        Action::ShowProperties(request) => {
            assert_eq!(request.conn_label, "营销");
            assert_eq!(request.driver, "mysql");
        }
        other => panic!("Goto ⌥↵ 次级动作应是属性面板，实际 {other:?}"),
    }
    assert!(!shared.quick_open.get(), "次级动作执行后面板要关");
}
