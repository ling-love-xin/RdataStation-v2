//! 工具栏按钮渲染（format/history/explain/translate/template/tx_auto_commit + 自定义面板）
//!
//! 自 host/toolbar.rs 拆分（2026-10-29 Step 8）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::switch::Switch;
use crate::translate;
use crate::ui;

impl EditorHostPanel {
    pub(crate) fn render_format_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-format")
            .ghost()
            .small()
            .debug_selector(|| "editor-format".to_string())
            .icon(crate::toolbar::ToolbarItemId::Format.icon())
            .tooltip("格式化")
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| panel.format_document(window, cx));
            })
    }

    /// 【B8 余项】文档级「⟲ 历史」：打开右 Dock 历史面板（端口注入——编辑器不依赖工作台
    /// 的 Dock）。未接端口时按钮置灰（`.disabled`），点了也会在状态栏给可读原因（不静默）。
    pub(crate) fn render_history_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        let available = self.shared.open_history_port().is_some();
        Button::new("editor-history")
            .ghost()
            .small()
            .disabled(!available)
            .debug_selector(|| "editor-history".to_string())
            .icon(crate::toolbar::ToolbarItemId::History.icon())
            .tooltip("历史")
            .on_click(move |_, _window, app| {
                if entity.read(app).shared.open_history_port().is_none() {
                    // 未接端口：状态栏给原因，而不是“点了没反应”
                    entity.update(app, |panel, cx| {
                        panel.set_message(Some("未接入历史面板".to_string()), cx);
                    });
                    return;
                }
                let port = entity.read(app).shared.open_history_port().unwrap();
                port(app);
            })
    }

    /// 【原型 §2.2 分析组】执行计划（EXPLAIN）：常驻按钮（2026-10-11 提升常驻，不再进"更多"）
    pub(crate) fn render_explain_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-explain")
            .ghost()
            .small()
            .debug_selector(|| "editor-explain".to_string())
            .icon(crate::toolbar::ToolbarItemId::Explain.icon())
            .tooltip("执行计划")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.explain_current(cx));
            })
    }

    /// 【原型 §2.2 分析组】方言转换：常驻按钮（目标方言列表）
    ///
    /// 目标清单在**渲染时**定好（源方言已知就排除它自己）；菜单回调是 `Fn`，不能在里头读面板。
    /// 源方言未知（未绑定连接）时仍把目标列出来——点下去会收到一句可读的拒绝，
    /// 比“菜单里什么都没有”更容易看懂。
    pub(crate) fn render_translate_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let targets: Vec<translate::Target> = match self.source_dialect() {
            Ok(source) => translate::targets_for(source),
            Err(_) => translate::TARGETS.to_vec(),
        };
        Button::new("editor-translate")
            .ghost()
            .small()
            .debug_selector(|| "editor-translate".to_string())
            .icon(crate::toolbar::ToolbarItemId::Translate.icon())
            .tooltip("方言转换")
            .dropdown_menu(move |menu, window, cx| {
                menu.submenu("转译为", window, cx, {
                    let entity = entity.clone();
                    let targets = targets.clone();
                    move |sub, _window, _cx| {
                        let mut sub = sub;
                        for target in targets.iter() {
                            let entity = entity.clone();
                            let dialect = target.dialect;
                            sub = sub.item(PopupMenuItem::new(target.label).on_click(
                                move |_, window, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.transpile_document(dialect, window, cx);
                                    });
                                },
                            ));
                        }
                        sub
                    }
                })
            })
    }

    /// 【原型 §2.2 事务/模板】模板 / 代码片段：常驻按钮（无模板时置灰，菜单不摆空项）
    pub(crate) fn render_template_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        // 【B9 切片二】模板片段（内存读：渲染时读一次；没模板 / 没接端口就置灰）
        let templates = self.shared.completion_templates();
        let has_templates = !templates.is_empty();
        Button::new("editor-template")
            .ghost()
            .small()
            .debug_selector(|| "editor-template".to_string())
            .disabled(!has_templates)
            .icon(crate::toolbar::ToolbarItemId::Template.icon())
            .tooltip("模板")
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for (index, template) in templates.iter().enumerate() {
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(template.name.clone()).on_click(
                        move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                panel.insert_template(index, window, cx);
                            });
                        },
                    ));
                }
                menu
            })
    }

    /// 【原型 §2.2 事务/模板】事务提交方式（自动 ⇄ 手动）：常驻开关（模式要一直看得见）
    ///
    /// 从状态栏 TX 区移到工具栏（原型 §2.2 的“事务开关常显”）；状态栏 TX 区只留
    /// 状态文本 + 事务开启时的提交 / 回滚。
    pub(crate) fn render_tx_auto_commit_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-autocommit")
            .ghost()
            .small()
            .debug_selector(|| "editor-autocommit".to_string())
            .toggled(self.autocommit)
            .icon(crate::toolbar::ToolbarItemId::TxAutoCommit.icon())
            .tooltip("自动提交")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.toggle_autocommit(cx));
            })
    }

    /// 【T2c-5】`···` 定制按钮 + 三栏设置面板（可见性 / 折叠 / 移动 / 竖线分隔符 / 重置）
    ///
    /// 为什么用 `Popover` 而不是 `DropdownMenu`：菜单实体是**缓存**的，切换可见性后内容会
    /// 陈旧；`Popover::content` 的闭包**每帧重建**，能实时反映面板状态。面板不是菜单——
    /// 它是「图标功能｜可见｜移动」三栏的设置视图，用 `Checkbox` / `Switch` / `Button` /
    /// `Separator` 现成组件拼装（组件禁手搓）。
    ///
    /// 面板分四段：标题 → 组头行（组名 + 折叠开关 + 组移动↑↓）→ 成员行（图标+标签｜
    /// 可见 ▢｜单项移动↑↓）→ 段尾分隔符行（┃ 竖线分隔符 + ✕）→ 底部（＋添加竖线分隔符｜
    /// ↺ 重置默认）。所有动作走面板方法（事件路径），render 只按 id 挂回调。
    pub(crate) fn render_toolbar_customize(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::toolbar::MoveDir;
        let entity = cx.entity();
        let panel_width = rems(ui::TOOLBAR_CUSTOMIZE_WIDTH);
        Popover::new("editor-toolbar-customize-popover")
            // 最右的按钮：面板向右下展开，避免被窗口右缘截断
            .anchor(Anchor::BottomRight)
            .offset(ui::TOOLBAR_CUSTOMIZE_OFFSET)
            .trigger(
                Button::new("editor-toolbar-customize")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-toolbar-customize".to_string())
                    .icon(IconName::Ellipsis)
                    .tooltip("自定义工具栏"),
            )
            .content(move |_state, _window, cx| {
                let muted = cx.theme().colors.muted_foreground;
                let panel = entity.read(cx);
                let layout = &panel.toolbar;
                let groups = layout.groups();

                let mut body = div()
                    .v_flex()
                    .gap_1()
                    .p_2()
                    .w(panel_width)
                    .debug_selector(|| "editor-toolbar-customize-panel".to_string());
                body = body.child(
                    div()
                        .px_1()
                        .text_xs()
                        .text_color(muted)
                        .child("自定义工具栏"),
                );
                body = body.child(Separator::horizontal());
                // 竖线分隔符位置快照：分离出来，避免 `'static` 闭包借用临时值
                let separator_positions = layout.separator_positions();

                for group in &groups {
                    // 组头行：组名 + 折叠开关 + 组移动↑↓
                    let head = group.head;
                    let collapsed = layout.is_collapsed(head);
                    let can_move_left = head != group_of_first(&groups);
                    let can_move_right = head != group_of_last(&groups);
                    body = body.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .debug_selector(move || {
                                format!("editor-toolbar-customize-group-{:?}", head)
                            })
                            .child(panel.item_icon_row(head, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(head.label()),
                            )
                            .child(Switch::new(format!("toolbar-fold-{:?}", head))
                                .checked(collapsed)
                                .label("折叠")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.toggle_toolbar_collapsed(head, cx);
                                        });
                                    }
                                }))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-group-up-{:?}", head),
                                IconName::ChevronUp,
                                can_move_left,
                                move |panel, cx| panel.move_toolbar_group(head, MoveDir::Left, cx),
                            ))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-group-down-{:?}", head),
                                IconName::ChevronDown,
                                can_move_right,
                                move |panel, cx| panel.move_toolbar_group(head, MoveDir::Right, cx),
                            )),
                    );
                    // 成员行：图标+标签｜可见 ▢｜单项移动↑↓
                    for id in &group.items {
                        let id = *id;
                        let visible = layout.is_visible(id);
                        let fixed = id.fixed_visible();
                        body = body.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .pl_3()
                                .debug_selector(move || {
                                    format!("editor-toolbar-customize-item-{:?}", id)
                                })
                                .child(panel.item_icon_row(id, cx))
                                .child(div().flex_1().text_xs().child(id.label()))
                                .child(
                                    Checkbox::new(format!("toolbar-vis-{:?}", id))
                                        .checked(visible)
                                        .disabled(fixed)
                                        .on_click({
                                            let entity = entity.clone();
                                            move |&on, _window, app| {
                                                entity.update(app, |panel, cx| {
                                                    panel.set_toolbar_visible(id, on, cx);
                                                });
                                            }
                                        }),
                                )
                                .child(panel.small_icon_button(
                                    &entity,
                                    format!("toolbar-item-up-{:?}", id),
                                    IconName::ChevronUp,
                                    true,
                                    move |panel, cx| panel.move_toolbar_item(id, MoveDir::Left, cx),
                                ))
                                .child(panel.small_icon_button(
                                    &entity,
                                    format!("toolbar-item-down-{:?}", id),
                                    IconName::ChevronDown,
                                    true,
                                    move |panel, cx| {
                                        panel.move_toolbar_item(id, MoveDir::Right, cx)
                                    },
                                )),
                        );
                    }
                }

                // 段尾分隔符行：每条竖线分隔符一个「┃ 竖线分隔符 + ✕」
                for (index, pos) in separator_positions.iter().enumerate() {
                    let key = format!("toolbar-sep-{index}");
                    let pos = *pos;
                    body = body.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .pl_3()
                            .debug_selector(move || key.clone())
                            .child(Separator::vertical().h(rems(0.75)))
                            .child(div().flex_1().text_xs().text_color(muted).child("竖线分隔符"))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-sep-up-{index}"),
                                IconName::ChevronUp,
                                true,
                                move |panel, cx| {
                                    panel.move_toolbar_separator(pos, MoveDir::Left, cx)
                                },
                            ))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-sep-down-{index}"),
                                IconName::ChevronDown,
                                true,
                                move |panel, cx| {
                                    panel.move_toolbar_separator(pos, MoveDir::Right, cx)
                                },
                            ))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-sep-remove-{index}"),
                                IconName::X,
                                true,
                                move |panel, cx| panel.remove_toolbar_separator(pos, cx),
                            )),
                    );
                }

                body = body.child(Separator::horizontal());
                // 底部：＋添加竖线分隔符｜↺ 重置默认
                body = body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("editor-toolbar-customize-add-sep")
                                .ghost()
                                .small()
                                .icon(IconName::Plus)
                                .label("添加竖线分隔符")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.insert_toolbar_separator(cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("editor-toolbar-customize-reset")
                                .ghost()
                                .small()
                                .icon(IconName::RotateCcw)
                                .label("重置默认")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _window, app| {
                                        entity.update(app, |panel, cx| panel.reset_toolbar(cx));
                                    }
                                }),
                        ),
                );
                body
            })
    }

}
