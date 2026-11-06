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
    ///
    /// 目录优先判定：目录名常常不带扩展名（`plugins/my-plugin`），而
    /// `path.extension()?` 会在没有扩展名时提前返回 `None`，导致目录形态永远
    /// 走不到 `Directory` 分支。
    pub fn detect_format(path: &Path) -> Option<PluginPackageFormat> {
        if path.is_dir() {
            return Some(PluginPackageFormat::Directory);
        }
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
            _ => None,
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

        // 2.5 定位真正的包根：压缩工具（含本仓 `tools/zip-dir.ps1`）默认会给包套一层
        // 目录名，根层就直接找 `plugin.toml` 会把这类最常见的包全判成"清单缺失"。
        let package_root = Self::locate_package_root(&extracted_dir)?;

        // 3. 验证插件
        let manifest = self.validate_plugin(&package_root)?;

        // 4. 复制到最终安装位置（拼路径前必须校验清单 id —— 路径注入防护）
        validate_plugin_id(&manifest.plugin.id).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "插件 id 不合法（拼路径前必须校验）: {}",
                e
            )))
        })?;
        let final_install_path = self.install_dir.join(&manifest.plugin.id);

        // 覆盖安装（同 id 重装 / 升级）：先把运行中的插件停掉。
        // Windows 上进程（尤其 sidecar 的 agent.exe）占用文件会让目录删不干净，
        // 旧版本文件就会和新包混在一起（清单指向新入口、旧入口仍在）。
        // 记住原来是否在跑：换入 / 登记失败时要把老插件恢复运行，否则一次失败的
        // 覆盖安装会把「DB 里 enabled」的插件留成运行时停用，直到用户手动点启用。
        let mut was_active = false;
        if final_install_path.exists() {
            if let Some(manager) = get_plugin_manager() {
                if let Some(loaded) = manager.get_loaded_plugin(&manifest.plugin.id) {
                    was_active = loaded.status == crate::manager::LoadStatus::Active;
                    manager.deactivate_plugin(&manifest.plugin.id).await?;
                }
            }
        }

        let outcome: Result<Plugin, CoreError> = async {
            // 4.1 先复制到暂存目录，再整体换入：复制过程可能持续数秒，热插拔每 5s 扫一次
            // `plugins/`，直接往最终目录写会让扫描看到半成品（清单已到、入口还没到）。
            self.swap_dir_into_place(&package_root, &final_install_path)?;

            // 5. 注册到数据库
            let plugin = register_manifest(&manifest, &final_install_path, db_manager).await?;

            // 6. 清理临时解压目录（如果是从压缩包解压的）
            if format != PluginPackageFormat::Directory {
                let _ = std::fs::remove_dir_all(&extracted_dir);
            }

            // 7. 加载插件（安装态；wasm / sidecar 运行时在激活时按需加载）
            //
            // 加载失败不撤销安装（目录与登记都已落盘），但必须留日志：此前是 `let _ =`
            // 静默吞掉，用户只会看到「清单里没有这个插件」却没有任何原因。
            // 激活按库里的启用状态决定：装完只停在「已加载」时，sidecar 驱动没有注册，
            // 用户装完立刻建连接会连不上，且看不出原因。
            if let Some(manager) = get_plugin_manager() {
                match manager.load_plugin_from_dir(&final_install_path).await {
                    Ok(_) if plugin.is_enabled => {
                        if let Err(e) = manager.activate_plugin(&manifest.plugin.id).await {
                            tracing::warn!(
                                plugin = %manifest.plugin.id,
                                error = %e,
                                "插件安装完成但激活失败（目录与登记保留）"
                            );
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(
                            plugin = %manifest.plugin.id,
                            error = %e,
                            "插件安装完成但加载失败（目录与登记保留）"
                        );
                    }
                }
            }

            Ok(plugin)
        }
        .await;

        // 失败恢复：老插件本来就跑着，安装没成功就不该把它留在停用态。
        if outcome.is_err() && was_active {
            if let Some(manager) = get_plugin_manager() {
                match manager.load_plugin_from_dir(&final_install_path).await {
                    Ok(_) => {
                        if let Err(e) = manager.activate_plugin(&manifest.plugin.id).await {
                            tracing::warn!(
                                plugin = %manifest.plugin.id,
                                error = %e,
                                "覆盖安装失败后恢复旧插件运行失败"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            plugin = %manifest.plugin.id,
                            error = %e,
                            "覆盖安装失败后重新加载旧插件失败（目录可能已被替换）"
                        );
                    }
                }
            }
        }

        outcome
    }

    /// 临时解压目录（用完即删；`paths::temp_dir()` 是 RDS 数据目录下的 temp，随版本走）
    fn temp_extract_dir(&self, package_path: &Path) -> Result<PathBuf, CoreError> {
        let name = package_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("plugin");
        let dest = paths::temp_dir().join(format!(
            "rds-plugin-extract-{name}-{}",
            uuid::Uuid::new_v4()
        ));
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
            CoreError::common(CommonError::general(format!("Failed to open zip: {}", e)))
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
                    CoreError::common(CommonError::general(format!("Failed to create dir: {}", e)))
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

    /// 找到包根：根层有 `plugin.toml` 就用它；否则只认**唯一**一个含清单的直接子目录。
    ///
    /// 压缩工具默认会把目录名带进条目（`my-plugin/plugin.toml`），本仓的
    /// `tools/zip-dir.ps1` 也如此。只认根层会让这种最常见的包全部装不上；
    /// 但也不能随便下探——多层目录里挑一个等于替用户猜"要装哪个"。
    fn locate_package_root(dir: &Path) -> Result<PathBuf, CoreError> {
        if dir.join("plugin.toml").is_file() {
            return Ok(dir.to_path_buf());
        }

        let mut candidates: Vec<PathBuf> = Vec::new();
        let entries = std::fs::read_dir(dir).map_err(|e| {
            CoreError::common(CommonError::general(format!(
                "Failed to read package dir: {}",
                e
            )))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                CoreError::common(CommonError::general(format!(
                    "Failed to read package entry: {}",
                    e
                )))
            })?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // macOS 打 zip 附带的资源目录，不是插件
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("__MACOSX")
            {
                continue;
            }
            if path.join("plugin.toml").is_file() {
                candidates.push(path);
            }
        }

        match candidates.as_slice() {
            [single] => Ok(single.clone()),
            [] => Err(CoreError::common(CommonError::general(
                "plugin.toml not found in plugin package（应在包根层，或只带一层插件目录）",
            ))),
            _ => Err(CoreError::common(CommonError::general(format!(
                "插件包里有多个候选插件目录（{} 个），无法确定安装哪一个",
                candidates.len()
            )))),
        }
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
        // 安装期就检查声明的入口是否在包里：否则「装成功、激活时才报错」，
        // 与「装前给可读原因」的口径不一致（目录约定见 plugin-layout.md）。
        crate::layout::check_declared_entries(&manifest, plugin_dir)?;
        // 随包完整性：有 checksums.json 就逐文件比对（缺失视为老包 / 开发目录，放行）。
        crate::checksums::verify_package(&manifest, plugin_dir)?;
        // 声明入口不许落在安装时会跳过的开发残留目录里：那份配置能通过上面两步，
        // 却会在复制时被静默丢掉，最后装出一个「清单指着不存在的文件」的包。
        reject_residue_entries(&manifest, plugin_dir)?;
        Ok(manifest)
    }

    /// 复制插件到安装目录
    fn copy_plugin(&self, source_dir: &Path, target_dir: &Path) -> Result<(), CoreError> {
        if target_dir.exists() {
            // 删除失败不能吞：继续复制会把新包叠在旧包上，留下清单里已经删掉的
            // 旧入口 / 旧 jar（校验只看源包，查不出来）。
            std::fs::remove_dir_all(target_dir).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "覆盖安装前删除旧目录失败（插件进程可能仍在占用文件）: {} — {e}",
                    target_dir.display()
                )))
            })?;
        }

        self.copy_dir(source_dir, target_dir)?;

        Ok(())
    }

    /// 复制到暂存目录后整体换入最终位置。
    ///
    /// 直接往 `plugins/<id>/` 里复制会有一个可见窗口：热插拔每 5s 扫一次目录，只要
    /// 扫描落在「清单已复制、入口文件还没到」的瞬间，就会加载一个残缺插件（要么失败，
    /// 要么登记成半成品）。暂存目录放在 `paths::temp_dir()`（与 `plugins/` 同属数据根，
    /// `rename` 不跨卷），换入只在「删旧 → 改名」两瞬间完成。
    fn swap_dir_into_place(
        &self,
        source_dir: &Path,
        final_install_path: &Path,
    ) -> Result<(), CoreError> {
        let name = final_install_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("plugin");
        let staging =
            paths::temp_dir().join(format!("rds-plugin-stage-{name}-{}", uuid::Uuid::new_v4()));
        if staging.exists() {
            let _ = std::fs::remove_dir_all(&staging);
        }
        self.copy_plugin(source_dir, &staging).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            CoreError::common(CommonError::General(format!(
                "复制插件到暂存目录失败: {} — {e}",
                staging.display()
            )))
        })?;

        let outcome = (|| -> Result<(), CoreError> {
            if final_install_path.exists() {
                std::fs::remove_dir_all(final_install_path).map_err(|e| {
                    CoreError::common(CommonError::General(format!(
                        "覆盖安装前删除旧目录失败（插件进程可能仍在占用文件）: {} — {e}",
                        final_install_path.display()
                    )))
                })?;
            }
            match std::fs::rename(&staging, final_install_path) {
                Ok(()) => Ok(()),
                Err(rename_err) => {
                    // 极端情况（不同卷 / 目标被占用）回退为逐文件复制；失败时把半成品
                    // 目录删掉，避免留下一个「看起来装好了」的残缺插件。
                    self.copy_dir(&staging, final_install_path).map_err(|e| {
                        let _ = std::fs::remove_dir_all(final_install_path);
                        CoreError::common(CommonError::General(format!(
                            "换入插件目录失败（暂存: {}，目标: {}）: {rename_err} / {e}",
                            staging.display(),
                            final_install_path.display()
                        )))
                    })?;
                    Ok(())
                }
            }
        })();

        let _ = std::fs::remove_dir_all(&staging);
        outcome
    }

    /// 递归复制目录（跳过开发残留：`.git` / `target` / `node_modules` 等，见
    /// `layout::DEV_RESIDUE_DIRS`）。清单声明的入口若落在这些目录里，`validate_plugin`
    /// 已经先拒了，这里只负责不把仓库垃圾带进安装目录。
    fn copy_dir(&self, source: &Path, dest: &Path) -> Result<(), std::io::Error> {
        std::fs::create_dir_all(dest)?;

        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                // 非 UTF-8 文件名不在白名单的判断范围内：照原样复制，不猜。
                let dest_path = dest.join(&name);
                if file_type.is_dir() {
                    self.copy_dir(&entry.path(), &dest_path)?;
                } else {
                    std::fs::copy(entry.path(), dest_path)?;
                }
                continue;
            };
            if crate::layout::is_dev_residue(name, file_type.is_dir()) {
                continue;
            }

            let dest_path = dest.join(name);
            if file_type.is_dir() {
                self.copy_dir(&entry.path(), &dest_path)?;
            } else {
                std::fs::copy(entry.path(), dest_path)?;
            }
        }

        Ok(())
    }
}

/// 注册插件到数据库（按 code 合并；返回落库后的实例）。
///
/// 收成自由函数是因为热插拔扫描也要用：目录直接拷进 `plugins/` 的插件如果只加载
/// 不登记，停用/卸载会写 0 行，重启后又按「DB 无记录 = 启用」复活。
pub(crate) async fn register_manifest(
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
        ) {
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

    // 库层会沿用同 code 旧行的启用状态；返回调用方的实例必须反映落库结果，
    // 否则重装一个已停用的插件会被当场当成启用并激活。
    db_manager
        .get_plugin_by_code(&manifest.plugin.id)
        .await?
        .ok_or_else(|| {
            CoreError::common(CommonError::General(format!(
                "插件登记后读不到记录: {}",
                manifest.plugin.id
            )))
        })
}

/// 宿主「安装 / 导入插件包」的唯一入口。
///
/// `install_package` 要求调用方自己给安装目录与 `'static` 的全局库句柄——那是插件侧的
/// 存储约定，宿主不该复制一份（尤其是 `plugins_dir()` 这种目录布局规则）。这里把
/// 「来源检查 + 目录 + 库 + 安装器」一次收拢，宿主只交一个用户选中的路径。
pub async fn install_from_path(package_path: &Path) -> Result<Plugin, CoreError> {
    let install_root = paths::plugins_dir();
    prepare_install(package_path, &install_root)?;

    let db_manager = engine::migration::get_global_db_manager().ok_or_else(|| {
        CoreError::common(CommonError::general(
            "全局数据库未初始化，无法安装插件（宿主启动流程未完成）".to_string(),
        ))
    })?;

    PluginInstaller::new(install_root)
        .install_package(package_path, db_manager)
        .await
}

/// 安装前的来源检查：路径存在、格式可识别、且不是某个插件的安装目录。
///
/// 最后一条是安全底线——`copy_plugin` 在目标已存在时会先 `remove_dir_all`，若用户
/// 选中的正是 `plugins/<id>` 本身，会先把源目录删掉再复制，装完后目录为空。
/// 无副作用、不碰全局状态，便于单测。
pub fn prepare_install(
    package_path: &Path,
    install_root: &Path,
) -> Result<PluginPackageFormat, CoreError> {
    if !package_path.exists() {
        return Err(CoreError::common(CommonError::general(format!(
            "插件包路径不存在: {}",
            package_path.display()
        ))));
    }

    let format = PluginInstaller::detect_format(package_path).ok_or_else(|| {
        CoreError::common(CommonError::general(format!(
            "不支持的插件包格式（支持 .zip / .tar.gz，或含 plugin.toml 的插件目录）: {}",
            package_path.display()
        )))
    })?;

    if let (Ok(source), Ok(root)) = (package_path.canonicalize(), install_root.canonicalize()) {
        if source.starts_with(&root) {
            return Err(CoreError::common(CommonError::general(format!(
                "选中的路径已是插件安装目录，不能作为导入源: {}",
                package_path.display()
            ))));
        }
    }

    Ok(format)
}

/// 声明的入口落在开发残留目录里就拒绝安装（否则复制时会被跳过，装出一个坏包）。
///
/// 复用 `checksums::required_entries`：覆盖率口径与"安装时真正需要的文件"是同一条，
/// 这里不必再列一遍 sidecar / script / wasm / frontend 的差别。
fn reject_residue_entries(manifest: &PluginManifest, plugin_dir: &Path) -> Result<(), CoreError> {
    let required = crate::checksums::required_entries(manifest, plugin_dir)?;
    let offenders: Vec<&str> = required
        .iter()
        .filter(|key| crate::layout::is_residue_key(key))
        .map(String::as_str)
        .collect();
    if offenders.is_empty() {
        return Ok(());
    }
    Err(CoreError::common(CommonError::General(format!(
        "插件入口落在安装时跳过的开发残留目录（不会被安装）：\n - {}",
        offenders.join("\n - ")
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksums::CHECKSUMS_FILE;
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// 一次性临时目录（Drop 清理；测试不写仓库目录）
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "rds-plugin-installer-{tag}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(dir: &Path, relative: &str, body: &str) {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    const MANIFEST: &str = r#"
[plugin]
id = "com.example.installer"
name = "Installer"
version = "1.0.0"
publisher = "test"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
executable = "bin/agent"
"#;

    fn sha256(dir: &Path, relative: &str) -> String {
        let bytes = std::fs::read(dir.join(relative)).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        hex::encode(hasher.finalize())
    }

    /// `install_package` 的顺序是「解包 → `validate_plugin` → 复制目录」；这里直接测
    /// 那一步校验本身：坏哈希的包在校验期就被拒，走不到复制与落库。
    #[test]
    fn validate_plugin_rejects_a_tampered_package() {
        let dir = TempDir::new("tampered");
        write(dir.path(), "plugin.toml", MANIFEST);
        write(dir.path(), "bin/agent", "original-binary");

        let checksums = serde_json::json!({
            "algorithm": "sha256",
            "files": {
                "plugin.toml": sha256(dir.path(), "plugin.toml"),
                "bin/agent": sha256(dir.path(), "bin/agent"),
            }
        })
        .to_string();
        write(dir.path(), CHECKSUMS_FILE, &checksums);

        let installer = PluginInstaller::new(dir.path().join("install"));
        installer.validate_plugin(dir.path()).unwrap();

        // 装包被篡改：哈希对不上就拒绝
        write(dir.path(), "bin/agent", "tampered-binary");
        let err = installer.validate_plugin(dir.path()).unwrap_err();
        let text = format!("{err}");
        assert!(
            text.contains("哈希不匹配") && text.contains("bin/agent"),
            "{text}"
        );
    }

    const MANIFEST_IN_RESIDUE: &str = r#"
[plugin]
id = "com.example.residue"
name = "Residue"
version = "1.0.0"
publisher = "test"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
executable = "target/agent"
"#;

    /// 入口落在 `target/` 这类会被跳过的目录时，必须在安装前拒绝，而不是
    /// 「校验通过 → 复制时丢掉 → 装出一个清单指着空文件的包」。
    #[test]
    fn declared_entry_in_dev_residue_is_rejected() {
        let dir = TempDir::new("residue-entry");
        write(dir.path(), "plugin.toml", MANIFEST_IN_RESIDUE);
        write(dir.path(), "target/agent", "binary");

        let installer = PluginInstaller::new(dir.path().join("install"));
        let err = installer.validate_plugin(dir.path()).unwrap_err();
        let text = format!("{err}");
        assert!(
            text.contains("开发残留") && text.contains("target/agent"),
            "{text}"
        );
    }

    /// 复制目录时不带仓库垃圾：`.git` / `target` / `node_modules` 跳过，`src/` 照常装。
    #[test]
    fn copy_plugin_skips_dev_residue() {
        let dir = TempDir::new("copy");
        let source = dir.path().join("package");
        write(&source, "plugin.toml", MANIFEST);
        write(&source, "bin/agent", "binary");
        write(&source, "src/main.py", "print('hi')");
        write(&source, "target/debug/junk.o", "obj");
        write(&source, ".git/HEAD", "ref: refs/heads/main");
        write(&source, "node_modules/dep/index.js", "// dep");
        write(&source, ".DS_Store", "junk");

        let install = dir.path().join("install");
        let installer = PluginInstaller::new(install.clone());
        installer.copy_plugin(&source, &install).unwrap();

        assert!(install.join("plugin.toml").is_file());
        assert!(install.join("bin/agent").is_file());
        assert!(install.join("src/main.py").is_file(), "src/ 属于包内容");
        assert!(!install.join("target").exists());
        assert!(!install.join(".git").exists());
        assert!(!install.join("node_modules").exists());
        assert!(!install.join(".DS_Store").exists());
    }

    /// 覆盖安装必须整目录替换：删除旧目录失败被吞掉再复制，会把旧版本独有
    /// 的文件留在新包里（校验只看源包，查不出来）。
    #[test]
    fn overwrite_install_drops_files_the_new_package_no_longer_has() {
        let dir = TempDir::new("copy-overwrite");
        let source = dir.path().join("package");
        write(&source, "plugin.toml", MANIFEST);
        write(&source, "bin/agent", "v2-binary");

        let install = dir.path().join("install");
        write(&install, "plugin.toml", MANIFEST);
        write(&install, "bin/agent", "v1-binary");
        write(&install, "bin/legacy-helper.exe", "old");

        let installer = PluginInstaller::new(install.clone());
        installer.copy_plugin(&source, &install).unwrap();

        assert_eq!(
            std::fs::read_to_string(install.join("bin/agent")).unwrap(),
            "v2-binary"
        );
        assert!(
            !install.join("bin/legacy-helper.exe").exists(),
            "旧包独有的文件必须清掉"
        );
    }

    /// 安装走「暂存目录 → 整体换入」：换入后旧文件不残留，暂存目录也不留垃圾。
    /// 直接往最终目录复制会让热插拔扫描看见半成品（清单已到、入口还没到）。
    #[test]
    fn swap_dir_into_place_replaces_the_old_tree_and_leaves_no_staging() {
        let dir = TempDir::new("swap-in-place");
        let source = dir.path().join("package");
        write(&source, "plugin.toml", MANIFEST);
        write(&source, "bin/agent", "v2-binary");

        let install = dir.path().join("install");
        write(&install, "plugin.toml", MANIFEST);
        write(&install, "bin/agent", "v1-binary");
        write(&install, "bin/legacy-helper.exe", "old");

        let installer = PluginInstaller::new(install.clone());
        installer.swap_dir_into_place(&source, &install).unwrap();

        assert_eq!(
            std::fs::read_to_string(install.join("bin/agent")).unwrap(),
            "v2-binary"
        );
        assert!(
            !install.join("bin/legacy-helper.exe").exists(),
            "换入后旧包独有的文件必须清掉"
        );

        let temp = paths::temp_dir();
        if let Ok(entries) = std::fs::read_dir(&temp) {
            let leftovers: Vec<_> = entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("rds-plugin-stage-install-")
                })
                .collect();
            assert!(leftovers.is_empty(), "暂存目录必须清理: {leftovers:?}");
        }
    }

    /// 目录形态的插件包（开发目录 / 解压后的包）应被接受。
    #[test]
    fn prepare_install_accepts_a_plugin_directory() {
        let dir = TempDir::new("prepare-dir");
        let package = dir.path().join("package");
        write(&package, "plugin.toml", MANIFEST);

        let format = prepare_install(&package, &dir.path().join("plugins")).unwrap();
        assert_eq!(format, PluginPackageFormat::Directory);
    }

    /// 不认识的扩展名要在安装前就报可读原因，而不是解包到一半才失败。
    #[test]
    fn prepare_install_rejects_unknown_format() {
        let dir = TempDir::new("prepare-unknown");
        let file = dir.path().join("package.txt");
        write(dir.path(), "package.txt", "not a plugin");

        let err = prepare_install(&file, &dir.path().join("plugins")).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("不支持的插件包格式"), "{text}");
    }

    /// 选中的正是 `plugins/<id>` 时必须拒绝：`copy_plugin` 会先删目标目录，
    /// 目标与源同路径等于先把源删掉。
    #[test]
    fn prepare_install_rejects_an_install_dir_as_source() {
        let dir = TempDir::new("prepare-inside");
        let root = dir.path().join("plugins");
        let installed = root.join("com.example.installer");
        write(&installed, "plugin.toml", MANIFEST);

        let err = prepare_install(&installed, &root).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("安装目录"), "{text}");
    }

    /// 压缩工具默认给包套一层目录（`my-plugin/plugin.toml`，本仓 zip-dir.ps1 亦然）。
    /// 包根定位要下探这一层，否则最常见的打包方式全部装不上。
    #[test]
    fn package_root_descends_into_a_single_wrapped_folder() {
        let dir = TempDir::new("root-wrapped");
        write(dir.path(), "my-plugin/plugin.toml", MANIFEST);

        let root = PluginInstaller::locate_package_root(dir.path()).unwrap();
        assert_eq!(root, dir.path().join("my-plugin"));
    }

    /// 多个目录都有清单时不替用户挑：报可读错误，而不是装错一个。
    #[test]
    fn package_root_rejects_ambiguous_wrapped_folders() {
        let dir = TempDir::new("root-ambiguous");
        write(dir.path(), "a/plugin.toml", MANIFEST);
        write(dir.path(), "b/plugin.toml", MANIFEST);

        let err = PluginInstaller::locate_package_root(dir.path()).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("多个候选插件目录"), "{text}");
    }
}
