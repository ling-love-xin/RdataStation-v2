//! 宿主侧用例：文件源端到端（真工作线程 + 真文件系统）+ toggle 三态纯函数。

use std::time::{Duration, Instant};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{App, AppContext as _, TestAppContext};

use crate::quick_open::host::{ToggleIntent, toggle_intent};
use crate::quick_open::model::Mode;

use super::{open_palette_in_project, temp_project};

/// 审计 D11：toggle 三态——关着开、同档关、**异档换**（旧实现是「开着必关」）。
#[test]
fn toggle_intent_opens_closes_and_switches() {
    use ToggleIntent::{Close, Open, Switch};

    // 关着：开
    assert_eq!(toggle_intent(false, None, Mode::Default), Open);
    assert_eq!(toggle_intent(false, None, Mode::Goto), Open);
    // 开着同档：关
    assert_eq!(toggle_intent(true, Some(Mode::Default), Mode::Default), Close);
    assert_eq!(toggle_intent(true, Some(Mode::Goto), Mode::Goto), Close);
    // 开着异档：换（不关闭）
    assert_eq!(toggle_intent(true, Some(Mode::Default), Mode::Goto), Switch);
    assert_eq!(toggle_intent(true, Some(Mode::Goto), Mode::Default), Switch);
}

/// 文件源端到端：打开面板 → 为当前项目排一次扁平清单 → 回填后出「文件」行。
///
/// 这条链路跨了三种时序：宿主端口（项目根）→ `scratchpad::jobs`（**真工作线程 + 真文件系统**）
/// → 浮层的泵（测试时钟驱动）。真线程只能用真实等待配合测试时钟推进，
/// 所以这是 Quick Open 用例里唯一需要 `advance_clock` 的一条。
#[gpui_kit::test]
fn opening_the_palette_loads_scratchpad_files(cx: &mut TestAppContext) {
    // 结果队列是**进程级共享**的（`scratchpad::jobs` 全局单例）：本用例经 `enqueue_flatten_files`
    // + 泵消费同一队列，若不同步，全量并行时结果会被别的测试 drain 走（实测稳定 20s 超时）。
    // 与 scratchpad 自身测试共用同一把锁（`jobs::test_lock`），全量并行才互不打扰。
    // 注意：**全部 quick_open 用例都要持锁**——它们都建 palette，各自的 60ms 泵都会
    // `drain_file_lists`，只锁文件用例的话，并行时其它泵会把这份清单抢走丢弃。
    let _queue_guard = scratchpad::jobs::test_lock();

    let project = temp_project("qo_files");
    std::fs::create_dir_all(project.join("scratchpad/notes")).unwrap();
    std::fs::write(project.join("scratchpad/notes/a.md"), "# 笔记").unwrap();

    let (_host, _shared, harness, cx) = open_palette_in_project(cx, project.clone());

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut keys: Vec<String> = Vec::new();
    while Instant::now() < deadline {
        // 测试时钟：把泵的 60ms 轮询推到点；真实小睡：等后台线程把清单做出来。
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
        keys = cx.update(|_window, cx| harness.read(cx).palette.read(cx).row_keys(cx));
        if keys.iter().any(|key| key == "file:notes/a.md") {
            break;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(
        keys.iter().any(|key| key == "file:notes/a.md"),
        "文件清单应在 20 s 内回填成行；实际行：{keys:?}"
    );

    std::fs::remove_dir_all(&project).ok();
}
