//! 编辑既有连接的 **UI 回填**（端到端：全局库 → 服务 → 对话框表单）。
//!
//! 覆盖生产入口 `EditorPanel::request_edit_connection` 的完整路径：
//! 编辑入口 → `open` → `load_for_edit` 读库 → 名称 / 地址 / 备注 / 标签 / 类型 /
//! 驱动 / 作用域逐项回填，并要求 5 个 Tab 都能渲染成帧。
//!
//! 本文件注入**临时**全局库（进程内单例，模式同 `connection_scope_and_state.rs`），
//! 不触碰用户真实数据目录。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入（会把 gpui 的 `test`
//! 宏带入作用域，与 `#[gpui_kit::test]` 冲突）。所有依赖显式列举。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;

use connection::model::DataSourceSaveInput;
use engine::persistence::global_db::GlobalDatabaseManager;
use gpui_kit::component::Root;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled as _,
    TestAppContext, VisualTestContext, Window, div,
};

use rds_workbench::panels::{EditorPanel, Shared};
use rds_workbench::services::data_source_service::DataSourceService;

static BASE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 注入临时全局库到应用单例（进程内一次；种子数据由迁移写入）。
fn base_dir() -> PathBuf {
    BASE_DIR
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("rds_backfill_{}", std::process::id()));
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

    /// 面板实体句柄（供宿主外部触发入口；见 `open_edit` 的重入说明）。
    fn editor(&self) -> Entity<EditorPanel> {
        self.editor.clone()
    }

    fn dialog(
        &self,
        cx: &App,
    ) -> Rc<rds_workbench::components::connection_dialog::ConnectionDialogState> {
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

/// 走生产入口打开编辑对话框：先取面板实体，再单独 update。
///
/// 宿主设置了 `host_redraw` 桥（与 `WorkbenchView` 同构），而面板入口会通过
/// `notify_host` 回调宿主重绘：若在 `Harness` 的 update 上下文内调用会重入 panic。
fn open_edit(harness: &Entity<Harness>, conn_id: String, cx: &mut VisualTestContext) {
    let editor = cx.update(|_, cx| harness.read(cx).editor());
    cx.update(|window, cx| {
        editor.update(cx, |e, cx| {
            e.request_edit_connection(conn_id, window, cx)
        })
    });
}

/// 编辑入口：服务层已落库的连接，表单必须逐项回填（此前只有服务层断言，无 UI 断言）。
#[gpui_kit::test]
fn editing_saved_connection_backfills_form(cx: &mut TestAppContext) {
    let base = base_dir();
    cx.update(gpui_kit::init);

    // 1) 用真实服务落一条全局只读（文件型，无需网络）连接。
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let service = DataSourceService::global()
        .expect("单例可用")
        .with_analysis_db(base.join("probe.duckdb"));
    let db_file = base.join("backfill.db");
    let mut input = DataSourceSaveInput::new(
        "回填用例",
        "sqlite",
        format!("sqlite://{}", db_file.display()),
    );
    input.description = Some("备注回填".into());
    input.tags = Some("[\"t1\",\"t2\"]".into());
    input.use_duckdb_fed = Some(false);
    let conn_id = rt.block_on(service.save(&input, None)).expect("保存连接");

    // 2) 走生产入口打开编辑对话框（`open` 内同步 `load_for_edit`）。
    let (harness, cx) = open_harness(cx);
    open_edit(&harness, conn_id.clone(), cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 3) 逐项回填断言（编辑 ID / 名称 / 地址 / 备注 / 标签 / 类型 / 驱动 / 作用域）。
    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));
    assert_eq!(
        cx.update(|_, _cx| dialog.editing_id.borrow().clone()).as_deref(),
        Some(conn_id.as_str()),
        "编辑入口应记录连接 ID"
    );
    assert_eq!(
        cx.update(|_, cx| dialog.name.read(cx).value().to_string()),
        "回填用例"
    );
    let url = cx.update(|_, cx| dialog.url.read(cx).value().to_string());
    assert!(url.contains("backfill.db"), "地址应回填文件路径：{url}");
    assert_eq!(
        cx.update(|_, cx| dialog.remark.read(cx).value().to_string()),
        "备注回填"
    );
    assert_eq!(
        cx.update(|_, cx| dialog.tags_input.read(cx).value().to_string()),
        "t1, t2",
        "标签 JSON 应回显为逗号分隔文本"
    );
    assert_eq!(
        cx.update(|_, _cx| dialog.selected_type.borrow().clone()),
        "sqlite",
        "类型应由驱动 id 反查回填"
    );
    let driver = cx.update(|_, cx| {
        dialog
            .driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string()
    });
    assert!(!driver.is_empty(), "驱动应回填（短名）：{driver:?}");
    let scope = cx.update(|_, cx| {
        dialog
            .scope
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string()
    });
    assert_eq!(scope, "仅全局", "默认为仅全局");

    // 4) 五个 Tab 逐一渲染（全局库就绪路径，类型 / 驱动 / 引用目录均有数据）。
    for tab in 0..5 {
        dialog.active_tab.set(tab);
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    // 5) 已保存连接不进暂存区（暂存区只放未保存草稿；用户决策）。
    assert!(
        cx.update(|_, _cx| dialog.drafts.borrow().iter().all(|d| d.saved_id.is_none())),
        "编辑既有连接不应把已保存条目录入暂存区"
    );

    // 6) 两列等高：目录就绪（类型树有内容）时，侧栏不得把对话框撑高。
    dialog.active_tab.set(0);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let side = cx
        .debug_bounds("conn-side-panel")
        .expect("侧栏应已渲染")
        .size
        .height;
    let body = cx
        .debug_bounds("conn-tab-body")
        .expect("Tab 内容区应已渲染")
        .size
        .height;
    assert_eq!(
        side, body,
        "侧栏与 Tab 内容区应等高（目录 / 类型树不得把对话框撑高）"
    );
}
