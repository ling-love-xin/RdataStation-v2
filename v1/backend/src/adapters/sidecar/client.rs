//! Sidecar JSON-RPC 客户端
//!
//! 负责与 Go Sidecar 进行 JSON-RPC 通信

use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::SidecarError;
use crate::core::models::QueryResult;

/// JSON-RPC 请求
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: u64,
}

/// JSON-RPC 响应
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 错误
#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

/// 连接参数
#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectionParams {
    pub driver_id: String,
    pub dsn: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
}

/// 查询参数
#[derive(Debug, Serialize, Deserialize)]
pub struct QueryParams {
    pub connection: ConnectionParams,
    pub sql: String,
    pub params: Option<Vec<Value>>,
}

/// 驱动元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverMetadata {
    pub id: String,
    pub name: String,
    pub db_type: String,
    pub driver_type: String,
    pub version: String,
    pub priority: u32,
    pub is_builtin: bool,
    pub capabilities: Vec<String>,
}

/// 可用驱动列表响应
#[derive(Debug, Serialize, Deserialize)]
pub struct ListDriversResponse {
    pub drivers: Vec<DriverMetadata>,
    pub count: usize,
}

/// Sidecar JSON-RPC 客户端
pub struct SidecarClient {
    client: Client,
    base_url: String,
    request_id: u64,
}

impl SidecarClient {
    /// 创建新的客户端
    pub fn new(port: u16) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
            base_url: format!("http://localhost:{}", port),
            request_id: 0,
        }
    }

    /// 发送 JSON-RPC 请求
    async fn request<T: for<'de> Deserialize<'de>>(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<T, SidecarError> {
        self.request_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: self.request_id,
        };

        let url = format!("{}/rpc", self.base_url);

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| SidecarError::CommunicationError(format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            let rpc_response: JsonRpcResponse = response
                .json()
                .await
                .map_err(|e| SidecarError::CommunicationError(format!("Failed to parse response: {}", e))?;

            if let Some(error) = rpc_response.error {
                return Err(SidecarError::CommunicationError(format!(
                    "RPC error (code={}): {}",
                    error.code, error.message
                )));
            }

            if let Some(result) = rpc_response.result {
                let value: T = serde_json::from_value(result).map_err(|e| {
                    SidecarError::CommunicationError(format!("Failed to parse result: {}", e))
                })?;
                Ok(value)
            } else {
                Err(SidecarError::CommunicationError(
                    "No result in response".to_string(),
                ))
            }
        } else {
            Err(SidecarError::CommunicationError(format!(
                "HTTP error: {}",
                response.status()
            )))
        }
    }

    /// 列出可用驱动
    pub async fn list_available_drivers(&mut self) -> Result<ListDriversResponse, SidecarError> {
        self.request("connectors.list_available", None).await
    }

    /// 测试连接
    pub async fn test_connection(
        &mut self,
        params: ConnectionParams,
    ) -> Result<HashMap<String, bool>, SidecarError> {
        self.request(
            "connectors.test_connection",
            Some(serde_json::to_value(params).map_err(|e| {
                SidecarError::CommunicationError(format!("Serialization failed: {}", e))
            })?),
        )
        .await
    }

    /// 执行查询
    pub async fn execute_query(
        &mut self,
        params: QueryParams,
    ) -> Result<QueryResult, SidecarError> {
        self.request(
            "connectors.execute_query",
            Some(serde_json::to_value(params).map_err(|e| {
                SidecarError::CommunicationError(format!("Serialization failed: {}", e))
            })?),
        )
        .await
    }

    /// 列出表
    pub async fn list_tables(
        &mut self,
        params: ConnectionParams,
    ) -> Result<HashMap<String, Value>, SidecarError> {
        self.request(
            "connectors.list_tables",
            Some(serde_json::to_value(params).map_err(|e| {
                SidecarError::CommunicationError(format!("Serialization failed: {}", e))
            })?),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = SidecarClient::new(12345);
        assert_eq!(client.base_url, "http://localhost:12345");
    }
}
