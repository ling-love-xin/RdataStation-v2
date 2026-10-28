//! 连接默认值设置 model（struct + 默认值 + 默认函数）。
use super::default_true;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionDefaults {
    /// 建连超时（毫秒）：超过即判定本次尝试失败（会自动重试一次）。默认 15000。
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    /// 未配置 SSL 档案且目标为 LAN / 本机时，显式关闭 TLS（规避 sqlx 默认 `prefer`
    /// 握手卡顿）。默认开；公网 / 需要 TLS 的场景可关。
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
