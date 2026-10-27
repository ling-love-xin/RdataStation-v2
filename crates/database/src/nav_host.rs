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

use crate::model::{ObjectRef, PropertyRequest};

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

/// B-1 对象名 mask 规则（glob / 正则双档，由 `use_regex` 切换）。
///
/// 与 `settings::model::ObjectMaskRule` 字段一一对应——不直接复用那个类型，
/// 是为了不引入 `database → settings` 依赖；宿主实现负责两侧互转。
///
/// 匹配语义：include 为空 = 全部候选进入候选集；exclude 为空 = 不排除；**exclude 优先于
/// include**。`use_regex=false` 走 glob（`*` 任意多字符 / `?` 单字符，大小写不敏感）；
/// `use_regex=true` 走正则（`(?i)` 大小写不敏感，编译失败条目视为永不命中）。匹配实现见
/// `nav_view::primitives::{compile_mask, mask_passes}`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjectMaskRule {
    /// include 模式列表（空 = 全量候选；glob / 正则由 `use_regex` 决定）。
    pub include: Vec<String>,
    /// exclude 模式列表（空 = 不排除；glob / 正则由 `use_regex` 决定）。
    pub exclude: Vec<String>,
    /// `true` = 正则匹配；`false` = glob 匹配。
    pub use_regex: bool,
}

impl ObjectMaskRule {
    /// 规则是否「启用」：include 或 exclude 任一非空（空规则 = 该层不过滤）。
    pub fn is_active(&self) -> bool {
        !self.include.is_empty() || !self.exclude.is_empty()
    }
}

/// 一个已保存视图的展示信息（宿主从 settings 转换而来；视图侧只渲染，不读 settings）。
#[derive(Debug, Clone)]
pub struct SavedViewMeta {
    /// 视图名。
    pub name: String,
    /// 筛选摘要（如 `source:project · type:mysql`；全空 = `全部`）。
    pub summary: String,
    /// 筛选快照（「应用视图」时直接落 chips）。
    pub filters: NavFilters,
}

/// 全局通知类型（映射到 gpui-kit `NotificationType`；宿主实现负责转换）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostNotify {
    /// 信息提示（中性）。
    Info,
    /// 操作成功（绿色）。
    Success,
    /// 警告（黄色）。
    Warning,
    /// 错误（红色）。
    Error,
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

    /// 全局浮层通知（连接成功 / 失败等事件路径调用）。默认空实现：宿主未接入
    /// 通知系统时静默丢弃，不影响行为。宿主实现见 `workbench::components::nav_host`
    /// （内部经 `workbench::notification` 弹全局通知）。
    fn notify(&self, _kind: HostNotify, _title: String, _message: Option<String>, _cx: &mut App) {}

    /// 让宿主重绘。模态层（连接对话框等）挂在宿主 `render` 上，只 `cx.notify` 子视图不够。
    fn notify_host(&self, cx: &mut App);

    // ==================== 视图偏好（宿主自持 settings） ====================

    /// 行下是否显示标签 chip。
    fn show_tags(&self, cx: &App) -> bool;
    fn set_show_tags(&self, value: bool, cx: &mut App);

    /// 行内是否显示归属域（`P` / `G` / `GP`）。
    fn show_scope(&self, cx: &App) -> bool;
    fn set_show_scope(&self, value: bool, cx: &mut App);

    /// 面板是否显示来源筛选 chips 行（全部/项目/全局/共享）。
    fn show_chips(&self, cx: &App) -> bool;
    fn set_show_chips(&self, value: bool, cx: &mut App);

    /// 面板是否显示搜索行（输入框 + 模式切换）。
    fn show_search(&self, cx: &App) -> bool;
    fn set_show_search(&self, value: bool, cx: &mut App);

    /// 表 / 视图行右侧统计徽标整体开关（需求④，默认开）：关闭后右侧 meta /
    /// 注释副标 / 分区·继承 / ≈N / 大小徽标全部不显。`row_estimate` 是更内层
    /// 的独立开关，此处是更上层总闸。
    fn show_inline_stats(&self, cx: &App) -> bool;
    fn set_show_inline_stats(&self, value: bool, cx: &mut App);

    /// 表行「注释副标」子开关（需求②）：总开关开时仍受此项门控。
    fn show_inline_comment(&self, cx: &App) -> bool;
    fn set_show_inline_comment(&self, value: bool, cx: &mut App);

    /// 表行「大小徽标」子开关（需求②）。
    fn show_inline_size(&self, cx: &App) -> bool;
    fn set_show_inline_size(&self, value: bool, cx: &mut App);

    /// 表行「行数（≈N）」子开关设置（需求②）：关时不排估计任务、行尾不显「≈ N」。
    /// 读取走 [`Self::row_estimate`]。
    fn set_row_estimate(&self, value: bool, cx: &mut App);

    /// 归属域用短码（`P`）还是文字（`项目`）。
    fn source_short_code(&self, cx: &App) -> bool;
    fn set_source_short_code(&self, value: bool, cx: &mut App);

    /// 连接名称前是否以 `[简称]` 显示类型简称（如 `[MY] 192.168.3.138`）。
    fn show_name_type_short(&self, cx: &App) -> bool;

    /// 数据库类型 → 简称（`type_id` → 简称；来源：设置「类型简称映射」表，
    /// 未登记回退类型名首 2 字母大写）。
    fn type_short(&self, type_id: &str, cx: &App) -> String;

    /// 当前筛选条件（面板头 facet）。
    fn nav_filters(&self, cx: &App) -> NavFilters;
    fn set_nav_filters(&self, filters: NavFilters, cx: &mut App);

    // ==================== 已保存视图（宿主自持 settings） ====================

    /// 全部已保存视图（按保存顺序，含筛选摘要与快照）。渲染「已保存视图 ▸」子菜单。
    fn saved_views(&self, cx: &App) -> Vec<SavedViewMeta>;

    /// 保存一个命名视图（同名拦截：已存在 → `Err`，由调用方提示改名）。
    fn save_saved_view(&self, name: String, filters: NavFilters, cx: &mut App) -> Result<(), String>;

    /// 删除一个命名视图（不存在 → `Err`）。
    fn delete_saved_view(&self, name: &str, cx: &mut App) -> Result<(), String>;

    /// 打开「将当前筛选存为视图」对话框（预填当前筛选预览；保存动作经 `save_saved_view`）。
    fn open_saved_view_dialog(&self, filters: NavFilters, window: &mut Window, cx: &mut App);

    /// 打开「管理视图」对话框（列表 + 删除；删除动作经 `delete_saved_view`）。
    fn open_manage_views_dialog(&self, window: &mut Window, cx: &mut App);

    /// 某连接的可见库白名单（S3-①）：`None` = 默认模式（树隐藏系统库 / 系统 schema）；
    /// `Some(集合)` = 白名单模式（原样展示集合里的库，含系统对象）。
    /// 集合可同时含 catalog 名与 schema 名（层级不同但同表存储，按名匹配各自层）。
    fn visible_databases(&self, conn_id: &str, cx: &App) -> Option<std::collections::BTreeSet<String>>;

    /// 保存可见库白名单（`None` = 回到默认模式）。保存后调用方负责刷新树。
    fn set_visible_databases(
        &self,
        conn_id: &str,
        visible: Option<Vec<String>>,
        cx: &mut App,
    );

    /// S3-④ 行估计徽标开关（默认开）：关闭后不排估计任务、行尾不显示「≈ N」。
    fn row_estimate(&self, cx: &App) -> bool;

    /// P1 连接级导航偏好（2026-10-25）：`show_system_objects` 开启时，默认模式
    /// （`visible_databases() == None`）下导航树也显示系统 catalog / schema。
    /// 白名单模式不受此开关影响（白名单集合里含系统对象时原样展示）。
    /// 读取走连接档案 `advanced_options.navigator`（与 settings 的可见库白名单两通道并存）。
    fn show_system_objects(&self, conn_id: &str, cx: &App) -> bool;

    /// B-1 某连接的对象名 mask（键不存在 = 该连接不过滤，返回默认空规则）。
    fn object_mask(&self, conn_id: &str, cx: &App) -> ObjectMaskRule;

    /// B-1 保存某连接的对象名 mask（`None` = 删除该连接规则，回到不过滤）。
    /// 保存后调用方负责刷新树 / 搜索命中区。
    fn set_object_mask(&self, conn_id: &str, rule: Option<ObjectMaskRule>, cx: &mut App);

    /// B-1 面板全局对象名 mask（空规则 = 不启用）。
    fn global_mask(&self, cx: &App) -> ObjectMaskRule;

    /// B-1 保存面板全局对象名 mask。保存后调用方负责刷新树 / 搜索命中区。
    fn set_global_mask(&self, rule: ObjectMaskRule, cx: &mut App);

    /// B-1 打开「对象名过滤器」对话框（`conn_id = Some` 连接级 / `None` 全局级；
    /// 宿主实现负责查连接名并刷新导航面板）。
    fn open_object_mask_dialog(&self, conn_id: Option<&str>, window: &mut Window, cx: &mut App);

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

    // ==================== 最近连接（排序数据源） ====================

    /// 最近使用过的连接 ID（新→旧；来自最近连接记录，可能不含从未保存过的连接）。
    fn recent_connection_ids(&self) -> Vec<String>;

    /// 记录一次连接**成功**（刷新「最近使用」时间）。静默失败。
    fn note_connection_used(&self, conn_id: &str);

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

    /// 打开属性面板并加载数据（双击对象 / `F4`）。需要窗口：属性面板是中央 Dock tab，首次打开要加进中央。
    fn show_properties(&self, request: PropertyRequest, window: &mut Window, cx: &mut App);

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

    /// 打开「管理分组」覆盖层对话框（批量重命名 / 合并分组 / 删除；标签部分归 tag crate 会话）。
    fn open_group_manager(&self, window: &mut Window, cx: &mut App);

    /// 打开「管理标签」覆盖层对话框（批量重命名 / 清理空标签；标签定义跨模块共享）。
    fn open_tag_manager(&self, window: &mut Window, cx: &mut App);

    /// 打开「打标签」对话框（勾选已有标签 + 输入新标签名）。
    fn open_tag_assign_dialog(&self, conn_id: &str, window: &mut Window, cx: &mut App);

    /// 读取连接当前标签（逗号分隔字符串列表）。
    fn list_tags_for(&self, conn_id: &str) -> Vec<String>;

    /// 覆盖式设置连接标签（打标签对话框确认后调用）。
    fn assign_tags(&self, conn_id: &str, tags: &[String], cx: &mut App);

    /// 全部标签定义名（global + project 两域合并去重）。
    fn all_tag_names(&self) -> Vec<String>;

    /// 展开右 Dock 并切到指定面板（右键「查看洞察」等）。
    fn open_right_panel(&self, panel: RightPanel, cx: &mut App);

    /// 打开 Mock 面板；`source` 给定时按**源库表**定向（读源库结构 + 预填目标表名）。
    ///
    /// 期望 `kind` = `Table` / `View`（宿主只按限定名取结构，不区分类别）。
    fn open_mock_panel(&self, source: Option<ObjectRef>, cx: &mut App);

    /// 【M8】在洞察面板里看一张源表的统计（导航右键「查看统计」）。
    ///
    /// 取样 SQL 由**宿主**构造（只有它知道该驱动方言下的限定名写法），
    /// 洞察侧负责取样成临时表并出画像（架构 D58）。
    fn open_insight_table(&self, source: ObjectRef, cx: &mut App);

    /// 打开「数据库可见性」对话框（S3-①b）：勾选该连接树显示的库 / schema。
    ///
    /// 宿主负责拿 `NavView` 实体（确定后重载该连接）与项目根（元数据缓存定位）。
    fn open_visible_databases_dialog(&self, conn_id: &str, window: &mut Window, cx: &mut App);

    /// 【M8】在洞察面板里看这个 Schema 的**结构**（导航右键「结构洞察」）。
    ///
    /// 期望 `kind` = `Schema`。与上一条的区别：那个看**数据**（取样 → 表探查），
    /// 这个看**结构**（源库内省 `information_schema` → 外键候选 / 类型不一致 /
    /// 孤立表 / 冗余列）——两套完全不同的取数，不共用通道。
    fn open_insight_schema(&self, schema: ObjectRef, cx: &mut App);
}
