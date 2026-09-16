//! 数据源导航的**存储门面**：按「连接 ID 前缀 + 项目根」路由到全局库 / 项目库。
//!
//! 真正的读写实现在 `engine::persistence`（`ConnectionOrgStore` / `NavigatorStateStore`），
//! 本模块只做两件事：
//!
//! 1. **路由**：`G_` / 遗留 `conn-` 前缀 → 全局库；`P_` / `GP_` → 项目库（须有项目根）；
//! 2. **容错**：读取类接口失败返回默认值（空表 / 默认状态），写出类接口返回 `Err(文案)`。
//!
//! 为什么住在 `database` 而不是 `workbench`：这些调用点全在导航视图里，视图现已归本 crate；
//! 而它们只依赖 `engine`，不需要宿主参与——故此**不走宿主 trait**，也就没有「分组存储 trait」
//! 这一层多余抽象。
//!
//! 落库语义见 `docs/architecture/database/database-navigator-prototype-design.md`：
//! 标签 / 分组 / 排序是连接域数据，导航状态是视图数据，两者都**不自动删除**。

use std::collections::HashMap;
use std::path::Path;

use engine::persistence::{
    id_prefix, ConnectionGroup, ConnectionOrgStore, NavState, NavigatorStateStore, UNGROUPED_SCOPE,
};

// ==================== 连接 → 库的路由 ====================

/// 打开导航状态存储（展开 / 选中 / 过滤）。
fn open_state_store(
    conn_id: &str,
    project_root: Option<&Path>,
) -> Result<NavigatorStateStore, String> {
    if id_prefix::uses_project_storage(conn_id) {
        let root = project_root.ok_or_else(|| "未打开项目，无法读写项目导航状态".to_string())?;
        NavigatorStateStore::open_project(root).map_err(|e| e.to_string())
    } else {
        NavigatorStateStore::open_global().map_err(|e| e.to_string())
    }
}

/// 打开连接组织存储（标签 / 分组的权威源）。
fn open_org_store(
    conn_id: &str,
    project_root: Option<&Path>,
) -> Result<ConnectionOrgStore, String> {
    if id_prefix::uses_project_storage(conn_id) {
        let root = project_root.ok_or_else(|| "未打开项目，无法读写项目连接标签".to_string())?;
        ConnectionOrgStore::open_project(root).map_err(|e| e.to_string())
    } else {
        ConnectionOrgStore::open_global().map_err(|e| e.to_string())
    }
}

/// 打开**项目侧**组织存储（分组本身只存在于项目库）。
fn open_org_project(project_root: Option<&Path>) -> Result<ConnectionOrgStore, String> {
    let root = project_root.ok_or_else(|| "未打开项目，无法读写分组".to_string())?;
    ConnectionOrgStore::open_project(root).map_err(|e| e.to_string())
}

// ==================== 导航视图状态 ====================

/// 读取导航状态（缺失返回默认）。
pub fn load_nav_state(conn_id: &str, project_root: Option<&Path>) -> NavState {
    match open_state_store(conn_id, project_root) {
        Ok(store) => store.load_state(conn_id),
        Err(_) => NavState::default(),
    }
}

/// 保存导航状态。
pub fn save_nav_state(
    conn_id: &str,
    project_root: Option<&Path>,
    state: &NavState,
) -> Result<(), String> {
    let store = open_state_store(conn_id, project_root)?;
    let scope = if id_prefix::uses_project_storage(conn_id) {
        "project"
    } else {
        "global"
    };
    store
        .save_state(conn_id, scope, state)
        .map_err(|e| e.to_string())
}

// ==================== 标签 ====================

/// 读取连接标签（多值）。权威源为连接组织存储（连接域数据），非导航视图状态。
pub fn list_tags(conn_id: &str, project_root: Option<&Path>) -> Vec<String> {
    open_org_store(conn_id, project_root)
        .map(|s| s.list_tags(conn_id))
        .unwrap_or_default()
}

/// 覆盖式设置连接标签。
pub fn set_tags(conn_id: &str, project_root: Option<&Path>, tags: &[String]) -> Result<(), String> {
    let store = open_org_store(conn_id, project_root)?;
    store.set_tags(conn_id, tags).map_err(|e| e.to_string())
}

/// 读取当前项目可见连接的全部标签映射（全局库 + 项目库合并）。
pub fn list_all_tags(project_root: Option<&Path>) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let mut merge = |pairs: Vec<(String, String)>| {
        for (conn_id, tag) in pairs {
            map.entry(conn_id).or_default().push(tag);
        }
    };
    if let Ok(store) = ConnectionOrgStore::open_global() {
        merge(store.list_tag_pairs());
    }
    if let Some(root) = project_root {
        if let Ok(store) = ConnectionOrgStore::open_project(root) {
            merge(store.list_tag_pairs());
        }
    }
    map
}

// ==================== 分组 ====================

/// 列出全部分组（项目级，按 `sort_order` 排序）。
pub fn list_groups(project_root: Option<&Path>) -> Vec<ConnectionGroup> {
    open_org_project(project_root)
        .map(|s| s.list_groups())
        .unwrap_or_default()
}

/// 新建分组（名称 + 描述），返回分组 ID。
pub fn create_group_with(
    project_root: Option<&Path>,
    name: &str,
    description: Option<&str>,
) -> Result<String, String> {
    let store = open_org_project(project_root)?;
    let id = id_prefix::generate_pid("grp");
    store
        .create_group(&id, name, description)
        .map_err(|e| e.to_string())?;
    Ok(id)
}

/// 更新分组（名称 + 描述），**排序保留库中现值**。
///
/// `update_group` 要求完整字段（名称 + 描述 + 排序），故先读回现值再写。
pub fn update_group(
    project_root: Option<&Path>,
    group_id: &str,
    name: &str,
    description: Option<&str>,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    let sort_order = store
        .list_groups()
        .into_iter()
        .find(|g| g.id == group_id)
        .map(|g| g.sort_order)
        .unwrap_or(0);
    store
        .update_group(group_id, name, description, sort_order)
        .map_err(|e| e.to_string())
}

/// 重命名分组（保留描述与排序）。
///
/// 不再传 `None` 描述：那会把已有描述洗掉（曾经的缺陷），改名时描述必须保留。
pub fn rename_group(
    project_root: Option<&Path>,
    group_id: &str,
    name: &str,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    let current = store
        .list_groups()
        .into_iter()
        .find(|g| g.id == group_id);
    let (description, sort_order) = current
        .map(|g| (g.description, g.sort_order))
        .unwrap_or((None, 0));
    store
        .update_group(group_id, name, description.as_deref(), sort_order)
        .map_err(|e| e.to_string())
}

/// 删除分组（不删连接）。
pub fn delete_group(project_root: Option<&Path>, group_id: &str) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store.delete_group(group_id).map_err(|e| e.to_string())
}

// ==================== 分组 ↔ 连接 ====================

/// 分组成员连接 ID（按组内顺序）。
///
/// 未手动排序的成员排在最后，且**未按名称排**——名称不在组织存储里，
/// 要靠视图侧 [`list_group_members_detailed`] + `nav_order_members` 补齐。
pub fn list_group_members(project_root: Option<&Path>, group_id: &str) -> Vec<String> {
    open_org_project(project_root)
        .map(|s| s.list_group_members(group_id))
        .unwrap_or_default()
}

/// 分组成员 + 是否手动排序过（`None` = 未排；`Some(idx)` = 手动序号）。
pub fn list_group_members_detailed(
    project_root: Option<&Path>,
    group_id: &str,
) -> Vec<(String, Option<i64>)> {
    open_org_project(project_root)
        .map(|s| s.list_group_members_detailed(group_id))
        .unwrap_or_default()
}

/// 加入分组。
pub fn add_to_group(
    project_root: Option<&Path>,
    group_id: &str,
    conn_id: &str,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store.add_member(group_id, conn_id).map_err(|e| e.to_string())
}

/// 移出分组。
pub fn remove_from_group(
    project_root: Option<&Path>,
    group_id: &str,
    conn_id: &str,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store.remove_member(group_id, conn_id).map_err(|e| e.to_string())
}

/// 移出全部分组（连接回到「未分组」）。
///
/// 与分组内联编辑器的「全部取消勾选」同效果：先清关系，再写未分组顺序
/// （`set_container_order` 会把该连接追加到未分组末尾）。
pub fn remove_from_all_groups(project_root: Option<&Path>, conn_id: &str) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store
        .set_connection_groups(conn_id, &[])
        .map_err(|e| e.to_string())
}

// ==================== 排序 ====================

/// 「未分组」容器的显式顺序（连接 ID，未手动排序的不出现）。
pub fn list_ungrouped_order(project_root: Option<&Path>) -> Vec<String> {
    open_org_project(project_root)
        .map(|s| s.list_ungrouped_order())
        .unwrap_or_default()
}

/// 重写容器内成员顺序（`scope_id` = 分组 ID 或 [`UNGROUPED_SCOPE`]）。
///
/// 一次性 `0..n` 重写（不是相对插入），保证序号完整、与屏上顺序一致。
/// 未分组容器走单独的表（它不是真实分组，成员由推导得出）。
pub fn set_container_order(
    project_root: Option<&Path>,
    scope_id: &str,
    conn_ids: &[String],
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    let result = if scope_id == UNGROUPED_SCOPE {
        store.set_ungrouped_order(conn_ids)
    } else {
        store.set_member_order_all(scope_id, conn_ids)
    };
    result.map_err(|e| e.to_string())
}

/// 重写分组之间的顺序（序号 = 下标）。
pub fn set_group_order(project_root: Option<&Path>, group_ids: &[String]) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store.set_group_order(group_ids).map_err(|e| e.to_string())
}

// ==================== 主组 ====================

/// 显式主组映射（连接 ID → 主组 ID；仅显式指定过的连接）。
pub fn list_primary_groups(project_root: Option<&Path>) -> HashMap<String, String> {
    open_org_project(project_root)
        .map(|s| s.list_primary_group_pairs().into_iter().collect())
        .unwrap_or_default()
}

/// 设置连接的主组（同一连接同时只能有一个）。
pub fn set_primary_group(
    project_root: Option<&Path>,
    conn_id: &str,
    group_id: &str,
) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store
        .set_primary_group(conn_id, group_id)
        .map_err(|e| e.to_string())
}

/// 清除主组标记（回退到按分组排序推导）。
pub fn clear_primary_group(project_root: Option<&Path>, conn_id: &str) -> Result<(), String> {
    let store = open_org_project(project_root)?;
    store
        .clear_primary_group(conn_id)
        .map_err(|e| e.to_string())
}
