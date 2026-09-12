use super::*;

use engine::persistence::connection_draft_store::{ConnectionDraftRow, ConnectionDraftStore};

/// 已保存条目的来源短码（暂存列表徽标）：P 仅项目 / G 仅全局 / GP 全局快照到项目。
///
/// 仅在 `GP_` 之前判定 `G_` 会误判，故按最长前缀优先。
pub(crate) fn saved_scope_short(id: &str) -> Option<&'static str> {
    if id.starts_with("GP_") {
        Some("GP")
    } else if id.starts_with("G_") {
        Some("G")
    } else if id.starts_with("P_") {
        Some("P")
    } else {
        None
    }
}

/// 模板格式标识（防误粘贴；导入时校验）。
pub const TEMPLATE_KIND: &str = "rds.connection.template";
/// 模板格式版本（字段演进时递增；导入拒绝高于当前版本）。
pub const TEMPLATE_VERSION: u32 = 1;

/// 连接模板（C4 导入导出；**不含密码**，与草稿快照同源）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConnectionTemplate {
    /// 格式标识（固定 [`TEMPLATE_KIND`]）。
    pub kind: String,
    /// 格式版本。
    pub version: u32,
    /// 连接条目。
    pub connections: Vec<TemplateItem>,
}

/// 模板中的单条连接（字段为创建连接所需的最小集；凭据一律不包含）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TemplateItem {
    pub name: String,
    #[serde(default)]
    pub type_id: String,
    #[serde(default)]
    pub driver_id: String,
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub remark: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub project_path: String,
    #[serde(default)]
    pub ssl_mode: String,
    #[serde(default)]
    pub tags: String,
    #[serde(default)]
    pub duckdb_fed: bool,
}

/// 暂存列表光标位条目的显示视图（见 `live_entry_view`）。
///
/// 只保留展示需要的字段，避免为脏标记每帧构造整份 `ConnectionDraft`。
pub struct LiveEntryView {
    /// 表单当前名称（未回写快照）。
    pub name: String,
    /// 表单当前数据库类型（类型徽标来源；未选类型为空串）。
    pub type_id: String,
    /// 是否有未回写草稿的表单修改（仅未保存草稿会为 true）。
    pub dirty: bool,
}

/// 暂存列表条目（多连接连续编辑；见原型设计 §2.2）。
///
/// 约束：仅存进程内存（关闭对话框不丢失、应用退出清除），不落盘、不写明文凭据；
/// `saved_id` 为 `Some` 时表示该条目对应一条已保存连接（点击走编辑回读）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConnectionDraft {
    /// 显示名称（空 → 列表显示「新建数据源」）。
    pub name: String,
    /// 已保存连接 ID（Some = 已保存条目）。
    pub saved_id: Option<String>,
    // ---- 表单快照（与 Header / 各 Tab 字段一一对应）----
    /// 数据库类型 id（侧栏选择；条目类型徽标与驱动过滤依据）。
    pub type_id: String,
    /// 驱动 id（drivers.id）：落库与回读最稳的驱动标识（如 mysql_native）。
    pub driver_id: String,
    /// 驱动实现短名（Header 下拉显示值，如 sqlx）。
    pub driver_name: String,
    pub url: String,
    pub user: String,
    pub pass: String,
    pub remark: String,
    /// 标签文本（逗号分隔；与表单一致，保存时解析为 JSON）。
    pub tags: String,
    /// 已勾选的项目分组 id（保存时以替换语义同步成员关系）。
    pub groups: Vec<String>,
    pub scope: String,
    pub project_path: String,
    pub ssl_mode: String,
    pub ssl_ca: String,
    pub ssl_cert: String,
    pub ssl_key: String,
    pub cache_path: String,
    pub duckdb_fed: bool,
    pub active_tab: usize,
    pub props: Vec<(String, String)>,
    /// 策略覆盖勾选（存 `environment_policies.policy_type`；旧草稿的布尔数组解析失败即忽略；
    /// 数据库列名仍为 `sec_overrides_json`，保留以兼容既有迁移）。
    pub sec_overrides: Vec<String>,
    /// 认证方法（`drivers.supported_auth_types` 中的键；空 = 未选）。
    pub auth_method: String,
    pub auth_ref: Option<String>,
    pub network_ref: Option<String>,
    pub env: Option<String>,
}

impl ConnectionDraft {
    /// 新建空草稿（默认值与对话框初始状态一致）。
    pub fn empty() -> Self {
        Self {
            duckdb_fed: true,
            sec_overrides: Vec::new(),
            ..Default::default()
        }
    }

    /// 列表显示名（空草稿回退「新建数据源」）。
    pub(crate) fn display_name(&self) -> String {
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

    /// 是否为空草稿（未保存且无任何有效字段）：导出模板时跳过。
    pub(crate) fn is_empty_draft(&self) -> bool {
        self.saved_id.is_none()
            && self.name.trim().is_empty()
            && self.url.trim().is_empty()
            && self.driver_id.trim().is_empty()
    }
}



impl ConnectionDialogState {
    // ===== 暂存列表（多连接连续编辑；原型设计 §2.2）=====

    /// UI 条目 → 持久化行（**不写密码**：记录结构本身无 password 字段）。
    fn draft_to_row(d: &ConnectionDraft) -> ConnectionDraftRow {
        ConnectionDraftRow {
            name: d.name.clone(),
            type_id: d.type_id.clone(),
            driver_id: d.driver_id.clone(),
            saved_id: d.saved_id.clone(),
            driver_name: d.driver_name.clone(),
            url: d.url.clone(),
            username: d.user.clone(),
            remark: d.remark.clone(),
            tags: d.tags.clone(),
            groups_json: serde_json::to_string(&d.groups).unwrap_or_else(|_| "[]".into()),
            scope: d.scope.clone(),
            project_path: d.project_path.clone(),
            ssl_mode: d.ssl_mode.clone(),
            ssl_ca: d.ssl_ca.clone(),
            ssl_cert: d.ssl_cert.clone(),
            ssl_key: d.ssl_key.clone(),
            cache_path: d.cache_path.clone(),
            duckdb_fed: d.duckdb_fed,
            active_tab: d.active_tab as i64,
            // 内联协议链已撤下（#25）：列保留（不迁移 schema），固定写空数组。
            hops_json: "[]".into(),
            props_json: serde_json::to_string(&d.props).unwrap_or_else(|_| "[]".into()),
            auth_method: d.auth_method.clone(),
            sec_overrides_json: serde_json::to_string(&d.sec_overrides)
                .unwrap_or_else(|_| "[]".into()),
            auth_ref: d.auth_ref.clone(),
            network_ref: d.network_ref.clone(),
            env: d.env.clone(),
        }
    }

    /// 持久化行 → UI 条目（JSON 解析失败时回退空值，保证列表可用）。
    fn row_to_draft(r: &ConnectionDraftRow) -> ConnectionDraft {
        ConnectionDraft {
            name: r.name.clone(),
            saved_id: r.saved_id.clone(),
            type_id: r.type_id.clone(),
            driver_id: r.driver_id.clone(),
            driver_name: r.driver_name.clone(),
            url: r.url.clone(),
            user: r.username.clone(),
            pass: String::new(),
            remark: r.remark.clone(),
            tags: r.tags.clone(),
            groups: serde_json::from_str(&r.groups_json).unwrap_or_default(),
            scope: r.scope.clone(),
            project_path: r.project_path.clone(),
            ssl_mode: r.ssl_mode.clone(),
            ssl_ca: r.ssl_ca.clone(),
            ssl_cert: r.ssl_cert.clone(),
            ssl_key: r.ssl_key.clone(),
            cache_path: r.cache_path.clone(),
            duckdb_fed: r.duckdb_fed,
            active_tab: r.active_tab.max(0) as usize,
            // `r.hops_json` 列保留但不再回填（旧草稿里的占位链直接忽略）。
            props: serde_json::from_str(&r.props_json).unwrap_or_default(),
            auth_method: r.auth_method.clone(),
            sec_overrides: serde_json::from_str(&r.sec_overrides_json).unwrap_or_default(),
            auth_ref: r.auth_ref.clone(),
            network_ref: r.network_ref.clone(),
            env: r.env.clone(),
        }
    }

    /// 当前条目索引（越界时回归 0）。
    pub(crate) fn draft_cursor_idx(&self) -> usize {
        let len = self.drafts.borrow().len();
        if len == 0 {
            0
        } else {
            self.draft_cursor.get().min(len - 1)
        }
    }

    /// 读取当前表单生成快照（不落暂存列表；脏标记与写回共用）。
    ///
    /// 公开给窗口测试做“两套比对等价性”基准（生产调用点都在本 crate）。
    pub fn snapshot_form(&self, cx: &App) -> ConnectionDraft {
        // 驱动：下拉显示短名；同时记录驱动 id（落库/恢复更稳）。
        let driver_value = self
            .driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default()
            .to_string();
        let driver_id = self
            .resolve_driver(&driver_value)
            .map(|d| d.id)
            .unwrap_or_default();
        ConnectionDraft {
            name: self.name.read(cx).value().to_string(),
            saved_id: None,
            type_id: self.selected_type.borrow().clone(),
            driver_id,
            driver_name: driver_value,
            url: self.url.read(cx).value().to_string(),
            user: self.user.read(cx).value().to_string(),
            pass: self.pass.read(cx).value().to_string(),
            remark: self.remark.read(cx).value().to_string(),
            tags: self.tags_input.read(cx).value().to_string(),
            groups: self
                .group_checks
                .borrow()
                .iter()
                .filter(|(_, _, checked)| *checked)
                .map(|(gid, _, _)| gid.clone())
                .collect(),
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
            props: self.props.borrow().clone(),
            sec_overrides: self.policy_override_keys.borrow().clone(),
            auth_method: self
                .auth_method
                .read(cx)
                .selected_value()
                .cloned()
                .map(|v| v.to_string())
                .unwrap_or_default(),
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
        }
    }

    /// 暂存列表「光标位条目」的显示视图：显示名 / 类型徽标来源 / 脏标记。
    ///
    /// 只取需要展示的字段，**不构造整份 `ConnectionDraft`**（§6 决策 #73）；
    /// 光标位不存在 → `None`（与旧行为一致）。
    pub fn live_entry_view(&self, cursor: usize, cx: &App) -> Option<LiveEntryView> {
        let drafts = self.drafts.borrow();
        let d = drafts.get(cursor)?;
        let name = self.name.read(cx).value().to_string();
        let type_id = self.selected_type.borrow().clone();
        // 脏标记仅对未保存草稿有意义（已保存条目不参与暂存编辑）。
        let dirty = d.saved_id.is_none() && !self.form_matches_draft(d, cx);
        Some(LiveEntryView {
            name,
            type_id,
            dirty,
        })
    }

    /// 当前表单是否与给定草稿一致（脏标记用；逐字段比较，**不分配**）。
    ///
    /// 关键：`InputState::value()` 返回 `SharedString`（引用计数克隆），因此逐字段比较
    /// 只有引用计数开销，而 [`Self::snapshot_form`] 要分配 ~40 个字符串 + 克隆两个 `Vec`。
    ///
    /// 字段集合必须与 [`Self::snapshot_form`] 保持一致（等价性由
    /// `connection_staging::form_matches_draft_agrees_with_snapshot` 锁定）；
    /// `saved_id` 不参与比较——它是条目身份，不是表单字段。
    pub fn form_matches_draft(&self, d: &ConnectionDraft, cx: &App) -> bool {
        let driver_name = self
            .driver
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default();
        let driver_id = self
            .resolve_driver(&driver_name)
            .map(|x| x.id)
            .unwrap_or_default();
        // Select 选中值：少量短字符串（≤ 5 个），在此分配可接受。
        let select_text = |sel: &Entity<SelectState<SearchableVec<SharedString>>>| -> Option<String> {
            sel.read(cx).selected_value().cloned().map(|v| v.to_string())
        };
        let groups_match = {
            let checks = self.group_checks.borrow();
            let checked: Vec<&String> = checks
                .iter()
                .filter(|(_, _, on)| *on)
                .map(|(gid, _, _)| gid)
                .collect();
            checked.len() == d.groups.len()
                && checked.iter().zip(d.groups.iter()).all(|(a, b)| **a == *b)
        };

        d.name.as_str() == &*self.name.read(cx).value()
            && d.type_id == *self.selected_type.borrow()
            && d.driver_id == driver_id
            && d.driver_name.as_str() == &*driver_name
            && d.url.as_str() == &*self.url.read(cx).value()
            && d.user.as_str() == &*self.user.read(cx).value()
            && d.pass.as_str() == &*self.pass.read(cx).value()
            && d.remark.as_str() == &*self.remark.read(cx).value()
            && d.tags.as_str() == &*self.tags_input.read(cx).value()
            && groups_match
            && d.scope == select_text(&self.scope).unwrap_or_default()
            && d.project_path.as_str() == &*self.project_path.read(cx).value()
            && d.ssl_mode == select_text(&self.ssl_mode).unwrap_or_default()
            && d.ssl_ca.as_str() == &*self.ssl_ca.read(cx).value()
            && d.ssl_cert.as_str() == &*self.ssl_cert.read(cx).value()
            && d.ssl_key.as_str() == &*self.ssl_key.read(cx).value()
            && d.cache_path.as_str() == &*self.cache_path.read(cx).value()
            && d.duckdb_fed == self.duckdb_fed.get()
            && d.active_tab == self.active_tab.get()
            && d.props.as_slice() == self.props.borrow().as_slice()
            && d.sec_overrides.as_slice() == self.policy_override_keys.borrow().as_slice()
            && d.auth_method == select_text(&self.auth_method).unwrap_or_default()
            && d.auth_ref == select_text(&self.auth_ref)
            && d.network_ref == select_text(&self.network_ref)
            && d.env == select_text(&self.env)
    }

    /// 把当前表单快照写回第 `idx` 个条目（切换前调用；已保存条目跳过）。
    pub(crate) fn capture_draft(&self, idx: usize, cx: &mut App) {
        if self.editing_id.borrow().is_some() {
            return;
        }
        let d = self.snapshot_form(cx);
        if let Some(slot) = self.drafts.borrow_mut().get_mut(idx) {
            *slot = d;
        }
    }

    /// 把第 `idx` 个条目载入表单（已保存条目走 `load_for_edit` 回读）。
    pub(crate) fn apply_draft(&self, idx: usize, window: &mut Window, cx: &mut App) {
        let Some(d) = self.drafts.borrow().get(idx).cloned() else {
            return;
        };
        if let Some(id) = d.saved_id.clone() {
            *self.editing_id.borrow_mut() = Some(id.clone());
            // 项目侧（P_/GP_）连接只存项目库：优先用条目记录的项目路径，
            // 旧条目无路径时回退当前会话项目根（否则回读取不到记录）。
            let root = if d.project_path.trim().is_empty() {
                self.session_project.borrow().as_ref().map(|(_, p)| p.clone())
            } else {
                Some(d.project_path.clone())
            };
            self.load_for_edit(&id, root.as_deref(), window, cx);
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
        self.tags_input
            .update(cx, |s, cx| s.set_value(d.tags.clone(), window, cx));
        {
            // 分组勾选恢复：以条目保存的 id 集合为准（目录在 refresh_meta 已加载）。
            let mut checks = self.group_checks.borrow_mut();
            for (gid, _name, checked) in checks.iter_mut() {
                *checked = d.groups.iter().any(|x| x == gid);
            }
        }
        self.project_path
            .update(cx, |s, cx| s.set_value(d.project_path.clone(), window, cx));
        // 项目栏显示同步（下拉命中 / 手动输入）。
        self.sync_project_selection(&d.project_path, window, cx);
        self.ssl_ca
            .update(cx, |s, cx| s.set_value(d.ssl_ca.clone(), window, cx));
        self.ssl_cert
            .update(cx, |s, cx| s.set_value(d.ssl_cert.clone(), window, cx));
        self.ssl_key
            .update(cx, |s, cx| s.set_value(d.ssl_key.clone(), window, cx));
        self.cache_path
            .update(cx, |s, cx| s.set_value(d.cache_path.clone(), window, cx));
        // 驱动：优先按驱动 id 定位（稳定），回退显示短名；同时把左侧类型恢复到条目值。
        let driver_value = if d.driver_id.is_empty() {
            d.driver_name.clone()
        } else {
            d.driver_id.clone()
        };
        self.set_driver_by_value(&d.type_id, &driver_value, window, cx);
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
        *self.props.borrow_mut() = d.props;
        *self.policy_override_keys.borrow_mut() = d.sec_overrides;
        set_select_value(&self.auth_method, d.auth_method.as_str(), window, cx);
        // 认证方法选项依赖驱动：切条目后重新按驱动解析（避免显示旧驱动的选项）。
        *self.auth_method_loaded_for.borrow_mut() = None;
        // 切到新环境后策略清单需重查（否则勾选项与环境的真实策略不一致）。
        *self.env_policies_loaded_for.borrow_mut() = None;
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
        self.staging_persist();
        *self.result.borrow_mut() = Some("已新建草稿（填好后保存）".to_string());
        self.result_ok.set(true);
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
        // 载入来源提示：让用户明白表单字段为何变化（本条目的快照 / 已保存连接回读）。
        let label = self
            .drafts
            .borrow()
            .get(idx)
            .map(|d| d.display_name())
            .unwrap_or_default();
        *self.result.borrow_mut() = Some(format!("已载入条目「{label}」"));
        self.result_ok.set(true);
        self.draft_cursor.set(idx);
        self.apply_draft(idx, window, cx);
        self.staging_persist();
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
        self.staging_persist();
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
        self.staging_persist();
    }

    /// 关闭对话框前把当前表单写回草稿（规则 5：关闭不丢失）。
    pub fn staging_flush(&self, cx: &mut App) {
        let cur = self.draft_cursor_idx();
        self.capture_draft(cur, cx);
        self.staging_persist();
    }

    // ===== 模板导入导出（C4；剪贴板 JSON，不含密码）=====
    //
    // 注：能力已实现并有测试覆盖（`tests/connection_template.rs`），但 **UI 入口按决策暂缓**
    // （避免当前阶段界面变动）；后期只需在暂存列表标题行接上「导出 / 导入」两个按钮。

    /// 导出未保存且非空的草稿为模板 JSON（已保存条目只有名称/驱动占位，故不导出）。
    ///
    /// 当前无 UI 入口（待启用）；供测试与后续接入使用。
    pub fn templates_export(&self) -> String {
        let connections: Vec<TemplateItem> = self
            .drafts
            .borrow()
            .iter()
            .filter(|d| d.saved_id.is_none() && !d.is_empty_draft())
            .map(|d| TemplateItem {
                name: d.name.clone(),
                type_id: d.type_id.clone(),
                driver_id: d.driver_id.clone(),
                url: d.url.clone(),
                username: d.user.clone(),
                remark: d.remark.clone(),
                scope: d.scope.clone(),
                project_path: d.project_path.clone(),
                ssl_mode: d.ssl_mode.clone(),
                tags: d.tags.clone(),
                duckdb_fed: d.duckdb_fed,
            })
            .collect();
        serde_json::to_string_pretty(&ConnectionTemplate {
            kind: TEMPLATE_KIND.to_string(),
            version: TEMPLATE_VERSION,
            connections,
        })
        .unwrap_or_default()
    }

    /// 可导出条目数（UI 提示用；与 [`Self::templates_export`] 的过滤规则一致）。
    pub fn templates_export_count(&self) -> usize {
        self.drafts
            .borrow()
            .iter()
            .filter(|d| d.saved_id.is_none() && !d.is_empty_draft())
            .count()
    }

    /// 从模板 JSON 导入到暂存列表（追加为新草稿，**密码留空**）；返回导入条数。
    ///
    /// 错误（非 JSON / 标识不符 / 版本过新 / 空列表）以中文消息返回，由调用方展示。
    /// 当前无 UI 入口（待启用）。
    pub fn templates_import(
        &self,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<usize, String> {
        let tpl: ConnectionTemplate =
            serde_json::from_str(text.trim()).map_err(|e| format!("模板解析失败：{e}"))?;
        if tpl.kind != TEMPLATE_KIND {
            return Err("不是 RDS 连接模板（kind 不匹配）".to_string());
        }
        if tpl.version > TEMPLATE_VERSION {
            return Err(format!(
                "模板版本过新（{} > {}），请升级应用",
                tpl.version, TEMPLATE_VERSION
            ));
        }
        if tpl.connections.is_empty() {
            return Err("模板中没有连接".to_string());
        }
        let n = tpl.connections.len();
        // 先把驱动 id 映射为实现短名（需读 drivers 目录），再写暂存列表，避免嵌套借用。
        let drafts_new: Vec<ConnectionDraft> = {
            let drivers = self.drivers.borrow();
            tpl.connections
                .into_iter()
                .map(|item| {
                    let driver_name = find_driver_by_value(&drivers, &item.driver_id)
                        .map(|d| driver_short_name(&d.name))
                        .unwrap_or_default();
                    ConnectionDraft {
                        name: item.name,
                        type_id: item.type_id,
                        driver_id: item.driver_id,
                        driver_name,
                        url: item.url,
                        user: item.username,
                        pass: String::new(),
                        remark: item.remark,
                        tags: item.tags,
                        scope: item.scope,
                        project_path: item.project_path,
                        ssl_mode: item.ssl_mode,
                        duckdb_fed: item.duckdb_fed,
                        ..ConnectionDraft::empty()
                    }
                })
                .collect()
        };
        let start = {
            let mut drafts = self.drafts.borrow_mut();
            let start = drafts.len();
            drafts.extend(drafts_new);
            start
        };
        self.draft_cursor.set(start);
        self.apply_draft(start, window, cx);
        self.staging_persist();
        Ok(n)
    }

    /// 保存到暂存表（关栏 / 各变更操作后调用；失败只记日志，不影响交互）。
    pub fn staging_persist(&self) {
        let rows: Vec<ConnectionDraftRow> = self
            .drafts
            .borrow()
            .iter()
            .map(Self::draft_to_row)
            .collect();
        let Ok(store) = ConnectionDraftStore::open_global() else {
            return;
        };
        if let Err(e) = store.replace_all(&rows) {
            tracing::warn!("暂存列表持久化失败: {e}");
        }
    }

    /// 首次打开对话框时从暂存表恢复（仅当仍为初始空草稿时替换，避免覆盖已编辑内容）。
    pub fn staging_restore(&self, window: &mut Window, cx: &mut App) {
        if self.drafts_restored.replace(true) {
            return;
        }
        let Ok(store) = ConnectionDraftStore::open_global() else {
            return;
        };
        let rows = store.list();
        if rows.is_empty() {
            return;
        }
        {
            let mut drafts = self.drafts.borrow_mut();
            if !(drafts.len() == 1 && drafts[0] == ConnectionDraft::empty()) {
                return;
            }
            *drafts = rows.iter().map(Self::row_to_draft).collect();
        }
        self.draft_cursor.set(0);
        // 恢复来源提示：说明字段为何是“上次会话”的值（区别于当前新建）。
        *self.result.borrow_mut() = Some(format!("已恢复上次暂存的草稿（{} 条）", rows.len()));
        self.result_ok.set(true);
        self.apply_draft(0, window, cx);
    }

    /// 打开对话框时合并**当前作用域可见**的已保存连接为列表条目（全局 + 项目侧 P_/GP_，
    /// 与导航栏同一加载器），并清理指向已删除连接的**幻影条目**（`saved_id` 已不在可见集合内）。
    ///
    /// 未保存草稿永不被清理；清理后列表若为空则补一条空草稿（保持“列表恒非空”）。
    /// 打开对话框时合并已保存连接（见实现注释）；集成测试直接调用以验证清理逻辑。
    pub fn staging_merge_saved(&self) {
        // 全局库未初始化（测试 / 降级启动）时不合并：`load_connections_for_scope` 在缺少
        // 单例时回退到“默认数据目录”，会让测试碰到用户真实库——宁可少一个便利功能。
        if engine::migration::get_global_db_manager().is_none() {
            tracing::debug!(
                target: "connection_dialog",
                "全局库未初始化：暂存列表不合并已保存连接"
            );
            return;
        }
        let drivers = self.drivers.borrow().clone();
        let project_root = self
            .session_project
            .borrow()
            .as_ref()
            .map(|(_, p)| std::path::PathBuf::from(p));
        let (items, notice) =
            crate::services::workspace_loader::load_connections_for_scope(project_root.as_deref());
        if let Some(n) = &notice {
            tracing::warn!(target: "connection_dialog", error = %n, "暂存列表加载已保存连接降级");
        }

        let mut drafts = self.drafts.borrow_mut();
        // 幻影条目清理：只删「已保存」条目（未保存草稿是用户输入，不动）。
        drafts.retain(|d| match d.saved_id.as_deref() {
            None => true,
            Some(id) => items.iter().any(|i| i.id == id),
        });
        for it in items {
            if drafts
                .iter()
                .any(|d| d.saved_id.as_deref() == Some(it.id.as_str()))
            {
                continue;
            }
            // 记录的 driver 即驱动 id：反查得到类型与实现短名，使条目能显示缩小的
            // 数据库类型 UI（且选中时下拉预填正确）。
            let did = it.driver.clone();
            let (type_id, driver_id, driver_name) = match find_driver_by_value(&drivers, &did) {
                Some(d) => (
                    d.type_id.clone(),
                    d.id.clone(),
                    driver_short_name(&d.name),
                ),
                None => (String::new(), did.clone(), String::new()),
            };
            drafts.push(ConnectionDraft {
                name: it.name.clone(),
                saved_id: Some(it.id.clone()),
                type_id,
                driver_id,
                driver_name,
                ..ConnectionDraft::empty()
            });
        }
        if drafts.is_empty() {
            drafts.push(ConnectionDraft::empty());
        }
        // 光标越界保护（清理后条目可能变少）。
        let cursor = self.draft_cursor.get().min(drafts.len() - 1);
        drop(drafts);
        self.draft_cursor.set(cursor);
    }
}
