use serde::Serialize;
use specta::Type;

use crate::core::api_version::API_VERSION_INFO;

#[derive(Debug, Clone, Serialize, Type)]
pub struct ApiVersionResponse {
    pub version: String,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub codename: String,
}

#[tauri::command]
#[specta::specta]
pub fn get_api_version() -> ApiVersionResponse {
    ApiVersionResponse {
        version: API_VERSION_INFO.version.to_string(),
        major: API_VERSION_INFO.major,
        minor: API_VERSION_INFO.minor,
        patch: API_VERSION_INFO.patch,
        codename: API_VERSION_INFO.codename.to_string(),
    }
}
