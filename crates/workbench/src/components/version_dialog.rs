//! 「关于 / 版本信息」对话框。
//!
//! 目的单一：真机验证前一眼确认**验证对象是谁**——
//! 这个 exe 对应哪次提交、工作树是否被改过、关键依赖锁在什么版本。
//! 数据全部来自构建期注入（见 `crate::version` 与 `crates/workbench/build.rs`），
//! 运行时不做任何推断。
//!
//! 版式（2026-11-22 按原型定稿）：
//! - 弹窗宽 **560**（`Dialog` 默认 448，必须显式给宽，改内容容器上限没有用）；
//! - `DescriptionList` 用**默认 horizontal**（label 120px + 值一行）。
//!   `vertical()` 会把每项拆成"带底色的 label 行 + 值行"两行，9 项就是 18 行，
//!   必然撑破对话框高度、触发内部滚动——这是本文件改版时踩过的坑；
//! - 脏树用 `Tag`（一眼可判）+ `Alert`（替代裸橙色文字）；
//! - 身份 5 项 / 构建 4 项用 `Separator` 分组；
//! - 「复制版本信息」是主操作（放右侧、primary 变体），「关闭」是次操作。
//!
//! 入口：状态栏右端常驻标识（`view::actions::render_status_bar`）。

use shared::i18n::tr;
use gpui_kit::assets::IconName;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::tag::Tag;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClipboardItem, ParentElement as _, SharedString, Styled as _, Window, div, px,
};

use crate::components::dialog_helpers::{standard_dialog_body, standard_dialog_footer};
use crate::version;

/// label 列宽（`DescriptionList` horizontal 的默认值，显式写出便于对齐其它弹窗）。
const LABEL_WIDTH: f32 = 120.0;

/// 打开版本信息对话框。
pub fn open_version_dialog(window: &mut Window, cx: &mut App) {
    // 构建期常量，取一次即可；build 闭包可能随 dialog 层重建而重跑，
    // 故把值先移进闭包，闭包内只做只读访问。
    let info = version::info();
    window.open_dialog(cx, move |dialog, _window, cx| {
        let colors = cx.theme().colors;

        // 顶部：版本标识（不含 -dirty 后缀）+ 状态徽标，后缀由徽标表达，避免读两遍。
        let status_tag = match info.dirty {
            Some(true) => Tag::danger().outline().child("DIRTY"),
            Some(false) => Tag::success().outline().child("CLEAN"),
            None => Tag::secondary().outline().child("UNKNOWN"),
        };
        let headline = div()
            .h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .child(div().text_lg().child(info.tag_base()))
            .child(status_tag);

        // 脏树警示：用标准 Alert（icon + 半透色底 + 同色描边），不再手搓彩色文字。
        let dirty_alert = Alert::warning(
            "version-tree-dirty",
            "这个构建不对应任何一次提交。据此得出的验证结论必须标注 -dirty，或重新编译后再验。",
        )
        .title(tr("工作树有未提交改动"));

        // 身份 5 项 + 构建 4 项：默认 horizontal 布局，一项一行。
        let mut identity = DescriptionList::new()
            .label_width(px(LABEL_WIDTH))
            .columns(1);
        for (label, value) in [
            ("版本标识", info.tag()),
            ("版本号", info.version.to_string()),
            ("提交", format!("{} · {}", info.commit, info.branch)),
            ("工作树", info.tree_text()),
            ("提交时间", info.commit_time_text()),
        ] {
            identity = identity.item(label, value, 1);
        }

        let mut build = DescriptionList::new()
            .label_width(px(LABEL_WIDTH))
            .columns(1);
        for (label, value) in [
            ("构建档", info.profile.to_string()),
            ("目标平台", info.target.to_string()),
            ("SQL 排版引擎", format!("lax-sql {}", info.lax_sql)),
            ("UI 组件库", format!("gpui-kit {}", info.gpui_kit)),
        ] {
            build = build.item(label, value, 1);
        }

        let body = div()
            .v_flex()
            .w_full()
            .gap_2()
            .child(headline)
            .when(info.dirty == Some(true), |this| this.child(dirty_alert))
            .child(identity)
            .child(Separator::horizontal_dashed().label(tr("构建与依赖")))
            .child(build)
            .child(div().text_xs().text_color(colors.muted_foreground).child(
                "核对只需两行：提交、工作树。要贴给别人或记进验证记录，点右下角「复制版本信息」。",
            ));

        // 复制是主动作：直接写系统剪贴板（与 `Clipboard` 组件内部同一路径），
        // 但需要文字标签，故用标准 Button 而不是只有图标的 `Clipboard`。
        let copy_text: SharedString = info.clipboard_text().into();
        let copy = Button::new("version-copy")
            .with_variant(ButtonVariant::Primary)
            .icon(IconName::Copy)
            .label(tr("复制版本信息"))
            .on_click(move |_, _window, app| {
                app.write_to_clipboard(ClipboardItem::new_string(copy_text.to_string()));
            });

        let close = Button::new("version-close")
            .label(tr("关闭"))
            .on_click(|_, window, cx| window.close_dialog(cx));

        dialog
            .title(tr("关于 RdataStation"))
            .w(px(560.))
            .child(standard_dialog_body(body))
            .footer(standard_dialog_footer(copy, Some(close)))
    });
}
