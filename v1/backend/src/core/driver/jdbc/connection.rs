use crate::core::error::CoreError;

pub struct JdbcConnection;

impl JdbcConnection {
    pub fn new() -> Result<Self, CoreError> {
        Ok(Self)
    }
}
