//! 面板顶部工具栏：新增表 / 引用结构 / 新增场景 / 刷新。

use super::*;

impl MockPanel {
    /// 标题栏下方的工具栏（2026-10-18 对齐原型）：新增 Mock 表（`Plus`）/ 引用结构（`HardDrive`）/
    /// 新增场景（`LayoutDashboard`）/ 刷新（`RefreshCw`）。
    ///
    /// 按钮语义全部是 `Button`（自带 hover / 焦点 / 键盘）；标题栏只留标题，
    /// 「＋」从标题栏工具栏迁到这里（原型同构：mock-toolbar 在面板头下方一行）。
    /// 行高走 `CONTROL_HEIGHT_SM`（与数据导航筛选行同档），底部分隔线用 `HAIRLINE`。
    pub(crate) fn render_toolbar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let running = self.is_running();
        let entity = cx.entity();

        let new_table = {
            let entity = entity.clone();
            icon_button(
                "mock-toolbar-new-table",
                IconName::Plus,
                "新增 Mock 表：开一个中央 tab 配字段与名称",
            )
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| panel.new_table_config(window, cx));
            })
        };
        // ⇄ 引用结构 = 原型 conn-pop：先弹连接列表选来源，再进对话框（库 / schema 按连接预填）。
        // 来源在 `open_import_dialog` 里填充，render 只读内存里的 `sources`（不触发 I/O）；
        // 还没有来源时菜单给一句说明 + 「在对话框里选择连接…」兜底（保持同一弹出组件形态）。
        let import = {
            let entity = entity.clone();
            let sources = self.sources.clone();
            icon_button(
                "mock-toolbar-import",
                IconName::HardDrive,
                "引用数据库结构：选连接读表结构",
            )
            // 显式 `small`：`dropdown_menu` 返回 `DropdownMenuPopover`（无 Sizable，
            // 进不了 Toolbar 的 size 托管槽），与 Toolbar 托管的按钮保持同档
            .small()
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if sources.is_empty() {
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new("（暂无数据源）").disabled(true));
                    return menu.item(PopupMenuItem::new("在对话框里选择连接…").on_click(
                        move |_, window, app| {
                            let entity = entity.clone();
                            entity.update(app, |panel, cx| {
                                panel.open_import_dialog(window, cx);
                            });
                        },
                    ));
                }
                for source in sources.iter() {
                    let entity = entity.clone();
                    let conn_id = source.conn_id.clone();
                    menu = menu.item(PopupMenuItem::new(source.label.clone()).on_click(
                        move |_, window, app| {
                            let entity = entity.clone();
                            let conn_id = conn_id.clone();
                            entity.update(app, |panel, cx| {
                                panel.open_import_for(&conn_id, window, cx);
                            });
                        },
                    ));
                }
                menu
            })
        };
        let new_scene = {
            let entity = entity.clone();
            icon_button(
                "mock-toolbar-new-scene",
                IconName::LayoutDashboard,
                "新增场景：从 0 张表开始的工作副本",
            )
            .disabled(running)
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.new_blank_scene(cx));
            })
        };
        let refresh = {
            let entity = entity.clone();
            icon_button(
                "mock-toolbar-refresh",
                IconName::RefreshCw,
                "刷新：重读临时表与生成状态",
            )
            .disabled(self.results.is_empty())
            .on_click(move |_, _, app| {
                entity.update(app, |panel, cx| panel.refresh_tables(cx));
            })
        };
        // 工具栏右侧的清单状态提示（对齐原型 mockToolHint：单表态「N 张表配置」；
        // 场景 / 结果态显示本批表数——比原型残留的「1 张表配置」更贴合当前清单）。
        let hint = if self.results.is_empty() {
            match &self.scenario {
                Some(template) => format!("{} 张表", template.tables.len()),
                None => "1 张表配置".to_string(),
            }
        } else {
            format!("{} 张结果", self.results.len())
        };

        // 工具栏走 `toolbar::Toolbar`（roving 方向键焦点 + toolbar a11y 语义 + 统一 size 托管）；
        // 外层只画高度 / 内距 / 底部分隔线（规范：背景边框高度外层自己画）。
        div()
            .w_full()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .px(rems(ui::PANEL_PADDING))
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(
                Toolbar::new("mock-toolbar")
                    .children([new_table, new_scene, refresh])
                    // import 是 `DropdownMenuPopover` 包装（无 Sizable），走非托管 content 槽
                    .content(import)
                    .content(card_note(cx.theme(), hint)),
            )
    }
}
