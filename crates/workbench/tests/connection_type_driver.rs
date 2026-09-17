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

fn driver_with_auth(
    id: &str,
    type_id: &str,
    name: &str,
    supported_auth_types: Option<&str>,
) -> Driver {
    Driver {
        supported_auth_types: supported_auth_types.map(|s| s.to_string()),
        ..driver(id, type_id, name, true)
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
fn type_without_enabled_driver_is_refused(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|window, cx| {
        let dialog = Rc::new(ConnectionDialogState::new(window, cx));
        // 目录里有 Oracle 类型，但无对应驱动（与种子目录一致：只内置 4 个驱动）。
        *dialog.types.borrow_mut() = vec![
            ds_type("mysql", "MySQL", "🐬"),
            ds_type("oracle", "Oracle", "🔴"),
        ];
        *dialog.drivers.borrow_mut() = vec![
            driver("mysql", "mysql", "MySQL (sqlx)", true),
            // 同类型下仅有的驱动被禁用 → 同样视为“无可用驱动”。
            driver("oracle_legacy", "oracle", "Oracle (Legacy)", false),
        ];
        dialog
    });

    // 选无可用驱动的类型：拒绝切换（选中仍为空）+ 结果行给出原因。
    cx.update(|window, cx| dialog.select_type("oracle", window, cx));
    assert_eq!(
        cx.update(|_, _cx| dialog.selected_type.borrow().clone()),
        "",
        "无可用驱动的类型不应被选中"
    );
    assert!(
        cx.update(|_, cx| dialog.driver.read(cx).selected_value().is_none()),
        "拒绝选中后驱动下拉仍为空"
    );
    let msg = cx
        .update(|_, _cx| dialog.result_summary())
        .unwrap_or_default();
    assert!(msg.contains("暂无可用驱动"), "应给出原因：{msg}");
    assert_eq!(
        cx.update(|_, _cx| dialog.result_level()),
        Some(rds_workbench::components::connection_dialog::ResultLevel::Error),
        "提示应为失败级（#28 分级）"
    );

    // 有可用驱动的类型仍可正常选中。
    cx.update(|window, cx| dialog.select_type("mysql", window, cx));
    assert_eq!(
        cx.update(|_, _cx| dialog.selected_type.borrow().clone()),
        "mysql"
    );

    cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));
}

#[gpui_kit::test]
fn auth_method_follows_driver_declaration(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|window, cx| {
        let dialog = Rc::new(ConnectionDialogState::new(window, cx));
        *dialog.types.borrow_mut() = vec![ds_type("mysql", "MySQL", "🐬")];
        // 两个驱动声明不同的认证方法：sqlx 只有 password；Official 额外声明 ssl。
        *dialog.drivers.borrow_mut() = vec![
            driver_with_auth("mysql", "mysql", "MySQL (sqlx)", Some(r#"["password"]"#)),
            driver_with_auth(
                "mysql_native",
                "mysql",
                "MySQL (Official)",
                Some(r#"["password","ssl"]"#),
            ),
        ];
        dialog
    });

    // 选类型 → 默认驱动 sqlx → 认证方法默认选中声明的第一个。
    cx.update(|window, cx| dialog.select_type("mysql", window, cx));
    let method = cx.update(|_, cx| {
        dialog
            .auth_method
            .read(cx)
            .selected_value()
            .cloned()
            .map(|v| v.to_string())
            .unwrap_or_default()
    });
    assert_eq!(method, "password", "应默认选中驱动声明的第一个方法");

    // 换驱动 → 选项与方法随驱动声明重建（Official 声明含 ssl，保持已选的 password）。
    cx.update(|window, cx| {
        dialog.set_driver_by_value("mysql", "mysql_native", window, cx);
    });
    let method = cx.update(|_, cx| {
        dialog
            .auth_method
            .read(cx)
            .selected_value()
            .cloned()
            .map(|v| v.to_string())
            .unwrap_or_default()
    });
    assert_eq!(method, "password", "已选方法仍在选项内时应保留");

    // 草稿快照应携带认证方法（切条目 / 重启后不丢）。
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("认证方法", window, cx));
        dialog
            .url
            .update(cx, |s, cx| s.set_value("mysql://h:3306/d", window, cx));
    });
    cx.update(|window, cx| dialog.staging_add(window, cx));
    let draft = cx.update(|_, _cx| dialog.drafts.borrow()[0].clone());
    assert_eq!(draft.auth_method, "password", "快照应带认证方法");

    cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));
}

#[gpui_kit::test]
fn connection_fields_and_uri_stay_in_sync(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    // 走生产入口打开对话框：双向同步写在 render builder 里，不打开就不会执行。
    let editor = cx.update(|_, cx| harness.read(cx).editor.clone());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| e.request_new_connection(window, cx));
    });
    let dialog = cx.update(|_, cx| editor.read(cx).dialog_state().expect("对话框状态已创建"));
    cx.update(|_, _cx| {
        *dialog.types.borrow_mut() = vec![ds_type("mysql", "MySQL", "🐬")];
        let mut mysql = driver("mysql", "mysql", "MySQL (sqlx)", true);
        mysql.default_port = Some(3306);
        *dialog.drivers.borrow_mut() = vec![mysql];
    });
    cx.update(|window, cx| dialog.select_type("mysql", window, cx));

    // 方向 A（URI → 字段）：从 URI 解析出主机 / 端口 / 数据库。
    cx.update(|window, cx| {
        dialog
            .url
            .update(cx, |s, cx| s.set_value("mysql://root:pw@h:3306/old", window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let (host, port, db) = cx.update(|_, cx| {
        (
            dialog.host_input.read(cx).value().to_string(),
            dialog.port_input.read(cx).value().to_string(),
            dialog.db_input.read(cx).value().to_string(),
        )
    });
    assert_eq!(host, "h");
    assert_eq!(port, "3306");
    assert_eq!(db, "old");

    // 方向 B（字段 → URI）：改字段回写 URI（凭据保留），因此“连接设置”可直接输入。
    cx.update(|window, cx| {
        dialog
            .host_input
            .update(cx, |s, cx| s.set_value("127.0.0.1", window, cx));
        dialog
            .db_input
            .update(cx, |s, cx| s.set_value("newdb", window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let url = cx.update(|_, cx| dialog.url.read(cx).value().to_string());
    assert_eq!(url, "mysql://root:pw@127.0.0.1:3306/newdb");

    // 幂等：再画一帧不得反复改动（两个方向已一致）。
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let url_again = cx.update(|_, cx| dialog.url.read(cx).value().to_string());
    assert_eq!(url_again, url);

    cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));
}

#[gpui_kit::test]
fn file_type_general_tab_renders_and_switches_back(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    let dialog = cx.update(|window, cx| {
        let dialog = Rc::new(ConnectionDialogState::new(window, cx));
        *dialog.types.borrow_mut() = vec![
            ds_type("sqlite", "SQLite", "🪶"),
            ds_type("mysql", "MySQL", "🐬"),
        ];
        *dialog.drivers.borrow_mut() = vec![
            driver("sqlite", "sqlite", "SQLite (rusqlite)", true),
            driver("mysql", "mysql", "MySQL (sqlx)", true),
        ];
        dialog
    });

    // 选文件型：默认驱动 rusqlite；常规 Tab 走文件型分支（地址 + 打开/新建，无认证/SSL 卡片）。
    cx.update(|window, cx| dialog.select_type("sqlite", window, cx));
    let sel = cx.update(|_, cx| {
        dialog
            .driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string()
    });
    assert_eq!(sel, "rusqlite");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 切回网络型：占位、卡片与 Tab 条随之重建（渲染不得 panic，且回归到 URI 行）。
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
    assert_eq!(sel, "sqlx");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 单列分组大纲：折叠态默认展开；切换折叠后重渲染（含文件型 / 网络型两种分组集合）。
    assert!(!cx.update(|_, _cx| dialog.section_collapsed("conn")));
    cx.update(|_, _cx| dialog.toggle_section("conn"));
    assert!(cx.update(|_, _cx| dialog.section_collapsed("conn")));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|_, _cx| dialog.toggle_section("conn"));
    assert!(!cx.update(|_, _cx| dialog.section_collapsed("conn")));

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

/// 地址占位：随驱动推导，且只在真正变化时写入（避免每帧 `set_placeholder` 触发多余重绘）。
///
/// 回归点：`InputState::set_placeholder` 是无条件赋值 + `notify`（gpui-base
/// `input/base/state.rs`），所以 render 侧必须有 `url_placeholder_for` 守卫。
#[gpui_kit::test]
fn address_placeholder_is_cached_and_follows_driver(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    // 走生产入口：占位写入在 render builder 里，不打开对话框不会执行。
    let editor = cx.update(|_, cx| harness.read(cx).editor.clone());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| e.request_new_connection(window, cx));
    });
    let dialog = cx.update(|_, cx| editor.read(cx).dialog_state().expect("对话框状态已创建"));
    cx.update(|_, _cx| {
        *dialog.types.borrow_mut() = vec![
            ds_type("mysql", "MySQL", "🐬"),
            ds_type("sqlite", "SQLite", "🪶"),
        ];
        *dialog.drivers.borrow_mut() = vec![
            driver("mysql", "mysql", "MySQL (sqlx)", true),
            driver("sqlite", "sqlite", "SQLite (rusqlite)", true),
        ];
    });

    // 网络型：占位按驱动推导，缓存值与输入框实际占位一致。
    cx.update(|window, cx| dialog.select_type("mysql", window, cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let ph_network = cx.update(|_, _cx| dialog.url_placeholder_for.borrow().clone());
    assert!(ph_network.contains("主机"), "网络型占位应含示例地址：{ph_network}");
    let input_ph = cx.update(|_, cx| dialog.url.read(cx).presentation().placeholder().to_string());
    assert_eq!(input_ph, ph_network, "缓存值应与输入框占位一致");

    // 再画一帧：占位未变 → 缓存不变（守卫拦下重复写入）。
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let ph_again = cx.update(|_, _cx| dialog.url_placeholder_for.borrow().clone());
    assert_eq!(ph_again, ph_network, "占位未变化时不应重写");

    // 切文件型：占位随驱动变化（缓存同步更新，输入框跟进）。
    cx.update(|window, cx| dialog.select_type("sqlite", window, cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let ph_file = cx.update(|_, _cx| dialog.url_placeholder_for.borrow().clone());
    assert_ne!(ph_file, ph_network, "文件型占位应与网络型不同");
    let input_ph = cx.update(|_, cx| dialog.url.read(cx).presentation().placeholder().to_string());
    assert_eq!(input_ph, ph_file, "切驱动后输入框占位应同步");
}
