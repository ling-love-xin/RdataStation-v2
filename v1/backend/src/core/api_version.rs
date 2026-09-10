pub const API_VERSION: &str = "1.0.0";

pub const API_VERSION_INFO: ApiVersionInfo = ApiVersionInfo {
    version: API_VERSION,
    major: 1,
    minor: 0,
    patch: 0,
    codename: "Foundation",
};

pub struct ApiVersionInfo {
    pub version: &'static str,
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub codename: &'static str,
}
