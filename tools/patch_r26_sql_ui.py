# -*- coding: utf-8 -*-
"""Round 26：EditorPanel 加 SQL 查询区（输入 + 执行 + 结果集）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# ---- 1. use QueryOutput ----
old_use = 'use crate::services::db_navigator::NavTable;'
new_use = old_use + '\nuse crate::services::query_runner::QueryOutput;'
assert t.count(old_use) == 1
t = t.replace(old_use, new_use)

# ---- 2. EditorPanel 字段 ----
old_struct = '''    form_user: Option<Entity<InputState>>,
    form_pass: Option<Entity<InputState>>,
}'''
new_struct = '''    form_user: Option<Entity<InputState>>,
    form_pass: Option<Entity<InputState>>,
    // Round 26：SQL 查询区（受控输入 + 结果集）。
    sql_input: Option<Entity<InputState>>,
    query_result: Rc<RefCell<Option<QueryOutput>>>,
}'''
assert t.count(old_struct) == 1
t = t.replace(old_struct, new_struct)

old_new = '''            show_form: false,
            form_name: None,
            form_driver: None,
            form_url: None,
            form_user: None,
            form_pass: None,
        }'''
new_new = '''            show_form: false,
            form_name: None,
            form_driver: None,
            form_url: None,
            form_user: None,
            form_pass: None,
            sql_input: None,
            query_result: Rc::new(RefCell::new(None)),
        }'''
assert t.count(old_new) == 1
t = t.replace(old_new, new_new)

# ---- 3. 懒创建块加 sql_input ----
old_lazy = '''            self.form_user = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_pass = Some(cx.new(|cx| InputState::new(window, cx)));
        }'''
new_lazy = '''            self.form_user = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_pass = Some(cx.new(|cx| InputState::new(window, cx)));
            self.sql_input = Some(cx.new(|cx| InputState::new(window, cx)));
        }'''
assert t.count(old_lazy) == 1
t = t.replace(old_lazy, new_lazy)

# ---- 4. SQL 查询区渲染：插在导航区闭合后、新建连接按钮前 ----
# 导航区结束后是 "        content = content.child(\n            Button::new(\"new-connection\")"
anchor = '''        content = content.child(
            Button::new("new-connection")'''
sql_block = '''        // Round 26：SQL 查询区——选中联邦连接时可对分析库执行只读 SQL。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed {
                let sql_state = self.sql_input.clone().expect("lazy init");
                let query_result = self.query_result.clone();
                let shared = self.shared.clone();
                let entity = entity.clone();

                let mut sql_ui = div()
                    .v_flex()
                    .gap_1()
                    .mt(px(8.))
                    .pt(px(8.))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("SQL 查询（DuckDB 分析库）"),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .w_full()
                            .child(Input::new(&sql_state))
                            .child(
                                Button::new("run-sql")
                                    .secondary()
                                    .label("执行")
                                    .on_click(move |_, _, app| {
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
                                    }),
                            ),
                    );

                let result = query_result.borrow().clone();
                if let Some(out) = result {
                    if out.columns.is_empty() {
                        sql_ui = sql_ui.child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("无结果（DDL/无返回行）"),
                        );
                    } else {
                        let mut table = div().v_flex().gap_1();
                        // 列头
                        let mut head = div().h_flex().gap_2();
                        for col in &out.columns {
                            head = head.child(
                                div()
                                    .w(px(150.))
                                    .flex_none()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(col.clone()),
                            );
                        }
                        table = table.child(head);
                        // 行
                        for row in &out.rows {
                            let mut row_div = div().h_flex().gap_2();
                            for v in row {
                                row_div = row_div.child(
                                    div()
                                        .w(px(150.))
                                        .flex_none()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child(v.clone()),
                                );
                            }
                            table = table.child(row_div);
                        }
                        sql_ui = sql_ui.child(table);
                    }
                }
                content = content.child(sql_ui);
            }
        }

        content = content.child(
            Button::new("new-connection")'''
assert t.count(anchor) == 1, 'sql anchor count %d' % t.count(anchor)
t = t.replace(anchor, sql_block)

p.write_text(t, encoding='utf-8')
print('SQL query area added')
