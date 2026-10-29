//! 编辑器宿主端口实现（P2-a）：把 `EditorHost` / `FederatedSourcePort` 接到宿主侧。
//!
//! `services/editor_*`（原 workbench 侧执行 / 连接 / 补全等端口实现）下沉到
//! `editor::ports` 后，它们不再依赖 `Shared` 的具体类型，只依赖
//! [`editor::host::EditorHost`]——本文件就是那个 trait 的 workbench 侧实现：
//! 连接快照 / 项目根 / 项目锁转发给 `Shared`，宿主命令（历史 / 洞察 / 导航定位）
//! 转发给 `Shared` 的既有 pub 方法。
//!
//! [`editor::host::FederatedSourcePort`] 是**工作线程**端口（exec 的 `QueryRunner`
//! 跨线程调用）：实现不持 `Shared`（`Rc` 不能跨线程），只调全局 `DataSourceService`。

use std::sync::Arc;

use editor::host::{EditorHost, FederatedSourcePort, FederatedSourceRecord};
use editor::shared::InsightColumnRequest;
use gpui_kit::App;
use workbench_shell::model::ConnectionItem;

use crate::panels::Shared;

/// 编辑器宿主实现：把所有问题都转发给 `Shared`（连接快照 / 项目根 / 项目锁 / 宿主命令）。
pub struct WorkbenchEditorHost {
    shared: Shared,
}

impl WorkbenchEditorHost {
    pub fn new(shared: Shared) -> Self {
        Self { shared }
    }
}

impl EditorHost for WorkbenchEditorHost {
    fn connections(&self) -> Vec<ConnectionItem> {
        self.shared.connections.borrow().clone()
    }

    fn project_root(&self) -> Option<std::path::PathBuf> {
        self.shared.project_root()
    }

    fn project_read_only(&self) -> bool {
        self.shared.project_ui.borrow().read_only
    }

    fn set_connection_connected(&self, conn_id: &str, connected: bool) {
        let mut connections = self.shared.connections.borrow_mut();
        if let Some(item) = connections.iter_mut().find(|c| c.id == conn_id) {
            item.connected = connected;
        }
    }

    fn is_connected(&self, conn_id: &str) -> bool {
        crate::services::nav_runtime::is_connected(conn_id)
    }

    fn connect_entry(&self, conn_id: &str) -> Result<(), String> {
        let root = self
            .shared
            .project_root()
            .map(|path| path.to_string_lossy().to_string());
        crate::services::nav_runtime::connect_entry(conn_id, root.as_deref())
    }

    fn open_history(&self, cx: &mut App) {
        self.shared.open_right_panel(crate::view::RightPanel::History, cx);
    }

    fn open_insight_source_column(&self, request: InsightColumnRequest, cx: &mut App) {
        let source = insight::SampleSource::new(
            request.connection.clone(),
            request.sql.clone(),
            request.label.clone(),
        );
        // 类型留空：编辑器手里没有真类型（行数据是字符串化的），取样后由洞察侧从临时表读出
        // 真类型——不编一个看起来像真的值（面板侧对空类型只显示来源）
        self.shared
            .open_insight_source_column(source, request.column.clone(), String::new(), cx);
    }

    fn request_reveal(&self, object: engine::ObjectRef) {
        self.shared.request_reveal(object);
    }
}

/// 联邦源清单端口实现：直接读全局 `DataSourceService`（不持 `Shared`，
/// 可在 `QueryRunner` 的工作线程上调用；与原 workbench 执行线程的
/// `block_on(service.list())` 同一条路）。
pub struct WorkbenchFederatedSource;

impl FederatedSourcePort for WorkbenchFederatedSource {
    fn records(&self) -> Vec<FederatedSourceRecord> {
        let Ok(service) = crate::services::data_source_service::DataSourceService::global() else {
            return Vec::new();
        };
        let Ok(rt) = tokio::runtime::Runtime::new() else {
            return Vec::new();
        };
        let Ok(items) = rt.block_on(service.list()) else {
            return Vec::new();
        };
        items
            .into_iter()
            .filter(|item| item.use_duckdb_fed)
            .filter_map(|item| {
                connection::url::build_connection_url(&item)
                    .ok()
                    .map(|url| FederatedSourceRecord {
                        conn_id: item.id.clone(),
                        name: item.name.clone(),
                        db_type: item.db_type.clone(),
                        url,
                    })
            })
            .collect()
    }
}

/// 便捷构造：`WorkbenchFederatedSource` 的 `Arc` 包装（view 装配点用）。
pub fn federated_source() -> Arc<dyn FederatedSourcePort> {
    Arc::new(WorkbenchFederatedSource)
}
