# -*- coding: utf-8 -*-
"""加密打点：定位崩溃精确语句。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''        let theme = cx.theme();
        let tool = self.shared.active_tool.get();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();'''
new = '''        let theme = cx.theme();
        eprintln!("[dbg] a-theme-ok");
        let tool = self.shared.active_tool.get();
        eprintln!("[dbg] b-tool-ok");
        let notice = self.shared.notice.borrow().clone();
        eprintln!("[dbg] c-notice-ok");
        let entity = cx.entity();
        eprintln!("[dbg] d-entity-ok");'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''        // Round 22：选中连接 → 真实元数据详情卡片（键值行）。
        if let Some(item) = self.shared.selected_connection() {'''
new = '''        eprintln!("[dbg] e-before-detail");
        // Round 22：选中连接 → 真实元数据详情卡片（键值行）。
        if let Some(item) = self.shared.selected_connection() {
            eprintln!("[dbg] f-detail-entry");'''
assert t.count(old) == 1
t = t.replace(old, new)

p.write_text(t, encoding='utf-8')
print('dense dbg added')
