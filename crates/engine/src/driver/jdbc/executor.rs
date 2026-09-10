use shared::error::CoreError;

pub struct JdbcExecutor;

impl JdbcExecutor {
    pub fn new() -> Result<Self, CoreError> {
        Ok(Self)
    }
}
