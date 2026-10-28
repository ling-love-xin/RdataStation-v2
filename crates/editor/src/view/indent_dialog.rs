//! 【2026-10-27】缩进对话框（状态栏「缩进」段点击弹出，对齐 VSCode 缩进命令面板）
//!
//! 面板结构对齐 VSCode（图 2）：顶部「选择操作」提示 + 双语列表项 + 右侧分组标签
//! （更改视图 / 转换文件）。列表项为**动作**（选中回车执行），非单选：
//! - 更改视图组：使用空格 / 制表符缩进（当前项勾选）、更改制表符显示大小（弹 tab_size
//!   输入对话框，可自定义任意值）、从内容中检测缩进方式；
//! - 转换文件组：将缩进转换为空格 / 制表符、裁剪尾随空格。
//!
//! 动作经 `on_action` 回调宿主（宿主实现见 `host.rs`：`set_hard_tabs` / `set_tab_size` /
//! `detect_indent_from_content` / `convert_indent` / `trim_trailing_whitespace`）。
//! headless 口径：断言纯函数（`indent_entries`）+ 直调 confirm 分发，点击到不了对话框层。

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::{ActiveTheme, Icon, IndexPath, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::ui;

/// 缩进动作（对话框选中回车 → 回调宿主；`ChangeTabDisplaySize(size)` 经 tab_size 输入框
/// 确认后携带目标宽度）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentAction {
    UseSpaces,
    UseTabs,
    ChangeTabDisplaySize(usize),
    DetectFromContent,
    ConvertToSpaces,
    ConvertToTabs,
    TrimTrailingWhitespace,
}

/// 右侧分组标签（VSCode「更改视图」/「转换文件」）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentGroup {
    View,
    Convert,
}

/// 一项（纯数据，render 与测试共用）
pub struct IndentEntry {
    pub action: IndentAction,
    pub group: IndentGroup,
    pub zh: &'static str,
    pub en: &'static str,
    pub checked: bool,
}

/// 构建缩进面板的列表项（纯函数）：`hard_tabs` 决定「使用空格 / 制表符」的勾选
pub fn indent_entries(hard_tabs: bool) -> Vec<IndentEntry> {
    vec![
        IndentEntry {
            action: IndentAction::UseSpaces,
            group: IndentGroup::View,
            zh: "使用空格缩进",
            en: "Indent Using Spaces",
            checked: !hard_tabs,
        },
        IndentEntry {
            action: IndentAction::UseTabs,
            group: IndentGroup::View,
            zh: "使用制表符缩进",
            en: "Indent Using Tabs",
            checked: hard_tabs,
        },
        IndentEntry {
            action: IndentAction::ChangeTabDisplaySize(0),
            group: IndentGroup::View,
            zh: "更改制表符显示大小",
            en: "Change Tab Display Size",
            checked: false,
        },
        IndentEntry {
            action: IndentAction::DetectFromContent,
            group: IndentGroup::View,
            zh: "从内容中检测缩进方式",
            en: "Detect Indentation from Content",
            checked: false,
        },
        IndentEntry {
            action: IndentAction::ConvertToSpaces,
            group: IndentGroup::Convert,
            zh: "将缩进转换为空格",
            en: "Convert Indentation to Spaces",
            checked: false,
        },
        IndentEntry {
            action: IndentAction::ConvertToTabs,
            group: IndentGroup::Convert,
            zh: "将缩进转换为制表符",
            en: "Convert Indentation to Tabs",
            checked: false,
        },
        IndentEntry {
            action: IndentAction::TrimTrailingWhitespace,
            group: IndentGroup::Convert,
            zh: "裁剪尾随空格",
            en: "Trim Trailing Whitespace",
            checked: false,
        },
    ]
}

/// 列表代理：双语项 + 分组标签 + 当前缩进方式勾选
pub struct IndentDelegate {
    entries: Vec<IndentEntry>,
    /// 当前制表符显示宽度（「更改制表符显示大小」输入框的默认值）
    tab_size: usize,
    selected: Option<IndexPath>,
    /// 宿主回调（动作 → 宿主执行；`ChangeTabDisplaySize` 由本模块先弹输入框）
    on_action: Rc<dyn Fn(IndentAction, &mut Window, &mut App)>,
}

impl ListDelegate for IndentDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.entries.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let entry = self.entries.get(ix.row)?;
        let theme = cx.theme();
        let fg = theme.colors.foreground;
        let muted = theme.colors.muted_foreground;
        let group_label = match entry.group {
            IndentGroup::View => "更改视图",
            IndentGroup::Convert => "转换文件",
        };
        Some(
            ListItem::new(SharedString::from(format!("indent-{}", entry.zh)))
                .py_0()
                .px_2()
                .gap_2()
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(
                            div()
                                .v_flex()
                                .gap_0p5()
                                .child(div().text_sm().text_color(fg).child(entry.zh))
                                .child(div().text_xs().text_color(muted).child(entry.en)),
                        )
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .child(div().text_xs().text_color(muted).child(group_label))
                                .when(entry.checked, |row| {
                                    row.child(
                                        Icon::new(IconName::Check)
                                            .size_3()
                                            .text_color(theme.colors.success),
                                    )
                                }),
                        ),
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
            .child("（无匹配项）")
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
        let Some(entry) = self.entries.get(ix.row) else {
            return;
        };
        match entry.action {
            IndentAction::ChangeTabDisplaySize(_) => {
                // 特殊：弹 tab_size 输入框（可自定义任意值）；缩进面板保留在后面
                let on_action = self.on_action.clone();
                let current = self.tab_size.max(1);
                crate::view::indent_dialog::open_tab_size_dialog(
                    window,
                    cx,
                    current,
                    move |size, window, cx| {
                        on_action(IndentAction::ChangeTabDisplaySize(size), window, cx);
                    },
                );
            }
            action => {
                let on_action = self.on_action.clone();
                window.close_dialog(cx);
                on_action(action, window, cx);
            }
        }
    }
}

/// 打开缩进对话框（VSCode 面板样式：顶部「选择操作」提示 + 双语列表 + 分组标签）
///
/// `on_action`：动作 → 宿主执行。返回 `ListState` 实体（headless 直接驱动 confirm）。
pub fn open_indent_dialog(
    window: &mut Window,
    cx: &mut App,
    hard_tabs: bool,
    tab_size: usize,
    on_action: impl Fn(IndentAction, &mut Window, &mut App) + 'static,
) -> Entity<ListState<IndentDelegate>> {
    let delegate = IndentDelegate {
        entries: indent_entries(hard_tabs),
        tab_size,
        selected: None,
        on_action: Rc::new(on_action),
    };
    let state = cx.new(|cx| {
        ListState::new(delegate, window, cx)
            .searchable(false)
            .selectable(true)
    });
    let list = state.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("选择操作（回车执行 / Esc 关闭）"),
            )
            .child(List::new(&list));
        dialog
            .title("缩进")
            .w(theme.font_size * ui::INDENT_DIALOG_WIDTH)
            .child(body)
    });
    state
}

/// tab_size 自定义输入对话框（VSCode「更改制表符显示大小」）：Input 接受任意 1-32 值
///
/// `on_confirm`：确认后带宽度回调宿主（`set_tab_size` + 关闭）。
pub fn open_tab_size_dialog(
    window: &mut Window,
    cx: &mut App,
    current: usize,
    on_confirm: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Entity<InputState> {
    let input = cx.new(|cx| InputState::new(window, cx));
    input.update(cx, |state, cx| {
        state.set_value(SharedString::from(current.to_string()), window, cx);
    });
    let input_entity = input.clone();
    let on_confirm = Rc::new(on_confirm);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let run = Button::new("editor-tabsize-confirm")
            .with_variant(ButtonVariant::Primary)
            .small()
            .label("确定")
            .on_click({
                let on_confirm = on_confirm.clone();
                let input = input_entity.clone();
                move |_, window, cx| {
                    let size = input
                        .read(cx)
                        .value()
                        .to_string()
                        .trim()
                        .parse::<usize>()
                        .unwrap_or(2)
                        .clamp(1, 32);
                    window.close_dialog(cx);
                    on_confirm(size, window, cx);
                }
            });
        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("制表符显示大小（1-32，空格数 / 制表符展开宽度）"),
            )
            .child(Input::new(&input_entity))
            .child(div().h_flex().justify_end().child(run));
        dialog
            .title("更改制表符显示大小")
            .w(theme.font_size * ui::TAB_SIZE_DIALOG_WIDTH)
            .child(body)
    });

    input
}

#[cfg(test)]
mod tests {
    // 注意：不要 `use super::*`——会把顶层 `gpui_kit::*` 的 `test` 宏带进来，递归展开崩溃
    use super::{indent_entries, IndentAction, IndentGroup};

    #[test]
    fn entries_mark_current_indent_mode() {
        let entries = indent_entries(true);
        let use_tabs = entries.iter().find(|e| e.action == IndentAction::UseTabs).unwrap();
        let use_spaces = entries.iter().find(|e| e.action == IndentAction::UseSpaces).unwrap();
        assert!(use_tabs.checked);
        assert!(!use_spaces.checked);
    }

    #[test]
    fn entries_contain_both_groups_and_actions() {
        let entries = indent_entries(false);
        let actions: Vec<_> = entries.iter().map(|e| e.action).collect();
        assert_eq!(actions.len(), 7);
        assert!(actions.contains(&IndentAction::ChangeTabDisplaySize(0)));
        assert!(actions.contains(&IndentAction::DetectFromContent));
        assert!(actions.contains(&IndentAction::ConvertToSpaces));
        assert!(actions.contains(&IndentAction::ConvertToTabs));
        assert!(actions.contains(&IndentAction::TrimTrailingWhitespace));
        assert!(entries.iter().any(|e| e.group == IndentGroup::View));
        assert!(entries.iter().any(|e| e.group == IndentGroup::Convert));
        // 双语齐全
        assert!(entries.iter().all(|e| !e.zh.is_empty() && !e.en.is_empty()));
    }
}
