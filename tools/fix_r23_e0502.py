# -*- coding: utf-8 -*-
"""修 E0502：懒建 inputs 移到 theme 之前。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

lazy = '''        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。
        if self.form_name.is_none() {
            self.form_name = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_driver = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_url = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_user = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_pass = Some(cx.new(|cx| InputState::new(window, cx)));
        }

'''
head = '''    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tool = self.shared.active_tool.get();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();
'''
# 把 lazy 从 theme 后移到最前
new_head = '''    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
''' + lazy + '''        let theme = cx.theme();
        let tool = self.shared.active_tool.get();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();
'''
assert lazy in t and head in t
t = t.replace(head + lazy, new_head)
# 清理可能残留的重复 lazy
assert t.count('受控输入懒创建') == 1, 'lazy block count: %d' % t.count('受控输入懒创建')
p.write_text(t, encoding='utf-8')
print('E0502 fixed')
