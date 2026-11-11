//! 默认档窗口级用例：输入聚焦 / ↑↓ 漫游与 ↵ / 鼠标确认 / 遮罩 / ⌥↵ 双通道 / Esc / 门槛。

use std::time::Duration;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{KeyBinding, TestAppContext};

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

/// 审计 D1 回归：**同词换档必须重发**。
///
/// 旧实现 `sent_query` 只以词为键——`#渠道` 与 `渠道` 判为同一批，FTS 从未发出，
/// `#` 档拿到的是名称档旧命中「换皮上屏」（组头变了、行上 `why` 全错）。
#[gpui_kit::test]
fn same_word_mode_change_researches(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (_host, _shared, harness, cx) = open_palette(cx);

    // `abc`：名称档批次（≥2 字达标）
    cx.simulate_keystrokes("abc");
    let (searching, sent, fulltext) = cx.update(|_window, cx| {
        let palette = harness.read(cx).palette.read(cx);
        (palette.searching(), palette.sent_query(), palette.sent_fulltext())
    });
    assert!(searching, "三字符应进入「搜索中」");
    assert_eq!(sent.as_deref(), Some("abc"));
    assert_eq!(fulltext, Some(false), "默认档走名称档");

    // 以 `#abc` 重设输入：词仍是 `abc`，但档位变全文——必须作为**新批次**重发
    // （FTS，三字满足 trigram 门槛）。旧实现只比词，这一步的 FTS 会被拦下。
    // （走 `set_value` + `refresh` 程序化路径：与档位 chip 点击同路——headless 下
    //   `#` 键无法经 keystroke 模拟，单行 Input 也不处理 Home 行首插入。）
    let palette = cx.update(|_window, cx| harness.read(cx).palette.clone());
    cx.update(|window, cx| {
        palette.update(cx, |palette, cx| palette.test_set_query("#abc", window, cx))
    });
    let (searching, sent, fulltext) = cx.update(|_window, cx| {
        let palette = harness.read(cx).palette.read(cx);
        (palette.searching(), palette.sent_query(), palette.sent_fulltext())
    });
    assert_eq!(sent.as_deref(), Some("abc"), "词没变");
    assert_eq!(
        fulltext,
        Some(true),
        "同词换档必须重发为 FTS 批次（D1：旧实现只比词、被拦截）"
    );
    assert!(searching, "新批次在途");
}

/// 审计 D5 回归：词长跌回门槛之下时「搜索中」要收掉。
///
/// 旧实现 `schedule_search` 的 `!async_ready` 提前 return 不清标志，叠加过期批次
/// 丢弃路径也不清，提示行会永久停在「元数据搜索中…」。
#[gpui_kit::test]
fn searching_resets_when_word_drops_below_threshold(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (_host, _shared, harness, cx) = open_palette(cx);

    cx.simulate_keystrokes("sa");
    let searching = cx.update(|_window, cx| harness.read(cx).palette.read(cx).searching());
    assert!(searching, "两字符应进入「搜索中」");

    cx.simulate_keystrokes("backspace");
    let (searching, sent) = cx.update(|_window, cx| {
        let palette = harness.read(cx).palette.read(cx);
        (palette.searching(), palette.sent_query())
    });
    assert!(
        !searching,
        "退格回单字符：没有可等的批次，提示要收掉（D5）"
    );
    assert_eq!(
        sent.as_deref(),
        Some("sa"),
        "已发批次键不清（防同键重发），清的只是提示"
    );
}

/// 审计空白点：过期批次（词已变 / 序号过旧）被泵丢弃时，不得污染当前状态。
#[gpui_kit::test]
fn stale_search_batch_is_dropped_without_side_effects(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (_host, _shared, harness, cx) = open_palette(cx);

    cx.simulate_keystrokes("or");
    let palette = cx.update(|_window, cx| harness.read(cx).palette.clone());
    let seq = cx.update(|_window, cx| palette.read(cx).search_seq());
    assert_eq!(seq, 1, "第一批已排队（防抖后才真正发出，序号同步自增）");

    // 词已改成 `ord`（第二批在途）：推一批「词 = or」的旧结果 → 必须整批丢弃
    cx.simulate_keystrokes("d");
    let seq = cx.update(|_window, cx| palette.read(cx).search_seq());
    database::nav_jobs::push_search_results_for_test(
        database::nav_jobs::SearchConsumer::QuickOpen,
        database::nav_jobs::SearchResult {
            consumer: database::nav_jobs::SearchConsumer::QuickOpen,
            seq,
            query: "or".to_string(),
            searched: 1,
            hits: vec![table_hit()],
        },
    );
    cx.update(|_window, cx| palette.update(cx, |palette, cx| palette.pump_results(cx)));
    let rows = cx.update(|_window, cx| palette.read(cx).row_count(cx));
    // `ord` 不命中两个连接（sales / sales_archive 都不含），若旧批未被丢弃会出现 meta 行
    assert_eq!(rows, 0, "词守卫应丢弃过期批次：{rows}");
}

/// 档位 chip 常驻可点击：默认档点 `#` 进入全文档（**词保留**），再点退出回默认档。
#[gpui_kit::test]
fn mode_chip_clicks_switch_and_exit_modes(cx: &mut TestAppContext) {
    let _queue_guard = scratchpad::jobs::test_lock();
    let (_host, _shared, harness, cx) = open_palette(cx);

    cx.simulate_keystrokes("a");
    cx.simulate_keystrokes("b");
    cx.simulate_keystrokes("c");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 点 `#` chip：输入变 `#abc`（词保留），全文档批次发出
    cx.update(|window, cx| window.click("qo-chip-fulltext", cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (query, fulltext) = cx.update(|_window, cx| {
        let palette = harness.read(cx).palette.read(cx);
        (palette.query_text(cx), palette.sent_fulltext())
    });
    assert_eq!(query, "#abc", "chip 进入全文档应保留原词");
    assert_eq!(fulltext, Some(true), "进入后应发 FTS 批次");

    // 再点 `#` chip：退出回默认档（词保留）
    cx.update(|window, cx| window.click("qo-chip-fulltext", cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (query, fulltext) = cx.update(|_window, cx| {
        let palette = harness.read(cx).palette.read(cx);
        (palette.query_text(cx), palette.sent_fulltext())
    });
    assert_eq!(query, "abc", "退出档位应保留原词");
    assert_eq!(fulltext, Some(false), "退出后键变，应重发名称批次");
}
