//! 图标与辅助按钮（item_icon_row / small_icon_button / editor_read_only）
//!
//! 自 host/toolbar.rs 拆分（2026-10-29 Step 8）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use gpui_kit::assets::IconName;

impl EditorHostPanel {
    pub(crate) fn item_icon_row(
        &self,
        id: crate::toolbar::ToolbarItemId,
        cx: &gpui_kit::App,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let key = format!("editor-toolbar-customize-icon-{:?}", id);
        div()
            .w_4()
            .h_flex()
            .items_center()
            .justify_center()
            .debug_selector(move || key.clone())
            .child(Icon::new(id.icon()).text_color(muted))
    }

    /// 【T2c-5】面板里的小图标按钮（↑↓ / ✕ 用；统一 ghost + small + 图标）
    ///
    /// 为什么显式传 `entity` 而不是 `cx.entity()`：Popover content 闭包里的 `cx` 是
    /// `Context<PopoverState>`，拿不到面板实体——由调用方把捕获的 `entity` 传进来，
    /// 动作统一 `entity.update` 转交面板方法。
    pub(crate) fn small_icon_button(
        &self,
        entity: &Entity<Self>,
        id: impl Into<gpui_kit::ElementId>,
        icon: IconName,
        enabled: bool,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Button {
        let entity = entity.clone();
        Button::new(id)
            .ghost()
            .small()
            .icon(icon)
            .disabled(!enabled)
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| action(panel, cx));
            })
    }

    pub(crate) fn editor_read_only(&self) -> bool {
        // 用户只读开关（roLock）与文档只读合并：任一为只读 → 编辑器只读
        self.user_ro_lock
            || self
                .with_document(|doc| !doc.read_only().can_edit())
                .unwrap_or(true)
    }

}
