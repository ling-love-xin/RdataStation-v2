# -*- coding: utf-8 -*-
"""R27：结果表格后加「导出 CSV」按钮行。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''                            table = table.child(row_div);
                        }
                        sql_ui = sql_ui.child(table);
                    }
                }
                content = content.child(sql_ui);'''
new = '''                            table = table.child(row_div);
                        }
                        sql_ui = sql_ui.child(table);

                        // Round 27：导出 CSV——结果落盘到全局目录 results/ 并按时间戳命名。
                        let out_clone = out.clone();
                        let shared_export = shared.clone();
                        let entity_export = entity.clone();
                        sql_ui = sql_ui.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child(format!("共 {} 行", out.rows.len())),
                                )
                                .child(
                                    Button::new("export-csv")
                                        .ghost()
                                        .label("导出 CSV")
                                        .on_click(move |_, _, app| {
                                            match crate::services::query_export::export_to_default(
                                                &out_clone,
                                            ) {
                                                Ok(path) => {
                                                    *shared_export.notice.borrow_mut() = Some(
                                                        format!(
                                                            "已导出: {}",
                                                            path.display()
                                                        ),
                                                    );
                                                }
                                                Err(e) => {
                                                    *shared_export.notice.borrow_mut() = Some(e);
                                                }
                                            }
                                            entity_export.update(app, |_, cx| cx.notify());
                                        }),
                                ),
                        );
                    }
                }
                content = content.child(sql_ui);'''
assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('export button inserted')
