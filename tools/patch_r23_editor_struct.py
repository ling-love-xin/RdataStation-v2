# -*- coding: utf-8 -*-
"""Round 23：EditorPanel 新建连接表单（Form + InputState 受控输入）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# ---- 1. imports ----
old_imp = '''use gpui_kit::component::button::{Button, ButtonVariants};'''
new_imp = '''use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::input::{Input, InputState};'''
assert old_imp in t
t = t.replace(old_imp, new_imp)

# ---- 2. EditorPanel 结构 + new ----
old_struct = '''pub struct EditorPanel {
    shared: Shared,
    focus_handle: FocusHandle,
}

impl EditorPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
        }
    }
}'''
new_struct = '''pub struct EditorPanel {
    shared: Shared,
    focus_handle: FocusHandle,
    // Round 23：新建连接表单状态（受控输入，render 首次懒创建）。
    show_form: bool,
    form_name: Option<Entity<InputState>>,
    form_driver: Option<Entity<InputState>>,
    form_url: Option<Entity<InputState>>,
    form_user: Option<Entity<InputState>>,
    form_pass: Option<Entity<InputState>>,
}

impl EditorPanel {
    pub fn new(shared: Shared, cx: &mut Context<Self>) -> Self {
        Self {
            shared,
            focus_handle: cx.focus_handle(),
            show_form: false,
            form_name: None,
            form_driver: None,
            form_url: None,
            form_user: None,
            form_pass: None,
        }
    }
}'''
assert old_struct in t
t = t.replace(old_struct, new_struct)

p.write_text(t, encoding='utf-8')
print('editor struct patched')
