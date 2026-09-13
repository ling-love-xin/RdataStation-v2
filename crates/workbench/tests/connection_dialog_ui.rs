//! 数据源连接对话框：窗口级测试（GPUI headless）。
//!
//! 参照 `crates/project/src/ui/tests.rs` 骨架：窗口根为 `Root`，宿主渲染时挂
//! `Root::render_dialog_layer`；全局库未初始化（本测试不接真实服务），
//! 对话框打开/各 Tab 渲染/关闭必须降级而非 panic。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的
//! `test` 属性宏带入作用域，导致 `#[gpui_kit::test]` 展开出的裸 `#[test]`
//! 解析到它自己，造成无限递归。所有依赖显式列举。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::{Root, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, div,
};

use rds_workbench::components::connection_dialog::ConnectionDialogState;
use rds_workbench::panels::{EditorPanel, Shared};

/// 测试宿主：持有对话框状态与 `Entity<EditorPanel>`（`open` 的宿主参数），
/// 并在渲染时挂上对话框层（`open_dialog` 依赖窗口根是 `Root`）。
struct DialogHarness {
    /// 面板持有 `Shared` 与对话框状态（`open` 走生产入口 `request_*`）。
    editor: Entity<EditorPanel>,
}

impl DialogHarness {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _ = window;
        Self {
            editor: cx.new(|cx| EditorPanel::new(Shared::new(), cx)),
        }
    }

    fn open(&self, editing_id: Option<String>, window: &mut Window, cx: &mut gpui_kit::App) {
        // 走生产入口（面板 request_*）：订阅建立等副作用与真实路径一致。
        self.editor.update(cx, |editor, cx| match editing_id {
            Some(id) => editor.request_edit_connection(id, window, cx),
            None => editor.request_new_connection(window, cx),
        });
    }

    /// 面板持有的对话框状态（首次 `open` 后才有）。
    fn dialog(&self, cx: &gpui_kit::App) -> Rc<ConnectionDialogState> {
        self.editor
            .read(cx)
            .dialog_state()
            .expect("对话框状态已创建")
    }
}

impl Render for DialogHarness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when_some(Root::render_dialog_layer(window, cx), |d, layer| {
                d.child(layer)
            })
    }
}

/// 打开测试窗口（窗口根 `Root`）并取回宿主实体。
fn open_harness(cx: &mut TestAppContext) -> (Entity<DialogHarness>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<DialogHarness>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let harness = cx.new(|cx| DialogHarness::new(window, cx));
        *slot_in.borrow_mut() = Some(harness.clone());
        Root::new(harness, window, cx)
    });
    let harness = slot.borrow().clone().expect("harness 已创建");
    (harness, cx)
}

#[gpui_kit::test]
fn dialog_opens_renders_and_closes(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);

    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(None, window, cx));
    });
    assert!(
        cx.update(|window, cx| window.has_active_dialog(cx)),
        "连接对话框应已打开"
    );

    // 渲染一帧（含对话框层）不 panic。
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.update(|window, cx| window.close_dialog(cx));
    assert!(
        !cx.update(|window, cx| window.has_active_dialog(cx)),
        "关闭后不应有活动对话框"
    );
}

#[gpui_kit::test]
fn dialog_renders_each_tab_and_keeps_state_across_reopen(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);

    // 编辑入口：服务未就绪 → 预填静默跳过，不应 panic；editing_id 应记录。
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(Some("G_conn_demo".to_string()), window, cx));
    });
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    let editing_id = cx.update(|_, cx| harness.read(cx).dialog(cx).editing_id.borrow().clone());
    assert_eq!(editing_id.as_deref(), Some("G_conn_demo"));

    // 逐个 Tab 渲染：0 常规 / 1 网络 / 2 能力 / 3 驱动属性 / 4 高级。
    let tab = cx.update(|_, cx| harness.read(cx).dialog(cx).active_tab.clone());
    for i in 0..5 {
        tab.set(i);
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    // 关闭再开（新建入口）：状态由宿主持有，Tab 选择保留。
    cx.update(|window, cx| window.close_dialog(cx));
    tab.set(3);
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(None, window, cx));
    });
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
    assert_eq!(tab.get(), 3, "重开后 Tab 选择应保留");
    let editing_after = cx.update(|_, cx| harness.read(cx).dialog(cx).editing_id.borrow().clone());
    assert!(editing_after.is_none(), "新建入口应清空编辑 ID");
}

#[gpui_kit::test]
fn dialog_reopen_does_not_duplicate_dialog_layer(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);

    // 重入保护（幂等打开）：连续两次 open 只保留一层，close 一次即全部关闭。
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(None, window, cx));
    });
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(Some("G_conn_again".to_string()), window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.update(|window, cx| window.has_active_dialog(cx)));

    cx.update(|window, cx| window.close_dialog(cx));
    assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
}

/// #28：结果行分级与详情入口（短消息只着色，长消息额外提供「详情 / 复制」）。
#[gpui_kit::test]
fn result_line_levels_and_detail_entry(cx: &mut TestAppContext) {
    use rds_workbench::components::connection_dialog::{ResultLevel, ResultLine};

    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    cx.update(|window, cx| {
        harness.update(cx, |h, cx| h.open(None, window, cx));
    });
    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));

    // 1) 短消息（Warning 级）：级别可读，但不渲染详情入口。
    cx.update(|_, _cx| {
        *dialog.result.borrow_mut() = Some(ResultLine::new(
            ResultLevel::Warning,
            "已保存：生产 PG（分组未同步：项目库不可写）",
        ));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|_, _cx| dialog.result_level()),
        Some(ResultLevel::Warning),
        "级别应可读（UI 用它着色）"
    );
    assert!(
        cx.debug_bounds("conn-result-copy").is_none(),
        "短消息不应出现详情入口"
    );

    // 2) 长错误：出现「详情 / 复制」入口（完整原文可展开 / 可复制）。
    let long = format!("保存失败: {}", "驱动拒绝连接；".repeat(12));
    cx.update(|_, _cx| {
        *dialog.result.borrow_mut() = Some(
            ResultLine::new(ResultLevel::Error, long.clone()).with_detail(long.clone()),
        );
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("conn-result-toggle").is_some(),
        "长错误应提供展开入口"
    );
    assert!(
        cx.debug_bounds("conn-result-copy").is_some(),
        "长错误应提供复制入口"
    );
    assert!(
        cx.debug_bounds("conn-result-detail").is_none(),
        "未展开时不渲染详情正文"
    );

    // 3) 展开态：详情正文节点出现。
    cx.update(|_, _cx| dialog.result_expanded.set(true));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("conn-result-detail").is_some(),
        "展开后应渲染详情正文"
    );
}
