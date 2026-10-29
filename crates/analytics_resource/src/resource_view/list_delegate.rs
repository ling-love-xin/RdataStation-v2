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
use crate::model::{ArchiveStatus, DeleteTarget, PayloadMode};
use crate::resource_view::{
    ArchiveRow, BadgeTone, DEFAULT_GROUP_MARKER, GroupOption, ResourcesHost, ResourcesPanel,
    RowClick, badge_tone, can_rename, can_view_stats, classify_row_click,
    default_group_entry_label, kind_icon, strength_badge, tag_entry_label, tag_targets,
};
use crate::ui;
// 行级共用原语（展开指示槽 / 图标）：与 M4 导航、M5 草稿箱同一份，不各自手搓（见 `workbench_shell::tree` 头注）。
use workbench_shell::tree;

// ==================== 列表委托 ====================

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

    /// 分组头（原型 §2.4）：色条 + 展开指示 + 名称 + 计数；点整行折叠 / 展开，
    /// 停在它上面按 Enter 也一样（见 `confirm`），
    /// 右键是分组自身的动作（重命名 / 删除 / 新建）——「全部分组」与「未分组」是虚拟分组，
    /// 没有可改的东西，所以不给菜单。
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
            // 分组色条取 `primary`：与 M4 分组头同角色（`rds-theme.json` 里与
            // `list_active_border` 明暗同值，所以是零视觉变化的口径统一）。不用
            // `sidebar_accent`：那个角色在浅色下与面板底几乎同色。
            (colors.primary, colors.muted_foreground, colors.foreground)
        };
        let panel = self.panel.clone();
        let key_owned = key.to_string();
        // 默认分组的标记要画在头上：只拷一份，供后面判断（`render_item` 里读不了面板）。
        let default_group = self.default_group.clone();
        let list_id = format!("archive-group-{key}");
        let debug_id = list_id.clone();
        let host = self.host.clone();
        let is_real_group =
            key != crate::filter::GROUP_ALL && key != crate::filter::GROUP_UNGROUPED;

        let head = div()
            .id(SharedString::from(format!("{list_id}-row")))
            .debug_selector(move || debug_id.clone())
            .w_full()
            .h(rems(ui::ROW_HEIGHT))
            .h_flex()
            .items_center()
            .gap_2()
            .when(depth > 0, |row| row.pl_2())
            .cursor_pointer()
            .on_click({
                let panel = panel.clone();
                move |_, _window, cx| {
                    // 头不是行：不参与选中（面板的选中永远指向一条存档）。
                    cx.stop_propagation();
                    let key = key_owned.clone();
                    let _ = panel.update(cx, |panel, cx| panel.toggle_group_collapse(&key, cx));
                }
            })
            .child(
                // 2px 分组色条：宽取 `ui::GROUP_BAR_WIDTH`（= 外壳的 `NAV_GROUP_BAR_WIDTH`，
                // 固定描边，不随主题字号缩放），高度 / 圆头与 M4 分组头同形。
                div()
                    .w(ui::GROUP_BAR_WIDTH)
                    .h(rems(0.875))
                    .flex_none()
                    .rounded_full()
                    .bg(bar),
            )
            // 展开指示走共用原语：`disclosure_slot` 的 10px 槽宽是缩进算式的一部分
            // （行内容 = `pl` + 槽 + `gap_1`），与 M4/M5 换载体时一样——只动这一行调用，
            // 同层行的标题左边缘不会漂。
            .child(tree::disclosure_slot().child(tree::disclosure_icon(!collapsed, muted)))
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
                // 「默认」标记：本项目里带这个标记的分组，就是归档对话框的预选落点。
                // 只有一条时才有——它是**事实提示**，不是控件（设 / 取消在右键菜单里）。
                div().flex_none().text_xs().text_color(muted).when_some(
                    (default_group.as_deref() == Some(key))
                        .then(|| SharedString::from(format!("archive-group-default-{key}"))),
                    |marker, id| {
                        marker
                            .debug_selector(move || id.to_string())
                            .child(DEFAULT_GROUP_MARKER)
                    },
                ),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(format!("{count}")),
            );
        // 落点：分组头接住存档行（原型 §3.2「拖动行到分组头 = 移动」）。
        // 高亮用 `list_active`（与选中底同 token，见 `ui-constraints.md` §8.3）：
        // 拖拽期间这行就是「东西要进到这里」的那个容器。
        let drop = dnd::group_drop_target(key);
        let head = if matches!(drop, GroupDrop::NotATarget) {
            head
        } else {
            head.drag_over::<ArchiveDragPayload>(|style, _, _, cx| {
                style.bg(cx.theme().colors.list_active)
            })
            .on_drop({
                let panel = panel.clone();
                let drop = drop.clone();
                move |payload: &ArchiveDragPayload, window, app| {
                    let ids = payload.ids.clone();
                    let drop = drop.clone();
                    let _ = panel.update(app, |panel, cx| {
                        panel.drop_rows_onto_group(&ids, drop.clone(), window, cx)
                    });
                }
            })
        };
        // 右键菜单只给真分组（「全部分组」与「未分组」是虚拟分组，没有可改的东西）。
        // `context_menu` 包一层（`ContextMenu<..>` 不是 `Div`）→ 两条分支各自转 `AnyElement` 后再拼。
        let head: AnyElement = if is_real_group {
            let folder_id = key.to_string();
            let is_default = default_group.as_deref() == Some(key);
            let panel_for_default = panel.clone();
            head.context_menu(move |menu, _window, _cx| {
                menu.item(PopupMenuItem::new("重命名…").on_click({
                    let host = host.clone();
                    let folder_id = folder_id.clone();
                    move |_, window, cx| host.request_rename_group(&folder_id, window, cx)
                }))
                // 「设为 / 取消归档默认分组」：默认分组的**唯一入口**（原型 §4.1 的预选来源）。
                // 放在重命名与删除之间：它是这个分组的日常属性，不是破坏性动作。
                .item(
                    PopupMenuItem::new(default_group_entry_label(is_default)).on_click({
                        let folder_id = folder_id.clone();
                        let panel = panel_for_default.clone();
                        move |_, _window, cx| {
                            let folder_id = folder_id.clone();
                            let _ = panel.update(cx, |panel, cx| {
                                panel.toggle_default_group(&folder_id, cx);
                            });
                        }
                    }),
                )
                .item(PopupMenuItem::new("删除分组").on_click({
                    let host = host.clone();
                    let folder_id = folder_id.clone();
                    move |_, window, cx| host.request_delete_group(&folder_id, window, cx)
                }))
                .separator()
                .item(PopupMenuItem::new("新建分组…").on_click({
                    let host = host.clone();
                    move |_, window, cx| host.request_create_group(window, cx)
                }))
            })
            .into_any_element()
        } else {
            head.into_any_element()
        };

        list_row(SharedString::from(list_id.clone())).child(head)
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
        // 全局行（§13）：本体在全局 DuckDB / 全局登记，项目侧动作一律不可用，
        // 徽标显示「全局」——强度徽标语义在全局侧不适用（升级产物 kind 都是 analysis）。
        let is_global = row.scope == crate::model::RowScope::Global;
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
        // 全局行徽标 = 「全局」；项目行保持复现强度徽标（原型 §3.2）。
        let badge = if is_global {
            "全局"
        } else {
            strength_badge(row.kind, row.status)
        };
        let badge_color = if is_global {
            info
        } else {
            match badge_tone(row.kind, row.status) {
                BadgeTone::Success => success,
                BadgeTone::Info => info,
                BadgeTone::Warning => warning,
                BadgeTone::Danger => danger,
            }
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
        // 删除 / 移动 / 打标签改为对**整个选择集**生效并带数量。
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
        // 详情映射缺项（全局行 / 异常旧行）回退 Copy，不臆造模式。
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
        // 全局行（§13）：本体在全局 DuckDB / 全局登记，项目侧动作（取回 / 改名 / 删除 /
        // 打标 / 分组 / 拖拽 / 版本历史）一律不可用——只有「分发到本项目」。
        // 取回要往草稿箱写一份工作副本：本体异常的存档（缺失 / 内容已变）与只读项目都不给走。
        let can_checkout =
            !is_global && row.status == ArchiveStatus::Normal && !self.read_only;
        // 只读项目连标签也不给改（菜单项的置灰判据在闭包内用，故先拷一份）。
        let read_only = self.read_only;
        // 拖到分组头 = 移动（原型 §3.2）：载荷带**一批** id（多选时是整选集）。
        // 只读项目不给拖：拖起来也落不下去（宿主会拒），不如与菜单项一样当场置灰。
        let drag_payload = (!is_global && !read_only).then(|| ArchiveDragPayload {
            ids: selection_ids.clone(),
            kind: row.kind,
            label: dnd::drag_label(&row.name, selection_ids.len()),
        });
        // 打开 / 取回都要整条详情（本体路径 / 扩展名在它身上）：渲染期提前拷一份——
        // 菜单回调触发时面板可能已被借用，不能再回头读。全局行不在详情映射里（None）。
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
        // 版本历史的入口只带 id（对话框的显示名由宿主查库得到）；重命名同样只带 id
        // （当前名字由宿主的 `row_name` 从快照取——渲染期不把名字搬进闭包）。
        let id_versions = row.id.clone();
        let rename_id = row.id.clone();
        // 分发要全局资源显示名（全局行本体不在项目库，分发只认全局登记的名字）。
        let distribute_name = row.name.clone();

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

        // 行菜单的子菜单要分组字典：**在闭包之外**拷一份（闭包是 `'static`，捕获 `self` 借用会逃逸）。
        let groups_for_menu = self.groups.clone();

        Some(
            list_row(SharedString::from(format!("archive-row-{}", row.id))).child(
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
                    .when_some(drag_payload, |container, payload| {
                        container.on_drag(payload, |payload: &ArchiveDragPayload, _, _, cx| {
                            let label = payload.label.clone();
                            let kind = payload.kind;
                            cx.new(|_| ArchiveDragGhost { label, kind })
                        })
                    })
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
                                            tracing::info!(?detail, "【探针】资源行菜单「打开（只读）」触发");
                                            host.request_open(detail, window, cx);
                                        }
                                    }
                                }),
                        );
                        // 「分发到本项目…」（§13）：全局 → 项目，跨项目唯一法定路径。
                        // 全局行本体在全局 DuckDB，项目侧动作全置灰，分发是唯一可用动作；
                        // 只读项目也不给分发（分发写项目库）。
                        menu = menu.item(
                            PopupMenuItem::new("分发到本项目…")
                                .disabled(!is_global || multi_count > 1 || read_only)
                                .on_click({
                                    let host = host.clone();
                                    let name = distribute_name.clone();
                                    move |_, window, cx| {
                                        host.request_distribute_from_global(&name, window, cx)
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
                        // 「升级到全局…」（§13）：项目 → 全局，表 + 版本信息进全局 DuckDB。
                        // 判据与「打开」同源（File 档且本体在位）；只读项目不给升级
                        // （升级会写全局库，本质是一次发布）。分析表型存档由宿主侧再挡一层
                        // （其升级依赖分析表读取路径，M8 后续批次接）。
                        menu = menu.item(
                            PopupMenuItem::new("升级到全局…")
                                .disabled(!can_open || multi_count > 1 || read_only)
                                .on_click({
                                    let host = host.clone();
                                    let detail = open_detail.clone();
                                    move |_, window, cx| {
                                        if let Some(detail) = detail.as_ref() {
                                            host.request_promote_to_global(detail, window, cx);
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
                                .disabled(multi_count > 1 || is_global)
                                .on_click({
                                    let host = host.clone();
                                    let id = id_versions.clone();
                                    move |_, window, cx| {
                                        host.request_version_history(&id, window, cx)
                                    }
                                }),
                        );
                        // 「重命名…」：只改**显示名**（文件名 / 路径不动，原型 §1 原则 2）；
                        // 多选不给改（改哪一条都是猜），只读项目不给改。
                        menu = menu.item(
                            PopupMenuItem::new("重命名…")
                                .disabled(!can_rename(multi_count, read_only) || is_global)
                                .on_click({
                                    let host = host.clone();
                                    let id = rename_id.clone();
                                    move |_, window, cx| host.request_rename(&id, window, cx)
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
                        // 「编辑标签…」：与「移动到分组」同类的组织动作，一起摆在破坏性项之上。
                        // 多选时作用于整选集（勾选 = 全部打上，取消 = 全部摘掉）。
                        menu = menu.item(
                            PopupMenuItem::new(tag_label.clone())
                                .disabled(read_only || is_global)
                                .on_click({
                                    let host = host.clone();
                                    let targets = tag_targets.clone();
                                    move |_, window, cx| {
                                        host.request_edit_tags(&targets, window, cx)
                                    }
                                }),
                        );
                        // 「移动到分组 ›」：子菜单列出各分组 + 未分组 + 新建（原型 §2.4：
                        // 拖拽之外的等价入口；多选也用这一条，批量移动是原型里多选解锁的动作）。
                        // 全局行没有项目分组归属，整个子菜单不给摆。
                        if !is_global {
                            let move_ids: Vec<String> = selection_ids.clone();
                            menu = menu.submenu("移动到分组", _window, _cx, {
                            let host = host.clone();
                            let move_ids = move_ids.clone();
                            let groups = groups_for_menu.clone();
                            let current = row.folder_id.clone();
                            move |menu, _window, _cx| {
                                let mut menu = menu;
                                // 「未分组」：当前已在未分组时置灰（点了也是白点）。
                                menu = menu.item(
                                    PopupMenuItem::new("未分组")
                                        .checked(current.is_none())
                                        .disabled(current.is_none())
                                        .on_click({
                                            let host = host.clone();
                                            let ids = move_ids.clone();
                                            move |_, window, cx| {
                                                host.request_move_to_group(&ids, None, window, cx)
                                            }
                                        }),
                                );
                                for group in &groups {
                                    let is_current = current.as_deref() == Some(group.id.as_str());
                                    menu = menu.item(
                                        PopupMenuItem::new(group.name.clone())
                                            .checked(is_current)
                                            .disabled(is_current)
                                            .on_click({
                                                let host = host.clone();
                                                let ids = move_ids.clone();
                                                let folder_id = group.id.clone();
                                                move |_, window, cx| {
                                                    host.request_move_to_group(
                                                        &ids,
                                                        Some(&folder_id),
                                                        window,
                                                        cx,
                                                    )
                                                }
                                            }),
                                    );
                                }
                                menu.separator()
                                    .item(PopupMenuItem::new("新建分组…").on_click({
                                        let host = host.clone();
                                        move |_, window, cx| host.request_create_group(window, cx)
                                    }))
                            }
                            });
                        }
                        // 删除是**唯一**多选可用的破坏性项（原型 §3.2）：多选时带上数量。
                        // 全局行不能进项目回收站（本体不在项目侧）。
                        let delete_targets = delete_targets.clone();
                        let delete_label = if multi_count > 1 {
                            format!("移入回收站（{multi_count} 项）")
                        } else {
                            "移入回收站".to_string()
                        };
                        menu.separator()
                            .item(PopupMenuItem::new(delete_label).disabled(is_global).on_click({
                                let host = host.clone();
                                let targets = delete_targets.clone();
                                move |_, window, cx| {
                                    tracing::info!(
                                        count = targets.len(),
                                        ids = ?targets,
                                        "【探针】资源行菜单「移入回收站」触发"
                                    );
                                    host.request_delete(&targets, window, cx)
                                }
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

