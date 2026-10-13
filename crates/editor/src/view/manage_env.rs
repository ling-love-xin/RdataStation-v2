//! 【T2b-3】管理环境对话框：环境配置表（增删改路径 / 参数）
//!
//! 入口：通用模式工具栏的环境选择器菜单 → 「管理环境…」。一张对话框两张视图：
//!
//! - **列表**：环境配置表（名称 / 类型 / 命令 / 参数 / 版本 / 操作）+「新增环境」按钮。
//!   内置环境（系统探测，`readonly`）只展示不改；自定义环境行内给「编辑 / 删除」。
//! - **表单**：新增 / 编辑一条（名称 / 类型 / 命令 / 参数）。
//!
//! 增删改经 [`crate::env::EnvPort`]（宿主实现：全局库 `script_environments` 表）；
//! 端口没接管理能力时，保存 / 删除会如实报「管理环境未接入」。
//!
//! ## 状态归属
//!
//! 与 `open_analysis_sql` 同一纪律：**输入实体在开窗前建好**（`open_dialog` 的 builder
//! 是 `Fn`，每帧重建，在里面 `cx.new` 会每帧漏一个实体）。所以 [`open_manage_environments`]
//! 先建好 [`ManageEnvState`]（含三个输入框实体）再开窗；一切动作都走 [`ManageEnvState`]
//! 的方法（`begin_create` / `begin_edit` / `save` / `delete` …），按钮只转发。
//! 动作方法内部 `cx.notify()` 触发重绘 → builder 重跑 → 从端口重新 `list()` 拿到最新表。
//!
//! ## 与宿主的边界
//!
//! 删除成功后要回调 `on_delete(id)`：宿主据此清掉「文档里正选中这条环境」的绑定
//! （否则选择器会显示一条已不存在的 id）。新增 / 更新不碰文档绑定，不回调。

use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Sizable as _, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::env::{EnvHandle, EnvKind, Environment};
use crate::ui;

/// 对话框当前在干什么（列表 or 表单）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManageMode {
    /// 列表：环境配置表
    List,
    /// 表单：`Some(id)` = 编辑这条；`None` = 新增
    Editing { id: Option<String> },
}

/// 管理环境对话框的局部状态（含三个输入框实体；**开窗前建好**）
///
/// 方法都带 `cx: &mut Context<Self>` 且内部 `cx.notify()`：改动后对话框重绘，
/// builder 重跑 → 从端口重新 `list()` 拿到最新表。
pub struct ManageEnvState {
    port: EnvHandle,
    /// 删除成功后的回调（宿主清掉文档里选了这条环境的绑定）
    on_delete: Rc<dyn Fn(String, &mut Window, &mut App)>,
    mode: ManageMode,
    kind: EnvKind,
    /// 保存 / 删除失败的原因（`None` = 无）
    message: Option<String>,
    name: Entity<InputState>,
    command: Entity<InputState>,
    args: Entity<InputState>,
}

impl ManageEnvState {
    /// 进「新增」表单：清空输入框（`InputState::set_value` 要 `&mut Window`）
    pub fn begin_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = ManageMode::Editing { id: None };
        self.kind = EnvKind::Custom;
        self.message = None;
        // 实体句柄是 Rc，先克隆出来再逐个设值：`set_input` 要 `&mut self`，
        // 直接传 `&self.name` 会同时借 `self` 不可变与可变（E0502）。
        let name = self.name.clone();
        let command = self.command.clone();
        let args = self.args.clone();
        self.set_input(window, cx, &name, "");
        self.set_input(window, cx, &command, "");
        self.set_input(window, cx, &args, "");
        cx.notify();
    }

    /// 进「编辑」表单：从端口列表里找到这条，回填输入框
    pub fn begin_edit(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(env) = self.port.list().into_iter().find(|e| e.id == id) else {
            self.message = Some("环境不存在（可能已被删除）".to_string());
            cx.notify();
            return;
        };
        self.mode = ManageMode::Editing {
            id: Some(env.id.clone()),
        };
        self.kind = env.kind;
        self.message = None;
        // 同 `begin_create`：先克隆句柄再设值（`set_input` 要 `&mut self`）
        let name = self.name.clone();
        let command = self.command.clone();
        let args = self.args.clone();
        self.set_input(window, cx, &name, &env.name);
        self.set_input(window, cx, &command, &env.command);
        self.set_input(window, cx, &args, &env.args.join(" "));
        cx.notify();
    }

    /// 表单里选类型（内置三档 + 自定义）
    pub fn set_kind(&mut self, kind: EnvKind, cx: &mut Context<Self>) {
        self.kind = kind;
        cx.notify();
    }

    /// 取消编辑：回列表（输入框里的字不保留——那只是这一次编辑的草稿）
    pub fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        self.mode = ManageMode::List;
        self.message = None;
        cx.notify();
    }

    /// 保存（新增 / 编辑共用）：读输入框 → 调端口 → 成功回列表 / 失败留提示
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().to_string();
        let command = self.command.read(cx).value().to_string();
        let args = self
            .args
            .read(cx)
            .value()
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let result = match &self.mode {
            ManageMode::Editing { id: Some(id) } => {
                self.port.update(id, &name, self.kind, &command, &args)
            }
            // 理论上不会走到：列表态点不到保存。真走到 = 当成新增（如实兜底）
            _ => self.port.create(&name, self.kind, &command, &args),
        };
        match result {
            Ok(_) => {
                self.mode = ManageMode::List;
                self.message = None;
            }
            Err(reason) => {
                self.message = Some(reason);
            }
        }
        let _ = window;
        cx.notify();
    }

    /// 删除一条自定义环境：调端口 → 成功后回调宿主（清文档绑定）；失败留提示
    pub fn delete(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.port.delete(id) {
            Ok(()) => {
                self.message = None;
                let on_delete = self.on_delete.clone();
                on_delete(id.to_string(), window, cx);
            }
            Err(reason) => {
                self.message = Some(reason);
            }
        }
        cx.notify();
    }

    /// 给输入框设值（`InputState::set_value` 需要 `&mut Window`）
    fn set_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        input: &Entity<InputState>,
        value: &str,
    ) {
        input.update(cx, |state, cx| {
            state.set_value(value.to_string(), window, cx)
        });
    }

    pub fn mode(&self) -> &ManageMode {
        &self.mode
    }

    pub fn kind(&self) -> EnvKind {
        self.kind
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// 列表态用的只读快照（端口每次给最新；对话框重绘时自动刷新）
    pub fn envs(&self) -> Vec<Environment> {
        self.port.list()
    }

    pub fn installed(&self, id: &str) -> bool {
        self.port.installed(id)
    }
}

/// 打开「管理环境」对话框，返回局部状态实体（测试经它直接驱动动作）
///
/// `on_delete`：删除成功后回调（宿主清掉文档里选了这条环境的绑定）。
pub fn open_manage_environments(
    window: &mut Window,
    cx: &mut App,
    port: EnvHandle,
    on_delete: impl Fn(String, &mut Window, &mut App) + 'static,
) -> Entity<ManageEnvState> {
    let name_input = cx.new(|cx| InputState::new(window, cx));
    let command_input = cx.new(|cx| InputState::new(window, cx));
    let args_input = cx.new(|cx| InputState::new(window, cx));
    let state = cx.new(|_cx| ManageEnvState {
        port,
        on_delete: Rc::new(on_delete),
        mode: ManageMode::List,
        kind: EnvKind::Custom,
        message: None,
        name: name_input,
        command: command_input,
        args: args_input,
    });

    let dialog_state = state.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let body = match dialog_state.read(cx).mode() {
            ManageMode::List => list_view(cx, &dialog_state),
            ManageMode::Editing { .. } => form_view(cx, &dialog_state),
        };
        let footer = match dialog_state.read(cx).mode() {
            ManageMode::List => DialogFooter::new().child(
                Button::new("editor-env-manage-close")
                    .with_variant(ButtonVariant::Secondary)
                    .small()
                    .debug_selector(|| "editor-env-manage-close".to_string())
                    .label("关闭")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            ),
            ManageMode::Editing { .. } => {
                let cancel = Button::new("editor-env-form-cancel")
                    .with_variant(ButtonVariant::Ghost)
                    .small()
                    .debug_selector(|| "editor-env-form-cancel".to_string())
                    .label("取消")
                    .on_click({
                        let state = dialog_state.clone();
                        move |_, _window, cx| {
                            state.update(cx, |s, cx| s.cancel_edit(cx));
                        }
                    });
                let save = Button::new("editor-env-form-save")
                    .with_variant(ButtonVariant::Primary)
                    .small()
                    .debug_selector(|| "editor-env-form-save".to_string())
                    .label("保存")
                    .on_click({
                        let state = dialog_state.clone();
                        move |_, window, cx| {
                            state.update(cx, |s, cx| s.save(window, cx));
                        }
                    });
                DialogFooter::new().child(cancel).child(save)
            }
        };

        dialog
            .title("管理环境")
            .w(cx.theme().font_size * ui::MANAGE_ENV_DIALOG_WIDTH)
            .child(body)
            .footer(footer)
    });

    state
}

/// 列表视图：环境配置表 + 「新增环境」
fn list_view(cx: &App, state: &Entity<ManageEnvState>) -> Div {
    let theme = cx.theme();
    let envs = state.read(cx).envs();
    let installed = envs
        .iter()
        .map(|env| (env.id.clone(), state.read(cx).installed(&env.id)))
        .collect::<std::collections::HashMap<String, bool>>();

    let mut body = div().v_flex().w_full().gap_2();
    // 顶部行：计数 + 提示 + 「新增环境」
    body = body.child(
        div()
            .h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(format!("共 {} 个环境（内置为系统探测，只读）", envs.len())),
            )
            .child(
                Button::new("editor-env-add")
                    .with_variant(ButtonVariant::Secondary)
                    .small()
                    .debug_selector(|| "editor-env-add".to_string())
                    .label("新增环境")
                    .on_click({
                        let state = state.clone();
                        move |_, window, cx| {
                            state.update(cx, |s, cx| s.begin_create(window, cx));
                        }
                    }),
            ),
    );
    // 保存 / 删除失败的提示（黄色，不是错误红：可重试）
    if let Some(message) = state.read(cx).message() {
        body = body.child(
            div()
                .w_full()
                .text_xs()
                .text_color(theme.colors.warning)
                .child(message.to_string()),
        );
    }

    if envs.is_empty() {
        body = body.child(
            div()
                .w_full()
                .py_2()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child("没有环境。点「新增环境」配置一个解释器。"),
        );
    } else {
        let mut list = div().v_flex().w_full();
        list = list.child(header_line(theme));
        for env in &envs {
            let can_manage = !env.readonly;
            let is_installed = installed.get(&env.id).copied().unwrap_or(false);
            list = list.child(env_line(theme, env, can_manage, is_installed, state.clone()));
        }
        body = body.child(
            list
                .max_h(rems(ui::MANAGE_ENV_LIST_MAX_HEIGHT))
                .overflow_y_scrollbar()
                .into_any_element(),
        );
    }

    body
}

/// 表头（列宽与下面各行一一对应）
fn header_line(theme: &Theme) -> Div {
    div()
        .h_flex()
        .w_full()
        .items_center()
        .gap_1()
        .px_1()
        .py_1()
        .border_b(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(div().flex_1().min_w_0().child("名称"))
        .child(div().w(rems(ui::MANAGE_ENV_COL_KIND)).flex_none().child("类型"))
        .child(div().w(rems(ui::MANAGE_ENV_COL_COMMAND)).flex_none().child("命令"))
        .child(div().w(rems(ui::MANAGE_ENV_COL_ARGS)).flex_none().child("参数"))
        .child(div().w(rems(ui::MANAGE_ENV_COL_VERSION)).flex_none().child("版本"))
        .child(div().w(rems(ui::MANAGE_ENV_COL_ACTION)).flex_none().child(""))
}

/// 一行环境：名称（+ 只读标）/ 类型 / 命令 / 参数 / 版本 / 操作
fn env_line(
    theme: &Theme,
    env: &Environment,
    can_manage: bool,
    is_installed: bool,
    state: Entity<ManageEnvState>,
) -> impl IntoElement {
    let muted = theme.colors.muted_foreground;
    let version = match (&env.version, is_installed) {
        (Some(v), true) => v.clone(),
        _ => "未安装".to_string(),
    };
    let version_color = if is_installed {
        theme.colors.foreground
    } else {
        muted
    };
    let args_text = if env.args.is_empty() {
        "—".to_string()
    } else {
        env.args.join(" ")
    };

    let edit_button = {
        let state = state.clone();
        let id = env.id.clone();
        let id_debug = id.clone();
        Button::new(format!("editor-env-edit-{}", env.id))
            .with_variant(ButtonVariant::Ghost)
            .xsmall()
            .debug_selector(move || format!("editor-env-edit-{id_debug}"))
            .label("编辑")
            .on_click(move |_, window, cx| {
                let state = state.clone();
                state.update(cx, |s, cx| s.begin_edit(&id, window, cx));
            })
    };
    let delete_button = {
        let state = state.clone();
        let id = env.id.clone();
        let id_debug = id.clone();
        Button::new(format!("editor-env-delete-{}", env.id))
            .with_variant(ButtonVariant::Ghost)
            .xsmall()
            .debug_selector(move || format!("editor-env-delete-{id_debug}"))
            .label("删除")
            .on_click(move |_, window, cx| {
                let state = state.clone();
                state.update(cx, |s, cx| s.delete(&id, window, cx));
            })
    };

    div()
        .h_flex()
        .w_full()
        .items_center()
        .gap_1()
        .px_1()
        .py_1()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .min_w_0()
                        .text_sm()
                        .text_ellipsis()
                        .text_color(theme.colors.foreground)
                        .child(env.name.clone()),
                )
                .when(env.readonly, |row| {
                    row.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child("· 系统"),
                    )
                }),
        )
        .child(
            div()
                .w(rems(ui::MANAGE_ENV_COL_KIND))
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(env.kind.label()),
        )
        .child(
            div()
                .w(rems(ui::MANAGE_ENV_COL_COMMAND))
                .flex_none()
                .min_w_0()
                .text_xs()
                .text_ellipsis()
                .font_family(theme.mono_font_family.clone())
                .text_color(theme.colors.foreground)
                .child(env.command.clone()),
        )
        .child(
            div()
                .w(rems(ui::MANAGE_ENV_COL_ARGS))
                .flex_none()
                .min_w_0()
                .text_xs()
                .text_ellipsis()
                .font_family(theme.mono_font_family.clone())
                .text_color(muted)
                .child(args_text),
        )
        .child(
            div()
                .w(rems(ui::MANAGE_ENV_COL_VERSION))
                .flex_none()
                .text_xs()
                .text_color(version_color)
                .child(version),
        )
        .child(
            div()
                .w(rems(ui::MANAGE_ENV_COL_ACTION))
                .flex_none()
                .h_flex()
                .items_center()
                .gap_1()
                .when(
                    can_manage,
                    |row| row.child(edit_button).child(delete_button),
                ),
        )
}

/// 表单视图：新增 / 编辑一条（名称 / 类型 / 命令 / 参数）
fn form_view(cx: &App, state: &Entity<ManageEnvState>) -> Div {
    let theme = cx.theme();
    let read = state.read(cx);
    let kind = read.kind();
    let message = read.message().map(str::to_string);
    let name_input = read.name.clone();
    let command_input = read.command.clone();
    let args_input = read.args.clone();

    let mut body = div().v_flex().w_full().gap_2().text_sm();
    body = body.child(field(theme, "名称", Input::new(&name_input)));
    body = body.child(
        div()
            .w_full()
            .h_flex()
            .items_center()
            .gap_1()
            .child(
                div()
                    .flex_none()
                    .w(rems(ui::MANAGE_ENV_FORM_LABEL))
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("类型"),
            )
            .child(kind_selector(state.clone(), kind)),
    );
    body = body.child(field(theme, "命令", Input::new(&command_input)));
    body = body.child(
        div()
            .w_full()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child("参数：空格分隔（如 `-u --no-site`）；脚本代码经 stdin 传入，不走参数"),
    );
    body = body.child(Input::new(&args_input));
    if let Some(message) = message {
        body = body.child(
            div()
                .w_full()
                .text_xs()
                .text_color(theme.colors.warning)
                .child(message),
        );
    }
    body
}

/// 一行「标签 + 输入框」字段
fn field(theme: &Theme, label: &str, input: impl IntoElement) -> impl IntoElement {
    div()
        .w_full()
        .h_flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_none()
                .w(rems(ui::MANAGE_ENV_FORM_LABEL))
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(label.to_string()),
        )
        .child(div().flex_1().min_w_0().child(input))
}

/// 类型选择：四档 Button（选中 = Primary，未选 = Ghost）
fn kind_selector(state: Entity<ManageEnvState>, kind: EnvKind) -> impl IntoElement {
    let mut row = div().h_flex().items_center().gap_1();
    for candidate in [
        EnvKind::Python,
        EnvKind::RScript,
        EnvKind::Rustc,
        EnvKind::Custom,
    ] {
        let selected = candidate == kind;
        let state = state.clone();
        row = row.child(
            Button::new(format!("editor-env-kind-{}", candidate.as_key()))
                .with_variant(if selected {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .xsmall()
                .debug_selector(move || format!("editor-env-kind-{}", candidate.as_key()))
                .label(candidate.label())
                .on_click(move |_, _window, cx| {
                    let state = state.clone();
                    state.update(cx, |s, cx| s.set_kind(candidate, cx));
                }),
        );
    }
    row
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**（父模块引入了 gpui 相关符号，`use super::*` 有风险）
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui_kit::component::Root;
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::{
        div, AppContext as _, Entity, IntoElement, Render, Styled as _, TestAppContext,
        VisualTestContext, Window,
    };

    use super::{ManageMode, ManageEnvState, open_manage_environments};
    use crate::env::{EnvHandle, EnvKind, EnvPort, Environment};

    /// 可管理的假端口：内存存自定义环境 + 记录调用
    #[derive(Default)]
    struct FakeManageablePort {
        envs: RefCell<Vec<Environment>>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeManageablePort {
        fn record(&self, call: String) {
            self.calls.borrow_mut().push(call);
        }
    }

    impl EnvPort for FakeManageablePort {
        fn list(&self) -> Vec<Environment> {
            self.envs.borrow().clone()
        }
        fn installed(&self, id: &str) -> bool {
            self.envs
                .borrow()
                .iter()
                .any(|env| env.id == id && env.version.is_some())
        }
        fn create(
            &self,
            name: &str,
            kind: EnvKind,
            command: &str,
            args: &[String],
        ) -> Result<Environment, String> {
            self.record(format!("create:{name}:{command}"));
            let env = Environment {
                id: format!("custom-test-{}", self.envs.borrow().len()),
                name: name.to_string(),
                kind,
                command: command.to_string(),
                args: args.to_vec(),
                version: None,
                readonly: false,
            };
            self.envs.borrow_mut().push(env.clone());
            Ok(env)
        }
        fn update(
            &self,
            id: &str,
            name: &str,
            kind: EnvKind,
            command: &str,
            args: &[String],
        ) -> Result<Environment, String> {
            self.record(format!("update:{id}:{name}:{command}"));
            let env = Environment {
                id: id.to_string(),
                name: name.to_string(),
                kind,
                command: command.to_string(),
                args: args.to_vec(),
                version: None,
                readonly: false,
            };
            let mut envs = self.envs.borrow_mut();
            if let Some(slot) = envs.iter_mut().find(|e| e.id == id) {
                *slot = env.clone();
            }
            Ok(env)
        }
        fn delete(&self, id: &str) -> Result<(), String> {
            self.record(format!("delete:{id}"));
            self.envs.borrow_mut().retain(|e| e.id != id);
            Ok(())
        }
    }

    fn port() -> Rc<FakeManageablePort> {
        let port = FakeManageablePort::default();
        port.envs.borrow_mut().push(Environment {
            id: "py".to_string(),
            name: "Python".to_string(),
            kind: EnvKind::Python,
            command: "python".to_string(),
            args: vec!["-u".to_string()],
            version: Some("3.12".to_string()),
            readonly: true,
        });
        port.envs.borrow_mut().push(Environment {
            id: "my-env".to_string(),
            name: "我的 Py".to_string(),
            kind: EnvKind::Custom,
            command: "C:/venv/python.exe".to_string(),
            args: vec!["-u".to_string()],
            version: None,
            readonly: false,
        });
        Rc::new(port)
    }

    /// 建一个以 `Root` 为根的窗口（`open_dialog` 依赖它），返回窗口级上下文。
    fn harness(cx: &mut TestAppContext) -> &mut VisualTestContext {
        struct Harness;
        impl Render for Harness {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut gpui_kit::Context<Self>,
            ) -> impl IntoElement {
                div().size_full()
            }
        }
        let (_, cx) = cx.add_window_view(|window, cx| {
            let harness = cx.new(|_cx| Harness);
            Root::new(harness, window, cx)
        });
        cx
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    fn setup(
        cx: &mut VisualTestContext,
    ) -> (
        Entity<ManageEnvState>,
        Rc<FakeManageablePort>,
        Rc<RefCell<Vec<String>>>,
    ) {
        let port = port();
        let deleted = Rc::new(RefCell::new(Vec::new()));
        let deleted_cb = deleted.clone();
        let port_in = port.clone();
        let state = cx.update(|window, cx| {
            open_manage_environments(window, cx, port_in, move |id, _window, _cx| {
                deleted_cb.borrow_mut().push(id);
            })
        });
        (state, port, deleted)
    }

    /// 开窗即列表：行与「新增环境」都在（层真渲染断言）
    #[gpui_kit::test]
    fn the_dialog_opens_in_list_mode_with_rows(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let cx = harness(cx);
        let (state, _port, _deleted) = setup(cx);
        draw(cx);
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(
            cx.update(|_, cx| state.read(cx).mode().clone()),
            ManageMode::List
        );
        assert_eq!(cx.update(|_, cx| state.read(cx).envs().len()), 2, "内置 + 自定义都在列表");
    }

    /// 新增：begin_create → 填输入框 → save → 端口收到 create，回到列表
    #[gpui_kit::test]
    fn adding_an_env_goes_through_the_port_and_returns_to_list(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let cx = harness(cx);
        let (state, port, _deleted) = setup(cx);
        draw(cx);
        cx.update(|window, cx| {
            state.update(cx, |s, cx| s.begin_create(window, cx));
        });
        assert_eq!(
            cx.update(|_, cx| state.read(cx).mode().clone()),
            ManageMode::Editing { id: None }
        );

        // 直接驱动表单字段：把输入框值设上（与按钮走的同一条方法）
        cx.update(|window, cx| {
            let name = state.read(cx).name.clone();
            let command = state.read(cx).command.clone();
            let args = state.read(cx).args.clone();
            name.update(cx, |s, cx| s.set_value("我的 Python".to_string(), window, cx));
            command.update(cx, |s, cx| {
                s.set_value("C:/python/python.exe".to_string(), window, cx)
            });
            args.update(cx, |s, cx| s.set_value("-u --no-site".to_string(), window, cx));
        });

        cx.update(|window, cx| {
            state.update(cx, |s, cx| s.save(window, cx));
        });
        assert_eq!(
            cx.update(|_, cx| state.read(cx).mode().clone()),
            ManageMode::List,
            "保存成功回列表"
        );
        assert_eq!(port.calls.borrow().as_slice(), &["create:我的 Python:C:/python/python.exe"]);
        let envs = cx.update(|_, cx| state.read(cx).envs());
        assert_eq!(envs.len(), 3);
        assert!(
            envs.iter().any(|e| e.name == "我的 Python" && e.args == vec!["-u", "--no-site"]),
            "参数空格切分后原样落库"
        );
    }

    /// 编辑：begin_edit 回填 → 改命令 → save → 端口收到 update，值原样
    #[gpui_kit::test]
    fn editing_an_env_prefills_and_updates(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let cx = harness(cx);
        let (state, port, _deleted) = setup(cx);
        draw(cx);
        cx.update(|window, cx| {
            state.update(cx, |s, cx| s.begin_edit("my-env", window, cx));
        });
        assert_eq!(
            cx.update(|_, cx| state.read(cx).mode().clone()),
            ManageMode::Editing {
                id: Some("my-env".to_string())
            }
        );
        // 回填：命令框里是原命令
        cx.update(|_window, cx| {
            let command = state.read(cx).command.clone();
            assert_eq!(command.read(cx).value(), "C:/venv/python.exe");
        });

        cx.update(|window, cx| {
            state.update(cx, |s, cx| s.save(window, cx));
        });
        assert_eq!(
            port.calls.borrow().as_slice(),
            &["update:my-env:我的 Py:C:/venv/python.exe"],
            "未改动的回填值原样提交（更新幂等）"
        );
    }

    /// 删除：成功 → 端口收到 delete + on_delete 回调宿主（清文档绑定）
    #[gpui_kit::test]
    fn deleting_an_env_calls_back_for_document_cleanup(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let cx = harness(cx);
        let (state, port, deleted) = setup(cx);
        draw(cx);
        cx.update(|window, cx| {
            state.update(cx, |s, cx| s.delete("my-env", window, cx));
        });
        assert_eq!(port.calls.borrow().as_slice(), &["delete:my-env"]);
        assert_eq!(deleted.borrow().as_slice(), &["my-env"], "删除成功后回调宿主");
        assert!(!cx.update(|_, cx| state.read(cx).envs().iter().any(|e| e.id == "my-env")));
    }

    /// 保存失败（端口拒绝）→ 留在表单 + 提示可见（不吞错、不假装成功）
    #[gpui_kit::test]
    fn save_failure_keeps_the_form_and_shows_the_reason(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let cx = harness(cx);
        // 端口不接管理能力的对照：换一个只读端口（默认 CRUD 拒绝）
        struct Noop;
        impl EnvPort for Noop {
            fn list(&self) -> Vec<Environment> {
                vec![]
            }
            fn installed(&self, _id: &str) -> bool {
                false
            }
        }
        let noop: EnvHandle = Rc::new(Noop);
        let deleted_cb = Rc::new(RefCell::new(Vec::new()));
        let deleted = deleted_cb.clone();
        let state = cx.update(|window, cx| {
            open_manage_environments(window, cx, noop, move |id, _w, _c| {
                deleted.borrow_mut().push(id)
            })
        });
        draw(cx);
        cx.update(|window, cx| {
            state.update(cx, |s, cx| s.begin_create(window, cx));
            state.update(cx, |s, cx| s.save(window, cx));
        });
        assert_eq!(
            cx.update(|_, cx| state.read(cx).mode().clone()),
            ManageMode::Editing { id: None },
            "失败留在表单"
        );
        assert!(
            cx.update(|_, cx| state.read(cx).message().is_some()),
            "失败原因可见（管理环境未接入）"
        );
    }
}
