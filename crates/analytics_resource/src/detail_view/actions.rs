//! 动作区：全局行只给「分发到本项目」；项目行给打开 / 取回 / 危险区。

use super::*;

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
        if detail.scope == RowScope::Global {
            let detail_for_distribute = detail.clone();
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
                                .on_click(move |_, window, cx| {
                                    host.request_distribute_from_global(
                                        &detail_for_distribute.name,
                                        window,
                                        cx,
                                    )
                                }),
                        ),
                    )
                    .child(div().text_xs().text_color(muted).child(
                        "全局实例只读：本体不可打开、取回或删除；需要内容时先分发到本项目。",
                    )),
            );
        } else {
            let missing = detail.status == ArchiveStatus::Missing;
            let changed = detail.status == ArchiveStatus::ContentChanged;
            let can_open = !missing && detail.payload_rel_path.is_some();
            let can_checkout = !missing && !changed && !actions.read_only;
            // 危险区（原型 §3.1）：与日常动作隔开（分隔线 + danger 文字按钮）。
            // copy 与 reference 都可移入回收站（B3 服务层已支持）：copy 搬本体进回收站，
            // reference 只删登记行 + 版本元数据，本体留在源位置。
            let has_payload = detail.payload_rel_path.is_some();
            let can_delete = !missing && has_payload && !actions.read_only;
            let is_reference = detail.payload_mode == PayloadMode::Reference;
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
