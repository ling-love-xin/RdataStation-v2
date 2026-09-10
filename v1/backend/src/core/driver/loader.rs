use std::path::Path;
use std::sync::Arc;

use crate::core::driver::DriverFactory;
use crate::core::error::CoreError;

/// 驱动加载器
///
/// 负责加载不同类型的驱动：
/// 1. 内置驱动
/// 2. WASM插件驱动
/// 3. JDBC驱动
pub struct DriverLoader {
    /// 内置驱动发现器
    builtin_discovery: BuiltinDriverDiscovery,
    /// WASM插件驱动发现器
    wasm_discovery: Option<WasmDriverDiscovery>,
    /// JDBC驱动发现器
    jdbc_discovery: Option<JdbcDriverDiscovery>,
}

impl DriverLoader {
    /// 创建新的驱动加载器
    pub fn new() -> Self {
        Self {
            builtin_discovery: BuiltinDriverDiscovery,
            wasm_discovery: Some(WasmDriverDiscovery::new()),
            jdbc_discovery: Some(JdbcDriverDiscovery::new()),
        }
    }

    /// 加载所有驱动
    pub async fn load_all_drivers(&self) -> Result<Vec<Arc<dyn DriverFactory>>, CoreError> {
        let mut drivers = Vec::new();

        // 加载内置驱动
        let builtin_drivers = self.load_builtin_drivers().await?;
        drivers.extend(builtin_drivers);

        // 加载WASM插件驱动
        if let Some(wasm_discovery) = &self.wasm_discovery {
            if let Ok(wasm_drivers) = wasm_discovery.load_drivers().await {
                drivers.extend(wasm_drivers);
            }
        }

        // 加载JDBC驱动
        if let Some(jdbc_discovery) = &self.jdbc_discovery {
            if let Ok(jdbc_drivers) = jdbc_discovery.load_drivers().await {
                drivers.extend(jdbc_drivers);
            }
        }

        Ok(drivers)
    }

    /// 加载内置驱动
    pub async fn load_builtin_drivers(&self) -> Result<Vec<Arc<dyn DriverFactory>>, CoreError> {
        self.builtin_discovery.load_drivers().await
    }

    /// 加载WASM插件驱动
    pub async fn load_wasm_drivers(&self) -> Result<Vec<Arc<dyn DriverFactory>>, CoreError> {
        if let Some(wasm_discovery) = &self.wasm_discovery {
            wasm_discovery.load_drivers().await
        } else {
            Ok(Vec::new())
        }
    }

    /// 加载JDBC驱动
    pub async fn load_jdbc_drivers(&self) -> Result<Vec<Arc<dyn DriverFactory>>, CoreError> {
        if let Some(jdbc_discovery) = &self.jdbc_discovery {
            jdbc_discovery.load_drivers().await
        } else {
            Ok(Vec::new())
        }
    }
}

/// 内置驱动发现器
///
/// 这是所有内置驱动的 **唯一真相源（Single Source of Truth）**。
/// 任何需要获取内置驱动列表的地方（Registry 注册、Manager 初始化、
/// Loader 发现）都必须通过此结构体的方法获取。
pub struct BuiltinDriverDiscovery;

impl BuiltinDriverDiscovery {
    /// 获取所有内置驱动的工厂实例（同步，无 I/O）
    ///
    /// 这是系统中唯一定义内置驱动列表的位置。
    /// 驱动 ID 与 Registry key / DB drivers.id 严格对齐：
    ///
    /// | Registry key    | 驱动实现               |
    /// |-----------------|-----------------------|
    /// | mysql           | sqlx::MySql           |
    /// | mysql_native    | mysql_async           |
    /// | postgres        | sqlx::Postgres        |
    /// | postgres_native | tokio-postgres        |
    /// | sqlite          | rusqlite              |
    /// | duckdb          | duckdb-rs             |
    pub fn builtin_factories() -> Vec<Arc<dyn DriverFactory>> {
        vec![
            Arc::new(crate::core::driver::factory::MySqlDriverFactory),
            Arc::new(crate::core::driver::factory::MySqlNativeDriverFactory),
            Arc::new(crate::core::driver::factory::PostgresDriverFactory),
            Arc::new(crate::core::driver::factory::PostgresNativeDriverFactory),
            Arc::new(crate::core::driver::factory::SqliteDriverFactory),
            Arc::new(crate::core::driver::factory::DuckDbDriverFactory),
        ]
    }

    /// 异步加载内置驱动（委托给 sync builtin_factories）
    pub async fn load_drivers(&self) -> Result<Vec<Arc<dyn DriverFactory>>, CoreError> {
        Ok(Self::builtin_factories())
    }
}

impl Default for DriverLoader {
    fn default() -> Self {
        Self::new()
    }
}

/// WASM插件驱动发现器
pub struct WasmDriverDiscovery {
    /// WASM插件目录
    plugin_dirs: Vec<String>,
}

impl WasmDriverDiscovery {
    /// 创建新的WASM驱动发现器
    pub fn new() -> Self {
        Self {
            plugin_dirs: vec![
                "./plugins".to_string(),
                "~/.rdatastation/plugins".to_string(),
            ],
        }
    }

    /// 加载WASM插件驱动
    pub async fn load_drivers(&self) -> Result<Vec<Arc<dyn DriverFactory>>, CoreError> {
        let drivers = Vec::new();

        // 扫描WASM插件目录
        for dir in &self.plugin_dirs {
            let path = Path::new(dir);
            if path.exists() && path.is_dir() {
                // 这里可以实现WASM插件的扫描和加载逻辑
                // 例如：查找.wasm文件并加载
            }
        }

        Ok(drivers)
    }
}

impl Default for WasmDriverDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// JDBC驱动发现器
pub struct JdbcDriverDiscovery {
    /// JDBC驱动目录
    driver_dirs: Vec<String>,
}

impl JdbcDriverDiscovery {
    /// 创建新的JDBC驱动发现器
    pub fn new() -> Self {
        Self {
            driver_dirs: vec![
                "./jdbc-drivers".to_string(),
                "~/.rdatastation/jdbc-drivers".to_string(),
            ],
        }
    }

    /// 加载JDBC驱动
    pub async fn load_drivers(&self) -> Result<Vec<Arc<dyn DriverFactory>>, CoreError> {
        let drivers = Vec::new();

        // 扫描JDBC驱动目录
        for dir in &self.driver_dirs {
            let path = Path::new(dir);
            if path.exists() && path.is_dir() {
                // 这里可以实现JDBC驱动的扫描和加载逻辑
                // 例如：查找.jar文件并加载
            }
        }

        Ok(drivers)
    }
}

impl Default for JdbcDriverDiscovery {
    fn default() -> Self {
        Self::new()
    }
}
