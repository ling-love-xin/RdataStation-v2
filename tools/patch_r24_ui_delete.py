# -*- coding: utf-8 -*-
"""Round 24：EditorPanel 详情卡片加删除连接按钮。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 1) 详情块开头：克隆 shared + 取 id/name
anchor1 = '''        if let Some(item) = self.shared.selected_connection() {
            let status = if item.connected { "已连接" } else { "未连接" };'''
new1 = '''        if let Some(item) = self.shared.selected_connection() {
            let shared = self.shared.clone();
            let selected_id = item.id.clone();
            let selected_name = item.name.clone();
            let status = if item.connected { "已连接" } else { "未连接" };'''
assert t.count(anchor1) == 1
t = t.replace(anchor1, new1)

# 2) 详情卡片末尾：rows 后加删除按钮
anchor2 = '''                    .child(rows),
            );
        }

        content = content.child(
            Button::new("new-connection")'''
del_btn = '''                    .child(rows)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .mt(px(8.))
                            .child(
                                Button::new("delete-connection")
                                    .danger()
                                    .label("删除连接")
                                    .on_click(move |_, _, app| {
                                        match crate::services::workspace_loader::delete_connection(&selected_id) {
                                            Ok(()) => {
                                                let (items, _) =
                                                    crate::services::workspace_loader::load_persisted_connections();
                                                *shared.connections.borrow_mut() = items;
                                                shared.selected.set(None);
                                                *shared.notice.borrow_mut() =
                                                    Some(format!("连接「{}」已删除", selected_name));
                                            }
                                            Err(e) => {
                                                *shared.notice.borrow_mut() = Some(format!("删除失败: {}", e));
                                            }
                                        }
                                        entity.update(app, |_, cx| cx.notify());
                                    }),
                            ),
                    ),
            );
        }

        content = content.child(
            Button::new("new-connection")'''
assert t.count(anchor2) == 1, 'anchor2 count %d' % t.count(anchor2)
t = t.replace(anchor2, del_btn)
p.write_text(t, encoding='utf-8')
print('delete button added')
