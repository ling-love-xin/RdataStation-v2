//! （由 view/tests.rs 拆分，2026-10-29）
//! 注意：不用 `use super::*` 通配（gpui `test` 宏递归），依赖显式列举。


use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    TestAppContext,
};

use crate::model::EditorMode;
use crate::service::OpenRequest;
use crate::shared::EditorShared;

use super::{
    open_panel,
    shared_with_document,
};

// ===== 【遗留 #10】单元内联编辑 =====

/// 一份带两个单元的 `.rdsnote`（SQL + 文本）
fn analysis_note_two_cells() -> &'static str {
    r###"{"version":1,"session":{"id":"default","default_connection":null,"env":null},"cells":[
        {"id":"c1","kind":"sql","body":"select 1;"},
        {"id":"c2","kind":"text","body":"## 备注"}]}"###
}

/// 打开分析文档 → 面板为每个单元建内核编辑器实例（编辑能力随文档就位；
/// 渲染据此走内核，不再是纯文本）
#[gpui_kit::test]
fn opening_an_analysis_document_builds_a_cell_editor_per_cell(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\a.rdsnote",
            analysis_note_two_cells(),
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    // 单元编辑器在事件路径建（生产=on_added_to；测试窗口没挂 Dock，手动走同一入口）
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let mut ids = cx
        .update(|_window, cx| panel.read(cx).cell_editors.keys().cloned().collect::<Vec<_>>());
    ids.sort();
    assert_eq!(
        ids,
        vec!["c1".to_string(), "c2".to_string()],
        "每个单元都要有内核编辑器实例（键 = 稳定 cell_id；HashMap 迭代顺序不保证，先排序再比）"
    );

    // 渲染层：单元源以内核呈现（带业务键 id 的包装元素在树里，可被 headless 观察）
    let rendered = cx
        .update(|window, _cx| window.try_find("editor-nb-cell-source-c1").is_some());
    assert!(rendered, "单元源在渲染树里是内核编辑器（不是纯文本回退）");
}

/// 新增 / 复制 / 删除单元时，编辑器实例与单元序列保持同步（创建 / 移除随生命周期）
#[gpui_kit::test]
fn cell_editors_follow_add_duplicate_and_delete(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\b.rdsnote",
            analysis_note_two_cells(),
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 新增文本单元 → 编辑器随之建出
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_add_cell(&id, crate::cell::CellLanguage::Markdown, window, cx)
        });
    });
    let keys = cx
        .update(|_window, cx| panel.read(cx).cell_editors.keys().cloned().collect::<Vec<_>>());
    assert!(keys.len() == 3, "新增后三个单元都有编辑器：{keys:?}");

    // 复制 c1 → 副本 c1-copy 有独立编辑器
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_duplicate_cell(&id, "c1", window, cx)
        });
    });
    let keys = cx
        .update(|_window, cx| panel.read(cx).cell_editors.keys().cloned().collect::<Vec<_>>());
    assert!(
        keys.contains(&"c1-copy".to_string()),
        "副本要有自己的编辑器：{keys:?}"
    );

    // 删除 c1-copy → 编辑器与订阅一并移除
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.notebook_delete_cell(&id, "c1-copy", window, cx)
        });
    });
    let keys = cx
        .update(|_window, cx| panel.read(cx).cell_editors.keys().cloned().collect::<Vec<_>>());
    assert!(
        !keys.contains(&"c1-copy".to_string()),
        "删除后编辑器要移除（订阅句柄一并释放）：{keys:?}"
    );
}

/// 回写回路：在单元的内核编辑器里真打字 → Change 事件 → 面板订阅 → 单元正文落回
/// shared（下次运行 / 落盘读的都是编辑后的正文）
#[gpui_kit::test]
fn typing_in_a_cell_editor_writes_back_to_the_notebook_body(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(
            r"D:\notes\c.rdsnote",
            r#"{"version":1,"session":{"id":"default","default_connection":null,"env":null},"cells":[
                {"id":"c1","kind":"sql","body":"select 1;"}]}"#,
            EditorMode::Analysis,
        ))
        .id()
        .clone();
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_cell_editors(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 聚焦第一个单元的内核编辑器（点带业务键的包装元素；点击即聚焦）
    cx.update(|window, cx| {
        window.click("editor-nb-cell-source-c1", cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 真打一个字符：内核发出 Change → 面板订阅回写 shared 的单元正文
    //（VisualTestContext 的单参版自动打到当前窗口 / 当前聚焦）
    cx.simulate_input("x");

    let body = cx.update(|_window, _cx| shared.notebook_cells(&id)[0].body.clone());
    assert_ne!(body, "select 1;", "击键要写回笔记本正文（当前：{body}）");
    assert!(body.contains('x'), "正文要包含刚打进去的字符：{body}");
}

/// 通用模式打开 `.md` 文档：默认源码编辑器；工具栏有「预览」切换；
/// 切预览 → 渲染视图（TextView::markdown）出现；再切回 → 编辑器恢复。
#[gpui_kit::test]
fn markdown_document_toggles_between_source_and_rendered_preview(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\notes.md", "# 标题\n\n- 列表项\n");
    // .md 走通用模式（与 SQL 文档区分）；`sync_mode` 让面板按记忆模式渲染
    shared.update(|service| service.set_mode(&id, EditorMode::General));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 初始：源码编辑器态（预览根不出现）；工具栏有切换按钮
    assert!(
        cx.debug_bounds("editor-markdown-preview-toggle").is_some(),
        "Markdown 文档工具栏应有预览切换按钮"
    );
    assert!(
        cx.update(|window, _| window.try_find("markdown-preview-root"))
            .is_none(),
        "初始应为源码编辑器态（预览根不出现）"
    );

    // 切到预览：渲染视图出现
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_markdown_preview(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, _| window.try_find("markdown-preview-root"))
            .is_some(),
        "预览态应有渲染视图根"
    );

    // 再切回：源码编辑器恢复
    cx.update(|_window, cx| panel.update(cx, |panel, cx| panel.toggle_markdown_preview(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|window, _| window.try_find("markdown-preview-root"))
            .is_none(),
        "切回源码态后预览根应消失"
    );
}

/// 非 Markdown 文档（如 `.py`）不出现预览切换按钮（切换只对 Markdown 语义有意义）
#[gpui_kit::test]
fn non_markdown_documents_have_no_preview_toggle(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (shared, id) = shared_with_document(r"D:\py\run.py", "print('hi')");
    shared.update(|service| service.set_mode(&id, EditorMode::General));
    let (panel, cx) = open_panel(cx, &shared, &id);
    cx.update(|window, cx| panel.update(cx, |panel, cx| panel.sync_mode(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-markdown-preview-toggle").is_none(),
        "非 Markdown 文档不出现预览切换"
    );
}
