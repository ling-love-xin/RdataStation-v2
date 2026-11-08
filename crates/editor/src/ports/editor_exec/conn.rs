//! 连接解析：本次执行用哪个连接、哪条连接串、加速源怎么组。

use super::*;

impl EngineQueryRunner {
    pub(crate) fn new(fed: Arc<dyn FederatedSourcePort>) -> Option<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("rds-editor-exec-rt")
            .build()
            .ok()?;
        let manager = engine::connection_manager::get_connection_manager();
        Some(Self {
            runtime,
            service: SqlService::new(manager.clone()),
            fed,
        })
    }

    /// 本次执行该用多长超时（毫秒）：按**连接**的 `query_timeout`（B3）
    ///
    /// 未绑定的文档看当前活动连接（与 `execute(None, …)` 同口径）；连接没配就是
    /// **没有超时**——不在这里造一个默认值（超时是连接属性，不是编辑器属性）。
    pub(crate) async fn query_timeout_ms(&self, connection: Option<&str>) -> Option<u64> {
        let manager = engine::connection_manager::get_connection_manager();
        let conn_id = match connection {
            Some(id) => Some(id.to_string()),
            None => manager.get_active_connection_id().await,
        }?;
        let config = manager.get_connection_config(&conn_id).await?;
        config.query_timeout.map(|secs| u64::from(secs) * 1000)
    }

    /// 本次执行到底用哪个连接 id（绑定优先 → 未绑定回退当前活动连接）
    pub(crate) fn resolve_conn_id(&self, connection: Option<&str>) -> Option<String> {
        let manager = engine::connection_manager::get_connection_manager();
        self.runtime.block_on(async {
            match connection {
                Some(id) => Some(id.to_string()),
                None => manager.get_active_connection_id().await,
            }
        })
    }

    /// 一条连接的**运行时连接串**（含凭据）——只有执行路径才拿它
    ///
    /// 为什么不用 `ConnectionInfo.url`：那是**脱敏**的（`user:******@`），`ATTACH` 拿它只会
    /// 认证失败（真机现场：`Access denied for user 'root'@…`）。为什么不用 Secret：实测
    /// **DuckDB 1.5.5 的 mysql / postgres 扫描器不认 Secret**（见
    /// `crates/engine/tests/federation_credentials_probe.rs` 的台账），凭据只能随连接串进 `ATTACH`
    /// ——代价是离开引擎的文本必须脱敏（引擎侧已收口：`accel::scrub_credentials`）。
    ///
    /// 取的是连接管理器里的 `DriverConnectionConfig.url_override`（重连用的那份，带凭据）；
    /// 拿不到时回退到 `ConnectionInfo.url`（会在认证上如实失败，不会静静挂错库）。
    pub(crate) fn runtime_connection_string(&self, conn_id: &str) -> Option<String> {
        let manager = engine::connection_manager::get_connection_manager();
        let config = self
            .runtime
            .block_on(manager.get_connection_config(&conn_id.to_string()));
        config
            .and_then(|config| config.url_override)
            .filter(|url| !url.trim().is_empty())
    }

    /// 加速档的源：宿主侧的连接信息（`db_type` + **带凭据的**运行时连接串）→ 引擎的 [`accel::AccelSource`]
    ///
    /// 引擎不读连接库、也不解密口令：这里把两样它需要的东西递过去。
    pub(crate) fn accel_source(&self, connection: Option<&str>) -> Result<accel::AccelSource, String> {
        let manager = engine::connection_manager::get_connection_manager();
        let conn_id = self
            .resolve_conn_id(connection)
            .ok_or_else(|| "本地加速需要一个连接：先绑定一个，或在导航里选中它".to_string())?;
        let info = self
            .runtime
            .block_on(manager.get_connection_info(&conn_id))
            .ok_or_else(|| format!("拿不到连接 {conn_id} 的信息，无法在本地挂载它"))?;
        let url = self
            .runtime_connection_string(&conn_id)
            .unwrap_or_else(|| info.url.clone());
        accel::AccelSource::new(&conn_id, &info.db_type, &url)
    }
}
