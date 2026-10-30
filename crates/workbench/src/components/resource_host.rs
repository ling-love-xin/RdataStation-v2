//! 资产库面板的宿主桥（M6）：`ResourceHostBackend` 的 workbench 实现。
//!
//! P2-b（2026-10-29）下沉后，`ResourcesHost` 的面板回调实现与对话框装配逻辑已随
//! `analytics_resource::resource_host` 下沉（只依赖 [`ResourceHostBackend`] trait）；
//! 本文件只剩「把 `workbench::panels::Shared` 的能力转发给该 trait」的薄层 +
//! `build_host` 装配点（与 `mock_host` 同构：宿主实现留在宿主 crate，crate 持有视图与状态）。
//!
//! 依赖方向：workbench → analytics_resource（trait 定义在消费方，宿主实现注入）。

use std::path::PathBuf;
use std::rc::Rc;

use analytics_resource::model::KeepVersions;
use analytics_resource::resource_host::ResourceHostBackend;
use analytics_resource::resource_view::{GroupOption, ResourcesHost};
use gpui_kit::App;
use insight::SampleSource;
use tokio::runtime::Runtime;

use crate::panels::Shared;

/// 把 `Shared` 的能力转发给 `analytics_resource::resource_host::ResourceHostBackend`
/// （只记录 / 转发，不复制任何业务逻辑——业务逻辑在 analytics_resource 侧）。
struct WorkbenchResourceBackend {
    shared: Shared,
}

impl ResourceHostBackend for WorkbenchResourceBackend {
    fn notice(&self, message: String, cx: &mut App) {
        // 治理（2026-10-30）：资产库状态栏回执统一走 say（写 notice + notify_host + info 日志）。
        self.shared.say(message, cx);
    }

    fn project_root(&self) -> Option<PathBuf> {
        self.shared.project_root()
    }

    fn read_only(&self) -> bool {
        self.shared.project_ui.borrow().read_only
    }

    fn scratchpad_store(&self) -> Result<(scratchpad::ScratchpadStore, Runtime), String> {
        self.shared.scratchpad_store()
    }

    fn refresh_resources(&self, cx: &mut App) {
        self.shared.refresh_resources(cx);
    }

    fn request_open_in_editor_read_only(&self, path: PathBuf, cx: &mut App) {
        self.shared.request_open_in_editor_read_only(path);
        self.shared.notify_host(cx);
    }

    fn insight_sample_sql(&self, conn_id: &str, parts: &[&str]) -> String {
        self.shared.insight_sample_sql(conn_id, parts)
    }

    fn open_insight_source_table(&self, source: SampleSource, name: String, cx: &mut App) {
        self.shared.open_insight_source_table(source, name, cx);
    }

    fn set_reference_stale(&self, resource_id: &str, stale: bool) {
        let mut slot = self.shared.reference_stale.borrow_mut();
        if stale {
            slot.insert(resource_id.to_string());
        } else {
            slot.remove(resource_id);
        }
    }

    fn is_reference_stale(&self, resource_id: &str) -> bool {
        self.shared.reference_stale.borrow().contains(resource_id)
    }

    fn snapshot_groups(&self, cx: &App) -> Vec<GroupOption> {
        self.shared
            .resources_panel
            .borrow()
            .as_ref()
            .and_then(|panel| panel.upgrade())
            .map(|panel| panel.read(cx).snapshot().groups.clone())
            .unwrap_or_default()
    }

    fn group_name(&self, folder_id: &str, cx: &App) -> Option<String> {
        let panel = self.shared.resources_panel.borrow().as_ref()?.upgrade()?;
        panel
            .read(cx)
            .snapshot()
            .groups
            .iter()
            .find(|group| group.id == folder_id)
            .map(|group| group.name.clone())
    }

    fn row_name(&self, resource_id: &str, cx: &App) -> Option<String> {
        let panel = self.shared.resources_panel.borrow().as_ref()?.upgrade()?;
        panel
            .read(cx)
            .view_rows()
            .iter()
            .find(|row| row.id == resource_id)
            .map(|row| row.name.clone())
    }

    fn row_alias(&self, resource_id: &str, cx: &App) -> Option<Option<String>> {
        let panel = self.shared.resources_panel.borrow().as_ref()?.upgrade()?;
        panel
            .read(cx)
            .snapshot()
            .details
            .get(resource_id)
            .map(|detail| detail.alias.clone())
    }

    fn default_group(&self, root: &str, cx: &App) -> Option<String> {
        settings::SettingsService::default_group(root, cx)
    }

    fn keep_versions(&self, cx: &App) -> KeepVersions {
        KeepVersions::from_setting(settings::SettingsService::keep_versions(cx))
    }

    fn set_default_resource_sort(&self, key: &str, cx: &mut App) {
        settings::SettingsService::set_default_resource_sort(key, cx);
    }

    fn set_collapsed_groups(&self, root: &str, collapsed_keys: &[String], cx: &mut App) {
        settings::SettingsService::set_collapsed_groups(root, collapsed_keys, cx);
    }

    fn set_default_group(&self, root: &str, folder_id: Option<&str>, cx: &mut App) {
        settings::SettingsService::set_default_group(root, folder_id, cx);
    }

    fn pick_path(&self) -> Option<PathBuf> {
        crate::services::editor_files::pick_open_path()
    }
}

/// 组装资产库面板宿主（面板实体创建时调用一次）。
pub fn build_host(shared: &Shared) -> Rc<dyn ResourcesHost> {
    let backend: Rc<dyn ResourceHostBackend> = Rc::new(WorkbenchResourceBackend {
        shared: shared.clone(),
    });
    analytics_resource::resource_host::new_host(backend)
}
