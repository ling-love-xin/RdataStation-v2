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
//! - **移入回收站**：单选与多选都是真实现——一次提交整批给工作线程，本体进项目级
//!   `ProjectTrash`、登记行软删（还原能恢复别名 / 标签 / 指纹，回收站 manifest 里没有这些）。
//! - **回收站…**：取数（后台线程读条目 manifest）→ 回收站对话框（还原 / 永久删除 / 清空）。
//!   对话框只列**本模块的**条目（`origin = "resources"`），别人的只给一句说明——
//!   共用一处回收站不代表可以在对方的界面里动对方的东西。
//! - 面板头「⋯」四项：**打开资源目录**（系统文件管理器开 `resources/`）、**刷新**（同一条取数路径）
//!   与 **重建索引…**（扫描 → 索引修复对话框）、**回收站…** 都是真实现。
//! - 索引修复：扫描（后台线程，逐个本体算 sha256）→ 对话框三分组 → 行内动作
//!   （补登 / 删记录 / 接受当前内容 / 打开版本历史）；「从回收站还原」走
//!   `restore_archive_by_rel_path`（那一组行只有登记记录，手上没有回收站条目 id）。
//!
//! 回执而不是空操作：面板上的按钮是既有入口，点了没反应比"明确说还没接入"更难排查。

pub(crate) use std::path::{Path, PathBuf};
pub(crate) use std::rc::Rc;

pub(crate) use gpui_kit::component::WindowExt as _;
pub(crate) use gpui_kit::{App, AppContext as _, Window};

pub(crate) use crate::detail_view::ArchiveDetail;
pub(crate) use crate::dialogs::archive::{ArchiveConflict, ArchiveDialogSeed, open_archive_dialog};
pub(crate) use crate::dialogs::checkout::{CheckoutDialogSeed, open_checkout_dialog, suggest_work_copy_name};
pub(crate) use crate::dialogs::group::{GroupNameEvent, GroupNameKind, open_group_name_dialog};
pub(crate) use crate::dialogs::pick::{
    DraftCandidate, PickDialogSeed, PickDialogState, open_draft_pick_dialog,
    open_draft_pick_dialog_with,
};
pub(crate) use crate::dialogs::rename::{RenameField, RenameSeed, open_rename_dialog, rename_placeholder};
pub(crate) use crate::filter::{SortField, SortOrder};
pub(crate) use crate::model::{
    ArchiveBinding, ArchiveKind, ArchiveRequest, ArchiveUndo, CheckoutRequest, DeleteTarget,
    KeepVersions, PayloadMode, TagTarget,
};
pub(crate) use crate::payload::{PayloadStore, RESOURCES_DIR_NAME};
pub(crate) use crate::resource_view::{GroupOption, ResourcesHost};

pub(crate) use crate::jobs as resource_jobs;

// 草稿箱在本项目里的目录名 / 来源标记：只是字符串约定，不为此反向依赖 scratchpad crate。
// 值须与 `scratchpad::MODULE_DIR_NAME` / `ORIGIN_SCRATCHPAD` 保持一致（2026-11-12 分层收口）。
pub(crate) const SCRATCHPAD_DIR: &str = "scratchpad";
pub(crate) const ORIGIN_SCRATCHPAD: &str = "scratchpad";

/// 资产库宿主的 workbench 能力面（P2-b 下沉引入，与 `ResourcesHost` 方向相反）：
///
/// `ResourcesHost`（面板 → 宿主回调，26 个带 `window/cx` 的动作请求）由本文件实现；
/// 本 trait（宿主实现 → workbench 能力）把「写回执 / 项目根 / 只读判定 / 面板快照 /
/// 编辑器只读打开 / 设置 / 系统文件对话框」从 `workbench::panels::Shared` 隔离出来——
/// 下沉的装配逻辑只依赖这个 trait，不碰 Shared 具体类型（与 P2-a 的 `EditorHost` 同构）。
///
/// 实现侧（workbench `components/resource_host.rs`）转发 Shared 的对应能力；
/// 测试侧可用只记录调用的替身（gpui-kit-dev「测试用测试桥替身」）。
pub trait ResourceHostBackend: 'static {
    /// 状态栏回执（写 `Shared::notice` 并请求宿主重绘）。
    fn notice(&self, message: String, cx: &mut App);
    /// 项目根（未打开项目时 `None`）。
    fn project_root(&self) -> Option<PathBuf>;
    /// 项目只读（只读时归档 / 取回 / 改标签都被拒）。
    fn read_only(&self) -> bool;
    /// 草稿箱里可归档的文件（**宿主侧读，返回本 crate 自有的视图模型**）。
    ///
    /// 2026-11-12 分层收口：原来这里返回 `ScratchpadStore` + `Runtime`，等于让
    /// `analytics_resource` 反向依赖上游类型、自己在事件路径里遍历草稿树。
    /// 现在宿主负责扫描与标签预取，M6 只收 `Vec<DraftCandidate>`——依赖方向回到
    /// `workbench → analytics_resource`，上游补依赖也不会成环。
    fn draft_candidates(&self) -> Result<Vec<DraftCandidate>, String>;
    /// 刷新资源面板（入队一次取数 + drain，下一帧列表更新）。
    fn refresh_resources(&self, cx: &mut App);
    /// 编辑器只读打开（打开动作由事件路径做——它才有窗口；成功后请求宿主重绘）。
    fn request_open_in_editor_read_only(&self, path: PathBuf, cx: &mut App);
    /// 洞察采样 SQL（结果集列 → 洞察面板的样本查询；宿主持有连接列表）。
    fn insight_sample_sql(&self, conn_id: &str, parts: &[&str]) -> String;
    /// 洞察面板打开一个取样来源（本体只读表 / 远端引用 / 分析表）。
    fn open_insight_source_table(&self, source: insight::SampleSource, name: String, cx: &mut App);
    /// 引用失效槽位读写（`reference_stale`：懒校验命中时置位，详情面板处置）。
    fn set_reference_stale(&self, resource_id: &str, stale: bool);
    fn is_reference_stale(&self, resource_id: &str) -> bool;
    /// 面板快照（分组字典 / 组名 / 行名；与 `shared.resources_panel` 同一处取法，不另查库）。
    fn snapshot_groups(&self, cx: &App) -> Vec<GroupOption>;
    fn group_name(&self, folder_id: &str, cx: &App) -> Option<String>;
    fn row_name(&self, resource_id: &str, cx: &App) -> Option<String>;
    /// 一条存档的当前别名（详情面板头部；`None` = 没别名）。
    fn row_alias(&self, resource_id: &str, cx: &App) -> Option<Option<String>>;
    /// 设置：默认分组 / 历史内容保留策略 / 资源列表默认排序 / 分组折叠（事件路径读写，不查库）。
    fn default_group(&self, root: &str, cx: &App) -> Option<String>;
    fn keep_versions(&self, cx: &App) -> KeepVersions;
    fn set_default_resource_sort(&self, key: &str, cx: &mut App);
    fn set_collapsed_groups(&self, root: &str, collapsed_keys: &[String], cx: &mut App);
    fn set_default_group(&self, root: &str, folder_id: Option<&str>, cx: &mut App);
    /// 系统文件对话框（编辑器侧 `pick_open_path`；取消返回 `None`，不是错误）。
    fn pick_path(&self) -> Option<PathBuf>;
}

/// 状态栏提示（自由函数版：对话框回调里没有 `self`）。
fn say(backend: &Rc<dyn ResourceHostBackend>, message: impl Into<String>, cx: &mut App) {
    backend.notice(message.into(), cx);
}

/// 归档种子（原型 §4.1）：来源标签 / 目标位置（含重名避让）/ 默认显示名 / 来源连接 / 冲突预探。
///
/// 返回种子与**已避让的目标相对路径**：提交时要用同一个值，两处各算一次就会不一致。
/// 分组名单与预选由调用方给（名单来自面板快照的字典，预选来自默认分组设置）。
fn archive_seed(
    root: &Path,
    source_label: String,
    rel_path: &str,
    display_name: &str,
    source_connection: Option<String>,
    kind: ArchiveKind,
    groups: Vec<GroupOption>,
    group_id: Option<String>,
    initial_tags: Vec<String>,
    source_abs_path: Option<String>,
) -> (ArchiveDialogSeed, String) {
    let resolved = PayloadStore::new(root.to_path_buf()).free_rel_path(rel_path);
    let conflict = (resolved != rel_path).then(|| ArchiveConflict {
        taken_rel_path: format!("{RESOURCES_DIR_NAME}/{rel_path}"),
        resolved_rel_path: format!("{RESOURCES_DIR_NAME}/{resolved}"),
    });
    // 引用模式只对受管文件开放（§12.1.1 ①）：分析快照固定复制，不提供引用选项。
    let allow_reference = kind == ArchiveKind::File;
    let seed = ArchiveDialogSeed {
        source_label,
        rel_path: format!("{RESOURCES_DIR_NAME}/{resolved}"),
        name: display_name.to_string(),
        source_connection,
        conflict,
        groups,
        group_id,
        initial_tags,
        allow_reference,
        default_mode: PayloadMode::DEFAULT,
        source_abs_path,
        // 断言预填需要读项目库（异步），宿主事件路径不做 I/O；首次归档从空开始，
        // 取回后再归档的预填留给后续批次（当前由服务层保留上一次断言，不会丢）。
        assertion: None,
    };
    (seed, resolved)
}

/// 归档时按来源选**档位**（P4.5 的核心判定）：扩展名认得出来的数据文件
/// （CSV / Parquet / Excel / JSON，口径在 `engine::duckdb::file_reader_function`）
/// 走 `analysis` 档——归档后能洞察、能按配方复算；其余（`.sql` / `.md` …）仍是文件型。
fn archive_kind_for(path: &std::path::Path) -> ArchiveKind {
    match engine::duckdb::file_reader_function(&path.to_string_lossy()) {
        Some(_) => ArchiveKind::Analysis,
        None => ArchiveKind::File,
    }
}

/// 提交一次归档（入队 + 立刻刷新 + 回执）：两条归档入口共用，
/// 避免"一处记得刷新、另一处忘了"。
///
/// 保留策略在这里（主线程）读好随作业带入：工作线程上拿不到 GPUI 的 global 设置。
fn submit_archive(
    root: &Path,
    read_only: bool,
    request: ArchiveRequest,
    assertion: Option<crate::model::ArchiveAssertion>,
    backend: &Rc<dyn ResourceHostBackend>,
    cx: &mut App,
) {
    let keep_versions = backend.keep_versions(cx);
    // 探测标志跟着档位走：分析档才值得连一次 DuckDB（文件型的指纹是字节 sha256）。
    let analysis_probe = request.kind == ArchiveKind::Analysis && request.analysis.is_none();
    resource_jobs::enqueue_archive(
        root.to_path_buf(),
        read_only,
        request,
        keep_versions,
        analysis_probe,
        assertion,
    );
    backend.refresh_resources(cx);
    say(backend, "资产库：正在归档…", cx);
}

/// 宿主桥：动作转发到 `ResourceHostBackend`（workbench 能力面；无自有状态）。
struct ResourceHost {
    backend: Rc<dyn ResourceHostBackend>,
}


pub mod archive_actions;
pub mod delete_actions;
pub mod detail_actions;
pub mod group_actions;
pub mod helpers;
pub mod misc_actions;

impl ResourcesHost for ResourceHost {
    fn request_archive_from_drafts(&self, window: &mut Window, cx: &mut App) {
        archive_actions::request_archive_from_drafts(self, window, cx);
    }
    fn request_archive_drafts(
        &self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut App,
    ) {
        archive_actions::request_archive_drafts(self, paths, window, cx);
    }
    fn request_archive_from_file(&self, window: &mut Window, cx: &mut App) {
        archive_actions::request_archive_from_file(self, window, cx);
    }
    fn request_open(&self, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {
        detail_actions::request_open(self, detail, _window, cx);
    }
    fn request_view_stats(&self, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {
        detail_actions::request_view_stats(self, detail, _window, cx);
    }
    fn request_checkout(&self, detail: &ArchiveDetail, window: &mut Window, cx: &mut App) {
        detail_actions::request_checkout(self, detail, window, cx);
    }
    fn request_promote_to_global(
        &self,
        detail: &ArchiveDetail,
        _window: &mut Window,
        cx: &mut App,
    ) {
        detail_actions::request_promote_to_global(self, detail, _window, cx);
    }
    fn request_distribute_from_global(&self, name: &str, _window: &mut Window, cx: &mut App) {
        detail_actions::request_distribute_from_global(self, name, _window, cx);
    }
    fn is_reference_stale(&self, resource_id: &str) -> bool {
        return detail_actions::is_reference_stale(self, resource_id);
    }
    fn accept_reference_current(&self, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {
        detail_actions::accept_reference_current(self, detail, _window, cx);
    }
    fn request_version_history(&self, resource_id: &str, _window: &mut Window, cx: &mut App) {
        detail_actions::request_version_history(self, resource_id, _window, cx);
    }
    fn request_delete(&self, targets: &[DeleteTarget], window: &mut Window, cx: &mut App) {
        delete_actions::request_delete(self, targets, window, cx);
    }
    fn request_reveal(&self, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {
        delete_actions::request_reveal(self, detail, _window, cx);
    }
    fn request_copy_path(&self, detail: &ArchiveDetail, _window: &mut Window, cx: &mut App) {
        delete_actions::request_copy_path(self, detail, _window, cx);
    }
    fn request_undo_archive(&self, undo: &ArchiveUndo, _window: &mut Window, cx: &mut App) {
        delete_actions::request_undo_archive(self, undo, _window, cx);
    }
    fn request_index_repair(&self, _window: &mut Window, cx: &mut App) {
        delete_actions::request_index_repair(self, _window, cx);
    }
    fn request_open_payload_dir(&self, _window: &mut Window, cx: &mut App) {
        delete_actions::request_open_payload_dir(self, _window, cx);
    }
    fn request_create_group(&self, window: &mut Window, cx: &mut App) {
        group_actions::request_create_group(self, window, cx);
    }
    fn request_rename_group(&self, folder_id: &str, window: &mut Window, cx: &mut App) {
        group_actions::request_rename_group(self, folder_id, window, cx);
    }
    fn request_rename(&self, resource_id: &str, window: &mut Window, cx: &mut App) {
        group_actions::request_rename(self, resource_id, window, cx);
    }
    fn request_edit_alias(&self, resource_id: &str, window: &mut Window, cx: &mut App) {
        group_actions::request_edit_alias(self, resource_id, window, cx);
    }
    fn request_delete_group(&self, folder_id: &str, window: &mut Window, cx: &mut App) {
        group_actions::request_delete_group(self, folder_id, window, cx);
    }
    fn request_move_to_group(
        &self,
        resource_ids: &[String],
        folder_id: Option<&str>,
        _window: &mut Window,
        cx: &mut App,
    ) {
        group_actions::request_move_to_group(self, resource_ids, folder_id, _window, cx);
    }
    /// 记住列表排序：写回设置（`resources.default_sort`）。
    ///
    /// 只存**字段**不存方向：方向由 `SortField::default_order` 给（名称升序，时间 / 大小 / 版本降序）
    /// ——同名字段两个方向存成两个 key 反而让设置页多出一堆只能二选一的行。
    fn remember_sort(&self, field: SortField, _order: SortOrder, cx: &mut App) {
        misc_actions::remember_sort(self, field, _order, cx);
    }
    /// 记住分组折叠状态：按项目分桶写设置（`resources.collapsed_groups`）。
    ///
    /// 写的是**当前全集**（不是增量）：增量写入在“切项目 / 分组被删”这些场景下
    /// 容易和旧值叠出幽灵记录。空集会把该项目的记录删掉，不留空壳。
    fn remember_collapsed(&self, collapsed_keys: &[String], cx: &mut App) {
        misc_actions::remember_collapsed(self, collapsed_keys, cx);
    }
    fn remember_default_group(&self, folder_id: Option<&str>, cx: &mut App) {
        misc_actions::remember_default_group(self, folder_id, cx);
    }
    fn request_open_trash(&self, _window: &mut Window, cx: &mut App) {
        misc_actions::request_open_trash(self, _window, cx);
    }
    fn request_edit_tags(&self, targets: &[TagTarget], _window: &mut Window, cx: &mut App) {
        misc_actions::request_edit_tags(self, targets, _window, cx);
    }
    fn request_remove_tag(
        &self,
        detail: &ArchiveDetail,
        tag_id: &str,
        _window: &mut Window,
        cx: &mut App,
    ) {
        misc_actions::request_remove_tag(self, detail, tag_id, _window, cx);
    }
    fn request_refresh(&self, _window: &mut Window, cx: &mut App) {
        misc_actions::request_refresh(self, _window, cx);
    }
}

/// 组装资产库面板宿主（workbench 在面板实体创建时调用一次）。
///
/// `backend` 是 workbench 实现的 [`ResourceHostBackend`]（转发 `Shared` 能力）；
/// 本文件的 [`ResourceHost`] 实现 [`ResourcesHost`] 面板回调，动作经 backend 落回宿主。
pub fn new_host(backend: Rc<dyn ResourceHostBackend>) -> Rc<dyn ResourcesHost> {
    Rc::new(ResourceHost { backend })
}

/// 扩展名白名单判定（与 `analytics_resource::global::file_read_expr` 同口径）：
/// 数据文件升级 = 映射出全局表；文本脚本 = 内容表（方案 A）；两者之外不能升级。
fn is_data_file_ext(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("csv" | "tsv" | "txt" | "parquet" | "xlsx" | "xls" | "json" | "ndjson")
    )
}

/// 文本脚本白名单：内容是纯文本，可以读成「脚本内容表」进全局库。
///
/// 数据文件与文本脚本都是**正向白名单**——不在两张表里的（png / jpg / zip / docx
/// 这类二进制，或没有扩展名）不允许升级，升级入口先回执说明，而不是让
/// `global::promote::Script` 分支的 `fs::read_to_string` 撞 UTF-8 失败。
fn is_text_script_ext(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("sql" | "py" | "r" | "md" | "yaml" | "yml" | "toml" | "js" | "ts" | "sh" | "ps1" | "ipynb")
    )
}

#[cfg(test)]
mod tests {
    use super::{is_data_file_ext, is_text_script_ext};
    use std::path::Path;

    #[test]
    fn upgrade_shapes_are_positive_whitelists() {
        // 数据文件 → 映射出表（与 `file_read_expr` 同一张白名单，大小写不敏感）。
        assert!(is_data_file_ext(Path::new("dau.CSV")));
        assert!(is_data_file_ext(Path::new("dau.parquet")));
        // 文本脚本 → 内容表。
        assert!(is_text_script_ext(Path::new("dau.sql")));
        assert!(is_text_script_ext(Path::new("README.md")));
        // 二进制与无扩展名：两张白名单都不收（升级入口据此回执，不交给 read_to_string 撞 UTF-8）。
        assert!(!is_data_file_ext(Path::new("chart.png")));
        assert!(!is_text_script_ext(Path::new("chart.png")));
        assert!(!is_data_file_ext(Path::new("bundle.zip")));
        assert!(!is_text_script_ext(Path::new("bundle.zip")));
        assert!(!is_text_script_ext(Path::new("doc")));
    }
}
