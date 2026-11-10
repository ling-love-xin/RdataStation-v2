use serde_json::{Value, json};

/// §4.2.2 的九个错误码，数值必须与宿主 `sidecar/proto.rs` 一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    DriverNotSupported,
    SessionNotFound,
    SqlError,
    Cancelled,
    Timeout,
    CapabilityDenied,
    ProtocolVersionMismatch,
    ResourceLimit,
    ConnectFailed,
}

impl ErrorCode {
    pub fn code(self) -> i64 {
        match self {
            Self::DriverNotSupported => -32001,
            Self::SessionNotFound => -32002,
            Self::SqlError => -32003,
            Self::Cancelled => -32004,
            Self::Timeout => -32005,
            Self::CapabilityDenied => -32006,
            Self::ProtocolVersionMismatch => -32007,
            Self::ResourceLimit => -32008,
            Self::ConnectFailed => -32009,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::DriverNotSupported => "driver_not_supported",
            Self::SessionNotFound => "session_not_found",
            Self::SqlError => "sql_error",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::CapabilityDenied => "capability_denied",
            Self::ProtocolVersionMismatch => "protocol_version_mismatch",
            Self::ResourceLimit => "resource_limit",
            Self::ConnectFailed => "connect_failed",
        }
    }
}

/// 驱动侧统一错误。`message` 原样进 JSON-RPC error.message。
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct DriverError {
    pub code: ErrorCode,
    pub message: String,
    pub sqlstate: Option<String>,
    /// 1 基字符位置；拿不到就是 None。
    pub position: Option<u32>,
}

impl DriverError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            sqlstate: None,
            position: None,
        }
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::DriverNotSupported, message)
    }

    pub fn connect(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ConnectFailed, message)
    }

    pub fn sql(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::SqlError, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::CapabilityDenied, message)
    }

    pub fn with_sqlstate(mut self, sqlstate: impl Into<String>) -> Self {
        self.sqlstate = Some(sqlstate.into());
        self
    }

    pub fn with_position(mut self, position: u32) -> Self {
        self.position = Some(position);
        self
    }

    pub fn to_json(&self) -> Value {
        let mut data = json!({ "driver_code": self.code.name() });
        if let Some(sqlstate) = &self.sqlstate {
            data["sqlstate"] = json!(sqlstate);
        }
        if let Some(position) = self.position {
            data["position"] = json!(position);
        }
        json!({
            "code": self.code.code(),
            "message": self.message,
            "data": data,
        })
    }
}
