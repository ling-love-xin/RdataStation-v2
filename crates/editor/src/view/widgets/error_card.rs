//! 结果区的错误卡片（原型 §2.4「错误呈现」）
//!
//! 失败不留一句灰字：摘要（驱动的原话）+ 两个动作——**定位到第 N 行**（这次认得出位置时才有）
//! 与**复制**（把原文复制走，便于搜索 / 报 bug）。
//!
//! 为什么用组件库的 `Alert` 而不是自己描边：`danger` 描边、配色、圆角、图标都是它的现成能力
//! （「组件优先」）。`Alert` 没有子槽位，所以动作按钮放在下面一行——形状仍是原型里那张卡片。
//!
//! 认不出位置就**不摆**定位按钮：摆了却按不动（或者按了跳错地方）比没有更糟。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::alert::Alert;
use gpui_kit::*;

/// 卡片内容（都是真实值）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorCard {
    /// 错误摘要（驱动给的原文；状态栏也有一份，这里是要留住的那份）
    pub message: String,
    /// 认出位置时的文案（`第 3 行`）；`None` = 只提示不定位
    pub location: Option<String>,
}

/// 卡片上的动作（由面板构造：它知道点击该干什么）
#[derive(Default)]
pub struct ErrorCardControls {
    /// 「定位到第 N 行」
    pub locate: Option<AnyElement>,
    /// 「复制」错误原文
    pub copy: Option<AnyElement>,
}

/// 定位按钮的文案（原型 §2.4 的「定位到第 N 行」）
pub fn locate_label(location: &str) -> String {
    format!("定位到{location}")
}

/// 画一张错误卡片
pub fn render(card: &ErrorCard, controls: ErrorCardControls, cx: &App) -> impl IntoElement {
    let muted = cx.theme().colors.muted_foreground;

    div()
        .v_flex()
        .gap_1()
        .p_2()
        // 测试按选择器断言“失败时有卡片、成功时没有”
        .debug_selector(|| "editor-result-error-card".to_string())
        .child(
            Alert::error(
                "editor-result-error",
                SharedString::from(card.message.clone()),
            )
            .small(),
        )
        .child(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .text_xs()
                .text_color(muted)
                // 动作按原型的顺序：定位在前、复制在后
                .children(controls.locate)
                .children(controls.copy),
        )
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{ErrorCard, locate_label};

    #[test]
    fn locate_button_says_where_it_goes() {
        assert_eq!(locate_label("第 3 行"), "定位到第 3 行");
    }

    /// 认不出位置时不摆按钮（按不动 / 跳错地方比没有更糟）
    #[test]
    fn a_card_without_a_location_has_no_locate_action() {
        let card = ErrorCard {
            message: "Query failed: connection reset by peer".to_string(),
            location: None,
        };
        assert!(card.location.is_none());
    }
}
