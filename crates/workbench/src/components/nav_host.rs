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

use database::model::{ObjectRef, PropertyRequest};
use database::nav_host::{ConnectionProbe, NavFilters, NavHost, ObjectMaskRule, SavedViewMeta};
use workbench_shell::model::{ConnectionItem, GroupFormSeed, QueryRequest, RightPanel};

use crate::panels::Shared;
use crate::services::data_source_service::DataSourceService;
use crate::services::{nav_runtime, workspace_loader};
use engine::persistence::connection_store;

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

    fn notify(&self, kind: database::nav_host::HostNotify, title: String, message: Option<String>, cx: &mut App) {
        use gpui_kit::component::notification::NotificationType;
        let kind = match kind {
            database::nav_host::HostNotify::Info => NotificationType::Info,
            database::nav_host::HostNotify::Success => NotificationType::Success,
            database::nav_host::HostNotify::Warning => NotificationType::Warning,
            database::nav_host::HostNotify::Error => NotificationType::Error,
        };
        crate::notification::notify(kind, title, message, cx);
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

    fn show_chips(&self, cx: &App) -> bool {
        settings::SettingsService::show_chips(cx)
    }

    fn set_show_chips(&self, value: bool, cx: &mut App) {
        settings::SettingsService::set_show_chips(value, cx);
    }

    fn show_search(&self, cx: &App) -> bool {
        settings::SettingsService::show_search(cx)
    }

    fn set_show_search(&self, value: bool, cx: &mut App) {
        settings::SettingsService::set_show_search(value, cx);
    }

    fn show_inline_stats(&self, cx: &App) -> bool {
        settings::SettingsService::show_inline_stats(cx)
    }

    fn set_show_inline_stats(&self, value: bool, cx: &mut App) {
        settings::SettingsService::set_show_inline_stats(value, cx);
    }

    fn show_inline_comment(&self, cx: &App) -> bool {
        settings::SettingsService::show_inline_comment(cx)
    }

    fn set_show_inline_comment(&self, value: bool, cx: &mut App) {
        settings::SettingsService::set_show_inline_comment(value, cx);
    }

    fn show_inline_size(&self, cx: &App) -> bool {
        settings::SettingsService::show_inline_size(cx)
    }

    fn set_show_inline_size(&self, value: bool, cx: &mut App) {
        settings::SettingsService::set_show_inline_size(value, cx);
    }

    fn source_short_code(&self, cx: &App) -> bool {
        settings::SettingsService::source_short_code(cx)
    }

    fn set_source_short_code(&self, value: bool, cx: &mut App) {
        settings::SettingsService::set_source_short_code(value, cx);
    }

    fn show_name_type_short(&self, cx: &App) -> bool {
        settings::SettingsService::show_name_type_short(cx)
    }

    fn type_short(&self, type_id: &str, cx: &App) -> String {
        settings::SettingsService::type_short(type_id, cx)
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

    // ==================== 已保存视图 ====================

    fn saved_views(&self, cx: &App) -> Vec<SavedViewMeta> {
        settings::SettingsService::saved_views(cx)
            .into_iter()
            .map(|v| SavedViewMeta {
                summary: crate::components::saved_view_dialog::view_summary(&v.filters),
                name: v.name,
                filters: NavFilters {
                    source: v.filters.source,
                    db_type: v.filters.db_type,
                    driver: v.filters.driver,
                    tag: v.filters.tag,
                },
            })
            .collect()
    }

    fn save_saved_view(
        &self,
        name: String,
        filters: NavFilters,
        cx: &mut App,
    ) -> Result<(), String> {
        settings::SettingsService::save_saved_view(
            name,
            settings::model::NavigatorFilters {
                source: filters.source,
                db_type: filters.db_type,
                driver: filters.driver,
                tag: filters.tag,
            },
            cx,
        )
    }

    fn delete_saved_view(&self, name: &str, cx: &mut App) -> Result<(), String> {
        settings::SettingsService::delete_saved_view(name, cx)
    }

    fn open_saved_view_dialog(&self, filters: NavFilters, window: &mut Window, cx: &mut App) {
        crate::components::saved_view_dialog::open_save_view_dialog(window, cx, filters);
    }

    fn open_manage_views_dialog(&self, window: &mut Window, cx: &mut App) {
        crate::components::saved_view_dialog::open_manage_views_dialog(window, cx, &self.shared);
    }

    // ==================== 可见库白名单（S3-①） ====================

    fn visible_databases(
        &self,
        conn_id: &str,
        cx: &App,
    ) -> Option<std::collections::BTreeSet<String>> {
        settings::SettingsService::visible_databases(conn_id, cx).map(|v| v.into_iter().collect())
    }

    fn set_visible_databases(
        &self,
        conn_id: &str,
        visible: Option<Vec<String>>,
        cx: &mut App,
    ) {
        settings::SettingsService::set_visible_databases(conn_id, visible, cx);
    }

    fn row_estimate(&self, cx: &App) -> bool {
        settings::SettingsService::row_estimate(cx)
    }

    fn set_row_estimate(&self, value: bool, cx: &mut App) {
        settings::SettingsService::set_row_estimate(value, cx);
    }

    // ==================== P1 连接级导航偏好 ====================

    fn show_system_objects(&self, conn_id: &str, _cx: &App) -> bool {
        // 与对话框编辑回读同源：走 `load_entry`（项目侧连接带项目根），
        // 读连接档案 advanced_options.navigator；缺失 / 解析失败回退默认（隐藏系统对象）。
        let root = self
            .shared
            .project_root()
            .map(|p| p.to_string_lossy().to_string());
        crate::services::nav_runtime::load_entry(conn_id, root.as_deref())
            .map(|ds| ds.navigator_settings().show_system_objects)
            .unwrap_or(false)
    }

    // ==================== B-1 对象名 mask ====================

    fn object_mask(&self, conn_id: &str, cx: &App) -> ObjectMaskRule {
        let r = settings::SettingsService::object_mask(conn_id, cx);
        ObjectMaskRule {
            include: r.include,
            exclude: r.exclude,
            use_regex: r.use_regex,
        }
    }

    fn set_object_mask(&self, conn_id: &str, rule: Option<ObjectMaskRule>, cx: &mut App) {
        let mapped = rule.map(|r| settings::model::ObjectMaskRule {
            include: r.include,
            exclude: r.exclude,
            use_regex: r.use_regex,
        });
        settings::SettingsService::set_object_mask(conn_id, mapped, cx);
    }

    fn global_mask(&self, cx: &App) -> ObjectMaskRule {
        let r = settings::SettingsService::global_mask(cx);
        ObjectMaskRule {
            include: r.include,
            exclude: r.exclude,
            use_regex: r.use_regex,
        }
    }

    fn set_global_mask(&self, rule: ObjectMaskRule, cx: &mut App) {
        settings::SettingsService::set_global_mask(
            settings::model::ObjectMaskRule {
                include: rule.include,
                exclude: rule.exclude,
                use_regex: rule.use_regex,
            },
            cx,
        );
    }

    fn open_object_mask_dialog(&self, conn_id: Option<&str>, window: &mut Window, cx: &mut App) {
        let conn_name = conn_id.map(|id| {
            self.shared
                .connections
                .borrow()
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| id.to_string())
        });
        crate::components::object_mask_dialog::open_object_mask_dialog(
            conn_id,
            conn_name.as_deref(),
            window,
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

    fn recent_connection_ids(&self) -> Vec<String> {
        // 最近连接记录按「最后使用」维护（`add_connection` 提到列表头 + touch），
        // 遍历顺序即新→旧；`None` conn_id（旧数据）跳过，未保存过的连接不在列。
        connection_store::get_recent_connections()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|r| r.conn_id)
            .collect()
    }

    fn note_connection_used(&self, conn_id: &str) {
        connection_store::touch_recent_connection(conn_id);
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
        // 连接清册换了：Mock 面板的「导入结构」候选来源随之失效（不刷的话菜单里
        // 会列着刚删掉的连接 / 缺刚建的连接）。只刷这一份，不碰要开分析库的既有表清单。
        self.shared.refresh_mock_connections(cx);
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
        let message =
            Self::block_on(service.delete(conn_id, root.as_deref())).map(|r| r.message)?;
        // 删除记录即回收 per-conn 信号量条目；随后调用方还会走 disconnect，双调幂等无害。
        database::nav_jobs::drop_conn_sem(conn_id);
        Ok(message)
    }

    // ==================== 宿主命令 ====================

    fn show_properties(&self, request: PropertyRequest, window: &mut Window, cx: &mut App) {
        self.shared.show_properties(request, window, cx);
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

    fn open_group_manager(&self, window: &mut Window, cx: &mut App) {
        crate::components::group_manager_dialog::open_group_manager_dialog(window, cx, &self.shared);
    }

    fn open_tag_manager(&self, window: &mut Window, cx: &mut App) {
        crate::components::tag_manager_dialog::open_tag_manager_dialog(window, cx, &self.shared);
    }

    fn open_tag_assign_dialog(&self, conn_id: &str, window: &mut Window, cx: &mut App) {
        crate::components::tag_assign_dialog::open_tag_assign_dialog(window, cx, &self.shared, conn_id);
    }

    fn list_tags_for(&self, conn_id: &str) -> Vec<String> {
        let root = self.shared.project_root();
        database::nav_store::list_tags(conn_id, root.as_deref())
    }

    fn assign_tags(&self, conn_id: &str, tags: &[String], cx: &mut App) {
        let root = self.shared.project_root();
        if let Err(e) = database::nav_store::set_tags(conn_id, root.as_deref(), tags) {
            self.notice(format!("保存标签失败：{e}"), cx);
        }
    }

    fn all_tag_names(&self) -> Vec<String> {
        let root = self.shared.project_root();
        database::nav_store::list_all_tags(root.as_deref())
            .into_values()
            .flatten()
            .map(|t| t.name)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn open_visible_databases_dialog(&self, conn_id: &str, window: &mut Window, cx: &mut App) {
        // 弹框候选清单按「当前项目根」定位元数据缓存（None = 全局连接缓存）。
        let root = self.shared.project_root().map(|p| p.to_string_lossy().to_string());
        let nav_view = self
            .shared
            .nav_panel
            .borrow()
            .clone()
            .and_then(|w| w.upgrade())
            .map(|e| e.downgrade());
        let conn_name = self
            .shared
            .connections
            .borrow()
            .iter()
            .find(|c| c.id == conn_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| conn_id.to_string());
        crate::components::visible_databases_dialog::open_visible_databases_dialog(
            conn_id,
            &conn_name,
            root.as_deref(),
            nav_view,
            window,
            cx,
            &self.shared,
        );
    }

    fn open_right_panel(&self, panel: RightPanel, cx: &mut App) {
        self.shared.open_right_panel(panel, cx);
    }

    fn open_mock_panel(&self, source: Option<ObjectRef>, cx: &mut App) {
        // `engine::ObjectRef` → `mock::mock_view::SchemaRequest`：
        // 两侧不想互相依赖，转换落在宿主。
        let source = source.map(|t| mock::mock_view::SchemaRequest {
            conn_id: t.conn_id,
            catalog: t.catalog,
            schema: t.schema,
            table: t.name,
        });
        self.shared.open_mock_panel(source, cx);
    }

    fn open_insight_table(&self, source: ObjectRef, cx: &mut App) {
        // 采集 SQL 在宿主拼：只有这里同时知道「连接是什么驱动」与「洞察要什么形状」（D58）。
        // 引号与拼装口径在 `Shared::insight_sample_sql`（与分析存档的「查看统计」同一处）。
        let qualified_sql = self.shared.insight_sample_sql(
            &source.conn_id,
            &[&source.catalog, &source.schema, &source.name],
        );

        // 展示文案用引用的点分限定名（空段略去）——与 `insight_sample_sql` 同一口径。
        let label = source.dotted();
        let sample = insight::SampleSource::new(source.conn_id.clone(), qualified_sql, label);
        self.shared
            .open_insight_source_table(sample, source.name, cx);
    }

    fn open_insight_schema(&self, schema: ObjectRef, cx: &mut App) {
        self.shared.open_insight_schema(schema, cx);
    }
}
