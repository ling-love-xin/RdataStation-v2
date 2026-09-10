# -*- coding: utf-8 -*-
"""重新打点（内存正常）：定位真实崩溃位置。"""
import pathlib

v = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs')
t = v.read_text(encoding='utf-8')
old = '''impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.area.is_none() {'''
new = '''impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        eprintln!("[dbg] WV-render");
        if self.area.is_none() {'''
assert t.count(old) == 1
v.write_text(t.replace(old, new), encoding='utf-8')

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')
old = '''impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。'''
new = '''impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        eprintln!("[dbg] EP-render");
        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''        let theme = cx.theme();
        let tool = self.shared.active_tool.get();'''
new = '''        eprintln!("[dbg] EP-lazy-done");
        let theme = cx.theme();
        let tool = self.shared.active_tool.get();
        eprintln!("[dbg] EP-theme-done");'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''        // Round 22：选中连接 → 真实元数据详情卡片（键值行）。
        if let Some(item) = self.shared.selected_connection() {'''
new = '''        eprintln!("[dbg] EP-before-detail");
        // Round 22：选中连接 → 真实元数据详情卡片（键值行）。
        if let Some(item) = self.shared.selected_connection() {
            eprintln!("[dbg] EP-detail-entry");'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''            content = content.child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .rounded_md()'''
new = '''            eprintln!("[dbg] EP-detail-card-build");
            content = content.child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .rounded_md()'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''        // Round 25：数据库导航区——选中联邦连接时按需加载分析库元数据树。'''
new = '''        eprintln!("[dbg] EP-detail-done");
        // Round 25：数据库导航区——选中联邦连接时按需加载分析库元数据树。'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''        // Round 26：SQL 查询区——选中联邦连接时可对分析库执行只读 SQL。'''
new = '''        eprintln!("[dbg] EP-nav-done");
        // Round 26：SQL 查询区——选中联邦连接时可对分析库执行只读 SQL。'''
assert t.count(old) == 1
t = t.replace(old, new)

p.write_text(t, encoding='utf-8')
print('dbg re-added')
