//! 动作区：全局行只给「分发到本项目」；项目行给打开 / 取回 / 危险区。

use super::*;

/// 全局行的分发按钮：`(是否可点, 按钮下方那句说明)`。
///
/// 抽成纯函数是为了能单测——`Button` 是 `RenderOnce`，渲染完读不到 `disabled`。
/// 判据本身在 [`crate::model::can_distribute`]（与列表右键菜单同一份），这里只负责
/// 把"为什么不能点"翻译成人话。
fn distribute_affordance(ctx: &crate::model::ActionContext) -> (bool, &'static str) {
    if ctx.read_only {
        (
            false,
            "项目处于只读模式：分发会向本项目库写表，解除只读后再分发。",
        )
    } else if ctx.status == ArchiveStatus::Missing {
        (
            false,
            "全局实例的本体缺失（表可能已被删除或移动）：先在其他项目重新升级一次，再分发。",
        )
    } else {
        (
            true,
            "全局实例只读：本体不可打开、取回或删除；需要内容时先分发到本项目。",
        )
    }
}

pub(super) fn render_actions(
    detail: &ArchiveDetail,
    actions: Option<&DetailActions>,
    cx: &App,
) -> Vec<Div> {
    let colors = cx.theme().colors;
    let border = colors.border;
    let muted = colors.muted_foreground;
    let tone_color = colors;
    let mut out: Vec<Div> = Vec::new();
    // 动作区：项目行 = 打开（只读）/ 取回（检出）+ 危险区；全局行（§13.4）只读，
    // 唯一动作 = 分发到本项目，打开 / 取回 / 删除 / 打标 / 分组 / 版本历史一律不摆。
    if let Some(actions) = actions {
        let host = actions.host.clone();
        // 动作判据统一在 `model::affordance`（详情恒单选，`multi_count` 给 1）。
        let ctx = detail.action_context(actions.read_only, 1);
        if detail.scope == RowScope::Global {
            let detail_for_distribute = detail.clone();
            // 分发要往本项目库写表：只读项目 / 本体缺失都挡住（与列表右键菜单同一判据）。
            let (can_distribute, distribute_hint) = distribute_affordance(&ctx);
            out.push(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .border_t(ui::HAIRLINE)
                    .border_color(border)
                    .pt_2()
                    .child(
                        div().h_flex().w_full().gap_2().child(
                            Button::new("archive-detail-distribute")
                                .ghost()
                                .label("分发到本项目…")
                                .disabled(!can_distribute)
                                .on_click(move |_, window, cx| {
                                    host.request_distribute_from_global(
                                        &detail_for_distribute.name,
                                        window,
                                        cx,
                                    )
                                }),
                        ),
                    )
                    .child(div().text_xs().text_color(muted).child(distribute_hint)),
            );
        } else {
            let missing = detail.status == ArchiveStatus::Missing;
            let changed = detail.status == ArchiveStatus::ContentChanged;
            // 三个判据都走同一份上下文（多选的限制在列表侧，详情恒单选）。
            let can_open = can_open(&ctx);
            let can_checkout = can_checkout(&ctx);
            let is_reference = detail.payload_mode == PayloadMode::Reference;
            // 危险区（原型 §3.1）：与日常动作隔开（分隔线 + danger 文字按钮）。
            // copy 与 reference 都可移入回收站（B3 服务层已支持）：copy 搬本体进回收站，
            // reference 只删登记行 + 版本元数据，本体留在源位置。
            let can_delete = can_delete_from_detail(&ctx);
            // 禁用时给的理由要**指向出口**（去哪儿处理），不是一句"不可用"。
            let hint = if missing {
                "本体缺失，先在状态行「修复…」处理"
            } else if actions.read_only {
                "项目处于只读模式"
            } else if changed {
                "内容已变：先在状态行「修复…」接受当前内容或从历史还原，再取回"
            } else {
                "打开 = 只读查看本体；取回 = 复制一份可写的工作副本（本体不动）"
            };
            let delete_hint = if missing {
                "本体缺失：先走索引修复（回收站里没有它的本体）"
            } else if actions.read_only {
                "项目处于只读模式"
            } else if is_reference {
                "只删除登记记录与版本元数据，源文件留在原位置不受影响；可从回收站还原登记"
            } else {
                "本体与登记记录一起进项目级回收站，之后可从面板头「⋯ → 回收站…」还原"
            };
            // 闭包是 `Fn`（每帧重建），且它比 `detail` 活得久——拷一份带走。
            let detail_for_open = detail.clone();
            let detail_for_checkout = detail.clone();
            let detail_for_delete = detail.clone();
            out.push(
                div()
                    .v_flex()
                    .w_full()
                    .gap_1()
                    .border_t(ui::HAIRLINE)
                    .border_color(border)
                    .pt_2()
                    .child(
                        div()
                            .h_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                Button::new("archive-detail-open")
                                    .ghost()
                                    .label("打开（只读）")
                                    .disabled(!can_open)
                                    .on_click({
                                        let host = host.clone();
                                        let detail = detail_for_open.clone();
                                        move |_, window, cx| host.request_open(&detail, window, cx)
                                    }),
                            )
                            .child(
                                Button::new("archive-detail-checkout")
                                    .ghost()
                                    .label("取回（检出）…")
                                    .disabled(!can_checkout)
                                    .on_click({
                                        let host = host.clone();
                                        let detail = detail_for_checkout.clone();
                                        move |_, window, cx| {
                                            host.request_checkout(&detail, window, cx)
                                        }
                                    }),
                            ),
                    )
                    .child(div().text_xs().text_color(muted).child(hint))
                    .child(
                        div()
                            .v_flex()
                            .w_full()
                            .gap_1()
                            .mt_2()
                            .pt_2()
                            .border_t(ui::HAIRLINE)
                            .border_color(border)
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(tone_color.danger)
                                    .child("危险区"),
                            )
                            .child(
                                Button::new("archive-detail-delete")
                                    .with_variant(ButtonVariant::Danger)
                                    .label("移入回收站")
                                    .disabled(!can_delete)
                                    .on_click({
                                        let host = host.clone();
                                        let detail = detail_for_delete.clone();
                                        move |_, window, cx| {
                                            host.request_delete(
                                                &[DeleteTarget {
                                                    id: detail.id.clone(),
                                                    mode: detail.payload_mode,
                                                }],
                                                window,
                                                cx,
                                            )
                                        }
                                    }),
                            )
                            .child(div().text_xs().text_color(muted).child(delete_hint)),
                    ),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::distribute_affordance;
    use crate::model::{ActionContext, ArchiveStatus, RowScope};

    fn global_ctx() -> ActionContext {
        let mut ctx = ActionContext::project_row();
        ctx.scope = RowScope::Global;
        ctx.has_payload = false; // 分析表型全局行：本体在全局库，没有项目侧路径
        ctx
    }

    #[test]
    fn distribute_is_disabled_on_a_read_only_project() {
        let mut ctx = global_ctx();
        ctx.read_only = true;
        let (enabled, hint) = distribute_affordance(&ctx);
        assert!(!enabled, "只读项目不能分发（会写本项目库）");
        assert!(
            hint.contains("只读模式"),
            "禁用时说明要指向原因，实际：{hint}"
        );
    }

    #[test]
    fn distribute_is_disabled_when_the_global_payload_is_missing() {
        let mut ctx = global_ctx();
        ctx.status = ArchiveStatus::Missing;
        let (enabled, hint) = distribute_affordance(&ctx);
        assert!(!enabled, "本体缺失时 COPY 必失败，入口就该置灰");
        assert!(
            hint.contains("本体缺失"),
            "禁用时说明要指向原因，实际：{hint}"
        );
    }

    #[test]
    fn distribute_is_enabled_otherwise() {
        let (enabled, hint) = distribute_affordance(&global_ctx());
        assert!(enabled);
        assert!(
            hint.contains("先分发到本项目"),
            "可分发时说明要指向出口，实际：{hint}"
        );
    }
}
