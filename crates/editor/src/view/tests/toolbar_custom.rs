//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。


use gpui_kit::{
    TestAppContext, VisualTestContext,
};


use super::{
    bind_editor_keys, open_panel,
    shared_with_document,
};

// ===== T2c-5：工具栏分组自定义 =====
//
// 分两扇：**布局模型**（`crate::toolbar`，纯函数单测在 toolbar.rs）与**接线**
// （渲染 + 面板方法）。这里验证接线：默认布局的两按钮形态、折叠、单项 / 整组移动、
// 竖线分隔符、重置——全部走「渲染断言 + 直调面板方法」纪律（不写 simulate_click）。

/// T2c-5①：默认布局 = 执行簇拆成「执行 + 全部运行」两按钮，显式目标项默认隐藏
#[gpui_kit::test]
fn toolbar_default_shows_exec_and_exec_all_but_not_explicit_targets(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\tb.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    let present = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds(selector).is_some()
    };

    // 两按钮都常驻（不再是单个「执行 ▾」下拉）
    assert!(present(cx, "editor-exec-run"), "执行主按钮在");
    assert!(present(cx, "editor-exec-all"), "全部运行在");
    // 分析组 / 事务开关 / 文档级常驻（原型 §2.2：密度靠 ⋯ 自定义不靠隐藏）
    assert!(present(cx, "editor-explain"), "执行计划常驻");
    assert!(present(cx, "editor-translate"), "方言转换常驻");
    assert!(present(cx, "editor-template"), "模板常驻");
    assert!(present(cx, "editor-autocommit"), "事务开关常驻");
    assert!(present(cx, "editor-format"), "格式化在");
    assert!(present(cx, "editor-toolbar-customize"), "最右 ··· 定制按钮在");
    // 默认隐藏的执行族显式目标
    assert!(!present(cx, "editor-exec-current"), "执行当前语句默认隐藏");
    assert!(!present(cx, "editor-exec-selection"), "执行选区默认隐藏");
    assert!(!present(cx, "editor-exec-batch"), "批量执行默认隐藏");

    // 模型也照同一份真值：默认可见集合与目录一致（执行簇两按钮 + 分析组 + 事务开关 + 文档级）
    let layout = cx.update(|_window, cx| panel.read(cx).toolbar_layout_for_test());
    for id in crate::toolbar::ToolbarItemId::ALL {
        assert_eq!(
            layout.is_visible(*id),
            id.visible_by_default(),
            "默认可见性与目录一致：{id:?}"
        );
    }
}

/// T2c-5②：折叠执行簇 → 只剩「首组件 + ▾」（editor-exec-all 消失），展开恢复两按钮
#[gpui_kit::test]
fn toolbar_collapse_folds_the_exec_group_to_head_and_chevron(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\tb.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    let present = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds(selector).is_some()
    };

    // 折叠前：两按钮都在
    assert!(present(cx, "editor-exec-run"));
    assert!(present(cx, "editor-exec-all"));

    // 折叠执行簇（组头 = ExecRun）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.toggle_toolbar_collapsed(crate::toolbar::ToolbarItemId::ExecRun, cx);
        });
    });

    // 折叠后：只有首组件（执行）在，全部运行收进 ▾
    assert!(present(cx, "editor-exec-run"), "首组件仍在");
    assert!(!present(cx, "editor-exec-all"), "全部运行收进 ▾");

    // 再折叠一次 = 展开：两按钮回来
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.toggle_toolbar_collapsed(crate::toolbar::ToolbarItemId::ExecRun, cx);
        });
    });
    assert!(present(cx, "editor-exec-all"), "展开后全部运行回来");
}

/// T2c-5③：单项移动跨过分隔符 → 组结构随真值变化（默认布局下把「格式化」左移进执行簇）
#[gpui_kit::test]
fn toolbar_move_item_crosses_separators_and_reshapes_groups(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\tb.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 默认：格式化在执行簇之后（单独一组）
    let layout = cx.update(|_window, cx| panel.read(cx).toolbar_layout_for_test());
    let heads: Vec<_> = layout.groups().iter().map(|g| g.head).collect();
    assert_eq!(
        heads,
        vec![
            crate::toolbar::ToolbarItemId::ExecRun,
            crate::toolbar::ToolbarItemId::Explain,
            crate::toolbar::ToolbarItemId::TxAutoCommit,
        ],
        "默认三段：执行簇 | 分析组 | 事务+文档级"
    );

    // 把格式化左移（跨过分隔符，进分析组）
    for _ in 0..2 {
        cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.move_toolbar_item(
                    crate::toolbar::ToolbarItemId::Format,
                    crate::toolbar::MoveDir::Left,
                    cx,
                );
            });
        });
    }

    // 移动后分析组里多了一个成员（格式化不再是事务组的成员）
    let layout = cx.update(|_window, cx| panel.read(cx).toolbar_layout_for_test());
    let explain_group = layout
        .group_of(crate::toolbar::ToolbarItemId::Explain)
        .expect("分析组仍在");
    assert!(
        explain_group.items.contains(&crate::toolbar::ToolbarItemId::Format),
        "格式化被移进分析组：{:?}",
        explain_group.items
    );
}

/// T2c-5④：整组移动 = 与相邻组整体交换（分析组移到最前，执行簇紧随其后）
#[gpui_kit::test]
fn toolbar_move_group_swaps_whole_groups(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\tb.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.move_toolbar_group(crate::toolbar::ToolbarItemId::Explain, crate::toolbar::MoveDir::Left, cx);
        });
    });

    let layout = cx.update(|_window, cx| panel.read(cx).toolbar_layout_for_test());
    let heads: Vec<_> = layout.groups().iter().map(|g| g.head).collect();
    assert_eq!(
        heads,
        vec![
            crate::toolbar::ToolbarItemId::Explain,
            crate::toolbar::ToolbarItemId::ExecRun,
            crate::toolbar::ToolbarItemId::TxAutoCommit,
        ],
        "分析组整体换到最前"
    );
}

/// T2c-5⑤：新增竖线分隔符 → 组被切开；重置默认 → 恢复两按钮布局
#[gpui_kit::test]
fn toolbar_insert_separator_then_reset_restores_default(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\tb.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    let present = |cx: &mut VisualTestContext, selector: &'static str| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds(selector).is_some()
    };

    // 新增一条竖线分隔符（追加在左区末尾）：默认两条之外的第三条
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.insert_toolbar_separator(cx));
    });
    let layout = cx.update(|_window, cx| panel.read(cx).toolbar_layout_for_test());
    assert!(layout.has_extra_separator(), "新增分隔符超过默认两条");
    assert_eq!(layout.separator_positions().len(), 3, "默认 2 + 新增 1");

    // 追加在末尾 → 事务+文档级组获得段尾分隔符（组数不变，但分隔符可再移动切开）
    assert_eq!(layout.groups().len(), 3, "末尾追加不切出新组");
    let tx_group = layout
        .group_of(crate::toolbar::ToolbarItemId::TxAutoCommit)
        .expect("事务+文档级 组仍在");
    assert!(tx_group.tail_separator, "事务+文档级 组现在有段尾分隔符");

    // 重置：分隔符回默认两条、执行簇两按钮回来
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.reset_toolbar(cx));
    });
    let layout = cx.update(|_window, cx| panel.read(cx).toolbar_layout_for_test());
    assert!(!layout.has_extra_separator(), "重置后没有新增分隔符");
    assert_eq!(layout.separator_positions().len(), 2, "回到默认两条分隔符");
    assert_eq!(layout.groups().len(), 3, "回到默认三段");
    assert!(present(cx, "editor-exec-run"), "执行主按钮回来");
    assert!(present(cx, "editor-exec-all"), "全部运行回来");
}

/// T2c-5⑥：执行主按钮不可隐藏（fixed_visible）——面板 Checkbox 置灰，set_visible 拒绝
#[gpui_kit::test]
fn toolbar_exec_run_cannot_be_hidden_from_the_panel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\sql\tb.sql", "select 1;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            let changed = panel.set_toolbar_visible(crate::toolbar::ToolbarItemId::ExecRun, false, cx);
            assert!(!changed, "执行主按钮不可隐藏");
        });
    });
    let layout = cx.update(|_window, cx| panel.read(cx).toolbar_layout_for_test());
    assert!(layout.is_visible(crate::toolbar::ToolbarItemId::ExecRun), "执行主按钮仍可见");

    // 其余成员可以隐藏（比如全部运行）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            let changed = panel.set_toolbar_visible(crate::toolbar::ToolbarItemId::ExecAll, false, cx);
            assert!(changed, "全部运行可以隐藏");
        });
    });
    let layout = cx.update(|_window, cx| panel.read(cx).toolbar_layout_for_test());
    assert!(!layout.is_visible(crate::toolbar::ToolbarItemId::ExecAll), "全部运行已隐藏");
}

/// 【T2c-3 收尾】列编辑 = 内核多光标：默认行编辑单光标；切列编辑后经公开的内核
/// `add_cursor_below_line` 真加光标（`cursor_count` 是内核真实值）；切回行编辑经
/// `enforce_edit_mode` 立即收拢回单光标。
#[gpui_kit::test]
fn column_edit_toggles_multi_cursor_via_the_kernel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    // 多行文本：给“在下方加光标”留出移动空间（最后一行再加会因到底被内核跳过）
    let (shared, id) = shared_with_document(r"D:\sql\col.sql", "select 1;\nselect 2;\nselect 3;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 默认行编辑 + 单光标
    let mode = cx.update(|_window, cx| panel.read(cx).edit_mode_for_test());
    assert_eq!(mode, crate::view::host::EditMode::Row, "默认行编辑");
    let count = cx.update(|_window, cx| panel.read(cx).cursor_count_for_test(cx));
    assert_eq!(count, 1, "初始单光标");

    // 光标停在第二行行首（字节偏移："select 1;\n" 是 10 字节）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_caret_for_test(10, cx));
    });

    // 切列编辑 → 加一个下方光标 → 内核真实多光标
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_edit_mode_for_test(crate::view::host::EditMode::Column, cx);
        });
    });
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.add_cursor_below_for_test(cx));
    });
    let count = cx.update(|_window, cx| panel.read(cx).cursor_count_for_test(cx));
    assert_eq!(count, 2, "列编辑下加光标：内核多光标 = 2");

    // 切回行编辑：立即收拢回单光标（`enforce_edit_mode` 走生产代码同一条路径）
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_edit_mode_for_test(crate::view::host::EditMode::Row, cx);
            panel.enforce_edit_mode_for_test(window, cx);
        });
    });
    let mode = cx.update(|_window, cx| panel.read(cx).edit_mode_for_test());
    assert_eq!(mode, crate::view::host::EditMode::Row, "已切回行编辑");
    let count = cx.update(|_window, cx| panel.read(cx).cursor_count_for_test(cx));
    assert_eq!(count, 1, "行编辑强制单光标");

    // 列编辑模式下多光标不受收拢影响（再次切回列编辑 + 加光标仍成立）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_edit_mode_for_test(crate::view::host::EditMode::Column, cx);
            panel.add_cursor_below_for_test(cx);
        });
    });
    let count = cx.update(|_window, cx| panel.read(cx).cursor_count_for_test(cx));
    assert_eq!(count, 2, "列编辑下多光标保持");
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 【T2c-3 收尾】行编辑模式下的多光标会在**下一次文本变更**（Change 事件）时被收拢——
/// 这是“行编辑 = 单光标”的后卫路径（键盘 Ctrl+Alt+↓ 加的光标也逃不掉）。
#[gpui_kit::test]
fn row_edit_collapses_multi_cursor_on_the_next_change(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let (shared, id) = shared_with_document(r"D:\sql\row.sql", "select 1;\nselect 2;\nselect 3;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 光标在第二行行首，行编辑模式下加一个下方光标（模拟 Ctrl+Alt+↓：内核真多光标）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_caret_for_test(10, cx));
    });
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.add_cursor_below_for_test(cx));
    });
    let count = cx.update(|_window, cx| panel.read(cx).cursor_count_for_test(cx));
    assert_eq!(count, 2, "加光标后内核多光标 = 2");

    // 聚焦内核并输入一个字符 → Change 事件 → 行编辑后卫收拢回单光标
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).editor_focus_handle_for_test(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("x");

    let count = cx.update(|_window, cx| panel.read(cx).cursor_count_for_test(cx));
    assert_eq!(count, 1, "行编辑下输入后收拢为单光标");
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// 【T2c-3 收尾】列编辑鼠标框选：直调内核公开的 `select_columnar_block` 框选一块——
/// 同 Alt+Shift 拖拽产出的矩形选区（每显示行一条选区、同一列跨），免鼠标捕获。
/// 真实鼠标路径由内核 `on_mouse_down`/`on_drag_move` 驱动，这里验证框选逻辑真到内核。
#[gpui_kit::test]
fn column_edit_selects_a_block_via_the_kernel(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    // 三行同长："select 1;\n" 10 字节 / "select 2;\n" 10 字节 / "select 3;" 9 字节
    let (shared, id) =
        shared_with_document(r"D:\sql\col.sql", "select 1;\nselect 2;\nselect 3;");
    let (panel, cx) = open_panel(cx, &shared, &id);

    // 框选 col 1..8 跨三行（起始 offset 1 → 行0列1；结束 offset 28 → 行2列8）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.set_edit_mode_for_test(crate::view::host::EditMode::Column, cx);
            panel.select_columnar_block_for_test(1, 28, cx);
        });
    });

    let count = cx.update(|_window, cx| panel.read(cx).cursor_count_for_test(cx));
    assert_eq!(count, 3, "三行各一条选区");
    let mut offsets = cx.update(|_window, cx| panel.read(cx).cursor_offsets_for_test(cx));
    offsets.sort_unstable();
    assert_eq!(offsets, vec![8, 18, 28], "每行同列（col 8）落一块的结尾（光标在选区尾）");
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

