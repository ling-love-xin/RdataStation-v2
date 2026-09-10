# -*- coding: utf-8 -*-
"""Round 25：Shared 加导航缓存 + EditorPanel 渲染导航树。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# ---- 1. use 引入 NavTable ----
old_use = 'use crate::view::{ConnectionItem, Tool};'
assert t.count(old_use) == 1
t = t.replace(old_use, old_use + '\nuse crate::services::db_navigator::NavTable;')

# ---- 2. Shared 结构 ----
old_shared = '''pub struct Shared {
    pub active_tool: Rc<Cell<Tool>>,
    pub selected: Rc<Cell<Option<usize>>>,
    pub connections: Rc<RefCell<Vec<ConnectionItem>>>,
    pub notice: Rc<RefCell<Option<String>>>,
}'''
new_shared = '''pub struct Shared {
    pub active_tool: Rc<Cell<Tool>>,
    pub selected: Rc<Cell<Option<usize>>>,
    pub connections: Rc<RefCell<Vec<ConnectionItem>>>,
    pub notice: Rc<RefCell<Option<String>>>,
    /// Round 25：数据库导航缓存（哪个连接加载的 + 表→列树）。
    pub nav_for: Rc<RefCell<Option<String>>>,
    pub nav_tables: Rc<RefCell<Vec<NavTable>>>,
}'''
assert t.count(old_shared) == 1
t = t.replace(old_shared, new_shared)

old_init = '''        Self {
            active_tool: Rc::new(Cell::new(Tool::Connections)),
            selected: Rc::new(Cell::new(if connections.is_empty() { None } else { Some(0) })),
            connections: Rc::new(RefCell::new(connections)),
            notice: Rc::new(RefCell::new(notice)),
        }'''
new_init = '''        Self {
            active_tool: Rc::new(Cell::new(Tool::Connections)),
            selected: Rc::new(Cell::new(if connections.is_empty() { None } else { Some(0) })),
            connections: Rc::new(RefCell::new(connections)),
            notice: Rc::new(RefCell::new(notice)),
            nav_for: Rc::new(RefCell::new(None)),
            nav_tables: Rc::new(RefCell::new(Vec::new())),
        }'''
assert t.count(old_init) == 1
t = t.replace(old_init, new_init)

# ---- 3. 详情块导航区：加在删除按钮之后（.child(rows) 已变为 .child(rows).child(删除按钮) 链）
# 定位删除按钮闭包结束后的内容块结束处：`                    ),\n            );\n        }\n\n        content = content.child(\n            Button::new("new-connection")`
nav_anchor = '''                    ),
            );
        }

        content = content.child(
            Button::new("new-connection")'''
nav_block = '''                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .mt(px(8.))
                            .pt(px(8.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("数据库导航（DuckDB 分析库）"),
                            ),
                    );
            }
        }

        // Round 25：数据库导航区——选中联邦连接时按需加载分析库元数据树。
        if let Some(item) = self.shared.selected_connection() {
            if item.use_duckdb_fed {
                if self.shared.nav_for.borrow().as_deref() != Some(item.id.as_str()) {
                    let dir = default_global_dir();
                    let path = dir.join("global.duckdb");
                    let tree = crate::services::db_navigator::load_navigator_tree(&path)
                        .unwrap_or_default();
                    *self.shared.nav_tables.borrow_mut() = tree;
                    *self.shared.nav_for.borrow_mut() = Some(item.id.clone());
                }
                let nav = self.shared.nav_tables.borrow();
                let mut nav_content = div().v_flex().gap_1().mt(px(6.));
                if nav.is_empty() {
                    nav_content = nav_content.child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("分析库暂无表——运行 seed_demo 或导入数据"),
                    );
                } else {
                    for table in nav.iter() {
                        nav_content = nav_content.child(
                            div()
                                .v_flex()
                                .gap_1()
                                .child(
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
                                )
                                .child(
                                    div().v_flex().gap_1().pl(px(12.)).children(
                                        table.columns.iter().map(|col| {
                                            div()
                                                .h_flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .w(px(130.))
                                                        .flex_none()
                                                        .text_xs()
                                                        .child(col.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .text_xs()
                                                        .text_color(theme.colors.muted_foreground)
                                                        .child(col.data_type.clone()),
                                                )
                                                .child(
                                                    if col.is_primary_key {
                                                        div()
                                                            .text_xs()
                                                            .text_color(theme.colors.info)
                                                            .child("PK")
                                                    } else {
                                                        div().text_xs().child("")
                                                    },
                                                )
                                        }),
                                    ),
                                ),
                        );
                    }
                }
                content = content.child(nav_content);
            }
        }

        content = content.child(
            Button::new("new-connection")'''
assert t.count(nav_anchor) == 1, 'nav_anchor count %d' % t.count(nav_anchor)
t = t.replace(nav_anchor, nav_block)

p.write_text(t, encoding='utf-8')
print('nav tree added')
