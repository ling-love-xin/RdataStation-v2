use std::path::PathBuf;
use std::sync::Arc;

use connection::config::ConnectionMethod;
use connection::connector::TunnelGuard;
use engine::connection_manager::{ConnectionInfo, ConnectionManager, ConnectionType};
use engine::driver::registry::DriverConnectionConfig;
use engine::driver::router::DataSourceRouter;
use engine::driver::traits::{DataSourceMeta, DynDatabase};
use engine::persistence::connection_store::{self, RecentConnectionInput};
use engine::persistence::global_db::{GlobalConnectionSaveInput, GlobalDatabaseManager};
use engine::persistence::MetadataCacheManager;
use shared::error::{ConnectionError, CoreError};

/// 保存全局连接输入参数
pub struct SaveGlobalConnectionInput<'a> {
    pub conn_id: &'a str,
    pub name: &'a str,
    pub db_type: &'a str,
    pub url: &'a str,
    pub username: Option<&'a str>,
    pub password: Option<&'a str>,
    pub tags: Option<&'a str>,
    pub server_version: Option<&'a str>,
    pub description: Option<&'a str>,
    pub driver_id: Option<&'a str>,
    pub environment_id: Option<&'a str>,
    pub auth_config_id: Option<&'a str>,
    pub auth_method: Option<&'a str>,
    pub network_config_id: Option<&'a str>,
    pub options: Option<&'a str>,
    pub driver_properties: Option<&'a str>,
    pub advanced_options: Option<&'a str>,
    pub use_duckdb_fed: Option<bool>,
    pub metadata_path: Option<&'a str>,
    pub schema_name: Option<&'a str>,
}

/// 连接请求 — 消除 connect_with_type 的 22 参数反模式
/// 对应前端 ConnectDatabaseInput，增加已解析的 connection_type / network_method
pub struct ConnectRequest {
    pub conn_id: Option<String>,
    pub db_type: String,
    pub url: String,
    pub name: Option<String>,
    pub connection_type: ConnectionType,
    pub project_path: Option<String>,
    pub description: Option<String>,
    pub driver_id: Option<String>,
    pub environment_id: Option<String>,
    pub auth_config_id: Option<String>,
    pub auth_method: Option<String>,
    pub network_config_id: Option<String>,
    pub driver_properties: Option<String>,
    pub advanced_options: Option<String>,
    pub options: Option<String>,
    pub tags: Option<String>,
    pub metadata_path: Option<String>,
    pub schema_name: Option<String>,
    pub use_duckdb_fed: Option<bool>,
    pub password: Option<String>,
    pub skip_persistence: Option<bool>,
    pub network_method: Option<ConnectionMethod>,
}

/// 测试连接的配置构建输入（`DataSourceSaveInput` 的建连相关字段子集）。
///
/// 不直接复用 `ConnectRequest`：测试连接不生成连接 ID、不落库、不写最近连接记录，
/// 只关心「用什么参数建连」。
pub struct ProbeConfigInput<'a> {
    pub db_type: &'a str,
    /// 已完成 UI 凭据合并的 URL（[`DataSourceService::test`](crate::services::data_source_service::DataSourceService::test) 负责）。
    pub url: &'a str,
    pub name: &'a str,
    pub username: Option<&'a str>,
    pub password: Option<&'a str>,
    pub auth_config_id: Option<&'a str>,
    pub auth_method: Option<&'a str>,
    pub network_config_id: Option<&'a str>,
    /// 项目根（含 `.RSmeta`）；用于解析 P_/GP_ 前缀的档案。
    pub project_path: Option<&'a str>,
    pub driver_properties: Option<&'a str>,
    pub advanced_options: Option<&'a str>,
}

/// 连接服务
///
/// 负责数据库连接的生命周期管理，包括：
/// - 创建新连接
/// - 连接复用
/// - 连接切换
/// - 连接关闭
/// - 最近连接记录
/// - 连接类型转换（全局↔项目）
#[derive(Clone)]
pub struct ConnectionService {
    manager: Arc<ConnectionManager>,
    /// 隧道守卫（协议链执行产物；断开时统一释放）。
    tunnels: connection::chain::TunnelRegistry,
}

impl ConnectionService {
    /// 创建新的连接服务
    ///
    /// 隧道注册表**取自连接管理器**（而非本实例）：生产连接入口是短生命周期的（每次调用
    /// `new` 一个服务），若守卫存在实例字段里，隧道会刚建好就随实例释放（审计 #20）。
    /// 传独立管理器的测试仍天然隔离。
    pub fn new(manager: Arc<ConnectionManager>) -> Self {
        let tunnels = manager.tunnels().clone();
        Self { manager, tunnels }
    }

    /// 诊断：两个服务实例是否共用同一张隧道注册表（同一管理器 = 共用）。
    pub fn shares_tunnels_with(&self, other: &ConnectionService) -> bool {
        self.tunnels.shares_with(&other.tunnels)
    }

    /// 创建或获取数据库连接（默认全局连接）
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接唯一标识（可选，不提供则自动生成）
    /// * `db_type` - 数据库类型 (mysql, postgres, sqlite, duckdb)
    /// * `url` - 数据库连接 URL
    /// * `name` - 连接名称（可选）
    ///
    /// # Returns
    ///
    /// 返回连接 ID 和数据库实例
    pub async fn connect(
        &self,
        conn_id: Option<String>,
        db_type: &str,
        url: &str,
        name: Option<String>,
    ) -> Result<(String, DynDatabase), CoreError> {
        self.connect_with_type(ConnectRequest {
            conn_id,
            db_type: db_type.to_string(),
            url: url.to_string(),
            name,
            connection_type: ConnectionType::Global,
            project_path: None,
            description: None,
            driver_id: None,
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: None,
            driver_properties: None,
            advanced_options: None,
            options: None,
            tags: None,
            metadata_path: None,
            schema_name: None,
            use_duckdb_fed: None,
            password: None,
            skip_persistence: None,
            network_method: None,
        })
        .await
    }

    /// 创建或获取数据库连接（指定连接类型）
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接唯一标识（可选，不提供则自动生成）
    /// * `db_type` - 数据库类型 (mysql, postgres, sqlite, duckdb)
    /// * `url` - 数据库连接 URL
    /// * `name` - 连接名称（可选）
    /// * `connection_type` - 连接类型（全局/项目）
    /// * `project_path` - 项目路径（仅项目连接时需要）
    /// * `description` - 连接描述（可选）
    /// * `network_method` - 网络连接方式（可选，用于 SSH 隧道/SSL 加密/代理等）
    /// * `driver_id` - 驱动 ID（可选，数据源模块字段）
    /// * `environment_id` - 环境 ID（可选）
    /// * `auth_config_id` - 认证配置 ID（可选）
    /// * `network_config_id` - 网络配置 ID（可选）
    /// * `driver_properties` - 驱动属性 JSON（可选）
    /// * `advanced_options` - 高级选项 JSON（可选）
    /// * `skip_persistence` - 跳过持久化到 SQLite（测试连接等场景，默认 false）
    ///
    /// # Returns
    ///
    /// 返回连接 ID 和数据库实例
    pub async fn connect_with_type(
        &self,
        req: ConnectRequest,
    ) -> Result<(String, DynDatabase), CoreError> {
        let ConnectRequest {
            conn_id,
            db_type,
            url,
            name,
            connection_type,
            project_path,
            description,
            driver_id,
            environment_id,
            auth_config_id,
            auth_method,
            network_config_id,
            driver_properties,
            advanced_options,
            options,
            tags,
            metadata_path,
            schema_name,
            use_duckdb_fed,
            password,
            skip_persistence,
            network_method,
        } = req;

        // 参数校验
        if url.is_empty() {
            return Err(CoreError::connection(ConnectionError::InvalidConfig {
                conn_id: "unknown".to_string(),
                reason: "Database URL cannot be empty".to_string(),
            }));
        }

        // 生成连接 ID：统一使用 URL 哈希，确保：
        //   - 短且唯一（8位 hex）
        //   - 文件系统安全（无 Windows 非法字符 : @ / \ 等）
        //   - 全局/项目双链路一致
        let conn_id = conn_id.unwrap_or_else(|| {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let type_prefix = match connection_type {
                ConnectionType::Global => "global",
                ConnectionType::Project => "project",
            };

            let mut hasher = DefaultHasher::new();
            url.hash(&mut hasher);
            let hash = hasher.finish();
            format!("{}-{}-{:x}", type_prefix, db_type, hash)
        });

        // 连接显示名称：用户指定 > 自动生成（db_type@host 简洁格式）
        let connection_name = name.unwrap_or_else(|| {
            let host = url
                .split('@')
                .nth(1)
                .and_then(|s| s.split('/').next())
                .map(|s| s.split('?').next().unwrap_or(s))
                .unwrap_or("localhost");
            format!("{}@{}", db_type.to_uppercase(), host)
        });

        // 检查是否已有连接（基于 conn_id）
        if let Some(db) = self.manager.get_connection(&conn_id).await {
            tracing::info!(conn_id = %conn_id, "Connection already exists, reusing it");
            return Ok((conn_id, db));
        }

        // 对于文件型数据库，额外检查是否有相同 URL 的连接（避免文件锁冲突）
        if url.starts_with("sqlite://") || url.starts_with("duckdb://") {
            let all_connections = self.manager.get_all_connection_info().await;
            for conn_info in all_connections {
                if conn_info.url == url {
                    tracing::info!(url = %url, conn_id = %conn_info.id, "Connection with URL already exists");
                    if let Some(db) = self.manager.get_connection(&conn_info.id).await {
                        tracing::info!(conn_id = %conn_info.id, "Reusing existing connection");
                        return Ok((conn_info.id, db));
                    }

                    // 如果连接管理器中没有该连接（可能已被关闭），但连接信息仍存在
                    // 需要先移除旧的连接信息，然后创建新连接
                    tracing::info!(
                        "旧连接 {} 已不存在于连接管理器中，准备移除旧信息并创建新连接",
                        conn_info.id
                    );
                    self.manager.remove_connection(&conn_info.id).await;
                    // 继续创建新连接
                    break;
                }
            }
        }

        // 创建新连接
        tracing::info!("Creating new connection with ID: {}", conn_id);

        // 如果有 auth_config_id，从数据库中读取认证凭据并注入到 URL
        // （规则与测试连接同源；档案缺失 / 注入失败直接报错，不静默回退直连）。
        let url_with_auth = Self::inject_auth_config_credentials(
            None,
            &url,
            auth_config_id.as_deref(),
            auth_method.as_deref(),
            connection_type,
            project_path.as_deref(),
        )
        .await?;
        if auth_config_id.is_some() {
            tracing::info!(conn_id = %conn_id, auth_id = ?auth_config_id, "认证档案凭据已注入 URL");
        }

        // 引用了网络档案但入口没解析出连接方式（档案被删 / 类型未知 / 内容非法）：
        // 拒绝静默直连（A2）——用户以为走隧道，实际会把数据库端暴露到公网链路上。
        if network_config_id.is_some() && network_method.is_none() {
            return Err(CoreError::from(format!(
                "引用的网络配置无法解析（{}）：请在「网络」Tab 重新选择，或检查档案类型与内容",
                network_config_id.as_deref().unwrap_or("")
            )));
        }

        // 应用网络连接方式（SSH 隧道 / SSL / 代理；协议链执行在 connection::chain）
        let (effective_url, tunnel_guards) = connection::chain::apply_network_method(
            &url_with_auth,
            &network_method,
            &conn_id,
            &db_type,
        )
        .await?;

        // 注册隧道守卫，确保隧道在连接生命周期内保持存活
        if !tunnel_guards.is_empty() {
            self.tunnels.insert(&conn_id, tunnel_guards).await;
            tracing::info!(
                conn_id = %conn_id,
                count = self.tunnels.count(&conn_id).await,
                "已注册隧道守卫"
            );
        }

        tracing::info!(
            conn_id = %conn_id,
            effective_url_has_creds = effective_url.contains('@'),
            "即将创建数据库连接（URL凭据={}）",
            effective_url.contains('@')
        );
        let db = match self
            .create_database(&db_type, &effective_url, driver_properties.as_deref())
            .await
        {
            Ok(db) => db,
            Err(e) => {
                // 隧道已登记但连接未建立：不回收会泄漏本地端口与后台任务
                self.release_tunnels(&conn_id, "连接建立失败").await;
                return Err(e);
            }
        };
        let server_version = db.meta().server_version.clone();
        let safe_url = connection::url::mask_password_in_url(&url_with_auth);

        // 创建连接信息
        let info = ConnectionInfo {
            id: conn_id.clone(),
            name: connection_name.clone(),
            db_type: db_type.to_string(),
            url: safe_url.clone(),
            server_version: server_version.clone(),
            connection_type,
            project_id: project_path.clone(),
            driver_id: driver_id.clone(),
            environment_id: environment_id.clone(),
            auth_config_id: auth_config_id.clone(),
            auth_method: auth_method.clone(),
            network_config_id: network_config_id.clone(),
            driver_properties: driver_properties.clone(),
            advanced_options: advanced_options.clone(),
            description: description.clone(),
            created_at: std::time::Instant::now(),
        };

        // 转换为 DriverConnectionConfig（用于重连）
        let mut driver_config =
            engine::driver::registry::DriverConnectionConfig::new(db_type.clone())
                .with_url_override(url.clone())
                .with_name(&connection_name);

        if let Some(ref opts_json) = advanced_options {
            Self::apply_advanced_options(&mut driver_config, opts_json);
        }
        if let Some(ref props_json) = driver_properties {
            Self::apply_driver_properties(&mut driver_config, props_json);
        }

        // 添加到连接管理器
        if let Err(e) = self
            .manager
            .add_connection(conn_id.clone(), Arc::clone(&db), info, driver_config)
            .await
        {
            // 同上：注册失败也要回收隧道
            self.release_tunnels(&conn_id, "连接注册失败").await;
            return Err(e);
        }

        // M3 本地加速：连接建立成功后注册 DuckDB Secret（联邦查询直连源库）。
        // 失败仅告警，不影响连接本身（加速是增强而非依赖）。
        crate::services::secret_integration::ensure_secret_registered(&conn_id, &db_type, &url);

        // NOTE: 元数据缓存不在连接时立即创建，改为懒加载。
        // 首次查询 schema / table / column 时通过 L2 cache write 路径自动创建。

        // 对于全局连接，保存到全局 SQLite 数据库（skip_persistence 时跳过）
        if !skip_persistence.unwrap_or(false) && connection_type == ConnectionType::Global {
            // 从 URL 中解析 username 和 password
            let (url_username, url_password) = connection::url::extract_credentials_from_url(&url);
            // 优先使用直接传入的 password，回退到 URL 解析
            let effective_password = password.or(url_password);

            // 标签：优先使用输入值，无输入时默认 ["global"]
            let final_tags = tags.clone().or(Some("[\"global\"]".to_string()));
            if let Err(e) = self
                .save_global_connection_to_db(SaveGlobalConnectionInput {
                    conn_id: &conn_id,
                    name: &connection_name,
                    db_type: &db_type,
                    url: &safe_url,
                    username: url_username.as_deref(),
                    password: effective_password.as_deref(),
                    tags: final_tags.as_deref(),
                    server_version: server_version.as_deref(),
                    description: description.as_deref(),
                    driver_id: driver_id.as_deref(),
                    environment_id: environment_id.as_deref(),
                    auth_config_id: auth_config_id.as_deref(),
                    auth_method: auth_method.as_deref(),
                    network_config_id: network_config_id.as_deref(),
                    options: options.as_deref(),
                    driver_properties: driver_properties.as_deref(),
                    advanced_options: advanced_options.as_deref(),
                    use_duckdb_fed,
                    metadata_path: metadata_path.as_deref(),
                    schema_name: schema_name.as_deref(),
                })
                .await
            {
                tracing::warn!("保存全局连接信息到 SQLite 失败: {}", e);
            }
        }

        // 保存到最近连接记录（skip_persistence 时跳过）
        if !skip_persistence.unwrap_or(false) {
            if let Err(e) = connection_store::save_recent_connection(RecentConnectionInput {
                name: &connection_name,
                db_type: &db_type,
                url: &safe_url,
                conn_id: Some(&conn_id),
                description: description.as_deref(),
                driver_id: driver_id.as_deref(),
                environment_id: environment_id.as_deref(),
                auth_config_id: auth_config_id.as_deref(),
                auth_method: auth_method.as_deref(),
                network_config_id: network_config_id.as_deref(),
                driver_properties: driver_properties.as_deref(),
                advanced_options: advanced_options.as_deref(),
            }) {
                tracing::warn!("Failed to save connection history: {}", e);
            }
        }

        Ok((conn_id, db))
    }

    /// 保存全局连接信息到全局 SQLite 数据库
    async fn save_global_connection_to_db(
        &self,
        input: SaveGlobalConnectionInput<'_>,
    ) -> Result<(), CoreError> {
        use engine::migration::global_init;

        let global_db = global_init::get_global_db_manager().ok_or_else(|| {
            CoreError::common(shared::error::CommonError::General(
                "Global database manager not initialized".to_string(),
            ))
        })?;

        global_db
            .save_global_connection(GlobalConnectionSaveInput {
                conn_id: input.conn_id,
                name: input.name,
                db_type: input.db_type,
                url: input.url,
                username: input.username,
                password: input.password,
                tags: input.tags,
                server_version: input.server_version,
                description: input.description,
                driver_id: input.driver_id,
                environment_id: input.environment_id,
                auth_config_id: input.auth_config_id,
                auth_method: input.auth_method,
                network_config_id: input.network_config_id,
                options: input.options,
                driver_properties: input.driver_properties,
                advanced_options: input.advanced_options,
                use_duckdb_fed: input.use_duckdb_fed,
                metadata_path: input.metadata_path,
                schema_name: input.schema_name,
            })
            .await
    }

    /// 脱敏 URL 中的密码，替换为 ***
    pub fn mask_password_in_url(url: &str) -> String {
        connection::url::mask_password_in_url(url)
    }

    /// 从数据库中加载认证配置数据
    ///
    /// 根据连接类型和认证配置 ID，从全局或项目数据库中读取认证配置，
    /// 并返回解密后的 auth_data JSON。
    ///
    /// 查询优先级：全局 DB → 项目 DB（project_path 存在时，不依赖 connection_type，
    /// 与 load_auth_data_from_db_for_network 行为一致，确保 test_connection（Global 类型）
    /// 也能查询到项目级 P_/GP_ 认证配置）
    ///
    /// `global_db` 为 `None` 时回退进程全局单例（连接入口的既有行为）；
    /// 持有库句柄的调用方（如 `DataSourceService`）可传入自己的库，测试可注入临时库。
    async fn load_auth_data_from_db(
        global_db: Option<&GlobalDatabaseManager>,
        auth_id: &str,
        connection_type: ConnectionType,
        project_path: Option<&str>,
    ) -> Result<Option<(String, String)>, CoreError> {
        use engine::persistence::auth_store;

        // 优先尝试从全局数据库读取；返回 (auth_type, 解密后的 auth_data)，
        // 供调用方在连接未记录认证方法时回退到配置声明的方法。
        let global: Option<&GlobalDatabaseManager> = match global_db {
            Some(g) => Some(g),
            None => engine::migration::get_global_db_manager(),
        };
        if let Some(gdb) = global {
            if let Ok(Some(auth_config)) = gdb.get_auth_config(auth_id).await {
                let auth_data = auth_store::decrypt_auth_data(&auth_config.auth_data)?;
                return Ok(Some((auth_config.auth_type, auth_data)));
            }
        }

        // 全局未找到，尝试从项目数据库读取（不依赖 connection_type，
        // 确保 test_connection 等非 Project 类型也能查询到项目级认证配置）
        let should_check_project =
            connection_type == ConnectionType::Project || project_path.is_some();
        if should_check_project {
            if let Some(pp) = project_path {
                let db_path = std::path::Path::new(pp).join(".RSmeta").join("project.db");
                if db_path.exists() {
                    let conn = rusqlite::Connection::open(&db_path)
                        .map_err(|e| CoreError::from(format!("打开项目数据库失败: {}", e)))?;
                    if let Some(auth_config) = auth_store::get_auth_config(&conn, auth_id)? {
                        let auth_data = auth_store::decrypt_auth_data(&auth_config.auth_data)?;
                        return Ok(Some((auth_config.auth_type, auth_data)));
                    }
                }
            }
        }

        Ok(None)
    }

    /// 按认证档案注入凭据到 URL（`connect` 与测试连接共用，保证同源）。
    ///
    /// 认证方法优先取连接记录上的 `auth_method`；缺失时回退到认证配置声明的
    /// `auth_type`——否则「引用了档案但没选方法」的连接会静默跳过凭据注入。
    ///
    /// **严格模式（A2）**：档案不存在 / 读取失败 / 注入失败一律返回 `Err`，不再回退到
    /// “无凭据直连”——静默降级会让用户以为在用专用账号 / 跳板机（安全风险），
    /// 真实原因（档案被删 / 改名 / 数据损坏）却看不到。
    async fn inject_auth_config_credentials(
        global_db: Option<&GlobalDatabaseManager>,
        url: &str,
        auth_config_id: Option<&str>,
        auth_method: Option<&str>,
        connection_type: ConnectionType,
        project_path: Option<&str>,
    ) -> Result<String, CoreError> {
        let Some(auth_id) = auth_config_id else {
            return Ok(url.to_string());
        };
        let (config_auth_type, auth_data) =
            Self::load_auth_data_from_db(global_db, auth_id, connection_type, project_path)
                .await?
                .ok_or_else(|| {
                    CoreError::from(format!(
                        "引用的认证配置不存在（{auth_id}）：请在「数据库认证」重新选择或新建"
                    ))
                })?;
        let method = auth_method.unwrap_or(config_auth_type.as_str());
        connection::url_params::inject_auth_into_url(url, method, &auth_data)
            .map_err(|e| CoreError::from(format!("认证配置凭据注入失败（{auth_id}）：{e}")))
    }

    /// 解析 advanced_options JSON 并应用到 DriverConnectionConfig
    ///
    /// advanced_options 结构（来自 AdvancedTab emit）：
    /// ```json
    /// {
    ///   "performance": { "poolSize": 10, "queryTimeout": 60, "connectTimeout": 30, ... },
    ///   "connection": { "connectTimeout": 30, "queryTimeout": 0, ... },
    ///   "encoding": "UTF-8"
    /// }
    /// ```
    fn apply_advanced_options(
        config: &mut engine::driver::registry::DriverConnectionConfig,
        json: &str,
    ) {
        let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
            return;
        };
        let Some(obj) = root.as_object() else { return };

        if let Some(perf) = obj.get("performance").and_then(|v| v.as_object()) {
            if let Some(v) = perf.get("poolSize").and_then(|v| v.as_u64()) {
                config.pool_size = Some(v as u32);
            }
            if let Some(v) = perf.get("queryTimeout").and_then(|v| v.as_u64()) {
                if v > 0 {
                    config.query_timeout = Some(v as u32);
                }
            }
            if let Some(v) = perf.get("heartbeat").and_then(|v| v.as_u64()) {
                config.heartbeat_interval = Some(v as u32);
            }
            if let Some(v) = perf.get("maxReconnect").and_then(|v| v.as_u64()) {
                config.max_reconnect = Some(v as u32);
            }
        }

        if let Some(conn) = obj.get("connection").and_then(|v| v.as_object()) {
            if config.connect_timeout.is_none() {
                if let Some(v) = conn.get("connectTimeout").and_then(|v| v.as_u64()) {
                    config.connect_timeout = Some(v as u32);
                }
            }
            if config.query_timeout.is_none() {
                if let Some(v) = conn.get("queryTimeout").and_then(|v| v.as_u64()) {
                    if v > 0 {
                        config.query_timeout = Some(v as u32);
                    }
                }
            }
            if config.heartbeat_interval.is_none() {
                if let Some(v) = conn.get("keepAlive").and_then(|v| v.as_u64()) {
                    config.heartbeat_interval = Some(v as u32);
                }
            }
            if config.max_reconnect.is_none() {
                if let Some(v) = conn.get("maxReconnect").and_then(|v| v.as_u64()) {
                    config.max_reconnect = Some(v as u32);
                }
            }
        }

        if let Some(enc) = obj.get("encoding").and_then(|v| v.as_str()) {
            config.encoding = Some(enc.to_string());
        }
    }

    /// 解析 driver_properties JSON 并应用到 DriverConnectionConfig
    fn apply_driver_properties(
        config: &mut engine::driver::registry::DriverConnectionConfig,
        json: &str,
    ) {
        let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
            return;
        };
        let Some(obj) = root.as_object() else { return };
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                config.driver_properties.insert(k.clone(), s.to_string());
            } else {
                config.driver_properties.insert(k.clone(), v.to_string());
            }
        }
    }

    /// 构建“测试连接”用的驱动配置（与 `connect` 同源）。
    ///
    /// 与 `connect` 共用同一套规则：
    /// 1. 认证档案凭据按 `auth_method`（回退档案 `auth_type`）注入 URL；
    /// 2. 网络档案解析为 `ConnectionMethod` 后**真实建立隧道**并改写 URL——
    ///    测试连接必须验证「档案配好就能连」，不建隧道会给出与真实连接相反的结论；
    /// 3. 驱动属性 / 高级选项照常应用；`username` / `password` 从最终 URL 回填
    ///    （`url_override` 以外的字段路径也需要它们，如驱动校验与旧版 URL 构建）。
    ///
    /// 返回 `(配置, 隧道守卫, 说明)`：
    /// - `隧道守卫` 必须在探测期间保持存活（drop 即关闭隧道），由调用方持有；
    /// - `说明` 是 UI 可见的测试范围提示（档案已生效 / 隧道已建立）。
    ///
    /// **严格模式（A2）**：引用的认证 / 网络档案不存在、读不出或无法注入时**返回错误**，
    /// 而不是回退到“直连 + 无凭据”——测试结果必须与真实连接的可能结果一致。
    ///
    /// `global_db` 为 `None` 时回退进程全局单例；`DataSourceService` 传入自身库，
    /// 测试可注入临时库。
    ///
    /// 公开为测试接缝（集成测试在同一进程内注入临时库，断言档案 → 配置的完整链路），
    /// 生产调用方只有 `DataSourceService::test`。
    pub async fn build_probe_config(
        global_db: Option<&GlobalDatabaseManager>,
        input: ProbeConfigInput<'_>,
    ) -> Result<(DriverConnectionConfig, Vec<TunnelGuard>, Vec<String>), CoreError> {
        let mut notes: Vec<String> = Vec::new();

        // —— 1) 认证档案凭据（与 connect 同序：auth_method 优先，回退档案 auth_type）
        let connection_type = if input.project_path.is_some() {
            ConnectionType::Project
        } else {
            ConnectionType::Global
        };
        let url = Self::inject_auth_config_credentials(
            global_db,
            input.url,
            input.auth_config_id,
            input.auth_method,
            connection_type,
            input.project_path,
        )
        .await?;
        if input.auth_config_id.is_some() {
            notes.push("已应用认证档案凭据".to_string());
        }

        // —— 2) 网络档案：隧道在本函数调用方的作用域内建立与释放
        let mut effective_url = url;
        let mut guards: Vec<TunnelGuard> = Vec::new();
        if let Some(net_id) = input.network_config_id {
            match resolve_network_method_with_project(Some(net_id), input.project_path).await {
                Ok(Some(method)) => {
                    let probe_id = format!("probe-{}", input.name);
                    match connection::chain::apply_network_method(
                        &effective_url,
                        &Some(method),
                        &probe_id,
                        input.db_type,
                    )
                    .await
                    {
                        Ok((rewritten, g)) => {
                            if !g.is_empty() {
                                notes.push("已建立网络档案隧道".to_string());
                            }
                            effective_url = rewritten;
                            guards = g;
                        }
                        // 隧道建不起来 = 真实连接同样起不来：直接失败，不拿未改写的地址探测
                        Err(e) => {
                            return Err(CoreError::from(format!("网络档案应用失败：{e}")));
                        }
                    }
                }
                // 引用了档案但解析不出连接方式（已删 / 类型未知 / 内容非法）：
                // 直接失败，不拿直连结果冒充“档案已生效”（A2 严格模式）
                Ok(None) => {
                    return Err(CoreError::from(format!(
                        "引用的网络配置无法解析（{net_id}）：请检查档案类型与内容"
                    )))
                }
                Err(e) => return Err(e),
            }
        }

        // —— 3) 驱动配置：URL 为准（url_override），字段凭据从最终 URL 回填
        let mut config = DriverConnectionConfig::new(input.db_type)
            .with_url_override(&effective_url)
            .with_name(input.name);
        let (url_user, url_pass) = connection::url::extract_credentials_from_url(&effective_url);
        if let Some(user) = input.username.map(str::to_string).or(url_user) {
            config = config.with_username(user);
        }
        if let Some(pass) = input.password.map(str::to_string).or(url_pass) {
            config = config.with_password(pass);
        }
        if let Some(opts) = input.advanced_options {
            Self::apply_advanced_options(&mut config, opts);
        }
        if let Some(props) = input.driver_properties {
            Self::apply_driver_properties(&mut config, props);
        }

        Ok((config, guards, notes))
    }

    /// 根据数据库类型创建对应的数据库实例
    /// 通过 DataSourceRouter 路由到 DriverRegistry 动态创建
    async fn create_database(
        &self,
        db_type: &str,
        url: &str,
        driver_properties: Option<&str>,
    ) -> Result<DynDatabase, CoreError> {
        let mut config = DriverConnectionConfig::new(db_type).with_url_override(url);
        if let Some(props_json) = driver_properties {
            Self::apply_driver_properties(&mut config, props_json);
        }
        DataSourceRouter::route(config).await
    }

    /// 确保连接级元数据缓存已初始化（懒加载，幂等）
    ///
    /// 元数据缓存跟随连接信息：
    /// - 全局连接：{data_dir}/system/global_metadata/conn_{id}.sqlite
    /// - 项目连接：{project_path}/meta/connection_metadata/conn_{id}.sqlite
    ///
    /// 如果缓存文件已存在则跳过，否则创建并执行迁移。
    /// 调用时机：首次查询 schema / table / column 时。
    pub fn ensure_metadata_cache(
        conn_id: &str,
        connection_type: engine::persistence::ConnectionType,
        project_path: Option<&str>,
    ) -> Result<(), CoreError> {
        let cache_manager = MetadataCacheManager::new(conn_id, connection_type, project_path)?;

        if cache_manager.exists() {
            tracing::debug!(path = ?cache_manager.db_path(), "Metadata cache already exists, skipping init");
            return Ok(());
        }

        let _conn = cache_manager.open()?;

        tracing::debug!(
            "Metadata cache lazily initialized ({:?}): {:?}",
            connection_type,
            cache_manager.db_path()
        );
        Ok(())
    }

    /// 获取现有连接
    pub async fn get_connection(&self, conn_id: &str) -> Option<DynDatabase> {
        self.manager.get_connection(&conn_id.to_string()).await
    }

    /// 切换活动连接
    pub async fn switch_connection(&self, conn_id: &str) -> Result<(), CoreError> {
        self.manager.switch_connection(&conn_id.to_string()).await
    }

    /// 获取当前活动连接
    pub async fn get_active_connection(&self) -> Option<DynDatabase> {
        self.manager.get_active_connection().await.map(|(_, db)| db)
    }

    /// 获取当前活动连接 ID
    pub async fn get_active_conn_id(&self) -> Option<String> {
        self.manager.get_active_conn_id().await
    }

    /// 回收某连接的隧道守卫（守卫 drop 即关闭本地端口与后台 accept 循环）。
    async fn release_tunnels(&self, conn_id: &str, reason: &str) {
        let guards = self.tunnels.take(conn_id).await;
        if !guards.is_empty() {
            tracing::info!(conn_id = %conn_id, count = guards.len(), reason, "清理隧道守卫");
            drop(guards);
        }
    }

    /// 某连接当前登记的隧道数量（诊断 / 测试用）。
    pub async fn tunnel_count(&self, conn_id: &str) -> usize {
        self.tunnels.count(conn_id).await
    }

    /// 关闭指定连接（**保留元数据缓存**）。
    ///
    /// 依据设计 `database-navigator-prototype-design.md` §5.2：断开只关闭运行时连接，
    /// 不删除 L2 元数据缓存（支持离线浏览 / 重连秒开）；清理仅经显式「缓存管理」入口。
    pub async fn close_connection(&self, conn_id: &str) -> Result<(), CoreError> {
        // 清理隧道守卫（释放本地端口 + 取消后台任务）
        self.release_tunnels(conn_id, "关闭连接").await;

        // 从连接管理器中移除连接（缓存文件保留）
        self.manager.remove_connection(&conn_id.to_string()).await;

        Ok(())
    }

    /// 关闭所有连接
    pub async fn close_all_connections(&self) -> Result<(), CoreError> {
        self.tunnels.clear().await;
        self.manager.close_all_connections().await;
        Ok(())
    }

    /// 获取所有连接信息
    pub async fn list_connections(&self) -> Vec<ConnectionInfo> {
        self.manager.get_all_connection_info().await
    }

    /// 获取连接的数据源元数据
    pub async fn get_connection_meta(&self, conn_id: &str) -> Option<DataSourceMeta> {
        self.manager
            .get_connection(&conn_id.to_string())
            .await
            .map(|db| db.meta())
    }

    /// 检查连接是否存在
    pub async fn has_connection(&self, conn_id: &str) -> bool {
        self.manager
            .get_connection(&conn_id.to_string())
            .await
            .is_some()
    }

    /// 获取最近使用的连接列表
    pub fn get_recent_connections(
        &self,
    ) -> Result<Vec<connection_store::ConnectionRecord>, CoreError> {
        connection_store::get_recent_connections().map_err(|e| {
            CoreError::storage(shared::error::StorageError::read(
                "recent_connections",
                e.to_string(),
            ))
        })
    }

    /// 删除最近连接记录
    pub fn remove_recent_connection(&self, name: &str) -> Result<(), CoreError> {
        connection_store::remove_recent_connection(name).map_err(|e| {
            CoreError::storage(shared::error::StorageError::write(
                "recent_connections",
                e.to_string(),
            ))
        })
    }

    /// 获取连接信息
    pub async fn get_connection_info(&self, conn_id: &str) -> Result<ConnectionInfo, CoreError> {
        self.manager
            .get_connection_info(&conn_id.to_string())
            .await
            .ok_or_else(|| CoreError::connection(ConnectionError::NotFound(conn_id.to_string())))
    }

    /// 执行 SQL 查询
    pub async fn execute_sql(
        &self,
        conn_id: Option<String>,
        sql: &str,
    ) -> Result<shared::models::QueryResult, CoreError> {
        let db = if let Some(id) = conn_id {
            self.manager
                .get_connection(&id)
                .await
                .ok_or_else(|| CoreError::connection(ConnectionError::NotFound(id.clone())))?
        } else {
            self.manager
                .get_active_connection()
                .await
                .map(|(_, db)| db)
                .ok_or_else(|| CoreError::connection(ConnectionError::NoActiveConnection))?
        };

        let result = db.query(sql).await?;
        Ok(result)
    }

    /// 转换连接类型：全局 → 项目
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接 ID
    /// * `project_id` - 目标项目 ID
    ///
    /// # Returns
    ///
    /// 返回新的连接信息
    pub async fn convert_to_project_connection(
        &self,
        conn_id: &str,
        project_id: &str,
    ) -> Result<ConnectionInfo, CoreError> {
        use shared::error::CommonError;

        let conn_id_string = conn_id.to_string();

        // 获取原连接信息
        let old_info = self
            .manager
            .get_connection_info(&conn_id_string)
            .await
            .ok_or_else(|| CoreError::connection(ConnectionError::NotFound(conn_id.to_string())))?;

        // 验证原连接是全局连接
        if old_info.connection_type != ConnectionType::Global {
            return Err(CoreError::common(CommonError::General(format!(
                "Connection {} is not a global connection",
                conn_id
            ))));
        }

        // 复制元数据文件到项目目录
        let old_meta_path = self.get_global_metadata_path(conn_id);
        let new_meta_path = self.get_project_metadata_path(conn_id, project_id);

        if old_meta_path.exists() {
            if let Some(parent) = new_meta_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to create project metadata directory: {}",
                        e
                    )))
                })?;
            }

            std::fs::copy(&old_meta_path, &new_meta_path).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to copy metadata file: {}",
                    e
                )))
            })?;

            tracing::info!(
                "Copied metadata from {:?} to {:?}",
                old_meta_path,
                new_meta_path
            );
        }

        // 更新连接信息
        let new_info = ConnectionInfo {
            id: old_info.id.clone(),
            name: old_info.name.clone(),
            db_type: old_info.db_type.clone(),
            url: old_info.url.clone(),
            server_version: old_info.server_version.clone(),
            connection_type: ConnectionType::Project,
            project_id: Some(project_id.to_string()),
            driver_id: old_info.driver_id.clone(),
            environment_id: old_info.environment_id.clone(),
            auth_config_id: old_info.auth_config_id.clone(),
            auth_method: old_info.auth_method.clone(),
            network_config_id: old_info.network_config_id.clone(),
            driver_properties: old_info.driver_properties.clone(),
            advanced_options: old_info.advanced_options.clone(),
            description: old_info.description.clone(),
            created_at: old_info.created_at,
        };

        // 更新连接管理器中的信息
        self.manager
            .update_connection_info(&conn_id_string, new_info.clone())
            .await?;

        tracing::info!("Connection {} converted from global to project", conn_id);
        Ok(new_info)
    }

    /// 转换连接类型：项目 → 全局
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接 ID
    ///
    /// # Returns
    ///
    /// 返回新的连接信息
    pub async fn convert_to_global_connection(
        &self,
        conn_id: &str,
    ) -> Result<ConnectionInfo, CoreError> {
        use shared::error::CommonError;

        let conn_id_string = conn_id.to_string();

        // 获取原连接信息
        let old_info = self
            .manager
            .get_connection_info(&conn_id_string)
            .await
            .ok_or_else(|| CoreError::connection(ConnectionError::NotFound(conn_id.to_string())))?;

        // 验证原连接是项目连接
        if old_info.connection_type != ConnectionType::Project {
            return Err(CoreError::common(CommonError::General(format!(
                "Connection {} is not a project connection",
                conn_id
            ))));
        }

        // 移动元数据文件到全局目录
        let project_id = old_info.project_id.as_deref().ok_or_else(|| {
            CoreError::common(CommonError::General("Project ID is missing".to_string()))
        })?;

        let old_meta_path = self.get_project_metadata_path(conn_id, project_id);
        let new_meta_path = self.get_global_metadata_path(conn_id);

        if old_meta_path.exists() {
            if let Some(parent) = new_meta_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "Failed to create global metadata directory: {}",
                        e
                    )))
                })?;
            }

            std::fs::copy(&old_meta_path, &new_meta_path).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to copy metadata file: {}",
                    e
                )))
            })?;

            // 删除原项目元数据文件
            let _ = std::fs::remove_file(&old_meta_path);

            tracing::info!(
                "Moved metadata from {:?} to {:?}",
                old_meta_path,
                new_meta_path
            );
        }

        // 更新连接信息
        let new_info = ConnectionInfo {
            id: old_info.id.clone(),
            name: old_info.name.clone(),
            db_type: old_info.db_type.clone(),
            url: old_info.url.clone(),
            server_version: old_info.server_version.clone(),
            connection_type: ConnectionType::Global,
            project_id: None,
            driver_id: old_info.driver_id.clone(),
            environment_id: old_info.environment_id.clone(),
            auth_config_id: old_info.auth_config_id.clone(),
            auth_method: old_info.auth_method.clone(),
            network_config_id: old_info.network_config_id.clone(),
            driver_properties: old_info.driver_properties.clone(),
            advanced_options: old_info.advanced_options.clone(),
            description: old_info.description.clone(),
            created_at: old_info.created_at,
        };

        // 更新连接管理器中的信息
        self.manager
            .update_connection_info(&conn_id_string, new_info.clone())
            .await?;

        tracing::info!("Connection {} converted from project to global", conn_id);
        Ok(new_info)
    }

    /// 获取全局连接的元数据路径
    fn get_global_metadata_path(&self, conn_id: &str) -> PathBuf {
        let cache_manager =
            MetadataCacheManager::new(conn_id, engine::persistence::ConnectionType::Global, None);
        cache_manager
            .map(|m| m.db_path().clone())
            .unwrap_or_else(|_| PathBuf::from(".").join(format!("conn_global_{}.sqlite", conn_id)))
    }

    /// 获取项目连接的元数据路径
    fn get_project_metadata_path(&self, conn_id: &str, project_id: &str) -> PathBuf {
        let cache_manager = MetadataCacheManager::new(
            conn_id,
            engine::persistence::ConnectionType::Project,
            Some(project_id),
        );
        cache_manager
            .map(|m| m.db_path().clone())
            .unwrap_or_else(|_| {
                PathBuf::from(project_id)
                    .join(".RSmeta")
                    .join("metadata")
                    .join(format!("conn_project_{}.sqlite", conn_id))
            })
    }

    /// 检测项目中是否存在全局连接
    ///
    /// # Arguments
    ///
    /// * `project_id` - 项目 ID
    ///
    /// # Returns
    ///
    /// 返回全局连接列表
    pub async fn detect_global_connections_in_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<ConnectionInfo>, CoreError> {
        let connections = self.manager.get_all_connection_info().await;
        let global_connections: Vec<ConnectionInfo> = connections
            .into_iter()
            .filter(|info| {
                info.connection_type == ConnectionType::Global
                    && info.project_id.as_deref() == Some(project_id)
            })
            .collect();

        Ok(global_connections)
    }
}

/// 从全局 DB 解析 network_config_id → ConnectionMethod
///
/// 只查询全局 network_configs 表（测试连接场景）
/// 根据 config 中的 network_type 字段进行 JSON 反序列化
pub async fn resolve_network_method(
    network_config_id: Option<&str>,
) -> Result<Option<ConnectionMethod>, CoreError> {
    resolve_network_method_with_project(network_config_id, None).await
}

/// 从全局 + 项目 DB 解析 network_config_id → ConnectionMethod
///
/// 支持 ID 前缀路由：
/// - `G_`: 全局 network_configs
/// - `P_`: 项目 network_configs（需要 project_path）
/// - `GP_`: 项目级全局快照配置（需要 project_path）
/// - 无前缀: 先查全局，再查项目（向后兼容）
pub async fn resolve_network_method_with_project(
    network_config_id: Option<&str>,
    project_path: Option<&str>,
) -> Result<Option<ConnectionMethod>, CoreError> {
    use std::path::Path;

    let Some(net_id) = network_config_id else {
        return Ok(None);
    };

    // ===== GP_ 前缀：项目级全局快照配置 =====
    if net_id.starts_with("GP_") {
        if let Some(pp) = project_path {
            let db_path = Path::new(pp).join(".RSmeta").join("project.db");
            if db_path.exists() {
                if let Ok((network_type, config_str, auth_config_id)) =
                    project_query_network_config_with_auth(&db_path, net_id)
                {
                    return parse_network_config_json(
                        &network_type,
                        &config_str,
                        auth_config_id.as_deref(),
                        project_path,
                    )
                    .await;
                }
            }
        }
        tracing::warn!(net_id = %net_id, "GP_ 前缀网络配置未找到（project_path={:?}）", project_path);
        return Ok(None);
    }

    // ===== P_ 前缀：项目级配置 =====
    if net_id.starts_with("P_") {
        if let Some(pp) = project_path {
            let db_path = Path::new(pp).join(".RSmeta").join("project.db");
            if db_path.exists() {
                if let Ok((network_type, config_str, auth_config_id)) =
                    project_query_network_config_with_auth(&db_path, net_id)
                {
                    return parse_network_config_json(
                        &network_type,
                        &config_str,
                        auth_config_id.as_deref(),
                        project_path,
                    )
                    .await;
                }
            }
        }
        tracing::warn!(net_id = %net_id, "P_ 前缀网络配置未找到（project_path={:?}）", project_path);
        return Ok(None);
    }

    // ===== 全局配置（G_ 前缀或无前缀） =====
    if let Some(gdb) = engine::migration::get_global_db_manager() {
        if let Ok(nets) = gdb.list_network_configs(None).await {
            if let Some(net) = nets.iter().find(|n| n.id == net_id) {
                return parse_network_config_json(
                    &net.network_type,
                    &net.config,
                    net.auth_config_id.as_deref(),
                    None,
                )
                .await;
            }
        }
    }

    // ===== 向后兼容：无前缀时也查项目 DB =====
    if !net_id.starts_with("G_") {
        if let Some(pp) = project_path {
            let db_path = Path::new(pp).join(".RSmeta").join("project.db");
            if db_path.exists() {
                if let Ok((network_type, config_str, auth_config_id)) =
                    project_query_network_config_with_auth(&db_path, net_id)
                {
                    return parse_network_config_json(
                        &network_type,
                        &config_str,
                        auth_config_id.as_deref(),
                        project_path,
                    )
                    .await;
                }
            }
        }
    }

    tracing::warn!(net_id = %net_id, "未找到网络配置");
    Ok(None)
}

/// 查询项目网络配置，同时返回 network_type、config 和 auth_config_id
fn project_query_network_config_with_auth(
    db_path: &std::path::Path,
    net_id: &str,
) -> Result<(String, String, Option<String>), String> {
    let conn = rusqlite::Connection::open(db_path).map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT network_type, config, auth_config_id FROM network_configs WHERE id = ?1",
        rusqlite::params![net_id],
        |row| {
            let network_type: String = row.get(0)?;
            let config: String = row.get(1)?;
            let auth_config_id: Option<String> = row.get(2)?;
            Ok((network_type, config, auth_config_id))
        },
    )
    .map_err(|e| e.to_string())
}

/// 根据 network_type 将 config JSON 解析为 ConnectionMethod
///
/// 公共函数，commands 层和 service 层共享
///
/// 如果提供了 auth_config_id，则从 auth_configs 表读取网络认证配置并注入
pub async fn parse_network_config_json(
    network_type: &str,
    config_json: &str,
    auth_config_id: Option<&str>,
    project_path: Option<&str>,
) -> Result<Option<ConnectionMethod>, CoreError> {
    // 类型键归一化：写入侧可能是 UI 标签（`SSH` / `Proxy`）或别名，统一小写后匹配。
    // 此前只匹配小写字面量，导致「档案存在、类型也写了，但整条链静默不生效」（审计 #20）。
    let kind = network_type.trim().to_ascii_lowercase();
    match kind.as_str() {
        "chain" => {
            let hops: Vec<connection::config::ChainHop> = serde_json::from_str(config_json)
                .map_err(|e| CoreError::from(format!("解析协议链配置 JSON 失败: {}", e)))?;
            if hops.is_empty() {
                return Ok(None);
            }
            Ok(Some(ConnectionMethod::Chain(hops)))
        }
        "ssh" | "ssh_tunnel" => {
            let mut ssh_config: connection::config::SshConfig =
                serde_json::from_str(config_json)
                    .map_err(|e| CoreError::from(format!("解析 SSH 隧道配置 JSON 失败: {}", e)))?;

            // 如果有 auth_config_id，从 auth_configs 读取网络认证并注入
            if let Some(auth_id) = auth_config_id {
                if let Some(auth_data) =
                    load_auth_data_from_db_for_network(auth_id, project_path).await?
                {
                    ssh_config = inject_ssh_auth_from_auth_data(ssh_config, &auth_data);
                }
            }

            Ok(Some(ConnectionMethod::Ssh(ssh_config)))
        }
        "ssl" | "tls" => {
            let ssl_config: connection::config::SslConfig = serde_json::from_str(config_json)
                .map_err(|e| CoreError::from(format!("解析 SSL 配置 JSON 失败: {}", e)))?;
            Ok(Some(ConnectionMethod::Ssl(ssl_config)))
        }
        "proxy" | "http_proxy" | "http" | "socks" | "socks5" | "socks_proxy" => {
            let mut proxy_config: connection::config::ProxyConfig =
                serde_json::from_str(config_json)
                    .map_err(|e| CoreError::from(format!("解析代理配置 JSON 失败: {}", e)))?;

            // 如果有 auth_config_id，从 auth_configs 读取网络认证并注入
            if let Some(auth_id) = auth_config_id {
                if let Some(auth_data) =
                    load_auth_data_from_db_for_network(auth_id, project_path).await?
                {
                    proxy_config = inject_proxy_auth_from_auth_data(proxy_config, &auth_data);
                }
            }

            if kind.starts_with("socks") {
                Ok(Some(ConnectionMethod::SocksProxy(proxy_config)))
            } else {
                Ok(Some(ConnectionMethod::HttpProxy(proxy_config)))
            }
        }
        other => {
            tracing::warn!(
                network_type = other,
                "未知的网络配置类型（档案已保存但不会生效）"
            );
            Ok(None)
        }
    }
}

/// 从 auth_configs 表加载网络认证配置（用于 SSH/Proxy 认证）
///
/// 此函数与 `load_auth_data_from_db` 不同，它用于加载网络认证，
/// 而 `load_auth_data_from_db` 用于加载数据库认证
async fn load_auth_data_from_db_for_network(
    auth_config_id: &str,
    project_path: Option<&str>,
) -> Result<Option<String>, CoreError> {
    // 优先从全局数据库读取
    if let Some(gdb) = engine::migration::get_global_db_manager() {
        if let Ok(Some(auth_config)) = gdb.get_auth_config(auth_config_id).await {
            let auth_data =
                engine::persistence::auth_store::decrypt_auth_data(&auth_config.auth_data)?;
            return Ok(Some(auth_data));
        }
    }

    // 全局未找到，尝试从项目数据库读取（项目级网络认证配置）
    if let Some(pp) = project_path {
        let db_path = std::path::Path::new(pp).join(".RSmeta").join("project.db");
        if db_path.exists() {
            if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                if let Ok(Some(auth_config)) =
                    engine::persistence::auth_store::get_auth_config(&conn, auth_config_id)
                {
                    let auth_data =
                        engine::persistence::auth_store::decrypt_auth_data(&auth_config.auth_data)?;
                    return Ok(Some(auth_data));
                }
            }
        }
    }

    Ok(None)
}

/// 从 auth_data JSON 注入 SSH 认证配置
fn inject_ssh_auth_from_auth_data(
    mut ssh_config: connection::config::SshConfig,
    auth_data_json: &str,
) -> connection::config::SshConfig {
    let auth_data: serde_json::Value = match serde_json::from_str(auth_data_json) {
        Ok(v) => v,
        Err(_) => return ssh_config,
    };

    let obj = match auth_data.as_object() {
        Some(o) => o,
        None => return ssh_config,
    };

    // 优先使用 auth_data 中的 username
    if let Some(username) = obj.get("username").and_then(|v| v.as_str()) {
        if !username.is_empty() {
            ssh_config.username = username.to_string();
        }
    }

    // 根据常见的 SSH 认证类型注入认证
    // 首先尝试私钥认证
    if let Some(key_path) = obj.get("keyPath").and_then(|v| v.as_str()) {
        if !key_path.is_empty() {
            let passphrase = obj
                .get("passphrase")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            ssh_config.auth = connection::config::SshAuth::PrivateKey {
                key_path: key_path.to_string(),
                passphrase,
            };
            return ssh_config;
        }
    }

    // 然后尝试密码认证
    if let Some(password) = obj.get("password").and_then(|v| v.as_str()) {
        if !password.is_empty() {
            ssh_config.auth = connection::config::SshAuth::Password {
                password: password.to_string(),
            };
            return ssh_config;
        }
    }

    ssh_config
}

/// 从 auth_data JSON 注入 Proxy 认证配置
fn inject_proxy_auth_from_auth_data(
    mut proxy_config: connection::config::ProxyConfig,
    auth_data_json: &str,
) -> connection::config::ProxyConfig {
    let auth_data: serde_json::Value = match serde_json::from_str(auth_data_json) {
        Ok(v) => v,
        Err(_) => return proxy_config,
    };

    let obj = match auth_data.as_object() {
        Some(o) => o,
        None => return proxy_config,
    };

    // 从 auth_data 读取 username 和 password
    let username = obj.get("username").and_then(|v| v.as_str());
    let password = obj.get("password").and_then(|v| v.as_str());

    if let (Some(u), Some(p)) = (username, password) {
        if !u.is_empty() && !p.is_empty() {
            proxy_config.auth = Some(connection::config::ProxyAuth {
                username: u.to_string(),
                password: p.to_string(),
            });
        }
    }

    proxy_config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_sharing_a_manager_share_tunnel_registry() {
        let manager = Arc::new(ConnectionManager::new());
        let a = ConnectionService::new(manager.clone());
        let b = ConnectionService::new(manager.clone());
        assert!(
            a.shares_tunnels_with(&b),
            "同一管理器下的服务实例必须共用隧道注册表（生产 = 全局单例）——\
             否则隧道会随建立它的临时实例释放（审计 #20）"
        );

        let isolated = ConnectionService::new(Arc::new(ConnectionManager::new()));
        assert!(!a.shares_tunnels_with(&isolated), "独立管理器保持测试隔离");
    }

    #[tokio::test]
    async fn network_config_type_keys_are_normalized() {
        // UI / 历史数据写入的是 `SSH` / `Proxy` 这类大写标签；解析必须大小写无关，
        // 否则「档案存在但整条链静默不生效」（审计 #20）。
        let proxy = parse_network_config_json(
            "Proxy",
            r#"{"host":"127.0.0.1","port":1080}"#,
            None,
            None,
        )
        .await
        .expect("parse proxy");
        assert!(matches!(proxy, Some(ConnectionMethod::HttpProxy(_))));

        let socks = parse_network_config_json(
            "SOCKS5",
            r#"{"host":"127.0.0.1","port":1080}"#,
            None,
            None,
        )
        .await
        .expect("parse socks");
        assert!(matches!(socks, Some(ConnectionMethod::SocksProxy(_))));

        // 未知类型：不 panic，返回 None（并告警）。
        let unknown = parse_network_config_json("proxy_pwd", "{}", None, None)
            .await
            .expect("parse unknown");
        assert!(unknown.is_none());
    }

    #[tokio::test]
    async fn test_connect_empty_url() {
        let manager = Arc::new(ConnectionManager::new());
        let service = ConnectionService::new(manager);

        let result = service.connect(None, "mysql", "", None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connect_invalid_db_type() {
        let manager = Arc::new(ConnectionManager::new());
        let service = ConnectionService::new(manager);

        let result = service
            .connect(None, "invalid", "mysql://localhost", None)
            .await;
        assert!(result.is_err());
    }

    /// A2：引用了网络档案但入口未解析出连接方式（档案被删 / 类型未知）→ 拒绝静默直连。
    ///
    /// 不建隧道、不碰数据库：错误必须在进入 `apply_network_method` 之前就产生。
    #[tokio::test]
    async fn referenced_network_profile_without_method_is_rejected() {
        let manager = Arc::new(ConnectionManager::new());
        let service = ConnectionService::new(manager);
        let req = ConnectRequest {
            conn_id: None,
            db_type: "mysql".into(),
            url: "mysql://h:3306/db".into(),
            name: Some("a2_guard".into()),
            connection_type: ConnectionType::Global,
            project_path: None,
            description: None,
            driver_id: None,
            environment_id: None,
            auth_config_id: None,
            auth_method: None,
            // 引用了档案，但 network_method 为 None（入口解析失败）
            network_config_id: Some("G_net_missing".into()),
            driver_properties: None,
            advanced_options: None,
            options: None,
            tags: None,
            metadata_path: None,
            schema_name: None,
            use_duckdb_fed: None,
            password: None,
            skip_persistence: Some(true),
            network_method: None,
        };
        let err = service
            .connect_with_type(req)
            .await
            .err()
            .expect("引用了网络档案却没有连接方式时必须失败");
        assert!(err.to_string().contains("网络配置无法解析"), "{err}");
    }
}
