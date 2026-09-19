//! 注册：把插件声明的驱动变成引擎侧的「可选项」。
//!
//! 装上插件之后，连接对话框要在驱动下拉里看到它、选中它、填表单、连上 —— 这条链路的入网口
//! 就是这里：一个插件的一个 driver 注册成一个 [`DriverFactory`]。
//!
//! # 注册期与运行期的分工
//!
//! | 时机 | 谁知道 | 用来干什么 |
//! | --- | --- | --- |
//! | 注册（装插件 / 打开项目） | **清单**（`contributes.drivers`） | 下拉里有什么、默认端口、图标 |
//! | 连接（用户点「连接」） | **跑起来的进程**（`driver.describe`） | 真能力、服务端版本（清单可以撒谎，进程不会） |
//!
//! # 表单字段与队列等待
//!
//! - 表单字段 P1 给的是**标准五件套**（主机 / 端口 / 库名 / 用户 / 口令）+ 可选的**文件路径**，
//!   且**一律不标必填**：宿主并不知道某个驱动到底需要哪几项（那在清单的 `connection_schema` 里，
//!   而 schema → `DriverField` 的翻译还没做）。标了必填反而会挡住合法配置（比如靠 `options`
//!   拼 DSN 的驱动），所以这里宁可让插件自己报错 —— 它比宿主清楚。
//! - 串行驱动的第二条连接会**排队**（内核在第一个实例上排队）：`create` 会等它被放行，
//!   等到超时就**明确失败并撤掉排队**（不能让用户挂在一个永远不来的连接上）。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use engine::driver::DynDatabase;
use engine::driver::registry::{
    DriverConnectionConfig, DriverDescriptor, DriverFactory, DriverField, DriverFieldType,
    DriverKind, DriverRegistry,
};
use shared::error::{CommonError, ConnectionError, CoreError, PluginError};

use super::conn::SidecarConn;
use super::driver::{DriverError, SessionDriver, SidecarDatabase};
use super::proto::RpcErrorCode;
use super::supervisor::{SESSION_RPC_TIMEOUT, SessionOpened, SidecarSupervisor, SupervisorError};

/// 排队等待的默认上限。
pub const DEFAULT_QUEUE_WAIT: Duration = Duration::from_secs(30);
/// 等的过程中多久看一眼（并顺手把事件落一落）。
const QUEUE_POLL: Duration = Duration::from_millis(25);

/// 一个 sidecar 驱动的工厂（引擎按 `descriptor().id` 注册与查找）。
pub struct SidecarDriverFactory {
    /// 宿主侧唯一那份进程/会话状态。
    supervisor: Arc<Mutex<SidecarSupervisor>>,
    plugin_id: String,
    driver_id: String,
    descriptor: DriverDescriptor,
    queue_wait: Duration,
}

impl SidecarDriverFactory {
    pub fn new(
        supervisor: Arc<Mutex<SidecarSupervisor>>,
        plugin_id: impl Into<String>,
        descriptor: DriverDescriptor,
    ) -> Self {
        let driver_id = descriptor.id.clone();
        Self {
            supervisor,
            plugin_id: plugin_id.into(),
            driver_id,
            descriptor,
            queue_wait: DEFAULT_QUEUE_WAIT,
        }
    }

    /// 改排队等待上限（测试与"用户点了取消"这类场景用）。
    pub fn with_queue_wait(mut self, wait: Duration) -> Self {
        self.queue_wait = wait;
        self
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }
}

impl DriverFactory for SidecarDriverFactory {
    fn descriptor(&self) -> DriverDescriptor {
        self.descriptor.clone()
    }

    fn create(
        &self,
        config: DriverConnectionConfig,
    ) -> Pin<Box<dyn Future<Output = Result<DynDatabase, CoreError>> + Send>> {
        let supervisor = Arc::clone(&self.supervisor);
        let plugin_id = self.plugin_id.clone();
        let driver_id = self.driver_id.clone();
        let queue_wait = self.queue_wait;
        Box::pin(async move {
            create_database(supervisor, plugin_id, driver_id, config, queue_wait).await
        })
    }
}

/// 连接：开会话 → 描述 → 包成驱动。
async fn create_database(
    supervisor: Arc<Mutex<SidecarSupervisor>>,
    plugin_id: String,
    driver_id: String,
    config: DriverConnectionConfig,
    queue_wait: Duration,
) -> Result<DynDatabase, CoreError> {
    let session_id = format!("{plugin_id}:{driver_id}:{}", uuid::Uuid::new_v4());
    // 连接配置**原样**交给驱动进程：宿主收集到的字段（主机/端口/库/用户/口令/连接方式/选项）
    // 就是插件要的那份，别在中间再做一次有损翻译。
    let params = serde_json::to_value(&config).map_err(|e| {
        CoreError::common(CommonError::invalid_argument(
            "connection_config",
            format!("连接配置序列化失败：{e}"),
        ))
    })?;

    let opened = {
        let mut supervisor = supervisor.lock().await;
        supervisor
            .open_session(&plugin_id, &driver_id, &session_id, params, Instant::now())
            .await
    };

    match opened {
        Ok(SessionOpened::Live { .. }) => {}
        Ok(SessionOpened::Queued { position, .. }) => {
            wait_for_session(&supervisor, &session_id, position, queue_wait).await?;
        }
        Err(e) => return Err(connect_error(&plugin_id, &driver_id, &session_id, e)),
    }

    let conn: Arc<SidecarConn> = {
        let supervisor = supervisor.lock().await;
        supervisor.session_conn_handle(&session_id)
    }
    .ok_or_else(|| CoreError::connection(ConnectionError::NoActiveConnection))?;

    let descriptor = SessionDriver::new(&conn, &session_id)
        .describe(&driver_id)
        .await
        .map_err(|e| connect_error_from_driver(&plugin_id, &driver_id, &session_id, e))?;

    Ok(
        SidecarDatabase::new(&conn, &session_id, &driver_id, descriptor)
            .owned_by(supervisor)
            .into_dyn(),
    )
}

/// 排队中的会话：等内核放行它。
///
/// 等待期间要**周期性 `drain_events`**：放行是内核在 `release` 时机推出来的，而事件只有落到
/// 内核上才算数（本项目的一条纪律：事件落地是显式的）。等不到就撤掉排队并如实报超时。
async fn wait_for_session(
    supervisor: &Arc<Mutex<SidecarSupervisor>>,
    session_id: &str,
    position: usize,
    queue_wait: Duration,
) -> Result<(), CoreError> {
    tracing::info!(session_id, position, "驱动忙，连接在排队");
    let deadline = Instant::now() + queue_wait;
    loop {
        {
            let mut supervisor = supervisor.lock().await;
            supervisor.drain_events(Instant::now()).await;
            if supervisor.session_conn_handle(session_id).is_some() {
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            let mut supervisor = supervisor.lock().await;
            let _ = supervisor.close_session(session_id, Instant::now()).await;
            return Err(CoreError::connection(ConnectionError::Timeout {
                conn_id: session_id.to_string(),
                duration_ms: queue_wait.as_millis() as u64,
            }));
        }
        tokio::time::sleep(QUEUE_POLL).await;
    }
}

/// supervisor 层的失败 → 连接错误域（这些都是"连不上"）。
fn connect_error(
    plugin_id: &str,
    driver_id: &str,
    session_id: &str,
    error: SupervisorError,
) -> CoreError {
    match error {
        SupervisorError::Rejected { reason, .. } => {
            // 内核的拒绝理由本来就是给人看的（`RejectReason` 的文案）
            CoreError::connection(ConnectionError::Network {
                conn_id: session_id.to_string(),
                reason: reason.to_string(),
            })
        }
        SupervisorError::Process { error, .. } => CoreError::connection(ConnectionError::Refused {
            conn_id: session_id.to_string(),
            reason: format!("驱动进程起不来：{error}"),
        }),
        SupervisorError::Rpc { method, error } => connect_error_from_driver(
            plugin_id,
            driver_id,
            session_id,
            DriverError::from_call(&method, error),
        ),
        other => CoreError::plugin(PluginError::ExecutionFailed {
            plugin_id: plugin_id.to_string(),
            function: "driver.create".to_string(),
            reason: other.to_string(),
        }),
    }
}

/// 驱动进程自己报的错 → 连接错误域。
///
/// 连接期的 `-32003 sql_error` 在语义上就是「连不上」（坏地址 / 口令错 / TLS 失败），
/// 所以落 `ConnectionError::Refused` 而不是 SQL 错 —— 用户在这里看到的是"连接失败"。
fn connect_error_from_driver(
    plugin_id: &str,
    driver_id: &str,
    session_id: &str,
    error: DriverError,
) -> CoreError {
    match error {
        DriverError::Rpc {
            code,
            message,
            data,
        } => match code {
            RpcErrorCode::CapabilityDenied => CoreError::common(CommonError::not_supported(
                format!("驱动 {driver_id} 不支持该能力：{message}"),
            )),
            RpcErrorCode::DriverNotSupported => {
                CoreError::connection(ConnectionError::DriverNotFound {
                    driver: driver_id.to_string(),
                })
            }
            RpcErrorCode::Timeout => CoreError::connection(ConnectionError::Timeout {
                conn_id: session_id.to_string(),
                duration_ms: 0,
            }),
            RpcErrorCode::ResourceLimit => CoreError::common(CommonError::not_supported(format!(
                "驱动 {driver_id} 达到资源上限：{message}"
            ))),
            _ => {
                let reason = match data {
                    Some(data) => format!("{message}（{data}）"),
                    None => message,
                };
                CoreError::connection(ConnectionError::Refused {
                    conn_id: session_id.to_string(),
                    reason,
                })
            }
        },
        DriverError::UnknownCode { raw_code, message } => {
            CoreError::connection(ConnectionError::Refused {
                conn_id: session_id.to_string(),
                reason: format!("驱动报错（未知码 {raw_code}）：{message}"),
            })
        }
        DriverError::Timeout { .. } => CoreError::connection(ConnectionError::Timeout {
            conn_id: session_id.to_string(),
            duration_ms: SESSION_RPC_TIMEOUT.as_millis() as u64,
        }),
        DriverError::Disconnected { reason } => CoreError::connection(ConnectionError::Network {
            conn_id: session_id.to_string(),
            reason,
        }),
        DriverError::Protocol { detail } => CoreError::plugin(PluginError::ExecutionFailed {
            plugin_id: plugin_id.to_string(),
            function: "driver.describe".to_string(),
            reason: detail,
        }),
    }
}

/// 把一个插件的全部驱动注册进引擎，返回注册了哪些 id。
///
/// 幂等：同一个 driver id 再注册一次就是覆盖（重装 / 升级插件会走到这条路）。
/// 不是子进程形态（wasm / 纯前端）返回空集合 —— 那类插件不走进程，也不该出现在驱动下拉里。
pub async fn register_sidecar_drivers(
    supervisor: Arc<Mutex<SidecarSupervisor>>,
    manifest: &crate::manifest::PluginManifest,
    plugin_dir: &std::path::Path,
) -> Result<Vec<String>, CoreError> {
    let plugin_id = manifest.plugin.id.clone();
    let deployed = {
        let mut supervisor = supervisor.lock().await;
        supervisor
            .deploy_manifest(manifest, plugin_dir)
            .map_err(|e| {
                CoreError::plugin(PluginError::ExecutionFailed {
                    plugin_id: plugin_id.clone(),
                    function: "register_sidecar_drivers".to_string(),
                    reason: e.to_string(),
                })
            })?
    };
    if !deployed {
        return Ok(Vec::new());
    }

    let mut registered = Vec::new();
    for driver in &manifest.contributes.drivers {
        let descriptor = descriptor_for(manifest, driver);
        registered.push(descriptor.id.clone());
        DriverRegistry::register_by_factory(
            descriptor.id.clone(),
            Arc::new(SidecarDriverFactory::new(
                Arc::clone(&supervisor),
                plugin_id.clone(),
                descriptor,
            )),
        );
    }
    Ok(registered)
}

/// 清单里的一个驱动 → 引擎的驱动描述符（**注册期**能知道的那部分）。
///
/// 能力（`capabilities`）与 `server_version` 是**运行期**的事（`driver.describe`），
/// 注册期不编：表单与下拉只用到这里这几项。
fn descriptor_for(
    manifest: &crate::manifest::PluginManifest,
    driver: &crate::manifest::ContributesDriver,
) -> DriverDescriptor {
    let mut descriptor = DriverDescriptor::new_external(
        driver.id.clone(),
        driver.display_name.clone(),
        DriverKind::Sidecar,
        driver.id.clone(),
    )
    .with_description(format!(
        "由插件 {} 提供的驱动（sidecar 进程）",
        manifest.plugin.id
    ))
    .with_category("relational");

    if let Some(port) = driver.default_port {
        descriptor = descriptor.with_default_port(port);
    }
    if let Some(icon) = &manifest.plugin.icon {
        descriptor = descriptor.with_icon(icon.clone());
    }

    // 标准五件套 + 可选文件路径；**都不标必填**（理由见模块文档）
    let fields = [
        ("host", "主机", DriverFieldType::Text),
        ("port", "端口", DriverFieldType::Number),
        ("database", "数据库", DriverFieldType::Text),
        ("username", "用户名", DriverFieldType::Text),
        ("password", "口令", DriverFieldType::Password),
        ("file_path", "文件路径", DriverFieldType::File),
    ];
    for (key, label, field_type) in fields {
        let mut field = DriverField {
            key: key.to_string(),
            label: label.to_string(),
            field_type,
            required: false,
            default_value: None,
            placeholder: None,
        };
        if key == "port" {
            field.default_value = driver.default_port.map(|port| port.to_string());
            field.placeholder = Some("留空则用驱动默认端口".to_string());
        }
        descriptor = descriptor.with_field(field);
    }

    // 清单声明的驱动特性 → 引擎的能力键（`capabilities` 是展示/门控用的粗粒度列表）
    if !driver.features.is_empty() {
        descriptor = descriptor.with_capabilities(driver.features.clone());
    }
    descriptor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    const MANIFEST: &str = r#"
[plugin]
id = "com.example.sqlserver"
name = "SQL Server Driver"
version = "1.0.0"
publisher = "example"
icon = "icon.png"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
executable = "bin/agent"

[[contributes.drivers]]
id = "mssql"
display_name = "SQL Server"
default_port = 1433
features = ["transactions", "cancel"]
"#;

    #[test]
    fn descriptor_comes_from_the_manifest() {
        let manifest: PluginManifest = toml::from_str(MANIFEST).unwrap();
        let driver = &manifest.contributes.drivers[0];
        let descriptor = descriptor_for(&manifest, driver);

        assert_eq!(descriptor.id, "mssql");
        assert_eq!(descriptor.name, "SQL Server");
        assert_eq!(descriptor.driver_kind, DriverKind::Sidecar);
        assert_eq!(descriptor.driver_kind.as_str(), "sidecar");
        assert_eq!(descriptor.target_database.as_deref(), Some("mssql"));
        assert_eq!(descriptor.default_port, Some(1433));
        assert_eq!(descriptor.icon.as_deref(), Some("icon.png"));
        assert!(descriptor.description.contains("com.example.sqlserver"));
        assert_eq!(
            descriptor.capabilities,
            vec!["transactions".to_string(), "cancel".to_string()]
        );

        // 表单字段：五件套 + 文件路径，都不标必填；端口带默认值
        let keys: Vec<&str> = descriptor.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "host",
                "port",
                "database",
                "username",
                "password",
                "file_path"
            ]
        );
        assert!(descriptor.fields.iter().all(|f| !f.required));
        let port = descriptor
            .fields
            .iter()
            .find(|f| f.key == "port")
            .expect("应当有端口字段");
        assert_eq!(port.default_value.as_deref(), Some("1433"));
        let password = descriptor
            .fields
            .iter()
            .find(|f| f.key == "password")
            .expect("应当有口令字段");
        assert!(matches!(password.field_type, DriverFieldType::Password));
    }

    #[test]
    fn a_driver_without_a_default_port_is_still_usable() {
        let manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "com.example.min"
name = "Min"
version = "1.0.0"
publisher = "example"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
executable = "bin/agent"

[[contributes.drivers]]
id = "min"
display_name = "Min Driver"
"#,
        )
        .unwrap();
        let descriptor = descriptor_for(&manifest, &manifest.contributes.drivers[0]);
        assert_eq!(descriptor.default_port, None);
        assert!(descriptor.capabilities.is_empty());
        let port = descriptor.fields.iter().find(|f| f.key == "port").unwrap();
        assert_eq!(port.default_value, None);
    }
}
