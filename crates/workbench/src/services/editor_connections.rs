//! 编辑器连接端口的工作台实现（B1）：把 M3/M4 的连接列表与自动建连接进编辑器
//!
//! ## 职责边界
//!
//! 编辑器只知道“有哪些连接可选、把某个连接连起来”这两件事（`editor::connection::ConnectionsPort`）；
//! 连接从哪个目录加载、怎么建连、失败原因怎么表述，都是工作台的事——这里复用既有路径：
//! - 列表：`Shared::connections`（导航面板维护的**内存快照**，渲染路径可安全读）
//! - 建连：`services::nav_runtime::{is_connected, connect_entry}`（与导航展开时的隐式建连同一条）
//!
//! ## 为什么列表要读 `Shared` 而不是再查一次库
//!
//! 端口方法会被**渲染路径**调用（工具栏每帧画选择器）。查库 / 读文件都是 I/O，绝不能进 render；
//! 而 `Shared::connections` 本来就是界面上看到的同一份列表——不存在第二份真值。
//!
//! ## 短码从哪来
//!
//! `P` / `G` / `GP`（项目 / 全局 / 共享快照）直接按 id 前缀判：`engine::persistence::id_prefix`
//! 是这套前缀的权威（`is_global` 只认 `G_`，`GP_` 得先判，因为后者也以 `G` 开头）。

use std::rc::Rc;

use editor::connection::{ConnectionOption, ConnectionsPort};
use editor::shared::EditorShared;

use crate::panels::Shared;

/// 工作台实现：连接列表（内存快照）+ 自动建连
struct WorkbenchConnections {
    shared: Shared,
}

impl ConnectionsPort for WorkbenchConnections {
    fn options(&self) -> Vec<ConnectionOption> {
        self.shared
            .connections
            .borrow()
            .iter()
            .map(|c| ConnectionOption {
                id: c.id.clone(),
                short: short_code(&c.id).to_string(),
                name: c.name.clone(),
                // 运行态只用**记录里的缓存值**：这里可能在 render 路径上被调，
                // 而查连接管理器（`nav_runtime::is_connected`）是 `block_on`，不能进帧。
                // 建连成功后由下面的 `ensure_connected` 把这条记录改成已连接。
                connected: c.connected,
                // 【B10】驱动类型：编辑器据此选格式化 / 执行计划 / 转译的方言
                db_type: c.driver.clone(),
            })
            .collect()
    }

    fn ensure_connected(&self, conn_id: &str) -> Result<(), String> {
        if crate::services::nav_runtime::is_connected(conn_id) {
            return Ok(());
        }
        let root = self
            .shared
            .project_root()
            .map(|path| path.to_string_lossy().to_string());
        crate::services::nav_runtime::connect_entry(conn_id, root.as_deref())?;

        // 快照跟着真状态走：否则状态栏会刚刚“已绑定”却显示空心的运行态点
        let mut connections = self.shared.connections.borrow_mut();
        if let Some(item) = connections.iter_mut().find(|c| c.id == conn_id) {
            item.connected = true;
        }
        Ok(())
    }
}

/// id 前缀 → 作用域短码（口径同 `engine::persistence::id_prefix`）
///
/// `GP_` 必须先判：`is_global` 只看首字符，会把快照也当成全局。
fn short_code(conn_id: &str) -> &'static str {
    if conn_id.starts_with("GP_") {
        "GP"
    } else if engine::persistence::id_prefix::is_global(conn_id) {
        "G"
    } else {
        "P"
    }
}

/// 把连接端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &EditorShared, workbench: &Shared) {
    shared.attach_connections(Rc::new(WorkbenchConnections {
        shared: workbench.clone(),
    }));
}
