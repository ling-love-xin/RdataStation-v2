//! 面板头（对齐 EDA 原型右栏 dock-head + right-toolbar）与无项目提示。
//!
//! 两个入口都只**发意图**：⚙ 交给规则对话框自己去取数（`jobs::attach_rules` 那条路径），
//! ⟳ 走 [`InsightView::reload`] 按当前目标重发请求（渲染路径零 I/O，D20）。
//!
//! 工具栏对齐原型（`insight-eda-prototype.html` `.right-toolbar`）：
//! **规则管理 / 重算画像 / 全量统计 [spacer] ＋新建洞察**——新建洞察固定在**最右**
//! （2026-10-30 用户拍板按原型，推翻此前「设置最右」的临时布局）。
//! 目标名 / 类型徽标 / 取样口径等不再由独立目标头承担：选中目标的信息并入
//! 下钻区 `drill.rs` 的 dz-title 与摘要卡（右栏已无 Tab 条与目标头）。

use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::toolbar::Toolbar;
use gpui_kit::component::{Icon, IconName, Theme};
use gpui_kit::*;

use crate::insight_view::{InsightEvent, InsightView};
use crate::InsightTarget;
use crate::ui;

impl InsightView {
    pub(crate) fn render_header(&self, entity: &Entity<Self>, theme: &Theme) -> Div {
        let colors = theme.colors;
        div()
            .flex_none()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .px(rems(ui::PANEL_PADDING))
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(colors.border)
            // 左：面板标题（dock-head 语义：标题是面板身份，不进 Toolbar——
            // Toolbar 是按钮行容器，不承担文字标题）
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child("洞察"),
            )
            .child(div().flex_1())
            // 右：操作按钮（对齐 EDA 原型 right-toolbar）。顺序 = 规则管理 →
            // 重算画像 → 全量统计 → spacer → ＋新建洞察（最右）。
            // 「刷新」按钮原型有、真机不加：洞察表集合是宿主推送的会话快照
            // （set_insight_set），无本地缓存需要手动刷新，加了就是死按钮。
            .child(
                Toolbar::new("insight-header-toolbar")
                    .child(
                        Button::new("insight-rules")
                            .ghost()
                            .icon(IconName::Settings)
                            .disabled(!self.project_open)
                            .tooltip(if self.project_open {
                                "规则管理"
                            } else {
                                "规则管理（需先打开项目）"
                            })
                            .on_click({
                                let rules = self.rules.clone();
                                move |_, window, app| {
                                    rules.update(app, |view, cx| view.begin_load(window, cx));
                                }
                            }),
                    )
                    .child(
                        Button::new("insight-refresh")
                            .ghost()
                            .icon(IconName::RotateCw)
                            .tooltip("重算画像")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |view, cx| view.reload(cx));
                                }
                            }),
                    )
                    // ⧉ 全量统计：源表/源列目标才有「取样」概念（其余目标数据已是
                    // 既定内容，request_full 内部也会忽略）；非源目标置灰并给说明。
                    .child({
                        let can_full = matches!(
                            self.target,
                            Some(InsightTarget::SourceTable { .. })
                                | Some(InsightTarget::SourceColumn { .. })
                        );
                        Button::new("insight-full")
                            .ghost()
                            // component::IconName 只是兼容子集（无 Expand）；
                            // 全量 Lucide 目录在 gpui_kit::assets::IconName（中间页同款）
                            .icon(gpui_kit::assets::IconName::Expand)
                            .disabled(!can_full)
                            .tooltip(if can_full {
                                "全量统计（大表全量重算，可能较慢）"
                            } else {
                                "全量统计（仅源表 / 源列目标可用）"
                            })
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |view, cx| view.request_full(cx));
                                }
                            })
                    })
                    // spacer：把「新建洞察」推到工具栏最右（原型 `.tb-btn.spacer`）
                    .content(div().flex_1())
                    .child(
                        Button::new("insight-new")
                            .ghost()
                            .icon(IconName::Plus)
                            .tooltip("新建洞察")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, app| {
                                    entity.update(app, |_view, cx| {
                                        cx.emit(InsightEvent::NewInsightRequested);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
    }

    /// 无项目提示：画像仍可算（临时表在内存里），但依赖项目目录的能力不可用。
    /// 信息条语义：前置 Info 图标 + muted 文案（对齐产品提示语言，不只是孤零零一行字）
    pub(crate) fn render_project_hint(&self, theme: &Theme) -> Option<Div> {
        if self.project_open {
            return None;
        }
        Some(
            div()
                .flex_none()
                .w_full()
                .px(rems(ui::PANEL_PADDING))
                .py_1()
                .h_flex()
                .items_center()
                .gap_1()
                .child(
                    Icon::new(IconName::Info)
                        .size(rems(ui::INSIGHT_INLINE_ICON_SIZE))
                        .text_color(theme.colors.muted_foreground),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child("未打开项目：画像可用，规则管理与快照不可用"),
                ),
        )
    }
}
