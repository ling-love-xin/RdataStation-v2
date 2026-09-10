/// WASM 插件 API 模块
use serde::{Deserialize, Serialize};

/// WASM 插件 API 请求
#[derive(Debug, Deserialize)]
pub struct WasmApiRequest {
    /// API 方法名
    pub method: String,
    /// API 参数
    pub params: serde_json::Value,
}

/// WASM 插件 API 响应
#[derive(Debug, Serialize)]
pub struct WasmApiResponse {
    /// 响应状态
    pub success: bool,
    /// 响应数据
    pub data: Option<serde_json::Value>,
    /// 错误信息
    pub error: Option<String>,
}

/// WASM 插件 API 接口
pub trait WasmPluginApi {
    /// 调用 API 方法
    fn call(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String>;
}

/// 实现默认的 WASM 插件 API
pub struct DefaultWasmPluginApi;

impl WasmPluginApi for DefaultWasmPluginApi {
    fn call(&self, method: &str, _params: serde_json::Value) -> Result<serde_json::Value, String> {
        Err(format!("Method not implemented: {}", method))
    }
}
