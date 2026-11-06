//! 随包完整性校验（`checksums.json`）
//!
//! 目录约定见 `docs/architecture/plugin/plugin-layout.md` §3/§7。这里只管一件事：
//! **安装时**确认包里的文件与作者发布的哈希一致，回答"这个目录里的字节还是作者签发的吗"。
//!
//! 文件可选：老包与开发目录（热插拔直接指源码目录）都不带它，缺失不阻断安装；
//! 发布包应在 P5 打包流程里生成。格式：
//!
//! ```json
//! {
//!   "algorithm": "sha256",
//!   "files": {
//!     "plugin.toml": "<64 位十六进制>",
//!     "bin/agent": "<64 位十六进制>"
//!   }
//! }
//! ```
//!
//! 两条硬规则：
//! 1. 键是**相对插件目录的 POSIX 路径**，与清单同一套白名单（不许逃逸、不许自指）；
//! 2. **覆盖率**：`plugin.toml`、清单声明的入口（进程 / wasm / 前端）必须列出；
//!    声明了且真实存在的展示资产（`plugin.icon` / `connection_schema`）也必须列出
//!    —— 否则"删掉一行哈希"就能绕过校验。
//!
//! 不在本模块职责内：`signature.json`（P5 签名，验的是"谁签发"而不是"字节是否一致"）、
//! 运行期校验（安装后目录是只读产物，由文件系统权限与卸载流程保证）。

use std::collections::BTreeSet;
use std::path::{Component, Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use shared::error::{CommonError, CoreError};

use crate::layout;
use crate::manifest::{BackendKind, PluginManifest};

/// 随包校验文件名（位于插件目录根，与 `plugin.toml` 同级）。
pub const CHECKSUMS_FILE: &str = "checksums.json";

/// 当前支持的算法（P5 只做 sha256；换算法要同时改打包工具与本文档）。
const SUPPORTED_ALGORITHM: &str = "sha256";

fn checksum_error(message: impl Into<String>) -> CoreError {
    CoreError::common(CommonError::General(message.into()))
}

/// `checksums.json` 的磁盘形态。
///
/// `deny_unknown_fields`：拼错的键（如 `file` / `hash`）会让整份校验**静默失效**，
/// 所以宁可解析失败，也不接受"看起来差不多"的结构。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChecksums {
    #[serde(default = "default_algorithm")]
    algorithm: String,
    files: std::collections::BTreeMap<String, String>,
}

fn default_algorithm() -> String {
    SUPPORTED_ALGORITHM.to_string()
}

/// 安装期校验：`checksums.json` 存在则逐文件比对哈希，并检查覆盖率。
///
/// 调用点在 `installer::validate_plugin`（复制目录**之前**，校验的是解压产物/源目录）。
/// 文件缺失时直接放行（见模块注释里的兼容口径）。
pub fn verify_package(manifest: &PluginManifest, plugin_dir: &Path) -> Result<(), CoreError> {
    let path = plugin_dir.join(CHECKSUMS_FILE);
    if !path.is_file() {
        return Ok(());
    }

    let text = std::fs::read_to_string(&path).map_err(|e| {
        checksum_error(format!(
            "读取 {CHECKSUMS_FILE} 失败：{e}（{}）",
            path.display()
        ))
    })?;
    let raw: RawChecksums = serde_json::from_str(&text)
        .map_err(|e| checksum_error(format!("{CHECKSUMS_FILE} 不是合法的 JSON：{e}")))?;

    if !raw.algorithm.eq_ignore_ascii_case(SUPPORTED_ALGORITHM) {
        return Err(checksum_error(format!(
            "{CHECKSUMS_FILE} 只支持 {SUPPORTED_ALGORITHM}（当前声明 {:?}）",
            raw.algorithm
        )));
    }
    if raw.files.is_empty() {
        return Err(checksum_error(format!(
            "{CHECKSUMS_FILE} 没有列出任何文件（列了也等于没校验）"
        )));
    }

    let mut problems: Vec<String> = Vec::new();
    let mut listed: BTreeSet<String> = BTreeSet::new();

    for (declared, expected) in &raw.files {
        let relative = match layout::validate_relative_path(CHECKSUMS_FILE, declared) {
            Ok(relative) => relative,
            Err(e) => {
                problems.push(format!("{declared:?}：{e}"));
                continue;
            }
        };
        let key = normalized_key(&relative);

        // 自指：文件内容包含自身哈希，写不出来也不该被接受。
        if key == CHECKSUMS_FILE {
            problems.push(format!(
                "{CHECKSUMS_FILE} 不能把自己列入校验（哈希无法自指）"
            ));
            continue;
        }
        if !listed.insert(key.clone()) {
            problems.push(format!("重复的校验项（规范化后同名）：{key}"));
            continue;
        }

        let file = plugin_dir.join(&relative);
        if !file.is_file() {
            problems.push(format!("列出但不是文件：{key}"));
            continue;
        }

        let expected = expected.trim();
        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            problems.push(format!(
                "{key}：哈希必须是 64 位十六进制（当前 {expected:?}）"
            ));
            continue;
        }

        let actual = sha256_of(&file)?;
        if !actual.eq_ignore_ascii_case(expected) {
            problems.push(format!(
                "哈希不匹配：{key}（清单 {}，实际 {actual}）",
                expected.to_ascii_lowercase()
            ));
        }
    }

    for required in required_entries(manifest, plugin_dir)? {
        if !listed.contains(&required) {
            problems.push(format!(
                "缺少校验项：{required}（清单声明的文件必须列在 {CHECKSUMS_FILE} 里）"
            ));
        }
    }

    if problems.is_empty() {
        return Ok(());
    }
    Err(checksum_error(format!(
        "插件包完整性校验失败（{CHECKSUMS_FILE}）：\n - {}",
        problems.join("\n - ")
    )))
}

/// 必须被校验覆盖的文件（相对插件目录、POSIX 形式）。
///
/// 入口一律用**解析后**的路径：多平台包实际跑的是 `bin/<platform>/agent`，
/// 校验它而不是清单里的基础路径（覆盖率按"真正会被执行的字节"算）。
fn required_entries(
    manifest: &PluginManifest,
    plugin_dir: &Path,
) -> Result<Vec<String>, CoreError> {
    let mut required = vec!["plugin.toml".to_string()];

    if let Some(backend) = &manifest.backend {
        if matches!(backend.kind, BackendKind::Sidecar | BackendKind::Script) {
            let declared = backend.executable.as_deref().unwrap_or_default();
            let resolved = layout::resolve_executable(plugin_dir, "backend.executable", declared)?;
            required.push(relative_key(plugin_dir, &resolved)?);
        }
    }

    if let Some(wasm) = &manifest.capabilities.wasm {
        let resolved = layout::resolve_asset(plugin_dir, "capabilities.wasm.entry", &wasm.entry)?;
        required.push(relative_key(plugin_dir, &resolved)?);
    }

    if let Some(frontend) = &manifest.capabilities.frontend {
        let resolved =
            layout::resolve_asset(plugin_dir, "capabilities.frontend.entry", &frontend.entry)?;
        required.push(relative_key(plugin_dir, &resolved)?);
    }

    // 展示 / 声明式资产：**存在才要求覆盖**。缺失不阻断安装（与
    // `layout::check_declared_entries` 的口径一致），存在则不能被悄悄替换。
    if let Some(icon) = manifest.plugin.icon.as_deref() {
        let resolved = layout::resolve_asset(plugin_dir, "plugin.icon", icon)?;
        if resolved.is_file() {
            required.push(relative_key(plugin_dir, &resolved)?);
        }
    }
    for driver in &manifest.contributes.drivers {
        if let Some(schema) = driver.connection_schema.as_deref() {
            let resolved = layout::resolve_asset(
                plugin_dir,
                "contributes.drivers[].connection_schema",
                schema,
            )?;
            if resolved.is_file() {
                required.push(relative_key(plugin_dir, &resolved)?);
            }
        }
    }

    Ok(required)
}

/// 绝对路径 → 相对插件目录的规范键（只在插件目录内成立；越界是内部错误）。
fn relative_key(plugin_dir: &Path, resolved: &Path) -> Result<String, CoreError> {
    let relative = resolved.strip_prefix(plugin_dir).map_err(|_| {
        checksum_error(format!(
            "内部错误：解析出的路径不在插件目录内：{}",
            resolved.display()
        ))
    })?;
    Ok(normalized_key(relative))
}

/// 统一成 POSIX `/` 形式的比较键。
///
/// 清单里的 `bin/agent` 与 Windows 上 `PathBuf` 渲染出的 `bin\agent` 是同一个文件，
/// 比较时必须归一，否则覆盖率会按平台误报。
fn normalized_key(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// 计算文件 sha256（小写十六进制）。
fn sha256_of(path: &Path) -> Result<String, CoreError> {
    let bytes = std::fs::read(path)
        .map_err(|e| checksum_error(format!("读取 {} 失败：{e}", path.display())))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
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
                "rds-plugin-checksums-{tag}-{}-{nanos}",
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

    fn manifest(extra: &str) -> PluginManifest {
        manifest_with("", extra)
    }

    /// `meta_extra` 落在 `[plugin]` 段（如 `icon`），`extra` 落在其后（如 `[backend]`）。
    fn manifest_with(meta_extra: &str, extra: &str) -> PluginManifest {
        let toml = format!(
            r#"
[plugin]
id = "com.example.checksums"
name = "Checksums"
version = "1.0.0"
publisher = "test"
{meta_extra}

[plugin.engines]
rdatastation = ">=0.1"
{extra}
"#
        );
        toml::from_str(&toml).unwrap()
    }

    fn digest(dir: &Path, relative: &str) -> String {
        sha256_of(&dir.join(relative)).unwrap()
    }

    fn write_checksums(dir: &Path, files: serde_json::Value) {
        let json = serde_json::json!({ "algorithm": "sha256", "files": files });
        write(dir, CHECKSUMS_FILE, &json.to_string());
    }

    #[test]
    fn absent_checksums_is_accepted() {
        let dir = TempDir::new("absent");
        write(dir.path(), "plugin.toml", "stub");
        verify_package(&manifest(""), dir.path()).unwrap();
    }

    #[test]
    fn accepts_a_fully_covered_package() {
        let dir = TempDir::new("covered");
        write(dir.path(), "plugin.toml", "stub");
        write(dir.path(), "bin/agent", "binary");
        write_checksums(
            dir.path(),
            serde_json::json!({
                "plugin.toml": digest(dir.path(), "plugin.toml"),
                "bin/agent": digest(dir.path(), "bin/agent"),
            }),
        );

        let m = manifest(
            r#"
[backend]
kind = "sidecar"
executable = "bin/agent"
"#,
        );
        verify_package(&m, dir.path()).unwrap();
    }

    #[test]
    fn reports_a_hash_mismatch_by_name() {
        let dir = TempDir::new("mismatch");
        write(dir.path(), "plugin.toml", "stub");
        write(dir.path(), "bin/agent", "tampered");
        write_checksums(
            dir.path(),
            serde_json::json!({
                "plugin.toml": digest(dir.path(), "plugin.toml"),
                "bin/agent": "0".repeat(64),
            }),
        );

        let m = manifest(
            r#"
[backend]
kind = "sidecar"
executable = "bin/agent"
"#,
        );
        let err = verify_package(&m, dir.path()).unwrap_err();
        let text = format!("{err}");
        assert!(
            text.contains("哈希不匹配") && text.contains("bin/agent"),
            "{text}"
        );
    }

    #[test]
    fn requires_covering_plugin_toml_and_declared_assets() {
        let dir = TempDir::new("coverage");
        write(dir.path(), "plugin.toml", "stub");
        write(dir.path(), "bin/agent", "binary");
        write(dir.path(), "media/icon.svg", "<svg/>");
        // 只列了入口：plugin.toml 与存在的 icon 都漏了
        write_checksums(
            dir.path(),
            serde_json::json!({ "bin/agent": digest(dir.path(), "bin/agent") }),
        );

        let m = manifest_with(
            r#"icon = "media/icon.svg""#,
            r#"
[backend]
kind = "sidecar"
executable = "bin/agent"
"#,
        );
        let err = verify_package(&m, dir.path()).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("缺少校验项：plugin.toml"), "{text}");
        assert!(text.contains("缺少校验项：media/icon.svg"), "{text}");
    }

    #[test]
    fn rejects_path_escape_and_self_reference() {
        let dir = TempDir::new("bad-keys");
        write(dir.path(), "plugin.toml", "stub");
        write_checksums(
            dir.path(),
            serde_json::json!({
                "plugin.toml": digest(dir.path(), "plugin.toml"),
                "../evil": "0".repeat(64),
                CHECKSUMS_FILE: "0".repeat(64),
            }),
        );

        let err = verify_package(&manifest(""), dir.path()).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("不得包含 `..`"), "{text}");
        assert!(text.contains("不能把自己列入校验"), "{text}");
    }

    #[test]
    fn rejects_unknown_algorithm_and_malformed_json() {
        let dir = TempDir::new("algorithm");
        write(dir.path(), "plugin.toml", "stub");

        let json = serde_json::json!({ "algorithm": "md5", "files": {} });
        write(dir.path(), CHECKSUMS_FILE, &json.to_string());
        let err = verify_package(&manifest(""), dir.path()).unwrap_err();
        assert!(format!("{err}").contains("只支持 sha256"), "{err}");

        write(dir.path(), CHECKSUMS_FILE, "{ not json");
        let err = verify_package(&manifest(""), dir.path()).unwrap_err();
        assert!(format!("{err}").contains("不是合法的 JSON"), "{err}");
    }

    #[test]
    fn rejects_listed_but_missing_files() {
        let dir = TempDir::new("missing");
        write(dir.path(), "plugin.toml", "stub");
        write_checksums(
            dir.path(),
            serde_json::json!({
                "plugin.toml": digest(dir.path(), "plugin.toml"),
                "lib/extra.jar": "0".repeat(64),
            }),
        );

        let err = verify_package(&manifest(""), dir.path()).unwrap_err();
        assert!(
            format!("{err}").contains("列出但不是文件：lib/extra.jar"),
            "{err}"
        );
    }
}
