//! 单表出口渲染：落库 / 追加 / 草稿箱 / 另存为 + 就地反馈。

use super::*;

impl MockPanel {
    /// 单表出口（落库 / 追加 / 草稿箱 / 另存为 + 就地反馈）：作用于「当前 tab 那张表」，
    /// 随表上移中央详情 tab（生成 → 落地闭环，2026-10-31）；右面板只留集合级出口。
    /// `window` 仅透传给「另存为」的系统保存对话框（下拉项闭包自取，无需本函数直接使用）。
    pub(crate) fn render_sinks(
        &self,
        id: u64,
        key: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let fg = cx.theme().colors.foreground;
        // 出口是「这张表」的动作：点之前先把活动指针切到这张 tab 对应的配置，
        // 否则会把上一张表的现场落出去。
        let config_id = id;

        let running = self.is_running();
        // 项目级出口（落库 / 追加 / 草稿箱）都要项目根：没打开项目时直接不可点，
        // 而不是点了才由任务层拒绝（「点了报错不如直接不可点」，与生成前禁用同一口径）。
        let sinks_ready = self.sinks_ready();
        let persist = {
            let entity = cx.entity();
            // 出口区唯一的 Main：其余三个是「换个去向」，只有这个是「落到项目里」。
            // 此前注释写着「视觉上给 primary」，代码挂的却是 `.secondary()`——
            // 口头的层级不算数，画出来的才是（四个同形态时，用户只能读文字分辨）
            button(
                format!("mock-persist-{key}"),
                "持久化到项目分析库",
                ActionKind::Main,
            )
            .disabled(running || !sinks_ready)
            .on_click(move |_, _, app| {
                entity.update(app, |panel, cx| {
                    panel.focus_config(config_id);
                    panel.persist_table(cx);
                });
            })
        };
        let append = {
            let entity = cx.entity();
            let tables = self.existing_tables.clone();
            button(
                format!("mock-append-{key}"),
                "追加到既有表",
                ActionKind::Soft,
            )
            .dropdown_caret(true)
            .disabled(running || !sinks_ready)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if tables.is_empty() {
                    return menu.item(PopupMenuItem::new("（项目分析库暂无表）").disabled(true));
                }
                for name in tables.iter() {
                    let table = name.clone();
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                        move |_, window, app| {
                            let table = table.clone();
                            entity.update(app, |panel, cx| {
                                panel.focus_config(config_id);
                                panel.open_append_dialog(table, window, cx);
                            });
                        },
                    ));
                }
                menu
            })
        };
        let scratchpad = {
            let entity = cx.entity();
            button(
                format!("mock-save-scratchpad-{key}"),
                "保存到草稿箱",
                ActionKind::Soft,
            )
            .dropdown_caret(true)
            .disabled(running || !sinks_ready)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for (label, format) in FILE_FORMATS {
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _, app| {
                        entity.update(app, |panel, cx| {
                            panel.focus_config(config_id);
                            panel.save_scratchpad(&format, cx)
                        });
                    }));
                }
                menu
            })
        };
        let export = {
            let entity = cx.entity();
            button(format!("mock-export-{key}"), "另存为", ActionKind::Soft)
                .dropdown_caret(true)
                .disabled(running || !self.has_result())
                .dropdown_menu(move |menu, _window, _cx| {
                    let mut menu = menu;
                    for (label, format) in FILE_FORMATS {
                        let entity = entity.clone();
                        menu =
                            menu.item(PopupMenuItem::new(label).on_click(move |_, window, app| {
                                let format = format.clone();
                                let name = entity.read(app).export_file_name(&format);
                                let dir = entity.read(app).host.export_dir();
                                let dir = if dir.trim().is_empty() {
                                    PathBuf::from(".")
                                } else {
                                    PathBuf::from(dir)
                                };
                                // 系统保存对话框（异步回传）：取消则不动
                                let receiver = app.prompt_for_new_path(&dir, Some(&name));
                                let entity = entity.clone();
                                window
                                    .spawn(app, async move |cx| {
                                        let Ok(Ok(Some(path))) = receiver.await else {
                                            return;
                                        };
                                        let path = path.to_string_lossy().to_string();
                                        cx.update(|_, app| {
                                            entity.update(app, |panel, cx| {
                                                panel.export_file(&format, path, cx)
                                            });
                                        })
                                        .ok();
                                    })
                                    .detach();
                            }));
                    }
                    menu
                })
        };
        let landed = self.landed.clone();

        div()
            .v_flex()
            .gap_1()
            .w_full()
            // 动作行：生成 + 四个出口横排（生成 → 落地，一条动作链走完）。
            // 原来四个出口是 `w_full` 竖排，独占 4 行高度把预览往下挤；横排后
            // 表头只多一行，且「生成在前、落地在后」的顺序本身就是这条链的读法。
            // 竖向 `Separator` 撤掉：它把一条动作链切成两截，而按钮文案已各自说了去向。
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(self.render_generate_button(id, cx))
                    .child(persist)
                    .child(append)
                    .child(scratchpad)
                    .child(export)
                    // 进度条 / 失败重试 / 结果摘要都在「生成」右侧这一个状态格里：
                    // 它们说的是「这次动作」的进展与结果，贴着按钮读，不再各占一行
                    .child(self.render_action_status(cx)),
            )
            // 落库目标是「落到哪张表」，不是状态：它留成动作行下面单独一句
            .children(landed.map(|table| {
                div()
                    .text_xs()
                    .text_color(fg)
                    .child(format!("落库目标：{table}"))
            }))
    }
}
