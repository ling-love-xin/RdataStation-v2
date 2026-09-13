//! 连续保存多个连接的端到端集成测试（C7）。
//!
//! 覆盖：注入临时全局库单例后，经 `DataSourceService::global()`（生产路径）连续保存
//! 两条连接；暂存列表按原型设计 §2.2 规则 4 呈现「条目转已保存 + 自动补空草稿」，
//! 且库中两条记录可读回。对话框保存按钮内部走同一服务调用，故以此覆盖该链路。
//!
//! 单例按进程只可注入一次：本文件启动时注入临时全局库（与其余测试二进制隔离）。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入（会把 gpui 的 `test`
//! 宏带入作用域，与 `#[gpui_kit::test]` 冲突）。

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;

use connection::model::DataSourceSaveInput;
use engine::persistence::global_db::GlobalDatabaseManager;
use gpui_kit::{
    IntoElement, Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
};
use rds_workbench::components::connection_dialog::{ConnectionDialogState, ConnectionDraft};
use rds_workbench::services::data_source_service::DataSourceService;

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 最小宿主视图（只为拿到窗口与测试上下文）。
struct Host;

impl Render for Host {
    fn render(&mut self, _window: &mut Window, _cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

/// 注入临时全局库到应用单例（进程内一次）。
fn base_dir() -> PathBuf {
    BASE_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("rds_multi_save_{}", std::process::id()));
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

fn sqlite_input(name: &str, path: &str) -> DataSourceSaveInput {
    let mut input = DataSourceSaveInput::new(name, "sqlite", &format!("sqlite://{path}"));
    input.use_duckdb_fed = Some(false);
    input
}

#[gpui_kit::test]
fn two_connections_saved_in_a_row(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let _ = base_dir();

    let (_, cx) = cx.add_window_view(|_, _cx| Host);
    let cx: &mut VisualTestContext = cx;

    let service = DataSourceService::global().expect("单例可用");
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let dialog = cx.update(|window, cx| Rc::new(ConnectionDialogState::new(window, cx)));

    // 第 1 条：保存 → 草稿移出暂存区 + 补空草稿（规则 4）。
    let id1 = rt
        .block_on(service.save(&sqlite_input("conn_one", "/tmp/one.db"), None))
        .expect("save first");
    assert!(id1.starts_with("G_conn_"), "{id1}");
    cx.update(|window, cx| dialog.staging_after_save(window, cx));

    {
        let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
        assert_eq!(
            drafts.len(),
            1,
            "首条保存后草稿移出暂存区（已落库），只留空草稿"
        );
        assert!(drafts[0].saved_id.is_none(), "暂存区不含已保存条目");
        assert!(drafts[0].name.is_empty());
        assert_eq!(cx.update(|_, _cx| dialog.draft_cursor.get()), 0);
    }

    // 第 2 条：在自动补位的空草稿上继续保存。
    let id2 = rt
        .block_on(service.save(&sqlite_input("conn_two", "/tmp/two.db"), None))
        .expect("save second");
    assert_ne!(id1, id2);
    cx.update(|window, cx| dialog.staging_after_save(window, cx));

    {
        let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
        assert_eq!(drafts.len(), 1, "连续保存两条后仍只剩空草稿");
        assert!(drafts[0].saved_id.is_none());
        assert_eq!(cx.update(|_, _cx| dialog.draft_cursor.get()), 0);
    }

    // 库中两条记录可读回（生产路径经服务单例）。
    let list = rt.block_on(service.list()).expect("list connections");
    let names: Vec<&str> = list.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"conn_one"), "{names:?}");
    assert!(names.contains(&"conn_two"), "{names:?}");
}

/// 暂存区只放未保存草稿：打开时不再并入库中已保存连接（用户决策，替代旧“合并已保存”）。
#[gpui_kit::test]
fn staging_lists_only_unsaved_drafts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let _ = base_dir();
    let (_, cx) = cx.add_window_view(|_, _cx| Host);
    let cx: &mut VisualTestContext = cx;

    let service = DataSourceService::global().expect("单例可用");
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let dialog = cx.update(|window, cx| Rc::new(ConnectionDialogState::new(window, cx)));

    // 库里先存一条真实连接（不得出现在暂存区），再塞一条历史残留的已保存条目（应被清理）。
    let saved = rt
        .block_on(service.save(&sqlite_input("only_drafts", "/tmp/only.db"), None))
        .expect("save");
    assert!(saved.starts_with("G_conn_"), "{saved}");
    cx.update(|_, _cx| {
        let mut drafts = dialog.drafts.borrow_mut();
        drafts[0].name = "未保存草稿".to_string();
        drafts.push(ConnectionDraft {
            name: "历史残留".to_string(),
            saved_id: Some("G_conn_gone".to_string()),
            ..ConnectionDraft::empty()
        });
    });

    cx.update(|_, _cx| dialog.staging_prune_saved());

    let drafts = cx.update(|_, _cx| dialog.drafts.borrow().clone());
    let names: Vec<&str> = drafts.iter().map(|d| d.name.as_str()).collect();
    assert!(
        names.contains(&"未保存草稿"),
        "未保存草稿不得被清理：{names:?}"
    );
    assert!(!names.contains(&"历史残留"), "已保存条目应被清理：{names:?}");
    assert!(
        !names.contains(&"only_drafts"),
        "库中连接不得被并入暂存区：{names:?}"
    );
    assert!(drafts.iter().all(|d| d.saved_id.is_none()), "{names:?}");
    let cursor = cx.update(|_, _cx| dialog.draft_cursor.get());
    assert!(cursor < drafts.len(), "光标不应越界：{cursor} / {}", drafts.len());
}

/// 分组同步（替换语义）经服务单例走进真实项目库。
#[test]
fn groups_sync_through_service() {
    let base = base_dir();
    let project_root = base.join("proj-groups");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let root_str = project_root.to_string_lossy().to_string();

    let service = DataSourceService::global().expect("单例可用");

    // 项目库里建两个分组（分组管理 UI 归导航模块，这里直接走 store）。
    let store = engine::persistence::ConnectionOrgStore::open_project(&project_root)
        .expect("open project org store");
    store.create_group("grp_a", "业务库", None).expect("create a");
    store.create_group("grp_b", "测试库", None).expect("create b");
    assert_eq!(service.list_groups(Some(&root_str)).len(), 2);

    // 替换语义：先勾两个 → 只留一个 → 清空。
    let conn = "P_conn_groups_probe";
    service
        .set_connection_groups(
            conn,
            &["grp_a".to_string(), "grp_b".to_string()],
            Some(&root_str),
        )
        .expect("分组同步");
    let mut got = service.groups_of(conn, Some(&root_str));
    got.sort();
    assert_eq!(got, vec!["grp_a".to_string(), "grp_b".to_string()]);

    service
        .set_connection_groups(conn, &["grp_b".to_string()], Some(&root_str))
        .expect("分组同步（替换为单组）");
    assert_eq!(
        service.groups_of(conn, Some(&root_str)),
        vec!["grp_b".to_string()]
    );

    service
        .set_connection_groups(conn, &[], Some(&root_str))
        .expect("分组同步（清空）");
    assert!(service.groups_of(conn, Some(&root_str)).is_empty());

    // 未打开项目：分组为项目级能力，读取为空、写入忽略（不报错，且不算降级）。
    assert!(service.list_groups(None).is_empty());
    service
        .set_connection_groups(conn, &["grp_a".to_string()], None)
        .expect("未打开项目时不报错");
}
