//! 书签弹窗：书签说明输入 + 书签管理器列表。

use super::*;

// ═══ 书签弹窗（书签说明输入 + 书签管理器列表）═══

/// 【书签】给某行写书签说明（折叠说明书签的输入口）
///
/// 与 `open_bind_dialog` 同构：输入实体在**开窗前**建好并预填（有说明 = 改、
/// 无 = 默认行号）；保存 → 回调宿主（`set_label` + 落盘 sidecar）。
/// Esc / 点遮罩 / 取消 = 不改。
pub fn open_bookmark_note_dialog(
    window: &mut Window,
    cx: &mut App,
    line: usize,
    initial: String,
    on_submit: impl Fn(String, &mut Window, &mut App) + 'static,
) {
    use gpui_kit::component::input::{Input, InputState};

    let input = cx.new(|cx| {
        let mut state = InputState::new(window, cx);
        state.set_value(initial, window, cx);
        state
    });
    let on_submit = Rc::new(on_submit);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(div().text_xs().text_color(muted).child(format!(
                "第 {line} 行 · 写一句为什么（折叠区域留说明会被记录）"
            )))
            .child(
                div()
                    .debug_selector(|| "editor-dialog-editor-bookmark-note-input".to_string())
                    .child(Input::new(&input)),
            );

        let save = Button::new("editor-bookmark-note-save")
            .with_variant(ButtonVariant::Primary)
            .small()
            .label("保存")
            .debug_selector(|| "editor-dialog-editor-bookmark-note-save".to_string())
            .on_click({
                let on_submit = on_submit.clone();
                let input = input.clone();
                move |_, window, cx| {
                    let text = input.read(cx).value().to_string();
                    window.close_dialog(cx);
                    on_submit(text, window, cx);
                }
            });
        let cancel = action_button(
            "editor-bookmark-note-cancel",
            "取消",
            ButtonVariant::Ghost,
            move |_window, _cx| {},
        );

        dialog
            .title(format!("书签说明 · 第 {line} 行"))
            .child(body)
            .footer(DialogFooter::new().child(cancel).child(save))
    });
}

/// 【书签】书签管理器浮层 v4：搜索定位（右侧内嵌类型筛选）+ 三栏列表 + 双击跳转
///
/// 结构（自上而下）：
/// - **搜索行**：`Input` 输入框，`suffix` 内嵌 `ButtonGroup` 类型筛选（全部 / 书签 / 折叠）——
///   类型切换只改 `Cell<BookmarkKind>` 并 `window.draw` 强制重绘（builder 会重跑）；
///   搜索文本由 builder 直接读 `InputState`（输入变化 → Input 内部 notify → dialog 层重绘）。
/// - **三栏列表**：位置（固定行号）｜标签（徽标 + 说明截断）｜操作（编辑 / 删除图标）。
///   徽标判据：`label == 行号` = 📌 书签（无说明）；否则 = ▾ 折叠（有说明，说明即记录）。
/// - **双击行 = 跳转**（`on_double_click`；先关浮层再回调宿主）。
/// - 空态区分：真的没有书签 vs 类型/搜索过滤后没有匹配。
///
/// 类型与查询叠加过滤在 [`crate::bookmark::filtered`]（纯函数，单测覆盖）；
/// 这里只负责把过滤结果渲染出来，不重复判定逻辑。
/// 实体（Input / kind Cell）必须在**开窗前**创建——dialog builder 会重跑（Root 重建时），
/// 闭包里不能建实体。
pub fn open_bookmark_manager(
    window: &mut Window,
    cx: &mut App,
    bookmarks: Vec<file_meta::Bookmark>,
    on_jump: impl Fn(usize, &mut Window, &mut App) + 'static,
    on_edit: impl Fn(usize, String, &mut Window, &mut App) + 'static,
    on_delete: impl Fn(usize, &mut Window, &mut App) + 'static,
) {
    use crate::bookmark::{BookmarkKind, filtered};
    use gpui_kit::component::button::ButtonGroup;
    use gpui_kit::component::input::{Input, InputState};

    let kind = Rc::new(std::cell::Cell::new(BookmarkKind::All));
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索行号 / 说明 / #标签…"));
    let on_jump = Rc::new(on_jump);
    let on_edit = Rc::new(on_edit);
    let on_delete = Rc::new(on_delete);

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let muted = theme.colors.muted_foreground;
        let border = theme.colors.border;

        let query = input.read(cx).value().to_string();
        let current_kind = kind.get();
        let mut list = bookmarks.clone();
        crate::bookmark::normalize(&mut list);
        let rows: Vec<_> = filtered(&list, current_kind, &query);

        // 类型切换按钮：内嵌在搜索框 suffix（用户口径「内嵌在搜索定位输入框内」）
        let kind_cell = kind.clone();
        let kind_button = |label: &'static str, kind: BookmarkKind| {
            let kind_cell = kind_cell.clone();
            let key = match kind {
                BookmarkKind::All => "all",
                BookmarkKind::Bookmark => "bookmark",
                BookmarkKind::Fold => "fold",
            };
            Button::new(format!("editor-bookmark-kind-{key}"))
                .ghost()
                .small()
                .label(label)
                .selected(current_kind == kind)
                .debug_selector(move || {
                    format!("editor-dialog-editor-bookmark-kind-{key}").to_string()
                })
                .on_click(move |_, window, cx| {
                    kind_cell.set(kind);
                    let _ = window.draw(cx); // 强制重绘 → builder 重跑 → 列表按新类型过滤
                })
        };
        let search = div()
            .debug_selector(|| "editor-dialog-editor-bookmark-search".to_string())
            .child(
                Input::new(&input).suffix(
                    ButtonGroup::new("editor-bookmark-kind-group")
                        .compact()
                        .child(kind_button("全部", BookmarkKind::All))
                        .child(kind_button("书签", BookmarkKind::Bookmark))
                        .child(kind_button("折叠", BookmarkKind::Fold)),
                ),
            );

        // 表头：位置（固定）｜标签｜操作
        let header = div()
            .h_flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(border)
            .text_xs()
            .text_color(muted)
            .child(div().w_10().child("位置"))
            .child(div().flex_1().child("标签"))
            .child(div().w_16().child("操作"));

        let row_children = rows.iter().map(|b| {
            let line = b.line;
            let is_note = b.label != line.to_string();
            let (badge, badge_color) = if is_note {
                ("▾ 折叠", muted)
            } else {
                ("📌 书签", theme.colors.primary)
            };
            let label_text = truncate_label(&b.label, 40);
            let label_full = b.label.clone();
            let on_jump = on_jump.clone();
            let on_edit = on_edit.clone();
            let on_delete = on_delete.clone();
            div()
                .id(format!("editor-bookmark-row-{line}"))
                .h_flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(border)
                .debug_selector(move || {
                    format!("editor-dialog-editor-bookmark-line-{line}").to_string()
                })
                .on_double_click(move |_, window, cx| {
                    window.close_dialog(cx);
                    on_jump(line, window, cx);
                })
                .child(
                    div()
                        .w_10()
                        .text_xs()
                        .text_color(muted)
                        .child(line.to_string()),
                )
                .child(
                    div()
                        .h_flex()
                        .flex_1()
                        .items_center()
                        .gap_1()
                        .child(div().text_xs().text_color(badge_color).child(badge))
                        .child(div().text_xs().child(label_text)),
                )
                .child(
                    div()
                        .w_16()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .justify_end()
                        .child(
                            Button::new(format!("editor-bookmark-edit-{line}"))
                                .ghost()
                                .small()
                                .icon(IconName::Pencil)
                                .tooltip("编辑")
                                .debug_selector(move || {
                                    format!("editor-dialog-editor-bookmark-edit-{line}").to_string()
                                })
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    on_edit(line, label_full.clone(), window, cx);
                                }),
                        )
                        .child(
                            Button::new(format!("editor-bookmark-delete-{line}"))
                                .ghost()
                                .small()
                                .icon(IconName::Trash)
                                .tooltip("删除")
                                .debug_selector(move || {
                                    format!("editor-dialog-editor-bookmark-delete-{line}")
                                        .to_string()
                                })
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    on_delete(line, window, cx);
                                }),
                        ),
                )
        });

        let body = if list.is_empty() {
            div()
                .w_full()
                .text_xs()
                .text_color(muted)
                .py_4()
                .flex()
                .justify_center()
                .debug_selector(|| "editor-dialog-editor-bookmark-empty".to_string())
                .child("还没有书签 —— Ctrl+Shift+B 或右键「添加书签/说明…」")
        } else if rows.is_empty() {
            div()
                .w_full()
                .text_xs()
                .text_color(muted)
                .py_4()
                .flex()
                .justify_center()
                .debug_selector(|| "editor-dialog-editor-bookmark-empty".to_string())
                .child("没有匹配的书签 —— 试试行号 / 说明 / #标签")
        } else {
            div()
                .v_flex()
                .w_full()
                .gap_0()
                .child(search)
                .child(header)
                .children(row_children)
        };

        let close = action_button("editor-bookmark-close", "关闭", ButtonVariant::Ghost, {
            move |_window, _cx| {}
        });
        dialog
            .title("书签")
            .child(body)
            .footer(DialogFooter::new().child(close))
    });
}

/// 标签列文案截断（按字符数；超长尾部补省略号——不依赖样式层，headless 可断言）
fn truncate_label(label: &str, max_chars: usize) -> String {
    let count = label.chars().count();
    if count <= max_chars {
        return label.to_string();
    }
    let head: String = label.chars().take(max_chars).collect();
    format!("{head}…")
}
