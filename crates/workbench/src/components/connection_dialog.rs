//! 数据源连接对话框（Phase B：完整 Tab + 管理引用覆盖层）。
//!
//! 基于 gpui-kit 0.6 组件（能力按 0.6.0 源码核对，杜绝"无法实现的 UI"）：
//! - 模态层：`window.open_dialog`（WorkbenchView 挂载 `Root::render_dialog_layer`）；管理器用嵌套 Dialog；
//! - 驱动/认证引用/网络引用/环境：`Select`（SearchableVec）；表单：`Form + Field + Input`；
//! - Tab 条与开关为自绘（gpui-component 无 Tabs/Switch 组件，自绘 div 行为等价、零 API 猜测）。
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
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::select::{SearchableVec, Select, SelectState};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Theme, WindowExt};
use gpui_kit::*;

use connection::model::{ConnectionScope, DataSourceSaveInput};
use engine::persistence::auth_store::AuthConfig;
use engine::persistence::driver_store::{DataSourceType, Driver};
use engine::persistence::env_store::Environment;
use engine::persistence::network_store::NetworkConfig;

use crate::panels::Shared;
use crate::services::data_source_service::DataSourceService;
use connection::model::DataSource;

/// 内置驱动类型（Phase A 骨架；完整目录来自 DataSourceService::list_drivers）。
const BUILTIN_DRIVERS: [&str; 4] = ["mysql", "postgres", "sqlite", "duckdb"];
/// 认证类型（v1 AUTH_TYPE_DEFS 子集；数据为 JSON：{"username":..,"password":..} 等）。
const AUTH_TYPES: [&str; 3] = ["password", "ssh_key", "proxy_pwd"];
/// 环境策略项（对齐 v1 5 类策略 + 审计）。
const POLICY_ITEMS: [&str; 6] = [
    "只读连接",
    "禁止 DDL",
    "禁止导出",
    "查询超时 30s",
    "最大行数 1000",
    "审计日志",
];
/// 能力矩阵（只读展示，驱动声明对齐）。
const CAPABILITIES: [(&str, bool); 6] = [
    ("数据库导航", true),
    ("SQL 执行", true),
    ("DuckDB 联邦", true),
    ("数据导出", false),
    ("Mock 生成", false),
    ("资源分析", false),
];
/// 协议链最大跳数（B1 约束校验器）。
const MAX_HOPS: usize = 4;
/// 作用域选项（与 ConnectionScope 对齐；GP_ 快照引用共享）。
const SCOPE_LABELS: [&str; 3] = ["仅全局", "仅项目", "全局+项目"];
/// SSL/TLS 模式（落库 advanced_options.ssl.mode；verify-ca/verify-full 需 CA）。
const SSL_MODES: [&str; 5] = ["disable", "prefer", "require", "verify-ca", "verify-full"];
/// 策略落库键（与 v1 策略类型对齐，UI 标签见 POLICY_ITEMS）。
const POLICY_KEYS: [&str; 6] = [
    "read_only",
    "no_ddl",
    "no_export",
    "query_timeout",
    "row_limit",
    "audit",
];

/// 协议链跳（内联编辑态）。
#[derive(Debug, Clone)]
pub struct Hop {
    pub kind: String,
    pub label: String,
    pub enabled: bool,
}

impl Hop {
    fn ssh(label: impl Into<String>) -> Self {
        Self {
            kind: "SSH".into(),
            label: label.into(),
            enabled: true,
        }
    }
    fn proxy(label: impl Into<String>) -> Self {
        Self {
            kind: "Proxy".into(),
            label: label.into(),
            enabled: true,
        }
    }
}

/// 暂存列表条目（多连接连续编辑；见原型设计 §2.2）。
///
/// 约束：仅存进程内存（关闭对话框不丢失、应用退出清除），不落盘、不写明文凭据；
/// `saved_id` 为 `Some` 时表示该条目对应一条已保存连接（点击走编辑回读）。
#[derive(Debug, Clone, Default)]
pub struct ConnectionDraft {
    /// 显示名称（空 → 列表显示「新建数据源」）。
    pub name: String,
    /// 已保存连接 ID（Some = 已保存条目）。
    pub saved_id: Option<String>,
    // ---- 表单快照（与 Header / 各 Tab 字段一一对应）----
    pub driver_name: String,
    pub url: String,
    pub user: String,
    pub pass: String,
    pub remark: String,
    pub scope: String,
    pub project_path: String,
    pub ssl_mode: String,
    pub ssl_ca: String,
    pub ssl_cert: String,
    pub ssl_key: String,
    pub cache_path: String,
    pub duckdb_fed: bool,
    pub active_tab: usize,
    pub hops: Vec<Hop>,
    pub props: Vec<(String, String)>,
    pub sec_overrides: Vec<bool>,
    pub auth_ref: Option<String>,
    pub network_ref: Option<String>,
    pub env: Option<String>,
}

impl ConnectionDraft {
    /// 新建空草稿（默认值与对话框初始状态一致）。
    fn empty() -> Self {
        Self {
            duckdb_fed: true,
            sec_overrides: vec![true, true, false, true, true, false],
            ..Default::default()
        }
    }

    /// 列表显示名（空草稿回退「新建数据源」）。
    fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            if self.saved_id.is_some() {
                "（未命名连接）".to_string()
            } else {
                "新建数据源".to_string()
            }
        } else {
            self.name.clone()
        }
    }
}

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
    /// 驱动目录（drivers 表；Header「驱动类型」= 驱动实现，如 MySQL (sqlx) / MySQL (Official)）。
    pub drivers: Rc<RefCell<Vec<Driver>>>,
    /// 当前选中的数据源类型（type_id；侧栏选中态）。
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
    pub auth_ref: Entity<SelectState<SearchableVec<SharedString>>>,
    pub network_ref: Entity<SelectState<SearchableVec<SharedString>>>,
    pub auth_list: Rc<RefCell<Vec<AuthConfig>>>,
    pub network_list: Rc<RefCell<Vec<NetworkConfig>>>,
    pub duckdb_fed: Rc<Cell<bool>>,
    pub cache_path: Entity<InputState>,
    pub sec_overrides: Rc<RefCell<Vec<bool>>>,
    pub props: Rc<RefCell<Vec<(String, String)>>>,
    pub prop_key: Entity<InputState>,
    pub prop_val: Entity<InputState>,
    pub mgr: Rc<RefCell<ManagerWorkspace>>,
    // ---- Phase C：编辑 / 作用域 / SSL ----
    /// 编辑中的连接 ID（Some = 编辑既有连接，None = 新建）。
    pub editing_id: Rc<RefCell<Option<String>>>,
    /// 作用域（仅全局 / 仅项目 / 全局+项目）。
    pub scope: Entity<SelectState<SearchableVec<SharedString>>>,
    /// 项目路径（作用域含项目侧时必需；.RSMETA 项目目录）。
    pub project_path: Entity<InputState>,
    /// SSL/TLS 模式。
    pub ssl_mode: Entity<SelectState<SearchableVec<SharedString>>>,
    pub ssl_ca: Entity<InputState>,
    pub ssl_cert: Entity<InputState>,
    ssl_key: Entity<InputState>,
    /// 暂存列表（多连接连续编辑；见原型设计 §2.2）：未保存草稿快照 + 已保存条目占位。
    pub drafts: Rc<RefCell<Vec<ConnectionDraft>>>,
    /// 当前编辑条目索引（暂存列表光标）。
    pub draft_cursor: Rc<Cell<usize>>,
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

impl ConnectionDialogState {
    /// 懒创建全部受控状态（window 参与 InputState / SelectState 构造）。
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let drivers = SearchableVec::new(
            BUILTIN_DRIVERS
                .iter()
                .map(|d| SharedString::from(*d))
                .collect::<Vec<_>>(),
        );
        let (name, url, user, pass) = state_inputs(window, cx);
        let (remark, cache_path, prop_key, prop_val) = state_inputs(window, cx);
        let driver_filter = cx.new(|cx| InputState::new(window, cx));
        let (new_name, new_data, _, _) = state_inputs(window, cx);
        let (project_path, ssl_ca, ssl_cert, ssl_key) = state_inputs(window, cx);
        let new_type = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    AUTH_TYPES
                        .iter()
                        .map(|t| SharedString::from(*t))
                        .collect::<Vec<_>>(),
                ),
                None,
                window,
                cx,
            )
        });
        let policy_type = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    POLICY_ITEMS
                        .iter()
                        .map(|t| SharedString::from(*t))
                        .collect::<Vec<_>>(),
                ),
                None,
                window,
                cx,
            )
        });

        Self {
            name,
            url,
            user,
            pass,
            driver: cx.new(|cx| SelectState::new(drivers, None, window, cx)),
            driver_filter,
            types: Rc::new(RefCell::new(Vec::new())),
            drivers: Rc::new(RefCell::new(Vec::new())),
            selected_type: Rc::new(RefCell::new(String::new())),
            result: Rc::new(RefCell::new(None)),
            result_ok: Rc::new(Cell::new(true)),
            remark,
            active_tab: Rc::new(Cell::new(0)),
            hops: Rc::new(RefCell::new(vec![
                Hop::ssh("跳板机·prod-gw"),
                Hop::proxy("公司代理·http"),
            ])),
            env: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(Vec::<SharedString>::new()),
                    None,
                    window,
                    cx,
                )
            }),
            env_list: Rc::new(RefCell::new(Vec::new())),
            auth_ref: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(Vec::<SharedString>::new()),
                    None,
                    window,
                    cx,
                )
            }),
            network_ref: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(Vec::<SharedString>::new()),
                    None,
                    window,
                    cx,
                )
            }),
            auth_list: Rc::new(RefCell::new(Vec::new())),
            network_list: Rc::new(RefCell::new(Vec::new())),
            duckdb_fed: Rc::new(Cell::new(true)),
            cache_path,
            sec_overrides: Rc::new(RefCell::new(vec![true, true, false, true, true, false])),
            props: Rc::new(RefCell::new(vec![
                ("connect_timeout".to_string(), "10".to_string()),
                ("ssl_mode".to_string(), "prefer".to_string()),
            ])),
            prop_key,
            prop_val,
            mgr: Rc::new(RefCell::new(ManagerWorkspace {
                kind: 0,
                items: Vec::new(),
                new_name,
                new_type,
                new_data,
                editing: None,
                msg: None,
                policy_env: None,
                policy_type,
                policy_enabled: Rc::new(Cell::new(true)),
                policies: Rc::new(RefCell::new(Vec::new())),
                policy_editing: None,
            })),
            editing_id: Rc::new(RefCell::new(None)),
            scope: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(
                        SCOPE_LABELS
                            .iter()
                            .map(|s| SharedString::from(*s))
                            .collect::<Vec<_>>(),
                    ),
                    None,
                    window,
                    cx,
                )
            }),
            project_path,
            ssl_mode: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(
                        SSL_MODES
                            .iter()
                            .map(|s| SharedString::from(*s))
                            .collect::<Vec<_>>(),
                    ),
                    None,
                    window,
                    cx,
                )
            }),
            ssl_ca,
            ssl_cert,
            ssl_key,
            drafts: Rc::new(RefCell::new(vec![ConnectionDraft::empty()])),
            draft_cursor: Rc::new(Cell::new(0)),
        }
    }

    /// 拉取元数据（认证/网络/环境列表 + Select 选项）；首次打开时调用。
    fn refresh_meta(&self, window: &mut Window, cx: &mut App) {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => return,
        };
        if let Ok(service) = DataSourceService::global() {
            if let Ok(list) = rt.block_on(service.list_auth_configs()) {
                let names: Vec<SharedString> = list
                    .iter()
                    .filter_map(|a| a.name.clone())
                    .map(SharedString::from)
                    .collect();
                *self.auth_list.borrow_mut() = list;
                self.auth_ref.update(cx, |s, cx| {
                    s.set_items(SearchableVec::new(names), window, cx)
                });
            }
            if let Ok(list) = rt.block_on(service.list_network_configs()) {
                let names: Vec<SharedString> = list
                    .iter()
                    .filter_map(|n| n.name.clone())
                    .map(SharedString::from)
                    .collect();
                *self.network_list.borrow_mut() = list;
                self.network_ref.update(cx, |s, cx| {
                    s.set_items(SearchableVec::new(names), window, cx)
                });
            }
            if let Ok(list) = rt.block_on(service.list_environments()) {
                let names: Vec<SharedString> = list
                    .iter()
                    .map(|e| SharedString::from(e.name.clone()))
                    .collect();
                *self.env_list.borrow_mut() = list;
                self.env.update(cx, |s, cx| {
                    s.set_items(SearchableVec::new(names), window, cx)
                });
            }
            // 数据源类型目录（侧栏分类树）+ 驱动目录（Header「驱动类型」下拉）。
            // 语义：数据库类型（mysql/postgresql/…）= 侧栏；驱动（drivers 表，如 MySQL (sqlx)）= Header。
            if let Ok(types) = rt.block_on(service.list_data_source_types()) {
                *self.types.borrow_mut() = types;
            }
            if let Ok(drivers) = rt.block_on(service.list_drivers()) {
                let names: Vec<SharedString> = drivers
                    .iter()
                    .map(|d| SharedString::from(d.name.clone()))
                    .collect();
                *self.drivers.borrow_mut() = drivers;
                self.driver
                    .update(cx, |s, cx| s.set_items(SearchableVec::new(names), window, cx));
            }
        }
    }

    // ===== 暂存列表（多连接连续编辑；原型设计 §2.2）=====

    /// 当前条目索引（越界时回归 0）。
    fn draft_cursor_idx(&self) -> usize {
        let len = self.drafts.borrow().len();
        if len == 0 {
            0
        } else {
            self.draft_cursor.get().min(len - 1)
        }
    }

    /// 把当前表单快照写回第 `idx` 个条目（切换前调用；已保存条目跳过）。
    fn capture_draft(&self, idx: usize, cx: &mut App) {
        if self.editing_id.borrow().is_some() {
            return;
        }
        let d = ConnectionDraft {
            name: self.name.read(cx).value().to_string(),
            saved_id: None,
            driver_name: self
                .driver
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .to_string(),
            url: self.url.read(cx).value().to_string(),
            user: self.user.read(cx).value().to_string(),
            pass: self.pass.read(cx).value().to_string(),
            remark: self.remark.read(cx).value().to_string(),
            scope: self
                .scope
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .to_string(),
            project_path: self.project_path.read(cx).value().to_string(),
            ssl_mode: self
                .ssl_mode
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .to_string(),
            ssl_ca: self.ssl_ca.read(cx).value().to_string(),
            ssl_cert: self.ssl_cert.read(cx).value().to_string(),
            ssl_key: self.ssl_key.read(cx).value().to_string(),
            cache_path: self.cache_path.read(cx).value().to_string(),
            duckdb_fed: self.duckdb_fed.get(),
            active_tab: self.active_tab.get(),
            hops: self.hops.borrow().clone(),
            props: self.props.borrow().clone(),
            sec_overrides: self.sec_overrides.borrow().clone(),
            auth_ref: self
                .auth_ref
                .read(cx)
                .selected_value()
                .cloned()
                .map(|v| v.to_string()),
            network_ref: self
                .network_ref
                .read(cx)
                .selected_value()
                .cloned()
                .map(|v| v.to_string()),
            env: self
                .env
                .read(cx)
                .selected_value()
                .cloned()
                .map(|v| v.to_string()),
        };
        if let Some(slot) = self.drafts.borrow_mut().get_mut(idx) {
            *slot = d;
        }
    }

    /// 把第 `idx` 个条目载入表单（已保存条目走 `load_for_edit` 回读）。
    fn apply_draft(&self, idx: usize, window: &mut Window, cx: &mut App) {
        let Some(d) = self.drafts.borrow().get(idx).cloned() else {
            return;
        };
        if let Some(id) = d.saved_id.clone() {
            *self.editing_id.borrow_mut() = Some(id.clone());
            self.load_for_edit(&id, window, cx);
            return;
        }
        *self.editing_id.borrow_mut() = None;
        self.name
            .update(cx, |s, cx| s.set_value(d.name.clone(), window, cx));
        self.url
            .update(cx, |s, cx| s.set_value(d.url.clone(), window, cx));
        self.user
            .update(cx, |s, cx| s.set_value(d.user.clone(), window, cx));
        self.pass
            .update(cx, |s, cx| s.set_value(d.pass.clone(), window, cx));
        self.remark
            .update(cx, |s, cx| s.set_value(d.remark.clone(), window, cx));
        self.project_path
            .update(cx, |s, cx| s.set_value(d.project_path.clone(), window, cx));
        self.ssl_ca
            .update(cx, |s, cx| s.set_value(d.ssl_ca.clone(), window, cx));
        self.ssl_cert
            .update(cx, |s, cx| s.set_value(d.ssl_cert.clone(), window, cx));
        self.ssl_key
            .update(cx, |s, cx| s.set_value(d.ssl_key.clone(), window, cx));
        self.cache_path
            .update(cx, |s, cx| s.set_value(d.cache_path.clone(), window, cx));
        set_select_value(&self.driver, &d.driver_name, window, cx);
        set_select_value(&self.scope, &d.scope, window, cx);
        set_select_value(&self.ssl_mode, &d.ssl_mode, window, cx);
        set_select_value(
            &self.auth_ref,
            d.auth_ref.as_deref().unwrap_or(""),
            window,
            cx,
        );
        set_select_value(
            &self.network_ref,
            d.network_ref.as_deref().unwrap_or(""),
            window,
            cx,
        );
        set_select_value(&self.env, d.env.as_deref().unwrap_or(""), window, cx);
        self.duckdb_fed.set(d.duckdb_fed);
        self.active_tab.set(d.active_tab);
        *self.hops.borrow_mut() = d.hops;
        *self.props.borrow_mut() = d.props;
        *self.sec_overrides.borrow_mut() = d.sec_overrides;
    }

    /// 暂存列表「+ 添加」：追加空草稿并选中（规则 2）。
    pub fn staging_add(&self, window: &mut Window, cx: &mut App) {
        let current = self.draft_cursor_idx();
        self.capture_draft(current, cx);
        let idx = {
            let mut drafts = self.drafts.borrow_mut();
            drafts.push(ConnectionDraft::empty());
            drafts.len() - 1
        };
        self.draft_cursor.set(idx);
        self.apply_draft(idx, window, cx);
    }

    /// 暂存列表：切换条目（规则 1：先写回当前，再载入目标）。
    pub fn staging_select(&self, idx: usize, window: &mut Window, cx: &mut App) {
        if idx >= self.drafts.borrow().len() {
            return;
        }
        let current = self.draft_cursor_idx();
        if current != idx {
            self.capture_draft(current, cx);
        }
        self.draft_cursor.set(idx);
        self.apply_draft(idx, window, cx);
    }

    /// 暂存列表：删除草稿（规则 3：删至最后一条自动补位；已保存条目不在此删除）。
    pub fn staging_remove(&self, idx: usize, window: &mut Window, cx: &mut App) {
        {
            let drafts = self.drafts.borrow();
            match drafts.get(idx) {
                Some(d) if d.saved_id.is_none() => {}
                _ => return,
            }
        }
        {
            let mut drafts = self.drafts.borrow_mut();
            drafts.remove(idx);
            if drafts.is_empty() {
                drafts.push(ConnectionDraft::empty());
            }
        }
        let len = self.drafts.borrow().len();
        let cur = self.draft_cursor_idx().min(len.saturating_sub(1));
        self.draft_cursor.set(cur);
        self.apply_draft(cur, window, cx);
    }

    /// 保存成功后：当前条目转为已保存，并追加空草稿保持连续新建（规则 4）。
    pub fn staging_after_save(
        &self,
        conn_id: &str,
        name: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        let cur = self.draft_cursor_idx();
        {
            let mut drafts = self.drafts.borrow_mut();
            if cur < drafts.len() {
                drafts[cur].saved_id = Some(conn_id.to_string());
                drafts[cur].name = name.to_string();
            }
            drafts.push(ConnectionDraft::empty());
        }
        let idx = self.drafts.borrow().len() - 1;
        self.draft_cursor.set(idx);
        self.apply_draft(idx, window, cx);
    }

    /// 关闭对话框前把当前表单写回草稿（规则 5：关闭不丢失）。
    pub fn staging_flush(&self, cx: &mut App) {
        let cur = self.draft_cursor_idx();
        self.capture_draft(cur, cx);
    }

    /// 打开对话框时合并已保存连接为列表条目（仅全局列表；P_/GP_ 编辑入口在侧边栏）。
    fn staging_merge_saved(&self) {
        let Ok(service) = DataSourceService::global() else {
            return;
        };
        let Ok(rt) = tokio::runtime::Runtime::new() else {
            return;
        };
        if let Ok(list) = rt.block_on(service.list()) {
            let mut drafts = self.drafts.borrow_mut();
            for ds in list {
                if !drafts
                    .iter()
                    .any(|d| d.saved_id.as_deref() == Some(ds.id.as_str()))
                {
                    drafts.push(ConnectionDraft {
                        name: ds.name.clone(),
                        saved_id: Some(ds.id.clone()),
                        ..ConnectionDraft::empty()
                    });
                }
            }
        }
    }

    /// 编辑回读：按连接 ID 预填全部 Tab 字段（协议链 / 驱动属性 / SSL / 策略覆盖 / 作用域 / 缓存路径）。
    fn load_for_edit(&self, id: &str, window: &mut Window, cx: &mut App) {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => return,
        };
        let Ok(service) = DataSourceService::global() else {
            return;
        };
        let Ok(Some(ds)) = rt.block_on(service.get(id)) else {
            return;
        };

        self.name
            .update(cx, |s, cx| s.set_value(ds.name.clone(), window, cx));
        self.url
            .update(cx, |s, cx| s.set_value(reconstruct_url(&ds), window, cx));
        self.user.update(cx, |s, cx| {
            s.set_value(ds.username.clone().unwrap_or_default(), window, cx)
        });
        self.remark.update(cx, |s, cx| {
            s.set_value(ds.description.clone().unwrap_or_default(), window, cx)
        });
        self.cache_path.update(cx, |s, cx| {
            s.set_value(ds.metadata_path.clone().unwrap_or_default(), window, cx)
        });
        self.duckdb_fed.set(ds.use_duckdb_fed);
        self.driver.update(cx, |s, cx| {
            if ds.db_type.is_empty() {
                return;
            }
            // db_type 存的是驱动 id（如 mysql_native）；下拉显示驱动 name，未命中时回退原值。
            let name = self
                .drivers
                .borrow()
                .iter()
                .find(|d| d.id == ds.db_type)
                .map(|d| d.name.clone())
                .unwrap_or_else(|| ds.db_type.clone());
            s.set_selected_value(&SharedString::from(name), window, cx)
        });
        self.scope.update(cx, |s, cx| {
            s.set_selected_value(&SharedString::from(scope_label(&ds.scope)), window, cx)
        });

        // 引用选择：名称已存在于下拉选项中则选中（选项在 refresh_meta 后已就绪）。
        if let Some(aid) = &ds.auth_config_id {
            if let Some(a) = self.auth_list.borrow().iter().find(|a| &a.id == aid) {
                if let Some(n) = &a.name {
                    let n = n.clone();
                    self.auth_ref.update(cx, |s, cx| {
                        s.set_selected_value(&SharedString::from(n), window, cx)
                    });
                }
            }
        }
        if let Some(nid) = &ds.network_config_id {
            if let Some(c) = self.network_list.borrow().iter().find(|c| &c.id == nid) {
                if let Some(n) = &c.name {
                    let n = n.clone();
                    self.network_ref.update(cx, |s, cx| {
                        s.set_selected_value(&SharedString::from(n), window, cx)
                    });
                }
            }
        }
        if let Some(eid) = &ds.environment_id {
            if let Some(e) = self.env_list.borrow().iter().find(|e| &e.id == eid) {
                let n = e.name.clone();
                self.env.update(cx, |s, cx| {
                    s.set_selected_value(&SharedString::from(n), window, cx)
                });
            }
        }

        // 驱动属性（JSON → key-value 列表）。
        if let Some(props_json) = &ds.driver_properties {
            if let Ok(map) =
                serde_json::from_str::<std::collections::BTreeMap<String, String>>(props_json)
            {
                let mut list: Vec<(String, String)> = map.into_iter().collect();
                if list.is_empty() {
                    list.push(("connect_timeout".into(), "10".into()));
                }
                *self.props.borrow_mut() = list;
            }
        }

        // 高级选项：network_chain / ssl / policy_overrides。
        if let Some(adv) = &ds.advanced_options {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(adv) {
                if let Some(chain) = v.get("network_chain").and_then(|c| c.as_array()) {
                    let hops: Vec<Hop> = chain
                        .iter()
                        .filter_map(|h| {
                            let kind = h
                                .get("kind")
                                .and_then(|k| k.as_str())
                                .unwrap_or("SSH")
                                .to_string();
                            let label = h
                                .get("label")
                                .and_then(|k| k.as_str())
                                .unwrap_or("")
                                .to_string();
                            let enabled =
                                h.get("enabled").and_then(|k| k.as_bool()).unwrap_or(true);
                            if label.is_empty() {
                                None
                            } else {
                                Some(Hop {
                                    kind,
                                    label,
                                    enabled,
                                })
                            }
                        })
                        .collect();
                    if !hops.is_empty() {
                        *self.hops.borrow_mut() = hops;
                    }
                }
                if let Some(ssl) = v.get("ssl").and_then(|c| c.as_object()) {
                    if let Some(mode) = ssl.get("mode").and_then(|m| m.as_str()) {
                        let mode = mode.to_string();
                        self.ssl_mode.update(cx, |s, cx| {
                            s.set_selected_value(&SharedString::from(mode), window, cx)
                        });
                    }
                    if let Some(ca) = ssl.get("ca").and_then(|m| m.as_str()) {
                        let ca = ca.to_string();
                        self.ssl_ca.update(cx, |s, cx| s.set_value(ca, window, cx));
                    }
                    if let Some(cert) = ssl.get("cert").and_then(|m| m.as_str()) {
                        let cert = cert.to_string();
                        self.ssl_cert
                            .update(cx, |s, cx| s.set_value(cert, window, cx));
                    }
                    if let Some(key) = ssl.get("key").and_then(|m| m.as_str()) {
                        let key = key.to_string();
                        self.ssl_key
                            .update(cx, |s, cx| s.set_value(key, window, cx));
                    }
                }
                if let Some(overrides) = v.get("policy_overrides").and_then(|c| c.as_array()) {
                    let flags: Vec<bool> = POLICY_KEYS
                        .iter()
                        .map(|k| overrides.iter().any(|o| o.as_str() == Some(*k)))
                        .collect();
                    if flags.len() == POLICY_ITEMS.len() {
                        *self.sec_overrides.borrow_mut() = flags;
                    }
                }
            }
        }
    }

    /// 收集表单值（保留语义完整副本；实际路径见 ClonedDialogState::collect）。
    #[allow(dead_code)]
    fn collect(
        &self,
        name: &Entity<InputState>,
        driver: &Entity<SelectState<SearchableVec<SharedString>>>,
        url: &Entity<InputState>,
        user: &Entity<InputState>,
        pass: &Entity<InputState>,
        cx: &mut App,
    ) -> Option<DataSourceSaveInput> {
        // 本副本仅作语义参考，未参与运行：实际落库路径在 `ClonedDialogState::collect`
        // （db_type 需按驱动名反查 drivers 表得到驱动 id，见该实现）。
        let db_type = driver.read(cx).selected_value().cloned().unwrap_or_default();
        if db_type.is_empty() {
            return None;
        }
        let name = name.read(cx).value().to_string();
        let url = url.read(cx).value().to_string();
        if name.trim().is_empty() || url.trim().is_empty() {
            return None;
        }
        let username = {
            let v = user.read(cx).value().to_string();
            if v.trim().is_empty() {
                None
            } else {
                Some(v.trim().to_string())
            }
        };
        let password = {
            let v = pass.read(cx).value().to_string();
            if v.is_empty() { None } else { Some(v) }
        };
        let auth_config_id = self
            .auth_ref
            .read(cx)
            .selected_value()
            .cloned()
            .and_then(|n| {
                self.auth_list
                    .borrow()
                    .iter()
                    .find(|a| a.name.as_deref() == Some(n.as_str()))
                    .map(|a| a.id.clone())
            });
        let network_config_id = self
            .network_ref
            .read(cx)
            .selected_value()
            .cloned()
            .and_then(|n| {
                self.network_list
                    .borrow()
                    .iter()
                    .find(|c| c.name.as_deref() == Some(n.as_str()))
                    .map(|c| c.id.clone())
            });
        let environment_id = self.env.read(cx).selected_value().cloned().and_then(|n| {
            self.env_list
                .borrow()
                .iter()
                .find(|e| e.name == n.as_str())
                .map(|e| e.id.clone())
        });
        let driver_properties = if self.props.borrow().is_empty() {
            None
        } else {
            let map: std::collections::BTreeMap<String, String> =
                self.props.borrow().iter().cloned().collect();
            serde_json::to_string(&map).ok()
        };
        Some(DataSourceSaveInput {
            name: name.trim().to_string(),
            db_type: db_type.to_string(),
            url: url.trim().to_string(),
            username,
            password,
            scope: ConnectionScope::Global,
            description: {
                let r = self.remark.read(cx).value().to_string();
                if r.trim().is_empty() {
                    None
                } else {
                    Some(r.trim().to_string())
                }
            },
            driver_id: None,
            environment_id,
            auth_config_id,
            auth_method: None,
            network_config_id,
            driver_properties,
            advanced_options: None,
            options: None,
            tags: None,
            use_duckdb_fed: Some(self.duckdb_fed.get()),
            schema_name: None,
            metadata_path: {
                let v = self.cache_path.read(cx).value().to_string();
                if v.trim().is_empty() {
                    None
                } else {
                    Some(v.trim().to_string())
                }
            },
        })
    }

    /// 协议链是否合法（≤4 跳；hop 命名非空；引用档案时跳过内联校验）。
    #[allow(dead_code)]
    fn hops_valid(&self, _cx: &mut App) -> Result<(), String> {
        if self.network_ref.read(_cx).selected_value().is_some() {
            return Ok(()); // 引用档案：字段只读，不校验内联链
        }
        let hops = self.hops.borrow();
        if hops.len() > MAX_HOPS {
            return Err(format!("协议链最多 {} 跳（当前 {}）", MAX_HOPS, hops.len()));
        }
        if hops.iter().any(|h| h.label.trim().is_empty()) {
            return Err("协议链跳的配置名不能为空".into());
        }
        Ok(())
    }
}

impl ConnectionDialogState {
    /// 打开数据源连接对话框（状态由 EditorPanel 持有；open_dialog builder 每次渲染重建 UI）。
    pub fn open(
        self: &Rc<Self>,
        entity: Entity<crate::panels::EditorPanel>,
        shared: Shared,
        editing_id: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 暂存列表回调需要克隆状态句柄（Rc）。
        let state = Rc::clone(self);
        // 重入保护：先关闭已有的本对话框层，避免连续 open 叠加（幂等打开）。
        window.close_dialog(cx);
        *self.editing_id.borrow_mut() = editing_id.clone();
        if let Some(id) = &editing_id {
            self.load_for_edit(id, window, cx);
        }
        // 当前项目会话接入（C2）：项目根自动预填，项目作用域无需手输；
        // 已填值（如编辑回读）不覆盖。
        let session_root = shared
            .project
            .borrow()
            .as_ref()
            .map(|p| p.root.to_string_lossy().to_string());
        if let Some(root) = session_root {
            if self.project_path.read(cx).value().trim().is_empty() {
                self.project_path
                    .update(cx, |s, cx| s.set_value(root, window, cx));
            }
        }
        let name = self.name.clone();
        let url = self.url.clone();
        let user = self.user.clone();
        let pass = self.pass.clone();
        let driver = self.driver.clone();
        let driver_filter = self.driver_filter.clone();
        let types_list = self.types.clone();
        let drivers_list = self.drivers.clone();
        let selected_type = self.selected_type.clone();
        let drafts_list = self.drafts.clone();
        let draft_cursor = self.draft_cursor.clone();
        let remark = self.remark.clone();
        let active_tab = self.active_tab.clone();
        let hops = self.hops.clone();
        let env = self.env.clone();
        let env_list = self.env_list.clone();
        let auth_ref = self.auth_ref.clone();
        let network_ref = self.network_ref.clone();
        let auth_list = self.auth_list.clone();
        let network_list = self.network_list.clone();
        let duckdb_fed = self.duckdb_fed.clone();
        let cache_path = self.cache_path.clone();
        let props = self.props.clone();
        let prop_key = self.prop_key.clone();
        let prop_val = self.prop_val.clone();
        let mgr = self.mgr.clone();
        let result = self.result.clone();
        let result_ok = self.result_ok.clone();
        let editing_id = self.editing_id.clone();
        let scope = self.scope.clone();
        let project_path = self.project_path.clone();
        let ssl_mode = self.ssl_mode.clone();
        let ssl_ca = self.ssl_ca.clone();
        let ssl_cert = self.ssl_cert.clone();
        let ssl_key = self.ssl_key.clone();
        let sec_overrides = self.sec_overrides.clone();

        // 拉取一次元数据（认证/网络/环境引用选项）。
        self.refresh_meta(window, cx);
        // 暂存列表：合并已保存连接条目（多连接连续编辑，原型设计 §2.2）。
        self.staging_merge_saved();

        // 输入占位（InputState 构造后设置；Input 组件本身无 placeholder 方法）。
        name.update(cx, |s, cx| s.set_placeholder("名称（如 生产 PG）", window, cx));
        url.update(cx, |s, cx| s.set_placeholder("mysql://host:3306/db", window, cx));
        remark.update(cx, |s, cx| s.set_placeholder("备注（可选）", window, cx));
        driver_filter.update(cx, |s, cx| s.set_placeholder("搜索类型…", window, cx));
        prop_key.update(cx, |s, cx| s.set_placeholder("key", window, cx));
        prop_val.update(cx, |s, cx| s.set_placeholder("value", window, cx));
        project_path.update(cx, |s, cx| {
            s.set_placeholder("项目根目录（含 .RSMETA）", window, cx)
        });
        ssl_ca.update(cx, |s, cx| {
            s.set_placeholder("CA 证书路径（可选）", window, cx)
        });
        ssl_cert.update(cx, |s, cx| {
            s.set_placeholder("客户端证书路径（可选）", window, cx)
        });
        ssl_key.update(cx, |s, cx| {
            s.set_placeholder("私钥路径（可选）", window, cx)
        });

        // 测试连接（block_on，与 workbench 现有服务调用模式一致）。
        let run_test = move |input: DataSourceSaveInput| -> (bool, String) {
            let service = match DataSourceService::global() {
                Ok(s) => s,
                Err(e) => return (false, format!("服务未就绪: {e}")),
            };
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => return (false, format!("运行时错误: {e}")),
            };
            let t = rt.block_on(service.test(&input));
            // 反馈拼上探测到的服务器版本（原型："成功（版本＋延迟）"）。
            let detail = match t.version.as_deref() {
                Some(v) => format!("{} · 版本 {}", t.message, v),
                None => t.message,
            };
            (t.success, detail)
        };

        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme();

            // ---- 当前驱动 / 类型同步（render 为权威同步点）----
            let driver_now = driver
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .to_string();
            let drivers_snapshot: Vec<Driver> = drivers_list.borrow().clone();
            let types_snapshot: Vec<DataSourceType> = types_list.borrow().clone();
            let current_driver = drivers_snapshot.iter().find(|d| d.name == driver_now).cloned();
            if let Some(d) = &current_driver {
                let mut st = selected_type.borrow_mut();
                if *st != d.type_id {
                    *st = d.type_id.clone();
                }
            }
            let selected_type_id = selected_type.borrow().clone();
            // 文件型判定优先用驱动元数据（drivers.is_file），无驱动记录时回退类型名。
            let is_file_db = current_driver
                .as_ref()
                .map(|d| d.is_file)
                .unwrap_or_else(|| matches!(selected_type_id.as_str(), "sqlite" | "duckdb"));

            // ---- Tab 条（自绘；gpui-component 无 Tabs 组件）----
            // 文件型驱动（SQLite/DuckDB）按原型隐藏「网络」Tab（无协议链 / SSL 语义）。
            let tab_defs: Vec<(&'static str, usize)> = if is_file_db {
                vec![("常规", 0), ("能力", 2), ("驱动属性", 3), ("高级", 4)]
            } else {
                vec![
                    ("常规", 0),
                    ("网络", 1),
                    ("能力", 2),
                    ("驱动属性", 3),
                    ("高级", 4),
                ]
            };
            let mut tab_bar = div()
                .h_flex()
                .items_center()
                .gap_1()
                .border_b_1()
                .border_color(theme.colors.border);
            for (label, tab_ix) in tab_defs {
                let on = active_tab.get() == tab_ix;
                let idx = tab_ix;
                let active_tab = active_tab.clone();
                let entity = entity.clone();
                tab_bar = tab_bar.child(
                    div()
                        .id(ElementId::Name(SharedString::from(format!("tab-{label}"))))
                        .cursor_pointer()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .font_weight(if on { FontWeight::BOLD } else { FontWeight::NORMAL })
                        .text_color(if on { theme.colors.foreground } else { theme.colors.muted_foreground })
                        .border_b_2()
                        .border_color(if on { theme.colors.primary } else { hsla(0., 0., 0., 0.) })
                        .child(label)
                        .on_click(move |_, _, app| {
                            active_tab.set(idx);
                            entity.update(app, |_, cx| cx.notify());
                        }),
                );
            }

            // ---- 各 Tab 内容 ----
            let tab_content = match active_tab.get() {
                1 => {
                    // ===== 网络：协议链 + 拓扑 =====
                    let network_ref_selected = network_ref.read(cx).selected_value().cloned();
                    let is_ref = network_ref_selected.is_some();
                    let hops_ui = {
                        let hops_outer = hops.clone();
                        let hops_ref = hops.borrow();
                        let mut rows = div().v_flex().gap_1();
                        if hops_ref.is_empty() {
                            rows = rows.child(
                                div().text_xs().text_color(theme.colors.muted_foreground)
                                    .child("无协议链跳——直接连接目标数据库"),
                            );
                        }
                        for (i, hop) in hops_ref.iter().enumerate() {
                            let hop = hop.clone();
                            let idx = i;
                            let hops_outer = hops_outer.clone();
                            let entity = entity.clone();
                            let kind_color = if hop.kind == "SSH" {
                                theme.colors.success
                            } else {
                                theme.colors.warning
                            };
                            rows = rows.child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.colors.border)
                                    .px_2()
                                    .py_1()
                                    .child(div().text_xs().text_color(theme.colors.muted_foreground).child(format!("{}", idx + 1)))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(kind_color)
                                            .child(hop.kind.clone()),
                                    )
                                    .child(div().text_sm().flex_1().child(hop.label.clone()))
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from(format!("hop-en-{idx}"))))
                                            .cursor_pointer()
                                            .text_xs()
                                            .text_color(if hop.enabled { theme.colors.success } else { theme.colors.muted_foreground })
                                            .child(if hop.enabled { "启用" } else { "停用" })
                                            .on_click({
                                                let hops = hops_outer.clone();
                                                let entity = entity.clone();
                                                move |_, _, app| {
                                                    let mut h = hops.borrow_mut();
                                                    if let Some(hop) = h.get_mut(idx) { hop.enabled = !hop.enabled; }
                                                    drop(h);
                                                    entity.update(app, |_, cx| cx.notify());
                                                }
                                            }),
                                    )
                                    .child(
                                        div().h_flex().gap_1()
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(format!("hop-up-{idx}"))))
                                                    .cursor_pointer()
                                                    .text_xs()
                                                    .text_color(theme.colors.muted_foreground)
                                                    .child("↑")
                                                    .on_click({
                                                        let hops = hops_outer.clone();
                                                        let entity = entity.clone();
                                                        move |_, _, app| {
                                                            let mut h = hops.borrow_mut();
                                                            if idx > 0 { h.swap(idx - 1, idx); }
                                                            drop(h);
                                                            entity.update(app, |_, cx| cx.notify());
                                                        }
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(format!("hop-dn-{idx}"))))
                                                    .cursor_pointer()
                                                    .text_xs()
                                                    .text_color(theme.colors.muted_foreground)
                                                    .child("↓")
                                                    .on_click({
                                                        let hops = hops_outer.clone();
                                                        let entity = entity.clone();
                                                        move |_, _, app| {
                                                            let mut h = hops.borrow_mut();
                                                            if idx + 1 < h.len() { h.swap(idx, idx + 1); }
                                                            drop(h);
                                                            entity.update(app, |_, cx| cx.notify());
                                                        }
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(format!("hop-del-{idx}"))))
                                                    .cursor_pointer()
                                                    .text_xs()
                                                    .text_color(theme.colors.danger)
                                                    .child("删除")
                                                    .on_click({
                                                        let hops = hops_outer.clone();
                                                        let entity = entity.clone();
                                                        move |_, _, app| {
                                                            hops.borrow_mut().remove(idx);
                                                            entity.update(app, |_, cx| cx.notify());
                                                        }
                                                    }),
                                            ),
                                    ),
                            );
                        }
                        rows
                    };

                    let add_hop = div().h_flex().items_center().gap_2()
                        .child(
                            Button::new("add-ssh")
                                .secondary()
                                .label("+ SSH 跳")
                                .on_click({
                                    let hops = hops.clone();
                                    let entity = entity.clone();
                                    let result = result.clone();
                                    let result_ok = result_ok.clone();
                                    move |_, _, app| {
                                        let mut h = hops.borrow_mut();
                                        if h.len() >= MAX_HOPS {
                                            *result.borrow_mut() =
                                                Some(format!("协议链最多 {} 跳", MAX_HOPS));
                                            result_ok.set(false);
                                            drop(h);
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let n = h.len() + 1;
                                        h.push(Hop::ssh(format!("跳板机·ssh-{n}")));
                                        drop(h);
                                        *result.borrow_mut() = Some("已添加 SSH 跳".into());
                                        result_ok.set(true);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        )
                        .child(
                            Button::new("add-proxy")
                                .secondary()
                                .label("+ Proxy 跳")
                                .on_click({
                                    let hops = hops.clone();
                                    let entity = entity.clone();
                                    let result = result.clone();
                                    let result_ok = result_ok.clone();
                                    move |_, _, app| {
                                        let mut h = hops.borrow_mut();
                                        if h.len() >= MAX_HOPS {
                                            *result.borrow_mut() =
                                                Some(format!("协议链最多 {} 跳", MAX_HOPS));
                                            result_ok.set(false);
                                            drop(h);
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let n = h.len() + 1;
                                        h.push(Hop::proxy(format!("代理·http-{n}")));
                                        drop(h);
                                        *result.borrow_mut() = Some("已添加 Proxy 跳".into());
                                        result_ok.set(true);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        )
                        .child(
                            div().text_xs().text_color(theme.colors.muted_foreground)
                                .child(format!("≤ {} 跳（SSH / HTTP(S) 代理）", MAX_HOPS)),
                        );

                    // 拓扑预览（DB 节点带 TLS 徽标，SSL 在常规→连接安全）。
                    let hops = hops.borrow();
                    let mut path = div().h_flex().items_center().gap_2().flex_wrap();
                    path = path.child(
                        div()
                            .text_xs()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.colors.border)
                            .px_2()
                            .py_1()
                            .child("本机客户端"),
                    );
                    for hop in hops.iter() {
                        path = path
                            .child(div().text_xs().text_color(theme.colors.muted_foreground).child("→"))
                            .child(
                                div()
                                    .text_xs()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.colors.border)
                                    .px_2()
                                    .py_1()
                                    .child(format!("{} · {}", hop.kind, hop.label)),
                            );
                    }
                    path = path
                        .child(div().text_xs().text_color(theme.colors.muted_foreground).child("→"))
                        .child(
                            div()
                                .text_xs()
                                .rounded_md()
                                .border_1()
                                .border_color(theme.colors.border)
                                .px_2()
                                .py_1()
                                .child("目标数据库")
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.info)
                                        .child(" TLS"),
                                ),
                        );

                    let mut content = div().v_flex().gap_3();
                    content = content.child(
                        div().v_flex().gap_2()
                            .child(
                                div().h_flex().items_center().gap_2()
                                    .child(div().text_sm().font_weight(FontWeight::BOLD).child("协议链"))
                                    .child(
                                        div().text_xs().text_color(theme.colors.muted_foreground)
                                            .child("引用网络配置："),
                                    )
                                    .child(Select::new(&network_ref).placeholder("（内联编辑）"))
                                    .child(
                                        Button::new("mgr-network")
                                            .ghost()
                                            .label("管理")
                                            .on_click({
                                                let mgr = mgr.clone();
                                                let entity = entity.clone();
                                                let shared = shared.clone();
                                                move |_, window, app| {
                                                    open_manager(1, &mgr, entity.clone(), shared.clone(), window, app);
                                                }
                                            }),
                                    ),
                            ),
                    );
                    if is_ref {
                        content = content.child(
                            div().text_xs().text_color(theme.colors.info)
                                .child("已引用网络档案 · 链字段只读（改档案一处全量生效）"),
                        );
                    } else {
                        content = content.child(hops_ui).child(add_hop);
                    }
                    content = content.child(
                        div().v_flex().gap_1()
                            .child(div().text_sm().font_weight(FontWeight::BOLD).child("数据路径预览"))
                            .child(path),
                    );
                    content
                }
                2 => {
                    // ===== 能力：只读矩阵 =====
                    let mut chips = div().h_flex().gap_2().flex_wrap();
                    for (label, ok) in CAPABILITIES {
                        chips = chips.child(
                            div()
                                .text_xs()
                                .rounded_full()
                                .border_1()
                                .px_3()
                                .py_1()
                                .border_color(if ok { theme.colors.success } else { theme.colors.border })
                                .text_color(if ok { theme.colors.success } else { theme.colors.muted_foreground })
                                .child(label),
                        );
                    }
                    div().v_flex().gap_2()
                        .child(
                            div().text_xs().text_color(theme.colors.muted_foreground)
                                .child("能力由驱动声明（driver.capabilities）· 只读展示"),
                        )
                        .child(chips)
                }
                3 => {
                    // ===== 驱动属性：key-value 动态增删 =====
                    let props_ui = {
                        let props_outer = props.clone();
                        let props_ref = props.borrow();
                        let mut rows = div().v_flex().gap_1();
                        for (i, (k, v)) in props_ref.iter().enumerate() {
                            let idx = i;
                            let k = k.clone();
                            let v = v.clone();
                            let props = props_outer.clone();
                            let entity = entity.clone();
                            rows = rows.child(
                                div().h_flex().items_center().gap_2()
                                    .child(div().text_xs().child(k))
                                    .child(div().text_xs().text_color(theme.colors.muted_foreground).child("="))
                                    .child(div().text_xs().flex_1().child(v))
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from(format!("prop-del-{idx}"))))
                                            .cursor_pointer()
                                            .text_xs()
                                            .text_color(theme.colors.danger)
                                            .child("删除")
                                            .on_click(move |_, _, app| {
                                                props.borrow_mut().remove(idx);
                                                entity.update(app, |_, cx| cx.notify());
                                            }),
                                    ),
                            );
                        }
                        rows
                    };
                    let add_prop = div().h_flex().items_center().gap_2()
                        .child(Input::new(&prop_key))
                        .child(Input::new(&prop_val))
                        .child(
                            Button::new("add-prop")
                                .secondary()
                                .label("添加")
                                .on_click({
                                    let props = props.clone();
                                    let entity = entity.clone();
                                    let prop_key = prop_key.clone();
                                    let prop_val = prop_val.clone();
                                    let result = result.clone();
                                    let result_ok = result_ok.clone();
                                    move |_, window, app| {
                                        let k = prop_key.read(app).value().to_string();
                                        let v = prop_val.read(app).value().to_string();
                                        if k.trim().is_empty() {
                                            *result.borrow_mut() = Some("属性 key 不能为空".into());
                                            result_ok.set(false);
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let mut p = props.borrow_mut();
                                        if let Some(e) = p.iter_mut().find(|(ek, _)| ek == &k) {
                                            e.1 = v;
                                        } else {
                                            p.push((k.trim().to_string(), v));
                                        }
                                        drop(p);
                                        prop_key.update(app, |s, cx| s.set_value("", window, cx));
                                        prop_val.update(app, |s, cx| s.set_value("", window, cx));
                                        *result.borrow_mut() = Some("驱动属性已更新".into());
                                        result_ok.set(true);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        );
                    div().v_flex().gap_2()
                        .child(
                            div().text_xs().text_color(theme.colors.muted_foreground)
                                .child("driver_properties · key-value（随连接落库，覆盖驱动默认）"),
                        )
                        .child(props_ui)
                        .child(add_prop)
                }
                4 => {
                    // ===== 高级：环境 + 策略覆盖 + DuckDB 加速 =====
                    let is_network_db = {
                        let db = driver.read(cx).selected_value().cloned().unwrap_or_default();
                        db != "sqlite" && db != "duckdb" && !db.is_empty()
                    };
                    let env_summary = {
                        let sel = env.read(cx).selected_value().cloned();
                        if let Some(n) = sel {
                            let list = env_list.borrow();
                            list.iter().find(|e| e.name == n.as_str())
                                .map(|e| format!("{} · 策略生效中", e.name))
                                .unwrap_or_else(|| format!("{n} · 策略生效中"))
                        } else {
                            "未选择环境（使用连接默认策略）".to_string()
                        }
                    };

                    let mut content = div().v_flex().gap_3();
                    content = content.child(
                        div().v_flex().gap_2()
                            .child(
                                div().h_flex().items_center().gap_2()
                                    .child(div().text_sm().font_weight(FontWeight::BOLD).child("环境"))
                                    .child(Select::new(&env).placeholder("选择环境…"))
                                    .child(
                                        Button::new("mgr-env")
                                            .secondary()
                                            .label("管理环境")
                                            .on_click({
                                                let mgr = mgr.clone();
                                                let entity = entity.clone();
                                                let shared = shared.clone();
                                                move |_, window, app| {
                                                    open_manager(2, &mgr, entity.clone(), shared.clone(), window, app);
                                                }
                                            }),
                                    ),
                            )
                            .child(div().text_xs().text_color(theme.colors.info).child(env_summary)),
                    );
                    // 安全策略覆盖（自绘开关；覆盖环境默认后标记"已覆盖"）。
                    let sec_rows = {
                        let sec_outer = sec_overrides.clone();
                        let sec_ref = sec_overrides.borrow();
                        let mut rows = div().v_flex().gap_1();
                        for (i, item) in POLICY_ITEMS.iter().enumerate() {
                            let on = sec_ref.get(i).copied().unwrap_or(false);
                            let idx = i;
                            let sec_overrides = sec_outer.clone();
                            let entity = entity.clone();
                            rows = rows.child(
                                div().h_flex().items_center().gap_2()
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from(format!("sec-{idx}"))))
                                            .cursor_pointer()
                                            .w_8()
                                            .h(rems(1.125))
                                            .rounded_full()
                                            .bg(if on { theme.colors.primary } else { theme.colors.border })
                                            .relative()
                                            .on_click(move |_, _, app| {
                                                let mut s = sec_overrides.borrow_mut();
                                                if let Some(v) = s.get_mut(idx) { *v = !*v; }
                                                drop(s);
                                                entity.update(app, |_, cx| cx.notify());
                                            })
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .left_0()
                                                    .m_0p5()
                                                    .w_3p5()
                                                    .h_3p5()
                                                    .rounded_full()
                                                    .bg(theme.colors.background)
                                                    .child(""),
                                            ),
                                    )
                                    .child(div().text_xs().child(*item))
                                    .child(
                                        if on {
                                            div().text_xs().text_color(theme.colors.info).child("已覆盖")
                                        } else {
                                            div().text_xs().text_color(theme.colors.muted_foreground).child("默认")
                                        },
                                    ),
                            );
                        }
                        rows
                    };
                    content = content.child(
                        div().v_flex().gap_1()
                            .child(div().text_sm().font_weight(FontWeight::BOLD).child("安全策略（覆盖环境默认）"))
                            .child(sec_rows),
                    );
                    // DuckDB 本地加速（仅网络型库可见；原型 accel-card：warning 卡片）。
                    if is_network_db {
                        let mut accel = div()
                            .w_full()
                            .v_flex()
                            .gap(rems(0.75))
                            .rounded(px(10.))
                            .border_1()
                            .border_color(theme.colors.warning.opacity(0.45))
                            .bg(theme.colors.warning.opacity(0.08))
                            .py(rems(0.75))
                            .px(rems(0.875))
                            .child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(rems(0.625))
                                    .child(
                                        div()
                                            .v_flex()
                                            .gap(rems(0.125))
                                            .child(
                                                div()
                                                    .h_flex()
                                                    .items_center()
                                                    .gap(rems(0.5))
                                                    .child(
                                                        lucide("icons/database-zap.svg")
                                                            .size(px(15.))
                                                            .text_color(theme.colors.warning),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(FontWeight::BOLD)
                                                            .text_color(theme.colors.warning)
                                                            .child("DuckDB 本地加速（联邦查询直连源库）"),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.colors.muted_foreground)
                                                    .child("仅网络数据库可用 · 凭据注册为 DuckDB Secret · 不落明文"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("duckdb-fed")
                                            .cursor_pointer()
                                            .w_8()
                                            .h(rems(1.125))
                                            .rounded_full()
                                            .bg(if duckdb_fed.get() {
                                                theme.colors.primary
                                            } else {
                                                theme.colors.border
                                            })
                                            .relative()
                                            .on_click({
                                                let duckdb_fed = duckdb_fed.clone();
                                                let entity = entity.clone();
                                                move |_, _, app| {
                                                    duckdb_fed.set(!duckdb_fed.get());
                                                    entity.update(app, |_, cx| cx.notify());
                                                }
                                            })
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .left_0()
                                                    .m_0p5()
                                                    .w_3p5()
                                                    .h_3p5()
                                                    .rounded_full()
                                                    .bg(theme.colors.background)
                                                    .child(""),
                                            ),
                                    ),
                            );
                        if duckdb_fed.get() {
                            accel = accel
                                .child(
                                    div()
                                        .h_flex()
                                        .items_center()
                                        .gap(rems(0.5))
                                        .child(
                                            div()
                                                .w(rems(5.75))
                                                .flex_shrink_0()
                                                .text_xs()
                                                .text_color(theme.colors.muted_foreground)
                                                .child("缓存路径"),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .child(Input::new(&cache_path)),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child("已开启：凭据注册为 DuckDB Secret（不落明文），分析引擎可直接联邦查询；缓存上限 / 自动刷新 / 压缩由分析引擎默认策略管理"),
                                );
                        } else {
                            accel = accel.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child("关闭：联邦查询不可用"),
                            );
                        }
                        content = content.child(accel);
                    }
                    content
                }
                _ => {
                    // ===== 常规（对齐原型 3.1）：driver 信息条 + 三张 section 卡片 =====
                    let driver_value = driver_now.clone();
                    let url_value = url.read(cx).value().to_string();
                    let (host_v, port_v, db_v) =
                        crate::services::data_source_service::parse_url_host_port_db(
                            &driver_value,
                            &url_value,
                        );
                    let auth_ref_selected = auth_ref.read(cx).selected_value().cloned();
                    let is_auth_ref = auth_ref_selected.is_some();

                    // 顶部 driver 信息条（info-banner）。
                    let info_text = if driver_value.is_empty() {
                        "未选择驱动 · 请先选择驱动类型".to_string()
                    } else if is_file_db {
                        format!("driver: {driver_value} · 文件型数据库 · 无网络与 SSL 配置")
                    } else {
                        format!(
                            "driver: {driver_value} · 原生连接（sqlx）· 支持 password / ssh_key 认证"
                        )
                    };
                    let info_banner = div()
                        .w_full()
                        .h_flex()
                        .items_center()
                        .gap(rems(0.5))
                        .rounded(rems(0.5))
                        .border_1()
                        .border_color(theme.colors.info.opacity(0.28))
                        .bg(theme.colors.info.opacity(0.08))
                        .px(rems(0.75))
                        .py(rems(0.4375))
                        .text_xs()
                        .text_color(theme.colors.info)
                        .child(
                            Icon::new(IconName::Info)
                                .size(px(14.))
                                .text_color(theme.colors.info),
                        )
                        .child(info_text);

                    // 卡片 1：连接设置（只读摘要；编辑在顶部 URI 行）。
                    let settings_body = if is_file_db {
                        div().v_flex().gap(rems(0.375)).child(grid_row(
                            theme,
                            "数据库文件",
                            val_readonly(
                                theme,
                                if url_value.is_empty() { "-" } else { url_value.as_str() },
                            ),
                        ))
                    } else {
                        div()
                            .v_flex()
                            .gap(rems(0.375))
                            .child(grid_row(
                                theme,
                                "主机",
                                val_readonly(theme, host_v.as_deref().unwrap_or("-")),
                            ))
                            .child(grid_row(
                                theme,
                                "端口",
                                val_readonly(
                                    theme,
                                    &port_v
                                        .map(|p| p.to_string())
                                        .unwrap_or_else(|| "-".to_string()),
                                ),
                            ))
                            .child(grid_row(
                                theme,
                                "数据库",
                                val_readonly(theme, db_v.as_deref().unwrap_or("-")),
                            ))
                    };

                    // 卡片 2：数据库认证（引用已保存配置 + 管理入口）。
                    let auth_body = div()
                        .v_flex()
                        .gap(rems(0.375))
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap(rems(0.5))
                                .child(
                                    div()
                                        .w(rems(5.75))
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child("引用配置"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .child(Select::new(&auth_ref).placeholder("引用已保存配置…")),
                                )
                                .child(
                                    Button::new("mgr-auth")
                                        .ghost()
                                        .label("管理")
                                        .on_click({
                                            let mgr = mgr.clone();
                                            let entity = entity.clone();
                                            let shared = shared.clone();
                                            move |_, window, app| {
                                                open_manager(0, &mgr, entity.clone(), shared.clone(), window, app);
                                            }
                                        }),
                                ),
                        )
                        .child(if is_auth_ref {
                            reuse_note(
                                theme,
                                "已引用认证档案 · 用户名/密码只读（修改档案一处全量生效）",
                            )
                        } else {
                            div()
                                .v_flex()
                                .gap(rems(0.375))
                                .child(grid_row(theme, "用户名", Input::new(&user)))
                                .child(grid_row(theme, "密码", Input::new(&pass)))
                        });

                    // 卡片 3：连接安全（SSL/TLS）。
                    let ssl_selected = ssl_mode
                        .read(cx)
                        .selected_value()
                        .cloned()
                        .unwrap_or_default()
                        .to_string();
                    let ssl_body = div()
                        .v_flex()
                        .gap(rems(0.375))
                        .child(grid_row(
                            theme,
                            "模式",
                            Select::new(&ssl_mode).placeholder("选择 SSL 模式…"),
                        ))
                        .child(if ssl_selected == "disable" {
                            div()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("未启用 TLS（disable / prefer / require / verify-ca / verify-full）")
                        } else {
                            div()
                                .v_flex()
                                .gap(rems(0.375))
                                .child(grid_row(theme, "CA 证书", Input::new(&ssl_ca)))
                                .child(grid_row(theme, "客户端证书", Input::new(&ssl_cert)))
                                .child(grid_row(theme, "私钥", Input::new(&ssl_key)))
                        });

                    div()
                        .w_full()
                        .v_flex()
                        .gap(rems(0.75))
                        .child(info_banner)
                        .child(
                            div()
                                .w_full()
                                .h_flex()
                                .items_start()
                                .flex_wrap()
                                .gap(rems(0.75))
                                .child(sec_card(
                                    theme,
                                    lucide("icons/database.svg"),
                                    theme.colors.primary,
                                    "连接设置",
                                    settings_body,
                                ))
                                .child(sec_card(
                                    theme,
                                    lucide("icons/lock.svg"),
                                    theme.colors.primary,
                                    "数据库认证",
                                    auth_body,
                                ))
                                .child(sec_card(
                                    theme,
                                    lucide("icons/shield-check.svg"),
                                    theme.colors.info,
                                    "连接安全（SSL/TLS）",
                                    ssl_body,
                                )),
                        )
                }
            };

            // ---- 左侧栏（对齐原型 §2）：类型搜索 + 数据库类型分类树 ----
            let filter_text = driver_filter.read(cx).value().trim().to_lowercase();
            let mut db_tree = div().v_flex().gap_1();
            for (cat, cat_label) in [
                ("relational", "关系型"),
                ("file-based", "文件型"),
                ("analytics", "分析型"),
                ("nosql", "NoSQL"),
            ] {
                let matched: Vec<DataSourceType> = types_snapshot
                    .iter()
                    .filter(|t| t.category == cat)
                    .filter(|t| {
                        filter_text.is_empty()
                            || t.name.to_lowercase().contains(&filter_text)
                            || t.id.to_lowercase().contains(&filter_text)
                            || drivers_snapshot.iter().any(|d| {
                                d.type_id == t.id && d.name.to_lowercase().contains(&filter_text)
                            })
                    })
                    .cloned()
                    .collect();
                if matched.is_empty() {
                    continue;
                }
                db_tree = db_tree.child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.colors.muted_foreground)
                        .child(format!("{cat_label}（{}）", matched.len())),
                );
                for t in matched {
                    let on = selected_type_id == t.id;
                    let type_id = t.id.clone();
                    let type_id_cmp = t.id.clone();
                    let type_name = t.name.clone();
                    let mut row = div()
                        .id(ElementId::Name(SharedString::from(format!("type-{}", t.id))))
                        .h_flex()
                        .items_center()
                        .gap(rems(0.375))
                        .h(rems(1.75))
                        .px(rems(0.5))
                        .rounded(rems(0.375))
                        .cursor_pointer();
                    if on {
                        row = row.bg(theme.colors.sidebar_accent);
                    }
                    db_tree = db_tree.child(
                        row.child(
                            div()
                                .w(px(2.))
                                .h(rems(1.))
                                .rounded_full()
                                .bg(if on { theme.colors.primary } else { theme.colors.border }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(if on {
                                    theme.colors.foreground
                                } else {
                                    theme.colors.muted_foreground
                                })
                                .child(type_name),
                        )
                        .on_click({
                            let driver = driver.clone();
                            let entity = entity.clone();
                            let drivers_list = drivers_list.clone();
                            let selected_type = selected_type.clone();
                            move |_, window, app| {
                                *selected_type.borrow_mut() = type_id.clone();
                                // Header 驱动下拉联动到该类型下第一个启用驱动。
                                let first = drivers_list
                                    .borrow()
                                    .iter()
                                    .find(|d| d.type_id == type_id_cmp && d.enabled)
                                    .map(|d| d.name.clone());
                                if let Some(n) = first {
                                    driver.update(app, |s, cx| {
                                        s.set_selected_value(&SharedString::from(n), window, cx)
                                    });
                                }
                                entity.update(app, |_, cx| cx.notify());
                            }
                        }),
                    );
                }
            }
            // ---- 暂存列表（多连接连续编辑；原型设计 §2.2）：草稿 + 已保存条目 ----
            let drafts_snapshot: Vec<ConnectionDraft> = drafts_list.borrow().clone();
            let cursor_now = draft_cursor.get();
            let mut staging_list = div().v_flex().gap(rems(0.25));
            for (i, d) in drafts_snapshot.iter().enumerate() {
                let on = i == cursor_now;
                let is_saved = d.saved_id.is_some();
                let label = d.display_name();
                let mut row = div()
                    .id(ElementId::Name(SharedString::from(format!("draft-{i}"))))
                    .h_flex()
                    .items_center()
                    .gap(rems(0.375))
                    .h(rems(1.75))
                    .px(rems(0.5))
                    .rounded(rems(0.375))
                    .cursor_pointer();
                if on {
                    row = row.bg(theme.colors.sidebar_accent);
                }
                row = row
                    .child(
                        div()
                            .w(px(2.))
                            .h(rems(1.))
                            .rounded_full()
                            .bg(if on { theme.colors.primary } else { theme.colors.border }),
                    )
                    .child(
                        // `type-dot`（原型）：已保存 = 连接有效（success），草稿 = 待保存（primary）。
                        div()
                            .w(px(7.))
                            .h(px(7.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(if is_saved {
                                theme.colors.success
                            } else {
                                theme.colors.primary
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_xs()
                            .text_color(if on {
                                theme.colors.foreground
                            } else {
                                theme.colors.muted_foreground
                            })
                            .child(label),
                    );
                if !is_saved {
                    row = row.child(
                        div()
                            .id(ElementId::Name(SharedString::from(format!("draft-del-{i}"))))
                            .cursor_pointer()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("✕")
                            .on_click({
                                let state = state.clone();
                                let shared = shared.clone();
                                move |_, window, app| {
                                    state.staging_remove(i, window, app);
                                    shared.notify_host(app);
                                }
                            }),
                    );
                }
                staging_list = staging_list.child(row.on_click({
                    let state = state.clone();
                    let shared = shared.clone();
                    move |_, window, app| {
                        state.staging_select(i, window, app);
                        shared.notify_host(app);
                    }
                }));
            }
            let side_panel = div()
                .w(rems(12.5))
                .flex_shrink_0()
                .v_flex()
                .gap(rems(0.75))
                .pr(rems(0.75))
                .border_r_1()
                .border_color(theme.colors.border)
                .child(
                    Input::new(&driver_filter).prefix(
                        lucide("icons/search.svg")
                            .size(px(13.))
                            .text_color(theme.colors.muted_foreground),
                    ),
                )
                .child(
                    div()
                        .v_flex()
                        .gap(rems(0.375))
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.colors.muted_foreground)
                                        .child("暂存列表"),
                                )
                                .child(
                                    div()
                                        .id("staging-add")
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(theme.colors.primary)
                                        .child("+ 添加")
                                        .on_click({
                                            let state = state.clone();
                                            let shared = shared.clone();
                                            move |_, window, app| {
                                                state.staging_add(window, app);
                                                shared.notify_host(app);
                                            }
                                        }),
                                ),
                        )
                        .child(staging_list),
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.colors.muted_foreground)
                        .child("数据库类型"),
                )
                .child(db_tree);

            // ---- Header（对齐原型 §2）：名称 + 驱动类型 + 作用域 / 备注 / URI / 提示行 ----
            let scope_sel = scope.read(cx).selected_value().cloned().unwrap_or_default().to_string();
            let includes_project = scope_from_label(&scope_sel).includes_project();
            let header_ui = div()
                .v_flex()
                .gap(rems(0.5))
                .pb(rems(0.625))
                .border_b_1()
                .border_color(theme.colors.border)
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(rems(0.75))
                        .min_w(px(0.))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("名称"),
                        )
                        .child(Input::new(&name).w(rems(9.875)))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("驱动类型"),
                        )
                        .child(
                            div()
                                .w(rems(14.))
                                .flex_shrink_0()
                                .child(Select::new(&driver).placeholder("选择驱动…")),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("作用域"),
                        )
                        .child(
                            div()
                                .w(rems(10.))
                                .flex_shrink_0()
                                .child(Select::new(&scope).placeholder("选择作用域…")),
                        ),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(rems(0.75))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("备注"),
                        )
                        .child(Input::new(&remark).flex_1().max_w(rems(27.5))),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap(rems(0.75))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(theme.colors.muted_foreground)
                                .child("URI"),
                        )
                        .child(Input::new(&url).flex_1())
                        .child(if includes_project {
                            div()
                                .h_flex()
                                .flex_shrink_0()
                                .items_center()
                                .gap(rems(0.5))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.colors.muted_foreground)
                                        .child("项目路径"),
                                )
                                .child(Input::new(&project_path).w(rems(18.)))
                        } else {
                            div()
                        }),
                )
                .child(
                    // 作用域语义提示（原型 scope-hint 药丸）。
                    div()
                        .h_flex()
                        .items_center()
                        .gap(rems(0.375))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.colors.info)
                                .bg(theme.colors.info.opacity(0.08))
                                .rounded(rems(0.375))
                                .px(rems(0.5625))
                                .py(rems(0.1875))
                                .child(scope_from_label(&scope_sel).hint()),
                        ),
                );

            let result_ui = {
                let msg = result.borrow().clone();
                if let Some(msg) = msg {
                    let color = if result_ok.get() { theme.colors.success } else { theme.colors.danger };
                    div().text_xs().text_color(color).child(msg)
                } else {
                    div().h(rems(1.125))
                }
            };

            // footer：0.6 中为 `impl IntoElement`，直接传按钮容器。
            let footer_ui = div()
                .h_flex()
                .justify_end()
                .items_center()
                .gap_2()
                .child(
                    Button::new("test-connection")
                        .secondary()
                        .label("测试连接")
                        .on_click({
                            let name = name.clone();
                            let url = url.clone();
                            let user = user.clone();
                            let pass = pass.clone();
                            let driver = driver.clone();
                            let result = result.clone();
                            let result_ok = result_ok.clone();
                                                        let state = ConnectionDialogState::cloned_state(
                                name.clone(), url.clone(), user.clone(), pass.clone(), driver.clone(),
                                remark.clone(), auth_ref.clone(), network_ref.clone(), env.clone(),
                                auth_list.clone(), network_list.clone(), env_list.clone(),
                                duckdb_fed.clone(), cache_path.clone(), props.clone(),
                                hops.clone(), scope.clone(), ssl_mode.clone(), ssl_ca.clone(),
                                ssl_cert.clone(), ssl_key.clone(), sec_overrides.clone(),
                                drivers_list.clone(),
                            );
                            let run_test = run_test.clone();
                            let entity = entity.clone();
                            move |_, _, app| {
                                let Some(input) = state.collect(&name, &driver, &url, &user, &pass, app) else {
                                    *result.borrow_mut() = Some("请填写名称、驱动类型与连接 URL".into());
                                    result_ok.set(false);
                                    entity.update(app, |_, cx| cx.notify());
                                    return;
                                };
                                if let Err(e) = state.hops_valid(app) {
                                    *result.borrow_mut() = Some(e);
                                    result_ok.set(false);
                                    entity.update(app, |_, cx| cx.notify());
                                    return;
                                }
                                *result.borrow_mut() = Some("测试中…".into());
                                result_ok.set(true);
                                entity.update(app, |_, cx| cx.notify());
                                let (ok, msg) = run_test(input);
                                *result.borrow_mut() = Some(msg);
                                result_ok.set(ok);
                                entity.update(app, |_, cx| cx.notify());
                            }
                        }),
                )
                .child(
                    Button::new("cancel-connection")
                        .label("取消")
                        .on_click({
                            let shared = shared.clone();
                            let state = state.clone();
                            move |_, window, app| {
                                // 关闭前写回当前草稿（暂存列表不丢失，原型设计 §2.2 规则 5）
                                state.staging_flush(app);
                                window.close_dialog(app);
                                // 宿主重绘：层才会从元素树移除（Root 的 notify 到不了子视图）
                                shared.notify_host(app);
                            }
                        }),
                )
                .child(
                    Button::new("save-connection")
                        .primary()
                        .icon(IconName::Plus)
                        .label("保存")
                        .on_click({
                            let name = name.clone();
                            let url = url.clone();
                            let user = user.clone();
                            let pass = pass.clone();
                            let driver = driver.clone();
                            let result = result.clone();
                            let result_ok = result_ok.clone();
                                                        // 暂存列表需要对话框状态句柄（Rc）——在 `state`（ClonedDialogState）遮蔽前取出。
                            let dialog = Rc::clone(&state);
                            let state = ConnectionDialogState::cloned_state(
                                name.clone(), url.clone(), user.clone(), pass.clone(), driver.clone(),
                                remark.clone(), auth_ref.clone(), network_ref.clone(), env.clone(),
                                auth_list.clone(), network_list.clone(), env_list.clone(),
                                duckdb_fed.clone(), cache_path.clone(), props.clone(),
                                hops.clone(), scope.clone(), ssl_mode.clone(), ssl_ca.clone(),
                                ssl_cert.clone(), ssl_key.clone(), sec_overrides.clone(),
                                drivers_list.clone(),
                            );
                            let entity = entity.clone();
                            let shared = shared.clone();
                            let editing_id = editing_id.clone();
                            let project_path = project_path.clone();
                            move |_, window, app| {
                                let Some(input) = state.collect(&name, &driver, &url, &user, &pass, app) else {
                                    *result.borrow_mut() = Some("请填写名称、驱动类型与连接 URL".into());
                                    result_ok.set(false);
                                    entity.update(app, |_, cx| cx.notify());
                                    return;
                                };
                                if let Err(e) = state.hops_valid(app) {
                                    *result.borrow_mut() = Some(e);
                                    result_ok.set(false);
                                    entity.update(app, |_, cx| cx.notify());
                                    return;
                                }
                                // 编辑模式走 update（按 ID 前缀路由 G_/P_/GP_）；新建走 save（按作用域落库）。
                                let editing = editing_id.borrow().clone();
                                let project_path_val = {
                                    let v = project_path.read(app).value().to_string();
                                    if v.trim().is_empty() { None } else { Some(v.trim().to_string()) }
                                };
                                let save = DataSourceService::global().and_then(|service| {
                                    let rt = tokio::runtime::Runtime::new().map_err(|e| {
                                        shared::error::CoreError::common(
                                            shared::error::CommonError::General(format!("tokio: {e}")),
                                        )
                                    })?;
                                    match &editing {
                                        Some(cid) => rt
                                            .block_on(service.update(cid, &input, project_path_val.as_deref()))
                                            .map(|_| cid.clone()),
                                        None => rt.block_on(service.save(&input, project_path_val.as_deref())),
                                    }
                                });
                                match save {
                                    Ok(conn_id) => {
                                        // 刷新列表：带上当前项目根，项目侧 P_/GP_ 连接一并可见。
                                        let root = shared
                                            .project
                                            .borrow()
                                            .as_ref()
                                            .map(|p| p.root.clone());
                                        let (items, _) = crate::services::workspace_loader::load_connections_for_scope(root.as_deref());
                                        *shared.connections.borrow_mut() = items;
                                        *shared.notice.borrow_mut() =
                                            Some(format!("连接「{}」已保存（{}）", input.name, conn_id));
                                        // 暂存列表（原型设计 §2.2 规则 4）：草稿转正式 + 自动补空草稿；
                                        // 保存后保持对话框打开，支持连续编辑多个连接。
                                        dialog.staging_after_save(&conn_id, &input.name, window, app);
                                        *result.borrow_mut() =
                                            Some(format!("已保存：{conn_id}（已加入暂存列表，可继续新建）"));
                                        result_ok.set(true);
                                        // 宿主重绘：刷新层内容（暂存列表 + 连接列表）
                                        shared.notify_host(app);
                                    }
                                    Err(e) => {
                                        *result.borrow_mut() = Some(format!("保存失败: {e}"));
                                        result_ok.set(false);
                                    }
                                }
                                entity.update(app, |_, cx| cx.notify());
                            }
                        }),
                );

            dialog
                .title(if editing_id.borrow().is_some() { "编辑数据源连接" } else { "新建数据源连接" })
                .w(cx.theme().font_size * 61.25)
                .overlay(true)
                .overlay_closable(true)
                .keyboard(true)
                // Esc / 点遮罩等关闭路径：同样需宿主重绘才会移除层；关闭前写回草稿。
                .on_close({
                    let shared = shared.clone();
                    let state = state.clone();
                    move |_, _, app| {
                        state.staging_flush(app);
                        shared.notify_host(app)
                    }
                })
                .child(
                    div()
                        .h_flex()
                        .gap(rems(1.))
                        .child(side_panel)
                        .child(
                            div()
                                .v_flex()
                                .flex_1()
                                .min_w(px(0.))
                                .gap_2()
                                .child(header_ui)
                                .child(tab_bar)
                                .child(tab_content)
                                .child(result_ui),
                        ),
                )
                .footer(footer_ui)
        });
    }
}

/// 由受控 Entity 重建轻量视图（测试/保存按钮复用 collect / hops_valid）。
struct ClonedDialogState {
    remark: Entity<InputState>,
    auth_ref: Entity<SelectState<SearchableVec<SharedString>>>,
    network_ref: Entity<SelectState<SearchableVec<SharedString>>>,
    env: Entity<SelectState<SearchableVec<SharedString>>>,
    auth_list: Rc<RefCell<Vec<AuthConfig>>>,
    network_list: Rc<RefCell<Vec<NetworkConfig>>>,
    env_list: Rc<RefCell<Vec<Environment>>>,
    duckdb_fed: Rc<Cell<bool>>,
    cache_path: Entity<InputState>,
    props: Rc<RefCell<Vec<(String, String)>>>,
    hops: Rc<RefCell<Vec<Hop>>>,
    // ---- Phase C ----
    scope: Entity<SelectState<SearchableVec<SharedString>>>,
    ssl_mode: Entity<SelectState<SearchableVec<SharedString>>>,
    ssl_ca: Entity<InputState>,
    ssl_cert: Entity<InputState>,
    ssl_key: Entity<InputState>,
    sec_overrides: Rc<RefCell<Vec<bool>>>,
    /// 驱动目录（drivers 表）：将下拉显示的驱动名解析为驱动 id（db_type / driver_id 落库）。
    drivers: Rc<RefCell<Vec<Driver>>>,
}

impl ConnectionDialogState {
    fn cloned_state(
        _name: Entity<InputState>,
        _url: Entity<InputState>,
        _user: Entity<InputState>,
        _pass: Entity<InputState>,
        _driver: Entity<SelectState<SearchableVec<SharedString>>>,
        remark: Entity<InputState>,
        auth_ref: Entity<SelectState<SearchableVec<SharedString>>>,
        network_ref: Entity<SelectState<SearchableVec<SharedString>>>,
        env: Entity<SelectState<SearchableVec<SharedString>>>,
        auth_list: Rc<RefCell<Vec<AuthConfig>>>,
        network_list: Rc<RefCell<Vec<NetworkConfig>>>,
        env_list: Rc<RefCell<Vec<Environment>>>,
        duckdb_fed: Rc<Cell<bool>>,
        cache_path: Entity<InputState>,
        props: Rc<RefCell<Vec<(String, String)>>>,
        hops: Rc<RefCell<Vec<Hop>>>,
        scope: Entity<SelectState<SearchableVec<SharedString>>>,
        ssl_mode: Entity<SelectState<SearchableVec<SharedString>>>,
        ssl_ca: Entity<InputState>,
        ssl_cert: Entity<InputState>,
        ssl_key: Entity<InputState>,
        sec_overrides: Rc<RefCell<Vec<bool>>>,
        drivers: Rc<RefCell<Vec<Driver>>>,
    ) -> ClonedDialogState {
        ClonedDialogState {
            remark,
            auth_ref,
            network_ref,
            env,
            auth_list,
            network_list,
            env_list,
            duckdb_fed,
            cache_path,
            props,
            hops,
            scope,
            ssl_mode,
            ssl_ca,
            ssl_cert,
            ssl_key,
            sec_overrides,
            drivers,
        }
    }
}

impl ClonedDialogState {
    fn collect(
        &self,
        name: &Entity<InputState>,
        driver: &Entity<SelectState<SearchableVec<SharedString>>>,
        url: &Entity<InputState>,
        user: &Entity<InputState>,
        pass: &Entity<InputState>,
        cx: &mut App,
    ) -> Option<DataSourceSaveInput> {
        // db_type 承载驱动 id（引擎 registry key）；下拉显示的是驱动 name（drivers 表），此处反查。
        let driver_name = driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default();
        let selected_driver = self
            .drivers
            .borrow()
            .iter()
            .find(|d| d.name == driver_name.as_ref())
            .cloned();
        let db_type = selected_driver
            .as_ref()
            .map(|d| d.id.clone())
            .unwrap_or_else(|| driver_name.to_string());
        if db_type.is_empty() {
            return None;
        }
        let name = name.read(cx).value().to_string();
        let url = url.read(cx).value().to_string();
        if name.trim().is_empty() || url.trim().is_empty() {
            return None;
        }
        let username = {
            let v = user.read(cx).value().to_string();
            if v.trim().is_empty() {
                None
            } else {
                Some(v.trim().to_string())
            }
        };
        let password = {
            let v = pass.read(cx).value().to_string();
            if v.is_empty() { None } else { Some(v) }
        };
        let auth_config_id = self
            .auth_ref
            .read(cx)
            .selected_value()
            .cloned()
            .and_then(|n| {
                self.auth_list
                    .borrow()
                    .iter()
                    .find(|a| a.name.as_deref() == Some(n.as_str()))
                    .map(|a| a.id.clone())
            });
        let network_config_id = self
            .network_ref
            .read(cx)
            .selected_value()
            .cloned()
            .and_then(|n| {
                self.network_list
                    .borrow()
                    .iter()
                    .find(|c| c.name.as_deref() == Some(n.as_str()))
                    .map(|c| c.id.clone())
            });
        let environment_id = self.env.read(cx).selected_value().cloned().and_then(|n| {
            self.env_list
                .borrow()
                .iter()
                .find(|e| e.name == n.as_str())
                .map(|e| e.id.clone())
        });
        let driver_properties = if self.props.borrow().is_empty() {
            None
        } else {
            let map: std::collections::BTreeMap<String, String> =
                self.props.borrow().iter().cloned().collect();
            serde_json::to_string(&map).ok()
        };
        // DuckDB 缓存路径 → metadata_path 落库（非空时）。
        let metadata_path = {
            let v = self.cache_path.read(cx).value().to_string();
            if v.trim().is_empty() {
                None
            } else {
                Some(v.trim().to_string())
            }
        };
        // 作用域（UI 标签 → ConnectionScope）。
        let scope = scope_from_label(
            self.scope
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default()
                .as_str(),
        );
        // 高级选项组装：network_chain + ssl + policy_overrides 合并为一个 JSON。
        let mut adv = serde_json::Map::new();
        if network_config_id.is_none() {
            let hops = self.hops.borrow();
            if !hops.is_empty() {
                let chain: Vec<serde_json::Value> = hops
                    .iter()
                    .map(|h| {
                        serde_json::json!({ "kind": h.kind, "label": h.label, "enabled": h.enabled })
                    })
                    .collect();
                adv.insert("network_chain".into(), serde_json::Value::Array(chain));
            }
        }
        let ssl_mode = self
            .ssl_mode
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string();
        if !ssl_mode.is_empty() && ssl_mode != "disable" {
            let mut ssl = serde_json::Map::new();
            ssl.insert("mode".into(), serde_json::json!(ssl_mode));
            let ca = self.ssl_ca.read(cx).value().to_string();
            if !ca.trim().is_empty() {
                ssl.insert("ca".into(), serde_json::json!(ca.trim()));
            }
            let cert = self.ssl_cert.read(cx).value().to_string();
            if !cert.trim().is_empty() {
                ssl.insert("cert".into(), serde_json::json!(cert.trim()));
            }
            let key = self.ssl_key.read(cx).value().to_string();
            if !key.trim().is_empty() {
                ssl.insert("key".into(), serde_json::json!(key.trim()));
            }
            adv.insert("ssl".into(), serde_json::Value::Object(ssl));
        }
        let overrides: Vec<serde_json::Value> = self
            .sec_overrides
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, on)| **on)
            .filter_map(|(i, _)| POLICY_KEYS.get(i))
            .map(|k| serde_json::json!(k))
            .collect();
        if !overrides.is_empty() {
            adv.insert(
                "policy_overrides".into(),
                serde_json::Value::Array(overrides),
            );
        }
        let advanced_options = if adv.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(adv).to_string())
        };
        Some(DataSourceSaveInput {
            name: name.trim().to_string(),
            db_type: db_type.to_string(),
            url: url.trim().to_string(),
            username,
            password,
            scope,
            description: {
                let r = self.remark.read(cx).value().to_string();
                if r.trim().is_empty() {
                    None
                } else {
                    Some(r.trim().to_string())
                }
            },
            driver_id: selected_driver.as_ref().map(|d| d.id.clone()),
            environment_id,
            auth_config_id,
            auth_method: None,
            network_config_id,
            driver_properties,
            advanced_options,
            options: None,
            tags: None,
            use_duckdb_fed: Some(self.duckdb_fed.get()),
            schema_name: None,
            metadata_path,
        })
    }

    fn hops_valid(&self, cx: &mut App) -> Result<(), String> {
        if self.network_ref.read(cx).selected_value().is_some() {
            return Ok(());
        }
        Ok(())
    }
}

/// 打开管理器覆盖层（嵌套 Dialog；kind: 0 认证 / 1 网络 / 2 环境）。
/// 组件默认集（`IconName`）不含数据库 / 锁 / 盾牌等图标；应用注册的是全量
/// Lucide 目录（`gpui_kit::assets::AllAssets`），故按资产路径直接加载。
fn lucide(path: &'static str) -> Icon {
    Icon::empty().path(path)
}

/// 设置字符串型 Select 的选中值（空字符串 → 清空选中；暂存列表载入用）。
fn set_select_value(
    sel: &Entity<SelectState<SearchableVec<SharedString>>>,
    value: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let v = value.trim().to_string();
    sel.update(cx, |s, cx| {
        if v.is_empty() {
            s.set_selected_index(None, window, cx);
        } else {
            s.set_selected_value(&SharedString::from(v), window, cx);
        }
    });
}

/// 原型 `sec-card`：section 卡片（边框 + 圆角 + 图标标题 + 内容）。
fn sec_card(
    theme: &Theme,
    icon: Icon,
    icon_color: Hsla,
    title: &'static str,
    body: impl IntoElement,
) -> Div {
    div()
        .flex_1()
        .min_w(rems(14.75))
        .border_1()
        .border_color(theme.colors.border)
        .rounded(px(10.))
        .bg(theme.colors.background)
        .py(rems(0.75))
        .px(rems(0.875))
        .child(
            div()
                .h_flex()
                .items_center()
                .gap(rems(0.4375))
                .mb(rems(0.625))
                .child(Icon::new(icon).size(px(14.)).text_color(icon_color))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.colors.foreground)
                        .child(title),
                ),
        )
        .child(body)
}

/// 原型 `form-grid` 行：固定标签列（92px）+ 弹性值列。
fn grid_row(theme: &Theme, label: &'static str, value: impl IntoElement) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap(rems(0.75))
        .child(
            div()
                .w(rems(5.75))
                .flex_shrink_0()
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w(px(0.)).child(value))
}

/// 原型 `val2`：只读值框（常规 Tab 卡片内的摘要展示）。
fn val_readonly(theme: &Theme, text: &str) -> Div {
    let placeholder = text.is_empty() || text == "-";
    div()
        .border_1()
        .border_color(theme.colors.border)
        .rounded(rems(0.375))
        .bg(theme.colors.background)
        .px(rems(0.5625))
        .py(rems(0.25))
        .text_xs()
        .text_color(if placeholder {
            theme.colors.muted_foreground
        } else {
            theme.colors.foreground
        })
        .child(text.to_string())
}

/// 原型 `reuse-note`：复用说明行（小字 + 图标）。
fn reuse_note(theme: &Theme, text: &str) -> Div {
    div()
        .h_flex()
        .items_center()
        .gap(rems(0.375))
        .text_xs()
        .text_color(theme.colors.muted_foreground)
        .child(
            lucide("icons/settings.svg")
                .size(px(12.))
                .text_color(theme.colors.success),
        )
        .child(text.to_string())
}

fn open_manager(
    kind: usize,
    mgr: &Rc<RefCell<ManagerWorkspace>>,
    entity: Entity<crate::panels::EditorPanel>,
    shared: Shared,
    window: &mut Window,
    cx: &mut App,
) {
    let mgr = mgr.clone();
    let entity = entity.clone();
    let shared_for_layer = shared.clone();

    // 拉取列表（按 kind）。
    {
        let mut m = mgr.borrow_mut();
        m.kind = kind;
        m.editing = None;
        m.msg = None;
        m.items.clear();
        m.policy_env = None;
        m.policy_editing = None;
        m.policies.borrow_mut().clear();
    }
    refresh_manager_items(kind, &mgr, cx);

    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme();
        let shared_layer = shared_for_layer.clone();
        let m = mgr.borrow();
        let title = match kind {
            0 => "认证配置管理（AuthConfigManager）",
            1 => "网络配置管理（NetworkConfigManager）",
            _ => "环境管理（EnvironmentManager）",
        };

        // 列表行。
        let mut rows = div().v_flex().gap_1();
        if m.items.is_empty() {
            rows = rows.child(
                div().text_xs().text_color(theme.colors.muted_foreground).child("暂无配置，点击下方「新建」"),
            );
        }
        for (i, item) in m.items.iter().enumerate() {
            let item = item.clone();
            let idx = i;
            let mut row = div()
                .h_flex()
                .items_center()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(theme.colors.border)
                .px_2()
                .py_1()
                .child(div().text_sm().flex_1().child(item.clone()));
            if kind == 2 {
                row = row.child(
                    Button::new(format!("mgr-policy-{idx}"))
                        .secondary()
                        .label("策略")
                        .on_click({
                            let item = item.clone();
                            let mgr = mgr.clone();
                            let entity = entity.clone();
                            move |_, _window, app| {
                                {
                                    let mut m = mgr.borrow_mut();
                                    m.policy_env = Some(item.clone());
                                    m.policy_editing = None;
                                    m.policy_enabled.set(true);
                                }
                                refresh_policy_items(&item, &mgr, app);
                                let mut m = mgr.borrow_mut();
                                m.msg = Some(format!("环境「{item}」策略管理"));
                                drop(m);
                                entity.update(app, |_, cx| cx.notify());
                            }
                        }),
                );
            }
            rows = rows.child(row.child(
                Button::new(format!("mgr-edit-{idx}"))
                            .secondary()
                            .label("编辑")
                            .on_click({
                                let item = item.clone();
                                let mgr = mgr.clone();
                                let entity = entity.clone();
                                move |_, window, app| {
                                    let mut m = mgr.borrow_mut();
                                    m.editing = Some(item.clone());
                                    m.msg = Some(format!("编辑中：{item}（保存后更新既有条目）"));
                                    m.new_name.update(app, |s, cx| s.set_value(item.clone(), window, cx));
                                    drop(m);
                                    entity.update(app, |_, cx| cx.notify());
                                }
                            }),
                    )
                    .child(
                        Button::new(format!("mgr-del-{idx}"))
                            .secondary()
                            .label("删除")
                            .on_click({
                                let item = item.clone();
                                let mgr = mgr.clone();
                                let entity = entity.clone();
                                move |_, _window, app| {
                                    let deleted = delete_manager_item(kind, &item);
                                    let mut m = mgr.borrow_mut();
                                    m.msg = Some(deleted.clone());
                                    drop(m);
                                    if deleted.starts_with("已删除") || deleted.starts_with("删除成功") {
                                        refresh_manager_items(kind, &mgr, app);
                                    }
                                    entity.update(app, |_, cx| cx.notify());
                                }
                            }),
                    ),
            );
        }
        drop(m);

        // 新建/编辑表单。
        let mgr_form = {
            let m = mgr.borrow();
            let new_name = m.new_name.clone();
            let new_type = m.new_type.clone();
            let new_data = m.new_data.clone();
            let editing = m.editing.clone();
            let msg = m.msg.clone();
            drop(m);
            let mgr = mgr.clone();
            let entity = entity.clone();
            div()
                .v_flex()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(theme.colors.border)
                .px_3()
                .py_2()
                .child(
                    div().text_sm().font_weight(FontWeight::BOLD)
                        .child(if editing.is_some() { "编辑配置" } else { "新建配置" }),
                )
                .child(
                    div().h_flex().items_center().gap_2()
                        .child(div().text_xs().child("名称"))
                        .child(Input::new(&new_name).w(rems(11.25))),
                )
                .child(
                    div().h_flex().items_center().gap_2()
                        .child(div().text_xs().child("类型"))
                        .child(Select::new(&new_type).placeholder("选择类型…"))
                        .child(div().text_xs().child("数据(JSON)"))
                        .child(Input::new(&new_data).w(rems(12.5))),
                )
                .child(
                    div().h_flex().items_center().gap_2()
                        .child(
                            Button::new("mgr-save")
                                .primary()
                                .label("保存")
                                .on_click({
                                    let mgr = mgr.clone();
                                    let entity = entity.clone();
                                    move |_, window, app| {
                                        let (name, tpe, data, editing) = {
                                            let m = mgr.borrow();
                                            (
                                                m.new_name.read(app).value().to_string(),
                                                m.new_type.read(app).selected_value().cloned().unwrap_or_default().to_string(),
                                                m.new_data.read(app).value().to_string(),
                                                m.editing.clone(),
                                            )
                                        };
                                        if name.trim().is_empty() {
                                            mgr.borrow_mut().msg = Some("名称不能为空".into());
                                            entity.update(app, |_, cx| cx.notify());
                                            return;
                                        }
                                        let outcome = upsert_manager_item(kind, &name, &tpe, &data, editing.as_deref());
                                        let mut m = mgr.borrow_mut();
                                        m.msg = Some(outcome.clone());
                                        if outcome.starts_with("已保存") || outcome.starts_with("保存成功") {
                                            m.editing = None;
                                            m.new_name.update(app, |s, cx| s.set_value("", window, cx));
                                            m.new_data.update(app, |s, cx| s.set_value("", window, cx));
                                        }
                                        drop(m);
                                        refresh_manager_items(kind, &mgr, app);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        )
                        .child(
                            Button::new("mgr-cancel-edit")
                                .label("取消编辑")
                                .on_click({
                                    let mgr = mgr.clone();
                                    let entity = entity.clone();
                                    move |_, window, app| {
                                        let mut m = mgr.borrow_mut();
                                        m.editing = None;
                                        m.msg = None;
                                        m.new_name.update(app, |s, cx| s.set_value("", window, cx));
                                        m.new_data.update(app, |s, cx| s.set_value("", window, cx));
                                        drop(m);
                                        entity.update(app, |_, cx| cx.notify());
                                    }
                                }),
                        ),
                )
                .child(
                    if let Some(msg) = msg {
                        div().text_xs().text_color(theme.colors.info).child(msg)
                    } else {
                        div().h(rems(1.125))
                    },
                )
        };

        // 策略管理面板（仅环境管理器且已点开某环境时显示）。
        let policy_panel = {
            if kind == 2 {
                let m = mgr.borrow();
                let policy_env = m.policy_env.clone();
                let policy_type = m.policy_type.clone();
                let policy_enabled = m.policy_enabled.clone();
                let policies = m.policies.clone();
                let policy_editing = m.policy_editing.clone();
                drop(m);
                match policy_env {
                    Some(env_name) => {
                        let mut plist = div().v_flex().gap_1();
                        let list = policies.borrow();
                        if list.is_empty() {
                            plist = plist.child(
                                div().text_xs().text_color(theme.colors.muted_foreground).child("该环境暂无策略"),
                            );
                        }
                        for (i, (ptype, pid, enabled)) in list.iter().enumerate() {
                            let (ptype, pid, enabled) = (ptype.clone(), pid.clone(), *enabled);
                            plist = plist.child(
                                div()
                                    .h_flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(theme.colors.border)
                                    .px_2()
                                    .py_1()
                                    .child(div().text_sm().flex_1().child(ptype.clone()))
                                    .child(
                                        div().text_xs()
                                            .text_color(if enabled { theme.colors.success } else { theme.colors.muted_foreground })
                                            .child(if enabled { "启用" } else { "停用" }),
                                    )
                                    .child(
                                        Button::new(format!("pol-del-{i}"))
                                            .secondary()
                                            .label("删除")
                                            .on_click({
                                                let pid = pid.clone();
                                                let mgr = mgr.clone();
                                                let entity = entity.clone();
                                                move |_, _window, app| {
                                                    let outcome = delete_policy_item(&pid);
                                                    let env = mgr.borrow().policy_env.clone();
                                                    {
                                                        let mut m = mgr.borrow_mut();
                                                        m.msg = Some(outcome);
                                                    }
                                                    if let Some(e) = env {
                                                        refresh_policy_items(&e, &mgr, app);
                                                    }
                                                    entity.update(app, |_, cx| cx.notify());
                                                }
                                            }),
                                    ),
                            );
                        }
                        drop(list);
                        let env_name2 = env_name.clone();
                        let mgr2 = mgr.clone();
                        let entity2 = entity.clone();
                        div()
                            .v_flex()
                            .gap_2()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.colors.border)
                            .px_3()
                            .py_2()
                            .child(
                                div().text_sm().font_weight(FontWeight::BOLD)
                                    .child(format!("策略管理：{}", env_name.clone())),
                            )
                            .child(plist)
                            .child(
                                div().h_flex().items_center().gap_2()
                                    .child(div().text_xs().child("类型"))
                                    .child(Select::new(&policy_type).placeholder("选择策略类型…"))
                                    .child(div().text_xs().child("启用"))
                                    .child(
                                        div()
                                            .id("pol-toggle")
                                            .cursor_pointer()
                                            .relative()
                                            .w(rems(1.875))
                                            .h_4()
                                            .rounded_full()
                                            .bg(if policy_enabled.get() { theme.colors.primary } else { theme.colors.border })
                                            .on_click({
                                                let policy_enabled = policy_enabled.clone();
                                                let entity = entity.clone();
                                                move |_, _, app| {
                                                    policy_enabled.set(!policy_enabled.get());
                                                    entity.update(app, |_, cx| cx.notify());
                                                }
                                            })
                                            .child(
                                                div()
                                                    .absolute()
                                                    .top_0()
                                                    .left_0()
                                                    .m_0p5()
                                                    .w_3()
                                                    .h_3()
                                                    .rounded_full()
                                                    .bg(theme.colors.background)
                                                    .child(""),
                                            ),
                                    )
                                    .child(
                                        Button::new("pol-save")
                                            .primary()
                                            .label(if policy_editing.is_some() { "更新策略" } else { "添加策略" })
                                            .on_click({
                                                let env_name = env_name2.clone();
                                                let mgr = mgr2.clone();
                                                let entity = entity2.clone();
                                                move |_, _window, app| {
                                                    let (label, enabled, editing) = {
                                                        let m = mgr.borrow();
                                                        (
                                                            m.policy_type.read(app).selected_value().cloned().unwrap_or_default().to_string(),
                                                            m.policy_enabled.get(),
                                                            m.policy_editing.clone(),
                                                        )
                                                    };
                                                    if label.is_empty() {
                                                        mgr.borrow_mut().msg = Some("请选择策略类型".into());
                                                        entity.update(app, |_, cx| cx.notify());
                                                        return;
                                                    }
                                                    let outcome = upsert_policy_item(&env_name, &label, enabled, editing.as_deref());
                                                    let ok = outcome.starts_with("已保存") || outcome.starts_with("保存成功");
                                                    let mut m = mgr.borrow_mut();
                                                    m.msg = Some(outcome);
                                                    if ok {
                                                        m.policy_editing = None;
                                                        m.policy_enabled.set(true);
                                                    }
                                                    drop(m);
                                                    refresh_policy_items(&env_name, &mgr, app);
                                                    entity.update(app, |_, cx| cx.notify());
                                                }
                                            }),
                                    ),
                            )
                    }
                    None => div().h_0(),
                }
            } else {
                div().h_0()
            }
        };

        dialog
            .title(title)
            .w(cx.theme().font_size * 35.)
            .overlay(true)
            .overlay_closable(true)
            .keyboard(true)
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .child(rows)
                    .child(policy_panel)
                    .child(mgr_form)
                    .child(
                        div().text_xs().text_color(theme.colors.muted_foreground)
                            .child(if kind == 0 {
                                "复用：数据库 A 可直接引用数据库 B 的认证配置；凭据 AES-256 加密存储，列表不回显明文。"
                            } else if kind == 1 {
                                "复用：网络配置（SSH/代理档案）可被多个连接引用，建一次全项目/全局复用。"
                            } else {
                                "复用：环境（含策略）可被多个数据源共享；点击「策略」直接管理该环境的策略。"
                            }),
                    ),
            )
            .footer(
                div().h_flex().justify_end().gap_2()
                    .child(
                        Button::new("mgr-close")
                            .label("关闭")
                            .on_click({
                                let shared = shared_layer.clone();
                                move |_, window, app| {
                                    window.close_dialog(app);
                                    // 宿主重绘：移除内层对话框
                                    shared.notify_host(app);
                                }
                            }),
                    ),
            )
            .on_close({
                let shared = shared_layer.clone();
                move |_, _, app| shared.notify_host(app)
            })
    });
    // 宿主重绘：内层（管理器）对话框由宿主 render 渲染，打开后需重建元素树。
    shared.notify_host(cx);
}

/// 刷新管理器列表（block_on stores）。
fn refresh_manager_items(kind: usize, mgr: &Rc<RefCell<ManagerWorkspace>>, cx: &mut App) {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let service = match DataSourceService::global() {
        Ok(s) => s,
        Err(_) => return,
    };
    let names: Vec<String> = match kind {
        0 => rt
            .block_on(service.list_auth_configs())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| a.name)
            .collect(),
        1 => rt
            .block_on(service.list_network_configs())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|n| n.name)
            .collect(),
        _ => rt
            .block_on(service.list_environments())
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.name)
            .collect(),
    };
    mgr.borrow_mut().items = names;
    let _ = cx;
}

/// 新建/更新配置（block_on store CRUD；auth_data 密文由 store 内部处理）。
fn upsert_manager_item(
    kind: usize,
    name: &str,
    tpe: &str,
    data: &str,
    editing: Option<&str>,
) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
    };
    let service = match DataSourceService::global() {
        Ok(s) => s,
        Err(e) => return format!("服务未就绪: {e}"),
    };
    let _ = service;
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };

    let id = match kind {
        0 => engine::persistence::id_prefix::generate_gid("auth", name),
        1 => engine::persistence::id_prefix::generate_gid("net", name),
        _ => engine::persistence::id_prefix::generate_gid("env", name),
    };

    let outcome = rt.block_on(async {
        match kind {
            0 => {
                let config = AuthConfig {
                    id,
                    name: Some(name.to_string()),
                    auth_type: if tpe.is_empty() {
                        "password".into()
                    } else {
                        tpe.into()
                    },
                    auth_data: if data.is_empty() {
                        "{}".into()
                    } else {
                        data.into()
                    },
                    origin: None,
                    source_id: None,
                    snapshot_at: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                };
                if editing.is_some() {
                    db.update_auth_config(&config).await
                } else {
                    db.create_auth_config(&config).await
                }
            }
            1 => {
                let config = NetworkConfig {
                    id,
                    name: Some(name.to_string()),
                    network_type: if tpe.is_empty() {
                        "SSH".into()
                    } else {
                        tpe.into()
                    },
                    config: if data.is_empty() {
                        "{}".into()
                    } else {
                        data.into()
                    },
                    auth_config_id: None,
                    origin: None,
                    source_id: None,
                    snapshot_at: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                };
                if editing.is_some() {
                    db.update_network_config(&config).await
                } else {
                    db.create_network_config(&config).await
                }
            }
            _ => {
                let env = Environment {
                    id,
                    name: name.to_string(),
                    description: Some(if data.is_empty() {
                        "新建环境".into()
                    } else {
                        data.into()
                    }),
                    color: None,
                    sort_order: 0,
                    origin: None,
                    source_id: None,
                    snapshot_at: None,
                    created_at: String::new(),
                };
                if editing.is_some() {
                    db.update_environment(&env).await
                } else {
                    db.create_environment(&env).await
                }
            }
        }
    });

    match outcome {
        Ok(()) => "保存成功".to_string(),
        Err(e) => format!("保存失败: {e}"),
    }
}

/// 删除配置（按名称查 ID → store delete）。
fn delete_manager_item(kind: usize, name: &str) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
    };
    let service = match DataSourceService::global() {
        Ok(s) => s,
        Err(e) => return format!("服务未就绪: {e}"),
    };
    let _ = service;
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };

    let outcome = rt.block_on(async {
        match kind {
            0 => {
                let items = db.list_auth_configs(None).await?;
                let id = items
                    .iter()
                    .find(|a| a.name.as_deref() == Some(name))
                    .map(|a| a.id.clone());
                match id {
                    Some(id) => db.delete_auth_config(&id).await,
                    None => Ok(()), // 不存在视为已删除
                }
            }
            1 => {
                let items = db.list_network_configs(None).await?;
                let id = items
                    .iter()
                    .find(|n| n.name.as_deref() == Some(name))
                    .map(|n| n.id.clone());
                match id {
                    Some(id) => db.delete_network_config(&id).await,
                    None => Ok(()),
                }
            }
            _ => {
                let items = db.list_environments().await?;
                let id = items.iter().find(|e| e.name == name).map(|e| e.id.clone());
                match id {
                    Some(id) => db.delete_environment(&id).await,
                    None => Ok(()),
                }
            }
        }
    });

    match outcome {
        Ok(()) => format!("已删除「{name}」"),
        Err(e) => format!("删除失败: {e}"),
    }
}

/// 刷新指定环境的策略摘要（(类型标签, ID, 启用)）。
fn refresh_policy_items(env_name: &str, mgr: &Rc<RefCell<ManagerWorkspace>>, cx: &mut App) {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return,
    };
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else { return };
    let entries = rt
        .block_on(async {
            let envs = db.list_environments().await?;
            let Some(env) = envs.iter().find(|e| e.name == env_name) else {
                return Ok::<_, shared::error::CoreError>(Vec::new());
            };
            let ps = db.list_environment_policies(&env.id).await?;
            Ok::<_, shared::error::CoreError>(
                ps.iter()
                    .map(|p| {
                        let pos = POLICY_KEYS
                            .iter()
                            .position(|k| k == &p.policy_type)
                            .unwrap_or(0);
                        let label = POLICY_ITEMS
                            .get(pos)
                            .copied()
                            .unwrap_or(&p.policy_type)
                            .to_string();
                        (label, p.id.clone(), p.enabled)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default();
    *mgr.borrow_mut().policies.borrow_mut() = entries;
    let _ = cx;
}

/// 新建/更新环境策略（按环境名定位 environment_id；类型标签 → POLICY_KEYS 落库）。
fn upsert_policy_item(env_name: &str, label: &str, enabled: bool, editing: Option<&str>) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
    };
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };
    let pos = POLICY_ITEMS.iter().position(|p| *p == label).unwrap_or(0);
    let ptype = POLICY_KEYS
        .get(pos)
        .copied()
        .unwrap_or("read_only")
        .to_string();

    let outcome = rt.block_on(async {
        use shared::error::CommonError;
        let envs = db.list_environments().await?;
        let env_id = envs
            .iter()
            .find(|e| e.name == env_name)
            .map(|e| e.id.clone())
            .ok_or_else(|| {
                shared::error::CoreError::common(CommonError::General(format!(
                    "环境「{env_name}」不存在"
                )))
            })?;
        let id = match editing {
            Some(pid) => pid.to_string(),
            None => engine::persistence::id_prefix::generate_gid(
                "pol",
                &format!("{}_{}", env_name, ptype),
            ),
        };
        let policy = engine::persistence::env_store::EnvironmentPolicy {
            id,
            environment_id: env_id,
            policy_type: ptype,
            policy_config: None,
            enabled,
            created_at: String::new(),
        };
        if editing.is_some() {
            db.update_environment_policy(&policy).await
        } else {
            db.create_environment_policy(&policy).await
        }
    });

    match outcome {
        Ok(()) => format!(
            "保存成功：{label}（{}）",
            if enabled { "启用" } else { "停用" }
        ),
        Err(e) => format!("保存失败: {e}"),
    }
}

/// 删除环境策略。
fn delete_policy_item(id: &str) -> String {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return format!("运行时错误: {e}"),
    };
    let db = engine::migration::global_init::get_global_db_manager();
    let Some(db) = db else {
        return "全局库未初始化".into();
    };
    match rt.block_on(db.delete_environment_policy(id)) {
        Ok(()) => "已删除策略".to_string(),
        Err(e) => format!("删除失败: {e}"),
    }
}

/// 重建编辑回读 URL（DataSource 无 url 字段，由 host/port/database 重拼）。
fn reconstruct_url(ds: &DataSource) -> String {
    if matches!(ds.db_type.as_str(), "sqlite" | "duckdb") {
        return format!(
            "{}:///{}",
            ds.db_type,
            ds.database.clone().unwrap_or_default()
        );
    }
    let auth = ds
        .username
        .as_deref()
        .map(|u| format!("{}@", u))
        .unwrap_or_default();
    match (&ds.host, ds.port) {
        (Some(h), Some(p)) => format!(
            "{}://{}{}:{}/{}",
            ds.db_type,
            auth,
            h,
            p,
            ds.database.clone().unwrap_or_default()
        ),
        (Some(h), None) => format!(
            "{}://{}{}/{}",
            ds.db_type,
            auth,
            h,
            ds.database.clone().unwrap_or_default()
        ),
        _ => format!(
            "{}://{}{}",
            ds.db_type,
            ds.database.clone().unwrap_or_default(),
            ""
        ),
    }
}

/// 作用域标签（UI ↔ ConnectionScope）。
fn scope_label(s: &ConnectionScope) -> &'static str {
    match s {
        ConnectionScope::Global => "仅全局",
        ConnectionScope::Project => "仅项目",
        ConnectionScope::GlobalAndProject => "全局+项目",
    }
}

fn scope_from_label(l: &str) -> ConnectionScope {
    match l {
        "仅项目" => ConnectionScope::Project,
        "全局+项目" => ConnectionScope::GlobalAndProject,
        _ => ConnectionScope::Global,
    }
}
