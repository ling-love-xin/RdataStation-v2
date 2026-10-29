//! 关联性域渲染：数值相关 / 分类关联 / 值重叠 + 多列规则触发。

use super::*;

impl super::InsightWorkspacePanel {
    /// 关联性区（原型 .rel-section）：子 Tab 三段——
    /// 数值相关 / 分类关联 = 多列规则执行（列对选择 + 规则选择 + 执行，结果即规则输出）；
    /// 值重叠 = 跨表入口引导（右栏集合卡「关系」是现有跨表值重叠排查）。
    ///
    /// 矩阵化两两相关需要 N×N 规则执行（负载重），后置到多列视图期次（见文档），
    /// 这里先给「选列对 → 跑规则 → 看输出」的科研可用路径。
    pub(crate) fn render_rel_section(
        &self,
        cx: &Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let current = self.rel_tab.index();
        let tabs = TabBar::new("insight-ws-rel-tabs")
            .segmented()
            .selected_index(current)
            .children(RelTab::ALL.iter().map(|t| Tab::new().label(t.label())))
            .on_click({
                let entity = entity.clone();
                move |index, _, app| {
                    entity.update(app, |panel, cx| {
                        if panel.rel_tab != RelTab::of_index(*index) {
                            panel.rel_tab = RelTab::of_index(*index);
                            cx.notify();
                        }
                    });
                }
            });
        let body = match self.rel_tab {
            RelTab::Corr => self.render_rel_corr(cx, profile, entity, theme),
            RelTab::Assoc => self.render_rel_assoc(cx, profile, entity, theme),
            RelTab::Overlap => {
                // 值重叠：右栏集合卡「关系」按钮 = 与当前目标做同名列值重叠排查
                panel_box(
                    &theme,
                    "值重叠",
                    "跨表排查".into(),
                    div()
                        .v_flex()
                        .w_full()
                        .gap_1()
                        .child(note_line(
                            IconName::Info,
                            "与另一张已洞察表的同名列做值重叠排查：在右侧「已洞察表」集合卡点「关系」".into(),
                            &theme,
                        ))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child("结果展示在右栏集合卡的关系区（T19 关系引擎，同名列类型族过滤）"),
                        ),
                )
            }
        };
        div().v_flex().w_full().gap_2().child(tabs).child(body)
    }

    /// 数值相关：列对（数值列多选 ≥2）→ 相关规则（Pearson / Spearman）→ 执行。
    ///
    /// 执行走多列规则链路（`MultiRunRequested` → 后台跑 DuckDB 规则 → 结果回填
    /// `PanelData::multi.result`），结果按规则输出形态渲染（键值行）。
    pub(crate) fn render_rel_corr(
        &self,
        cx: &Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let numeric: Vec<String> = profile
            .columns
            .iter()
            .filter(|c| c.kind == ColumnKind::Numeric)
            .map(|c| c.name.clone())
            .collect();
        let selected: Vec<String> = self
            .corr_cols
            .iter()
            .filter(|n| numeric.contains(n))
            .cloned()
            .collect();
        // 可用的相关规则（多列清单里按名字识别；未加载时给占位引导）
        let multi = self.view.read(cx).data().multi.clone();
        let rule = self.rel_rule.clone();
        let rules_ready = multi.as_ref().map(|m| !m.rules.is_empty()).unwrap_or(false);
        // 当前规则名（构建时快照，浮层内容闭包用）
        let rule_name = multi
            .as_ref()
            .and_then(|m| {
                m.rules.iter().find(|r| {
                    Some(&r.id) == rule.as_ref()
                        && (r.id.contains("correlation") || r.id.contains("spearman"))
                })
            })
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "Pearson 相关".into());
        let can_run = selected.len() >= 2 && rule.is_some();
        // 规则清单快照：浮层闭包 move 用（`multi` 本身留给结果渲染 when_some）
        let rules = multi.as_ref().map(|m| m.rules.clone()).unwrap_or_default();

        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    // 列对选择：Popover 多选（Checkbox）
                    .child({
                        let entity = entity.clone();
                        let current = selected.clone();
                        Popover::new("insight-ws-corr-cols")
                            .anchor(Anchor::TopRight)
                            .trigger(
                                Button::new("insight-ws-corr-cols-btn")
                                    .ghost()
                                    .xsmall()
                                    .icon(CatalogIcon::ListFilter)
                                    .label(format!("列对 {}/{}", selected.len(), numeric.len()))
                                    .tooltip("选 2 个以上数值列算相关"),
                            )
                            .content(move |_state, _window, cx| {
                                let colors = cx.theme().colors;
                                let entity = entity.clone();
                                let mut body = div()
                                    .v_flex()
                                    .w(rems(ui::INSIGHT_PICKER_WIDTH))
                                    .gap_1()
                                    .px_2()
                                    .py_2();
                                for name in &numeric {
                                    let entity = entity.clone();
                                    let name = name.clone();
                                    let label = name.clone();
                                    let checked = current.contains(&name);
                                    body = body.child(
                                        div()
                                            .h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                Checkbox::new(SharedString::from(format!(
                                                    "insight-ws-corr-{name}"
                                                )))
                                                .checked(checked)
                                                .on_click(move |_, _, app| {
                                                    entity.update(app, |panel, cx| {
                                                        if let Some(pos) = panel
                                                            .corr_cols
                                                            .iter()
                                                            .position(|n| n == &name)
                                                        {
                                                            panel.corr_cols.remove(pos);
                                                        } else {
                                                            panel.corr_cols.push(name.clone());
                                                        }
                                                        cx.notify();
                                                    });
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.foreground)
                                                    .child(label),
                                            ),
                                    );
                                }
                                body
                            })
                    })
                    // 规则选择：下拉（Pearson / Spearman；Kendall τ DuckDB 无直接函数，后置）
                    .child({
                        let entity = entity.clone();
                        let rule_name = rule_name.clone();
                        Button::new("insight-ws-corr-rule")
                            .ghost()
                            .xsmall()
                            .icon(CatalogIcon::Sigma)
                            .label(rule_name)
                            .tooltip("相关方法（DuckDB 原生可算的统计量）")
                            .dropdown_menu(move |menu, _, _| {
                                let mut menu = menu;
                                for r in rules.iter().filter(|r| {
                                    r.id.contains("correlation") || r.id.contains("spearman")
                                }) {
                                    let entity = entity.clone();
                                    let id = r.id.clone();
                                    let name = r.name.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(name)
                                            .checked(Some(&id) == rule.as_ref())
                                            .on_click(move |_, _, app| {
                                                entity.update(app, |panel, cx| {
                                                    panel.rel_rule = Some(id.clone());
                                                    cx.notify();
                                                });
                                            }),
                                    );
                                }
                                menu
                            })
                    })
                    // 执行：先确保多列规则清单已加载（未加载时点执行 = 请求清单）
                    .child(
                        Button::new("insight-ws-corr-run")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Play)
                            .label("计算")
                            .disabled(!can_run)
                            .tooltip(if selected.len() < 2 {
                                "至少选 2 个数值列"
                            } else {
                                "执行所选相关规则"
                            })
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        let Some(rule_id) = panel.rel_rule.clone() else {
                                            return;
                                        };
                                        if !rules_ready {
                                            // 规则清单未加载：先请求清单，用户在下一轮点执行
                                            panel.request_multi_rules(cx);
                                            return;
                                        }
                                        if let Some(temp_table) = panel
                                            .view
                                            .read(cx)
                                            .target()
                                            .and_then(|t| panel.view.read(cx).data().sample_table(t))
                                            .map(|s| s.to_string())
                                        {
                                            let columns = panel.corr_cols.clone();
                                            panel.view.update(cx, |_view, cx| {
                                                cx.emit(crate::insight_view::InsightEvent::MultiRunRequested {
                                                    temp_table,
                                                    rule_id,
                                                    columns,
                                                });
                                            });
                                        }
                                    });
                                }
                            }),
                    ),
            )
            // 结果：多列规则输出（键值行或表格）
            .when_some(multi.as_ref().and_then(|m| m.result.clone()), |this, result| {
                this.child(render_multi_result(&result, theme))
            })
            .when_some(
                if rules_ready { None } else { Some(()) },
                |this, ()| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("多列规则清单未加载：点「计算」会先请求清单（需已进入多列视图）"),
                    )
                },
            )
    }

    /// 分类关联：两列文本/分类 → Cramér's V（DuckDB 原生卡方现算，规则见
    /// `insight-rules/multi/cramer-v.rule.toml`）。
    pub(crate) fn render_rel_assoc(
        &self,
        cx: &Context<Self>,
        profile: &TableProfileView,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> Div {
        let colors = theme.colors;
        let text_cols: Vec<String> = profile
            .columns
            .iter()
            .filter(|c| c.kind == ColumnKind::Text || c.kind == ColumnKind::Boolean)
            .map(|c| c.name.clone())
            .collect();
        let selected: Vec<String> = self
            .corr_cols
            .iter()
            .filter(|n| text_cols.contains(n))
            .cloned()
            .collect();
        let multi = self.view.read(cx).data().multi.clone();
        let rules_ready = multi.as_ref().map(|m| !m.rules.is_empty()).unwrap_or(false);
        let has_cramer = multi
            .as_ref()
            .map(|m| m.rules.iter().any(|r| r.id.contains("cramer")))
            .unwrap_or(false);
        let can_run = selected.len() >= 2 && has_cramer;
        div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child({
                        let entity = entity.clone();
                        let current = selected.clone();
                        Popover::new("insight-ws-assoc-cols")
                            .anchor(Anchor::TopRight)
                            .trigger(
                                Button::new("insight-ws-assoc-cols-btn")
                                    .ghost()
                                    .xsmall()
                                    .icon(CatalogIcon::ListFilter)
                                    .label(format!("列对 {}/{}", selected.len(), text_cols.len()))
                                    .tooltip("选 2 个文本/分类列算关联"),
                            )
                            .content(move |_state, _window, cx| {
                                let colors = cx.theme().colors;
                                let entity = entity.clone();
                                let mut body = div()
                                    .v_flex()
                                    .w(rems(ui::INSIGHT_PICKER_WIDTH))
                                    .gap_1()
                                    .px_2()
                                    .py_2();
                                for name in &text_cols {
                                    let entity = entity.clone();
                                    let name = name.clone();
                                    let label = name.clone();
                                    let checked = current.contains(&name);
                                    body = body.child(
                                        div()
                                            .h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                Checkbox::new(SharedString::from(format!(
                                                    "insight-ws-assoc-{name}"
                                                )))
                                                .checked(checked)
                                                .on_click(move |_, _, app| {
                                                    entity.update(app, |panel, cx| {
                                                        if let Some(pos) = panel
                                                            .corr_cols
                                                            .iter()
                                                            .position(|n| n == &name)
                                                        {
                                                            panel.corr_cols.remove(pos);
                                                        } else {
                                                            panel.corr_cols.push(name.clone());
                                                        }
                                                        cx.notify();
                                                    });
                                                }),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(colors.foreground)
                                                    .child(label),
                                            ),
                                    );
                                }
                                body
                            })
                    })
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("Cramér's V（卡方独立性）"),
                    )
                    .child(
                        Button::new("insight-ws-assoc-run")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Play)
                            .label("计算")
                            .disabled(!can_run)
                            .tooltip(if selected.len() < 2 {
                                "至少选 2 个文本/分类列"
                            } else {
                                "执行 Cramér's V"
                            })
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        if !rules_ready {
                                            panel.request_multi_rules(cx);
                                            return;
                                        }
                                        let Some(rule_id) = panel
                                            .view
                                            .read(cx)
                                            .data()
                                            .multi
                                            .as_ref()
                                            .and_then(|m| {
                                                m.rules
                                                    .iter()
                                                    .find(|r| r.id.contains("cramer"))
                                                    .map(|r| r.id.clone())
                                            })
                                        else {
                                            return;
                                        };
                                        if let Some(temp_table) = panel
                                            .view
                                            .read(cx)
                                            .target()
                                            .and_then(|t| panel.view.read(cx).data().sample_table(t))
                                            .map(|s| s.to_string())
                                        {
                                            let columns = panel.corr_cols.clone();
                                            panel.view.update(cx, |_view, cx| {
                                                cx.emit(crate::insight_view::InsightEvent::MultiRunRequested {
                                                    temp_table,
                                                    rule_id,
                                                    columns,
                                                });
                                            });
                                        }
                                    });
                                }
                            }),
                    ),
            )
            .when_some(multi.as_ref().and_then(|m| m.result.clone()), |this, result| {
                this.child(render_multi_result(&result, theme))
            })
            .when_some(
                if rules_ready { None } else { Some(()) },
                |this, ()| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("多列规则清单未加载：点「计算」会先请求清单（需已进入多列视图）"),
                    )
                },
            )
    }

    /// 请求多列规则清单（关联性子 Tab 的执行前置：清单未加载时先拉一次）
    pub(crate) fn request_multi_rules(&self, cx: &mut Context<Self>) {
        let Some(target) = self.view.read(cx).target().cloned() else {
            return;
        };
        let Some(temp_table) = self
            .view
            .read(cx)
            .data()
            .sample_table(&target)
            .map(|s| s.to_string())
        else {
            return;
        };
        let table_name = target.table_name().to_string();
        self.view.update(cx, |_view, cx| {
            cx.emit(crate::insight_view::InsightEvent::MultiColumnRequested {
                temp_table,
                table_name,
            });
        });
    }
}
