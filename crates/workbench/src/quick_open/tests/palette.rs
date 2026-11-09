//! 默认档窗口级用例：输入聚焦 / ↑↓ 漫游与 ↵ / 鼠标确认 / 遮罩 / ⌥↵ 双通道 / Esc / 门槛。

use std::time::Duration;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{App, AppContext as _, KeyBinding, TestAppContext};

use crate::commands::QuickOpenLocate;
use crate::quick_open::model::Action;

use super::{open_palette, table_hit};

/// 打开即聚焦：真按键要落进输入框（而不是掉在地上）。
#[gpui_kit::test]
fn typing_reaches_the_input_after_open(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
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
    let _queue_guard = scratchpad::jobs::test_lock();
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
    let second = cx.update(|_window, cx| harness.read(cx).palette.read(cx).selected_key());
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

/// 鼠标点击结果行：经列表行 `on_click` → 委托 `confirm` → 宿主执行并关面板。
///
/// 回归：遮罩关闭监听曾挂在浮层根上，点行时 `mouse_down` 先冒泡到根把面板关掉，
/// 随后的 `click` 不再命中行，鼠标确认失效（Enter 走输入框事件，所以一直正常）。
#[gpui_kit::test]
fn clicking_a_row_executes_the_selected_action(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (host, shared, _harness, cx) = open_palette(cx);
    cx.simulate_keystrokes("s");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|window, cx| {
        window.click("qo-row-conn:0:sales", cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let executed = host.taken();
    assert_eq!(
        executed.len(),
        1,
        "鼠标点击行应经宿主端口执行一次：{executed:?}"
    );
    assert_eq!(executed[0].0, Action::SelectConnection(0));
    assert!(!executed[0].1, "普通点击不保留面板");
    assert!(
        !shared.quick_open.get(),
        "执行后面板要关（下一次打开是全新的）"
    );
}

/// 点遮罩空白处仍关闭面板，且不派发任何动作（遮罩关闭能力回归）。
#[gpui_kit::test]
fn clicking_the_overlay_closes_without_executing(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (host, shared, _harness, cx) = open_palette(cx);
    cx.simulate_keystrokes("s");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|window, cx| {
        window.click("qo-overlay", cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        host.taken().is_empty(),
        "点遮罩不应派发动作：{:?}",
        host.taken()
    );
    assert!(!shared.quick_open.get(), "点遮罩空白必须关掉浮层");
}

/// `⌥↵`（在树中定位）真按键：元数据命中行上派发 `RevealMetadata`，不可定位的行上不动。
///
/// 这条用例是**判别性**的：它自己注册生产那份键位（app 层的 `alt-enter` / 上下文 `workbench`），
/// 并伪造一批真元数据命中（见 `push_search_results_for_test` 的理由）——若键位、λ 处理或
/// 行上的定位引用任一断，它都会挂。
#[gpui_kit::test]
fn locate_key_dispatches_reveal_for_metadata_rows(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (host, shared, harness, cx) = open_palette(cx);
    let palette = cx.update(|_window, cx| harness.read(cx).palette.clone());
    // 与生产同形：键位绑在 `workbench` 上下文，调度沿焦点链向上找。
    cx.update(|_, cx| {
        cx.bind_keys([KeyBinding::new(
            "alt-enter",
            QuickOpenLocate,
            Some("workbench"),
        )]);
    });

    cx.simulate_keystrokes("o");
    cx.simulate_keystrokes("r");
    // 先等真搜索收尾（没有缓存连接 → 空结果）：它会写同一槽 `self.meta`，
    // 不等它就把伪造批覆盖掉，用例会随机绿/红。
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while database::nav_jobs::has_pending_search(database::nav_jobs::SearchConsumer::QuickOpen)
        && std::time::Instant::now() < deadline
    {
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();

    // 推泵 + 重画，直到元数据行出现。
    //
    // 每轮都**重投一次**：真搜索（无缓存连接 → 空结果）可能晚于我们投批到达并清空 `meta`，
    // 重投保证「最后一次入队的总是我们这一批」，用例不会因调度顺序时绿时红。
    let mut keys: Vec<String> = Vec::new();
    for _ in 0..20 {
        database::nav_jobs::push_search_results_for_test(
            database::nav_jobs::SearchConsumer::QuickOpen,
            database::nav_jobs::SearchResult {
                consumer: database::nav_jobs::SearchConsumer::QuickOpen,
                seq: 1,
                query: "or".to_string(),
                searched: 1,
                hits: vec![table_hit()],
            },
        );
        cx.update(|_window, cx| {
            palette.update(cx, |palette, cx| palette.pump_results(cx));
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        keys = cx.update(|_window, cx| palette.read(cx).row_keys(cx));
        if keys.iter().any(|key| key.starts_with("meta:")) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let meta_key = keys
        .iter()
        .find(|key| key.starts_with("meta:"))
        .unwrap_or_else(|| {
            let q = cx.update(|_window, cx| palette.read(cx).query_text(cx));
            panic!("元数据行应已入列（输入={q:?}）；实际行：{keys:?}")
        })
        .clone();

    // 选中那一行后按 `⌥↵`：应派出「在树中定位」而不是打开属性面板
    let palette = cx.update(|_window, cx| harness.read(cx).palette.clone());
    cx.update(|_window, cx| {
        palette.update(cx, |palette, cx| {
            palette.set_selection(Some(meta_key.clone()), cx)
        });
    });
    cx.simulate_keystrokes("alt-enter");

    let executed = host.taken();
    assert_eq!(executed.len(), 1, "⌥↵ 应经宿主端口执行一次：{executed:?}");
    match &executed[0].0 {
        Action::RevealMetadata(object) => {
            assert_eq!(object.name, "orders");
            assert_eq!(object.schema, "public");
            assert_eq!(object.catalog, "shop");
        }
        other => panic!("⌥↵ 应派发 RevealMetadata，实际 {other:?}"),
    }
    assert!(!executed[0].1, "定位不保留面板");
    assert!(
        !shared.quick_open.get(),
        "定位后面板要关（下一拍才看得到树）"
    );
}

/// `⌥↵` 在不支持定位的行上**什么都不做**：不派发动作、也不关面板。
#[gpui_kit::test]
fn locate_key_is_a_no_op_on_rows_that_cannot_be_located(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (host, shared, _harness, cx) = open_palette(cx);
    cx.update(|_, cx| {
        cx.bind_keys([KeyBinding::new(
            "alt-enter",
            QuickOpenLocate,
            Some("workbench"),
        )]);
    });
    cx.simulate_keystrokes("s");

    cx.simulate_keystrokes("alt-enter");

    assert!(
        host.taken().is_empty(),
        "连接 / 命令行不可定位，⌥↵ 不该派发动作：{:?}",
        host.taken()
    );
    assert!(shared.quick_open.get(), "⌥↵ 在不可定位的行上不该关面板");
}

/// Esc 关闭（单行 Input 不消费 Esc，会冒泡到浮层根）。
#[gpui_kit::test]
fn escape_closes_the_palette(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
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
