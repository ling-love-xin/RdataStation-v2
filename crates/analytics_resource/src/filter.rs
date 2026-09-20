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

/// 排序字段（原型 §2.2 的五项）。
///
/// 比的是**行上的原始值**（时间戳 / 字节数），不是格式化后的尾巴字符串——
/// 拿 `1.2 KB` 与 `900 B` 比较会静默排错（前者小于后者的字节数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    #[default]
    Name,
    ArchivedAt,
    UpdatedAt,
    Size,
    Version,
}

impl SortField {
    /// 菜单里的顺序（原型 §2.2 的“名称 / 归档时间 / 更新时间 / 大小 / 版本号”）。
    pub const ALL: [SortField; 5] = [
        Self::Name,
        Self::ArchivedAt,
        Self::UpdatedAt,
        Self::Size,
        Self::Version,
    ];

    /// 菜单文案（与 `ui.rs` 的尺寸常量为同一类"就近常量"）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "名称",
            Self::ArchivedAt => "归档时间",
            Self::UpdatedAt => "更新时间",
            Self::Size => "大小",
            Self::Version => "版本号",
        }
    }

    /// 落盘 key（设置项 `resources.default_sort` 的取值；与菜单文案分家——文案会改，key 不会）。
    pub fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::ArchivedAt => "archived_at",
            Self::UpdatedAt => "updated_at",
            Self::Size => "size",
            Self::Version => "version",
        }
    }

    /// 解析落盘 key；未知值返回 `None`（**不猜**，由调用方回退到默认）。
    pub fn from_key(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|field| field.key() == text)
    }

    /// 该字段的默认方向：名称升序（A→Z 是直觉），时间 / 大小 / 版本**降序**
    /// （用户点这几列想看的是"最新 / 最大"，不是"最早 / 最小"）。
    pub fn default_order(self) -> SortOrder {
        match self {
            Self::Name => SortOrder::Asc,
            Self::ArchivedAt | Self::UpdatedAt | Self::Size | Self::Version => SortOrder::Desc,
        }
    }

    /// 工具栏按钮上的短文案。
    ///
    /// 按钮要跟着搜索框抢 240px 面板的宽度：四个字的「归档时间」会把搜索框挤到没法用
    /// （菜单里有全名，不靠按钮认字段）。
    pub fn short_label(self) -> &'static str {
        match self {
            Self::Name => "名称",
            Self::ArchivedAt => "归档",
            Self::UpdatedAt => "更新",
            Self::Size => "大小",
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
        if !query.is_empty() && !search_haystack(row).contains(&query) {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&row.kind) {
            return false;
        }
        if !self.tags.is_empty() && !row.tags.iter().any(|tag| self.tags.contains(&tag.id)) {
            return false;
        }
        if self.only_issues && row.status == ArchiveStatus::Normal {
            return false;
        }
        true
    }
}

/// 匹配面包含的字段（**人读短名**，按用户找东西的先后排）。
///
/// 这份清单与 [`search_haystack`] 是同一件事的两种表达：一份给程序拼串，一份给用户看
/// （[`no_match_hint`]）。加字段时两边一起改——单测会拦住“清单与实现不同步”。
pub const SEARCH_FIELDS: [&str; 5] = ["显示名", "别名", "标签", "来源表", "尾部信息"];

/// 无匹配空态的副文案（原型 §5）。
///
/// **搜不到的那一刻**才是最需要知道“哪些字段能搜”的时候：不写这句，用户会以为存档不在了，
/// 而实际只是搜了行上看不见的字段。搜索词为空（纯筛选无匹配）时改指筛选。
pub fn no_match_hint(query: &str) -> String {
    if query.trim().is_empty() {
        "去掉几个筛选条件，或清空筛选".to_string()
    } else {
        format!("换个关键词，或改搜这些字段：{}", SEARCH_FIELDS.join(" / "))
    }
}

/// 一行的**匹配面**：显示名 / 别名 / 标签名 / 来源表 / 尾部字段拼成一个已小写化的串。
///
/// 匹配面**故意比展示面宽**（原型 §2.2）：别名、标签名、来源表都不在行上显示（面板窄，
/// 摆不下），但它们恰恰是用户记得住的线索——“我叫它月报”“那张 `dwd_orders` 表来的”。
/// 与 quick_open 的 `keywords` 同一口径：**可搜，不高亮**（高亮只认看得见的那几个字段）。
///
/// 拼一个串而不是逐字段比较：搜索是**逐行调用的**（每敲一个字重跑一遍列表），
/// 这里只做一次装配，[`ResourcesFilter::matches`] 就只剩一次 `contains`。
/// 字段之间留空格：不留的话跨字段的偶然连缀（名尾 + 尾巴首字母）会造出假命中。
pub fn search_haystack(row: &ArchiveRow) -> String {
    let mut haystack = String::with_capacity(row.name.len() + row.tail.len() + 32);
    let mut push = |part: &str| {
        if part.is_empty() {
            return;
        }
        if !haystack.is_empty() {
            haystack.push(' ');
        }
        haystack.push_str(&part.to_lowercase());
    };
    push(&row.name);
    push(row.alias.as_deref().unwrap_or_default());
    push(row.source_table.as_deref().unwrap_or_default());
    for tag in &row.tags {
        push(&tag.name);
    }
    push(&row.tail);
    haystack
}

/// 一行是否需要用户处理（与状态行的"异常"口径一致）。
pub fn needs_attention(row: &ArchiveRow) -> bool {
    row.status != ArchiveStatus::Normal
}

/// 缺值（没记体积 / 没记归档时间的旧行）的排序键：**两个方向上都排最后**。
///
/// 降序时把未知值顶到最前，等于让“不知道”冒充“最大”——那不是用户点「大小 ↓」想看的东西。
fn opt_key(value: Option<i64>, order: SortOrder) -> i64 {
    match (value, order) {
        (Some(value), _) => value,
        (None, SortOrder::Asc) => i64::MAX,
        (None, SortOrder::Desc) => i64::MIN,
    }
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
            SortField::UpdatedAt => left.updated_epoch.cmp(&right.updated_epoch),
            SortField::ArchivedAt => {
                opt_key(left.archived_epoch, order).cmp(&opt_key(right.archived_epoch, order))
            }
            SortField::Size => {
                opt_key(left.size_bytes, order).cmp(&opt_key(right.size_bytes, order))
            }
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
    use super::{ResourcesFilter, SortField, SortOrder, apply_view, needs_attention};
    use crate::detail_view::ArchiveTagChip;
    use crate::model::{ArchiveKind, ArchiveStatus};
    use crate::resource_view::ArchiveRow;

    fn row(
        id: &str,
        name: &str,
        kind: ArchiveKind,
        status: ArchiveStatus,
        version: i32,
        tail: &str,
    ) -> ArchiveRow {
        ArchiveRow {
            id: id.to_string(),
            name: name.to_string(),
            alias: None,
            kind,
            version,
            status,
            tail: tail.to_string(),
            source_table: None,
            tags: Vec::new(),
            folder_id: None,
            // 原始值默认给中性值：只有排序用例关心它们（需要时用结构体更新语法盖掉）。
            updated_epoch: 0,
            archived_epoch: None,
            size_bytes: None,
        }
    }

    fn row_with_tags(id: &str, tags: &[&str]) -> ArchiveRow {
        ArchiveRow {
            // 测试里标签名 = id（名字与 id 的对应关系不是本模块的事）。
            tags: tags
                .iter()
                .map(|t| ArchiveTagChip {
                    id: t.to_string(),
                    name: t.to_string(),
                })
                .collect(),
            ..row(id, id, ArchiveKind::File, ArchiveStatus::Normal, 1, "")
        }
    }

    fn sample() -> Vec<ArchiveRow> {
        vec![
            row(
                "ar_2",
                "Beta.sql",
                ArchiveKind::File,
                ArchiveStatus::Normal,
                2,
                "1.2 KB · 3 天前",
            ),
            row(
                "ar_1",
                "alpha.sql",
                ArchiveKind::Analysis,
                ArchiveStatus::Normal,
                5,
                "12,480 行 × 18 列",
            ),
            row(
                "ar_3",
                "gamma.sql",
                ArchiveKind::File,
                ArchiveStatus::Missing,
                1,
                "",
            ),
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
        assert_eq!(
            apply_view(&rows, &by_tail, SortField::Name, SortOrder::Asc).len(),
            1
        );
    }

    /// 匹配面比展示面宽（原型 §2.2）：别名 / 标签名 / 来源表都可搜。
    #[test]
    fn query_matches_alias_tag_name_and_source_table() {
        let mut alias_row = row(
            "ar_1",
            "dau_report.sql",
            ArchiveKind::Analysis,
            ArchiveStatus::Normal,
            1,
            "",
        );
        alias_row.alias = Some("月报".to_string());

        let mut tag_row = row_with_tags("ar_2", &["at_1"]);
        tag_row.tags[0].name = "财务报表".to_string();

        let mut table_row = row(
            "ar_3",
            "orders_archive.sql",
            ArchiveKind::TableRef,
            ArchiveStatus::Normal,
            1,
            "无指纹",
        );
        table_row.source_table = Some("dwd.dwd_orders".to_string());

        let rows = vec![alias_row, tag_row, table_row];
        let hit = |query: &str| {
            let filter = ResourcesFilter {
                query: query.to_string(),
                ..ResourcesFilter::default()
            };
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc)
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        };

        assert_eq!(hit("月报"), vec!["ar_1"], "别名命中（行上不显示它）");
        assert_eq!(
            hit("财务报表"),
            vec!["ar_2"],
            "标签名命中（筛标签维比的是 id）"
        );
        assert_eq!(hit("DWD_ORDERS"), vec!["ar_3"], "来源表命中且大小写不敏感");
        assert_eq!(hit("dwd.dwd"), vec!["ar_3"], "来源表可整串搜");

        // 跨字段的偶然连缀不算命中（字段之间留了空格），无关词一律不命中。
        assert!(hit("sql 无").is_empty());
        assert!(hit("zzz").is_empty());
    }

    /// 匹配面是**加**进去的而不是替换：缺别名 / 标签 / 来源表的行依旧按名称与尾巴命中。
    #[test]
    fn search_haystack_keeps_name_and_tail_for_every_row() {
        let row = row(
            "ar_1",
            "alpha.sql",
            ArchiveKind::File,
            ArchiveStatus::Normal,
            3,
            "1.2 KB · 3 天前",
        );
        let haystack = super::search_haystack(&row);
        assert!(haystack.contains("alpha.sql"));
        assert!(haystack.contains("1.2 kb"), "尾巴已小写化：{haystack}");
        assert_eq!(haystack, haystack.to_lowercase(), "装配即小写");
    }

    /// 给用户看的那份字段清单（[`super::SEARCH_FIELDS`]）与实现真的对得上：
    /// 每一项都能在某行上搜到——“说明了却搜不到”比不说还糟。
    #[test]
    fn search_fields_list_covers_what_the_haystack_really_matches() {
        let mut row = row(
            "ar_1",
            "dau_report.sql",
            ArchiveKind::TableRef,
            ArchiveStatus::Normal,
            3,
            "1.2 KB · 3 天前",
        );
        row.alias = Some("月报".to_string());
        row.source_table = Some("dwd.dwd_orders".to_string());
        row.tags = vec![ArchiveTagChip {
            id: "at_1".to_string(),
            name: "财务报表".to_string(),
        }];
        let haystack = super::search_haystack(&row);

        // 与 `SEARCH_FIELDS` 一一对应（顺序即清单顺序）。
        let probes = ["dau_report", "月报", "财务报表", "dwd.dwd_orders", "1.2 kb"];
        assert_eq!(super::SEARCH_FIELDS.len(), probes.len());
        for (field, probe) in super::SEARCH_FIELDS.iter().zip(probes) {
            assert!(haystack.contains(probe), "{field} 应可被搜到（{probe}）");
        }
    }

    /// 空态副文案：搜不到时告知能搜哪些字段，纯筛选无匹配时改指筛选。
    #[test]
    fn no_match_hint_speaks_about_the_query_only_when_there_is_one() {
        let filtered = super::no_match_hint("  ");
        assert!(
            filtered.contains("筛选"),
            "没搜索词就不谈搜什么：{filtered}"
        );
        assert!(!filtered.contains("别名"));

        let searched = super::no_match_hint("月报");
        for field in super::SEARCH_FIELDS {
            assert!(searched.contains(field), "缺字段说明：{field} / {searched}");
        }
    }

    #[test]
    fn kind_and_issue_filters_narrow_the_list() {
        let rows = sample();

        let only_files = ResourcesFilter {
            kinds: vec![ArchiveKind::File],
            ..ResourcesFilter::default()
        };
        assert_eq!(
            apply_view(&rows, &only_files, SortField::Name, SortOrder::Asc).len(),
            2
        );
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

    /// 造一条带原始值的行（名称故意与时间 / 体积反着排，这样“按名称兜底”不会被误认为排对了）。
    fn row_with_raw(
        id: &str,
        name: &str,
        updated: i64,
        archived: Option<i64>,
        size: Option<i64>,
    ) -> ArchiveRow {
        ArchiveRow {
            updated_epoch: updated,
            archived_epoch: archived,
            size_bytes: size,
            ..row(id, name, ArchiveKind::File, ArchiveStatus::Normal, 1, "")
        }
    }

    #[test]
    fn time_and_size_sorts_use_the_raw_values() {
        let rows = vec![
            row_with_raw("ar_1", "a_big.sql", 300, Some(30), Some(1_228_800)),
            row_with_raw("ar_2", "z_small.sql", 100, Some(10), Some(900)),
            row_with_raw("ar_3", "m_mid.sql", 200, Some(20), Some(4096)),
        ];
        let none = ResourcesFilter::default();
        let ids = |sorted: Vec<ArchiveRow>| sorted.iter().map(|r| r.id.clone()).collect::<Vec<_>>();

        // 时间：新的在前（降序是这一列的常态口径）。
        assert_eq!(
            ids(apply_view(
                &rows,
                &none,
                SortField::UpdatedAt,
                SortOrder::Desc
            )),
            vec!["ar_1", "ar_3", "ar_2"]
        );
        assert_eq!(
            ids(apply_view(
                &rows,
                &none,
                SortField::ArchivedAt,
                SortOrder::Asc
            )),
            vec!["ar_2", "ar_3", "ar_1"]
        );

        // 大小按字节比：900 B 在 1.2 KB 之前（若拿尾巴字符串比就会反过来）。
        assert_eq!(
            ids(apply_view(&rows, &none, SortField::Size, SortOrder::Asc)),
            vec!["ar_2", "ar_3", "ar_1"]
        );
    }

    #[test]
    fn unknown_time_and_size_rows_stay_last_in_both_directions() {
        let rows = vec![
            row_with_raw("ar_unknown", "a_unknown.sql", 0, None, None),
            row_with_raw("ar_small", "z_small.sql", 100, Some(10), Some(900)),
            row_with_raw("ar_big", "m_big.sql", 200, Some(20), Some(4096)),
        ];
        let none = ResourcesFilter::default();
        let ids = |sorted: Vec<ArchiveRow>| sorted.iter().map(|r| r.id.clone()).collect::<Vec<_>>();

        // 缺值不随方向翻转：升序降序都在末尾（“不知道”不当“最大”）。
        assert_eq!(
            ids(apply_view(&rows, &none, SortField::Size, SortOrder::Asc)),
            vec!["ar_small", "ar_big", "ar_unknown"]
        );
        assert_eq!(
            ids(apply_view(&rows, &none, SortField::Size, SortOrder::Desc)),
            vec!["ar_big", "ar_small", "ar_unknown"]
        );
        assert_eq!(
            ids(apply_view(
                &rows,
                &none,
                SortField::ArchivedAt,
                SortOrder::Desc
            )),
            vec!["ar_big", "ar_small", "ar_unknown"]
        );
    }

    /// 同一排序值（含两个都缺值）时按名称兜底，且兜底键不随方向翻转。
    #[test]
    fn ties_fall_back_to_name_without_flipping() {
        let rows = vec![
            row_with_raw("ar_b", "beta.sql", 0, None, None),
            row_with_raw("ar_a", "Alpha.sql", 0, None, None),
        ];
        let none = ResourcesFilter::default();

        for order in [SortOrder::Asc, SortOrder::Desc] {
            let sorted = apply_view(&rows, &none, SortField::ArchivedAt, order);
            assert_eq!(
                sorted.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
                vec!["ar_a", "ar_b"],
                "兜底键恒为名称升序（{order:?}）"
            );
        }
    }

    #[test]
    fn order_flips_on_repeat_click() {
        assert_eq!(SortOrder::Asc.flipped(), SortOrder::Desc);
        assert_eq!(SortOrder::Desc.flipped(), SortOrder::Asc);
        assert_eq!(SortField::Name.label(), "名称");
        // 菜单按原型 §2.2 的顺序：名称 / 归档时间 / 更新时间 / 大小 / 版本号。
        assert_eq!(
            SortField::ALL.map(SortField::label),
            ["名称", "归档时间", "更新时间", "大小", "版本号"]
        );
        // 工具栏按钮用两字短文案（240px 面板里要跟搜索框分宽度）。
        assert_eq!(
            SortField::ALL.map(SortField::short_label),
            ["名称", "归档", "更新", "大小", "版本"]
        );
        // 落盘 key 与菜单文案分家：文案改文案、key 不改（设置项 `resources.default_sort` 存它）。
        assert_eq!(
            SortField::ALL.map(SortField::key),
            ["name", "archived_at", "updated_at", "size", "version"]
        );
        for field in SortField::ALL {
            assert_eq!(SortField::from_key(field.key()), Some(field));
        }
        assert_eq!(SortField::from_key("看不见的列"), None, "未知 key 不猜");
        assert_eq!(SortField::Name.default_order(), SortOrder::Asc);
        assert_eq!(SortField::UpdatedAt.default_order(), SortOrder::Desc);
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
        assert_eq!(
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc).len(),
            4
        );

        filter.toggle_tag("at_a");
        assert!(filter.has_tag("at_a"));
        assert!(
            !filter.is_empty(),
            "选了标签就是真筛选（空库不该显示“没有匹配”）"
        );
        assert_eq!(
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc)
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ar_1", "ar_3"]
        );

        // 再选一个 = 并集（不是交集）。
        filter.toggle_tag("at_b");
        assert_eq!(
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc).len(),
            3
        );
        assert_eq!(filter.menu_dims(), 2);

        // 再点一次取消勾选。
        filter.toggle_tag("at_a");
        assert!(!filter.has_tag("at_a"));
        assert_eq!(
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc).len(),
            2
        );
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
        let rows = vec![
            row_in("ar_1", Some("af_gone")),
            row_in("ar_2", Some("af_1")),
        ];
        let items = build_visible_items(&rows, &groups, &Default::default());
        assert_eq!(
            keys(&items),
            vec![GROUP_ALL, GROUP_UNGROUPED, "ar_1", "af_1", "ar_2"],
            "认不出的分组归属算未分组（行不丢）"
        );
    }
}
