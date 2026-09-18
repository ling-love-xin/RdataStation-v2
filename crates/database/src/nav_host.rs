//! 数据库导航视图的**宿主端口**（`workbench` 实现并注入）。
//!
//! ## 为什么需要这个 trait
//!
//! 导航视图（`nav_view`）从 `workbench` 下沉到本 crate 后，不能再看见 `Shared`、
//! `crate::components::*`、`crate::services::*`——那些是宿主的东西。参照本仓已有的两个
//! 正例（`mock::mock_view::MockHost`、`insight` 的取数端口），视图只拿一个**本 crate 定义**的
//! trait：crate 之间保持「单向、无环」。
//!
//! ## 边界怎么划
//!
//! 端口只收**真正需要宿主**的能力，判据是「换个宿主还成立吗」：
//!
//! | 能力 | 去向 | 理由 |
//! | --- | --- | --- |
//! | 标签 / 分组 / 排序 / 导航状态的落库 | **不进端口**，走 [`crate::nav_store`] | 只依赖 `engine` + 项目根 |
//! | 驱动目录读取 | **不进端口**，走 `engine::persistence::driver_catalog` | 引擎侧元数据 |
//! | 对象树元数据读取 | **不进端口**，走 [`crate::navigator_service`] | 只依赖引擎连接管理器 |
//! | 连接清单、选中态、项目根、状态栏提示 | 端口（[`NavHost::connections`] 等） | 宿主自持的全局状态 |
//! | 视图偏好（标签 / 归属域显隐、短码、筛选） | 端口读取器 | 存在宿主 `settings`，视图不该依赖 `settings` |
//! | 连接生命周期与连接增删改 / 共享 | 端口 | 现由 `workbench::services::*` 提供（`block_on` 桥接） |
//! | 开对话框 / 开属性面板 / 开右 Dock 面板 | 端口 | 需要 `Window` 或别的面板，只有宿主有 |
//!
//! ## 视图侧的取值口径
//!
//! 与 `MockHost` 一致：**渲染期不做 I/O**。端口里凡是会落盘 / 连库的方法
//! （`connect` / `test_connection` / `reload_connections` …）都只允许在事件路径调用；
//! 渲染只读 [`NavHost::connections`] 这类内存快照。
//!
//! 注入方式：`NavView::new(host: Rc<dyn NavHost>, cx)`。宿主实现见
//! `crates/workbench/src/panels/nav_host.rs`。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{App, Window};
use workbench_shell::model::{ConnectionItem, GroupFormSeed, QueryRequest, RightPanel};

use crate::model::{PropertyRequest, SchemaRef, TableRef};

/// 独立会话探测入口（连接 id + 项目根 → 可展示结果文案）。
///
/// 刻意做成**函数指针**而不是 `&self` 方法：它要随任务被送到视图的后台工作线程，
/// 而宿主句柄 `Rc<dyn NavHost>` 不能跨线程。实现必须是无状态函数。
pub type ConnectionProbe = fn(conn_id: &str, project_root: Option<&str>) -> Result<String, String>;

/// 导航筛选条件（来源域 / 数据库类型 / 驱动 / 标签）。
///
/// 与 `settings::model::NavigatorFilters` 字段一一对应——不直接复用那个类型，
/// 是为了不引入 `database → settings` 依赖；宿主实现负责两侧互转。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavFilters {
    /// 归属域筛选（`project` / `global` / `shared`；`None` = 全部）。
    pub source: Option<String>,
    /// 数据库类型筛选（`drivers.type_id`）。
    pub db_type: Option<String>,
    /// 驱动 id 筛选。
    pub driver: Option<String>,
    /// 标签筛选。
    pub tag: Option<String>,
}

/// 宿主注入的能力（`workbench` 实现，见 `crates/workbench/src/panels/nav_host.rs`）。
pub trait NavHost: 'static {
    // ==================== 宿主状态（读） ====================

    /// 当前作用域可见的连接（全局 + 项目，已按持久化层过滤）。
    fn connections(&self) -> Vec<ConnectionItem>;

    /// 当前选中连接的**下标**（`None` = 未选中）。
    fn selected_index(&self) -> Option<usize>;

    /// 当前项目根（未打开项目为 `None`）。分组 / 项目级连接 / 项目级导航状态都用它定位。
    fn project_root(&self) -> Option<PathBuf>;

    // ==================== 宿主状态（写） ====================

    /// 设置选中连接。实现须一并处理「选中切换 = 导航缓存失效 + 宿主重绘」的连带动作
    /// （原先由宿主订阅 `SidebarEvent::SelectConnection` 完成）。
    fn select_connection(&self, index: Option<usize>, cx: &mut App);

    /// 状态栏提示（一行文本；由宿主决定展示与清除时机）。
    fn notice(&self, message: String, cx: &mut App);

    /// 让宿主重绘。模态层（连接对话框等）挂在宿主 `render` 上，只 `cx.notify` 子视图不够。
    fn notify_host(&self, cx: &mut App);

    // ==================== 视图偏好（宿主自持 settings） ====================

    /// 行下是否显示标签 chip。
    fn show_tags(&self, cx: &App) -> bool;
    fn set_show_tags(&self, value: bool, cx: &mut App);

    /// 行内是否显示归属域（`P` / `G` / `GP`）。
    fn show_scope(&self, cx: &App) -> bool;
    fn set_show_scope(&self, value: bool, cx: &mut App);

    /// 归属域用短码（`P`）还是文字（`项目`）。
    fn source_short_code(&self, cx: &App) -> bool;

    /// 当前筛选条件（面板头 facet）。
    fn nav_filters(&self, cx: &App) -> NavFilters;
    fn set_nav_filters(&self, filters: NavFilters, cx: &mut App);

    // ==================== 连接生命周期 ====================

    /// 运行时是否已连接。
    fn is_connected(&self, conn_id: &str) -> bool;

    /// 建立运行时连接（URL 组装 + 网络档案解析在宿主侧）。
    fn connect(&self, conn_id: &str) -> Result<(), String>;

    /// 关闭运行时连接。**保留**元数据缓存与状态（缓存只在「缓存管理」中清理）。
    fn disconnect(&self, conn_id: &str) -> Result<(), String>;

    /// 独立会话探测的**可跨线程**入口（视图把它交给后台任务线程执行）。
    ///
    /// 为什么是函数指针而不是 `&self` 方法：探测在视图的后台工作线程上跑
    /// （`database::nav_jobs`），而 `Rc<dyn NavHost>` 不能跨线程。实现须返回一个
    /// **无状态**函数（内部自取服务单例与进程级桥接运行时），因而天然 `Send`。
    fn connection_probe(&self) -> ConnectionProbe;

    // ==================== 连接增删改 / 共享 ====================

    /// 重载当前作用域可见连接并回填 [`NavHost::connections`]；
    /// 连接数变化时一并修正选中下标（空表 → 取消选中，越界 → 回到首项）。
    fn reload_connections(&self, cx: &mut App);

    /// 按模板复制连接（新名称；不含密码）。项目根由宿主自持。
    fn copy_connection(&self, from_id: &str, new_name: &str) -> Result<(), String>;

    /// 把全局连接共享进当前项目（`GP_` 快照）；未打开项目 → `Err`。
    fn share_connection(&self, conn_id: &str) -> Result<(), String>;

    /// 删除连接记录；项目侧 `GP_` 走同一条路（删掉即「取消共享」，全局定义保留）。
    ///
    /// 返回服务层的可展示文案。调用方负责随后 [`NavHost::disconnect`]（缓存保留）。
    fn delete_connection(&self, conn_id: &str) -> Result<String, String>;

    // ==================== 宿主命令（需窗口 / 需别的面板） ====================

    /// 打开属性面板并加载数据（双击对象 / `F4`）。
    fn show_properties(&self, request: PropertyRequest, cx: &mut App);

    /// 把一份查询送到中央编辑区（「在 SQL 编辑器中打开」/「查看数据」/「生成 SQL」）。
    fn open_query(&self, request: QueryRequest, cx: &mut App);

    /// 打开「编辑数据源连接」对话框。
    fn edit_connection(&self, conn_id: &str, window: &mut Window, cx: &mut App);

    /// 打开「新建数据源连接」对话框。
    fn new_connection(&self, window: &mut Window, cx: &mut App);

    /// 打开分组表单对话框。`on_submit(分组 ID, 名称, 描述, app)` 只在名称非空时回调一次；
    /// 落库由**视图**用 [`crate::nav_store`] 完成（对话框与存储互不认识）。
    fn open_group_form(
        &self,
        seed: GroupFormSeed,
        window: &mut Window,
        cx: &mut App,
        on_submit: Rc<dyn Fn(Option<String>, String, Option<String>, &mut App)>,
    );

    /// 打开「缓存管理」对话框。
    fn open_cache_dialog(&self, window: &mut Window, cx: &mut App);

    /// 展开右 Dock 并切到指定面板（右键「查看洞察」等）。
    fn open_right_panel(&self, panel: RightPanel, cx: &mut App);

    /// 打开 Mock 面板；`source` 给定时按**源库表**定向（读源库结构 + 预填目标表名）。
    fn open_mock_panel(&self, source: Option<TableRef>, cx: &mut App);

    /// 【M8】在洞察面板里看一张源表的统计（导航右键「查看统计」）。
    ///
    /// 取样 SQL 由**宿主**构造（只有它知道该驱动方言下的限定名写法），
    /// 洞察侧负责取样成临时表并出画像（架构 D58）。
    fn open_insight_table(&self, source: TableRef, cx: &mut App);

    /// 【M8】在洞察面板里看这个 Schema 的**结构**（导航右键「结构洞察」）。
    ///
    /// 与 [`Self::open_insight_table`] 分工：那个看**数据**（取样 → 表探查），
    /// 这个看**结构**（源库内省 `information_schema` → 外键候选 / 类型不一致 /
    /// 孤立表 / 冗余列）——两套完全不同的取数，不共用通道。
    fn open_insight_schema(&self, schema: SchemaRef, cx: &mut App);
}
