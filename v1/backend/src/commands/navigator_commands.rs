//! 导航器状态相关命令
//!
//! 处理导航器展开/选中状态、过滤器配置等持久化操作

use crate::core::error::CoreError;
use crate::core::migration::global_init;

/// 保存导航器状态请求参数
#[derive(serde::Deserialize, Debug, specta::Type)]
pub struct SaveNavigatorStateInput {
    pub connection_id: String,
    pub expanded_keys: Vec<String>,
    pub selected_keys: Vec<String>,
    #[specta(skip)]
    pub filter_config: Option<serde_json::Value>,
}

/// 加载导航器状态响应
#[derive(serde::Serialize, Debug, specta::Type)]
pub struct LoadNavigatorStateResponse {
    pub expanded_keys: Vec<String>,
    pub selected_keys: Vec<String>,
    #[specta(skip)]
    pub filter_config: Option<serde_json::Value>,
}

/// 保存导航器状态
#[tauri::command]
#[specta::specta]
pub async fn save_navigator_state(input: SaveNavigatorStateInput) -> Result<(), CoreError> {
    let global_db = global_init::get_global_db_manager()
        .ok_or_else(|| "Global database manager not initialized".to_string())?;

    let expanded_keys_json = serde_json::to_string(&input.expanded_keys)
        .map_err(|e| CoreError::from(format!("序列化展开键失败: {}", e)))?;

    let selected_keys_json = serde_json::to_string(&input.selected_keys)
        .map_err(|e| CoreError::from(format!("序列化选中键失败: {}", e)))?;

    let filter_config_json = match input.filter_config {
        Some(config) => serde_json::to_string(&config)
            .map_err(|e| CoreError::from(format!("序列化过滤器配置失败: {}", e)))?,
        None => "{}".to_string(),
    };

    global_db
        .save_navigator_state(
            &input.connection_id,
            &expanded_keys_json,
            &selected_keys_json,
            &filter_config_json,
        )
        .await
        .map_err(|e| CoreError::from(format!("保存导航器状态失败: {}", e)))
}

/// 加载导航器状态
#[tauri::command]
#[specta::specta]
pub async fn load_navigator_state(
    connection_id: String,
) -> Result<LoadNavigatorStateResponse, CoreError> {
    let global_db = global_init::get_global_db_manager()
        .ok_or_else(|| "Global database manager not initialized".to_string())?;

    match global_db
        .load_navigator_state(&connection_id)
        .await
        .map_err(|e| CoreError::from(format!("加载导航器状态失败: {}", e)))?
    {
        Some((expanded_keys_json, selected_keys_json, filter_config_json)) => {
            let expanded_keys: Vec<String> = serde_json::from_str(&expanded_keys_json)
                .map_err(|e| CoreError::from(format!("反序列化展开键失败: {}", e)))
                .unwrap_or_default();

            let selected_keys: Vec<String> = serde_json::from_str(&selected_keys_json)
                .map_err(|e| CoreError::from(format!("反序列化选中键失败: {}", e)))
                .unwrap_or_default();

            let filter_config: Option<serde_json::Value> =
                serde_json::from_str(&filter_config_json).ok();

            Ok(LoadNavigatorStateResponse {
                expanded_keys,
                selected_keys,
                filter_config,
            })
        }
        None => {
            // 没有保存的状态，返回默认值
            Ok(LoadNavigatorStateResponse {
                expanded_keys: vec![],
                selected_keys: vec![],
                filter_config: None,
            })
        }
    }
}
