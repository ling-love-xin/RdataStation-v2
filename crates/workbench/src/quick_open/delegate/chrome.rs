//! 列表外壳：组头（标题 / 补充 / 计数）、组尾（截断提示）、空态。
//!
//! trait 方法（`render_section_header` 等）留在 `mod.rs` 的唯一 `ListDelegate` impl 块里
//! （trait impl 不可拆多块），这里提供渲染实现供其转发。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::empty::{
    Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant, EmptyTitle,
};
use gpui_kit::component::{Icon, IconName, Theme};
use gpui_kit::*;

use crate::quick_open::model::Mode;
use crate::ui;

use super::QuickOpenDelegate;

impl QuickOpenDelegate {
    /// 组头：标题 + note（info 色「搜索中…」）+ 计数。
    pub(super) fn section_header_element(
        &self,
        section: usize,
        theme: &Theme,
        pt: &settings::product_tokens::ProductTokens,
    ) -> Option<Div> {
        let group = self.groups.get(section)?;
        let mut head = div()
            .h_flex()
            .items_center()
            .gap_1()
            .h(rems(ui::QUICK_OPEN_GROUP_HEADER_HEIGHT))
            .px_2()
            .text_xs()
            // 组头文字取 `foreground` 而非 `muted_foreground`：`muted_foreground` 压在
            // `quick_open.group.header` 底上只有 light 4.32:1 / dark 3.16:1，不到 AA 的 4.5。
            // 不动底色（改底色会让 dark 与 background 只剩 1.06:1 落差、分区感消失），
            // 弱化层级改由 `text_xs` + 底色承担 —— 实测 light 10.69:1 / dark 6.8:1。
            .text_color(theme.colors.foreground)
            .bg(pt.quick_open_group_header(theme))
            .child(group.title);
        // 「搜索中…」等补充：与计数区分开色（info），一眼看出“还没搜完”而不是“没结果”
        if !group.note.is_empty() {
            head = head.child(
                div()
                    .text_color(theme.colors.info)
                    .child(group.note.clone()),
            );
        }
        Some(head.child(div().ml_auto().child(format!("{} 条", group.rows.len()))))
    }

    /// 组尾：被截掉的条数（**不静默丢**：告诉用户「还有，但没全显示」）。
    pub(super) fn section_footer_element(&self, section: usize, theme: &Theme) -> Option<Div> {
        let group = self.groups.get(section)?;
        if group.hidden == 0 {
            return None;
        }
        Some(
            div()
                .h_flex()
                .items_center()
                .h(rems(ui::QUICK_OPEN_GROUP_HEADER_HEIGHT))
                .px_2()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(format!("还有 {} 条（继续输入缩小范围）", group.hidden)),
        )
    }

    /// 空态：按档位给「下一步能做什么」的文案。
    ///
    /// 走标准组件（`gpui-kit-dev` 选型表：空搜索结果 → `empty::Empty`），不手搓居中列。
    pub(super) fn empty_element(&self) -> Empty {
        let (title, hint) = match self.mode {
            Mode::Command => ("无匹配命令", "删掉 > 前缀回到默认搜索"),
            Mode::FullText => (
                "无匹配全文命中",
                "元数据可能没有注释文本；试试名称搜索，或去掉 # 前缀",
            ),
            Mode::Goto => ("无匹配对象", "试试输入表名 / 视图名 / 例程名 / 序列名"),
            Mode::Default => ("无匹配结果", "试试输入 > 搜命令 · # 搜元数据全文"),
        };
        Empty::new().header(
            EmptyHeader::new()
                .media(
                    EmptyMedia::new()
                        .with_variant(EmptyMediaVariant::Icon)
                        .child(Icon::new(IconName::Search).size_4()),
                )
                .title(EmptyTitle::new().child(title))
                .description(EmptyDescription::new().child(hint)),
        )
    }
}
