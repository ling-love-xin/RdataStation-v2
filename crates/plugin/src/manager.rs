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

use super::contributes::{ContributionCounts, ContributionRegistry};
use super::manifest::{
    BackendKind, BackendRuntime, ManifestParser, PluginManifest, PluginPermissions,
};
use crate::sidecar::factory::register_sidecar_drivers;
use crate::sidecar::supervisor::SidecarSupervisor;
#[cfg(feature = "wasm")]
use crate::wasm::ExtismPluginManager;
use crate::wasm::{PluginMetadata, PluginState, PluginType};

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
    /// 清单解析 / id / 目录名校验失败，形态无法判定（只用于把坏包留在列表里）
    Unknown,
}

/// 热插拔扫描快照里的一项（`scan_and_diff_plugins` 的 diff 基准）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEntry {
    /// 清单版本（变化判定走 manifest_hash；这里留着供面板 / 排障对照）
    pub version: String,
    /// `plugin.toml` 内容 hash（换版本 / 改能力 / 改权限都会变）
    pub manifest_hash: u64,
    /// 清单声明的外部载荷（`[backend] driver_jars` 的绝对路径，文件或目录）。
    ///
    /// 存进快照是为了**不必每个 tick 重新解析清单**：清单 hash 没变时这份列表必然
    /// 没变，直接复用即可（解析 TOML + 全量校验比读一次文件贵得多，5 秒一轮不值得）。
    pub driver_jars: Vec<String>,
    /// 插件目录里**载荷文件**的指纹（长度 + 修改时间，见 `payload_fingerprint`）。
    ///
    /// 只看 `plugin.toml` 是不够的：重编 `agent.exe`、换 wasm、更新 py 脚本时清单一个
    /// 字节都没变，旧快照会判「无变化」，运行中的 sidecar 就一直跑着旧镜像——实测
    /// 2026-11-12 重编两个驱动后，已连接的 Oracle/MSSQL 会话仍走旧二进制，只能重连
    /// 或重启宿主。有了这个指纹，扫描能发现「壳没变、载荷变了」。
    pub payload_hash: u64,
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
    /// 只有载荷变了（清单没变）、已**自动重启**的插件。
    ///
    /// 与 `changed` 分开记：`changed` 是「要人点一下重载」，这个是「已经自己换好了」。
    /// 重启失败时该 id 会退回 `changed`，仍然需要人工重载。
    pub reloaded: Vec<String>,
}

/// 一个插件声明的驱动（管理页展示用，不参与运行时注册）。
///
/// 只带展示必需字段：驱动 id 是否进入 `drivers` 表由加载链路决定，
/// UI 不能凭这个结构推断「已可用」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginDriverSummary {
    pub id: String,
    pub display_name: String,
    pub default_port: Option<u16>,
    pub category: Option<String>,
}

impl PluginDriverSummary {
    fn from_manifest(manifest: &PluginManifest) -> Vec<Self> {
        manifest
            .contributes
            .drivers
            .iter()
            .map(|driver| Self {
                id: driver.id.clone(),
                display_name: driver.display_name.clone(),
                default_port: driver.default_port,
                category: driver.category.clone(),
            })
            .collect()
    }
}

/// 一个驱动在 `global.db` 的 `drivers` 表里的真实登记状态。
///
/// 清单声明 ≠ 已可用：插件未激活、驱动行被清理、或加载失败时，连接对话框都看不到它。
/// 管理页把这份缓存快照与清单并排展示，用户不必再去翻日志判断「到底登记没有」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginDriverRegistration {
    pub id: String,
    pub enabled: bool,
}

/// 一条清单贡献点的展示摘要（管理页详情栏用）。
///
/// 只保留「用户能看懂的一条」所需字段：类别、id、标题、附加说明。
/// 权限 / 驱动各自有独立结构，不塞进这里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginContributionSummary {
    pub kind: String,
    pub id: String,
    pub label: String,
    pub detail: Option<String>,
}

impl PluginContributionSummary {
    fn from_manifest(manifest: &PluginManifest) -> Vec<Self> {
        let mut items = Vec::new();
        for command in &manifest.contributes.commands {
            let mut parts = Vec::new();
            if let Some(category) = &command.category {
                parts.push(format!("分类 {category}"));
            }
            if let Some(shortcut) = &command.shortcut {
                parts.push(format!("快捷键 {shortcut}"));
            }
            items.push(Self {
                kind: "命令".to_string(),
                id: command.id.clone(),
                label: command.title.clone(),
                detail: (!parts.is_empty()).then(|| parts.join(" · ")),
            });
        }
        for panel in &manifest.contributes.panels {
            items.push(Self {
                kind: "面板".to_string(),
                id: panel.id.clone(),
                label: panel.title.clone(),
                detail: Some(format!("位置 {}", panel.location)),
            });
        }
        for setting in &manifest.contributes.settings {
            items.push(Self {
                kind: "设置".to_string(),
                id: setting.key.clone(),
                label: setting.label.clone().unwrap_or_else(|| setting.key.clone()),
                detail: Some(format!(
                    "{} · 默认 {}",
                    setting.setting_type,
                    serde_json::to_string(&setting.default).unwrap_or_else(|_| "—".to_string())
                )),
            });
        }
        items
    }
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
    /// 最近一次加载 / 激活失败的原因（`state == Error` 时非空）。
    ///
    /// 面板只显示状态徽标时，用户看不到「为什么出错」；把失败原因带出来，
    /// 才能在 240px 窄面板里一行说明（完整日志仍以 tracing 为准）。
    pub error: Option<String>,
    /// 清单里声明的驱动（管理页展示「这个插件贡献了哪些数据库驱动」）。
    pub drivers: Vec<PluginDriverSummary>,
    /// 本次会话缓存里这些驱动的真实登记状态（`drivers` 表的 `driver_kind='sidecar'` 子集）。
    ///
    /// `None` = 本次刷新没读到全局库，具体「未登记」无法判定；`Some(空)` 才是真的一个都没登记。
    pub registered_drivers: Option<Vec<PluginDriverRegistration>>,
    /// `[backend]` 的形态与运行时语言；wasm / 坏包没有清单时为空。
    ///
    /// 详情栏用它说明「原生 sidecar 不需要解释器」这类差异，避免用户把
    /// `Rust` 插件当成「缺 Java/Python」。
    pub backend_kind: Option<BackendKind>,
    pub runtime: Option<BackendRuntime>,
    /// 清单声明的权限（展示与告警轨，不门控；掉权限时用户至少能看见）。
    pub permissions: PluginPermissions,
    /// 清单里的命令 / 面板 / 设置条目（驱动单独在 `drivers`）。
    pub contribution_items: Vec<PluginContributionSummary>,
    /// 插件安装目录（`plugin.toml` 所在目录）。
    ///
    /// 管理页据此提供「打开插件目录」；wasm 运行时列表不回报磁盘路径，故可能为空。
    /// 坏目录也带上路径（用户要能手动清理），而不是只有 id。
    pub install_path: Option<PathBuf>,
    /// 本插件声明依赖的其他插件 id（清单 `dependencies`）。
    pub dependencies: Vec<String>,
    /// 依赖本插件的其他插件 id（全局库反向索引快照）。
    ///
    /// `None` = 没读到全局库，无法判定；`Some(空)` = 读过且没有任何插件依赖它。
    pub dependents: Option<Vec<String>>,
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

/// 安装态的 `LoadStatus` → 面板用的会话态。
///
/// 此前 `list_plugins` 对 sidecar / frontend 插件一律写死 `Inactive`：面板把运行中的插件
/// 显示成「已停用」，行尾只给「启用」（没有「停用」），用户无法从面板停掉正在跑的插件。
impl LoadStatus {
    /// 加载 / 激活结果 → 会话态（面板徽标与安装回执共用这一处口径）。
    pub fn session_state(&self) -> PluginState {
        match self {
            LoadStatus::Active => PluginState::Active,
            LoadStatus::LoadFailed(_) | LoadStatus::ActivationFailed(_) => PluginState::Error,
            _ => PluginState::Inactive,
        }
    }

    /// 加载 / 激活失败的可读原因（成功态与「未激活」均为 `None`）。
    pub fn error_reason(&self) -> Option<&str> {
        match self {
            LoadStatus::LoadFailed(reason) | LoadStatus::ActivationFailed(reason) => Some(reason),
            _ => None,
        }
    }
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
    /// 清单解析 / id / 目录名校验失败的目录（目录名 → 原因）。
    ///
    /// 这些目录连 `PluginManifest` 都建不出来，进不了 `loaded_plugins`；单独记一份，
    /// 面板才看得到坏包、并能按目录名卸载（否则只能手删目录）。
    failed_loads: RwLock<HashMap<String, String>>,
    /// 会话态索引（id → kind；决定 activate 走哪条运行时）
    plugin_index: Arc<RwLock<HashMap<String, PluginKind>>>,
    /// 热插拔扫描快照（id → 清单 hash；`scan_and_diff_plugins` 与上次扫描对比用）
    scan_snapshot: RwLock<HashMap<String, ScanEntry>>,
    /// 清单已变化、等待重载的插件（VS Code 口径：更新不热生效，重启 / 手动重载后生效）
    needs_reload: RwLock<Vec<String>>,
    /// `drivers` 表里 sidecar 行的最近一次快照（id → enabled）。
    ///
    /// `None` = 还没成功读过全局库；`Some(空)` = 读过且确实没有插件驱动行。
    /// 只在 `resync_sidecar_driver_rows` 成功后刷新；`list_plugins` 纯读这份缓存，
    /// 不在同步路径里碰异步数据库。
    registered_drivers: RwLock<Option<HashMap<String, bool>>>,
    /// 反向依赖快照（被依赖插件 id → 依赖者 id 列表）。
    ///
    /// 与 `registered_drivers` 同口径：`None` = 没读到全局库，`Some(空)` = 读过且无依赖。
    /// 在 `resync_sidecar_driver_rows` 里顺带刷新，`list_plugins` 只读缓存。
    dependents: RwLock<Option<HashMap<String, Vec<String>>>>,
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

/// `path` 是否在 `root` 目录下（Windows 盘符 / 大小写不敏感；其他平台按原样比较）。
///
/// 只用于「幽灵登记行」清理：`install_path` 是历史字符串，盘符大小写可能已变
/// （旧行写的是 `D:\RdataStation\Rds\...`），不能用 `Path::starts_with` 直接判。
fn path_is_under(path: &Path, root: &Path) -> bool {
    let normalize = |p: &Path| {
        let text = p.to_string_lossy().replace('\\', "/");
        #[cfg(windows)]
        {
            text.to_lowercase()
        }
        #[cfg(not(windows))]
        {
            text.to_string()
        }
    };
    let child = normalize(path);
    let root = normalize(root);
    let root = root.trim_end_matches('/');
    child == root || child.starts_with(&format!("{root}/"))
}

/// 从清单文件取「版本 + 声明的外部 jar」。
///
/// 清单坏了解析不出来就给空——坏包会走自己的错误路径，扫描不该跟着炸。
fn manifest_side(manifest_path: &Path) -> (String, Vec<String>) {
    match crate::manifest::ManifestParser::parse(manifest_path) {
        Ok(parsed) => (
            parsed.plugin.version.clone(),
            parsed.backend.map(|backend| backend.driver_jars).unwrap_or_default(),
        ),
        Err(_) => (String::new(), Vec::new()),
    }
}

/// 扫描每个插件时决定「版本 + 外部 jar」从哪来：快照里清单 hash 相同就直接复用，
/// 否则读文件解析。
///
/// 抽成自由函数是为了让这条捷径能被单测直接钉住（传一个不存在的清单路径 + 相同
/// hash 的快照条目，若实现偷偷解析文件就会拿到空值）。
fn resolve_manifest_side(
    previous: Option<&ScanEntry>,
    manifest_hash: u64,
    manifest_path: &Path,
) -> (String, Vec<String>) {
    match previous {
        Some(prev) if prev.manifest_hash == manifest_hash => {
            (prev.version.clone(), prev.driver_jars.clone())
        }
        _ => manifest_side(manifest_path),
    }
}

/// 插件**载荷**的指纹：插件目录内文件 + 清单声明的外部 jar，都按
/// (标识, 长度, 纳秒修改时间) 汇总成 hash。
///
/// 为什么要带外部路径：清单的 `backend.driver_jars` 是**绝对路径**（用户自己下载的
/// JDBC 包，不塞进插件目录），换 jar 时插件目录与 `plugin.toml` 都不动。只管目录的话
/// 这种更新检测不到，运行中的 Java 壳会一直用已加载的旧 jar。
///
/// 为什么不用内容 hash：扫描是每 5 秒一轮的后台动作，3 MB 的 exe 每轮读一遍纯属浪费；
/// (长度, 修改时间) 已经覆盖真实更新方式——重编覆盖、重装、换文件、删文件都会变，
/// 且只做 stat、不读内容。
///
/// 边界（写在这里免得后人误以为它是全保真）：
/// - 用保留时间戳的工具覆盖（`robocopy /COPY:T`、`git checkout` 保时间）指纹不变，
///   那种情况仍要手动重载；
/// - 目录整体被替换成另一份（长度与时间都恰好相同）属于低概率巧合，忽略；
/// - 遍历失败的子目录直接跳过（正在写入的目录下一轮再算）。
fn payload_fingerprint(dir: &Path, extra_paths: &[String]) -> u64 {
    use std::hash::Hasher;

    fn mtime_nanos(meta: &std::fs::Metadata) -> u128 {
        // 纳秒粒度：同一秒内两次写入、且长度恰好相同的覆盖（比如只改一个字符）
        // 也能被认出来；秒级粒度在这种场景下会漏判。
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    }

    /// `prefix` 让外部路径的文件与目录内文件不会撞名（`ext:<路径>#`）
    fn walk(dir: &Path, root: &Path, prefix: &str, out: &mut Vec<(String, u64, u128)>, depth: usize) {
        // 深度兜底：插件包约定是浅目录，异常深的树（被人塞了 node_modules 之类）
        // 不必每 5 秒全部 stat 一遍
        if depth > 6 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                walk(&path, root, prefix, out, depth + 1);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| path.to_string_lossy().to_string());
            out.push((format!("{prefix}{rel}"), meta.len(), mtime_nanos(&meta)));
        }
    }

    let mut files = Vec::new();
    walk(dir, dir, "", &mut files, 0);
    for raw in extra_paths {
        let path = Path::new(raw);
        match std::fs::metadata(path) {
            // 目录（清单允许写 `/drivers/mysql` 这种目录）→ 递归计入其中每个文件，
            // 否则「往目录里加/换一个 jar」也检测不到
            Ok(meta) if meta.is_dir() => {
                walk(path, path, &format!("ext:{raw}#"), &mut files, 0)
            }
            Ok(meta) => files.push((format!("ext:{raw}"), meta.len(), mtime_nanos(&meta))),
            // 不存在的也记一条：这样「后来才放进去」的 jar 也能被认出来。
            // 注意部署期本就要求路径存在（不存在报可读错误），这里只是兜底。
            Err(_) => files.push((format!("ext-missing:{raw}"), 0, 0)),
        }
    }
    // 路径排序：`read_dir` 顺序不保证稳定，不排序会让同一份目录算出不同指纹
    files.sort();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (rel, len, mtime) in &files {
        hasher.write(rel.as_bytes());
        hasher.write_u64(*len);
        hasher.write_u128(*mtime);
    }
    hasher.finish()
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
            failed_loads: RwLock::new(HashMap::new()),
            plugin_index: Arc::new(RwLock::new(HashMap::new())),
            scan_snapshot: RwLock::new(HashMap::new()),
            needs_reload: RwLock::new(Vec::new()),
            registered_drivers: RwLock::new(None),
            dependents: RwLock::new(None),
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

        let dirs = self
            .plugin_dirs
            .read()
            .map_err(|_| lock_err("plugin_dirs"))?;

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
            error: None,
            drivers: PluginDriverSummary::from_manifest(&manifest),
            registered_drivers: self.driver_registrations(&manifest),
            backend_kind: manifest.backend.as_ref().map(|backend| backend.kind),
            runtime: manifest
                .backend
                .as_ref()
                .and_then(|backend| backend.runtime),
            permissions: manifest.permissions.clone(),
            contribution_items: PluginContributionSummary::from_manifest(&manifest),
            install_path: Some(path.to_path_buf()),
            dependencies: manifest
                .dependencies
                .iter()
                .map(|dependency| dependency.id.clone())
                .collect(),
            dependents: self.dependents_of(&manifest.plugin.id),
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
                crate::manifest::BackendKind::Sidecar | crate::manifest::BackendKind::Script => {
                    return Ok(PluginKind::Sidecar);
                }
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

    /// 记下「连清单都没读出来」的坏目录，供面板展示 / 卸载。
    ///
    /// 目录名先过 [`validate_plugin_id`]：面板的卸载按钮按 id 拼 `install_dir/<id>`，
    /// 不合法的目录名既不能安全删、也不该出现在可操作列表里。
    fn record_failed_load(&self, plugin_dir: &Path, error: &CoreError) {
        let Some(name) = plugin_dir.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        if validate_plugin_id(name).is_err() {
            return;
        }
        if let Ok(mut map) = self.failed_loads.write() {
            map.insert(name.to_string(), format!("{error}"));
        }
    }

    /// 成功加载后清掉同名的历史失败记录（修好清单 / 目录名后不该再显示错误行）。
    fn clear_failed_load(&self, id: &str) {
        if let Ok(mut map) = self.failed_loads.write() {
            map.remove(id);
        }
    }

    /// 加载前检查候选清单与已在会话里的健康插件的贡献点是否撞车。
    ///
    /// 跨插件冲突若推迟到 `contributions()` 查询时才暴露，两个插件都会显示「已加载」，
    /// 但注册表整体不可用；这里在第二个插件加载时就拒绝，坏包以错误行留在列表里，
    /// 用户能看到原因并卸载。同名插件（重载 / 覆盖）先排除自身，避免自己和自己冲突。
    fn check_contribution_conflicts(&self, candidate: &PluginManifest) -> Result<(), CoreError> {
        let loaded = self
            .loaded_plugins
            .read()
            .map_err(|_| lock_err("loaded_plugins"))?;
        let mut manifests: Vec<&PluginManifest> = loaded
            .values()
            .filter(|plugin| {
                plugin.id != candidate.plugin.id && plugin.status.error_reason().is_none()
            })
            .map(|plugin| &plugin.manifest)
            .collect();
        manifests.push(candidate);
        // HashMap 遍历顺序不稳定；按插件 id 排一下，冲突文案里的「先声明者」才可复现。
        manifests.sort_by(|a, b| a.plugin.id.cmp(&b.plugin.id));
        // 驱动 id 也是全局唯一键（连接对话框按 id 选驱动），但 `ContributionRegistry`
        // 只登记 commands / panels / settings —— 不在这里挡，两个插件会都显示「已加载」，
        // 后加载者把先加载者的 `drivers` 行与工厂槽位覆盖掉。
        let mut driver_sources: HashMap<&str, &str> = HashMap::new();
        for manifest in &manifests {
            for driver in &manifest.contributes.drivers {
                if let Some(previous) =
                    driver_sources.insert(driver.id.as_str(), manifest.plugin.id.as_str())
                    && previous != manifest.plugin.id.as_str()
                {
                    return Err(CoreError::common(CommonError::General(format!(
                        "插件加载被拒绝：驱动 id `{}` 已被插件 `{previous}` 占用，`{}` 不能重复声明",
                        driver.id, manifest.plugin.id
                    ))));
                }
            }
        }
        ContributionRegistry::build(manifests).map_err(|error| {
            CoreError::common(CommonError::General(format!("插件加载被拒绝：{error}")))
        })?;
        Ok(())
    }

    /// 从目录加载单个插件（解析清单、校验、入缓存；**不启动运行时**）。
    ///
    /// 清单 `plugin.id` 由第三方给出，进 `install_path` / 索引前必须先过
    /// [`validate_plugin_id`]（路径注入防护）。
    pub async fn load_plugin_from_dir(&self, plugin_dir: &Path) -> Result<LoadedPlugin, CoreError> {
        let manifest_path = plugin_dir.join("plugin.toml");
        if !manifest_path.exists() {
            let err = CoreError::common(CommonError::General(format!(
                "Plugin manifest not found at {}",
                manifest_path.display()
            )));
            self.record_failed_load(plugin_dir, &err);
            return Err(err);
        }

        let manifest = match ManifestParser::parse(&manifest_path) {
            Ok(manifest) => manifest,
            Err(e) => {
                self.record_failed_load(plugin_dir, &e);
                return Err(e);
            }
        };
        if let Err(e) = validate_plugin_id(&manifest.plugin.id) {
            let err = CoreError::common(CommonError::General(format!(
                "插件 id 不合法（拼路径前必须校验）: {}",
                e
            )));
            self.record_failed_load(plugin_dir, &err);
            return Err(err);
        }
        // 驱动 id 撞内置：表 upsert 与运行时注册都会覆盖内置驱动，加载期就拒
        //（与贡献点跨插件冲突同一风格：错误行留列表，修清单可重试）。
        if let Err(e) = crate::driver_decl::check_builtin_collisions(&manifest) {
            self.record_failed_load(plugin_dir, &e);
            return Err(e);
        }

        // 目录名必须等于清单 id（见 `layout` 模块文档）：热插拔 diff 按目录名索引、
        // 卸载按 id 删 `install_dir/<id>`，不一致会出现「加载成功但 diff 认不出、
        // 卸载删不到目录」。
        crate::layout::validate_dir_name(plugin_dir, &manifest.plugin.id).map_err(|e| {
            self.record_failed_load(plugin_dir, &e);
            e
        })?;

        // 运行期完整性：带 `checksums.json` 的包在加载时再校验一遍。安装期校验的是
        // 解压产物，装好之后仍可能被改（热插拔直接拷目录、用户手改文件），只验安装期
        // 等于只验了「第一次」。不带该文件的老包 / 开发目录照旧放行。
        //
        // 形态判定提到校验之前：校验失败也要把插件留在缓存里（`LoadFailed`），
        // 否则面板列表里根本没有这一行，用户既看不到原因、也无法从面板卸载坏包。
        let kind = match self.detect_plugin_kind(&manifest) {
            Ok(kind) => kind,
            Err(e) => {
                self.record_failed_load(plugin_dir, &e);
                return Err(e);
            }
        };
        if let Err(e) = crate::checksums::verify_package(&manifest, plugin_dir) {
            let plugin_id = manifest.plugin.id.clone();
            self.clear_failed_load(&plugin_id);
            let failed = LoadedPlugin {
                id: plugin_id.clone(),
                manifest,
                install_path: plugin_dir.to_path_buf(),
                status: LoadStatus::LoadFailed(e.to_string()),
            };
            if let Ok(mut index) = self.plugin_index.write() {
                index.insert(plugin_id.clone(), kind);
            }
            if let Ok(mut map) = self.loaded_plugins.write() {
                map.insert(plugin_id, failed);
            }
            return Err(e);
        }

        // 跨插件贡献点冲突在加载期就拒绝：否则两个插件都显示「已加载」，
        // 直到宿主查注册表才整体报错，用户不知道该停用谁。
        if let Err(e) = self.check_contribution_conflicts(&manifest) {
            self.record_failed_load(plugin_dir, &e);
            return Err(e);
        }

        let plugin_id = manifest.plugin.id.clone();
        self.clear_failed_load(&plugin_id);
        let loaded_plugin = LoadedPlugin {
            id: plugin_id.clone(),
            manifest,
            install_path: plugin_dir.to_path_buf(),
            status: LoadStatus::Loaded,
        };

        // 同步登记形态索引：热插拔新增路径只走这里（不经 load_plugin），
        // 不登记的话 activate 会 not_found、removed 分支会跳过 unload（曾静默失败）
        self.plugin_index
            .write()
            .map_err(|_| lock_err("plugin_index"))?
            .insert(plugin_id.clone(), kind);

        self.loaded_plugins
            .write()
            .map_err(|_| lock_err("loaded_plugins"))?
            .insert(plugin_id, loaded_plugin.clone());

        // 加载即同步驱动行（enabled=0 的置灰行）：热插拔只 load 不 activate 的目录
        // 也要在连接对话框里出现；激活成功后 activate_plugin 会再同步一次翻成 enabled
        self.resync_sidecar_driver_rows().await;

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

    /// 依赖未就绪（缺失 / 已装未启用）时拒绝继续，一次列出所有 `id@version`。
    ///
    /// 启用（写 `is_enabled` 前）与激活（真正拉起运行时前）共用这一道判定：
    /// 前者防止 DB 先翻成启用造成假成功，后者兜住重试 / 热插拔等不经 `enable_plugin` 的路径。
    async fn ensure_dependencies_ready(&self, manifest: &PluginManifest) -> Result<(), CoreError> {
        let Some(db) = engine::migration::get_global_db_manager() else {
            return Ok(());
        };
        let resolution = crate::dependency::DependencyManager::new(db)
            .resolve_dependencies(manifest)
            .await?;
        let mut blocked: Vec<String> = resolution
            .to_install
            .iter()
            .chain(resolution.missing.iter())
            .map(|d| format!("{}@{}", d.id, d.version))
            .collect();
        blocked.sort();
        blocked.dedup();
        if blocked.is_empty() {
            return Ok(());
        }
        Err(CoreError::plugin(PluginError::DependencyMissing {
            plugin_id: manifest.plugin.id.clone(),
            dep_code: String::new(),
            dep_version: format!("以下依赖未安装或未启用，请先处理: {}", blocked.join(", ")),
        }))
    }

    /// 激活插件：
    ///
    /// - `Wasm`：读 wasm 字节 → 加载进 Extism 运行时 → 调 `activate` 钩子（幂等）；
    /// - `Sidecar`：部署到 supervisor 并把声明的驱动注册进 `DriverRegistry`（幂等覆盖）；
    /// - `Frontend`：只更新状态（扩展宿主进程是 P3/P4 的事，现在没有运行时）。
    pub async fn activate_plugin(&self, id: &str) -> Result<(), CoreError> {
        // 激活是所有运行时的共同入口（启用、重试、热插拔都走到这里），
        // 依赖未就绪时不能把状态标成 Active——否则列表看起来正常、实际缺底座。
        let manifest = self
            .loaded_plugins
            .read()
            .map_err(|_| lock_err("loaded_plugins"))?
            .get(id)
            .map(|p| p.manifest.clone());
        if let Some(manifest) = manifest {
            if let Err(e) = self.ensure_dependencies_ready(&manifest).await {
                let mut map = self
                    .loaded_plugins
                    .write()
                    .map_err(|_| lock_err("loaded_plugins"))?;
                if let Some(p) = map.get_mut(id) {
                    p.status = LoadStatus::ActivationFailed(e.to_string());
                }
                return Err(e);
            }
        }

        // 状态机：先置 Activating，结果回填 Active / ActivationFailed
        {
            let mut map = self
                .loaded_plugins
                .write()
                .map_err(|_| lock_err("loaded_plugins"))?;
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
            // 失败目录只进列表、不进形态索引；走到这里说明索引被写坏了
            PluginKind::Unknown => Err(CoreError::common(CommonError::Internal(format!(
                "无法判定插件形态（清单未成功解析）: {id}"
            )))),
        };

        {
            let mut map = self
                .loaded_plugins
                .write()
                .map_err(|_| lock_err("loaded_plugins"))?;
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
        }
        // 守卫已随块结束释放（async Send 分析不吃 drop()，只认作用域边界）

        crate::events::emit_plugin_activated(id);
        // 状态已翻转（Active）再同步驱动行：行上的 enabled 跟随激活态
        self.resync_sidecar_driver_rows().await;
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

        // 路径走统一的目录约定闸（相对 / POSIX `/` / 不许 `..`），
        // 不再直接 `install_path.join(entry)` —— 后者允许 `../../x.wasm` 读到目录外。
        let wasm_path = crate::layout::resolve_asset(
            &loaded.install_path,
            "capabilities.wasm.entry",
            &wasm_entry,
        )?;
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

        // 激活即「这个包能跑」的承诺：声明的入口（backend.executable / wasm / 前端模块）
        // 必须在包里真实存在。部署与拉起是**按需**的（首个会话才 spawn），不做这道守门时
        // 缺可执行文件的坏包会被标成 Active，用户要等到第一次连库才看到莫名其妙的启动失败。
        // 与安装期 `check_declared_entries` 同一口径，但激活时再查一次：目录可能被外部改过。
        crate::layout::check_declared_entries(&loaded.manifest, &loaded.install_path)?;

        register_sidecar_drivers(
            Arc::clone(&self.sidecar),
            &loaded.manifest,
            &loaded.install_path,
        )
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
            // 解析失败的目录没有运行时实例，停用是空操作
            PluginKind::Unknown => {}
        }

        {
            let mut map = self
                .loaded_plugins
                .write()
                .map_err(|_| lock_err("loaded_plugins"))?;
            if let Some(p) = map.get_mut(id) {
                p.status = LoadStatus::Loaded;
            }
        }
        // 守卫已随块结束释放（async Send 分析不吃 drop()，只认作用域边界）

        crate::events::emit_plugin_deactivated(id);
        // 停用后行要置灰（enabled=0），状态翻转后再同步
        self.resync_sidecar_driver_rows().await;
        Ok(())
    }

    /// 把「当前所有已加载插件声明的驱动」全量重算进 global.db 的 `drivers` 表
    ///（连接对话框驱动目录的唯一数据源；`driver_decl` 模块头有背景）。
    ///
    /// 失败只记日志：可见性同步不该反过来拦生命周期操作；全局库未初始化时
    ///（测试宿主 / 启动早期）`driver_decl::resync_driver_rows` 自己保守放行。
    pub async fn resync_sidecar_driver_rows(&self) {
        let entries: Vec<(String, crate::manifest::PluginManifest, bool)> = {
            let Ok(loaded) = self.loaded_plugins.read() else {
                return;
            };
            loaded
                .iter()
                .map(|(id, lp)| {
                    (
                        id.clone(),
                        lp.manifest.clone(),
                        matches!(lp.status, LoadStatus::Active),
                    )
                })
                .collect()
        };
        match crate::driver_decl::resync_driver_rows(&entries).await {
            Ok((written, deleted)) => {
                if written > 0 || deleted > 0 {
                    tracing::info!("插件驱动行同步：写 {written} 行，清理 {deleted} 行");
                }
            }
            Err(e) => {
                tracing::warn!("插件驱动行同步失败（连接对话框可能看不到插件驱动）：{e}")
            }
        }
        // 驱动行同步失败也要刷新：登记状态 / 反向依赖都是「展示事实」，
        // 不能因为另一个写库动作失败就长期停在「未知」。
        self.refresh_registered_driver_cache().await;
        self.refresh_dependents_cache().await;
    }

    /// 从 `global.db` 的 `drivers` 表刷新 sidecar 行缓存（失败保留上一份快照）。
    async fn refresh_registered_driver_cache(&self) {
        let Some(db) = engine::migration::get_global_db_manager() else {
            return;
        };
        match db.get_all_drivers().await {
            Ok(rows) => {
                let snapshot: HashMap<String, bool> = rows
                    .into_iter()
                    .filter(|driver| driver.driver_kind == "sidecar")
                    .map(|driver| (driver.id, driver.enabled))
                    .collect();
                if let Ok(mut cache) = self.registered_drivers.write() {
                    *cache = Some(snapshot);
                }
            }
            Err(e) => {
                tracing::warn!("读取已登记插件驱动失败（管理页登记状态可能过期）：{e}");
            }
        }
    }

    /// 刷新反向依赖快照（全局库不可用 / 读取失败时保留上一份）。
    async fn refresh_dependents_cache(&self) {
        let Some(db) = engine::migration::get_global_db_manager() else {
            return;
        };
        match crate::dependency::DependencyManager::new(db)
            .all_dependents()
            .await
        {
            Ok(snapshot) => {
                if let Ok(mut cache) = self.dependents.write() {
                    *cache = Some(snapshot);
                }
            }
            Err(e) => {
                tracing::warn!("读取插件反向依赖失败（管理页「项目引用」可能过期）：{e}");
            }
        }
    }

    /// 该插件的反向依赖（`None` = 全局库未初始化，无法判定）。
    fn dependents_of(&self, plugin_id: &str) -> Option<Vec<String>> {
        let cache = self.dependents.read().ok()?;
        let cache = cache.as_ref()?;
        Some(cache.get(plugin_id).cloned().unwrap_or_default())
    }

    /// 清单声明驱动与登记缓存按 id 对齐（未登记的驱动不出现在结果里）。
    fn driver_registrations(
        &self,
        manifest: &PluginManifest,
    ) -> Option<Vec<PluginDriverRegistration>> {
        let cache = self.registered_drivers.read().ok()?;
        let cache = cache.as_ref()?;
        Some(
            manifest
                .contributes
                .drivers
                .iter()
                .filter_map(|driver| {
                    cache
                        .get(&driver.id)
                        .map(|enabled| PluginDriverRegistration {
                            id: driver.id.clone(),
                            enabled: *enabled,
                        })
                })
                .collect(),
        )
    }

    /// 卸载插件：先停用，再从运行时 / 缓存 / 索引移除。
    /// 该插件当前是否有运行时实例（在插件索引里）。
    ///
    /// service 层只应在有实例时调 `deactivate_plugin` / `unload_plugin`：只有登记行、
    /// 没有实例时这些调用会返回 not_found，旧代码用 `let _ =` 吞掉，反而掩盖真正的
    /// 停用失败（DB 已被改成停用，进程却还活着）。
    pub fn is_loaded(&self, id: &str) -> bool {
        self.plugin_index
            .read()
            .map(|index| index.contains_key(id))
            .unwrap_or(false)
    }

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
        if self.is_loaded(id) {
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

        // 4. 清 global.db 的 plugin_store 记录（DB 未初始化时跳过；按 code 删，uuid 删不到）
        if let Some(db) = engine::migration::get_global_db_manager() {
            db.delete_plugin_by_code(id).await?;
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
        // 卸载后清掉待重载标记与扫描快照：否则面板还会提示重载一个已删除的插件，
        // 下一轮扫描也会把它当成「又一次消失」。
        self.needs_reload
            .write()
            .map_err(|_| lock_err("needs_reload"))?
            .retain(|plugin_id| plugin_id != id);
        self.scan_snapshot
            .write()
            .map_err(|_| lock_err("scan_snapshot"))?
            .remove(id);
        // 坏包目录也走同一条卸载路径，清掉失败记录（否则面板会留下一条幽灵错误行）
        self.clear_failed_load(id);

        // 6. 事件
        crate::events::emit_plugin_uninstalled(id);
        // 卸载后清掉它留下的驱动行（全量重算会把不在存活集合里的 sidecar 行删掉）
        self.resync_sidecar_driver_rows().await;
        Ok(())
    }

    /// 禁用插件（VS Code 口径：持久标记，启动不激活，清单仍可见）。
    ///
    /// 权威状态 = global.db `plugin_store.is_enabled`（启动扫描 / 未来插件面板按它决定
    /// 要不要激活）。**运行中禁用 = 立刻停用**（sidecar 分支会停进程），不是等下次启动。
    pub async fn disable_plugin(&self, id: &str) -> Result<(), CoreError> {
        // 停用被依赖的插件会让依赖方立即失去驱动；面板侧已做前置拦截，但这里
        // 是权威入口（自动停用 / 其它调用方），必须自己查一遍，不能只靠 UI。
        if let Some(db) = engine::migration::get_global_db_manager() {
            let dependents = crate::dependency::DependencyManager::new(db)
                .check_dependents(id)
                .await?;
            if !dependents.is_empty() {
                return Err(CoreError::plugin(PluginError::DeactivationFailed {
                    plugin_id: id.to_string(),
                    reason: format!(
                        "被以下插件依赖，请先停用或卸载它们: {}",
                        dependents.join(", ")
                    ),
                }));
            }
        }
        // 先停运行时，再写停用标记：停不下来时保持 enabled，避免
        // DB 已显示停用、进程却还在跑。
        if self.is_loaded(id) {
            self.deactivate_plugin(id).await?;
        }
        if let Some(db) = engine::migration::get_global_db_manager() {
            // 运行时身份是清单 id（= code），不是安装实例 uuid；按 id 写会 0 行命中
            db.update_plugin_enabled_by_code(id, false).await?;
        }
        crate::events::emit_plugin_disabled(id);
        Ok(())
    }

    /// 重试加载一个失败插件（错误行上的「重试」入口）。
    ///
    /// 冲突拒绝发生在清单**没变**的情况下：即使冲突方后来被卸载，热插拔扫描也不会
    /// 把它当「新增 / 变化」自动重试。这里给用户一个显式动作：重新解析清单、复核
    /// checksums、再试建注册表；成功后按 DB 的启用状态决定是否激活，并清掉变更标记。
    pub async fn retry_load_plugin(&self, id: &str) -> Result<LoadedPlugin, CoreError> {
        validate_plugin_id(id).map_err(|e| {
            CoreError::common(CommonError::Internal(format!(
                "插件 id 不合法，无法重试: {id} ({e})"
            )))
        })?;
        let plugin_dir = self.install_dir.join(id);
        if !plugin_dir.is_dir() {
            return Err(CoreError::common(CommonError::Internal(format!(
                "插件目录不存在，无法重试: {}",
                plugin_dir.display()
            ))));
        }

        let loaded = self.load_plugin_from_dir(&plugin_dir).await?;
        self.needs_reload
            .write()
            .map_err(|_| lock_err("needs_reload"))?
            .retain(|plugin_id| plugin_id != id);

        let enabled = match engine::migration::get_global_db_manager() {
            Some(db) => match db.get_plugin_by_code(id).await {
                Ok(Some(plugin)) => plugin.is_enabled,
                Ok(None) => {
                    // 目录直接拷进来的插件没有登记行：补登记前先确认依赖就绪，
                    // 否则会先写一行 is_enabled=1，再被 activate_plugin 拒绝，留下假启用。
                    if let Err(e) = self.ensure_dependencies_ready(&loaded.manifest).await {
                        let mut map = self
                            .loaded_plugins
                            .write()
                            .map_err(|_| lock_err("loaded_plugins"))?;
                        if let Some(p) = map.get_mut(id) {
                            p.status = LoadStatus::ActivationFailed(e.to_string());
                        }
                        return Err(e);
                    }
                    // 重试成功后补登记，否则之后的停用 / 卸载按 code 写库命中 0 行，
                    // 重启按「无记录=启用」复活。
                    if let Err(e) = crate::installer::register_manifest(
                        &loaded.manifest,
                        &loaded.install_path,
                        db,
                        true,
                    )
                    .await
                    {
                        tracing::warn!(plugin_id = %id, error = %e, "重试加载后登记失败");
                    }
                    true
                }
                Err(e) => {
                    tracing::warn!(plugin_id = %id, error = %e, "重试读取插件登记失败，按启用处理");
                    true
                }
            },
            None => true,
        };
        if enabled {
            self.activate_plugin(id).await?;
        }
        Ok(loaded)
    }

    /// 启用插件（与 [`Self::disable_plugin`] 对称）：持久标记 + 若已加载则重新激活。
    pub async fn enable_plugin(&self, id: &str) -> Result<(), CoreError> {
        // 坏包只以错误行留在列表里，没有可激活的运行时；先拒绝并说清原因，
        // 不要把 DB 标记翻成启用却什么都没发生（面板上也别给「启用」）。
        if self
            .failed_loads
            .read()
            .map_err(|_| lock_err("failed_loads"))?
            .contains_key(id)
        {
            return Err(CoreError::common(CommonError::Internal(format!(
                "插件清单损坏，无法启用（请先修复或卸载）: {id}"
            ))));
        }
        if let Some(db) = engine::migration::get_global_db_manager() {
            // 启用前确认声明依赖已安装且已启用：否则 DB 会先翻成启用，
            // 实际激活却缺底座（依赖方随后报错，用户只看到「已启用」）。
            let manifest = self
                .loaded_plugins
                .read()
                .map_err(|_| lock_err("loaded_plugins"))?
                .get(id)
                .map(|p| p.manifest.clone());
            if let Some(manifest) = manifest {
                self.ensure_dependencies_ready(&manifest).await?;
            }
            db.update_plugin_enabled_by_code(id, true).await?;
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
        // 读快照算 diff（快照只读、不跨 await 持锁：std RwLockGuard 非 Send，
        // 跨 await 会让 tokio::spawn 的循环 future 不 Send）。
        // 提前到扫描之前读：扫描时要靠它判断「清单没变，可以复用上次解析结果」。
        let snapshot = self
            .scan_snapshot
            .read()
            .map_err(|_| lock_err("scan_snapshot"))?
            .clone();

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
                let manifest_hash = hasher.finish();
                // 清单 hash 没变就复用上次解析出来的版本与外部 jar：内容相同 ⇒
                // `driver_jars` 必然相同，没必要每个 tick 重新解析 TOML 再全量校验
                // （5 秒一轮 × 每个插件，纯浪费）。清单变了才真读一遍。
                let (version, driver_jars) =
                    resolve_manifest_side(snapshot.get(&id), manifest_hash, &manifest_path);
                disk.insert(
                    id,
                    ScanEntry {
                        version,
                        manifest_hash,
                        payload_hash: payload_fingerprint(&entry.path(), &driver_jars),
                        driver_jars,
                    },
                );
            }
        }

        let mut diff = PluginDiff::default();
        let mut payload_changed: Vec<String> = Vec::new();
        for (id, entry) in &disk {
            match snapshot.get(id) {
                None => diff.added.push(id.clone()),
                // 清单变了优先（可能同时改了能力 / 权限 / 载荷）：清单变化要人工重载确认
                Some(prev) if prev.manifest_hash != entry.manifest_hash => {
                    diff.changed.push(id.clone())
                }
                // 壳没变、载荷变了：重编 exe / 换 wasm / 改 py 脚本就是这一种，
                // 先记下来，等本轮 diff 收尾时自动重启（只有它不需要人工介入）。
                Some(prev) if prev.payload_hash != entry.payload_hash => {
                    payload_changed.push(id.clone())
                }
                Some(_) => {}
            }
        }
        for id in snapshot.keys() {
            if !disk.contains_key(id) {
                diff.removed.push(id.clone());
            }
        }

        // 新增：先加载并登记全部，再统一激活——依赖可能也在本轮新增，
        // 边扫边激活会因目录遍历顺序把后到的依赖判成缺失。
        let mut pending_activation = Vec::new();
        for id in &diff.added {
            let loaded = match self.load_plugin_from_dir(&self.install_dir.join(id)).await {
                Ok(loaded) => loaded,
                Err(e) => {
                    tracing::warn!(plugin_id = %id, error = %e, "热插拔加载失败");
                    continue;
                }
            };
            let (enabled, newly_registered) = match engine::migration::get_global_db_manager() {
                Some(db) => match db.get_plugin_by_code(id).await {
                    Ok(Some(p)) => (p.is_enabled, false),
                    Ok(None) => {
                        // 目录直接拷进来的插件没有登记行：先补登记，否则停用/卸载按 code
                        // 写库命中 0 行，重启后又按「DB 无记录 = 启用」复活。
                        // 依赖未就绪时登记为停用：先写 is_enabled=1 再被 activate 拒绝，
                        // 用户会看到「已启用」而运行时从未成功；第二阶段依赖补齐再翻回启用。
                        let ready = self
                            .ensure_dependencies_ready(&loaded.manifest)
                            .await
                            .is_ok();
                        let registered = match crate::installer::register_manifest(
                            &loaded.manifest,
                            &loaded.install_path,
                            db,
                            ready,
                        )
                        .await
                        {
                            Ok(_) => true,
                            Err(e) => {
                                tracing::warn!(plugin_id = %id, error = %e, "热插拔登记失败");
                                false
                            }
                        };
                        (ready, registered)
                    }
                    Err(e) => {
                        tracing::warn!(plugin_id = %id, error = %e, "读取插件登记失败，按启用处理");
                        (true, false)
                    }
                },
                None => (true, false),
            };
            if enabled || newly_registered {
                pending_activation.push((id.clone(), newly_registered, loaded.manifest.clone()));
            }
        }
        // 第二阶段：所有新增行都落库后再校验依赖并激活，依赖解析能看到本轮新增的底座。
        for (id, newly_registered, manifest) in pending_activation {
            if let Err(e) = self.ensure_dependencies_ready(&manifest).await {
                {
                    let mut map = self
                        .loaded_plugins
                        .write()
                        .map_err(|_| lock_err("loaded_plugins"))?;
                    if let Some(p) = map.get_mut(&id) {
                        p.status = LoadStatus::ActivationFailed(e.to_string());
                    }
                }
                // 刚登记的行保持停用，不能留下「已启用但从未激活」的假状态。
                if newly_registered {
                    if let Some(db) = engine::migration::get_global_db_manager() {
                        if let Err(db_err) = db.update_plugin_enabled_by_code(&id, false).await {
                            tracing::warn!(plugin_id = %id, error = %db_err, "撤回热插拔启用失败");
                        }
                    }
                }
                tracing::warn!(plugin_id = %id, error = %e, "热插拔激活失败");
                continue;
            }
            if newly_registered {
                if let Some(db) = engine::migration::get_global_db_manager() {
                    if let Err(e) = db.update_plugin_enabled_by_code(&id, true).await {
                        tracing::warn!(plugin_id = %id, error = %e, "补登记转启用失败");
                    }
                }
            }
            if let Err(e) = self.activate_plugin(&id).await {
                tracing::warn!(plugin_id = %id, error = %e, "热插拔激活失败");
            }
        }

        // 消失：停用 + 移出运行时索引（unload_plugin 不碰磁盘，目录已没了也安全）；
        // 顺带清掉该插件的重载标记（插件都没了，提示重载没有意义）
        for id in &diff.removed {
            // 坏目录被手动删掉时也要清掉错误行（它本来就不在形态索引里）
            self.clear_failed_load(id);
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
        // 兜底：目录在管理器启动前就被删掉的行从没进过快照，`diff.removed` 覆盖不到。
        self.prune_missing_registrations().await;

        // 载荷变化（清单没变）：自动重启这一个插件，让它换上新二进制 / wasm / 脚本。
        // 这是「重编驱动后运行中的 sidecar 还在跑旧镜像」的正解：不重启宿主、不断别的插件。
        // 没在跑的插件不用动——本轮快照会更新成新指纹，下次启用时加载的就是新载荷。
        for id in payload_changed {
            if !self.is_loaded(&id) {
                tracing::debug!(plugin_id = %id, "插件未加载，载荷变化只更新指纹");
                continue;
            }
            match self.restart_plugin(&id).await {
                Ok(()) => {
                    tracing::info!(plugin_id = %id, "插件载荷已更新，已自动重启该插件");
                    diff.reloaded.push(id);
                }
                // 重启失败不能悄悄留在旧镜像上：退回 needs_reload，面板至少能提示
                Err(e) => {
                    tracing::warn!(plugin_id = %id, error = %e, "载荷更新后自动重启失败，转为待重载");
                    diff.changed.push(id);
                }
            }
        }

        // 变化：置 needs_reload（更新不热生效，VS Code 口径），UI 层读它提示重载。
        // 每次**新进入**待重载集合时发一条事件：面板订阅后能立刻看到「重载 N」，
        // 不必等用户手动刷新；同一插件在未重载前不再重复发（扫描每 5s 一轮）。
        for id in &diff.changed {
            let newly_marked = {
                let mut pending = self
                    .needs_reload
                    .write()
                    .map_err(|_| lock_err("needs_reload"))?;
                if pending.contains(id) {
                    false
                } else {
                    pending.push(id.clone());
                    true
                }
            };
            if newly_marked {
                crate::events::emit_plugin_changed(id);
            }
        }

        *self
            .scan_snapshot
            .write()
            .map_err(|_| lock_err("scan_snapshot"))? = disk;
        Ok(diff)
    }

    /// 清理「目录已不存在」的插件登记行（`plugins` 表）。
    ///
    /// `diff.removed` 只覆盖「上一轮快照里出现过、这一轮消失」的目录；如果目录在管理器
    /// 启动前就被删掉，它从没进过快照，登记行会永久留在 `plugins` 表——旧
    /// `rds.driver.oracle` 就是这种：目录不存在、`is_enabled` 仍为 1，面板还会把它当成
    /// 一个可启用插件。这里按 `install_path` 兜底，只处理安装根目录下、磁盘确实不存在、
    /// 且当前未加载的非内置行；目录存在但清单坏掉的行不碰（那是「可修复」而不是「已消失」）。
    async fn prune_missing_registrations(&self) {
        let Some(db) = engine::migration::get_global_db_manager() else {
            return;
        };
        let plugins = match db.get_all_plugins().await {
            Ok(plugins) => plugins,
            Err(e) => {
                tracing::warn!(error = %e, "读取插件登记失败，跳过幽灵行清理");
                return;
            }
        };
        for plugin in plugins {
            if plugin.is_builtin || plugin.install_path.trim().is_empty() {
                continue;
            }
            let path = PathBuf::from(&plugin.install_path);
            if !path_is_under(&path, &self.install_dir) || path.exists() {
                continue;
            }
            // 仍在运行时缓存里的先交给 `diff.removed` 的 unload 分支，下一轮再清登记。
            if self
                .loaded_plugins
                .read()
                .map(|map| map.contains_key(&plugin.code))
                .unwrap_or(true)
            {
                continue;
            }
            if let Err(e) = db.delete_plugin_by_code(&plugin.code).await {
                tracing::warn!(plugin_id = %plugin.code, error = %e, "清理幽灵插件登记失败");
            } else {
                tracing::info!(
                    plugin_id = %plugin.code,
                    install_path = %plugin.install_path,
                    "清理目录已缺失的插件登记"
                );
            }
        }
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

    /// 重启单个插件：停用（含停该插件全部 sidecar 进程）→ 重新激活。
    ///
    /// 触发场景是「清单没变、载荷变了」：重编 `agent.exe`、换 wasm、改 py 脚本。
    /// 旧行为下运行中的 sidecar 会一直用旧内存镜像，只有重连或重启宿主才换新；
    /// 现在由 [`Self::scan_and_diff_plugins`] 检测到后调这里。
    ///
    /// 与 [`Self::reload_plugins`] 的区别是**只碰这一个插件**：别的插件进程、别的
    /// 连接会话完全不受影响。代价是该插件上已打开的会话会断（进程真的停了），
    /// 这正是「换成新二进制」应有的代价，不额外弹提示。
    pub async fn restart_plugin(&self, id: &str) -> Result<(), CoreError> {
        if !self.is_loaded(id) {
            return Err(not_found(id));
        }
        self.deactivate_plugin(id).await?;
        self.activate_plugin(id).await?;
        // 已经吃到新版本，之前挂着的重载提示可以撤掉
        self.needs_reload
            .write()
            .map_err(|_| lock_err("needs_reload"))?
            .retain(|p| p != id);
        Ok(())
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
                .map_err(|e| {
                    CoreError::common(CommonError::Internal(format!(
                        "Failed to list plugins: {}",
                        e
                    )))
                })?
                .into_iter()
                .map(|(meta, state)| {
                    let dependents = self.dependents_of(&meta.id);
                    PluginInfo {
                        id: meta.id.clone(),
                        name: meta.name.clone(),
                        version: meta.version.clone(),
                        kind: PluginKind::Wasm,
                        metadata: meta,
                        state,
                        // Extism 侧只保存状态枚举，没有错误文本；wasm 失败原因仍看日志。
                        error: None,
                        // wasm 清单不声明驱动（驱动必须走 sidecar/script 形态）。
                        drivers: Vec::new(),
                        registered_drivers: None,
                        backend_kind: Some(BackendKind::Wasm),
                        runtime: None,
                        permissions: PluginPermissions::default(),
                        contribution_items: Vec::new(),
                        install_path: None,
                        dependencies: Vec::new(),
                        dependents,
                    }
                })
                .collect();
            plugins.extend(wasm_plugins);
        }

        // sidecar / frontend：形态索引里有、但不在 wasm 运行时里的
        let index = self
            .plugin_index
            .read()
            .map_err(|_| lock_err("plugin_index"))?;
        let loaded = self
            .loaded_plugins
            .read()
            .map_err(|_| lock_err("loaded_plugins"))?;
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
                    state: lp.status.session_state(),
                    error: lp.status.error_reason().map(str::to_string),
                    drivers: PluginDriverSummary::from_manifest(&lp.manifest),
                    registered_drivers: self.driver_registrations(&lp.manifest),
                    backend_kind: lp.manifest.backend.as_ref().map(|backend| backend.kind),
                    runtime: lp
                        .manifest
                        .backend
                        .as_ref()
                        .and_then(|backend| backend.runtime),
                    permissions: lp.manifest.permissions.clone(),
                    contribution_items: PluginContributionSummary::from_manifest(&lp.manifest),
                    install_path: Some(lp.install_path.clone()),
                    dependencies: lp
                        .manifest
                        .dependencies
                        .iter()
                        .map(|dependency| dependency.id.clone())
                        .collect(),
                    dependents: self.dependents_of(&lp.manifest.plugin.id),
                });
            }
        }

        // 连清单都没读出来的坏目录：没有 PluginManifest，只能按目录名合成一行，
        // 状态固定 Error，错误原因来自加载期记录。跳过已成功加载的同名条目。
        let failed = self
            .failed_loads
            .read()
            .map_err(|_| lock_err("failed_loads"))?;
        for (id, reason) in failed.iter() {
            if plugins.iter().any(|p| &p.id == id) {
                continue;
            }
            plugins.push(PluginInfo {
                id: id.clone(),
                name: id.clone(),
                version: "未知".to_string(),
                kind: PluginKind::Unknown,
                metadata: PluginMetadata {
                    id: id.clone(),
                    name: id.clone(),
                    version: "未知".to_string(),
                    plugin_type: PluginType::Analytics,
                    description: String::new(),
                    author: String::new(),
                    entry_point: String::new(),
                },
                state: PluginState::Error,
                error: Some(reason.clone()),
                drivers: Vec::new(),
                registered_drivers: None,
                backend_kind: None,
                runtime: None,
                permissions: PluginPermissions::default(),
                contribution_items: Vec::new(),
                install_path: {
                    let dir = paths::plugin_dir(id);
                    dir.exists().then_some(dir)
                },
                // 清单都没读出来，声明依赖无从谈起；反向依赖仍按目录名查一次。
                dependencies: Vec::new(),
                dependents: self.dependents_of(id),
            });
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

    /// 汇总当前已成功加载插件的贡献点。
    ///
    /// 加载失败 / 校验失败的插件不参与：它们的清单不可信，不能把命令或面板
    /// 挂到宿主上。输出按插件 id 排序，保证同一个插件集合每次查到的顺序一致
    /// （`HashMap` 的遍历顺序不稳定，宿主列表会莫名其妙重排）。
    pub fn contributions(&self) -> Result<ContributionRegistry, CoreError> {
        let loaded = self
            .loaded_plugins
            .read()
            .map_err(|_| lock_err("loaded_plugins"))?;
        let mut manifests: Vec<&PluginManifest> = loaded
            .values()
            .filter(|plugin| plugin.status.error_reason().is_none())
            .map(|plugin| &plugin.manifest)
            .collect();
        manifests.sort_by(|a, b| a.plugin.id.cmp(&b.plugin.id));
        ContributionRegistry::build(manifests)
    }

    /// 按插件统计贡献点数量（命令 / 面板 / 设置来自注册表，驱动来自清单）。
    ///
    /// 面板用它在行上显示「贡献 N」：此前注册表虽已建好，但宿主完全没消费，
    /// 用户看不出一个插件到底声明了什么。加载失败的插件不在结果里（清单不可信）。
    pub fn contribution_counts(&self) -> Result<HashMap<String, ContributionCounts>, CoreError> {
        let registry = self.contributions()?;
        let mut counts: HashMap<String, ContributionCounts> = HashMap::new();
        for command in registry.commands() {
            counts
                .entry(command.plugin_id.clone())
                .or_default()
                .commands += 1;
        }
        for panel in registry.panels() {
            counts.entry(panel.plugin_id.clone()).or_default().panels += 1;
        }
        for setting in registry.settings() {
            counts
                .entry(setting.plugin_id.clone())
                .or_default()
                .settings += 1;
        }

        let loaded = self
            .loaded_plugins
            .read()
            .map_err(|_| lock_err("loaded_plugins"))?;
        for plugin in loaded
            .values()
            .filter(|plugin| plugin.status.error_reason().is_none())
        {
            counts.entry(plugin.id.clone()).or_default().drivers +=
                plugin.manifest.contributes.drivers.len();
        }
        Ok(counts)
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

/// sidecar stderr 日志路径（与 [`crate::sidecar::process::SpawnSpec`] 同一口径）。
///
/// 管理页「查看 sidecar 日志」用它；日志可能还没产生（驱动从未启动过），调用方先判存在。
pub fn sidecar_log_path(plugin_id: &str) -> PathBuf {
    paths::plugin_cache_dir(plugin_id).join(crate::sidecar::process::SIDECAR_LOG_NAME)
}

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
    set_runtime_handle(tokio::runtime::Handle::try_current().map_err(|_| {
        CoreError::common(shared::error::CommonError::General(
            "插件管理器初始化必须在 tokio runtime 上下文内".to_string(),
        ))
    })?);

    let manager = Arc::new(PluginManager::new(install_dir));
    manager.init().await?;

    PLUGIN_MANAGER.set(manager.clone()).map_err(|_| {
        CoreError::common(shared::error::CommonError::General(
            "Plugin manager already initialized".to_string(),
        ))
    })?;

    // 首轮扫描同步完成再拉起循环：连接对话框在窗口首帧就会读 `drivers` 行，
    // 若等后台 tick，可能先读到空目录 / `enabled=0`，显示「暂无驱动」。
    // 扫描失败不阻断启动：运行时循环下一拍还会重试。
    if let Err(e) = manager.scan_and_diff_plugins().await {
        tracing::warn!(error = %e, "插件首轮扫描失败（由运行时循环下一拍重试）");
    }

    crate::sidecar::runtime::PluginRuntimeLoop::new(manager, crate::sidecar::runtime::RUNTIME_TICK)
        .spawn(); // 句柄 drop 即 detach（任务生命周期随 tokio runtime，宿主退出即停）

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一次性临时目录（Drop 清掉，不留过程产物）
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("rds-payload-fp-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 写文件并保证「长度不同」：纳秒 mtime 已经能区分同长度覆盖，
    /// 但测试里用不同长度可以让断言不依赖文件系统的时间戳精度。
    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    /// Java 运行时 + 声明两个外部 jar 的最小合法清单
    const MANIFEST_WITH_JARS: &str = r#"
[plugin]
id = "ext.demo"
name = "Demo"
version = "1.2.3"
publisher = "me"
description = "demo"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "script"
runtime = "java"
executable = "lib/shell.jar"
interpreter_args = ["-jar"]
driver_jars = ["D:/drivers/a.jar", "C:\\extra\\mysql"]

[permissions]
sidecar = ["spawn.child_process", "net.connect"]

[capabilities.driver]
concurrency = "serial"

[[contributes.drivers]]
id = "demo"
display_name = "Demo Database"
default_port = 1521
url_template = "jdbc:demo://{host}:{port}"
"#;

    /// 清单能解析出「版本 + 外部 jar」；不存在 / 坏清单给空，不 panic。
    #[test]
    fn manifest_side_reads_version_and_external_jars() {
        let dir = TempDir::new("manifest-side");
        let manifest = dir.0.join("plugin.toml");
        write(&manifest, MANIFEST_WITH_JARS);

        let (version, jars) = manifest_side(&manifest);
        if version.is_empty() {
            // 夹具清单必须真的能过 validate，否则整条测试变成「空对空」
            let err = crate::manifest::ManifestParser::parse(&manifest).unwrap_err();
            panic!("夹具清单没通过校验：{err}");
        }
        assert_eq!(version, "1.2.3", "版本要来自清单，不是空串");
        assert_eq!(jars.len(), 2, "外部 jar 要按清单原样取出");

        assert_eq!(
            manifest_side(&dir.0.join("missing.toml")),
            (String::new(), Vec::new()),
            "清单不存在时给空"
        );
        let broken = dir.0.join("broken.toml");
        write(&broken, "[this is not toml");
        assert_eq!(
            manifest_side(&broken),
            (String::new(), Vec::new()),
            "坏清单给空（坏包走自己的错误路径）"
        );
    }

    /// 快照里清单 hash 相同 → 直接复用，**不碰文件**。
    ///
    /// 证明方式：清单路径指向不存在的文件——若实现偷偷去解析，拿到的就是空值。
    #[test]
    fn same_manifest_hash_reuses_the_snapshot_without_parsing() {
        let prev = ScanEntry {
            version: "9.9.9".to_string(),
            manifest_hash: 42,
            driver_jars: vec!["D:/drivers/prev.jar".to_string()],
            payload_hash: 7,
        };
        let missing = Path::new("D:/rds-no-such-dir/plugin.toml");

        let (version, jars) = resolve_manifest_side(Some(&prev), 42, missing);
        assert_eq!(version, "9.9.9", "清单没变就该复用快照里的版本");
        assert_eq!(jars, vec!["D:/drivers/prev.jar".to_string()]);

        // hash 不同 → 必须真解析（路径不存在，所以拿到空值）
        let (version, jars) = resolve_manifest_side(Some(&prev), 43, missing);
        assert!(version.is_empty() && jars.is_empty(), "清单变了要重新解析");
    }

    /// 目录内文件换了 → 指纹变；同样的目录再算一次 → 指纹稳定（排序生效）。
    #[test]
    fn directory_payload_change_moves_the_fingerprint() {
        let dir = TempDir::new("dir");
        write(&dir.0.join("agent.exe"), "v1");
        let before = payload_fingerprint(&dir.0, &[]);
        assert_eq!(
            before,
            payload_fingerprint(&dir.0, &[]),
            "同一份目录两次结果必须一致（read_dir 顺序不稳定，靠排序收敛）"
        );

        write(&dir.0.join("agent.exe"), "v2-longer");
        assert_ne!(
            before,
            payload_fingerprint(&dir.0, &[]),
            "目录内载荷变了必须反映到指纹上"
        );
    }

    /// 清单声明的外部 jar 换了 → 指纹变（这是 `driver_jars` 的场景：
    /// jar 在插件目录外，换它时目录与清单都不动）。
    #[test]
    fn external_jar_is_part_of_the_payload() {
        let dir = TempDir::new("ext-file");
        write(&dir.0.join("plugin.toml"), "[plugin]");
        let jar = dir.0.join("drivers").join("ojdbc11.jar");
        write(&jar, "jar-v1");
        let jars = vec![jar.to_string_lossy().to_string()];

        let before = payload_fingerprint(&dir.0, &jars);
        assert_eq!(before, payload_fingerprint(&dir.0, &jars));

        write(&jar, "jar-v2-longer");
        assert_ne!(
            before,
            payload_fingerprint(&dir.0, &jars),
            "外部 jar 更新后指纹必须变（否则 Java 壳一直用旧 jar）"
        );

        // 不声明 jar 时，同样的目录算出来是另一个值（外部内容不参与）
        assert_ne!(before, payload_fingerprint(&dir.0, &[]));
    }

    /// 外部路径「先不存在、后放进去」也要能被认出来（部署期会校验存在，
    /// 但用户手动补 jar 是真实场景）。
    #[test]
    fn external_jar_appearing_later_changes_the_fingerprint() {
        let dir = TempDir::new("ext-appear");
        write(&dir.0.join("plugin.toml"), "[plugin]");
        let jar = dir.0.join("drivers").join("late.jar");
        let jars = vec![jar.to_string_lossy().to_string()];

        let missing = payload_fingerprint(&dir.0, &jars);
        write(&jar, "now-here");
        assert_ne!(missing, payload_fingerprint(&dir.0, &jars));
    }

    /// 清单允许把外部路径写成**目录**（`driver_jars = ["D:/drivers/mysql"]`），
    /// 目录里加 / 换 jar 同样要反映到指纹上。
    #[test]
    fn external_directory_contents_are_walked() {
        let dir = TempDir::new("ext-dir");
        write(&dir.0.join("plugin.toml"), "[plugin]");
        let ext = dir.0.join("drivers");
        let jars = vec![ext.to_string_lossy().to_string()];

        // 目录还没建：记 missing
        let empty = payload_fingerprint(&dir.0, &jars);

        write(&ext.join("mysql-connector.jar"), "a");
        let one = payload_fingerprint(&dir.0, &jars);
        assert_ne!(empty, one, "外部目录出现后指纹要变");

        write(&ext.join("mysql-connector.jar"), "a-longer");
        assert_ne!(one, payload_fingerprint(&dir.0, &jars), "目录内文件换掉要变");

        write(&ext.join("extra.jar"), "b");
        assert_ne!(
            one,
            payload_fingerprint(&dir.0, &jars),
            "目录里新增一个 jar 也要变"
        );
    }
}
