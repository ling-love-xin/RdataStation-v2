//! 资产库面板的宿主桥（M6）。
//!
//! `rds-analytics-resource` 自带面板视图（`resource_view::ResourcesPanel`）但不依赖 workbench：
//! 工作台在这里把「重绘 / 只读判定 / 动作去向」注入为 [`ResourcesHost`]——与 `mock_host`
//! 同一形态（crate 持有视图与状态，宿主只装配）。
//!
//! ## 动作去向（本批起：归档 / 取回 / 打开都是真实现）
//!
//! - **归档**：系统文件选择 → 归档确认对话框 → 入队后台任务（`services::resource_jobs`）。
//!   目标相对路径与重名避让由 `PayloadStore::{rel_path_taken, free_rel_path}` 定
//!   （搬本体与命名规则都是本模块的知识，宿主不自己拼 `resources/`）。
//! - **打开（只读）**：本体以**编辑器只读**打开（`Shared::request_open_in_editor_read_only` →
//!   编辑器 `persist::open_file_read_only`）；路径由 `PayloadStore::resolve` 解析（守卫在那一层）。
//! - **取回**：取回对话框 → 入队后台任务 → 回执与"顺手打开"由侧栏轮询印做。
//! - **撤销归档**：撤销栏的凭据原样交给工作线程（本体移回原位 + 删登记行）。
//! - 面板头「⋯」四项：**打开资源目录**（系统文件管理器开 `resources/`）与 **刷新**（同一条取数路径）
//!   是真实现；**回收站…** 与 **重建索引…** 沿用下两条的明确回执（入口先摆出，点了要说清为什么没动）。
//! - 移入回收站：一律走项目级 `ProjectTrash`，而上提尚未落地（P0.8）——**不做**先软删
//!   再等回收站那条（会变成两套回收站，违反模块硬约束 5）；
//! - 索引修复对话框：Phase 3（异常计数已在状态行可见）。
//!
//! 回执而不是空操作：面板上的按钮是既有入口，点了没反应比"明确说还没接入"更难排查。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::{App, Window};

use analytics_resource::detail_view::ArchiveDetail;
use analytics_resource::dialogs::archive::{
    ArchiveConflict, ArchiveDialogSeed, open_archive_dialog,
};
use analytics_resource::dialogs::checkout::{
    CheckoutDialogSeed, open_checkout_dialog, suggest_work_copy_name,
};
use analytics_resource::dialogs::pick::{DraftCandidate, PickDialogSeed, open_draft_pick_dialog};
use analytics_resource::model::{
    ArchiveBinding, ArchiveKind, ArchiveRequest, ArchiveUndo, CheckoutRequest,
};
use analytics_resource::payload::{PayloadStore, RESOURCES_DIR_NAME};
use analytics_resource::resource_view::ResourcesHost;

use crate::panels::Shared;
use crate::services::resource_jobs;

/// 草稿扫描深度：与草稿箱自己的遍历上限同档（再深就不是"随手归档"的场景了）。
const DRAFT_SCAN_DEPTH: u32 = 4;

/// 状态栏提示（自由函数版：对话框回调里没有 `self`）。
fn say(shared: &Shared, message: impl Into<String>, cx: &mut App) {
    *shared.notice.borrow_mut() = Some(message.into());
    shared.notify_host(cx);
}

/// 归档种子（原型 §4.1）：来源标签 / 目标位置（含重名避让）/ 默认显示名 / 来源连接 / 冲突预探。
///
/// 返回种子与**已避让的目标相对路径**：提交时要用同一个值，两处各算一次就会不一致。
fn archive_seed(
    root: &Path,
    source_label: String,
    rel_path: &str,
    display_name: &str,
    source_connection: Option<String>,
) -> (ArchiveDialogSeed, String) {
    let resolved = PayloadStore::new(root.to_path_buf()).free_rel_path(rel_path);
    let conflict = (resolved != rel_path).then(|| ArchiveConflict {
        taken_rel_path: format!("{RESOURCES_DIR_NAME}/{rel_path}"),
        resolved_rel_path: format!("{RESOURCES_DIR_NAME}/{resolved}"),
    });
    let seed = ArchiveDialogSeed {
        source_label,
        rel_path: format!("{RESOURCES_DIR_NAME}/{resolved}"),
        name: display_name.to_string(),
        source_connection,
        conflict,
    };
    (seed, resolved)
}

/// 提交一次归档（入队 + 立刻刷新 + 回执）：两条归档入口共用，
/// 避免"一处记得刷新、另一处忘了"。
fn submit_archive(
    root: &Path,
    read_only: bool,
    request: ArchiveRequest,
    shared: &Shared,
    cx: &mut App,
) {
    resource_jobs::enqueue_archive(root.to_path_buf(), read_only, request);
    shared.refresh_resources(cx);
    say(shared, "资产库：正在归档…", cx);
}

/// 把草稿树压平成可归档的候选（只收文件：目录只是组织方式）。
fn flatten_drafts(
    entries: &[scratchpad::ScratchpadEntry],
    file_meta: &std::collections::HashMap<String, scratchpad::FileMeta>,
    store: &scratchpad::ScratchpadStore,
    out: &mut Vec<DraftCandidate>,
) {
    for entry in entries {
        match entry.kind {
            scratchpad::ScratchpadEntryKind::File => {
                // `relative_path_of` 同时把内部/隐藏路径挡在门外（与草稿箱自身的 API 同一守卫）。
                let Some(rel) = store.relative_path_of(&entry.path) else {
                    continue;
                };
                let connection_id = file_meta
                    .get(&rel)
                    .and_then(|meta| meta.preferred_connection())
                    .map(str::to_string);
                out.push(DraftCandidate {
                    abs_path: entry.path.clone(),
                    rel_path: rel,
                    display_name: entry
                        .name
                        .rsplit_once('.')
                        .map(|(stem, _)| stem.to_string())
                        .unwrap_or_else(|| entry.name.clone()),
                    connection_id,
                });
            }
            scratchpad::ScratchpadEntryKind::Folder => {
                if let Some(children) = entry.children.as_ref() {
                    flatten_drafts(children, file_meta, store, out);
                }
            }
        }
    }
}

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

    /// 草稿箱里的可归档文件（**宿主侧读**：M6 不认识草稿箱的内部结构）。
    ///
    /// 目录列表与配置读取是**元数据级**操作（单次 readdir + 一个小 JSON，K1c 口径），
    /// 留在事件路径；来源连接取草稿 `file_meta` 的首选连接（显式绑定优先，其次最近执行过的那条）。
    fn draft_candidates(&self, cx: &mut App) -> Option<Vec<DraftCandidate>> {
        let (store, runtime) = match self.shared.scratchpad_store() {
            Ok(pair) => pair,
            Err(reason) => {
                self.notice(format!("资产库：读不到草稿箱（{reason}）"), cx);
                return None;
            }
        };
        let entries = match runtime.block_on(store.list_local_entries(DRAFT_SCAN_DEPTH)) {
            Ok(entries) => entries,
            Err(error) => {
                self.notice(format!("资产库：读草稿箱失败（{error}）"), cx);
                return None;
            }
        };
        // 配置读不到不算错：来源连接只是"能带就带"，不该阻断归档。
        let file_meta = runtime
            .block_on(store.load_config())
            .map(|config| config.file_meta)
            .unwrap_or_default();
        let mut out = Vec::new();
        flatten_drafts(&entries, &file_meta, &store, &mut out);
        out.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        Some(out)
    }

    /// 本体绝对路径（越界 / 点前缀守卫由 `PayloadStore::resolve` 把关）。
    ///
    /// 拿不到就**给回执**：旧行可能没登记路径，或项目已关闭——两种都要说清，不能静默。
    fn payload_path(&self, detail: &ArchiveDetail, cx: &mut App) -> Option<PathBuf> {
        let Some(root) = self.shared.project_root() else {
            self.pending("无法定位本体", "：还没有打开项目", cx);
            return None;
        };
        let Some(rel) = detail.payload_rel_path.as_deref() else {
            self.notice("资产库：这条存档没有登记本体路径（旧行）", cx);
            return None;
        };
        match PayloadStore::new(root).resolve(rel) {
            Ok(path) => Some(path),
            Err(error) => {
                self.notice(format!("资产库：无法定位本体（{error}）"), cx);
                None
            }
        }
    }
}

impl ResourcesHost for WorkbenchResourceHost {
    fn request_archive_from_drafts(&self, window: &mut Window, cx: &mut App) {
        let Some(root) = self.require_project("无法归档", cx) else {
            return;
        };
        if self.read_only() {
            self.notice("资产库：项目为只读模式，不能归档", cx);
            return;
        }
        let Some(candidates) = self.draft_candidates(cx) else {
            return;
        };
        if candidates.is_empty() {
            self.notice("资产库：草稿箱里还没有可归档的文件", cx);
            return;
        }

        let shared = self.shared.clone();
        let read_only = self.read_only();
        open_draft_pick_dialog(
            window,
            cx,
            PickDialogSeed { candidates },
            move |picked, window, cx| match picked.as_slice() {
                // 选一个：展开完整的归档确认（与「从本地文件归档…」同一条路）——
                // 能改显示名、能看到"目标被占则改名到哪"。
                [draft] => {
                    let (seed, resolved_rel) = archive_seed(
                        &root,
                        format!("草稿箱：{}", draft.rel_path),
                        &draft.rel_path,
                        &draft.display_name,
                        draft.connection_id.clone(),
                    );
                    let shared = shared.clone();
                    let draft = draft.clone();
                    // 本回调签名是 `Fn`（对话框可能被调用多次的历史原因）：内层 `move` 闭包
                    // 只能拿副本，不能把外层捕获的 `root` 移走。
                    let root = root.clone();
                    open_archive_dialog(window, cx, seed, move |result, cx| {
                        let request = ArchiveRequest {
                            source_path: draft.abs_path.clone(),
                            rel_path: resolved_rel.clone(),
                            name: result.name,
                            alias: None,
                            kind: ArchiveKind::File,
                            binding: ArchiveBinding {
                                // 归档凭证的"出处"：草稿相对路径（带模块前缀，与文档口径一致）
                                promoted_from: Some(format!(
                                    "{}/{}",
                                    scratchpad::MODULE_DIR_NAME,
                                    draft.rel_path
                                )),
                                source_connection_id: draft.connection_id.clone(),
                                source_table: None,
                            },
                            tags: result.tags,
                            group_id: None,
                            keep_versions: result.keep_versions,
                            existing_resource_id: None,
                        };
                        submit_archive(&root, read_only, request, &shared, cx);
                    });
                }
                // 选多个：默认名 + 各自来源，逐个入队（列表里没有位置让用户逐个改名）。
                many => {
                    let payload = PayloadStore::new(root.clone());
                    let mut renamed = 0usize;
                    for draft in many {
                        let resolved_rel = payload.free_rel_path(&draft.rel_path);
                        if resolved_rel != draft.rel_path {
                            renamed += 1;
                        }
                        let request = ArchiveRequest {
                            source_path: draft.abs_path.clone(),
                            rel_path: resolved_rel,
                            name: draft.display_name.clone(),
                            alias: None,
                            kind: ArchiveKind::File,
                            binding: ArchiveBinding {
                                promoted_from: Some(format!(
                                    "{}/{}",
                                    scratchpad::MODULE_DIR_NAME,
                                    draft.rel_path
                                )),
                                source_connection_id: draft.connection_id.clone(),
                                source_table: None,
                            },
                            tags: Vec::new(),
                            group_id: None,
                            keep_versions: None,
                            existing_resource_id: None,
                        };
                        resource_jobs::enqueue_archive(root.clone(), read_only, request);
                    }
                    let mut message = format!("资产库：已提交 {} 个归档…", many.len());
                    if renamed > 0 {
                        message.push_str(&format!("（{renamed} 个目标被占，已自动改名）"));
                    }
                    shared.refresh_resources(cx);
                    say(&shared, message, cx);
                }
            },
        );
    }

    fn request_archive_from_file(&self, window: &mut Window, cx: &mut App) {
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
        let default_name = std::path::Path::new(&file_name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(&file_name)
            .to_string();
        // 目标相对路径：`rel_path` **相对 `resources/`**（`PayloadStore::resolve` 的口径），
        // 这里只给文件名——本地文件没有草稿相对路径可保留。
        let (seed, resolved_rel) = archive_seed(
            &root,
            format!("本地文件：{}", source.display()),
            &file_name,
            &default_name,
            // 本地文件没有"来源草稿的连接"可带出：**不猜**当前活动连接（那不一定是它的来路）。
            None,
        );

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
            submit_archive(&root, read_only, request, &shared, cx);
        });
    }

    fn request_open(&self, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {
        let Some(path) = self.payload_path(detail, cx) else {
            return;
        };
        // **编辑器只读**：改它要先去取回（本体不可写是模块硬约束 2）。
        self.shared.request_open_in_editor_read_only(path);
        self.shared.notify_host(cx);
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

    fn request_version_history(&self, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {
        // 取数在后台线程：版本行要读索引表 + 问 `.RSmeta` 下的副本清单，
        // 回来之后由侧栏轮询开窗（开窗要 `Window`，轮询任务里没有）。
        let Some(root) = self.require_project("无法打开版本历史", cx) else {
            return;
        };
        resource_jobs::enqueue_versions(root, detail.id.clone());
        self.notice("资产库：正在读取版本历史…", cx);
    }

    fn request_delete(&self, _resource_id: &str, _window: &mut Window, cx: &mut App) {
        self.pending(
            "移入回收站尚未接入",
            "（等项目级回收站上提，P0.8；不做两套回收站）",
            cx,
        );
    }

    fn request_reveal(&self, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {
        let Some(path) = self.payload_path(detail, cx) else {
            return;
        };
        // `opener::reveal` 会选中该文件（而不是只打开目录）；失败给回执而不是静默。
        if let Err(error) = opener::reveal(&path) {
            self.notice(format!("资产库：打开资源目录失败（{error}）"), cx);
        }
    }

    fn request_copy_path(&self, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {
        let Some(path) = self.payload_path(detail, cx) else {
            return;
        };
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
            path.to_string_lossy().to_string(),
        ));
        self.notice("资产库：已复制本体路径", cx);
    }

    fn request_undo_archive(&self, undo: &ArchiveUndo, _window: &mut Window, cx: &mut App) {
        let Some(root) = self.require_project("无法撤销", cx) else {
            return;
        };
        if self.read_only() {
            self.notice("资产库：项目为只读模式，不能撤销", cx);
            return;
        }
        // 凭据原样交给工作线程（面板只持有它，不认识服务层）。
        resource_jobs::enqueue_undo(root, self.read_only(), undo.clone());
        self.shared.refresh_resources(cx);
        self.notice("资产库：正在撤销…", cx);
    }

    fn request_index_repair(&self, _window: &mut Window, cx: &mut App) {
        self.pending(
            "索引修复对话框尚未接入",
            "（Phase 3；异常计数已在状态行显示）",
            cx,
        );
    }

    fn request_open_payload_dir(&self, _window: &mut Window, cx: &mut App) {
        let Some(root) = self.require_project("无法打开资源目录", cx) else {
            return;
        };
        let dir = PayloadStore::new(root).resources_dir();
        // 目录由第一次归档创建：还没有存档时它不存在，**说清这一点**而不是报一个系统错误。
        if !dir.exists() {
            self.notice(
                format!("资产库：资源目录还不存在（{}）——归档第一个存档时会创建", dir.display()),
                cx,
            );
            return;
        }
        if let Err(error) = opener::open(&dir) {
            self.notice(format!("资产库：打开资源目录失败（{error}）"), cx);
        }
    }

    fn request_open_trash(&self, _window: &mut Window, cx: &mut App) {
        self.pending(
            "资源回收站尚未接入",
            "（等项目级回收站上提，P0.8；不做两套回收站）",
            cx,
        );
    }

    fn request_refresh(&self, _window: &mut Window, cx: &mut App) {
        // 刷新不写回执：状态行的「加载中…」就是它的回执，再叠一条只是噪声。
        if self.shared.project_root().is_none() {
            self.pending("无法刷新", "：还没有打开项目", cx);
            return;
        }
        self.shared.refresh_resources(cx);
    }
}

/// 组装资产库面板宿主（面板实体创建时调用一次）。
pub fn build_host(shared: &Shared) -> Rc<dyn ResourcesHost> {
    Rc::new(WorkbenchResourceHost {
        shared: shared.clone(),
    })
}
