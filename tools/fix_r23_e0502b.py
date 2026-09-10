# -*- coding: utf-8 -*-
"""修 E0502（v2）：删除原 lazy 块并插到 theme 之前。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

lazy_block = '''        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。
        if self.form_name.is_none() {
            self.form_name = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_driver = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_url = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_user = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_pass = Some(cx.new(|cx| InputState::new(window, cx)));
        }

'''
anchor = '''    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();'''
new_anchor = '''    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
''' + lazy_block + '''        let theme = cx.theme();'''

# 1) 删除原 lazy 块（出现在 theme 之后的那个）
assert t.count(lazy_block) >= 1, 'lazy block occurrences: %d' % t.count(lazy_block)
t = t.replace(lazy_block, '', 1)

# 2) 在 theme 前插入
assert anchor in t, 'anchor not found'
t = t.replace(anchor, new_anchor, 1)

p.write_text(t, encoding='utf-8')
# 校验
lines = t.splitlines()
idx_theme = next(k for k, l in enumerate(lines) if 'let theme = cx.theme();' in l and 'fn render' in ''.join(lines[max(0,k-3):k]))
print('lazy before theme:', any('受控输入懒创建' in lines[j] for j in range(max(0,idx_theme-6), idx_theme)))
print('lazy count:', t.count('受控输入懒创建'))
