//! 插件包安装器
//!
//! 支持从 .zip、.tar.gz 等压缩包安装插件到系统中
//!
//! 2026-10-15：`extract_zip` / `extract_tar_gz` 从占位实现落地（依赖 zip / tar / flate2
//! 已入 workspace）；`plugin_type` 判定补上 P1 的 `[backend]` 段（sidecar/script 是子进程
//! 形态，此前会被标成 "unknown"）；拼安装路径前强制过 [`paths::validate_plugin_id`]
//! （README §5 硬约束 #6：id 由第三方清单给出）。

use std::path::{Path, PathBuf};

use engine::persistence::global_db::GlobalDatabaseManager;
use engine::persistence::plugin_store::Plugin;
use paths::validate_plugin_id;
use shared::error::{CommonError, CoreError};

use crate::manager::get_plugin_manager;
use crate::manifest::{BackendKind, ManifestParser, PluginManifest};

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

        // 4. 复制到最终安装位置（拼路径前必须校验清单 id —— 路径注入防护）
        validate_plugin_id(&manifest.plugin.id).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "插件 id 不合法（拼路径前必须校验）: {}",
                e
            )))
        })?;
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

        // 7. 加载插件（安装态；wasm / sidecar 运行时在激活时按需加载）
        if let Some(manager) = get_plugin_manager() {
            let _ = manager.load_plugin_from_dir(&final_install_path).await;
        }

        Ok(plugin)
    }

    /// 临时解压目录（用完即删；`paths::temp_dir()` 是 RDS 数据目录下的 temp，随版本走）
    fn temp_extract_dir(&self, package_path: &Path) -> Result<PathBuf, CoreError> {
        let name = package_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("plugin");
        let dest = paths::temp_dir().join(format!("rds-plugin-extract-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dest).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to create temp extract dir: {}",
                e
            )))
        })?;
        Ok(dest)
    }

    /// 解压 ZIP 包（zip 0.6；`enclosed_name()` 拒绝路径穿越 —— 压缩包条目可能带 `../`）
    fn extract_zip(&self, zip_path: &Path) -> Result<PathBuf, CoreError> {
        let file = std::fs::File::open(zip_path).map_err(|e| {
            CoreError::common(CommonError::general(format!(
                "Failed to open zip: {}",
                e
            )))
        })?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| {
            CoreError::common(CommonError::general(format!(
                "Failed to read zip archive: {}",
                e
            )))
        })?;

        let dest = self.temp_extract_dir(zip_path)?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| {
                CoreError::common(CommonError::general(format!(
                    "Failed to read zip entry: {}",
                    e
                )))
            })?;

            // zip 0.6 的 enclosed_name()：条目路径在解压目录内才返回 Some，
            // 含 `..` / 绝对路径 / 盘符前缀的条目返回 None —— 直接拒绝整个包。
            let out_path = entry.enclosed_name().ok_or_else(|| {
                CoreError::common(CommonError::general(
                    "压缩包内含有不安全路径的条目（路径穿越），已拒绝".to_string(),
                ))
            })?;
            let out_path = dest.join(out_path);

            if entry.is_dir() {
                std::fs::create_dir_all(&out_path).map_err(|e| {
                    CoreError::common(CommonError::general(format!(
                        "Failed to create dir: {}",
                        e
                    )))
                })?;
            } else {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        CoreError::common(CommonError::general(format!(
                            "Failed to create parent dir: {}",
                            e
                        )))
                    })?;
                }
                let mut out_file = std::fs::File::create(&out_path).map_err(|e| {
                    CoreError::common(CommonError::general(format!(
                        "Failed to create file: {}",
                        e
                    )))
                })?;
                std::io::copy(&mut entry, &mut out_file).map_err(|e| {
                    CoreError::common(CommonError::general(format!(
                        "Failed to extract entry: {}",
                        e
                    )))
                })?;
            }
        }

        Ok(dest)
    }

    /// 解压 TAR.GZ 包（flate2 + tar 0.4；`Archive::unpack` 自带路径穿越防护）
    fn extract_tar_gz(&self, tar_path: &Path) -> Result<PathBuf, CoreError> {
        let file = std::fs::File::open(tar_path).map_err(|e| {
            CoreError::common(CommonError::general(format!(
                "Failed to open tar.gz: {}",
                e
            )))
        })?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(gz);

        let dest = self.temp_extract_dir(tar_path)?;

        // tar 0.4 的 unpack 默认拒绝 `..` / 绝对路径 / 链接逃逸
        archive.unpack(&dest).map_err(|e| {
            let _ = std::fs::remove_dir_all(&dest);
            CoreError::common(CommonError::general(format!(
                "Failed to extract tar.gz: {}",
                e
            )))
        })?;

        Ok(dest)
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

        // 形态判定与 detect_plugin_kind 同口径：`[backend]` 优先（P1 契约），
        // 再 capabilities.wasm / frontend。此前漏看 backend，sidecar 插件被标成 "unknown"。
        let plugin_type = if manifest.capabilities.wasm.is_some()
            || matches!(
                manifest.backend.as_ref().map(|b| &b.kind),
                Some(BackendKind::Wasm)
            )
        {
            "wasm".to_string()
        } else if let Some(backend) = &manifest.backend {
            // sidecar / script 都是子进程形态（进进程池）
            match backend.kind {
                BackendKind::Sidecar | BackendKind::Script => "sidecar".to_string(),
                BackendKind::Wasm => "wasm".to_string(),
            }
        } else if manifest.capabilities.frontend.is_some() {
            "frontend".to_string()
        } else {
            "unknown".to_string()
        };

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
            plugin_type,
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
