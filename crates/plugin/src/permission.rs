//! 插件权限管理
//!
//! 管理插件权限的定义、验证和授予

use crate::manifest::PluginManifest;
use serde::{Deserialize, Serialize};
use shared::error::{CommonError, CoreError};
use std::collections::HashMap;
use std::sync::Arc;

/// 权限类型
///
/// 四轨的**语义不同**（dev-plan §4.6.1），不是四个标签：
///
/// | 轨 | 能否强制沙箱 | `permissions` 字段的作用 | 信任来源 |
/// | --- | --- | --- | --- |
/// | `Frontend` / `Wasm` | ✅（无 host function 即无能力） | **门控** | 沙箱 + 清单声明 ∩ 宿主授予 |
/// | `Sidecar` / `Driver` | ❌（原生进程权限等同宿主） | **仅展示与告警** | 安装时的显式信任动作 + 包签名 + 来源标注 |
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionType {
    /// 前端权限
    Frontend,
    /// WASM 权限
    Wasm,
    /// sidecar（驱动插件）的**进程级**权限：子进程 / 网络 / 文件 / 环境变量。
    ///
    /// 危害面是**这台机器**。沙箱强制不了，故只能靠安装时逐项明示 + 签名。
    Sidecar,
    /// 驱动插件声明的**数据面能力**：query / metadata / transaction / cancel / cursor / arrow。
    ///
    /// 危害面是**数据**（怎么用数据），与 [`Self::Sidecar`] 的「动这台机器」分开列，
    /// 因为两者在安装页要说的话完全不同。同样是展示/告警口径。
    Driver,
}

impl PermissionType {
    /// 是否参与门控（未授予即拒绝）。
    ///
    /// `Sidecar` / `Driver` **不是**门控轨：原生进程沙箱强制不了，它的把关在安装那一步；
    /// 这里若也拦一道，只会把「安装时必须显式授权」这条设计绕过去。
    pub fn is_gating(&self) -> bool {
        matches!(self, Self::Frontend | Self::Wasm)
    }
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

    /// 获取插件需要的权限列表（四轨合并）。
    ///
    /// **找不到的 id 会在这里被静默丢弃**（那是个洞：声明了但没登记 = 不检查）。
    /// 要展示这类项用 [`Self::unknown_permissions`]。
    pub fn get_required_permissions(&self, manifest: &PluginManifest) -> Vec<Permission> {
        let p = &manifest.permissions;

        p.frontend
            .iter()
            .chain(&p.wasm)
            .chain(&p.sidecar)
            .chain(&p.driver)
            .filter_map(|id| self.available_permissions.get(id).cloned())
            .collect()
    }

    /// 清单里声明了、但权限登记表里没有的 id。
    ///
    /// 这类项**不能被当成"没申请"**：要么是清单写错了，要么是宿主太旧不认识这个新权限。
    /// 两种都该在安装页如实显示（「能力缺失必须有处表达」）。
    pub fn unknown_permissions(&self, manifest: &PluginManifest) -> Vec<String> {
        let p = &manifest.permissions;

        p.frontend
            .iter()
            .chain(&p.wasm)
            .chain(&p.sidecar)
            .chain(&p.driver)
            .filter(|id| !self.available_permissions.contains_key(*id))
            .cloned()
            .collect()
    }

    /// 验证插件权限是否满足。
    ///
    /// ⚠️ **只有门控轨参与"未授予即拒绝"**（见 [`PermissionType::is_gating`]）。
    /// `Sidecar` / `Driver` 是展示轨：原生进程的能力不可能靠这张表拦住，
    /// 它靠的是安装时的逐项授权 + 包签名 —— 在这里多拦一道反而会让作者以为"声明就够了"。
    pub async fn validate_permissions(
        &self,
        plugin_id: &str,
        manifest: &PluginManifest,
    ) -> Result<(), CoreError> {
        let required: Vec<Permission> = self
            .get_required_permissions(manifest)
            .into_iter()
            .filter(|p| p.permission_type.is_gating())
            .collect();
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

    // ---- 驱动插件（M9）的两轨：id 沿用设计文档的写法（点号，见
    // `docs/architecture/plugin/plugin-prototype-design.md` §2.1 与 driver-plugin 原型）。
    // 上面 `data:read` / `system:filesystem` 那批是 v1 遗留的冒号风格，P0 不动它们。

    // sidecar 轨：进程级权限（危害面 = 这台机器）；**安装时逐项明示，不进运行时门控**
    for (id, desc) in [
        ("spawn.child_process", "启动子进程（sidecar 本体）"),
        ("net.connect", "发起网络连接（仅清单声明的端口段）"),
        ("fs.read_plugin_data", "读取自己的插件数据目录"),
        ("env.inherit", "继承宿主环境变量（默认关闭，仅白名单键）"),
        (
            "credentials.database",
            "接收数据库凭据（该插件的日志会记录每次交付）",
        ),
    ] {
        permissions.insert(
            id.to_string(),
            Permission::new(id, PermissionType::Sidecar, desc, "Sidecar"),
        );
    }

    // 驱动轨：数据面能力（危害面 = 数据）；用于「这个驱动会做什么」的如实告知
    for (id, desc) in [
        ("driver.query", "执行查询"),
        (
            "driver.metadata",
            "浏览元数据（catalogs / schemas / tables / columns）",
        ),
        ("driver.transaction", "真事务（begin / commit / rollback）"),
        ("driver.cancel", "真取消（中断正在执行的查询）"),
        ("driver.cursor", "流式游标（边读边回）"),
        ("driver.arrow", "结果集走 Arrow IPC（而非 JSON）"),
    ] {
        permissions.insert(
            id.to_string(),
            Permission::new(id, PermissionType::Driver, desc, "Driver"),
        );
    }
}

/// 全局权限管理器实例
static PERMISSION_MANAGER: std::sync::OnceLock<Arc<PermissionManager>> = std::sync::OnceLock::new();

/// 获取全局权限管理器
pub fn get_permission_manager() -> Arc<PermissionManager> {
    PERMISSION_MANAGER
        .get_or_init(|| Arc::new(PermissionManager::new()))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个最小可解析的清单，只把 `[permissions]` 交出去。
    fn manifest(permissions: &str) -> PluginManifest {
        let toml = format!(
            r#"
[plugin]
id = "example.test"
name = "Test Plugin"
version = "1.0.0"
publisher = "example"

[plugin.engines]
rdatastation = "^0.1.0"

[capabilities.frontend]
entry = "./extension.js"

{permissions}
"#
        );
        toml::from_str(&toml).expect("清单应能解析")
    }

    #[test]
    fn required_permissions_covers_all_four_tracks() {
        let m = manifest(
            r#"
[permissions]
frontend = ["data:query"]
wasm = ["db:query"]
sidecar = ["spawn.child_process"]
driver = ["driver.transaction"]
"#,
        );

        let got = PermissionManager::new().get_required_permissions(&m);
        assert_eq!(got.len(), 4, "四轨各一条：{got:?}");
        assert!(
            got.iter()
                .any(|p| p.permission_type == PermissionType::Sidecar)
        );
        assert!(
            got.iter()
                .any(|p| p.permission_type == PermissionType::Driver)
        );
    }

    /// 声明了但登记表里没有的 id 必须能被问出来 —— 静默丢弃等于「不用检查」。
    #[test]
    fn unknown_permissions_are_surfaced_not_dropped() {
        let m = manifest(
            r#"
[permissions]
sidecar = ["spawn.child_process", "no.such.thing"]
"#,
        );

        let mgr = PermissionManager::new();
        assert_eq!(mgr.unknown_permissions(&m), vec!["no.such.thing"]);
        assert_eq!(mgr.get_required_permissions(&m).len(), 1);
    }

    /// §4.6.1：sidecar / driver 是展示轨，「未授予」不能拿去拒加载（把关在安装那一步）；
    /// 而 wasm / frontend 未授予必须拒。这条断言把双轨语义钉住。
    #[tokio::test]
    async fn validate_permissions_ignores_display_tracks() {
        let mgr = PermissionManager::new();

        let display_only = manifest(
            r#"
[permissions]
sidecar = ["spawn.child_process", "net.connect"]
driver = ["driver.query"]
credentials = "database"
"#,
        );
        assert!(
            mgr.validate_permissions("example.test", &display_only)
                .await
                .is_ok(),
            "展示轨未授予不应导致校验失败"
        );

        let gating = manifest(
            r#"
[permissions]
wasm = ["db:query"]
"#,
        );
        assert!(
            mgr.validate_permissions("example.test", &gating)
                .await
                .is_err(),
            "门控轨未授予必须失败"
        );
    }

    #[test]
    fn gating_matches_the_two_track_semantics() {
        assert!(PermissionType::Frontend.is_gating());
        assert!(PermissionType::Wasm.is_gating());
        assert!(!PermissionType::Sidecar.is_gating());
        assert!(!PermissionType::Driver.is_gating());
    }
}
