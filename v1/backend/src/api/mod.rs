//! API 层模块
//!
//! 负责与前端交互，定义所有 API 数据传输对象（DTO）：
//! - 错误响应格式（ErrorResponse）
//! - 成功响应包装（ApiResponse<T>）
//! - 分页请求/响应（PageRequest, PageResponse<T>）
//! - 共享数据模型（QueryResult, Value 等，从 core 重新导出）
//!
//! 注意：所有业务模型从 core 层重新导出，保持单一数据源

pub mod dto;

// 重新导出 DTO 类型
pub use dto::{
    // 响应包装
    ApiResponse,
    // 错误相关
    ErrorResponse,
    // 分页相关
    PageRequest,
    PageResponse,
    // 数据模型（从 core 重新导出）
    QueryResult,
    Row,
    Value,
};
