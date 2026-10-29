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
mod tests;
