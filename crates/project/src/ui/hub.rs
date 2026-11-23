//! 项目中心：**任何时候都能打开**的项目列表 + 管理覆盖层。
//!
//! 与无项目态的选择器（`workbench` 的「项目」tab）共用同一份
//! [`render_picker`](super::render_picker)——列表 / 搜索 / 筛选 / 卡片菜单都不另写一套。
//! 区别只有三点：
//! - 有当前项目时也能打开（不关闭当前会话，只盖一层覆盖层，Esc 由关闭按钮退出）；
//! - 当前项目在卡片上带「当前」标记，点它不会重新打开（避免白丢锁）；
//! - 点别的项目＝[`request_switch`](super::request_switch)（先释放当前项目再打开）。
//!
//! 为什么不做成 Dock 中央常驻 tab：那会让「项目」tab 在每次打开项目后都占着中央区，
//! 与编辑器 tab 争位置；覆盖层是「用的时候才出现」，且不动 Dock 装配逻辑。

use super::*;

/// 打开项目中心（不关闭当前项目）。
///
/// 列表可能是上次留下的旧快照：每次打开都重查一次（事件路径，render 不做 I/O）。
pub fn open_project_hub(host: &ProjectUiHost, cx: &mut App) {
    host.state.borrow_mut().hub_open = true;
    refresh_picker(host, cx);
}

/// 关闭项目中心。
pub fn close_project_hub(host: &ProjectUiHost, cx: &mut App) {
    host.state.borrow_mut().hub_open = false;
    host.notify(cx);
}

/// 项目中心覆盖层：无当前项目、或用户没打开时返回 `None`。
///
/// 无项目态不需要它——中央「项目」tab 本身就是完整的选择器。
pub fn render_project_hub(
    host: &ProjectUiHost,
    inputs: &ProjectInputs,
    cx: &mut App,
) -> Option<Div> {
    if !host.state.borrow().hub_open {
        return None;
    }
    let current = host.current()?;
    let theme = cx.theme();
    let host_close = host.clone();
    Some(
        div()
            .absolute()
            .inset_0()
            .v_flex()
            .bg(theme.colors.background)
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.colors.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("项目中心"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child(format!("当前：{}", current.name)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("点「打开」切换到该项目；离开前会先拦截未保存草稿"),
                    )
                    .child(
                        div().ml_auto().child(
                            Button::new("project-hub-close")
                                .ghost()
                                .small()
                                .icon(IconName::Close)
                                .label("关闭")
                                .on_click(move |_, _, cx| close_project_hub(&host_close, cx)),
                        ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(render_picker(host, inputs, cx)),
            ),
    )
}
