# -*- coding: utf-8 -*-
"""重建详情卡（从早先读取的 R22-R25 代码恢复）+ 子节点级打点。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''        // [SKIP] 详情卡临时跳过
        eprintln!("[dbg] EP-detail-done");'''

new = '''        // Round 22：选中连接 → 真实元数据详情卡片（键值行）。
        if let Some(item) = self.shared.selected_connection() {
            eprintln!("[dbg] EP-detail-entry");
            let shared = self.shared.clone();
            let entity = entity.clone();
            let selected_id = item.id.clone();
            let selected_name = item.name.clone();
            let status = if item.connected { "已连接" } else { "未连接" };
            let status_color = if item.connected { theme.colors.success } else { theme.colors.muted };
            let fields: Vec<(&'static str, String)> = vec![
                ("名称", item.name.clone()),
                ("驱动", item.driver.clone()),
                ("主机", item.host.clone().unwrap_or_else(|| "-".to_string())),
                ("端口", item.port.map(|p| p.to_string()).unwrap_or_else(|| "-".to_string())),
                ("数据库", item.database.clone().unwrap_or_else(|| "-".to_string())),
                ("Schema", item.schema.clone().unwrap_or_else(|| "-".to_string())),
                (
                    "DuckDB 联邦",
                    if item.use_duckdb_fed { "开启（本地加速）".to_string() } else { "关闭".to_string() },
                ),
                ("描述", item.description.clone().unwrap_or_else(|| "-".to_string())),
                ("创建时间", item.created_at.clone()),
                ("更新时间", item.updated_at.clone()),
            ];

            let mut rows = div().v_flex().gap_1();
            eprintln!("[dbg] EP-detail-rows-div");
            for (label, value) in fields {
                rows = rows.child(
                    div()
                        .h_flex()
                        .items_center()
                        .w_full()
                        .gap_2()
                        .child(
                            div()
                                .w(px(96.))
                                .flex_none()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme.colors.foreground)
                                .child(value),
                        ),
                );
            }
            eprintln!("[dbg] EP-detail-rows-done");
            content = content.child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .rounded_md()
                    .pl(px(12.))
                    .pr(px(12.))
                    .pt(px(10.))
                    .pb(px(10.))
                    .border_1()
                    .border_color(theme.colors.border)
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(item.name))
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .w(px(8.))
                                            .h(px(8.))
                                            .rounded_full()
                                            .bg(status_color),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.colors.muted_foreground)
                                            .child(status),
                                    ),
                            ),
                    )
                    .child(rows)
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
                    )
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
                    ),
            );
            eprintln!("[dbg] EP-detail-card-done");
        }

        eprintln!("[dbg] EP-detail-done");'''

assert t.count(old) == 1
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('detail card restored with sub-dbg')
