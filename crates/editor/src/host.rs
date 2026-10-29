//! 编辑器宿主端口：editor crate 通过本模块访问宿主（workbench）能力。
//!
//! ## 为什么需要这些 trait
//!
//! `services/editor_*`（原 workbench 侧的执行 / 连接 / 补全 / schema 等端口实现，
//! 见 `shared-ownership-plan.md` §5）下沉到本 crate 后，不能再依赖
//! `workbench::panels::Shared` 的具体类型（分层硬约束：workbench → editor）。
//! 参照 `database::nav_host::NavHost` 与 `mock::mock_view::MockHost` 两个正例：
//! 端口实现只拿一个**本 crate 定义**的 trait，crate 之间保持「单向、无环」。
//!
//! ## 为什么拆成两个 trait
//!
//! - [`EditorHost`]：**UI 线程**端口。实现持 `Shared`（`Rc<RefCell>` 家族，不是 Send），
//!   供补全 / schema / 连接 / 历史 / 洞察等渲染路径或事件路径使用。
//! - [`FederatedSourcePort`]：**工作线程**端口。执行器（`editor_exec`）的联邦查询
//!   在 `QueryRunner`（`Send + Sync`）的工作线程上跑，端口必须可跨线程；
//!   实现**不持 Shared**，只调全局 `DataSourceService`（原 workbench 代码就是
//!   在工作线程 `block_on(service.list())`，线程边界不变）。
//!
//! 实现方是 workbench（`crates/workbench/src/components/editor_host.rs`），
//! 装配期把 `Rc<dyn EditorHost>` 与 `Arc<dyn FederatedSourcePort>` 分别注入各
//! `attach_*` 端口。

use std::path::PathBuf;

use gpui_kit::App;
use workbench_shell::model::ConnectionItem;

use crate::shared::InsightColumnRequest;

/// 联邦查询候选（`editor_exec` 的 `federated_plan` 用）。
///
/// 连接串（`url`）由**宿主**组装（`connection::url::build_connection_url` 解密口令）：
/// editor 不依赖 connection crate，也不知道口令存在哪。
#[derive(Debug, Clone)]
pub struct FederatedSourceRecord {
    /// 连接 id（`G_` / `P_` / `GP_`）。
    pub conn_id: String,
    /// 展示名。
    pub name: String,
    /// 数据库类型（`db_type`）。
    pub db_type: String,
    /// 已解密的连接串（DuckDB 联邦直连用）。
    pub url: String,
}

/// 编辑器宿主能力（`workbench` 实现并注入；**UI 线程专用**）。
///
/// 取值口径与 `NavHost` 一致：**渲染期不做 I/O**。`connections` / `project_root` /
/// `project_read_only` 是内存快照，渲染路径可安全读；会落盘 / 连库的方法
/// （`connect_entry`）只允许在事件路径调用。
pub trait EditorHost: 'static {
    // ==================== 宿主状态（读，渲染路径安全） ====================

    /// 当前作用域可见的连接（全局 + 项目，内存快照；渲染路径可读，不做 I/O）。
    fn connections(&self) -> Vec<ConnectionItem>;

    /// 当前项目根（未打开项目为 `None`）。
    fn project_root(&self) -> Option<PathBuf>;

    /// 项目只读锁（渲染路径每帧读；与草稿箱 / 资产库 / 连接对话框的写保护同一判据）。
    fn project_read_only(&self) -> bool;

    // ==================== 宿主状态（写，事件路径） ====================

    /// 更新一条连接快照的 `connected` 标记（建连成功 / 断开后同步，
    /// 避免状态栏刚刚“已绑定”却显示空心的运行态点）。
    fn set_connection_connected(&self, conn_id: &str, connected: bool);

    /// 运行时是否已连接（事件路径；与导航展开时隐式建连同一条路）。
    fn is_connected(&self, conn_id: &str) -> bool;

    /// 建立运行时连接（事件路径；URL 组装在宿主侧）。
    fn connect_entry(&self, conn_id: &str) -> Result<(), String>;

    // ==================== 宿主命令（需窗口 / 需别的面板） ====================

    /// 打开历史面板（工具栏「⟲ 历史」→ 右 Dock）。
    fn open_history(&self, cx: &mut App);

    /// 结果集列头右键「洞察此列」→ 右 Dock 洞察面板。
    ///
    /// 编辑器只说「这一列 + 产生它的 SQL + 在哪条连接上」（`InsightColumnRequest`），
    /// 翻译成 `insight::SampleSource` 是宿主的事（editor 不依赖 insight crate）。
    fn open_insight_source_column(&self, request: InsightColumnRequest, cx: &mut App);

    /// 导航树定位对象（`rds://navigator/<限定表名>` 跳转定义；复用 Quick Open `⌥↵`
    /// 同款 `request_reveal` 机制，宿主侧只入队，导航面板下一帧消费）。
    fn request_reveal(&self, object: engine::ObjectRef);
}

/// 联邦源清单端口（**工作线程专用**：`editor_exec` 的 `QueryRunner` 是
/// `Send + Sync`，在它自己的 tokio runtime 工作线程上跑联邦查询）。
///
/// 实现**不持 `Shared`**（`Rc` 不能跨线程），只调全局 `DataSourceService`——
/// 原 workbench 代码就是执行线程 `block_on(service.list())`，线程边界不变。
pub trait FederatedSourcePort: Send + Sync + 'static {
    /// 当前开了「DuckDB 本地加速（联邦查询直连源库）」的连接清单（宿主组装连接串）。
    fn records(&self) -> Vec<FederatedSourceRecord>;
}
