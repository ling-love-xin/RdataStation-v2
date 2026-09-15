//! 分组表单对话框（新建 / 编辑分组：名称 + 描述）。
//!
//! 背景：原「新建分组」直接落一个默认名（`新建分组 N`）再靠行内重命名改，**描述无处输入**；
//! 本对话框把名称与描述一次问清（原型 §6.2 分组菜单「新建分组 / 编辑描述」）。
//!
//! 约定：
//! - 名称**必填**：空名不提交、对话框保持打开，并在字段下给一条内联提示；
//! - 描述全为空白时归一化为 `None`（不写空串）；
//! - 提示是**从输入值推导**的（每帧重算），不用标志位，所以改回非空后自然消失；
//! - 新建时名称预填**自动去重的默认名**，避免开局就是空表单。

use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Theme, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// 表单初值。
#[derive(Clone, Debug)]
pub struct GroupFormSeed {
    /// 编辑的分组 ID（`None` = 新建）。
    pub id: Option<String>,
    /// 名称初值。
    pub name: String,
    /// 描述初值（`None` / 空白都视为空）。
    pub description: Option<String>,
}

impl GroupFormSeed {
    /// 新建：`default_name` 建议给自动去重后的默认名（避免空表单与空名校验）。
    pub fn for_new(default_name: impl Into<String>) -> Self {
        Self {
            id: None,
            name: default_name.into(),
            description: None,
        }
    }

    /// 编辑现有分组。
    pub fn for_existing(group: &engine::persistence::ConnectionGroup) -> Self {
        Self {
            id: Some(group.id.clone()),
            name: group.name.clone(),
            description: group.description.clone(),
        }
    }
}

/// 打开分组表单对话框。
///
/// `on_submit(分组 ID, 名称, 描述, app)` 只在**名称非空**时回调一次；`分组 ID` 为 `None`
/// 表示新建。回调负责落库与刷新（对话框自身不碰存储）。
pub fn open_group_form_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: GroupFormSeed,
    on_submit: impl Fn(Option<String>, String, Option<String>, &mut App) + 'static,
) {
    let editing = seed.id.is_some();
    let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("分组名称"));
    if !seed.name.trim().is_empty() {
        let value = seed.name.clone();
        name_input.update(cx, |s, cx| s.set_value(value, window, cx));
    }
    let desc_input = cx.new(|cx| InputState::new(window, cx).placeholder("描述（可选）"));
    if let Some(desc) = seed.description.as_ref().filter(|d| !d.trim().is_empty()) {
        let value = desc.clone();
        desc_input.update(cx, |s, cx| s.set_value(value, window, cx));
    }

    let title = if editing {
        "编辑分组"
    } else {
        "新建分组"
    };
    let ok_text = if editing { "保存" } else { "创建" };
    let target = seed.id.clone();
    let on_submit = Rc::new(on_submit);

    let submit = {
        let name_input = name_input.clone();
        let desc_input = desc_input.clone();
        let target = target.clone();
        let on_submit = on_submit.clone();
        move |cx: &mut App| -> bool {
            let name = name_input.read(cx).value().trim().to_string();
            if name.is_empty() {
                return false;
            }
            let desc = desc_input.read(cx).value().trim().to_string();
            on_submit(target.clone(), name, (!desc.is_empty()).then_some(desc), cx);
            true
        }
    };
    // 对话框 builder 是 `Fn`（每帧重建），故两份克隆都在 builder 内部生成。
    let submit_for_frame = submit.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        // 提示从输入值推导（不是标志位）：改回非空后下一帧自然消失。
        let name_empty = name_input.read(cx).value().trim().is_empty();
        let submit_ok = submit_for_frame.clone();
        let submit_key = submit_for_frame.clone();
        dialog
            .title(title)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_2()
                    .child(field_label(theme, "名称"))
                    .child(Input::new(&name_input))
                    .when(name_empty, |this| {
                        this.child(hint_line(theme, "名称不能为空"))
                    })
                    .child(field_label(theme, "描述"))
                    .child(Input::new(&desc_input)),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("group-form-cancel")
                            .secondary()
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("group-form-ok")
                            .primary()
                            .label(ok_text)
                            .on_click(move |_, window, cx| {
                                // 名称空：不关闭，让用户接着改（内联提示已经出现）。
                                if submit_ok(cx) {
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
            // Enter 确认 / Esc 取消（对话框 key context 自带绑定）。
            .on_ok(move |_, _window, cx| submit_key(cx))
            .on_cancel(|_, _, _| true)
    });
}

/// 字段名（弱化色，与对话框内的输入框一组）。
fn field_label(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.to_string())
}

/// 校验提示（`danger`，不是普通说明）。
fn hint_line(theme: &Theme, text: &str) -> Div {
    div()
        .text_xs()
        .text_color(theme.colors.danger)
        .child(text.to_string())
}
