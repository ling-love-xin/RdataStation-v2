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

use analytics_resource::detail_view::VersionSelection;
use analytics_resource::dialogs::pick::DraftCandidate;
use analytics_resource::model::KeepVersions;
use analytics_resource::resource_host::ResourceHostBackend;
use analytics_resource::resource_view::{GroupOption, ResourcesHost};
use gpui_kit::{App, Entity, Render};
use insight::SampleSource;

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

    fn global_system_dir(&self) -> Option<PathBuf> {
        engine::migration::get_system_dir().ok()
    }

    fn read_only(&self) -> bool {
        self.shared.project_ui.borrow().read_only
    }

    /// 草稿箱候选：扫描、来源连接与标签预取都在宿主侧完成，只把 M6 自有的
    /// `DraftCandidate` 交给资产库（2026-11-12 分层收口：trait 不再返回上游存储类型）。
    fn draft_candidates(&self) -> Result<Vec<DraftCandidate>, String> {
        use std::collections::HashMap;

        use scratchpad::{FileMeta, ScratchpadEntry, ScratchpadEntryKind, ScratchpadStore};

        fn flatten(
            entries: &[ScratchpadEntry],
            file_meta: &HashMap<String, FileMeta>,
            store: &ScratchpadStore,
            out: &mut Vec<DraftCandidate>,
        ) {
            for entry in entries {
                match entry.kind {
                    ScratchpadEntryKind::File => {
                        // `relative_path_of` 同时把内部 / 隐藏路径挡在门外。
                        let Some(rel) = store.relative_path_of(&entry.path) else {
                            continue;
                        };
                        let connection_id = file_meta
                            .get(&rel)
                            .and_then(|meta| meta.preferred_connection())
                            .map(str::to_string);
                        // 标签继承是「能带就带」：读失败只给空，不阻断归档。
                        let tags = store
                            .tags_of_entry(&rel)
                            .map(|tags| tags.into_iter().map(|t| t.name).collect())
                            .unwrap_or_default();
                        out.push(DraftCandidate {
                            abs_path: entry.path.clone(),
                            rel_path: rel,
                            display_name: entry
                                .name
                                .rsplit_once('.')
                                .map(|(stem, _)| stem.to_string())
                                .unwrap_or_else(|| entry.name.clone()),
                            connection_id,
                            tags,
                        });
                    }
                    ScratchpadEntryKind::Folder => {
                        if let Some(children) = entry.children.as_ref() {
                            flatten(children, file_meta, store, out);
                        }
                    }
                }
            }
        }

        let (store, runtime) = self.shared.scratchpad_store()?;
        let entries = runtime
            .block_on(store.list_local_entries(4))
            .map_err(|e| format!("读草稿箱失败：{e}"))?;
        let file_meta = runtime
            .block_on(store.load_config())
            .map(|config| config.file_meta)
            .unwrap_or_default();
        let mut out = Vec::new();
        flatten(&entries, &file_meta, &store, &mut out);
        Ok(out)
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

    /// 来源链追下一跳要按 id 找行：直接从面板快照拷一份（不另查库）。
    fn snapshot_details(&self, cx: &App) -> Vec<analytics_resource::detail_view::ArchiveDetail> {
        let Some(panel) = self
            .shared
            .resources_panel
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
        else {
            return Vec::new();
        };
        panel
            .read(cx)
            .snapshot()
            .details
            .values()
            .cloned()
            .collect()
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

    /// 详情「来源连接」只摆名字（索引里存的是 `conn_…` 的 id）。
    ///
    /// 名字取自导航同一份内存连接表（`shared.connections`）——不查库、不开连接。
    /// 连接被删 / 换了 id 时返回 `None`，渲染侧回落成 id，不假装有名字。
    fn connection_label(&self, connection_id: &str) -> Option<String> {
        self.shared
            .connections
            .borrow()
            .iter()
            .find(|conn| conn.id == connection_id)
            .map(|conn| conn.name.clone())
    }
}

/// 组装资产库面板宿主（面板实体创建时调用一次）。
pub fn build_host(shared: &Shared) -> Rc<dyn ResourcesHost> {
    let backend: Rc<dyn ResourceHostBackend> = Rc::new(WorkbenchResourceBackend {
        shared: shared.clone(),
    });
    analytics_resource::resource_host::new_host(backend)
}

/// 详情「版本时间线」的选中态接线（侧边面板形态）。
///
/// 两件事：
/// - **状态存哪**：存 `Shared.detail_version`（窗口级 UI 状态，不进快照——否则点一下
///   节点就要重建整份快照）。按资产 id 配对，换资产自动失效。
/// - **谁来重绘**：`panel` 必须是**渲染详情的那张面板**（详情不在 `Shared` 的子树里，
///   通知 Shared 没人听）。所以这个助手要泛型到面板实体，右栏与归档 tab 共用一份。
pub fn version_selection<T: Render + 'static>(
    shared: &Shared,
    detail_id: &str,
    panel: Entity<T>,
) -> VersionSelection {
    // 只认同一个资产的选中；换了资产就当没选（渲染侧兜底当前版本）。
    let selected = shared
        .detail_version
        .borrow()
        .as_ref()
        .filter(|(id, _)| id == detail_id)
        .map(|(_, version)| *version);
    let shared = shared.clone();
    let id = detail_id.to_string();
    VersionSelection {
        selected,
        on_select: Some(Rc::new(move |version, _window, cx| {
            *shared.detail_version.borrow_mut() = Some((id.clone(), version));
            panel.update(cx, |_, cx| cx.notify());
        })),
    }
}
