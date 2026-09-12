//! 数据源连接对话框（Phase B：完整 Tab + 管理引用覆盖层）。
//!
//! 基于 gpui-kit 0.6.1 组件（能力按 crates.io 0.6.1 源码核对，路径见
//! `.agents/skills/gpui-kit-dev/SKILL.md`「查 API」；**不要**用本地 main 分支源码当签名参考）：
//! - 模态层：`window.open_dialog`（WorkbenchView 挂载 `Root::render_dialog_layer`）；管理器用嵌套 Dialog；
//! - 驱动/认证引用/网络引用/环境：`Select`（SearchableVec）；表单：`Form + Field + Input`；
//! - Tab 条与开关目前是自绘：**这是待迁移的技术债，不是「库没有」**——0.6.1 已提供
//!   `component::tab::TabBar`（`underline()` / `segmented()`）与 `component::switch::Switch`。
//!   新增代码不要照抄本目录的自绘实现；迁移计划见 `docs/architecture/connection/connection-prototype-design.md`。
//!
//! Phase B 范围（dev-plan B1-B6）：
//! - 网络 Tab：SSH/Proxy 协议链（添加/启用/上移/下移/删除，≤4 跳校验）+ 拓扑预览（DB 带 TLS 徽标）；
//! - 高级 Tab：环境选择 + 安全策略覆盖（"已覆盖"标记）+ DuckDB 本地加速卡片（仅网络型库）；
//! - 能力 Tab：驱动 capabilities 只读矩阵；驱动属性 Tab：key-value 动态增删；
//! - 三管理器覆盖层：认证/网络/环境 CRUD（AES 密文落库）+ 引用联动（选中引用→字段只读→落库引用 ID）。
//! - 拖拽排序在 0.6 无开箱组件，协议链排序用上移/下移（交互等价的确定性实现）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::base::StyledExt;
// 按钮 / 复选框 / 下拉的 `.disabled(bool)` 来自该 trait（Input 是本体方法，不需导入）。
use gpui_kit::base::Disableable as _;
use gpui_kit::base::input::Enter as InputEnter;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Theme, WindowExt};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use connection::model::{ConnectionScope, DataSourceSaveInput};
use engine::persistence::auth_store::AuthConfig;
use engine::persistence::connection_org_store::ConnectionGroup;
use engine::persistence::driver_store::{DataSourceType, Driver};
use engine::persistence::env_store::Environment;
use engine::persistence::id_prefix;
use engine::persistence::network_store::NetworkConfig;

use crate::commands::{DraftNext, DraftPrev, SaveConnection, TestConnection};
use crate::panels::Shared;
use crate::services::data_source_service::DataSourceService;
use connection::model::DataSource;

/// 认证类型（v1 AUTH_TYPE_DEFS 子集；数据为 JSON：{"username":..,"password":..} 等）。
const AUTH_TYPES: [&str; 3] = ["password", "ssh_key", "proxy_pwd"];
/// SSL/TLS 模式（domain 枚举：与 `url_params` 的 SSL 参数注入对齐，非业务数据）。
const SSL_MODES: [&str; 5] = ["disable", "prefer", "require", "verify-ca", "verify-full"];
/// 作用域选项（与 ConnectionScope 对齐；GP_ 快照引用共享）。
const SCOPE_LABELS: [&str; 3] = ["仅全局", "仅项目", "全局+项目"];
/// 作用域分段按钮显示文本（短版，节省 Header 宽度）→ 写库用上的全称标签。
const SCOPE_SEG_LABELS: [(&str, &str); 3] = [
    ("全局", SCOPE_LABELS[0]),
    ("项目", SCOPE_LABELS[1]),
    ("全局+项目", SCOPE_LABELS[2]),
];
/// 协议链最大跳数（B1 约束校验器）。
const MAX_HOPS: usize = 4;
/// 能力矩阵为空时的提示（驱动未声明任何能力）。
const CAP_EMPTY_HINT: &str = "该驱动未声明任何能力（drivers.capabilities 为空）";

mod helpers;
mod managers;
mod project_picker;
mod render;
mod staging;
mod state;

pub(crate) use helpers::*;
pub(crate) use managers::*;
pub use project_picker::{
    PROJECT_NEW_LABEL, PROJECT_NONE_LABEL, PROJECT_OPEN_LABEL, ProjectItem, ProjectItemKind,
};
pub(crate) use staging::saved_scope_short;
pub use staging::{ConnectionDraft, Hop};

/// 管理器工作区（三个管理器共用；列表 + 新建/编辑表单，Entity 状态持久）。
pub struct ManagerWorkspace {
    pub kind: usize,
    pub items: Vec<String>,
    pub new_name: Entity<InputState>,
    pub new_type: Entity<SelectState<SearchableVec<SharedString>>>,
    pub new_data: Entity<InputState>,
    /// 编辑中的条目名（Some = 更新既有条目，None = 新建）。
    pub editing: Option<String>,
    pub msg: Option<String>,
    // ---- 环境策略（kind=2 展开面板）----
    /// 当前展开策略管理的环境名（None = 收起）。
    pub policy_env: Option<String>,
    pub policy_type: Entity<SelectState<SearchableVec<SharedString>>>,
    pub policy_enabled: Rc<Cell<bool>>,
    /// (策略类型标签, 策略 ID, 是否启用)。
    pub policies: Rc<RefCell<Vec<(String, String, bool)>>>,
    /// 编辑中的策略 ID（None = 新建）。
    pub policy_editing: Option<String>,
}

/// 对话框状态（EditorPanel 持有；open_dialog builder 每次渲染重建 UI，状态持久）。
pub struct ConnectionDialogState {
    // ---- Phase A ----
    pub name: Entity<InputState>,
    pub url: Entity<InputState>,
    pub user: Entity<InputState>,
    pub pass: Entity<InputState>,
    pub driver: Entity<SelectState<SearchableVec<SharedString>>>,
    /// 左侧栏「数据库类型」搜索框（过滤类型 / 驱动名）。
    pub driver_filter: Entity<InputState>,
    /// 数据源类型目录（侧栏分类树；来自 global.db `data_source_types`）。
    pub types: Rc<RefCell<Vec<DataSourceType>>>,
    /// 驱动目录（drivers 表；Header「驱动」= 驱动实现，仅当前类型下选项、显示短名如 sqlx）。
    pub drivers: Rc<RefCell<Vec<Driver>>>,
    /// 当前选中的数据源类型（type_id；侧栏选中态 + 条目类型徽标 + 驱动过滤）。
    pub selected_type: Rc<RefCell<String>>,
    pub result: Rc<RefCell<Option<String>>>,
    pub result_ok: Rc<Cell<bool>>,
    // ---- Phase B ----
    pub remark: Entity<InputState>,
    /// 0 常规 / 1 网络 / 2 能力 / 3 驱动属性 / 4 高级。
    pub active_tab: Rc<Cell<usize>>,
    pub hops: Rc<RefCell<Vec<Hop>>>,
    pub env: Entity<SelectState<SearchableVec<SharedString>>>,
    pub env_list: Rc<RefCell<Vec<Environment>>>,
    /// 认证方法（选项来自当前驱动的 `supported_auth_types`；连接时用于注入凭据）。
    pub auth_method: Entity<SelectState<SearchableVec<SharedString>>>,
    /// 认证方法选项已按哪个驱动值加载（驱动变化时重建；None = 尚未加载）。
    pub auth_method_loaded_for: Rc<RefCell<Option<String>>>,
    pub auth_ref: Entity<SelectState<SearchableVec<SharedString>>>,
    pub network_ref: Entity<SelectState<SearchableVec<SharedString>>>,
    pub auth_list: Rc<RefCell<Vec<AuthConfig>>>,
    pub network_list: Rc<RefCell<Vec<NetworkConfig>>>,
    pub duckdb_fed: Rc<Cell<bool>>,
    pub cache_path: Entity<InputState>,
    /// 当前选中环境下的启用策略（来自 `environment_policies`，供高级 Tab 覆盖勾选）。
    /// 元素：(策略类型, 中文标签, 配置摘要)。
    pub env_policies: Rc<RefCell<Vec<(String, String, String)>>>,
    /// 已勾选的策略覆盖（存策略类型，与 `advanced_options.policy_overrides` 对应）。
    pub policy_override_keys: Rc<RefCell<Vec<String>>>,
    /// 策略清单已按哪个环境加载（环境切换时重查；None = 尚未加载）。
    pub env_policies_loaded_for: Rc<RefCell<Option<String>>>,
    pub props: Rc<RefCell<Vec<(String, String)>>>,
    pub prop_key: Entity<InputState>,
    pub prop_val: Entity<InputState>,
    pub mgr: Rc<RefCell<ManagerWorkspace>>,
    // ---- Phase C：编辑 / 作用域 / SSL ----
    /// 编辑中的连接 ID（Some = 编辑既有连接，None = 新建）。
    pub editing_id: Rc<RefCell<Option<String>>>,
    /// 作用域（仅全局 / 仅项目 / 全局+项目）。
    pub scope: Entity<SelectState<SearchableVec<SharedString>>>,
    /// 项目路径（作用域含项目侧时必需；.RSmeta 项目目录）。
    pub project_path: Entity<InputState>,
    /// 标签输入（逗号分隔；保存时解析为 JSON 数组写入 `tags` 并同步 `connection_tags`）。
    pub tags_input: Entity<InputState>,
    /// 项目分组目录（项目级；未打开项目为空）。
    pub groups: Rc<RefCell<Vec<ConnectionGroup>>>,
    /// 分组勾选态（id, name, checked）——多对多，保存时替换成员关系。
    pub group_checks: Rc<RefCell<Vec<(String, String, bool)>>>,
    /// SSL/TLS 模式。
    pub ssl_mode: Entity<SelectState<SearchableVec<SharedString>>>,
    /// 连接设置字段（网络型可编辑；与 Header URI 双向同步，URI 仍是落库权威）。
    pub host_input: Entity<InputState>,
    pub port_input: Entity<InputState>,
    pub db_input: Entity<InputState>,
    /// 字段同步标记：上次完成同步的（驱动 id, URL）。
    /// 两个方向靠它避免循环：URL 变了 → 反向填字段；字段变了 → 用 `rebuild_url_from_fields` 回写 URL。
    pub fields_synced_for: Rc<RefCell<Option<(String, String)>>>,
    /// 地址输入占位按驱动变化（每帧写入）。
    pub url_placeholder_for: Rc<RefCell<String>>,
    pub ssl_ca: Entity<InputState>,
    pub ssl_cert: Entity<InputState>,
    ssl_key: Entity<InputState>,
    /// 暂存列表（多连接连续编辑；见原型设计 §2.2）：未保存草稿快照 + 已保存条目占位。
    pub drafts: Rc<RefCell<Vec<ConnectionDraft>>>,
    /// 当前编辑条目索引（暂存列表光标）。
    pub draft_cursor: Rc<Cell<usize>>,
    /// 是否已从 `connection_drafts` 表恢复过（进程内只恢复一次，避免覆盖会话内编辑）。
    pub drafts_restored: Rc<Cell<bool>>,
    /// 元数据（引用列表 / 类型 / 驱动目录）是否已拉取：
    /// dialog builder 每次渲染都会执行，不加标记会反复建 runtime + 查库（hover 即卡顿）。
    /// 每次「打开对话框」入口会重置为 false（见 `EditorPanel::request_*`）。
    pub meta_refreshed: Rc<Cell<bool>>,
    /// 项目选择下拉（项目名 + 路径左右结构；末项为「＋ 新增项目」）。
    pub project_sel: Entity<SelectState<SearchableVec<ProjectItem>>>,
    /// 下拉选项 → 路径映射（项目名, 路径）；「新增项目」项路径为空。
    pub project_options: Rc<RefCell<Vec<(String, String)>>>,
    /// 当前项目会话快照（名称, 路径）；由 `open()` 从 `Shared::project` 写入，供项目下拉使用。
    pub session_project: Rc<RefCell<Option<(String, String)>>>,
    /// 单列大纲分组的折叠态（仅 UI 偏好，不落库）：在列 = 已折叠，缺省 = 展开。
    pub collapsed_sections: Rc<RefCell<Vec<String>>>,
}

fn state_inputs(
    window: &mut Window,
    cx: &mut App,
) -> (
    Entity<InputState>,
    Entity<InputState>,
    Entity<InputState>,
    Entity<InputState>,
) {
    (
        cx.new(|cx| InputState::new(window, cx)),
        cx.new(|cx| InputState::new(window, cx)),
        cx.new(|cx| InputState::new(window, cx)),
        cx.new(|cx| InputState::new(window, cx)),
    )
}
