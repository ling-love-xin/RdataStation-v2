# -*- coding: utf-8 -*-
"""R31 修正：Mock 入口移到 SQL 区下方（固定 id 按钮，作用于导航树首表）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''                content = content.child(sql_ui);
            }
        }'''
new = '''                // Round 31：M7 Mock——生成测试数据到分析引擎库（仅落 duckdb，不回传源库）。
                // 作用于导航树第一张表，固定生成 50 行（后续轮开放行数/表选择配置）。
                sql_ui = sql_ui.child(
                    div()
                        .v_flex()
                        .gap_1()
                        .mt(px(8.))
                        .pt(px(8.))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Mock 数据（生成测试数据）"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("基于导航树表结构自动生成 50 行，仅落分析引擎库，不回传源库"),
                        )
                        .child(
                            Button::new("run-mock")
                                .secondary()
                                .label("生成 Mock（首表）")
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
                        ),
                );

                content = content.child(sql_ui);
            }
        }'''
assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('mock section added to sql_ui')
