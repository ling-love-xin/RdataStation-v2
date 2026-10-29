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


use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::IndexPath;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent, TabGroup};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::list::ListState;
use gpui_kit::*;

use crate::detail_view::ArchiveDetail;
use crate::dnd::{self, GroupDrop};
use crate::filter::{self, ResourcesFilter, SortField, SortOrder, VisibleItem};
use crate::model::{ArchiveKind, ArchiveUndo, DeleteTarget, TagTarget};

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

impl ResourcesPanel {
    pub const PANEL_NAME: &'static str = "analytics_resource";

    pub fn new(host: Rc<dyn ResourcesHost>, cx: &mut Context<Self>) -> Self {
        Self {
            host,
            snapshot: ResourcesSnapshot::default(),
            view_rows: Vec::new(),
            view_items: Vec::new(),
            collapsed: std::collections::HashSet::new(),
            default_group: None,
            filter: ResourcesFilter::default(),
            instance: InstanceFilter::default(),
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
            announced_index_issue: Cell::new(false),
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
            groups: self.snapshot.groups.clone(),
            default_group: self.default_group.clone(),
            selected_id: self.selected.clone(),
            multi_ids: self.multi.iter().cloned().collect(),
            syncing_from_panel: false,
            focused_header: None,
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
                self.view_items
                    .iter()
                    .position(|item| matches!(item, VisibleItem::Row(row) if row.id == id))
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
        // 实例维度在**面板层**拼接（不进 `filter::apply_view`：全局行没有项目标签 /
        // 分组 / 异常概念，让它们进 `matches` 只会制造"永远筛不出"的维度）。
        let source: Vec<ArchiveRow> = match self.instance {
            InstanceFilter::Project => self.snapshot.rows.clone(),
            InstanceFilter::Global => self.snapshot.global_rows.clone(),
            InstanceFilter::All => {
                let mut merged = self.snapshot.rows.clone();
                merged.extend(self.snapshot.global_rows.iter().cloned());
                merged
            }
        };
        self.view_rows = filter::apply_view(
            &source,
            &self.filter,
            self.sort_field,
            self.sort_order,
        );
        // 全局行可被选中但不给详情（详情映射只有项目行）；选中清理按 view_rows 走。
        if let Some(selected) = self.selected.as_deref() {
            if !self.view_rows.iter().any(|row| row.id == selected) {
                self.selected = None;
            }
        }
        // 多选集合同样清悬空项（筛选 / 排序也会让行消失）。
        self.multi
            .retain(|id| self.view_rows.iter().any(|row| &row.id == id));
        if self.multi.is_empty() {
            self.anchor = None;
        } else if self.multi.len() == 1 {
            // 只剩一条 = 回到单选态：焦点行就是它（不然详情面板与列表会各指一个）。
            self.selected = self.multi.first().cloned();
        }
        // 分组被删后同样清掉它的折叠标记（不然那条 key 永远留着）。
        let alive: Vec<&str> = self.snapshot.groups.iter().map(|g| g.id.as_str()).collect();
        self.collapsed.retain(|key| {
            key == filter::GROUP_ALL
                || key == filter::GROUP_UNGROUPED
                || alive.contains(&key.as_str())
        });
        self.refresh_view_items(cx);
    }

    /// 折叠 / 展开一个分组（分组头点击；`GROUP_ALL` / `GROUP_UNGROUPED` 也是合法的 key）。
    ///
    /// 折叠态不是纯视图状态：它要被记住（见 `remember_collapsed`），否则每次打开项目
    /// 都得重新折一遍。
    pub fn toggle_group_collapse(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.insert(key.to_string()) {
            self.collapsed.remove(key);
        }
        self.refresh_view_items(cx);
        let keys = self.collapsed_keys();
        self.host.remember_collapsed(&keys, cx);
        cx.notify();
    }

    /// 折叠集合（**排序后**的：写进设置里的东西不该每次换个顺序）。
    pub fn collapsed_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.collapsed.iter().cloned().collect();
        keys.sort();
        keys
    }

    /// 把一批行拖到分组头（原型 §3.2）。
    ///
    /// 拖拽是「移动到分组」的**第二条入口**，不是第二套实现：落点解成目标分组后，
    /// 走的仍是宿主的 `request_move_to_group`（面板不碰库），连「拖到原分组 = 无操作」
    /// 也与菜单里当前项置灰同一口径（`dnd::rows_to_move`）。
    ///
    /// 无落点（「全部分组」聚合头）或没有行真的需要改时**不发请求**：拖着鼠标扫过一行
    /// 分组头不应该产生一次写库 + 一条回执。
    pub fn drop_rows_onto_group(
        &mut self,
        ids: &[String],
        target: GroupDrop,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.snapshot.read_only {
            return;
        }
        let moving = dnd::rows_to_move(&self.view_rows, ids, &target);
        if moving.is_empty() {
            return;
        }
        self.host
            .request_move_to_group(&moving, dnd::drop_folder(&target), window, cx);
        cx.notify();
    }

    /// `F2` / 「重命名…」的**目标**：单选语义 + 只读项目不给改（判据在 [`can_rename`]）。
    ///
    /// 与菜单项同一判据：改名没有“批量”的意义（改哪一条都是猜），只管焦点行。
    pub fn rename_target(&self) -> Option<String> {
        can_rename(self.multi.len(), self.snapshot.read_only)
            .then(|| self.selected.clone())
            .flatten()
    }

    /// 归档默认分组（当前值；`None` = 没设）。
    pub fn default_group(&self) -> Option<&str> {
        self.default_group.as_deref()
    }

    /// 注入默认分组（宿主构造期从设置读；**不回调宿主**——否则注入会被当成用户动作回写）。
    pub fn set_default_group(&mut self, folder_id: Option<String>, cx: &mut Context<Self>) {
        self.default_group = folder_id;
        // 标记画在分组头上，而头由**列表委托**渲染：得把新值推给它（否则要等下一次快照推送才更新）。
        self.push_rows_to_list(cx);
        cx.notify();
    }

    /// 设为 / 取消「归档默认分组」（分组头右键，原型 §4.1 的预选来源）。
    ///
    /// 与 [`toggle_group_collapse`](Self::toggle_group_collapse) 同一形态：先改本地（标记立刻
    /// 对上），再把结果交宿主落盘。它**不碰**已有存档的归属——只影响下一次归档的预选值。
    pub fn toggle_default_group(&mut self, folder_id: &str, cx: &mut Context<Self>) {
        let next =
            (self.default_group.as_deref() != Some(folder_id)).then(|| folder_id.to_string());
        self.default_group = next.clone();
        self.push_rows_to_list(cx);
        self.host.remember_default_group(next.as_deref(), cx);
        cx.notify();
    }

    /// 注入折叠集合（宿主在构造期从设置里读出来；不回调宿主——否则默认值会被当成用户动作回写）。
    pub fn set_collapsed(&mut self, keys: &[String], cx: &mut Context<Self>) {
        self.collapsed = keys.iter().cloned().collect();
        self.refresh_view_items(cx);
        cx.notify();
    }

    /// 重建可见项（分组头 + 行）。折叠 / 展开与注入都走它，免得两处各抄一遍参数。
    fn refresh_view_items(&mut self, cx: &mut Context<Self>) {
        self.view_items =
            filter::build_visible_items(&self.view_rows, &self.snapshot.groups, &self.collapsed);
        self.push_rows_to_list(cx);
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
        let groups = self.snapshot.groups.clone();
        let default_group = self.default_group.clone();
        let selected = self.selected.clone();
        let multi = self.multi.iter().cloned().collect();
        let read_only = self.snapshot.read_only;
        list.update(cx, |state, cx| {
            state.delegate_mut().set_rows(
                items,
                details,
                groups,
                default_group,
                selected,
                multi,
                read_only,
                cx,
            );
        });
    }

    /// 宿主推送数据（事件路径调用；面板不自己取数）。
    pub fn set_snapshot(&mut self, snapshot: ResourcesSnapshot, cx: &mut Context<Self>) {
        self.snapshot = snapshot;
        // 标签被删后清掉悬空条件（否则列表会“什么都没匹配”，而菜单上的勾还在）——
        // 与选中 / 多选的悬空清理同一个道理，但条件在筛选器里，所以在推送时先清。
        let alive: Vec<String> = self
            .snapshot
            .tags
            .iter()
            .map(|tag| tag.id.clone())
            .collect();
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

    /// 切换实例维度（§13：全部 / 项目 / 全局）。行源变了，悬空选中由
    /// [`Self::refresh_view_rows`] 顺带清理。
    pub fn set_instance(&mut self, instance: InstanceFilter, cx: &mut Context<Self>) {
        if self.instance == instance {
            return;
        }
        self.instance = instance;
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
    ///
    /// 选完把（字段 + 方向）告诉宿主——它会把**字段**写进设置（`resources.default_sort`），
    /// 下次打开面板就还是这一列。
    pub fn choose_sort(&mut self, field: SortField, cx: &mut Context<Self>) {
        if field == self.sort_field {
            self.sort_order = self.sort_order.flipped();
        } else {
            self.sort_field = field;
        }
        self.host
            .remember_sort(self.sort_field, self.sort_order, cx);
        self.refresh_view_rows(cx);
        cx.notify();
    }

    /// 直接设排序（宿主在构造期注入设置项里的默认排序）。
    ///
    /// 与 [`choose_sort`](Self::choose_sort) 分开：这一条**不回调宿主**——否则注入的默认值
    /// 会被原样写回去，看不出写回到底是用户的动作还是自己的回声。
    pub fn set_sort(&mut self, field: SortField, order: SortOrder, cx: &mut Context<Self>) {
        self.sort_field = field;
        self.sort_order = order;
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


#[cfg(test)]
mod tests;
