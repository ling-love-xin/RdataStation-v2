use shared::error::CoreError;

pub struct WasmAdapter;

impl WasmAdapter {
    pub fn new() -> Result<Self, CoreError> {
        Ok(Self)
    }
}
