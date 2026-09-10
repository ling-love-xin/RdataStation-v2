use std::collections::HashMap;
use std::sync::Mutex;

use duckdb::Connection;

use crate::core::error::{CommonError, CoreError};

/// 扩展状态
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionStatus {
    /// 未安装
    NotInstalled,
    /// 已安装但未加载
    Installed,
    /// 已加载
    Loaded,
}

impl std::fmt::Display for ExtensionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtensionStatus::NotInstalled => write!(f, "未安装"),
            ExtensionStatus::Installed => write!(f, "已安装"),
            ExtensionStatus::Loaded => write!(f, "已加载"),
        }
    }
}

/// 扩展信息
#[derive(Clone)]
pub struct ExtensionInfo {
    /// 扩展名称
    pub name: String,
    /// 当前状态
    pub status: ExtensionStatus,
    /// 版本号（如果已知）
    pub version: Option<String>,
    /// 是否已签名
    pub signed: bool,
}

impl ExtensionInfo {
    /// 创建新的扩展信息。
    pub fn new(name: &str, status: ExtensionStatus, signed: bool) -> Self {
        ExtensionInfo {
            name: name.to_string(),
            status,
            version: None,
            signed,
        }
    }

    /// 生成安装扩展的 SQL 语句。
    pub fn install_sql(&self) -> String {
        format!("INSTALL {}", self.name)
    }

    /// 生成加载扩展的 SQL 语句。
    pub fn load_sql(&self) -> String {
        format!("LOAD {}", self.name)
    }

    /// 生成卸载扩展的 SQL 语句。
    pub fn unload_sql(&self) -> String {
        format!("UNLOAD {}", self.name)
    }
}

/// DuckDB 扩展管理器
///
/// 负责 DuckDB 自身扩展的发现、安装、加载、卸载、状态查询。
///
/// # 扩展类型
/// - **内置扩展**: parquet、json 等，启动自动加载
/// - **按需扩展**: spatial、excel、httpfs、fts、mysql/postgres 等，首次使用自动安装加载
///
/// # 关键设计约束
/// 1. 扩展二进制与 DuckDB 版本、操作系统强绑定，不兼容自动拦截
/// 2. INSTALL 为全局缓存操作，LOAD 为会话级操作
/// 3. 仅签名扩展默认加载，未签名需手动开启配置
/// 4. 扩展目录固定指定路径: ~/.rdatastation/duckdb/extensions/
pub struct ExtensionManager {
    /// 扩展缓存: name -> ExtensionInfo
    extensions: Mutex<HashMap<String, ExtensionInfo>>,
    /// 扩展目录路径
    extension_dir: String,
}

impl ExtensionManager {
    /// 创建新的扩展管理器。
    ///
    /// # 参数
    /// - `extension_dir`: 扩展文件存放目录
    pub fn new(extension_dir: &str) -> Self {
        ExtensionManager {
            extensions: Mutex::new(HashMap::new()),
            extension_dir: extension_dir.to_string(),
        }
    }

    /// 发现并查询当前 DuckDB 实例的扩展状态。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    ///
    /// # 返回
    /// - `Ok(Vec<ExtensionInfo>)`: 扩展信息列表
    /// - `Err(CoreError)`: 查询失败
    pub fn discover_extensions(&self, conn: &Connection) -> Result<Vec<ExtensionInfo>, CoreError> {
        let sql = Self::generate_discover_sql();

        let mut stmt = conn.prepare(&sql).map_err(|e| {
            CoreError::common(CommonError::General(format!("查询扩展状态失败: {}", e)))
        })?;

        let mut rows = stmt.query([]).map_err(|e| {
            CoreError::common(CommonError::General(format!("执行扩展查询失败: {}", e)))
        })?;

        let mut extensions = Vec::new();
        while let Some(row) = rows.next().map_err(|e| {
            CoreError::common(CommonError::General(format!("获取扩展行失败: {}", e)))
        })? {
            let name: String = row.get(0).unwrap_or_default();
            let installed: bool = row.get(2).unwrap_or(false);
            let loaded: bool = row.get(3).unwrap_or(false);

            let status = if loaded {
                ExtensionStatus::Loaded
            } else if installed {
                ExtensionStatus::Installed
            } else {
                ExtensionStatus::NotInstalled
            };

            extensions.push(ExtensionInfo {
                name,
                status,
                version: None,
                signed: true,
            });
        }

        Ok(extensions)
    }

    /// 安装扩展。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// - `Ok(())`: 安装成功
    /// - `Err(CoreError)`: 安装失败
    pub fn install_extension(&self, conn: &Connection, name: &str) -> Result<(), CoreError> {
        Self::validate_extension_name(name)?;

        let sql = Self::generate_install_sql(name);
        tracing::info!("[ExtensionManager] 安装扩展 SQL: {}", sql);

        conn.execute_batch(&sql)
            .map_err(|e| CoreError::common(CommonError::General(format!("安装扩展失败: {}", e))))?;

        // 更新缓存状态
        self.register_extension(ExtensionInfo::new(name, ExtensionStatus::Installed, true));

        tracing::info!("[ExtensionManager] 扩展已安装: {}", name);

        Ok(())
    }

    /// 加载扩展。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// - `Ok(())`: 加载成功
    /// - `Err(CoreError)`: 加载失败
    pub fn load_extension(&self, conn: &Connection, name: &str) -> Result<(), CoreError> {
        Self::validate_extension_name(name)?;

        let sql = Self::generate_load_sql(name);
        tracing::info!("[ExtensionManager] 加载扩展 SQL: {}", sql);

        conn.execute_batch(&sql)
            .map_err(|e| CoreError::common(CommonError::General(format!("加载扩展失败: {}", e))))?;

        // 更新缓存状态
        self.register_extension(ExtensionInfo::new(name, ExtensionStatus::Loaded, true));

        tracing::info!("[ExtensionManager] 扩展已加载: {}", name);

        Ok(())
    }

    /// 卸载扩展。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// - `Ok(())`: 卸载成功
    /// - `Err(CoreError)`: 卸载失败
    pub fn unload_extension(&self, conn: &Connection, name: &str) -> Result<(), CoreError> {
        Self::validate_extension_name(name)?;

        let sql = Self::generate_unload_sql(name);
        tracing::info!("[ExtensionManager] 卸载扩展 SQL: {}", sql);

        conn.execute_batch(&sql)
            .map_err(|e| CoreError::common(CommonError::General(format!("卸载扩展失败: {}", e))))?;

        // 更新缓存状态
        self.update_status(name, ExtensionStatus::Installed);

        tracing::info!("[ExtensionManager] 扩展已卸载: {}", name);

        Ok(())
    }

    /// 生成查询扩展状态的 SQL 语句。
    ///
    /// # 返回
    /// 查询扩展状态的 SQL
    pub fn generate_discover_sql() -> String {
        "SELECT * FROM duckdb_extensions()".to_string()
    }

    /// 生成安装扩展的 SQL 语句。
    ///
    /// # 参数
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// INSTALL SQL
    pub fn generate_install_sql(name: &str) -> String {
        format!("INSTALL {}", name)
    }

    /// 生成加载扩展的 SQL 语句。
    ///
    /// # 参数
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// LOAD SQL
    pub fn generate_load_sql(name: &str) -> String {
        format!("LOAD {}", name)
    }

    /// 生成卸载扩展的 SQL 语句。
    ///
    /// # 参数
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// UNLOAD SQL
    pub fn generate_unload_sql(name: &str) -> String {
        format!("UNLOAD {}", name)
    }

    /// 注册扩展信息。
    ///
    /// # 参数
    /// - `info`: 扩展信息
    pub fn register_extension(&self, info: ExtensionInfo) {
        if let Ok(mut extensions) = self.extensions.lock() {
            extensions.insert(info.name.clone(), info);
        }
    }

    /// 获取扩展信息。
    ///
    /// # 参数
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// 扩展信息（如果存在）
    pub fn get_extension(&self, name: &str) -> Option<ExtensionInfo> {
        self.extensions
            .lock()
            .ok()
            .and_then(|exts| exts.get(name).cloned())
    }

    /// 获取所有扩展信息。
    ///
    /// # 返回
    /// 扩展信息列表
    pub fn list_extensions(&self) -> Vec<ExtensionInfo> {
        self.extensions
            .lock()
            .map(|exts| exts.values().cloned().collect())
            .unwrap_or_default()
    }

    /// 更新扩展状态。
    ///
    /// # 参数
    /// - `name`: 扩展名称
    /// - `status`: 新状态
    ///
    /// # 返回
    /// true 表示扩展存在并更新成功
    pub fn update_status(&self, name: &str, status: ExtensionStatus) -> bool {
        if let Ok(mut extensions) = self.extensions.lock() {
            if let Some(info) = extensions.get_mut(name) {
                info.status = status;
                return true;
            }
        }
        false
    }

    /// 获取扩展目录路径。
    ///
    /// # 返回
    /// 扩展目录路径
    pub fn extension_dir(&self) -> &str {
        &self.extension_dir
    }

    /// 检查扩展是否已加载。
    ///
    /// # 参数
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// true 表示已加载
    pub fn is_loaded(&self, name: &str) -> bool {
        self.get_extension(name)
            .map(|info| info.status == ExtensionStatus::Loaded)
            .unwrap_or(false)
    }

    /// 获取内置扩展列表。
    ///
    /// # 返回
    /// 内置扩展名称列表
    pub fn builtin_extensions() -> Vec<&'static str> {
        vec!["parquet", "json"]
    }

    /// 获取常用按需扩展列表。
    ///
    /// # 返回
    /// 按需扩展名称列表
    pub fn on_demand_extensions() -> Vec<&'static str> {
        vec![
            "spatial",
            "excel",
            "httpfs",
            "fts",
            "mysql",
            "postgres_scanner",
        ]
    }

    /// 按需安装并加载扩展（自动安装加载）。
    ///
    /// # 参数
    /// - `conn`: DuckDB 连接
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// - `Ok(())`: 安装并加载成功
    /// - `Err(CoreError)`: 失败
    pub fn ensure_installed_and_loaded(
        &self,
        conn: &Connection,
        name: &str,
    ) -> Result<(), CoreError> {
        Self::validate_extension_name(name)?;

        // 检查当前状态
        let extensions = self.discover_extensions(conn)?;
        let existing = extensions.iter().find(|e| e.name == name);

        match existing {
            Some(info) => match info.status {
                ExtensionStatus::NotInstalled => {
                    self.install_extension(conn, name)?;
                    self.load_extension(conn, name)?;
                }
                ExtensionStatus::Installed => {
                    self.load_extension(conn, name)?;
                }
                ExtensionStatus::Loaded => {
                    tracing::info!("[ExtensionManager] 扩展已加载: {}", name);
                }
            },
            None => {
                tracing::warn!("[ExtensionManager] 扩展 '{}' 不在列表中，尝试安装", name);
                self.install_extension(conn, name)?;
                self.load_extension(conn, name)?;
            }
        }

        Ok(())
    }

    /// 批量确保多个扩展已安装并加载。
    pub fn ensure_batch_installed(
        &self,
        conn: &Connection,
        names: &[&str],
    ) -> Result<Vec<String>, CoreError> {
        let mut success = Vec::new();
        for &name in names {
            self.ensure_installed_and_loaded(conn, name)?;
            success.push(name.to_string());
        }
        Ok(success)
    }

    /// 生成设置扩展目录的 SQL 语句。
    ///
    /// # 返回
    /// SET extension_directory SQL
    pub fn generate_set_extension_dir_sql(&self) -> String {
        format!("SET extension_directory = '{}'", self.extension_dir)
    }

    /// 验证扩展名称是否合法。
    ///
    /// # 参数
    /// - `name`: 扩展名称
    ///
    /// # 返回
    /// - `Ok(())`: 名称合法
    /// - `Err(CoreError)`: 名称不合法
    pub fn validate_extension_name(name: &str) -> Result<(), CoreError> {
        if name.trim().is_empty() {
            return Err(CoreError::common(CommonError::General(
                "扩展名称不能为空".to_string(),
            )));
        }

        // 扩展名称只能包含字母、数字、下划线
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(CoreError::common(CommonError::General(format!(
                "扩展名称包含非法字符: {}",
                name
            ))));
        }

        Ok(())
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_status_display() {
        assert_eq!(format!("{}", ExtensionStatus::NotInstalled), "未安装");
        assert_eq!(format!("{}", ExtensionStatus::Installed), "已安装");
        assert_eq!(format!("{}", ExtensionStatus::Loaded), "已加载");
    }

    #[test]
    fn test_extension_info_sql() {
        let info = ExtensionInfo::new("httpfs", ExtensionStatus::Installed, true);

        assert_eq!(info.install_sql(), "INSTALL httpfs");
        assert_eq!(info.load_sql(), "LOAD httpfs");
        assert_eq!(info.unload_sql(), "UNLOAD httpfs");
    }

    #[test]
    fn test_register_and_get_extension() -> Result<(), CoreError> {
        let manager = ExtensionManager::new("/tmp/extensions");

        let info = ExtensionInfo::new("httpfs", ExtensionStatus::Loaded, true);
        manager.register_extension(info);

        let retrieved = manager.get_extension("httpfs");
        assert!(retrieved.is_some());
        let ext = retrieved
            .ok_or_else(|| CoreError::common(CommonError::General("扩展不存在".to_string())))?;
        assert_eq!(ext.name, "httpfs");
        Ok(())
    }

    #[test]
    fn test_list_extensions() {
        let manager = ExtensionManager::new("/tmp/extensions");

        manager.register_extension(ExtensionInfo::new("parquet", ExtensionStatus::Loaded, true));
        manager.register_extension(ExtensionInfo::new("json", ExtensionStatus::Loaded, true));

        let extensions = manager.list_extensions();
        assert_eq!(extensions.len(), 2);
    }

    #[test]
    fn test_update_status() {
        let manager = ExtensionManager::new("/tmp/extensions");

        manager.register_extension(ExtensionInfo::new(
            "httpfs",
            ExtensionStatus::NotInstalled,
            true,
        ));

        assert!(manager.update_status("httpfs", ExtensionStatus::Installed));
        assert!(manager.update_status("httpfs", ExtensionStatus::Loaded));

        assert!(manager.is_loaded("httpfs"));
    }

    #[test]
    fn test_update_non_existent_extension() {
        let manager = ExtensionManager::new("/tmp/extensions");
        assert!(!manager.update_status("non-existent", ExtensionStatus::Loaded));
    }

    #[test]
    fn test_builtin_extensions() {
        let builtins = ExtensionManager::builtin_extensions();
        assert!(builtins.contains(&"parquet"));
        assert!(builtins.contains(&"json"));
    }

    #[test]
    fn test_on_demand_extensions() {
        let on_demand = ExtensionManager::on_demand_extensions();
        assert!(on_demand.contains(&"spatial"));
        assert!(on_demand.contains(&"httpfs"));
        assert!(on_demand.contains(&"fts"));
    }

    #[test]
    fn test_generate_discover_sql() {
        let sql = ExtensionManager::generate_discover_sql();
        assert_eq!(sql, "SELECT * FROM duckdb_extensions()");
    }

    #[test]
    fn test_generate_set_extension_dir_sql() {
        let manager = ExtensionManager::new("/path/to/extensions");
        let sql = manager.generate_set_extension_dir_sql();
        assert_eq!(sql, "SET extension_directory = '/path/to/extensions'");
    }

    #[test]
    fn test_validate_extension_name_valid() {
        assert!(ExtensionManager::validate_extension_name("httpfs").is_ok());
        assert!(ExtensionManager::validate_extension_name("postgres_scanner").is_ok());
    }

    #[test]
    fn test_validate_extension_name_invalid() {
        assert!(ExtensionManager::validate_extension_name("").is_err());
        assert!(ExtensionManager::validate_extension_name("http-fs").is_err());
        assert!(ExtensionManager::validate_extension_name("http.fs").is_err());
    }
}
