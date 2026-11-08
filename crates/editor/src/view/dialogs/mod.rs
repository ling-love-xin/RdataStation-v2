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
use gpui_kit::component::{
    Icon, IndexPath, InteractiveElementExt as _, Selectable as _, Sizable as _, WindowExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::mode::{CellGranularity, ConfirmKind};
use crate::model::EditorMode;
use crate::ui;

mod analysis_sql;
mod bind;
mod bookmarks;
mod confirm;
mod connection_picker;
mod schema_picker;
mod value_preview;

#[cfg(test)]
mod picker_tests;

pub use analysis_sql::*;
pub use bind::*;
pub use bookmarks::*;
pub use confirm::*;
pub use connection_picker::*;
pub use schema_picker::*;
pub use value_preview::*;

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
