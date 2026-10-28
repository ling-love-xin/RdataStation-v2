//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。

use std::rc::Rc;

use gpui_kit::component::dock::{BasePanel as _, DockPlacement, DockSkin, Panel as _};
use gpui_kit::component::Root;
use gpui_kit::{
    AppContext as _, Entity, Focusable as _, TestAppContext,
};

use crate::fold::{FoldSpan, spans as fold_spans};
use crate::model::{DocumentId, EditorMode};
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::view::fold;
use crate::view::host::{
    EditorHostPanel, close_document_in_dock,
};

use super::{
    bind_editor_keys, open_panel,
    shared_with_document, temp_dir,
};
use super::Harness;

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

    // SQL 模式：内核里真的装着 SQL 语义着色器（不是“只算了一份 token 没装上去”），
    // 图例就是染色的对外契约——名字对不上主题词汇时内核会静默跳过该 token。
    assert!(
        cx.update(|_window, cx| panel.read(cx).highlight_installed_for_test(cx)),
        "SQL 文档的编辑内核上应当装有着色器"
    );
    let legend = cx.update(|_window, cx| panel.read(cx).highlight_legend_for_test(cx));
    for name in ["keyword", "string", "number", "comment", "function"] {
        assert!(
            legend.iter().any(|candidate| candidate == name),
            "图例里少了 `{name}`：{legend:?}"
        );
    }

    // 切到通用模式：**仍然上色**（通用模式里写的就是 SQL 脚本 / 片段，UIST 反馈要求一并
    // 着色；与 `view::fold` 对通用 + SQL 两模式开启折叠同口径）
    shared.update(|service| {
        service.set_mode(&id, EditorMode::General);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert_eq!(
        cx.update(|_window, _cx| shared.service().find(&id).map(|doc| doc.mode())),
        Some(EditorMode::General)
    );
    assert!(
        cx.update(|_window, cx| panel.read(cx).highlight_installed_for_test(cx)),
        "通用模式也要着上 SQL 色（通用模式里常常写的就是 SQL）"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 切回 SQL：恢复着色
    shared.update(|service| {
        service.set_mode(&id, EditorMode::Sql);
    });
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    assert!(
        cx.update(|_window, cx| panel.read(cx).highlight_installed_for_test(cx)),
        "切回 SQL 要重新装上着色器"
    );
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

/// 【书签·第①步】切换当前行书签：落盘 sidecar（`.query.sql.meta.json`）→ 空书签也回写 →
/// 重开面板从 sidecar 读回。行号 1 起。
#[gpui_kit::test]
fn bookmarks_toggle_persists_to_and_loads_from_the_sidecar(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let dir = temp_dir("bookmark");
    let path = dir.join("query.sql");
    std::fs::write(&path, "select 1\nselect 2\nselect 3").expect("写盘");
    let sidecar = dir.join(".query.sql.meta.json");

    let shared = EditorShared::new();
    let id = crate::persist::open_file(&shared, &path, EditorMode::Sql)
        .expect("打开")
        .id()
        .clone();

    let (harness, cx) = {
        let shared = shared.clone();
        let id = id.clone();
        cx.add_window_view(move |window, cx| {
            let (area, _skin) = DockSkin::dock_area("editor-bookmark", Some(1), window, cx);
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
    // 面板已进 Dock（on_added_to → load_bookmarks；初始 sidecar 缺失 = 空）
    let panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());

    // 打开时无书签
    assert!(shared.bookmarks(&id).is_empty());

    // 画一帧建 dispatch tree → 聚焦 → 按 Ctrl+Shift+B 切换当前行（光标默认第 1 行）
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-shift-b");

    // 内存书签 + sidecar 都应有第 1 行
    assert_eq!(shared.bookmarks(&id).len(), 1, "切换后内存应有一枚书签");
    assert_eq!(shared.bookmarks(&id)[0].line, 1, "书签行号 1 起");
    assert!(sidecar.exists(), "切换后必须落盘 sidecar");
    let meta = file_meta::read_meta(&path).expect("读回 meta");
    assert_eq!(meta.bookmarks.len(), 1, "sidecar 应有一枚书签");
    assert_eq!(meta.bookmarks[0].line, 1, "sidecar 行号 1 起");

    // 再切一次 → 移除并回写为空（空书签也落盘，即“清空即持久化”）
    cx.simulate_keystrokes("ctrl-shift-b");
    assert!(shared.bookmarks(&id).is_empty(), "再切应移除书签");
    assert!(sidecar.exists(), "清空后 sidecar 仍应存在（记录当前空状态）");
    let meta = file_meta::read_meta(&path).expect("读回 meta");
    assert!(meta.bookmarks.is_empty(), "sidecar 应无书签");

    // 落一条书签 → 清掉内存 → 开第二个面板 → 应**从 sidecar 读回**（而非沿用内存）
    cx.simulate_keystrokes("ctrl-shift-b");
    shared.set_bookmarks(&id, vec![]);
    // 复用同一 Dock：第二个面板进 Dock（on_added_to → load_bookmarks）会从 sidecar 读回书签
    let _panel2 = cx.update(|window, cx| {
        let area = harness.read(cx).area.clone();
        let panel = cx.new(|cx| EditorHostPanel::new(shared.clone(), id.clone(), window, cx));
        let handle = panel.clone();
        area.update(cx, |area, cx| {
            area.add_panel(handle, DockPlacement::Center, None, window, cx);
        });
        panel
    });
    assert_eq!(shared.bookmarks(&id).len(), 1, "重开面板应从 sidecar 读回书签");
    assert_eq!(shared.bookmarks(&id)[0].line, 1);

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

/// 【B17 / 2026-10-27 恢复】折叠：能力表置回 `true` 后，打开文档喂一次候选、
/// 文本变更后再喂一次
///
/// 内核没有公开的候选读口（`display_map` 不在公开面上），所以这里用探针（`fold::probe`）
/// 确认**路径真的在走**：打开文档喂候选、`Ctrl+/` 文本变更后再喂——`sync_fold`
/// 走的是 `install` 分支（见 `Capabilities::folding: true` 与 `fold::is_enabled`）。
#[gpui_kit::test]
fn folding_candidates_are_fed_after_the_gutter_restored(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    bind_editor_keys(cx);

    let sql = "SELECT (\n  1,\n  2\n) AS x\nFROM t";
    assert_eq!(
        // 默认折叠参数（缩进折叠开、无自定义关键字）；该段只有括号块
        fold_spans(sql, true, &[], &[]),
        [FoldSpan {
            start_line: 0,
            end_line: 3
        }],
        "文本本身仍有可折行块（`fold_spans` 是纯文本分析，与开关无关）"
    );

    let (shared, id) = shared_with_document("folding.sql", sql);
    let (feeds_before, _) = fold::probe::read();

    let (panel, cx) = open_panel(cx, &shared, &id);
    let (feeds_at_open, candidates_at_open) = fold::probe::read();
    assert!(
        feeds_at_open > feeds_before,
        "恢复后打开文档应喂一次候选（构造路径）"
    );
    assert_ne!(
        candidates_at_open, usize::MAX,
        "恢复后已喂过候选（探针哨兵 MAX 只在从未记录时出现）"
    );

    // 改一次文本：`Ctrl+/`（行注释开关）走 `replace_all`——与模板 / 格式化 / 转译**同一条**
    // 内容替换路径，最终都发 `InputEvent::Change`。
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let handle = cx.update(|_window, cx| panel.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&handle, cx));
    cx.simulate_keystrokes("ctrl-/");
    // 等一帧让替换落地（折叠有扫描回执要等；这里等文本变更处理完）
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let text = cx.update(|_window, cx| panel.read(cx).text_for_test(cx));
    assert_eq!(text, format!("-- {sql}"), "注释真的落到文本上");
    // 文本变更后的**重喂**由后台扫描泵驱动（`drain_exec_results` 收回执 → `fold::refresh`；
    // 见 `scan::is_current` + `scan_revision`），headless 不推后台泵，这里不强求；
    // 恢复折叠的核心断言是上面「打开文档即喂候选 + 候选非哨兵」。

    // 带着候选再画一帧：确认渲染路径在有折叠下不炸
    cx.update(|window, cx| window.draw(cx).clear(cx));
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
        let panel_slot = Rc::new(std::cell::RefCell::new(None::<Entity<EditorHostPanel>>));
        let slot = panel_slot.clone();
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let shell = cx.new(|cx| EditorHostPanel::new(shared, id, window, cx));
            *slot.borrow_mut() = Some(shell.clone());
            Root::new(shell, window, cx)
        });
        let panel = panel_slot.borrow().clone().expect("EditorHostPanel 已创建");
        (panel, cx)
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
    assert!(
        shared.service().find(&ids[1]).is_some(),
        "另一个文档不受影响"
    );
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

/// 回归测试（2026-10-14 闪退）：同组**多个标签**时，对非激活面板调 `focus_self`。
///
/// 闪退现场：`focus_self` 永远跑在面板自己的 `update` 闭包里，而 `TabGroup::select_tab`
/// 内部会经 `active_panel() → visible()` **同步 read 回本面板**——面板还在更新中就被读，
/// 触发 gpui 的 double-lease panic（“cannot read … while it is already being updated”）。
/// 修复：切标签改用 `spawn_in`（异步任务在当前帧所有 effect 处理完之后才运行，
/// 面板更新租约早已归还；`defer_in` 不够——它的闭包又包了一次本面板的 `update`，
/// 仍然会重入）。
#[gpui_kit::test]
fn focusing_a_non_active_tab_in_a_multi_tab_group_does_not_panic(cx: &mut TestAppContext) {
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
            let (area, _skin) = DockSkin::dock_area("editor-focus-multi", Some(1), window, cx);
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
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 第二个面板是激活标签（后 add 者 active_ix = 1）。
    // 对第一个（非激活）面板调 `focus_self`：修复前这里直接 double-lease panic。
    let first_panel = cx.update(|_window, cx| harness.read(cx).panels[0].clone());
    cx.update(|window, cx| {
        first_panel.update(cx, |panel, cx| panel.focus_self(window, cx))
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert_eq!(
        shared.service().active_id(),
        Some(&ids[0]),
        "非激活标签被激活后，服务层当前文档要切到它"
    );
}

/// 写入模式提示（需求②）：空白文档显示提示、内容非空即消失；空编辑器能正常画出
/// （替代原「开始编写」欢迎页挡路按钮——打开即编辑态）。
#[gpui_kit::test]
fn write_hint_shows_for_empty_and_hides_after_typing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let shared = EditorShared::new();
    let blank = shared
        .open(OpenRequest::untitled("", EditorMode::Sql))
        .id()
        .clone();

    let (panel, cx) = {
        let shared = shared.clone();
        let blank = blank.clone();
        cx.add_window_view(move |window, cx| {
            EditorHostPanel::new(shared.clone(), blank, window, cx)
        })
    };

    // 空白未命名文档：写入模式提示判定成立（可编辑且内容空）
    assert!(
        cx.update(|_window, cx| panel.read(cx).editor_is_empty()),
        "空白未命名文档应显示写入模式提示"
    );
    // 提示要真能画出来（render 路径不 panic）
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 内容非空 → 提示消失（判据读服务层内容）
    shared.update(|service| service.set_content(&blank, "select 9".to_string()));
    assert!(
        !cx.update(|_window, cx| panel.read(cx).editor_is_empty()),
        "内容非空后不再显示提示"
    );

    // 内容清空 → 提示重新出现
    shared.update(|service| service.set_content(&blank, String::new()));
    assert!(
        cx.update(|_window, cx| panel.read(cx).editor_is_empty()),
        "内容清空后提示重新出现"
    );

    // 回到正常编辑器后也要能画出来
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

