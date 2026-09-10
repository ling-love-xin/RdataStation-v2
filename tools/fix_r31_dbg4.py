# -*- coding: utf-8 -*-
"""详情卡内部加密打点。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''            eprintln!("[dbg] f-detail-entry");
            let shared = self.shared.clone();
            let entity = entity.clone();
            let selected_id = item.id.clone();'''
new = '''            eprintln!("[dbg] f-detail-entry");
            let shared = self.shared.clone();
            eprintln!("[dbg] g-shared-clone");
            let entity = entity.clone();
            let selected_id = item.id.clone();'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''            let mut rows = div().v_flex().gap_1();
            for (label, value) in fields {'''
new = '''            eprintln!("[dbg] h-fields-built");
            let mut rows = div().v_flex().gap_1();
            eprintln!("[dbg] i-rows-div");
            for (label, value) in fields {'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''            content = content.child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .rounded_md()'''
new = '''            eprintln!("[dbg] j-rows-done");
            content = content.child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .rounded_md()'''
assert t.count(old) == 1
t = t.replace(old, new)

p.write_text(t, encoding='utf-8')
print('detail dbg added')
