//! 版本对话框的渲染件：版本轨道、表头、版本行、动作条。

use shared::i18n::tr;
use super::*;

/// 横排轨道最多摆多少个节点（再多就糊成一团，尾部提示“更早 N 版见下表”）。
const RAIL_MAX_NODES: usize = 12;

/// 表头（列名与下面各行的列宽一一对应）。
pub(super) fn header_line(theme: &Theme) -> Div {
    let muted = theme.colors.muted_foreground;
    div()
        .h_flex()
        .w_full()
        .items_center()
        .gap_3()
        .px_2()
        .py_1()
        .border_b(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .text_xs()
        .text_color(muted)
        .child(
            div()
                .w(rems(ui::VERSION_COL_VERSION))
                .flex_none()
                .child(tr("版本")),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_TIME))
                .flex_none()
                .child(tr("时间")),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_SIZE))
                .flex_none()
                .child(tr("大小")),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_HASH))
                .flex_none()
                .child(tr("指纹")),
        )
        .child(div().flex_1().min_w_0().child(tr("变化")))
        .child(
            div()
                .w(rems(ui::VERSION_COL_COPY))
                .flex_none()
                .child(tr("副本")),
        )
}

/// 一行版本（点击 = 选中；选中后动作栏才出现——768px 里塞不下每行三个按钮）。
///
/// 行本体走组件的 `ListItem`（与面板列表行同源）：hover / 选中 / 键盘都归它，
/// 不再手搓 `when(selected, bg) + hover(bg)` 那对——那对写反顺序就会变成
/// 「悬停把选中底盖掉」（导航侧 V11 口径：悬停不覆盖选中，草稿箱侧同批修过）。
pub(super) fn version_line(
    theme: &Theme,
    row: &VersionRow,
    selected: bool,
    state: VersionDialogState,
) -> impl IntoElement {
    let (foreground, muted, danger, success) = {
        let colors = theme.colors;
        (
            colors.foreground,
            colors.muted_foreground,
            colors.danger,
            colors.success,
        )
    };
    let version = row.version;
    let toggle = {
        let state = state.clone();
        move |_: &ClickEvent, _window: &mut Window, _cx: &mut App| {
            state.set_selected(Some(version));
        }
    };

    ListItem::new(SharedString::from(format!("version-row-{version}")))
        // 调试选择器：窗口测试按它定位并真点（选中态只有这一处落笔）。
        .debug_selector(move || format!("version-row-{version}"))
        // 列内距对齐表头（`header_line` 也是 `px_2 py_1`）：`ListItem` 自带的是 `px_3`。
        .px_2()
        .selected(selected)
        .on_click(toggle)
        .child(
            div()
                .h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_3()
                .text_xs()
                .child(
                    div()
                        .h_flex()
                        .w(rems(ui::VERSION_COL_VERSION))
                        .flex_none()
                        .items_center()
                        .gap_1()
                        .child(div().text_color(foreground).child(format!("v{version}")))
                        .when(row.is_current, |cell| {
                            cell.child(div().text_color(success).child(tr("当前")))
                        }),
                )
                .child(
                    div()
                        .w(rems(ui::VERSION_COL_TIME))
                        .flex_none()
                        .text_ellipsis()
                        .text_color(muted)
                        .child(row.time_label.clone()),
                )
                .child(
                    div()
                        .w(rems(ui::VERSION_COL_SIZE))
                        .flex_none()
                        .text_ellipsis()
                        .text_color(muted)
                        .child(row.size_label.clone()),
                )
                .child(
                    div()
                        .w(rems(ui::VERSION_COL_HASH))
                        .flex_none()
                        .text_ellipsis()
                        .text_color(muted)
                        .child(row.hash_short.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_color(muted)
                        .child(row.delta_label.clone()),
                )
                .child(
                    div()
                        .w(rems(ui::VERSION_COL_COPY))
                        .flex_none()
                        .text_ellipsis()
                        .child(if row.is_table {
                            // 表型资产没有副本文件：这一列只能说清“本体是什么”，
                            // 别按副本口径写「副本缺失」。
                            div().text_color(muted).child(tr("项目表"))
                        } else if row.has_copy {
                            div().text_color(muted).child(tr("在位"))
                        } else {
                            div().text_color(danger).child(tr("副本缺失"))
                        }),
                ),
        )
}

/// 版本轨道（**横排**，像进度条）：版本即节点，从左到右 v1 → 最新。
///
/// 为什么这里横排：对话框 48rem 宽，横向才排得下“一串版本”；侧边详情只有 320px，
/// 那边走竖排轨道（`detail_view::render_version_timeline`）。两处语义完全一致：
/// **点节点 = 选中那一版**，且都落在同一份选中状态上（这里是
/// `VersionDialogState::selected`，所以点轨道与点表格行等价）。
///
/// 只放最近 `RAIL_MAX_NODES` 个（版本行永久保留，攒到几十上百个时轨道会先糊成一团）；
/// 截断时在轨道尾部明说“更早 N 版见下表”，不静默吞。
pub(super) fn version_rail(
    theme: &Theme,
    rows: &[VersionRow],
    selected: Option<i32>,
    state: VersionDialogState,
) -> Div {
    let colors = theme.colors;
    // 行是降序（当前在前）；轨道按时间从左往右读，所以取最新的一批再倒过来。
    let shown: Vec<VersionRow> = rows
        .iter()
        .take(RAIL_MAX_NODES)
        .rev()
        .cloned()
        .collect();
    let hidden = rows.len().saturating_sub(shown.len());
    let mut rail = div().h_flex().w_full().items_center().gap_1();
    for (index, row) in shown.iter().enumerate() {
        if index > 0 {
            // 节点之间的那段“轨道”：一根 1px 线，读起来才像进度条。
            rail = rail.child(
                div()
                    .flex_none()
                    .w(rems(0.75))
                    .h(ui::HAIRLINE)
                    .bg(colors.border),
            );
        }
        let version = row.version;
        let is_selected = selected == Some(version);
        let is_current = row.is_current;
        let dot = if is_current || is_selected {
            colors.primary
        } else {
            colors.muted_foreground
        };
        let label_color = if is_selected {
            colors.primary
        } else {
            colors.muted_foreground
        };
        let toggle = {
            let state = state.clone();
            move |_: &ClickEvent, _window: &mut Window, _cx: &mut App| {
                state.set_selected(Some(version));
            }
        };
        rail = rail.child(
            Button::new(SharedString::from(format!("version-rail-v{version}")))
                .ghost()
                .h(rems(ui::ROW_HEIGHT))
                .px_1()
                .debug_selector(move || format!("version-rail-v{version}"))
                .when(is_selected, |button| button.bg(colors.list_active))
                .on_click(toggle)
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .child(
                            div()
                                .flex_none()
                                .w(rems(0.375))
                                .h(rems(0.375))
                                .rounded_full()
                                .bg(dot),
                        )
                        .child(div().text_color(label_color).child(format!("v{version}"))),
                ),
        );
    }
    if hidden > 0 {
        rail = rail.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(format!("更早 {hidden} 版见下表")),
        );
    }
    rail
}

/// 动作栏：选中一行后才出现（原型 §4.3 的取舍：表格装不下每行三个按钮）。
pub(super) fn action_bar(
    theme: &Theme,
    row: &VersionRow,
    busy: bool,
    read_only: bool,
    state: VersionDialogState,
    on_action: Rc<dyn Fn(VersionAction, &mut Window, &mut App)>,
) -> Div {
    let muted = theme.colors.muted_foreground;
    let version = row.version;
    // 当前版本不走这两个动作（它就是本体）：取回走详情面板的「取回（检出）…」。
    let can_act = !row.is_current && row.has_copy && !busy && !read_only;
    // 对比是**只读查看**（不写库，只读项目也能看），只要求"这一版内容还在"；
    // 文本类型与大小上限由宿主在取数线程挡（对话框不查文件系统）。
    // 表型资产没有本体文件，文本对比无从谈起——直接不给这个按钮。
    let can_diff = !row.is_current && row.has_copy && !row.is_table && !busy;
    // 「用这一版分析」= 把历史副本挂成分析视图（只读查看：不写库、不动本体，只读项目也能用）。
    let can_analyze = can_diff;
    // 引用资产没有本地副本，只有「固化」：复制 pin 住的全局版本为独立本地表。
    let can_solidify = row.is_global_ref && !busy && !read_only;
    // 取回 / 删副本都基于 `resources/` 下的本体副本，表型资产没有。
    let can_checkout = can_act && !row.is_table;
    let hint = if row.is_global_ref {
        if read_only {
            "项目处于只读模式：固化会往本项目库写一张新表，需要先退出只读"
        } else {
            "这是全局引用：引用本身不复制数据；固化会把选中的全局版本复制成本地独立表（原引用保留）"
        }
    } else if read_only {
        "项目处于只读模式：还原 / 取回 / 删副本都是写操作；对比与「用 vN 分析」是查看，可以直接用"
    } else if row.is_current {
        "这是当前版本（本体上的内容）：取回请用详情面板的「取回（检出）…」"
    } else if row.is_table {
        "表型资产没有内容副本：回退 = 把本体指回这一版的表（两张表都在，不删数据）"
    } else if !row.has_copy {
        "该版本只留了元数据（内容副本已被保留策略裁剪），不能还原、取回或拿来分析"
    } else {
        "还原 = 用这一版的内容生成新版本；取回 = 复制一份可写草稿（本体不动）；分析 = 把这一版挂成视图看旧数（不动本体）"
    };

    let restore = {
        let state = state.clone();
        let on_action = on_action.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            state.set_selected(Some(version));
            state.set_busy(true);
            state.set_note(Some(format!("正在还原 v{version}…")));
            on_action(VersionAction::Restore { version }, window, cx);
        }
    };
    let checkout = {
        let state = state.clone();
        let on_action = on_action.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            state.set_busy(true);
            state.set_note(Some(format!("正在取回 v{version}…")));
            on_action(VersionAction::CheckoutDraft { version }, window, cx);
        }
    };
    // 对比不置忙、不开回执：它是**查看**，打开 diff 对话框（独立窗口），版本对话框保持打开。
    let diff = {
        let state = state.clone();
        let on_action = on_action.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            state.set_selected(Some(version));
            on_action(VersionAction::Diff { version }, window, cx);
        }
    };
    // 分析不置忙、不开回执：它只是把这一版挂成视图（与对比同属查看；版本对话框保持打开）。
    let analyze = {
        let state = state.clone();
        let on_action = on_action.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            state.set_selected(Some(version));
            on_action(VersionAction::Analyze { version }, window, cx);
        }
    };
    // 固化：复制这一版 pin 住的全局表成本地独立资产（原引用保留），非破坏性，不弹确认。
    let solidify = {
        let state = state.clone();
        let on_action = on_action.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            state.set_selected(Some(version));
            state.set_busy(true);
            state.set_note(Some(format!("正在固化 v{version}…")));
            on_action(VersionAction::Solidify { version }, window, cx);
        }
    };
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .border_t(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .pt_2()
        .child(
            div()
                .h_flex()
                .w_full()
                .gap_2()
                .child(
                    Button::new("version-diff")
                        .ghost()
                        .small()
                        .debug_selector(|| "version-diff".to_string())
                        .label(tr("与当前版本对比…"))
                        .tooltip(tr("打开文本 diff 查看器，看这一版和当前版本差在哪（只读）"))
                        .disabled(!can_diff)
                        .on_click(diff),
                )
                .child(
                    Button::new("version-analyze")
                        .ghost()
                        .small()
                        .debug_selector(|| "version-analyze".to_string())
                        .label(format!("用 v{version} 分析…"))
                        .tooltip("把这一版的内容副本挂成分析视图（只读：本体与版本都不动）")
                        .disabled(!can_analyze)
                        .on_click(analyze),
                )
                .child(
                    Button::new("version-solidify")
                        .small()
                        .debug_selector(|| "version-solidify".to_string())
                        .label(format!("固化为本地表（v{version}）"))
                        .tooltip("把这一版引用的全局表复制成本地独立表；原引用保留，其他版本仍按引用访问")
                        .disabled(!can_solidify)
                        .on_click(solidify),
                )
                .child(
                    Button::new("version-restore")
                        .small()
                        .debug_selector(|| "version-restore".to_string())
                        .label(if row.is_current {
                            "当前版本".to_string()
                        } else if row.is_table {
                            format!("回退到 v{}（生成 v{}）", row.version, row.version + 1)
                        } else {
                            format!("还原为当前版本（生成 v{}）", row.version + 1)
                        })
                        .disabled(!can_act)
                        .on_click(restore),
                )
                .child(
                    Button::new("version-checkout")
                        .ghost()
                        .small()
                        .debug_selector(|| "version-checkout".to_string())
                        .label(tr("取回该版本为草稿"))
                        .disabled(!can_checkout)
                        .on_click(checkout),
                ),
        )
        .child(div().text_xs().text_color(muted).child(hint))
}

/// 履历区一次列多少条（版本行永久保留，事件还会随动作增长）。
const MAX_TIMELINE_ROWS: usize = 30;

/// 资产履历区：版本节点 + 事件节点，时间倒序纯展示（不可点、不写状态）。
///
/// 用行而不是表格：履历的说明长度不定（口径变更可能是一整句话），固定列宽会把它截断。
/// 颜色只走主题 token；效力 / 异常类事件用 warning 强调，不做常驻装饰。
pub(super) fn timeline_section(theme: &Theme, rows: &[crate::model::TimelineRow]) -> Div {
    let shown = rows.len().min(MAX_TIMELINE_ROWS);
    let mut list = div()
        .v_flex()
        .w_full()
        .gap_1()
        .border_t(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .pt_2()
        .child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .py_1()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.foreground)
                        .child(tr("资产履历")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("共 {} 条", rows.len())),
                ),
        );

    for row in rows.iter().take(shown) {
        let title_color = if row.is_state_change {
            theme.colors.warning
        } else {
            theme.colors.foreground
        };
        list = list.child(
            div()
                .h_flex()
                .w_full()
                .min_w_0()
                .items_center()
                .gap_3()
                .px_2()
                .py_1()
                .text_xs()
                .child(
                    div()
                        .w(rems(ui::VERSION_COL_TIME))
                        .flex_none()
                        .text_ellipsis()
                        .text_color(theme.colors.muted_foreground)
                        .child(row.time_label.clone()),
                )
                .child(
                    div()
                        .w(rems(ui::VERSION_COL_VERSION))
                        .flex_none()
                        .text_ellipsis()
                        .text_color(title_color)
                        .child(row.title.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_color(theme.colors.muted_foreground)
                        .child(row.detail.clone()),
                ),
        );
    }

    if rows.len() > shown {
        list = list.child(
            div()
                .w_full()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(format!(
                    "还有 {} 条更早的履历未列出（一次最多列 {MAX_TIMELINE_ROWS} 条）",
                    rows.len() - shown
                )),
        );
    }
    list
}
