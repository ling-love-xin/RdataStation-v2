# -*- coding: utf-8 -*-
"""R29：panels.rs 接入 SQL 历史——字段、初始化、执行后写入、历史列表渲染。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 1) struct 字段
old = '''    // Round 26：SQL 查询区（受控输入 + 结果集）。
    sql_textarea: Option<Entity<TextareaState>>,
    query_result: Rc<RefCell<Option<QueryOutput>>>,
}'''
new = '''    // Round 26：SQL 查询区（受控输入 + 结果集）。
    sql_textarea: Option<Entity<TextareaState>>,
    query_result: Rc<RefCell<Option<QueryOutput>>>,
    // Round 29：SQL 历史（最新在前，跨会话持久化）。
    sql_history: Rc<RefCell<Vec<String>>>,
}'''
assert t.count(old) == 1
t = t.replace(old, new)

# 2) new() 初始化
old = '''            sql_textarea: None,
            query_result: Rc::new(RefCell::new(None)),
        }
    }
}'''
new = '''            sql_textarea: None,
            query_result: Rc::new(RefCell::new(None)),
            sql_history: Rc::new(RefCell::new(
                crate::services::query_history::load_history(),
            )),
        }
    }
}'''
assert t.count(old) == 1
t = t.replace(old, new)

# 3) SQL 区开头：为历史按钮提前克隆 entity 副本
old = '''                let shared = self.shared.clone();
                let shared_export = self.shared.clone();
                let entity = entity.clone();
                let entity_export = entity.clone();
'''
new = '''                let shared = self.shared.clone();
                let shared_export = self.shared.clone();
                let entity = entity.clone();
                let entity_export = entity.clone();
                let entity_hist = entity.clone();
'''
assert t.count(old) == 1
t = t.replace(old, new)

# 4) 执行按钮：Ok 时写入历史（entity.update 内更新 self.sql_history）
old = '''                                        match crate::services::query_runner::execute_sql(&path, &sql) {
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
                                        entity.update(app, |_, cx| cx.notify());'''
new = '''                                        let ok = match crate::services::query_runner::execute_sql(
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
                                        };
                                        if ok {
                                            entity.update(app, |this, cx| {
                                                if let Ok(hist) = crate::services::query_history::append_history(&sql) {
                                                    *this.sql_history.borrow_mut() = hist;
                                                }
                                                cx.notify();
                                            });
                                        } else {
                                            entity.update(app, |_, cx| cx.notify());
                                        }'''
assert t.count(old) == 1
t = t.replace(old, new)

# 5) 历史列表渲染：插入在结果集读取之前（执行按钮行之后）
old = '''                let result = query_result.borrow().clone();'''
new = '''                // Round 29：SQL 历史——点击回填到编辑器（最新在前，截断预览）。
                {
                    let history = self.sql_history.borrow();
                    if !history.is_empty() {
                        let mut hist_ui = div().v_flex().gap_1().mt(px(4.));
                        hist_ui = hist_ui.child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.colors.muted_foreground)
                                .child("历史"),
                        );
                        for (i, sql) in history.iter().enumerate() {
                            let preview: String = if sql.chars().count() > 42 {
                                let mut s: String = sql.chars().take(42).collect();
                                s.push('…');
                                s
                            } else {
                                sql.clone()
                            };
                            let sql_clone = sql.clone();
                            let st = sql_state.clone();
                            let e_hist = entity_hist.clone();
                            hist_ui = hist_ui.child(
                                div()
                                    .id(ElementId::Name(SharedString::from(format!("hist-{i}"))))
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .cursor_pointer()
                                    .child(preview)
                                    .on_click(move |_, window, app| {
                                        st.update(app, |s, cx| {
                                            s.set_value(sql_clone.clone().into(), window, cx)
                                        });
                                        e_hist.update(app, |_, cx| cx.notify());
                                    }),
                            );
                        }
                        sql_ui = sql_ui.child(hist_ui);
                    }
                }

                let result = query_result.borrow().clone();'''
assert t.count(old) == 1
t = t.replace(old, new)

p.write_text(t, encoding='utf-8')
print('history wired')
