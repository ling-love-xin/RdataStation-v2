//! 插件目录布局与路径解析（**唯一入口**）
//!
//! 约定见 `docs/architecture/plugin/plugin-layout.md`；这里是它在代码里的落点。
//! 目录即插件（`install_dir/<id>/plugin.toml`），所以清单里任何"指文件"的字段
//! 都必须满足同一组规则：
//!
//! 1. **相对插件目录**，不得是绝对路径 / 盘符 / `..`；
//! 2. 一律 POSIX `/` 分隔（反斜杠在 Linux 上不是分隔符，跨平台包会当场裂开）；
//! 3. 解析结果必须仍在插件目录内（逐段白名单，等价于"不许逃逸"）；
//! 4. 入口文件必须真实存在（安装期检查；热拷贝时由激活如实报错）。
//!
//! 为什么集中在这里：此前只有 `backend.executable` 过了路径闸，`wasm.entry`
//! 是直接 `join`（`entry = "../../x.wasm"` 能读到插件目录外）；`icon` /
//! `frontend.entry` / `connection_schema` 一旦接上消费方就是同一个洞。
//!
//! 多平台：`platforms` 是矩阵，`executable` 是单值。约定 `bin/<platform>/agent`
//! 优先、`bin/agent` 回退（Windows 再补 `.exe`），这样一份包可以同时带多个平台的
//! 二进制，清单不用为平台分叉。

use std::path::{Component, Path, PathBuf};

use shared::error::{CommonError, CoreError};

use crate::manifest::{BackendKind, PluginManifest, current_platform};

/// 布局类错误（消息面向插件作者，给出字段名与期望形态）。
fn layout_error(message: impl Into<String>) -> CoreError {
    CoreError::common(CommonError::General(message.into()))
}

/// 安装时跳过的开发残留目录（包内容只放运行产物，见 `plugin-layout.md` §5 规则 4）。
///
/// 这些是"仓库里的东西"而不是"包里的东西"：`.git` 可能带整段历史，
/// `target` / `node_modules` / `__pycache__` 是构建与依赖缓存，装进安装目录只会变胖。
/// `src/` 不在此列 —— 脚本类插件的入口本身就可能写在 `src/` 里，误排除会静默丢文件。
pub const DEV_RESIDUE_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".rds",
    ".idea",
    ".vscode",
    "target",
    "node_modules",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
];

/// 安装时跳过的开发残留文件（IDE / 系统残留）。
pub const DEV_RESIDUE_FILES: &[&str] = &[".DS_Store", "Thumbs.db"];

/// 某个名字是否属于开发残留（目录与文件两张表；大小写不敏感 —— Windows 文件系统本就忽略大小写）。
pub fn is_dev_residue(name: &str, is_dir: bool) -> bool {
    let table = if is_dir {
        DEV_RESIDUE_DIRS
    } else {
        DEV_RESIDUE_FILES
    };
    table.iter().any(|item| item.eq_ignore_ascii_case(name))
}

/// 相对键（POSIX `/`）是否落在开发残留目录下（如 `target/junk`）。
pub fn is_residue_key(key: &str) -> bool {
    key.split('/').next().is_some_and(|first| {
        DEV_RESIDUE_DIRS
            .iter()
            .any(|d| d.eq_ignore_ascii_case(first))
    })
}

/// 校验清单声明的相对路径，返回规范化后的路径（去掉 `./` 前缀）。
///
/// `field` 只用于报错定位（如 `backend.executable` / `capabilities.wasm.entry`）。
pub fn validate_relative_path(field: &str, declared: &str) -> Result<PathBuf, CoreError> {
    if declared.is_empty() {
        return Err(layout_error(format!("{field} 不能为空")));
    }
    if declared.trim() != declared {
        return Err(layout_error(format!(
            "{field} = {declared:?} 首尾不能有空白"
        )));
    }
    // 反斜杠在 Linux/macOS 上是普通字符，写进清单会让同一个包在两种平台上
    // 解析出不同的目录层级 —— 当场拒绝，避免"Windows 上能跑、Linux 上找不到"。
    if declared.contains('\\') {
        return Err(layout_error(format!(
            "{field} = {declared:?} 必须用 POSIX `/` 分隔（不得出现反斜杠）"
        )));
    }

    let raw = Path::new(declared);
    if raw.is_absolute() {
        return Err(layout_error(format!(
            "{field} = {declared:?} 必须是相对插件目录的路径（不得是绝对路径）"
        )));
    }

    let mut seen_normal = false;
    for component in raw.components() {
        match component {
            // 开头的 `./` 跳过（`./bin/agent` 等价于 `bin/agent`）。注意
            // `Path::components()` 本来就会把中间的 `.` 归一化掉，所以只有
            // 位置上的需要处理；`.` 不构成逃逸。
            Component::CurDir if !seen_normal => {}
            Component::Normal(_) => seen_normal = true,
            // `..` / 根 / 盘符（Windows 的 `C:`）全部拒绝
            _ => {
                return Err(layout_error(format!(
                    "{field} = {declared:?} 不得包含 `..` / 根 / 盘符（必须待在插件目录内）"
                )));
            }
        }
    }

    if !seen_normal {
        return Err(layout_error(format!(
            "{field} = {declared:?} 没有有效的文件段"
        )));
    }

    // **保留清单原样的分隔符**，只去掉开头的 `./`：Windows 上 `PathBuf::join`
    // 会给相对段补反斜杠，把清单里的 `lib/x.jar` 渲染成 `lib\x.jar`，改变
    // 运行期字符串（两者都是合法路径，但没理由改）。
    let trimmed = declared.trim_start_matches("./");
    // 去前缀后**再校验一次**：`".//evil"` 去前缀会留下 `/evil`，join 时直接
    // 替换掉插件目录（PathBuf::join 对绝对路径就是这个语义），必须当场挡住。
    if trimmed.is_empty()
        || Path::new(trimmed).is_absolute()
        || !Path::new(trimmed)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(layout_error(format!(
            "{field} = {declared:?} 规范化后不是合法的相对文件路径"
        )));
    }
    Ok(PathBuf::from(trimmed))
}

/// 清单路径 → 插件目录内的绝对路径（只拼路径，不碰文件系统）。
pub fn resolve_asset(plugin_dir: &Path, field: &str, declared: &str) -> Result<PathBuf, CoreError> {
    let relative = validate_relative_path(field, declared)?;
    Ok(plugin_dir.join(relative))
}

/// 平台变体：`bin/agent` → `bin/<platform>/agent`（顶层单文件没有父目录，返回 `None`）。
fn platform_variant(relative: &Path, platform: &str) -> Option<PathBuf> {
    let file = relative.file_name()?;
    let parent = relative.parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }
    Some(parent.join(platform).join(file))
}

/// 存在即返回；Windows 上允许"只写了无后缀名、实际只有 `.exe`"的清单（不改变清单形态）。
fn existing_file(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    if cfg!(windows) && path.extension().is_none() {
        let with_exe = path.with_extension("exe");
        if with_exe.is_file() {
            return Some(with_exe);
        }
    }
    None
}

/// 可执行入口解析：`bin/<platform>/agent` 优先 → `bin/agent` → Windows `.exe` 补全。
///
/// 都没找到时返回**基础路径**（不是平台变体），让报错里显示清单原样的目标，
/// 而不是我们凭空拼出来的候选。
pub fn resolve_executable(
    plugin_dir: &Path,
    field: &str,
    declared: &str,
) -> Result<PathBuf, CoreError> {
    let relative = validate_relative_path(field, declared)?;

    if let Some(variant) = platform_variant(&relative, &current_platform()) {
        if let Some(found) = existing_file(&plugin_dir.join(&variant)) {
            return Ok(found);
        }
    }

    let base = plugin_dir.join(&relative);
    Ok(existing_file(&base).unwrap_or(base))
}

/// 目录名必须等于清单 `plugin.id`。
///
/// 热插拔 diff 按**目录名**建 key、加载与卸载按**清单 id** 建 key；两者不一致时
/// 会出现"加载成功但 diff 认不出、卸载删不到目录"（`uninstall` 删的是
/// `install_dir/<id>`）。这条不变式在加载入口一次性挡住，比事后对账便宜。
pub fn validate_dir_name(plugin_dir: &Path, plugin_id: &str) -> Result<(), CoreError> {
    let Some(dir_name) = plugin_dir.file_name().and_then(|name| name.to_str()) else {
        // 路径没有文件名（根目录 / 空路径）：不是正常插件目录，交给上游报"找不到清单"
        return Ok(());
    };
    if dir_name == plugin_id {
        return Ok(());
    }
    Err(layout_error(format!(
        "插件目录名与清单 id 不一致：目录 {dir_name:?}，清单 {plugin_id:?}。\
         目录必须命名为清单里的 plugin.id（扫描与卸载都按它索引）"
    )))
}

/// 安装期检查：清单声明的入口文件必须在包里真实存在。
///
/// 只检查**入口**（进程可执行 / wasm 模块 / 前端模块）。图标、schema 这类
/// 展示或声明式资产缺失不阻断安装（有各自回退），但路径形态仍由
/// [`PluginManifest::validate`] 校验。
pub fn check_declared_entries(
    manifest: &PluginManifest,
    plugin_dir: &Path,
) -> Result<(), CoreError> {
    let mut missing: Vec<String> = Vec::new();

    if let Some(backend) = &manifest.backend {
        if matches!(backend.kind, BackendKind::Sidecar | BackendKind::Script) {
            let declared = backend.executable.as_deref().unwrap_or_default();
            let resolved = resolve_executable(plugin_dir, "backend.executable", declared)?;
            if !resolved.is_file() {
                missing.push(format!(
                    "backend.executable = {declared}（解析为 {}）",
                    resolved.display()
                ));
            }
        }
    }

    if let Some(wasm) = &manifest.capabilities.wasm {
        let resolved = resolve_asset(plugin_dir, "capabilities.wasm.entry", &wasm.entry)?;
        if !resolved.is_file() {
            missing.push(format!(
                "capabilities.wasm.entry = {}（解析为 {}）",
                wasm.entry,
                resolved.display()
            ));
        }
    }

    if let Some(frontend) = &manifest.capabilities.frontend {
        let resolved = resolve_asset(plugin_dir, "capabilities.frontend.entry", &frontend.entry)?;
        if !resolved.is_file() {
            missing.push(format!(
                "capabilities.frontend.entry = {}（解析为 {}）",
                frontend.entry,
                resolved.display()
            ));
        }
    }

    if missing.is_empty() {
        return Ok(());
    }
    Err(layout_error(format!(
        "插件包缺少声明的入口文件：\n - {}",
        missing.join("\n - ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
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
                "rds-plugin-layout-{tag}-{}-{nanos}",
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

    fn touch(dir: &Path, relative: &str, body: &str) {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn manifest(extra: &str) -> PluginManifest {
        let toml = format!(
            r#"
[plugin]
id = "com.example.layout"
name = "Layout"
version = "1.0.0"
publisher = "test"

[plugin.engines]
rdatastation = ">=0.1"
{extra}
"#
        );
        toml::from_str(&toml).unwrap()
    }

    #[test]
    fn accepts_normal_and_dot_slash_paths() {
        assert_eq!(
            validate_relative_path("f", "bin/agent").unwrap(),
            PathBuf::from("bin/agent")
        );
        assert_eq!(
            validate_relative_path("f", "./bin/agent").unwrap(),
            PathBuf::from("bin/agent")
        );
        assert_eq!(
            validate_relative_path("f", "schemas/mssql.json").unwrap(),
            PathBuf::from("schemas/mssql.json")
        );
    }

    #[test]
    fn rejects_escaping_absolute_and_backslash_paths() {
        for bad in [
            "../evil",
            "bin/../evil",
            "/etc/passwd",
            "C:/evil",
            "bin\\agent",
            // 去 `./` 前缀会留下 `/evil`：join 时直接替换掉插件目录
            ".//evil",
        ] {
            assert!(validate_relative_path("f", bad).is_err(), "{bad} 应当被拒");
        }
        assert!(validate_relative_path("f", "").is_err());
        assert!(validate_relative_path("f", " bin/agent").is_err());
        assert!(validate_relative_path("f", ".").is_err());
    }

    #[test]
    fn platform_variant_wins_when_present() {
        let dir = TempDir::new("platform");
        let platform = current_platform();
        touch(
            dir.path(),
            &format!("bin/{platform}/agent"),
            "platform-binary",
        );

        let resolved = resolve_executable(dir.path(), "backend.executable", "bin/agent").unwrap();
        assert_eq!(
            resolved,
            dir.path().join("bin").join(&platform).join("agent")
        );
    }

    #[test]
    fn falls_back_to_the_base_path() {
        let dir = TempDir::new("fallback");
        touch(dir.path(), "bin/agent", "generic-binary");

        let resolved = resolve_executable(dir.path(), "backend.executable", "bin/agent").unwrap();
        assert_eq!(resolved, dir.path().join("bin/agent"));
    }

    #[test]
    fn missing_executable_returns_the_declared_base_path() {
        let dir = TempDir::new("missing");
        let resolved = resolve_executable(dir.path(), "backend.executable", "bin/agent").unwrap();
        assert_eq!(resolved, dir.path().join("bin/agent"));
        assert!(!resolved.is_file());
    }

    #[cfg(windows)]
    #[test]
    fn windows_exe_suffix_is_applied() {
        let dir = TempDir::new("exe");
        touch(dir.path(), "bin/agent.exe", "exe-binary");

        let resolved = resolve_executable(dir.path(), "backend.executable", "bin/agent").unwrap();
        assert_eq!(resolved, dir.path().join("bin/agent.exe"));
    }

    #[test]
    fn dir_name_must_equal_plugin_id() {
        let dir = TempDir::new("dirname");
        let plugin_dir = dir.path().join("com.example.layout");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        assert!(validate_dir_name(&plugin_dir, "com.example.layout").is_ok());

        let wrong = dir.path().join("my-plugin");
        let err = validate_dir_name(&wrong, "com.example.layout").unwrap_err();
        assert!(format!("{err}").contains("目录名与清单 id 不一致"), "{err}");
    }

    #[test]
    fn check_declared_entries_reports_missing_and_accepts_render() {
        let dir = TempDir::new("entries");
        let wasm = manifest(
            r#"
[capabilities.wasm]
entry = "wasm/render.wasm"
"#,
        );
        let err = check_declared_entries(&wasm, dir.path()).unwrap_err();
        assert!(
            format!("{err}").contains("wasm/render.wasm"),
            "报错必须点名缺失入口：{err}"
        );

        touch(dir.path(), "wasm/render.wasm", "stub");
        check_declared_entries(&wasm, dir.path()).unwrap();
    }

    #[test]
    fn check_declared_entries_covers_subprocess_and_frontend() {
        let dir = TempDir::new("entries-more");
        let sidecar = manifest(
            r#"
[backend]
kind = "sidecar"
executable = "bin/agent"
"#,
        );
        touch(dir.path(), "bin/agent", "stub");
        check_declared_entries(&sidecar, dir.path()).unwrap();

        let frontend = manifest(
            r#"
[capabilities.frontend]
entry = "dist/extension.js"
"#,
        );
        let err = check_declared_entries(&frontend, dir.path()).unwrap_err();
        assert!(format!("{err}").contains("dist/extension.js"), "{err}");
        touch(dir.path(), "dist/extension.js", "// stub");
        check_declared_entries(&frontend, dir.path()).unwrap();
    }

    #[test]
    fn dev_residue_covers_repo_and_build_noise_but_keeps_src() {
        assert!(is_dev_residue("target", true));
        assert!(is_dev_residue("TARGET", true), "Windows 文件系统忽略大小写");
        assert!(is_dev_residue(".git", true));
        assert!(is_dev_residue("node_modules", true));
        assert!(is_dev_residue("Thumbs.db", false));
        assert!(!is_dev_residue("plugin.toml", false));
        // `src/` 是包内容：脚本类插件的入口可能就在里面，误排除会静默丢文件
        assert!(!is_dev_residue("src", true));

        assert!(is_residue_key("target/debug/junk"));
        assert!(!is_residue_key("bin/agent"));
        assert!(!is_residue_key("src/main.py"));
    }
}
