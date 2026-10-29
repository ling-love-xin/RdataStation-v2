use shared::error::CoreError;

pub struct JvmManager;

impl JvmManager {
    pub fn new() -> Result<Self, CoreError> {
        Ok(Self)
    }
}
