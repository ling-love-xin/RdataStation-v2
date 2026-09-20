//! 侧栏「暂存列表」（未保存草稿）的 `List` 组件化（决策 #107）。
//!
//! 与类型列表同一套做法（`type_tree.rs`）：小节 + 行交给组件，行用 `ListItem`
//! （悬停 = `list.hover`、选中 = `list.active` 底 + `list.active.border` 描边、a11y 角色齐全）；
//! 行尾的「✕ 删除」走 `ListItem::suffix`，用真 `Button`（自带 hover / 焦点 / 键盘激活 / tooltip）。
//!
//! 键盘：列表自己绑 ↑↓（"List" context），比对话框容器级的 `DraftPrev` / `DraftNext`
//! 更深——焦点在列表里时由列表接管（不会两个都动）；焦点在输入框或其它地方时，
//! 容器级绑定照旧生效（原型 §2.2 规则 6 的行为不变）。

use super::*;

use gpui_kit::component::IndexPath;
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};

/// 草稿列表委托（小节只有 1 个）。
pub struct DraftListDelegate {
    dialog: Rc<ConnectionDialogState>,
    entity: Entity<crate::panels::EditorPanel>,
    shared: Shared,
    /// 行数 / 光标位（渲染期同步）。
    len: usize,
    /// 组件记录的行号（确认时用）。
    active_index: Option<IndexPath>,
    /// 同步指纹：行数 + 光标 + 每条草稿的（saved_id, 名称）。
    fingerprint: String,
}

impl DraftListDelegate {
    pub(crate) fn new(
        dialog: Rc<ConnectionDialogState>,
        entity: Entity<crate::panels::EditorPanel>,
        shared: Shared,
    ) -> Self {
        Self {
            dialog,
            entity,
            shared,
            len: 0,
            active_index: None,
            fingerprint: String::new(),
        }
    }

    /// 渲染期同步（**权威同步点**）：行数 / 光标 / 行文案变了才重建。返回 true = 有变化。
    pub(crate) fn sync(&mut self, drafts: &[ConnectionDraft], cursor: usize) -> bool {
        let mut fp = format!("{cursor}/{}", drafts.len());
        for d in drafts {
            fp.push('\u{1}');
            fp.push_str(d.saved_id.as_deref().unwrap_or("-"));
            fp.push('\u{1}');
            fp.push_str(&d.display_name());
        }
        if fp == self.fingerprint {
            return false;
        }
        self.fingerprint = fp;
        self.len = drafts.len();
        true
    }

    /// 光标位对应的列表下标（无草稿 → None）。
    pub(crate) fn selected_path(&self) -> Option<IndexPath> {
        (self.len > 0).then(|| IndexPath::new(self.dialog.draft_cursor.get().min(self.len - 1)))
    }
}

impl ListDelegate for DraftListDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _: &App) -> usize {
        self.len
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let (draft_type_id, draft_name, saved_id) = {
            let drafts = self.dialog.drafts.borrow();
            let d = drafts.get(ix.row)?;
            (d.type_id.clone(), d.display_name(), d.saved_id.clone())
        };
        let is_saved = saved_id.is_some();
        // 当前条目：类型 / 名称取**正在编辑的表单**而不是快照——否则“刚从 MySQL 切到 SQLite”
        // 时表单已变、条目还显示 mysql 图标（真机反馈过）。其余条目只读快照。
        let live = (ix.row == self.dialog.draft_cursor.get())
            .then(|| self.dialog.live_entry_view(ix.row, cx))
            .flatten();
        // 借用 live 里的字段（`live` 后面还要用）：先取出引用再调用。
        let live_type_id = live.as_ref().map(|l| l.type_id.as_str());
        let display_type_id = staging_display_type_id(&draft_type_id, live_type_id);
        let label = match live.as_ref() {
            Some(l) if !l.name.trim().is_empty() => l.name.trim().to_string(),
            _ => draft_name,
        };
        let dirty = live.as_ref().map(|l| l.dirty).unwrap_or(false);
        let scope_code = saved_id.as_deref().and_then(saved_scope_short);
        // ElementId 用业务键：已保存的残余条目取 `saved_id`（持久实体不用位置 id）；
        // 未保存草稿的列表身份本就是下标（`staging_*` 全以 index 为键），故保留 `new-{i}`（#17）。
        let row_key = saved_id
            .as_deref()
            .map(|id| format!("saved-{id}"))
            .unwrap_or_else(|| format!("new-{}", ix.row));
        let on = ix.row == self.dialog.draft_cursor.get();

        // 「✕ 删除草稿」：真按钮（hover / tooltip / 键盘），并**阻断点击冒泡**——
        // 否则删除后还会继续走行的确认（切到已被顶掉位置的另一条草稿）。
        // 已保存残余条目不给删除入口（清理由 `staging_prune_saved` 负责）。
        let del_dialog = self.dialog.clone();
        let del_entity = self.entity.clone();
        let del_shared = self.shared.clone();
        let del_row = ix.row;
        let del_key = row_key.clone();
        let delete_button = move |_: &mut Window, _: &mut App| -> AnyElement {
            let dialog = del_dialog.clone();
            let entity = del_entity.clone();
            let shared = del_shared.clone();
            Button::new(SharedString::from(format!("draft-del-{del_key}")))
                .ghost()
                .icon(IconName::Delete)
                .with_size(Size::XSmall)
                .tooltip("删除这条草稿")
                .on_click(move |_, window, app| {
                    dialog.staging_remove(del_row, window, app);
                    shared.notify_host(app);
                    let _ = entity.update(app, |_, cx| cx.notify());
                    app.stop_propagation();
                })
                .into_any_element()
        };

        let mut item = ListItem::new(ElementId::Name(SharedString::from(format!(
            "draft-{row_key}"
        ))))
        .h(rems(ROW_H))
        .px_2()
        .rounded(rems(0.375))
        .text_xs()
        .child(
            div()
                .h_flex()
                .items_center()
                .gap(rems(0.375))
                .flex_1()
                .min_w(rems(0.))
                .child({
                    // 缩小的类型徽标；无类型信息（旧草稿）回退状态点。
                    match type_badge(&self.dialog.types.borrow()[..], &display_type_id) {
                        Some((icon, _)) => div()
                            .flex_shrink_0()
                            .h_flex()
                            .items_center()
                            .justify_center()
                            .w(rems(crate::ui::ICON_SIZE_MD))
                            .h(rems(crate::ui::ICON_SIZE_MD))
                            .rounded(rems(0.25))
                            .bg(cx.theme().colors.sidebar_accent)
                            .child(icon),
                        None => div()
                            .w(rems(crate::ui::DIALOG_STATUS_DOT_SIZE))
                            .h(rems(crate::ui::DIALOG_STATUS_DOT_SIZE))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(if is_saved {
                                cx.theme().colors.success
                            } else {
                                cx.theme().colors.primary
                            }),
                    }
                })
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(rems(0.25))
                        .flex_1()
                        .min_w(rems(0.))
                        .text_color(if on {
                            cx.theme().colors.foreground
                        } else {
                            cx.theme().colors.muted_foreground
                        })
                        .child(label)
                        .when_some(scope_code, |d, code| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .px_1()
                                    .rounded(rems(crate::ui::DIALOG_CHIP_RADIUS))
                                    .border_1()
                                    .border_color(cx.theme().colors.border)
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(code),
                            )
                        })
                        .when(dirty, |d| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .text_color(cx.theme().colors.warning)
                                    .child("●"),
                            )
                        }),
                ),
        );
        if !is_saved {
            item = item.suffix(delete_button);
        }
        Some(item)
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        // 正常路径不会到这里（删到最后一条会自动补一条空草稿）；真到了也别留空白。
        div()
            .w_full()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child("暂无草稿（点右上「添加」新建一条）")
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.active_index = ix;
        // ↑↓ 即**切换条目**（与容器级 `DraftPrev` / `DraftNext` 同一语义：原型 §2.2 规则 6）。
        // 渲染期同步来的同一行不重复动作（否则会写回表单 → 触发下一轮同步）。
        if let Some(ix) = ix {
            if ix.row != self.dialog.draft_cursor.get() && ix.row < self.len {
                self.dialog.staging_select(ix.row, window, cx);
                self.shared.notify_host(cx);
                let _ = self.entity.update(cx, |_, cx| cx.notify());
            }
        }
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(ix) = self.active_index else {
            return;
        };
        if ix.row >= self.len {
            return;
        }
        // 与点击路径同一个入口：先把当前表单写回草稿，再载入目标草稿（原型 §2.2 规则 1）。
        self.dialog.staging_select(ix.row, window, cx);
        self.shared.notify_host(cx);
        let _ = self.entity.update(cx, |_, cx| cx.notify());
    }
}
