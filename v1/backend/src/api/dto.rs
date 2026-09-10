//! API 数据传输对象 (DTO)
//!
//! 定义前后端之间的所有数据传输契约，包括：
//! - 请求/响应数据结构
//! - 错误响应格式
//! - 成功响应包装
//! - 分页响应等
//!
//! 注意：所有模型数据从 core 层重新导出，保持单一数据源

use serde::{Deserialize, Serialize};

// ==================== 模型重新导出 ====================

pub use crate::core::models::{QueryResult, Row, Value};

// ==================== 错误响应 ====================

/// 错误响应结构（用于前端）
///
/// 这是前端与后端之间的错误契约，包含：
/// - 错误代码：用于程序处理
/// - 错误分类：用于错误分组
/// - 错误消息：用于用户显示
/// - 详细信息：用于调试
/// - 是否可重试：用于重试策略
/// - 建议操作：用于用户引导
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// 错误代码
    pub code: String,
    /// 错误分类
    pub category: String,
    /// 错误消息
    pub message: String,
    /// 详细信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// 是否可重试
    pub retryable: bool,
    /// 建议操作
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl ErrorResponse {
    /// 创建新的错误响应
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            category: "Unknown".to_string(),
            message: message.into(),
            details: None,
            retryable: false,
            suggestion: None,
        }
    }

    /// 设置错误分类
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// 设置详细信息
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// 设置是否可重试
    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    /// 设置建议操作
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

impl std::fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

// ==================== 从 CoreError 转换 ====================

use crate::core::error::{CoreError, ErrorCategory};

impl From<CoreError> for ErrorResponse {
    fn from(err: CoreError) -> Self {
        let code = err.code().to_string();
        let category = err.category().to_string();
        let message = err.to_string();
        let retryable = err.is_retryable();

        let suggestion = match err.category() {
            ErrorCategory::Connection => Some("请检查网络连接和数据库配置".to_string()),
            ErrorCategory::Database => Some("请检查 SQL 语法或联系数据库管理员".to_string()),
            ErrorCategory::Storage => Some("请检查磁盘空间和文件权限".to_string()),
            ErrorCategory::Cache => Some("请检查缓存配置或尝试清理缓存".to_string()),
            ErrorCategory::Common => None,
            ErrorCategory::Plugin => Some("请检查插件配置".to_string()),
        };

        Self {
            code,
            category,
            message,
            details: None,
            retryable,
            suggestion,
        }
    }
}

// ==================== 便捷构造函数 ====================

impl ErrorResponse {
    /// 创建连接错误响应
    pub fn connection_error(message: impl Into<String>) -> Self {
        Self::new("CONN_ERROR", message)
            .with_category("Connection")
            .with_suggestion("请检查网络连接和数据库配置")
    }

    /// 创建数据库错误响应
    pub fn database_error(message: impl Into<String>) -> Self {
        Self::new("DB_ERROR", message)
            .with_category("Database")
            .with_suggestion("请检查 SQL 语法或联系数据库管理员")
    }

    /// 创建验证错误响应
    pub fn validation_error(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(
            "VALIDATION_ERROR",
            format!("{}: {}", field.into(), message.into()),
        )
        .with_category("Validation")
    }

    /// 创建超时错误响应
    pub fn timeout_error(operation: impl Into<String>) -> Self {
        Self::new(
            "TIMEOUT",
            format!("Operation '{}' timed out", operation.into()),
        )
        .with_category("Timeout")
        .with_retryable(true)
        .with_suggestion("请稍后重试或增加超时时间")
    }

    /// 创建内部错误响应
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", message)
            .with_category("Internal")
            .with_suggestion("请联系技术支持")
    }
}

// ==================== 成功响应包装 ====================

/// 通用 API 响应包装
///
/// 统一的成功/失败响应格式，前端可以统一处理
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum ApiResponse<T> {
    /// 成功响应
    Success {
        /// 响应数据
        data: T,
        /// 可选的消息
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// 错误响应
    Error {
        /// 错误信息
        #[serde(flatten)]
        error: ErrorResponse,
    },
}

impl<T> ApiResponse<T> {
    /// 创建成功响应
    pub fn success(data: T) -> Self {
        ApiResponse::Success {
            data,
            message: None,
        }
    }

    /// 创建带消息的成功响应
    pub fn success_with_message(data: T, message: impl Into<String>) -> Self {
        ApiResponse::Success {
            data,
            message: Some(message.into()),
        }
    }

    /// 创建错误响应
    pub fn error(error: ErrorResponse) -> Self {
        ApiResponse::Error { error }
    }

    /// 从 CoreError 创建错误响应
    pub fn from_error(err: CoreError) -> Self {
        ApiResponse::Error { error: err.into() }
    }

    /// 检查是否成功
    pub fn is_success(&self) -> bool {
        matches!(self, ApiResponse::Success { .. })
    }

    /// 获取数据（如果是成功响应）
    pub fn data(&self) -> Option<&T> {
        match self {
            ApiResponse::Success { data, .. } => Some(data),
            _ => None,
        }
    }

    /// 获取错误（如果是错误响应）
    pub fn error_ref(&self) -> Option<&ErrorResponse> {
        match self {
            ApiResponse::Error { error } => Some(error),
            _ => None,
        }
    }
}

impl<T: Serialize> ApiResponse<T> {
    /// 转换为 JSON 字符串
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

// ==================== 分页响应 ====================

/// 分页请求参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRequest {
    /// 页码（从 1 开始）
    #[serde(default = "default_page")]
    pub page: u32,
    /// 每页大小
    #[serde(default = "default_page_size")]
    pub page_size: u32,
}

fn default_page() -> u32 {
    1
}

fn default_page_size() -> u32 {
    20
}

impl PageRequest {
    /// 创建默认分页请求
    pub fn new() -> Self {
        Self {
            page: 1,
            page_size: 20,
        }
    }

    /// 设置页码
    pub fn with_page(mut self, page: u32) -> Self {
        self.page = page.max(1);
        self
    }

    /// 设置每页大小
    pub fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = page_size.clamp(1, 1000);
        self
    }

    /// 计算偏移量
    pub fn offset(&self) -> u32 {
        (self.page - 1) * self.page_size
    }

    /// 计算总页数
    pub fn total_pages(&self, total_items: u64) -> u32 {
        total_items.div_ceil(self.page_size as u64) as u32
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self::new()
    }
}

/// 分页响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageResponse<T> {
    /// 当前页数据
    pub items: Vec<T>,
    /// 总条目数
    pub total: u64,
    /// 当前页码
    pub page: u32,
    /// 每页大小
    pub page_size: u32,
    /// 总页数
    pub total_pages: u32,
    /// 是否有下一页
    pub has_next: bool,
    /// 是否有上一页
    pub has_prev: bool,
}

impl<T> PageResponse<T> {
    /// 创建分页响应
    pub fn new(items: Vec<T>, total: u64, page_request: &PageRequest) -> Self {
        let page = page_request.page;
        let page_size = page_request.page_size;
        let total_pages = page_request.total_pages(total);
        let has_next = page < total_pages;
        let has_prev = page > 1;

        Self {
            items,
            total,
            page,
            page_size,
            total_pages,
            has_next,
            has_prev,
        }
    }

    /// 映射数据类型
    pub fn map<U, F>(self, f: F) -> PageResponse<U>
    where
        F: FnMut(T) -> U,
    {
        PageResponse {
            items: self.items.into_iter().map(f).collect(),
            total: self.total,
            page: self.page,
            page_size: self.page_size,
            total_pages: self.total_pages,
            has_next: self.has_next,
            has_prev: self.has_prev,
        }
    }

    /// 获取当前页条目数
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl<T: Serialize> PageResponse<T> {
    /// 包装为 API 响应
    pub fn into_response(self) -> ApiResponse<Self> {
        ApiResponse::success(self)
    }
}

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_response() {
        let error = ErrorResponse::new("TEST_ERROR", "Test message")
            .with_category("Test")
            .with_retryable(true);

        assert_eq!(error.code, "TEST_ERROR");
        assert_eq!(error.message, "Test message");
        assert_eq!(error.category, "Test");
        assert!(error.retryable);
    }

    #[test]
    fn test_api_response_success() {
        let response: ApiResponse<i32> = ApiResponse::success(42);
        assert!(response.is_success());
        assert_eq!(response.data(), Some(&42));
    }

    #[test]
    fn test_api_response_error() {
        let error = ErrorResponse::new("ERROR", "Something went wrong");
        let response: ApiResponse<i32> = ApiResponse::error(error);
        assert!(!response.is_success());
        assert!(response.data().is_none());
        assert!(response.error_ref().is_some());
    }

    #[test]
    fn test_page_request() {
        let req = PageRequest::new().with_page(2).with_page_size(50);
        assert_eq!(req.page, 2);
        assert_eq!(req.page_size, 50);
        assert_eq!(req.offset(), 50);
    }

    #[test]
    fn test_page_response() {
        let req = PageRequest::new().with_page(1).with_page_size(10);
        let items: Vec<i32> = (1..=10).collect();
        let response = PageResponse::new(items, 100, &req);

        assert_eq!(response.total, 100);
        assert_eq!(response.page, 1);
        assert_eq!(response.total_pages, 10);
        assert!(response.has_next);
        assert!(!response.has_prev);
    }
}
