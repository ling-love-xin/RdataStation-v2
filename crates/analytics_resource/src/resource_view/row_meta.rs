//! 视图模型与行级纯函数（资源面板的「数据 + 判定」部分，不依赖面板实体）。
//!
//! 拆分自 `resource_view/mod.rs`（P1）：`ArchiveRow` / 分组 / 标签 / 计数 /
//! 实例维度等快照结构，以及强度徽标、行尾、行点击归一化等**纯函数**都在这里；
//! 不接触 `ResourcesPanel` 实体，便于单测。
//!
//! 对外符号契约保持 `crate::resource_view::X` 不变（`mod.rs` 里 `pub use` 承接）。

use gpui_kit::assets::IconName as CatalogIcon;
use gpui_kit::{ClickEvent, Modifiers};

use engine::file_reader_function;

use crate::detail_view::{ArchiveDetail, ArchiveTagChip};
use crate::model::{ArchiveKind, ArchiveStatus, TagTarget};

// ==================== 视图模型（纯数据，便于单测） ====================

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

/// 复现强度徽标文案（原型 §3.2：**行内唯一的颜色信号**）。
pub fn strength_badge(kind: ArchiveKind, status: ArchiveStatus) -> &'static str {
    match status {
        ArchiveStatus::Missing => "缺失",
        ArchiveStatus::ContentChanged => "内容已变",
        ArchiveStatus::Normal => match kind {
            ArchiveKind::File => "已归档",
            ArchiveKind::Analysis => "分析表",
            ArchiveKind::TableRef => "引用",
        },
    }
}

/// 徽标色调（纯枚举；渲染层再映射到主题色，便于单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeTone {
    Success,
    Info,
    Warning,
    Danger,
}

/// 徽标色调：异常态压过 kind；`引用`与 `内容已变` 同为警示。
pub fn badge_tone(kind: ArchiveKind, status: ArchiveStatus) -> BadgeTone {
    match (status, kind) {
        (ArchiveStatus::Missing, _) => BadgeTone::Danger,
        (ArchiveStatus::ContentChanged, _) => BadgeTone::Warning,
        (ArchiveStatus::Normal, ArchiveKind::File) => BadgeTone::Success,
        (ArchiveStatus::Normal, ArchiveKind::Analysis) => BadgeTone::Info,
        (ArchiveStatus::Normal, ArchiveKind::TableRef) => BadgeTone::Warning,
    }
}

/// kind 图标（形状区分，**不靠颜色**——原型 §6 的视觉通道预算：一行只允许一个颜色信号，
/// 而那个信号已经给了复现强度徽标）。
///
/// 三个形状：文件 = 文档形；分析表 = 表格形；引用 = 外链形。
///
/// `pub(crate)`：详情面板头部用同一套图标（同一口径只此一份）。
pub(crate) fn kind_icon(kind: ArchiveKind) -> CatalogIcon {
    match kind {
        ArchiveKind::File => CatalogIcon::FileText,
        ArchiveKind::Analysis => CatalogIcon::Table,
        ArchiveKind::TableRef => CatalogIcon::ExternalLink,
    }
}

/// 尾部字段：字段优先级为 显示名 > 强度徽标 > 版本徽标 > 相对时间 > 大小/规模。
///
/// 240px 面板放不下的信息一律省略号 + tooltip（渲染层补），**不缩行高、不做两行行**。
pub fn row_tail(detail: &str, modified: &str, version: i32) -> String {
    match (detail.is_empty(), modified.is_empty()) {
        (false, false) => format!("{detail} · {modified}"),
        (false, true) => detail.to_string(),
        (true, false) => modified.to_string(),
        // 两者都缺时给出版本，避免尾部整段空白。
        (true, true) => format!("v{version}"),
    }
}

/// 行右键菜单里「编辑标签」那一项的文案（多选时带项数）。
///
/// 为何抽成纯函数：菜单内部的项读不到（`PopupMenu` 不暴露），而文案是**用户看到的契约**
/// （“这一下会改几条”），所以把可断言的那一半拎出来（与 `dispatch_header_action` 同法）。
pub fn tag_entry_label(multi_count: usize) -> String {
    if multi_count > 1 {
        format!("编辑标签（{multi_count} 项）…")
    } else {
        "编辑标签…".to_string()
    }
}

/// 「编辑标签」要交给宿主的目标集（纯函数）。
///
/// 规则与删除 / 移动完全一致：**这一行在多选集里**→ 整选集（按可见行顺序，回执与测试都要确定性）；
/// 否则只给这一行。抽出来的理由：菜单里那一句只有真点开才能验，而“到底送了几项”
/// 是这张批量功能最容易错的地方（漏送 = 只改了用户点中的那一条）。
pub fn tag_targets(
    rows: &[&ArchiveRow],
    multi_ids: &std::collections::HashSet<String>,
    row: &ArchiveRow,
) -> Vec<TagTarget> {
    let multi_selected = multi_ids.len() > 1 && multi_ids.contains(&row.id);
    if multi_selected {
        rows.iter()
            .filter(|candidate| multi_ids.contains(&candidate.id))
            .map(|candidate| TagTarget {
                id: candidate.id.clone(),
                name: candidate.name.clone(),
            })
            .collect()
    } else {
        vec![TagTarget {
            id: row.id.clone(),
            name: row.name.clone(),
        }]
    }
}

/// 「默认分组」标记文案（分组头上的小字；也是窗口测试的锚点）。
pub const DEFAULT_GROUP_MARKER: &str = "默认";

/// 分组头菜单里那一项的文案（纯函数，带单测）：已是默认分组时是“取消”，否则是“设为”。
pub fn default_group_entry_label(is_default: bool) -> &'static str {
    if is_default {
        "取消归档默认分组"
    } else {
        "设为归档默认分组"
    }
}

/// 行是否可重命名（原型 §3.2 / §9：`F2` 与右键「重命名…」**同一判据**，两处各写一遍就会漂）。
///
/// 两条不许：多选（改名没有“批量”的意义——改哪一条都是猜）与只读项目（写不进去）。
pub fn can_rename(multi_count: usize, read_only: bool) -> bool {
    multi_count <= 1 && !read_only
}

/// 这条存档能否「查看统计」（M8 洞察）：判定按种类分，三类的可分析性来源不同。
///
/// - **受管文件**：本体得是 DuckDB 读得动的数据文件——扩展名口径只有一处
///   （[`engine::file_reader_function`]：CSV / Parquet / Excel / JSON）；
///   `.sql` / `.md` 这类文本不是数据，不给入口。
/// - **远端引用**：有来源连接与表名就能取样（取样 SQL 由宿主按驱动拼，不在面板）。
/// - **分析表**：本体是项目内的 `analytics.duckdb`，要 ATTACH + 重建定义——随后续批次接。
///
/// 缺失的行一律不给：本体不在，取不到数。
pub fn can_view_stats(detail: &ArchiveDetail) -> bool {
    if detail.status == ArchiveStatus::Missing {
        return false;
    }
    match detail.kind {
        ArchiveKind::File => detail
            .payload_rel_path
            .as_deref()
            .and_then(file_reader_function)
            .is_some(),
        ArchiveKind::TableRef => {
            detail.source_connection_id.is_some() && detail.source_table.is_some()
        }
        ArchiveKind::Analysis => false,
    }
}

/// 行点击归一化后的语义（修饰键与点击数 → 四种行为；纯数据便于单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowClick {
    /// 无修饰：单选这一行。
    Select,
    /// `Ctrl` / `Cmd`：切换这一行的多选状态。
    Toggle,
    /// `Shift`：从锚点到这一行的区间选择。
    Range,
    /// 双击：打开（只读）。
    Open,
}

/// 归一化行点击（原型 §2.3：单击选中 / `Ctrl`·`Shift` 多选 / 双击打开）。
///
/// 判定顺序：**双击优先**（双击必然也带 click_count ≥ 2）——打开是更强的意图；
/// `Shift` 优先于 `Ctrl`（两个都按时按区间处理，与多数桌面应用一致）。
pub fn classify_row_click(event: &ClickEvent) -> RowClick {
    classify_click(event.click_count(), event.modifiers())
}

/// [`classify_row_click`] 的纯函数内核（事件取值与判定分开：判定可单测）。
pub(crate) fn classify_click(click_count: usize, modifiers: Modifiers) -> RowClick {
    if click_count >= 2 {
        return RowClick::Open;
    }
    if modifiers.shift {
        return RowClick::Range;
    }
    // gpui-pre 0.3.10 起 `secondary()` 需 `&App` 参数；本函数为纯函数（无 cx），
    // 改用编译期平台判断实现相同语义（macOS = Command 键，其余 = Ctrl）。
    let secondary = if cfg!(target_os = "macos") {
        modifiers.platform
    } else {
        modifiers.control
    };
    if secondary {
        return RowClick::Toggle;
    }
    RowClick::Select
}

/// 行点击对选择集的变换（纯函数）：`multi` 始终按可见行顺序维护，`anchor` 是区间锚点。
///
/// 返回**焦点行**（= 最后点的那条；选择集为空时 `None`）。`order` 是当前可见行顺序。
pub(crate) fn apply_row_click(
    multi: &mut Vec<String>,
    anchor: &mut Option<String>,
    order: &[String],
    id: &str,
    click: RowClick,
) -> Option<String> {
    match click {
        RowClick::Select | RowClick::Open => {
            *multi = vec![id.to_string()];
            *anchor = Some(id.to_string());
        }
        RowClick::Toggle => {
            if let Some(index) = multi.iter().position(|candidate| candidate == id) {
                // 取消最后一条 = 回到无选中（与多数列表一致）。
                multi.remove(index);
            } else {
                multi.push(id.to_string());
                sort_multi_by_order(multi, order);
            }
            *anchor = Some(id.to_string());
        }
        RowClick::Range => {
            let target = order.iter().position(|candidate| candidate == id);
            let from = anchor
                .as_deref()
                .and_then(|anchor| order.iter().position(|candidate| candidate == anchor));
            match (from, target) {
                (Some(from), Some(target)) => {
                    let (low, high) = if from <= target {
                        (from, target)
                    } else {
                        (target, from)
                    };
                    *multi = order[low..=high].to_vec();
                }
                // 锚点不在了（被筛掉 / 从没点过）：退化为单选。
                _ => {
                    *multi = vec![id.to_string()];
                    *anchor = Some(id.to_string());
                }
            }
        }
    }
    (!multi.is_empty()).then(|| id.to_string())
}

/// 按可见行顺序排序（不在列表里的排最后：集合语义，但顺序影响批量回执与测试）。
fn sort_multi_by_order(multi: &mut Vec<String>, order: &[String]) {
    multi.sort_by_key(|id| {
        order
            .iter()
            .position(|candidate| candidate == id)
            .unwrap_or(usize::MAX)
    });
}

