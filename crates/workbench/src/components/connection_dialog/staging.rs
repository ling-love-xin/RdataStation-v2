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

/// 协议链跳（内联编辑态）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Hop {
    pub kind: String,
    pub label: String,
    pub enabled: bool,
}

impl Hop {
    pub(crate) fn ssh(label: impl Into<String>) -> Self {
        Self {
            kind: "SSH".into(),
            label: label.into(),
            enabled: true,
        }
    }
    pub(crate) fn proxy(label: impl Into<String>) -> Self {
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
    pub hops: Vec<Hop>,
    pub props: Vec<(String, String)>,
    pub sec_overrides: Vec<bool>,
    pub auth_ref: Option<String>,
    pub network_ref: Option<String>,
    pub env: Option<String>,
}

impl ConnectionDraft {
    /// 新建空草稿（默认值与对话框初始状态一致）。
    pub(crate) fn empty() -> Self {
        Self {
            duckdb_fed: true,
            sec_overrides: vec![true, true, false, true, true, false],
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
            hops_json: serde_json::to_string(&d.hops).unwrap_or_else(|_| "[]".into()),
            props_json: serde_json::to_string(&d.props).unwrap_or_else(|_| "[]".into()),
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
            hops: serde_json::from_str(&r.hops_json).unwrap_or_default(),
            props: serde_json::from_str(&r.props_json).unwrap_or_default(),
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
    pub(crate) fn snapshot_form(&self, cx: &App) -> ConnectionDraft {
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
        }
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

    /// 当前编辑条目是否有未写回的表单改动（侧栏脏标记 ●）。
    pub(crate) fn draft_dirty(&self, idx: usize, cx: &App) -> bool {
        let drafts = self.drafts.borrow();
        match drafts.get(idx) {
            Some(d) if d.saved_id.is_none() => self.snapshot_form(cx) != *d,
            _ => false,
        }
    }

    /// 把第 `idx` 个条目载入表单（已保存条目走 `load_for_edit` 回读）。
    pub(crate) fn apply_draft(&self, idx: usize, window: &mut Window, cx: &mut App) {
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
        self.staging_persist();
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
        self.apply_draft(0, window, cx);
    }

    /// 打开对话框时合并已保存连接为列表条目（仅全局列表；P_/GP_ 编辑入口在侧边栏）。
    pub(crate) fn staging_merge_saved(&self) {
        let Ok(service) = DataSourceService::global() else {
            return;
        };
        let Ok(rt) = tokio::runtime::Runtime::new() else {
            return;
        };
        if let Ok(list) = rt.block_on(service.list()) {
            let drivers = self.drivers.borrow().clone();
            let mut drafts = self.drafts.borrow_mut();
            for ds in list {
                if !drafts
                    .iter()
                    .any(|d| d.saved_id.as_deref() == Some(ds.id.as_str()))
                {
                    // 记录的 db_type / driver_id 均为驱动 id：反查得到类型与实现短名，
                    // 使条目能显示缩小的数据库类型 UI（且选中时下拉预填正确）。
                    let did = if ds.db_type.is_empty() {
                        ds.driver_id.clone().unwrap_or_default()
                    } else {
                        ds.db_type.clone()
                    };
                    let (type_id, driver_id, driver_name) = match find_driver_by_value(&drivers, &did)
                    {
                        Some(d) => (
                            d.type_id.clone(),
                            d.id.clone(),
                            driver_short_name(&d.name),
                        ),
                        None => (String::new(), did.clone(), String::new()),
                    };
                    drafts.push(ConnectionDraft {
                        name: ds.name.clone(),
                        saved_id: Some(ds.id.clone()),
                        type_id,
                        driver_id,
                        driver_name,
                        ..ConnectionDraft::empty()
                    });
                }
            }
        }
    }
}
