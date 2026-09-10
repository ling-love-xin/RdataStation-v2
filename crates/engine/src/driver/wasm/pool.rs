use shared::error::CoreError;

pub struct WasmPool;

impl WasmPool {
    pub fn new() -> Result<Self, CoreError> {
        Ok(Self)
    }
}
