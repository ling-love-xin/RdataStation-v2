use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

use shared::error::{CommonError, CoreError, StorageError};

use crate::sidecar::lifecycle::{Concurrency, DEFAULT_MAX_INSTANCES, ProcessSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub contributes: PluginContributes,
    #[serde(default)]
    pub dependencies: Vec<PluginDependency>,
    /// `[backend]`：这个插件**怎么跑**（dev-plan §4.3）。不写 `[backend]` = 只有进程内形态（前端/wasm）。
    #[serde(default)]
    pub backend: Option<PluginBackend>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub id: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    pub engines: PluginEngines,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEngines {
    pub rdatastation: String,
}

// ========== [backend]：怎么跑 ==========

/// `[backend]` 段（dev-plan §4.3）。
///
/// 三种形态：`sidecar`（原生进程，P1 的目标）、`wasm`（进程内沙箱，P3 收紧）、
/// `script`（解释器 + 脚本，**也是子进程**，进同一个进程池）。
///
/// ⚠️ §4.3 里的 `[backend.wasm]`（entry / 限额并进来）**未实现**：那是 P3 收 wasm 时的迁移，
/// 在那之前 wasm 的入口仍在 `capabilities.wasm`；现在两处都写才是真的乱。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginBackend {
    pub kind: BackendKind,
    /// 相对**插件目录**的可执行文件（如 `bin/agent`）。
    ///
    /// 不得是绝对路径、也不得含 `..`（第三方清单给的字符串，拼路径前必过白名单；
    /// 与 `paths::validate_plugin_id` 同一个理由）。
    #[serde(default)]
    pub executable: Option<String>,
    /// `script` 型的解释器（如 `python3`；按 PATH 解析）。
    #[serde(default)]
    pub interpreter: Option<String>,
    /// 传输方式；P1 起只有 `framed`（dev-plan §4.2.1）。
    #[serde(default)]
    pub transport: BackendTransport,
    /// 协议标识（如 `rds-driver/1`）。真正的版本闸在 `initialize` 握手（proto §4.2.3），
    /// 这里只把声明的值带到手上，供诊断与「两处不一致」报警。
    #[serde(default)]
    pub protocol: Option<String>,
    /// 平台矩阵（`win-x64` / `darwin-arm64` / `linux-x64`）；**空 = 未声明**（不阻挡，向后兼容）。
    #[serde(default)]
    pub platforms: Vec<String>,
    /// 实例数上限（§4.1 规则 1）；不写 = 1。
    #[serde(default = "default_max_instances")]
    pub max_instances: usize,
}

fn default_max_instances() -> usize {
    DEFAULT_MAX_INSTANCES
}

/// 运行形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    /// 原生子进程：宿主起进程 + stdio 二进制分帧。
    Sidecar,
    /// 进程内沙箱（无 host function 即无能力）；不进进程池。
    Wasm,
    /// 解释器 + 脚本；同样是子进程。
    Script,
}

impl BackendKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sidecar => "sidecar",
            Self::Wasm => "wasm",
            Self::Script => "script",
        }
    }

    /// 是不是子进程形态（是就进进程池、走 `sidecar/process.rs`）。
    pub fn is_subprocess(self) -> bool {
        matches!(self, Self::Sidecar | Self::Script)
    }
}

/// 传输方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackendTransport {
    /// stdio 二进制分帧（4B 大端整帧长 + 1B 种类），P1 起唯一支持。
    #[default]
    Framed,
    /// 一行一个 JSON。
    ///
    /// **故意留着这个取值**：让老形态的清单在解析期就能得到一个明确的拒绝，
    /// 而不是跑起来才发现（「不得静默退化」）。
    Jsonl,
}

impl BackendTransport {
    pub fn name(self) -> &'static str {
        match self {
            Self::Framed => "framed",
            Self::Jsonl => "jsonl",
        }
    }
}

/// 起进程要用的东西（`script` 型的脚本路径在 `args` 里）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl PluginBackend {
    /// 解析出「跑什么」（`executable` / 脚本路径都相对 `plugin_dir`）。
    ///
    /// 四道闸的顺序固定：**形态 → 传输 → 平台 → 路径**，每道都给可读原因 ——
    /// 让「起不来」当场能说清，而不是丢一句「进程启动失败」让人去猜。
    pub fn command(&self, plugin_dir: &Path) -> Result<BackendCommand, CoreError> {
        if !self.kind.is_subprocess() {
            return Err(manifest_error(format!(
                "backend.kind = {} has no process to start",
                self.kind.name()
            )));
        }
        if self.transport != BackendTransport::Framed {
            return Err(manifest_error(format!(
                "backend.transport = {} is not supported yet (P1 only supports framed)",
                self.transport.name()
            )));
        }
        if !self.supports_current_platform() {
            return Err(manifest_error(format!(
                "plugin does not declare the current platform {} (declared: {})",
                current_platform(),
                self.platforms.join(",")
            )));
        }

        match self.kind {
            BackendKind::Script => {
                let interpreter = self
                    .interpreter
                    .as_deref()
                    .ok_or_else(|| manifest_error("backend.interpreter is required for script"))?;
                let script = self.resolve_executable(plugin_dir)?;
                Ok(BackendCommand {
                    program: PathBuf::from(interpreter),
                    args: vec![script.to_string_lossy().into_owned()],
                })
            }
            _ => Ok(BackendCommand {
                program: self.resolve_executable(plugin_dir)?,
                args: Vec::new(),
            }),
        }
    }

    /// 声明的 `executable` 是否为合法的**相对路径**（只做形参校验，不碰文件系统）。
    pub fn relative_executable(&self) -> Result<&Path, CoreError> {
        let declared = self
            .executable
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| manifest_error("backend.executable is required for sidecar / script"))?;

        let relative = Path::new(declared);
        if relative.is_absolute() {
            return Err(manifest_error(format!(
                "backend.executable = {declared} must be relative to the plugin directory"
            )));
        }
        // 逐段白名单：只留普通段（`..` / 盘符 / 根都拒绝）
        if !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
        {
            return Err(manifest_error(format!(
                "backend.executable = {declared} escapes the plugin directory"
            )));
        }
        Ok(relative)
    }

    /// 拼出真实路径。
    ///
    /// Windows 上 `bin/agent` 实际通常叫 `bin/agent.exe`：**只在没写后缀、且带后缀的那个
    /// 真的存在**时补上 —— 这样清单不用为平台分叉，而真找不到时错误信息里显示的是原样路径
    /// （不是我们凭空加的 `.exe`）。
    pub fn resolve_executable(&self, plugin_dir: &Path) -> Result<PathBuf, CoreError> {
        let relative = self.relative_executable()?;
        let path = plugin_dir.join(relative);
        if cfg!(windows) && path.extension().is_none() {
            let with_exe = path.with_extension("exe");
            if with_exe.is_file() {
                return Ok(with_exe);
            }
        }
        Ok(path)
    }

    /// 清单声明的平台里有没有当前平台（**空 = 未声明**，不阻挡）。
    pub fn supports_current_platform(&self) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|p| *p == current_platform())
    }
}

/// 当前平台标识，与清单 `platforms` 同构（`win-x64` / `darwin-arm64` / `linux-x64`）。
pub fn current_platform() -> String {
    let os = match std::env::consts::OS {
        "windows" => "win",
        "macos" => "darwin",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{os}-{arch}")
}

fn manifest_error(message: impl Into<String>) -> CoreError {
    CoreError::common(CommonError::General(message.into()))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub frontend: Option<CapabilitiesFrontend>,
    #[serde(default)]
    pub wasm: Option<CapabilitiesWasm>,
    #[serde(default)]
    pub driver: Option<CapabilitiesDriver>,
}

/// `[capabilities.driver]`：驱动能力矩阵（dev-plan §4.4）。
///
/// ⚠️ **P1 只落 `concurrency`** —— 它是进程池（`lifecycle::ProcessSpec`）要用的那一项。
/// §4.4 其余字段（`schemas` / `views` / `routines` / `transactions` / `cancel` / `cursor` …）
/// 与 `DriverCapability` 门控一起在 P2 落：现在先写一份没人读的字段，只会变成「两份契约」
/// （参考实现的坑，见 §3.5）。serde 默认忽略未知键，所以现在写全字段的清单也能解析。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitiesDriver {
    /// `serial` | `parallel`；不写 = `serial`（§4.1 规则 3）。
    #[serde(default)]
    pub concurrency: Concurrency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesFrontend {
    pub entry: String,
    #[serde(default)]
    pub activation_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitiesWasm {
    pub entry: String,
    #[serde(default)]
    pub max_memory_mb: Option<usize>,
    #[serde(default)]
    pub max_cpu_time_ms: Option<u64>,
    #[serde(default)]
    pub allowed_host_functions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginPermissions {
    #[serde(default)]
    pub frontend: Vec<String>,
    #[serde(default)]
    pub wasm: Vec<String>,
    /// sidecar 轨（驱动插件）的**进程级**权限：`spawn.child_process` / `net.connect` /
    /// `fs.read_plugin_data` / `env.inherit` / `credentials.database`。
    ///
    /// **展示与告警轨，不门控**（dev-plan §4.6.1）；安装页要逐项明示。
    #[serde(default)]
    pub sidecar: Vec<String>,
    /// 驱动轨的**数据面能力**声明：`driver.query` / `driver.metadata` /
    /// `driver.transaction` / `driver.cancel` / `driver.cursor` / `driver.arrow`。
    ///
    /// 同样是展示/告警轨；真门控在 `DriverCapabilities`（engine 侧）。
    #[serde(default)]
    pub driver: Vec<String>,
    /// 凭据口径：`"database"` = 该插件会收到你的数据库密码（dev-plan §4.6.3）。
    #[serde(default)]
    pub credentials: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginContributes {
    #[serde(default)]
    pub commands: Vec<ContributesCommand>,
    #[serde(default)]
    pub panels: Vec<ContributesPanel>,
    #[serde(default)]
    pub drivers: Vec<ContributesDriver>,
    #[serde(default)]
    pub settings: Vec<ContributesSetting>,
    #[serde(default)]
    pub menus: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesCommand {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesPanel {
    pub id: String,
    pub title: String,
    pub location: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub order: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesDriver {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub default_port: Option<u16>,
    #[serde(default)]
    pub connection_schema: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributesSetting {
    pub key: String,
    #[serde(rename = "type")]
    pub setting_type: String,
    pub default: serde_json::Value,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDependency {
    pub id: String,
    pub version: String,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.plugin.id.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "Manifest missing required field: plugin.id".into(),
            )));
        }
        if self.plugin.name.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "Manifest missing required field: plugin.name".into(),
            )));
        }
        if self.plugin.version.is_empty() {
            return Err(CoreError::common(CommonError::General(
                "Manifest missing required field: plugin.version".into(),
            )));
        }
        if self.capabilities.frontend.is_none()
            && self.capabilities.wasm.is_none()
            && self.backend.is_none()
        {
            return Err(manifest_error(
                "Manifest must declare at least one runtime form: backend / capabilities.frontend / capabilities.wasm",
            ));
        }
        self.validate_backend()?;
        Ok(())
    }

    /// `[backend]` 声明了就要自洽：形态要的字段必须有，路径必须待在自己目录里。
    ///
    /// （其余四道闸 —— 传输 / 平台 —— 是**运行期**的事，放在 [`PluginBackend::command`]：
    /// 一个平台不支持的插件仍然应该能被安装、在列表里显示为「不支持」，而不是解析不过。）
    fn validate_backend(&self) -> Result<(), CoreError> {
        let Some(backend) = &self.backend else {
            return Ok(());
        };

        match backend.kind {
            BackendKind::Wasm => {
                // `[backend.wasm]` 并入是 P3 的事；在那之前 wasm 的入口仍在 capabilities.wasm
                if self.capabilities.wasm.is_none() {
                    return Err(manifest_error(
                        "backend.kind = wasm requires capabilities.wasm (the [backend.wasm] merge is P3)",
                    ));
                }
            }
            BackendKind::Sidecar | BackendKind::Script => {
                // 路径白名单：第三方清单给的字符串，拼路径前先过一道
                backend.relative_executable()?;
                if backend.kind == BackendKind::Script
                    && backend.interpreter.as_deref().is_none_or(|s| s.is_empty())
                {
                    return Err(manifest_error("backend.interpreter is required for script"));
                }
            }
        }
        Ok(())
    }

    /// 这个插件要不要进进程池、以及怎么进（`lifecycle::ProcessSpec`）。
    ///
    /// 只有**子进程形态**（sidecar / script）才有进程池；wasm 是进程内沙箱，返回 `None`。
    /// 清单没写 `[backend]` 也一样是 `None`。
    pub fn process_spec(&self) -> Option<ProcessSpec> {
        let backend = self.backend.as_ref()?;
        if !backend.kind.is_subprocess() {
            return None;
        }
        Some(ProcessSpec::new(
            self.contributes.drivers.iter().map(|d| d.id.clone()),
            backend.max_instances,
            self.driver_concurrency(),
        ))
    }

    /// 并发策略的家在 `[capabilities.driver].concurrency`（dev-plan §4.4）；未声明即 `serial`。
    ///
    /// ⚠️ **只此一处**：不要在 `[backend]` 里再放一份 —— 重复的契约迟早两边跑偏（§3.5）。
    pub fn driver_concurrency(&self) -> Concurrency {
        self.capabilities
            .driver
            .as_ref()
            .map(|d| d.concurrency)
            .unwrap_or_default()
    }

    pub fn check_engine_compatibility(&self, current_version: &str) -> Result<(), CoreError> {
        let required = &self.plugin.engines.rdatastation;

        let current_major: u32 = current_version
            .split('.')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let range_str = required
            .trim_start_matches('^')
            .trim_start_matches('~')
            .trim_start_matches('>')
            .trim_start_matches('<')
            .trim_start_matches('=');
        let required_major: u32 = range_str
            .split('.')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if current_major < required_major && required.starts_with('^') {
            return Err(CoreError::common(CommonError::General(format!(
                "Plugin '{}' requires engine version {}, but current version is {}",
                self.plugin.id, required, current_version
            ))));
        }
        Ok(())
    }
}

pub struct ManifestParser;

impl ManifestParser {
    pub fn parse(path: &Path) -> Result<PluginManifest, CoreError> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            CoreError::storage(StorageError::io(
                path.to_string_lossy().to_string(),
                "read".to_string(),
                e.to_string(),
            ))
        })?;

        let manifest: PluginManifest = toml::from_str(&content).map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "Failed to parse manifest '{}': {}",
                path.display(),
                e
            )))
        })?;

        manifest.validate()?;

        manifest.check_engine_compatibility(env!("CARGO_PKG_VERSION"))?;

        Ok(manifest)
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // v1 缺陷修复：固定临时文件名导致并行测试互相覆盖（读到他测的文件内容，
    // 使 parse 结果不定）；改用进程内原子计数器保证唯一
    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn write_temp_toml(content: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir();
        let seq = TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let path = dir.join(format!(
            "test_rdata_plugin_{}_{}.toml",
            std::process::id(),
            seq
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_parse_valid_manifest_with_all_fields() {
        let toml_content = r#"
[plugin]
id = "com.example.test"
name = "Test Plugin"
version = "1.0.0"
publisher = "Example Corp"
description = "A test plugin"
icon = "icon.png"
homepage = "https://example.com"
license = "MIT"

[plugin.engines]
rdatastation = "^0.1.0"

[capabilities.frontend]
entry = "./extension.ts"
activation_events = ["onStartup"]

[capabilities.wasm]
entry = "./plugin.wasm"
max_memory_mb = 256
max_cpu_time_ms = 15000
allowed_host_functions = ["db_query"]

[permissions]
frontend = ["data:query"]
wasm = ["wasm:db_query"]

[[contributes.commands]]
id = "test.hello"
title = "Say Hello"
category = "Test"
icon = "hand"
shortcut = "Ctrl+H"

[[contributes.panels]]
id = "test.panel"
title = "Test Panel"
location = "right"
icon = "flask"
order = 100

[[contributes.drivers]]
id = "testdb"
display_name = "Test Database"
default_port = 9999
features = ["tables", "views"]

[[contributes.settings]]
key = "test.enabled"
type = "boolean"
default = true
label = "Enable Test"
description = "Enable or disable the test feature"

[[dependencies]]
id = "com.example.base"
version = "^1.0.0"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);

        assert!(result.is_ok(), "Expected Ok but got: {:?}", result.err());

        let manifest = result.unwrap();
        assert_eq!(manifest.plugin.id, "com.example.test");
        assert_eq!(manifest.plugin.name, "Test Plugin");
        assert_eq!(manifest.plugin.version, "1.0.0");
        assert_eq!(manifest.plugin.publisher, "Example Corp");
        assert_eq!(manifest.plugin.description, "A test plugin");
        assert_eq!(manifest.plugin.icon, Some("icon.png".to_string()));
        assert_eq!(
            manifest.plugin.homepage,
            Some("https://example.com".to_string())
        );
        assert_eq!(manifest.plugin.license, Some("MIT".to_string()));
        assert_eq!(manifest.plugin.engines.rdatastation, "^0.1.0");

        let frontend = manifest.capabilities.frontend.unwrap();
        assert_eq!(frontend.entry, "./extension.ts");
        assert_eq!(frontend.activation_events, vec!["onStartup"]);

        let wasm = manifest.capabilities.wasm.unwrap();
        assert_eq!(wasm.entry, "./plugin.wasm");
        assert_eq!(wasm.max_memory_mb, Some(256));
        assert_eq!(wasm.max_cpu_time_ms, Some(15000));
        assert_eq!(wasm.allowed_host_functions, vec!["db_query"]);

        assert_eq!(manifest.permissions.frontend, vec!["data:query"]);
        assert_eq!(manifest.permissions.wasm, vec!["wasm:db_query"]);

        assert_eq!(manifest.contributes.commands.len(), 1);
        assert_eq!(manifest.contributes.commands[0].id, "test.hello");

        assert_eq!(manifest.contributes.panels.len(), 1);
        assert_eq!(manifest.contributes.panels[0].id, "test.panel");
        assert_eq!(manifest.contributes.panels[0].location, "right");

        assert_eq!(manifest.contributes.drivers.len(), 1);
        assert_eq!(manifest.contributes.drivers[0].id, "testdb");

        assert_eq!(manifest.contributes.settings.len(), 1);
        assert_eq!(manifest.contributes.settings[0].key, "test.enabled");

        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].id, "com.example.base");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_parse_manifest_missing_plugin_id() {
        let toml_content = r#"
[plugin]
# id is intentionally missing
name = "Test Plugin"
version = "1.0.0"
publisher = "Example Corp"

[plugin.engines]
rdatastation = "^0.1.0"

[capabilities.frontend]
entry = "./extension.ts"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("plugin.id") || msg.contains("missing field"),
            "Error should mention plugin.id, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_parse_manifest_incompatible_engine_version() {
        let toml_content = r#"
[plugin]
id = "com.example.test"
name = "Test Plugin"
version = "1.0.0"
publisher = "Example Corp"

[plugin.engines]
rdatastation = "^99.0.0"

[capabilities.frontend]
entry = "./extension.ts"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("requires engine version") || msg.contains("version"),
            "Error should mention version incompatibility, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_parse_manifest_missing_publisher() {
        let toml_content = r#"
[plugin]
id = "com.example.test"
name = "Test Plugin"
version = "1.0.0"
# publisher intentionally missing

[plugin.engines]
rdatastation = "^0.1.0"

[capabilities.frontend]
entry = "./extension.ts"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("publisher") || msg.contains("missing field"),
            "Error should mention publisher, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_parse_manifest_missing_capabilities() {
        let toml_content = r#"
[plugin]
id = "com.example.test"
name = "Test Plugin"
version = "1.0.0"
publisher = "Example Corp"

[plugin.engines]
rdatastation = "^0.1.0"
"#;

        let path = write_temp_toml(toml_content);
        let manifest: PluginManifest = toml::from_str(toml_content).unwrap();
        let result = manifest.validate();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("runtime form"),
            "Error should mention the runtime forms, got: {}",
            msg
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_check_engine_compatibility_same_major() {
        let manifest = PluginManifest {
            plugin: PluginMeta {
                id: "test".into(),
                name: "test".into(),
                version: "1.0.0".into(),
                publisher: "test".into(),
                description: String::new(),
                icon: None,
                homepage: None,
                license: None,
                engines: PluginEngines {
                    rdatastation: "^0.1.0".into(),
                },
            },
            capabilities: PluginCapabilities {
                frontend: Some(CapabilitiesFrontend {
                    entry: "test.js".into(),
                    activation_events: vec![],
                }),
                wasm: None,
                driver: None,
            },
            permissions: PluginPermissions::default(),
            contributes: PluginContributes::default(),
            dependencies: vec![],
            backend: None,
        };

        assert!(manifest.check_engine_compatibility("0.1.0").is_ok());
        assert!(manifest.check_engine_compatibility("0.2.0").is_ok());
        assert!(manifest.check_engine_compatibility("0.10.0").is_ok());
    }

    #[test]
    fn test_check_engine_compatibility_different_major() {
        let manifest = PluginManifest {
            plugin: PluginMeta {
                id: "test".into(),
                name: "test".into(),
                version: "1.0.0".into(),
                publisher: "test".into(),
                description: String::new(),
                icon: None,
                homepage: None,
                license: None,
                engines: PluginEngines {
                    rdatastation: "^1.0.0".into(),
                },
            },
            capabilities: PluginCapabilities {
                frontend: Some(CapabilitiesFrontend {
                    entry: "test.js".into(),
                    activation_events: vec![],
                }),
                wasm: None,
                driver: None,
            },
            permissions: PluginPermissions::default(),
            contributes: PluginContributes::default(),
            dependencies: vec![],
            backend: None,
        };

        assert!(manifest.check_engine_compatibility("1.0.0").is_ok());
        assert!(manifest.check_engine_compatibility("0.5.0").is_err());
    }

    #[test]
    fn test_default_values() {
        let toml_content = r#"
[plugin]
id = "minimal.plugin"
name = "Minimal"
version = "1.0.0"
publisher = "Minimal Corp"

[plugin.engines]
rdatastation = "^0.1.0"

[capabilities.wasm]
entry = "./plugin.wasm"
"#;

        let path = write_temp_toml(toml_content);
        let result = ManifestParser::parse(&path);
        assert!(result.is_ok());

        let manifest = result.unwrap();
        let wasm = manifest.capabilities.wasm.unwrap();
        assert_eq!(wasm.max_memory_mb, None);
        assert_eq!(wasm.max_cpu_time_ms, None);
        assert!(wasm.allowed_host_functions.is_empty());
        assert!(manifest.dependencies.is_empty());
        assert!(manifest.contributes.commands.is_empty());

        let _ = std::fs::remove_file(&path);
    }

    // ========== [backend]：怎么跑（P1） ==========

    /// 与 `plugin-dev-plan.md` §4.3 的样例一致：那段 TOML 就是契约，
    /// 改字段名这份测试得先红（省掉的只有可选的 `interpreter` 行，另有专项测试）。
    const BACKEND_SAMPLE: &str = r#"
[plugin]
id = "com.example.sqlserver"
name = "SQL Server Driver"
version = "1.0.0"
publisher = "example"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind          = "sidecar"
executable    = "bin/agent"
transport     = "framed"
protocol      = "rds-driver/1"
platforms     = ["linux-x64", "darwin-arm64", "darwin-x64", "win-x64"]
max_instances = 1

[[contributes.drivers]]
id = "mssql"
display_name = "SQL Server"
default_port = 1433

[capabilities.driver]
concurrency = "parallel"
"#;

    #[test]
    fn backend_parses_the_documented_sample() {
        let manifest: PluginManifest = toml::from_str(BACKEND_SAMPLE).expect("样例应当能解析");
        // 只有 [backend] 也是合法清单（不必再去凑 capabilities.frontend / wasm）
        manifest.validate().expect("sidecar 形态应当自洽");

        let backend = manifest.backend.as_ref().expect("应当有 [backend]");
        assert_eq!(backend.kind, BackendKind::Sidecar);
        assert_eq!(backend.kind.name(), "sidecar");
        assert!(backend.kind.is_subprocess());
        assert_eq!(backend.executable.as_deref(), Some("bin/agent"));
        assert_eq!(backend.transport, BackendTransport::Framed);
        assert_eq!(backend.protocol.as_deref(), Some("rds-driver/1"));
        assert_eq!(backend.platforms.len(), 4);
        assert_eq!(backend.max_instances, 1);
        assert!(backend.supports_current_platform(), "样例列了四个主流平台");

        let spec = manifest.process_spec().expect("sidecar 应当进进程池");
        assert_eq!(spec.drivers, vec!["mssql".to_string()]);
        assert_eq!(spec.max_instances, 1);
        assert_eq!(spec.concurrency, Concurrency::Parallel);
    }

    #[test]
    fn backend_defaults_are_conservative() {
        let manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "com.example.min"
name = "Minimal Sidecar"
version = "1.0.0"
publisher = "example"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
executable = "bin/agent"
"#,
        )
        .unwrap();
        manifest.validate().expect("最小声明应当合法");

        let backend = manifest.backend.as_ref().unwrap();
        assert_eq!(backend.transport, BackendTransport::Framed, "帧是默认传输");
        assert_eq!(backend.max_instances, 1, "默认单实例");
        assert_eq!(backend.protocol, None);
        assert!(backend.platforms.is_empty(), "未声明平台不阻挡");

        let spec = manifest.process_spec().unwrap();
        assert_eq!(spec.concurrency, Concurrency::Serial, "默认串行");
        assert!(
            spec.drivers.is_empty(),
            "没声明 driver 就是空集（不是任意都放行）"
        );
    }

    #[test]
    fn backend_command_resolves_sidecar_and_script() {
        let dir = Path::new("plugins/com.example.agent");

        let sidecar: PluginBackend = toml::from_str(
            r#"
kind = "sidecar"
executable = "bin/agent"
"#,
        )
        .unwrap();
        let command = sidecar.command(dir).expect("sidecar 应当能解析");
        assert!(
            command.program.ends_with("bin/agent"),
            "{:?}",
            command.program
        );
        assert!(command.args.is_empty());

        let script: PluginBackend = toml::from_str(
            r#"
kind = "script"
executable = "agent.py"
interpreter = "python3"
"#,
        )
        .unwrap();
        let command = script.command(dir).expect("script 应当能解析");
        assert_eq!(
            command.program,
            PathBuf::from("python3"),
            "解释器按 PATH 解析"
        );
        assert_eq!(command.args.len(), 1);
        assert!(
            Path::new(&command.args[0]).ends_with("agent.py"),
            "{:?}",
            command.args
        );
    }

    /// 不支持的传输 / 平台要在**解析完、起进程前**就说清原因（「不得静默退化」）。
    #[test]
    fn backend_command_refuses_jsonl_and_foreign_platforms() {
        let dir = Path::new("plugins/com.example.agent");

        let jsonl: PluginBackend = toml::from_str(
            r#"
kind = "sidecar"
executable = "bin/agent"
transport = "jsonl"
"#,
        )
        .unwrap();
        let err = jsonl.command(dir).unwrap_err().to_string();
        assert!(err.contains("jsonl") && err.contains("framed"), "{err}");

        let foreign: PluginBackend = toml::from_str(
            r#"
kind = "sidecar"
executable = "bin/agent"
platforms = ["plan9-sparc"]
"#,
        )
        .unwrap();
        let err = foreign.command(dir).unwrap_err().to_string();
        assert!(err.contains(&current_platform()), "要说清是哪个平台：{err}");

        // wasm 形态没有进程可起
        let wasm: PluginBackend = toml::from_str("kind = \"wasm\"\n").unwrap();
        assert!(wasm.command(dir).is_err());
    }

    /// 第三方清单给的路径必须待在自己目录里（与 `paths::validate_plugin_id` 同一个理由）。
    #[test]
    fn backend_executable_may_not_escape_the_plugin_dir() {
        for declared in ["../evil", "bin/../../evil", "/abs/agent", ""] {
            let backend: PluginBackend = toml::from_str(&format!(
                "kind = \"sidecar\"\nexecutable = \"{declared}\"\n"
            ))
            .unwrap();
            assert!(
                backend.relative_executable().is_err(),
                "{declared:?} 应当被拒"
            );
        }

        // 校验期就要挡住，不是等到起进程
        let manifest: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "com.example.evil"
name = "Evil"
version = "1.0.0"
publisher = "x"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "sidecar"
executable = "../evil.exe"
"#,
        )
        .unwrap();
        assert!(manifest.validate().is_err(), "逃出插件目录的执行文件应被拒");
    }

    #[test]
    fn wasm_backend_requires_capabilities_wasm_until_p3() {
        let missing_entry: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "com.example.wasm"
name = "Wasm"
version = "1.0.0"
publisher = "x"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "wasm"
"#,
        )
        .unwrap();
        assert!(
            missing_entry.validate().is_err(),
            "P3 之前 wasm 入口仍在 capabilities.wasm"
        );

        let ok: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "com.example.wasm"
name = "Wasm"
version = "1.0.0"
publisher = "x"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "wasm"

[capabilities.wasm]
entry = "plugin.wasm"
"#,
        )
        .unwrap();
        ok.validate().expect("有入口就合法");
        assert!(ok.process_spec().is_none(), "wasm 不进进程池");
    }

    #[test]
    fn process_spec_is_none_without_a_subprocess_backend() {
        let frontend_only: PluginManifest = toml::from_str(
            r#"
[plugin]
id = "com.example.ui"
name = "UI"
version = "1.0.0"
publisher = "x"

[plugin.engines]
rdatastation = ">=0.1"

[capabilities.frontend]
entry = "main.js"
"#,
        )
        .unwrap();
        frontend_only.validate().unwrap();
        assert!(frontend_only.process_spec().is_none());
    }

    /// Windows 上写 `bin/agent`（不写 `.exe`）应当能找到 `bin/agent.exe`。
    #[cfg(windows)]
    #[test]
    fn windows_finds_the_exe_suffix_when_it_is_not_declared() {
        let dir = std::env::temp_dir().join(format!(
            "rds_plugin_backend_{}_{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin").join("agent.exe"), b"").unwrap();

        let backend: PluginBackend = toml::from_str(
            r#"
kind = "sidecar"
executable = "bin/agent"
"#,
        )
        .unwrap();
        let command = backend.command(&dir).unwrap();
        assert_eq!(
            command.program.file_name().and_then(|n| n.to_str()),
            Some("agent.exe")
        );

        // 没写后缀、带后缀的也不存在时，原样返回（错误信息里好对照）
        std::fs::remove_file(dir.join("bin").join("agent.exe")).unwrap();
        let command = backend.command(&dir).unwrap();
        assert_eq!(
            command.program.file_name().and_then(|n| n.to_str()),
            Some("agent")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
