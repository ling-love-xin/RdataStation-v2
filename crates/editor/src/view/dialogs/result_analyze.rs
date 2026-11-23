//! 结果集「分析」前的命名与去向（08 原型「分析这份结果集」落地）
//!
//! 口径（2026-11-16 拍板）：**点「分析」不等于开洞察**。这里只问两件事——叫什么、
//! 落到哪里——然后把对象交给数据对象面板；做不做 EDA 由用户在那里面按需决定。
//!
//! 两种去处是同一份结果集的两个去向，差别只在写不写盘：
//! - 临时：`tmp_q_<描述>_<时间戳>_<随机>`，随会话走（TTL / 关项目清场），只在项目侧；
//! - 持久：`res_<描述>_<时间戳>`，落本项目分析库，可回溯版本。
//!
//! **全局不在这里选**：去全局只有资产归档处「升级到全局」一条路（面板不给第二个入口）。
//!
//! 名字能进表名，但临时表名的尺子只留 ASCII（`temp_table::sanitize_description`），
//! 中文会被折成下划线——所以正文里必须把这条说白，不能让用户以为框里写的中文会原样成为表名。

use shared::i18n::tr;
use gpui_kit::component::input::{Input, InputState};

use super::*;

/// 物化后的去处
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultSetDestination {
    /// 会话内临时表（项目侧，不写盘）
    Temporary,
    /// 项目分析库的持久表（可回溯版本）
    Persistent,
}

/// 弹框前由面板算好的事实（都在事件路径上算，不在 render 里读结果集）
#[derive(Debug, Clone)]
pub struct ResultSetFacts {
    /// 结果集在界面上的名字（「结果 3 · 订单明细」）
    pub label: String,
    pub rows: usize,
    pub columns: usize,
}

/// 结果集「分析」对话框：名字 + 去向
///
/// 与 [`open_federated_confirm`] 同款：关窗**先于**回调（回调里还要物化、写库，
/// 别让两层叠着）。取消回调不做任何事——框里的名字只属于这一次。
pub fn open_result_analyze(
    window: &mut Window,
    cx: &mut App,
    facts: ResultSetFacts,
    on_run: impl Fn(String, ResultSetDestination, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&mut App) + 'static,
) {
    // 需要输入，所以状态是 InputState 实体（它自己管光标 / 键盘 / 重绘）
    let state = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(tr("给这份结果集起个名字"))
            .default_value(facts.label.clone())
    });
    let on_run = Rc::new(on_run);
    let on_cancel = Rc::new(on_cancel);
    let facts_for_body = facts.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;

        let summary = format!(
            "{} · {} 行 · {} 列",
            facts_for_body.label, facts_for_body.rows, facts_for_body.columns
        );
        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(div().text_sm().child(summary))
            .child(
                div()
                    .w_full()
                    .debug_selector(|| "editor-result-analyze-name".to_string())
                    .child(Input::new(&state)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("存为临时表：会话里可用，关项目即清，只能留在本项目。"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("存入项目库：落到本项目分析库，可回溯版本；要进全局请到资产归档处升级。"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("临时表名只保留 ASCII，中文会折成下划线；持久表名保留中文。"),
            );

        let cancel = action_button("editor-result-analyze-cancel", "取消", ButtonVariant::Ghost, {
            let on_cancel = on_cancel.clone();
            move |_window, cx| on_cancel(cx)
        });

        // 两个去处各一个按钮：都是「动作即提交」，不引入框内单选状态
        let temp_state = state.clone();
        let temp_run = on_run.clone();
        let temp = action_button(
            "editor-result-analyze-temp",
            "存为临时表",
            ButtonVariant::Primary,
            move |window, cx| {
                let name = temp_state.read(cx).value().to_string();
                temp_run(name, ResultSetDestination::Temporary, window, cx);
            },
        );

        let keep_state = state.clone();
        let keep_run = on_run.clone();
        let keep = action_button(
            "editor-result-analyze-persist",
            "存入项目库",
            ButtonVariant::Ghost,
            move |window, cx| {
                let name = keep_state.read(cx).value().to_string();
                keep_run(name, ResultSetDestination::Persistent, window, cx);
            },
        );

        dialog
            .title(tr("分析这份结果集"))
            .child(body)
            .footer(DialogFooter::new().child(cancel).child(keep).child(temp))
    });
}
