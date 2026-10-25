//! 插件管理器核心（生命周期唯一入口）
//!
//! 2026-10-15 重构（全量优化，见 `docs/architecture/plugin/plugin-dev-plan.md` §11）：
//! `PluginLoader`（loader.rs）并入本模块 —— 原来「PluginManager 管运行时、PluginLoader 管
//! 安装态」两套入口并存：扫描 / 加载 / 激活逻辑重复，且参数口径不一致（曾把 wasm **文件**
//! 路径传给期望**插件目录**的 `load_plugin`，导致任何 wasm 插件激活都报
//! "Plugin not found or invalid"）。现在一个 PluginManager 走完
//! 「发现 → 加载 → 激活 → 停用 → 卸载」，安装态状态机（`LoadStatus`）与查询
//! （`get_loaded_plugin` / `list_loaded_plugins`）一并并入；`init_plugin_loader` /
//! `get_plugin_loader` 删除，初始化统一走 [`init_plugin_manager`]。
//!
//! 形态判定（[`PluginManager::detect_plugin_kind`]）2026-10-15 修正：优先看 P1 的
//! `[backend]` 段（`backend.kind = sidecar/script` 才是子进程形态），再 `capabilities.wasm`，
//! 最后 `capabilities.frontend`（D1 三轨的「扩展宿主进程」形态，P3/P4 落地前只记状态）。
//! 此前 frontend 被误映射为 Sidecar（那是驱动进程，不是前端扩展）。
//!
//! 路径安全：所有用清单 `plugin.id` 拼路径的地方先过 [`paths::validate_plugin_id`]
//! （README §5 硬约束 #6；id 由第三方清单给出）。

use std::collections::HashMap;
use std::hash::Hasher as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::{Arc, RwLock};

use paths::validate_plugin_id;
use serde::{Deserialize, Serialize};

use shared::error::{CommonError, CoreError, PluginError};

use super::manifest::{ManifestParser, PluginManifest};
use crate::sidecar::factory::register_sidecar_drivers;
use crate::sidecar::supervisor::SidecarSupervisor;
use crate::wasm::{PluginMetadata, PluginState, PluginType};
#[cfg(feature = "wasm")]
use crate::wasm::ExtismPluginManager;

/// 插件形态（三轨）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginKind {
    /// 进程内沙箱（Extism；仅 `wasm` feature 下编译，见 detect_plugin_kind）
    #[cfg(feature = "wasm")]
    Wasm,
    /// 独立子进程（sidecar / script，进进程池）
    Sidecar,
    /// 面板 / 命令 / 设置扩展（D1 的「独立扩展宿主进程」形态；P3/P4 落地前只记状态）
    Frontend,
}

/// 热插拔扫描快照里的一项（`scan_and_diff_plugins` 的 diff 基准）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEntry {
    /// 清单版本（当前只用于占位，变化判定走 manifest_hash）
    pub version: String,
    /// `plugin.toml` 内容 hash（换版本 / 改能力 / 改权限都会变）
    pub manifest_hash: u64,
}

/// 一次热插拔扫描的结果（供运行时循环打日志 / 后续插件面板提示）
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PluginDiff {
    /// 新出现的插件目录（已自动加载；未禁用则已激活）
    pub added: Vec<String>,
    /// 目录消失的插件（已停用并移出运行时索引）
    pub removed: Vec<String>,
    /// 清单变化、置 `needs_reload` 的插件（VS Code 口径：不热生效）
    pub changed: Vec<String>,
}

/// 插件信息（会话态展示用）
#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
    pub metadata: PluginMetadata,
    pub state: PluginState,
}

/// 已加载的插件（安装态记录；原 loader.rs 的 `LoadedPlugin` 并入）
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    /// 插件 ID
    pub id: String,
    /// 插件清单
    pub manifest: PluginManifest,
    /// 插件安装路径
    pub install_path: PathBuf,
    /// 加载状态
    pub status: LoadStatus,
}

/// 加载状态（原 loader.rs 的 `LoadStatus` 并入）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LoadStatus {
    /// 未加载
    #[default]
    Unloaded,
    /// 加载中
    Loading,
    /// 已加载
    Loaded,
    /// 激活中
    Activating,
    /// 已激活
    Active,
    /// 加载失败
    LoadFailed(String),
    /// 激活失败
    ActivationFailed(String),
}

/// 插件管理器核心：发现 → 加载 → 激活 → 停用 → 卸载，单一入口。
pub struct PluginManager {
    /// wasm 运行时（Extism；进程内沙箱；仅 `wasm` feature 下存在）
    #[cfg(feature = "wasm")]
    wasm_manager: Arc<ExtismPluginManager>,
    /// sidecar 运行时（P1 的 `SidecarSupervisor`；构造期创建，部署 / 注册是幂等的）
    sidecar: Arc<tokio::sync::Mutex<SidecarSupervisor>>,
    /// 插件发现目录（可多个）
    plugin_dirs: RwLock<Vec<PathBuf>>,
    /// 安装目录（扫描 / 安装落点）
    install_dir: PathBuf,
    /// 安装态缓存（id → LoadedPlugin）
    loaded_plugins: RwLock<HashMap<String, LoadedPlugin>>,
    /// 会话态索引（id → kind；决定 activate 走哪条运行时）
    plugin_index: Arc<RwLock<HashMap<String, PluginKind>>>,
    /// 热插拔扫描快照（id → 清单 hash；`scan_and_diff_plugins` 与上次扫描对比用）
    scan_snapshot: RwLock<HashMap<String, ScanEntry>>,
    /// 清单已变化、等待重载的插件（VS Code 口径：更新不热生效，重启 / 手动重载后生效）
    needs_reload: RwLock<Vec<String>>,
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}

/// 锁失败的统一错误
fn lock_err(what: &str) -> CoreError {
    CoreError::common(CommonError::Internal(format!(
        "Failed to acquire lock on {what}"
    )))
}

/// 插件不存在的统一错误
fn not_found(id: &str) -> CoreError {
    CoreError::common(CommonError::Internal(format!("Plugin {id} not found")))
}

/// 无 `wasm` feature 时遇到 wasm 形态插件的统一报错（诚实口径：不是不支持，
/// 是本构建没编入 wasm 轨；需要 `--features rds-plugin/wasm` 的构建）。
#[cfg(not(feature = "wasm"))]
fn wasm_not_compiled(id: &str) -> CoreError {
    CoreError::common(CommonError::Internal(format!(
        "wasm 插件需要带 `wasm` feature 的构建（当前构建未启用，见 crates/plugin/Cargo.toml）: {id}"
    )))
}

impl PluginManager {
    /// 创建插件管理器；`install_dir` 是插件安装目录（`paths::plugins_dir()`）。
    pub fn new(install_dir: PathBuf) -> Self {
        Self {
            #[cfg(feature = "wasm")]
            wasm_manager: Arc::new(ExtismPluginManager::new()),
            sidecar: Arc::new(tokio::sync::Mutex::new(SidecarSupervisor::new())),
            plugin_dirs: RwLock::new(Vec::new()),
            install_dir,
            loaded_plugins: RwLock::new(HashMap::new()),
            plugin_index: Arc::new(RwLock::new(HashMap::new())),
            scan_snapshot: RwLock::new(HashMap::new()),
            needs_reload: RwLock::new(Vec::new()),
        }
    }

    /// 添加插件发现目录
    pub fn add_plugin_dir(&self, path: PathBuf) {
        if let Ok(mut dirs) = self.plugin_dirs.write() {
            dirs.push(path);
        }
    }

    /// 初始化安装目录（不存在时创建；空路径 = 未配置，跳过）
    pub async fn init(&self) -> Result<(), CoreError> {
        if self.install_dir.as_os_str().is_empty() {
            return Ok(());
        }
        if !self.install_dir.exists() {
            std::fs::create_dir_all(&self.install_dir).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to create plugin directory: {}",
                    e
                )))
            })?;
        }
        Ok(())
    }

    /// 扫描并发现插件（只解析清单、判定形态，不加载运行时）
    pub fn scan_plugins(&self) -> Result<Vec<PluginInfo>, CoreError> {
        let mut discovered_plugins = Vec::new();

        let dirs = self.plugin_dirs.read().map_err(|_| lock_err("plugin_dirs"))?;

        for dir in dirs.iter() {
            if !dir.exists() {
                continue;
            }

            let entries = std::fs::read_dir(dir).map_err(|e| {
                CoreError::common(CommonError::Internal(format!(
                    "Failed to read plugin dir: {}",
                    e
                )))
            })?;

            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(info) = self.try_discover_plugin(&path)? {
                    discovered_plugins.push(info);
                }
            }
        }

        Ok(discovered_plugins)
    }

    /// 尝试发现单个插件：读 `<dir>/plugin.toml`，解析清单并判定形态。
    ///
    /// 目录里没有 `plugin.toml` 视为「不是插件」，返回 `None`（不报错，容忍同目录其他文件）。
    fn try_discover_plugin(&self, path: &Path) -> Result<Option<PluginInfo>, CoreError> {
        let manifest_path = path.join("plugin.toml");

        if !manifest_path.exists() {
            return Ok(None);
        }

        let manifest = ManifestParser::parse(&manifest_path)?;
        let kind = self.detect_plugin_kind(&manifest)?;

        Ok(Some(PluginInfo {
            id: manifest.plugin.id.clone(),
            name: manifest.plugin.name.clone(),
            version: manifest.plugin.version.clone(),
            kind,
            metadata: PluginMetadata {
                id: manifest.plugin.id.clone(),
                name: manifest.plugin.name.clone(),
                version: manifest.plugin.version.clone(),
                plugin_type: if manifest.capabilities.wasm.is_some() {
                    PluginType::Tool
                } else {
                    PluginType::Analytics
                },
                description: manifest.plugin.description.clone(),
                author: manifest.plugin.publisher.clone(),
                entry_point: String::new(),
            },
            state: PluginState::Inactive,
        }))
    }

    /// 判定插件形态：`[backend]` 优先（P1 契约），再 `capabilities.wasm`，最后 `capabilities.frontend`。
    ///
    /// 2026-10-15 修正：此前 `capabilities.frontend` 被映射为 `Sidecar` —— frontend 是
    /// 面板 / 命令 / 设置扩展（D1 三轨的「扩展宿主进程」形态），不是驱动子进程。
    fn detect_plugin_kind(&self, manifest: &PluginManifest) -> Result<PluginKind, CoreError> {
        if let Some(backend) = &manifest.backend {
            match backend.kind {
                crate::manifest::BackendKind::Wasm => {
                    #[cfg(feature = "wasm")]
                    return Ok(PluginKind::Wasm);
                    #[cfg(not(feature = "wasm"))]
                    return Err(wasm_not_compiled(&manifest.plugin.id));
                }
                crate::manifest::BackendKind::Sidecar
                | crate::manifest::BackendKind::Script => return Ok(PluginKind::Sidecar),
            }
        }
        if manifest.capabilities.wasm.is_some() {
            #[cfg(feature = "wasm")]
            return Ok(PluginKind::Wasm);
            #[cfg(not(feature = "wasm"))]
            return Err(wasm_not_compiled(&manifest.plugin.id));
        }
        if manifest.capabilities.frontend.is_some() {
            return Ok(PluginKind::Frontend);
        }
        Err(CoreError::common(CommonError::Internal(format!(
            "无法判定插件形态（无 [backend]，也无 capabilities.wasm/frontend）: {}",
            manifest.plugin.id
        ))))
    }

    /// 从安装目录扫描并加载全部插件（安装态；逐目录走 [`Self::load_plugin_from_dir`]）
    pub async fn scan_and_load_plugins(&self) -> Result<Vec<LoadedPlugin>, CoreError> {
        let mut loaded = Vec::new();

        if self.install_dir.as_os_str().is_empty() || !self.install_dir.exists() {
            return Ok(loaded);
        }

        let entries = std::fs::read_dir(&self.install_dir).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to read plugin directory: {}",
                e
            )))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Ok(plugin) = self.load_plugin_from_dir(&path).await {
                    loaded.push(plugin);
                }
            }
        }

        Ok(loaded)
    }

    /// 从目录加载单个插件（解析清单、校验、入缓存；**不启动运行时**）。
    ///
    /// 清单 `plugin.id` 由第三方给出，进 `install_path` / 索引前必须先过
    /// [`validate_plugin_id`]（路径注入防护）。
    pub async fn load_plugin_from_dir(&self, plugin_dir: &Path) -> Result<LoadedPlugin, CoreError> {
        let manifest_path = plugin_dir.join("plugin.toml");
        if !manifest_path.exists() {
            return Err(CoreError::common(CommonError::General(format!(
                "Plugin manifest not found at {}",
                manifest_path.display()
            ))));
        }

        let manifest = ManifestParser::parse(&manifest_path)?;
        validate_plugin_id(&manifest.plugin.id).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "插件 id 不合法（拼路径前必须校验）: {}",
                e
            )))
        })?;

        let plugin_id = manifest.plugin.id.clone();
        let loaded_plugin = LoadedPlugin {
            id: plugin_id.clone(),
            manifest,
            install_path: plugin_dir.to_path_buf(),
            status: LoadStatus::Loaded,
        };

        // 同步登记形态索引：热插拔新增路径只走这里（不经 load_plugin），
        // 不登记的话 activate 会 not_found、removed 分支会跳过 unload（曾静默失败）
        let kind = self.detect_plugin_kind(&loaded_plugin.manifest)?;
        self.plugin_index
            .write()
            .map_err(|_| lock_err("plugin_index"))?
            .insert(plugin_id.clone(), kind);

        self.loaded_plugins
            .write()
            .map_err(|_| lock_err("loaded_plugins"))?
            .insert(plugin_id, loaded_plugin.clone());

        Ok(loaded_plugin)
    }

    /// 加载插件到会话态（记录形态索引；wasm 运行时在激活时按需加载）。
    ///
    /// `dir` 是**插件目录**（内含 `plugin.toml`），不是 wasm 文件 —— 曾把 wasm 文件路径
    /// 传到这里导致激活必失败（D1 修复）。
    pub fn load_plugin(&self, id: &str, dir: &Path) -> Result<PluginInfo, CoreError> {
        let info = self.try_discover_plugin(dir)?.ok_or_else(|| {
            CoreError::common(CommonError::Internal(
                "Plugin not found or invalid".to_string(),
            ))
        })?;

        self.plugin_index
            .write()
            .map_err(|_| lock_err("plugin_index"))?
            .insert(id.to_string(), info.kind);

        Ok(info)
    }

    /// 激活插件：
    ///
    /// - `Wasm`：读 wasm 字节 → 加载进 Extism 运行时 → 调 `activate` 钩子（幂等）；
    /// - `Sidecar`：部署到 supervisor 并把声明的驱动注册进 `DriverRegistry`（幂等覆盖）；
    /// - `Frontend`：只更新状态（扩展宿主进程是 P3/P4 的事，现在没有运行时）。
    pub async fn activate_plugin(&self, id: &str) -> Result<(), CoreError> {
        // 状态机：先置 Activating，结果回填 Active / ActivationFailed
        {
            let mut map = self.loaded_plugins.write().map_err(|_| lock_err("loaded_plugins"))?;
            if let Some(p) = map.get_mut(id) {
                p.status = LoadStatus::Activating;
            }
        }

        let kind = self
            .plugin_index
            .read()
            .map_err(|_| lock_err("plugin_index"))?
            .get(id)
            .copied()
            .ok_or_else(|| not_found(id))?;

        let result = match kind {
            #[cfg(feature = "wasm")]
            PluginKind::Wasm => self.activate_wasm(id),
            PluginKind::Sidecar => self.activate_sidecar(id).await,
            PluginKind::Frontend => Ok(()),
        };

        let mut map = self.loaded_plugins.write().map_err(|_| lock_err("loaded_plugins"))?;
        match &result {
            Ok(()) => {
                if let Some(p) = map.get_mut(id) {
                    p.status = LoadStatus::Active;
                }
            }
            Err(e) => {
                if let Some(p) = map.get_mut(id) {
                    p.status = LoadStatus::ActivationFailed(e.to_string());
                }
                return Err(e.clone());
            }
        }
        drop(map);

        crate::events::emit_plugin_activated(id);
        Ok(())
    }

    /// wasm 激活路径：从安装态缓存取清单（`capabilities.wasm.entry`）+ 目录，读字节进 Extism。
    #[cfg(feature = "wasm")]
    fn activate_wasm(&self, id: &str) -> Result<(), CoreError> {
        // 幂等：已在 Active 的插件重复激活直接返回
        if self
            .wasm_manager
            .get_plugin_state(id)
            .map_err(|e| CoreError::common(CommonError::Internal(e.to_string())))?
            == PluginState::Active
        {
            return Ok(());
        }

        let loaded = self
            .loaded_plugins
            .read()
            .map_err(|_| lock_err("loaded_plugins"))?
            .get(id)
            .cloned()
            .ok_or_else(|| not_found(id))?;

        let wasm_entry = loaded
            .manifest
            .capabilities
            .wasm
            .as_ref()
            .map(|w| w.entry.clone())
            .ok_or_else(|| {
                CoreError::common(CommonError::Internal(
                    "wasm 插件缺少 capabilities.wasm".to_string(),
                ))
            })?;

        let wasm_path = loaded.install_path.join(&wasm_entry);
        let wasm_bytes = std::fs::read(&wasm_path).map_err(|e| {
            CoreError::common(CommonError::Internal(format!(
                "Failed to read wasm {}: {}",
                wasm_path.display(),
                e
            )))
        })?;

        // 元数据来自清单（曾硬编码 name=id / version="0.1.0"，与清单脱节 —— D8 修复）
        let m = &loaded.manifest.plugin;
        let metadata = PluginMetadata {
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            plugin_type: PluginType::Tool,
            description: m.description.clone(),
            author: m.publisher.clone(),
            entry_point: String::new(),
        };

        self.wasm_manager
            .load_plugin(id, &wasm_bytes, metadata, None, None)?;
        self.wasm_manager.activate_plugin(id)
    }

    /// sidecar 激活路径：部署清单到 supervisor 并把声明的驱动注册进 `DriverRegistry`。
    async fn activate_sidecar(&self, id: &str) -> Result<(), CoreError> {
        let loaded = self
            .loaded_plugins
            .read()
            .map_err(|_| lock_err("loaded_plugins"))?
            .get(id)
            .cloned()
            .ok_or_else(|| not_found(id))?;

        register_sidecar_drivers(Arc::clone(&self.sidecar), &loaded.manifest, &loaded.install_path)
            .await?;
        Ok(())
    }

    /// 停用插件：wasm 调 Extism 停用；sidecar 注销其注册的驱动（`DriverRegistry::unregister`）；
    /// frontend 只更新状态。
    pub async fn deactivate_plugin(&self, id: &str) -> Result<(), CoreError> {
        let kind = self
            .plugin_index
            .read()
            .map_err(|_| lock_err("plugin_index"))?
            .get(id)
            .copied()
            .ok_or_else(|| not_found(id))?;

        match kind {
            #[cfg(feature = "wasm")]
            PluginKind::Wasm => {
                self.wasm_manager.deactivate_plugin(id)?;
            }
            PluginKind::Sidecar => {
                // 注销该插件声明过的全部驱动 id（部署时按 contributes.drivers 注册）
                let loaded = self
                    .loaded_plugins
                    .read()
                    .map_err(|_| lock_err("loaded_plugins"))?
                    .get(id)
                    .cloned()
                    .ok_or_else(|| not_found(id))?;
                for driver in &loaded.manifest.contributes.drivers {
                    engine::driver::registry::DriverRegistry::unregister(&driver.id);
                }
                // 停掉该插件全部 sidecar 实例：卸载安全的前提是进程停干净
                // （Windows 删插件目录时 exe 不能被占用）。会话在这里如实作废。
                self.sidecar
                    .lock()
                    .await
                    .shutdown_plugin(
                        id,
                        std::time::Instant::now(),
                        crate::sidecar::supervisor::SHUTDOWN_GRACE,
                    )
                    .await;
            }
            PluginKind::Frontend => {}
        }

        let mut map = self.loaded_plugins.write().map_err(|_| lock_err("loaded_plugins"))?;
        if let Some(p) = map.get_mut(id) {
            p.status = LoadStatus::Loaded;
        }
        drop(map);

        crate::events::emit_plugin_deactivated(id);
        Ok(())
    }

    /// 卸载插件：先停用，再从运行时 / 缓存 / 索引移除。
    pub async fn unload_plugin(&self, id: &str) -> Result<(), CoreError> {
        // 形态判定只在 wasm 轨需要（sidecar / frontend 的卸载统一走 deactivate）。
        // cfg(not(wasm)) 时不读 `kind`，避免 unused variable。
        #[cfg(feature = "wasm")]
        let kind = self
            .plugin_index
            .read()
            .map_err(|_| lock_err("plugin_index"))?
            .get(id)
            .copied()
            .ok_or_else(|| not_found(id))?;

        self.deactivate_plugin(id).await?;

        #[cfg(feature = "wasm")]
        if kind == PluginKind::Wasm {
            self.wasm_manager.unload_plugin(id)?;
        }

        self.plugin_index
            .write()
            .map_err(|_| lock_err("plugin_index"))?
            .remove(id);
        self.loaded_plugins
            .write()
            .map_err(|_| lock_err("loaded_plugins"))?
            .remove(id);

        crate::events::emit_plugin_unloaded(id);
        Ok(())
    }

    /// 卸载插件（**磁盘删除**，VS Code 口径）：
    /// 依赖检查（被依赖不让卸）→ 停用（含停 sidecar 进程）→ 删 `install_dir/<id>/`
    /// → 清 global.db 的 plugin_store 记录 → 移出运行时索引 → 事件。
    ///
    /// **保留 `plugin-data/<id>/`**：插件数据可能含连接 / 凭据，卸载不删用户数据。
    pub async fn uninstall_plugin(&self, id: &str) -> Result<(), CoreError> {
        validate_plugin_id(id).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "插件 id 不合法（拼路径前必须校验）: {e}"
            )))
        })?;

        // 1. 依赖检查（VS Code：被依赖的扩展不让卸，列出引用者）
        if let Some(db) = engine::migration::get_global_db_manager() {
            let dependents = crate::dependency::DependencyManager::new(db)
                .check_dependents(id)
                .await?;
            if !dependents.is_empty() {
                return Err(CoreError::plugin(PluginError::UninstallFailed {
                    plugin_id: id.to_string(),
                    reason: format!("被以下插件依赖，请先卸载它们: {}", dependents.join(", ")),
                }));
            }
        }

        // 2. 停用（sidecar 分支会先停进程 —— 进程停干净了，目录才删得掉）
        if self
            .plugin_index
            .read()
            .map_err(|_| lock_err("plugin_index"))?
            .contains_key(id)
        {
            self.deactivate_plugin(id).await?;
        }

        // 3. 删插件目录（保留 plugin-data：卸载不删用户数据）
        let dir = self.install_dir.join(id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| {
                CoreError::plugin(PluginError::UninstallFailed {
                    plugin_id: id.to_string(),
                    reason: format!("删除目录失败（进程未停干净?）: {e}"),
                })
            })?;
        }

        // 4. 清 global.db 的 plugin_store 记录（DB 未初始化时跳过）
        if let Some(db) = engine::migration::get_global_db_manager() {
            db.delete_plugin(id).await?;
        }

        // 5. 移出运行时索引
        self.plugin_index
            .write()
            .map_err(|_| lock_err("plugin_index"))?
            .remove(id);
        self.loaded_plugins
            .write()
            .map_err(|_| lock_err("loaded_plugins"))?
            .remove(id);

        // 6. 事件
        crate::events::emit_plugin_uninstalled(id);
        Ok(())
    }

    /// 禁用插件（VS Code 口径：持久标记，启动不激活，清单仍可见）。
    ///
    /// 权威状态 = global.db `plugin_store.is_enabled`（启动扫描 / 未来插件面板按它决定
    /// 要不要激活）。**运行中禁用 = 立刻停用**（sidecar 分支会停进程），不是等下次启动。
    pub async fn disable_plugin(&self, id: &str) -> Result<(), CoreError> {
        if let Some(db) = engine::migration::get_global_db_manager() {
            db.update_plugin_enabled(id, false).await?;
        }
        if self
            .plugin_index
            .read()
            .map_err(|_| lock_err("plugin_index"))?
            .contains_key(id)
        {
            self.deactivate_plugin(id).await?;
        }
        crate::events::emit_plugin_disabled(id);
        Ok(())
    }

    /// 启用插件（与 [`Self::disable_plugin`] 对称）：持久标记 + 若已加载则重新激活。
    pub async fn enable_plugin(&self, id: &str) -> Result<(), CoreError> {
        if let Some(db) = engine::migration::get_global_db_manager() {
            db.update_plugin_enabled(id, true).await?;
        }
        if self
            .plugin_index
            .read()
            .map_err(|_| lock_err("plugin_index"))?
            .contains_key(id)
        {
            self.activate_plugin(id).await?;
        }
        crate::events::emit_plugin_enabled(id);
        Ok(())
    }

    /// 热插拔扫描（挂在运行时循环的每个 tick；不做 watcher，5s 目录 diff 足够）：
    /// - **目录即插件**：`install_dir/<id>/plugin.toml` 存在就算一个插件
    /// - 新目录 → 解析清单 → 自动加载；DB 未禁用（或 DB 无记录，默认启用）则激活
    /// - 目录消失 → 停用（含停 sidecar 进程）+ 移出运行时索引
    /// - 清单内容变化 → 置 `needs_reload`，**不热生效**（VS Code 口径：更新要重载）
    pub async fn scan_and_diff_plugins(&self) -> Result<PluginDiff, CoreError> {
        let mut disk: HashMap<String, ScanEntry> = HashMap::new();
        if !self.install_dir.as_os_str().is_empty() && self.install_dir.exists() {
            let entries = std::fs::read_dir(&self.install_dir).map_err(|e| {
                CoreError::common(CommonError::General(format!(
                    "Failed to read plugin dir: {}",
                    e
                )))
            })?;
            for entry in entries.flatten() {
                if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let id = entry.file_name().to_string_lossy().to_string();
                // 目录即插件：没有 plugin.toml 的目录不算（也防误扫拖进来的其他文件）
                let manifest_path = entry.path().join("plugin.toml");
                if !manifest_path.exists() {
                    continue;
                }
                let bytes = match std::fs::read(&manifest_path) {
                    Ok(b) => b,
                    // 正在写入的目录：跳过本 tick，下个 tick 再试
                    Err(_) => continue,
                };
                // 变化判定用内容 hash：换版本 / 改能力 / 改权限都会触发
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                std::hash::Hasher::write(&mut hasher, &bytes);
                disk.insert(
                    id,
                    ScanEntry {
                        version: String::new(),
                        manifest_hash: hasher.finish(),
                    },
                );
            }
        }

        // 读快照算 diff（快照只读、不跨 await 持锁：std RwLockGuard 非 Send，
        // 跨 await 会让 tokio::spawn 的循环 future 不 Send）
        let snapshot = self
            .scan_snapshot
            .read()
            .map_err(|_| lock_err("scan_snapshot"))?
            .clone();

        let mut diff = PluginDiff::default();
        for (id, entry) in &disk {
            match snapshot.get(id) {
                None => diff.added.push(id.clone()),
                Some(prev) if prev.manifest_hash != entry.manifest_hash => {
                    diff.changed.push(id.clone())
                }
                Some(_) => {}
            }
        }
        for id in snapshot.keys() {
            if !disk.contains_key(id) {
                diff.removed.push(id.clone());
            }
        }

        // 新增：加载 + 激活（DB 未禁用才激活）
        for id in &diff.added {
            if let Err(e) = self.load_plugin_from_dir(&self.install_dir.join(id)).await {
                tracing::warn!(plugin_id = %id, error = %e, "热插拔加载失败");
                continue;
            }
            let enabled = match engine::migration::get_global_db_manager() {
                Some(db) => match db.get_plugin(id).await {
                    Ok(Some(p)) => p.is_enabled,
                    // DB 里没记录 = 目录直接拷进来的插件，默认启用
                    _ => true,
                },
                None => true,
            };
            if enabled {
                if let Err(e) = self.activate_plugin(id).await {
                    tracing::warn!(plugin_id = %id, error = %e, "热插拔激活失败");
                }
            }
        }

        // 消失：停用 + 移出运行时索引（unload_plugin 不碰磁盘，目录已没了也安全）；
        // 顺带清掉该插件的重载标记（插件都没了，提示重载没有意义）
        for id in &diff.removed {
            if self
                .plugin_index
                .read()
                .map_err(|_| lock_err("plugin_index"))?
                .contains_key(id)
            {
                if let Err(e) = self.unload_plugin(id).await {
                    tracing::warn!(plugin_id = %id, error = %e, "热插拔卸载失败");
                }
            }
            self.needs_reload
                .write()
                .map_err(|_| lock_err("needs_reload"))?
                .retain(|p| p != id);
        }

        // 变化：置 needs_reload（更新不热生效，VS Code 口径），UI 层读它提示重载
        for id in &diff.changed {
            if !self
                .needs_reload
                .read()
                .map_err(|_| lock_err("needs_reload"))?
                .contains(id)
            {
                self.needs_reload
                    .write()
                    .map_err(|_| lock_err("needs_reload"))?
                    .push(id.clone());
            }
        }

        *self
            .scan_snapshot
            .write()
            .map_err(|_| lock_err("scan_snapshot"))? = disk;
        Ok(diff)
    }

    /// 待重载插件列表（P4 插件面板 / 状态栏据此提示「重启生效」）
    pub fn pending_reloads(&self) -> Vec<String> {
        self.needs_reload
            .read()
            .map(|m| m.clone())
            .unwrap_or_default()
    }

    /// 全量重载（VS Code「Reload Window」口径）：
    /// 停掉所有已激活插件 → 清 needs_reload → 从磁盘重新加载并激活全部（DB 禁用的除外）。
    ///
    /// 调用的前提是用户明确触发了重载命令；插件内部更新不自动走到这里。
    pub async fn reload_plugins(&self) -> Result<PluginDiff, CoreError> {
        let ids: Vec<String> = self
            .plugin_index
            .read()
            .map_err(|_| lock_err("plugin_index"))?
            .keys()
            .cloned()
            .collect();
        for id in ids {
            // 逐个停（含停 sidecar 进程），失败的记日志、不中断整轮重载
            if let Err(e) = self.deactivate_plugin(&id).await {
                tracing::warn!(plugin_id = %id, error = %e, "重载停用失败");
            }
        }
        self.needs_reload
            .write()
            .map_err(|_| lock_err("needs_reload"))?
            .clear();
        // 清快照：让重载后的扫描把**所有**插件当作「新增」重新加载 + 激活
        // （不清的话 diff 对比上次快照会判「无变化」，全量重载就空转了）
        self.scan_snapshot
            .write()
            .map_err(|_| lock_err("scan_snapshot"))?
            .clear();

        // 重新扫描整个安装目录并加载 + 激活
        self.scan_and_diff_plugins().await
    }

    /// 列出所有已加载插件（wasm 从运行时取，sidecar / frontend 从安装态缓存 + 形态索引组装）
    pub fn list_plugins(&self) -> Result<Vec<PluginInfo>, CoreError> {
        let mut plugins = Vec::new();

        // wasm：运行时里真实存在、且还在形态索引里的（仅 `wasm` feature 下才有运行时）
        #[cfg(feature = "wasm")]
        {
            let wasm_plugins: Vec<_> = self
                .wasm_manager
                .list_plugins()
                .map_err(|e| CoreError::common(CommonError::Internal(format!(
                    "Failed to list plugins: {}",
                    e
                ))))?
                .into_iter()
                .map(|(meta, state)| PluginInfo {
                    id: meta.id.clone(),
                    name: meta.name.clone(),
                    version: meta.version.clone(),
                    kind: PluginKind::Wasm,
                    metadata: meta,
                    state,
                })
                .collect();
            plugins.extend(wasm_plugins);
        }

        // sidecar / frontend：形态索引里有、但不在 wasm 运行时里的
        let index = self.plugin_index.read().map_err(|_| lock_err("plugin_index"))?;
        let loaded = self.loaded_plugins.read().map_err(|_| lock_err("loaded_plugins"))?;
        for (id, kind) in index.iter() {
            #[cfg(feature = "wasm")]
            if matches!(kind, PluginKind::Wasm) {
                continue; // 上面已经列了
            }
            if let Some(lp) = loaded.get(id) {
                let m = &lp.manifest.plugin;
                plugins.push(PluginInfo {
                    id: m.id.clone(),
                    name: m.name.clone(),
                    version: m.version.clone(),
                    kind: *kind,
                    metadata: PluginMetadata {
                        id: m.id.clone(),
                        name: m.name.clone(),
                        version: m.version.clone(),
                        plugin_type: PluginType::Analytics,
                        description: m.description.clone(),
                        author: m.publisher.clone(),
                        entry_point: String::new(),
                    },
                    state: PluginState::Inactive,
                });
            }
        }

        Ok(plugins)
    }

    /// 获取单个已加载插件（安装态）
    pub fn get_loaded_plugin(&self, plugin_id: &str) -> Option<LoadedPlugin> {
        self.loaded_plugins.read().ok()?.get(plugin_id).cloned()
    }

    /// 获取全部已加载插件（安装态）
    pub fn list_loaded_plugins(&self) -> Vec<LoadedPlugin> {
        self.loaded_plugins
            .read()
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    /// 获取 WASM 管理器引用（仅 `wasm` feature 下存在；宿主接入 wasm 渲染轨用）
    #[cfg(feature = "wasm")]
    pub fn wasm_manager(&self) -> Arc<ExtismPluginManager> {
        Arc::clone(&self.wasm_manager)
    }

    /// 获取 sidecar 运行时引用（宿主注入 / 诊断用）
    pub fn sidecar_supervisor(&self) -> Arc<tokio::sync::Mutex<SidecarSupervisor>> {
        Arc::clone(&self.sidecar)
    }
}

/// 全局插件管理器实例
pub static PLUGIN_MANAGER: OnceLock<Arc<PluginManager>> = OnceLock::new();

/// 全局 tokio runtime 句柄（UI 事件回调跨线程 `block_on` 用）。
///
/// 插件管理器的 `enable/disable/uninstall` 等操作依赖 tokio 上下文（DB 异步锁），
/// UI 线程不在 tokio 里，由 `init_plugin_manager` 把宿主 runtime 句柄存到这里，
/// 插件面板的事件回调在后台 executor 里 `handle.block_on(...)` 执行（不阻塞 UI）。
pub static PLUGIN_RUNTIME_HANDLE: OnceLock<tokio::runtime::Handle> = OnceLock::new();

/// 宿主注入 tokio runtime 句柄（`init_plugin_manager` 内部调用；重复注入忽略）。
pub fn set_runtime_handle(handle: tokio::runtime::Handle) {
    let _ = PLUGIN_RUNTIME_HANDLE.set(handle);
}

/// 取全局 tokio runtime 句柄；未初始化（如测试进程）返回 `None`。
pub fn runtime_handle() -> Option<&'static tokio::runtime::Handle> {
    PLUGIN_RUNTIME_HANDLE.get()
}

/// 获取全局插件管理器实例
///
/// 如果尚未初始化，将返回 None。必须先调用 [`init_plugin_manager`] 进行初始化。
pub fn get_plugin_manager() -> Option<&'static Arc<PluginManager>> {
    PLUGIN_MANAGER.get()
}

/// 初始化全局插件管理器（应用启动时调用一次；`install_dir` 为插件安装目录）
///
/// 2026-10-15 起带 `install_dir` 参数：`init_plugin_loader` 已删除，安装目录统一在这里注入。
/// 2026-10-15 起在初始化完成后自动 spawn **运行时循环**（sidecar 的 sweep / ping /
/// 事件落地全靠它 tick —— 此前只有测试在 tick）；`detach`：任务生命周期随 tokio
/// runtime（宿主退出即停），无需句柄。
pub async fn init_plugin_manager(install_dir: PathBuf) -> Result<(), CoreError> {
    // 让 UI 事件回调能跨线程 block_on 管理操作（enable/disable/uninstall 依赖 tokio）
    set_runtime_handle(
        tokio::runtime::Handle::try_current().map_err(|_| {
            CoreError::common(shared::error::CommonError::General(
                "插件管理器初始化必须在 tokio runtime 上下文内".to_string(),
            ))
        })?,
    );

    let manager = Arc::new(PluginManager::new(install_dir));
    manager.init().await?;

    PLUGIN_MANAGER.set(manager.clone()).map_err(|_| {
        CoreError::common(shared::error::CommonError::General(
            "Plugin manager already initialized".to_string(),
        ))
    })?;

    crate::sidecar::runtime::PluginRuntimeLoop::new(
        manager,
        crate::sidecar::runtime::RUNTIME_TICK,
    )
    .spawn(); // 句柄 drop 即 detach（任务生命周期随 tokio runtime，宿主退出即停）

    Ok(())
}
