//! 「关于 / 版本信息」对话框。
//!
//! 目的单一：真机验证前一眼确认**验证对象是谁**——
//! 这个 exe 对应哪次提交、工作树是否被改过、关键依赖锁在什么版本。
//! 数据全部来自构建期注入（见 `crate::version` 与 `crates/workbench/build.rs`），
//! 运行时不做任何推断。
//!
//! 版式（2026-11-22 定稿，2026-11-25 扩字段）：
//! - 弹窗宽 **560**（`Dialog` 默认 448，必须显式给宽，改内容容器上限没有用）；
//! - `DescriptionList` 用**默认 horizontal**（label 120px + 值一行）。
//!   `vertical()` 会把每项拆成"带底色的 label 行 + 值行"两行，14 项就是 28 行，
//!   必然撑破对话框高度、触发内部滚动——这是本文件改版时踩过的坑；
//! - 明细共 14 项：身份 5 项 + 构建与依赖 9 项（后者补 sqlglot / duckdb / sqlite /
//!   gpui-fast / fake，见 `docs/architecture/ui/version-info-page-design.md`）；
//!   顺序由 `version::BuildInfo::rows()` 单一提供，本文件只负责切分与渲染；
//! - 脏树用 `Tag`（一眼可判）+ `Alert`（替代裸橙色文字）；
//! - 「复制版本信息」是主操作（放右侧、primary 变体），「关闭」是次操作。
//!
//! 入口：状态栏右端常驻标识（`view::actions::render_status_bar`）。

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
use shared::i18n::tr;

use crate::components::dialog_helpers::{standard_dialog_body, standard_dialog_footer};
use crate::version;

/// 弹窗宽度。`Dialog` 默认 448，14 项明细在这个宽度下会让引擎类值折成三行。
const WIDTH: f32 = 560.0;

/// label 列宽（`DescriptionList` horizontal 的默认值，显式写出便于对齐其它弹窗）。
const LABEL_WIDTH: f32 = 120.0;

/// 明细里「身份」占前几项，其余归入「构建与依赖」。
///
/// 与 `version::BuildInfo::rows()` 的顺序绑定：前 5 项身份，后 9 项构建与依赖。
/// 增删字段时两处一起改（`version.rs` 的单测会卡住 `rows.len()`）。
const IDENTITY_ROWS: usize = 5;

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

        // 14 项明细：前 5 项身份、后 9 项构建与依赖。顺序与文案由 `rows()` 统一给出，
        // 这里只做切分与渲染，避免弹窗与剪贴板/单测三处各写一份字段清单。
        let rows = info.rows();
        let mut identity = DescriptionList::new()
            .label_width(px(LABEL_WIDTH))
            .columns(1);
        for (label, value) in rows.iter().take(IDENTITY_ROWS) {
            identity = identity.item(*label, value.clone(), 1);
        }

        let mut build = DescriptionList::new()
            .label_width(px(LABEL_WIDTH))
            .columns(1);
        for (label, value) in rows.iter().skip(IDENTITY_ROWS) {
            build = build.item(*label, value.clone(), 1);
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
            .w(px(WIDTH))
            .child(standard_dialog_body(body))
            .footer(standard_dialog_footer(copy, Some(close)))
    });
}

#[cfg(test)]
mod tests {
    use super::{IDENTITY_ROWS, LABEL_WIDTH, WIDTH};
    use crate::version;

    /// 尺寸与切分点是「关于」弹窗的版式契约：改宽、挪字段、重排 `rows()` 都会先撞这里。
    ///
    /// 560 不是随手取的：14 项 × 约 28px 行高 + 头部 + 页脚，1280×720 屏一屏放得下；
    /// 缩回默认 448 会让 `DuckDB 1.5.6（crate 1.10506.1）` 这类双包值折行。
    #[test]
    fn dialog_layout_is_pinned() {
        assert_eq!(WIDTH, 560.0);
        assert_eq!(LABEL_WIDTH, 120.0);
        assert_eq!(IDENTITY_ROWS, 5);

        // 切分点必须真的落在身份段之后，否则「构建与依赖」段会从头混进身份字段。
        let rows = version::info().rows();
        assert_eq!(rows.len(), 14);
        assert_eq!(rows[IDENTITY_ROWS - 1].0, "提交时间");
        assert_eq!(rows[IDENTITY_ROWS].0, "构建档");
    }
}
