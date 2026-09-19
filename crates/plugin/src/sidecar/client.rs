//! Sidecar JSON-RPC 客户端
//!
//! 负责与 Go Sidecar 进行 JSON-RPC 通信

use std::collections::HashMap;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::SidecarError;
use shared::models::QueryResult;

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
    ///
    /// TODO(M9/P1)：这一层走 `http://localhost:<port>`（零鉴权），与 D5（stdio + 二进制分帧）
    /// 相反；P1 换传输时这个 HTTP 客户端会被整层替换。P0 只修了「判成功/失败的判据取反」这个 bug。
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
            .map_err(|e| SidecarError::CommunicationError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            SidecarError::CommunicationError(format!("Failed to read response body: {}", e))
        })?;

        parse_rpc_response(status, &body)
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

/// 把「HTTP 状态 + 响应体」判定成结果或错误。
///
/// 判据（抽成纯函数是因为**原来这四条是反的**，必须能被单测钉住）：
/// 1. body 不是 JSON-RPC → 通信错误（网关 / 代理 / 端口被占才会这样）；
/// 2. body 里有 `error` → 报 RPC 错误（**无论 HTTP 状态**——sidecar 出错时也是 200）；
/// 3. 非 2xx 且 body 里没有 error → 报 HTTP 状态，不能当成空结果；
/// 4. 2xx 且无 error → `result` 必须存在，否则就是协议错。
fn parse_rpc_response<T: for<'de> Deserialize<'de>>(
    status: reqwest::StatusCode,
    body: &str,
) -> Result<T, SidecarError> {
    let rpc_response: JsonRpcResponse = serde_json::from_str(body).map_err(|e| {
        SidecarError::CommunicationError(format!(
            "Response is not valid JSON-RPC (HTTP {}): {}",
            status, e
        ))
    })?;

    if let Some(error) = rpc_response.error {
        return Err(SidecarError::CommunicationError(format!(
            "RPC error (code={}): {}",
            error.code, error.message
        )));
    }

    if !status.is_success() {
        return Err(SidecarError::CommunicationError(format!(
            "HTTP {} with no JSON-RPC error in body",
            status
        )));
    }

    let result = rpc_response
        .result
        .ok_or_else(|| SidecarError::CommunicationError("No result in response".to_string()))?;

    serde_json::from_value(result)
        .map_err(|e| SidecarError::CommunicationError(format!("Failed to parse result: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn test_client_creation() {
        let client = SidecarClient::new(12345);
        assert_eq!(client.base_url, "http://localhost:12345");
    }

    #[test]
    fn parse_rpc_response_accepts_success_body() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"drivers":[],"count":0}}"#;
        let got: ListDriversResponse = parse_rpc_response(StatusCode::OK, body).expect("应成功");
        assert_eq!(got.count, 0);
    }

    /// sidecar 出错时回的是 HTTP 200 + JSON-RPC error（不是 4xx/5xx）——
    /// 原实现只在「非 2xx」分支里找 error，所以永远看不到它。
    #[test]
    fn parse_rpc_response_reports_rpc_error_even_with_http_200() {
        let body = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32040,"message":"driver_not_found","data":null}}"#;
        let err = parse_rpc_response::<ListDriversResponse>(StatusCode::OK, body).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("-32040") && msg.contains("driver_not_found"),
            "{msg}"
        );
    }

    /// 非 2xx + 非 JSON body（网关 / 代理 / 端口被占）：要报出 HTTP 状态，不能吞掉。
    #[test]
    fn parse_rpc_response_reports_plain_http_failure() {
        let err = parse_rpc_response::<ListDriversResponse>(
            StatusCode::BAD_GATEWAY,
            "<html>502 Bad Gateway</html>",
        )
        .unwrap_err();
        assert!(err.to_string().contains("502"), "{err}");
    }

    /// 2xx 但既没 error 也没 result：协议错，不能当成"空结果"。
    #[test]
    fn parse_rpc_response_rejects_result_less_success() {
        let body = r#"{"jsonrpc":"2.0","id":1}"#;
        let err = parse_rpc_response::<ListDriversResponse>(StatusCode::OK, body).unwrap_err();
        assert!(err.to_string().contains("No result"), "{err}");
    }
}
