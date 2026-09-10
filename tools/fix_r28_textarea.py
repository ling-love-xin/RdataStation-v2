# -*- coding: utf-8 -*-
"""R28：SQL 查询区单行 Input → 多行 Textarea。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 1) import
old = 'use gpui_kit::component::input::{Input, InputState};'
new = 'use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};'
assert t.count(old) == 1
t = t.replace(old, new)

# 2) struct 字段
old = '    sql_input: Option<Entity<InputState>>,'
new = '    sql_textarea: Option<Entity<TextareaState>>,'
assert t.count(old) == 1
t = t.replace(old, new)

# 3) new() 初始化
old = '            sql_input: None,'
new = '            sql_textarea: None,'
assert t.count(old) == 1
t = t.replace(old, new)

# 4) 懒创建
old = '            self.sql_input = Some(cx.new(|cx| InputState::new(window, cx)));'
new = '            self.sql_textarea = Some(cx.new(|cx| TextareaState::new(window, cx)));'
assert t.count(old) == 1
t = t.replace(old, new)

# 5) SQL 区取状态
old = '                let sql_state = self.sql_input.clone().expect("lazy init");'
new = '                let sql_state = self.sql_textarea.clone().expect("lazy init");'
assert t.count(old) == 1
t = t.replace(old, new)

# 6) SQL 区布局：Input → Textarea（多行，高 96px），改为纵向堆叠 + 执行按钮右对齐
old = '''                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(Input::new(&sql_state))
                            .child(
                                Button::new("run-sql")
                                    .secondary()
                                    .label("执行")
                                    .on_click(move |_, _, app| {'''
new = '''                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .w_full()
                            .child(Textarea::new(&sql_state).h(px(96.)))
                            .child(
                                div()
                                    .h_flex()
                                    .justify_end()
                                    .w_full()
                                    .child(
                                        Button::new("run-sql")
                                            .secondary()
                                            .label("执行")
                                            .on_click(move |_, _, app| {'''
assert t.count(old) == 1
t = t.replace(old, new)

# 7) 执行按钮闭包收尾（原闭包后紧跟 `}),\n                            ),\n                    );`）——多包了一层 div，
#    需要补一个收尾括号。查找闭包后的结构：
old = '''                                        entity.update(app, |_, cx| cx.notify());
                                    }),
                            ),
                    );

                let result = query_result.borrow().clone();'''
new = '''                                        entity.update(app, |_, cx| cx.notify());
                                            }),
                                    ),
                            ),
                    );

                let result = query_result.borrow().clone();'''
assert t.count(old) == 1
t = t.replace(old, new)

p.write_text(t, encoding='utf-8')
print('textarea swapped')
