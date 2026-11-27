//! 发布基准过期对话框：只能**分叉为新资产**或取消，绝不静默覆盖全局 head。
//!
//! 触发口：全局表 / 数据文件发布带 `base_global_version`，但全局 head 已前进
//! （或首次发布时别处已经抢先建了同名资产）。服务层回 `StaleBase`，事件路径把
//! 父资产名与当前 head 交到这里；用户填新名后以父 head **零复制**分叉出 v1。

use shared::i18n::tr;
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::*;

use super::archive::name_hint;
use crate::ui;

/// 开窗信息：父资产名、发布者看到的 base（None = 首次发布）、全局当前 head。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleBaseSeed {
    pub name: String,
    pub base: Option<i32>,
    pub head: i32,
}

/// 分叉名闸门：空名 / 与父同名都挡下（同名会立刻再撞登记层）。
fn fork_gate(parent: &str, value: &str) -> Result<(), &'static str> {
    let trimmed = value.trim();
    if let Some(hint) = name_hint(trimmed) {
        return Err(hint);
    }
    if trimmed == parent.trim() {
        return Err("与父资产同名，分叉会立刻再撞登记层");
    }
    Ok(())
}

/// 打开「发布基准过期」对话框。
pub fn open_stale_base_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: StaleBaseSeed,
    name_input: Entity<InputState>,
    on_fork: impl Fn(String, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    let on_fork: Rc<dyn Fn(String, &mut Window, &mut App)> = Rc::new(on_fork);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let value = name_input.read(cx).value().trim().to_string();
        let gate = fork_gate(&seed.name, &value);
        let (hint, hint_color, can_fork) = match gate {
            Ok(()) => (
                format!("将创建独立新资产「{value}」，v1 零复制引用当前 head"),
                theme.colors.muted_foreground,
                true,
            ),
            Err(hint) => (hint.to_string(), theme.colors.danger, false),
        };

        let fork = {
            let on_fork = on_fork.clone();
            let input = name_input.clone();
            let parent = seed.name.clone();
            move |window: &mut Window, cx: &mut App| -> bool {
                let name = input.read(cx).value().trim().to_string();
                if fork_gate(&parent, &name).is_err() {
                    return false;
                }
                on_fork(name, window, cx);
                true
            }
        };
        let fork_for_ok = fork.clone();

        let base_text = match seed.base {
            Some(base) => format!("v{base}"),
            None => tr("首次发布").to_string(),
        };
        let intro = format!(
            "全局「{}」当前已是 v{}，本次发布基准是 {base_text}，不能直接追加。\n可把本次内容分叉成一条独立新资产，父资产 head 作为新资产 v1 零复制起点。",
            seed.name, seed.head,
        );

        dialog
            .title(tr("发布基准已过期"))
            .w(cx.theme().font_size * ui::CONFLICT_DIALOG_WIDTH)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        div()
                            .w_full()
                            .text_sm()
                            .text_color(theme.colors.foreground)
                            .child(intro),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(tr("新资产名称")),
                    )
                    .child(Input::new(&name_input))
                    .child(div().w_full().text_xs().text_color(hint_color).child(hint)),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("stale-base-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "stale-base-cancel".to_string())
                            .label(tr("取消"))
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("stale-base-fork")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .disabled(!can_fork)
                            .debug_selector(|| "stale-base-fork".to_string())
                            .label(tr("分叉为新资产"))
                            .on_click({
                                let fork = fork.clone();
                                move |_, window, cx| {
                                    if !fork(window, cx) {
                                        return;
                                    }
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
            .on_ok(move |_, window, cx| {
                if fork_for_ok(window, cx) {
                    window.close_dialog(cx);
                }
                true
            })
            .on_close({
                let on_close = on_close.clone();
                move |_, _window, cx| on_close(cx)
            })
            .on_cancel(|_, _, _| true)
    });
}

#[cfg(test)]
mod tests {
    use super::fork_gate;

    #[test]
    fn fork_gate_blocks_blank_and_parent_name() {
        assert_eq!(fork_gate("月报", "  "), Err("显示名不能为空"));
        assert_eq!(
            fork_gate("月报", "月报"),
            Err("与父资产同名，分叉会立刻再撞登记层")
        );
        assert!(fork_gate("月报", "月报-分支").is_ok());
    }
}
