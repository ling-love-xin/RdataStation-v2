//! 同名冲突三选对话框（§13.3 升级 / §13.6 分发）：覆盖 / 改名 / 取消。
//!
//! 升级（项目 → 全局）与分发（全局 → 项目）在目标实例撞上同名资源时，服务层返回
//! `InstanceOpError::Conflict`；工作线程把凭据回传事件路径，本对话框据此让用户在
//! **同一次决策**里选一条路：
//!
//! - **覆盖**：旧本体移除、旧登记软删，新内容接续版本链（动作不可撤销，按钮走 Danger）；
//! - **改名**：输入一个新名字，按新名重新提交（不碰旧内容）；
//! - **取消**：不提交，只关窗。
//!
//! 纪律与其余对话框一致：**输入实体在开窗前建好**（builder 是 `Fn`）、**校验从输入值推导**
//! （每帧重算，改回去提示自然消失）、执行一律回宿主——crate 不认识服务层与连接池。

use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Sizable as _, WindowExt as _};
use gpui_kit::*;

use super::archive::name_hint;
use crate::ui;

/// 冲突发生在哪条链路上：决定标题动词、描述里的持有方与覆盖提示口径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// 项目 → 全局：全局实例已有同名表 / 资源。
    Promote,
    /// 全局 → 项目：当前项目已有同名表 / 文件。
    Distribute,
}

/// 开窗所需的全部信息（宿主在事件路径从冲突回执里取好）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSeed {
    pub kind: ConflictKind,
    /// 冲突名（目标实例里已存在的那个名字，展示与「与原名相同」校验都用它）。
    pub name: String,
}

/// 用户选择（执行一律回宿主；宿主按类型重新入队覆盖 / 改名任务）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictChoice {
    /// 覆盖旧内容。
    Overwrite,
    /// 改用新名（已 trim、已过改名闸门）。
    Rename(String),
}

/// 改名提交闸门（与 `rename::RenameGate` 同构；只复用，不依赖重命名对话框）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictRenameGate {
    /// 可以提交改名。
    Ready,
    /// 名字和原来一样：冲突依旧存在，不给提交。
    Unchanged,
    /// 挡住（空名等）：带一句为什么。
    Blocked(&'static str),
}

/// 判定改名能不能提交（空名走 `name_hint`；与原名相同 = 冲突依旧，挡下）。
pub fn conflict_gate(current: &str, value: &str) -> ConflictRenameGate {
    let trimmed = value.trim();
    if let Some(hint) = name_hint(trimmed) {
        return ConflictRenameGate::Blocked(hint);
    }
    if trimmed == current.trim() {
        return ConflictRenameGate::Unchanged;
    }
    ConflictRenameGate::Ready
}

impl ConflictKind {
    /// 动作动词（回执与描述共用）。
    fn verb(&self) -> &'static str {
        match self {
            ConflictKind::Promote => "升级到全局",
            ConflictKind::Distribute => "从全局分发",
        }
    }

    /// 冲突持有方（描述里说清"是谁已经有同名"）。
    fn holder(&self) -> &'static str {
        match self {
            ConflictKind::Promote => "全局实例",
            ConflictKind::Distribute => "当前项目",
        }
    }

    /// 覆盖提示（旧内容会被怎么处理，两条链路口径不同）。
    fn overwrite_note(&self) -> &'static str {
        match self {
            ConflictKind::Promote => "覆盖将删除全局旧表并软删旧登记，新内容接续版本链；旧表内容不可恢复。",
            ConflictKind::Distribute => "覆盖将删除项目旧表 / 旧文件并软删旧登记，新内容接续项目版本链；旧本体不可恢复。",
        }
    }
}

/// 打开同名冲突三选对话框。
///
/// `name_input` 由调用方在开窗前建好并预填冲突名；`on_choice` 只在用户选定
/// 「覆盖」或合法「改名」时回调一次，取消不回调（关窗即可）。
pub fn open_conflict_dialog(
    window: &mut Window,
    cx: &mut App,
    seed: ConflictSeed,
    name_input: Entity<InputState>,
    on_choice: impl Fn(ConflictChoice, &mut Window, &mut App) + 'static,
    on_close: impl Fn(&mut App) + 'static,
) {
    let on_choice: Rc<dyn Fn(ConflictChoice, &mut Window, &mut App)> = Rc::new(on_choice);
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        // 探针字段：在闭包外备好（on_click 闭包需 'static，不能借用局部 seed）。
        let seed_kind = seed.kind;
        let seed_name = seed.name.clone();
        let value = name_input.read(cx).value().trim().to_string();
        let gate = conflict_gate(&seed.name, &value);
        let (hint, hint_color, can_rename) = match gate {
            ConflictRenameGate::Ready => (
                format!("将以「{value}」{verb}，不覆盖现有内容", verb = seed.kind.verb()),
                theme.colors.muted_foreground,
                true,
            ),
            ConflictRenameGate::Unchanged => ("名字与现有资源相同，请换一个新名称".to_string(), theme.colors.warning, false),
            ConflictRenameGate::Blocked(hint) => (hint.to_string(), theme.colors.danger, false),
        };

        // 覆盖闭包：不碰输入框，直接回宿主重入队覆盖任务。
        let overwrite = {
            let on_choice = on_choice.clone();
            let kind = seed_kind;
            let name = seed_name.clone();
            move |window: &mut Window, cx: &mut App| {
                tracing::info!(kind = ?kind, name = %name, "【探针】冲突对话框「覆盖」按钮触发");
                on_choice(ConflictChoice::Overwrite, window, cx);
            }
        };
        // 改名闭包：过闸后回宿主重入队改名任务；不过闸不关窗（内联提示已出现）。
        let rename = {
            let on_choice = on_choice.clone();
            let input = name_input.clone();
            let current = seed.name.clone();
            let kind = seed_kind;
            move |window: &mut Window, cx: &mut App| {
                let name = input.read(cx).value().trim().to_string();
                if !matches!(conflict_gate(&current, &name), ConflictRenameGate::Ready) {
                    return false;
                }
                tracing::info!(kind = ?kind, old = %current, new = %name, "【探针】冲突对话框「改名」按钮触发");
                on_choice(ConflictChoice::Rename(name), window, cx);
                true
            }
        };
        let rename_for_ok = rename.clone();

        let intro = format!(
            "{}已有同名资源「{}」。选择处理方式：覆盖旧内容、改用新名称，或取消本次{}。",
            seed.kind.holder(),
            seed.name,
            seed.kind.verb(),
        );

        dialog
            .title("同名冲突")
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
                            .text_color(theme.colors.warning)
                            .child(seed.kind.overwrite_note()),
                    )
                    .v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("改用新名称"),
                    )
                    .child(Input::new(&name_input))
                    .child(div().w_full().text_xs().text_color(hint_color).child(hint)),
            )
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("conflict-dialog-cancel")
                            .with_variant(ButtonVariant::Secondary)
                            .small()
                            .debug_selector(|| "conflict-dialog-cancel".to_string())
                            .label("取消")
                            .on_click({
                                let kind = seed_kind;
                                let name = seed_name.clone();
                                move |_, window, cx| {
                                    tracing::info!(kind = ?kind, name = %name, "【探针】冲突对话框「取消」按钮触发");
                                    window.close_dialog(cx);
                                }
                            }),
                    )
                    .child(
                        Button::new("conflict-dialog-overwrite")
                            .with_variant(ButtonVariant::Danger)
                            .small()
                            .debug_selector(|| "conflict-dialog-overwrite".to_string())
                            .label("覆盖")
                            .on_click(move |_, window, cx| {
                                overwrite(window, cx);
                                window.close_dialog(cx);
                            }),
                    )
                    .child(
                        Button::new("conflict-dialog-rename")
                            .with_variant(ButtonVariant::Primary)
                            .small()
                            .disabled(!can_rename)
                            .debug_selector(|| "conflict-dialog-rename".to_string())
                            .label("改名")
                            .on_click({
                                let rename = rename.clone();
                                move |_, window, cx| {
                                    if !rename(window, cx) {
                                        return;
                                    }
                                    window.close_dialog(cx);
                                }
                            }),
                    ),
            )
            // Enter = 改名（输入框聚焦时的主路径）；Esc = 取消。
            .on_ok(move |_, window, cx| {
                if rename_for_ok(window, cx) {
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
    // 安全模式：测试模块不通配导入（会与 `gpui_kit` 的 `test` 宏展开自相残杀）。
    use super::{ConflictRenameGate, conflict_gate};

    /// 改名闸门：空名挡下、与原名相同挡下、新名放行。
    #[test]
    fn rename_gate_blocks_blank_and_unchanged() {
        assert_eq!(
            conflict_gate("月报", "  "),
            ConflictRenameGate::Blocked("显示名不能为空")
        );
        assert_eq!(conflict_gate("月报", "月报"), ConflictRenameGate::Unchanged);
        assert_eq!(conflict_gate("月报", "  月报 "), ConflictRenameGate::Unchanged);
        assert_eq!(conflict_gate("月报", "季度月报"), ConflictRenameGate::Ready);
    }
}
