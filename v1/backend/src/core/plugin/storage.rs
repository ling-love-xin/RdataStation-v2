
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde::{de::DeserializeOwned, Serialize};
use serde_json;

use crate::core::error::{CommonError, CoreError};

/// 插件存储管理器
pub struct PluginStorage {
    base_path: PathBuf,
    in_memory_cache: Arc<RwLock<HashMap<String, String>>>,
}

impl PluginStorage {
    /// 创建新的存储管理器
    pub fn new(base_path: PathBuf) -> Self {
        Self {
            base_path,
            in_memory_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 获取插件的存储键
    fn get_storage_path(&self, plugin_id: &str, key: &str) -> PathBuf {
        self.base_path.join(plugin_id).join(key)
    }

    /// 保存数据（内存）
    pub fn set(&self, plugin_id: &str, key: &str, value: String) -> Result<(), CoreError> {
        let mut cache = self
            .in_memory_cache
            .write()
            .map_err(|e| CoreError::common(CommonError::Internal(format!("Lock poisoned: {}", e))))?;

        let full_key = format!("{}/{}", plugin_id, key);
        cache.insert(full_key, value);
        Ok(())
    }

    /// 获取数据（先从内存）
    pub fn get(&self, plugin_id: &str, key: &str) -> Result<Option<String>, CoreError> {
        let full_key = format!("{}/{}", plugin_id, key);
        let cache = self
            .in_memory_cache
            .read()
            .map_err(|e| CoreError::common(CommonError::Internal(format!("Lock poisoned: {}", e))))?;

        if let Some(value) = cache.get(&full_key) {
            return Ok(Some(value.clone()));
        }

        Ok(None)
    }

    /// 序列化并保存
    pub fn set_serialized<T: Serialize>(&self, plugin_id: &str, key: &str, value: &T) -> Result<(), CoreError> {
        let serialized = serde_json::to_string(value)
            .map_err(|e| CoreError::common(CommonError::Internal(format!("Serialization failed: {}", e))))?;
        self.set(plugin_id, key, serialized)?;
        Ok(())
    }

    /// 获取并反序列化
    pub fn get_deserialized<T: DeserializeOwned>(&self, plugin_id: &str, key: &str) -> Result<Option<T>, CoreError> {
        if let Some(serialized) = self.get(plugin_id, key)? {
            let value = serde_json::from_str(&serialized)
                .map_err(|e| CoreError::common(CommonError::Internal(format!("Deserialization failed: {}", e))))?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    /// 删除键
    pub fn delete(&self, plugin_id: &str, key: &str) -> Result<(), CoreError> {
        let mut cache = self
            .in_memory_cache
            .write()
            .map_err(|e| CoreError::common(CommonError::Internal(format!("Lock poisoned: {}", e))))?;

        let full_key = format!("{}/{}", plugin_id, key);
        cache.remove(&full_key);
        Ok(())
    }

    /// 清除插件所有数据
    pub fn clear_plugin_data(&self, plugin_id: &str) -> Result<(), CoreError> {
        let mut cache = self
            .in_memory_cache
            .write()
            .map_err(|e| CoreError::common(CommonError::Internal(format!("Lock poisoned: {}", e))))?;

        let prefix = format!("{}/", plugin_id);
        cache.retain(|key, _| !key.starts_with(&prefix));
        Ok(())
    }

    /// 持久化内存数据到磁盘
    pub fn flush_to_disk(&self) -> Result<(), CoreError> {
        // TODO: 实际项目中需要实现
        Ok(())
    }
}

