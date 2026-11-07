//! 存档列表的 `List` 委托（虚拟化 + 组件化 hover / 选中 / 键盘漫游）。
//!
//! 拆分自 `resource_view/mod.rs`（P1）：`ArchiveListDelegate` 定义、`impl ArchiveListDelegate`
//! 与 `impl ListDelegate` 原样搬移。行数据是面板可见行的一份副本（`render_item` 在列表
//! 渲染期被调用，那一刻面板正被借用，不能回头读面板）。
//!
//! 可见性调整（P1 缝合）：结构体与供面板直连的字段/方法提为 `pub(crate)`——
//! `mod.rs` 的 `ensure_list` 用字段初始化构造它、`push_rows_to_list` 调 `set_rows`、
//! `sync_list_selection` 改 `syncing_from_panel`；`header_fold_key` 被窗口测试引用。

use std::rc::Rc;

use gpui_kit::base::StyledExt;
use gpui_kit::component::IndexPath;
use gpui_kit::component::list::{ListDelegate, ListItem, ListState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::detail_view::ArchiveDetail;
use crate::dnd::{self, ArchiveDragGhost, ArchiveDragPayload, GroupDrop};
use crate::filter::VisibleItem;
use crate::model::{ActionContext, ArchiveStatus, DeleteTarget, PayloadMode};
use crate::resource_view::{
    ArchiveRow, BadgeTone, DEFAULT_GROUP_MARKER, GroupOption, ResourcesHost, ResourcesPanel,
    RowClick, badge_tone, can_checkout, can_delete_row, can_distribute, can_edit_tags,
    can_move_to_group, can_open, can_promote, can_rename, can_view_stats, classify_row_click,
    default_group_entry_label, kind_icon, strength_badge, tag_entry_label, tag_targets,
};
use crate::ui;
// 行级共用原语（展开指示槽 / 图标）：与 M4 导航、M5 草稿箱同一份，不各自手搓（见 `workbench_shell::tree` 头注）。
use workbench_shell::tree;
mod group_header;
mod menu;
mod row;

/// 列表行的统一外壳：**行距是单一值**（`List` 只量一个样本行定全局行距），所以两处行
/// （分组头 / 存档行）必须同形同高，外壳只此一处。
///
/// 为什么要压掉 `ListItem` 自带的 `py_1`（4px）：不压的话实际行距是 24 + 8 = 32px
/// （规格 `ui::ROW_HEIGHT` 是 24px），而且多选行自绘的 `list_active` 底只盖中间 24px，
/// 悬停时上下各露出一条 `list_hover` 光晕。
fn list_row(id: SharedString) -> ListItem {
    ListItem::new(id).py_0()
}

/// 停在分组头上按 Enter 该折叠哪个分组（`None` = 当前位置不是分组头）。
///
/// `List` 把 Enter 走 `confirm` 通道，而分组头不是行（没有打开 / 取回语义），所以
/// 它的唯一确认含义就是「折叠 / 展开自己」。抽成纯函数：索引→key 的映射是这里唯一的规则，
/// 免得跟 `render_group_header` 里的判定各写一份。
pub(crate) fn header_fold_key(items: &[VisibleItem], ix: IndexPath) -> Option<String> {
    match items.get(ix.row) {
        Some(VisibleItem::GroupHeader { key, .. }) => Some(key.clone()),
        _ => None,
    }
}

/// 存档列表的 `List` 委托（虚拟化 + 组件化 hover / 选中 / 键盘漫游）。
///
/// 行数据是**面板可见行的一份副本**：`render_item` 在列表渲染期被调用，而那一刻面板实体
/// 正被借用（渲染就是从面板的 `render` 进来的），回头去读面板的行集合会直接 panic——
/// 与 `mock` 的生成器搜索委托同例。面板每次重算可见行都把副本推过来。
pub(crate) struct ArchiveListDelegate {
    /// 动作去向（右键菜单用；面板不认识服务层）。
    pub(crate) host: Rc<dyn ResourcesHost>,
    /// 面板句柄（选中变化回传；面板已销毁时静默丢弃）。
    pub(crate) panel: WeakEntity<ResourcesPanel>,
    /// 可见项（分组头 + 行）：分区渲染后列表的“行”就是它们。
    pub(crate) items: Vec<VisibleItem>,
    /// 行 id → 详情：右键菜单发起取回时要用整条详情（`render_item` 里读不了面板，只能提前拷一份）。
    pub(crate) details: std::collections::HashMap<String, ArchiveDetail>,
    /// 分组字典：行菜单的「移动到分组」子菜单与分组头菜单要用它（`render_item` 里读不了面板）。
    pub(crate) groups: Vec<GroupOption>,
    /// 归档默认分组（本项目）：分组头上的「默认」标记按它画。
    pub(crate) default_group: Option<String>,
    /// 选中的行 id（面板是语义权威，这里是渲染与漫游的锚点）。
    pub(crate) selected_id: Option<String>,
    /// 多选集合（**含焦点行**）：行背景与右键菜单的“多选态”靠它；
    /// 选中变更时由面板推一份新副本过来（`render_item` 里读不了面板）。
    pub(crate) multi_ids: std::collections::HashSet<String>,
    /// 面板正在把自己的选中镜像进列表。
    ///
    /// 此间组件回调的 `set_selected_index` **不再回写面板**：镜像发生在面板渲染期，
    /// 回写就是"更新正在被更新的实体"（GPUI 直接 panic）。
    pub(crate) syncing_from_panel: bool,
    /// 漫游 / 选中位置停在**分组头**上时它的 key（停在行上 / 空列表时为 `None`）。
    ///
    /// 分组头不参与选中（面板的选中永远指向一条存档），但它确实可能停在漫游位置上；
    /// 记住它才能让 Enter 与点击同义（见 `confirm`）。
    pub(crate) focused_header: Option<String>,
    /// 项目只读（标题栏锁）：只读时行内动作只剩「打开」。
    pub(crate) read_only: bool,
}

impl ArchiveListDelegate {
    /// 面板推送可见项（快照 / 筛选 / 排序 / 折叠变化时调用）。
    pub(crate) fn set_rows(
        &mut self,
        items: Vec<VisibleItem>,
        details: std::collections::HashMap<String, ArchiveDetail>,
        groups: Vec<GroupOption>,
        default_group: Option<String>,
        selected_id: Option<String>,
        multi_ids: std::collections::HashSet<String>,
        read_only: bool,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.items = items;
        self.details = details;
        self.groups = groups;
        self.default_group = default_group;
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
}


impl ListDelegate for ArchiveListDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.items.len()
    }

    /// 一行（或一个分组头）：信息层交 `row` / `group_header`，动作层交 `menu`。
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
            return Some(group_header::render_group_header(
                self, &key, &label, count, depth, collapsed, cx,
            ));
        }
        let row = self.row_at(ix)?.clone();
        // 全局行（§13）：本体在全局 DuckDB / 全局登记，项目侧动作一律不可用。
        let is_global = row.scope == crate::model::RowScope::Global;
        let multi_count = self.multi_ids.len();
        let multi_selected = self.multi_selected(&row.id);
        // 本次操作的**选集**（多选时是整选集，否则就是这一行）——删除 / 移动 / 打标签 / 拖拽共用一份。
        let selection_ids: Vec<String> = if multi_selected {
            // 按可见行顺序过滤（HashSet 迭代序不定）：回执与测试都要确定性。
            self.visible_rows()
                .filter(|candidate| self.multi_ids.contains(&candidate.id))
                .map(|candidate| candidate.id.clone())
                .collect()
        } else {
            vec![row.id.clone()]
        };
        // 打标签要连**显示名**一起给宿主（对话框标题与回执要报“哪几项”）：同样按选集顺序取。
        let all_rows: Vec<&ArchiveRow> = self.visible_rows().collect();
        let tag_targets = tag_targets(&all_rows, &self.multi_ids, &row);
        // 删除选集条目：带上归档粒度，确认框文案按 copy / reference 区分本体命运（§12.1.1 ③）。
        let delete_targets: Vec<DeleteTarget> = selection_ids
            .iter()
            .map(|id| DeleteTarget {
                id: id.clone(),
                mode: self
                    .details
                    .get(id)
                    .map(|detail| detail.payload_mode)
                    .unwrap_or(PayloadMode::DEFAULT),
            })
            .collect();
        let tag_label = tag_entry_label(multi_count);
        // 打开 / 取回都要整条详情：渲染期提前拷一份（菜单回调触发时面板已被借用）。
        let open_detail = self.details.get(&row.id).cloned();
        let read_only = self.read_only;
        // 动作判据**统一在 `model::affordance`**：右键菜单 / `F2` / 详情头部 / 详情动作区
        // 共用同一份判定，这里只把上下文装好（逐项判据不再各写一份）。
        let ctx = open_detail
            .as_ref()
            .map(|detail| detail.action_context(read_only, multi_count))
            .unwrap_or_else(|| ActionContext::from_row(row.scope, row.status, read_only, multi_count));
        // 逐项判据（变量名带 `_row` 后缀：避免遮蔽同名判定函数，下面还有调用）。
        let can_open_row = can_open(&ctx);
        // 「查看统计」按种类判（`can_view_stats` 要本体路径与连接信息，比只判状态细）。
        let can_stats = open_detail.as_ref().is_some_and(can_view_stats);
        let can_checkout_row = can_checkout(&ctx);
        let can_distribute_row = can_distribute(&ctx);
        let can_promote_row = can_promote(&ctx);
        let can_rename_row = can_rename(&ctx);
        let can_edit_tags_row = can_edit_tags(&ctx);
        let can_move_to_group_row = can_move_to_group(&ctx);
        let can_delete_row_flag = can_delete_row(&ctx);
        // 拖到分组头 = 移动：只读项目 / 全局行不给拖（拖起来也落不下去）。
        let drag_payload = can_move_to_group_row.then(|| ArchiveDragPayload {
            ids: selection_ids.clone(),
            kind: row.kind,
            label: dnd::drag_label(&row.name, selection_ids.len()),
        });
        let line = row::render_row_line(&row, is_global, cx);
        let multi_bg = cx.theme().colors.list_active;
        let params = menu::RowMenuParams {
            host: self.host.clone(),
            can_open: can_open_row,
            can_checkout: can_checkout_row,
            can_stats,
            can_distribute: can_distribute_row,
            can_promote: can_promote_row,
            can_rename: can_rename_row,
            can_edit_tags: can_edit_tags_row,
            can_move_to_group: can_move_to_group_row,
            can_delete: can_delete_row_flag,
            can_versions: !is_global && multi_count <= 1,
            multi_count,
            tag_label,
            tag_targets,
            selection_ids,
            delete_targets,
            groups: self.groups.clone(),
            open_detail: open_detail.clone(),
            distribute_name: row.name.clone(),
            row_id: row.id.clone(),
            folder_id: row.folder_id.clone(),
        };

        // 行点击自己接管（原型 §2.3）：单击=选中、双击=打开，确认通道只留给 `Enter`。
        let click_detail = open_detail.clone();
        let click_id = row.id.clone();
        let click_handler = {
            let host = self.host.clone();
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
            list_row(SharedString::from(format!("archive-row-{}", row.id))).child(
                // 菜单挂在行内带稳定 id 的容器上：`context_menu` 返回 `ContextMenu<ListItem>`，
                // 而委托的 Item 必须是 `ListItem`。
                div()
                    .id(SharedString::from(format!("archive-row-menu-{}", row.id)))
                    .w_full()
                    // 多选高亮自己画：组件的选中样式只认它的单选索引（= 焦点行）。
                    .when(multi_selected, |container| container.bg(multi_bg))
                    .on_click(click_handler)
                    .when_some(drag_payload, |container, payload| {
                        container.on_drag(payload, |payload: &ArchiveDragPayload, _, _, cx| {
                            let label = payload.label.clone();
                            let kind = payload.kind;
                            cx.new(|_| ArchiveDragGhost { label, kind })
                        })
                    })
                    .child(line)
                    .context_menu(move |menu, window, cx| {
                        menu::build(menu, params.clone(), window, cx)
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
        // 位置落在分组头上就记下它的 key（分组头不参与选中，所以它没有别的确认语义）：
        // 这一步要放在下面的早退**之前**——面板镜像也会移动列表索引，而「停在哪个头上」
        // 正是由那一次位置变化决定的。
        self.focused_header = ix.and_then(|ix| header_fold_key(&self.items, ix));
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
        let _ = self
            .panel
            .update(cx, |panel, cx| panel.set_selected(id, cx));
    }

    /// 回车 / 双击：打开（只读），与右键菜单第一项同口径。
    ///
    /// 例外：停在**分组头**上时回车 = 折叠 / 展开它（与点头部同一条路）——头不是行，
    /// 没有可打开的东西，而 Enter 是它键盘上唯一的入口（头部本身是自绘的可点行）。
    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        if let Some(key) = self.focused_header.clone() {
            let _ = self
                .panel
                .update(cx, |panel, cx| panel.toggle_group_collapse(&key, cx));
            return;
        }
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
