# -*- coding: utf-8 -*-
"""修 R26 脚本：gap_0_5 -> gap_1，去掉 placeholder。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\tools\patch_r26_sql_ui.py')
t = p.read_text(encoding='utf-8')
t = t.replace('let mut table = div().v_flex().gap_0_5();', 'let mut table = div().v_flex().gap_1();')
t = t.replace('.child(Input::new(&sql_state).placeholder("SELECT * FROM orders LIMIT 100"))', '.child(Input::new(&sql_state))')
p.write_text(t, encoding='utf-8')
print('fixed')
