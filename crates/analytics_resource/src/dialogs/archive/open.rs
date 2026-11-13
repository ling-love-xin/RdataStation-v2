//! 归档对话框：打开入口与表单装配。

use super::*;
use super::fields::{field_label, group_picker, hint_line, mode_picker, readonly_block};
use gpui_kit::component::scroll::ScrollableElement as _;

/// 打开归档确认对话框（宿主入口）。
///
/// `on_submit` 只在**校验通过**时回调一次，负责构造 `ArchiveRequest` 并落库（对话框不碰存储）。
pub fn open_archive_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: ArchiveDialogSeed,
    on_submit: impl Fn(ArchiveDialogResult, &mut App) + 'static,
) {
    let inputs = build_inputs(window, cx, &seed);
    open_archive_dialog_with(window, cx, seed, inputs, on_submit);
}

/// 同上，但用调用方建好的输入实体（窗口测试从这条缝进来，复核"同一批实体 + 真渲染"）。
pub fn open_archive_dialog_with(
    window: &mut Window,
    cx: &mut App,
    seed: ArchiveDialogSeed,
    inputs: ArchiveDialogInputs,
    on_submit: impl Fn(ArchiveDialogResult, &mut App) + 'static,
) {
    let ArchiveDialogInputs {
        name,
        alias,
        tags,
        keep_versions,
        group_id,
        mode,
        assertion_metric,
        assertion_definition,
        assertion_window,
        assertion_conclusion,
    } = inputs;
    let on_submit = Rc::new(on_submit);
    // 分组名单只在开窗时取一份：分组是单层的，一份列表就是这个对话框的生命周期里真实可选项。
    let groups = seed.groups.clone();

    // 按钮文案把"会发生什么"写进动作里：有冲突时不是"归档"而是"改名归档"。
    let ok_text = if seed.conflict.is_some() {
        "改名归档"
    } else {
        "归档"
    };

    let submit: Rc<dyn Fn(&mut App) -> bool> = {
        let name = name.clone();
        let alias = alias.clone();
        let tags = tags.clone();
        let keep_versions = keep_versions.clone();
        let group_id = group_id.clone();
        let mode = mode.clone();
        let assertion_metric = assertion_metric.clone();
        let assertion_definition = assertion_definition.clone();
        let assertion_window = assertion_window.clone();
        let assertion_conclusion = assertion_conclusion.clone();
        let on_submit = on_submit.clone();
        Rc::new(move |cx: &mut App| -> bool {
            let inputs = ArchiveDialogInputs {
                name: name.clone(),
                alias: alias.clone(),
                tags: tags.clone(),
                keep_versions: keep_versions.clone(),
                group_id: group_id.clone(),
                mode: mode.clone(),
                assertion_metric: assertion_metric.clone(),
                assertion_definition: assertion_definition.clone(),
                assertion_window: assertion_window.clone(),
                assertion_conclusion: assertion_conclusion.clone(),
            };
            let Some(result) = submit_archive(&inputs, cx) else {
                return false;
            };
            tracing::info!(
                name = %result.name,
                mode = ?result.payload_mode,
                group_id = ?result.group_id,
                tags = result.tags.len(),
                "【探针】归档对话框「归档」提交（校验通过）"
            );
            on_submit(result, cx);
            true
        })
    };

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        // 提示每帧从当前值推导：用户改回去，提示自然消失（无标志位可残留）。
        let name_hint = name_hint(&name.read(cx).value());
        let keep = parse_keep_versions(&keep_versions.read(cx).value());
        let keep_hint = keep.err();
        // 写了 `-1` 时把它的含义摆出来：这是个哨兵值，不说明就等于让用户猜。
        let keep_note = match keep {
            Ok(Some(KeepVersions::All)) => Some("全部保留：不裁剪任何历史内容副本".to_string()),
            Ok(Some(keep)) => Some(format!("本次归档：{}", keep.label())),
            _ => None,
        };
        // builder 是 `Fn`（每帧重建）：两条提交路径各拿一份**本帧的**副本——
        // 内层 `move` 闭包只能拿走副本，外层不能再持有它们（否则 builder 就不是 `Fn` 了）。
        let submit_for_button = submit.clone();
        let submit_for_ok = submit.clone();

        let form = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(field_label(theme, "显示名"))
            .child(Input::new(&name))
            .when_some(name_hint, |this, hint| {
                this.child(hint_line(hint, theme.colors.danger))
            })
            .child(field_label(theme, "别名"))
            .child(Input::new(&alias))
            .child(field_label(theme, "分组"))
            .child(group_picker(&group_id, &groups))
            .when(seed.allow_reference, |this| {
                this.child(field_label(theme, "归档方式"))
                    .child(mode_picker(theme, &mode))
                    .child(hint_line(
                        mode.borrow().note(),
                        theme.colors.muted_foreground,
                    ))
            })
            .when(!seed.allow_reference, |this| {
                this.child(field_label(theme, "归档方式")).child(hint_line(
                    "分析快照固定为复制存档",
                    theme.colors.muted_foreground,
                ))
            })
            .child(field_label(theme, "标签"))
            .child(Input::new(&tags))
            .child(field_label(theme, "保留历史内容"))
            .child(Input::new(&keep_versions))
            .when_some(keep_hint, |this, hint| {
                this.child(hint_line(hint, theme.colors.danger))
            })
            .when_some(keep_note, |this, note| {
                this.child(hint_line(&note, theme.colors.muted_foreground))
            })
            // 业务断言（选填）：默认空、不打断归档；填了才进资产履历与语义检索。
            .child(
                div()
                    .h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .pt_1()
                    .border_t(ui::HAIRLINE)
                    .border_color(theme.colors.border)
                    .child(div().text_xs().text_color(theme.colors.foreground).child("业务断言（选填）")),
            )
            .child(hint_line("写清口径，半年后才知道这个数是什么意思；取回后再归档会保留上次口径", theme.colors.muted_foreground))
            .child(field_label(theme, "指标名"))
            .child(Input::new(&assertion_metric))
            .child(field_label(theme, "口径定义"))
            .child(Input::new(&assertion_definition))
            .child(field_label(theme, "业务时间窗"))
            .child(Input::new(&assertion_window))
            .child(field_label(theme, "结论摘要"))
            .child(Input::new(&assertion_conclusion))
            .max_h(rems(22.))
            .overflow_y_scrollbar();

        dialog
            .title("归档为存档")
            .w(cx.theme().font_size * ui::ARCHIVE_DIALOG_WIDTH)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .child(readonly_block(theme, &seed, &mode))
                    .when_some(seed.conflict.as_ref(), |this, conflict| {
                        this.child(hint_line(
                            &format!(
                                "{} 已被占用：本次将归档为 {}（不覆盖）",
                                conflict.taken_rel_path, conflict.resolved_rel_path
                            ),
                            theme.colors.warning,
                        ))
                    })
                    .child(form),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("archive-dialog-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "archive-dialog-cancel".to_string())
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("archive-dialog-ok")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .debug_selector(|| "archive-dialog-ok".to_string())
                            .label(ok_text)
                            .on_click(move |_, window, cx| {
                                // 校验不过：不关窗（内联提示已出现，让用户接着改）。
                                if !submit_for_button(cx) {
                                    return;
                                }
                                window.close_dialog(cx);
                            }),
                    ),
            )
            // Enter 确认 / Esc 取消（对话框 key context 自带绑定）。
            .on_ok(move |_, _window, cx| submit_for_ok(cx))
            .on_cancel(|_, _, _| true)
    });
}

