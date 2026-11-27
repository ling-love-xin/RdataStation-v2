//! 分区表：基本信息 / 指纹 / 组织…（含版本分区的「查看全部…」入口）。

use shared::i18n::tr;
use super::*;

pub(super) fn render_body_sections(
    detail: &ArchiveDetail,
    actions: Option<&DetailActions>,
    selection: &VersionSelection,
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
        let is_source_section = title == SOURCE_SECTION_TITLE;
        let is_basic_section = title == BASIC_SECTION_TITLE;
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
                            .label(tr("复制"))
                            .text_color(muted)
                            // 带结构指纹的行口径与普通文件不一样（定义 + 结构，行数不算）：
                            // 说明就挂在指纹那一行的按钮上——那里正是用户会问“这串是什么”的地方。
                            // 判据是「有没有指纹」而不是种类：种类收成两档后，指纹是能力不是分类。
                            .tooltip(if detail.has_definition {
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
            } else if label == ANALYTICS_TABLE_LABEL {
                // 表名是拿去拼 SQL 的：只给看不给抄，用户就得回列表点一次复制。
                let table = detail.analytics_table.clone().unwrap_or_default();
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
                        Button::new("archive-detail-copy-table")
                            .ghost()
                            .xsmall()
                            .label(tr("复制"))
                            .text_color(muted)
                            .tooltip(tr("复制表名（拿去写 SQL 时不用回列表）"))
                            .on_click(move |_, _window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(table.clone()))
                            }),
                    )
                    .into_any_element();
                kv_list = kv_list.item(label, DescriptionText::AnyElement(value_el), 1);
            } else if label == PAYLOAD_PATH_LABEL {
                // 相对项目根的路径：要拿它去文件管理器里找本体，所以给复制；
                // 顺手给「显示」——用户看着这个路径的下一步就是去文件夹里翻它。
                let path = value.clone();
                let mut row = div().h_flex().items_center().gap_1().min_w_0().child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(foreground)
                        .child(value),
                );
                row = row.child(
                    Button::new("archive-detail-copy-payload-path")
                        .ghost()
                        .xsmall()
                        .label(tr("复制"))
                        .text_color(muted)
                        .tooltip(tr("复制相对项目根的路径"))
                        .on_click(move |_, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(path.clone()))
                        }),
                );
                // 本体缺失时没有可显示的文件（`request_reveal` 会去找路径，必然落空）。
                if let Some(actions) = actions.filter(|_| detail.status != ArchiveStatus::Missing) {
                    let host = actions.host.clone();
                    let detail = detail.clone();
                    row = row.child(
                        Button::new("archive-detail-reveal-payload")
                            .ghost()
                            .xsmall()
                            .label(tr("显示"))
                            .text_color(muted)
                            .tooltip(tr("在系统文件管理器里定位本体"))
                            .on_click(move |_, window, cx| {
                                host.request_reveal(&detail, window, cx)
                            }),
                    );
                }
                let value_el = row.into_any_element();
                kv_list = kv_list.item(label, DescriptionText::AnyElement(value_el), 1);
            } else if label == SOURCE_CONNECTION_LABEL {
                // 索引里存的是 `conn_…` 的 id，直接摆出去是一串乱码：按 id 换名字。
                // 只换名字、不给入口——按 id 跳导航定位属数据导航面板的活，不在这里重复。
                // 连接已删 / 名字查不到时原样回落 id（比留空有用：至少还能按 id 找回来）。
                let display = actions
                    .and_then(|actions| actions.host.connection_label(&value))
                    .unwrap_or(value);
                kv_list = kv_list.item(label, display, 1);
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
        let mut title_row = div()
            .h_flex()
            .w_full()
            .items_center()
            .h(rems(ui::DETAIL_SECTION_HEAD_HEIGHT))
            .border_b(ui::HAIRLINE)
            .border_color(colors.border)
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(muted)
            .child(title);
        // 「基本信息」整块只读（改内容只能走「取回」）：标题右侧标一句，
        // 免得用户对着这些行找编辑入口（原型 `.sect-h .lock`）。
        if is_basic_section {
            title_row = title_row.child(div().text_xs().text_color(colors.warning).child(tr("只读")));
        }
        let mut section = div().v_flex().w_full().gap_1().child(title_row).child(kv_list);
        // 版本区的**时间线**（原型 §3.1 的「历史」那几行细化成节点）：当前版本在最前，
        // 其后是最近几条历史版。点某个节点 → 浮层看那一版的时间 / 大小 / 指纹 / 变更 / 副本。
        // 时间、大小、指纹、差异都是宿主格式化好的字，这里只排版，不在渲染期算。
        if is_version_section && !detail.version_timeline.is_empty() {
            section = section.child(render_version_timeline(detail, selection, cx));
        }
        // 来源区下面接「重建定义」：表型资产的配方（只登记、不执行）。
        // 与内容预览同一处理：多行等宽块，硬塞进键值两列会把每行都多拉一列标签。
        if is_source_section {
            if let Some(sql) = detail.definition_sql.as_deref() {
                section = section.child(render_definition_block(sql, cx));
            }
        }
        // 版本分区多一个入口（原型 §3.1 的「查看全部…」）：打开版本历史对话框。
        // 只读项目下也可用——历史读取与还原是两件事，后者在对话框里自己管。
        // 全局行同样摆：全局实例有独立版本链，对话框按全局 scope 收敛为只读 + 对比。
        if is_version_section {
            if let Some(actions) = actions {
                let host = actions.host.clone();
                let resource_id = detail.id.clone();
                section = section.child(
                    Button::new("archive-detail-versions")
                        .ghost()
                        .xsmall()
                        .debug_selector(|| "archive-detail-versions".to_string())
                        .label(tr("查看全部…"))
                        .on_click(move |_, window, cx| {
                            host.request_version_history(&resource_id, window, cx)
                        }),
                );
            }
        }
        // 组织区的「移动…」（原型 §3.1 的分组选择）：与行右键的「移动到分组 ›」同一套宿主动作，
        // 只是入口在详情面板。未分组 / 当前分组置灰（点了也是白点）；末尾留「新建分组…」。
        // 判据走 [`crate::model::can_move_to_group`]（与右键菜单、拖拽同一份）。
        if is_org_section {
            if let Some(actions) = actions.filter(|actions| {
                crate::model::can_move_to_group(&detail.action_context(actions.read_only, 1))
            }) {
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

/// 版本时间线（**侧边面板形态**）：竖排轨道，选中一版就在轨道下方看这一版的信息。
///
/// 为什么竖排：详情是 280px 宽的侧栏，横向排不下「版本号 + 时间 + 大小」；竖排还更像
/// 一根可以“停在某一版上”的刻度轨。宽容器（版本历史对话框 48rem）另走横排形态。
///
/// 选中态**不在渲染层**：`selection` 由宿主给（谁渲染详情谁持有），点击回调也只回宿主；
/// 本函数是纯读——`selected = None` 时兜底第一个节点（当前版本），不会出现“选中为空”。
fn render_version_timeline(
    detail: &ArchiveDetail,
    selection: &VersionSelection,
    cx: &App,
) -> Div {
    let colors = cx.theme().colors;
    // 兜底：没选中就看当前版本（时间线第一个节点），逻辑上它一定在。
    let selected = selection
        .selected
        .unwrap_or_else(|| detail.version_timeline[0].version);
    let mut out = div().v_flex().w_full().gap_1();
    let mut rail = div()
        .v_flex()
        .w_full()
        .gap_1()
        .pl_2()
        .border_l(ui::HAIRLINE)
        .border_color(colors.border);
    for line in &detail.version_timeline {
        rail = rail.child(render_timeline_node(
            line,
            line.version == selected,
            selection,
            cx,
        ));
    }
    out = out.child(rail);
    // 选中那一版的信息卡**就地**摆在轨道下面（不是浮层）：侧栏里浮层会和内容抢地方，
    // 就地一块还能顺着面板一起滚。
    if let Some(node) = detail
        .version_timeline
        .iter()
        .find(|line| line.version == selected)
    {
        out = out.child(render_timeline_card(node, cx));
    }
    out
}

/// 时间线上的一个节点：圆点 + `vN` + 时间 · 大小（当前版本另标「当前」）。
///
/// 节点用 `Button`（不自绘可点 div）：hover / 键盘焦点 / a11y 都是组件带的；
/// 点它 = 选中这一版（回调回宿主），不是开浮层。
fn render_timeline_node(
    line: &DetailVersionLine,
    selected: bool,
    selection: &VersionSelection,
    cx: &App,
) -> AnyElement {
    let colors = cx.theme().colors;
    // 点用主色：当前版本与“正在看的那一版”都算“眼前这一版”，其余弱化。
    let dot_color = if line.is_current || selected {
        colors.primary
    } else {
        colors.muted_foreground
    };
    // 副本被裁过：版本行还在，但“还原它”做不到。明说比让用户点一下失败强。
    // 表型资产没有副本这回事（回退只改指向），一律按“可回退”呈现。
    let meta = match (line.size_label.is_empty(), line.has_copy || line.is_table) {
        (true, true) => line.time_label.clone(),
        (true, false) => format!("{} · 副本已裁剪", line.time_label),
        (false, true) => format!("{} · {}", line.time_label, line.size_label),
        (false, false) => format!("{} · {} · 副本已裁剪", line.time_label, line.size_label),
    };
    let row = div()
        .h_flex()
        .w_full()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_none()
                .w(rems(0.5))
                .h(rems(0.5))
                .rounded_full()
                .bg(dot_color),
        )
        .child(
            div()
                .flex_none()
                .w(rems(ui::DETAIL_VERSION_LABEL_WIDTH))
                .text_xs()
                // 选中那一版把版本号染成主色：侧栏里没有多余位置摆“已选中”徽标，
                // 颜色 + 下面那张信息卡就是选中标记。
                .text_color(if selected {
                    colors.primary
                } else {
                    colors.foreground
                })
                .child(format!("v{}", line.version)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_ellipsis()
                .text_color(colors.muted_foreground)
                .child(meta),
        );
    let row = if line.is_current {
        row.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.primary)
                .child(tr("当前")),
        )
    } else {
        row
    };
    // 点节点 = 选中这一版（宿主写选中态并重绘）。没接回调（单测纯渲染）时节点只展示。
    let version = line.version;
    let mut button = Button::new(SharedString::from(format!(
        "archive-timeline-v{}",
        line.version
    )))
    .ghost()
    .w_full()
    .h(rems(ui::ROW_HEIGHT))
    .px_1()
    .justify_start()
    .debug_selector(move || format!("archive-timeline-v{version}"))
    .child(row);
    if let Some(on_select) = selection.on_select.clone() {
        let version = line.version;
        button = button.on_click(move |_, window, cx| on_select(version, window, cx));
    }
    button.into_any_element()
}

/// 选中那一版的信息卡：时间 / 大小 / 指纹 / 相对上一版的变更 / 副本状态。
///
/// 只摆**这一版自己的**信息（每一版的完整列清单与数据导航的属性重复，不做）；
/// 动作不在这里重复——要还原或对比，走下面那颗「查看全部…」。
fn render_timeline_card(line: &DetailVersionLine, cx: &App) -> AnyElement {
    let colors = cx.theme().colors;
    let mut list = DescriptionList::new()
        .columns(1)
        .bordered(false)
        .with_size(Size::Small)
        .label_width(rems(3.5));
    list = list.item("时间", line.time_label.clone(), 1);
    if !line.size_label.is_empty() {
        list = list.item("大小", line.size_label.clone(), 1);
    }
    list = list.item("内容指纹", line.hash_short.clone(), 1);
    if !line.delta_label.is_empty() {
        list = list.item("变更", line.delta_label.clone(), 1);
    }
    list = list.item(
        if line.is_table { "本体" } else { "内容副本" },
        if line.is_table {
            "项目分析库里的表，可回退"
        } else if line.has_copy {
            "在位，可还原"
        } else {
            "已裁剪，只剩登记"
        },
        1,
    );
    let version = line.version;
    let mut card = div()
        .id(("archive-timeline-card", version as usize))
        .debug_selector(move || format!("archive-timeline-card-v{version}"))
        .v_flex()
        .w_full()
        .min_w_0()
        .gap_1()
        .p_2()
        .rounded_sm()
        .border_1()
        .border_color(colors.border)
        .bg(colors.sidebar)
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .text_color(colors.foreground)
                .child(format!("v{}", line.version))
                .child(if line.is_current {
                    div().text_color(colors.primary).child(tr("当前版本"))
                } else {
                    div().text_color(colors.muted_foreground).child(tr("历史版本"))
                }),
        )
        .child(list);
    // 末尾一句实话：能做什么、不能做什么。副本没了就别让人以为点得开。
    card = card.child(
        div()
            .text_xs()
            .text_color(colors.muted_foreground)
            .child(if line.is_table {
                "要回退到这一版，用下面的「查看全部…」"
            } else if line.has_copy {
                "要还原或对比，用下面的「查看全部…」"
            } else {
                "内容副本已被保留策略裁掉，只能看登记信息"
            }),
    );
    card.into_any_element()
}

/// 「重建定义」等宽块（表型资产的配方）。
///
/// **只登记、不执行**（`analysis::FINGERPRINT_HINT` 同一口径）：定义进指纹是为了让人看懂
/// 这一版的口径，不是让程序据此重算——所以底部明写“不执行”，免得用户以为点一下就能复原。
fn render_definition_block(sql: &str, cx: &App) -> Div {
    let colors = cx.theme().colors;
    let (muted, foreground, border) = (colors.muted_foreground, colors.foreground, colors.border);
    let mono_family = cx.theme().mono_font_family.clone();
    let mono_size = cx.theme().mono_font_size;
    let comment = "只登记留痕：定义不执行，也不会据此重建或校验数据".to_string();
    let copy_text = sql.to_string();
    let mut block = div()
        .id("archive-detail-definition")
        .debug_selector(|| "archive-detail-definition".to_string())
        .v_flex()
        .w_full()
        .min_w_0()
        .gap_0p5()
        .p_2()
        // 定义可能很长：给个上限，超出部分裁掉（要看全的点「复制」或走版本对比）。
        .max_h(rems(10.))
        .overflow_hidden()
        .rounded_sm()
        .border_1()
        .border_color(border)
        .font_family(mono_family)
        .text_size(mono_size)
        .text_color(foreground);
    for line in sql.lines() {
        block = block.child(
            div()
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .child(line.to_string()),
        );
    }
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(muted)
                        .child(DEFINITION_SECTION_TITLE),
                )
                .child(
                    Button::new("archive-detail-copy-definition")
                        .ghost()
                        .xsmall()
                        .label(tr("复制"))
                        .text_color(muted)
                        .tooltip(tr("复制完整定义（面板里只摆得下前几行）"))
                        .on_click(move |_, _window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()))
                        }),
                ),
        )
        .child(block)
        .child(div().text_xs().text_color(muted).child(comment))
}

/// 组织区的「移动…」：一个按钮 + 下拉菜单（未分组 / 各分组 / 新建分组…）。
///
/// 与行右键的「移动到分组 ›」**同一套宿主动作**（`request_move_to_group`），
/// 这里只是第二个入口——详情面板里也要能改分组，不能只让用户回列表右键。
/// 多选时动的是**整选集**（与右键同一口径），按钮上标条数，免得用户以为只动眼前这条。
fn render_move_to_group(detail: &ArchiveDetail, actions: &DetailActions) -> impl IntoElement {
    let host = actions.host.clone();
    let ids = move_ids(&detail.id, &actions.selected_ids);
    let count = ids.len();
    // 多选时各条的分组可能不同，一个勾代表不了整集：菜单既不标 check 也不置灰。
    let single = count <= 1;
    let current = detail.group_id.clone();
    let groups = detail.groups.clone();
    Button::new("archive-detail-move-group")
        .ghost()
        .xsmall()
        .debug_selector(|| "archive-detail-move-group".to_string())
        .label(move_label(count))
        .dropdown_menu(move |menu, _window, _cx| {
            let mut menu = menu;
            // 菜单项与行右键「移动到分组 ›」逐项对齐：未分组 / 各分组 / 新建分组…
            // （标题写在按钮上就够了，菜单里不再摆一行不可点的标题。）
            // 单选时当前项置灰：点了也不会变（与右键子菜单同一口径）。
            menu = menu.item(
                PopupMenuItem::new(tr("未分组"))
                    .checked(single && current.is_none())
                    .disabled(single && current.is_none())
                    .on_click({
                        let host = host.clone();
                        let ids = ids.clone();
                        move |_, window, cx| host.request_move_to_group(&ids, None, window, cx)
                    }),
            );
            for group in &groups {
                let is_current = single && current.as_deref() == Some(group.id.as_str());
                let group_id = group.id.clone();
                menu = menu.item(
                    PopupMenuItem::new(group.name.clone())
                        .checked(is_current)
                        .disabled(is_current)
                        .on_click({
                            let host = host.clone();
                            let ids = ids.clone();
                            move |_, window, cx| {
                                host.request_move_to_group(&ids, Some(&group_id), window, cx)
                            }
                        }),
                );
            }
            menu.separator()
                .item(PopupMenuItem::new(tr("新建分组…")).on_click({
                    let host = host.clone();
                    move |_, window, cx| host.request_create_group(window, cx)
                }))
        })
}

/// 「移动…」要动的 id 集：面板给了选集就用选集（多选是整集），
/// 没给（crate 单测、无宿主场景）就退回眼前这一条详情。
pub(super) fn move_ids(detail_id: &str, selected: &[String]) -> Vec<String> {
    if selected.is_empty() {
        vec![detail_id.to_string()]
    } else {
        selected.to_vec()
    }
}

/// 按钮文案：多条时标出数量——详情面板只显示焦点行，不标条数等于让用户误判范围。
pub(super) fn move_label(count: usize) -> String {
    if count <= 1 {
        "移动…".to_string()
    } else {
        format!("移动 {count} 条…")
    }
}
