//! 端口协商相关命令
//!
//! 处理端口分配、检查、释放等操作

use crate::core::error::CoreError;
use crate::core::{PortNegotiator, PortRange, COMMON_DB_PORTS, DEFAULT_PORT_RANGE};

// ==================== Port Negotiation Commands ====================

/// 端口协商请求
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct NegotiatePortInput {
    /// 首选端口（可选）
    pub preferred_port: Option<u16>,
    /// 端口范围起始
    pub range_start: Option<u16>,
    /// 端口范围结束
    pub range_end: Option<u16>,
    /// 是否允许使用常用数据库端口
    pub allow_common_ports: Option<bool>,
}

/// 端口协商响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct NegotiatePortResponse {
    /// 分配的端口
    pub port: u16,
    /// 是否是首选端口
    pub is_preferred: bool,
    /// 尝试次数
    pub attempts: u32,
}

/// 协商端口
#[tauri::command]
#[specta::specta]
pub async fn negotiate_port(input: NegotiatePortInput) -> Result<NegotiatePortResponse, CoreError> {
    let range = if let (Some(start), Some(end)) = (input.range_start, input.range_end) {
        PortRange::new(start, end)
    } else {
        PortRange::default()
    };

    let negotiator = PortNegotiator::with_range(range);

    let port = negotiator
        .negotiate(input.preferred_port)
        .map_err(|e| CoreError::from(e.to_string()))?;

    Ok(NegotiatePortResponse {
        port,
        is_preferred: input.preferred_port == Some(port),
        attempts: 1,
    })
}

/// 协商本地端口（用于 SSH 隧道等）
#[tauri::command]
#[specta::specta]
pub async fn negotiate_local_port(preferred: Option<u16>) -> Result<u16, CoreError> {
    let negotiator = PortNegotiator::new();

    negotiator
        .negotiate_local_port(preferred)
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 检查端口是否可用
#[tauri::command]
#[specta::specta]
pub async fn is_port_available(port: u16) -> Result<bool, CoreError> {
    let negotiator = PortNegotiator::new();

    negotiator
        .is_port_available(port)
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 释放端口
#[tauri::command]
#[specta::specta]
pub async fn release_port(port: u16) -> Result<(), CoreError> {
    let negotiator = PortNegotiator::new();

    negotiator
        .release_port(port)
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 批量协商端口
#[tauri::command]
#[specta::specta]
pub async fn negotiate_multiple_ports(count: usize) -> Result<Vec<u16>, CoreError> {
    let negotiator = PortNegotiator::new();

    negotiator
        .negotiate_multiple(count)
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 协商连续端口范围
#[tauri::command]
#[specta::specta]
pub async fn negotiate_port_range(start: u16, count: usize) -> Result<Vec<u16>, CoreError> {
    let negotiator = PortNegotiator::new();

    negotiator
        .negotiate_range(start, count)
        .map_err(|e| CoreError::from(e.to_string()))
}

/// 获取常用数据库端口列表
#[tauri::command]
#[specta::specta]
pub async fn get_common_db_ports() -> Result<Vec<u16>, CoreError> {
    Ok(COMMON_DB_PORTS.to_vec())
}

/// 端口范围信息
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct PortRangeInfo {
    pub default_start: u16,
    pub default_end: u16,
    pub common_ports: Vec<u16>,
}

/// 获取端口范围信息
#[tauri::command]
#[specta::specta]
pub async fn get_port_range_info() -> Result<PortRangeInfo, CoreError> {
    Ok(PortRangeInfo {
        default_start: DEFAULT_PORT_RANGE.start,
        default_end: DEFAULT_PORT_RANGE.end,
        common_ports: COMMON_DB_PORTS.to_vec(),
    })
}
