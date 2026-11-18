//! 弹窗域（`panel/` 第五块）：SQL 预览弹出层（等效浮层 + kit Textarea）。

use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState, Textarea};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use super::data::ResultPanel;
use crate::ports::{FilterSettingsTab, ResultRequest};
use crate::ui;

impl ResultPanel {
    pub(crate) fn render_sql_dialog(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let close_queue = self.queue.clone();
        let close_queue2 = self.queue.clone();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            // 遮罩：走主题 `overlay` token（明暗各一套），不再写死半透明黑。
            // ⚠️ `overlay` 与 Quick Open 的遮罩是同一个 token（palette.rs:715），此处靠它表达
            // 「模态」强度；两者若要分档，须等 schema 支持独立 scrim 键（0.8.1 无，
            // colors 内写未知键会被 serde 忽略并回退透明）。
            // 无点击关闭（kit Div 无 on_click，交互走 ✕ 按钮 / Esc）
            .bg(theme.colors.overlay)
            .debug_selector(|| "editor-result-sql-dialog".to_string())
            .on_key_down(move |event, _window, app| {
                if event.keystroke.key == "escape" {
                    close_queue.request(ResultRequest::CloseSqlDialog, app);
                }
            })
            .child(
                div()
                    .relative()
                    .flex_col()
                    .w(px(680.))
                    .max_h(px(480.))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.background)
                    .overflow_hidden()
                    .debug_selector(|| "editor-result-sql-dialog-card".to_string())
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .border_b(ui::HAIRLINE)
                            .border_color(theme.colors.border)
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .min_w_0()
                                    .child(div().text_sm().flex_none().child("原始 SQL"))
                                    // 【B5 余项】来源行：血缘 + 连接（B15 血缘摘要自
                                    // 2026-10-28 状态栏三项重构后断头，这里把它接回——
                                    // 看 SQL 全文时顺带知道这份数据怎么来的）。
                                    // 数据来自工具栏投影（title 优先，其次血缘），
                                    // 没有就整段缺席，不编。
                                    .children(self.toolbar.as_ref().and_then(|toolbar| {
                                        let lineage = toolbar.lineage.as_deref()?;
                                        let connection = toolbar.connection.as_deref();
                                        let text = match connection {
                                            Some(connection) => {
                                                format!("来自{lineage} · {connection}")
                                            }
                                            None => format!("来自{lineage}"),
                                        };
                                        Some(
                                            div()
                                                .text_xs()
                                                .text_color(theme.colors.muted_foreground)
                                                .text_ellipsis()
                                                .overflow_hidden()
                                                .debug_selector(|| {
                                                    "editor-result-sql-dialog-lineage".to_string()
                                                })
                                                .child(SharedString::from(text)),
                                        )
                                    })),
                            )
                            .child(
                                Button::new("editor-result-sql-dialog-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .debug_selector(|| "editor-result-sql-dialog-close".to_string())
                                    .tooltip("关闭".to_string())
                                    .on_click(move |_, _window, app| {
                                        close_queue2.request(ResultRequest::CloseSqlDialog, app);
                                    }),
                            ),
                    )
                    .child(
                        div().flex_1().min_h_0().p_3().child(
                            self.sql_textarea
                                .as_ref()
                                .map(|ta| {
                                    Textarea::new(ta)
                                        .readonly(true)
                                        .h(px(360.))
                                        .into_any_element()
                                })
                                .unwrap_or_else(|| {
                                    div()
                                        .p_4()
                                        .text_color(theme.colors.muted_foreground)
                                        .child("（无 SQL）")
                                        .into_any_element()
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// 【筛选设置】自定义条件 / 排序浮层
    ///
    /// 对齐 DBeaver `FilterSettingsDialog` 的 **Custom** 页签（`createCustomFilters`，
    /// `FilterSettingsDialog.java:425-451`）：两个自由文本 `whereText` / `orderText`，
    /// 应用时 `setWhere(this.getText())` / `setOrder(this.getText())`（`:493-500`）；
    /// 另有警告文案「自定义排序会禁用列排序」（`controls_resultset_filter_warning_custom_order_disabled`）。
    ///
    /// 本仓口径差异（有意）：
    /// - WHERE 一栏**只读回显**当前筛选文本，不在浮层里放第二个输入源——筛选框本体就是它的
    ///   唯一编辑处（「同一个 `InputState` 只渲染一处」，见 gpui-kit-dev 规范）。
    /// - 浮层是本面板内的 absolute 遮罩（与 SQL 预览弹窗同一范式），不是窗口级 dialog：
    ///   面板拿不到 `&mut Window` 的 render 路径，且宿主已用同一范式落地。
    pub(crate) fn render_filter_settings(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let close_queue = self.queue.clone();
        let close_queue2 = self.queue.clone();
        let esc_queue = self.queue.clone();
        let apply_queue = self.queue.clone();
        let order = self.filter_order_input.clone();
        // WHERE 摘要：当前筛选文本（空则提示去哪填）
        let where_text: SharedString = match self.filter_text.trim() {
            "" => "（空）在筛选框里填条件".into(),
            text => text.to_string().into(),
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.colors.overlay)
            .debug_selector(|| "editor-result-filter-settings".to_string())
            .on_key_down(move |event, _window, app| {
                if event.keystroke.key == "escape" {
                    esc_queue.request(ResultRequest::CloseFilterSettings, app);
                }
            })
            .child(
                div()
                    .relative()
                    .flex_col()
                    .w(px(ui::RESULT_FILTER_SETTINGS_WIDTH * 16.0))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.background)
                    .overflow_hidden()
                    .debug_selector(|| "editor-result-filter-settings-card".to_string())
                    // 标题行：标题 + ✕
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .border_b(ui::HAIRLINE)
                            .border_color(theme.colors.border)
                            .child(div().text_sm().child("筛选设置"))
                            .child(
                                Button::new("editor-result-filter-settings-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .debug_selector(|| {
                                        "editor-result-filter-settings-close".to_string()
                                    })
                                    .tooltip("关闭".to_string())
                                    .on_click(move |_, _window, app| {
                                        close_queue2.request(ResultRequest::CloseFilterSettings, app);
                                    }),
                            ),
                    )
                    // 页签条：列约束 / 自定义（DBeaver 对话框的两页，默认列页）
                    .child(
                        div()
                            .px_3()
                            .pt_2()
                            .debug_selector(|| "editor-result-filter-settings-tabs".to_string())
                            .child({
                                let tabs_queue = self.queue.clone();
                                let selected = match self.filter_settings_tab {
                                    FilterSettingsTab::Columns => 0,
                                    FilterSettingsTab::Custom => 1,
                                };
                                TabBar::new("editor-result-filter-settings-tabbar")
                                    .segmented()
                                    .selected_index(selected)
                                    .children([
                                        {
                                            let queue = tabs_queue.clone();
                                            Tab::new().label("列约束").on_click(
                                                move |_, _window, app| {
                                                    queue.request(
                                                        ResultRequest::SetFilterSettingsTab(
                                                            FilterSettingsTab::Columns,
                                                        ),
                                                        app,
                                                    );
                                                },
                                            )
                                        },
                                        {
                                            let queue = tabs_queue.clone();
                                            Tab::new().label("自定义条件").on_click(
                                                move |_, _window, app| {
                                                    queue.request(
                                                        ResultRequest::SetFilterSettingsTab(
                                                            FilterSettingsTab::Custom,
                                                        ),
                                                        app,
                                                    );
                                                },
                                            )
                                        },
                                    ])
                            }),
                    )
                    // 页内容：列约束 / 自定义
                    .child(match self.filter_settings_tab {
                        FilterSettingsTab::Columns => self.render_filter_settings_columns(cx),
                        FilterSettingsTab::Custom => self.render_filter_settings_custom(
                            cx,
                            where_text,
                            order.clone(),
                        ),
                    })
                    .child(
                        // 页脚：提示 + 取消 / 应用（列约束页只在自定义页可「应用」）
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_3()
                            .py_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(match self.filter_settings_tab {
                                        FilterSettingsTab::Columns => {
                                            "列约束改完即时生效（与列头菜单同一通道落盘）"
                                        }
                                        FilterSettingsTab::Custom => {
                                            "应用后按条件重查源库，替换当前结果集"
                                        }
                                    }),
                            )
                            .child(
                                Button::new("editor-result-filter-settings-cancel")
                                    .ghost()
                                    .small()
                                    .label("取消")
                                    .debug_selector(|| {
                                        "editor-result-filter-settings-cancel".to_string()
                                    })
                                    .on_click(move |_, _window, app| {
                                        close_queue.request(ResultRequest::CloseFilterSettings, app);
                                    }),
                            )
                            .child(
                                Button::new("editor-result-filter-settings-apply")
                                    .primary()
                                    .small()
                                    .label("应用")
                                    .disabled(self.filter_settings_tab != FilterSettingsTab::Custom)
                                    .debug_selector(|| {
                                        "editor-result-filter-settings-apply".to_string()
                                    })
                                    .on_click(move |_, _window, app| {
                                        apply_queue.request(ResultRequest::CommitFilterSettings, app);
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 【筛选设置 · 列约束】列清单（列名 / 可见 / 冻结 / 条件 / 排序 / 位置）+ 页内工具条。
    ///
    /// 对齐 DBeaver `FilterSettingsDialog` 的 Columns 页签：`addColumn`(名称, `:129`) /
    /// `addBooleanColumn`(可见 `:148`、固定 `:178`) / order(`:214`) / criteria(`:230`)，
    /// 以及页顶工具条（`:276-325`）。
    ///
    /// 本仓口径：
    /// - **可见与冻结可切**（改网格 delegate + `ColumnLayoutChanged` 落盘，与列头菜单同一条路）；
    /// - **条件 / 排序只读投影**：条件取列级筛选（`delegate.column_filter()`，从列头菜单来）、
    ///   排序取本地排序（`delegate.sort_state()`）；浮层里不摆假控件；
    /// - **工具条只接三颗有落点的**：全部显示 / 全部不显示 / 重置（列布局）。DBeaver 还有
    ///   移顶 / 上移 / 下移 / 移底 / 排序（列名 A→Z）——本仓列序改动只有拖拽一条通道
    ///   （组件侧列序接口未接 UI），故不摆（见 `result-panel.md §8.72`）。
    fn render_filter_settings_columns(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let grid = self.grid.clone();
        let layout_queue = self.queue.clone();
        let rows = self.column_constraint_rows(cx);

        // 页内工具条：三颗（全部显示 / 全部不显示 / 重置）+ 四颗移动钮 + 排序，
        // 顺序与 DBeaver `FilterSettingsDialog.java:276-325` 一致。
        // 移动四钮作用在**当前选中列**（选中列不存在 / 已是首列或末列时置灰）。
        let show_all_queue = self.queue.clone();
        let hide_all_queue = self.queue.clone();
        let reset_queue = self.queue.clone();
        let move_state = self.selected_col.and_then(|data| {
            let delegate = self.grid.read(cx).delegate();
            delegate.view_column(data).map(|view| {
                let last = delegate.visible_column_count().saturating_sub(1);
                (view, last)
            })
        });
        let sort_queue = self.queue.clone();
        let move_button = |id: &'static str,
                           label: &'static str,
                           icon: IconName,
                           direction: crate::ports::ColumnMove,
                           enabled: bool| {
            let queue = self.queue.clone();
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(label.to_string())
                .disabled(!enabled)
                .debug_selector(move || id.to_string())
                .on_click(move |_, _window, app| {
                    queue.request(ResultRequest::MoveSelectedColumn(direction), app);
                })
        };
        let toolbar = Toolbar::new("editor-result-filter-settings-columns-toolbar")
            .xsmall()
            .p_0()
            .contents([
                move_button(
                    "editor-result-filter-settings-move-top",
                    "移到最前",
                    IconName::ArrowUpToLine,
                    crate::ports::ColumnMove::Top,
                    move_state.is_some_and(|(view, _)| view > 0),
                )
                .into_any_element(),
                move_button(
                    "editor-result-filter-settings-move-up",
                    "上移一列",
                    IconName::ArrowUp,
                    crate::ports::ColumnMove::Up,
                    move_state.is_some_and(|(view, _)| view > 0),
                )
                .into_any_element(),
                move_button(
                    "editor-result-filter-settings-move-down",
                    "下移一列",
                    IconName::ArrowDown,
                    crate::ports::ColumnMove::Down,
                    move_state.is_some_and(|(view, last)| view < last),
                )
                .into_any_element(),
                move_button(
                    "editor-result-filter-settings-move-bottom",
                    "移到最后",
                    IconName::ArrowDownToLine,
                    crate::ports::ColumnMove::Bottom,
                    move_state.is_some_and(|(view, last)| view < last),
                )
                .into_any_element(),
                Separator::vertical().into_any_element(),
                Button::new("editor-result-filter-settings-sort-name")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ArrowUpAZ)
                    .tooltip("按列名排序（A→Z）".to_string())
                    .debug_selector(|| "editor-result-filter-settings-sort-name".to_string())
                    .on_click(move |_, _window, app| {
                        sort_queue.request(ResultRequest::SortColumnsByName, app);
                    })
                    .into_any_element(),
                Separator::vertical().into_any_element(),
                Button::new("editor-result-filter-settings-show-all")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Eye)
                    .tooltip("全部显示".to_string())
                    .debug_selector(|| "editor-result-filter-settings-show-all".to_string())
                    .on_click(move |_, _window, app| {
                        show_all_queue.request(ResultRequest::ColumnsShowAll, app);
                    })
                    .into_any_element(),
                Button::new("editor-result-filter-settings-hide-all")
                    .ghost()
                    .xsmall()
                    .icon(IconName::EyeOff)
                    .tooltip("全部不显示（至少保留一列可见）".to_string())
                    .debug_selector(|| "editor-result-filter-settings-hide-all".to_string())
                    .on_click(move |_, _window, app| {
                        hide_all_queue.request(ResultRequest::ColumnsHideAll, app);
                    })
                    .into_any_element(),
                Button::new("editor-result-filter-settings-reset")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RotateCcw)
                    .tooltip("重置列布局（列序 + 显示全部 + 取消全部冻结）".to_string())
                    .debug_selector(|| "editor-result-filter-settings-reset".to_string())
                    .on_click(move |_, _window, app| {
                        reset_queue.request(ResultRequest::ColumnsResetLayout, app);
                    })
                    .into_any_element(),
            ]);

        let header = |text: &'static str| {
            div()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(text)
        };
        // 「条件 / 排序」两列固定窄宽，条件过长由 ellipsis 收口
        let criteria_width = rems(9.);
        let sort_width = rems(2.);

        let mut list = div().v_flex().gap_1().max_h(rems(18.)).overflow_y_scrollbar();
        for (index, row) in rows.iter().enumerate() {
            let name = row.name.clone();
            let visible = row.visible;
            let frozen = row.frozen;
            let criteria = row
                .criteria
                .clone()
                .unwrap_or_else(|| "—".to_string());
            let criteria_set = row.criteria.is_some();
            let sort_mark = match row.sort {
                Some(true) => "↓",
                Some(false) => "↑",
                None => "—",
            };

            // 可见：勾上 = 在网格里显示（隐藏列改 delegate，至少留 1 列可见由它兜底）
            let grid_vis = grid.clone();
            let queue_vis = layout_queue.clone();
            let vis_box = Checkbox::new(ElementId::Name(
                SharedString::from(format!("editor-result-filter-settings-vis-{index}")),
            ))
            .checked(visible)
            .accessibility_label(format!("{name} 可见"))
            .on_click(move |_checked, _window, app| {
                let show = !visible;
                grid_vis.update(app, |state, cx| {
                    state.delegate_mut().set_columns_hidden(&[index], !show);
                    state.refresh(cx);
                });
                queue_vis.request(ResultRequest::ColumnLayoutChanged, app);
            });

            // 固定：冻结列（集合语义，同列头菜单的 `ToggleFreeze`）
            let grid_pin = grid.clone();
            let queue_pin = layout_queue.clone();
            let pin_box = Checkbox::new(ElementId::Name(
                SharedString::from(format!("editor-result-filter-settings-pin-{index}")),
            ))
            .checked(frozen)
            .accessibility_label(format!("{name} 冻结"))
            .on_click(move |_checked, _window, app| {
                grid_pin.update(app, |state, cx| {
                    state.delegate_mut().toggle_freeze(index);
                    state.refresh(cx);
                });
                queue_pin.request(ResultRequest::ColumnLayoutChanged, app);
            });

            list = list.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .debug_selector({
                        let index = index;
                        move || format!("editor-result-filter-settings-row-{index}")
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .text_ellipsis()
                            .overflow_hidden()
                            .child(name.clone()),
                    )
                    .child(vis_box)
                    .child(pin_box)
                    // 条件（只读投影；有值才用正文色，避免满屏都是「—」抢注意力）
                    .child(
                        div()
                            .w(criteria_width)
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .overflow_hidden()
                            .text_color(if criteria_set {
                                theme.colors.foreground
                            } else {
                                theme.colors.muted_foreground
                            })
                            .debug_selector({
                                let index = index;
                                move || format!("editor-result-filter-settings-criteria-{index}")
                            })
                            .child(criteria),
                    )
                    // 排序（只读投影：↑ 升序 / ↓ 降序）
                    .child(
                        div()
                            .w(sort_width)
                            .text_xs()
                            .text_center()
                            .text_color(theme.colors.muted_foreground)
                            .debug_selector({
                                let index = index;
                                move || format!("editor-result-filter-settings-sort-{index}")
                            })
                            .child(sort_mark),
                    )
                    .child(
                        div()
                            .w(rems(2.))
                            .text_right()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("{}", index + 1)),
                    ),
            );
        }

        div()
            .v_flex()
            .gap_1()
            .px_3()
            .pt_3()
            .child(toolbar)
            // 表头：与行同布局（列名 / 可见 / 冻结 / 条件 / 排序 / 位置）
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(div().flex_1().min_w_0().child(header("列")))
                    .child(div().child(header("可见")))
                    .child(div().child(header("冻结")))
                    .child(div().w(criteria_width).child(header("条件")))
                    .child(div().w(sort_width).text_center().child(header("排序")))
                    .child(div().w(rems(2.)).text_right().child(header("#"))),
            )
            .child(list)
            .into_any_element()
    }

    /// 【筛选设置 · 自定义条件】WHERE 只读回显 + 自定义 ORDER BY 输入
    fn render_filter_settings_custom(
        &self,
        cx: &App,
        where_text: SharedString,
        order: Entity<InputState>,
    ) -> AnyElement {
        let theme = cx.theme();
        div()
            .v_flex()
            .gap_2()
            .px_3()
            .pt_3()
            // 条件行（只读）
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("WHERE 条件（在筛选框里编辑）"),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(theme.colors.border)
                            .bg(theme.colors.muted)
                            .text_sm()
                            .text_ellipsis()
                            .overflow_hidden()
                            .debug_selector(|| "editor-result-filter-settings-where".to_string())
                            .child(where_text),
                    ),
            )
            // 排序行（唯一可编辑项）
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("自定义排序（留空 = 沿用原查询的 ORDER BY）"),
                    )
                    .child(
                        div()
                            .debug_selector(|| "editor-result-filter-settings-order".to_string())
                            .child(Input::new(&order).small()),
                    ),
            )
            .into_any_element()
    }
}
