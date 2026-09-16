//! 资产库面板的宿主桥（M6）。
//!
//! `rds-analytics-resource` 自带面板视图（`resource_view::ResourcesPanel`）但不依赖 workbench：
//! 工作台在这里把「重绘 / 只读判定 / 动作去向」注入为 [`ResourcesHost`]——与 `mock_host`
//! 同一形态（crate 持有视图与状态，宿主只装配）。
//!
//! ## 动作去向（本批起：归档与取回是真实现）
//!
//! - **归档**：系统文件选择 → 归档确认对话框 → 入队后台任务（`services::resource_jobs`）。
//!   目标相对路径与重名避让由 `PayloadStore::{rel_path_taken, free_rel_path}` 定
//!   （搬本体与命名规则都是本模块的知识，宿主不自己拼 `resources/`）。
//! - **取回**：取回对话框 → 入队后台任务 → 回执与"顺手打开"由侧栏轮询印做。
//! - 打开（只读）：仍需编辑器侧"分析资源锁定"只读来源（P1.6），本批仍给明确回执；
//! - 移入回收站：一律走项目级 `ProjectTrash`，而上提尚未落地（P0.8）——**不做**先软删
//!   再等回收站那条（会变成两套回收站，违反模块硬约束 5）；
//! - 索引修复对话框：Phase 3（异常计数本批已在状态行可见）。
//!
//! 回执而不是空操作：面板上的按钮是既有入口，点了没反应比"明确说还没接入"更难排查。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{App, Window};

use analytics_resource::detail_view::ArchiveDetail;
use analytics_resource::dialogs::archive::{
    ArchiveConflict, ArchiveDialogSeed, open_archive_dialog,
};
use analytics_resource::dialogs::checkout::{
    CheckoutDialogSeed, open_checkout_dialog, suggest_work_copy_name,
};
use analytics_resource::model::{ArchiveBinding, ArchiveKind, ArchiveRequest, CheckoutRequest};
use analytics_resource::payload::{PayloadStore, RESOURCES_DIR_NAME};
use analytics_resource::resource_view::ResourcesHost;

use crate::panels::Shared;
use crate::services::resource_jobs;

/// 宿主桥：动作转发到 `Shared` 的状态栏提示（无自有状态）。
struct WorkbenchResourceHost {
    shared: Shared,
}

impl WorkbenchResourceHost {
    /// 给一次明确回执（写入 `Shared::notice` 并请求宿主重绘）。
    fn pending(&self, action: &str, detail: &str, cx: &mut App) {
        *self.shared.notice.borrow_mut() = Some(format!("资产库：{action}{detail}"));
        self.shared.notify_host(cx);
    }

    /// 状态栏提示（动作结果与失败都走它）。
    fn notice(&self, message: impl Into<String>, cx: &mut App) {
        *self.shared.notice.borrow_mut() = Some(message.into());
        self.shared.notify_host(cx);
    }

    /// 项目根（未打开项目时给回执并返回 `None`）。
    fn require_project(&self, action: &str, cx: &mut App) -> Option<PathBuf> {
        match self.shared.project_root() {
            Some(root) => Some(root),
            None => {
                self.pending(action, "：还没有打开项目", cx);
                None
            }
        }
    }

    fn read_only(&self) -> bool {
        self.shared.project_ui.borrow().read_only
    }
}

impl ResourcesHost for WorkbenchResourceHost {
    fn request_archive(&self, window: &mut Window, cx: &mut App) {
        let Some(root) = self.require_project("无法归档", cx) else {
            return;
        };
        if self.read_only() {
            self.notice("资产库：项目为只读模式，不能归档", cx);
            return;
        }
        // 来源：系统文件选择（事件路径上的模态框）。**取消不是错误**：不提示、不入队。
        let Some(source) = crate::services::editor_files::pick_open_path() else {
            return;
        };
        let Some(file_name) = source
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
        else {
            self.notice("资产库：归档失败（文件名无法解析）", cx);
            return;
        };

        // 目标相对路径：`rel_path` **相对 `resources/`**（`PayloadStore::resolve` 的口径），
        // 这里只给文件名——本地文件没有草稿相对路径可保留（草稿箱发起的入口见开发方案 P1.4）。
        let payload = PayloadStore::new(root.clone());
        let resolved_rel = payload.free_rel_path(&file_name);
        let conflict = (resolved_rel != file_name).then(|| ArchiveConflict {
            taken_rel_path: format!("{RESOURCES_DIR_NAME}/{file_name}"),
            resolved_rel_path: format!("{RESOURCES_DIR_NAME}/{resolved_rel}"),
        });
        let default_name = std::path::Path::new(&file_name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(&file_name)
            .to_string();
        let seed = ArchiveDialogSeed {
            source_label: format!("本地文件：{}", source.display()),
            rel_path: format!("{RESOURCES_DIR_NAME}/{resolved_rel}"),
            name: default_name,
            // 本地文件没有"来源草稿的连接"可带出：**不猜**当前活动连接（那不一定是它的来路）。
            // 草稿箱发起的归档会带上 `file_meta.last_connection_id`（P1.4 的另一半）。
            source_connection: None,
            conflict,
        };

        let shared = self.shared.clone();
        let read_only = self.read_only();
        open_archive_dialog(window, cx, seed, move |result, cx| {
            let request = ArchiveRequest {
                source_path: source.clone(),
                rel_path: resolved_rel.clone(),
                name: result.name,
                // 别名留空：原型 §4.1 没有这一格，改别名走 Phase 2 的重命名入口。
                alias: None,
                kind: ArchiveKind::File,
                binding: ArchiveBinding {
                    promoted_from: None,
                    source_connection_id: None,
                    source_table: None,
                },
                tags: result.tags,
                group_id: None,
                keep_versions: result.keep_versions,
                existing_resource_id: None,
            };
            resource_jobs::enqueue_archive(root.clone(), read_only, request);
            shared.refresh_resources(cx);
            *shared.notice.borrow_mut() = Some("资产库：正在归档…".to_string());
            shared.notify_host(cx);
        });
    }

    fn request_open(&self, _resource_id: &str, _window: &mut Window, cx: &mut App) {
        self.pending("只读打开尚未接入", "（需编辑器只读来源，P1.6）", cx);
    }

    fn request_checkout(&self, detail: &ArchiveDetail, window: &mut Window, cx: &mut App) {
        let Some(root) = self.require_project("无法取回", cx) else {
            return;
        };
        if self.read_only() {
            self.notice("资产库：项目为只读模式，不能取回", cx);
            return;
        }
        // 目标目录 = 草稿箱模块目录（取回产出的是一份**草稿**；目录名取自 crate 常量，
        // 不在宿主里再写一次字面量）。
        let target_dir = root.join(scratchpad::MODULE_DIR_NAME);
        let extension = detail
            .payload_rel_path
            .as_deref()
            .and_then(|rel| std::path::Path::new(rel).extension())
            .and_then(|ext| ext.to_str());
        let seed = CheckoutDialogSeed {
            resource_name: detail.name.clone(),
            file_name: suggest_work_copy_name(&detail.name, extension),
            target_dir_label: target_dir.to_string_lossy().to_string(),
            version: detail.version,
        };

        let shared = self.shared.clone();
        let resource_id = detail.id.clone();
        let read_only = self.read_only();
        open_checkout_dialog(window, cx, seed, move |result, cx| {
            let request = CheckoutRequest {
                resource_id: resource_id.clone(),
                dest_path: target_dir.join(&result.file_name),
            };
            resource_jobs::enqueue_checkout(root.clone(), read_only, request, result.open_after);
            shared.refresh_resources(cx);
            *shared.notice.borrow_mut() = Some("资产库：正在取回…".to_string());
            shared.notify_host(cx);
        });
    }

    fn request_delete(&self, _resource_id: &str, _window: &mut Window, cx: &mut App) {
        self.pending(
            "移入回收站尚未接入",
            "（等项目级回收站上提，P0.8；不做两套回收站）",
            cx,
        );
    }

    fn request_index_repair(&self, _window: &mut Window, cx: &mut App) {
        self.pending(
            "索引修复对话框尚未接入",
            "（Phase 3；异常计数已在状态行显示）",
            cx,
        );
    }
}

/// 组装资产库面板宿主（面板实体创建时调用一次）。
pub fn build_host(shared: &Shared) -> Rc<dyn ResourcesHost> {
    Rc::new(WorkbenchResourceHost {
        shared: shared.clone(),
    })
}
