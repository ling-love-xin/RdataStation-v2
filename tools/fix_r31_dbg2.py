# -*- coding: utf-8 -*-
"""细粒度打点：EditorPanel::render 各阶段。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 保留 render 入口打点，另在关键阶段加打点
old = '''        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。
        if self.form_name.is_none() {'''
new = '''        eprintln!("[dbg] EditorPanel::render step0-lazy");
        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。
        if self.form_name.is_none() {'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''        let theme = cx.theme();
        let tool = self.shared.active_tool.get();'''
new = '''        eprintln!("[dbg] EditorPanel::render step1-theme");
        let theme = cx.theme();
        let tool = self.shared.active_tool.get();'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''        // Round 25：数据库导航区——选中联邦连接时按需加载分析库元数据树。'''
new = '''        eprintln!("[dbg] EditorPanel::render step2-detail");
        // Round 25：数据库导航区——选中联邦连接时按需加载分析库元数据树。'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''        // Round 26：SQL 查询区——选中联邦连接时可对分析库执行只读 SQL。'''
new = '''        eprintln!("[dbg] EditorPanel::render step3-nav");
        // Round 26：SQL 查询区——选中联邦连接时可对分析库执行只读 SQL。'''
assert t.count(old) == 1
t = t.replace(old, new)

old = '''                content = content.child(sql_ui);
            }
        }'''
new = '''                eprintln!("[dbg] EditorPanel::render step4-sql-done");
                content = content.child(sql_ui);
            }
        }'''
assert t.count(old) == 1
t = t.replace(old, new)

p.write_text(t, encoding='utf-8')
print('fine dbg added')
