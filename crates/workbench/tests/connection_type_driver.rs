//! 数据库类型 × 驱动实现的两层选择测试（M3 C10）。
//!
//! 覆盖「左侧选类型 / 右侧选驱动实现（短名去噪）」链路：
//! - 侧栏选类型 → 驱动下拉选项切到该类型启用驱动并默认选中第一个，值为实现短名（如 `sqlx`）；
//! - 编辑回读 / 草稿恢复：按驱动 id 在当前类型下定位，选中短名（跨类型同名短名不歧义）；
//! - 表单快照携带 `type_id` / `driver_id`（条目类型徽标与恢复精确定位）。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入（会把 gpui 的 `test`
//! 宏带入作用域，与 `#[gpui_kit::test]` 冲突）。

use std::cell::RefCell;
use std::rc::Rc;

use engine::persistence::driver_store::{DataSourceType, Driver};
use gpui_kit::component::Root;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
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

fn driver(id: &str, type_id: &str, name: &str, enabled: bool) -> Driver {
    Driver {
        id: id.into(),
        type_id: type_id.into(),
        name: name.into(),
        driver_kind: "native".into(),
        is_file: matches!(type_id, "sqlite" | "duckdb"),
        default_port: None,
        url_template: None,
        download_url: None,
        download_checksum: None,
        version: None,
        config_schema: String::new(),
        supported_auth_types: None,
        capabilities: None,
        driver_properties: None,
        enabled,
    }
}

fn ds_type(id: &str, name: &str, icon: &str) -> DataSourceType {
    DataSourceType {
        id: id.into(),
        name: name.into(),
        category: "relational".into(),
        icon: Some(icon.into()),
        enabled: true,
        created_at: String::new(),
    }
}

/// 预置类型 + 驱动目录（不依赖全局库）。
fn seed_catalog(dialog: &ConnectionDialogState) {
    *dialog.types.borrow_mut() = vec![
        ds_type("mysql", "MySQL", "🐬"),
        ds_type("postgresql", "PostgreSQL", "🐘"),
    ];
    *dialog.drivers.borrow_mut() = vec![
        driver("mysql", "mysql", "MySQL (sqlx)", true),
        driver("mysql_native", "mysql", "MySQL (Official)", true),
        driver("mysql_legacy", "mysql", "MySQL (Legacy)", false),
        driver("postgres", "postgresql", "PostgreSQL (sqlx)", true),
    ];
}

#[gpui_kit::test]
fn selecting_type_scopes_driver_options_to_short_names(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|window, cx| {
        let dialog = Rc::new(ConnectionDialogState::new(window, cx));
        seed_catalog(&dialog);
        dialog
    });

    // 初始：未选类型 → 驱动下拉无选中值。
    assert!(cx.update(|_, cx| dialog.driver.read(cx).selected_value().is_none()));

    // 侧栏选 MySQL：默认选中该类型第一个启用驱动，值为实现短名（去噪）。
    cx.update(|window, cx| dialog.select_type("mysql", window, cx));
    let sel = cx.update(|_, cx| {
        dialog
            .driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string()
    });
    assert_eq!(sel, "sqlx", "驱动下拉应显示实现短名而非 MySQL (sqlx)");
    assert_eq!(cx.update(|_, _cx| dialog.selected_type.borrow().clone()), "mysql");

    // 实体状态未变更时，下拉选中值可被 set_driver_by_value 覆盖（编辑回读路径）。
    cx.update(|window, cx| {
        let hit = dialog.set_driver_by_value("mysql", "mysql_native", window, cx);
        assert_eq!(hit.map(|d| d.id), Some("mysql_native".to_string()));
    });
    let sel = cx.update(|_, cx| {
        dialog
            .driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string()
    });
    assert_eq!(sel, "Official", "按驱动 id 定位后应显示对应实现短名");

    // 跨类型同名短名（sqlx）不歧义：类型作用域优先。
    cx.update(|window, cx| {
        let hit = dialog.set_driver_by_value("postgresql", "postgres", window, cx);
        assert_eq!(hit.map(|d| d.id), Some("postgres".to_string()));
    });
    assert_eq!(
        cx.update(|_, _cx| dialog.selected_type.borrow().clone()),
        "postgresql"
    );
    let sel = cx.update(|_, cx| {
        dialog
            .driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string()
    });
    assert_eq!(sel, "sqlx");

    cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));
}

#[gpui_kit::test]
fn draft_snapshot_carries_type_and_driver_id(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|window, cx| {
        let dialog = Rc::new(ConnectionDialogState::new(window, cx));
        seed_catalog(&dialog);
        dialog
    });

    // 选类型（自动选中 sqlx）→ 表单快照应带 type_id + driver_id。
    cx.update(|window, cx| dialog.select_type("mysql", window, cx));
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("快照连接", window, cx));
    });
    cx.update(|window, cx| dialog.staging_add(window, cx));

    let first = cx.update(|_, _cx| dialog.drafts.borrow()[0].clone());
    assert_eq!(first.name, "快照连接");
    assert_eq!(first.type_id, "mysql");
    assert_eq!(first.driver_id, "mysql");
    assert_eq!(first.driver_name, "sqlx");

    // 切到另一类型 + 驱动：新条目快照独立记录。
    cx.update(|window, cx| dialog.select_type("postgresql", window, cx));
    cx.update(|window, cx| dialog.staging_add(window, cx));
    let second = cx.update(|_, _cx| dialog.drafts.borrow()[1].clone());
    assert_eq!(second.type_id, "postgresql");
    assert_eq!(second.driver_id, "postgres");

    cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));
}
