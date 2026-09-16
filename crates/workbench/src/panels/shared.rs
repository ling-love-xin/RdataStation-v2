//! 面板与工作台之间共享的状态（`Shared`）与宿主动作请求。
//!
//! 自 `panels.rs` 纯位移迁出（无语义改动）。单独成模块的理由：`Shared` 是三方的
//! 共用状态——面板层（`SidebarPanel` / `EditorPanel` / `RightSidebarPanel`）、
//! 组件层（`crate::components::*`）与宿主（`crate::view::WorkbenchView`）——
//! 放在面板实现文件里时，任何一处面板改动都会牵动组件层与宿主的编译与 review。
//!
//! 该模块不承载渲染逻辑；字段的写入 / 消费口径见各字段注释。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use insight::{InsightTarget, InsightView};
use mock::mock_view::{MockDetailView, MockPanel, SchemaRequest};
use scratchpad::ScratchpadStore;

use analytics_resource::resource_view::ResourcesPanel;

use gpui_kit::*;

use database::model::PropertyRequest;

use super::scratchpad_panel::ScratchpadSearchView;
use crate::view::{ConnectionItem, LeftPanel, RightPanel, SidebarMode};
// 纯数据类型已下沉到 shell（导航视图下沉后需与 `database` 共用同一份定义），此处重导保持旧路径。
pub use workbench_shell::model::QueryRequest;
/// 连接对话框「项目栏」动作项 → 宿主消费分支的动作请求（#9）。
///
/// 生产方是 `connection_dialog::handle_project_confirm`（置位两个标记之一），
/// 消费方是 `WorkbenchView::render`——`Shared::take_project_action_request` 把
/// “标记 → 动作”的决策与“只消费一次”的语义收在一处，可直接单测（无需窗口）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectActionRequest {
    /// 「＋ 新增项目」：走未保存拦截后直接打开新建项目对话框。
    CreateProject,
    /// 「打开现有目录…」：打开目录选择对话框。
    OpenFolder,
}

/// 编辑区对外命令端口（装配期由 `WorkbenchView::init_workspace` 注入）。
///
/// 导航 / 草稿箱不再直写 `Shared` 的请求字段，改调这里的方法：方法内部**立即**
/// 让编辑区完成动作（打开对话框等），不再等到编辑区渲染时 `take()`——
/// 副作用因此从 render 路径回到事件路径（`gpui-kit-dev` skill「状态与副作用」）。
#[derive(Clone)]
pub struct EditorBridge {
    /// 打开「编辑数据源连接」对话框。
    pub edit_connection: Rc<dyn Fn(String, &mut Window, &mut App)>,
    /// 打开「新建数据源连接」对话框。
    pub new_connection: Rc<dyn Fn(&mut Window, &mut App)>,
    /// 打开属性面板并加载数据（导航双击对象 / 键盘 F4）。
    pub show_properties: Rc<dyn Fn(PropertyRequest, &mut App)>,
    /// 投递草稿箱内容搜索结果（`None` = 清空）：结果在中央编辑区展示。
    pub show_search_results: Rc<dyn Fn(Option<ScratchpadSearchView>, &mut App)>,
}

/// 草稿箱对外命令端口（装配期由 `WorkbenchView::init_workspace` 注入）。
///
/// 编辑区的「全部替换」需要草稿箱的轮询印在跑：原先靠 `Shared` 布尔标记 +
/// 侧栏渲染时 `take`（副作用落在 render），现改为事件路径直接调端口。
#[derive(Clone)]
pub struct ScratchpadBridge {
    /// 确保草稿箱轮询在跑（结果回填由侧栏渲染消费）。
    pub ensure_pump: Rc<dyn Fn(&mut App)>,
}

/// 资产库刷新端口（M6，装配期由 `WorkbenchView::init_workspace` 注入）。
///
/// “入队 + 轮询”两件事都在侧栏面板手里（`SidebarPanel::request_resources_refresh`），
/// 而发起动作的地方可能在右栏「存档详情」或对话框回调里——那里只有 `Shared`。
/// 与 `ScratchpadBridge` 同例：面板之间不互订，端口只此一份。
#[derive(Clone)]
pub struct ResourcesBridge {
    /// 刷新资产库列表（入队 + 确保轮询印在跑）。
    pub refresh: Rc<dyn Fn(&mut App)>,
}

/// 把一段 SQL 追加到草稿末尾（空草稿直接落片段）。
///
/// 原先在 `nav.rs`（`nav_draft_append`），现在由导航菜单、拖拽与宿主打开查询共用，
/// 因此上提到 `Shared` 同层：**拼接规则只有一处**。
pub fn append_sql(current: &str, snippet: &str) -> String {
    if current.trim().is_empty() {
        snippet.to_string()
    } else {
        format!("{}\n{}", current.trim_end(), snippet)
    }
}

/// 面板与工作台共享的状态。
#[derive(Clone)]
pub struct Shared {
    pub active_left: Rc<Cell<LeftPanel>>,
    pub active_right: Rc<Cell<RightPanel>>,
    /// 左右边栏三模式（展开 / 收起 / 完全隐藏），互不影响。
    pub left_mode: Rc<Cell<SidebarMode>>,
    pub right_mode: Rc<Cell<SidebarMode>>,
    /// 完全隐藏前的模式快照：状态栏独立开关恢复时还原，避免丢失隐藏前状态。
    pub left_mode_before_hidden: Rc<Cell<SidebarMode>>,
    pub right_mode_before_hidden: Rc<Cell<SidebarMode>>,
    /// Quick Open 弹层开关（Ctrl+P / 标题栏搜索条）。
    pub quick_open: Rc<Cell<bool>>,
    /// 设置面板开关（活动栏底部 ⚙ / OpenSettings 命令）。
    pub settings_open: Rc<Cell<bool>>,
    pub selected: Rc<Cell<Option<usize>>>,
    pub connections: Rc<RefCell<Vec<ConnectionItem>>>,
    pub notice: Rc<RefCell<Option<String>>>,
    /// 导航元数据缓存失效戳：宿主 / 项目宿主 / Mock 宿主只递增（`invalidate_nav_cache`），
    /// 缓存数据本身由 `EditorPanel` 自持——外部不再直接读写别家的缓存。
    pub nav_cache_epoch: Rc<Cell<u64>>,
    /// 编辑区命令端口（`None` = 装配未完成，调用方需容忍空端口）。
    pub editor_bridge: Rc<RefCell<Option<EditorBridge>>>,
    /// 连接对话框的项目下拉选中「＋ 新增项目」→ 宿主打开项目新建入口（由 `WorkbenchView` 消费）。
    pub project_new_request: Rc<Cell<bool>>,
    /// 连接对话框的项目下拉选中「打开现有目录…」→ 宿主打开目录选择（由 `WorkbenchView` 消费）。
    pub project_open_request: Rc<Cell<bool>>,
    /// P0：当前项目会话（草稿箱根 / 项目作用域连接 / 标题栏项目名共用）。
    pub project: Rc<RefCell<Option<project::ui::OpenProject>>>,
    /// M5：草稿箱内容搜索结果（侧栏发起，结果落中央编辑区）。
    /// 草稿箱命令端口（`None` = 装配未完成，调用方需容忍空端口）。
    pub scratchpad_bridge: Rc<RefCell<Option<ScratchpadBridge>>>,
    /// M5：请求在中央编辑器中打开某个文件（草稿箱双击 / Enter 置位，宿主 render 消费）。
    ///
    /// 只传**绝对路径**：编辑器无根，按路径自己判定模式 / 只读等级（Phase C 契约）。
    /// 字段已收为私有：外部只能走 `request_open_in_editor` / `take_open_in_editor`。
    open_file_request: Rc<RefCell<Option<std::path::PathBuf>>>,
    /// B11：请求在中央编辑器里打开一条查询（导航「在 SQL 编辑器中打开 / 查看数据 / 生成 SQL」
    /// 与拖拽都入这里，宿主 render 消费）。字段同样私有，走 `request_query` / `take_query_request`。
    query_request: Rc<RefCell<Option<QueryRequest>>>,
    /// M1 项目管理 UI 状态（选择器 / 菜单 / 对话框 / 设置 / 项目锁）。
    pub project_ui: Rc<RefCell<project::ui::ProjectUiState>>,
    /// M7：Mock 面板实体句柄（弱引用；用于导航右键定向导入源库结构）。
    ///
    /// 面板自带状态与对话框（`mock::mock_view::MockPanel`），工作台只持句柄。
    pub mock_panel: Rc<RefCell<Option<WeakEntity<MockPanel>>>>,
    /// M7：中央「Mock 数据」详情 tab 实体句柄（弱引用；已关闭则重新创建）。
    pub mock_detail: Rc<RefCell<Option<WeakEntity<MockDetailView>>>>,
    /// M8：洞察面板实体句柄（弱引用；右键入口与 Quick Open 用）。
    ///
    /// 面板自带状态与视图（`insight::InsightView`），工作台只持句柄 + 订阅它的取数请求。
    pub insight_panel: Rc<RefCell<Option<WeakEntity<InsightView>>>>,
    /// M6：资产库面板实体句柄（弱引用；右侧「存档详情」取它的选中项）。
    ///
    /// 与 `mock_panel` / `insight_panel` 同例：面板自带状态与视图
    /// （`analytics_resource::resource_view::ResourcesPanel`），宿主只持句柄。
    pub resources_panel: Rc<RefCell<Option<WeakEntity<ResourcesPanel>>>>,
    /// M6：资产库刷新端口（动作完成后由任意宿主侧位置触发刷新）。
    pub resources_bridge: Rc<RefCell<Option<ResourcesBridge>>>,
    /// M7：打开 Mock 详情 tab 的宿主命令（面板「查看详情」调用；需要窗口，照 `editor_clear` 口径）。
    pub open_mock_detail: Rc<RefCell<Option<Rc<dyn Fn(&mut Window, &mut App)>>>>,
    /// 驱动 id → 类型 / 显示名（徽标、hover 卡与属性面板共用；随组织数据一次性加载）。
    pub driver_catalog: Rc<RefCell<HashMap<String, engine::persistence::DriverMeta>>>,
    /// 宿主重绘桥：连接对话框层挂在 `WorkbenchView::render` 上，而 `Root` 的
    /// notify 不会让子视图重建元素树；打开 / 关闭对话框后必须显式通知宿主重渲染。
    pub host_redraw: Rc<RefCell<Option<Rc<dyn Fn(&mut App)>>>>,
}

impl Shared {
    /// 用真实连接数据初始化共享状态（连接为空时 selected=None，进入空态 UI）。
    pub fn with_connections(connections: Vec<ConnectionItem>, notice: Option<String>) -> Self {
        Self {
            active_left: Rc::new(Cell::new(LeftPanel::Database)),
            active_right: Rc::new(Cell::new(RightPanel::Insight)),
            left_mode: Rc::new(Cell::new(SidebarMode::Expanded)),
            right_mode: Rc::new(Cell::new(SidebarMode::Collapsed)),
            left_mode_before_hidden: Rc::new(Cell::new(SidebarMode::Expanded)),
            right_mode_before_hidden: Rc::new(Cell::new(SidebarMode::Expanded)),
            quick_open: Rc::new(Cell::new(false)),
            settings_open: Rc::new(Cell::new(false)),
            selected: Rc::new(Cell::new(if connections.is_empty() {
                None
            } else {
                Some(0)
            })),
            connections: Rc::new(RefCell::new(connections)),
            notice: Rc::new(RefCell::new(notice)),
            nav_cache_epoch: Rc::new(Cell::new(0)),
            project_new_request: Rc::new(Cell::new(false)),
            project_open_request: Rc::new(Cell::new(false)),
            project: Rc::new(RefCell::new(None)),
            scratchpad_bridge: Rc::new(RefCell::new(None)),
            open_file_request: Rc::new(RefCell::new(None)),
            query_request: Rc::new(RefCell::new(None)),
            project_ui: Rc::new(RefCell::new(Default::default())),
            editor_bridge: Rc::new(RefCell::new(None)),
            mock_panel: Rc::new(RefCell::new(None)),
            insight_panel: Rc::new(RefCell::new(None)),
            resources_panel: Rc::new(RefCell::new(None)),
            resources_bridge: Rc::new(RefCell::new(None)),
            mock_detail: Rc::new(RefCell::new(None)),
            open_mock_detail: Rc::new(RefCell::new(None)),
            driver_catalog: Rc::new(RefCell::new(HashMap::new())),
            host_redraw: Rc::new(RefCell::new(None)),
        }
    }

    pub fn new() -> Self {
        Self::with_connections(Vec::new(), None)
    }

    /// 当前项目根（未打开项目时为 `None`）。
    ///
    /// 草稿箱的全部后台任务都要带项目根入队；导航面板与编辑区同样从这里取。
    pub fn project_root(&self) -> Option<std::path::PathBuf> {
        self.project.borrow().as_ref().map(|p| p.root.clone())
    }

    /// 构建草稿箱存储 + 运行时（未打开项目时报错）。
    ///
    /// 侧栏（草稿箱面板）与中央编辑区（内容搜索结果 / 替换）共用。
    /// 仅用于**事件路径的元数据级操作**（新建/重命名/删除入回收站/引用增删改）；
    /// 搬运字节或遍历全树的操作走 `services::scratchpad_jobs`。
    pub fn scratchpad_store(&self) -> Result<(ScratchpadStore, tokio::runtime::Runtime), String> {
        let root = self
            .project
            .borrow()
            .as_ref()
            .map(|s| s.root.clone())
            .ok_or_else(|| "未打开项目".to_string())?;
        let rt = tokio::runtime::Runtime::new().map_err(|e| format!("运行时错误: {e}"))?;
        Ok((ScratchpadStore::new(root), rt))
    }

    /// 取出（并清空）「在编辑器中打开」请求：宿主 render 每帧调用一次。
    ///
    /// 与 `take_project_action_request` 同口径：取出即清空，同一次请求不会重复打开。
    pub fn take_open_in_editor(&self) -> Option<std::path::PathBuf> {
        self.open_file_request.borrow_mut().take()
    }

    /// 请求在中央编辑器中打开文件（草稿箱双击 / Enter / 右键「打开」）。
    ///
    /// 生产端（Enter / 右键路径）拿不到 `Window`，消费端（宿主打开文档）必须有 `Window`，
    /// 因此保留「请求 → 宿主 render 消费」的一帧延迟；字段私有，外部只能走这一对方法。
    pub fn request_open_in_editor(&self, path: std::path::PathBuf) {
        *self.open_file_request.borrow_mut() = Some(path);
    }

    /// 取出（并清空）项目栏的动作请求：宿主 render 每帧调用一次。
    ///
    /// - 两个标记同时置位 → 以「＋ 新增项目」优先（同一帧内的竞态，无语义歧义）；
    /// - 都未置位 → `None`（绝大多数帧，零开销）；
    /// - 取出即清空 → 同一次请求不会在后续帧重复开窗。
    pub fn take_project_action_request(&self) -> Option<ProjectActionRequest> {
        let new = self.project_new_request.replace(false);
        let open = self.project_open_request.replace(false);
        match (new, open) {
            (true, _) => Some(ProjectActionRequest::CreateProject),
            (false, true) => Some(ProjectActionRequest::OpenFolder),
            (false, false) => None,
        }
    }

    /// 导航元数据缓存失效（切换连接 / 项目 / 分析库被写入后调用）。
    ///
    /// 语义：**只声明“这些缓存不再可信”**，不关心数据在哪——缓存由 `EditorPanel`
    /// 自持并在下一次渲染时对比戳后丢弃重载。
    pub fn invalidate_nav_cache(&self) {
        self.nav_cache_epoch
            .set(self.nav_cache_epoch.get().wrapping_add(1));
    }

    /// 请求在中央编辑器里打开一条查询（导航「在 SQL 编辑器中打开 / 查看数据 / 生成 SQL」
    /// 与拖拽共用）。
    ///
    /// 生产端拿不到 `Window`（菜单回调 / 拖拽回调 / 后台回填），而开文档与写内核都要窗口，
    /// 因此这里只入队，由宿主 `render` 消费（与 `request_open_in_editor` 同一口径）。
    pub fn request_query(&self, request: QueryRequest) {
        *self.query_request.borrow_mut() = Some(request);
    }

    /// 取出（并清空）「打开查询」请求：宿主 render 每帧调用一次。
    pub fn take_query_request(&self) -> Option<QueryRequest> {
        self.query_request.borrow_mut().take()
    }

    /// 递「编辑连接」请求（走 `EditorBridge`；装配未完成时静默丢弃）。
    pub fn edit_connection(&self, id: String, window: &mut Window, cx: &mut App) {
        if let Some(bridge) = self.editor_bridge.borrow().clone() {
            (*bridge.edit_connection)(id, window, cx);
        }
    }

    /// 递「新建连接」请求（同上）。
    pub fn new_connection(&self, window: &mut Window, cx: &mut App) {
        if let Some(bridge) = self.editor_bridge.borrow().clone() {
            (*bridge.new_connection)(window, cx);
        }
    }

    /// 确保草稿箱轮询在跑（走 `ScratchpadBridge`；装配未完成时静默丢弃）。
    pub fn ensure_scratchpad_pump(&self, cx: &mut App) {
        if let Some(bridge) = self.scratchpad_bridge.borrow().clone() {
            (*bridge.ensure_pump)(cx);
        }
    }

    /// 请求一次资产库刷新（走 `ResourcesBridge`；装配未完成时静默丢弃）。
    ///
    /// “静默丢弃”是对的：端口未装配只出现在测试宿主里，而那里面板压根没挂。
    pub fn refresh_resources(&self, cx: &mut App) {
        if let Some(bridge) = self.resources_bridge.borrow().clone() {
            (*bridge.refresh)(cx);
        }
    }

    /// 投递草稿箱内容搜索结果（走 `EditorBridge`；装配未完成时静默丢弃）。
    ///
    /// 结果展示归编辑区（`EditorPanel::scratchpad_search`）；草稿箱不再回读这份展示数据。
    pub fn show_scratchpad_search(&self, view: Option<ScratchpadSearchView>, cx: &mut App) {
        if let Some(bridge) = self.editor_bridge.borrow().clone() {
            (*bridge.show_search_results)(view, cx);
        }
    }

    /// 打开属性面板并加载数据（同上）。
    pub fn show_properties(&self, request: PropertyRequest, cx: &mut App) {
        if let Some(bridge) = self.editor_bridge.borrow().clone() {
            (*bridge.show_properties)(request, cx);
        }
    }

    /// 当前选中连接（克隆，避免长时间持有 RefCell 借用）。
    pub fn selected_connection(&self) -> Option<ConnectionItem> {
        let conns = self.connections.borrow();
        self.selected.get().and_then(|i| conns.get(i)).cloned()
    }

    /// 通知宿主（`WorkbenchView`）重绘：模态层的挂载点在宿主 render 中，
    /// 仅靠 `Root` 的 notify 不会更新子视图元素树（`cx.notify` 只重渲染该视图子树）。
    pub fn notify_host(&self, cx: &mut App) {
        let bridge = self.host_redraw.borrow().clone();
        if let Some(redraw) = bridge {
            redraw(cx);
        }
    }

    /// 展开右 Dock 并切到指定面板。
    ///
    /// 与导航面板「查看洞察」同一口径（现走 `NavHost::open_right_panel`）：只改状态，布局同步由宿主 render
    /// （`apply_right_mode`）完成，因此还需要 `notify_host` 让宿主重渲染。
    pub fn open_right_panel(&self, panel: RightPanel, cx: &mut App) {
        self.active_right.set(panel);
        self.right_mode.set(SidebarMode::Expanded);
        self.notify_host(cx);
    }

    /// M8：打开洞察面板并**指向一列**（结果表列头右键「洞察此列」的宿主侧入口）。
    ///
    /// 入口只发这一条命令：面板状态与取数链都在 insight crate（`insight::jobs`），
    /// 这里只做「展开右 Dock + 递目标」——面板收到目标后会发 `ProfileRequested`，
    /// 由右栏面板构造期建立的订阅接手取数。
    pub fn open_insight_column(
        &self,
        temp_table: impl Into<String>,
        column: impl Into<String>,
        data_type: impl Into<String>,
        cx: &mut App,
    ) {
        self.open_right_panel(RightPanel::Insight, cx);
        let panel = self.insight_panel.borrow().clone();
        if let Some(panel) = panel.and_then(|weak| weak.upgrade()) {
            panel.update(cx, |panel, cx| {
                panel.set_target(
                    InsightTarget::Column {
                        temp_table: temp_table.into(),
                        column: column.into(),
                        data_type: data_type.into(),
                    },
                    cx,
                );
            });
        }
    }

    /// 打开 Mock 面板；`source` 给定时按**源库表**定向（导航右键「生成 Mock 数据」）。
    ///
    /// 定向动作（读源库结构 + 预填目标表名）在事件路径执行：面板实体随右栏面板
    /// **构造期创建**（`RightSidebarPanel::new`），因此这里总能拿到句柄。
    /// 顺带让面板重读生成历史：面板可能已摆了几个项目（也可能刚切过项目）。
    pub fn open_mock_panel(&self, source: Option<SchemaRequest>, cx: &mut App) {
        self.open_right_panel(RightPanel::Mock, cx);
        let panel = self.mock_panel.borrow().clone();
        let Some(panel) = panel.and_then(|weak| weak.upgrade()) else {
            return;
        };
        panel.update(cx, |panel, cx| panel.refresh_history(cx));
        if let Some(source) = source {
            panel.update(cx, |panel, cx| panel.preset_from_source(source, cx));
        }
    }
}

#[cfg(test)]
mod tests {
    // 注意：不通配导入（`use super::*` / `use gpui_kit::*` 会把 gpui 的 `test` 宏带入作用域）。
    use std::path::PathBuf;

    use super::{Shared, append_sql};

    /// 「打开查询」请求取出即清空（宿主 render 每帧取用，不得重复开文档）。
    #[test]
    fn query_request_is_consumed_once() {
        let shared = Shared::new();
        assert!(shared.take_query_request().is_none(), "初始无请求");

        let request = super::QueryRequest {
            conn_id: Some("P_orders".to_string()),
            sql: "SELECT * FROM mall.order LIMIT 200;".to_string(),
            run: true,
        };
        shared.request_query(request.clone());
        assert_eq!(shared.take_query_request(), Some(request), "首次取出得到请求");
        assert!(
            shared.take_query_request().is_none(),
            "取出即清空：同一请求不会重复打开"
        );
    }

    /// 草稿追加：空草稿直接落片段（不留前导换行），非空吃掉尾随空白后再追加。
    #[test]
    fn append_sql_keeps_existing_draft() {
        assert_eq!(append_sql("", "mall.order "), "mall.order ");
        assert_eq!(append_sql("   \n ", "mall.order "), "mall.order ");
        assert_eq!(
            append_sql("SELECT * FROM x\n\n", "mall.order "),
            "SELECT * FROM x\nmall.order "
        );
    }

    /// 「在编辑器中打开」请求取出即清空（宿主 render 每帧取用，不得重复打开）。
    #[test]
    fn open_file_request_is_consumed_once() {
        let shared = Shared::new();
        assert!(shared.take_open_in_editor().is_none(), "初始无请求");
        shared.request_open_in_editor(PathBuf::from("/p/a.sql"));
        assert_eq!(
            shared.take_open_in_editor(),
            Some(PathBuf::from("/p/a.sql")),
            "首次取出得到路径"
        );
        assert!(
            shared.take_open_in_editor().is_none(),
            "取出即清空：同一请求不会重复打开"
        );
    }
}
