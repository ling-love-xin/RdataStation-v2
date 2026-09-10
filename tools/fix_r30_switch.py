# -*- coding: utf-8 -*-
"""R30：连接切换联动——SQL 结果归属 sql_for + 切换清空导航/查询残留。"""
import pathlib

# ============ panels.rs ============
p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 1) Shared struct 加 sql_for
old = '''    /// Round 25：数据库导航缓存（哪个连接加载的 + 表→列树）。
    pub nav_for: Rc<RefCell<Option<String>>>,
    pub nav_tables: Rc<RefCell<Vec<NavTable>>>,
}'''
new = '''    /// Round 25：数据库导航缓存（哪个连接加载的 + 表→列树）。
    pub nav_for: Rc<RefCell<Option<String>>>,
    pub nav_tables: Rc<RefCell<Vec<NavTable>>>,
    /// Round 30：SQL 结果归属（哪个连接执行的，切换连接即失效）。
    pub sql_for: Rc<RefCell<Option<String>>>,
}'''
assert t.count(old) == 1
t = t.replace(old, new)

# 2) with_connections 初始化
old = '''            nav_for: Rc::new(RefCell::new(None)),
            nav_tables: Rc::new(RefCell::new(Vec::new())),
        }
    }'''
new = '''            nav_for: Rc::new(RefCell::new(None)),
            nav_tables: Rc::new(RefCell::new(Vec::new())),
            sql_for: Rc::new(RefCell::new(None)),
        }
    }'''
assert t.count(old) == 1
t = t.replace(old, new)

# 3) SQL 区开头：conn_id + shared_view（渲染校验用，执行闭包 move shared 之前）
old = '''                let sql_state = self.sql_textarea.clone().expect("lazy init");
                let sql_state_hist = sql_state.clone();
                let query_result = self.query_result.clone();
                let qr_closure = query_result.clone();
                let shared = self.shared.clone();
                let shared_export = self.shared.clone();
                let entity = entity.clone();
                let entity_export = entity.clone();
                let entity_hist = entity.clone();
'''
new = '''                let sql_state = self.sql_textarea.clone().expect("lazy init");
                let sql_state_hist = sql_state.clone();
                let query_result = self.query_result.clone();
                let qr_closure = query_result.clone();
                let shared = self.shared.clone();
                let shared_export = self.shared.clone();
                let shared_view = self.shared.clone();
                let entity = entity.clone();
                let entity_export = entity.clone();
                let entity_hist = entity.clone();
                let conn_id = item.id.clone();
'''
assert t.count(old) == 1
t = t.replace(old, new)

# 4) 执行按钮：Ok 记录归属 / Err 清空归属
old = '''                                        let ok = match crate::services::query_runner::execute_sql(
                                            &path,
                                            &sql,
                                        ) {
                                            Ok(out) => {
                                                let n = out.row_count;
                                                *qr_closure.borrow_mut() = Some(out);
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询完成，返回 {} 行", n));
                                                true
                                            }
                                            Err(e) => {
                                                *qr_closure.borrow_mut() = None;
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询失败: {}", e));
                                                false
                                            }
                                        };'''
new = '''                                        let ok = match crate::services::query_runner::execute_sql(
                                            &path,
                                            &sql,
                                        ) {
                                            Ok(out) => {
                                                let n = out.row_count;
                                                *qr_closure.borrow_mut() = Some(out);
                                                *shared.sql_for.borrow_mut() = Some(conn_id.clone());
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询完成，返回 {} 行", n));
                                                true
                                            }
                                            Err(e) => {
                                                *qr_closure.borrow_mut() = None;
                                                *shared.sql_for.borrow_mut() = None;
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("查询失败: {}", e));
                                                false
                                            }
                                        };'''
assert t.count(old) == 1
t = t.replace(old, new)

# 5) 结果渲染：仅在结果归属 == 当前连接时显示
old = '''                let result = query_result.borrow().clone();
                if let Some(out) = result {'''
new = '''                let result_visible =
                    shared_view.sql_for.borrow().as_deref() == Some(item.id.as_str());
                let result = if result_visible {
                    query_result.borrow().clone()
                } else {
                    None
                };
                if let Some(out) = result {'''
assert t.count(old) == 1
t = t.replace(old, new)

p.write_text(t, encoding='utf-8')
print('panels.rs wired')

# ============ view.rs ============
v = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs')
t2 = v.read_text(encoding='utf-8')

old2 = '''                SidebarEvent::SelectConnection(idx) => {
                    this.shared.selected.set(Some(*idx));
                    if let Some(editor) = &this.editor {
                        editor.update(cx, |_, cx| cx.notify());
                    }
                }'''
new2 = '''                SidebarEvent::SelectConnection(idx) => {
                    this.shared.selected.set(Some(*idx));
                    // Round 30：切换连接 → 清空导航树 / SQL 结果残留，防止串数据。
                    *this.shared.nav_for.borrow_mut() = None;
                    this.shared.nav_tables.borrow_mut().clear();
                    *this.shared.sql_for.borrow_mut() = None;
                    if let Some(editor) = &this.editor {
                        editor.update(cx, |_, cx| cx.notify());
                    }
                }'''
assert t2.count(old2) == 1
t2 = t2.replace(old2, new2)
v.write_text(t2, encoding='utf-8')
print('view.rs wired')
