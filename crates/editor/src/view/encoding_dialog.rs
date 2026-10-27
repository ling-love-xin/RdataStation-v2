//! 【2026-10-27】编码选择对话框（状态栏「编码」段点击弹出）
//!
//! 场景：状态栏右段显示 `UTF-8`，点击弹出编码列表——5 种编码单选，当前编码勾选，
//! 点击即切换并关弹层。与函数手册同款结构（组件 `List` + `window.open_dialog`）；
//! 交互：↑↓ 移动选中、Enter / 点击确认、Esc 关闭（组件自带）。
//!
//! 返回 `Entity<ListState<EncodingDelegate>>`：测试经 delegate 直接驱动 `confirm`
//! （headless 点击到不了对话框层，沿用项目既有口径，见 skill 窗口测试一节）。

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::{ActiveTheme, Icon, IndexPath, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::encoding::TextEncoding;
use crate::ui;

/// 编码列表代理：一行 = 编码名（当前编码勾选）
pub struct EncodingDelegate {
    /// 候选（顺序稳定：UTF-8 排最前 = 默认）
    entries: Vec<(String, TextEncoding)>,
    current: TextEncoding,
    selected: Option<IndexPath>,
    /// 宿主切换回调：确认后由它把编码写进文档（`'static` 闭包捕获宿主实体）
    on_pick: Rc<dyn Fn(TextEncoding, &mut Window, &mut App)>,
}

/// 全部候选（与重设菜单旧清单同口径：5 种；UTF-8 默认排首）
pub fn encoding_entries() -> Vec<(String, TextEncoding)> {
    vec![
        ("UTF-8".to_string(), TextEncoding::Utf8),
        ("UTF-8 (BOM)".to_string(), TextEncoding::Utf8Bom),
        ("GBK".to_string(), TextEncoding::Gbk),
        ("UTF-16 LE".to_string(), TextEncoding::Utf16Le),
        ("UTF-16 BE".to_string(), TextEncoding::Utf16Be),
    ]
}

impl ListDelegate for EncodingDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.entries.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let (label, encoding) = self.entries.get(ix.row)?;
        let theme = cx.theme();
        let checked = *encoding == self.current;
        Some(
            ListItem::new(SharedString::from(format!("encoding-{}", label)))
                .h(rems(ui::ENCODING_DIALOG_ROW_HEIGHT))
                .py_0()
                .px_2p5()
                .gap_2()
                // 当前编码右侧勾选标记（主文案左对齐，勾选右对齐）
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .justify_between()
                        .w_full()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.colors.foreground)
                                .child(label.clone()),
                        )
                        .when(checked, |row| {
                            row.child(
                                Icon::new(IconName::Check)
                                    .size_3()
                                    .text_color(theme.colors.success),
                            )
                        }),
                ),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .v_flex()
            .items_center()
            .py_6()
            .text_xs()
            .text_color(theme.colors.muted_foreground)
            .child("（无可用编码）")
            .into_any_element()
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix;
        cx.notify();
    }

    /// Enter / 点击确认：把选中的编码交回宿主（宿主关弹层前已由组件处理）
    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(ix) = self.selected else {
            return;
        };
        let Some((_, encoding)) = self.entries.get(ix.row) else {
            return;
        };
        let encoding = *encoding;
        let on_pick = self.on_pick.clone();
        window.close_dialog(cx);
        on_pick(encoding, window, cx);
    }
}

/// 打开编码选择对话框
///
/// `on_pick`：确认后携带目标编码回调宿主（宿主负责写文档保存格式 + 状态栏刷新）。
/// 返回 `ListState` 实体：测试经它直接驱动确认（headless 口径，见模块头）。
pub fn open_encoding_dialog(
    window: &mut Window,
    cx: &mut App,
    current: TextEncoding,
    on_pick: impl Fn(TextEncoding, &mut Window, &mut App) + 'static,
) -> Entity<ListState<EncodingDelegate>> {
    let delegate = EncodingDelegate {
        entries: encoding_entries(),
        current,
        selected: None,
        on_pick: Rc::new(on_pick),
    };
    let state = cx.new(|cx| {
        ListState::new(delegate, window, cx)
            .searchable(true)
            .selectable(true)
    });
    let list = state.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let theme = cx.theme();
        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("选择操作（搜索编码 / 回车确认 / Esc 关闭）"),
            )
            .child(List::new(&list));
        dialog
            .title("编码")
            .w(theme.font_size * ui::ENCODING_DIALOG_WIDTH)
            .child(body)
    });
    state
}
