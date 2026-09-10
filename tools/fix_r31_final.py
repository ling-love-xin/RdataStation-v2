# -*- coding: utf-8 -*-
"""R31 收尾：清理残留打点半块 + 加回完整 Mock 小节（标题+说明+按钮+闭包）。"""
import pathlib

# 1. view.rs 清残留打点半块（WorkbenchView::render 开头 { use ...; static N...; let n...; }）
v = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs')
t = v.read_text(encoding='utf-8')
old = '''impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
        }
        if self.area.is_none() {'''
new = '''impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.area.is_none() {'''
if t.count(old) == 1:
    v.write_text(t.replace(old, new), encoding='utf-8')
    print('view.rs cleaned')
else:
    print(f'view.rs anchor mismatch: {t.count(old)}')

# 2. panels.rs 清残留打点半块 + 加回 Mock 小节
p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 2a. 清残留（render 入口半块）
old = '''impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
        }
        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。'''
new = '''impl Render for EditorPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。'''
if t.count(old) == 1:
    t = t.replace(old, new)
    print('panels render head cleaned')
else:
    print(f'panels render head mismatch: {t.count(old)}')

# 2b. 加回 Mock 小节（SQL 区末尾）
anchor = '''                content = content.child(sql_ui);
            }
        }'''
mock = '''                // Round 31：M7 Mock——基于导航树首表生成 50 行测试数据，仅落分析引擎库（DuckDB），不回传源库。
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

''' + anchor
if t.count(anchor) == 1:
    t = t.replace(anchor, mock)
    print('mock section added')
else:
    print(f'mock anchor mismatch: {t.count(anchor)}')

p.write_text(t, encoding='utf-8')
print('done')
