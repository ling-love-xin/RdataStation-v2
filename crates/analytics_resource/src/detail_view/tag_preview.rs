//! 标签分区、别名行与内容预览分区。

use super::*;

/// 标签分区：chips（每个带 ×）+ 「＋ 标签」。
///
/// `actions` 为 `None`（crate 单测 / 无宿主场景）或项目只读时，只摆 chips 不给动作——
/// 与另两个动作区的口径一致。
///
/// chip 用 gpui-kit `tag::Tag`（规格 §7.2 UI 统一；Small 档字号/内距最接近原自绘 chip），
/// 关闭入口组合 `Button::ghost` 内嵌（0.7.1 `Tag` 无内置 ×，决策 D5）。
pub(crate) fn render_tag_section(detail: &ArchiveDetail, actions: Option<&DetailActions>, cx: &App) -> Div {
    let muted = cx.theme().colors.muted_foreground;
    // 判据走 [`crate::model::can_edit_tags`]（与右键菜单同一份）：全局行没有项目标签归属，
    // 旧判据只看项目只读，于是全局行详情也能摘标签。
    let can_edit = actions.is_some_and(|a| crate::model::can_edit_tags(&detail.action_context(a.read_only, 1)));

    let mut chips = div().h_flex().w_full().flex_wrap().items_center().gap_1();
    if detail.tags.is_empty() {
        chips = chips.child(div().text_xs().text_color(muted).child("（还没有标签）"));
    }
    for chip in &detail.tags {
        let mut line = Tag::secondary()
            .with_size(Size::Small)
            .gap_1()
            .child(chip.name.clone());
        if let Some(actions) = actions {
            if can_edit {
                // × = 去掉这个标签（不再要一次确认：重新打上只需两步）。
                // 语义按钮（不是自绘 div）：自带 hover / 焦点 / 键盘，且命中区有 20px 下限。
                let host = actions.host.clone();
                // 闭包是 `Fn` 且比 `detail` 活得久：每枚 chip 各拷一份带走。
                let detail = detail.clone();
                let tag_id = chip.id.clone();
                let remove_id = format!("archive-tag-remove-{}", chip.id);
                line = line.child(
                    Button::new(SharedString::from(remove_id))
                        .ghost()
                        .xsmall()
                        .debug_selector({
                            let id = chip.id.clone();
                            move || format!("archive-tag-remove-{id}")
                        })
                        .label("×")
                        .text_color(muted)
                        .tooltip("从这个存档上摘掉这个标签")
                        .on_click(move |_, window, cx| {
                            host.request_remove_tag(&detail, &tag_id, window, cx)
                        }),
                );
            }
        }
        chips = chips.child(line);
    }

    let mut section = div()
        .v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(muted)
                .child("标签"),
        )
        .child(chips);

    if let Some(actions) = actions {
        let host = actions.host.clone();
        let resource_id = detail.id.clone();
        let resource_name = detail.name.clone();
        section = section.child(
            Button::new("archive-detail-add-tag")
                .ghost()
                .xsmall()
                .debug_selector(|| "archive-detail-add-tag".to_string())
                .label("＋ 标签")
                .disabled(!can_edit)
                .on_click(move |_, window, cx| {
                    // 详情面板永远是**一元**：批量走列表的多选 + 右键菜单。
                    host.request_edit_tags(
                        &[crate::model::TagTarget {
                            id: resource_id.clone(),
                            name: resource_name.clone(),
                        }],
                        window,
                        cx,
                    )
                }),
        );
    }

    section
}
/// 渲染详情面板内容（只读信息区 + 标签分区 + 动作区 + 危险区）。
/// 头部显示名：可点时是个 ghost 按钮（`F2` 的同一条链路），没宿主 / 不可改时是纯文本。
///
/// 判据走 [`can_rename`]（与右键菜单、`F2` 同一份）：全局行与只读项目都不给入口——
/// 旧判据只看项目只读，于是**全局行详情的名字也能点**，点下去必然失败。
pub(crate) fn name_link(detail: &ArchiveDetail, actions: Option<&DetailActions>, foreground: Hsla) -> Div {
    let base = div()
        .min_w_0()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_ellipsis()
        .text_color(foreground);
    let editable = actions.is_some_and(|a| can_rename(&detail.action_context(a.read_only, 1)));
    let Some(actions) = actions.filter(|_| editable) else {
        return base.child(detail.name.clone());
    };
    let host = actions.host.clone();
    let id = detail.id.clone();
    base.child(
        Button::new("archive-detail-rename")
            .ghost()
            .xsmall()
            .debug_selector(|| "archive-detail-rename".to_string())
            .label(detail.name.clone())
            .text_color(foreground)
            .tooltip("重命名显示名（与 `F2` 同一条路；文件名与路径不变）")
            .on_click(move |_, window, cx| host.request_rename(&id, window, cx)),
    )
}

/// 头部别名：有值就显示它（可点改），没值就只给一个「＋ 别名」入口；只读 / 无宿主时只展示。
pub(crate) fn alias_line(
    detail: &ArchiveDetail,
    actions: Option<&DetailActions>,
    alias: &str,
    muted: Hsla,
) -> Div {
    let label = if alias.is_empty() {
        "＋ 别名".to_string()
    } else {
        alias.to_string()
    };
    let editable = actions.is_some_and(|a| can_rename(&detail.action_context(a.read_only, 1)));
    let Some(actions) = actions.filter(|_| editable) else {
        return div().text_xs().text_color(muted).child(label);
    };
    let host = actions.host.clone();
    let id = detail.id.clone();
    div().text_xs().text_color(muted).child(
        Button::new("archive-detail-alias")
            .ghost()
            .xsmall()
            .debug_selector(|| "archive-detail-alias".to_string())
            .label(label)
            .text_color(muted)
            .tooltip("编辑别名（留空 = 清除）")
            .on_click(move |_, window, cx| host.request_edit_alias(&id, window, cx)),
    )
}

/// 「内容预览」分区（原型 §3.1）：文本给前 20 行（等宽、只读、溢出不折行），其余给一句说明。
///
/// 为何不放进 `detail_rows` 的表行：这里是**多行等宽块**，与「标签 / 值」两列不是一回事；
/// 硬塞进去会让每行都多出一列标签，而预览的行本来就不需要标签。
pub fn render_preview_section(detail: &ArchiveDetail, cx: &App) -> Div {
    let (muted, foreground, border) = {
        let colors = cx.theme().colors;
        (colors.muted_foreground, colors.foreground, colors.border)
    };
    let mono_family = cx.theme().mono_font_family.clone();
    let mono_size = cx.theme().mono_font_size;

    let mut section = div().v_flex().w_full().gap_1().child(
        div()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(muted)
            .child(PREVIEW_SECTION_TITLE),
    );

    let lines = detail.preview.lines();
    if !lines.is_empty() {
        let mut block = div()
            .id("archive-detail-preview")
            .debug_selector(|| "archive-detail-preview".to_string())
            .v_flex()
            .w_full()
            .min_w_0()
            .gap_0p5()
            .p_2()
            .rounded_sm()
            .border_1()
            .border_color(border)
            .font_family(mono_family)
            .text_size(mono_size)
            .text_color(foreground);
        for line in lines {
            // 一行一条：长行省略号而不是折行（折行会让“前 20 行”变成“前 20 行的一部分”）。
            block = block.child(
                div()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(line.clone()),
            );
        }
        section = section.child(block);
    }
    if let Some(note) = detail.preview.note() {
        section = section.child(div().text_xs().text_color(muted).child(note));
    }
    section
}
