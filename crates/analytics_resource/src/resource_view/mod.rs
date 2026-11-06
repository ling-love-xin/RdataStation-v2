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

pub(crate) use std::cell::Cell;
pub(crate) use std::rc::Rc;

pub(crate) use gpui_kit::component::IndexPath;
pub(crate) use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
pub(crate) use gpui_kit::component::input::{InputEvent, InputState};
pub(crate) use gpui_kit::component::list::ListState;
pub(crate) use gpui_kit::*;

pub(crate) use crate::detail_view::ArchiveDetail;
pub(crate) use crate::dnd::{self, GroupDrop};
pub(crate) use crate::filter::{self, ResourcesFilter, SortField, SortOrder, VisibleItem};
pub(crate) use crate::model::{ArchiveKind, ArchiveUndo, DeleteTarget, TagTarget};

// ---- P1 拆分：视图模型/行级纯函数 -> row_meta；列表委托 -> list_delegate；区域渲染 -> panel_render ----
mod row_meta;
pub use row_meta::{
    ArchiveCounts, ArchiveRow, BadgeTone, DEFAULT_GROUP_MARKER, GroupOption, InstanceFilter,
    ResourcesSnapshot, RowClick, TagOption, badge_tone, can_rename, can_view_stats,
    classify_row_click, default_group_entry_label, row_tail, strength_badge, tag_entry_label,
    tag_targets,
};
pub(crate) use row_meta::{apply_row_click, kind_icon};
// `classify_click` 仅窗口测试引用（`tests.rs` 经 `use super::` 取），生产路径无调用方。
#[cfg(test)]
pub(crate) use row_meta::classify_click;

mod list_delegate;
pub(crate) use list_delegate::ArchiveListDelegate;
// `header_fold_key` 仅窗口测试引用（`tests.rs` 经 `use super::` 取），生产路径无调用方。
#[cfg(test)]
pub(crate) use list_delegate::header_fold_key;

mod panel_render;
// ==================== 宿主契约 ====================

/// 宿主能力：面板不认识服务层与会话，动作一律回宿主（事件路径执行）。
///
/// 每个动作都带上 `Window` / `App`：宿主侧要开对话框与只读打开（都需要窗口句柄），
/// 而面板天然持有它们（`on_click` / `on_action` 都提供）——先把上下文带出去，比将来改签名轻。
pub trait ResourcesHost: 'static {
    /// 归档入口（面板头「＋ ▾」第一项）：从**草稿箱**选文件归档（宿主弹草稿多选对话框）。
    fn request_archive_from_drafts(&self, window: &mut Window, cx: &mut App);
    /// 草稿箱行右键「归档为存档…」：候选由**调用方点名**（草稿箱点的是哪几行它自己知道）。
    ///
    /// 与 [`Self::request_archive_from_drafts`] 同一套流程（同一个挑选对话框、同一个
    /// 确认对话框、同一份批量入队），差别只在候选来源与预勾选：一条直接展开确认，
    /// 多条预勾上再摆挑选对话框。
    ///
    /// 默认实现不做事——不接草稿箱的宿主（测试 / 其他入口）不会摆出这条菜单。
    fn request_archive_drafts(
        &self,
        _paths: Vec<std::path::PathBuf>,
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }
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
    /// 收条目切片而不是单个 id：多选批量与单选走同一条路（批量时确认框与回执要带数量）；
    /// 批量中途失败的错误里会带“本批已移入 N 项”，**不做预回滚**（部分成功就部分成功）。
    /// 每条带上归档粒度：确认框文案按 copy / reference 区分本体命运（§12.1.1 ③）。
    fn request_delete(&self, targets: &[DeleteTarget], window: &mut Window, cx: &mut App);
    /// 移动到分组（`None` = 移回未分组）：右键菜单「移动到分组」。
    ///
    /// 收切片而不是单个 id：多选也走这一条（批量移动是原型里多选解锁的动作之一）。
    fn request_move_to_group(
        &self,
        resource_ids: &[String],
        folder_id: Option<&str>,
        window: &mut Window,
        cx: &mut App,
    );
    /// 新建分组（行菜单 / 分组头菜单共用）。
    fn request_create_group(&self, window: &mut Window, cx: &mut App);
    /// 重命名分组（分组头右键）。
    fn request_rename_group(&self, folder_id: &str, window: &mut Window, cx: &mut App);
    /// 重命名一条存档的**显示名**（行右键「重命名…」/ `F2` / 详情头部点名字）。
    ///
    /// 只收 id：当前显示名由宿主从面板快照取（与 `request_rename_group` 同一口径），
    /// 宿主拿到后开那个单输入的小对话框（`dialogs::rename`），提交才写库。
    fn request_rename(&self, resource_id: &str, window: &mut Window, cx: &mut App);
    /// 编辑一条存档的**别名**（详情头部点别名 / 「＋ 别名」）。
    ///
    /// 与 `request_rename` 同一形状（同一个对话框、只换字段）：留空 = **清除别名**。
    fn request_edit_alias(&self, resource_id: &str, window: &mut Window, cx: &mut App);
    /// 记下 / 清掉本项目的**归档默认分组**（分组头右键；设置项 `resources.default_group`）。
    ///
    /// `None` = 未分组（也即“没默认”）：归档对话框就不再预选任何分组。
    /// 不校验分组是否还在——那份名单在项目库里，面板的标记与对话框的预选都会把认不出的
    /// id 当作未分组（与筛选的脏数据口径一致）。
    fn remember_default_group(&self, folder_id: Option<&str>, cx: &mut App);
    /// 删除分组（分组头右键；**成员回未分组**，存档本身不受影响）。
    fn request_delete_group(&self, folder_id: &str, window: &mut Window, cx: &mut App);
    /// 打开「标签」对话框（详情面板「＋ 标签」/ 行右键菜单）：勾选/取消标签、顺带新建。
    ///
    /// 收**目标切片**（单击一元、多选 N 元）：多选批量是原型里多选解锁的动作之一，
    /// 与 `request_delete` / `request_move_to_group` 同一口径——面板给出选集，宿主负责
    /// 取数（全部标签 + 每个目标的已挂标签）与批量执行。
    fn request_edit_tags(&self, targets: &[TagTarget], window: &mut Window, cx: &mut App);
    /// 去掉一个标签（详情面板 chip 上的 ×）。
    ///
    /// 与其它动作同口径：收**面板已有的那条详情**（回执文案要显示名），不回头读面板选中态。
    /// 不进对话框：点 × 就是“去掉它”，再要一次确认是多余的。
    fn request_remove_tag(
        &self,
        detail: &ArchiveDetail,
        tag_id: &str,
        window: &mut Window,
        cx: &mut App,
    );
    /// 索引修复入口（状态行异常段的「修复…」与面板头「⋯ → 重建索引…」共用）。
    fn request_index_repair(&self, window: &mut Window, cx: &mut App);
    /// 用系统文件管理器打开受管内容根（面板头「⋯ → 打开资源目录」）。
    ///
    /// 开的是**目录**（与行的「在系统中显示」不同：那条要选中某个本体文件）。
    fn request_open_payload_dir(&self, window: &mut Window, cx: &mut App);
    /// 记住列表排序（工具栏点选排序后调用）。
    ///
    /// 宿主把它写回设置（`resources.default_sort`）——“默认排序”在这里就是“上次用的那个”，
    /// 不另给一个“默认排序”菜单（两处各存一份就会不一致）。面板不知道 settings 的存在。
    fn remember_sort(&self, field: SortField, order: SortOrder, cx: &mut App);
    /// 记住分组折叠状态（点一次分组头就调一次）。
    ///
    /// 宿主按项目分桶写设置（`resources.collapsed_groups`）；传的是**当前全集**而不是增量：
    /// 增量写入在“面板换了项目 / 分组被删”这些场景下容易和旧值叠出幽灵记录。
    fn remember_collapsed(&self, collapsed_keys: &[String], cx: &mut App);
    /// 打开资源回收站（面板头「⋯ → 回收站…」）。
    ///
    /// 回收站是**项目级**的（与草稿箱共用一处，模块硬约束 5）：对话框只列本模块的条目，
    /// 别人的只给一句说明（还原归它自己的模块）。
    fn request_open_trash(&self, window: &mut Window, cx: &mut App);
    /// 重新取数（面板头「⋯ → 刷新」）。
    ///
    /// 与「打开 / 切换项目」同一条取数路径；不做局部刷新——列表是一整份快照。
    fn request_refresh(&self, window: &mut Window, cx: &mut App);
    /// 升级到全局实例（§13：项目 → 全局，表 + 版本信息进全局 DuckDB）。
    ///
    /// 收**面板已有的那条详情**（本体路径与指纹都在它身上）；File 档才可升级
    /// （分析表型升级依赖分析表读取路径，M8 后续批次接）。
    fn request_promote_to_global(&self, detail: &ArchiveDetail, window: &mut Window, cx: &mut App);
    /// 从全局分发到当前项目（§13：全局 → 项目，跨项目唯一法定路径）。
    ///
    /// 只带全局资源的**显示名**（分发只认全局登记的名字；本体在全局 DuckDB）。
    fn request_distribute_from_global(&self, name: &str, window: &mut Window, cx: &mut App);

    /// reference 快照是否已失效（打开时懒校验发现源内容与登记指纹不一致，§12.1.1 ②）。
    ///
    /// 只做**只读查询**：失效槽位由宿主维护（按 `resource_id` 存内存，不进索引状态统计）；
    /// 面板渲染期问一句，不触发任何取数。源被删 / 被移由 `ArchiveStatus::Missing` 表达，
    /// 不走这个槽位。
    fn is_reference_stale(&self, resource_id: &str) -> bool;

    /// reference 失效后「接受当前内容」：以当前源绝对路径为基准再归档，指纹 / 版本落库
    /// （走 `archive_into_existing` 的 reference 分支），成功后清失效槽位并刷新快照。
    fn accept_reference_current(&self, detail: &ArchiveDetail, window: &mut Window, cx: &mut App);
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
    /// 归档默认分组（设置项 `resources.default_group`，按项目分桶）：构造期注入，
    /// 之后由分组头右键改（面板先改本地标记，再交宿主落盘——与折叠态同一形态）。
    default_group: Option<String>,
    /// 工具栏条件（搜索词 / 种类 / 只看需处理）。
    filter: ResourcesFilter,
    /// 实例维度（§13 双实例）：全部 / 项目 / 全局。
    instance: InstanceFilter,
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
    /// 索引异常提示条是否已在本会话提示过（D2：首次进入一次性弹出，关闭后不再出现；
    /// 纯内存、不进持久层，也不随项目切换重置——提示的是"当前打开项目里的异常"）。
    announced_index_issue: Cell<bool>,
    focus_handle: FocusHandle,
    group: Option<WeakEntity<TabGroup>>,
}

mod accessors;
mod filters;
mod groups;
mod init;
mod list;
mod selection;
mod status;

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

#[cfg(test)]
mod tests;
