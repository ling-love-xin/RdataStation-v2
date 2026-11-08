//! 参数绑定表单（B16：`实名` → 弹参数 → 字面替换后执行）。

use super::*;

/// 【B16】参数绑定表单（DBeaver 式 `:name` → 弹参数 → 字面替换后执行）
///
/// 输入实体在**开窗前**建好（manage_env 的纪律：`open_dialog` 的 builder 是一次性的，
/// 而 `InputState` 要在有 `window` 的上下文里 `cx.new`）。提交回调把
/// 「参数名 → 用户输入」整包交给宿主替换并继续执行；取消回调清掉宿主的挂起状态
/// （弹层默认 Esc / 点遮罩也会关，但挂起状态只有显式取消才清）。
pub fn open_bind_dialog(
    window: &mut Window,
    cx: &mut App,
    names: Vec<String>,
    on_run: impl Fn(HashMap<String, String>, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut App) + 'static,
) {
    use gpui_kit::component::input::{Input, InputState};

    let inputs: Vec<(SharedString, Entity<InputState>)> = names
        .into_iter()
        .map(|name| {
            let input = cx.new(|cx| InputState::new(window, cx));
            (SharedString::from(name), input)
        })
        .collect();
    let on_run = Rc::new(on_run);
    let on_cancel = Rc::new(on_cancel);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;

        // 每个参数一行：名字（灰）+ 输入框（测试可观测：`editor-bind-input-{名}`）
        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .children(inputs.iter().map(|(name, input)| {
                let name_for_selector = name.to_string();
                div()
                    .v_flex()
                    .gap_1()
                    .child(div().text_xs().text_color(muted).child(name.clone()))
                    .child(
                        // Input 是 RenderOnce（无 id 可观测），套一层带 selector 的 div
                        div()
                            .debug_selector(move || {
                                format!("editor-bind-input-{name_for_selector}")
                            })
                            .child(Input::new(input)),
                    )
            }));

        // 执行：收集输入 → 关弹层 → 交回宿主（替换后继续原提交链）
        let run = Button::new("editor-bind-run")
            .with_variant(ButtonVariant::Primary)
            .small()
            .label("执行")
            .debug_selector(|| "editor-bind-run".to_string())
            .on_click({
                let on_run = on_run.clone();
                let inputs = inputs.clone();
                move |_, window, cx| {
                    let values = inputs
                        .iter()
                        .map(|(name, input)| (name.to_string(), input.read(cx).value().to_string()))
                        .collect();
                    window.close_dialog(cx);
                    on_run(values, window, cx);
                }
            });
        // 取消：关弹层 + 清宿主挂起（动作是「这次不跑了」，不是只关个窗）
        let cancel = Button::new("editor-bind-cancel")
            .with_variant(ButtonVariant::Ghost)
            .small()
            .label("取消")
            .debug_selector(|| "editor-bind-cancel".to_string())
            .on_click({
                let on_cancel = on_cancel.clone();
                move |_, window, cx| {
                    window.close_dialog(cx);
                    on_cancel(cx);
                }
            });

        dialog
            .title("参数")
            .child(body)
            .footer(DialogFooter::new().child(cancel).child(run))
    });
}
