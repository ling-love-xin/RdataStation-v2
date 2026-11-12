//! 连接默认值设置 model（struct + 默认值 + 默认函数）。
use super::default_true;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDefaults {
    /// 建连超时（毫秒）：超过即判定本次尝试失败（会自动重试一次）。默认 15000。
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    /// 未配置 SSL 档案且目标为 LAN / 本机时，给**默认先试 TLS** 的驱动（sqlx 插件
    /// `mysql` / `postgres`、官方 `postgres_native`）显式关掉 TLS，规避 `sslmode=prefer`
    /// 的握手卡顿。默认开；公网 / 需要 TLS 的场景可关。`mysql_native` 不受影响：
    /// mysql_async 默认 `ssl_opts = None`（不加密），且它不认 `ssl-mode` 参数。
    #[serde(default = "default_true")]
    pub lan_disable_tls: bool,
}

impl Default for ConnectionDefaults {
    fn default() -> Self {
        Self {
            connect_timeout_ms: default_connect_timeout_ms(),
            lan_disable_tls: true,
        }
    }
}
fn default_connect_timeout_ms() -> u64 {
    15_000
}
