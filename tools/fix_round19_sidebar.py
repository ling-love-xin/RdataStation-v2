import pathlib

p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs')
t = p.read_text(encoding='utf-8')

old = '''    /// 侧边栏（按当前工具渲染内容）。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let content: Div = match self.active_tool {
            Tool::Connections => self.render_connection_list(cx),
            Tool::Navigation => self.render_navigation_placeholder(theme.colors.foreground),
            Tool::Resources => self.render_resources_placeholder(theme.colors.foreground),
            Tool::Settings => self.render_settings_placeholder(theme.colors.foreground),
        };
        let entity = cx.entity();

        div()
            .v_flex()
            .w(px(240.))
            .h_full()
            .flex_none()
            .min_h_0()
            .border_1()
            .border_color(theme.colors.border)
            .bg(theme.colors.background)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .h(px(32.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .border_1()
                    .border_color(theme.colors.border)'''

new = '''    /// 侧边栏（按当前工具渲染内容）。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors.border;
        let background = cx.theme().colors.background;
        let fg = cx.theme().colors.foreground;
        let content: Div = match self.active_tool {
            Tool::Connections => self.render_connection_list(cx),
            Tool::Navigation => self.render_navigation_placeholder(fg),
            Tool::Resources => self.render_resources_placeholder(fg),
            Tool::Settings => self.render_settings_placeholder(fg),
        };
        let entity = cx.entity();

        div()
            .v_flex()
            .w(px(240.))
            .h_full()
            .flex_none()
            .min_h_0()
            .border_1()
            .border_color(border)
            .bg(background)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .h(px(32.))
                    .pl(px(12.))
                    .pr(px(8.))
                    .border_1()
                    .border_color(border)'''

assert old in t, 'old block not found'
t = t.replace(old, new, 1)
p.write_text(t, encoding='utf-8')
print('patched render_sidebar')
