//! 对话框：保存模板 + 导入源库结构。

use super::*;

impl MockPanel {
    /// 「保存为模板」对话框：只问名字——行数 / 种子 / 语言 / 列 / 生成器参数都取当前配置。
    ///
    /// 校验本身在 [`MockPanel::save_template`]；这里提前拒空配置，只是为了别弹一个
    /// 必然失败的框。名字默认填「{目标表名} 配置」。
    pub fn open_save_template_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.draft.columns.is_empty() {
            self.history_error =
                Some("先加列（或导入结构）再存模板：空配置存下来没有意义".to_string());
            cx.notify();
            return;
        }
        let name = self
            .template_name
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("模板名")))
            .clone();
        let suggestion = format!("{} 配置", self.draft.table_name.trim());
        name.update(cx, |state, cx| state.set_value(suggestion, window, cx));

        let panel = cx.entity();
        let columns = self.draft.columns.len();
        let rows = with_thousands(self.draft.options.rows as u64);
        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let body = div()
                .v_flex()
                .gap_2()
                .child(hint_line(
                    theme,
                    format!(
                        "存下当前配置：{rows} 行 · {columns} 列（含生成器与参数）。目标表名不存——套用时按当时的输入走。"
                    ),
                ))
                .child(form_line(theme, "模板名", &name));
            let panel_save = panel.clone();
            let input = name.clone();
            dialog.title("保存为模板").child(body).footer(
                DialogFooter::new()
                    .child(
                        button("mock-template-dialog-cancel", "取消", ActionKind::Soft).on_click(
                            move |_, window, app| {
                                window.close_dialog(app);
                            },
                        ),
                    )
                    .child(
                        button("mock-template-dialog-ok", "保存", ActionKind::Main).on_click(
                            move |_, window, app| {
                                let value = input.read(app).value().to_string();
                                panel_save.update(app, |panel, cx| panel.save_template(value, cx));
                                window.close_dialog(app);
                            },
                        ),
                    ),
            )
        });
    }

    /// ⇄ 连接菜单选中：记下连接后直接进导入对话框（库 / schema 由对话框按连接记录预填，
    /// 见 [`Self::open_import_dialog`] 的 `panel_match` 分支）——对应原型 conn-pop 的
    /// 「选一个连接读它的库表结构」。
    pub fn open_import_for(&mut self, conn_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        *self.import_conn.borrow_mut() = Some(conn_id.to_string());
        self.open_import_dialog(window, cx);
    }

    pub fn open_import_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.sources.is_empty() {
            self.sources = self.host.schema_sources();
        }
        let panel_match = self
            .import_conn
            .borrow()
            .clone()
            .and_then(|id| self.sources.iter().find(|s| s.conn_id == id).cloned());
        let catalog = self
            .import_catalog
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("数据库")))
            .clone();
        let schema = self
            .import_schema
            .get_or_insert_with(|| {
                cx.new(|cx| InputState::new(window, cx).placeholder("schema（可留空）"))
            })
            .clone();
        let table = self
            .import_table
            .get_or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder("表名")))
            .clone();
        if let Some(source) = panel_match.as_ref() {
            let (catalog_value, schema_value, table_value) = (
                source.catalog.clone(),
                source.schema.clone(),
                self.draft.table_name.clone(),
            );
            catalog.update(cx, |s, cx| s.set_value(catalog_value, window, cx));
            schema.update(cx, |s, cx| s.set_value(schema_value, window, cx));
            table.update(cx, |s, cx| s.set_value(table_value, window, cx));
        }

        let panel = cx.entity();
        // `Rc` 包裹：dialog builder 每帧重建，内层菜单闭包逐个 clone 只增计数
        let sources = Rc::new(self.sources.clone());
        let selected = self.import_conn.clone();
        let catalog_input = catalog.clone();
        let schema_input = schema.clone();

        window.open_dialog(cx, move |dialog, _window, cx| {
            let theme = cx.theme();
            let current = selected.borrow().clone();
            let mut body = div().v_flex().gap_2().child(hint_line(
                theme,
                "从源库读取表结构（列名 / 类型 / 可空 / 主键）并自动推断生成器；不读取数据本身。",
            ));
            // 连接：菜单选择（可见触发按钮）——走 `kv` 原语，与下面三行表单同起一条右端基线
            let conn_row = kv(theme, "连接", {
                let selected = selected.clone();
                let catalog_input = catalog_input.clone();
                let schema_input = schema_input.clone();
                let sources = sources.clone();
                let current_label = current
                    .as_deref()
                    .and_then(|id| sources.iter().find(|s| s.conn_id == id))
                    .map(|s| s.label.clone());
                dropdown_button(
                    "mock-import-conn",
                    match current_label {
                        Some(label) => label,
                        None => "选择连接".to_string(),
                    },
                )
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    if sources.is_empty() {
                        return menu.item(PopupMenuItem::new("（暂无数据源）").disabled(true));
                    }
                    for candidate in sources.iter() {
                        let source = candidate.clone();
                        let selected = selected.clone();
                        let catalog_input = catalog_input.clone();
                        let schema_input = schema_input.clone();
                        menu = menu.item(PopupMenuItem::new(candidate.label.clone()).on_click(
                            move |_, window, app| {
                                // 选中连接：记下 conn_id，并把连接记录的库 / schema 预填进输入框
                                *selected.borrow_mut() = Some(source.conn_id.clone());
                                let catalog = source.catalog.clone();
                                let schema = source.schema.clone();
                                catalog_input.update(app, |s, cx| s.set_value(catalog, window, cx));
                                schema_input.update(app, |s, cx| s.set_value(schema, window, cx));
                            },
                        ));
                    }
                    menu
                })
            });
            body = body.child(conn_row);
            body = body
                .child(form_line(theme, "数据库 / catalog", &catalog))
                .child(form_line(theme, "schema", &schema))
                .child(form_line(theme, "表名", &table));

            let panel_ok = panel.clone();
            let selected_ok = selected.clone();
            dialog.title("导入源库结构").child(body).footer(
                DialogFooter::new()
                    .child(
                        button("mock-import-cancel", "取消", ActionKind::Soft).on_click(
                            move |_, window, app| {
                                window.close_dialog(app);
                            },
                        ),
                    )
                    .child(button("mock-import-ok", "导入", ActionKind::Main).on_click(
                        move |_, window, app| {
                            panel_ok.update(app, |panel, cx| {
                                let request = SchemaRequest {
                                    conn_id: selected_ok.borrow().clone().unwrap_or_default(),
                                    catalog: panel
                                        .import_catalog
                                        .as_ref()
                                        .map(|i| i.read(cx).value().to_string())
                                        .unwrap_or_default(),
                                    schema: panel
                                        .import_schema
                                        .as_ref()
                                        .map(|i| i.read(cx).value().to_string())
                                        .unwrap_or_default(),
                                    table: panel
                                        .import_table
                                        .as_ref()
                                        .map(|i| i.read(cx).value().to_string())
                                        .unwrap_or_default(),
                                };
                                if request.conn_id.is_empty() {
                                    panel.fail("请选择连接", cx);
                                    return;
                                }
                                if request.table.trim().is_empty() {
                                    panel.fail("请填写表名", cx);
                                    return;
                                }
                                panel.preset_from_source(request, cx);
                            });
                            window.close_dialog(app);
                        },
                    )),
            )
        });
    }
}
