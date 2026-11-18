//! 弹窗域（`panel/` 第五块）：SQL 预览弹出层（等效浮层 + kit Textarea）。

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use super::data::ResultPanel;
use crate::ports::ResultRequest;
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
                    // 条件行（只读）
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .px_3()
                            .pt_3()
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
                                    .debug_selector(|| {
                                        "editor-result-filter-settings-where".to_string()
                                    })
                                    .child(where_text),
                            ),
                    )
                    // 排序行（唯一可编辑项）
                    .child(
                        div()
                            .v_flex()
                            .gap_1()
                            .px_3()
                            .pt_3()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("自定义排序（留空 = 沿用原查询的 ORDER BY）"),
                            )
                            .child(
                                div()
                                    .debug_selector(|| {
                                        "editor-result-filter-settings-order".to_string()
                                    })
                                    .child(Input::new(&order).small()),
                            ),
                    )
                    // 页脚：提示（对齐 DBeaver 的自定义排序警告）+ 应用 / 取消
                    .child(
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
                                    .child("应用后按条件重查源库，替换当前结果集"),
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
}
