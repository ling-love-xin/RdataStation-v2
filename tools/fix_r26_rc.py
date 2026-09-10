# -*- coding: utf-8 -*-
"""修 R26 E0382：执行按钮闭包用独立 Rc 克隆。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''                let sql_state = self.sql_input.clone().expect("lazy init");
                let query_result = self.query_result.clone();
                let shared = self.shared.clone();
                let entity = entity.clone();
'''
new = '''                let sql_state = self.sql_input.clone().expect("lazy init");
                let query_result = self.query_result.clone();
                let qr_closure = query_result.clone();
                let shared = self.shared.clone();
                let entity = entity.clone();
'''
assert t.count(old) == 1
t = t.replace(old, new)

# 闭包内用 qr_closure
old2 = '''                                    .on_click(move |_, _, app| {
                                        let sql = sql_state.read(app).value().to_string();
                                        let dir = crate::services::workspace_loader::default_global_dir();
                                        let path = dir.join("global.duckdb");
                                        match crate::services::query_runner::execute_sql(&path, &sql) {
                                            Ok(out) => {
                                                *query_result.borrow_mut() = Some(out);
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询完成，返回 {} 行", {
                                                        let r = query_result.borrow();
                                                        r.as_ref().map(|o| o.row_count).unwrap_or(0)
                                                    }));
                                            }
                                            Err(e) => {
                                                *query_result.borrow_mut() = None;
                                                *shared.notice.borrow_mut() = Some(format!("查询失败: {}", e));
                                            }
                                        }
                                        entity.update(app, |_, cx| cx.notify());
                                    }),'''
new2 = '''                                    .on_click(move |_, _, app| {
                                        let sql = sql_state.read(app).value().to_string();
                                        let dir = crate::services::workspace_loader::default_global_dir();
                                        let path = dir.join("global.duckdb");
                                        match crate::services::query_runner::execute_sql(&path, &sql) {
                                            Ok(out) => {
                                                let n = out.row_count;
                                                *qr_closure.borrow_mut() = Some(out);
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询完成，返回 {} 行", n));
                                            }
                                            Err(e) => {
                                                *qr_closure.borrow_mut() = None;
                                                *shared.notice.borrow_mut() = Some(format!("查询失败: {}", e));
                                            }
                                        }
                                        entity.update(app, |_, cx| cx.notify());
                                    }),'''
assert t.count(old2) == 1
t = t.replace(old2, new2)

p.write_text(t, encoding='utf-8')
print('E0382 fixed')
