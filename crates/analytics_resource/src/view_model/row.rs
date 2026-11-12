//! 列表与面板快照：行、分组、标签、计数与实例维度。

use crate::model::{ArchiveKind, ArchiveStatus};

use super::detail::{ArchiveDetail, ArchiveTagChip};

/// 一行存档：**宿主已格式化好的快照**（面板渲染不做计算与 I/O）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveRow {
    pub id: String,
    pub name: String,
    /// 实例归属（§13）：全局行不可在项目侧修改，只有「分发到本项目」。
    pub scope: crate::model::RowScope,
    /// 别名（`None` = 没起过）。
    ///
    /// **行上不显示**（面板只有 240px），但它是搜索匹配面的一员（原型 §2.2）：
    /// 别名往往正是用户记得住的那个叫法。
    pub alias: Option<String>,
    pub kind: ArchiveKind,
    pub version: i32,
    pub status: ArchiveStatus,
    /// 尾部字段（按字段优先级规则拼好，见 [`row_tail`]）。
    pub tail: String,
    /// 来源表 `schema.table`（引用型必有，其余多缺）。
    ///
    /// 与 `alias` 同理：不在行上显示，只进搜索匹配面——表名常是找存档的线索。
    pub source_table: Option<String>,
    /// 这行挂的标签（**带名字**）。
    ///
    /// 一处数据两处用：筛选的标签维比 `id`（名字会改），搜索的匹配面比 `name`（原型 §2.2 的“标签”）。
    /// 名字与 id 同源装配（[`crate::present::to_row`]），不存在两份列表错位的可能。
    pub tags: Vec<ArchiveTagChip>,
    /// 这行归属的分组（`None` = 未分组）。
    ///
    /// 单层分组（架构 D8）：最多一个，所以是 `Option` 而不是集合——类型上就把
    /// “一个资源同时属于两个分组”这种不存在的状态挡掉。
    pub folder_id: Option<String>,
    /// 最近更新时间（Unix 秒）——「更新时间」排序用。
    ///
    /// 排序比的是**原始值**：尾巴里是“3 天前”这种人读的字符串，比它等于没比（见 `filter::SortField`）。
    pub updated_epoch: i64,
    /// 归档时刻（Unix 秒，`None` = 没记录：未归档过的旧行）——「归档时间」排序用。
    pub archived_epoch: Option<i64>,
    /// 本体字节数（`None` = 未知：分析表 / 引用型 / 没记体积的旧行）——「大小」排序用。
    pub size_bytes: Option<i64>,
}

/// 分组字典项（分组折叠区与「移动到分组」菜单共用）。
///
/// 不带成员数：计数在渲染前从**当前可见行**现算（见 `filter::build_visible_items`），
/// 这样筛选后头里的数就是眼前的行数，不会出现“头说 5 行、列表里只有 2 行”。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupOption {
    pub id: String,
    pub name: String,
}

/// 标签字典项（筛选菜单与打标对话框共用）。
///
/// `count` = 有多少条**存活**存档在用这个标签（筛选菜单里给个量级；没有它用户不知道
/// 勾了这个标签会不会得到空列表）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagOption {
    pub id: String,
    pub name: String,
    pub count: usize,
}

/// 状态行计数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArchiveCounts {
    pub total: usize,
    pub archived: usize,
    pub analysis: usize,
    pub table_ref: usize,
    pub missing: usize,
    pub drifted: usize,
}

impl ArchiveCounts {
    /// 状态行文案（零桶不显示）。
    pub fn line(&self) -> String {
        let mut parts = vec![
            format!("{} 项", self.total),
            format!("已归档 {}", self.archived),
        ];
        for (count, label) in [
            (self.analysis, "分析表"),
            (self.table_ref, "引用"),
            (self.missing, "缺失"),
            (self.drifted, "索引异常"),
        ] {
            if count > 0 {
                parts.push(format!("{label} {count}"));
            }
        }
        parts.join(" · ")
    }

    /// 状态行完整文案：加载中时加前缀，但**不隐藏计数**——
    /// 那几项是上一次的库口径，抹掉反而让人以为库空了（原型 §5 的"加载中"行）。
    pub fn status_line(&self, loading: bool) -> String {
        let line = self.line();
        if loading {
            format!("加载中… · {line}")
        } else {
            line
        }
    }

    /// 是否有需要用户处理的异常（决定状态行是否给"修复…"入口）。
    pub fn has_issues(&self) -> bool {
        self.missing > 0 || self.drifted > 0
    }
}

/// 面板快照：宿主每次推送一整份（结构小，克隆代价可忽略）。
#[derive(Debug, Clone, Default)]
pub struct ResourcesSnapshot {
    pub rows: Vec<ArchiveRow>,
    /// 业务断言搜索索引（随快照一并下发，宿主推快照时同步进筛选器）。
    pub assertion_index: std::collections::HashMap<String, String>,
    /// 全局实例行（§13）：与 `rows` 分开存——全局行不参与项目标签 / 分组 / 异常扫描，
    /// 面板按「实例」维度拼接展示（`InstanceFilter`）。
    pub global_rows: Vec<ArchiveRow>,
    pub counts: ArchiveCounts,
    pub read_only: bool,
    /// 行 id → 详情快照（右侧详情面板用；与 `rows` 同一次取数产出，不在渲染期补取）。
    pub details: std::collections::HashMap<String, ArchiveDetail>,
    /// 标签字典（全部存活标签 + 用量）：筛选菜单与打标对话框共用，快照里带一份就不必再查库。
    pub tags: Vec<TagOption>,
    /// 分组字典（全部存活分组 + 成员数，按名字升序）：分组折叠区与「移动到分组」菜单共用。
    pub groups: Vec<GroupOption>,
}

/// 实例维度筛选（§13 双实例展示）：面板头三态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InstanceFilter {
    /// 项目 + 全局都显示。
    #[default]
    All,
    /// 只显示项目实例行。
    Project,
    /// 只显示全局实例行。
    Global,
}

impl InstanceFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Project => "项目",
            Self::Global => "全局",
        }
    }
}
