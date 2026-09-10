# -*- coding: utf-8 -*-
"""Round 23：EditorPanel render 表单集成（懒建 inputs + toggle + 保存）。"""
import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# ---- 1. render 签名 + 懒建 inputs ----
old_sig = '''    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tool = self.shared.active_tool.get();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();'''
new_sig = '''    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tool = self.shared.active_tool.get();
        let notice = self.shared.notice.borrow().clone();
        let entity = cx.entity();

        // Round 23：受控输入懒创建（render 首次初始化，需要 window）。
        if self.form_name.is_none() {
            self.form_name = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_driver = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_url = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_user = Some(cx.new(|cx| InputState::new(window, cx)));
            self.form_pass = Some(cx.new(|cx| InputState::new(window, cx)));
        }'''
assert old_sig in t
t = t.replace(old_sig, new_sig)

# ---- 2. 新建连接按钮区 → toggle + 表单 ----
old_btn = '''        content = content.child(
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

        if notice.is_some() {'''
new_btn = '''        content = content.child(
            Button::new("new-connection")
                .primary()
                .icon(IconName::Plus)
                .label(if self.show_form { "收起表单" } else { "新建连接" })
                .on_click(move |_, _, app| {
                    entity.update(app, |editor, cx| {
                        editor.show_form = !editor.show_form;
                        cx.notify();
                    });
                }),
        );

        // Round 23：新建连接表单（保存后真实落库并刷新列表）。
        if self.show_form {
            let name_state = self.form_name.clone().expect("lazy init");
            let driver_state = self.form_driver.clone().expect("lazy init");
            let url_state = self.form_url.clone().expect("lazy init");
            let user_state = self.form_user.clone().expect("lazy init");
            let pass_state = self.form_pass.clone().expect("lazy init");
            let shared = self.shared.clone();
            let entity = cx.entity();

            content = content.child(
                div()
                    .v_flex()
                    .gap_2()
                    .w_full()
                    .max_w(px(460.))
                    .rounded_md()
                    .pl(px(12.))
                    .pr(px(12.))
                    .pt(px(10.))
                    .pb(px(10.))
                    .border_1()
                    .border_color(theme.colors.border)
                    .child(
                        Form::vertical()
                            .label_width(px(64.))
                            .child(Field::new().label("名称").child(Input::new(&name_state)))
                            .child(Field::new().label("驱动").child(Input::new(&driver_state)))
                            .child(Field::new().label("URL").child(Input::new(&url_state)))
                            .child(Field::new().label("用户名").child(Input::new(&user_state)))
                            .child(Field::new().label("密码").child(Input::new(&pass_state))),
                    )
                    .child(
                        div().text_xs().text_color(theme.colors.muted_foreground)
                            .child("驱动填 mysql / postgres / sqlite / duckdb；URL 形如 mysql://host:3306/db"),
                    )
                    .child(
                        Button::new("save-connection")
                            .secondary()
                            .label("保存连接")
                            .on_click(move |_, window, app| {
                                let name = name_state.read(app).value().to_string();
                                let driver = driver_state.read(app).value().to_string();
                                let url = url_state.read(app).value().to_string();
                                let user = user_state.read(app).value().to_string();
                                let pass = pass_state.read(app).value().to_string();

                                if name.trim().is_empty() || driver.trim().is_empty() || url.trim().is_empty() {
                                    *shared.notice.borrow_mut() = Some("名称、驱动、URL 不能为空".into());
                                    entity.update(app, |_, cx| cx.notify());
                                    return;
                                }

                                match crate::services::workspace_loader::save_connection(
                                    &name,
                                    &driver,
                                    &url,
                                    &user,
                                    &pass,
                                ) {
                                    Ok(()) => {
                                        // 刷新真实连接列表。
                                        let (items, _) =
                                            crate::services::workspace_loader::load_persisted_connections();
                                        *shared.connections.borrow_mut() = items;
                                        *shared.notice.borrow_mut() = Some(format!("连接「{}」已保存", name));
                                        // 清空表单。
                                        for (state, value) in [
                                            (&name_state, ""),
                                            (&driver_state, ""),
                                            (&url_state, ""),
                                            (&user_state, ""),
                                            (&pass_state, ""),
                                        ] {
                                            state.update(app, |s, cx| s.set_value(value, window, cx));
                                        }
                                    }
                                    Err(e) => {
                                        *shared.notice.borrow_mut() = Some(format!("保存失败: {}", e));
                                    }
                                }
                                entity.update(app, |_, cx| cx.notify());
                            }),
                    ),
            );
        }

        if notice.is_some() {'''
assert old_btn in t
t = t.replace(old_btn, new_btn)

p.write_text(t, encoding='utf-8')
print('editor render patched')
