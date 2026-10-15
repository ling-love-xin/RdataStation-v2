//! 规则 1/2（决策 #110/#111，2026-10-02）行为断言。
//!
//! 规则 1（#110）：新建文件型连接（SQLite / DuckDB）选中类型时，地址自动预填当前项目根
//! （「地址=本项目」）；网络型不预填；已填值不覆盖。默认作用域=「仅项目」的渲染回退口径
//! 由 `helpers::default_new_scope` 单测钉住（本文件不重复）。
//!
//! 规则 2（#111）：保存 = 追加暂存条目 + 自动新建空草稿。对话框保存按钮内部的
//! `do_stage` 闭包不可从外部测试，本文件钉住其**组成动作**：`capture_draft` 把表单写回
//! 当前草稿（快照口径）→ 非空草稿经 `staging_add` 追加空草稿并选中 → 新草稿为空、
//! 表单清空（可直接填下一条）；空草稿拦截逻辑在闭包内，由代码 review 覆盖。
//!
//! 注入临时全局库到服务单例（与 connection_multi_save 同构，进程内一次）。

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;

use engine::persistence::driver_store::Driver;
use engine::persistence::global_db::GlobalDatabaseManager;
use gpui_kit::{
    IntoElement, Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
};
use rds_workbench::components::connection_dialog::ConnectionDialogState;

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

struct Host;

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

/// 最小驱动声明（与 state.rs 集成测试同构：字段按当前 Driver 结构填充）。
fn driver(id: &str, type_id: &str, kind: &str, is_file: bool) -> Driver {
    Driver {
        id: id.into(),
        type_id: type_id.into(),
        name: id.into(),
        driver_kind: kind.into(),
        is_file,
        default_port: None,
        url_template: None,
        download_url: None,
        download_checksum: None,
        version: None,
        config_schema: "{}".into(),
        supported_auth_types: None,
        capabilities: None,
        driver_properties: None,
        enabled: true,
    }
}

/// 注入临时全局库到应用单例（进程内一次）。
fn base_dir() -> PathBuf {
    BASE_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("rds_rule12_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let manager = rt
                .block_on(GlobalDatabaseManager::new(
                    dir.join("global.db"),
                    dir.join("analytics.duckdb"),
                    2,
                ))
                .expect("init global db");
            engine::migration::install_global_db_manager(manager).expect("inject singleton");
            std::mem::forget(rt);
            dir
        })
        .clone()
}

/// 规则 1：选中文件型类型（SQLite / DuckDB）→ 地址自动预填当前项目根；
/// 网络型（MySQL）不预填；用户已填值不被覆盖。
#[gpui_kit::test]
fn select_type_prefills_project_root_for_file_db_only(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let _ = base_dir();

    let (_, cx) = cx.add_window_view(|_, _cx| Host);
    let cx: &mut VisualTestContext = cx;
    let dialog = cx.update(|window, cx| Rc::new(ConnectionDialogState::<Host>::new(window, cx)));

    // 项目根（open() 会从会话注入；这里直接设置模拟已打开项目）。
    cx.update(|window, cx| {
        dialog
            .project_path
            .update(cx, |s, cx| s.set_value("D:/proj/root", window, cx));
    });

    // 驱动目录（select_type 的 type_has_driver 检查依赖它；生产由服务注入）。
    cx.update(|_, _cx| {
        *dialog.drivers.borrow_mut() = vec![
            driver("sqlite", "sqlite", "file", true),
            driver("duckdb", "duckdb", "file", true),
            driver("mysql", "mysql", "network", false),
        ];
    });

    // SQLite（文件型）→ 地址 = 项目根。
    cx.update(|window, cx| dialog.select_type("sqlite", window, cx));
    let url = cx.update(|_, cx| dialog.url.read(cx).value().to_string());
    assert_eq!(url, "D:/proj/root", "文件型选中后地址应预填项目根");

    // 用户已填值不覆盖：改地址后再选 DuckDB（同为文件型）。
    cx.update(|window, cx| {
        dialog
            .url
            .update(cx, |s, cx| s.set_value("keep.db", window, cx));
    });
    cx.update(|window, cx| dialog.select_type("duckdb", window, cx));
    let url = cx.update(|_, cx| dialog.url.read(cx).value().to_string());
    assert_eq!(url, "keep.db", "已填地址不被类型切换覆盖");

    // 网络型（MySQL）→ 不预填（URI 没有「本项目」语义），保持原值。
    cx.update(|window, cx| dialog.select_type("mysql", window, cx));
    let url = cx.update(|_, cx| dialog.url.read(cx).value().to_string());
    assert_eq!(url, "keep.db", "网络型不应预填项目根");

    // 无项目根时不预填（文件型也不应凭空造地址）。
    cx.update(|window, cx| {
        dialog
            .url
            .update(cx, |s, cx| s.set_value("", window, cx));
        dialog
            .project_path
            .update(cx, |s, cx| s.set_value("", window, cx));
    });
    cx.update(|window, cx| dialog.select_type("sqlite", window, cx));
    let url = cx.update(|_, cx| dialog.url.read(cx).value().to_string());
    assert!(url.is_empty(), "无项目根时地址保持为空");
}

/// 规则 2：保存流程的组成动作——`capture_draft` 写回表单快照；
/// 非空草稿经 `staging_add` 追加空草稿并选中；新草稿为空、表单清空可继续填。
#[gpui_kit::test]
fn save_flow_captures_then_appends_blank_draft(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let _ = base_dir();

    let (_, cx) = cx.add_window_view(|_, _cx| Host);
    let cx: &mut VisualTestContext = cx;
    let dialog = cx.update(|window, cx| Rc::new(ConnectionDialogState::<Host>::new(window, cx)));

    // 初始 1 个空草稿：do_stage 的空草稿拦截口径（全空不追加；`is_empty_draft` 为
    // pub(crate)，集成测试按同口径内联判断）。
    let d0 = cx.update(|_, _cx| dialog.drafts.borrow()[0].clone());
    assert!(
        d0.saved_id.is_none()
            && d0.name.trim().is_empty()
            && d0.url.trim().is_empty()
            && d0.driver_id.trim().is_empty(),
        "空草稿应保持全空（保存时拦截、不产生空条目）"
    );

    // 填写名称（规则 2 的“有内容才暂存”最小条件：名称非空即非空草稿）。
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("新连接", window, cx));
    });
    // 保存 → `staging_add`（do_stage 的追加动作：capture 写回 + 追加空草稿 + 选中）。
    cx.update(|window, cx| dialog.staging_add(window, cx));
    let (d0, len, cursor, d1, name_now) = cx.update(|_, cx| {
        let drafts = dialog.drafts.borrow();
        (
            drafts[0].clone(),
            drafts.len(),
            dialog.draft_cursor.get(),
            drafts[1].clone(),
            dialog.name.read(cx).value().to_string(),
        )
    });
    assert_eq!(d0.name, "新连接", "保存前表单已写回当前草稿快照");
    assert_eq!(len, 2, "保存后应追加一条新草稿");
    assert_eq!(cursor, 1, "光标应指向新草稿（可直接填下一条）");
    assert!(
        d1.name.trim().is_empty() && d1.url.trim().is_empty() && d1.saved_id.is_none(),
        "追加的应是空草稿"
    );
    assert!(name_now.is_empty(), "表单应清空（载入新空草稿）");
}
