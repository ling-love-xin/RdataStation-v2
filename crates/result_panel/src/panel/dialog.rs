//! 弹窗域（`panel/` 第五块）：SQL 预览弹出层（等效浮层 + kit Textarea）。

use shared::i18n::tr;
use gpui_kit::assets::IconName;
use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState, Textarea};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::*;

// 【B5】事实帧只读句柄随网格投影进 delegate（列式统计 / 原生数值排序交互派生时读取）

use super::data::ResultPanel;
use crate::ports::{FilterSettingsTab, ResultRequest};
use crate::ui;

impl ResultPanel {
    pub(crate) fn render_sql_dialog(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
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
            // 【第 81 批】`ESC` 收起统一由面板根的 `DismissOverlay` 收口：原来这里的
            // `on_key_down` 在焦点落进浮层输入框后永远收不到（`ESC` 被 input 自己的
            // binding 抢走，propagate 只找下一条 binding），已删。
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
                                    .child(div().text_sm().flex_none().child(
                                        self.sql_dialog_title
                                            .clone()
                                            .unwrap_or_else(|| "原始 SQL".into()),
                                    ))
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
                                    .tooltip(tr("关闭").to_string())
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
                                        .child(tr("（无 SQL）"))
                                        .into_any_element()
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }

    /// 【第 75 批】「跳转到列」浮层（DBeaver `CMD_GO_TO_COLUMN` → `askForColumnToGo`，
    /// `ResultSetHandlerMain:526-534 / 762-849`；标题文案取自 `bundle_zh.properties:252-253`
    /// 「跳转到列」）。
    ///
    /// **有意差异**（三处，都是为了本仓输入组件的能力边界）：
    /// 1. 上游是 `InputDialog`（模态窗口），本仓做成面板内遮罩卡片——与 SQL 预览 /
    ///    筛选设置 / 取全部确认同一范式（理由见 `render_filter_settings` 注释）；
    /// 2. 上游预填当前属性名且默认全选，本仓把当前列名放 placeholder（`InputState`
    ///    没有 `select_all`，预填后敲字会拼成误值）；
    /// 3. 上游带后缀匹配的内容补全（`ContentAssistUtils.installContentProposal`，
    ///    `:836-845`），本仓 `InputState` 无补全通道——留待输入组件补齐。
    ///
    /// 提交路径与 Enter 同一条终点：这里只入队 `SubmitGotoColumn`，宿主拿 window 后调
    /// `ResultPanel::submit_goto_column`（解析 / 报错 / 焦点回收都在那边）。
    pub(crate) fn render_goto_column(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let close_queue = self.queue.clone();
        let cancel_queue = self.queue.clone();
        let submit_queue = self.queue.clone();
        let columns = self.grid.read(cx).delegate().columns().len();
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.colors.overlay)
            .debug_selector(|| "editor-result-goto-column".to_string())
            // 【第 81 批】`ESC` 收起统一由面板根的 `DismissOverlay` 收口（原 `on_key_down` 已删）
            .child(
                div()
                    .relative()
                    .flex_col()
                    .w(px(ui::RESULT_GOTO_COLUMN_WIDTH * 16.0))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.background)
                    .overflow_hidden()
                    .debug_selector(|| "editor-result-goto-column-card".to_string())
                    // 标题行：「跳转到列」+ ✕
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .px_3()
                            .py_2()
                            .border_b(ui::HAIRLINE)
                            .border_color(theme.colors.border)
                            .child(div().text_sm().child(tr("跳转到列")))
                            .child(
                                Button::new("editor-result-goto-column-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .debug_selector(|| "editor-result-goto-column-close".to_string())
                                    .tooltip(tr("关闭").to_string())
                                    .on_click(move |_, _window, app| {
                                        close_queue.request(ResultRequest::CloseGotoColumn, app);
                                    }),
                            ),
                    )
                    // 正文：输入框 + 错误行 + 口径提示
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .px_3()
                            .py_3()
                            .debug_selector(|| "editor-result-goto-column-body".to_string())
                            .child(
                                gpui_kit::component::input::Input::new(&self.goto_column_input)
                                    .with_size(gpui_kit::component::Size::Small),
                            )
                            .children(self.goto_column_error.as_ref().map(|error| {
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.danger)
                                    .debug_selector(|| {
                                        "editor-result-goto-column-error".to_string()
                                    })
                                    .child(error.clone())
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(SharedString::from(format!(
                                        "共 {columns} 列；列名不区分大小写，也可输入从 1 起的序号。"
                                    ))),
                            ),
                    )
                    // 页脚：口径提示 + 取消 / 跳转
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
                                    .child(tr("跳转后保持当前行，只横向滚到该列。")),
                            )
                            .child(
                                Button::new("editor-result-goto-column-cancel")
                                    .ghost()
                                    .small()
                                    .label(tr("取消"))
                                    .debug_selector(|| "editor-result-goto-column-cancel".to_string())
                                    .on_click(move |_, _window, app| {
                                        cancel_queue.request(ResultRequest::CloseGotoColumn, app);
                                    }),
                            )
                            .child(
                                Button::new("editor-result-goto-column-go")
                                    .primary()
                                    .small()
                                    .label(tr("跳转"))
                                    .debug_selector(|| "editor-result-goto-column-go".to_string())
                                    .on_click(move |_, _window, app| {
                                        submit_queue.request(ResultRequest::SubmitGotoColumn, app);
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 【第 72 批】「获取全部数据」确认浮层
    /// （DBeaver `CONFIRM_RS_FETCH_ALL`：`ConfirmationDialog.WARNING` + `QUESTION`，
    /// `ResultSetViewer.readAllData:4433-4440`）。
    ///
    /// 文案取自 `bundle_zh.properties:161-162`：
    /// - 标题「读取所有行」；
    /// - 正文「读取结果集中的所有行会会花费大量时间和内存。确定要获取所有行吗？」
    ///
    /// **有意差异**：中文包原文是「会**会**花费」（重复字，上游笔误），本仓写作
    /// 「会花费」。第二段另加一句本仓事实：当前已取多少行——DBeaver 只说「会花时间
    /// 和内存」，用户点下去前其实不知道规模；这里把已抓行数摆出来（0 行也有意义）。
    ///
    /// 与 SQL 预览 / 筛选设置同一范式：面板内 absolute 遮罩，不是窗口级 dialog。
    pub(crate) fn render_fetch_all_confirm(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let close_queue = self.queue.clone();
        let close_queue2 = self.queue.clone();
        let go_queue = self.queue.clone();
        let rows = self.status.as_ref().map(|status| status.total_rows);
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.colors.overlay)
            .debug_selector(|| "editor-result-fetch-all-confirm".to_string())
            // 【第 81 批】`ESC` 收起统一由面板根的 `DismissOverlay` 收口（原 `on_key_down` 已删）
            .child(
                div()
                    .relative()
                    .flex_col()
                    .w(px(ui::RESULT_FETCH_ALL_CONFIRM_WIDTH * 16.0))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.background)
                    .overflow_hidden()
                    .debug_selector(|| "editor-result-fetch-all-confirm-card".to_string())
                    // 标题行：警告图标 + 「读取所有行」+ ✕
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
                                    .child(
                                        Icon::new(IconName::TriangleAlert)
                                            .text_color(theme.colors.warning),
                                    )
                                    .child(div().text_sm().child(tr("读取所有行"))),
                            )
                            .child(
                                Button::new("editor-result-fetch-all-confirm-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .debug_selector(|| {
                                        "editor-result-fetch-all-confirm-close".to_string()
                                    })
                                    .tooltip(tr("关闭").to_string())
                                    .on_click(move |_, _window, app| {
                                        close_queue
                                            .request(ResultRequest::CloseFetchAllConfirm, app);
                                    }),
                            ),
                    )
                    // 正文：两段（警告 + 问句），再补一行本仓事实
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .px_3()
                            .py_3()
                            .debug_selector(|| {
                                "editor-result-fetch-all-confirm-body".to_string()
                            })
                            .child(
                                div()
                                    .text_sm()
                                    .child(tr("读取结果集中的所有行会花费大量时间和内存。")),
                            )
                            .child(div().text_sm().child(tr("确定要获取所有行吗？")))
                            .children(rows.map(|rows| {
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .debug_selector(|| {
                                        "editor-result-fetch-all-confirm-rows".to_string()
                                    })
                                    .child(SharedString::from(format!(
                                        "当前已取 {rows} 行，新抓到的行会接在后面。"
                                    )))
                            })),
                    )
                    // 页脚：提示 + 取消 / 继续
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
                                    .child(tr("取数期间切换结果集会取消这次取全部。")),
                            )
                            .child(
                                Button::new("editor-result-fetch-all-cancel")
                                    .ghost()
                                    .small()
                                    .label(tr("取消"))
                                    .debug_selector(|| {
                                        "editor-result-fetch-all-cancel".to_string()
                                    })
                                    .on_click(move |_, _window, app| {
                                        close_queue2
                                            .request(ResultRequest::CloseFetchAllConfirm, app);
                                    }),
                            )
                            .child(
                                Button::new("editor-result-fetch-all-go")
                                    .primary()
                                    .small()
                                    .label(tr("继续"))
                                    .debug_selector(|| "editor-result-fetch-all-go".to_string())
                                    .on_click(move |_, _window, app| {
                                        go_queue.request(ResultRequest::FetchAll, app);
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 【自动刷新】「自定义...」配置对话框（`AutoRefreshConfigDialog.java:31-79`）。
    ///
    /// 真值是一个独立 `BaseDialog`（标题 `"Auto-refresh configuration"`，图标
    /// `UIIcon.CLOCK_START`），内容区 `createTitledComposite(..., "Settings", 2, FILL_BOTH)`
    /// 里两个控件：
    /// 1. `createLabelSpinner("Interval (sec)", "Auto-refresh interval in seconds", 当前, 1, MAX)`
    ///    —— 只能输正整数；
    /// 2. `createCheckbox("Stop on error", "Stop auto-refresh if error happens", 当前, 2)`。
    ///
    /// 本仓落法（三处有意差异，全部登记在 `result-panel.md` §9.68）：
    /// - 面板内模态浮层（与 SQL 预览 / 筛选设置同范式），不是窗口级 dialog；
    /// - 普通输入框 + 解析兜底（`InputState` 没有 spinner 档；非法输入不覆盖已生效间隔）；
    /// - 标题不带图标（`IconName` 里没有对应 `CLOCK_START` 的项，不拿别的图标顶替）。
    pub(crate) fn render_auto_refresh_config(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let close_queue = self.queue.clone();
        let close_queue2 = self.queue.clone();
        let apply_queue = self.queue.clone();
        let toggle_queue = self.queue.clone();
        let current_interval = self.auto_refresh_interval_secs;
        let stop_on_error = self.auto_refresh_stop_on_error;
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.colors.overlay)
            .debug_selector(|| "editor-result-auto-refresh-config".to_string())
            // `ESC` 收起同样由面板根的 `DismissOverlay` 收口
            .child(
                div()
                    .relative()
                    .flex_col()
                    .w(px(420.))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.background)
                    .overflow_hidden()
                    .debug_selector(|| "editor-result-auto-refresh-config-card".to_string())
                    // 标题行：title + ✕
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
                                    .text_sm()
                                    .child(crate::auto_refresh::AUTO_REFRESH_CONFIG_TITLE),
                            )
                            .child(
                                Button::new("editor-result-auto-refresh-config-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .debug_selector(|| {
                                        "editor-result-auto-refresh-config-close".to_string()
                                    })
                                    .tooltip(tr("关闭").to_string())
                                    .on_click(move |_, _window, app| {
                                        close_queue.request(
                                            ResultRequest::CloseAutoRefreshConfig,
                                            app,
                                        );
                                    }),
                            ),
                    )
                    // 内容：带框「设置」分组（真值 `createTitledComposite`）
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .m_3()
                            .p_2()
                            .rounded_sm()
                            .border_1()
                            .border_color(theme.colors.border)
                            .debug_selector(|| {
                                "editor-result-auto-refresh-config-settings".to_string()
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(crate::auto_refresh::AUTO_REFRESH_CONFIG_GROUP),
                            )
                            // 间隔行：label + 输入框 + 说明
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(rems(6.))
                                            .text_sm()
                                            .child(
                                                crate::auto_refresh::AUTO_REFRESH_CONFIG_INTERVAL_LABEL,
                                            ),
                                    )
                                    .child(
                                        div()
                                            .w(rems(9.))
                                            .debug_selector(|| {
                                                "editor-result-auto-refresh-config-interval"
                                                    .to_string()
                                            })
                                            .child(Input::new(&self.auto_refresh_interval_input).small()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_xs()
                                            .text_color(theme.colors.muted_foreground)
                                            .child(
                                                crate::auto_refresh::AUTO_REFRESH_CONFIG_INTERVAL_HINT,
                                            ),
                                    ),
                            )
                            // 勾选行：错误时停止（点即写）
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .debug_selector(|| {
                                        "editor-result-auto-refresh-config-stop-on-error".to_string()
                                    })
                                    .child(
                                        Checkbox::new(ElementId::Name(SharedString::from(
                                            "editor-result-auto-refresh-config-stop-on-error-box",
                                        )))
                                        .checked(stop_on_error)
                                        .accessibility_label(
                                            crate::auto_refresh::AUTO_REFRESH_CONFIG_STOP_ON_ERROR_LABEL
                                                .to_string(),
                                        )
                                        .on_click(move |_checked, _window, app| {
                                            toggle_queue.request(
                                                ResultRequest::SetAutoRefreshStopOnError(
                                                    !stop_on_error,
                                                ),
                                                app,
                                            );
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(
                                                crate::auto_refresh::AUTO_REFRESH_CONFIG_STOP_ON_ERROR_LABEL,
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_xs()
                                            .text_color(theme.colors.muted_foreground)
                                            .child(
                                                crate::auto_refresh::AUTO_REFRESH_CONFIG_STOP_ON_ERROR_HINT,
                                            ),
                                    ),
                            ),
                    )
                    // 页脚：本仓事实（间隔判定口径）+ 取消 / 确定
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_3()
                            .pb_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(SharedString::from(format!(
                                        "确定后按新间隔开启自动刷新；当前间隔 {}。",
                                        if current_interval > 0 {
                                            format!("{current_interval} 秒")
                                        } else {
                                            "未配置".to_string()
                                        }
                                    ))),
                            )
                            .child(
                                Button::new("editor-result-auto-refresh-config-cancel")
                                    .ghost()
                                    .small()
                                    .label(tr("取消"))
                                    .debug_selector(|| {
                                        "editor-result-auto-refresh-config-cancel".to_string()
                                    })
                                    .on_click(move |_, _window, app| {
                                        close_queue2.request(
                                            ResultRequest::CloseAutoRefreshConfig,
                                            app,
                                        );
                                    }),
                            )
                            .child(
                                Button::new("editor-result-auto-refresh-config-apply")
                                    .primary()
                                    .small()
                                    .label(tr("确定"))
                                    .debug_selector(|| {
                                        "editor-result-auto-refresh-config-apply".to_string()
                                    })
                                    .on_click(move |_, _window, app| {
                                        apply_queue
                                            .request(ResultRequest::ApplyAutoRefreshSettings, app);
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 【复制列名】「选项」对话框（真值 `ResultSetHandlerCopySpecial.CopyConfigDialog`，
    /// 实例化于 `ResultSetHandlerMain.java:352`，dialogId = `CopyGridNamesOptionsDialog`）
    ///
    /// 形态按截图 1:1：标题「选项」+ 三行（列分隔符 / 行分隔符 / 引号字符，均下拉，
    /// 下拉里显示转义字面量 `\t` / `\n`）+ 提示行
    /// 「将来在此设置下可以使用快捷键：Ctrl+Alt+Shift+C」+ 取消 / 确定。
    ///
    /// 本仓差异（登记 `result-panel.md` §9.73.3）：
    /// - 面板内 absolute 遮罩，不是窗口级 `BaseDialog`（面板 render 拿不到 `&mut Window`）；
    /// - 下拉是组件 `DropdownButton`（左半显当前值、右半 caret 开候选），真值是可编辑 `Combo`；
    /// - 没有「重复上次复制特殊」命令，提示行按真值文案固定显示（不随快捷键解析）。
    pub(crate) fn render_copy_names_options(&self, cx: &App) -> AnyElement {
        let theme = cx.theme();
        let close_queue = self.queue.clone();
        let close_queue2 = self.queue.clone();
        let apply_queue = self.queue.clone();
        let count = self
            .copy_names_dialog
            .as_ref()
            .map(|dialog| dialog.names.len())
            .unwrap_or(0);
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.colors.overlay)
            .debug_selector(|| "editor-result-copy-names".to_string())
            // `ESC` 收起同样由面板根的 `DismissOverlay` 收口（= 取消，不写剪贴板）
            .child(
                div()
                    .relative()
                    .flex_col()
                    .w(px(420.))
                    .rounded_md()
                    .border_1()
                    .border_color(theme.colors.border)
                    .bg(theme.colors.background)
                    .overflow_hidden()
                    .debug_selector(|| "editor-result-copy-names-card".to_string())
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
                                    .text_sm()
                                    .child(crate::copy_names::COPY_NAMES_OPTIONS_TITLE),
                            )
                            .child(
                                Button::new("editor-result-copy-names-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .tooltip(tr("关闭").to_string())
                                    .on_click(move |_, _window, app| {
                                        close_queue.request(
                                            ResultRequest::CloseCopyNamesOptions,
                                            app,
                                        );
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .m_3()
                            .child(self.render_copy_names_row(
                                "column-delimiter",
                                crate::copy_names::COPY_NAMES_COLUMN_DELIMITER_LABEL,
                                crate::copy_names::CopyNamesField::ColumnDelimiter,
                                &crate::copy_names::COLUMN_DELIMITER_CANDIDATES,
                            ))
                            .child(self.render_copy_names_row(
                                "row-delimiter",
                                crate::copy_names::COPY_NAMES_ROW_DELIMITER_LABEL,
                                crate::copy_names::CopyNamesField::RowDelimiter,
                                &crate::copy_names::ROW_DELIMITER_CANDIDATES,
                            ))
                            .child(self.render_copy_names_row(
                                "quote",
                                crate::copy_names::COPY_NAMES_QUOTE_LABEL,
                                crate::copy_names::CopyNamesField::QuoteString,
                                &crate::copy_names::QUOTE_CANDIDATES,
                            ))
                            // 提示行：真值 `copy_special_hint_for_hotkey`（截图同一行文案）
                            .child(
                                div()
                                    .pt_1()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(crate::copy_names::COPY_NAMES_HOTKEY_HINT),
                            ),
                    )
                    .child(
                        div()
                            .h_flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px_3()
                            .pb_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(SharedString::from(format!(
                                        "将复制 {count} 个列名（{})",
                                        crate::copy_names::escape_display_string(
                                            self.copy_names_dialog
                                                .as_ref()
                                                .map(|dialog| dialog
                                                    .settings
                                                    .column_delimiter
                                                    .as_str())
                                                .unwrap_or("\t")
                                        )
                                    ))),
                            )
                            .child(
                                Button::new("editor-result-copy-names-cancel")
                                    .ghost()
                                    .small()
                                    .label(tr("取消"))
                                    .on_click(move |_, _window, app| {
                                        close_queue2.request(
                                            ResultRequest::CloseCopyNamesOptions,
                                            app,
                                        );
                                    }),
                            )
                            .child(
                                Button::new("editor-result-copy-names-apply")
                                    .primary()
                                    .small()
                                    .label(tr("确定"))
                                    .on_click(move |_, _window, app| {
                                        apply_queue
                                            .request(ResultRequest::ApplyCopyNamesOptions, app);
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// 「选项」对话框的一行：label + 下拉（左半显当前值、右半 caret 开候选）。
    ///
    /// 显示值走 [`crate::copy_names::escape_display_string`]：`\t` 显示成 `\t` 两个字面字符
    /// （真值 `UIWidgets.createDelimiterCombo` 同口径，截图里就是这样）。
    fn render_copy_names_row(
        &self,
        id: &'static str,
        label: &'static str,
        field: crate::copy_names::CopyNamesField,
        candidates: &'static [&'static str],
    ) -> AnyElement {
        let current = self
            .copy_names_dialog
            .as_ref()
            .map(|dialog| field.value(&dialog.settings).to_string())
            .unwrap_or_default();
        let shown = crate::copy_names::escape_display_string(&current);
        let queue = self.queue.clone();
        div()
            .h_flex()
            .items_center()
            .gap_2()
            .child(div().w(rems(6.)).text_sm().child(label))
            .child(
                div()
                    .w(rems(9.))
                    .debug_selector(move || format!("editor-result-copy-names-{id}"))
                    .child(
                        DropdownButton::new(ElementId::Name(SharedString::from(format!(
                            "editor-result-copy-names-dd-{id}"
                        ))))
                        .small()
                        .button(
                            Button::new(ElementId::Name(SharedString::from(format!(
                                "editor-result-copy-names-value-{id}"
                            ))))
                            .label(shown)
                            .ghost()
                            .small(),
                        )
                        .dropdown_menu(move |mut menu, _window, _cx| {
                            for candidate in candidates.iter() {
                                let queue = queue.clone();
                                // 菜单项显示转义字面量（`\t`），值也按**显示串**回传：
                                // 真值 Combo 存的就是显示串，`okPressed` 才
                                // `unescapeDisplayString` 还原成控制字符（`:178-180`）
                                let shown = crate::copy_names::escape_display_string(candidate);
                                let value = shown.clone();
                                menu = menu.item(
                                    PopupMenuItem::new(shown).on_click(move |_, _window, app| {
                                        queue.request(
                                            ResultRequest::SetCopyNamesField {
                                                field,
                                                value: value.clone(),
                                            },
                                            app,
                                        );
                                    }),
                                );
                            }
                            menu
                        }),
                    ),
            )
            .child(div().flex_1())
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
            // 【第 81 批】`ESC` 收起统一由面板根的 `DismissOverlay` 收口（原 `on_key_down` 已删）
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
                            .child(div().text_sm().child(tr("筛选设置")))
                            .child(
                                Button::new("editor-result-filter-settings-close")
                                    .ghost()
                                    .small()
                                    .icon(IconName::X)
                                    .debug_selector(|| {
                                        "editor-result-filter-settings-close".to_string()
                                    })
                                    .tooltip(tr("关闭").to_string())
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
                                            Tab::new().label(tr("列约束")).on_click(
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
                                            Tab::new().label(tr("自定义条件")).on_click(
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
                                    .label(tr("取消"))
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
                                    .label(tr("应用"))
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
                    .tooltip(tr("按列名排序（A→Z）").to_string())
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
                    .tooltip(tr("全部显示").to_string())
                    .debug_selector(|| "editor-result-filter-settings-show-all".to_string())
                    .on_click(move |_, _window, app| {
                        show_all_queue.request(ResultRequest::ColumnsShowAll, app);
                    })
                    .into_any_element(),
                Button::new("editor-result-filter-settings-hide-all")
                    .ghost()
                    .xsmall()
                    .icon(IconName::EyeOff)
                    .tooltip(tr("全部不显示（至少保留一列可见）").to_string())
                    .debug_selector(|| "editor-result-filter-settings-hide-all".to_string())
                    .on_click(move |_, _window, app| {
                        hide_all_queue.request(ResultRequest::ColumnsHideAll, app);
                    })
                    .into_any_element(),
                Button::new("editor-result-filter-settings-reset")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RotateCcw)
                    .tooltip(tr("重置列布局（列序 + 显示全部 + 取消全部冻结）").to_string())
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
                    // `#`：列位置（1 起）。**紧跟在列名之后**，与 DBeaver
                    // `FilterSettingsDialog.addColumn("#"):139-146` 的列序一致
                    .child(
                        div()
                            .w(rems(2.))
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .debug_selector({
                                let index = index;
                                move || format!("editor-result-filter-settings-pos-{index}")
                            })
                            .child(format!("{}", index + 1)),
                    )
                    // 勾选框各包一层带 id 的 div：`Checkbox` 是 `RenderOnce`，
                    // 自身 id 进不了 `debug_bounds`（项目规范里的同类做法）
                    .child(
                        div()
                            .debug_selector({
                                let index = index;
                                move || format!("editor-result-filter-settings-vis-cell-{index}")
                            })
                            .child(vis_box),
                    )
                    .child(
                        div()
                            .debug_selector({
                                let index = index;
                                move || format!("editor-result-filter-settings-pin-cell-{index}")
                            })
                            .child(pin_box),
                    )
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
                    ),
            );
        }

        div()
            .v_flex()
            .gap_1()
            .px_3()
            .pt_3()
            .child(toolbar)
            // 表头：与行同布局（列名 / # / 可见 / 冻结 / 条件 / 排序）。
            // 列序照 DBeaver `FilterSettingsDialog.addColumn`：Name → `#`（`:139-146`）
            // → Visible（`:148-176`）→ Pinned（`:178-211`）→ Order（`:213+`）；
            // 「条件 / 排序」两列是本仓扩展（列头菜单里设置的东西在这里做只读投影）。
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .child(div().flex_1().min_w_0().child(header("列")))
                    .child(div().w(rems(2.)).child(header("#")))
                    .child(div().child(header("可见")))
                    .child(div().child(header("冻结")))
                    .child(div().w(criteria_width).child(header("条件")))
                    .child(div().w(sort_width).text_center().child(header("排序"))),
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
                            .child(tr("WHERE 条件（在筛选框里编辑）")),
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
                            .child(tr("自定义排序（留空 = 沿用原查询的 ORDER BY）")),
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
