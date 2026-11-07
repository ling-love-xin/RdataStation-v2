//! 列区渲染：批量起手按钮 / 加列按钮 / 表清单列行。

use super::*;

impl MockPanel {
    /// 批量起手按钮（预设 / 导入结构）：与「场景模板」同属一列——把这张表的列**起手**。
    /// 放在操作行左侧（紧跟场景模板），与「＋加列」（单个新增，永远最右）分开。
    pub(crate) fn render_batch_source_buttons(&self, id: u64, cx: &mut Context<Self>) -> Div {
        let import = {
            let entity = cx.entity();
            let config_id = id;
            button("mock-import-schema", "导入结构", ActionKind::Soft)
                .xsmall()
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.focus_config(config_id);
                        panel.open_import_dialog(window, cx);
                    });
                })
        };
        // 字段预设：常用字段（电话/地址/姓名等）一行打包，从分组菜单一键加列。
        // 与「导入结构」（源库批量）同属批量起手；应用后是普通列可再编辑
        let presets = {
            let entity = cx.entity();
            let config_id = id;
            button("mock-preset-column", "预设", ActionKind::Soft)
                .xsmall()
                .icon(IconName::Folder)
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for category in PresetCategory::ALL {
                        let items: Vec<FieldPreset> = builtin_presets()
                            .into_iter()
                            .filter(|p| p.category == category)
                            .collect();
                        if items.is_empty() {
                            continue;
                        }
                        // 组标题：disabled 只读行，不参与点击
                        menu = menu.item(PopupMenuItem::new(category.label()).disabled(true));
                        for preset in items {
                            let entity = entity.clone();
                            let id = preset.id;
                            menu = menu.item(PopupMenuItem::new(preset.label).on_click(
                                move |_, _, app| {
                                    let Some(preset) = preset_by_id(id) else {
                                        return;
                                    };
                                    entity.update(app, |panel, cx| {
                                        panel.focus_config(config_id);
                                        panel.add_preset_column(&preset, cx);
                                    });
                                },
                            ));
                        }
                    }
                    menu
                })
        };
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(presets)
            .child(import)
    }

    /// ＋加列：列区主行动，永远在操作行最右。
    pub(crate) fn render_add_column_button(&self, id: u64, cx: &mut Context<Self>) -> Div {
        let entity = cx.entity();
        let config_id = id;
        // 列区的主行动；「＋」走组件图标（与工具栏「新增 Mock 表」同一 `Plus`），不手拼字符
        let add = button("mock-add-column", "加列", ActionKind::Main)
            .xsmall()
            .icon(IconName::Plus)
            .on_click(move |_, _, app| {
                entity.update(app, |panel, cx| {
                    panel.focus_config(config_id);
                    let index = panel.draft.columns.len() + 1;
                    panel.add_column(format!("column_{index}"), ColumnDataType::Integer, cx);
                });
            });
        div().h_flex().items_center().child(add)
    }
}
