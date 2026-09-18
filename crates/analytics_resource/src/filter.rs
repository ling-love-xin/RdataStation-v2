//! 工具栏的数据层：筛选与排序（纯函数，零 GPUI 依赖）。
//!
//! 独立成模块的理由：这些规则要被**面板**（行列表）与**后续的筛选/排序菜单**共同使用，
//! 而且必须能在没有窗口的环境下单测——放在 `resource_view.rs` 里会让"规则"和"渲染"互相拖累。
//!
//! 面板的空态判定依赖 [`ResourcesFilter::is_empty`]：
//! **空筛选 + 无行 = "还没有任何存档"**（引导归档）；**非空筛选 + 无行 = "没有匹配的存档"**（引导清筛选）。

use crate::model::{ArchiveKind, ArchiveStatus};
use crate::resource_view::{ArchiveRow, GroupOption};

/// 「全部分组」头的 key（分组折叠区最上面那行）。
pub const GROUP_ALL: &str = "__all__";
/// 「未分组」头的 key（没有分组归属的行的区）。
pub const GROUP_UNGROUPED: &str = "__ungrouped__";

/// 列表里的一项：分组头或存档行。
///
/// 分组头也是列表的一项（而不是另画一套）：虚拟化 / 漫游 / 滚动的行为只维护一份，
/// 否则分区渲染要重写一遍列表已经解决过的事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisibleItem {
    GroupHeader {
        /// [`GROUP_ALL`] / [`GROUP_UNGROUPED`] / 分组 id（折叠状态按它记）。
        key: String,
        label: String,
        count: usize,
        /// 层级：0 = 「全部分组」，1 = 「未分组」与各分组（只用于缩进）。
        depth: u8,
        collapsed: bool,
    },
    Row(ArchiveRow),
}

/// 行的分组归属，**只认字典里还活着的分组**（认不出的一律当未分组：行不能丢）。
fn known_folder_of<'r>(
    row: &'r ArchiveRow,
    known: &std::collections::HashSet<&str>,
) -> Option<&'r str> {
    row.folder_id
        .as_deref()
        .filter(|folder| known.contains(folder))
}

/// 分组折叠区的分区（原型 §2.4：全部分组 → 未分组 → 各分组）。
///
/// 三条规则：
/// 1. **一个分组都没有时不出头**（空库与“全部未分组”的常见情形不该多两行噪声）；
/// 2. 折叠只影响行的出场，头自己总在（否则折了就再也展不开）；
/// 3. **计数从当前可见行现算**：筛选后数的是“筛出来的那几行”，与用户眼前的列表一致；
///    认不出的分组归属（分组被删而关联还在的脏数据）算进未分组，而不是把行丢掉。
pub fn build_visible_items(
    rows: &[ArchiveRow],
    groups: &[GroupOption],
    collapsed: &std::collections::HashSet<String>,
) -> Vec<VisibleItem> {
    if groups.is_empty() {
        return rows.iter().cloned().map(VisibleItem::Row).collect();
    }

    let known: std::collections::HashSet<&str> = groups.iter().map(|g| g.id.as_str()).collect();
    // 不用闭包：闭包写不出“返回值借用第一个参数”的签名（生周期会报错）。
    let count_in = |folder: &str| {
        rows.iter()
            .filter(|row| known_folder_of(row, &known) == Some(folder))
            .count()
    };
    let ungrouped_count = rows
        .iter()
        .filter(|row| known_folder_of(row, &known).is_none())
        .count();

    let mut items = Vec::with_capacity(rows.len() + groups.len() + 2);
    items.push(VisibleItem::GroupHeader {
        key: GROUP_ALL.to_string(),
        label: "全部分组".to_string(),
        count: rows.len(),
        depth: 0,
        collapsed: collapsed.contains(GROUP_ALL),
    });
    if collapsed.contains(GROUP_ALL) {
        return items;
    }

    let ungrouped_collapsed = collapsed.contains(GROUP_UNGROUPED);
    items.push(VisibleItem::GroupHeader {
        key: GROUP_UNGROUPED.to_string(),
        label: "未分组".to_string(),
        count: ungrouped_count,
        depth: 1,
        collapsed: ungrouped_collapsed,
    });
    if !ungrouped_collapsed {
        items.extend(
            rows.iter()
                .filter(|row| known_folder_of(row, &known).is_none())
                .cloned()
                .map(VisibleItem::Row),
        );
    }

    for group in groups {
        let is_collapsed = collapsed.contains(&group.id);
        items.push(VisibleItem::GroupHeader {
            key: group.id.clone(),
            label: group.name.clone(),
            count: count_in(&group.id),
            depth: 1,
            collapsed: is_collapsed,
        });
        if is_collapsed {
            continue;
        }
        items.extend(
            rows.iter()
                .filter(|row| known_folder_of(row, &known) == Some(group.id.as_str()))
                .cloned()
                .map(VisibleItem::Row),
        );
    }
    items
}

/// 排序字段。
///
/// 目前只支持**行上真实存在的键**（名称 / 版本号）。归档时间与大小需要 `ArchiveRow` 另带原始值
/// （`updated_epoch` / `size_bytes`）——拿格式化后的尾巴字符串比较，会在 `1.2 KB` 与 `900 B`
/// 之间得出错误顺序；宁可先不支持，也不做一个会静默排错的字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    #[default]
    Name,
    Version,
}

impl SortField {
    /// 菜单文案（与 `ui.rs` 的尺寸常量为同一类"就近常量"）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "名称",
            Self::Version => "版本",
        }
    }
}

/// 排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

impl SortOrder {
    /// 同一字段再点一次即翻转（沿用 M4 与 v1 `use-pagination` 的语义）。
    pub fn flipped(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }

    /// 方向箭头（排序菜单项与工具栏按钮共用，见 `SortField::label` 的同类注释）。
    pub fn arrow(self) -> &'static str {
        match self {
            Self::Asc => "↑",
            Self::Desc => "↓",
        }
    }
}

/// 工具栏筛选条件（全空 = 不限）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourcesFilter {
    /// 关键字：匹配显示名与尾部字段，**大小写不敏感**。
    pub query: String,
    /// 种类多选（空 = 不限）。
    pub kinds: Vec<ArchiveKind>,
    /// 标签多选（**存 id**：名字会改，id 不会；空 = 不限）。
    ///
    /// 多选是 **OR**：选中「重要」与「待办」意味着“这两个标签的存档都看”，
    /// 而不是“同时打两个标签”——后者是更细的诉求，不靠筛选器表达（用户在列表里一眼能看出来）。
    pub tags: Vec<String>,
    /// 只看需要处理的异常（缺失 / 内容已变），对应状态行的“修复…”场景。
    pub only_issues: bool,
}

impl ResourcesFilter {
    /// 是否为空条件（决定面板显示哪一种空态，见模块头注释）。
    pub fn is_empty(&self) -> bool {
        self.query.trim().is_empty()
            && self.kinds.is_empty()
            && self.tags.is_empty()
            && !self.only_issues
    }

    /// 勾选 / 取消一个种类（筛选菜单用）。
    ///
    /// **三个都选中即视为"不限"**：全选与不选在语义上等价，但留下非空 `kinds` 会让
    /// [`is_empty`](Self::is_empty) 把"全选"误判成筛选态——空库就会被显示成"没有匹配"，
    /// 而用户明明什么都没排除。所以这里把等价条件规范化掉，不留脏态。
    pub fn toggle_kind(&mut self, kind: ArchiveKind) {
        match self.kinds.iter().position(|k| *k == kind) {
            Some(index) => {
                self.kinds.remove(index);
            }
            None => {
                self.kinds.push(kind);
                if self.kinds.len() == ArchiveKind::ALL.len() {
                    self.kinds.clear();
                }
            }
        }
    }

    /// 该种类是否在筛选集合里（菜单打勾用）。
    ///
    /// 全选会被规范化为不限制（见 [`toggle_kind`](Self::toggle_kind)），故"全选"态下
    /// 这里一律为 `false`——菜单显示"都没勾"，与"什么都没过滤"的事实一致。
    pub fn has_kind(&self, kind: ArchiveKind) -> bool {
        self.kinds.contains(&kind)
    }

    /// 勾选 / 取消一个标签（筛选菜单用）。
    ///
    /// 与 [`toggle_kind`](Self::toggle_kind) 不同：标签没有“全选 = 不限”的归一化（标签集合是
    /// 开放的，全选它没有意义）——逐个勾/取就是它的全部语义。
    pub fn toggle_tag(&mut self, tag_id: &str) {
        match self.tags.iter().position(|id| id == tag_id) {
            Some(index) => {
                self.tags.remove(index);
            }
            None => self.tags.push(tag_id.to_string()),
        }
    }

    pub fn has_tag(&self, tag_id: &str) -> bool {
        self.tags.iter().any(|id| id == tag_id)
    }

    /// 抹掉一个不再存在的标签（标签被删后清悬空条件，与选择集的悬空清理同一个道理）。
    pub fn drop_tag(&mut self, tag_id: &str) {
        self.tags.retain(|id| id != tag_id);
    }

    /// 菜单里设的条件个数（**不含搜索词**）。
    ///
    /// 搜索词在输入框里看得见，算进来会让"筛选 N"这个徽标口径混乱；它只负责数
    /// "必须开菜单才能看出来"的那几维。
    pub fn menu_dims(&self) -> usize {
        self.kinds.len() + self.tags.len() + usize::from(self.only_issues)
    }

    /// 单行是否命中。
    pub fn matches(&self, row: &ArchiveRow) -> bool {
        let query = self.query.trim().to_lowercase();
        if !query.is_empty() {
            let haystack = format!("{}{}", row.name.to_lowercase(), row.tail.to_lowercase());
            if !haystack.contains(&query) {
                return false;
            }
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&row.kind) {
            return false;
        }
        if !self.tags.is_empty() && !row.tag_ids.iter().any(|id| self.tags.contains(id)) {
            return false;
        }
        if self.only_issues && row.status == ArchiveStatus::Normal {
            return false;
        }
        true
    }
}

/// 一行是否需要用户处理（与状态行的"异常"口径一致）。
pub fn needs_attention(row: &ArchiveRow) -> bool {
    row.status != ArchiveStatus::Normal
}

/// 筛选 → 排序（返回新集合，不改入参）。
///
/// 排序**带名称兜底且兜底键不随方向翻转**：否则同一版本号的多行顺序不可预测，
/// 会出现"同一份数据两次渲染顺序不同"的观感问题。
pub fn apply_view(
    rows: &[ArchiveRow],
    filter: &ResourcesFilter,
    field: SortField,
    order: SortOrder,
) -> Vec<ArchiveRow> {
    let mut result: Vec<ArchiveRow> = rows
        .iter()
        .filter(|row| filter.matches(row))
        .cloned()
        .collect();

    result.sort_by(|left, right| {
        let primary = match field {
            SortField::Name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
            SortField::Version => left.version.cmp(&right.version),
        };
        let primary = match order {
            SortOrder::Asc => primary,
            SortOrder::Desc => primary.reverse(),
        };
        if primary.is_eq() {
            return left.name.to_lowercase().cmp(&right.name.to_lowercase());
        }
        primary
    });

    result
}

#[cfg(test)]
mod tests {
    use super::{
        ResourcesFilter, SortField, SortOrder, apply_view, needs_attention,
    };
    use crate::model::{ArchiveKind, ArchiveStatus};
    use crate::resource_view::ArchiveRow;

    fn row(id: &str, name: &str, kind: ArchiveKind, status: ArchiveStatus, version: i32, tail: &str) -> ArchiveRow {
        ArchiveRow {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            version,
            status,
            tail: tail.to_string(),
            tag_ids: Vec::new(),
            folder_id: None,
        }
    }

    fn row_with_tags(id: &str, tags: &[&str]) -> ArchiveRow {
        ArchiveRow {
            tag_ids: tags.iter().map(|t| t.to_string()).collect(),
            ..row(id, id, ArchiveKind::File, ArchiveStatus::Normal, 1, "")
        }
    }

    fn sample() -> Vec<ArchiveRow> {
        vec![
            row("ar_2", "Beta.sql", ArchiveKind::File, ArchiveStatus::Normal, 2, "1.2 KB · 3 天前"),
            row("ar_1", "alpha.sql", ArchiveKind::Analysis, ArchiveStatus::Normal, 5, "12,480 行 × 18 列"),
            row("ar_3", "gamma.sql", ArchiveKind::File, ArchiveStatus::Missing, 1, ""),
        ]
    }

    #[test]
    fn query_matches_name_and_tail_case_insensitively() {
        let rows = sample();

        let by_name = ResourcesFilter {
            query: "ALPHA".to_string(),
            ..ResourcesFilter::default()
        };
        let hit = apply_view(&rows, &by_name, SortField::Name, SortOrder::Asc);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, "ar_1");

        // 用户也会拿尾部线索找（"1.2"、"48" 之类）。
        let by_tail = ResourcesFilter {
            query: "1.2".to_string(),
            ..ResourcesFilter::default()
        };
        assert_eq!(apply_view(&rows, &by_tail, SortField::Name, SortOrder::Asc).len(), 1);
    }

    #[test]
    fn kind_and_issue_filters_narrow_the_list() {
        let rows = sample();

        let only_files = ResourcesFilter {
            kinds: vec![ArchiveKind::File],
            ..ResourcesFilter::default()
        };
        assert_eq!(apply_view(&rows, &only_files, SortField::Name, SortOrder::Asc).len(), 2);
        assert!(!only_files.is_empty());

        let issues = ResourcesFilter {
            only_issues: true,
            ..ResourcesFilter::default()
        };
        let hit = apply_view(&rows, &issues, SortField::Name, SortOrder::Asc);
        assert_eq!(hit.len(), 1);
        assert!(needs_attention(&hit[0]));
        assert_eq!(hit[0].id, "ar_3");

        // 全空条件是"空"，面板据此区分"无匹配"与"还没有任何存档"。
        assert!(ResourcesFilter::default().is_empty());
    }

    #[test]
    fn sort_is_stable_and_direction_aware() {
        let rows = sample();
        let none = ResourcesFilter::default();

        let asc = apply_view(&rows, &none, SortField::Name, SortOrder::Asc);
        assert_eq!(
            asc.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["ar_1", "ar_2", "ar_3"],
            "名称升序且大小写不敏感"
        );

        let desc = apply_view(&rows, &none, SortField::Name, SortOrder::Desc);
        assert_eq!(
            desc.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["ar_3", "ar_2", "ar_1"]
        );

        let by_version = apply_view(&rows, &none, SortField::Version, SortOrder::Desc);
        assert_eq!(
            by_version.iter().map(|r| r.version).collect::<Vec<_>>(),
            vec![5, 2, 1]
        );
    }

    #[test]
    fn order_flips_on_repeat_click() {
        assert_eq!(SortOrder::Asc.flipped(), SortOrder::Desc);
        assert_eq!(SortOrder::Desc.flipped(), SortOrder::Asc);
        assert_eq!(SortField::Name.label(), "名称");
        assert_eq!(SortField::Version.label(), "版本");
        assert_eq!(SortOrder::Asc.arrow(), "↑");
        assert_eq!(SortOrder::Desc.arrow(), "↓");
    }

    #[test]
    fn toggle_kind_normalizes_select_all_to_unlimited() {
        let mut filter = ResourcesFilter::default();

        filter.toggle_kind(ArchiveKind::File);
        filter.toggle_kind(ArchiveKind::Analysis);
        assert!(filter.has_kind(ArchiveKind::File));
        assert_eq!(filter.menu_dims(), 2);
        assert!(!filter.is_empty(), "选了两种就是真筛选");

        // 三个全选 = 不限：不能留下"看起来在筛选"的等价条件。
        filter.toggle_kind(ArchiveKind::TableRef);
        assert!(filter.kinds.is_empty());
        assert!(filter.is_empty());
        assert!(!filter.has_kind(ArchiveKind::TableRef));

        // 再点一次就取消勾选（不是又选中）。
        filter.toggle_kind(ArchiveKind::File);
        filter.toggle_kind(ArchiveKind::File);
        assert!(!filter.has_kind(ArchiveKind::File));
    }

    #[test]
    fn menu_dims_counts_menu_conditions_only() {
        let mut filter = ResourcesFilter {
            query: "dau".to_string(),
            ..ResourcesFilter::default()
        };
        // 搜索词不算徽标（它在输入框里看得见）。
        assert_eq!(filter.menu_dims(), 0);
        filter.only_issues = true;
        assert_eq!(filter.menu_dims(), 1);
    }

    /// 标签筛选：多选是 OR（“这两个标签的存档都看”），没有“全选 = 不限”的归一化。
    #[test]
    fn tag_filter_is_multi_select_or() {
        let rows = vec![
            row_with_tags("ar_1", &["at_a"]),
            row_with_tags("ar_2", &["at_b"]),
            row_with_tags("ar_3", &["at_a", "at_b"]),
            row_with_tags("ar_4", &[]),
        ];
        let mut filter = ResourcesFilter::default();
        assert_eq!(apply_view(&rows, &filter, SortField::Name, SortOrder::Asc).len(), 4);

        filter.toggle_tag("at_a");
        assert!(filter.has_tag("at_a"));
        assert!(!filter.is_empty(), "选了标签就是真筛选（空库不该显示“没有匹配”）");
        assert_eq!(
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc)
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ar_1", "ar_3"]
        );

        // 再选一个 = 并集（不是交集）。
        filter.toggle_tag("at_b");
        assert_eq!(apply_view(&rows, &filter, SortField::Name, SortOrder::Asc).len(), 3);
        assert_eq!(filter.menu_dims(), 2);

        // 再点一次取消勾选。
        filter.toggle_tag("at_a");
        assert!(!filter.has_tag("at_a"));
        assert_eq!(apply_view(&rows, &filter, SortField::Name, SortOrder::Asc).len(), 2);
    }

    /// 标签被删后要能抹掉悬空条件（否则列表会“什么都没匹配”，而菜单上的勾还在）。
    #[test]
    fn drop_tag_removes_a_stale_condition() {
        let mut filter = ResourcesFilter::default();
        filter.toggle_tag("at_a");
        filter.drop_tag("at_a");
        assert!(filter.is_empty());
        assert_eq!(filter.menu_dims(), 0);
    }

    // ==================== 分组折叠区 ====================

    use super::{GROUP_ALL, GROUP_UNGROUPED, VisibleItem, build_visible_items};
    use crate::resource_view::GroupOption;

    fn row_in(id: &str, folder: Option<&str>) -> ArchiveRow {
        ArchiveRow {
            folder_id: folder.map(str::to_string),
            ..row(id, id, ArchiveKind::File, ArchiveStatus::Normal, 1, "")
        }
    }

    fn group(id: &str, name: &str, count: usize) -> GroupOption {
        let _ = count;
        GroupOption {
            id: id.to_string(),
            name: name.to_string(),
        }
    }

    fn keys(items: &[VisibleItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                VisibleItem::GroupHeader { key, .. } => key.clone(),
                VisibleItem::Row(row) => row.id.clone(),
            })
            .collect()
    }

    /// 没有分组就不出头：空库与“全部未分组”不该多两行噪声。
    #[test]
    fn no_groups_means_no_headers() {
        let rows = vec![row_in("ar_1", None), row_in("ar_2", None)];
        let items = build_visible_items(&rows, &[], &Default::default());
        assert_eq!(keys(&items), vec!["ar_1", "ar_2"]);
    }

    /// 三层结构：全部分组 → 未分组 → 各分组；计数来自行集合，不另查库。
    #[test]
    fn groups_partition_rows_with_counts() {
        let rows = vec![
            row_in("ar_1", Some("af_1")),
            row_in("ar_2", None),
            row_in("ar_3", Some("af_1")),
            row_in("ar_4", Some("af_2")),
        ];
        let groups = vec![group("af_1", "月报", 0), group("af_2", "周报", 0)];
        let items = build_visible_items(&rows, &groups, &Default::default());
        assert_eq!(
            keys(&items),
            vec![
                GROUP_ALL,
                GROUP_UNGROUPED,
                "ar_2",
                "af_1",
                "ar_1",
                "ar_3",
                "af_2",
                "ar_4",
            ]
        );
        let total = items
            .iter()
            .find_map(|item| match item {
                VisibleItem::GroupHeader { key, count, .. } if key == GROUP_ALL => Some(*count),
                _ => None,
            })
            .expect("全部分组头");
        assert_eq!(total, 4, "“全部分组”数的是可见行总数");
    }

    /// 折叠：头还在（否则展不开），行不出场；折“全部分组”就只剩那一行。
    #[test]
    fn collapsing_hides_rows_but_keeps_headers() {
        let rows = vec![row_in("ar_1", Some("af_1")), row_in("ar_2", None)];
        let groups = vec![group("af_1", "月报", 1)];

        let collapsed: std::collections::HashSet<String> =
            ["af_1".to_string()].into_iter().collect();
        let items = build_visible_items(&rows, &groups, &collapsed);
        assert_eq!(
            keys(&items),
            vec![GROUP_ALL, GROUP_UNGROUPED, "ar_2", "af_1"],
            "折叠的分组只出头不出行"
        );

        let all_collapsed: std::collections::HashSet<String> =
            [GROUP_ALL.to_string()].into_iter().collect();
        let items = build_visible_items(&rows, &groups, &all_collapsed);
        assert_eq!(keys(&items), vec![GROUP_ALL], "折全部 = 只剩一行");
    }

    /// 计数按**当前可见行**现算：筛选后头里的数就是眼前的行数。
    #[test]
    fn header_counts_follow_the_visible_rows() {
        let groups = vec![group("af_1", "月报", 99)];
        let rows = vec![row_in("ar_1", Some("af_1")), row_in("ar_2", Some("af_1"))];
        let items = build_visible_items(&rows, &groups, &Default::default());
        let counts: Vec<usize> = items
            .iter()
            .filter_map(|item| match item {
                VisibleItem::GroupHeader { count, .. } => Some(*count),
                _ => None,
            })
            .collect();
        assert_eq!(counts, vec![2, 0, 2], "全部分组 / 未分组 / 月报");
    }

    /// 脏数据（分组被删而关联还在）：归属认不出就算未分组，不能把行吞掉。
    #[test]
    fn unknown_folders_fall_back_to_ungrouped() {
        let groups = vec![group("af_1", "月报", 0)];
        let rows = vec![row_in("ar_1", Some("af_gone")), row_in("ar_2", Some("af_1"))];
        let items = build_visible_items(&rows, &groups, &Default::default());
        assert_eq!(
            keys(&items),
            vec![GROUP_ALL, GROUP_UNGROUPED, "ar_1", "af_1", "ar_2"],
            "认不出的分组归属算未分组（行不丢）"
        );
    }
}
