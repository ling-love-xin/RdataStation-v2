//! 插件包安装器
//!
//! 支持从 .zip、.tar.gz 等压缩包安装插件到系统中

use crate::core::error::{CommonError, CoreError};
use crate::core::persistence::global_db::GlobalDatabaseManager;
use crate::core::persistence::plugin_store::Plugin;
use crate::core::plugin::loader::get_plugin_loader;
use crate::core::plugin::manifest::{ManifestParser, PluginManifest};
use std::path::{Path, PathBuf};

/// 插件包格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginPackageFormat {
    Zip,
    TarGz,
    Directory,
}

/// 插件包安装器
pub struct PluginInstaller {
    install_dir: PathBuf,
}

impl PluginInstaller {
    /// 创建新的安装器
    pub fn new(install_dir: PathBuf) -> Self {
        Self { install_dir }
    }

    /// 从文件路径检测插件包格式
    pub fn detect_format(path: &Path) -> Option<PluginPackageFormat> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "zip" => Some(PluginPackageFormat::Zip),
            "gz" => {
                if let Some(stem) = path.file_stem() {
                    let stem_path = Path::new(stem);
                    if stem_path.extension().and_then(|e| e.to_str()) == Some("tar") {
                        return Some(PluginPackageFormat::TarGz);
                    }
                }
                None
            }
            _ => {
                if path.is_dir() {
                    Some(PluginPackageFormat::Directory)
                } else {
                    None
                }
            }
        }
    }

    /// 安装插件包
    pub async fn install_package(
        &self,
        package_path: &Path,
        db_manager: &'static GlobalDatabaseManager,
    ) -> Result<Plugin, CoreError> {
        // 1. 检测格式
        let format = Self::detect_format(package_path).ok_or_else(|| {
            CoreError::common(CommonError::general(format!(
                "Unsupported plugin package format: {}",
                package_path.display()
            )))
        })?;

        // 2. 解压或直接使用
        let extracted_dir = match format {
            PluginPackageFormat::Zip => self.extract_zip(package_path)?,
            PluginPackageFormat::TarGz => self.extract_tar_gz(package_path)?,
            PluginPackageFormat::Directory => package_path.to_path_buf(),
        };

        // 3. 验证插件
        let manifest = self.validate_plugin(&extracted_dir)?;

        // 4. 复制到最终安装位置
        let final_install_path = self.install_dir.join(&manifest.plugin.id);
        self.copy_plugin(&extracted_dir, &final_install_path)?;

        // 5. 注册到数据库
        let plugin = self
            .register_plugin(&manifest, &final_install_path, db_manager)
            .await?;

        // 6. 清理临时解压目录（如果是从压缩包解压的）
        if format != PluginPackageFormat::Directory {
            let _ = std::fs::remove_dir_all(&extracted_dir);
        }

        // 7. 加载插件
        let loader = get_plugin_loader()?;
        let _ = loader.load_plugin_from_dir(&final_install_path).await;

        Ok(plugin)
    }

    /// 解压 ZIP 包（占位实现 - 需要添加 zip crate 依赖）
    fn extract_zip(&self, zip_path: &Path) -> Result<PathBuf, CoreError> {
        let _ = zip_path;
        Err(CoreError::common(CommonError::general(
            "ZIP extraction is not yet available (zip crate not included)".to_string(),
        )))
    }

    /// 解压 TAR.GZ 包（占位实现 - 需要添加 tar 和 flate2 crate 依赖）
    fn extract_tar_gz(&self, tar_path: &Path) -> Result<PathBuf, CoreError> {
        let _ = tar_path;
        Err(CoreError::common(CommonError::general(
            "TAR.GZ extraction is not yet available (tar/flate2 crates not included)".to_string(),
        )))
    }

    /// 验证插件
    fn validate_plugin(&self, plugin_dir: &Path) -> Result<PluginManifest, CoreError> {
        let manifest_path = plugin_dir.join("plugin.toml");
        if !manifest_path.exists() {
            return Err(CoreError::common(CommonError::general(
                "plugin.toml not found in plugin package".to_string(),
            )));
        }

        let manifest = ManifestParser::parse(&manifest_path)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// 复制插件到安装目录
    fn copy_plugin(&self, source_dir: &Path, target_dir: &Path) -> Result<(), CoreError> {
        if target_dir.exists() {
            let _ = std::fs::remove_dir_all(target_dir);
        }

        self.copy_dir(source_dir, target_dir)?;

        Ok(())
    }

    /// 递归复制目录
    fn copy_dir(&self, source: &Path, dest: &Path) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(dest)?;

        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let dest_path = dest.join(entry.file_name());

            if file_type.is_dir() {
                self.copy_dir(&entry.path(), &dest_path)?;
            } else {
                std::fs::copy(entry.path(), dest_path)?;
            }
        }

        Ok(())
    }

    /// 注册插件到数据库
    async fn register_plugin(
        &self,
        manifest: &PluginManifest,
        install_path: &Path,
        db_manager: &'static GlobalDatabaseManager,
    ) -> Result<Plugin, CoreError> {
        let now = chrono::Utc::now().to_rfc3339();
        let plugin = Plugin {
            id: uuid::Uuid::new_v4().to_string(),
            code: manifest.plugin.id.clone(),
            name: manifest.plugin.name.clone(),
            version: manifest.plugin.version.clone(),
            author: Some(manifest.plugin.publisher.clone()),
            description: if manifest.plugin.description.is_empty() {
                None
            } else {
                Some(manifest.plugin.description.clone())
            },
            repo_url: None,
            plugin_type: if manifest.capabilities.wasm.is_some() {
                "wasm".to_string()
            } else if manifest.capabilities.frontend.is_some() {
                "frontend".to_string()
            } else {
                "unknown".to_string()
            },
            manifest_json: Some(serde_json::to_string(manifest)?),
            install_path: install_path.to_string_lossy().to_string(),
            is_enabled: true,
            is_builtin: false,
            installed_at: now.clone(),
            updated_at: now,
        };

        db_manager.register_plugin(&plugin).await?;

        Ok(plugin)
    }
}

// 为了避免编译错误，我们暂时不导出 Installer，只在内部使用
// 实际项目中需要添加 zip 和 flate2、tar 等依赖
