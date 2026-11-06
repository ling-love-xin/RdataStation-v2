//! 存档详情（M6 自持视图）：右侧属性面板的内容层。
//!
//! 与面板同一纪律：**纯渲染 + 纯数据**——取值全部来自宿主推来的 [`ArchiveDetail`]（已格式化），
//! render 期零 I/O、零计算。
//!
//! 本批做**只读信息区**（头部 / 基本信息 / 来源 / 版本 / 标签与分组）+ **动作区**（打开（只读）/ 取回（检出）…）
//! + **危险区**（移入回收站）。动作按钮一律经宿主端口（`DetailActions`），crate 不认识服务层。

pub(crate) use gpui_kit::base::{Disableable as _, StyledExt};
pub(crate) use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
pub(crate) use gpui_kit::component::description_list::{DescriptionList, DescriptionText};
pub(crate) use gpui_kit::component::hover_card::HoverCard;
pub(crate) use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
pub(crate) use gpui_kit::component::tag::Tag;
pub(crate) use gpui_kit::component::{ActiveTheme, Icon, Sizable as _, Size};
pub(crate) use gpui_kit::*;

pub(crate) use crate::model::{ArchiveKind, ArchiveStatus, DeleteTarget, PayloadMode, RowScope};
pub(crate) use crate::resource_view::{BadgeTone, ResourcesHost, badge_tone, kind_icon, strength_badge};
pub(crate) use crate::ui;

/// 指纹展示长度（前 12 位：足够比对，又不至于把面板撑爆）。
pub const HASH_PREVIEW_LEN: usize = 12;

/// 「内容指纹」那一行的标签文案。
///
/// 渲染层靠它认出“哪一行可以复制”（复制的是**完整指纹**，展示的仍是缩略）：
/// 两处共用同一个常量，改文案不会把复制入口改丢。
pub const HASH_LABEL: &str = "内容指纹";

/// 「版本」分区的标题（渲染层靠它认出“这个分区要多一个『查看全部…』入口”）。
pub const VERSION_SECTION_TITLE: &str = "版本";

/// 「组织」分区的标题（渲染层靠它认出“这个分区下面挂『移动…』分组入口”）。
pub const ORG_SECTION_TITLE: &str = "组织";

/// 「内容预览」分区的标题（原型 §3.1；渲染层与窗口测试共用一处）。
pub const PREVIEW_SECTION_TITLE: &str = "内容预览";

pub use crate::view_model::detail::{ArchiveDetail, ArchiveTagChip, DetailGroupOption, DetailVersionLine};

/// 指纹缩略：前 [`HASH_PREVIEW_LEN`] 位；无指纹（旧行 / `table_ref`）给破折号而不是空白。
pub fn short_hash(hash: Option<&str>) -> String {
    match hash {
        Some(value) if !value.is_empty() => value.chars().take(HASH_PREVIEW_LEN).collect(),
        _ => "—".to_string(),
    }
}

/// 种类中文名（面板与详情共用一处，避免两处各写一份）。
pub fn kind_label(kind: ArchiveKind) -> &'static str {
    match kind {
        ArchiveKind::File => "受管文件",
        ArchiveKind::Analysis => "分析表",
        ArchiveKind::TableRef => "远端引用",
    }
}

/// 详情面板的动作接线（`None` = 纯只读渲染：crate 单测与无宿主场景）。
///
/// 动作一律经宿主端口（与列表右键菜单同一套）：面板不认识服务层，也不自己取数。
#[derive(Clone)]
pub struct DetailActions {
    pub host: std::rc::Rc<dyn ResourcesHost>,
    /// 项目只读：写类动作一律禁用（与右键菜单同一判据）。
    pub read_only: bool,
    /// 面板当前选集（多选时整集，单选时含焦点行）；空 = 只有这一条详情。
    ///
    /// 详情面板只展示焦点行，但「移动…」这类批量动作要动整集——与右键菜单
    /// 「移动到分组 ›」同一口径（`list_delegate/mod.rs` 的 `selection_ids`）。
    pub selected_ids: Vec<String>,
}


mod actions;
mod banners;
mod body_sections;
mod header;
mod rows;
mod tag_preview;

pub use self::rows::{alert_line, detail_rows};
pub use self::tag_preview::render_preview_section;
pub(crate) use self::tag_preview::{alias_line, name_link, render_tag_section};

pub fn render_detail(detail: &ArchiveDetail, actions: Option<DetailActions>, cx: &App) -> Div {
    let mut body = div().v_flex().w_full().gap_2().p_2();
    body = body.child(header::render_header(detail, actions.as_ref(), cx));
    for element in banners::render_banners(detail, actions.as_ref(), cx) {
        body = body.child(element);
    }
    for element in body_sections::render_body_sections(detail, actions.as_ref(), cx) {
        body = body.child(element);
    }
    for element in actions::render_actions(detail, actions.as_ref(), cx) {
        body = body.child(element);
    }

    div()
        .v_flex()
        .w_full()
        .min_h_0()
        .border_l(ui::HAIRLINE)
        .border_color(cx.theme().colors.border)
        .bg(cx.theme().colors.background)
        .child(body)
}

#[cfg(test)]
mod tests;
