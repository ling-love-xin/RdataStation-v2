//! 缺失驱动信息（自 workbench driver_service 上移，project/workbench 共用）

use specta::Type;

/// 项目打开时检测到的缺失驱动信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct MissingDriver {
    pub driver_id: String,
    pub driver_name: String,
    pub download_url: String,
}
