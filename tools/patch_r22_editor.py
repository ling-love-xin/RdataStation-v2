# -*- coding: utf-8 -*-
"""Round 22：EditorPanel 渲染真实连接详情（键值行卡片）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

old = '''impl Render for EditorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tool = self.shared.active_tool.get();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();

        let mut content = div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .bg(theme.colors.background)
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.colors.foreground)
                    .child("数据源连接"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.colors.muted_foreground)
                    .child("选中连接后可在状态栏查看上下文；本地加速（DuckDB Secret）将在连接建立后自动注册"),
            )
            .child(
                Button::new("new-connection")
                    .primary()
                    .icon(IconName::Plus)
                    .label("新建连接")
                    .on_click(move |_, _, app| {
                        let shared = entity.read(app).shared.clone();
                        *shared.notice.borrow_mut() = Some("连接表单将在下一轮接入（M3 ConnectionService 已就绪）".into());
                        entity.update(app, |_, cx| cx.notify());
                    }),
            );

        if let Some(item) = self.shared.selected_connection() {
            content = content.child(
                div()
                    .v_flex()
                    .items_center()
                    .gap_1()
                    .rounded_md()
                    .pl(px(16.))
                    .pr(px(16.))
                    .pt(px(12.))
                    .pb(px(12.))
                    .border_1()
                    .border_color(theme.colors.border)
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(item.name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("驱动：{} ｜ 状态：{}", item.driver, if item.connected { "已连接" } else { "未连接" })),
                    ),
            );
        }

        if notice.is_some() {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.info)
                    .child(notice.unwrap()),
            );
        }

        if tool != Tool::Connections {
            content = content.child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.colors.foreground)
                    .child(tool.label()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.colors.muted_foreground)
                    .child("视图骨架已就绪，业务面板在下一轮接入"),
            );
        }

        content
    }'''
new = '''impl Render for EditorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tool = self.shared.active_tool.get();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();

        let mut content = div()
            .v_flex()
            .size_full()
            .pt(px(16.))
            .pl(px(16.))
            .pr(px(16.))
            .gap_3()
            .bg(theme.colors.background)
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.colors.foreground)
                            .child("数据源连接"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("选中连接查看真实元数据；本地加速（DuckDB Secret）将在连接建立后自动注册"),
                    ),
            );

        // Round 22：选中连接 → 真实元数据详情卡片（键值行）。
        if let Some(item) = self.shared.selected_connection() {
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
                    .child(rows),
            );
        }

        content = content.child(
            Button::new("new-connection")
                .primary()
                .icon(IconName::Plus)
                .label("新建连接")
                .on_click(move |_, _, app| {
                    let shared = entity.read(app).shared.clone();
                    *shared.notice.borrow_mut() = Some("连接表单将在下一轮接入（M3 ConnectionService 已就绪）".into());
                    entity.update(app, |_, cx| cx.notify());
                }),
        );

        if notice.is_some() {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.info)
                    .child(notice.unwrap()),
            );
        }

        if tool != Tool::Connections {
            content = content.child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.colors.foreground)
                    .child(tool.label()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.colors.muted_foreground)
                    .child("视图骨架已就绪，业务面板在下一轮接入"),
            );
        }

        content
    }'''
assert old in t, 'EditorPanel render not found'
t = t.replace(old, new)
p.write_text(t, encoding='utf-8')
print('EditorPanel render rewritten')
