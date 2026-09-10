# -*- coding: utf-8 -*-
"""R31：导航树每表行加「Mock」按钮（M7 入口，生成 50 行到分析库）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''                                .child(
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(format!("{}（{} 列）", table.name, table.columns.len())),
                                        ),
                                )'''
new = '''                                .child(
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(format!("{}（{} 列）", table.name, table.columns.len())),
                                        )
                                        // Round 31：M7 mock——一键生成 50 行测试数据到分析引擎库（不回传源库）。
                                        .child(
                                            Button::new(format!("mock-{}", table.name))
                                                .ghost()
                                                .label("Mock")
                                                .on_click({
                                                    let table_clone = table.clone();
                                                    let shared_mock = self.shared.clone();
                                                    let entity_mock = cx.entity();
                                                    move |_, _, app| {
                                                        let dir = crate::services::workspace_loader::default_global_dir();
                                                        let path = dir.join("global.duckdb");
                                                        match crate::services::mock_generator::generate_for_table(
                                                            &path,
                                                            &table_clone,
                                                            50,
                                                        ) {
                                                            Ok(out) => {
                                                                *shared_mock.notice.borrow_mut() = Some(format!(
                                                                    "已生成 {} 行到表 {}（耗时 {}ms，仅落分析库）",
                                                                    out.row_count, table_clone.name, out.elapsed_ms
                                                                ));
                                                            }
                                                            Err(e) => {
                                                                *shared_mock.notice.borrow_mut() =
                                                                    Some(format!("Mock 生成失败: {}", e));
                                                            }
                                                        }
                                                        entity_mock.update(app, |_, cx| cx.notify());
                                                    }
                                                }),
                                        ),
                                )'''
assert t.count(old) == 1
t = t.replace(old, new)

p.write_text(t, encoding='utf-8')
print('mock button inserted')
