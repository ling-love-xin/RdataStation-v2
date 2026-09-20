//! rds-paths — 运行时数据路径的**唯一解析点**。
//!
//! 口径（设计见 `docs/architecture/runtime/data-paths.md`）：这个软件生成的**任何信息**
//! （配置 / 数据 / 日志 / 临时 / 扩展）都待在软件自己的目录下，默认 = **可执行文件所在目录**
//! （即安装目录），一律不进 git。
//!
//! ```text
//! <RDS_HOME>/
//! ├── config/settings.json        # 全局设置
//! ├── data/                       # global.db / analytics.duckdb / 密钥库 / samples
//! ├── logs/app.YYYY-MM-DD         # 日志（按天滚动）
//! ├── tmp/                        # DuckDB spill / 联邦临时库 / 进程 scratch / sidecar 工作目录
//! ├── extensions/                 # **DuckDB** 扩展（不是插件！）
//! ├── plugins/                    # 插件本体：plugins/<id>/（.registry 是分发包缓存）
//! ├── plugin-data/<id>/           # 插件持久数据（卸载时可选保留）
//! └── plugin-cache/<id>/          # 插件可清缓存（wasmtime 缓存 / sidecar 日志）
//! ```
//!
//! ⚠️ `extensions/` 与 `plugins/` 是**两件事**：前者是 DuckDB 的 SQL 扩展，后者是插件系统（M9）。
//! 别把插件装进 `extensions/`（旧布局迁移也用这个目录名做判定）。
//!
//! **其它 crate 不要再自己拼路径**，也不要直接读 `APPDATA` / `LOCALAPPDATA` / 主目录：
//! 路径散在多处时，换一个位置要改 N 个文件且必漏（改造前有 9 处硬编码 `"RdataStation"`）。
//!
//! ## 覆盖
//!
//! | 变量 | 作用 | 默认 |
//! | --- | --- | --- |
//! | `RDS_HOME` | 覆盖整个数据根 | 可执行文件所在目录（不可写时回退平台本地数据目录） |
//! | `RDS_TEMP_DIR` | 只覆盖临时目录（放到机械盘 / 网络盘会拖慢 DuckDB spill） | `<RDS_HOME>/tmp` |
//! | `RDS_ASSETS_DIR` | 覆盖**随包只读资源**目录（主题 / 图标） | 可执行文件同级 `assets/` → 开发期仓库 `assets/` |
//!
//! ⚠️ 上面那张表里只有 `assets/` 不是生成物：它随安装包走、只读，**不落在 `RDS_HOME` 下**。
//! 为什么也放在本 crate：发布版由云端构建，编译期路径（`CARGO_MANIFEST_DIR`）指的是构建机的
//! 检出目录——用户机上不存在，表现为主题与标题栏图标**静默**丢失（读目录失败即返回）。
//!
//! ## 启动契约
//!
//! `crates/app/src/main.rs` **第一条语句**必须是 [`install_process_temp_dir`]：
//! 它把进程的 `TEMP` / `TMP` / `TMPDIR` 重定向到 [`temp_dir`]，从而一次性覆盖所有
//! `std::env::temp_dir()` 调用点（DuckDB spill、联邦临时库、各处 scratch），
//! 不必逐个改代码。必须在任何线程 / 运行时启动前调用，原因见该函数的安全注释。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub mod legacy;
pub mod migrate;

#[cfg(test)]
mod tests;

pub use migrate::{MigrationReport, migrate_legacy_layout};

/// 数据根环境变量。
const ENV_HOME: &str = "RDS_HOME";
/// 临时目录环境变量（单独覆盖）。
const ENV_TEMP: &str = "RDS_TEMP_DIR";
/// 随包只读资源目录环境变量（单独覆盖）。
const ENV_ASSETS: &str = "RDS_ASSETS_DIR";
/// 回退目录名：数据根不可写时落到平台本地数据目录下的这个名字（与旧布局同名，便于识别）。
const FALLBACK_DIR_NAME: &str = "RdataStation";
/// 可写性探测用的临时文件名（写完即删）。
const PROBE_FILE: &str = ".rds-write-probe";

/// 新布局的目录名（`migrate` 用它避免把新目录当旧数据搬）。
///
/// ⚠️ **新增顶层目录必须同步加到这里**：否则 `RDS_HOME` 恰好落在旧布局目录上时，
/// 迁移会把新目录当旧数据搬走（历史坑：`data/` 被再搬一次成 `data/data/`）。
pub(crate) const NEW_LAYOUT_DIRS: [&str; 8] = [
    "config",
    "data",
    "logs",
    "tmp",
    "extensions",
    "plugins",
    "plugin-data",
    "plugin-cache",
];

/// 插件 id 的最大字节数（够长且不撞文件系统上限）。
const PLUGIN_ID_MAX_LEN: usize = 128;

/// 数据根是怎么定下来的（诊断用：出问题时先看这里）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeOrigin {
    /// 来自 `RDS_HOME` 环境变量。
    EnvHome,
    /// 可执行文件所在目录（默认规则 = 安装目录）。
    ExecutableDir,
    /// `RDS_HOME` / 安装目录都不可写，回退平台本地数据目录。
    FallbackLocalAppData,
    /// **测试构建**：隔离到进程专属临时目录（见 `docs/architecture/runtime/data-paths.md` §5），
    /// 生产构建永远不会出现这个来源。
    TestRoot,
}

impl HomeOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::EnvHome => "环境变量 RDS_HOME",
            Self::ExecutableDir => "可执行文件所在目录（安装目录）",
            Self::FallbackLocalAppData => "安装目录不可写，回退平台本地数据目录",
            Self::TestRoot => "测试构建的隔离数据根",
        }
    }
}

static HOME: OnceLock<PathBuf> = OnceLock::new();
static HOME_ORIGIN: OnceLock<HomeOrigin> = OnceLock::new();
/// 重定向前的 `TEMP`（迁移要从旧临时目录里把数据捞回来，见 `legacy::temp_app_dir`）。
static PREVIOUS_TEMP: OnceLock<Option<PathBuf>> = OnceLock::new();

/// 数据根。首次调用时解析并缓存（进程内恒定，避免多处解析出不同结果）。
pub fn home() -> PathBuf {
    HOME.get_or_init(|| {
        let (path, origin) = resolve_home();
        let _ = HOME_ORIGIN.set(origin);
        path
    })
    .clone()
}

/// 数据根的来源（见 [`HomeOrigin`]）。
pub fn home_origin() -> HomeOrigin {
    let _ = home(); // 保证已解析
    HOME_ORIGIN
        .get()
        .copied()
        .unwrap_or(HomeOrigin::ExecutableDir)
}

/// 全局设置目录：`<RDS_HOME>/config`。
pub fn config_dir() -> PathBuf {
    home().join("config")
}

/// 全局数据目录：`<RDS_HOME>/data`（global.db / analytics.duckdb / 密钥库 / samples）。
pub fn data_dir() -> PathBuf {
    home().join("data")
}

/// 日志目录：`<RDS_HOME>/logs`。
pub fn log_dir() -> PathBuf {
    home().join("logs")
}

/// 临时目录：`RDS_TEMP_DIR` → 否则 `<RDS_HOME>/tmp`。
pub fn temp_dir() -> PathBuf {
    match std::env::var_os(ENV_TEMP) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => home().join("tmp"),
    }
}

/// DuckDB 扩展目录：`<RDS_HOME>/extensions`。
///
/// ⚠️ 这是 **DuckDB 的 SQL 扩展**，与插件系统（M9）无关；插件用 [`plugins_dir`]。
pub fn extensions_dir() -> PathBuf {
    home().join("extensions")
}

// ==================== 随包只读资源（`assets/`）====================
//
// 与上面全体成员的分别：上面那些是**软件生成的**（可写、跟着用户走），这里这一份是
// **随包发布的**（只读、跟着安装包走）。放在本 crate 的理由同样是「路径解析只在一处发生」。
//
// 为什么不能只写编译期路径：发布包由 CI 在云端构建（`.github/workflows/release.yml`），
// `env!("CARGO_MANIFEST_DIR")` 指的是**构建机**的检出目录，用户机上根本不存在。
// 主题目录读不出来时 `read_dir` 直接返回、产品 token 加载失败只打一行 stderr，
// 界面照常起来——这类「静默降级」正是发版后最难查的问题，故把规则显式化。

/// 随包只读资源目录（`assets/`：主题 / 应用图标）。
///
/// 取值顺序（首个**存在**的目录胜出；进程内恒定）：
///
/// 1. `RDS_ASSETS_DIR`（显式覆盖，不要求存在）；
/// 2. **可执行文件同级 `assets/`** —— 发布包布局（`rds-app.exe` + `duckdb.dll` + `assets/`），
///    也覆盖用户自己解压到任意目录、或把 `assets/` 拷到 exe 旁边的场景；
/// 3. **仓库根的 `assets/`**（编译期路径）—— 开发期布局：`cargo run` 的 exe 在 `target/debug/`，
///    旁边没有 `assets/`；
/// 4. 都不存在：返回「可执行文件同级 `assets/`」并在 stderr 提示——路径可预测，
///    读不到就说明那份包少了文件，而不是去别处乱找。
///
/// 只读语义由调用方遵守：这里不建目录、不探测可写性（与 [`home`] 的差别所在）。
pub fn assets_dir() -> PathBuf {
    static ASSETS: OnceLock<PathBuf> = OnceLock::new();
    ASSETS.get_or_init(resolve_assets_dir).clone()
}

fn resolve_assets_dir() -> PathBuf {
    if let Some(raw) = std::env::var_os(ENV_ASSETS) {
        let candidate = PathBuf::from(raw);
        if !candidate.as_os_str().is_empty() {
            return candidate;
        }
    }

    let exe_side = executable_dir().map(|dir| dir.join("assets"));
    let dev_side = dev_assets_dir();
    let fallback = exe_side
        .clone()
        .or_else(|| dev_side.clone())
        .unwrap_or_else(|| PathBuf::from("assets"));
    let dir = pick_existing_dir(&[exe_side.as_deref(), dev_side.as_deref()], fallback);
    if !dir.is_dir() {
        eprintln!(
            "[paths] 未找到随包资源目录 {}：主题与标题栏图标会缺失\
             （发布包应把 assets/ 放在可执行文件同级）",
            dir.display()
        );
    }
    dir
}

/// 按顺序取第一个存在的候选目录；都不存在时返回 `fallback`。
fn pick_existing_dir(candidates: &[Option<&Path>], fallback: PathBuf) -> PathBuf {
    candidates
        .iter()
        .filter_map(|candidate| *candidate)
        .find(|candidate| candidate.is_dir())
        .map_or(fallback, Path::to_path_buf)
}

/// 开发期的仓库 `assets/`（编译期路径；发布版走 exe 同级那一份）。
fn dev_assets_dir() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .parent()
        .map(|root| root.join("assets"))
}

// ==================== 插件系统（M9）====================
//
// 布局与理由见 `docs/architecture/plugin/plugin-dev-plan.md` §4.7。
// 目录说明：
//   plugins/          插件本体（安装产物）；契约是「一个插件一个子目录」，卸载 = 删目录
//   plugin-data/      插件持久数据（globalStoragePath；卸载时可选保留）
//   plugin-cache/     可清缓存（wasmtime 编译缓存 / sidecar 日志）；删了只影响性能
//   tmp/sidecar/      sidecar 子进程的 current_dir（进程退出后回收）

/// 插件根目录：`<RDS_HOME>/plugins`（每个插件一个子目录）。
pub fn plugins_dir() -> PathBuf {
    home().join("plugins")
}

/// 单个插件目录：`<RDS_HOME>/plugins/<id>`（安装产物；卸载 = 删这个目录）。
///
/// **调用前必须过 [`validate_plugin_id`]** —— 这个函数直接拼接 id，不做清洗。
pub fn plugin_dir(id: &str) -> PathBuf {
    plugins_dir().join(id)
}

/// 插件持久数据目录：`<RDS_HOME>/plugin-data/<id>`。
///
/// **调用前必须过 [`validate_plugin_id`]**。
pub fn plugin_data_dir(id: &str) -> PathBuf {
    home().join("plugin-data").join(id)
}

/// 插件可清缓存目录：`<RDS_HOME>/plugin-cache/<id>`（wasmtime 缓存 / sidecar 日志）。
///
/// **调用前必须过 [`validate_plugin_id`]**；这里的内容随时可删。
pub fn plugin_cache_dir(id: &str) -> PathBuf {
    home().join("plugin-cache").join(id)
}

/// sidecar 工作目录：`<RDS_HOME>/tmp/sidecar/<id>`（子进程的 `current_dir`）。
///
/// 放 tmp 下是故意的：子进程崩溃留下的临时文件不该污染数据目录。
/// **调用前必须过 [`validate_plugin_id`]**。
pub fn sidecar_work_dir(id: &str) -> PathBuf {
    temp_dir().join("sidecar").join(id)
}

/// 插件注册表目录：`<RDS_HOME>/plugins/.registry`（分发包缓存 + 清单索引）。
///
/// 点号开头是故意的：扫描插件时会被跳过，不会把它当成一个插件。
pub fn plugin_registry_dir() -> PathBuf {
    plugins_dir().join(".registry")
}

/// 插件 id 非法。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidPluginId(String);

impl std::fmt::Display for InvalidPluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "非法的插件 id：{:?}", self.0)
    }
}

impl std::error::Error for InvalidPluginId {}

/// 校验插件 id 能不能安全地拼进路径。
///
/// 规则（白名单，不是黑名单）：ASCII 字母 / 数字 / `.` / `_` / `-`；
/// 不得以 `.` 开头（避免 `.`、`..` 与隐藏目录）；长度 ≤ [`PLUGIN_ID_MAX_LEN`]。
///
/// **为什么必须有这一道**：插件 id 来自第三方清单，`plugin_dir(id)` 直接 `join`，
/// 一个 `../../..` 的 id 就能把「安装」写到数据根外面去（Windows 上连 `C:\` 也拼得出来）。
/// 这里**只做形参校验**，真正的穿越防御（symlink / TOCTOU）按 dev-plan §4.7 在打开句柄那一层做。
pub fn validate_plugin_id(id: &str) -> Result<(), InvalidPluginId> {
    let bad = || InvalidPluginId(id.to_string());

    if id.is_empty() || id.len() > PLUGIN_ID_MAX_LEN {
        return Err(bad());
    }
    if id.starts_with('.') {
        return Err(bad());
    }
    let ok = id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-');
    if !ok {
        return Err(bad());
    }
    // 至少一个字母或数字，挡掉 `---` / `...` 这类纯符号名
    if !id.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Err(bad());
    }
    Ok(())
}

/// 建好全部派生目录（幂等）。启动时装一次，后续写入不必各自 `create_dir_all`。
///
/// 只建**根**（如 `plugin-data/`）；`plugin-data/<id>/` 这类按插件建的目录由安装/加载流程
/// 自己 `create_dir_all`（它们要先过 [`validate_plugin_id`]）。
pub fn ensure_dirs() -> std::io::Result<()> {
    for dir in [
        config_dir(),
        data_dir(),
        log_dir(),
        temp_dir(),
        extensions_dir(),
        plugins_dir(),
        plugin_registry_dir(),
        home().join("plugin-data"),
        home().join("plugin-cache"),
        temp_dir().join("sidecar"),
    ] {
        std::fs::create_dir_all(&dir)?;
    }
    Ok(())
}

/// 把进程的 `TEMP` / `TMP` / `TMPDIR` 重定向到 [`temp_dir`]。
///
/// 只改这一处，所有 `std::env::temp_dir()` 调用点（DuckDB spill、联邦临时库、
/// 各 crate 的 scratch）自动落到 `<RDS_HOME>/tmp`。
///
/// # 调用契约
///
/// **必须是 `main()` 的第一条语句**（早于任何线程 / gpui / tokio 运行时创建）。
///
/// # Safety
///
/// 内部对 `std::env::set_var` 的使用是 `unsafe`（Rust 2024）：环境变量表是进程级
/// 全局状态，多线程下写它会与并发读它的线程构成数据竞争。在 `main()` 第一条语句
/// 调用时进程只有主线程，不存在并发读写者；此后本进程不再改写这几个变量。
pub fn install_process_temp_dir() -> std::io::Result<PathBuf> {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir)?;
    // 记下改造前的 TEMP：旧布局在"数据目录不可用"时曾回退到 %TEMP%/RdataStation，
    // 迁移要能从那里把真实数据捞回来（见 `legacy::temp_app_dir`）。
    let _ = PREVIOUS_TEMP.set(std::env::var_os("TEMP").map(PathBuf::from));
    // SAFETY: 见本函数文档的调用契约——此刻进程只有主线程。
    unsafe {
        std::env::set_var("TEMP", &dir);
        std::env::set_var("TMP", &dir);
        std::env::set_var("TMPDIR", &dir);
    }
    Ok(dir)
}

/// 重定向前的 `TEMP`（未重定向时为 `None`）。
pub(crate) fn previous_temp() -> Option<PathBuf> {
    PREVIOUS_TEMP.get().cloned().flatten()
}

/// 一段人类可读的路径摘要（启动日志 / 报错诊断用）。
pub fn summary() -> String {
    format!(
        "数据根 {}（来源：{}）\n  配置 {}\n  数据 {}\n  日志 {}\n  临时 {}\n  扩展 {}\n  插件 {}",
        home().display(),
        home_origin().label(),
        config_dir().display(),
        data_dir().display(),
        log_dir().display(),
        temp_dir().display(),
        extensions_dir().display(),
        plugins_dir().display(),
    )
}

// ==================== 内部：根目录解析 ====================

fn resolve_home() -> (PathBuf, HomeOrigin) {
    // 测试构建优先：数据根隔离到临时目录，不让测试写进产品目录。
    // 口径与理由见 `docs/architecture/runtime/data-paths.md` §5。
    if let Some(root) = test_root() {
        return (root, HomeOrigin::TestRoot);
    }

    if let Some(raw) = std::env::var_os(ENV_HOME) {
        let candidate = PathBuf::from(raw);
        if !candidate.as_os_str().is_empty() {
            if probe_writable(&candidate) {
                return (candidate, HomeOrigin::EnvHome);
            }
            eprintln!(
                "[paths] {ENV_HOME}={} 不可写，改用默认位置",
                candidate.display()
            );
        }
    }

    if let Some(dir) = executable_dir() {
        if probe_writable(&dir) {
            return (dir, HomeOrigin::ExecutableDir);
        }
        eprintln!(
            "[paths] 安装目录 {} 不可写（如装在 Program Files），回退本地数据目录",
            dir.display()
        );
    }

    let fallback = fallback_home();
    eprintln!("[paths] 数据根回退到 {}", fallback.display());
    (fallback, HomeOrigin::FallbackLocalAppData)
}

/// 可执行文件所在目录 = 安装目录。
fn executable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(Path::to_path_buf)
}

/// 回退根：`%LOCALAPPDATA%/RdataStation`（非 Windows 走 `dirs::data_local_dir()`）。
fn fallback_home() -> PathBuf {
    dirs::data_local_dir()
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
        .join(FALLBACK_DIR_NAME)
}

/// 目录可写性探测：建目录 + 写一个探针文件再删掉。
///
/// 只看 `metadata().permissions().readonly()` 不够——Windows 上 `Program Files`
/// 的目录属性未必带只读位，真正的判据是"能不能写进去"。
fn probe_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(PROBE_FILE);
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

// ==================== 测试构建的数据根隔离 ====================
//
// 为什么要有这一段：测试会调用**产品代码里的全局路径**（密钥库、草稿库、SQL 历史、
// 元数据缓存），于是往产品数据根写垃圾；因为旧布局迁移是"只补不盖"，这些垃圾
// 还会把真数据挡在门外（详见 data-paths.md §5）。
//
// 为什么不是逐个测试注入：全仓几十个测试文件、以后还会加；靠纪律必漏。
// 这里把隔离做成**构建期**的事：`test-support` feature 只被各成员的
// `[dev-dependencies]` 打开，所以"测试构建拿到隔离根"是结构性的，不靠人记得。

/// 覆盖隔离根的环变量（想让测试数据待在某处时用；也用于 example 定向到开发根）。
#[cfg(any(test, feature = "test-support"))]
const ENV_TEST_HOME: &str = "RDS_TEST_HOME";

/// 测试构建的数据根：`RDS_TEST_HOME` → 否则 `<临时目录>/rds-test-root-<pid>`。
///
/// 生产构建（无 `test-support`、非本 crate 单测）返回 `None`，一切照旧。
fn test_root() -> Option<PathBuf> {
    #[cfg(not(any(test, feature = "test-support")))]
    {
        None
    }

    #[cfg(any(test, feature = "test-support"))]
    {
        if let Some(raw) = std::env::var_os(ENV_TEST_HOME) {
            let path = PathBuf::from(raw);
            if !path.as_os_str().is_empty() {
                return Some(path);
            }
        }
        let root = std::env::temp_dir().join(format!("rds-test-root-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        Some(root)
    }
}

/// 把本进程的数据根钉到 `dir`（**一次性**；已解析过就返回 `false`）。
///
/// 两个用途：
/// 1. 测试想把数据放在指定目录（而不是默认的进程专属临时目录）；
/// 2. `cargo run --example ...`：example 用的是带 `test-support` 的构建，
///    但**要写开发数据根**——把它指回 `RDS_HOME`（见 `examples/seed_demo.rs`）。
#[cfg(any(test, feature = "test-support"))]
pub fn pin_root(dir: impl Into<PathBuf>) -> bool {
    let dir = dir.into();
    if dir.as_os_str().is_empty() {
        return false;
    }
    // 数据根一旦被解析过就钉不住了（各模块可能已经拿过路径）——返回 false 而不是假装成功
    if HOME.set(dir).is_err() {
        return false;
    }
    let _ = HOME_ORIGIN.set(HomeOrigin::TestRoot);
    true
}

/// 本进程是否用着"隔离数据根"（测试 / example 里用于断言隔离生效）。
pub fn is_isolated_root() -> bool {
    home_origin() == HomeOrigin::TestRoot
}
