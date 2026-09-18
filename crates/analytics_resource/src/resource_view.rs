//! 资产库面板（M6 自持视图，左 Dock）。
//!
//! # 归属
//!
//! 视图随能力同 crate（与 `project::ui` / `mock::mock_view` 同例）；本 crate **不依赖 workbench**，
//! 宿主能力经 [`ResourcesHost`] 注入（workbench 侧桥接留待接线那一批）。
//!
//! # 渲染纪律
//!
//! - **render 期零 I/O**：面板只读自己持有的快照（[`ArchiveRow`] 由宿主取好并格式化）；
//!   一切服务调用（归档 / 取回 / 打开 / 扫描）都在事件路径经宿主发起。
//! - 面板不知道 `project.db` / `resources/` 的存在，也不该知道。
//!
//! # 本批范围（Phase 1 第五刀）
//!
//! 已落地：面板头（标题 + 归档入口）、**工具栏（搜索框 / 筛选 / 排序）**、提示行（只读与通知分色）、
//! **行列表（`list::List`：虚拟化 + 组件化 hover / 选中 / 键盘漫游）**、**kind 图标**、
//! **行的右键菜单**（打开 / 取回 / 移入回收站）、底部状态行（异常时给"修复…"入口）、
//! 空态**与"无匹配"两态分开**、只读禁写。
//!
//! **未落地（下一批，已在开发方案留档）**：详情属性面板接入、五个对话框、Action 与快捷键
//! （`Ctrl+F` 聚焦 / `Esc` 清空 / 行漫游在 Action 批）、行内 hover 动作（原型 §2.3 的 hover 版，
//! 随详情面板批）、批量多选。

use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::component::IndexPath;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::{List, ListDelegate, ListItem, ListState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use engine::dbi::engine::duckdb_engine::DuckDBEngine;

/// kind 图标取**完整 Lucide 目录**（`gpui_kit::assets`）：组件子集（`default-icons.txt`）
/// 里没有表格形；另外用别名避免与组件子集的同名枚举混淆。
use gpui_kit::assets::IconName as CatalogIcon;

use crate::commands;
use crate::detail_view::ArchiveDetail;
use crate::filter::{self, ResourcesFilter, SortField, SortOrder, VisibleItem};
use crate::model::{ArchiveKind, ArchiveStatus, ArchiveUndo};
use crate::ui;

// ==================== 视图模型（纯数据，便于单测） ====================

/// 一行存档：**宿主已格式化好的快照**（面板渲染不做计算与 I/O）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveRow {
    pub id: String,
    pub name: String,
    pub kind: ArchiveKind,
    pub version: i32,
    pub status: ArchiveStatus,
    /// 尾部字段（按字段优先级规则拼好，见 [`row_tail`]）。
    pub tail: String,
    /// 这行挂的标签 id（筛选的标签维用它；名字会改，所以比的是 id）。
    pub tag_ids: Vec<String>,
    /// 这行归属的分组（`None` = 未分组）。
    ///
    /// 单层分组（架构 D8）：最多一个，所以是 `Option` 而不是集合——类型上就把
    /// “一个资源同时属于两个分组”这种不存在的状态挡掉。
    pub folder_id: Option<String>,
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
    pub counts: ArchiveCounts,
    pub read_only: bool,
    /// 行 id → 详情快照（右侧详情面板用；与 `rows` 同一次取数产出，不在渲染期补取）。
    pub details: std::collections::HashMap<String, ArchiveDetail>,
    /// 标签字典（全部存活标签 + 用量）：筛选菜单与打标对话框共用，快照里带一份就不必再查库。
    pub tags: Vec<TagOption>,
    /// 分组字典（全部存活分组 + 成员数，按名字升序）：分组折叠区与「移动到分组」菜单共用。
    pub groups: Vec<GroupOption>,
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

/// 这条存档能否「查看统计」（M8 洞察）：判定按种类分，三类的可分析性来源不同。
///
/// - **受管文件**：本体得是 DuckDB 读得动的数据文件——扩展名口径只有一处
///   （[`DuckDBEngine::file_reader_function`]：CSV / Parquet / Excel / JSON）；
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
            .and_then(DuckDBEngine::file_reader_function)
            .is_some(),
        ArchiveKind::TableRef => {
            detail.source_connection_id.is_some() && detail.source_table.is_some()
        }
        ArchiveKind::Analysis => false,
    }
}

// ==================== 宿主契约 ====================

/// 宿主能力：面板不认识服务层与会话，动作一律回宿主（事件路径执行）。
///
/// 每个动作都带上 `Window` / `App`：宿主侧要开对话框与只读打开（都需要窗口句柄），
/// 而面板天然持有它们（`on_click` / `on_action` 都提供）——先把上下文带出去，比将来改签名轻。
pub trait ResourcesHost: 'static {
    /// 归档入口（面板头「＋ ▾」第一项）：从**草稿箱**选文件归档（宿主弹草稿多选对话框）。
    fn request_archive_from_drafts(&self, window: &mut Window, cx: &mut App);
    /// 归档入口（面板头「＋ ▾」第二项）：从**本地文件**选（宿主弹系统文件选择）。
    fn request_archive_from_file(&self, window: &mut Window, cx: &mut App);
    /// 打开（只读）：宿主以**编辑器只读**打开本体——改它要先去取回。
    ///
    /// 与 `request_checkout` 同理：收的是**面板已有的那条详情**（本体路径在它身上），
    /// 宿主不必回头读面板的选中态。
    fn request_open(&self, detail: &ArchiveDetail, window: &mut Window, cx: &mut App);
    /// 在**洞察面板**里看这条存档的数据统计（M8：右键「查看统计」）。
    ///
    /// 与 [`Self::request_open`] 同理：只收**面板已有的那条详情**（本体位置 / 来源连接
    /// 都在它身上），宿主不必回头读面板的选中态。只有 [`can_view_stats`] 为真的行
    /// 会摆这一项；宿主仍按种类分流，走不通的那类要给回执而不是静默。
    fn request_view_stats(&self, detail: &ArchiveDetail, window: &mut Window, cx: &mut App);
    /// 在系统文件管理器里显示本体（原型 §3.2：选中文件而不是只开目录）。
    fn request_reveal(&self, detail: &ArchiveDetail, window: &mut Window, cx: &mut App);
    /// 复制本体绝对路径到剪贴板（原型 §3.2）。
    fn request_copy_path(&self, detail: &ArchiveDetail, window: &mut Window, cx: &mut App);
    /// 取回（检出）。
    ///
    /// 参数给的是**面板已有的那条详情**（而不是一个 id）：宿主据此命工作副本名与
    /// 版本提示，不必回头读面板的选中态——那是渲染期正在被借用的对象（已踩过）。
    fn request_checkout(&self, detail: &ArchiveDetail, window: &mut Window, cx: &mut App);
    /// 打开版本历史对话框（右键菜单「版本历史…」、详情面板「查看全部…」与索引修复的
    /// 「打开版本历史…」共用）。
    ///
    /// 只收 id：对话框的数据（含显示名）由宿主在取数线程上查库得到，
    /// 调用方不必为了一个 id 去克隆整条详情。
    fn request_version_history(&self, resource_id: &str, window: &mut Window, cx: &mut App);
    /// 撤销上一次归档（底部撤销栏的「撤销」按钮）。
    ///
    /// 凭据里的原路径只在内存里活一会儿（见 [`ArchiveUndo`]）：失效了就调不到这里。
    fn request_undo_archive(&self, undo: &ArchiveUndo, window: &mut Window, cx: &mut App);
    /// 移入回收站（单选一行或多选 N 行）。
    ///
    /// 收切片而不是单个 id：多选批量与单选走同一条路（批量时确认框与回执要带数量）；
    /// 批量中途失败的错误里会带“本批已移入 N 项”，**不做预回滚**（部分成功就部分成功）。
    fn request_delete(&self, resource_ids: &[String], window: &mut Window, cx: &mut App);
    /// 打开「标签」对话框（详情面板「＋ 标签」）：勾选/取消标签、顺带新建。
    ///
    /// 只收 id 与显示名：对话框的数据由宿主在取数线程上查库得到（与 `request_version_history`
    /// 同一口径——调用方不必为了一个 id 去克隆整条详情）。
    fn request_edit_tags(&self, resource_id: &str, resource_name: &str, window: &mut Window, cx: &mut App);
    /// 去掉一个标签（详情面板 chip 上的 ×）。
    ///
    /// 与其它动作同口径：收**面板已有的那条详情**（回执文案要显示名），不回头读面板选中态。
    /// 不进对话框：点 × 就是“去掉它”，再要一次确认是多余的。
    fn request_remove_tag(&self, detail: &ArchiveDetail, tag_id: &str, window: &mut Window, cx: &mut App);
    /// 索引修复入口（状态行异常段的「修复…」与面板头「⋯ → 重建索引…」共用）。
    fn request_index_repair(&self, window: &mut Window, cx: &mut App);
    /// 用系统文件管理器打开受管内容根（面板头「⋯ → 打开资源目录」）。
    ///
    /// 开的是**目录**（与行的「在系统中显示」不同：那条要选中某个本体文件）。
    fn request_open_payload_dir(&self, window: &mut Window, cx: &mut App);
    /// 打开资源回收站（面板头「⋯ → 回收站…」）。
    ///
    /// 回收站是**项目级**的（与草稿箱共用一处，模块硬约束 5）：对话框只列本模块的条目，
    /// 别人的只给一句说明（还原归它自己的模块）。
    fn request_open_trash(&self, window: &mut Window, cx: &mut App);
    /// 重新取数（面板头「⋯ → 刷新」）。
    ///
    /// 与「打开 / 切换项目」同一条取数路径；不做局部刷新——列表是一整份快照。
    fn request_refresh(&self, window: &mut Window, cx: &mut App);
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
fn classify_click(click_count: usize, modifiers: Modifiers) -> RowClick {
    if click_count >= 2 {
        return RowClick::Open;
    }
    if modifiers.shift {
        return RowClick::Range;
    }
    if modifiers.secondary() {
        return RowClick::Toggle;
    }
    RowClick::Select
}

/// 行点击对选择集的变换（纯函数）：`multi` 始终按可见行顺序维护，`anchor` 是区间锚点。
///
/// 返回**焦点行**（= 最后点的那条；选择集为空时 `None`）。`order` 是当前可见行顺序。
fn apply_row_click(
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

/// 面板头「⋯」菜单的动作（原型 §2.1 四项；顺序即菜单顺序）。
///
/// 单独成枚举而不是散在渲染闭包里：菜单项文案、顺序与去向可以被单测钉住
/// ——弹层点击在窗口测试里模拟不了，这是那份测试的入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderMenuAction {
    /// 重建索引…（三类孤儿的人工确认入口，与状态行「修复…」同一条路）。
    RebuildIndex,
    /// 打开资源目录（`{项目}/resources/`）。
    OpenPayloadDir,
    /// 回收站…
    OpenTrash,
    /// 刷新。
    Refresh,
}

impl HeaderMenuAction {
    /// 菜单顺序（原型 §2.1 自上而下：重建索引… / 打开资源目录 / 回收站… / 刷新）。
    pub const ALL: [Self; 4] = [
        Self::RebuildIndex,
        Self::OpenPayloadDir,
        Self::OpenTrash,
        Self::Refresh,
    ];

    /// 菜单项文案（省略号表示它会开一个对话框/入口，而不是立即生效）。
    pub fn label(self) -> &'static str {
        match self {
            Self::RebuildIndex => "重建索引…",
            Self::OpenPayloadDir => "打开资源目录",
            Self::OpenTrash => "回收站…",
            Self::Refresh => "刷新",
        }
    }
}

/// 分发一个面板头菜单动作：面板不认识服务层，动作原样转给宿主。
///
/// 独立成函数（而不是写在菜单闭包里）是为了让窗口测试能走**生产入口**调用它：
/// 弹层里的菜单项没法在测试里点到。
pub fn dispatch_header_action(
    host: &Rc<dyn ResourcesHost>,
    action: HeaderMenuAction,
    window: &mut Window,
    cx: &mut App,
) {
    match action {
        HeaderMenuAction::RebuildIndex => host.request_index_repair(window, cx),
        HeaderMenuAction::OpenPayloadDir => host.request_open_payload_dir(window, cx),
        HeaderMenuAction::OpenTrash => host.request_open_trash(window, cx),
        HeaderMenuAction::Refresh => host.request_refresh(window, cx),
    }
}

// ==================== 列表委托 ====================

/// 存档列表的 `List` 委托（虚拟化 + 组件化 hover / 选中 / 键盘漫游）。
///
/// 行数据是**面板可见行的一份副本**：`render_item` 在列表渲染期被调用，而那一刻面板实体
/// 正被借用（渲染就是从面板的 `render` 进来的），回头去读面板的行集合会直接 panic——
/// 与 `mock` 的生成器搜索委托同例。面板每次重算可见行都把副本推过来。
struct ArchiveListDelegate {
    /// 动作去向（右键菜单用；面板不认识服务层）。
    host: Rc<dyn ResourcesHost>,
    /// 面板句柄（选中变化回传；面板已销毁时静默丢弃）。
    panel: WeakEntity<ResourcesPanel>,
    /// 可见项（分组头 + 行）：分区渲染后列表的“行”就是它们。
    items: Vec<VisibleItem>,
    /// 行 id → 详情：右键菜单发起取回时要用整条详情（`render_item` 里读不了面板，只能提前拷一份）。
    details: std::collections::HashMap<String, ArchiveDetail>,
    /// 选中的行 id（面板是语义权威，这里是渲染与漫游的锚点）。
    selected_id: Option<String>,
    /// 多选集合（**含焦点行**）：行背景与右键菜单的“多选态”靠它；
    /// 选中变更时由面板推一份新副本过来（`render_item` 里读不了面板）。
    multi_ids: std::collections::HashSet<String>,
    /// 面板正在把自己的选中镜像进列表。
    ///
    /// 此间组件回调的 `set_selected_index` **不再回写面板**：镜像发生在面板渲染期，
    /// 回写就是"更新正在被更新的实体"（GPUI 直接 panic）。
    syncing_from_panel: bool,
    /// 项目只读（标题栏锁）：只读时行内动作只剩「打开」。
    read_only: bool,
}

impl ArchiveListDelegate {
    /// 面板推送可见项（快照 / 筛选 / 排序 / 折叠变化时调用）。
    fn set_rows(
        &mut self,
        items: Vec<VisibleItem>,
        details: std::collections::HashMap<String, ArchiveDetail>,
        selected_id: Option<String>,
        multi_ids: std::collections::HashSet<String>,
        read_only: bool,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.items = items;
        self.details = details;
        self.selected_id = selected_id;
        self.multi_ids = multi_ids;
        self.read_only = read_only;
        cx.notify();
    }

    /// 多选态（> 1 条）且这一行在其中：行背景由我们画（组件的选中样式只认它的单选索引）。
    fn multi_selected(&self, id: &str) -> bool {
        self.multi_ids.len() > 1 && self.multi_ids.contains(id)
    }

    /// 可见行（跳过分组头）：选择集顺序、行内菜单都按它算。
    fn visible_rows(&self) -> impl Iterator<Item = &ArchiveRow> {
        self.items.iter().filter_map(|item| match item {
            VisibleItem::Row(row) => Some(row),
            VisibleItem::GroupHeader { .. } => None,
        })
    }

    /// 索引 → 行（越界或落在分组头上则 `None`：行集合刚变的那一帧可能还拿着旧索引）。
    fn row_at(&self, ix: IndexPath) -> Option<&ArchiveRow> {
        match self.items.get(ix.row) {
            Some(VisibleItem::Row(row)) => Some(row),
            _ => None,
        }
    }

    /// 分组头（原型 §2.4）：色条 + 折叠三角 + 名称 + 计数；点整行折叠 / 展开。
    fn render_group_header(
        &self,
        key: &str,
        label: &str,
        count: usize,
        depth: u8,
        collapsed: bool,
        cx: &mut Context<ListState<Self>>,
    ) -> ListItem {
        let (bar, muted, fg) = {
            let colors = cx.theme().colors;
            (colors.list_active_border, colors.muted_foreground, colors.foreground)
        };
        let panel = self.panel.clone();
        let key_owned = key.to_string();
        let chevron = if collapsed { "▸" } else { "▾" };
        let list_id = format!("archive-group-{key}");
        let debug_id = list_id.clone();
        ListItem::new(SharedString::from(list_id.clone())).child(
            div()
                .id(SharedString::from(format!("{list_id}-row")))
                .debug_selector(move || debug_id.clone())
                .w_full()
                .h(rems(ui::ROW_HEIGHT))
                .h_flex()
                .items_center()
                .gap_2()
                .when(depth > 0, |row| row.pl_2())
                .cursor_pointer()
                .on_click(move |_, _window, cx| {
                    // 头不是行：不参与选中（面板的选中永远指向一条存档）。
                    cx.stop_propagation();
                    let key = key_owned.clone();
                    let _ = panel.update(cx, |panel, cx| panel.toggle_group_collapse(&key, cx));
                })
                .child(
                    // 2px 色条：`list_active_border`（原型 §6——不用 `sidebar_accent`：
                    // 那个角色在浅色下与面板底几乎同色）。
                    div().w(rems(ui::GROUP_BAR_WIDTH)).h(rems(1.0)).flex_none().bg(bar),
                )
                .child(div().flex_none().text_xs().text_color(muted).child(chevron))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_ellipsis()
                        .text_color(if depth == 0 { fg } else { muted })
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{count}")),
                ),
        )
    }
}

impl ListDelegate for ArchiveListDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.items.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        // 分组头：与行同一套虚拟化（它是列表的一项，不是另画的一层）。
        if let Some(VisibleItem::GroupHeader {
            key,
            label,
            count,
            depth,
            collapsed,
        }) = self.items.get(ix.row).cloned()
        {
            return Some(self.render_group_header(&key, &label, count, depth, collapsed, cx));
        }
        let row = self.row_at(ix)?.clone();
        let (foreground, muted, success, info, warning, danger) = {
            let colors = cx.theme().colors;
            (
                colors.foreground,
                colors.muted_foreground,
                colors.success,
                colors.info,
                colors.warning,
                colors.danger,
            )
        };
        let badge = strength_badge(row.kind, row.status);
        let badge_color = match badge_tone(row.kind, row.status) {
            BadgeTone::Success => success,
            BadgeTone::Info => info,
            BadgeTone::Warning => warning,
            BadgeTone::Danger => danger,
        };
        // 缺失的行灰显（不是禁用：它仍可被选中看详情、仍可走索引修复）。
        let name_color = if row.status == ArchiveStatus::Missing {
            muted
        } else {
            foreground
        };
        let host = self.host.clone();
        let multi_bg = cx.theme().colors.list_active;
        // 多选态（> 1 条）：单行动作一律置灰（原型 §3.2“单/多选 → 单选可用”），
        // 删除项改为对**整个选择集**生效并带数量。
        let multi_count = self.multi_ids.len();
        let multi_selected = self.multi_selected(&row.id);
        let multi_delete_ids: Vec<String> = if multi_selected {
            // 按可见行顺序过滤（HashSet 迭代序不定）：回执与测试都要确定性。
            self.visible_rows()
                .filter(|candidate| self.multi_ids.contains(&candidate.id))
                .map(|candidate| candidate.id.clone())
                .collect()
        } else {
            vec![row.id.clone()]
        };
        // 取回要往草稿箱写一份工作副本：本体异常的存档（缺失 / 内容已变）与只读项目都不给走。
        let can_checkout = row.status == ArchiveStatus::Normal && !self.read_only;
        // 打开 / 取回都要整条详情（本体路径 / 扩展名在它身上）：渲染期提前拷一份——
        // 菜单回调触发时面板可能已被借用，不能再回头读。
        let open_detail = self.details.get(&row.id).cloned();
        // 「打开（只读）」要本体在位（缺失的行没什么可开的，旧行也可能没登记路径）。
        let can_open = open_detail
            .as_ref()
            .map(|detail| {
                detail.status != ArchiveStatus::Missing && detail.payload_rel_path.is_some()
            })
            .unwrap_or(false);
        // 「查看统计」只对**数据可得**的行摆出来（口径见 `can_view_stats`）。
        let can_stats = open_detail.as_ref().is_some_and(can_view_stats);
        let checkout_detail = open_detail.clone();
        // 版本历史的入口只带 id（对话框的显示名由宿主查库得到）。
        let id_versions = row.id.clone();

        let mut line = div()
            .h_flex()
            .w_full()
            .min_w_0()
            .h(rems(ui::ROW_HEIGHT))
            .gap_2()
            .child(
                // kind 图标：一律 muted（颜色信号留给复现强度徽标）；
                // `size_3p5` 是 `ui::ICON_SIZE_SM`（0.875rem = 14px）的 Tailwind 等价写法。
                Icon::new(kind_icon(row.kind))
                    .flex_none()
                    .size_3p5()
                    .text_color(muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_sm()
                    .text_ellipsis()
                    .text_color(name_color)
                    .child(row.name.clone()),
            )
            // v1 不显示版本徐标（减少噪声）。
            .when(row.version > 1, |line| {
                line.child(
                    div()
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("v{}", row.version)),
                )
            })
            .child(
                div()
                    .flex_none()
                    .h(rems(ui::ARCHIVE_BADGE_HEIGHT))
                    .text_xs()
                    .text_color(badge_color)
                    .child(badge),
            );
        if !row.tail.is_empty() {
            line = line.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(row.tail.clone()),
            );
        }

        // 行点击自己接管（原型 §2.3）：组件默认把**单击**接到 `confirm`（= 打开），
        // 而我们要求“单击=选中、双击=打开”——从源头掐掉它，外层的确认通道只留给 `Enter`。
        let click_detail = open_detail.clone();
        let click_id = row.id.clone();
        let click_handler = {
            let host = host.clone();
            let panel = self.panel.clone();
            move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
                cx.stop_propagation();
                let click = classify_row_click(event);
                let id = click_id.clone();
                let _ = panel.update(cx, |panel, cx| panel.handle_row_click(&id, click, cx));
                if click == RowClick::Open {
                    if let Some(detail) = click_detail.as_ref() {
                        host.request_open(detail, window, cx);
                    }
                }
            }
        };

        Some(
            ListItem::new(SharedString::from(format!("archive-row-{}", row.id))).child(
                // 行的动作入口是右键菜单（原型 §3.2）：行本身只承载信息——240px 面板里
                // 常驻按钮会把尾部字段挤没（hover 版随详情面板批）。
                //
                // 菜单挂在行内的容器上（而不是 ListItem 本身）：`context_menu` 返回的是
                // `ContextMenu<ListItem>`，而委托的 `Item` 必须是 `ListItem`（它要实现
                // `Selectable`）；容器带**按行 id 的稳定 id**，否则各行会共用同一个菜单位。
                div()
                    .id(SharedString::from(format!("archive-row-menu-{}", row.id)))
                    .w_full()
                    // 多选高亮自己画：组件的选中样式只认它的**单选**索引（= 焦点行）。
                    .when(multi_selected, |container| container.bg(multi_bg))
                    .on_click(click_handler)
                    .child(line)
                    .context_menu(move |menu, _window, _cx| {
                        let mut menu = menu;
                        menu = menu.item(
                            PopupMenuItem::new("打开（只读）")
                                .disabled(!can_open || multi_count > 1)
                                .on_click({
                                    let host = host.clone();
                                    let detail = open_detail.clone();
                                    move |_, window, cx| {
                                        if let Some(detail) = detail.as_ref() {
                                            host.request_open(detail, window, cx);
                                        }
                                    }
                                }),
                        );
                        menu = menu.item(
                            PopupMenuItem::new("查看统计")
                                .disabled(!can_stats || multi_count > 1)
                                .on_click({
                                    let host = host.clone();
                                    let detail = open_detail.clone();
                                    move |_, window, cx| {
                                        if let Some(detail) = detail.as_ref() {
                                            host.request_view_stats(detail, window, cx);
                                        }
                                    }
                                }),
                        );
                        menu = menu.item(
                            PopupMenuItem::new("取回（检出）…")
                                .disabled(!can_checkout || multi_count > 1)
                                .on_click({
                                    let host = host.clone();
                                    let detail = checkout_detail.clone();
                                    move |_, window, cx| {
                                        if let Some(detail) = detail.as_ref() {
                                            host.request_checkout(detail, window, cx);
                                        }
                                    }
                                }),
                        );
                        menu = menu.item(
                            PopupMenuItem::new("版本历史…")
                                .disabled(multi_count > 1)
                                .on_click({
                                    let host = host.clone();
                                    let id = id_versions.clone();
                                    move |_, window, cx| {
                                        host.request_version_history(&id, window, cx)
                                    }
                                }),
                        );
                        // 破坏性项用分隔线隔离；其上的两项是"本体在哪儿"的日常动作（原型 §3.2）。
                        menu = menu.item(
                            PopupMenuItem::new("复制路径")
                                .disabled(!can_open || multi_count > 1)
                                .on_click({
                                    let host = host.clone();
                                    let detail = open_detail.clone();
                                    move |_, window, cx| {
                                        if let Some(detail) = detail.as_ref() {
                                            host.request_copy_path(detail, window, cx);
                                        }
                                    }
                                }),
                        );
                        menu = menu.item(
                            PopupMenuItem::new("在系统中显示")
                                .disabled(!can_open || multi_count > 1)
                                .on_click({
                                    let host = host.clone();
                                    let detail = open_detail.clone();
                                    move |_, window, cx| {
                                        if let Some(detail) = detail.as_ref() {
                                            host.request_reveal(detail, window, cx);
                                        }
                                    }
                                }),
                        );
                        // 删除是**唯一**多选可用的项（原型 §3.2）：多选时带上数量。
                        let delete_ids = multi_delete_ids.clone();
                        let delete_label = if multi_count > 1 {
                            format!("移入回收站（{multi_count} 项）")
                        } else {
                            "移入回收站".to_string()
                        };
                        menu.separator().item(PopupMenuItem::new(delete_label).on_click({
                            let host = host.clone();
                            move |_, window, cx| host.request_delete(&delete_ids, window, cx)
                        }))
                    }),
            ),
        )
    }

    /// 组件把选中变化回传（点击 / 键盘漫游）：语义上仍以面板的 `selected` 为准。
    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let id = ix.and_then(|ix| self.row_at(ix).map(|row| row.id.clone()));
        if self.syncing_from_panel {
            // 面板镜像：只按它更新锚点，不回写（见字段注释）。
            self.selected_id = id;
            return;
        }
        // 同一行不重复通知：面板渲染时会把自己的选中镜像回列表（见 `sync_list_selection`），
        // 没有这层防抖就会变成"面板 → 列表 → 面板"的渲染循环。
        if id == self.selected_id {
            return;
        }
        self.selected_id = id.clone();
        let _ = self.panel.update(cx, |panel, cx| panel.set_selected(id, cx));
    }

    /// 回车 / 双击：打开（只读），与右键菜单第一项同口径。
    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        let Some(detail) = self
            .selected_id
            .as_deref()
            .and_then(|id| self.details.get(id))
            .cloned()
        else {
            return;
        };
        // 本体缺失 / 旧行没登记路径：回车与菜单项同口径地什么都不做（菜单项也是禁用的）。
        if detail.status == ArchiveStatus::Missing || detail.payload_rel_path.is_none() {
            return;
        }
        self.host.request_open(&detail, window, cx);
    }
}

// ==================== 面板 ====================

pub struct ResourcesPanel {
    host: Rc<dyn ResourcesHost>,
    snapshot: ResourcesSnapshot,
    /// 筛选 + 排序后的可见行。
    ///
    /// **事件路径维护**（快照推送 / 条件变更时重算），render 只读：每帧重算会在
    /// 滚动时持续分配（行集合克隆），且把"规则"搬回渲染路径。
    view_rows: Vec<ArchiveRow>,
    /// 可见项（分组头 + 行，见 `filter::build_visible_items`）：列表委托渲染的就是它。
    ///
    /// 与 `view_rows` 的关系：`view_rows` 是“筛选 + 排序后的行”（选中 / 多选 / 测试读它），
    /// `view_items` 是“再按分组分区与折叠状态展开后的列表项”。
    view_items: Vec<VisibleItem>,
    /// 已折叠的分组 key（含 `GROUP_ALL` / `GROUP_UNGROUPED`）。
    ///
    /// 会话级（面板内）：原型要求“折叠状态持久化”，而设置项（`settings.json`）
    /// 属 P2.4——在那之前不假装持久化，重开项目回到展开态。
    collapsed: std::collections::HashSet<String>,
    /// 工具栏条件（搜索词 / 种类 / 只看需处理）。
    filter: ResourcesFilter,
    sort_field: SortField,
    sort_order: SortOrder,
    /// 搜索框（**首帧渲染时创建**）。
    ///
    /// `InputState::new` 需要 `&mut Window`，而工作台侧的侧栏面板构造期拿不到窗口
    /// （`SidebarPanel::new` 无窗口参数）——懒创建是本仓库 crate 内面板的既定处理
    /// （`mock::mock_view::MockPanel` 的输入框同例）。
    search_input: Option<Entity<InputState>>,
    /// 搜索框订阅句柄（仅持有；释放即取消）。
    _search_sub: Option<Subscription>,
    /// 行列表状态（**首帧渲染时创建**：`ListState` 同样需要窗口；空态不渲染它）。
    list: Option<Entity<ListState<ArchiveListDelegate>>>,
    selected: Option<String>,
    /// 多选集合（按可见行顺序；单选时就是 `selected` 那一条，空 = 无选中）。
    ///
    /// 与 `selected` 并存：`selected` 是**焦点行**（详情面板读它），多选只是额外的选择集
    /// （原型 §3.2：多选可用的动作只有批量删除，其余仍是单行语义）。
    multi: Vec<String>,
    /// `Shift` 区间的锚点：最后一次“无修饰 / Ctrl”点击的行。
    anchor: Option<String>,
    notice: Option<String>,
    /// 正在取数（宿主在事件路径置位；原型 §5 的"加载中"行）。
    ///
    /// 用途有二：状态行加前缀，以及**空库那一帧不闪**（没有它，首个快照到达前
    /// 会先给用户看一眼"还没有任何存档"）。
    loading: bool,
    /// 上一次归档的撤销凭据（**只在内存里活几秒**，见 [`ArchiveUndo`]）。
    ///
    /// 由宿主在归档成功后推进来（面板不自己造），过期或下一次动作时清掉。
    undo: Option<ArchiveUndo>,
    focus_handle: FocusHandle,
    group: Option<WeakEntity<TabGroup>>,
}

impl ResourcesPanel {
    pub const PANEL_NAME: &'static str = "analytics_resource";

    pub fn new(host: Rc<dyn ResourcesHost>, cx: &mut Context<Self>) -> Self {
        Self {
            host,
            snapshot: ResourcesSnapshot::default(),
            view_rows: Vec::new(),
            view_items: Vec::new(),
            collapsed: std::collections::HashSet::new(),
            filter: ResourcesFilter::default(),
            sort_field: SortField::default(),
            sort_order: SortOrder::default(),
            search_input: None,
            _search_sub: None,
            list: None,
            selected: None,
            multi: Vec::new(),
            anchor: None,
            notice: None,
            loading: false,
            undo: None,
            focus_handle: cx.focus_handle(),
            group: None,
        }
    }

    /// 懒创建搜索框与订阅（首帧渲染时执行一次，见 `search_input` 字段注释）。
    fn ensure_search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search_input.is_some() {
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索存档"));
        // 用户真在打字的那条路径：输入变化 → 条件更新 → 重算可见行。
        //
        // 注意：`InputState::set_value` **不会**发 `InputEvent::Change`（gpui-component 内部注释明说），
        // 所以程序性改词（清空筛选 / 宿主预填）必须自己同步条件（见 `set_query` / `clear_filter`），
        // 不能指望这个订阅——否则输入框里的字与实际生效的条件会静默不一致。
        let sub = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, emitter, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                // 不用 `set_query`：它会把同一个值再写回输入框（正在输入时是无谓的往返）。
                this.filter.query = emitter.read(cx).value().to_string();
                this.refresh_view_rows(cx);
                cx.notify();
            },
        );
        self.search_input = Some(input);
        self._search_sub = Some(sub);
    }

    /// 懒创建列表状态（首帧渲染时执行一次，理由同 [`ensure_search_input`](Self::ensure_search_input)）。
    ///
    /// 空态（无可见行）时面板不渲染列表，但状态照建：行回来时直接可用，
    /// 不用在"有行的那一帧"再走一次创建。
    fn ensure_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list.is_some() {
            return;
        }
        let delegate = ArchiveListDelegate {
            host: self.host.clone(),
            panel: cx.entity().downgrade(),
            items: self.view_items.clone(),
            details: self.snapshot.details.clone(),
            selected_id: self.selected.clone(),
            multi_ids: self.multi.iter().cloned().collect(),
            syncing_from_panel: false,
            read_only: self.snapshot.read_only,
        };
        let state = cx.new(|cx| ListState::new(delegate, window, cx).selectable(true));
        self.list = Some(state);
    }

    /// 把面板的选中（行 id）同步成列表的选中（索引）。
    ///
    /// 只在真的不一致时才动：宿主推送快照 / 筛选走的是无窗口路径，改不了列表的索引，
    /// 于是在渲染时补一次（这是把面板的选中"镜像"给组件，不是业务状态变更）。
    fn sync_list_selection(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(list) = self.list.clone() else {
            return;
        };
        let target = self
            .selected
            .as_deref()
            .and_then(|id| {
                // 列表的“行”含分组头：索引必须按 `view_items` 数（不能用 `view_rows` 的位置）。
                self.view_items.iter().position(
                    |item| matches!(item, VisibleItem::Row(row) if row.id == id),
                )
            })
            .map(IndexPath::new);
        list.update(cx, |state, cx| {
            if state.selected_index() != target {
                // 先立"面板镜像"标志：否则组件的回调会反过来更新面板，而此刻面板正在渲染。
                state.delegate_mut().syncing_from_panel = true;
                state.set_selected_index(target, window, cx);
                state.delegate_mut().syncing_from_panel = false;
            }
        });
    }

    /// 重算可见行（筛选 → 排序）并清理悬空选中，同时把行副本推给列表委托。
    ///
    /// 选中清理放在这里而不是只放在 `set_snapshot` 里：**筛选与排序也会让行消失**，
    /// 只盯着快照会留下一个指向"看不见的行"的选中态。
    fn refresh_view_rows(&mut self, cx: &mut Context<Self>) {
        self.view_rows = filter::apply_view(
            &self.snapshot.rows,
            &self.filter,
            self.sort_field,
            self.sort_order,
        );
        if let Some(selected) = self.selected.as_deref() {
            if !self.view_rows.iter().any(|row| row.id == selected) {
                self.selected = None;
            }
        }
        // 多选集合同样清悬空项（筛选 / 排序也会让行消失）。
        self.multi.retain(|id| self.view_rows.iter().any(|row| &row.id == id));
        if self.multi.is_empty() {
            self.anchor = None;
        } else if self.multi.len() == 1 {
            // 只剩一条 = 回到单选态：焦点行就是它（不然详情面板与列表会各指一个）。
            self.selected = self.multi.first().cloned();
        }
        // 分组被删后同样清掉它的折叠标记（不然那条 key 永远留着）。
        let alive: Vec<&str> = self.snapshot.groups.iter().map(|g| g.id.as_str()).collect();
        self.collapsed.retain(|key| {
            key == filter::GROUP_ALL || key == filter::GROUP_UNGROUPED || alive.contains(&key.as_str())
        });
        self.view_items = filter::build_visible_items(
            &self.view_rows,
            &self.snapshot.groups,
            &self.collapsed,
        );
        self.push_rows_to_list(cx);
    }

    /// 折叠 / 展开一个分组（分组头点击；`GROUP_ALL` / `GROUP_UNGROUPED` 也是合法的 key）。
    pub fn toggle_group_collapse(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.insert(key.to_string()) {
            self.collapsed.remove(key);
        }
        self.view_items = filter::build_visible_items(
            &self.view_rows,
            &self.snapshot.groups,
            &self.collapsed,
        );
        self.push_rows_to_list(cx);
        cx.notify();
    }

    /// 把可见行推给列表委托。
    ///
    /// 列表尚未创建时什么都不做——首帧渲染会带着现有行创建它（`ensure_list`）。
    fn push_rows_to_list(&self, cx: &mut Context<Self>) {
        let Some(list) = self.list.clone() else {
            return;
        };
        let items = self.view_items.clone();
        let details = self.snapshot.details.clone();
        let selected = self.selected.clone();
        let multi = self.multi.iter().cloned().collect();
        let read_only = self.snapshot.read_only;
        list.update(cx, |state, cx| {
            state
                .delegate_mut()
                .set_rows(items, details, selected, multi, read_only, cx);
        });
    }

    /// 宿主推送数据（事件路径调用；面板不自己取数）。
    pub fn set_snapshot(&mut self, snapshot: ResourcesSnapshot, cx: &mut Context<Self>) {
        self.snapshot = snapshot;
        // 标签被删后清掉悬空条件（否则列表会“什么都没匹配”，而菜单上的勾还在）——
        // 与选中 / 多选的悬空清理同一个道理，但条件在筛选器里，所以在推送时先清。
        let alive: Vec<String> = self.snapshot.tags.iter().map(|tag| tag.id.clone()).collect();
        self.filter.tags.retain(|id| alive.contains(id));
        self.refresh_view_rows(cx);
        cx.notify();
    }

    /// 换一套筛选条件（新建 / 清空走这里）。
    pub fn set_filter(&mut self, filter: ResourcesFilter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.refresh_view_rows(cx);
        cx.notify();
    }

    /// 设置搜索词：条件与输入框**同一次改**（见 `ensure_search_input` 里的注释）。
    pub fn set_query(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.filter.query = text.to_string();
        if let Some(input) = self.search_input.clone() {
            input.update(cx, |state, cx| state.set_value(text, window, cx));
        }
        self.refresh_view_rows(cx);
        cx.notify();
    }

    /// 勾选 / 取消一个种类（全选会被规范化为不限，见 `ResourcesFilter::toggle_kind`）。
    pub fn toggle_kind(&mut self, kind: ArchiveKind, cx: &mut Context<Self>) {
        self.filter.toggle_kind(kind);
        self.refresh_view_rows(cx);
        cx.notify();
    }

    /// 只看需处理的异常（缺失 / 内容已变）。
    pub fn toggle_only_issues(&mut self, cx: &mut Context<Self>) {
        self.filter.only_issues = !self.filter.only_issues;
        self.refresh_view_rows(cx);
    }

    /// 勾选 / 取消一个标签（多选是并集，见 `ResourcesFilter::tags`）。
    pub fn toggle_tag(&mut self, tag_id: &str, cx: &mut Context<Self>) {
        self.filter.toggle_tag(tag_id);
        self.refresh_view_rows(cx);
    }

    /// 选排序字段：同一字段再点一次翻转方向；换字段保持当前方向
    /// （沿用 M4 与 v1 `use-pagination` 的语义——用户刚调完方向再换字段，不该被重置）。
    pub fn choose_sort(&mut self, field: SortField, cx: &mut Context<Self>) {
        if field == self.sort_field {
            self.sort_order = self.sort_order.flipped();
        } else {
            self.sort_field = field;
        }
        self.refresh_view_rows(cx);
        cx.notify();
    }

    /// 清空筛选（含搜索框）：只在"没有匹配"的空态给这个入口。
    pub fn clear_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter = ResourcesFilter::default();
        if let Some(input) = self.search_input.clone() {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        self.refresh_view_rows(cx);
        cx.notify();
    }

    pub fn set_notice(&mut self, notice: Option<String>, cx: &mut Context<Self>) {
        self.notice = notice;
        cx.notify();
    }

    /// 置"正在取数"（**事件路径调用**：入队时置真、快照到达/失败时置假）。
    pub fn set_loading(&mut self, loading: bool, cx: &mut Context<Self>) {
        if self.loading == loading {
            return;
        }
        self.loading = loading;
        cx.notify();
    }

    /// 推进（或清掉）撤销凭据（**事件路径调用**：宿主归档成功后 / 撤销完成后）。
    ///
    /// 带凭据时同时安排 5 秒后自动消失（与 M5 撤销栏同一时长）：撤销窗口是"立即反悔"，
    /// 不是一条待办事项；过期就收，免得一个小时后点下去才发现已失效。
    pub fn set_undo(&mut self, undo: Option<ArchiveUndo>, cx: &mut Context<Self>) {
        let schedule = undo.clone();
        self.undo = undo;
        cx.notify();
        if let Some(token) = schedule {
            self.schedule_undo_expiry(token, cx);
        }
    }

    /// 安排撤销凭据 5 秒后自动消失（**仅当仍指向同一次归档**）。
    ///
    /// 守卫不能省：这 5 秒里可能又归档了一次，新凭据不该被旧计时器清掉（M5 同例）。
    fn schedule_undo_expiry(&self, token: ArchiveUndo, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            executor.timer(std::time::Duration::from_secs(5)).await;
            let _ = weak.update(cx, |this, cx| {
                if this.undo.as_ref() == Some(&token) {
                    this.undo = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn snapshot(&self) -> &ResourcesSnapshot {
        &self.snapshot
    }

    /// 可见行（筛选 + 排序后）：render 与测试都读它。
    pub fn view_rows(&self) -> &[ArchiveRow] {
        &self.view_rows
    }

    /// 可见项（分组头 + 行）：列表委托渲染的就是它（测试用它验证折叠与分区）。
    pub fn view_items(&self) -> &[VisibleItem] {
        &self.view_items
    }

    /// 当前筛选条件。
    pub fn filter(&self) -> &ResourcesFilter {
        &self.filter
    }

    /// 当前排序（字段 + 方向）。
    pub fn sort(&self) -> (SortField, SortOrder) {
        (self.sort_field, self.sort_order)
    }

    /// 选中行的详情快照（右侧详情面板用）。
    ///
    /// 未选中、或快照里没有该行的详情时返回 `None`（详情面板据此显示空态）。
    pub fn selected_detail(&self) -> Option<&ArchiveDetail> {
        let id = self.selected.as_deref()?;
        self.snapshot.details.get(id)
    }

    /// 搜索框实体（宿主接 `Ctrl+F` 聚焦 / `Esc` 清空时用，见 Action 批）。
    ///
    /// 首帧渲染前为 `None`（懒创建，见字段注释）。
    pub fn search_input(&self) -> Option<&Entity<InputState>> {
        self.search_input.as_ref()
    }

    /// 宿主驱动选中（生产入口；列表选中变化也走它）。
    ///
    /// 单选语义：选择集重置为这一条（多选是用户手势的产物，不由宿主推入）。
    pub fn set_selected(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.multi = id.iter().cloned().collect();
        self.anchor = id.clone();
        self.selected = id;
        cx.notify();
    }

    /// 行点击（生产入口：行的 `on_click` 归一化后调它；窗口测试也走这里）。
    ///
    /// 四种语义见 [`RowClick`]；双击只更新选中，**打开由行的回调直接转给宿主**
    /// （面板不认识服务层，也不该在这里再拼一条打开请求）。
    pub fn handle_row_click(&mut self, id: &str, click: RowClick, cx: &mut Context<Self>) {
        let order: Vec<String> = self.view_rows.iter().map(|row| row.id.clone()).collect();
        let focus = apply_row_click(&mut self.multi, &mut self.anchor, &order, id, click);
        // 焦点行总是最后点的那条：详情面板跟它走；选择集空 = 没有焦点。
        self.selected = focus;
        cx.notify();
    }

    /// 全选当前可见行（`Ctrl+A`；原型 §2.3 说“当前分组”，分组未落 → 即全部可见行）。
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.multi = self.view_rows.iter().map(|row| row.id.clone()).collect();
        // 焦点行：原来的还在就保留（详情面板不跳），否则取第一条。
        let keep = self
            .selected
            .as_deref()
            .is_some_and(|selected| self.multi.iter().any(|id| id == selected));
        if !keep {
            self.selected = self.multi.first().cloned();
        }
        self.anchor = self.selected.clone();
        cx.notify();
    }

    /// 当前选择集（多选优先；空 = 没有选中）——批量动作与窗口测试读它。
    pub fn selected_ids(&self) -> Vec<String> {
        self.multi.clone()
    }

    // ==================== 区域渲染 ====================
    //
    // 返回值写具体类型（`Div` / `AnyElement`）：edition 2024 的 RPIT 会捕获入参生命周期，
    // 返回 `impl IntoElement` 时它会借住 `cx`，连续调两个区域函数就变成"重复可变借用"。

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let (border, muted, foreground) = {
            let colors = cx.theme().colors;
            (colors.border, colors.muted_foreground, colors.foreground)
        };
        let read_only = self.snapshot.read_only;
        let host = self.host.clone();

        div()
            .h(rems(ui::PANEL_HEADER_HEIGHT))
            .flex_none()
            .h_flex()
            .justify_between()
            .gap_2()
            .px_2()
            // 1px 固定描边：不随字号缩放（允许的 physical boundary 例外）。
            .border_b(px(1.0))
            .border_color(border)
            .child(
                // 标题 = 前缀图标 + 文字（原型 §2.1）：图标形状与活动栏该面板的图标一致，
                // 一眼就对得上"这里是资产库"。
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .min_w_0()
                    .child(
                        Icon::default()
                            .path("icons/chart-column.svg")
                            .flex_none()
                            .size_3p5()
                            .text_color(muted),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_ellipsis()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(foreground)
                            .child("资产库"),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        // 原型 §2.1：`＋ ▾` 主操作，两项——**存档的本体必须来自某处**，
                        // 所以菜单里是两个来源，而不是"新建存档"。
                        Button::new("archive-add")
                            .ghost()
                            .label("＋ ▾")
                            .disabled(read_only)
                            .dropdown_menu({
                                let host = host.clone();
                                move |menu, _window, _cx| {
                                    let drafts_host = host.clone();
                                    let file_host = host.clone();
                                    menu.item(PopupMenuItem::new("从草稿箱归档…").on_click(
                                        move |_, window, cx| {
                                            drafts_host.request_archive_from_drafts(window, cx)
                                        },
                                    ))
                                    .item(PopupMenuItem::new("从本地文件归档…").on_click(
                                        move |_, window, cx| {
                                            file_host.request_archive_from_file(window, cx)
                                        },
                                    ))
                                }
                            }),
                    )
                    .child(
                        // 原型 §2.1：`⋯` 更多四项。写操作（重建索引）不单独置灰：
                        // 菜单里三项与只读无关，置灰整个 `⋯` 反而把刷新也锁上了。
                        Button::new("archive-more")
                            .ghost()
                            .label("⋯")
                            .debug_selector(|| "archive-more".to_string())
                            .dropdown_menu({
                                let host = host.clone();
                                move |menu, _window, _cx| {
                                    let mut menu = menu;
                                    for action in HeaderMenuAction::ALL {
                                        let target = host.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(action.label()).on_click(
                                                move |_, window, cx| {
                                                    dispatch_header_action(&target, action, window, cx)
                                                },
                                            ),
                                        );
                                    }
                                    menu
                                }
                            }),
                    ),
            )
    }

    /// 工具栏（高 2rem）：搜索框 + 筛选 + 排序（原型 §2.2）。
    ///
    /// 空库时也照常渲染：工具栏是面板版式的一部分，不能随"有没有存档"上下跳。
    /// 条件在渲染时快照一份给菜单（菜单开启时读一次，之后用 `checked` 表达状态）。
    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let border = cx.theme().colors.border;
        let panel = cx.entity();
        let kinds = self.filter.kinds.clone();
        let tags = self.filter.tags.clone();
        let only_issues = self.filter.only_issues;
        // 菜单里设了几个条件就标在按钮上：搜索词不计数（它在输入框里看得见）。
        let filter_label = match self.filter.menu_dims() {
            0 => "筛选 ▾".to_string(),
            dims => format!("筛选 {dims} ▾"),
        };
        let (field, order) = (self.sort_field, self.sort_order);
        let sort_label = format!("{} {} ▾", field.label(), order.arrow());

        let filter_button = Button::new("archive-filter")
            .ghost()
            .small()
            .label(filter_label)
            .dropdown_menu({
                let panel = panel.clone();
                let dictionary = self.snapshot.tags.clone();
                move |menu, _window, _cx| {
                    let mut menu = menu.item(PopupMenuItem::label("种类"));
                    for kind in ArchiveKind::ALL {
                        let target = panel.clone();
                        let is_checked = kinds.contains(&kind);
                        menu = menu.item(
                            PopupMenuItem::new(kind.label())
                                .checked(is_checked)
                                .on_click(move |_, _, app| {
                                    target.update(app, |panel, cx| panel.toggle_kind(kind, cx));
                                }),
                        );
                    }
                    // 标签组：一个标签都没有时不摆（空菜单节只会占地方）。
                    if !dictionary.is_empty() {
                        menu = menu.separator().item(PopupMenuItem::label("标签"));
                        for option in &dictionary {
                            let target = panel.clone();
                            let is_checked = tags.iter().any(|id| id == &option.id);
                            let label = format!("{}（{}）", option.name, option.count);
                            let tag_id = option.id.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label)
                                    .checked(is_checked)
                                    .on_click(move |_, _, app| {
                                        let tag_id = tag_id.clone();
                                        target.update(app, |panel, cx| {
                                            panel.toggle_tag(&tag_id, cx)
                                        });
                                    }),
                            );
                        }
                    }
                    menu.separator().item(
                        PopupMenuItem::new("只看需处理")
                            .checked(only_issues)
                            .on_click({
                                let target = panel.clone();
                                move |_, _, app| {
                                    target.update(app, |panel, cx| panel.toggle_only_issues(cx));
                                }
                            }),
                    )
                }
            });

        let sort_button = Button::new("archive-sort")
            .ghost()
            .small()
            .label(sort_label)
            .dropdown_menu({
                let panel = panel.clone();
                move |menu, _window, _cx| {
                    let mut menu = menu.item(PopupMenuItem::label("排序"));
                    for candidate in [SortField::Name, SortField::Version] {
                        let target = panel.clone();
                        let is_current = candidate == field;
                        // 当前字段带上方向箭头：不然用户得回忆上次点的是哪个方向。
                        let text = if is_current {
                            format!("{} {}", candidate.label(), order.arrow())
                        } else {
                            candidate.label().to_string()
                        };
                        menu = menu.item(
                            PopupMenuItem::new(text)
                                .checked(is_current)
                                .on_click(move |_, _, app| {
                                    target.update(app, |panel, cx| {
                                        panel.choose_sort(candidate, cx)
                                    });
                                }),
                        );
                    }
                    menu
                }
            });

        div()
            .flex_none()
            .h(rems(ui::TOOLBAR_HEIGHT))
            .h_flex()
            .gap_1()
            .px_1()
            .border_b(px(1.0))
            .border_color(border)
            .child(
                // 搜索框占满除两个按钮之外的宽度（`min_w_0`：窄面板下允许被压缩）。
                div().flex_1().min_w_0().when_some(self.search_input.clone(), |row, input| {
                    row.child(
                        Input::new(&input)
                            .h(rems(ui::CONTROL_HEIGHT_SM))
                            .cleanable(true),
                    )
                }),
            )
            .child(filter_button)
            .child(sort_button)
    }

    fn render_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let notice = self.notice.clone()?;
        let (border, warning, info) = {
            let colors = cx.theme().colors;
            (colors.border, colors.warning, colors.info)
        };
        // 只读是约束提示（warning），其余是信息（info）——同一字段不一律同色。
        let color = if self.snapshot.read_only { warning } else { info };
        Some(
            div()
                .flex_none()
                .px_2()
                .py_1()
                .text_xs()
                .border_b(px(1.0))
                .border_color(border)
                .text_color(color)
                .child(notice)
                .into_any_element(),
        )
    }

    /// 列表主体：`List`（虚拟化 + 组件化的 hover / 选中 / 键盘漫游）。
    ///
    /// 行数据在委托里（面板重算可见行时推过去）：`render_item` 在列表渲染期被调用，
    /// 那一刻面板正被借用，不能回头读面板的行集合。滚动归 `List` 自己（虚拟化列表自带）。
    fn render_rows(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(list) = self.list.clone() else {
            // 理论上不可达（`render` 已先 `ensure_list`）：退化成空区而不是 panic。
            let _ = cx;
            return div().flex_1().into_any_element();
        };
        div()
            .flex_1()
            .min_h_0()
            .child(List::new(&list).flex_1())
            .into_any_element()
    }

    /// 归档撤销栏（原型 §4.1）：给"把文件从工作区搬走"一个立即反悔的窗口。
    ///
    /// 固定在状态行**上方**（与 M5 撤销栏同位）：它是刚发生那次动作的出口，不能随列表滚走；
    /// 文案直接引用显示名，用户不用回想刚搬的是哪一条。
    fn render_undo_bar(&self, undo: &ArchiveUndo, cx: &mut Context<Self>) -> Div {
        let (foreground, primary, hover_bg) = {
            let colors = cx.theme().colors;
            (colors.foreground, colors.primary, colors.list_hover)
        };
        let host = self.host.clone();
        let token = undo.clone();
        div()
            .flex_none()
            .h_flex()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_1()
            .bg(hover_bg)
            .text_xs()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_ellipsis()
                    .text_color(foreground)
                    .child(format!("已归档「{}」", undo.name)),
            )
            .child(
                div()
                    // 稳定 id + 调试选择器：这栏只存在一条（且 5 秒后自走），窗口测试按它定位。
                    .id("archive-undo")
                    .debug_selector(|| "archive-undo".to_string())
                    .cursor_pointer()
                    .text_color(primary)
                    .child("撤销")
                    .on_click(move |_, window, cx| host.request_undo_archive(&token, window, cx)),
            )
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> Div {
        let (border, warning, muted) = {
            let colors = cx.theme().colors;
            (colors.border, colors.warning, colors.muted_foreground)
        };
        let counts = self.snapshot.counts;
        let has_issues = counts.has_issues();
        let host = self.host.clone();

        div()
            .flex_none()
            .h(rems(ui::ROW_HEIGHT))
            .h_flex()
            .justify_between()
            .px_2()
            .border_t(px(1.0))
            .border_color(border)
            .child(
                div()
                    .text_xs()
                    .text_color(if has_issues { warning } else { muted })
                    .child(counts.status_line(self.loading)),
            )
            .when(has_issues, |bar| {
                bar.child(
                    Button::new("index-repair")
                        .ghost()
                        .label("修复…")
                        .on_click(move |_, window, cx| host.request_index_repair(window, cx)),
                )
            })
    }

    /// 加载中骨架（原型 §5）：3 行灰条，**不用转圈**——形状直接提示"这里将要出现行"，
    /// 宽度递减以免看着像真行。
    fn render_loading(&self, cx: &mut Context<Self>) -> Div {
        let bar_bg = cx.theme().colors.list_hover;
        let mut skeleton = div()
            .flex_1()
            .v_flex()
            .w_full()
            .min_h_0()
            .gap_2()
            .p_2()
            // 调试选择器：窗口测试靠它断言"取数期间不摆空态"。
            .debug_selector(|| "archive-loading".to_string());
        for ratio in [0.75_f32, 0.55, 0.65] {
            skeleton = skeleton.child(
                div()
                    .h(rems(ui::ROW_HEIGHT))
                    .w(relative(ratio))
                    .rounded_sm()
                    .bg(bar_bg),
            );
        }
        skeleton
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let read_only = self.snapshot.read_only;
        // 两个按钮各拿一份（`Rc` 不是 `Copy`：两个闭包都要 `move` 走它）。
        let host_for_drafts = self.host.clone();
        let host_for_file = self.host.clone();

        div()
            .flex_1()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("还没有任何存档"),
            )
            .child(
                div()
                    .px_4()
                    .text_xs()
                    .text_color(muted)
                    .child("把草稿箱里值得留存的文件归档进来，它就变成只读、带版本、可追溯的存档"),
            )
            .child(
                Button::new("archive-for-empty")
                    .primary()
                    .debug_selector(|| "archive-empty-action".to_string())
                    .label("从草稿箱归档…")
                    .disabled(read_only)
                    .on_click(move |_, window, cx| host_for_drafts.request_archive_from_drafts(window, cx)),
            )
            .child(
                Button::new("archive-for-empty-file")
                    .secondary()
                    .label("从本地文件归档…")
                    .disabled(read_only)
                    .on_click(move |_, window, cx| host_for_file.request_archive_from_file(window, cx)),
            )
    }

    /// 搜索 / 筛选无结果：**不是空库态**（原型 §5 状态与空态矩阵）。
    ///
    /// 两态的出口才是关键区别：空库引导"归档"（没有东西可用），无匹配引导"清筛选"
    /// （东西在，只是被挡住了）。文案混用会把用户引到错误的下一步。
    fn render_no_match(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().colors.muted_foreground;
        let panel = cx.entity();

        div()
            .flex_1()
            .v_flex()
            .items_center()
            .justify_center()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("没有匹配的存档"),
            )
            .child(
                div()
                    .px_4()
                    .text_xs()
                    .text_color(muted)
                    .child("换个关键词，或去掉几个筛选条件"),
            )
            .child(
                Button::new("archive-clear-filter")
                    .ghost()
                    .label("清空筛选")
                    .on_click(move |_, window, app| {
                        panel.update(app, |panel, cx| panel.clear_filter(window, cx));
                    }),
            )
    }
}

// ==================== 面板协议 ====================

impl EventEmitter<PanelEvent> for ResourcesPanel {}

impl Focusable for ResourcesPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for ResourcesPanel {
    fn panel_name(&self) -> &'static str {
        Self::PANEL_NAME
    }

    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }
}

impl ComponentPanel for ResourcesPanel {
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().child("资产库")
    }
}

impl Render for ResourcesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 首帧创建搜索框与列表状态（懒创建，见字段注释）：之后本帧即可渲染工具栏与行。
        self.ensure_search_input(window, cx);
        self.ensure_list(window, cx);
        // 宿主推送（无窗口）改不了列表索引，这里把面板的选中镜像回去。
        self.sync_list_selection(window, cx);
        let background = cx.theme().colors.background;
        // 各区域函数返回具体类型（见上文注释），故可依次调用、各自持有已完成的元素。
        let header = self.render_header(cx);
        let toolbar = self.render_toolbar(cx);
        let notice = self.render_notice(cx);
        // 两种空：**空库**（条件为空，没有东西可用）与**无匹配**（条件非空，东西被挡住了）。
        // `ResourcesFilter::is_empty` 就是为这行判定存在的（见 `filter.rs` 模块头）。
        let body = if self.view_rows.is_empty() {
            if self.filter.is_empty() {
                // 首个快照到达前不摆空态：那是"还没读到"，不是"没有存档"（原型 §5）。
                if self.loading {
                    self.render_loading(cx).into_any_element()
                } else {
                    self.render_empty(cx).into_any_element()
                }
            } else {
                self.render_no_match(cx).into_any_element()
            }
        } else {
            self.render_rows(cx)
        };
        let status = self.render_status_bar(cx);
        // 撤销栏固定在上方（与 M5 撤销栏同位）：它是刚发生那次动作的出口，不能随列表滚走。
        let undo_bar = self
            .undo
            .as_ref()
            .map(|undo| self.render_undo_bar(undo, cx));
        let host = self.host.clone();

        div()
            .id("analytics-resource-panel")
            // `track_focus` 不能省：没有它面板不在 dispatch path 上，快捷键就落不到动作（已踩）。
            .track_focus(&self.focus_handle)
            // 键盘上下文：快捷键由 app 层绑到 `analytics-resource` context（视图不自注全局键）。
            .key_context("analytics-resource")
            .on_action(cx.listener({
                let host = host.clone();
                move |_: &mut Self, _: &commands::RequestArchive, window, cx| {
                    // 动作没有"选菜单"这一步，落到**本地文件**那条（与面板头第二项一致）。
                    host.request_archive_from_file(window, cx)
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::OpenSelected, window, cx| {
                    let detail = panel
                        .selected
                        .as_deref()
                        .and_then(|id| panel.snapshot.details.get(id))
                        .cloned();
                    if let Some(detail) = detail.as_ref() {
                        host.request_open(detail, window, cx);
                    }
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::CheckoutSelected, window, cx| {
                    let detail = panel
                        .selected
                        .as_deref()
                        .and_then(|id| panel.snapshot.details.get(id))
                        .cloned();
                    if let Some(detail) = detail.as_ref() {
                        host.request_checkout(detail, window, cx);
                    }
                }
            }))
            .on_action(cx.listener({
                let host = host.clone();
                move |panel: &mut Self, _: &commands::DeleteSelected, window, cx| {
                    // 多选时对整个选择集生效（原型 §3.2：删除是唯一多选可用的项）。
                    let ids = panel.selected_ids();
                    if !ids.is_empty() {
                        host.request_delete(&ids, window, cx);
                    }
                }
            }))
            .on_action(cx.listener({
                // `Ctrl+A`：全选当前可见行（分组未落，即全部可见行）。
                move |panel: &mut Self, _: &commands::SelectAllRows, _window, cx| {
                    panel.select_all(cx);
                }
            }))
            .on_action(cx.listener({
                // `Ctrl+F`：聚焦搜索框（`InputState` 需要窗口，而 action 处理器能拿到）。
                move |panel: &mut Self, _: &commands::FocusSearch, window, cx| {
                    if let Some(input) = panel.search_input.clone() {
                        input.update(cx, |state, cx| state.focus(window, cx));
                    }
                }
            }))
            .on_action(cx.listener({
                // `Esc`：只清搜索词（种类 / 只看需处理留在菜单里，误清会让人以为筛选坏了）。
                move |panel: &mut Self, _: &commands::ClearSearch, window, cx| {
                    panel.set_query("", window, cx);
                }
            }))
            .v_flex()
            .size_full()
            // 面板自己拥有滚动（Dock 的 tab-content 不产生滚动）：滚动挂在 body 上。
            .min_h_0()
            .bg(background)
            .child(header)
            .child(toolbar)
            .when_some(notice, |panel, notice| panel.child(notice))
            .child(body)
            .when_some(undo_bar, |panel, bar| panel.child(bar))
            .child(status)
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：测试模块不通配导入（会与 `#[gpui_kit::test]` 展开的 `#[test]` 自相残杀）。
    use super::{
        ArchiveCounts, BadgeTone, HeaderMenuAction, RowClick, apply_row_click, badge_tone,
        can_view_stats, classify_click, kind_icon, row_tail, strength_badge,
    };
    use crate::detail_view::ArchiveDetail;
    use crate::model::{ArchiveKind, ArchiveStatus};
    use gpui_kit::Modifiers;

    #[test]
    fn row_click_classification_follows_the_prototype() {
        let none = Modifiers::default();
        // 次修饰键按平台取（macOS = Cmd，其余 = Ctrl）：测试不能写死一个平台。
        let secondary = if cfg!(target_os = "macos") {
            Modifiers {
                platform: true,
                ..none
            }
        } else {
            Modifiers {
                control: true,
                ..none
            }
        };

        assert_eq!(classify_click(1, none), RowClick::Select);
        assert_eq!(classify_click(2, none), RowClick::Open, "双击优先");
        assert_eq!(
            classify_click(1, Modifiers { shift: true, ..none }),
            RowClick::Range
        );
        assert_eq!(classify_click(1, secondary), RowClick::Toggle);
        assert_eq!(
            classify_click(
                1,
                Modifiers {
                    shift: true,
                    ..secondary
                }
            ),
            RowClick::Range,
            "两个修饰键都按：区间优先"
        );
        assert_eq!(
            classify_click(2, Modifiers { shift: true, ..none }),
            RowClick::Open,
            "双击带修饰键仍是打开"
        );
    }

    #[test]
    fn row_click_updates_the_selection_set() {
        let order: Vec<String> = ["ar_1", "ar_2", "ar_3"]
            .iter()
            .map(|id| id.to_string())
            .collect();
        let mut multi = Vec::new();
        let mut anchor = None;

        // 单击：单选（多选被重置）。
        assert_eq!(
            apply_row_click(&mut multi, &mut anchor, &order, "ar_2", RowClick::Select),
            Some("ar_2".to_string())
        );
        assert_eq!(multi, vec!["ar_2"]);

        // Ctrl 点击：加入，并按**可见行顺序**排（不是点击顺序）。
        apply_row_click(&mut multi, &mut anchor, &order, "ar_1", RowClick::Toggle);
        assert_eq!(multi, vec!["ar_1", "ar_2"]);
        assert_eq!(anchor.as_deref(), Some("ar_1"));

        // Ctrl 再点同一条：移出；最后一条被取消 = 无选中。
        apply_row_click(&mut multi, &mut anchor, &order, "ar_1", RowClick::Toggle);
        assert_eq!(multi, vec!["ar_2"]);
        let focus = apply_row_click(&mut multi, &mut anchor, &order, "ar_2", RowClick::Toggle);
        assert!(multi.is_empty() && focus.is_none());

        // Shift：从锚点到被点行的区间（反向也成立）。
        apply_row_click(&mut multi, &mut anchor, &order, "ar_1", RowClick::Select);
        apply_row_click(&mut multi, &mut anchor, &order, "ar_3", RowClick::Range);
        assert_eq!(multi, vec!["ar_1", "ar_2", "ar_3"]);
        apply_row_click(&mut multi, &mut anchor, &order, "ar_1", RowClick::Select);
        apply_row_click(&mut multi, &mut anchor, &order, "ar_2", RowClick::Select);
        apply_row_click(&mut multi, &mut anchor, &order, "ar_3", RowClick::Range);
        assert_eq!(multi, vec!["ar_2", "ar_3"], "锚点跟着最后一次单选走");

        // 锚点不在了（被筛掉）：区间退化为单选。
        let mut anchor = Some("ar_gone".to_string());
        let mut multi = vec!["ar_1".to_string()];
        apply_row_click(&mut multi, &mut anchor, &order, "ar_3", RowClick::Range);
        assert_eq!(multi, vec!["ar_3"]);

        // 双击：与单击同样落到单选（打开由行回调直接转给宿主）。
        let mut multi = vec!["ar_1".to_string(), "ar_2".to_string()];
        apply_row_click(&mut multi, &mut anchor, &order, "ar_3", RowClick::Open);
        assert_eq!(multi, vec!["ar_3"]);
    }

    #[test]
    fn header_menu_actions_follow_the_prototype_order() {
        // 原型 §2.1 的 `⋯` 四项，自上而下：重建索引… / 打开资源目录 / 回收站… / 刷新。
        // 顺序与文案都是用户可见的约定，改动要有意识地改这条测试。
        let labels: Vec<&str> = HeaderMenuAction::ALL.iter().map(|a| a.label()).collect();
        assert_eq!(
            labels,
            vec!["重建索引…", "打开资源目录", "回收站…", "刷新"]
        );
        assert_eq!(HeaderMenuAction::ALL.len(), 4);
    }

    #[test]
    fn kind_icons_use_three_distinct_shapes() {
        // 形状区分（不靠颜色）：三个 kind 的图标互不相同，且都来自已捆绑的资产目录
        // （枚举由 gpui-kit-assets 的构建脚本按 svg 文件名生成，编译通过即资产存在）。
        let icons = [
            kind_icon(ArchiveKind::File),
            kind_icon(ArchiveKind::Analysis),
            kind_icon(ArchiveKind::TableRef),
        ];
        assert_eq!(icons[0], gpui_kit::assets::IconName::FileText);
        assert_eq!(icons[1], gpui_kit::assets::IconName::Table);
        assert_eq!(icons[2], gpui_kit::assets::IconName::ExternalLink);
        assert!(icons[0] != icons[1] && icons[1] != icons[2] && icons[0] != icons[2]);
    }

    #[test]
    fn strength_badge_follows_kind_and_status() {
        assert_eq!(
            strength_badge(ArchiveKind::File, ArchiveStatus::Normal),
            "已归档"
        );
        assert_eq!(
            strength_badge(ArchiveKind::Analysis, ArchiveStatus::Normal),
            "分析表"
        );
        assert_eq!(
            strength_badge(ArchiveKind::TableRef, ArchiveStatus::Normal),
            "引用"
        );
        // 异常态压过 kind。
        assert_eq!(
            strength_badge(ArchiveKind::File, ArchiveStatus::Missing),
            "缺失"
        );
        assert_eq!(
            strength_badge(ArchiveKind::File, ArchiveStatus::ContentChanged),
            "内容已变"
        );
    }

    #[test]
    fn badge_tone_marks_weak_strength_as_warning() {
        assert_eq!(
            badge_tone(ArchiveKind::File, ArchiveStatus::Normal),
            BadgeTone::Success
        );
        assert_eq!(
            badge_tone(ArchiveKind::Analysis, ArchiveStatus::Normal),
            BadgeTone::Info
        );
        // `引用`复现强度最弱：必须显眼（warning），不能与"已归档"同为成功色。
        assert_eq!(
            badge_tone(ArchiveKind::TableRef, ArchiveStatus::Normal),
            BadgeTone::Warning
        );
        assert_eq!(
            badge_tone(ArchiveKind::File, ArchiveStatus::Missing),
            BadgeTone::Danger
        );
    }

    #[test]
    fn row_tail_prefers_detail_then_time_then_version() {
        assert_eq!(row_tail("1.2 KB", "3 天前", 3), "1.2 KB · 3 天前");
        assert_eq!(row_tail("1.2 KB", "", 3), "1.2 KB");
        assert_eq!(row_tail("", "3 天前", 3), "3 天前");
        assert_eq!(row_tail("", "", 3), "v3");
    }

    #[test]
    fn counts_line_hides_zero_buckets() {
        let clean = ArchiveCounts {
            total: 3,
            archived: 3,
            ..ArchiveCounts::default()
        };
        assert_eq!(clean.line(), "3 项 · 已归档 3");
        assert!(!clean.has_issues());
        // 加载中只加前缀，**不藏计数**（那是上一次的库口径，抹掉会让人以为库空了）。
        assert_eq!(clean.status_line(true), "加载中… · 3 项 · 已归档 3");
        assert_eq!(clean.status_line(false), clean.line());

        let with_issues = ArchiveCounts {
            total: 4,
            archived: 3,
            analysis: 1,
            missing: 1,
            drifted: 2,
            ..ArchiveCounts::default()
        };
        assert_eq!(
            with_issues.line(),
            "4 项 · 已归档 3 · 分析表 1 · 缺失 1 · 索引异常 2"
        );
        assert!(with_issues.has_issues());
    }

    /// 构造一条详情：只写与判定相关的字段，其余取最小可用值。
    fn detail(kind: ArchiveKind, status: ArchiveStatus, payload: Option<&str>) -> ArchiveDetail {
        ArchiveDetail {
            id: "ar_1".to_string(),
            name: "订单".to_string(),
            alias: None,
            kind,
            version: 1,
            status,
            readonly: true,
            size_label: String::new(),
            modified_label: String::new(),
            archived_label: String::new(),
            promoted_from: None,
            source_connection_id: None,
            source_table: None,
            content_hash: None,
            payload_rel_path: payload.map(str::to_string),
            history_label: String::new(),
            tags: Vec::new(),
            group: None,
        }
    }

    #[test]
    fn view_stats_only_for_data_sources_that_can_be_sampled() {
        let file = |payload: Option<&str>| {
            detail(ArchiveKind::File, ArchiveStatus::Normal, payload)
        };

        // 受管文件：本体扩展名得是 DuckDB 读得动的（大小写不敏感、中文名照认）。
        assert!(can_view_stats(&file(Some("orders.csv"))));
        assert!(can_view_stats(&file(Some("2026 预算.XLSX"))));
        assert!(can_view_stats(&file(Some("data/events.ndjson"))));
        // 非数据文件：`.sql` 是脚本不是数据，不给入口（不是“点了才说不行”）。
        assert!(!can_view_stats(&file(Some("dau.sql"))));
        assert!(!can_view_stats(&file(None)));
        // 缺失的行：本体不在，取不到数（即便扩展名可读）。
        assert!(!can_view_stats(&detail(
            ArchiveKind::File,
            ArchiveStatus::Missing,
            Some("orders.csv")
        )));

        // 远端引用：连接与表名都齐了才给（取样 SQL 由宿主按驱动拼）。
        let mut reference = detail(ArchiveKind::TableRef, ArchiveStatus::Normal, None);
        assert!(!can_view_stats(&reference), "没有表名就没有可取样的对象");
        reference.source_connection_id = Some("conn_1".to_string());
        assert!(!can_view_stats(&reference), "没有连接就不知道去哪取样");
        reference.source_table = Some("public.orders".to_string());
        assert!(can_view_stats(&reference));

        // 分析表：本体是 analytics.duckdb，要 ATTACH + 重建定义——后续批次接前不给入口。
        assert!(!can_view_stats(&detail(
            ArchiveKind::Analysis,
            ArchiveStatus::Normal,
            Some("analytics.duckdb")
        )));
    }
}
