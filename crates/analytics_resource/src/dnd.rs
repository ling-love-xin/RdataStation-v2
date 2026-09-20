//! 行拖拽：载荷、拖拽幽灵与落点语义（原型 §3.2：**拖动行到分组头 = 移动**）。
//!
//! 与 M4 导航的行拖拽同一形态（`database/src/nav_view/dnd.rs`）：载荷带**一批 id**，
//! 落点决定动作，拖拽本身不承诺语义（所以载荷里没有「动作」字段）。
//!
//! 本模块只表达「拖的是什么、能落在哪」，真正的改动走宿主端口
//! （`ResourcesHost::request_move_to_group`）——面板不碰库，与右键菜单那条路径**同一出口**：
//! 拖拽是第二条入口，不是第二套实现。
//!
//! 与导航的两处差别（有意为之）：
//! 1. 落点**只有分组头**，没有「拖到某一行之前」——资产库没有手工排序（排序键就是那五列），
//!    拖到行上会承诺一个我们做不到的语义；
//! 2. 「全部分组」头**不是落点**：它是聚合头（下挂未分组 + 各分组），
//!    落上去「移出分组」与「什么都不做」都说得通，索性不接。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::*;

use crate::filter::{GROUP_ALL, GROUP_UNGROUPED};
use crate::model::ArchiveKind;
use crate::resource_view::{ArchiveRow, kind_icon};

/// 存档行拖拽载荷（**一批**：多选时是整选集，单选就是那一条）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveDragPayload {
    /// 被拖动的存档 id（面板按可见行顺序给，回执与测试都要确定性）。
    pub ids: Vec<String>,
    /// 这一批里被抓住那行的种类：幽灵用它画形状，与行内图标同一口径。
    pub kind: ArchiveKind,
    /// 展示文案：一条写显示名，多条写「N 项」。
    pub label: String,
}

/// 拖拽幽灵文案（纯函数）：一条给名字（用户抓的是「月报」），多条给数量（抓的是一批）。
///
/// 0 条不会出现（空选集不给拖，见 `render_row`），但给个确定答案好过 panic——
/// 它将来会出现在别处（例如"拖到回收站"之类）。
pub fn drag_label(name: &str, count: usize) -> String {
    match count {
        0 | 1 => name.to_string(),
        count => format!("{count} 项"),
    }
}

/// 拖拽幽灵：跟随光标的轻量预览（不参与命中测试）。
pub struct ArchiveDragGhost {
    pub label: String,
    pub kind: ArchiveKind,
}

impl Render for ArchiveDragGhost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        // 高度与 M4 幽灵同（22px），**不随行高**：幽灵是光标上的东西，不是行。
        div()
            .h_flex()
            .items_center()
            .gap_1()
            .h(rems(1.375))
            .px_2()
            .rounded_md()
            .bg(colors.popover)
            .border_1()
            .border_color(colors.border)
            .shadow_md()
            .child(
                // 形状给种类（与行内同一份 `kind_icon`），颜色一律中性——幽灵不是状态。
                Icon::new(kind_icon(self.kind))
                    .flex_none()
                    .size_3p5()
                    .text_color(colors.muted_foreground),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.foreground)
                    .child(self.label.clone()),
            )
    }
}

/// 行拖拽的落点语义（**只认分组头**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupDrop {
    /// 不是落点（「全部分组」聚合头）。
    NotATarget,
    /// 「未分组」头：移出全部分组。
    Ungroup,
    /// 某个真分组头：移入该分组。
    Into(String),
}

/// 分组头 → 落点语义（纯函数，带单测）。
pub fn group_drop_target(key: &str) -> GroupDrop {
    if key == GROUP_ALL {
        GroupDrop::NotATarget
    } else if key == GROUP_UNGROUPED {
        GroupDrop::Ungroup
    } else {
        GroupDrop::Into(key.to_string())
    }
}

/// 该落点对应的目标分组（`None` = 未分组）。
pub fn drop_folder(target: &GroupDrop) -> Option<&str> {
    match target {
        GroupDrop::NotATarget => None,
        GroupDrop::Ungroup => None,
        GroupDrop::Into(id) => Some(id),
    }
}

/// 真正需要改归属的那几条（**拖到原分组 = 无操作**，与「移动到分组」菜单里当前项置灰同一口径）。
///
/// 顺序沿用传入的 `ids`（面板按可见行顺序给）：回执、宿主侧的批量写入与测试断言都要确定性。
/// 认不出 id 的行（刚被筛掉 / 刚被删）直接不发——拿不到 `folder_id` 就无法判断"要不要改"。
pub fn rows_to_move(rows: &[ArchiveRow], ids: &[String], target: &GroupDrop) -> Vec<String> {
    if matches!(target, GroupDrop::NotATarget) {
        return Vec::new();
    }
    let folder = drop_folder(target);
    ids.iter()
        .filter(|id| {
            rows.iter()
                .find(|row| &row.id == *id)
                .is_some_and(|row| row.folder_id.as_deref() != folder)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{GroupDrop, drag_label, drop_folder, group_drop_target, rows_to_move};
    use crate::model::{ArchiveKind, ArchiveStatus};
    use crate::resource_view::ArchiveRow;

    fn row_in(id: &str, folder: Option<&str>) -> ArchiveRow {
        ArchiveRow {
            id: id.to_string(),
            name: format!("{id}.sql"),
            alias: None,
            kind: ArchiveKind::File,
            version: 1,
            status: ArchiveStatus::Normal,
            tail: String::new(),
            source_table: None,
            tags: Vec::new(),
            folder_id: folder.map(str::to_string),
            updated_epoch: 0,
            archived_epoch: None,
            size_bytes: None,
        }
    }

    #[test]
    fn drag_label_names_one_and_counts_many() {
        assert_eq!(drag_label("月报", 1), "月报");
        assert_eq!(drag_label("月报", 0), "月报");
        assert_eq!(drag_label("月报", 3), "3 项");
    }

    /// 落点：只有分组头接得住；「全部分组」是聚合头，不是落点。
    #[test]
    fn only_group_headers_accept_the_drop() {
        assert_eq!(group_drop_target("__all__"), GroupDrop::NotATarget);
        assert_eq!(group_drop_target("__ungrouped__"), GroupDrop::Ungroup);
        assert_eq!(
            group_drop_target("af_1"),
            GroupDrop::Into("af_1".to_string())
        );
        assert_eq!(drop_folder(&GroupDrop::Ungroup), None);
        assert_eq!(
            drop_folder(&GroupDrop::Into("af_1".to_string())),
            Some("af_1")
        );
    }

    /// 拖到原分组不发（与菜单置灰同一口径）；认不出的 id 不发；顺序沿用传入的 ids。
    #[test]
    fn only_rows_that_change_group_are_sent() {
        let rows = vec![
            row_in("ar_1", None),
            row_in("ar_2", Some("af_1")),
            row_in("ar_3", Some("af_2")),
        ];
        let ids = vec!["ar_3".to_string(), "ar_1".to_string(), "ar_2".to_string()];

        let into = GroupDrop::Into("af_1".to_string());
        assert_eq!(
            rows_to_move(&rows, &ids, &into),
            vec!["ar_3".to_string(), "ar_1".to_string()],
            "已在 af_1 的 ar_2 不发；顺序按 ids"
        );

        let ungroup = GroupDrop::Ungroup;
        assert_eq!(
            rows_to_move(&rows, &ids, &ungroup),
            vec!["ar_3".to_string(), "ar_2".to_string()]
        );

        // 认不出的 id（刚被筛掉 / 刚被删）不发。
        let stale = vec!["ar_1".to_string(), "ar_ghost".to_string()];
        assert_eq!(rows_to_move(&rows, &stale, &into), vec!["ar_1".to_string()]);

        // 聚合头：一条都不发（落点本身就不接）。
        assert!(rows_to_move(&rows, &ids, &GroupDrop::NotATarget).is_empty());
    }
}
