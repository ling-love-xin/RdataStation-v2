//! 暂存列表跨会话持久化测试（C6）——对应开发计划「草稿持久化」与架构文档 §5。
//!
//! 覆盖：变更后落库（`connection_drafts`）、重启后 `staging_restore` 恢复草稿与表单、
//! 密码不落库（恢复条目的 `pass` 为空）。
//!
//! 隔离：通过 `set_db_path_override` 注入临时库（进程级一次），不触碰用户真实 global.db。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入（会把 gpui 的 `test`
//! 宏带入作用域，与 `#[gpui_kit::test]` 冲突）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled as _,
    TestAppContext, Window, div,
};

use rds_workbench::components::connection_dialog::ConnectionDialogState;
use rds_workbench::panels::{EditorPanel, Shared};

/// 简化宿主：与 `WorkbenchView` 同构（挂对话框层 + 注入宿主重绘桥）。
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

fn temp_db_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_drafts_persist_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir.join("global.db")
}

#[gpui_kit::test]
fn drafts_survive_restart_and_drop_password(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    // 1) 测试库注入（进程级一次；本文件只此一个用例）。
    let db_path = temp_db_path("restart");
    engine::persistence::connection_draft_store::set_db_path_override(db_path.clone());

    let slot: Rc<RefCell<Option<Entity<Harness>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let harness = cx.new(|cx| Harness::new(window, cx));
        *slot_in.borrow_mut() = Some(harness.clone());
        Root::new(harness, window, cx)
    });
    let harness = slot.borrow().clone().expect("harness 已创建");

    // 2) 会话一：填写两条草稿（含密码）并触发持久化。
    let d1 = cx.update(|window, cx| Rc::new(ConnectionDialogState::new(window, cx)));
    cx.update(|window, cx| {
        d1.name.update(cx, |s, cx| s.set_value("草稿A", window, cx));
        d1.url
            .update(cx, |s, cx| s.set_value("mysql://h:3306/a", window, cx));
        d1.pass.update(cx, |s, cx| s.set_value("secret-a", window, cx));
        // 首次恢复（无非空库）应为空操作。
        d1.staging_restore(window, cx);
    });
    cx.update(|window, cx| d1.staging_add(window, cx));
    cx.update(|window, cx| {
        d1.name.update(cx, |s, cx| s.set_value("草稿B", window, cx));
        d1.remark
            .update(cx, |s, cx| s.set_value("备注B", window, cx));
    });
    // 第 0 条带认证方法（选项来自驱动声明；验证新列随草稿持久化）。
    cx.update(|window, cx| {
        d1.drafts.borrow_mut()[0].auth_method = "password".to_string();
        let _ = window;
        let _ = cx;
    });
    // 关闭对话框：写回当前表单 + 落库。
    cx.update(|_window, cx| d1.staging_flush(cx));
    cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));

    // 3) 会话二（模拟重启）：新状态从库恢复。
    let d2 = cx.update(|window, cx| Rc::new(ConnectionDialogState::new(window, cx)));
    cx.update(|window, cx| d2.staging_restore(window, cx));

    let drafts = cx.update(|_, _cx| d2.drafts.borrow().clone());
    assert_eq!(drafts.len(), 2, "重启后应恢复两条暂存条目");
    assert_eq!(drafts[0].name, "草稿A");
    assert_eq!(drafts[1].name, "草稿B");
    assert_eq!(drafts[0].url, "mysql://h:3306/a");
    assert_eq!(drafts[1].remark, "备注B");
    assert_eq!(
        drafts[0].auth_method, "password",
        "认证方法应随草稿持久化（新列 auth_method）"
    );
    assert!(
        drafts.iter().all(|d| d.pass.is_empty()),
        "密码不落库：恢复条目不应带密码"
    );

    // 表单应载入第 0 条（名称与 URL 与库中一致，密码为空）。
    let name_now = cx.update(|_, cx| d2.name.read(cx).value().to_string());
    let url_now = cx.update(|_, cx| d2.url.read(cx).value().to_string());
    let pass_now = cx.update(|_, cx| d2.pass.read(cx).value().to_string());
    assert_eq!(name_now, "草稿A");
    assert_eq!(url_now, "mysql://h:3306/a");
    assert!(pass_now.is_empty(), "恢复后密码框应为空（凭据不落库）");

    // 清理临时库。
    if let Some(dir) = db_path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}
