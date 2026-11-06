//! 分区表：基本信息 / 指纹 / 组织…（含版本分区的「查看全部…」入口）。

use super::*;

pub(super) fn render_body_sections(
    detail: &ArchiveDetail,
    actions: Option<&DetailActions>,
    cx: &App,
) -> Vec<Div> {
    let colors = cx.theme().colors;
    let foreground = colors.foreground;
    let muted = colors.muted_foreground;
    let mut out: Vec<Div> = Vec::new();
    // 分区
    for (title, rows) in detail_rows(detail) {
        // 「这个分区要多一个入口吗」得在 `title` 被 move 进子元素之前定下。
        let is_version_section = title == VERSION_SECTION_TITLE;
        let is_org_section = title == ORG_SECTION_TITLE;
        // kv 行 → gpui-kit `description_list::DescriptionList`（规格 §7.2 UI 统一）：
        // label 对齐由组件管；指纹复制按钮经 `DescriptionText::AnyElement` 承载。
        let mut kv_list = DescriptionList::new()
            .columns(1)
            .bordered(false)
            .with_size(Size::Small)
            .label_width(rems(ui::DETAIL_LABEL_WIDTH));
        for (label, value) in rows {
            // 「内容指纹」那一行多一个复制口（原型 §3.1：前 12 位可复制）——
            // 拷贝的是**完整指纹**：要拿去比对的时候，截断版没用。
            let copy_hash = (label == HASH_LABEL)
                .then(|| detail.content_hash.clone())
                .flatten();
            if let Some(hash) = copy_hash {
                let value_el = div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(foreground)
                            .child(value),
                    )
                    .child(
                        Button::new("archive-detail-copy-hash")
                            .ghost()
                            .xsmall()
                            .label("复制")
                            .text_color(muted)
                            // 分析表的指纹口径与人不一样（定义 + 结构，行数不算）：
                            // 就写在指纹那一行的按钮上——那里正是用户会问“这串是什么”的地方。
                            .tooltip(if detail.kind == ArchiveKind::Analysis {
                                crate::analysis::FINGERPRINT_HINT
                            } else {
                                "复制完整指纹（展示的是前 12 位）"
                            })
                            .on_click(move |_, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(hash.clone()))
                            }),
                    )
                    .into_any_element();
                kv_list = kv_list.item(label, DescriptionText::AnyElement(value_el), 1);
            } else if label == "源路径" {
                // 源绝对路径常被 DescriptionList 裁切（无省略号），HoverCard 悬停显示完整路径（P3）。
                // gpui-kit 无通用 `.tooltip` 扩展（`ManagedTooltipExt` 是 crate 私有），走公共 `HoverCard`
                //（database/nav_view/primitives.rs 同例：悬停展开归属域短码）。
                let value_el = HoverCard::new(SharedString::from("archive-detail-source-path"))
                    .open_delay(std::time::Duration::from_millis(300))
                    .trigger(
                        div()
                            .text_xs()
                            .text_ellipsis()
                            .text_color(foreground)
                            .child(value.clone()),
                    )
                    .content(move |_, _window, cx| {
                        let fg = cx.theme().colors.foreground;
                        div()
                            .px_2()
                            .py_1()
                            .text_xs()
                            .text_color(fg)
                            .child(value.clone())
                    })
                    .into_any_element();
                kv_list = kv_list.item(label, DescriptionText::AnyElement(value_el), 1);
            } else {
                kv_list = kv_list.item(label, value, 1);
            }
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
                    .child(title),
            )
            .child(kv_list);
        // 版本区的「最近 3 条」明细（原型 §3.1）：每行 版本号 + 时间 · 体积。
        // 只列历史版本（当前版本已在「基本信息」的「版本」行），无明细时这一块不出现。
        // 时间/体积都是宿主格式化好的字，这里只排版；不塞按钮（点开进对话框看全）。
        if is_version_section && !detail.recent_versions.is_empty() {
            let mut lines = div().v_flex().w_full().gap_1();
            for line in &detail.recent_versions {
                let label = format!("v{}", line.version);
                let meta = if line.size_label.is_empty() {
                    line.time_label.clone()
                } else {
                    format!("{} · {}", line.time_label, line.size_label)
                };
                lines = lines.child(
                    div()
                        .h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .w(rems(ui::DETAIL_VERSION_LABEL_WIDTH))
                                .flex_none()
                                .text_xs()
                                .text_color(foreground)
                                .child(label),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_ellipsis()
                                .text_color(muted)
                                .child(meta),
                        ),
                );
            }
            section = section.child(lines);
        }
        // 版本分区多一个入口（原型 §3.1 的「查看全部…」）：打开版本历史对话框。
        // 只读项目下也可用——历史读取与还原是两件事，后者在对话框里自己管。
        // 全局行没有本地版本历史（本体在全局实例），入口不摆（§13.4 动作收敛）。
        if is_version_section && detail.scope != RowScope::Global {
            if let Some(actions) = actions {
                let host = actions.host.clone();
                let resource_id = detail.id.clone();
                section = section.child(
                    Button::new("archive-detail-versions")
                        .ghost()
                        .xsmall()
                        .debug_selector(|| "archive-detail-versions".to_string())
                        .label("查看全部…")
                        .on_click(move |_, window, cx| {
                            host.request_version_history(&resource_id, window, cx)
                        }),
                );
            }
        }
        // 组织区的「移动…」（原型 §3.1 的分组选择）：与行右键的「移动到分组 ›」同一套宿主动作，
        // 只是入口在详情面板。未分组 / 当前分组置灰（点了也是白点）；末尾留「新建分组…」。
        // 只读项目下不给（写类动作一律禁用，与右键同一判据）；全局行没有项目分组归属。
        if is_org_section && detail.scope != RowScope::Global {
            if let Some(actions) = actions.filter(|actions| !actions.read_only) {
                section = section.child(render_move_to_group(detail, actions));
            }
        }
        out.push(section);
    }

    // 标签分区（原型 §3.1 的“标签与分组”里的标签那一半）：chips 可逐个去掉，「＋ 标签」开打标对话框。
    // 全局行不带本地标签、也不允许打标（§13.4 动作收敛），整块分区不渲染。
    if detail.scope != RowScope::Global {
        out.push(render_tag_section(detail, actions, cx));
    }

    // 内容预览（原型 §3.1 的最后一块，摆在标签与危险区之间）：文本型给前 20 行，
    // 二进制 / 大文件给一句说明。预览**只读**：这里没有任何编辑入口（原型 §1 原则 3）。
    out.push(render_preview_section(detail, cx));
    out
}

/// 组织区的「移动…」：一个按钮 + 下拉菜单（未分组 / 各分组 / 新建分组…）。
///
/// 与行右键的「移动到分组 ›」**同一套宿主动作**（`request_move_to_group`），
/// 这里只是第二个入口——详情面板里也要能改分组，不能只让用户回列表右键。
fn render_move_to_group(detail: &ArchiveDetail, actions: &DetailActions) -> impl IntoElement {
    let host = actions.host.clone();
    let resource_id = detail.id.clone();
    let current = detail.group_id.clone();
    let groups = detail.groups.clone();
    Button::new("archive-detail-move-group")
        .ghost()
        .xsmall()
        .debug_selector(|| "archive-detail-move-group".to_string())
        .label("移动…")
        .dropdown_menu(move |menu, _window, _cx| {
            let mut menu = menu;
            // 菜单项与行右键「移动到分组 ›」逐项对齐：未分组 / 各分组 / 新建分组…
            // （标题写在按钮上就够了，菜单里不再摆一行不可点的标题。）
            // 当前项置灰：点了也不会变（与右键子菜单同一口径）。
            menu = menu.item(
                PopupMenuItem::new("未分组")
                    .checked(current.is_none())
                    .disabled(current.is_none())
                    .on_click({
                        let host = host.clone();
                        let ids = vec![resource_id.clone()];
                        move |_, window, cx| host.request_move_to_group(&ids, None, window, cx)
                    }),
            );
            for group in &groups {
                let is_current = current.as_deref() == Some(group.id.as_str());
                let group_id = group.id.clone();
                menu = menu.item(
                    PopupMenuItem::new(group.name.clone())
                        .checked(is_current)
                        .disabled(is_current)
                        .on_click({
                            let host = host.clone();
                            let ids = vec![resource_id.clone()];
                            move |_, window, cx| {
                                host.request_move_to_group(&ids, Some(&group_id), window, cx)
                            }
                        }),
                );
            }
            menu.separator()
                .item(PopupMenuItem::new("新建分组…").on_click({
                    let host = host.clone();
                    move |_, window, cx| host.request_create_group(window, cx)
                }))
        })
}
