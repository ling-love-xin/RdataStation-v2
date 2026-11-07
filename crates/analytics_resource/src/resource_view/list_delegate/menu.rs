//! 存档行右键菜单：动作清单、置灰判据与「移动到分组」子菜单。

use gpui_kit::component::menu::PopupMenu;
use crate::model::TagTarget;

use super::*;

/// 菜单的可用性**由调用方算好传进来**（判据在 `model::affordance`，本文件只按结果挂 `.disabled`）。
///
/// 早先这里内联 `!can_open || multi > 1 || read_only` 一类表达式：同一件事在菜单里一份、
/// `F2` 里一份、详情里两份，四处漂移（全局行甚至会漏掉「不能再升级」这条）。
#[derive(Clone)]
pub(super) struct RowMenuParams {
    pub(super) host: Rc<dyn ResourcesHost>,
    pub(super) can_open: bool,
    pub(super) can_checkout: bool,
    pub(super) can_stats: bool,
    pub(super) can_distribute: bool,
    pub(super) can_promote: bool,
    pub(super) can_rename: bool,
    pub(super) can_edit_tags: bool,
    pub(super) can_move_to_group: bool,
    pub(super) can_delete: bool,
    pub(super) can_versions: bool,
    /// 只影响文案（「移入回收站（3 项）」）与子菜单摆不摆，不参与判据。
    pub(super) multi_count: usize,
    pub(super) tag_label: String,
    pub(super) tag_targets: Vec<TagTarget>,
    pub(super) selection_ids: Vec<String>,
    pub(super) delete_targets: Vec<DeleteTarget>,
    pub(super) groups: Vec<GroupOption>,
    pub(super) open_detail: Option<ArchiveDetail>,
    pub(super) distribute_name: String,
    pub(super) row_id: String,
    pub(super) folder_id: Option<String>,
}

pub(super) fn build(
    menu: PopupMenu,
    p: RowMenuParams,
    window: &mut Window,
    _cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    // 渲染期把参数拷成局部量：内层闭包要 `'static`，不能捕获 `p`。
    let host = p.host.clone();
    let open_detail = p.open_detail.clone();
    let checkout_detail = p.open_detail.clone();
    let groups_for_menu = p.groups.clone();
    let can_open = p.can_open;
    let can_checkout = p.can_checkout;
    let can_stats = p.can_stats;
    let can_distribute = p.can_distribute;
    let can_promote = p.can_promote;
    let can_rename = p.can_rename;
    let can_edit_tags = p.can_edit_tags;
    let can_move_to_group = p.can_move_to_group;
    let can_delete = p.can_delete;
    let can_versions = p.can_versions;
    let multi_count = p.multi_count;
    let tag_label = p.tag_label.clone();
    let tag_targets = p.tag_targets.clone();
    let selection_ids = p.selection_ids.clone();
    let delete_targets = p.delete_targets.clone();
    let distribute_name = p.distribute_name.clone();
    let id_versions = p.row_id.clone();
    let rename_id = p.row_id.clone();
    let folder_id = p.folder_id.clone();

                        let mut menu = menu;
                        menu = menu.item(
                            PopupMenuItem::new("打开（只读）")
                                .disabled(!can_open)
                                .on_click({
                                    let host = host.clone();
                                    let detail = open_detail.clone();
                                    move |_, window, cx| {
                                        if let Some(detail) = detail.as_ref() {
                                            tracing::info!(
                                                ?detail,
                                                "【探针】资源行菜单「打开（只读）」触发"
                                            );
                                            host.request_open(detail, window, cx);
                                        }
                                    }
                                }),
                        );
                        // 「分发到本项目…」（§13）：全局 → 项目，跨项目唯一法定路径。
                        // 全局行本体在全局 DuckDB，项目侧动作全置灰，分发是唯一可用动作；
                        // 判据（全局行 / 本体在位 / 单选 / 项目非只读）在 `can_distribute`。
                        menu = menu.item(
                            PopupMenuItem::new("分发到本项目…")
                                .disabled(!can_distribute)
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
                        // 判据 = 项目行 + 本体在位 + 单选 + 项目非只读（`can_promote`）：
                        // 全局行必须挡（否则把全局表再来一次升级）；分析表型存档由宿主侧再挡一层
                        // （其升级依赖分析表读取路径，M8 后续批次接）。
                        menu = menu.item(
                            PopupMenuItem::new("升级到全局…")
                                .disabled(!can_promote)
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
                                .disabled(!can_checkout)
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
                                .disabled(!can_versions)
                                .on_click({
                                    let host = host.clone();
                                    let id = id_versions.clone();
                                    move |_, window, cx| {
                                        host.request_version_history(&id, window, cx)
                                    }
                                }),
                        );
                        // 「重命名…」：只改**显示名**（文件名 / 路径不动，原型 §1 原则 2）；
                        // 判据（单选 / 项目非只读 / 项目行）在 `can_rename`——与 `F2` 同一份。
                        menu = menu.item(
                            PopupMenuItem::new("重命名…")
                                .disabled(!can_rename)
                                .on_click({
                                    let host = host.clone();
                                    let id = rename_id.clone();
                                    move |_, window, cx| host.request_rename(&id, window, cx)
                                }),
                        );
                        // 破坏性项用分隔线隔离；其上的两项是"本体在哪儿"的日常动作（原型 §3.2）。
                        menu = menu.item(
                            PopupMenuItem::new("复制路径")
                                .disabled(!can_open)
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
                                .disabled(!can_open)
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
                                .disabled(!can_edit_tags)
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
                        // 全局行没有项目分组归属、只读项目写不进去——两者都不摆（判据同拖拽）。
                        if can_move_to_group {
                            let move_ids: Vec<String> = selection_ids.clone();
                            menu = menu.submenu("移动到分组", window, _cx, {
                                let host = host.clone();
                                let move_ids = move_ids.clone();
                                let groups = groups_for_menu.clone();
                                let current = folder_id.clone();
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
                                                    host.request_move_to_group(
                                                        &ids, None, window, cx,
                                                    )
                                                }
                                            }),
                                    );
                                    for group in &groups {
                                        let is_current =
                                            current.as_deref() == Some(group.id.as_str());
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
                                            move |_, window, cx| {
                                                host.request_create_group(window, cx)
                                            }
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
                        menu.separator().item(
                            PopupMenuItem::new(delete_label)
                                .disabled(!can_delete)
                                .on_click({
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
                                }),
                        )
}
