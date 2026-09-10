use crate::core::error::CoreError;
use crate::core::persistence::driver_store::{DataSourceType, Driver, DriverFile};
use crate::core::persistence::global_db::GlobalDatabaseManager;
use crate::core::persistence::project_connection_store::ProjectConnectionStore;
use specta::Type;

/// 驱动管理服务，提供驱动发现、安装验证、项目可用性检查
pub struct DriverService {
    global_db: &'static GlobalDatabaseManager,
}

/// 驱动可用性状态枚举
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status")]
pub enum DriverAvailability {
    /// 驱动已就绪，可以创建连接
    #[serde(rename = "ready")]
    Ready,
    /// 驱动在项目中已启用但本机未安装文件
    #[serde(rename = "not_installed")]
    NotInstalled { download_url: String },
    /// 驱动未在项目中启用
    #[serde(rename = "not_enabled")]
    NotEnabled,
    /// 驱动定义不存在
    #[serde(rename = "not_defined")]
    NotDefined,
}

/// 项目打开时检测到的缺失驱动信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Type)]
pub struct MissingDriver {
    pub driver_id: String,
    pub driver_name: String,
    pub download_url: String,
}

impl DriverService {
    /// 创建驱动服务实例
    pub fn new(global_db: &'static GlobalDatabaseManager) -> Self {
        Self { global_db }
    }

    /// 获取所有可用驱动的定义列表
    pub async fn get_available_drivers(&self) -> Result<Vec<Driver>, CoreError> {
        self.global_db.get_all_drivers().await
    }

    /// 获取数据源类型目录
    pub async fn get_data_source_types(&self) -> Result<Vec<DataSourceType>, CoreError> {
        self.global_db.list_data_source_types().await
    }

    /// 获取指定驱动在本机已安装的文件版本列表
    pub async fn list_driver_files(&self, driver_id: &str) -> Result<Vec<DriverFile>, CoreError> {
        self.global_db.list_driver_files(driver_id).await
    }

    /// 安装外部驱动文件（当前为占位实现，需后续完成下载+校验逻辑）
    pub async fn install_driver(&self, _driver_id: &str) -> Result<(), CoreError> {
        Err(CoreError::from(
            "驱动安装功能将在后续版本中开放。用户需手动下载 JDBC jar 文件并放入应用驱动目录。"
                .to_string(),
        ))
    }

    /// 检查指定驱动对某个项目是否可用
    pub fn check_driver_for_project(
        &self,
        driver_id: &str,
        project_store: &ProjectConnectionStore,
    ) -> Result<DriverAvailability, CoreError> {
        let rt = tokio::runtime::Handle::current();

        let enabled = rt.block_on(project_store.is_driver_enabled(driver_id))?;
        if !enabled {
            return Ok(DriverAvailability::NotEnabled);
        }

        let drivers = rt.block_on(self.global_db.get_all_drivers())?;
        let driver = drivers.iter().find(|d| d.id == driver_id);
        let driver = match driver {
            Some(d) => d,
            None => return Ok(DriverAvailability::NotDefined),
        };

        if driver.driver_kind == "native" {
            return Ok(DriverAvailability::Ready);
        }

        let version = driver.version.as_deref().unwrap_or("0.0.0");
        let installed = rt.block_on(self.global_db.is_driver_installed(driver_id, version))?;
        if !installed {
            let download_url = driver.download_url.clone().unwrap_or_default();
            return Ok(DriverAvailability::NotInstalled { download_url });
        }

        Ok(DriverAvailability::Ready)
    }
}
