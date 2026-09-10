//! Core 错误处理宏模块
//!
//! 提供便捷的错误创建宏，限制在 core 模块内使用。

/// 创建通用错误
///
/// # 示例
/// ```
/// return core_err!("Something went wrong");
/// ```
#[macro_export]
macro_rules! core_err {
    ($msg:expr) => {
        $crate::core::error::common_err($msg)
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::core::error::common_err(format!($fmt, $($arg)*))
    };
}

/// 提前返回错误
///
/// # 示例
/// ```
/// bail!("Connection failed");
/// bail!("Failed to connect to {}", host);
/// ```
#[macro_export]
macro_rules! bail {
    ($msg:expr) => {
        return Err($crate::core_err!($msg))
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err($crate::core_err!($fmt, $($arg)*))
    };
}

/// 条件断言，失败时返回错误
///
/// # 示例
/// ```
/// ensure!(!url.is_empty(), "URL cannot be empty");
/// ensure!(port > 0, "Port must be positive, got {}", port);
/// ```
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $msg:expr) => {
        if !$cond {
            $crate::bail!($msg);
        }
    };
    ($cond:expr, $fmt:expr, $($arg:tt)*) => {
        if !$cond {
            $crate::bail!($fmt, $($arg)*);
        }
    };
}

/// 创建连接错误
///
/// # 示例
/// ```
/// return conn_err!("conn1", ConnectionError::timeout("conn1", 5000));
/// ```
#[macro_export]
macro_rules! conn_err {
    ($conn_id:expr, $kind:expr) => {
        $crate::core::error::CoreError::connection($kind)
    };
}

/// 创建数据库查询错误
///
/// # 示例
/// ```
/// return query_err!("SELECT * FROM users", "Table not found");
/// ```
#[macro_export]
macro_rules! query_err {
    ($sql:expr, $reason:expr) => {
        $crate::core::error::query_err($sql, $reason)
    };
    ($sql:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::core::error::query_err($sql, format!($fmt, $($arg)*))
    };
}

/// 创建存储错误
///
/// # 示例
/// ```
/// return storage_err!("connection_store", "save", "disk full");
/// ```
#[macro_export]
macro_rules! storage_err {
    ($store:expr, $op:expr, $reason:expr) => {
        $crate::core::error::storage_err($store, $op, $reason)
    };
    ($store:expr, $op:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::core::error::storage_err($store, $op, format!($fmt, $($arg)*))
    };
}

/// 创建无效参数错误
///
/// # 示例
/// ```
/// return invalid_arg!("host", "cannot be empty");
/// ```
#[macro_export]
macro_rules! invalid_arg {
    ($param:expr, $reason:expr) => {
        $crate::core::error::invalid_arg($param, $reason)
    };
    ($param:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::core::error::invalid_arg($param, format!($fmt, $($arg)*))
    };
}

/// 创建不支持功能错误
///
/// # 示例
/// ```
/// return not_supported!("MySQL SSL mode");
/// ```
#[macro_export]
macro_rules! not_supported {
    ($feature:expr) => {
        $crate::core::error::not_supported($feature)
    };
}

/// 创建超时错误
///
/// # 示例
/// ```
/// return timeout!("query execution", 30000);
/// ```
#[macro_export]
macro_rules! timeout {
    ($operation:expr, $duration_ms:expr) => {
        $crate::core::error::timeout($operation, $duration_ms)
    };
}

#[cfg(test)]
mod tests {
    use crate::core::error::{CommonError, CoreError};

    #[test]
    fn test_core_err_macro() {
        let err = core_err!("test error");
        assert!(matches!(err, CoreError::Common(CommonError::General(_))));

        let err = core_err!("formatted {} error", "test");
        assert!(matches!(err, CoreError::Common(CommonError::General(_))));
    }

    #[test]
    fn test_invalid_arg_macro() {
        let err = invalid_arg!("host", "cannot be empty");
        assert!(matches!(
            err,
            CoreError::Common(CommonError::InvalidArgument { .. })
        ));
    }

    #[test]
    fn test_not_supported_macro() {
        let err = not_supported!("feature");
        assert!(matches!(
            err,
            CoreError::Common(CommonError::NotSupported(_))
        ));
    }

    #[test]
    fn test_timeout_macro() {
        let err = timeout!("operation", 5000);
        assert!(matches!(
            err,
            CoreError::Common(CommonError::Timeout { .. })
        ));
    }

    #[test]
    fn test_query_err_macro() {
        let err = query_err!("SELECT * FROM users", "table not found");
        assert!(matches!(err, CoreError::Database(_)));
    }

    #[test]
    fn test_storage_err_macro() {
        let err = storage_err!("store", "save", "disk full");
        assert!(matches!(err, CoreError::Storage(_)));
    }
}
