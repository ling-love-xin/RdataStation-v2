//! 面板主体渲染：表清单 + 集合动作 + 出口 + 出口反馈。

use super::*;

impl MockPanel {
    /// 右 Dock 的面板主体：表清单 + 集合动作 + 出口 + 出口反馈（D38）。
    pub(crate) fn render_actions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let running = self.is_running();
        let generating = self.is_generating();

        // 集合级动作：生成 N 张表 / 退出场景（它属于这一批，不属于某张表 → 留在管理侧，D38）
        let scenario_actions = {
            let mut row = div().h_flex().items_center().gap_2().w_full();
            if let Some(template) = self.scenario.clone() {
                let start = {
                    let entity = cx.entity();
                    // 场景态的主行动：这一批就这一个填色按钮
                    let label = if generating {
                        "生成中…".to_string()
                    } else {
                        format!("生成 {} 张表", template.tables.len())
                    };
                    let mut btn = button("mock-scenario-generate", label, ActionKind::Main);
                    if !running {
                        btn = btn.on_click(move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                // 点生成 = 这一轮数据重新验收：先清掉上一轮残留的搜索过滤，
                                // 免得新清单被旧关键词挡住（曾误判「只生成了一张表」）
                                if let Some(search) = panel.search_input.clone() {
                                    search.update(cx, |state, cx| state.set_value("", window, cx));
                                }
                                panel.run_scenario(cx);
                            });
                        });
                    }
                    btn
                };
                // S5：场景种子（留空 = 随机；填了 = 整组可复现，历史行标注 seed）
                let seed_box = self
                    .scenario_seed_input
                    .clone()
                    .map(|input| {
                        div()
                            .w(rems(ui::NUM_INPUT_WIDTH))
                            .flex_none()
                            .child(Input::new(&input))
                    })
                    .unwrap_or_else(|| div().flex_none());
                let exit = {
                    let entity = cx.entity();
                    let mut btn = button("mock-scenario-exit", "退出场景", ActionKind::Soft)
                        .disabled(running);
                    if !running {
                        btn = btn.on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.close_scenario(cx));
                        });
                    }
                    btn
                };
                row = row
                    .child(div().flex_1().min_w_0().child(start))
                    .child(seed_box)
                    .child(div().flex_none().child(exit));
            }
            row
        };

        // ① 表清单：唯一的管理入口（单表 / 本次生成 / 结果表三态共用一个渲染器）
        let table_list = self.render_table_list(window, cx);

        // 关系闭包里还没落库的：摆出来 + 一键依次落（免得漏落，留下悬空引用）
        let pending_row = {
            let pending = self.pending_relation_tables();
            if pending.is_empty() {
                div()
            } else {
                // 出口区（批量）的主行动：与单表的「持久化」同口径
                let persist_btn = {
                    let entity = cx.entity();
                    let mut btn = button(
                        "mock-persist-related",
                        format!("落库这 {} 张", pending.len()),
                        ActionKind::Main,
                    )
                    .xsmall()
                    .disabled(running);
                    if !running {
                        btn = btn.on_click(move |_, _, app| {
                            entity.update(app, |panel, cx| panel.persist_related(cx));
                        });
                    }
                    btn
                };
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        warning_line(
                            cx.theme(),
                            format!("还有 {} 张没落库：{}", pending.len(), pending.join("、")),
                        )
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis(),
                    )
                    .child(persist_btn)
            }
        };

        // 多表结果（场景生成）：与「落库这 N 张」对称的批量草稿箱出口。
        // 单表结果（1 张）不显示——单表走出口区「保存到草稿箱」即可。
        // 没项目根不显示：草稿箱只写项目目录（与单表出口同口径）。
        let batch_row = {
            let count = self.results.len();
            let has_root = self.host.project_root().is_some();
            if count > 1 && has_root && !running {
                let entity = cx.entity();
                let mut btn = button(
                    "mock-scratchpad-all",
                    format!("全部存草稿箱（{} 张）", count),
                    ActionKind::Soft,
                )
                .xsmall();
                btn = btn.on_click(move |_, _, app| {
                    entity.update(app, |panel, cx| panel.scratchpad_all(cx));
                });
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .text_ellipsis()
                            .child("场景结果一键导出（CSV）："),
                    )
                    .child(btn)
            } else {
                div()
            }
        };

        // 任务进行中：进度条（组件，不手搓）+ 量纲文案 + 取消
        // 集合任务的进度与取消（单表任务的在中央表头，见 D38）
        let job_row = self.render_job_row(JobRowScope::Collection, cx);

        // 单表出口（落库 / 追加 / 草稿箱 / 另存为）已随表上移中央详情 tab（见 `render_sinks`）；
        // 这里只保留集合级动作、进度与边界说明。
        let read_only = self.host.read_only();

        // 边界状态一行说清（未打开项目 / 只读 / 常驻 M7）：原来三行常驻压缩成一条，
        // 只有当前生效的那条露面，完整句进 HoverCard（280px 面板里三行长说明 = 半屏清单）
        let project_open = self.host.project_root().is_some();
        let status_line = {
            let (main, detail) = if !project_open {
                (
                    "未打开项目：落库 / 追加 / 草稿箱不可用",
                    "未打开项目：落库 / 追加 / 草稿箱不可用（纯生成、预览与另存为不受影响）",
                )
            } else if read_only {
                (
                    "只读模式：不允许落库与写文件",
                    "只读模式：不允许落库与写文件（仍可生成预览）",
                )
            } else {
                (
                    "数据只写入项目分析库与文件，不回传源库（M7）",
                    "数据只写入项目分析库（{项目}/.RSmeta/analytics.duckdb）与文件，不回传源库（M7）",
                )
            };
            HoverCard::new("mock-sink-status-hint")
                .trigger(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .w_full()
                        // info 图标走 `info_hint_icon` 原语（尺寸 / 弱化色的单一来源）：
                        // 悬停明细的记号全模块同一档，用户才认得出「这里能悬停看详情」
                        .child(info_hint_icon(cx.theme()))
                        .child(card_note(cx.theme(), main)),
                )
                .content(move |_, _, _cx| {
                    div()
                        .p_2()
                        .text_xs()
                        .max_w(ui::STATUS_TOOLTIP_MAX_WIDTH)
                        .child(detail)
                })
        };

        // 不再自带 `p_2`：面板滚动容器已经给过一遍内距，这里再给就是 16px——
        // 而工具栏是 8px（`PANEL_PADDING`），清单的左边缘比工具栏右错 8px，
        // 一眼看上去就是没排过的表配置（工具栏有分隔线压着，错开格外显眼）
        div()
            .v_flex()
            .gap_2()
            .w_full()
            .child(table_list)
            .child(pending_row)
            .child(batch_row)
            // ② 集合级动作与它的进度：单表任务的在中央表头（D38）
            .child(scenario_actions)
            .child(job_row)
            // ③ 单表出口已随表上移中央 tab（生成 → 落地闭环，见 §9）；这里只留集合级与边界说明
            .child(status_line)
    }
}
