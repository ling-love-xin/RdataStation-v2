//! 结果区的脚本输出卡片（2b 通用模式）
//!
//! 脚本执行的结果**不是网格形状**（stdout 是文本），所以不走结果集表格，而是落一张卡片：
//! 成功 = 环境名 + stdout（等宽字）+ stderr 附注 + 退出码 / 耗时；失败 = 环境名 + 失败原因
//! （含 spawn 失败 / 超时中断）。为什么用卡片而不是普通文本行：脚本是一次性的、有边界的
//! 结论（跑完就定格），卡片把「这次跑的结果」钉成一格，与结果区的错误卡片同构。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::alert::Alert;
use gpui_kit::*;

/// 卡片内容（都是真实值）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptCard {
    /// 环境展示名（宿主经 EnvPort 解析；环境已不在列表就回退用 id）
    pub env_name: String,
    /// 成功输出（`Ok` 才有；`Err` = 失败原因）
    pub result: Result<ScriptOutcomeView, String>,
}

/// 一次成功执行的展示值
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptOutcomeView {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    /// 是否因超时被中断
    pub timed_out: bool,
    /// 执行耗时（毫秒）
    pub elapsed_ms: u64,
}

/// 画一张脚本输出卡片
pub fn render(card: &ScriptCard, cx: &App) -> impl IntoElement {
    let muted = cx.theme().colors.muted_foreground;
    let mono_family = cx.theme().mono_font_family.clone();

    div()
        .v_flex()
        .gap_1()
        .p_2()
        // 测试按选择器断言“脚本跑完有卡片”
        .debug_selector(|| "editor-result-script-card".to_string())
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(muted)
                .child(format!("脚本 · {}", card.env_name)),
        )
        .child(match &card.result {
            Ok(outcome) => {
                // 成功：stdout 正文（等宽、可读）；stderr 有内容就附注；退出码 / 超时也如实说
                let mut body = div().v_flex().gap_1();
                body = body.child(
                    div()
                        .bg(cx.theme().colors.background)
                        .p_2()
                        .rounded_md()
                        .font_family(mono_family)
                        .whitespace_normal()
                        .overflow_hidden()
                        .text_color(muted)
                        .child(if outcome.stdout.is_empty() {
                            "(无输出)".to_string()
                        } else {
                            outcome.stdout.clone()
                        }),
                );
                if !outcome.stderr.is_empty() {
                    body = body.child(
                        Alert::warning(
                            "editor-result-script-stderr",
                            SharedString::from(outcome.stderr.clone()),
                        )
                        .small(),
                    );
                }
                let mut meta = format!("退出码 {}", outcome.exit_code.unwrap_or(-1));
                if outcome.timed_out {
                    meta.push_str(" · 超时已中断");
                }
                meta.push_str(&format!(" · {} ms", outcome.elapsed_ms));
                body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .text_xs()
                        .text_color(muted)
                        .child(meta),
                )
            }
            Err(reason) => {
                // 失败：Alert danger（与错误卡片同构）
                div().child(
                    Alert::error(
                        "editor-result-script-error",
                        SharedString::from(reason.clone()),
                    )
                    .small(),
                )
            }
        })
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{ScriptCard, ScriptOutcomeView};

    /// 成功卡片的展示值是真实值（stdout / 退出码 / 耗时）
    #[test]
    fn success_card_keeps_the_real_outcome() {
        let card = ScriptCard {
            env_name: "Python 3.12".to_string(),
            result: Ok(ScriptOutcomeView {
                stdout: "hi\n".to_string(),
                stderr: String::new(),
                exit_code: Some(0),
                timed_out: false,
                elapsed_ms: 42,
            }),
        };
        let outcome = card.result.as_ref().expect("成功");
        assert_eq!(outcome.stdout, "hi\n");
        assert_eq!(outcome.exit_code, Some(0));
        assert_eq!(outcome.elapsed_ms, 42);
        assert!(!outcome.timed_out);
    }

    /// 失败卡片把失败原因留下（spawn 失败 / 超时都会走到这里）
    #[test]
    fn failure_card_keeps_the_reason() {
        let card = ScriptCard {
            env_name: "Python".to_string(),
            result: Err("解释器未找到".to_string()),
        };
        assert!(card.result.is_err());
    }
}
