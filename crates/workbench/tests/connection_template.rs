//! 连接模板导入导出测试（C4）。
//!
//! 覆盖：导出未保存草稿为模板 JSON（**不含密码**）、导入追加为草稿并解析驱动短名、
//! 非法输入被拒绝（非 JSON / kind 不符 / 版本过新 / 空列表）。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入（会把 gpui 的 `test`
//! 宏带入作用域，与 `#[gpui_kit::test]` 冲突）。

use std::cell::RefCell;
use std::rc::Rc;

use engine::persistence::driver_store::Driver;
use gpui_kit::component::Root;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
};

use rds_workbench::components::connection_dialog::ConnectionDialogState;
use rds_workbench::panels::{EditorPanel, Shared};

struct Harness {
    _shared: Shared,
    editor: Entity<EditorPanel>,
}

impl Harness {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shared = Shared::new();
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

fn driver(id: &str, type_id: &str, name: &str) -> Driver {
    Driver {
        id: id.into(),
        type_id: type_id.into(),
        name: name.into(),
        driver_kind: "native".into(),
        is_file: false,
        default_port: None,
        url_template: None,
        download_url: None,
        download_checksum: None,
        version: None,
        config_schema: String::new(),
        supported_auth_types: None,
        capabilities: None,
        driver_properties: None,
        enabled: true,
    }
}

fn seed_catalog(dialog: &ConnectionDialogState) {
    *dialog.drivers.borrow_mut() = vec![
        driver("mysql", "mysql", "MySQL (sqlx)"),
        driver("postgres", "postgresql", "PostgreSQL (sqlx)"),
    ];
}

/// 造一条可导出的草稿（含密码，用于验证导出脱敏）。
fn seed_draft(dialog: &ConnectionDialogState, window: &mut Window, cx: &mut gpui_kit::App) {
    dialog
        .name
        .update(cx, |s, cx| s.set_value("生产 PG", window, cx));
    dialog
        .url
        .update(cx, |s, cx| s.set_value("postgres://h:5432/db", window, cx));
    dialog
        .user
        .update(cx, |s, cx| s.set_value("reader", window, cx));
    dialog
        .pass
        .update(cx, |s, cx| s.set_value("secret-a", window, cx));
    dialog
        .remark
        .update(cx, |s, cx| s.set_value("只读账号", window, cx));
    dialog
        .tags_input
        .update(cx, |s, cx| s.set_value("prod, core", window, cx));
    dialog.select_type("postgresql", window, cx);
}

#[gpui_kit::test]
fn export_excludes_password_and_import_restores(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|window, cx| {
        let dialog = Rc::new(ConnectionDialogState::new(window, cx));
        seed_catalog(&dialog);
        dialog
    });

    // 造草稿：写入后「+ 添加」把当前表单快照存入条目 0。
    cx.update(|window, cx| seed_draft(&dialog, window, cx));
    cx.update(|window, cx| dialog.staging_add(window, cx));

    let json = cx.update(|_, _cx| dialog.templates_export());
    assert!(json.contains("rds.connection.template"), "{json}");
    assert!(json.contains("生产 PG"), "{json}");
    assert!(
        !json.contains("secret-a") && !json.contains("\"pass\""),
        "模板不得包含密码：{json}"
    );
    assert_eq!(cx.update(|_, _cx| dialog.templates_export_count()), 1);

    // 导入到同一对话框：应追加一条草稿，密码为空，类型与驱动短名已解析。
    let n = cx.update(|window, cx| {
        dialog
            .templates_import(&json, window, cx)
            .expect("import ok")
    });
    assert_eq!(n, 1);
    let imported = cx.update(|_, _cx| {
        let drafts = dialog.drafts.borrow();
        drafts.last().cloned().expect("imported draft")
    });
    assert_eq!(imported.name, "生产 PG");
    assert_eq!(imported.url, "postgres://h:5432/db");
    assert_eq!(imported.user, "reader");
    assert!(imported.pass.is_empty(), "导入条目的密码应为空");
    assert_eq!(imported.type_id, "postgresql");
    assert_eq!(imported.driver_id, "postgres");
    assert_eq!(imported.driver_name, "sqlx", "驱动短名应由驱动目录解析");
    assert_eq!(imported.tags, "prod, core");

    // 导入后光标与表单落在新条目上。
    let name_now = cx.update(|_, cx| dialog.name.read(cx).value().to_string());
    assert_eq!(name_now, "生产 PG");

    cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));
}

#[gpui_kit::test]
fn import_rejects_invalid_payloads(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|window, cx| {
        let dialog = Rc::new(ConnectionDialogState::new(window, cx));
        seed_catalog(&dialog);
        dialog
    });

    let err = cx.update(|window, cx| {
        dialog
            .templates_import("not json", window, cx)
            .expect_err("非 JSON 应被拒绝")
    });
    assert!(err.contains("解析失败"), "{err}");

    let err = cx.update(|window, cx| {
        dialog
            .templates_import(r#"{"kind":"other","version":1,"connections":[]}"#, window, cx)
            .expect_err("kind 不符应被拒绝")
    });
    assert!(err.contains("kind"), "{err}");

    let err = cx.update(|window, cx| {
        dialog
            .templates_import(
                r#"{"kind":"rds.connection.template","version":99,"connections":[]}"#,
                window,
                cx,
            )
            .expect_err("版本过新应被拒绝")
    });
    assert!(err.contains("版本过新"), "{err}");

    let err = cx.update(|window, cx| {
        dialog
            .templates_import(
                r#"{"kind":"rds.connection.template","version":1,"connections":[]}"#,
                window,
                cx,
            )
            .expect_err("空列表应被拒绝")
    });
    assert!(err.contains("没有连接"), "{err}");

    // 失败不应污染暂存列表（仍为初始单空草稿）。
    let len = cx.update(|_, _cx| dialog.drafts.borrow().len());
    assert_eq!(len, 1);

    cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));
}
