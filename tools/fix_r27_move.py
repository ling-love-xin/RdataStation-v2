# -*- coding: utf-8 -*-
"""修 R27 E0382：shared/entity 提前克隆双副本给导出按钮。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 1) 前置克隆：在 R26 闭包捕获前为导出按钮准备独立副本
old = '''                let sql_state = self.sql_input.clone().expect("lazy init");
                let query_result = self.query_result.clone();
                let qr_closure = query_result.clone();
                let shared = self.shared.clone();
                let entity = entity.clone();
'''
new = '''                let sql_state = self.sql_input.clone().expect("lazy init");
                let query_result = self.query_result.clone();
                let qr_closure = query_result.clone();
                let shared = self.shared.clone();
                let shared_export = self.shared.clone();
                let entity = entity.clone();
                let entity_export = entity.clone();
'''
assert t.count(old) == 1
t = t.replace(old, new)

# 2) 导出按钮闭包改用提前克隆的副本
old2 = '''                        // Round 27：导出 CSV——结果落盘到全局目录 results/ 并按时间戳命名。
                        let out_clone = out.clone();
                        let shared_export = shared.clone();
                        let entity_export = entity.clone();
                        sql_ui = sql_ui.child('''
new2 = '''                        // Round 27：导出 CSV——结果落盘到全局目录 results/ 并按时间戳命名。
                        let out_clone = out.clone();
                        sql_ui = sql_ui.child('''
assert t.count(old2) == 1
t = t.replace(old2, new2)

p.write_text(t, encoding='utf-8')
print('E0382 fixed')
