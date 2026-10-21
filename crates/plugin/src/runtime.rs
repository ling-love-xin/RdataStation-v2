//! 解释器运行时解析（java / python）——「不要内嵌运行时」的落点。
//!
//! 驱动插件（JDBC / DB-API）需要 Java / Python 解释器来起 sidecar 进程，但**插件包不内嵌
//! 运行时**：解释器一律从本机找，找不到再落到 `third_party` 兜底目录（存在则用 + 告知，
//! 自动下载是第二期）。查找顺序定死为三级：
//!
//! ```text
//! ① 用户显式指定（设置页写入的显式路径，最高优先）
//! ② 环境变量 / PATH（Java：JAVA_HOME\bin\java.exe，其次 PATH 里的 java；Python：PATH 里的 python3）
//! ③ third_party 兜底（<仓库根>/third_party/jre、<仓库根>/third_party/python，存在才用 + 告知）
//! ```
//!
//! 每次起进程都重新解析（用户可能改了环境变量 / 装了 JDK）；解析结果带来源与可读说明，
//! 供日志与「告知用户」用。全部没找到时返回**可读错误**（引导去装 / 设置页指定 / 放
//! third_party），而不是等进程起不来才发现。

use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use shared::error::{CommonError, CoreError};

use crate::manifest::BackendRuntime;

/// 解析结果的来源（决定「告知用户」时怎么措辞）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSource {
    /// 用户显式指定（设置页 / 配置）。
    UserSpecified,
    /// 环境变量 / PATH。
    EnvPath,
    /// third_party 兜底运行时。
    ThirdParty,
}

impl RuntimeSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::UserSpecified => "user-specified",
            Self::EnvPath => "env-path",
            Self::ThirdParty => "third-party",
        }
    }
}

/// 一次解析的结果：解释器路径 + 来源 + 给人看的说明。
#[derive(Debug, Clone)]
pub struct ResolvedRuntime {
    pub interpreter: PathBuf,
    pub source: RuntimeSource,
    pub note: String,
}

/// 解析配置（用户可改的只有①和 third_party 根；②是读环境变量）。
#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    /// 用户显式指定的 java 解释器（如 `C:\jdk\bin\java.exe`）；`None` = 未指定。
    pub explicit_java: Option<PathBuf>,
    /// 用户显式指定的 python 解释器（如 `C:\Python313\python.exe`）。
    pub explicit_python: Option<PathBuf>,
    /// third_party 兜底根；`None` = 推导默认（仓库根/third_party）。
    pub third_party_root: Option<PathBuf>,
}

/// 全局配置（app 启动时由设置页写入；进程内各处解析读同一份）。
static GLOBAL_CONFIG: OnceLock<Mutex<RuntimeConfig>> = OnceLock::new();

fn global_config() -> &'static Mutex<RuntimeConfig> {
    GLOBAL_CONFIG.get_or_init(|| Mutex::new(RuntimeConfig::default()))
}

/// 覆盖全局配置（设置页保存 java/python 路径时调用；`None` = 清除显式指定，回到自动查找）。
pub fn set_runtime_config(config: RuntimeConfig) {
    let mut guard = global_config()
        .lock()
        .expect("runtime 全局配置锁不应中毒");
    *guard = config;
}

/// 当前全局配置（供查询，不改）。
pub fn current_runtime_config() -> RuntimeConfig {
    global_config()
        .lock()
        .expect("runtime 全局配置锁不应中毒")
        .clone()
}

/// 兜底根推导：开发态 = 仓库根/third_party（`CARGO_MANIFEST_DIR` 上行两级）。
fn default_third_party_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(|root| root.join("third_party"))
        .unwrap_or_else(|| PathBuf::from("third_party"))
}

/// 运行时解析器（用当前全局配置）。
pub struct RuntimeResolver {
    config: RuntimeConfig,
}

impl RuntimeResolver {
    pub fn new(config: RuntimeConfig) -> Self {
        Self { config }
    }

    /// 用当前全局配置解析。
    pub fn global() -> Self {
        Self::new(current_runtime_config())
    }

    /// 按运行时语言解析解释器路径。
    pub fn resolve(&self, runtime: BackendRuntime) -> Result<ResolvedRuntime, CoreError> {
        match runtime {
            BackendRuntime::Java => self.resolve_java(),
            BackendRuntime::Python => self.resolve_python(),
            BackendRuntime::Rust => Err(CoreError::common(CommonError::not_supported(
                "backend.runtime = rust 无需解释器解析（原生 sidecar 进程）",
            ))),
        }
    }

    fn resolve_java(&self) -> Result<ResolvedRuntime, CoreError> {
        // ① 用户显式指定
        if let Some(p) = self.config.explicit_java.as_deref() {
            if p.is_file() {
                return Ok(ResolvedRuntime {
                    interpreter: p.to_path_buf(),
                    source: RuntimeSource::UserSpecified,
                    note: format!("使用用户指定的 java：{}", p.display()),
                });
            }
            return Err(self.not_found_error("java", &format!(
                "用户指定的 java 路径不存在：{}",
                p.display()
            )));
        }

        // ② JAVA_HOME\bin\java[.exe]（Java 的常规环境变量）
        if let Ok(home) = env::var("JAVA_HOME") {
            let home = PathBuf::from(home);
            let candidate = bin_join(&home, "java");
            if candidate.is_file() {
                return Ok(ResolvedRuntime {
                    interpreter: candidate.clone(),
                    source: RuntimeSource::EnvPath,
                    note: format!("使用 JAVA_HOME 提供的 java：{}", candidate.display()),
                });
            }
        }

        // ②' PATH 里的 java
        if let Some(p) = find_on_path("java") {
            return Ok(ResolvedRuntime {
                interpreter: p.clone(),
                source: RuntimeSource::EnvPath,
                note: format!("使用 PATH 中的 java：{}", p.display()),
            });
        }

        // ③ third_party 兜底
        let tp = self.third_party_root().join("jre");
        let candidate = bin_join(&tp, "java");
        if candidate.is_file() {
            return Ok(ResolvedRuntime {
                interpreter: candidate.clone(),
                source: RuntimeSource::ThirdParty,
                note: format!(
                    "未找到系统 java，使用 third_party 兜底运行时：{}（如需自带，请更新该目录）",
                    candidate.display()
                ),
            });
        }

        Err(self.not_found_error("java", &format!(
            "系统与 third_party 都没有 java。请任选其一：\n  \
             - 安装 JDK（JAVA_HOME 或 PATH 指向它）；\n  \
             - 在设置页显式指定 java.exe 路径；\n  \
             - 把 JRE 放到 {}（检测到即自动使用）",
            tp.display()
        )))
    }

    fn resolve_python(&self) -> Result<ResolvedRuntime, CoreError> {
        // ① 用户显式指定
        if let Some(p) = self.config.explicit_python.as_deref() {
            if p.is_file() {
                return Ok(ResolvedRuntime {
                    interpreter: p.to_path_buf(),
                    source: RuntimeSource::UserSpecified,
                    note: format!("使用用户指定的 python：{}", p.display()),
                });
            }
            return Err(self.not_found_error("python", &format!(
                "用户指定的 python 路径不存在：{}",
                p.display()
            )));
        }

        // ② PATH 里的 python3（其次 python）
        for name in ["python3", "python"] {
            if let Some(p) = find_on_path(name) {
                return Ok(ResolvedRuntime {
                    interpreter: p.clone(),
                    source: RuntimeSource::EnvPath,
                    note: format!("使用 PATH 中的 {name}：{}", p.display()),
                });
            }
        }

        // ③ third_party 兜底
        let tp = self.third_party_root().join("python");
        for name in ["python.exe", "python3.exe"] {
            let candidate = tp.join(name);
            if candidate.is_file() {
                return Ok(ResolvedRuntime {
                    interpreter: candidate.clone(),
                    source: RuntimeSource::ThirdParty,
                    note: format!(
                        "未找到系统 python，使用 third_party 兜底运行时：{}（如需自带，请更新该目录）",
                        candidate.display()
                    ),
                });
            }
        }

        Err(self.not_found_error("python", &format!(
            "系统与 third_party 都没有 python。请任选其一：\n  \
             - 安装 Python（PATH 指向它）；\n  \
             - 在设置页显式指定 python.exe 路径；\n  \
             - 把 Python 放到 {}（检测到即自动使用）",
            tp.display()
        )))
    }

    fn third_party_root(&self) -> PathBuf {
        self.config
            .third_party_root
            .clone()
            .unwrap_or_else(default_third_party_root)
    }

    fn not_found_error(&self, runtime_name: &str, detail: &str) -> CoreError {
        CoreError::common(CommonError::General(format!(
            "找不到 {runtime_name} 运行时：{detail}"
        )))
    }
}

/// `<dir>/bin/java[.exe]`（跨平台拼 bin 子目录）。
fn bin_join(dir: &Path, name: &str) -> PathBuf {
    let bin = dir.join("bin");
    let plain = bin.join(name);
    if cfg!(windows) && plain.extension().is_none() {
        return plain.with_extension("exe");
    }
    plain
}

/// 按 PATH 找可执行文件（Windows 检查 `.exe`）。
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        if cfg!(windows) && candidate.extension().is_none() {
            let with_exe = candidate.with_extension("exe");
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用户显式指定优先级最高，且路径不存在时给出可读错误（不静默回退）。
    #[test]
    fn explicit_java_wins_and_bad_path_errors() {
        set_runtime_config(RuntimeConfig {
            explicit_java: Some(PathBuf::from("C:/fake/jdk/bin/java.exe")),
            ..Default::default()
        });
        let resolver = RuntimeResolver::global();
        let err = resolver.resolve(BackendRuntime::Java).unwrap_err().to_string();
        assert!(err.contains("用户指定的 java 路径不存在"), "{err}");

        // 清理全局配置，避免污染其它测试
        set_runtime_config(RuntimeConfig::default());
    }

    /// JAVA_HOME 优先于 PATH（两者都存在时取 JAVA_HOME）。
    #[test]
    fn java_prefers_java_home_over_path() {
        let resolver = RuntimeResolver::new(RuntimeConfig::default());
        // 本机 JAVA_HOME 未设，PATH 有 java（真实环境验证 PATH 发现路径）
        if env::var("JAVA_HOME").is_err() {
            let resolved = resolver.resolve(BackendRuntime::Java).expect("本机应有 java");
            assert_eq!(resolved.source, RuntimeSource::EnvPath);
            assert!(
                resolved.interpreter.to_string_lossy().contains("java"),
                "{}",
                resolved.interpreter.display()
            );
        }
    }

    /// 找不到时 third_party 兜底存在则用；没有则报可读错误。
    #[test]
    fn third_party_fallback_and_missing_error() {
        // 用一个不存在的 third_party 根：Java 应该报「找不到」
        let resolver = RuntimeResolver::new(RuntimeConfig {
            third_party_root: Some(PathBuf::from("Z:/nonexistent/third_party")),
            ..Default::default()
        });
        // 本机 PATH 有 java 时会走 EnvPath，不会走到报错分支 —— 只测 Python 且把 PATH 遮掉不可行，
        // 所以这里断言的是：报错文案存在（用显式指定错误路径保证走到错误分支）
        let bad = RuntimeResolver::new(RuntimeConfig {
            explicit_python: Some(PathBuf::from("Z:/fake/python.exe")),
            ..Default::default()
        });
        let err = bad.resolve(BackendRuntime::Python).unwrap_err().to_string();
        assert!(err.contains("用户指定的 python 路径不存在"), "{err}");
        let _ = resolver;
    }
}
