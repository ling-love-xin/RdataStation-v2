//! 项目管理视图（M1）：选择器 / 标题栏项目菜单 / 项目设置 / 语义对话框。
//!
//! 本模块随 `project` crate 一同发布（GPUI-kit 指南：model / service / view 同 crate），
//! 不依赖 workbench 与 settings——宿主（工作台）通过 [`ProjectUiHost`] 注入状态句柄、
//! 重绘桥、编辑区桥、排序偏好与打开后回调。
//!
//! 选择器与项目设置由宿主渲染为受控叠加层（宿主 render 是权威同步点）；项目菜单用
//! `Popover`，创建 / 打开 / 删除 / 重定位 / 锁占用 / 未保存拦截一律用语义组件
//! `Dialog` / `AlertDialog`（焦点陷阱、Escape、点击遮罩关闭由组件负责）。
//!
//! 语义见 `docs/architecture/project/project-prototype-design.md`。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::ActiveTheme;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, IconName, Sizable as _, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::service::{self as project_service, OpenOutcome, ProjectSummary, TargetDirState};

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
}

impl Default for PickerState {
    fn default() -> Self {
        Self {
            tab: PickerTab::Recent,
            sort: ProjectSort::LastOpened,
            items: Vec::new(),
            loaded: false,
            error: None,
        }
    }
}

/// 待处理的项目动作（未保存草稿拦截用）。
#[derive(Debug, Clone)]
pub enum PendingAction {
    OpenPath(PathBuf),
    Close,
}

// ==================== 宿主桥 ====================

/// 当前打开的项目（根目录 + 显示名）。
///
/// 宿主（工作台）与项目视图共用；命名沿用产品语义「一个实例一个项目」。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OpenProject {
    pub root: PathBuf,
    pub name: String,
}

impl OpenProject {
    pub fn new(root: PathBuf, name: impl Into<String>) -> Self {
        Self {
            root,
            name: name.into(),
        }
    }

    /// 由根目录推导显示名（取末级目录名，空则「未命名项目」）。
    pub fn from_root(root: PathBuf) -> Self {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "未命名项目".to_string());
        Self { root, name }
    }
}

/// 宿主重绘桥（宿主把自身视图实体的 notify 注入）。
pub trait ProjectUiNotifier: 'static {
    fn notify(&self, cx: &mut App);
}

/// 编辑区桥：未保存草稿判定 / 草稿内容 / 清空编辑区。
pub trait ProjectEditorBridge: 'static {
    fn is_dirty(&self) -> bool;
    fn sql(&self) -> String;
    fn clear(&self, window: &mut Window, cx: &mut App);
    /// 清除脏标记（打开 / 关闭项目、草稿落盘后）。
    fn mark_clean(&self);
}

/// 未接编辑区的空实现（测试 / 无编辑区宿主）。
impl ProjectEditorBridge for () {
    fn is_dirty(&self) -> bool {
        false
    }

    fn sql(&self) -> String {
        String::new()
    }

    fn clear(&self, _window: &mut Window, _cx: &mut App) {}

    fn mark_clean(&self) {}
}

/// 宿主注入的能力集合：项目视图的全部外部依赖都从这里取，
/// `project` crate 因而不依赖 workbench 与 settings。
#[derive(Clone)]
#[non_exhaustive]
pub struct ProjectUiHost {
    /// 项目 UI 状态（选择器 / 菜单 / 设置 / 对话框错误 / 锁 / 只读 / 世代）。
    pub state: Rc<RefCell<ProjectUiState>>,
    /// 当前会话（根 + 名称）。
    pub session: Rc<RefCell<Option<OpenProject>>>,
    /// 重绘宿主视图。
    pub notifier: Rc<dyn ProjectUiNotifier>,
    /// 编辑区桥。
    pub editor: Rc<dyn ProjectEditorBridge>,
    /// 排序偏好持久化（宿主接设置服务）。
    pub save_sort: Rc<dyn Fn(ProjectSort, &mut App)>,
    /// 打开项目后的宿主刷新（连接列表 / 导航缓存 / 结果归属）。
    pub on_opened: Rc<dyn Fn(&mut App)>,
}

impl ProjectUiHost {
    /// 最小构造：宿主只需提供状态与重绘时使用；其余能力用 `with_*` 补。
    pub fn new(
        state: Rc<RefCell<ProjectUiState>>,
        session: Rc<RefCell<Option<OpenProject>>>,
        notifier: Rc<dyn ProjectUiNotifier>,
    ) -> Self {
        Self {
            state,
            session,
            notifier,
            editor: Rc::new(()),
            save_sort: Rc::new(|_, _| {}),
            on_opened: Rc::new(|_| {}),
        }
    }

    pub fn with_editor(mut self, editor: Rc<dyn ProjectEditorBridge>) -> Self {
        self.editor = editor;
        self
    }

    pub fn with_sort_saver(mut self, save_sort: Rc<dyn Fn(ProjectSort, &mut App)>) -> Self {
        self.save_sort = save_sort;
        self
    }

    pub fn with_on_opened(mut self, on_opened: Rc<dyn Fn(&mut App)>) -> Self {
        self.on_opened = on_opened;
        self
    }

    /// 请求宿主重绘。
    pub fn notify(&self, cx: &mut App) {
        self.notifier.notify(cx);
    }

    /// 当前会话快照。
    pub fn current(&self) -> Option<OpenProject> {
        self.session.borrow().clone()
    }

    /// 记录当前会话。
    pub fn set_current(&self, project: Option<OpenProject>) {
        *self.session.borrow_mut() = project;
    }

    /// 当前项目根目录（未打开时为 `None`）。
    pub fn root(&self) -> Option<PathBuf> {
        self.session.borrow().as_ref().map(|s| s.root.clone())
    }
}

/// 项目 UI 状态（挂在宿主共享状态与 [`ProjectUiHost`] 上）。
#[non_exhaustive]
pub struct ProjectUiState {
    pub picker: PickerState,
    pub menu_open: bool,
    pub settings_open: bool,
    /// 当前对话框的校验错误：对话框 builder 每帧读取，提交失败时写入并重绘。
    pub dialog_error: Option<String>,
    /// 当前持有的项目写锁（只读打开时为 `None`）。
    pub lock: Option<crate::ProjectLock>,
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
            dialog_error: None,
            lock: None,
            read_only: false,
            epoch: 0,
            notice: None,
        }
    }
}

/// 项目相关输入实体（由宿主懒创建并持有）。
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
pub fn refresh_picker(host: &ProjectUiHost, cx: &mut App) {
    load_picker(host);
    host.notify(cx);
}

/// 加载列表（不 notify；供 render 首帧调用，避免 render 内 notify 循环）。
pub fn load_picker(host: &ProjectUiHost) {
    let (tab, sort) = {
        let ui = host.state.borrow();
        (ui.picker.tab, ui.picker.sort)
    };
    let result = match tab {
        PickerTab::Recent => project_service::list_recent(12),
        PickerTab::All => project_service::list_all(),
        PickerTab::Removed => project_service::list_removed(),
    };

    let mut ui = host.state.borrow_mut();
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
pub fn set_tab(host: &ProjectUiHost, tab: PickerTab, cx: &mut App) {
    host.state.borrow_mut().picker.tab = tab;
    refresh_picker(host, cx);
}

/// 循环切换排序（交宿主持久化）并刷新。
pub fn cycle_sort(host: &ProjectUiHost, cx: &mut App) {
    let next = host.state.borrow().picker.sort.next();
    host.state.borrow_mut().picker.sort = next;
    (host.save_sort)(next, cx);
    refresh_picker(host, cx);
}

// ==================== 语义对话框 ====================

/// 表单字段标签。
fn label(theme: &gpui_kit::component::Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 对话框内联错误（校验失败时显示）。
fn dialog_error_line(theme: &gpui_kit::component::Theme, message: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.danger)
        .child(message.to_string())
}

/// 打开系统目录选择器，把选中的目录回填到输入框（取消则保持原值）。
///
/// `App::prompt_for_paths` 通过 oneshot 异步回传结果；这里用 `Window::spawn` 在窗口上下文
/// 等待，再用 `AsyncWindowContext::update` 取得 `&mut Window` 回填（`set_value` 需要窗口）。
fn pick_directory(target: Entity<InputState>, window: &mut Window, cx: &mut App) {
    let receiver = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("选择目录".into()),
    });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let value = path.to_string_lossy().to_string();
            let _ = cx.update(|window, cx| {
                target.update(cx, |state, cx| state.set_value(value, window, cx));
            });
        })
        .detach();
}

/// 目录输入行：文本框 + 「浏览…」按钮（系统目录选择器）。
fn directory_row(target: &Entity<InputState>, browse_id: &'static str) -> Div {
    let picker_target = target.clone();
    div()
        .h_flex()
        .gap_2()
        .child(div().flex_1().child(Input::new(target)))
        .child(
            Button::new(browse_id)
                .secondary()
                .label("浏览…")
                .on_click(move |_, window, cx| pick_directory(picker_target.clone(), window, cx)),
        )
}

/// 新建项目的目标路径预览（原型要求展示 `位置/名称`）。
fn target_preview(theme: &gpui_kit::component::Theme, location: &str, name: &str) -> Div {
    let (location, name) = (location.trim(), name.trim());
    let text = if location.is_empty() || name.is_empty() {
        "目标：填写位置与名称后在此预览".to_string()
    } else {
        format!("目标：{}", PathBuf::from(location).join(name).display())
    };
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text)
}

/// 对话框底部按钮组（右对齐）：取消 + 主操作。
///
/// 关闭时机由回调自行决定（`window.close_dialog`），不使用 `Dialog::on_ok` 的返回值——
/// 这样「提交成功才关闭」与「先关掉本对话框再执行可能另开对话框的动作」两种路径行为一致。
fn dialog_footer(
    cancel_id: &'static str,
    ok_id: &'static str,
    ok_label: &'static str,
    ok_variant: ButtonVariant,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
) -> DialogFooter {
    DialogFooter::new()
        .child(
            Button::new(cancel_id)
                .secondary()
                .label("取消")
                .on_click(move |_, window, cx| on_cancel(window, cx)),
        )
        .child(
            Button::new(ok_id)
                .with_variant(ok_variant)
                .label(ok_label)
                .on_click(move |_, window, cx| on_ok(window, cx)),
        )
}

/// 三选一底部按钮组（锁逃生口 / 未保存拦截）：取消 + 次要逃生 + 主操作。
fn dialog_footer_three(
    cancel_id: &'static str,
    alt_id: &'static str,
    alt_label: &'static str,
    ok_id: &'static str,
    ok_label: &'static str,
    on_alt: impl Fn(&mut Window, &mut App) + 'static,
    on_ok: impl Fn(&mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut Window, &mut App) + 'static,
) -> DialogFooter {
    DialogFooter::new()
        .child(
            Button::new(cancel_id)
                .secondary()
                .label("取消")
                .on_click(move |_, window, cx| on_cancel(window, cx)),
        )
        .child(
            Button::new(alt_id)
                .secondary()
                .label(alt_label)
                .on_click(move |_, window, cx| on_alt(window, cx)),
        )
        .child(
            Button::new(ok_id)
                .primary()
                .label(ok_label)
                .on_click(move |_, window, cx| on_ok(window, cx)),
        )
}

/// 打开新建项目对话框（重置表单）。
pub fn open_create_dialog(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
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
    host.state.borrow_mut().dialog_error = None;

    let host = host.clone();
    let inputs = inputs.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        let location = inputs.create_location.read(cx).value().to_string();
        let name = inputs.create_name.read(cx).value().to_string();
        dialog
            .title("新建项目")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(label(theme, "项目名称 *"))
                    .child(Input::new(&inputs.create_name))
                    .child(label(theme, "位置 *"))
                    .child(directory_row(&inputs.create_location, "create-browse"))
                    .child(target_preview(theme, &location, &name))
                    .child(label(theme, "描述（可选）"))
                    .child(Input::new(&inputs.create_desc))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("当前版本仅支持本地目录；DuckLake 远程项目待后续版本。"),
                    )
                    .when_some(error, |d, e| d.child(dialog_error_line(theme, &e))),
            )
            .footer(dialog_footer(
                "create-cancel",
                "create-ok",
                "创建项目",
                ButtonVariant::Primary,
                {
                    let host = host.clone();
                    let inputs = inputs.clone();
                    move |window, cx| {
                        if submit_create(&host, &inputs, window, cx) {
                            window.close_dialog(cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
            .on_ok({
                let host = host.clone();
                let inputs = inputs.clone();
                move |_, window, cx| {
                    // Enter 与「创建项目」按钮同路径；关闭由提交成功分支显式执行。
                    if submit_create(&host, &inputs, window, cx) {
                        window.close_dialog(cx);
                    }
                    false
                }
            })
    });
}

/// 打开「打开现有目录」对话框。
pub fn open_folder_dialog(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .create_location
        .update(cx, |s, cx| s.set_value(default_location(), window, cx));
    host.state.borrow_mut().dialog_error = None;

    let host = host.clone();
    let inputs = inputs.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        dialog
            .title("打开现有项目")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(label(theme, "项目目录（含 .RSmeta）"))
                    .child(directory_row(&inputs.create_location, "of-browse"))
                    .when_some(error, |d, e| d.child(dialog_error_line(theme, &e))),
            )
            .footer(dialog_footer(
                "of-cancel",
                "of-ok",
                "打开",
                ButtonVariant::Primary,
                {
                    let host = host.clone();
                    let inputs = inputs.clone();
                    move |window, cx| {
                        if submit_open_folder(&host, &inputs, window, cx) {
                            window.close_dialog(cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
            .on_ok({
                let host = host.clone();
                let inputs = inputs.clone();
                move |_, window, cx| {
                    if submit_open_folder(&host, &inputs, window, cx) {
                        window.close_dialog(cx);
                    }
                    false
                }
            })
    });
}

/// 从输入读取并创建项目；成功返回 `true`（由调用方关闭对话框）。
pub fn submit_create(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let name = inputs.create_name.read(cx).value().trim().to_string();
    let location = inputs.create_location.read(cx).value().trim().to_string();
    let description = {
        let d = inputs.create_desc.read(cx).value().trim().to_string();
        (!d.is_empty()).then_some(d)
    };

    if let Err(e) = project_service::validate_project_name(&name) {
        set_dialog_error(host, e, cx);
        return false;
    }
    if location.is_empty() {
        set_dialog_error(host, "请选择项目位置".to_string(), cx);
        return false;
    }
    let path = PathBuf::from(&location).join(&name);

    let input = project_service::CreateProjectInput::new(name, path).with_description(description);
    match project_service::create(input) {
        Ok(summary) => {
            // 创建成功后直接打开（走统一打开路径，取得锁）。
            open_path(host, &summary.path, window, cx);
            true
        }
        Err(e) => {
            set_dialog_error(host, e, cx);
            false
        }
    }
}

/// 从输入读取并打开目录；成功返回 `true`（由调用方关闭对话框）。
pub fn submit_open_folder(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let path = PathBuf::from(inputs.create_location.read(cx).value().trim().to_string());
    if path.as_os_str().is_empty() {
        set_dialog_error(host, "请输入项目目录".to_string(), cx);
        return false;
    }
    match project_service::inspect_target(&path) {
        TargetDirState::Missing => {
            set_dialog_error(host, format!("目录不存在：{}", path.display()), cx);
            false
        }
        TargetDirState::NonEmpty => {
            set_dialog_error(
                host,
                "目录不是项目（缺少 .RSmeta），且非空，无法作为项目打开".to_string(),
                cx,
            );
            false
        }
        TargetDirState::Empty => {
            // 空目录不视为项目；提示改用「新建项目」。
            set_dialog_error(host, "空目录：请改用「新建项目」".to_string(), cx);
            false
        }
        TargetDirState::ExistingProject => {
            open_path(host, &path, window, cx);
            true
        }
    }
}

/// 内置示例项目的落点：系统数据目录旁的 `samples/示例项目`。
fn sample_project_dir() -> PathBuf {
    let system = engine::migration::get_system_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("RdataStation").join("system"));
    system
        .parent()
        .map(|p| p.join("samples"))
        .unwrap_or_else(|| std::env::temp_dir().join("RdataStation").join("samples"))
        .join("示例项目")
}

/// 从示例项目开始（内置最小示例：示例 SQL 草稿）。
pub fn create_sample(host: &ProjectUiHost, window: &mut Window, cx: &mut App) {
    let path = sample_project_dir();
    if !project_service::is_valid_project(&path) {
        let input = project_service::CreateProjectInput::new("示例项目", path.clone())
            .with_description(Some("RdataStation 内置示例".to_string()))
            .with_sample_drafts(true);
        if let Err(e) = project_service::create(input) {
            host.state.borrow_mut().picker.error = Some(e);
            host.notify(cx);
            return;
        }
        // 写入示例草稿（失败不阻断打开）。
        let _ = std::fs::write(
            path.join("welcome.sql"),
            "-- RdataStation 示例项目\n-- 在此编写并执行 SQL。\nSELECT 1 AS hello;\n",
        );
    }
    open_path(host, &path, window, cx);
}

/// 打开项目（含未保存拦截 + 锁占用分支）。
pub fn request_open(host: &ProjectUiHost, path: &PathBuf, window: &mut Window, cx: &mut App) {
    if host.editor.is_dirty() {
        open_unsaved_dialog(host, PendingAction::OpenPath(path.clone()), window, cx);
        return;
    }
    open_path(host, path, window, cx);
}

/// 直接打开（不再拦截）。
pub fn open_path(host: &ProjectUiHost, path: &PathBuf, window: &mut Window, cx: &mut App) {
    match project_service::open(path) {
        Ok(OpenOutcome::Opened(opened)) => apply_opened(host, opened, cx),
        Ok(OpenOutcome::Busy(info)) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "项目".to_string());
            open_lock_busy_dialog(host, name, path.clone(), info.pid, window, cx);
        }
        Err(e) => {
            host.state.borrow_mut().picker.error = Some(e);
            host.notify(cx);
        }
    }
}

/// 锁占用逃生口：只读打开 / 仍要打开 / 取消。
pub fn open_lock_busy_dialog(
    host: &ProjectUiHost,
    name: String,
    root: PathBuf,
    pid: u32,
    window: &mut Window,
    cx: &mut App,
) {
    host.state.borrow_mut().dialog_error = None;
    let host = host.clone();
    window.open_alert_dialog(cx, move |alert, _window, cx| {
        let theme = cx.theme();
        let host_ro = host.clone();
        let root_ro = root.clone();
        let host_force = host.clone();
        let root_force = root.clone();
        alert
            .icon(Icon::new(IconName::TriangleAlert).text_color(theme.colors.warning))
            .title(format!("项目「{name}」已在另一实例打开"))
            .description(format!(
                "另一进程（pid {pid}）持有写锁。可只读打开；仅在确认对方已退出时，才选择「仍要打开」。"
            ))
            .footer(dialog_footer_three(
                "lock-cancel",
                "lock-force",
                "仍要打开",
                "lock-ro",
                "只读打开",
                move |window, cx| {
                    window.close_dialog(cx);
                    // 「仍要打开」：清除陈旧锁文件后重新抢锁（对方已退出的场景）。
                    let _ = std::fs::remove_file(crate::ProjectLock::lock_path(&root_force));
                    open_path(&host_force, &root_force, window, cx);
                },
                move |window, cx| {
                    window.close_dialog(cx);
                    open_read_only(&host_ro, &root_ro, cx);
                },
                |window, cx| window.close_dialog(cx),
            ))
    });
}

/// 未保存草稿拦截：放弃 / 保存并继续 / 取消。
pub fn open_unsaved_dialog(
    host: &ProjectUiHost,
    pending: PendingAction,
    window: &mut Window,
    cx: &mut App,
) {
    host.state.borrow_mut().dialog_error = None;
    let host = host.clone();
    window.open_alert_dialog(cx, move |alert, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        let pending_discard = pending.clone();
        let pending_save = pending.clone();
        alert
            .icon(Icon::new(IconName::TriangleAlert).text_color(theme.colors.warning))
            .title("未保存的草稿")
            .description(
                "编辑区存在未保存的 SQL 草稿。「保存并继续」会另存为项目草稿文件，「放弃更改」则直接执行切换 / 关闭。",
            )
            .when_some(error, |a, e| a.child(dialog_error_line(theme, &e)))
            .footer(dialog_footer_three(
                "unsaved-cancel",
                "unsaved-discard",
                "放弃更改并继续",
                "unsaved-save",
                "保存并继续",
                {
                    let host = host.clone();
                    move |window, cx| {
                        // 先关本对话框再推进动作：推进可能另开对话框（如锁占用），
                        // 否则栈顶变化会让 pop 关错对象。
                        if prepare_unsaved(&host, false, window, cx) {
                            window.close_dialog(cx);
                            advance_pending(&host, &pending_discard, window, cx);
                        }
                    }
                },
                {
                    let host = host.clone();
                    move |window, cx| {
                        if prepare_unsaved(&host, true, window, cx) {
                            window.close_dialog(cx);
                            advance_pending(&host, &pending_save, window, cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
    });
}

/// 只读打开（逃生口）。
pub fn open_read_only(host: &ProjectUiHost, path: &PathBuf, cx: &mut App) {
    match project_service::open_read_only(path) {
        Ok(opened) => apply_opened(host, opened, cx),
        Err(e) => {
            host.state.borrow_mut().picker.error = Some(e);
            host.notify(cx);
        }
    }
}

/// 应用打开结果：写会话 + 持锁 + 自增世代 + 交宿主刷新。
fn apply_opened(host: &ProjectUiHost, opened: project_service::OpenedProject, cx: &mut App) {
    let (store, lock, read_only, summary) = opened.into_parts();
    // store 目前仅用于确认加载成功；会话只保留根与名（与既有 P0 会话一致）。
    drop(store);

    host.set_current(Some(OpenProject::new(
        summary.path.clone(),
        summary.name.clone(),
    )));

    {
        let mut ui = host.state.borrow_mut();
        ui.lock = lock;
        ui.read_only = read_only;
        ui.epoch += 1;
        ui.menu_open = false;
        ui.settings_open = false;
        ui.dialog_error = None;
        ui.notice = read_only.then(|| "只读打开：该项目已被另一实例占用".to_string());
        ui.picker.error = None;
    }

    // 切换项目 → 宿主刷新连接列表 / 导航缓存 / 结果归属，并清掉编辑区脏标记。
    (host.on_opened)(cx);
    host.editor.mark_clean();

    host.notify(cx);
}

/// 请求关闭项目（含未保存拦截）。
pub fn request_close(host: &ProjectUiHost, window: &mut Window, cx: &mut App) {
    if host.editor.is_dirty() {
        open_unsaved_dialog(host, PendingAction::Close, window, cx);
        return;
    }
    do_close(host, cx);
}

/// 执行关闭：释放锁 + 清会话 + 回选择器。
pub fn do_close(host: &ProjectUiHost, cx: &mut App) {
    let lock = host.state.borrow_mut().lock.take();
    if let Some(lock) = lock {
        let _ = lock.release();
    }
    *host.session.borrow_mut() = None;
    {
        let mut ui = host.state.borrow_mut();
        ui.read_only = false;
        ui.epoch += 1;
        ui.menu_open = false;
        ui.settings_open = false;
        ui.dialog_error = None;
        ui.notice = None;
    }
    refresh_picker(host, cx);
}

/// 保存编辑区 SQL 草稿到项目根（唯一命名）。
fn save_draft(host: &ProjectUiHost) -> Result<std::path::PathBuf, String> {
    let root = host.root().ok_or_else(|| "未打开项目".to_string())?;
    let sql = host.editor.sql();
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

/// 未保存拦截的「准备」阶段：按选择另存草稿，并清空编辑区。
///
/// 返回 `true` 表示可以推进后续动作（保存成功或选择放弃）；`false` 时已写入
/// 对话框错误，拦截对话框保持打开。
fn prepare_unsaved(host: &ProjectUiHost, save: bool, window: &mut Window, cx: &mut App) -> bool {
    if save {
        match save_draft(host) {
            Ok(path) => {
                host.state.borrow_mut().notice = Some(format!("草稿已保存：{}", path.display()));
            }
            Err(e) => {
                set_dialog_error(host, e, cx);
                return false;
            }
        }
    }
    // 保存或放弃后都要清空编辑区（命令式，事件上下文；避免下一轮 render 再次判脏）。
    host.editor.clear(window, cx);
    host.editor.mark_clean();
    true
}

/// 未保存拦截的「推进」阶段：执行被拦截的打开 / 关闭动作。
///
/// 必须在拦截对话框已关闭之后调用——推进可能另开对话框（如锁占用）。
fn advance_pending(
    host: &ProjectUiHost,
    pending: &PendingAction,
    window: &mut Window,
    cx: &mut App,
) {
    match pending {
        PendingAction::OpenPath(path) => open_path(host, path, window, cx),
        PendingAction::Close => do_close(host, cx),
    }
}

/// 记录对话框校验错误并重绘（语义对话框 builder 每帧重读）。
fn set_dialog_error(host: &ProjectUiHost, message: String, cx: &mut App) {
    host.state.borrow_mut().dialog_error = Some(message);
    host.notify(cx);
}

/// 固定 / 取消固定。
pub fn toggle_pin(host: &ProjectUiHost, item: &ProjectSummary, cx: &mut App) {
    if let Err(e) = project_service::set_pinned(&item.id, !item.is_pinned) {
        host.state.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(host, cx);
}

/// 软删（移出名册）。
pub fn soft_remove(host: &ProjectUiHost, id: &str, cx: &mut App) {
    if let Err(e) = project_service::soft_remove(id) {
        host.state.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(host, cx);
}

/// 恢复已移除项目。
pub fn restore(host: &ProjectUiHost, id: &str, cx: &mut App) {
    if let Err(e) = project_service::restore(id) {
        host.state.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(host, cx);
}

/// 失效路径项目：移出名册（不动磁盘）。
pub fn forget(host: &ProjectUiHost, id: &str, cx: &mut App) {
    if let Err(e) = project_service::forget(id) {
        host.state.borrow_mut().picker.error = Some(e);
    }
    refresh_picker(host, cx);
}

/// 打开「重新定位」对话框。
pub fn open_relocate_dialog(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .create_location
        .update(cx, |s, cx| s.set_value(String::new(), window, cx));
    host.state.borrow_mut().dialog_error = None;

    let id = item.id.clone();
    let name = item.name.clone();
    let host = host.clone();
    let inputs = inputs.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        dialog
            .title("重新定位项目")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("项目「{name}」原路径已失效，请指定新目录。")),
                    )
                    .child(label(theme, "新的项目目录（含 .RSmeta）"))
                    .child(directory_row(&inputs.create_location, "rl-browse"))
                    .when_some(error, |d, e| d.child(dialog_error_line(theme, &e))),
            )
            .footer(dialog_footer(
                "rl-cancel",
                "rl-ok",
                "重新定位",
                ButtonVariant::Primary,
                {
                    let host = host.clone();
                    let inputs = inputs.clone();
                    let id = id.clone();
                    move |window, cx| {
                        if submit_relocate(&host, &inputs, &id, cx) {
                            window.close_dialog(cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
            .on_ok({
                let host = host.clone();
                let inputs = inputs.clone();
                let id = id.clone();
                move |_, window, cx| {
                    if submit_relocate(&host, &inputs, &id, cx) {
                        window.close_dialog(cx);
                    }
                    false
                }
            })
    });
}

/// 提交重新定位：校验新目录后改写名册与项目路径；成功返回 `true`。
pub fn submit_relocate(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    id: &str,
    cx: &mut App,
) -> bool {
    let new_root = PathBuf::from(inputs.create_location.read(cx).value().trim().to_string());
    if new_root.as_os_str().is_empty() {
        set_dialog_error(host, "请输入新的项目目录".to_string(), cx);
        return false;
    }
    match project_service::relocate(id, &new_root) {
        Ok(()) => {
            refresh_picker(host, cx);
            true
        }
        Err(e) => {
            set_dialog_error(host, e, cx);
            false
        }
    }
}

/// 打开删除确认（输入项目名）。
pub fn open_delete_dialog(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    window: &mut Window,
    cx: &mut App,
) {
    inputs
        .delete_confirm
        .update(cx, |s, cx| s.set_value("", window, cx));
    host.state.borrow_mut().dialog_error = None;

    let id = item.id.clone();
    let name = item.name.clone();
    let root = item.path.clone();
    let host = host.clone();
    let inputs = inputs.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let error = host.state.borrow().dialog_error.clone();
        dialog
            .title("删除项目数据")
            .child(
                div()
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.danger)
                            .child(format!(
                                "将删除项目内部元数据（{}），用户文件保留。",
                                project_service::RS_META_DIR_NAME
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
                    .when_some(error, |d, e| d.child(dialog_error_line(theme, &e))),
            )
            .footer(dialog_footer(
                "del-cancel",
                "del-ok",
                "删除数据",
                ButtonVariant::Danger,
                {
                    let host = host.clone();
                    let inputs = inputs.clone();
                    let id = id.clone();
                    let name = name.clone();
                    let root = root.clone();
                    move |window, cx| {
                        if confirm_delete(&host, &inputs, &id, &name, &root, cx) {
                            window.close_dialog(cx);
                        }
                    }
                },
                |window, cx| window.close_dialog(cx),
            ))
            .on_ok({
                let host = host.clone();
                let inputs = inputs.clone();
                let id = id.clone();
                let name = name.clone();
                let root = root.clone();
                move |_, window, cx| {
                    if confirm_delete(&host, &inputs, &id, &name, &root, cx) {
                        window.close_dialog(cx);
                    }
                    false
                }
            })
    });
}

/// 确认删除磁盘数据（需输入项目名匹配）；成功返回 `true`。
pub fn confirm_delete(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    id: &str,
    name: &str,
    root: &std::path::Path,
    cx: &mut App,
) -> bool {
    let typed = inputs.delete_confirm.read(cx).value().trim().to_string();
    if typed != name {
        set_dialog_error(host, "输入的项目名不匹配".to_string(), cx);
        return false;
    }
    // 若删除的是当前打开项目，先关闭以释放锁。
    let is_current = host.root().map(|r| r == root).unwrap_or(false);
    if is_current {
        do_close(host, cx);
    }
    match project_service::delete_disk(id, root) {
        Ok(()) => {
            refresh_picker(host, cx);
            true
        }
        Err(e) => {
            set_dialog_error(host, e, cx);
            false
        }
    }
}

/// 只读模式守卫：被拦截时写入提示并返回 `true`。
fn read_only_blocked(host: &ProjectUiHost, cx: &mut App, action: &str) -> bool {
    if host.state.borrow().read_only {
        host.state.borrow_mut().notice = Some(format!("只读模式：不允许{action}"));
        host.notify(cx);
        true
    } else {
        false
    }
}

/// 保存项目设置中的重命名。
pub fn save_rename(host: &ProjectUiHost, inputs: &ProjectInputs, cx: &mut App) {
    if read_only_blocked(host, cx, "重命名") {
        return;
    }
    let name = inputs.rename.read(cx).value().trim().to_string();
    let (id, root) = {
        // 通过名册查当前项目 id（会话只存根/名）。
        let root = host.session.borrow().as_ref().map(|s| s.root.clone());
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
            if let Some(session) = host.session.borrow_mut().as_mut() {
                session.name = name;
            }
        }
        Err(e) => {
            host.state.borrow_mut().notice = Some(e);
        }
    }
    host.notify(cx);
}

/// 归档 / 取消归档当前项目。
pub fn toggle_archive(host: &ProjectUiHost, cx: &mut App) {
    if read_only_blocked(host, cx, "归档") {
        return;
    }
    let root = match host.session.borrow().as_ref().map(|s| s.root.clone()) {
        Some(r) => r,
        None => return,
    };
    let current = project_service::list_all()
        .ok()
        .and_then(|v| v.into_iter().find(|p| p.path == root));
    let Some(item) = current else { return };
    let archived = item.status == "archived";
    if let Err(e) = project_service::set_archived(&item.id, &root, !archived) {
        host.state.borrow_mut().notice = Some(e);
    } else {
        host.state.borrow_mut().menu_open = false;
        host.state.borrow_mut().notice = Some(format!(
            "已{}项目",
            if archived { "取消归档" } else { "归档" }
        ));
    }
    host.notify(cx);
}

// ==================== 渲染 ====================

/// 项目选择器（覆盖中央区）。
pub fn render_picker(host: &ProjectUiHost, inputs: &ProjectInputs, cx: &mut App) -> Div {
    let theme = cx.theme();
    let ui = host.state.borrow();
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
        let host_t = host.clone();
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
                .on_click(move |_, _, app| set_tab(&host_t, tab, app))
                .child(tab.label()),
        );
    }

    // ---- 搜索 + 排序 ----
    let host_sort = host.clone();
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
                .on_click(move |_, _, app| cycle_sort(&host_sort, app))
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
            list = list.child(project_card(host, inputs, item, picker.tab, theme));
        }
    }

    // ---- 右侧操作栏 ----
    let host_new = host.clone();
    let inputs_new = inputs.clone();
    let host_open = host.clone();
    let inputs_open = inputs.clone();
    let host_sample = host.clone();

    let rail = div()
        .v_flex()
        .gap_2()
        // 240px @ 16px 基准：用 rem 让选择器侧栏随界面缩放。
        .w(rems(15.))
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
                    open_create_dialog(&host_new, &inputs_new, window, app);
                }),
        )
        .child(
            Button::new("picker-open")
                .secondary()
                .label("🗀 打开文件夹…")
                .on_click(move |_, window, app| {
                    open_folder_dialog(&host_open, &inputs_open, window, app);
                }),
        )
        .child(
            Button::new("picker-sample")
                .secondary()
                .label("▦ 从示例项目开始")
                .on_click(move |_, window, app| create_sample(&host_sample, window, app)),
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

/// 项目卡片：一个可见主操作（打开）+ 次要命令收进 `DropdownMenu`。
///
/// 设计指南不允许列表行铺一排 hover 才可见的图标命令：主操作常显，其余命令由
/// 带可见触发按钮的菜单承载（方向键、dismiss、焦点恢复由组件负责）。
fn project_card(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    tab: PickerTab,
    theme: &gpui_kit::component::Theme,
) -> Div {
    let host_open = host.clone();
    let path_open = item.path.clone();

    let status_color = match item.status.as_str() {
        "active" => theme.colors.success,
        "archived" => theme.colors.muted_foreground,
        "syncing" => theme.colors.info,
        "offline" => theme.colors.warning,
        _ => theme.colors.muted_foreground,
    };

    let open = Button::new(ElementId::Name(SharedString::from(format!(
        "open-{}",
        item.id
    ))))
    .small()
    .label("打开")
    .on_click(move |_, window, app| request_open(&host_open, &path_open, window, app));

    let more = more_menu(host, inputs, item, tab);

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
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.colors.foreground)
                        .child(item.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(status_color)
                        .child(item.status.clone()),
                )
                .when(item.is_pinned, |d| {
                    d.child(Icon::new(IconName::Star).text_color(theme.colors.primary))
                })
                .child(
                    div().ml_auto().child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(open)
                            .child(more),
                    ),
                ),
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

/// 卡片「更多」菜单：主操作之外的次要命令。
///
/// 破坏性命令用分隔线与普通命令分开，名称带「…」表示会再开对话框。
fn more_menu(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    item: &ProjectSummary,
    tab: PickerTab,
) -> impl IntoElement {
    let item = item.clone();
    let inputs = inputs.clone();
    let host = host.clone();
    let menu_id = SharedString::from(format!("more-{}", item.id));

    Button::new(ElementId::Name(menu_id))
        .ghost()
        .small()
        .icon(IconName::Ellipsis)
        .dropdown_menu(move |menu, _window, _cx| {
            let pinned = item.is_pinned;
            let mut menu = menu.item(
                PopupMenuItem::new(if pinned { "取消固定" } else { "固定" })
                    .icon(if pinned {
                        IconName::StarOff
                    } else {
                        IconName::Star
                    })
                    .on_click({
                        let host = host.clone();
                        let item = item.clone();
                        move |_, _, app| toggle_pin(&host, &item, app)
                    }),
            );

            if tab == PickerTab::Removed {
                let id = item.id.clone();
                menu = menu.separator().item(
                    PopupMenuItem::new("恢复")
                        .icon(IconName::RotateCw)
                        .on_click({
                            let host = host.clone();
                            move |_, _, app| restore(&host, &id, app)
                        }),
                );
            } else if item.path_exists {
                let path = item.path.clone();
                menu = menu
                    .separator()
                    .item(
                        PopupMenuItem::new("在资源管理器中显示")
                            .icon(IconName::ExternalLink)
                            .on_click(move |_, _, _| reveal_in_explorer(&path)),
                    )
                    .item(PopupMenuItem::new("移出列表").on_click({
                        let host = host.clone();
                        let id = item.id.clone();
                        move |_, _, app| soft_remove(&host, &id, app)
                    }));
                let item_del = item.clone();
                let inputs_del = inputs.clone();
                menu = menu.separator().item(
                    PopupMenuItem::new("删除数据…")
                        .icon(IconName::Delete)
                        .on_click({
                            let host = host.clone();
                            move |_, window, app| {
                                open_delete_dialog(&host, &inputs_del, &item_del, window, app)
                            }
                        }),
                );
            } else {
                let item_rl = item.clone();
                let inputs_rl = inputs.clone();
                menu = menu.separator().item(
                    PopupMenuItem::new("重新定位…")
                        .icon(IconName::FolderOpen)
                        .on_click({
                            let host = host.clone();
                            move |_, window, app| {
                                open_relocate_dialog(&host, &inputs_rl, &item_rl, window, app)
                            }
                        }),
                );
                let id = item.id.clone();
                menu = menu.item(PopupMenuItem::new("移出列表").on_click({
                    let host = host.clone();
                    move |_, _, app| forget(&host, &id, app)
                }));
            }
            menu
        })
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

/// 项目菜单内容（由宿主用 `Popover` 承载表面 / 焦点 / 点击外部关闭）。
///
/// 不再自绘弹层——编码指南要求 menu/popup 使用语义组件，不要用 generic `div` 重做
/// focus keyboard 与 dismissal。
pub fn render_menu_content(host: &ProjectUiHost, inputs: &ProjectInputs, cx: &mut App) -> Div {
    let theme = cx.theme();
    let (name, root, read_only) = {
        let ui = host.state.borrow();
        let p = host.session.borrow();
        let name = p.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        let root = p.as_ref().map(|s| s.root.clone()).unwrap_or_default();
        (name, root, ui.read_only)
    };

    let inputs_settings = inputs.clone();
    let inputs_rename = inputs.clone();
    let host_close = host.clone();
    let host_settings = host.clone();
    let host_rename = host.clone();
    let host_switch = host.clone();
    let host_archive = host.clone();
    let name_settings = name.clone();
    let name_rename = name.clone();
    let path = root.clone();

    let mut menu = div()
        .v_flex()
        .child(menu_item(
            "switch",
            "切换项目…",
            theme,
            move |window, app| {
                // 切换项目 = 关闭当前 + 回到选择器（一实例一项目）。
                request_close(&host_switch, window, app);
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
                let mut ui = host_settings.state.borrow_mut();
                ui.menu_open = false;
                ui.settings_open = true;
                drop(ui);
                host_settings.notify(app);
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
                let mut ui = host_rename.state.borrow_mut();
                ui.menu_open = false;
                ui.settings_open = true;
                drop(ui);
                host_rename.notify(app);
            },
        ))
        .child(menu_item("reveal", "在资源管理器中显示", theme, {
            let path = path.clone();
            move |_window, _app| reveal_in_explorer(&path)
        }))
        .child(div().h_px().my_1().bg(theme.colors.border))
        .child(menu_item(
            "archive",
            "归档 / 取消归档",
            theme,
            move |_window, app| {
                toggle_archive(&host_archive, app);
            },
        ))
        .child(div().h_px().my_1().bg(theme.colors.border))
        .child(menu_item(
            "close",
            "关闭项目",
            theme,
            move |window, app| {
                host_close.state.borrow_mut().menu_open = false;
                request_close(&host_close, window, app);
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
    menu
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

// ==================== 项目设置 ====================

/// 项目设置（覆盖中央区，Tab 式）。
pub fn render_settings(host: &ProjectUiHost, inputs: &ProjectInputs, cx: &mut App) -> Option<Div> {
    if !host.state.borrow().settings_open {
        return None;
    }
    let (name, root) = {
        let p = host.session.borrow();
        match p.as_ref() {
            Some(s) => (s.name.clone(), s.root.clone()),
            None => return None,
        }
    };
    let theme = cx.theme();
    let read_only = host.state.borrow().read_only;

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

    let host_close = host.clone();
    let host_reveal = root.clone();
    let host_reload = host.clone();
    let inputs_rename = inputs.clone();
    let host_rename = host.clone();
    let inputs_version = inputs.clone();
    let host_version = host.clone();

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
                                host_close.state.borrow_mut().settings_open = false;
                                host_close.notify(app);
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
                                        save_rename(&host_rename, &inputs_rename, app)
                                    }),
                            ),
                    )
                    .child(section(theme, "存储（.RSmeta）"))
                    .child(files)
                    .child(
                        Button::new("proj-open-meta")
                            .secondary()
                            .label("🗀 打开 .RSmeta")
                            .on_click(move |_, _, _| reveal_in_explorer(&host_reveal)),
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
                                        create_version_action(&host_version, &inputs_version, app)
                                    }),
                            ),
                    )
                    .child(section(theme, "危险区"))
                    .child(
                        Button::new("proj-reload")
                            .secondary()
                            .label("刷新列表")
                            .on_click(move |_, _, app| {
                                refresh_picker(&host_reload, app);
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
pub fn create_version_action(host: &ProjectUiHost, inputs: &ProjectInputs, cx: &mut App) {
    if read_only_blocked(host, cx, "创建版本") {
        return;
    }
    let root = match host.session.borrow().as_ref().map(|s| s.root.clone()) {
        Some(r) => r,
        None => return,
    };
    let message = inputs.version_msg.read(cx).value().trim().to_string();
    match project_service::create_version(&root, &message) {
        Ok(()) => host.state.borrow_mut().notice = Some("已创建版本快照".to_string()),
        Err(e) => host.state.borrow_mut().notice = Some(e),
    }
    host.notify(cx);
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

#[cfg(test)]
mod tests;
