//! 数据库导航的宿主端口实现（`database::nav_host::NavHost`）。
//!
//! 与 `mock_host.rs` / `project_host.rs` 同形：特性 crate 定义 trait，宿主持 `Shared`
//! 并实现它，装配期注入面板（`NavView::new(host, cx)`）。
//!
//! 实现原则——**只做转接，不加戏**：
//!
//! - 宿主状态读写直接落到 `Shared` 的对应字段（连接清单 / 选中 / 提示 / 项目根）；
//! - 连接生命周期与增删改转交 `crate::services::{nav_runtime, data_source_service}`，
//!   统一走**进程级桥接运行时**（`nav_runtime::bridge_runtime`）。这些是同步 `block_on`，
//!   只允许在事件路径调用（渲染期不调，见 trait 文档）；
//! - 视图偏好转交 `settings::SettingsService`，`NavFilters` 与
//!   `settings::model::NavigatorFilters` 在此互转（避免 `database → settings` 依赖）；
//! - 需要窗口 / 别的面板的命令转交既有组件（`group_form_dialog` / `cache_dialog`）与
//!   `Shared` 的端口方法（`editor_bridge` 等）。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{App, Window};

use database::model::{PropertyRequest, TableRef};
use database::nav_host::{ConnectionProbe, NavFilters, NavHost};
use workbench_shell::model::{ConnectionItem, GroupFormSeed, QueryRequest, RightPanel};

use crate::panels::Shared;
use crate::services::data_source_service::DataSourceService;
use crate::services::{nav_runtime, workspace_loader};

/// 宿主端口实现（无状态外壳，状态全在 [`Shared`]）。
pub struct WorkbenchNavHost {
    shared: Shared,
}

impl WorkbenchNavHost {
    pub fn new(shared: Shared) -> Self {
        Self { shared }
    }

    /// 当前项目根（字符串形态：服务层与 `nav_runtime` 的入参口径）。
    fn root_text(&self) -> Option<String> {
        self.shared
            .project_root()
            .map(|p| p.to_string_lossy().to_string())
    }

    /// 在**进程级**桥接运行时上跑一次异步服务调用。
    ///
    /// 不用 `tokio::runtime::Runtime::new()`：连接池建立在首次 `connect` 的运行时上，
    /// 临时运行时被丢弃会让池的后台任务失去宿主（见 `nav_runtime::bridge_runtime` 文档）。
    fn block_on<T>(
        fut: impl std::future::Future<Output = Result<T, shared::error::CoreError>>,
    ) -> Result<T, String> {
        let rt = nav_runtime::bridge_runtime()?;
        rt.block_on(fut).map_err(|e| e.to_string())
    }
}

impl NavHost for WorkbenchNavHost {
    // ==================== 宿主状态（读） ====================

    fn connections(&self) -> Vec<ConnectionItem> {
        self.shared.connections.borrow().clone()
    }

    fn selected_index(&self) -> Option<usize> {
        self.shared.selected.get()
    }

    fn project_root(&self) -> Option<PathBuf> {
        self.shared.project_root()
    }

    // ==================== 宿主状态（写） ====================

    fn select_connection(&self, index: Option<usize>, cx: &mut App) {
        self.shared.selected.set(index);
        // 切换连接 → 导航元数据缓存失效（原先由宿主订阅 `SidebarEvent::SelectConnection`
        // 完成；选中态现由端口直达，订阅只剩重绘一件事）。
        self.shared.invalidate_nav_cache();
        self.shared.notify_host(cx);
    }

    fn notice(&self, message: String, cx: &mut App) {
        *self.shared.notice.borrow_mut() = Some(message);
        // 状态栏由宿主渲染，子视图 notify 不会重绘它，故这里显式请求宿主重绘。
        self.shared.notify_host(cx);
    }

    fn notify_host(&self, cx: &mut App) {
        self.shared.notify_host(cx);
    }

    // ==================== 视图偏好 ====================

    fn show_tags(&self, cx: &App) -> bool {
        settings::SettingsService::show_tags(cx)
    }

    fn set_show_tags(&self, value: bool, cx: &mut App) {
        settings::SettingsService::set_show_tags(value, cx);
    }

    fn show_scope(&self, cx: &App) -> bool {
        settings::SettingsService::show_scope(cx)
    }

    fn set_show_scope(&self, value: bool, cx: &mut App) {
        settings::SettingsService::set_show_scope(value, cx);
    }

    fn source_short_code(&self, cx: &App) -> bool {
        settings::SettingsService::source_short_code(cx)
    }

    fn nav_filters(&self, cx: &App) -> NavFilters {
        let f = settings::SettingsService::nav_filters(cx);
        NavFilters {
            source: f.source,
            db_type: f.db_type,
            driver: f.driver,
            tag: f.tag,
        }
    }

    fn set_nav_filters(&self, filters: NavFilters, cx: &mut App) {
        settings::SettingsService::set_nav_filters(
            settings::model::NavigatorFilters {
                source: filters.source,
                db_type: filters.db_type,
                driver: filters.driver,
                tag: filters.tag,
            },
            cx,
        );
    }

    // ==================== 连接生命周期 ====================

    fn is_connected(&self, conn_id: &str) -> bool {
        nav_runtime::is_connected(conn_id)
    }

    fn connect(&self, conn_id: &str) -> Result<(), String> {
        let root = self.root_text();
        nav_runtime::connect_entry(conn_id, root.as_deref())
    }

    fn disconnect(&self, conn_id: &str) -> Result<(), String> {
        nav_runtime::disconnect_entry(conn_id)
    }

    fn connection_probe(&self) -> ConnectionProbe {
        // `nav_runtime::test_entry` 的签名与本类型一致（无状态函数：内部自取
        // `DataSourceService` 单例 + 进程级桥接运行时），故可直接作为函数指针使用。
        nav_runtime::test_entry
    }

    // ==================== 连接增删改 / 共享 ====================

    fn reload_connections(&self, cx: &mut App) {
        let root = self.shared.project_root();
        let (items, notice) = workspace_loader::load_connections_for_scope(root.as_deref());
        let len = items.len();
        *self.shared.connections.borrow_mut() = items;
        // 选中下标修正：空表取消选中，越界回退首项。
        if len == 0 {
            self.shared.selected.set(None);
        } else if self.shared.selected.get().is_some_and(|i| i >= len) {
            self.shared.selected.set(Some(0));
        }
        if let Some(n) = notice {
            *self.shared.notice.borrow_mut() = Some(n);
        }
        self.shared.notify_host(cx);
    }

    fn copy_connection(&self, from_id: &str, new_name: &str) -> Result<(), String> {
        let root = self.root_text();
        let service = DataSourceService::global().map_err(|e| e.to_string())?;
        Self::block_on(service.duplicate_as_template(from_id, root.as_deref(), new_name))
            .map(|_| ())
    }

    fn share_connection(&self, conn_id: &str) -> Result<(), String> {
        let root = self
            .root_text()
            .ok_or_else(|| "未打开项目：无法共享".to_string())?;
        let service = DataSourceService::global().map_err(|e| e.to_string())?;
        Self::block_on(service.share_to_project(conn_id, &root)).map(|_| ())
    }

    fn delete_connection(&self, conn_id: &str) -> Result<String, String> {
        let root = self.root_text();
        let service = DataSourceService::global().map_err(|e| e.to_string())?;
        Self::block_on(service.delete(conn_id, root.as_deref())).map(|r| r.message)
    }

    // ==================== 宿主命令 ====================

    fn show_properties(&self, request: PropertyRequest, cx: &mut App) {
        self.shared.show_properties(request, cx);
    }

    fn open_query(&self, request: QueryRequest, cx: &mut App) {
        self.shared.request_query(request);
        self.shared.notify_host(cx);
    }

    fn edit_connection(&self, conn_id: &str, window: &mut Window, cx: &mut App) {
        self.shared.edit_connection(conn_id.to_string(), window, cx);
    }

    fn new_connection(&self, window: &mut Window, cx: &mut App) {
        self.shared.new_connection(window, cx);
    }

    fn open_group_form(
        &self,
        seed: GroupFormSeed,
        window: &mut Window,
        cx: &mut App,
        on_submit: Rc<dyn Fn(Option<String>, String, Option<String>, &mut App)>,
    ) {
        crate::components::group_form_dialog::open_group_form_dialog(
            window,
            cx,
            seed,
            move |gid, name, desc, app| on_submit(gid, name, desc, app),
        );
    }

    fn open_cache_dialog(&self, window: &mut Window, cx: &mut App) {
        crate::components::cache_dialog::open_cache_dialog(window, cx, &self.shared);
    }

    fn open_right_panel(&self, panel: RightPanel, cx: &mut App) {
        self.shared.open_right_panel(panel, cx);
    }

    fn open_mock_panel(&self, source: Option<TableRef>, cx: &mut App) {
        // `database::model::TableRef` → `mock::mock_view::SchemaRequest`：
        // 两侧不想互相依赖，转换落在宿主。
        let source = source.map(|t| mock::mock_view::SchemaRequest {
            conn_id: t.conn_id,
            catalog: t.catalog,
            schema: t.schema,
            table: t.table,
        });
        self.shared.open_mock_panel(source, cx);
    }
}
