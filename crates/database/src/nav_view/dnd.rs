//! 导航面板的拖拽载荷、落点与拖拽幽灵。
//!
//! 载荷是**跨 crate 的公开类型**（编辑区要认 `NavDragPayload` 才能接住表 / 视图拖拽），
//! 所以在根模块 `pub use` 回去，外部路径 `database::nav_view::NavDragPayload` 不变。

use super::*;

/// 数据源导航 → 编辑区拖拽载荷（仅表 / 视图行携带）。
///
/// 载荷只带**限定名 + 展示文案**：拖拽本身不承诺语义，落点决定动作
/// （SQL 区插到光标处 / 编辑区其它位置追加），因此不携带连接句柄或执行计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavDragPayload {
    /// 已去重的限定名（`db.table` / `schema.table`）。
    pub qualified: String,
    /// 展示文案（拖拽幽灵 + 落点通知）。
    pub label: String,
}

/// 拖拽幽灵：跟随光标的轻量预览（不参与命中测试）。
pub(super) struct NavDragGhost {
    pub(super) label: String,
    /// 类别图标路径，与树内节点同形（`nav_kind_icon`）——颜色不给身份，故幽灵也用中性色。
    pub(super) icon: &'static str,
}

impl Render for NavDragGhost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .h(rems(1.375))
            .px_2()
            .rounded_md()
            .bg(cx.theme().colors.popover)
            .border_1()
            .border_color(cx.theme().colors.border)
            .shadow_md()
            .child(nav_icon(
                self.icon,
                ui::ICON_SIZE_SM,
                cx.theme().colors.muted_foreground,
            ))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.foreground)
                    .child(self.label.clone()),
            )
    }
}

/// 连接行拖拽载荷（归组与组内排序共用）。
///
/// 与表 / 视图的拖拽载荷是**不同类型**：两类落点的 `on_drop` 只认自己的类型，
/// 拖到不相干的落点上自然什么都不发生（回调里无需再判类型）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavConnDragPayload {
    /// 被拖动的连接 ID。
    pub conn_id: String,
    /// 展示名（拖拽幽灵 + 落点通知）。
    pub name: String,
}

/// 连接拖拽的落点语义。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ConnDropTarget {
    /// 落到分组头 / 未分组头：只处理归属（已在组内时不改位置）。
    Container,
    /// 落到某一行：归组 + 插到该行之前（落到自己身上 = 位置不变）。
    BeforeRow(String),
}

/// 分组头拖拽载荷（分组之间的排序）。
///
/// 与连接载荷分开：拖**分组**到分组头 = 给分组排序，拖**连接**到分组头 = 归组，
/// 两者落在同一个元素上但语义不同，用载荷类型区分最省事。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavGroupDragPayload {
    /// 被拖动的分组 ID。
    pub group_id: String,
    /// 展示名（拖拽幽灵 + 通知）。
    pub name: String,
}
