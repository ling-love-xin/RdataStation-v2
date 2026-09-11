//! 项目管理 UI（M1）：选择器 / 标题栏项目菜单 / 新建对话框 / 项目设置 / 覆盖确认。
//!
//! 全部为**受控自绘叠加层**（与 Quick Open、设置面板一致的模式）：状态存于
//! `Shared::project_ui`，`WorkbenchView::render` 是权威同步点；视图只消费
//! `services::project_service`，不直接读写数据库。
//!
//! 语义见 `docs/architecture/project/project-prototype-design.md`。

use std::path::PathBuf;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::panels::Shared;
use crate::view::WorkbenchView;
use project::service::{self as project_service, OpenOutcome, ProjectSummary, TargetDirState};
use settings::SettingsService;

/// 选择器视图 Tab。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerTab {
    Recent,
    All,
    Removed,
}

impl PickerTab {
    pub fn label(self) -> &'static str {
        match self {
            PickerTab::Recent => "最近",
            PickerTab::All => "全部",
            PickerTab::Removed => "已移除",
        }
    }

    /// 稳定标识键（ElementId 用，不用本地化 label）。
    pub fn key(self) -> &'static str {
        match self {
            PickerTab::Recent => "recent",
            PickerTab::All => "all",
            PickerTab::Removed => "removed",
        }
    }
}

/// 排序方式（跨会话保留由设置层负责，本期先会话内生效）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSort {
    LastOpened,
    Name,
    Created,
}

impl ProjectSort {
    pub fn label(self) -> &'static str {
        match self {
            ProjectSort::LastOpened => "最近打开",
            ProjectSort::Name => "名称",
            ProjectSort::Created => "创建时间",
        }
    }

    fn next(self) -> Self {
        match self {
            ProjectSort::LastOpened => ProjectSort::Name,
            ProjectSort::Name => ProjectSort::Created,
            ProjectSort::Created => ProjectSort::LastOpened,
        }
    }

    /// 持久化键（写入 settings.projects.sort_mode）。
    pub fn key(self) -> &'static str {
        match self {
            ProjectSort::LastOpened => "last_opened",
            ProjectSort::Name => "name",
            ProjectSort::Created => "created",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "name" => ProjectSort::Name,
            "created" => ProjectSort::Created,
            _ => ProjectSort::LastOpened,
        }
    }
}

/// 选择器状态。
#[non_exhaustive]
pub struct PickerState {
    pub tab: PickerTab,
    pub sort: ProjectSort,
    pub items: Vec<ProjectSummary>,
    pub loaded: bool,
    pub error: Option<String>,
    /// 排序是否已从 settings 初始化（避免每次 render 重读）。
    pub sort_initialized: bool,
}

impl Default for PickerState {
    fn default() -> Self {
        Self {
            tab: PickerTab::Recent,
            sort: ProjectSort::LastOpened,
            items: Vec::new(),
            loaded: false,
            error: None,
            sort_initialized: false,
        }
    }
}

/// 待处理的项目动作（未保存草稿拦截用）。
#[derive(Debug, Clone)]
pub enum PendingAction {
    OpenPath(PathBuf),
    Close,
}

/// 覆盖层对话框。
pub enum ProjectDialog {
    /// 新建项目（名称/位置/描述取自 `ProjectInputs`）。
    Create { error: Option<String> },
    /// 打开现有目录（路径取自 `create_location` 输入）。
    OpenFolder {
        path: PathBuf,
        error: Option<String>,
    },
    /// 重新定位（旧路径失效）：把项目指向新目录。
    Relocate {
        id: String,
        name: String,
        error: Option<String>,
    },
    /// 删除磁盘数据（输入项目名确认）。
    ConfirmDelete {
        id: String,
        name: String,
        root: PathBuf,
        error: Option<String>,
    },
    /// 项目已被其他实例占用（只读打开 / 仍要打开）。
    LockBusy {
        name: String,
        root: PathBuf,
        pid: u32,
    },
    /// 未保存草稿拦截。
    Unsaved { pending: PendingAction },
}

/// 项目 UI 状态（挂在 `Shared` 上，避免散落多字段）。
#[non_exhaustive]
pub struct ProjectUiState {
    pub picker: PickerState,
    pub menu_open: bool,
    pub settings_open: bool,
    pub dialog: Option<ProjectDialog>,
    /// 当前持有的项目写锁（只读打开时为 `None`）。
    pub lock: Option<project::ProjectLock>,
    pub read_only: bool,
    /// 项目世代：切换/关闭后自增，供各面板订阅重载。
    pub epoch: u64,
    pub notice: Option<String>,
}

impl Default for ProjectUiState {
    fn default() -> Self {
        Self {
            picker: PickerState::default(),
            menu_open: false,
            settings_open: false,
            dialog: None,
            lock: None,
            read_only: false,
            epoch: 0,
            notice: None,
        }
    }
}

/// 项目相关输入实体（由 `WorkbenchView` 懒创建并持有）。
#[derive(Clone)]
#[non_exhaustive]
pub struct ProjectInputs {
    pub search: Entity<InputState>,
    pub create_name: Entity<InputState>,
    pub create_location: Entity<InputState>,
    pub create_desc: Entity<InputState>,
    pub delete_confirm: Entity<InputState>,
    /// 项目设置中的名称编辑（重命名）。
    pub rename: Entity<InputState>,
    /// 项目设置中的版本说明输入。
    pub version_msg: Entity<InputState>,
}

impl ProjectInputs {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            search: cx.new(|cx| InputState::new(window, cx)),
            create_name: cx.new(|cx| InputState::new(window, cx)),
            create_location: cx.new(|cx| InputState::new(window, cx)),
            create_desc: cx.new(|cx| InputState::new(window, cx)),
            delete_confirm: cx.new(|cx| InputState::new(window, cx)),
            rename: cx.new(|cx| InputState::new(window, cx)),
            version_msg: cx.new(|cx| InputState::new(window, cx)),
        }
    }
}

/// 默认位置（用户主目录）。
fn default_location() -> String {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string())
}

// ==================== 加载 / 刷新 ====================

/// 刷新选择器列表（切换 / 关闭后调用，会重绘）。
pub fn refresh_picker(shared: &Shared, entity: &Entity<WorkbenchView>, cx: &mut App) {
    load_picker(shared);
    entity.update(cx, |_, cx| cx.notify());
}

/// 加载列表（不 notify；供 render 首帧调用，避免 render 内 notify 循环）。
pub fn load_picker(shared: &Shared) {
    let (tab, sort) = {
        let ui = shared.project_ui.borrow();
        (ui.picker.tab, ui.picker.sort)
    };
    let result = match tab {
        PickerTab::Recent => project_service::list_recent(12),
        PickerTab::All => project_service::list_all(),
        PickerTab::Removed => project_service::list_removed(),
    };

    let mut ui = shared.project_ui.borrow_mut();
    match result {
        Ok(mut items) => {
            sort_items(&mut items, sort);
            ui.picker.items = items;
            ui.picker.error = None;
        }
        Err(e) => {
            ui.picker.items.clear();
            ui.picker.error = Some(e);
        }
    }
    ui.picker.loaded = true;
}

fn sort_items(items: &mut [ProjectSummary], sort: ProjectSort) {
    items.sort_by(|a, b| {
        b.is_pinned.cmp(&a.is_pinned).then_with(|| match sort {
            ProjectSort::LastOpened => b.last_opened_at.cmp(&a.last_opened_at),
            ProjectSort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            ProjectSort::Created => b.created_at.cmp(&a.created_at),
        })
    });
}

// ==================== 动作 ====================

/// 切换 Tab 并刷新。
pub fn set_tab(shared: &Shared, tab: PickerTab, entity: &Entity<WorkbenchView>, cx: &mut App) {
    shared.project_ui.borrow_mut().picker.tab = tab;
    refresh_picker(shared, entity, cx);
}

/// 循环切换排序（持久化到 settings）并刷新。
pub fn cycle_sort(shared: &Shared, entity: &Entity<WorkbenchView>, cx: &mut App) {
    let next = shared.project_ui.borrow().picker.sort.next();
    shared.project_ui.borrow_mut().picker.sort = next;
    SettingsService::set_project_sort_mode(next.key(), cx);
    refresh_picker(shared, entity, cx);
}

/// 打开新建项目对话框（重置表单）。
pub fn open_create_dialog(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .create_name
        .update(cx, |s, cx| s.set_value("", window, cx));
    inputs
        .create_location
        .update(cx, |s, cx| s.set_value(default_location(), window, cx));
    inputs
        .create_desc
        .update(cx, |s, cx| s.set_value("", window, cx));
    shared.project_ui.borrow_mut().dialog = Some(ProjectDialog::Create { error: None });
    entity.update(cx, |_, cx| cx.notify());
}

/// 打开「打开现有目录」对话框。
pub fn open_folder_dialog(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .create_location
        .update(cx, |s, cx| s.set_value(default_location(), window, cx));
    shared.project_ui.borrow_mut().dialog = Some(ProjectDialog::OpenFolder {
        path: PathBuf::new(),
        error: None,
    });
    entity.update(cx, |_, cx| cx.notify());
}

/// 从输入读取并创建项目。
pub fn submit_create(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    let name = inputs.create_name.read(cx).value().trim().to_string();
    let location = inputs.create_location.read(cx).value().trim().to_string();
    let description = {
        let d = inputs.create_desc.read(cx).value().trim().to_string();
        (!d.is_empty()).then_some(d)
    };

    if let Err(e) = project_service::validate_project_name(&name) {
        set_dialog_error(shared, e, entity, cx);
        return;
    }
    if location.is_empty() {
        set_dialog_error(shared, "请选择项目位置".to_string(), entity, cx);
        return;
    }
    let path = PathBuf::from(&location).join(&name);

    let input = project_service::CreateProjectInput::new(name, path).with_description(description);
    match project_service::create(input) {
        Ok(summary) => {
            // 创建成功后直接打开（走统一打开路径，取得锁）。
            open_path(shared, &summary.path, entity, cx);
        }
        Err(e) => set_dialog_error(shared, e, entity, cx),
    }
}

/// 从输入读取并打开目录。
pub fn submit_open_folder(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    let path = PathBuf::from(inputs.create_location.read(cx).value().trim().to_string());
    if path.as_os_str().is_empty() {
        set_dialog_error(shared, "请输入项目目录".to_string(), entity, cx);
        return;
    }
    match project_service::inspect_target(&path) {
        TargetDirState::Missing => {
            set_dialog_error(
                shared,
                format!("目录不存在：{}", path.display()),
                entity,
                cx,
            );
        }
        TargetDirState::NonEmpty => {
            set_dialog_error(
                shared,
                "目录不是项目（缺少 .RSmeta），且非空，无法作为项目打开".to_string(),
                entity,
                cx,
            );
        }
        TargetDirState::Empty => {
            // 空目录不视为项目；提示改用「新建项目」。
            set_dialog_error(shared, "空目录：请改用「新建项目」".to_string(), entity, cx);
        }
        TargetDirState::ExistingProject => {
            shared.project_ui.borrow_mut().dialog = None;
            open_path(shared, &path, entity, cx);
        }
    }
}

/// 从示例项目开始（内置最小示例：示例 SQL 草稿）。
pub fn create_sample(shared: &Shared, entity: &Entity<WorkbenchView>, cx: &mut App) {
    let base = crate::services::workspace_loader::default_global_dir()
        .parent()
        .map(|p| p.join("samples"))
        .unwrap_or_else(|| std::env::temp_dir().join("RdataStation").join("samples"));
    let path = base.join("示例项目");
    if !project_service::is_valid_project(&path) {
        let input = project_service::CreateProjectInput::new("示例项目", path.clone())
            .with_description(Some("RdataStation 内置示例".to_string()))
            .with_sample_drafts(true);
        if let Err(e) = project_service::create(input) {
            shared.project_ui.borrow_mut().picker.error = Some(e);
            entity.update(cx, |_, cx| cx.notify());
            return;
        }
        // 写入示例草稿（失败不阻断打开）。
        let _ = std::fs::write(
            path.join("welcome.sql"),
            "-- RdataStation 示例项目\n-- 在此编写并执行 SQL。\nSELECT 1 AS hello;\n",
        );
    }
    open_path(shared, &path, entity, cx);
}

/// 打开项目（含未保存拦截 + 锁占用分支）。
pub fn request_open(shared: &Shared, path: &PathBuf, entity: &Entity<WorkbenchView>, cx: &mut App) {
    if shared.editor_dirty.get() {
        shared.project_ui.borrow_mut().dialog = Some(ProjectDialog::Unsaved {
            pending: PendingAction::OpenPath(path.clone()),
        });
        entity.update(cx, |_, cx| cx.notify());
        return;
    }
    open_path(shared, path, entity, cx);
}

/// 直接打开（不再拦截）。
pub fn open_path(shared: &Shared, path: &PathBuf, entity: &Entity<WorkbenchView>, cx: &mut App) {
    match project_service::open(path) {
        Ok(OpenOutcome::Opened(opened)) => apply_opened(shared, opened, entity, cx),
        Ok(OpenOutcome::Busy(info)) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "项目".to_string());
            shared.project_ui.borrow_mut().dialog = Some(ProjectDialog::LockBusy {
                name,
                root: path.clone(),
                pid: info.pid,
            });
            entity.update(cx, |_, cx| cx.notify());
        }
        Err(e) => {
            shared.project_ui.borrow_mut().picker.error = Some(e);
            entity.update(cx, |_, cx| cx.notify());
        }
    }
}

/// 只读打开（逃生口）。
pub fn open_read_only(
    shared: &Shared,
    path: &PathBuf,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    match project_service::open_read_only(path) {
        Ok(opened) => apply_opened(shared, opened, entity, cx),
        Err(e) => {
            shared.project_ui.borrow_mut().picker.error = Some(e);
            entity.update(cx, |_, cx| cx.notify());
        }
    }
}

/// 应用打开结果：写会话 + 持锁 + 自增世代 + 刷新连接。
fn apply_opened(
    shared: &Shared,
    opened: project_service::OpenedProject,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    let (store, lock, read_only, summary) = opened.into_parts();
    // store 目前仅用于确认加载成功；会话只保留根与名（与既有 P0 会话一致）。
    drop(store);

    let session = crate::services::project_session::ProjectSession {
        root: summary.path.clone(),
        name: summary.name.clone(),
    };
    *shared.project.borrow_mut() = Some(session);

    {
        let mut ui = shared.project_ui.borrow_mut();
        ui.lock = lock;
        ui.read_only = read_only;
        ui.epoch += 1;
        ui.menu_open = false;
        ui.settings_open = false;
        ui.dialog = None;
        ui.notice = read_only.then(|| "只读打开：该项目已被另一实例占用".to_string());
        ui.picker.error = None;
    }

    // 切换项目 → 刷新连接列表与选中项（避免残留上一项目数据）。
    let (conns, notice) = crate::services::workspace_loader::load_persisted_connections();
    *shared.connections.borrow_mut() = conns;
    *shared.notice.borrow_mut() = notice;
    let has = !shared.connections.borrow().is_empty();
    shared.selected.set(if has { Some(0) } else { None });
    *shared.nav_for.borrow_mut() = None;
    shared.nav_tables.borrow_mut().clear();
    *shared.sql_for.borrow_mut() = None;
    shared.editor_dirty.set(false);

    entity.update(cx, |_, cx| cx.notify());
}

/// 请求关闭项目（含未保存拦截）。
pub fn request_close(shared: &Shared, entity: &Entity<WorkbenchView>, cx: &mut App) {
    if shared.editor_dirty.get() {
        shared.project_ui.borrow_mut().dialog = Some(ProjectDialog::Unsaved {
            pending: PendingAction::Close,
        });
        entity.update(cx, |_, cx| cx.notify());
        return;
    }
    do_close(shared, entity, cx);
}

/// 执行关闭：释放锁 + 清会话 + 回选择器。
pub fn do_close(shared: &Shared, entity: &Entity<WorkbenchView>, cx: &mut App) {
    let lock = shared.project_ui.borrow_mut().lock.take();
    if let Some(lock) = lock {
        let _ = lock.release();
    }
    *shared.project.borrow_mut() = None;
    {
        let mut ui = shared.project_ui.borrow_mut();
        ui.read_only = false;
        ui.epoch += 1;
        ui.menu_open = false;
        ui.settings_open = false;
        ui.dialog = None;
        ui.notice = None;
    }
    refresh_picker(shared, entity, cx);
}

/// 保存编辑区 SQL 草稿到项目根（唯一命名）。
fn save_draft(shared: &Shared) -> Result<std::path::PathBuf, String> {
    let root = shared
        .project
        .borrow()
        .as_ref()
        .map(|s| s.root.clone())
        .ok_or_else(|| "未打开项目".to_string())?;
    let sql = shared.editor_sql.borrow().clone();
    if sql.trim().is_empty() {
        return Err("没有可保存的内容".to_string());
    }
    let mut name = "未命名.sql".to_string();
    let mut i = 1;
    while root.join(&name).exists() {
        name = format!("未命名_{i}.sql");
        i += 1;
    }
    let path = root.join(&name);
    std::fs::write(&path, sql).map_err(|e| format!("保存草稿失败: {e}"))?;
    Ok(path)
}

/// 处理未保存拦截结果（保存 / 放弃 / 取消）；`window` 用于命令式清空编辑区。
pub fn resolve_unsaved(
    shared: &Shared,
    save: bool,
    entity: &Entity<WorkbenchView>,
    window: &mut Window,
    cx: &mut App,
) {
    let pending = match shared.project_ui.borrow_mut().dialog.take() {
        Some(ProjectDialog::Unsaved { pending }) => pending,
        other => {
            shared.project_ui.borrow_mut().dialog = other;
            return;
        }
    };
    if save {
        match save_draft(shared) {
            Ok(path) => {
                shared.project_ui.borrow_mut().notice =
                    Some(format!("草稿已保存：{}", path.display()));
            }
            Err(e) => {
                // 保存失败：恢复拦截对话框，不推进动作。
                shared.project_ui.borrow_mut().dialog = Some(ProjectDialog::Unsaved { pending });
                shared.project_ui.borrow_mut().notice = Some(e);
                entity.update(cx, |_, cx| cx.notify());
                return;
            }
        }
    }
    // 保存或放弃后都要清空编辑区（命令式，事件上下文；避免下一轮 render 再次判脏）。
    let clear = shared.editor_clear.borrow().clone();
    if let Some(clear) = clear {
        clear(window, cx);
    }
    shared.editor_dirty.set(false);
    match pending {
        PendingAction::OpenPath(path) => open_path(shared, &path, entity, cx),
        PendingAction::Close => do_close(shared, entity, cx),
    }
}

/// 取消当前对话框。
pub fn cancel_dialog(shared: &Shared, entity: &Entity<WorkbenchView>, cx: &mut App) {
    shared.project_ui.borrow_mut().dialog = None;
    entity.update(cx, |_, cx| cx.notify());
}

fn set_dialog_error(
    shared: &Shared,
    message: String,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    let mut ui = shared.project_ui.borrow_mut();
    ui.dialog = match ui.dialog.take() {
        Some(ProjectDialog::Create { .. }) => Some(ProjectDialog::Create {
            error: Some(message),
        }),
        Some(ProjectDialog::OpenFolder { path, .. }) => Some(ProjectDialog::OpenFolder {
            path,
            error: Some(message),
        }),
        Some(ProjectDialog::ConfirmDelete { id, name, root, .. }) => {
            Some(ProjectDialog::ConfirmDelete {
                id,
                name,
                root,
                error: Some(message),
            })
        }
        Some(ProjectDialog::Relocate { id, name, .. }) => Some(ProjectDialog::Relocate {
            id,
            name,
            error: Some(message),
        }),
        other => other,
    };
    drop(ui);
    entity.update(cx, |_, cx| cx.notify());
}

/// 固定 / 取消固定。
pub fn toggle_pin(
    shared: &Shared,
    item: &ProjectSummary,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    if let Err(e) = project_service::set_pinned(&item.id, !item.is_pinned) {
        shared.project_ui.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(shared, entity, cx);
}

/// 软删（移出名册）。
pub fn soft_remove(shared: &Shared, id: &str, entity: &Entity<WorkbenchView>, cx: &mut App) {
    if let Err(e) = project_service::soft_remove(id) {
        shared.project_ui.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(shared, entity, cx);
}

/// 恢复已移除项目。
pub fn restore(shared: &Shared, id: &str, entity: &Entity<WorkbenchView>, cx: &mut App) {
    if let Err(e) = project_service::restore(id) {
        shared.project_ui.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(shared, entity, cx);
}

/// 失效路径项目：移出名册（不动磁盘）。
pub fn forget(shared: &Shared, id: &str, entity: &Entity<WorkbenchView>, cx: &mut App) {
    if let Err(e) = project_service::forget(id) {
        shared.project_ui.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(shared, entity, cx);
}

/// 打开「重新定位」对话框。
pub fn open_relocate_dialog(
    shared: &Shared,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    entity: &Entity<WorkbenchView>,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .create_location
        .update(cx, |s, cx| s.set_value(String::new(), window, cx));
    shared.project_ui.borrow_mut().dialog = Some(ProjectDialog::Relocate {
        id: item.id.clone(),
        name: item.name.clone(),
        error: None,
    });
    entity.update(cx, |_, cx| cx.notify());
}

/// 提交重新定位：校验新目录后改写名册与项目路径。
pub fn submit_relocate(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    let new_root = PathBuf::from(inputs.create_location.read(cx).value().trim().to_string());
    let id = match &shared.project_ui.borrow().dialog {
        Some(ProjectDialog::Relocate { id, .. }) => id.clone(),
        _ => return,
    };
    if new_root.as_os_str().is_empty() {
        set_dialog_error(shared, "请输入新的项目目录".to_string(), entity, cx);
        return;
    }
    match project_service::relocate(&id, &new_root) {
        Ok(()) => {
            shared.project_ui.borrow_mut().dialog = None;
            refresh_picker(shared, entity, cx);
        }
        Err(e) => set_dialog_error(shared, e, entity, cx),
    }
}

/// 打开删除确认（输入项目名）。
pub fn open_delete_dialog(
    shared: &Shared,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    entity: &Entity<WorkbenchView>,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .delete_confirm
        .update(cx, |s, cx| s.set_value("", window, cx));
    shared.project_ui.borrow_mut().dialog = Some(ProjectDialog::ConfirmDelete {
        id: item.id.clone(),
        name: item.name.clone(),
        root: item.path.clone(),
        error: None,
    });
    entity.update(cx, |_, cx| cx.notify());
}

/// 确认删除磁盘数据（需输入项目名匹配）。
pub fn confirm_delete(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    let typed = inputs.delete_confirm.read(cx).value().trim().to_string();
    let (id, name, root) = match shared.project_ui.borrow().dialog {
        Some(ProjectDialog::ConfirmDelete {
            ref id,
            ref name,
            ref root,
            ..
        }) => (id.clone(), name.clone(), root.clone()),
        _ => return,
    };
    if typed != name {
        set_dialog_error(shared, "输入的项目名不匹配".to_string(), entity, cx);
        return;
    }
    // 若删除的是当前打开项目，先关闭以释放锁。
    let is_current = shared
        .project
        .borrow()
        .as_ref()
        .map(|s| s.root == root)
        .unwrap_or(false);
    if is_current {
        do_close(shared, entity, cx);
    }
    match project_service::delete_disk(&id, &root) {
        Ok(()) => {
            shared.project_ui.borrow_mut().dialog = None;
            refresh_picker(shared, entity, cx);
        }
        Err(e) => set_dialog_error(shared, e, entity, cx),
    }
}

/// 只读模式守卫：被拦截时写入提示并返回 `true`。
fn read_only_blocked(
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
    action: &str,
) -> bool {
    if shared.project_ui.borrow().read_only {
        shared.project_ui.borrow_mut().notice = Some(format!("只读模式：不允许{action}"));
        entity.update(cx, |_, cx| cx.notify());
        true
    } else {
        false
    }
}

/// 保存项目设置中的重命名。
pub fn save_rename(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    if read_only_blocked(shared, entity, cx, "重命名") {
        return;
    }
    let name = inputs.rename.read(cx).value().trim().to_string();
    let (id, root) = {
        // 通过名册查当前项目 id（会话只存根/名）。
        let root = shared.project.borrow().as_ref().map(|s| s.root.clone());
        let Some(root) = root else { return };
        let id = project_service::list_all()
            .ok()
            .and_then(|v| v.into_iter().find(|p| p.path == root).map(|p| p.id));
        match id {
            Some(id) => (id, root),
            None => return,
        }
    };
    match project_service::update_meta(&id, &root, &name, None) {
        Ok(()) => {
            if let Some(session) = shared.project.borrow_mut().as_mut() {
                session.name = name;
            }
        }
        Err(e) => {
            shared.project_ui.borrow_mut().notice = Some(e);
        }
    }
    entity.update(cx, |_, cx| cx.notify());
}

/// 归档 / 取消归档当前项目。
pub fn toggle_archive(shared: &Shared, entity: &Entity<WorkbenchView>, cx: &mut App) {
    if read_only_blocked(shared, entity, cx, "归档") {
        return;
    }
    let root = match shared.project.borrow().as_ref().map(|s| s.root.clone()) {
        Some(r) => r,
        None => return,
    };
    let current = project_service::list_all()
        .ok()
        .and_then(|v| v.into_iter().find(|p| p.path == root));
    let Some(item) = current else { return };
    let archived = item.status == "archived";
    if let Err(e) = project_service::set_archived(&item.id, &root, !archived) {
        shared.project_ui.borrow_mut().notice = Some(e);
    } else {
        shared.project_ui.borrow_mut().menu_open = false;
        shared.project_ui.borrow_mut().notice = Some(format!(
            "已{}项目",
            if archived { "取消归档" } else { "归档" }
        ));
    }
    entity.update(cx, |_, cx| cx.notify());
}

// ==================== 渲染 ====================

/// 项目选择器（覆盖中央区）。
pub fn render_picker(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) -> Div {
    let theme = cx.theme();
    let ui = shared.project_ui.borrow();
    let picker = &ui.picker;

    // 搜索在渲染时过滤（避免每次输入都重查数据库）。
    let needle = inputs.search.read(cx).value().trim().to_lowercase();
    let items: Vec<ProjectSummary> = picker
        .items
        .iter()
        .filter(|p| {
            needle.is_empty()
                || p.name.to_lowercase().contains(&needle)
                || p.path.to_string_lossy().to_lowercase().contains(&needle)
        })
        .cloned()
        .collect();
    let count = items.len();

    // ---- 顶部：标题 + Tab ----
    let mut tabs = div().h_flex().items_center().gap_2();
    for tab in [PickerTab::Recent, PickerTab::All, PickerTab::Removed] {
        let on = picker.tab == tab;
        let shared_t = shared.clone();
        let entity_t = entity.clone();
        tabs = tabs.child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "picker-tab-{}",
                    tab.key()
                ))))
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .text_sm()
                .when(on, |d| {
                    d.bg(theme.colors.sidebar_accent)
                        .font_weight(FontWeight::MEDIUM)
                })
                .text_color(if on {
                    theme.colors.foreground
                } else {
                    theme.colors.muted_foreground
                })
                .on_click(move |_, _, app| set_tab(&shared_t, tab, &entity_t, app))
                .child(tab.label()),
        );
    }

    // ---- 搜索 + 排序 ----
    let shared_sort = shared.clone();
    let entity_sort = entity.clone();
    let search_row = div()
        .h_flex()
        .items_center()
        .gap_2()
        .child(div().flex_1().child(Input::new(&inputs.search)))
        .child(
            div()
                .id("picker-sort")
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(theme.colors.border)
                .cursor_pointer()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .on_click(move |_, _, app| cycle_sort(&shared_sort, &entity_sort, app))
                .child(format!("排序：{}", picker.sort.label())),
        );

    // ---- 列表 ----
    let mut list = div()
        .v_flex()
        .gap_2()
        .flex_1()
        .min_h_0()
        .overflow_y_scrollbar();
    if let Some(err) = &picker.error {
        list = list.child(error_line(err, theme));
    }
    if picker.items.is_empty() {
        list = list.child(empty_state(picker.tab, picker.loaded));
    } else if items.is_empty() {
        list = list.child(
            div()
                .v_flex()
                .items_center()
                .py_8()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("无匹配项目"),
        );
    } else {
        for item in &items {
            list = list.child(project_card(
                shared, inputs, item, picker.tab, entity, theme,
            ));
        }
    }

    // ---- 右侧操作栏 ----
    let shared_new = shared.clone();
    let entity_new = entity.clone();
    let inputs_new = inputs.clone();
    let shared_open = shared.clone();
    let entity_open = entity.clone();
    let inputs_open = inputs.clone();
    let shared_sample = shared.clone();
    let entity_sample = entity.clone();

    let rail = div()
        .v_flex()
        .gap_2()
        .w(px(240.))
        .flex_none()
        .p_3()
        .bg(theme.colors.sidebar)
        .border_l_1()
        .border_color(theme.colors.border)
        .child(
            Button::new("picker-new")
                .primary()
                .label("＋ 新建项目")
                .on_click(move |_, window, app| {
                    open_create_dialog(&shared_new, &inputs_new, &entity_new, window, app);
                }),
        )
        .child(
            Button::new("picker-open")
                .secondary()
                .label("🗀 打开文件夹…")
                .on_click(move |_, window, app| {
                    open_folder_dialog(&shared_open, &inputs_open, &entity_open, window, app);
                }),
        )
        .child(
            Button::new("picker-sample")
                .secondary()
                .label("▦ 从示例项目开始")
                .on_click(move |_, _, app| create_sample(&shared_sample, &entity_sample, app)),
        )
        .child(
            div()
                .mt_2()
                .v_flex()
                .gap_1()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(div().font_weight(FontWeight::MEDIUM).child("快速开始"))
                .child("· 新建空项目")
                .child("· 从示例项目开始")
                .child("· 打开现有目录（.RSmeta）"),
        );

    if let Some(notice) = &ui.notice {
        // 只读提示等
        list = list.child(error_line(notice, theme));
    }

    div()
        .h_flex()
        .items_stretch()
        .size_full()
        .bg(theme.colors.background)
        .child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .gap_3()
                .p_4()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("项目"),
                        )
                        .child(tabs)
                        .child(
                            div()
                                .ml_auto()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child(format!("{count} 个项目")),
                        ),
                )
                .child(search_row)
                .child(list),
        )
        .child(rail)
}

/// 项目卡片。
fn project_card(
    shared: &Shared,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    tab: PickerTab,
    entity: &Entity<WorkbenchView>,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let shared_open = shared.clone();
    let entity_open = entity.clone();
    let path_open = item.path.clone();

    let shared_pin = shared.clone();
    let entity_pin = entity.clone();
    let item_pin = item.clone();

    let shared_remove = shared.clone();
    let entity_remove = entity.clone();
    let item_remove = item.clone();

    let shared_del = shared.clone();
    let entity_del = entity.clone();
    let item_del = item.clone();
    let inputs_del = inputs.clone();

    let status_color = match item.status.as_str() {
        "active" => theme.colors.success,
        "archived" => theme.colors.muted_foreground,
        "syncing" => theme.colors.info,
        "offline" => theme.colors.warning,
        _ => theme.colors.muted_foreground,
    };

    let mut actions = div().h_flex().items_center().gap_1();
    // 固定 / 取消固定
    actions = actions.child(
        div()
            .id(ElementId::Name(SharedString::from(format!(
                "pin-{}",
                item.id
            ))))
            .px_1()
            .rounded_sm()
            .cursor_pointer()
            .text_xs()
            .text_color(if item.is_pinned {
                theme.colors.primary
            } else {
                theme.colors.muted_foreground
            })
            .on_click(move |_, _, app| toggle_pin(&shared_pin, &item_pin, &entity_pin, app))
            .child(if item.is_pinned {
                "📌 已固定"
            } else {
                "固定"
            }),
    );

    if tab == PickerTab::Removed {
        let shared_r = shared.clone();
        let entity_r = entity.clone();
        let id_r = item.id.clone();
        actions = actions.child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "restore-{}",
                    item.id
                ))))
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(theme.colors.primary)
                .on_click(move |_, _, app| restore(&shared_r, &id_r, &entity_r, app))
                .child("恢复"),
        );
    } else if item.path_exists {
        actions = actions.child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "remove-{}",
                    item.id
                ))))
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .on_click(move |_, _, app| {
                    soft_remove(&shared_remove, &item_remove.id, &entity_remove, app)
                })
                .child("移出列表"),
        );
        actions = actions.child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "delete-{}",
                    item.id
                ))))
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(theme.colors.danger)
                .on_click(move |_, window, app| {
                    open_delete_dialog(
                        &shared_del,
                        &inputs_del,
                        &item_del,
                        &entity_del,
                        window,
                        app,
                    )
                })
                .child("删除数据"),
        );
    } else {
        let shared_rl = shared.clone();
        let entity_rl = entity.clone();
        let item_rl = item.clone();
        let inputs_rl = inputs.clone();
        actions = actions.child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "relocate-{}",
                    item.id
                ))))
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(theme.colors.primary)
                .on_click(move |_, window, app| {
                    open_relocate_dialog(&shared_rl, &inputs_rl, &item_rl, &entity_rl, window, app)
                })
                .child("重新定位"),
        );
        let shared_f = shared.clone();
        let entity_f = entity.clone();
        let id_f = item.id.clone();
        actions = actions.child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "forget-{}",
                    item.id
                ))))
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .on_click(move |_, _, app| forget(&shared_f, &id_f, &entity_f, app))
                .child("移出列表"),
        );
    }

    let mut card = div()
        .v_flex()
        .gap_1()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(if item.path_exists {
            theme.colors.border
        } else {
            theme.colors.warning
        })
        .bg(theme.colors.background)
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id(ElementId::Name(SharedString::from(format!(
                            "open-{}",
                            item.id
                        ))))
                        .cursor_pointer()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.colors.foreground)
                        .on_click(move |_, _, app| {
                            request_open(&shared_open, &path_open, &entity_open, app)
                        })
                        .child(item.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(status_color)
                        .child(item.status.clone()),
                )
                .child(div().ml_auto().child(actions)),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(item.path.to_string_lossy().to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(meta_line(item)),
        );

    if !item.path_exists {
        card = card.child(
            div()
                .text_xs()
                .text_color(theme.colors.warning)
                .child("⚠ 路径已失效，可能已移动或删除"),
        );
    } else if !item.missing_drivers.is_empty() {
        card = card.child(
            div()
                .text_xs()
                .text_color(theme.colors.warning)
                .child(format!("⚠ 缺失驱动：{}", item.missing_drivers.join("、"))),
        );
    }
    if item.lock.is_some() {
        card = card.child(
            div()
                .text_xs()
                .text_color(theme.colors.info)
                .child("● 已在另一实例打开"),
        );
    }
    card
}

fn meta_line(item: &ProjectSummary) -> String {
    let last = item
        .last_opened_at
        .clone()
        .unwrap_or_else(|| "从未打开".to_string());
    format!("最后打开：{} · 状态：{}", last, item.status)
}

fn empty_state(tab: PickerTab, loaded: bool) -> Div {
    let (title, desc) = match tab {
        PickerTab::Recent => (
            "还没有最近项目",
            "新建一个项目，或打开包含 .RSmeta 的现有目录。",
        ),
        PickerTab::All => ("还没有项目", "新建一个项目开始使用。"),
        PickerTab::Removed => ("没有已移除的项目", "被移出名册的项目会出现在这里，可恢复。"),
    };
    div()
        .v_flex()
        .items_center()
        .justify_center()
        .gap_1()
        .py_8()
        .child(if loaded {
            div().child(title)
        } else {
            div().child("加载中…")
        })
        .child(div().child(desc))
}

fn error_line(message: &str, theme: &gpui_kit::component::Theme) -> Div {
    div()
        .p_2()
        .rounded_md()
        .text_xs()
        .text_color(theme.colors.danger)
        .child(message.to_string())
}

// ==================== 标题栏项目菜单 ====================

/// 标题栏项目菜单（有项目态）。
pub fn render_menu(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) -> Option<Div> {
    if !shared.project_ui.borrow().menu_open {
        return None;
    }
    let theme = cx.theme();
    let (name, root, read_only) = {
        let ui = shared.project_ui.borrow();
        let p = shared.project.borrow();
        let name = p.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        let root = p.as_ref().map(|s| s.root.clone()).unwrap_or_default();
        (name, root, ui.read_only)
    };

    let inputs_settings = inputs.clone();
    let inputs_rename = inputs.clone();
    let shared_close = shared.clone();
    let entity_close = entity.clone();
    let shared_settings = shared.clone();
    let entity_settings = entity.clone();
    let shared_rename = shared.clone();
    let entity_rename = entity.clone();
    let shared_switch = shared.clone();
    let entity_switch = entity.clone();
    let shared_archive = shared.clone();
    let entity_archive = entity.clone();
    let name_settings = name.clone();
    let name_rename = name.clone();
    let path = root.clone();

    let mut menu = div()
        .absolute()
        .top(px(38.))
        .left(px(52.))
        .w(px(230.))
        .p_1()
        .rounded_lg()
        .border_1()
        .border_color(theme.colors.border)
        .bg(theme.colors.popover)
        .shadow_lg()
        .child(menu_item(
            "switch",
            "切换项目…",
            theme,
            move |_window, app| {
                // 切换项目 = 关闭当前 + 回到选择器（一实例一项目）。
                request_close(&shared_switch, &entity_switch, app);
            },
        ))
        .child(menu_item(
            "settings",
            "项目设置…",
            theme,
            move |window, app| {
                inputs_settings
                    .rename
                    .update(app, |s, cx| s.set_value(name_settings.clone(), window, cx));
                let mut ui = shared_settings.project_ui.borrow_mut();
                ui.menu_open = false;
                ui.settings_open = true;
                drop(ui);
                entity_settings.update(app, |_, cx| cx.notify());
            },
        ))
        .child(menu_item(
            "rename",
            "重命名…",
            theme,
            move |window, app| {
                inputs_rename
                    .rename
                    .update(app, |s, cx| s.set_value(name_rename.clone(), window, cx));
                let mut ui = shared_rename.project_ui.borrow_mut();
                ui.menu_open = false;
                ui.settings_open = true;
                drop(ui);
                entity_rename.update(app, |_, cx| cx.notify());
            },
        ))
        .child(menu_item("reveal", "在资源管理器中显示", theme, {
            let path = path.clone();
            move |_window, _app| reveal_in_explorer(&path)
        }))
        .child(div().h(px(1.)).my_1().bg(theme.colors.border))
        .child(menu_item(
            "archive",
            "归档 / 取消归档",
            theme,
            move |_window, app| {
                toggle_archive(&shared_archive, &entity_archive, app);
            },
        ))
        .child(div().h(px(1.)).my_1().bg(theme.colors.border))
        .child(menu_item(
            "close",
            "关闭项目",
            theme,
            move |_window, app| {
                shared_close.project_ui.borrow_mut().menu_open = false;
                request_close(&shared_close, &entity_close, app);
            },
        ));

    if read_only {
        menu = menu.child(
            div()
                .p_2()
                .text_xs()
                .text_color(theme.colors.warning)
                .child("只读模式（另一实例持有写锁）"),
        );
    }
    let _ = name;
    Some(menu)
}

fn menu_item(
    key: &'static str,
    label: &'static str,
    theme: &gpui_kit::component::Theme,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(ElementId::Name(SharedString::from(format!("menu-{key}"))))
        .px_2()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .text_xs()
        .text_color(theme.colors.foreground)
        .hover(|s| s.bg(theme.colors.list_hover))
        .on_click(move |_, window, app| on_click(window, app))
        .child(label)
}

fn reveal_in_explorer(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(not(target_os = "windows"))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

// ==================== 覆盖对话框 ====================

/// 覆盖层对话框集合。
pub fn render_overlays(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) -> Option<Div> {
    let dialog = shared.project_ui.borrow().dialog.as_ref().map(kind_of)?;
    let theme = cx.theme().clone();

    let body: Div = match dialog {
        DialogKind::Create => create_dialog(shared, inputs, entity, cx, &theme),
        DialogKind::OpenFolder => open_folder_body(shared, inputs, entity, cx, &theme),
        DialogKind::ConfirmDelete => delete_body(shared, inputs, entity, cx, &theme),
        DialogKind::Relocate => relocate_body(shared, inputs, entity, cx, &theme),
        DialogKind::LockBusy => lock_body(shared, entity, cx, &theme),
        DialogKind::Unsaved => unsaved_body(shared, entity, cx, &theme),
    };

    Some(
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.colors.overlay)
            .child(body),
    )
}

/// 对话框种类（避免借用冲突，仅取 tag）。
#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    Create,
    OpenFolder,
    ConfirmDelete,
    Relocate,
    LockBusy,
    Unsaved,
}

fn kind_of(d: &ProjectDialog) -> DialogKind {
    match d {
        ProjectDialog::Create { .. } => DialogKind::Create,
        ProjectDialog::OpenFolder { .. } => DialogKind::OpenFolder,
        ProjectDialog::ConfirmDelete { .. } => DialogKind::ConfirmDelete,
        ProjectDialog::Relocate { .. } => DialogKind::Relocate,
        ProjectDialog::LockBusy { .. } => DialogKind::LockBusy,
        ProjectDialog::Unsaved { .. } => DialogKind::Unsaved,
    }
}

fn dialog_shell(theme: &gpui_kit::component::Theme, title: &str, body: Div, foot: Div) -> Div {
    div()
        .w(px(520.))
        .v_flex()
        .rounded_lg()
        .border_1()
        .border_color(theme.colors.border)
        .bg(theme.colors.popover)
        .shadow_lg()
        .child(
            div()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(theme.colors.border)
                .text_sm()
                .font_weight(FontWeight::MEDIUM)
                .child(title.to_string()),
        )
        .child(div().p_3().v_flex().gap_2().child(body))
        .child(
            div()
                .h_flex()
                .justify_end()
                .gap_2()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(theme.colors.border)
                .child(foot),
        )
}

fn create_dialog(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    _cx: &mut App,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let error = match &shared.project_ui.borrow().dialog {
        Some(ProjectDialog::Create { error }) => error.clone(),
        _ => None,
    };
    let shared_ok = shared.clone();
    let entity_ok = entity.clone();
    let inputs_ok = inputs.clone();
    let shared_cancel = shared.clone();
    let entity_cancel = entity.clone();

    let body = div()
        .v_flex()
        .gap_2()
        .child(label(theme, "项目名称 *"))
        .child(Input::new(&inputs.create_name))
        .child(label(theme, "位置 *"))
        .child(Input::new(&inputs.create_location))
        .child(label(theme, "描述（可选）"))
        .child(Input::new(&inputs.create_desc))
        .when_some(error, |d, e| {
            d.child(div().text_xs().text_color(theme.colors.danger).child(e))
        });

    let foot = div()
        .h_flex()
        .gap_2()
        .child(
            Button::new("create-cancel")
                .secondary()
                .label("取消")
                .on_click(move |_, _, app| cancel_dialog(&shared_cancel, &entity_cancel, app)),
        )
        .child(
            Button::new("create-ok")
                .primary()
                .label("创建项目")
                .on_click(move |_, _, app| submit_create(&shared_ok, &inputs_ok, &entity_ok, app)),
        );

    dialog_shell(theme, "新建项目", body, foot)
}

fn open_folder_body(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    _cx: &mut App,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let error = match &shared.project_ui.borrow().dialog {
        Some(ProjectDialog::OpenFolder { error, .. }) => error.clone(),
        _ => None,
    };
    let shared_ok = shared.clone();
    let entity_ok = entity.clone();
    let inputs_ok = inputs.clone();
    let shared_cancel = shared.clone();
    let entity_cancel = entity.clone();

    let body = div()
        .v_flex()
        .gap_2()
        .child(label(theme, "项目目录（含 .RSmeta）"))
        .child(Input::new(&inputs.create_location))
        .when_some(error, |d, e| {
            d.child(div().text_xs().text_color(theme.colors.danger).child(e))
        });
    let foot = div()
        .h_flex()
        .gap_2()
        .child(
            Button::new("of-cancel")
                .secondary()
                .label("取消")
                .on_click(move |_, _, app| cancel_dialog(&shared_cancel, &entity_cancel, app)),
        )
        .child(
            Button::new("of-ok")
                .primary()
                .label("打开")
                .on_click(move |_, _, app| {
                    submit_open_folder(&shared_ok, &inputs_ok, &entity_ok, app)
                }),
        );
    dialog_shell(theme, "打开现有项目", body, foot)
}

fn delete_body(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    _cx: &mut App,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let (name, root, error) = match &shared.project_ui.borrow().dialog {
        Some(ProjectDialog::ConfirmDelete {
            name, root, error, ..
        }) => (name.clone(), root.clone(), error.clone()),
        _ => (String::new(), PathBuf::new(), None),
    };
    let shared_ok = shared.clone();
    let entity_ok = entity.clone();
    let inputs_ok = inputs.clone();
    let shared_cancel = shared.clone();
    let entity_cancel = entity.clone();

    let body = div()
        .v_flex()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.danger)
                .child(format!(
                    "将删除项目内部元数据（{}），用户文件保留。",
                    ".RSmeta"
                )),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(root.to_string_lossy().to_string()),
        )
        .child(label(theme, &format!("输入项目名「{name}」以确认")))
        .child(Input::new(&inputs.delete_confirm))
        .when_some(error, |d, e| {
            d.child(div().text_xs().text_color(theme.colors.danger).child(e))
        });
    let foot = div()
        .h_flex()
        .gap_2()
        .child(
            Button::new("del-cancel")
                .secondary()
                .label("取消")
                .on_click(move |_, _, app| cancel_dialog(&shared_cancel, &entity_cancel, app)),
        )
        .child(
            Button::new("del-ok")
                .secondary()
                .label("删除")
                .on_click(move |_, _, app| confirm_delete(&shared_ok, &inputs_ok, &entity_ok, app)),
        );
    dialog_shell(theme, "删除项目数据", body, foot)
}

fn relocate_body(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    _cx: &mut App,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let (name, error) = match &shared.project_ui.borrow().dialog {
        Some(ProjectDialog::Relocate { name, error, .. }) => (name.clone(), error.clone()),
        _ => (String::new(), None),
    };
    let shared_ok = shared.clone();
    let entity_ok = entity.clone();
    let inputs_ok = inputs.clone();
    let shared_cancel = shared.clone();
    let entity_cancel = entity.clone();

    let body = div()
        .v_flex()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(format!("项目「{name}」原路径已失效，请指定新目录。")),
        )
        .child(label(theme, "新的项目目录（含 .RSmeta）"))
        .child(Input::new(&inputs.create_location))
        .when_some(error, |d, e| {
            d.child(div().text_xs().text_color(theme.colors.danger).child(e))
        });
    let foot = div()
        .h_flex()
        .gap_2()
        .child(
            Button::new("rl-cancel")
                .secondary()
                .label("取消")
                .on_click(move |_, _, app| cancel_dialog(&shared_cancel, &entity_cancel, app)),
        )
        .child(
            Button::new("rl-ok")
                .primary()
                .label("重新定位")
                .on_click(move |_, _, app| {
                    submit_relocate(&shared_ok, &inputs_ok, &entity_ok, app)
                }),
        );
    dialog_shell(theme, "重新定位项目", body, foot)
}

fn lock_body(
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    _cx: &mut App,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let (name, root, pid) = match &shared.project_ui.borrow().dialog {
        Some(ProjectDialog::LockBusy { name, root, pid }) => (name.clone(), root.clone(), *pid),
        _ => (String::new(), PathBuf::new(), 0),
    };
    let shared_ro = shared.clone();
    let entity_ro = entity.clone();
    let root_ro = root.clone();
    let shared_force = shared.clone();
    let entity_force = entity.clone();
    let root_force = root.clone();
    let shared_cancel = shared.clone();
    let entity_cancel = entity.clone();

    let body = div()
        .v_flex()
        .gap_2()
        .child(
            div()
                .text_sm()
                .child(format!("项目「{name}」正被另一实例占用（pid {pid}）。")),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("可只读打开（不写锁），或仍要打开（仅在确认对方已退出时使用）。"),
        );
    let foot = div()
        .h_flex()
        .gap_2()
        .child(
            Button::new("lock-cancel")
                .secondary()
                .label("取消")
                .on_click(move |_, _, app| cancel_dialog(&shared_cancel, &entity_cancel, app)),
        )
        .child(
            Button::new("lock-ro")
                .secondary()
                .label("只读打开")
                .on_click(move |_, _, app| {
                    let path = root_ro.clone();
                    shared_ro.project_ui.borrow_mut().dialog = None;
                    open_read_only(&shared_ro, &path, &entity_ro, app);
                }),
        )
        .child(
            Button::new("lock-force")
                .primary()
                .label("仍要打开")
                .on_click(move |_, _, app| {
                    let path = root_force.clone();
                    // 「仍要打开」：删除陈旧锁文件后重新抢锁（对方已退出场景）。
                    let _ = std::fs::remove_file(project::ProjectLock::lock_path(&path));
                    shared_force.project_ui.borrow_mut().dialog = None;
                    open_path(&shared_force, &path, &entity_force, app);
                }),
        );
    dialog_shell(theme, "项目已在另一实例打开", body, foot)
}

fn unsaved_body(
    shared: &Shared,
    entity: &Entity<WorkbenchView>,
    _cx: &mut App,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let shared_discard = shared.clone();
    let entity_discard = entity.clone();
    let shared_save = shared.clone();
    let entity_save = entity.clone();
    let shared_cancel = shared.clone();
    let entity_cancel = entity.clone();

    let body = div()
        .v_flex()
        .gap_1()
        .child(div().text_sm().child("编辑区存在未保存的 SQL 草稿。"))
        .child(
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("「保存并继续」会另存为项目草稿文件；放弃更改则直接切换 / 关闭。"),
        );
    let foot = div()
        .h_flex()
        .gap_2()
        .child(
            Button::new("unsaved-cancel")
                .secondary()
                .label("取消")
                .on_click(move |_, _, app| cancel_dialog(&shared_cancel, &entity_cancel, app)),
        )
        .child(
            Button::new("unsaved-discard")
                .secondary()
                .label("放弃更改并继续")
                .on_click(move |_, window, app| {
                    resolve_unsaved(&shared_discard, false, &entity_discard, window, app)
                }),
        )
        .child(
            Button::new("unsaved-save")
                .primary()
                .label("保存并继续")
                .on_click(move |_, window, app| {
                    resolve_unsaved(&shared_save, true, &entity_save, window, app)
                }),
        );
    dialog_shell(theme, "未保存的草稿", body, foot)
}

fn label(theme: &gpui_kit::component::Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

// ==================== 项目设置 ====================

/// 项目设置（覆盖中央区，Tab 式）。
pub fn render_settings(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) -> Option<Div> {
    if !shared.project_ui.borrow().settings_open {
        return None;
    }
    let (name, root) = {
        let p = shared.project.borrow();
        match p.as_ref() {
            Some(s) => (s.name.clone(), s.root.clone()),
            None => return None,
        }
    };
    let theme = cx.theme();
    let read_only = shared.project_ui.borrow().read_only;

    // 元数据文件大小
    let meta = root.join(project_service::RS_META_DIR_NAME);
    let mut files = div().v_flex().gap_1().text_xs();
    for entry in [
        "project.db",
        "analytics.duckdb",
        "project.json",
        "project.lock",
    ] {
        let p = meta.join(entry);
        let size = std::fs::metadata(&p)
            .map(|m| format!("{} B", m.len()))
            .unwrap_or_else(|_| "—".to_string());
        files = files.child(
            div()
                .h_flex()
                .gap_2()
                .child(
                    div()
                        .text_color(theme.colors.foreground)
                        .child(entry.to_string()),
                )
                .child(
                    div()
                        .ml_auto()
                        .text_color(theme.colors.muted_foreground)
                        .child(size),
                ),
        );
    }

    let shared_close = shared.clone();
    let entity_close = entity.clone();
    let shared_reveal = root.clone();
    let shared_reload = shared.clone();
    let entity_reload = entity.clone();
    let inputs_rename = inputs.clone();
    let shared_rename = shared.clone();
    let entity_rename = entity.clone();
    let inputs_version = inputs.clone();
    let shared_version = shared.clone();
    let entity_version = entity.clone();

    let deps = {
        let missing = project_service::list_recent(12)
            .ok()
            .and_then(|v| v.into_iter().find(|p| p.path == root))
            .map(|p| p.missing_drivers)
            .unwrap_or_default();
        if missing.is_empty() {
            div()
                .text_xs()
                .text_color(theme.colors.success)
                .child("依赖自检：未发现缺失驱动")
        } else {
            div()
                .text_xs()
                .text_color(theme.colors.warning)
                .child(format!("⚠ 缺失驱动：{}", missing.join("、")))
        }
    };

    Some(
        div()
            .absolute()
            .inset_0()
            .v_flex()
            .bg(theme.colors.background)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.colors.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("项目设置 · {name}")),
                    )
                    .when(read_only, |d| {
                        d.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.warning)
                                .child("只读"),
                        )
                    })
                    .child(
                        div()
                            .ml_auto()
                            .id("proj-settings-close")
                            .cursor_pointer()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .on_click(move |_, _, app| {
                                shared_close.project_ui.borrow_mut().settings_open = false;
                                entity_close.update(app, |_, cx| cx.notify());
                            })
                            .child("关闭"),
                    ),
            )
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .p_4()
                    .child(section(theme, "概览"))
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .text_xs()
                            .child(kv(theme, "名称", &name))
                            .child(kv(theme, "路径", &root.to_string_lossy()))
                            .child(kv(theme, "状态", if read_only { "只读" } else { "可写" })),
                    )
                    .child(label(theme, "名称（重命名）"))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&inputs.rename)))
                            .child(
                                Button::new("proj-rename-save")
                                    .secondary()
                                    .label("保存名称")
                                    .on_click(move |_, _, app| {
                                        save_rename(
                                            &shared_rename,
                                            &inputs_rename,
                                            &entity_rename,
                                            app,
                                        )
                                    }),
                            ),
                    )
                    .child(section(theme, "存储（.RSmeta）"))
                    .child(files)
                    .child(
                        Button::new("proj-open-meta")
                            .secondary()
                            .label("🗀 打开 .RSmeta")
                            .on_click(move |_, _, _| reveal_in_explorer(&shared_reveal)),
                    )
                    .child(section(theme, "依赖"))
                    .child(deps)
                    .child(section(theme, "版本"))
                    .child(versions_view(&root, theme))
                    .child(
                        div()
                            .h_flex()
                            .gap_2()
                            .child(div().flex_1().child(Input::new(&inputs.version_msg)))
                            .child(
                                Button::new("proj-version-add")
                                    .secondary()
                                    .label("创建版本快照")
                                    .on_click(move |_, _, app| {
                                        create_version_action(
                                            &shared_version,
                                            &inputs_version,
                                            &entity_version,
                                            app,
                                        )
                                    }),
                            ),
                    )
                    .child(section(theme, "危险区"))
                    .child(
                        Button::new("proj-reload")
                            .secondary()
                            .label("刷新列表")
                            .on_click(move |_, _, app| {
                                refresh_picker(&shared_reload, &entity_reload, app);
                            }),
                    ),
            ),
    )
}

fn section(theme: &gpui_kit::component::Theme, title: &str) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.colors.muted_foreground)
        .child(title.to_string())
}

/// 项目版本列表（只读；失败降级为提示行）。
fn versions_view(root: &std::path::Path, theme: &gpui_kit::component::Theme) -> Div {
    let mut list = div().v_flex().gap_1().text_xs();
    match project_service::list_versions(root) {
        Ok(versions) if versions.is_empty() => {
            list = list.child(
                div()
                    .text_color(theme.colors.muted_foreground)
                    .child("暂无版本记录"),
            );
        }
        Ok(versions) => {
            for v in versions {
                list = list.child(
                    div()
                        .h_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_color(theme.colors.muted_foreground)
                                .child(v.created_at),
                        )
                        .child(div().text_color(theme.colors.foreground).child(v.message)),
                );
            }
        }
        Err(e) => {
            list = list.child(div().text_color(theme.colors.warning).child(e));
        }
    }
    list
}

/// 创建版本快照（只读模式拦截）。
pub fn create_version_action(
    shared: &Shared,
    inputs: &ProjectInputs,
    entity: &Entity<WorkbenchView>,
    cx: &mut App,
) {
    if read_only_blocked(shared, entity, cx, "创建版本") {
        return;
    }
    let root = match shared.project.borrow().as_ref().map(|s| s.root.clone()) {
        Some(r) => r,
        None => return,
    };
    let message = inputs.version_msg.read(cx).value().trim().to_string();
    match project_service::create_version(&root, &message) {
        Ok(()) => shared.project_ui.borrow_mut().notice = Some("已创建版本快照".to_string()),
        Err(e) => shared.project_ui.borrow_mut().notice = Some(e),
    }
    entity.update(cx, |_, cx| cx.notify());
}

fn kv(theme: &gpui_kit::component::Theme, key: &str, value: &str) -> Div {
    div()
        .h_flex()
        .gap_2()
        .child(
            div()
                .text_color(theme.colors.muted_foreground)
                .child(key.to_string()),
        )
        .child(
            div()
                .text_color(theme.colors.foreground)
                .child(value.to_string()),
        )
}

// 供 `render_picker` 内部构造不完整 `ProjectInputs` 时使用的桥接（仅取用到的实体）。
