//! 自定义分析 SQL 输入框（B15 切片二）。

use super::*;

/// 自定义分析 SQL（B15 切片二）：预置菜单之外，自己写一条聚合 SQL 在结果集上跑
///
/// 与其它对话框不同，这个要**输入**，所以状态不是闭包里的 `Rc<Cell<_>>`，而是一个
/// `TextareaState` 实体：它自己管重绘、光标与键盘（这正是“局部状态”该有的形状）。
/// 初始值由调用方给——预填一条**能直接跑**的模板（`{table}` 占位符写在提示里）。
///
/// 回调拿到的就是框里的原文；“空不空”由落地入口判（见 `analysis::is_runnable`）。
pub fn open_analysis_sql(
    window: &mut Window,
    cx: &mut App,
    hint: impl Into<SharedString>,
    initial_sql: impl Into<SharedString>,
    on_run: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    let hint = hint.into();
    let state = cx.new(|cx| {
        TextareaState::new(window, cx)
            .placeholder(crate::analysis::CUSTOM_PLACEHOLDER)
            .default_value(initial_sql)
    });
    let on_run = Rc::new(on_run);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let muted = cx.theme().colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(div().text_xs().text_color(muted).child(hint.clone()))
            .child(
                div()
                    .w_full()
                    // 输入框本身不给 selector（它不是可交互 Div）；窗口测试按这个包装定位
                    .debug_selector(|| "editor-analysis-sql".to_string())
                    .child(Textarea::new(&state).h(rems(ui::ANALYSIS_SQL_HEIGHT))),
            );

        let cancel = action_button("editor-analysis-cancel", "取消", ButtonVariant::Ghost, {
            // 取消：对话框消失即结束（框里的字不保留——它只属于这一次分析）
            move |_window, _cx| {}
        });
        let run_state = state.clone();
        // 对话框 builder 是 `Fn`（会被重建），所以在这里再克隆一份 Rc 交给按钮
        let run_on_run = on_run.clone();
        let run = action_button(
            "editor-analysis-run",
            "运行分析",
            ButtonVariant::Primary,
            move |window, cx| {
                let sql = run_state.read(cx).value().to_string();
                run_on_run(sql, window, cx);
            },
        );

        dialog
            .title(crate::analysis::CUSTOM_TITLE)
            .child(body)
            .footer(DialogFooter::new().child(cancel).child(run))
    });
}
