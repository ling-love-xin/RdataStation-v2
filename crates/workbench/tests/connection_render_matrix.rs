//! 连接对话框**渲染状态矩阵**（降级路径）。
//!
//! 目标是覆盖“状态 × 渲染”的组合面，防止某一状态下的渲染分支无人触碰：
//! - 未选类型的空态引导条 → 填名称后消失 → 清空后复现（判据是表单内容，不是一次性标记）；
//! - 5 个 Tab（常规 / 网络 / 能力 / 驱动属性 / 高级）在**全局库未初始化**时全部可渲染；
//! - 作用域三态（仅全局 / 仅项目 / 全局+项目）切换后渲染；
//! - 结果行四个级别（Info / Success / Warning / Error）各渲染一帧；
//! - 暂存区固定高度：草稿累加到 13 条时区域高度**不得增长**（用户实测过“拉长页面”）。
//!
//! 本文件**不注入**全局库单例：走的就是“服务不可用”的降级路径（与 UI 首启、
//! 服务未就绪时的真实分支一致），也因此不会写到用户真实库。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的
//! `test` 属性宏带入作用域，导致 `#[gpui_kit::test]` 展开出的裸 `#[test]`
//! 解析到它自己，造成无限递归。所有依赖显式列举。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled as _, TestAppContext, VisualTestContext, Window, div,
};

use rds_workbench::components::connection_dialog::{ConnectionDialogState, ResultLevel, ResultLine};
use rds_workbench::panels::{EditorPanel, Shared};

/// 测试宿主：与 `WorkbenchView` 同构（渲染面板 + 挂对话框层 + 宿主重绘桥）。
struct Harness {
    _shared: Shared,
    editor: Entity<EditorPanel>,
}

impl Harness {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shared = Shared::new();
        // 面板构造会读设置 global（`SettingsService::*`）：注入默认值，不读用户磁盘配置。
        if !cx.has_global::<settings::model::Settings>() {
            cx.set_global(settings::model::Settings::default());
        }
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        let weak = cx.entity().downgrade();
        let bridge: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
            let _ = weak.update(cx, |_, cx| cx.notify());
        });
        *shared.host_redraw.borrow_mut() = Some(bridge);
        Self {
            _shared: shared,
            editor,
        }
    }

    /// 走生产入口打开「新建连接」对话框。
    ///
    /// 注意：调用点必须在 `Harness` 的 update 上下文**之外**（先取 `Entity<EditorPanel>`，
    /// 再单独 `update`）——面板入口会通过 `notify_host` 回调宿主重绘，嵌套 update 会重入 panic。
    fn editor(&self) -> Entity<EditorPanel> {
        self.editor.clone()
    }

    /// 面板持有的对话框状态（首次 open 后才有）。
    fn dialog(&self, cx: &App) -> Rc<ConnectionDialogState> {
        self.editor
            .read(cx)
            .dialog_state()
            .expect("对话框状态已创建")
    }
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.editor.clone())
            .when_some(Root::render_dialog_layer(window, cx), |d, layer| {
                d.child(layer)
            })
    }
}

fn open_harness(cx: &mut TestAppContext) -> (Entity<Harness>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<Harness>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let harness = cx.new(|cx| Harness::new(window, cx));
        *slot_in.borrow_mut() = Some(harness.clone());
        Root::new(harness, window, cx)
    });
    let harness = slot.borrow().clone().expect("harness 已创建");
    (harness, cx)
}

/// 走生产入口打开对话框：先取面板实体，再单独 update（避开 `notify_host` → 宿主重绘的嵌套 update）。
fn open_new(harness: &Entity<Harness>, cx: &mut VisualTestContext) {
    let editor = cx.update(|_, cx| harness.read(cx).editor());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| e.request_new_connection(window, cx))
    });
}

/// 空态引导 / 五个 Tab / 作用域三态 / 结果行四级：逐一渲染不 panic。
#[gpui_kit::test]
fn dialog_state_matrix_renders_on_degraded_path(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    open_new(&harness, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 1) 空态（未选类型 + 名称/地址为空）：首次引导条出现。
    assert!(
        cx.debug_bounds("conn-general-guide").is_some(),
        "空态应渲染首次引导条"
    );
    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));

    // 2) 填名称 → 引导条消失（老手版面不被占用）。
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("矩阵用例", window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("conn-general-guide").is_none(),
        "名称非空后引导条应消失"
    );

    // 3) 清空名称 → 引导条复现（判据是表单内容，不是"已展示过"标记）。
    cx.update(|window, cx| {
        dialog.name.update(cx, |s, cx| s.set_value("", window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("conn-general-guide").is_some(),
        "清空后引导条应复现"
    );

    // 4) 五个 Tab 逐一渲染（类型 / 驱动 / 引用目录均为空的降级分支）。
    for tab in 0..5 {
        dialog.active_tab.set(tab);
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    // 5) 作用域三态：切换后各渲染一帧。
    for label in ["仅全局", "仅项目", "全局+项目"] {
        cx.update(|window, cx| {
            dialog.scope.update(cx, |s, cx| {
                s.set_selected_value(&SharedString::from(label), window, cx)
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    // 6) 结果行四级：级别可读（UI 据此着色），渲染不 panic。
    for (level, summary) in [
        (ResultLevel::Info, "正在连接…"),
        (ResultLevel::Success, "连接成功 · 12 ms"),
        (ResultLevel::Warning, "已保存（分组未同步）"),
        (ResultLevel::Error, "保存失败: 名称为空"),
    ] {
        cx.update(|_, _cx| {
            *dialog.result.borrow_mut() = Some(ResultLine::new(level, summary));
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            cx.update(|_, _cx| dialog.result_level()),
            Some(level),
            "结果行级别应为 {level:?}"
        );
    }
}

/// 暂存区固定高度：草稿累加不得拉长区域（用户实测过该回归）。
#[gpui_kit::test]
fn staging_area_keeps_fixed_height_with_many_drafts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    open_new(&harness, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let h0 = cx
        .debug_bounds("conn-staging-scroll")
        .expect("暂存滚动容器应已渲染")
        .size
        .height;
    assert!(
        h0 > gpui_kit::Pixels::default(),
        "暂存区应有固定高度（当前 {h0:?}）"
    );

    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));
    // 累加到 13 条（1 条初始空草稿 + 12 次「+ 添加」）。
    for _ in 0..12 {
        cx.update(|window, cx| dialog.staging_add(window, cx));
        cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));
    }
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let h1 = cx
        .debug_bounds("conn-staging-scroll")
        .expect("暂存滚动容器应仍在")
        .size
        .height;
    assert_eq!(
        h1, h0,
        "草稿增多不得拉长暂存区（应为固定高度 + 内部滚动）"
    );
    assert!(
        cx.update(|_, _cx| dialog.drafts.borrow().len()) >= 13,
        "草稿应已累加"
    );
}
