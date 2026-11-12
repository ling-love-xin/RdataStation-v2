//! 版本对话框的渲染件：表头、版本行、动作条。

use super::*;

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
                .child("版本"),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_TIME))
                .flex_none()
                .child("时间"),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_SIZE))
                .flex_none()
                .child("大小"),
        )
        .child(
            div()
                .w(rems(ui::VERSION_COL_HASH))
                .flex_none()
                .child("指纹"),
        )
        .child(div().flex_1().min_w_0().child("变化"))
        .child(
            div()
                .w(rems(ui::VERSION_COL_COPY))
                .flex_none()
                .child("副本"),
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
                            cell.child(div().text_color(success).child("当前"))
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
                        .child(if row.has_copy {
                            div().text_color(muted).child("在位")
                        } else {
                            div().text_color(danger).child("副本缺失")
                        }),
                ),
        )
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
    let can_diff = !row.is_current && row.has_copy && !busy;
    let hint = if read_only {
        "项目处于只读模式：还原 / 取回 / 删副本都是写操作"
    } else if row.is_current {
        "这是当前版本（本体上的内容）：取回请用详情面板的「取回（检出）…」"
    } else if !row.has_copy {
        "该版本只留了元数据（内容副本已被保留策略裁剪），不能还原或取回"
    } else {
        "还原 = 用这一版的内容生成新版本；取回 = 复制一份可写草稿（本体不动）"
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
    // 删副本不可恢复（不进回收站）：先过一个确认框，再交给宿主。
    let delete = {
        let state = state.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let state = state.clone();
            let on_action = on_action.clone();
            window.open_alert_dialog(cx, move |alert, _window, _cx| {
                let state = state.clone();
                let on_action = on_action.clone();
                alert
                    .title(format!("删除 v{version} 的内容副本？"))
                    .description(
                        "版本记录会保留（列表里仍能看到这一版），但它的内容将无法再还原或取回。\
                         副本不入回收站，删除后不可恢复。",
                    )
                    .show_cancel(true)
                    .on_ok(move |_, window, cx| {
                        state.set_busy(true);
                        state.set_note(Some(format!("正在删除 v{version} 的内容副本…")));
                        on_action(VersionAction::DeleteCopy { version }, window, cx);
                        true
                    })
            });
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
                        .label("与当前版本对比…")
                        .tooltip("打开文本 diff 查看器，看这一版和当前版本差在哪（只读）")
                        .disabled(!can_diff)
                        .on_click(diff),
                )
                .child(
                    Button::new("version-restore")
                        .small()
                        .debug_selector(|| "version-restore".to_string())
                        .label(if row.is_current {
                            "当前版本".to_string()
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
                        .label("取回该版本为草稿")
                        .disabled(!can_act)
                        .on_click(checkout),
                )
                .child(
                    Button::new("version-delete-copy")
                        .ghost()
                        .small()
                        .debug_selector(|| "version-delete-copy".to_string())
                        .label("删除内容副本")
                        .disabled(!can_act)
                        .on_click(delete),
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
                        .child("资产履历"),
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

