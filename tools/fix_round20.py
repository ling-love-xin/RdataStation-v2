import pathlib

# ---------- view.rs ----------
p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\view.rs')
t = p.read_text(encoding='utf-8')

# 1. DockArea import
t = t.replace(
    'use gpui_kit::component::dock::{panel_handle, DockLayout, DockPlacement, DockSkin};',
    'use gpui_kit::component::dock::{panel_handle, DockArea, DockLayout, DockPlacement, DockSkin};',
)
# 2. subscribe 4 参闭包 + idx
t = t.replace(
    'cx.subscribe(&sidebar, |this, event: &SidebarEvent, cx| {',
    'cx.subscribe(&sidebar, |this, event: &SidebarEvent, _window, cx| {',
)
# 3. 删 tooltip x2
t = t.replace(
    '                    .tooltip("收起/展开侧边栏".into())\n',
    '',
)
t = t.replace(
    '                    .tooltip(tool.label().into())\n',
    '',
)
p.write_text(t, encoding='utf-8')
print('view.rs patched')

# ---------- panels.rs ----------
p = pathlib.Path(r'D:\RdataStation\RDS\RdataStation-v2\crates\workbench\src\panels.rs')
t = p.read_text(encoding='utf-8')

# 4. EventEmitter trait 显式导入
t = t.replace(
    'use gpui_kit::prelude::FluentBuilder as _;',
    'use gpui_kit::prelude::FluentBuilder as _;\nuse gpui_kit::EventEmitter;',
)
# 5. notice RefCell
t = t.replace(
    '    pub notice: Rc<Cell<Option<String>>>,\n',
    '    pub notice: Rc<RefCell<Option<String>>>,\n',
)
t = t.replace(
    '            notice: Rc::new(Cell::new(None)),',
    '            notice: Rc::new(RefCell::new(None)),',
)
t = t.replace(
    '        let notice = self.shared.notice.get();',
    '        let notice = self.shared.notice.borrow().clone();',
)
t = t.replace(
    '                        shared.notice.set(Some("连接表单将在下一轮接入（M3 ConnectionService 已就绪）".into()));',
    '                        *shared.notice.borrow_mut() = Some("连接表单将在下一轮接入（M3 ConnectionService 已就绪）".into());',
)
# 6. selected_connection 借用修复
t = t.replace(
    '''    pub fn selected_connection(&self) -> Option<ConnectionItem> {
        self.selected.get().and_then(|i| self.connections.borrow().get(i)).cloned()
    }''',
    '''    pub fn selected_connection(&self) -> Option<ConnectionItem> {
        let conns = self.connections.borrow();
        self.selected.get().and_then(|i| conns.get(i)).cloned()
    }''',
)
# 7. SidebarPanel render 的 E0502：theme 拷贝
t = t.replace(
    '''        let theme = cx.theme();
        let tool = self.shared.active_tool.get();
        let content: Div = match tool {
            Tool::Connections => self.render_connection_list(cx),
            Tool::Navigation => self.render_navigation_placeholder(theme.colors.foreground),
            Tool::Resources => self.render_resources_placeholder(theme.colors.foreground),
            Tool::Settings => self.render_settings_placeholder(theme.colors.foreground),
        };
        div().v_flex().size_full().min_h_0().bg(theme.colors.background).child(content)''',
    '''        let bg = cx.theme().colors.background;
        let fg = cx.theme().colors.foreground;
        let tool = self.shared.active_tool.get();
        let content: Div = match tool {
            Tool::Connections => self.render_connection_list(cx),
            Tool::Navigation => self.render_navigation_placeholder(fg),
            Tool::Resources => self.render_resources_placeholder(fg),
            Tool::Settings => self.render_settings_placeholder(fg),
        };
        div().v_flex().size_full().min_h_0().bg(bg).child(content)''',
)
p.write_text(t, encoding='utf-8')
print('panels.rs patched')
