//! 编辑器对话框（A9 收尾）：关闭三态 / 保存失败 / 模式切换确认 / 自定义分析 SQL
//!
//! ## 为什么对话框在 editor 侧、动作却在宿主侧
//!
//! “先保存再关”“切换模式要付出什么代价”是**编辑器的语义**（文案表在 [`crate::mode::ConfirmKind`]），
//! 所以对话框连同它的文案归 editor；而**执行动作**（把面板从 Dock 上摘掉、写盘、改模式）
//! 交给调用方回调——面板不能在自己的 `update` 里让 Dock 移除自己（架构 §12 #23），宿主可以。
//!
//! ## 三条取舍
//!
//! 1. **不出现“确定 / 取消”**：按钮文案说清按下去会发生什么（`ConfirmKind::confirm_label`），
//!    取消是唯一保留通用词的地方。
//! 2. **不引入对话框内局部状态**：单元粒度用两个动作按钮表达，而不是“单选 + 确定”——
//!    对话框重建由 Root 决定，`Rc<Cell<_>>` 改了不会自动重绘，做成单选就得去戳 Root 重绘
//!    （见 `Root::update`）。少一层状态，也少一处“点了没反应”。
//!    **例外**：需要**输入**的场合（[`open_analysis_sql`]）用 `TextareaState` 实体——重绘与
//!    键盘归它自己管，这正好是“局部状态”该有的形状；普通选择仍不引入局部状态。
//! 3. **Esc / 点遮罩 = 取消**：`Dialog` 默认行为即是如此，不额外挂 `on_cancel`。
//!
//! 所有函数都必须在**事件路径**上调用（菜单 / 动作 / 关闭请求），渲染期不开对话框。

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, IndexPath, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::mode::{CellGranularity, ConfirmKind};
use crate::model::EditorMode;
use crate::ui;

/// 关闭脏文档时用户的选择（三态）
///
/// `Cancel` 与关闭对话框本身是两件事：Esc / 点遮罩也会让对话框消失，但那是“取消”，
/// 不会有回调；这里的 `Cancel` 是**用户显式点了“不关”**，调用方据此清掉状态栏提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
    /// 先保存，保存成功再关
    Save,
    /// 丢弃未保存的改动直接关
    Discard,
    /// 什么都不做
    Cancel,
}

/// 保存失败后的选择（“保存失败二次确认”）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFailureChoice {
    /// 重试保存（可能是文件被占用 / 短暂无权限）
    Retry,
    /// 换个路径再试
    SaveAs,
    /// 放弃保存（**文档仍是脏的**，不关也不清脏）
    Cancel,
}

/// 关闭脏文档前的三态确认（“保存 / 不保存 / 取消”）
///
/// 调用方在 `on_choice` 里执行真正的动作：`Discard` 后才移除面板，`Save` 后再移除。
pub fn open_close_confirm(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    on_choice: impl Fn(CloseChoice, &mut Window, &mut App) + 'static,
) {
    let title = title.into();
    let on_choice = Rc::new(on_choice);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let muted = cx.theme().colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .text_sm()
            .child(SharedString::from(format!("「{title}」有未保存的改动。")))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("保存后关闭，或不保存直接丢弃这次改动。"),
            );

        let cancel = action_button("editor-close-cancel", "取消", ButtonVariant::Ghost, {
            let on_choice = on_choice.clone();
            move |window, cx| {
                // 显式取消：留痕清掉（“按了没反应”的提示不该继续挂着）
                on_choice(CloseChoice::Cancel, window, cx)
            }
        });
        let discard = action_button("editor-close-discard", "不保存", ButtonVariant::Danger, {
            let on_choice = on_choice.clone();
            move |window, cx| on_choice(CloseChoice::Discard, window, cx)
        });
        let save = action_button("editor-close-save", "保存", ButtonVariant::Primary, {
            let on_choice = on_choice.clone();
            move |window, cx| on_choice(CloseChoice::Save, window, cx)
        });

        dialog
            .title("关闭前确认")
            .child(body)
            // 自绘 footer 会关掉组件库的默认“确定 / 取消”按钮（footer 与 button_props 互斥）
            .footer(DialogFooter::new().child(cancel).child(discard).child(save))
    });
}

/// 保存失败后的二次确认（原因必须显示出来：用户要能据此决定重试还是换路径）
pub fn open_save_failure_confirm(
    window: &mut Window,
    cx: &mut App,
    title: impl Into<SharedString>,
    reason: impl Into<SharedString>,
    on_choice: impl Fn(SaveFailureChoice, &mut Window, &mut App) + 'static,
) {
    let title = title.into();
    let reason = reason.into();
    let on_choice = Rc::new(on_choice);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let danger = cx.theme().colors.danger;
        let muted = cx.theme().colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .text_sm()
            .child(SharedString::from(format!("「{title}」保存失败。")))
            .child(div().text_xs().text_color(danger).child(reason.clone()))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("文档仍处于未保存状态（改动不会丢失）。"),
            );

        let cancel = action_button("editor-save-cancel", "取消保存", ButtonVariant::Ghost, {
            let on_choice = on_choice.clone();
            move |window, cx| on_choice(SaveFailureChoice::Cancel, window, cx)
        });
        let save_as = action_button(
            "editor-save-as",
            "另存为…",
            ButtonVariant::Secondary,
            {
                let on_choice = on_choice.clone();
                move |window, cx| on_choice(SaveFailureChoice::SaveAs, window, cx)
            },
        );
        let retry = action_button("editor-save-retry", "重试", ButtonVariant::Primary, {
            let on_choice = on_choice.clone();
            move |window, cx| on_choice(SaveFailureChoice::Retry, window, cx)
        });

        dialog
            .title("保存失败")
            .child(body)
            .footer(DialogFooter::new().child(cancel).child(save_as).child(retry))
    });
}

/// 模式切换确认（原型 §1.3：禁止静默切换）
///
/// `kind.picks_granularity()` 为真时，footer 直接给两个动作按钮——按钮文案就是选择本身
/// （“整篇一个单元”/“按语句拆分”），不再额外摆一组单选框。
/// 其它确认分支只有“确认 + 取消”，回调收到的粒度用 `CellGranularity::default()`。
pub fn open_switch_confirm(
    window: &mut Window,
    cx: &mut App,
    kind: ConfirmKind,
    from: EditorMode,
    to: EditorMode,
    note: Option<&'static str>,
    on_confirm: impl Fn(CellGranularity, &mut Window, &mut App) + 'static,
) {
    let on_confirm = Rc::new(on_confirm);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let muted = cx.theme().colors.muted_foreground;

        let mut body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .text_sm()
            .child(kind.body());
        // 状态栏同款说明（例如“结果仍保留但置灰”）：与切换后的提示保持一致口径
        if let Some(note) = note {
            body = body.child(div().text_xs().text_color(muted).child(note));
        }

        let cancel = action_button("editor-switch-cancel", "取消", ButtonVariant::Ghost, {
            // 取消：对话框消失即结束，不需要回调（不改任何状态）
            move |_window, _cx| {}
        });

        let footer = if kind.picks_granularity() {
            let single = action_button(
                "editor-switch-single",
                CellGranularity::Single.label(),
                ButtonVariant::Secondary,
                {
                    let on_confirm = on_confirm.clone();
                    move |window, cx| on_confirm(CellGranularity::Single, window, cx)
                },
            );
            let per_statement = action_button(
                "editor-switch-per-statement",
                CellGranularity::PerStatement.label(),
                ButtonVariant::Primary,
                {
                    let on_confirm = on_confirm.clone();
                    move |window, cx| on_confirm(CellGranularity::PerStatement, window, cx)
                },
            );
            DialogFooter::new()
                .child(cancel)
                .child(single)
                .child(per_statement)
        } else {
            let confirm = action_button(
                "editor-switch-confirm",
                kind.confirm_label(),
                ButtonVariant::Primary,
                {
                    let on_confirm = on_confirm.clone();
                    move |window, cx| on_confirm(CellGranularity::default(), window, cx)
                },
            );
            DialogFooter::new().child(cancel).child(confirm)
        };

        dialog
            .title(kind.title())
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(SharedString::from(format!(
                                "{} → {}",
                                from.label(),
                                to.label()
                            ))),
                    )
                    .child(body),
            )
            .footer(footer)
    });
}

/// 自定义分析 SQL（B15 切片二）：预置菜单之外，自己写一条聚合 SQL 在结果集上跑
///
/// 与其它对话框不同，这个要**输入**，所以状态不是闭包里的 `Rc<Cell<_>>`，而是一个
/// `TextareaState` 实体：它自己管重绘、光标与键盘（这正是“局部状态”该有的形状）。
/// 初始值由调用方给——预填一条**能直接跑**的模板（`{table}` 占位符写在提示里）。
///
/// 回调拿到的就是框里的原文；“空不空”由落地入口判（见 `analysis::is_runnable`）。
pub fn open_analysis_sql(
    window: &mut Window,
    cx: &mut App,
    hint: impl Into<SharedString>,
    initial_sql: impl Into<SharedString>,
    on_run: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let hint = hint.into();
    let state = cx.new(|cx| {
        TextareaState::new(window, cx)
            .placeholder(crate::analysis::CUSTOM_PLACEHOLDER)
            .default_value(initial_sql)
    });
    let on_run = Rc::new(on_run);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let muted = cx.theme().colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(div().text_xs().text_color(muted).child(hint.clone()))
            .child(
                div()
                    .w_full()
                    // 输入框本身不给 selector（它不是可交互 Div）；窗口测试按这个包装定位
                    .debug_selector(|| "editor-analysis-sql".to_string())
                    .child(Textarea::new(&state).h(rems(ui::ANALYSIS_SQL_HEIGHT))),
            );

        let cancel = action_button("editor-analysis-cancel", "取消", ButtonVariant::Ghost, {
            // 取消：对话框消失即结束（框里的字不保留——它只属于这一次分析）
            move |_window, _cx| {}
        });
        let run_state = state.clone();
        // 对话框 builder 是 `Fn`（会被重建），所以在这里再克隆一份 Rc 交给按钮
        let run_on_run = on_run.clone();
        let run = action_button(
            "editor-analysis-run",
            "运行分析",
            ButtonVariant::Primary,
            move |window, cx| {
                let sql = run_state.read(cx).value().to_string();
                run_on_run(sql, window, cx);
            },
        );

        dialog
            .title(crate::analysis::CUSTOM_TITLE)
            .child(body)
            .footer(DialogFooter::new().child(cancel).child(run))
    });
}

/// 一个“点完即关”的 footer 按钮
///
/// 关对话框**先于**执行回调：回调里常常要再开一个对话框（保存失败 → 二次确认），
/// 先关掉自己才不会出现两层叠着的对话框。
fn action_button(
    id: &'static str,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id)
        .with_variant(variant)
        .small()
        // 测试按选择器定位并真点：对话框流程（闭三态 / 切换确认）要有点击级回归
        .debug_selector(move || format!("editor-dialog-{id}"))
        .label(label)
        .on_click(move |_, window, cx| {
            window.close_dialog(cx);
            action(window, cx);
        })
}

/// 【遗留 #7】值查看器：右键「查看完整值」弹的模态层
///
/// 与右键菜单里「按值筛选「预览」」（24 字符截断）的分工：那是菜单文案，不该把菜单
/// 撑开；这里是**看全一个值**的地方——等宽字体、保留原始换行、可滚动、可复制。
/// 复制**不关弹层**（用户复制完可能还要接着看），关闭 / Esc / 点遮罩才收。
pub fn open_value_preview(
    window: &mut Window,
    cx: &mut App,
    value: impl Into<SharedString>,
    column: impl Into<SharedString>,
    row: usize,
    on_copy: impl Fn(&mut App) + 'static,
) {
    let value = value.into();
    let column = column.into();
    let on_copy = Rc::new(on_copy);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("列「{column}」 · 行 {row}")),
            )
            .child(
                div()
                    .w_full()
                    .max_h(rems(ui::VALUE_PREVIEW_MAX_HEIGHT))
                    .overflow_y_scrollbar()
                    .bg(theme.colors.background)
                    .border_1()
                    .border_color(theme.colors.border)
                    .rounded_md()
                    .p_3()
                    .font_family("monospace")
                    .text_sm()
                    // 测试可观测：值内容区（窗口级验收断言「值真的渲染在弹层里」）
                    .debug_selector(|| "editor-dialog-editor-value-preview-body".to_string())
                    // 多行值（JSON / 文本）按 `\n` 自然换行；连续空格折叠可接受——
                    // 看全的是「换行结构」，不是空格字节
                    .child(value.clone()),
            );

        // 复制不关弹层（复制完可能还要接着看），所以不复用 action_button 的“点完即关”
        let copy = Button::new("editor-value-preview-copy")
            .with_variant(ButtonVariant::Primary)
            .small()
            .debug_selector(|| "editor-dialog-editor-value-preview-copy".to_string())
            .label("复制")
            .on_click({
                let on_copy = on_copy.clone();
                move |_, _window, cx| on_copy(cx)
            });
        let close = action_button("editor-value-preview-close", "关闭", ButtonVariant::Ghost, {
            move |_window, _cx| {}
        });

        dialog
            .title("查看值")
            .child(body)
            .footer(DialogFooter::new().child(copy).child(close))
    });
}

/// 【B16】参数绑定表单（DBeaver 式 `:name` → 弹参数 → 字面替换后执行）
///
/// 输入实体在**开窗前**建好（manage_env 的纪律：`open_dialog` 的 builder 是一次性的，
/// 而 `InputState` 要在有 `window` 的上下文里 `cx.new`）。提交回调把
/// 「参数名 → 用户输入」整包交给宿主替换并继续执行；取消回调清掉宿主的挂起状态
/// （弹层默认 Esc / 点遮罩也会关，但挂起状态只有显式取消才清）。
pub fn open_bind_dialog(
    window: &mut Window,
    cx: &mut App,
    names: Vec<String>,
    on_run: impl Fn(HashMap<String, String>, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut App) + 'static,
) {
    use gpui_kit::component::input::{Input, InputState};

    let inputs: Vec<(SharedString, Entity<InputState>)> = names
        .into_iter()
        .map(|name| {
            let input = cx.new(|cx| InputState::new(window, cx));
            (SharedString::from(name), input)
        })
        .collect();
    let on_run = Rc::new(on_run);
    let on_cancel = Rc::new(on_cancel);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;

        // 每个参数一行：名字（灰）+ 输入框（测试可观测：`editor-bind-input-{名}`）
        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .children(inputs.iter().map(|(name, input)| {
                let name_for_selector = name.to_string();
                div()
                    .v_flex()
                    .gap_1()
                    .child(div().text_xs().text_color(muted).child(name.clone()))
                    .child(
                        // Input 是 RenderOnce（无 id 可观测），套一层带 selector 的 div
                        div()
                            .debug_selector(move || format!("editor-bind-input-{name_for_selector}"))
                            .child(Input::new(input)),
                    )
            }));

        // 执行：收集输入 → 关弹层 → 交回宿主（替换后继续原提交链）
        let run = Button::new("editor-bind-run")
            .with_variant(ButtonVariant::Primary)
            .small()
            .label("执行")
            .debug_selector(|| "editor-bind-run".to_string())
            .on_click({
                let on_run = on_run.clone();
                let inputs = inputs.clone();
                move |_, window, cx| {
                    let values = inputs
                        .iter()
                        .map(|(name, input)| {
                            (name.to_string(), input.read(cx).value().to_string())
                        })
                        .collect();
                    window.close_dialog(cx);
                    on_run(values, window, cx);
                }
            });
        // 取消：关弹层 + 清宿主挂起（动作是「这次不跑了」，不是只关个窗）
        let cancel = Button::new("editor-bind-cancel")
            .with_variant(ButtonVariant::Ghost)
            .small()
            .label("取消")
            .debug_selector(|| "editor-bind-cancel".to_string())
            .on_click({
                let on_cancel = on_cancel.clone();
                move |_, window, cx| {
                    window.close_dialog(cx);
                    on_cancel(cx);
                }
            });

        dialog
            .title("参数")
            .child(body)
            .footer(DialogFooter::new().child(cancel).child(run))
    });
}

// ==== 连接选择器（工具栏「连接名称」点击弹窗；2026-10-27）====
//
// 图：标题「选择数据源」+ 连接列表（短码·名称 + 类型 + 在线标记 + 当前勾选）+
// 底部「仅显示连接的数据库」复选（取消 = 显示全部）。点击 / 回车绑定该连接并关闭；
// Esc / 点遮罩 = 取消（不改绑定）。回调 `None` 语义留给调用方（绑定 None = 跟随当前连接，
// 但此选择器只给出 `Some(id)`——跟随项由工具栏 ▾ 下拉提供）。

/// 连接选择列表代理：一行 = 一条连接；`only_connected` 过滤由底部复选切换
/// （`ListState::delegate_mut` 重绘，不重建对话框）。
pub struct ConnectionPickerDelegate {
    /// 全部连接（内存快照；渲染路径只读）
    options: Vec<crate::connection::ConnectionOption>,
    /// 仅显示已建连的连接（图 1 底部复选；默认开）
    only_connected: bool,
    /// 当前文档绑定的连接 id（勾选标记）
    current: Option<String>,
    selected: Option<IndexPath>,
    /// 宿主绑定回调（`Some(id)` = 绑定该连接）
    on_pick: Rc<dyn Fn(Option<String>, &mut Window, &mut App)>,
}

impl ConnectionPickerDelegate {
    fn filtered(&self) -> Vec<&crate::connection::ConnectionOption> {
        self.options
            .iter()
            .filter(|o| !self.only_connected || o.connected)
            .collect()
    }
}

impl ListDelegate for ConnectionPickerDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.filtered().len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let filtered = self.filtered();
        let opt = filtered.get(ix.row)?;
        let theme = cx.theme();
        let checked = self.current.as_deref() == Some(opt.id.as_str());
        Some(
            ListItem::new(SharedString::from(format!("conn-picker-{}", opt.id)))
                .h(rems(ui::ENCODING_DIALOG_ROW_HEIGHT))
                .py_0()
                .px_2p5()
                .gap_2()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .text_sm()
                                .text_color(theme.colors.foreground)
                                .child(format!("{}·{}", opt.short, opt.name))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child(opt.db_type.clone()),
                                ),
                        )
                        .when(checked, |row| {
                            row.child(
                                Icon::new(IconName::Check)
                                    .size_3()
                                    .text_color(theme.colors.success),
                            )
                        })
                        .when(!opt.connected, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("未连接"),
                            )
                        }),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .v_flex()
            .items_center()
            .py_6()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child("（无可用连接）")
            .into_any_element()
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
        cx.notify();
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(ix) = self.selected else {
            return;
        };
        let filtered = self.filtered();
        let Some(opt) = filtered.get(ix.row) else {
            return;
        };
        let id = opt.id.clone();
        let on_pick = self.on_pick.clone();
        window.close_dialog(cx);
        on_pick(Some(id), window, cx);
    }
}

/// 打开连接选择器对话框
///
/// `on_pick`：确认后携带选中连接 id 回调宿主（宿主负责 `bind_connection` + 状态刷新）。
/// 返回 `ListState` 实体：测试经它直接驱动确认（headless 口径，见模块头）。
pub fn open_connection_picker(
    window: &mut Window,
    cx: &mut App,
    options: Vec<crate::connection::ConnectionOption>,
    current: Option<String>,
    on_pick: impl Fn(Option<String>, &mut Window, &mut App) + 'static,
) -> Entity<ListState<ConnectionPickerDelegate>> {
    let delegate = ConnectionPickerDelegate {
        options,
        only_connected: true,
        current,
        selected: None,
        on_pick: Rc::new(on_pick),
    };
    let state = cx.new(|cx| {
        ListState::new(delegate, window, cx)
            .searchable(false)
            .selectable(true)
    });
    let list = state.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let only = list.clone();
        let check = Checkbox::new("conn-picker-only-connected")
            .checked(list.read(cx).delegate().only_connected)
            .label("仅显示连接的数据库")
            .on_click(move |checked, _window, cx| {
                only.update(cx, |list, cx| {
                    list.delegate_mut().only_connected = *checked;
                    cx.notify();
                });
            });
        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("选择要绑定到本文档的连接（回车 / 点击确认，Esc 关闭）"),
            )
            .child(List::new(&list).max_h(rems(ui::PICKER_LIST_MAX_HEIGHT)))
            .child(check);
        dialog
            .title("选择数据源")
            .w(theme.font_size * ui::ENCODING_DIALOG_WIDTH)
            .child(body)
    });
    state
}

// ==== 活跃 Database / Schema 选择器（工具栏「schema@库」点击弹窗；2026-10-27）====
//
// 图：左栏 Database 列表 + 右栏该库的 Schema 列表。左栏选中切右栏；点击右栏 Schema
// 绑定并关闭。显示规则：有独立 Schema 层时 `schema@库`，库即 Schema（MySQL 等 catalog
// 为 None）时只显示库（Schema）名——与工具栏槽位同一口径。
//
// 需要左栏切换驱动右栏重绘，故状态用 `Entity<SchemaPickerState>`（对话框内局部状态
// 的正当形状：点击即改，`cx.notify` 触发重绘，见 dialogs 模块头取舍 #2 的例外）。

/// 库 / Schema 选择器内部状态（两栏：左 Database、右 Schema）
pub struct SchemaPickerState {
    /// 去重后的库（catalog）列表；`None` = 库即 Schema（MySQL 无独立 Catalog 层）
    catalogs: Vec<Option<String>>,
    /// 每个库下的 Schema 列表（与 `catalogs` 对齐）
    schemas_by: Vec<Vec<String>>,
    /// 左栏当前选中的库下标
    selected: usize,
    /// 当前文档绑定的 Schema（勾选标记）
    current: Option<String>,
    /// 宿主绑定回调（确认后写文档 Schema + 状态刷新）
    on_pick: Rc<dyn Fn(String, &mut Window, &mut App)>,
}

impl SchemaPickerState {
    /// 从 SchemaInfo 列表聚合出「库 → Schema」两栏数据（去重、保序）
    pub fn new(
        schemas: Vec<crate::connection::SchemaInfo>,
        current: Option<String>,
        on_pick: impl Fn(String, &mut Window, &mut App) + 'static,
    ) -> Self {
        let mut catalogs: Vec<Option<String>> = Vec::new();
        let mut schemas_by: Vec<Vec<String>> = Vec::new();
        for item in schemas {
            let idx = catalogs
                .iter()
                .position(|c| c == &item.catalog)
                .unwrap_or_else(|| {
                    catalogs.push(item.catalog.clone());
                    schemas_by.push(Vec::new());
                    catalogs.len() - 1
                });
            schemas_by[idx].push(item.schema);
        }
        let selected = schemas_by
            .iter()
            .position(|schs| current.as_ref().is_some_and(|c| schs.contains(c)))
            .unwrap_or(0);
        Self {
            catalogs,
            schemas_by,
            selected,
            current,
            on_pick: Rc::new(on_pick),
        }
    }

    /// 渲染对话框正文（左库 + 右 Schema 两栏 + 底部当前绑定；只读，交互经捕获实体回写）
    fn body(&self, entity: Entity<Self>, _window: &mut Window, cx: &App) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let foreground = theme.colors.foreground;
        let border = theme.colors.border;

        // 左栏：Database 列表（当前选中 toggled）
        let left = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .children(self.catalogs.iter().enumerate().map(|(i, cat)| {
                let entity = entity.clone();
                let label = match cat {
                    Some(c) => c.clone(),
                    None => "默认库".to_string(),
                };
                Button::new(SharedString::from(format!("schema-picker-cat-{i}")))
                    .ghost()
                    .small()
                    .toggled(i == self.selected)
                    .debug_selector(move || format!("schema-picker-cat-{i}"))
                    .label(label)
                    .on_click(move |_, _window, cx| {
                        entity.update(cx, |state, cx| {
                            state.selected = i;
                            cx.notify();
                        });
                    })
            }));

        // 右栏：当前库下的 Schema 列表（点击绑定并关闭）
        let right_schemas = self.schemas_by.get(self.selected).cloned().unwrap_or_default();
        let right_on_pick = self.on_pick.clone();
        let right = div()
            .v_flex()
            .flex_1()
            .min_w_0()
            .children(right_schemas.iter().map(|schema| {
                let on_pick = right_on_pick.clone();
                let schema = schema.clone();
                let selector = format!("schema-picker-sch-{schema}");
                let checked = self.current.as_deref() == Some(schema.as_str());
                Button::new(SharedString::from(selector.clone()))
                    .ghost()
                    .small()
                    .toggled(checked)
                    .debug_selector(move || selector.clone())
                    .label(schema.clone())
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        on_pick(schema.clone(), window, cx);
                    })
            }));

        // 底部当前绑定（`schema@库` / 仅库名，与工具栏槽位同口径）
        let current_label = match &self.current {
            Some(s) => {
                let catalog = self.catalogs.get(self.selected).and_then(|c| c.clone());
                match catalog {
                    Some(c) => format!("{s}@{c}"),
                    None => s.clone(),
                }
            }
            None => "（未选 Schema）".to_string(),
        };

        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .gap_2()
                    .items_stretch()
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .flex_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child("Database"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .border_1()
                                    .border_color(border)
                                    .rounded_md()
                                    .p_1()
                                    .child(left),
                            ),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .flex_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child("Schema"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .border_1()
                                    .border_color(border)
                                    .rounded_md()
                                    .p_1()
                                    .child(right),
                            ),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(if self.current.is_some() { foreground } else { muted })
                    .child(current_label),
            )
    }
}

/// 打开 Database / Schema 选择器对话框
///
/// 数据 = 绑定连接的 `Vec<SchemaInfo>`（聚合出 库 → Schema 两栏）；`on_pick` 携带选中的
/// Schema 名回调宿主（`set_schema`）。返回 `Entity<SchemaPickerState>` 供测试驱动。
pub fn open_schema_picker(
    window: &mut Window,
    cx: &mut App,
    schemas: Vec<crate::connection::SchemaInfo>,
    current: Option<String>,
    on_pick: impl Fn(String, &mut Window, &mut App) + 'static,
) -> Entity<SchemaPickerState> {
    let state = cx.new(|_cx| SchemaPickerState::new(schemas, current, on_pick));
    let entity = state.clone();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let body = entity.read(cx).body(entity.clone(), _window, cx);
        let width = cx.theme().font_size * ui::SCHEMA_PICKER_DIALOG_WIDTH;
        dialog
            .title("选择 Database/Schema")
            .w(width)
            .child(body)
    });
    state
}

// ═══ 连接 / 库 / Schema 选择器纯逻辑测试（headless 口径：不点对话框，驱动实体方法/纯函数）═══
#[cfg(test)]
mod picker_tests {
    use std::rc::Rc;

    use gpui_kit::{App, Window};

    use super::{ConnectionPickerDelegate, SchemaPickerState};
    use crate::connection::{ConnectionOption, SchemaInfo};

    fn conn(id: &str, name: &str, connected: bool) -> ConnectionOption {
        ConnectionOption {
            id: id.into(),
            short: id.chars().next().unwrap_or('?').to_string(),
            name: name.into(),
            connected,
            db_type: "test".into(),
        }
    }

    #[test]
    fn schema_picker_groups_by_catalog_and_preserves_order() {
        let schemas = vec![
            SchemaInfo {
                catalog: Some("postgres".into()),
                schema: "public".into(),
            },
            SchemaInfo {
                catalog: Some("warehouse_supply_chain".into()),
                schema: "public".into(),
            },
            SchemaInfo {
                catalog: Some("warehouse_supply_chain".into()),
                schema: "rds_nav_probe_a".into(),
            },
            SchemaInfo {
                catalog: None,
                schema: "main".into(), // MySQL 无 Catalog 层：库即 Schema
            },
        ];
        let state = SchemaPickerState::new(schemas, Some("rds_nav_probe_a".into()), |_, _, _| {});
        // 库列表去重保序：postgres → warehouse_supply_chain → None(默认库)
        assert_eq!(
            state.catalogs,
            vec![
                Some("postgres".to_string()),
                Some("warehouse_supply_chain".to_string()),
                None
            ]
        );
        assert_eq!(state.schemas_by[0], vec!["public"]);
        assert_eq!(state.schemas_by[1], vec!["public", "rds_nav_probe_a"]);
        assert_eq!(state.schemas_by[2], vec!["main"]);
        // 当前 schema 在非首库 → 左栏定位到对应库（选中 warehouse_supply_chain 那栏）
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn schema_picker_defaults_to_first_catalog_when_current_is_missing() {
        let schemas = vec![SchemaInfo {
            catalog: Some("postgres".into()),
            schema: "public".into(),
        }];
        let state = SchemaPickerState::new(schemas, Some("nope".into()), |_, _, _| {});
        assert_eq!(state.selected, 0);
        assert_eq!(state.current.as_deref(), Some("nope"));
    }

    #[test]
    fn connection_picker_filters_to_connected_when_toggled() {
        let delegate = ConnectionPickerDelegate {
            options: vec![
                conn("a", "Alpha", true),
                conn("b", "Beta", false),
                conn("c", "Gamma", true),
            ],
            only_connected: true,
            current: None,
            selected: None,
            on_pick: Rc::new(|_: Option<String>, _: &mut Window, _: &mut App| {}),
        };
        let ids: Vec<&str> = delegate.filtered().iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "c"]);
    }

    #[test]
    fn connection_picker_shows_all_when_only_connected_is_off() {
        let delegate = ConnectionPickerDelegate {
            options: vec![conn("a", "Alpha", true), conn("b", "Beta", false)],
            only_connected: false,
            current: None,
            selected: None,
            on_pick: Rc::new(|_: Option<String>, _: &mut Window, _: &mut App| {}),
        };
        assert_eq!(delegate.filtered().len(), 2);
    }
}
