//! 插件权限管理
//!
//! 管理插件权限的定义、验证和授予

use crate::core::error::{CommonError, CoreError};
use crate::core::plugin::manifest::PluginManifest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// 权限类型
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionType {
    /// 前端权限
    Frontend,
    /// WASM 权限
    Wasm,
}

/// 权限定义
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Permission {
    /// 权限 ID
    pub id: String,
    /// 权限类型
    pub permission_type: PermissionType,
    /// 权限描述
    pub description: String,
    /// 权限分类
    pub category: String,
}

impl Permission {
    /// 创建新权限
    pub fn new(
        id: &str,
        permission_type: PermissionType,
        description: &str,
        category: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            permission_type,
            description: description.to_string(),
            category: category.to_string(),
        }
    }
}

/// 权限授予状态
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrantStatus {
    /// 已授予
    Granted,
    /// 已拒绝
    Denied,
    /// 待用户确认
    Pending,
    /// 从未请求过
    NotRequested,
}

/// 插件权限授予记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionGrant {
    /// 插件 ID
    pub plugin_id: String,
    /// 权限 ID
    pub permission_id: String,
    /// 授予状态
    pub status: GrantStatus,
    /// 授予时间
    pub granted_at: Option<String>,
}

/// 权限管理器
pub struct PermissionManager {
    /// 所有可用权限
    available_permissions: Arc<HashMap<String, Permission>>,
    /// 插件权限授予记录
    grants: Arc<std::sync::RwLock<HashMap<String, Vec<PermissionGrant>>>>,
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionManager {
    /// 创建新的权限管理器
    pub fn new() -> Self {
        let mut available = HashMap::new();

        // 内置权限定义
        self::register_builtin_permissions(&mut available);

        Self {
            available_permissions: Arc::new(available),
            grants: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// 获取插件需要的权限列表
    pub fn get_required_permissions(&self, manifest: &PluginManifest) -> Vec<Permission> {
        let mut permissions = Vec::new();

        // 前端权限
        for perm_id in &manifest.permissions.frontend {
            if let Some(perm) = self.available_permissions.get(perm_id) {
                permissions.push(perm.clone());
            }
        }

        // WASM 权限
        for perm_id in &manifest.permissions.wasm {
            if let Some(perm) = self.available_permissions.get(perm_id) {
                permissions.push(perm.clone());
            }
        }

        permissions
    }

    /// 验证插件权限是否满足
    pub async fn validate_permissions(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Result<(), CoreError> {
        let required = self.get_required_permissions(manifest);
        let grants = self.get_plugin_grants(plugin_id).await;

        for perm in required {
            let granted = grants.iter().find(|g| g.permission_id == perm.id);

            if let Some(grant) = granted {
                if grant.status != GrantStatus::Granted {
                    return Err(CoreError::common(CommonError::general(format!(
                        "Permission {} not granted",
                        perm.id
                    ))));
                }
            } else {
                return Err(CoreError::common(CommonError::general(format!(
                    "Permission {} not requested",
                    perm.id
                ))));
            }
        }

        Ok(())
    }

    /// 授予插件权限
    pub async fn grant_permission(
        &self,
        plugin_id: &str,
        permission_id: &str,
    ) -> Result<(), CoreError> {
        if !self.available_permissions.contains_key(permission_id) {
            return Err(CoreError::common(CommonError::general(format!(
                "Unknown permission: {}",
                permission_id
            ))));
        }

        let mut grants = self.grants.write().map_err(|_| {
            CoreError::common(CommonError::general("Failed to lock grants".to_string()))
        })?;

        let plugin_grants = grants.entry(plugin_id.to_string()).or_default();

        // 查找或创建权限授予记录
        if let Some(grant) = plugin_grants
            .iter_mut()
            .find(|g| g.permission_id == permission_id)
        {
            grant.status = GrantStatus::Granted;
            grant.granted_at = Some(chrono::Utc::now().to_rfc3339());
        } else {
            plugin_grants.push(PermissionGrant {
                plugin_id: plugin_id.to_string(),
                permission_id: permission_id.to_string(),
                status: GrantStatus::Granted,
                granted_at: Some(chrono::Utc::now().to_rfc3339()),
            });
        }

        Ok(())
    }

    /// 拒绝插件权限
    pub async fn deny_permission(
        &self,
        plugin_id: &str,
        permission_id: &str,
    ) -> Result<(), CoreError> {
        let mut grants = self.grants.write().map_err(|_| {
            CoreError::common(CommonError::general("Failed to lock grants".to_string()))
        })?;

        if let Some(plugin_grants) = grants.get_mut(plugin_id) {
            if let Some(grant) = plugin_grants
                .iter_mut()
                .find(|g| g.permission_id == permission_id)
            {
                grant.status = GrantStatus::Denied;
            } else {
                plugin_grants.push(PermissionGrant {
                    plugin_id: plugin_id.to_string(),
                    permission_id: permission_id.to_string(),
                    status: GrantStatus::Denied,
                    granted_at: None,
                });
            }
        }

        Ok(())
    }

    /// 获取插件的权限授予记录
    pub async fn get_plugin_grants(&self, plugin_id: &str) -> Vec<PermissionGrant> {
        let grants = self.grants.read();
        let Ok(grants) = grants else {
            return Vec::new();
        };

        grants.get(plugin_id).cloned().unwrap_or_default()
    }

    /// 检查插件是否有指定权限
    pub async fn has_permission(&self, plugin_id: &str, permission_id: &str) -> bool {
        let grants = self.get_plugin_grants(plugin_id).await;

        grants
            .iter()
            .any(|g| g.permission_id == permission_id && g.status == GrantStatus::Granted)
    }

    /// 重置插件权限
    pub async fn reset_plugin_permissions(&self, plugin_id: &str) {
        let mut grants = self.grants.write().ok();
        if let Some(ref mut g) = grants {
            g.remove(plugin_id);
        }
    }

    /// 获取所有可用权限
    pub fn get_all_permissions(&self) -> Vec<Permission> {
        self.available_permissions.values().cloned().collect()
    }

    /// 按分类获取权限
    pub fn get_permissions_by_category(&self, category: &str) -> Vec<Permission> {
        self.available_permissions
            .values()
            .filter(|p| p.category == category)
            .cloned()
            .collect()
    }
}

/// 注册内置权限
fn register_builtin_permissions(permissions: &mut HashMap<String, Permission>) {
    // 数据相关权限
    permissions.insert(
        "data:read".to_string(),
        Permission::new(
            "data:read",
            PermissionType::Frontend,
            "Read data from connections",
            "Data",
        ),
    );
    permissions.insert(
        "data:write".to_string(),
        Permission::new(
            "data:write",
            PermissionType::Frontend,
            "Write data to connections",
            "Data",
        ),
    );
    permissions.insert(
        "data:query".to_string(),
        Permission::new(
            "data:query",
            PermissionType::Frontend,
            "Execute SQL queries",
            "Data",
        ),
    );

    // UI 相关权限
    permissions.insert(
        "ui:modify".to_string(),
        Permission::new(
            "ui:modify",
            PermissionType::Frontend,
            "Modify user interface",
            "UI",
        ),
    );
    permissions.insert(
        "ui:add-panel".to_string(),
        Permission::new(
            "ui:add-panel",
            PermissionType::Frontend,
            "Add custom panels",
            "UI",
        ),
    );

    // WASM 权限
    permissions.insert(
        "plugin:wasm".to_string(),
        Permission::new(
            "plugin:wasm",
            PermissionType::Wasm,
            "Execute WASM code",
            "Plugin",
        ),
    );
    permissions.insert(
        "db:read".to_string(),
        Permission::new(
            "db:read",
            PermissionType::Wasm,
            "Read database metadata",
            "Database",
        ),
    );
    permissions.insert(
        "db:query".to_string(),
        Permission::new(
            "db:query",
            PermissionType::Wasm,
            "Execute database queries",
            "Database",
        ),
    );

    // 系统权限
    permissions.insert(
        "system:filesystem".to_string(),
        Permission::new(
            "system:filesystem",
            PermissionType::Wasm,
            "Access file system",
            "System",
        ),
    );
    permissions.insert(
        "system:network".to_string(),
        Permission::new(
            "system:network",
            PermissionType::Wasm,
            "Make network requests",
            "System",
        ),
    );
}

/// 全局权限管理器实例
static PERMISSION_MANAGER: std::sync::OnceLock<Arc<PermissionManager>> = std::sync::OnceLock::new();

/// 获取全局权限管理器
pub fn get_permission_manager() -> Arc<PermissionManager> {
    PERMISSION_MANAGER
        .get_or_init(|| Arc::new(PermissionManager::new()))
        .clone()
}
