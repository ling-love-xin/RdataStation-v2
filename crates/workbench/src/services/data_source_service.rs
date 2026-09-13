//! 数据源连接服务（Phase A：DataSourceService）
//!
//! 组合层服务：类型/驱动/认证/网络/环境元数据查询 + 测试连接 + 保存/更新/删除。
//! 持久化走 `GlobalDatabaseManager`（engine persistence），测试走
//! `engine::services::connection_probe`（独立会话，不污染正式连接状态）。
//!
//! ## 落点说明
//! dev-plan 原落点 `crates/connection/src/service.rs` 因依赖方向
//! （engine → connection，connection 不得反向依赖 engine）调整至 workbench，
//! 与遗留 `connection_service.rs`（v1 迁移）同层；领域模型仍在 connection crate。
//!
//! ## 作用域（双轨制）
//! - `ConnectionScope::Global`：仅全局（G_xxx）
//! - `ConnectionScope::Project`：仅项目（P_xxx）
//! - `ConnectionScope::GlobalAndProject`：保存全局定义（G_xxx）+ 项目快照（GP_xxx）
//!
//! 写入前做项目路径预检与同名检查（见 `save`），避免半成品落库与静默覆盖。

use connection::model::{
    ConnectionScope, DataSource, DataSourceSaveInput, DeleteResult, TestResult,
};
use engine::persistence::auth_store::AuthConfig;
use engine::persistence::connection_org_store::ConnectionGroup;
use engine::persistence::driver_store::{DataSourceType, Driver};
use engine::persistence::env_store::{Environment, EnvironmentPolicy};
use engine::persistence::global_db::{
    GlobalConnectionSaveInput, GlobalConnectionUpdateInput, GlobalDatabaseManager,
};
use engine::persistence::id_prefix;
use engine::persistence::network_store::NetworkConfig;
use engine::persistence::project_connection_store::{ProjectConnection, ProjectConnectionStore};
use engine::persistence::project_db::ProjectDatabaseManager;
use shared::error::CoreError;
use std::sync::Arc;

use super::connection_service::{ConnectionService, ProbeConfigInput};
use super::driver_service::DriverService;

/// 可被连接引用的元数据种类（删除前的引用检查用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceField {
    AuthConfig,
    NetworkConfig,
    Environment,
}

impl ReferenceField {
    /// 中文名（错误消息用；与 UI 下拉标题一致）。
    fn label(self) -> &'static str {
        match self {
            Self::AuthConfig => "认证配置",
            Self::NetworkConfig => "网络配置",
            Self::Environment => "环境",
        }
    }
}

/// 引用统计（按存储位置拆分，便于错误消息说清范围）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferenceCount {
    /// 全局库连接（G_ 定义与遗留 conn-）
    pub global: usize,
    /// 当前打开项目的 P_/GP_ 连接
    pub project: usize,
    /// 其它已登记项目里命中该元数据的**项目名**（去重；#33：不再漏掉未打开的项目）
    pub other: Vec<String>,
}

impl ReferenceCount {
    pub fn total(&self) -> usize {
        self.global + self.project + self.other.len()
    }

    /// 其它项目数（便捷读法）。
    pub fn other_projects(&self) -> usize {
        self.other.len()
    }
}

/// 数据源连接服务（无状态组合，持全局库单例）。
pub struct DataSourceService {
    global_db: &'static GlobalDatabaseManager,
    drivers: DriverService,
    /// DuckDB Secret 目标库；`None` = 全局分析库（测试/多环境可注入）。
    analysis_db: Option<std::path::PathBuf>,
}

impl DataSourceService {
    /// 创建服务实例（`global_db` 取自全局初始化单例）。
    pub fn new(global_db: &'static GlobalDatabaseManager) -> Self {
        Self {
            global_db,
            drivers: DriverService::new(global_db),
            analysis_db: None,
        }
    }

    /// 全局单例便捷构造（应用启动后调用）。
    pub fn global() -> Result<Self, CoreError> {
        let db = engine::migration::global_init::get_global_db_manager().ok_or_else(|| {
            CoreError::common(shared::error::CommonError::General(
                "Global database manager not initialized".to_string(),
            ))
        })?;
        Ok(Self::new(db))
    }

    /// 指定 DuckDB Secret 目标库（测试/多环境注入；不设则用全局分析库）。
    pub fn with_analysis_db(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.analysis_db = Some(path.into());
        self
    }

    // ==================== 元数据查询 ====================

    /// 数据源类型目录（关系型/文件型/NoSQL…）
    pub async fn list_data_source_types(&self) -> Result<Vec<DataSourceType>, CoreError> {
        self.drivers.get_data_source_types().await
    }

    /// 全部驱动定义
    pub async fn list_drivers(&self) -> Result<Vec<Driver>, CoreError> {
        self.drivers.get_available_drivers().await
    }

    /// 按类型列出驱动
    pub async fn list_drivers_by_type(&self, type_id: &str) -> Result<Vec<Driver>, CoreError> {
        self.global_db.get_drivers_by_type(type_id).await
    }

    /// 认证配置列表（**UI 列表专用**：`auth_data` 一律置空）。
    ///
    /// 存储层 `list_auth_configs` 会解密后返回明文（供内部凭据注入用），但列表
    /// ——下拉选项 / 管理器行——只需要 id / name / auth_type；把明文带进 UI 内存
    /// 属于无谓的凭据暴露面（原型 §3.6「列表脱敏」）。需回填字段时走
    /// [`Self::auth_config_detail_by_name`]（单条、显式、仅在编辑时）。
    pub async fn list_auth_configs(&self) -> Result<Vec<AuthConfig>, CoreError> {
        let mut list = self.global_db.list_auth_configs(None).await?;
        for a in &mut list {
            a.auth_data.clear();
        }
        Ok(list)
    }

    /// 按名称读取认证配置的**解密后** `auth_data`（管理器编辑回填用）。
    ///
    /// 列表接口在服务层已把凭据置空，回填必须走这里：
    /// 按名称查 ID → `get_auth_config` → `decrypt_auth_data`。
    ///
    /// 返回 `(auth_type, auth_data JSON)`；档案不存在 → `Ok(None)`，解密失败 → `Err`。
    pub async fn auth_config_detail_by_name(
        &self,
        name: &str,
    ) -> Result<Option<(String, String)>, CoreError> {
        let items = self.global_db.list_auth_configs(None).await?;
        let Some(id) = items
            .iter()
            .find(|a| a.name.as_deref() == Some(name))
            .map(|a| a.id.clone())
        else {
            return Ok(None);
        };
        let Some(cfg) = self.global_db.get_auth_config(&id).await? else {
            return Ok(None);
        };
        let data = engine::persistence::auth_store::decrypt_auth_data(&cfg.auth_data)?;
        Ok(Some((cfg.auth_type, data)))
    }

    /// 网络配置列表（**UI 列表 / 下拉专用**：`config` 一律置空）。
    ///
    /// 存储层读路径会解密 `config`（连接解析需要明文），但列表消费方（下拉选项 /
    /// 管理器行 / 名称匹配）只需要 id / name / type——明文凭据不进 UI 内存（原型 §3.6）。
    /// 需回填字段时走 [`Self::network_config_detail_by_name`]。
    pub async fn list_network_configs(&self) -> Result<Vec<NetworkConfig>, CoreError> {
        let mut list = self.global_db.list_network_configs(None).await?;
        for n in &mut list {
            n.config.clear();
        }
        Ok(list)
    }

    /// 按名称读取网络档案的**解密后** `config`（管理器编辑回填用）。
    ///
    /// 与 `auth_config_detail_by_name` 同款：列表接口在服务层已脱敏（`config` 置空），
    /// 回填必须走这里。返回 `(network_type, config JSON)`；不存在 → `Ok(None)`。
    pub async fn network_config_detail_by_name(
        &self,
        name: &str,
    ) -> Result<Option<(String, String)>, CoreError> {
        let items = self.global_db.list_network_configs(None).await?;
        let Some(id) = items
            .iter()
            .find(|n| n.name.as_deref() == Some(name))
            .map(|n| n.id.clone())
        else {
            return Ok(None);
        };
        // 单条读路径同样解密（存储层保证）
        let Some(cfg) = self.global_db.get_network_config(&id).await? else {
            return Ok(None);
        };
        Ok(Some((cfg.network_type, cfg.config)))
    }

    /// 环境列表
    pub async fn list_environments(&self) -> Result<Vec<Environment>, CoreError> {
        self.global_db.list_environments().await
    }

    /// 按环境名列出其策略（数据源：`environment_policies`；环境不存在 → 空列表）。
    ///
    /// 供高级 Tab「策略覆盖」勾选与连接对话框的降级展示——**UI 不自造策略清单**。
    pub async fn list_environment_policies_by_name(
        &self,
        env_name: &str,
    ) -> Result<Vec<EnvironmentPolicy>, CoreError> {
        let envs = self.global_db.list_environments().await?;
        let Some(env) = envs.into_iter().find(|e| e.name == env_name) else {
            return Ok(Vec::new());
        };
        self.global_db.list_environment_policies(&env.id).await
    }

    // ==================== 元数据引用计数（删除守卫） ====================

    /// 统计引用某一元数据的连接数（批量；批量查询避免逐条扫库）。
    ///
    /// 范围：全局库全部连接 + 当前打开项目 + **名册里的其它项目**（#33：删除守卫不再漏掉
    /// 已关闭的项目；每个项目库打开一次，项目数通常个位数）。每条档案只被计入
    /// 「其它项目」一次（记项目名，不重复），便于错误消息直接列出项目。
    /// 读路径全程 `is_project_root` 过滤，不会建目录。
    pub async fn count_references_batch(
        &self,
        field: ReferenceField,
        ids: &[String],
        project_path: Option<&str>,
    ) -> std::collections::HashMap<String, ReferenceCount> {
        let mut map: std::collections::HashMap<String, ReferenceCount> = ids
            .iter()
            .filter(|id| !id.trim().is_empty())
            .map(|id| (id.clone(), ReferenceCount::default()))
            .collect();
        if map.is_empty() {
            return map;
        }

        if let Ok(list) = self.global_db.get_global_connections(None, None).await {
            for c in &list {
                let id = match field {
                    ReferenceField::AuthConfig => c.auth_config_id.as_deref(),
                    ReferenceField::NetworkConfig => c.network_config_id.as_deref(),
                    ReferenceField::Environment => c.environment_id.as_deref(),
                };
                if let Some(id) = id {
                    if let Some(entry) = map.get_mut(id) {
                        entry.global += 1;
                    }
                }
            }
        }

        // 项目侧：当前打开项目 + **名册里的其它项目**（#33：删除守卫不再漏掉已关闭项目；
        // 路径逐一 `is_project_root` 过，读路径不建目录）
        let mut roots: Vec<(String, String, bool)> = Vec::new(); // (root, 显示名, 是否当前项目)
        if let Some(path) = project_path.filter(|p| !p.trim().is_empty() && is_project_root(p)) {
            roots.push((path.to_string(), project_display_name(path), true));
        }
        if let Ok(projects) = self.global_db.get_all_projects().await {
            for p in projects {
                let path = p.path.trim().to_string();
                if path.is_empty() || roots.iter().any(|(r, _, _)| r == &path) {
                    continue;
                }
                if !is_project_root(&path) {
                    continue;
                }
                let name = if p.name.trim().is_empty() {
                    project_display_name(&path)
                } else {
                    p.name.clone()
                };
                roots.push((path, name, false));
            }
        }

        for (root, name, is_current) in roots {
            let Ok(store) = open_project_store(&root).await else {
                continue;
            };
            let Ok(rows) = store.get_all_connections().await else {
                continue;
            };
            // 本库命中的元数据 id（先收集再改 map，避免借用冲突）
            let mut hit_ids: Vec<String> = Vec::new();
            for c in &rows {
                let id = match field {
                    ReferenceField::AuthConfig => c.auth_config_id.as_deref(),
                    ReferenceField::NetworkConfig => c.network_config_id.as_deref(),
                    ReferenceField::Environment => c.environment_id.as_deref(),
                };
                if let Some(id) = id {
                    if map.contains_key(id) {
                        hit_ids.push(id.to_string());
                    }
                }
            }
            for id in hit_ids {
                if let Some(entry) = map.get_mut(&id) {
                    if is_current {
                        entry.project += 1;
                    } else if !entry.other.iter().any(|n| n == &name) {
                        entry.other.push(name.clone());
                    }
                }
            }
        }

        map
    }

    /// 单个元数据的引用计数（单条便捷入口）。
    pub async fn count_references(
        &self,
        field: ReferenceField,
        id: &str,
        project_path: Option<&str>,
    ) -> ReferenceCount {
        self.count_references_batch(field, &[id.to_string()], project_path)
            .await
            .remove(id)
            .unwrap_or_default()
    }

    /// 删除前的引用守卫（原型 §3.6：「被引用的配置不可删除」）。
    ///
    /// 返回 `Err` 时消息已包含「引用数 + 范围（全局 / 当前项目 / 其它项目名）」，UI 直接展示即可。
    pub async fn ensure_no_references(
        &self,
        field: ReferenceField,
        id: &str,
        display_name: &str,
        project_path: Option<&str>,
    ) -> Result<(), CoreError> {
        let count = self.count_references(field, id, project_path).await;
        if count.total() == 0 {
            return Ok(());
        }
        let mut parts: Vec<String> = Vec::new();
        if count.global > 0 {
            parts.push(format!("全局 {} 条", count.global));
        }
        if count.project > 0 {
            parts.push(format!("当前项目 {} 条", count.project));
        }
        if !count.other.is_empty() {
            let names = if count.other.len() > 3 {
                format!("{} 等 {} 个项目", count.other[..3].join("、"), count.other.len())
            } else {
                count.other.join("、")
            };
            parts.push(format!("其它项目 {} 条（{}）", count.other.len(), names));
        }
        Err(CoreError::common(shared::error::CommonError::General(format!(
            "{}{}正被 {} 连接引用，不能删除；请先修改或删除这些连接（统计范围：全局库 + 已登记项目库）",
            field.label(),
            if display_name.trim().is_empty() {
                String::new()
            } else {
                format!("「{}」", display_name.trim())
            },
            parts.join("、")
        ))))
    }

    // ==================== 组织元数据（标签 / 分组） ====================

    /// 项目可见分组（项目级能力；未打开项目返回空列表）。
    pub fn list_groups(&self, project_path: Option<&str>) -> Vec<ConnectionGroup> {
        open_org_store(self.global_db, project_path)
            .map(|store| store.list_groups())
            .unwrap_or_default()
    }

    /// 某连接所属分组 id（项目级；未打开项目返回空）。
    pub fn groups_of(&self, conn_id: &str, project_path: Option<&str>) -> Vec<String> {
        open_org_store(self.global_db, project_path)
            .map(|store| store.list_groups_for_connection(conn_id))
            .unwrap_or_default()
    }

    /// 同步连接分组（**替换语义**：以 UI 勾选为准；分组为项目级能力，未打开项目时忽略）。
    ///
    /// 与标签同步（`sync_connection_tags`）同策略：失败仅告警，不阻断连接保存。
    pub fn set_connection_groups(
        &self,
        conn_id: &str,
        group_ids: &[String],
        project_path: Option<&str>,
    ) {
        match open_org_store(self.global_db, project_path)
            .and_then(|store| store.set_connection_groups(conn_id, group_ids))
        {
            Ok(()) => tracing::debug!(
                target: "data_source_service",
                conn_id,
                count = group_ids.len(),
                "连接分组已同步"
            ),
            Err(e) => tracing::warn!(
                target: "data_source_service",
                conn_id,
                error = %e,
                "连接分组同步失败（不影响连接保存）"
            ),
        }
    }

    // ==================== 连接 CRUD ====================

    /// 列出全部数据源（global_connections 回读 → 领域模型）。
    pub async fn list(&self) -> Result<Vec<DataSource>, CoreError> {
        let infos = self.global_db.get_global_connections(None, None).await?;
        Ok(infos.into_iter().map(map_info_to_data_source).collect())
    }

    /// 按 ID 读取**全局侧**数据源（只查 `global_connections`）。
    ///
    /// 项目侧连接（`P_`/`GP_`）请用 [`Self::get_with_project`]；重命名为 `get_global` 是
    /// 为了在调用点就能看出“只查全局库”这个前提（历史上有两处因此踩坑：对话框编辑回读、
    /// 导航树「连接」入口）。
    pub async fn get_global(&self, conn_id: &str) -> Result<Option<DataSource>, CoreError> {
        Ok(self.list().await?.into_iter().find(|ds| ds.id == conn_id))
    }

    /// 按 ID 读取数据源（按 ID 前缀路由到全局库 / 项目库）。
    ///
    /// 项目侧连接（`P_` / `GP_` 快照）只存在项目库里，`get` 查不到；编辑回读 / 详情
    /// 必须走本方法并带项目根，否则表现为“点编辑后字段全空”。未打开项目（无项目根）
    /// 时项目侧返回 `None`（不报错，由 UI 降级）。
    pub async fn get_with_project(
        &self,
        conn_id: &str,
        project_path: Option<&str>,
    ) -> Result<Option<DataSource>, CoreError> {
        if id_prefix::uses_project_storage(conn_id) {
            let Some(path) = project_path.filter(|p| !p.trim().is_empty()) else {
                return Ok(None);
            };
            // 读路径也预检：不合法（非项目根）时直接降级为空，**绝不建目录**。
            if !is_project_root(path) {
                tracing::warn!(
                    target: "data_source_service",
                    conn_id = %conn_id,
                    project_path = %path,
                    "项目路径不是项目根（缺 .RSmeta）：项目侧回读降级为空"
                );
                return Ok(None);
            }
            let store = open_project_store(path).await?;
            let row = store.get_connection(conn_id).await?;
            return Ok(row.map(map_project_connection_to_data_source));
        }
        self.get_global(conn_id).await
    }

    /// 保存新连接（全局：G_ 前缀；凭据由 engine 侧 AES-256-GCM 加密落库）。
    ///
    /// 返回生成的连接 ID。保存成功后联动注册 DuckDB Secret（联邦加速，失败仅告警）。
    ///
    /// 作用域落库：Global → global_connections（G_）；Project → 项目库（P_）；
    /// GlobalAndProject → 全局定义（G_）+ 项目快照（GP_）。
    /// `project_path` 在作用域含项目侧时必需（项目根目录，内含 .RSmeta/project.db）。
    pub async fn save(
        &self,
        input: &DataSourceSaveInput,
        project_path: Option<&str>,
    ) -> Result<String, CoreError> {
        let url = build_effective_url(input);
        let global_side = input.scope.includes_global();
        let project_side = input.scope.includes_project();

        // 项目侧预检：路径无效时提前失败，避免“全局已写入、项目失败”的半成品，
        // 也避免 `open` 顺手 `create_dir_all(.RSmeta)` 造出假的“项目骨架”。
        let project_store = if project_side {
            let Some(path) = project_path.filter(|p| !p.trim().is_empty()) else {
                return Err(CoreError::common(shared::error::CommonError::General(
                    "未打开项目：项目/全局+项目作用域需要项目路径（.RSmeta）".to_string(),
                )));
            };
            ensure_project_root(path)?;
            Some((path.to_string(), open_project_store(path).await?))
        } else {
            None
        };

        // 全局侧：同名检查 + 落库（G_ 为确定性 ID，同名会静默覆盖）。
        let global_id = if global_side {
            self.ensure_name_available(&input.name, None).await?;
            let conn_id = id_prefix::generate_gid("conn", &input.name);
            self.global_db
                .save_global_connection(GlobalConnectionSaveInput {
                    conn_id: &conn_id,
                    name: &input.name,
                    db_type: &input.db_type,
                    url: &url,
                    username: input.username.as_deref(),
                    password: input.password.as_deref(),
                    tags: input.tags.as_deref(),
                    server_version: None,
                    description: input.description.as_deref(),
                    driver_id: input.driver_id.as_deref(),
                    environment_id: input.environment_id.as_deref(),
                    auth_config_id: input.auth_config_id.as_deref(),
                    auth_method: input.auth_method.as_deref(),
                    network_config_id: input.network_config_id.as_deref(),
                    options: input.options.as_deref(),
                    driver_properties: input.driver_properties.as_deref(),
                    advanced_options: input.advanced_options.as_deref(),
                    use_duckdb_fed: input.use_duckdb_fed,
                    metadata_path: input.metadata_path.as_deref(),
                    schema_name: input.schema_name.as_deref(),
                })
                .await?;
            // Secret 联动：仅开启本地加速时注册（失败仅告警）。
            if input.use_duckdb_fed.unwrap_or(false) {
                super::secret_integration::ensure_secret_registered_at(
                    self.analysis_db.as_deref(),
                    &conn_id,
                    &input.db_type,
                    &url,
                );
            }
            // 标签同步到连接组织存储（connection_tags 为权威检索源；JSON 字段保留兼容）。
            sync_connection_tags(self.global_db, &conn_id, input.tags.as_deref(), None);
            Some(conn_id)
        } else {
            None
        };

        // 项目侧：P_ 本地连接 / GP_ 全局快照连接（引用共享；快照深度复制见 Phase C 深化）。
        if let Some((path, store)) = project_store {
            let pid = if input.scope == ConnectionScope::GlobalAndProject {
                id_prefix::generate_gpid("conn", &input.name)
            } else {
                id_prefix::generate_pid("conn")
            };
            let proj = project_connection_from_input(&pid, input, &url)?;
            store.create_connection(&proj).await?;
            sync_connection_tags(self.global_db, &pid, input.tags.as_deref(), Some(&path));
            tracing::info!(target: "data_source_service", conn_id = %pid, name = %input.name, "数据源已保存（项目侧）");
            return Ok(pid);
        }

        let conn_id = global_id.ok_or_else(|| {
            CoreError::common(shared::error::CommonError::General(
                "作用域未包含任何落库侧（内部错误）".to_string(),
            ))
        })?;
        tracing::info!(target: "data_source_service", conn_id = %conn_id, name = %input.name, "数据源已保存（仅全局）");
        Ok(conn_id)
    }

    /// 用全局定义刷新「全局+项目」快照（`GP_`）——显式同步入口。
    ///
    /// 语义：把全局侧（`G_`）的配置字段与凭据（密文）复制到项目侧快照；项目侧的分组关系、
    /// 记录 ID 与创建时间保持不变。快照是**独立副本**，全局定义后续修改不会自动跟随——
    /// 需要同步时显式调用本方法。全局定义不存在时报错（不静默降级）。
    pub async fn sync_snapshot_from_global(
        &self,
        snapshot_id: &str,
        project_path: &str,
    ) -> Result<(), CoreError> {
        if project_path.trim().is_empty() {
            return Err(CoreError::common(shared::error::CommonError::General(
                "未打开项目：同步快照需要项目路径（.RSmeta）".to_string(),
            )));
        }
        ensure_project_root(project_path)?;
        let Some(global_id) = id_prefix::source_global_id(snapshot_id) else {
            return Err(CoreError::common(shared::error::CommonError::General(
                "仅「全局+项目」快照连接（GP_）支持从全局定义同步".to_string(),
            )));
        };
        let Some(src) = self.get_global(&global_id).await? else {
            return Err(CoreError::common(shared::error::CommonError::General(format!(
                "全局定义 {global_id} 不存在（可能已被删除），无法同步"
            ))));
        };
        let store = open_project_store(project_path).await?;
        let Some(mut row) = store.get_connection(snapshot_id).await? else {
            return Err(CoreError::common(shared::error::CommonError::General(format!(
                "项目侧快照 {snapshot_id} 不存在"
            ))));
        };
        // 只覆盖配置字段与凭据密文；`id` / `created_at` / 分组关系保留项目侧现状。
        row.name = src.name.clone();
        row.driver = src.db_type.clone();
        row.host = src.host.clone();
        row.port = src.port.map(|p| p as i32);
        row.database = src.database.clone();
        row.schema_name = src.schema_name.clone();
        row.username = src.username.clone();
        row.password_encrypted = src.password_encrypted.clone();
        row.options = src.options.clone();
        row.tags = src.tags.clone();
        row.use_duckdb_fed = src.use_duckdb_fed;
        row.metadata_path = src.metadata_path.clone();
        row.description = src.description.clone();
        row.driver_id = src.driver_id.clone();
        row.environment_id = src.environment_id.clone();
        row.auth_config_id = src.auth_config_id.clone();
        row.auth_method = src.auth_method.clone();
        row.network_config_id = src.network_config_id.clone();
        row.driver_properties = src.driver_properties.clone();
        row.advanced_options = src.advanced_options.clone();
        // 时间戳格式与全局库一致（SQLite CURRENT_TIMESTAMP）。
        row.updated_at = chrono::Utc::now()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        store.update_connection(&row).await?;
        // 标签双源一致：快照的 JSON 投影更新后，权威检索表（`connection_tags`）必须跟着变，
        // 否则 `tag:x` 检索与导航标签视图会读到同步前的旧标签。
        sync_connection_tags(
            self.global_db,
            snapshot_id,
            row.tags.as_deref(),
            Some(project_path),
        );
        tracing::info!(
            target: "data_source_service",
            snapshot_id = %snapshot_id,
            global_id = %global_id,
            "项目快照已从全局定义同步"
        );
        Ok(())
    }

    /// 更新连接（密码为空时保留现有密文，见 engine update_global_connection）。
    ///
    /// host/port/database 从 `input.url` 解析；driver/认证/网络/环境/策略等字段直接透传。
    pub async fn update(
        &self,
        conn_id: &str,
        input: &DataSourceSaveInput,
        project_path: Option<&str>,
    ) -> Result<(), CoreError> {
        let url = build_effective_url(input);
        let (host, port, database) = parse_url_host_port_db(&input.db_type, &url);

        if id_prefix::uses_project_storage(conn_id) {
            let Some(path) = project_path.filter(|p| !p.trim().is_empty()) else {
                return Err(CoreError::common(shared::error::CommonError::General(
                    "未打开项目：项目作用域连接更新需要项目路径（.RSmeta）".to_string(),
                )));
            };
            ensure_project_root(path)?;
            let store = open_project_store(path).await?;
            let proj = project_connection_from_input(conn_id, input, &url)?;
            store.update_connection(&proj).await?;
            sync_connection_tags(self.global_db, conn_id, input.tags.as_deref(), Some(path));
            return Ok(());
        }

        // 同名检查（排除自身）。
        self.ensure_name_available(&input.name, Some(conn_id))
            .await?;

        self.global_db
            .update_global_connection(GlobalConnectionUpdateInput {
                conn_id: conn_id.to_string(),
                name: input.name.clone(),
                driver: Some(input.db_type.clone()),
                host,
                port,
                database,
                schema_name: input.schema_name.clone(),
                username: input.username.clone(),
                password: input.password.clone(),
                tags: input.tags.clone(),
                server_version: None,
                description: input.description.clone(),
                driver_id: input.driver_id.clone(),
                environment_id: input.environment_id.clone(),
                auth_config_id: input.auth_config_id.clone(),
                auth_method: input.auth_method.clone(),
                network_config_id: input.network_config_id.clone(),
                options: input.options.clone(),
                driver_properties: input.driver_properties.clone(),
                advanced_options: input.advanced_options.clone(),
                use_duckdb_fed: input.use_duckdb_fed,
                metadata_path: input.metadata_path.clone(),
            })
            .await?;

        // 联邦加速开关联动：开 → 注册；关 → 清理（避免残留旧凭据）。
        if input.use_duckdb_fed.unwrap_or(false) {
            super::secret_integration::ensure_secret_registered_at(
                self.analysis_db.as_deref(),
                conn_id,
                &input.db_type,
                &url,
            );
        } else {
            let _ = super::secret_integration::remove_connection_secret_at(
                self.analysis_db.as_deref(),
                conn_id,
            );
        }
        // 标签同步（更新路径同样以连接组织存储为权威检索源）。
        sync_connection_tags(self.global_db, conn_id, input.tags.as_deref(), None);
        Ok(())
    }

    /// 删除连接（物理删除 + 清理分析库中的 DuckDB Secret）。
    pub async fn delete(
        &self,
        conn_id: &str,
        project_path: Option<&str>,
    ) -> Result<DeleteResult, CoreError> {
        if id_prefix::uses_project_storage(conn_id) {
            let Some(path) = project_path.filter(|p| !p.trim().is_empty()) else {
                return Err(CoreError::common(shared::error::CommonError::General(
                    "未打开项目：项目作用域连接删除需要项目路径（.RSmeta）".to_string(),
                )));
            };
            ensure_project_root(path)?;
            let store = open_project_store(path).await?;
            store.delete_connection(conn_id).await?;
            // 一致性清理：标签 + 分组成员（避免孤儿数据）。
            cleanup_connection_org(self.global_db, conn_id, Some(path));
            return Ok(DeleteResult {
                conn_id: conn_id.to_string(),
                removed_secret: false,
                message: "项目连接已删除".to_string(),
            });
        }

        self.global_db.delete_global_connection(conn_id).await?;

        // Secret 清理（分析库持久 Secret；未注册过不算失败）。
        let removed_secret = super::secret_integration::remove_connection_secret_at(
            self.analysis_db.as_deref(),
            conn_id,
        );
        // 一致性清理：全局库侧（标签 + 会员）。
        cleanup_connection_org(self.global_db, conn_id, None);
        // 「全局连接加入项目分组」是合法配置（成员关系存项目库），删除时一并清理项目侧，
        // 否则 B3 分组视图会出现幻影成员；项目根不合法时跳过（避免误清全局库）。
        if let Some(path) = project_path.filter(|p| !p.trim().is_empty() && is_project_root(p)) {
            cleanup_connection_org(self.global_db, conn_id, Some(path));
        }

        tracing::info!(target: "data_source_service", conn_id, "数据源已删除");
        Ok(DeleteResult {
            conn_id: conn_id.to_string(),
            removed_secret,
            message: if removed_secret {
                "连接已删除，DuckDB Secret 已清理".to_string()
            } else {
                "连接已删除".to_string()
            },
        })
    }

    // ==================== 测试连接 ====================

    /// 测试连接（独立会话：构建 DriverConnectionConfig → engine connection_probe）。
    ///
    /// 与真实连接**同源**：认证档案凭据、网络档案隧道（探测期间建立、结束即释放）、
    /// 驱动属性与高级选项均走 `ConnectionService::build_probe_config`，与 `connect`
    /// 同一套规则；不注册连接池 / 管理器，不写库。
    ///
    /// `project_path` 用于解析 P_/GP_ 前缀的档案（项目根，含 `.RSmeta`）。
    pub async fn test(&self, input: &DataSourceSaveInput, project_path: Option<&str>) -> TestResult {
        let url = build_effective_url(input);
        let (config, guards, notes) = match ConnectionService::build_probe_config(
            Some(self.global_db),
            ProbeConfigInput {
                db_type: &input.db_type,
                url: &url,
                name: &input.name,
                username: input.username.as_deref(),
                password: input.password.as_deref(),
                auth_config_id: input.auth_config_id.as_deref(),
                auth_method: input.auth_method.as_deref(),
                network_config_id: input.network_config_id.as_deref(),
                project_path,
                driver_properties: input.driver_properties.as_deref(),
                advanced_options: input.advanced_options.as_deref(),
            },
        )
        .await
        {
            Ok(v) => v,
            // 网络档案建不起隧道：真实连接同样会失败，直接把原因作为测试结果
            Err(e) => return TestResult::err(e.to_string()),
        };

        let mut result = engine::services::connection_probe::test_connection_result(config).await;
        // 探测结束：立即释放隧道（守卫 drop 关闭本地端口与后台任务）
        drop(guards);
        if !notes.is_empty() {
            result.message = format!("{} · {}", result.message, notes.join("；"));
        }
        result
    }

    // ==================== 私有校验 ====================

    /// 同名连接检查（大小写不敏感）。
    ///
    /// `generate_gid("conn", name)` 是确定性 ID，同名新建会经 `INSERT OR REPLACE`
    /// 静默覆盖已有连接；此处提前拦截。`exclude_id` 用于更新场景排除自身。
    async fn ensure_name_available(
        &self,
        name: &str,
        exclude_id: Option<&str>,
    ) -> Result<(), CoreError> {
        let wanted = name.trim().to_lowercase();
        let existing = self.global_db.get_global_connections(None, None).await?;
        if let Some(dup) = existing
            .iter()
            .find(|c| c.name.trim().to_lowercase() == wanted && Some(c.id.as_str()) != exclude_id)
        {
            return Err(CoreError::common(shared::error::CommonError::General(
                format!(
                    "连接名称「{}」已存在（{}），请更换名称",
                    name.trim(),
                    dup.id
                ),
            )));
        }
        Ok(())
    }
}

// ==================== 私有工具 ====================

/// 解析 tags JSON 数组（非法 / None → 空集）。
fn parse_tags_json(tags: Option<&str>) -> Vec<String> {
    tags.and_then(|t| serde_json::from_str::<Vec<String>>(t).ok())
        .unwrap_or_default()
}

/// 连接组织元数据存储（项目路径为空 → 全局库；路径取自单例自身，保证同库）。
///
/// 项目路径**不是项目根**时回退全局库：`open_project` 会 `create_dir_all(.RSmeta)`，
/// 让一次读操作在磁盘上造出假的“项目骨架”（脏数据）。
fn open_org_store(
    global_db: &GlobalDatabaseManager,
    project_path: Option<&str>,
) -> Result<engine::persistence::ConnectionOrgStore, CoreError> {
    match project_path.filter(|p| !p.trim().is_empty() && is_project_root(p)) {
        Some(path) => {
            engine::persistence::ConnectionOrgStore::open_project(std::path::Path::new(path))
        }
        None => {
            let db_path = global_db.sqlite_pool().path().clone();
            engine::persistence::ConnectionOrgStore::open_at(db_path, false)
        }
    }
}

/// 项目显示名：路径最后一段（名册里取不到名字时的回退）。
fn project_display_name(path: &str) -> String {
    std::path::Path::new(path.trim())
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.trim().to_string())
}

/// 同步连接标签到权威检索表（`connection_tags`）。
///
/// 连接的 `tags` JSON 字段保留作为兼容投影；检索（`tag:x`）与导航消费
/// 统一读连接组织存储。同步失败仅告警，不阻断连接保存。
fn sync_connection_tags(
    global_db: &GlobalDatabaseManager,
    conn_id: &str,
    tags: Option<&str>,
    project_path: Option<&str>,
) {
    let parsed = parse_tags_json(tags);
    match open_org_store(global_db, project_path).and_then(|store| store.set_tags(conn_id, &parsed))
    {
        Ok(()) => tracing::debug!(target: "data_source_service", conn_id, count = parsed.len(), "连接标签已同步"),
        Err(e) => tracing::warn!(target: "data_source_service", conn_id, error = %e, "连接标签同步失败（不影响连接保存）"),
    }
}

/// 删除连接时清理其组织关系（标签 + 分组成员）。
fn cleanup_connection_org(
    global_db: &GlobalDatabaseManager,
    conn_id: &str,
    project_path: Option<&str>,
) {
    match open_org_store(global_db, project_path).and_then(|store| store.remove_connection(conn_id))
    {
        Ok(()) => tracing::debug!(target: "data_source_service", conn_id, "连接组织关系已清理"),
        Err(e) => tracing::warn!(target: "data_source_service", conn_id, error = %e, "连接组织关系清理失败"),
    }
}

/// 构建有效 URL：URL 无凭据但单独提供 username / password 时注入 userinfo
/// （经 RFC 3986 转义，密码含 `@` / `:` / `/` 不会破坏 URL 结构）。
///
/// 文件型（SQLite/DuckDB）没有凭据语义：地址就是本地路径，原样返回（只规范化前缀）。
fn build_effective_url(input: &DataSourceSaveInput) -> String {
    if is_file_db_driver(&input.db_type) {
        return normalize_file_db_path(&input.db_type, &input.url);
    }
    connection::url_params::merge_credentials(
        &input.url,
        input.username.as_deref().unwrap_or(""),
        input.password.as_deref().unwrap_or(""),
    )
}

/// 用「主机 / 端口 / 数据库」字段重建 URL（对话框字段 → URI 方向）。
///
/// 规则（与渲染层的双向同步对齐）：
/// - 文件型驱动：地址就是路径，不由这三个字段重建（原样返回）；
/// - 字段与 URL 解析结果一致 → 原样返回（幂等，避免无意义改写）；
/// - 主机为空 → 原样返回（不因半成品输入产生非法 URL）；
/// - 端口空 → 用驱动声明的默认端口，否则保持 URL 原端口；
/// - 数据库空 → 去掉路径段。
pub fn rebuild_url_from_fields(
    driver_id: &str,
    url: &str,
    host: &str,
    port: &str,
    database: &str,
    default_port: Option<i32>,
) -> String {
    if is_file_db_driver(driver_id) {
        return url.to_string();
    }
    let (cur_host, cur_port, cur_db) = parse_url_host_port_db(driver_id, url);
    let port_num: Option<u16> = port.trim().parse().ok();
    let changed = host.trim() != cur_host.as_deref().unwrap_or("")
        || port_num.map(|p| p as i32) != cur_port
        || database.trim() != cur_db.as_deref().unwrap_or("");
    if !changed || host.trim().is_empty() {
        return url.to_string();
    }
    let port_final = port_num
        .or_else(|| default_port.map(|p| p as u16))
        .or_else(|| cur_port.map(|p| p as u16));
    connection::url_params::rewrite_url_authority(url, Some(host.trim()), port_final, Some(database.trim()))
}

/// 项目根预检：必须是**已存在且含 `.RSmeta`** 的目录（比较忽略大小写）。
///
/// 为何必须：`ProjectDatabaseManager::open` / `ConnectionOrgStore::open_project` 都会
/// `create_dir_all(.RSmeta)`——传入任意目录会静默创建一套项目骨架（磁盘脏数据）。
/// 写路径调用它并报错；读路径调用它并降级为空（不建目录）。
fn is_project_root(path: &str) -> bool {
    crate::services::workspace_loader::is_project_root(std::path::Path::new(path.trim()))
}

/// 写路径的项目根预检（不合法 → 明确的用户可见错误）。
fn ensure_project_root(project_path: &str) -> Result<(), CoreError> {
    if is_project_root(project_path) {
        return Ok(());
    }
    Err(CoreError::common(shared::error::CommonError::General(format!(
        "项目路径无效：{} 不是项目根（缺少 .RSmeta，或目录不存在）",
        project_path.trim()
    ))))
}

/// 打开项目持久化存储（ProjectDatabaseManager::open → ProjectConnectionStore）。
async fn open_project_store(project_path: &str) -> Result<ProjectConnectionStore, CoreError> {
    let db = ProjectDatabaseManager::open(std::path::Path::new(project_path), 4).await?;
    Ok(ProjectConnectionStore::new(Arc::new(db)))
}

/// 内置文件型驱动 id（与种子目录的 `drivers.is_file = 1` 一致）。
///
/// 插件驱动落地后此判定应改为读驱动目录（`drivers.is_file`）；当前只有内置四种驱动，
/// 且服务层纯函数拿不到目录快照，故先用 id 字典（注释即约束，改动点只此一处）。
pub fn is_file_db_driver(driver_id: &str) -> bool {
    matches!(driver_id, "sqlite" | "duckdb")
}

/// 文件型地址规范化：用户输入可能是裸路径 / `sqlite://path` / `sqlite:///C:/x.db`，
/// 库内 `database` 统一存**裸路径**（引擎按 `{driver}://{path}` 还原连接 URL）。
///
/// `sqlite:///C:/x.db` 会留下盘符前的多余前导斜杠（Windows 上会被当成相对/错误路径），
/// 这里去掉它；Unix 绝对路径（`/data/x.db`）保持原样。
pub fn normalize_file_db_path(driver_id: &str, url: &str) -> String {
    let trimmed = url.trim();
    let body = trimmed
        .strip_prefix(&format!("{driver_id}://"))
        .or_else(|| trimmed.split_once("://").map(|(_, rest)| rest))
        .unwrap_or(trimmed);
    let bytes = body.as_bytes();
    if bytes.first() == Some(&b'/')
        && bytes.len() > 2
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
    {
        return body[1..].to_string();
    }
    body.to_string()
}

/// 从连接 URL 解析（主机 / 端口 / 数据库）。
///
/// `db_type` 传**驱动 id**（引擎 registry key）：文件型驱动（SQLite/DuckDB）返回
/// `(None, None, Some(裸路径))`——路径必须落到 `database`，否则项目侧连接回读时地址丢失
/// （引擎 `build_connection_url` 也只认 `database`/`host`）。
///
/// 供对话框（常规 Tab 摘要）与服务层共用，保证展示与落库同源。
pub fn parse_url_host_port_db(
    db_type: &str,
    url: &str,
) -> (Option<String>, Option<i32>, Option<String>) {
    if is_file_db_driver(db_type) {
        let path = normalize_file_db_path(db_type, url);
        return (None, None, if path.is_empty() { None } else { Some(path) });
    }
    let rest = url.split("://").nth(1).unwrap_or(url);
    let (authority, database) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i + 1..].trim_end_matches('/').to_string()),
        None => (rest, String::new()),
    };
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    let (host, port) = match hostport.rfind(':') {
        Some(i) => (
            Some(hostport[..i].to_string()),
            hostport[i + 1..].parse::<i32>().ok(),
        ),
        None => (Some(hostport.to_string()), None),
    };
    (host, port, Some(database))
}

/// 由 SaveInput 构造项目连接（密码经 AES-256-GCM 加密；G_ 定义共享引用）。
fn project_connection_from_input(
    id: &str,
    input: &DataSourceSaveInput,
    url: &str,
) -> Result<ProjectConnection, CoreError> {
    let (host, port, database) = parse_url_host_port_db(&input.db_type, url);
    let password_encrypted = match &input.password {
        Some(p) if !p.is_empty() => Some(shared::crypto::encrypt_password(p)?),
        _ => None,
    };
    Ok(ProjectConnection {
        id: id.to_string(),
        name: input.name.clone(),
        driver: input.db_type.clone(),
        host,
        port,
        database,
        schema_name: input.schema_name.clone(),
        username: input.username.clone(),
        password_encrypted,
        options: input.options.clone(),
        tags: input.tags.clone(),
        use_duckdb_fed: input.use_duckdb_fed.unwrap_or(false),
        metadata_path: input.metadata_path.clone(),
        is_active: true,
        server_version: None,
        description: input.description.clone(),
        driver_id: input.driver_id.clone(),
        environment_id: input.environment_id.clone(),
        auth_config_id: input.auth_config_id.clone(),
        auth_method: input.auth_method.clone(),
        network_config_id: input.network_config_id.clone(),
        driver_properties: input.driver_properties.clone(),
        advanced_options: input.advanced_options.clone(),
        created_at: String::new(),
        updated_at: String::new(),
    })
}

/// GlobalConnectionInfo → DataSource（作用域按 ID 前缀推导，遗留 `conn-` 视为全局）。
fn map_info_to_data_source(
    info: engine::persistence::global_db::GlobalConnectionInfo,
) -> DataSource {
    let scope = if id_prefix::is_snapshot(&info.id) {
        ConnectionScope::GlobalAndProject
    } else if id_prefix::is_global_connection(&info.id) {
        ConnectionScope::Global
    } else {
        ConnectionScope::Project
    };

    DataSource {
        id: info.id,
        name: info.name,
        db_type: info.driver,
        host: info.host,
        port: info.port.map(|p| p as u16),
        database: info.database,
        schema_name: info.schema_name,
        username: info.username,
        password_encrypted: info.password_encrypted,
        description: info.description,
        driver_id: info.driver_id,
        environment_id: info.environment_id,
        auth_config_id: info.auth_config_id,
        auth_method: info.auth_method,
        network_config_id: info.network_config_id,
        driver_properties: info.driver_properties,
        advanced_options: info.advanced_options,
        options: info.options,
        tags: info.tags,
        use_duckdb_fed: info.use_duckdb_fed,
        metadata_path: info.metadata_path,
        server_version: info.server_version,
        scope,
        is_active: info.is_active,
        created_at: info.created_at,
        updated_at: info.updated_at,
    }
}

/// ProjectConnection → DataSource（作用域按 ID 前缀推导：P_ = 仅项目，GP_ = 全局+项目快照）。
///
/// 项目库不存 URL（只有 host/port/database），回读时由对话框用 `reconstruct_url` 重拼。
fn map_project_connection_to_data_source(row: ProjectConnection) -> DataSource {
    let scope = if id_prefix::is_snapshot(&row.id) {
        ConnectionScope::GlobalAndProject
    } else {
        ConnectionScope::Project
    };
    DataSource {
        id: row.id,
        name: row.name,
        db_type: row.driver,
        host: row.host,
        port: row.port.map(|p| p as u16),
        database: row.database,
        schema_name: row.schema_name,
        username: row.username,
        password_encrypted: row.password_encrypted,
        description: row.description,
        driver_id: row.driver_id,
        environment_id: row.environment_id,
        auth_config_id: row.auth_config_id,
        auth_method: row.auth_method,
        network_config_id: row.network_config_id,
        driver_properties: row.driver_properties,
        advanced_options: row.advanced_options,
        options: row.options,
        tags: row.tags,
        use_duckdb_fed: row.use_duckdb_fed,
        metadata_path: row.metadata_path,
        server_version: row.server_version,
        scope,
        is_active: row.is_active,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_effective_url_file_db_stays_bare_path() {
        // 文件型：不注入凭据（用户名/密码对文件库无意义），只规范化前缀。
        let mut input = DataSourceSaveInput::new("a", "sqlite", "sqlite:///C:/data/app.db");
        input.username = Some("root".into());
        input.password = Some("pwd".into());
        assert_eq!(build_effective_url(&input), "C:/data/app.db");
        let input = DataSourceSaveInput::new("a", "duckdb", ":memory:");
        assert_eq!(build_effective_url(&input), ":memory:");
    }

    #[test]
    fn test_normalize_file_db_path_variants() {
        // 裸路径 / 带 scheme / 三斜杠（Windows 盘符）/ Unix 绝对路径。
        for raw in ["C:/data/a.db", "sqlite://C:/data/a.db", "sqlite:///C:/data/a.db"] {
            assert_eq!(normalize_file_db_path("sqlite", raw), "C:/data/a.db", "raw={raw}");
        }
        assert_eq!(normalize_file_db_path("sqlite", "sqlite:///tmp/a.db"), "/tmp/a.db");
        assert_eq!(normalize_file_db_path("sqlite", "  "), "");
    }

    #[test]
    fn test_build_effective_url_inject() {
        let mut input = DataSourceSaveInput::new("a", "mysql", "mysql://10.0.0.1:3306/db");
        input.username = Some("root".into());
        input.password = Some("pwd".into());
        assert_eq!(
            build_effective_url(&input),
            "mysql://root:pwd@10.0.0.1:3306/db"
        );
    }

    #[test]
    fn test_build_effective_url_keeps_existing() {
        let input = DataSourceSaveInput::new("a", "mysql", "mysql://u:p@h:3306/db");
        assert_eq!(build_effective_url(&input), "mysql://u:p@h:3306/db");
    }

    #[test]
    fn test_rebuild_url_from_fields() {
        // 改主机 + 端口 + 数据库（凭据保持）
        assert_eq!(
            rebuild_url_from_fields(
                "mysql",
                "mysql://root:pw@h:3306/old",
                "127.0.0.1",
                "3307",
                "newdb",
                Some(3306),
            ),
            "mysql://root:pw@127.0.0.1:3307/newdb"
        );
        // 幂等：字段与 URL 一致 → 不改写
        assert_eq!(
            rebuild_url_from_fields("mysql", "mysql://h:3306/db", "h", "3306", "db", Some(3306)),
            "mysql://h:3306/db"
        );
        // 端口字段为空 → 用驱动默认端口
        assert_eq!(
            rebuild_url_from_fields("postgres", "postgres://u@h:5432/db", "h", "", "db", Some(5433)),
            "postgres://u@h:5433/db"
        );
        // 主机为空 → 不重建（不产生非法 URL）
        assert_eq!(
            rebuild_url_from_fields("mysql", "mysql://h:3306/db", "", "3306", "db", None),
            "mysql://h:3306/db"
        );
        // 数据库字段清空 → 去掉路径段
        assert_eq!(
            rebuild_url_from_fields("mysql", "mysql://h:3306/db", "h", "3306", "", None),
            "mysql://h:3306"
        );
        // 文件型：不被三字段重建（地址就是路径）
        assert_eq!(
            rebuild_url_from_fields("sqlite", "C:/data/a.db", "h", "1", "x", None),
            "C:/data/a.db"
        );
    }

    #[test]
    fn test_parse_url_host_port_db() {
        let (h, p, d) = parse_url_host_port_db("postgres", "postgres://u:p@h:5432/mydb");
        assert_eq!(h.as_deref(), Some("h"));
        assert_eq!(p, Some(5432));
        assert_eq!(d.as_deref(), Some("mydb"));

        // 文件型：路径必须落到 database（否则项目侧连接回读时地址丢失）。
        let (h, p, d) = parse_url_host_port_db("sqlite", "C:/data/app.db");
        assert_eq!(h, None);
        assert_eq!(p, None);
        assert_eq!(d.as_deref(), Some("C:/data/app.db"));
        let (_, _, d) = parse_url_host_port_db("sqlite", "sqlite:///C:/data/app.db");
        assert_eq!(d.as_deref(), Some("C:/data/app.db"), "盘符前的多余斜杠应去除");
        let (_, _, d) = parse_url_host_port_db("duckdb", "duckdb:///data/a.duckdb");
        assert_eq!(d.as_deref(), Some("/data/a.duckdb"), "Unix 绝对路径保持原样");
        let (_, _, d) = parse_url_host_port_db("duckdb", "  ");
        assert_eq!(d, None, "空地址不造空 database");
    }

    #[test]
    fn test_scope_mapping() {
        assert_eq!(
            map_info_to_data_source(info_with_id("G_conn_demo")).scope,
            ConnectionScope::Global
        );
        assert_eq!(
            map_info_to_data_source(info_with_id("P_conn_x")).scope,
            ConnectionScope::Project
        );
        assert_eq!(
            map_info_to_data_source(info_with_id("GP_conn_demo_20260910")).scope,
            ConnectionScope::GlobalAndProject
        );
    }

    fn info_with_id(id: &str) -> engine::persistence::global_db::GlobalConnectionInfo {
        engine::persistence::global_db::GlobalConnectionInfo {
            id: id.to_string(),
            name: "demo".to_string(),
            driver: "mysql".to_string(),
            host: None,
            port: None,
            database: None,
            schema_name: None,
            username: None,
            password_encrypted: None,
            options: None,
            tags: None,
            use_duckdb_fed: false,
            metadata_path: None,
            is_active: true,
            created_at: String::new(),
            updated_at: String::new(),
            server_version: None,
            description: None,
            driver_id: None,
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: None,
            driver_properties: None,
            advanced_options: None,
        }
    }
}
