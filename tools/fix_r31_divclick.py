# -*- coding: utf-8 -*-
"""用 div+on_click 替代 Button 实现 Mock 生成入口。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''                content = content.child(
                    Button::new("run-mock")
                        .secondary()
                        .label("生成 Mock（首表）"),
                );
'''
new = '''                content = content.child(
                    div()
                        .id("run-mock-div")
                        .cursor_pointer()
                        .rounded_md()
                        .px(px(10.))
                        .py(px(6.))
                        .bg(theme.colors.surface)
                        .border_1()
                        .border_color(theme.colors.border)
                        .text_sm()
                        .text_color(theme.colors.foreground)
                        .child("生成 Mock（首表）：基于导航树首表生成 50 行，仅落分析库")
                        .on_click({
                            let shared_mock = self.shared.clone();
                            let entity_mock = cx.entity();
                            move |_, _, app| {
                                let table = shared_mock.nav_tables.borrow().first().cloned();
                                match table {
                                    Some(tbl) => {
                                        let dir = crate::services::workspace_loader::default_global_dir();
                                        let path = dir.join("global.duckdb");
                                        match crate::services::mock_generator::generate_for_table(
                                            &path,
                                            &tbl,
                                            50,
                                        ) {
                                            Ok(out) => {
                                                *shared_mock.notice.borrow_mut() = Some(format!(
                                                    "已生成 {} 行到表 {}（耗时 {}ms，仅落分析库）",
                                                    out.row_count, tbl.name, out.elapsed_ms
                                                ));
                                            }
                                            Err(e) => {
                                                *shared_mock.notice.borrow_mut() =
                                                    Some(format!("Mock 生成失败: {}", e));
                                            }
                                        }
                                    }
                                    None => {
                                        *shared_mock.notice.borrow_mut() =
                                            Some("导航树为空——先选中联邦连接加载元数据".to_string());
                                    }
                                }
                                entity_mock.update(app, |_, cx| cx.notify());
                            }
                        }),
                );
'''
assert t.count(old) == 1, f'count={t.count(old)}'
p.write_text(t.replace(old, new), encoding='utf-8')
print('div+on_click mock entry')
