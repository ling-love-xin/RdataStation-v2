//! 行内容渲染：标题命中高亮、内容档 snippet、why 与次级动作徽章。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::Theme;
use gpui_kit::*;

use crate::quick_open::model::{Mode, Row, markup_segments, match_span};
use crate::ui;

use super::QuickOpenDelegate;

/// 统一徽章：行内 `why`、`⌥↵` 次级提示与浮层的档位 chip 共用一套样式
/// （`border + px_1 + rounded_sm + text_xs + muted`）——三处三种做法的时代结束了。
pub(crate) fn badge(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .flex_none()
        .rounded_sm()
        .border_1()
        .border_color(theme.colors.border)
        .px_1()
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(text.into())
}

impl QuickOpenDelegate {
    /// 行元素：类型标签 + 标题（命中高亮）+ 「为什么命中」（内容档）+ 右侧归属；
    /// 内容档多一行 snippet（含命中标记）。
    ///
    /// 高度按**档**取（不是按行）：`List` 只量一个样本行、要求同行同高；
    /// 内容档全部行都带 snippet，整档切两行高。
    pub(super) fn row_element(
        &self,
        row: &Row,
        theme: &Theme,
        pt: &settings::product_tokens::ProductTokens,
    ) -> ListItem {
        let muted = theme.colors.muted_foreground;
        let two_line = self.mode == Mode::FullText;
        let height = if two_line {
            ui::QUICK_OPEN_ROW_HEIGHT_FULLTEXT
        } else {
            ui::ROW_HEIGHT
        };

        let mut first = div().h_flex().items_center().w_full().min_w_0().gap_2();
        first = first.child(
            div()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(row.kind.label()),
        );
        first = first.child(self.title_element(row, theme, pt));
        if let Some(why) = row.why {
            first = first.child(badge(why, theme));
        }
        first = first.child(
            div()
                .ml_auto()
                .flex_none()
                .text_xs()
                .text_color(muted)
                .child(row.secondary.clone()),
        );
        // 「次级动作」是**选中行专属**的键位提示：只在可定位 / 带次级动作的行 + 它是当前选中时出现。
        // 不做成行内按钮：`List` 自己管指针与选中，行内可点元素会和它的确认抢事件。
        // 默认档 ⌥↵ = 在树中定位；Goto 档主动作已是定位，⌥↵ = 打开属性面板。
        if (row.locate.is_some() || row.secondary_action.is_some())
            && self.selected_key.as_deref() == Some(row.key.as_str())
        {
            let hint = match self.mode {
                Mode::Goto => "⌥↵ 属性",
                _ => "⌥↵ 定位",
            };
            first = first.child(badge(hint, theme));
        }

        let mut content = div().v_flex().w_full().min_w_0().gap_1().child(first);
        if two_line {
            content = content.child(self.snippet_element(row, theme, pt, muted));
        }

        ListItem::new(SharedString::from(format!("qo-row-{}", row.key)))
            .h(rems(height))
            .py_0()
            .px_2()
            .gap_2()
            .text_xs()
            .child(content)
    }

    /// 标题（含命中高亮）。
    fn title_element(
        &self,
        row: &Row,
        theme: &Theme,
        pt: &settings::product_tokens::ProductTokens,
    ) -> Div {
        let mut title = div()
            .h_flex()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .text_color(theme.colors.foreground)
            .text_xs();
        match match_span(&row.title, &self.needle) {
            Some((start, end)) if end > start => {
                title = title
                    .child(row.title[..start].to_string())
                    .child(
                        div()
                            .rounded_sm()
                            .bg(pt.search_match_background(theme))
                            .child(row.title[start..end].to_string()),
                    )
                    .child(row.title[end..].to_string());
            }
            _ => title = title.child(row.title.clone()),
        }
        title
    }

    /// 第二行：snippet（`<mark>` 切段上色；名称档没有这一行）。
    fn snippet_element(
        &self,
        row: &Row,
        theme: &Theme,
        pt: &settings::product_tokens::ProductTokens,
        muted: Hsla,
    ) -> Div {
        let mut line = div()
            .h_flex()
            .items_center()
            .min_w_0()
            .overflow_hidden()
            .text_xs()
            .text_color(muted);
        if let Some(markup) = row.snippet.as_deref() {
            for (text, hit) in markup_segments(markup) {
                line = if hit {
                    line.child(
                        div()
                            .rounded_sm()
                            .bg(pt.search_match_background(theme))
                            .child(text),
                    )
                } else {
                    line.child(text)
                };
            }
        }
        line
    }
}
