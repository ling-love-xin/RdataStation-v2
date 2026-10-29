//! 中间工作页工具栏（刷新 / 取样 / 导出 + 目标切换 + 内存占用读数）。

use super::*;

impl super::InsightWorkspacePanel {
    /// 工具栏（D66 对齐原型）：刷新 / 取样 / 导出 + 分隔线 + 目标切换菜单 + 当前目标 chip。
    ///
    /// - 刷新 = 重算当前画像；取样 = 重新取样并画像（源目标重走「取样 → 画像」两段）。
    /// - 导出报告尚无宿主写文件通道（列 / 结构导出各有事件，表画像导出未接）——
    ///   disabled + tooltip 标注，不做点了没反应的按钮。
    /// - 目标切换用语义 `Button::dropdown_menu`：方向键 / Escape / 焦点恢复由组件负责。
    pub(crate) fn render_toolbar(&self, entity: &Entity<Self>, theme: &Theme, cx: &Context<Self>) -> Div {
        let colors = theme.colors;
        // 菜单构造需要 opened 快照（闭包 'static，不能借面板状态）：只带 key + 标题
        let candidates: Vec<(String, String)> = self
            .opened
            .iter()
            .map(|e| (e.key.clone(), e.target.title()))
            .collect();
        let current_key = self.view.read(cx).target().map(target_key);
        // chip 文本：连接 / 来源描述 · 表名（原型「连接 · 表」）
        let chip = self
            .view
            .read(cx)
            .target()
            .map(|t| (t.detail().unwrap_or_else(|| "DuckDB".into()), t.title()));

        // 面板头命令按钮行（刷新/取样/导出）：2026-10-28 迁移到 `toolbar::Toolbar`
        // （容器层反例修复）——Toolbar 统一 size 托管 + roving 方向键焦点 + toolbar a11y；
        // 按钮保留 ghost 语义（`prepare_for_toolbar` 强制 ghost+compact，与面板头一致）。
        let refresh_btn = Button::new("insight-ws-refresh")
            .ghost()
            .icon(IconName::RotateCw)
            .label("刷新")
            .tooltip("重算当前画像")
            .on_click({
                let entity = entity.clone();
                move |_, _, app| {
                    entity.update(app, |panel, cx| panel.refresh(cx));
                }
            });
        let sample_btn = Button::new("insight-ws-sample")
            .ghost()
            .label("取样")
            .tooltip("重新从源库取样并画像（源目标）")
            .on_click({
                let entity = entity.clone();
                move |_, _, app| {
                    // 与 scope 按钮同口径：切回取样口径并重新画像。
                    // 对临时表/结果集目标静默忽略（数据已是既定内容）。
                    entity.update(app, |panel, cx| {
                        panel.view.update(cx, |view, cx| view.request_sample(cx));
                    });
                }
            });
        let export_btn = Button::new("insight-ws-export")
            .ghost()
            .icon(IconName::FileText)
            .label("导出")
            .tooltip("导出画像报告（Markdown）")
            .disabled(!matches!(
                *self.view.read(cx).state(),
                InsightPanelState::Data
            ))
            .on_click({
                let view = self.view.clone();
                move |_, _, app| {
                    // 报告内容由面板算好（D36：导出「用户看到的这份结论」）；
                    // 事件路径里组装，不占 render。无画像时 title 用于日志。
                    let title = view
                        .read(app)
                        .target()
                        .map(|t| t.title())
                        .unwrap_or_default();
                    let content = view
                        .read(app)
                        .data()
                        .table
                        .as_ref()
                        .map(render_markdown_report);
                    if let Some(content) = content {
                        view.update(app, |v, cx| {
                            v.request_report_export(content, cx);
                        });
                    } else {
                        tracing::info!("导出报告：无画像数据（{title}）");
                    }
                }
            });
        // 面板头容器：背景/边框/高度外层自画；命令按钮行由 Toolbar 渲染
        // （统一 size 托管 + roving 方向键焦点 + toolbar a11y 语义）。
        div()
            .flex_none()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .px(rems(ui::PANEL_PADDING))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(colors.border)
            .child(
                Toolbar::new("insight-ws-toolbar")
                    .xsmall()
                    .children([refresh_btn, sample_btn, export_btn]),
            )
            .child(divider(theme))
            // 目标切换下拉：已进入洞察的目标 → 切换画像；末项 → 打开多表对比。
            // 菜单闭包要 'static：先克隆实体句柄，不借用参数引用
            .child({
                let entity_owned = entity.clone();
                Button::new("insight-ws-target-switch")
                    .ghost()
                    .xsmall()
                    // 常显下拉箭头：DropdownButton 的 caret 半块在 ghost 变体下
                    // hover 才显现，静止时只剩表名 label——用户看不出可下拉
                    .icon(IconName::ChevronDown)
                    .label(
                        chip.as_ref()
                            .map(|(_, title)| title.clone())
                            .unwrap_or_else(|| "切换目标".into()),
                    )
                    .tooltip("切换洞察目标")
                    .dropdown_menu(move |menu, _, _| {
                        let mut menu = menu;
                        for (key, title) in &candidates {
                            let entity = entity_owned.clone();
                            let key = key.clone();
                            let item = PopupMenuItem::new(title.clone())
                                .checked(Some(key.as_str()) == current_key.as_deref())
                                .on_click(move |_, _, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.open_candidate(&key, cx);
                                    });
                                });
                            menu = menu.item(item);
                        }
                        if !candidates.is_empty() {
                            menu = menu.separator();
                        }
                        let open_compare = entity_owned.clone();
                        menu.item(
                            PopupMenuItem::new("打开多表对比…").on_click(move |_, _, app| {
                                open_compare.update(app, |panel, cx| {
                                    panel.set_mode(WorkspaceMode::Compare, cx);
                                });
                            }),
                        )
                    })
            })
            .child(div().flex_1().min_w_0())
            // 当前目标 chip（原型 target-chip：连接 · 表）
            .when_some(chip, |this, (source, title)| {
                this.child(
                    div()
                        .flex_none()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .px_1()
                        .h(rems(ui::ROW_HEIGHT * 0.8))
                        .rounded_sm()
                        .border_1()
                        .border_color(colors.border)
                        .text_xs()
                        .child(
                            div()
                                .max_w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                .text_ellipsis()
                                .text_color(colors.muted_foreground)
                                .child(source),
                        )
                        .child(div().text_color(colors.muted_foreground).child("·"))
                        .child(
                            div()
                                .max_w(rems(ui::INSIGHT_WS_SOURCE_WIDTH))
                                .text_ellipsis()
                                .text_color(colors.foreground)
                                .child(title),
                        ),
                )
            })
            // 分析引擎内存占用（设置「显示内存占用」关闭时整段不渲染）
            .when(self.view.read(cx).status_usage, |bar| {
                bar.child(self.render_memory_usage(cx, theme))
            })
    }

    /// 内存占用只读文本：「内存 X MB / 上限 Y MB · 临时表 N」。
    ///
    /// 读数由后台轮询回填（`InsightView::memory`，5 秒一次）；还没读数时显示
    /// 「内存 —」（不比 0 更有信息量）。上限取分析引擎实际生效值（含设置页改动）。
    pub(crate) fn render_memory_usage(&self, cx: &Context<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        let Some(stats) = self.view.read(cx).memory else {
            return div()
                .flex_none()
                .px_2()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child("内存 —");
        };
        let used_mb = stats.database_bytes.unwrap_or(0) / (1024 * 1024);
        let limit_mb = stats.limit_bytes / (1024 * 1024);
        let text = if limit_mb > 0 {
            format!(
                "内存 {used_mb} MB / {limit_mb} MB · 表 {}",
                stats.table_count
            )
        } else {
            format!("内存 {used_mb} MB · 表 {}", stats.table_count)
        };
        div()
            .flex_none()
            .px_2()
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(text)
    }
}
