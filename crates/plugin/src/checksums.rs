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
use std::path::{Component, Path, PathBuf};

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
    /// 插件目录**之外**的随包文件：绝对路径 → sha256。
    ///
    /// 典型是 `[backend] driver_jars` 声明的 JDBC 包——它们按约定放在插件目录外面，
    /// 包内的 `files` 覆盖不到；换 jar 时载荷指纹能认出变化，但「坏 jar / 拷了半截」
    /// 只有这里有哈希才能发现。
    ///
    /// **可选**：不写就只核包内（旧包、开发目录照旧）。写了之后必须把
    /// `driver_jars` 声明的外部文件覆盖完整，否则完整性是假承诺。
    #[serde(default)]
    external_files: std::collections::BTreeMap<String, String>,
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

        // 安装时会被跳过的开发残留：列了也装不进去，等于是给"完整性"一个假承诺。
        // 按**任意一层**判断：`lib/__pycache__/x.pyc` 同样不会进安装目录。
        if is_residue_path(&key) {
            problems.push(format!(
                "{key} 属于安装时跳过的开发残留目录，不应列入 {CHECKSUMS_FILE}"
            ));
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

    // 插件目录**之外**的随包件（`[backend] driver_jars` 这类）：只认绝对路径的文件。
    // 目录不直接列哈希（没有"目录哈希"这回事），要求把里面每个文件单独列出来。
    let listed_externals: Vec<PathBuf> = raw
        .external_files
        .keys()
        .map(|key| PathBuf::from(key))
        .collect();
    for (declared, expected) in &raw.external_files {
        let path = PathBuf::from(declared);
        if !path.is_absolute() {
            problems.push(format!("外部文件必须是绝对路径：{declared}"));
            continue;
        }
        let expected = expected.trim();
        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            problems.push(format!(
                "{declared}：哈希必须是 64 位十六进制（当前 {expected:?}）"
            ));
            continue;
        }
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() => {
                let actual = sha256_of(&path)?;
                if !actual.eq_ignore_ascii_case(expected) {
                    problems.push(format!(
                        "外部文件哈希不匹配：{declared}（清单 {}，实际 {actual}）",
                        expected.to_ascii_lowercase()
                    ));
                }
            }
            Ok(_) => problems.push(format!(
                "外部路径不是普通文件（目录请把里面每个文件单独列出）：{declared}"
            )),
            Err(_) => problems.push(format!("外部文件不存在：{declared}")),
        }
    }

    // 声明了 `external_files` 就必须覆盖完整：少列一个，那句"完整性"就是假的。
    // 没声明（旧包 / 开发目录）不做这个要求——只有清单声明了外部依赖的老包不该因此装不上。
    if !raw.external_files.is_empty() {
        if let Some(backend) = &manifest.backend {
            for declared in &backend.driver_jars {
                let path = PathBuf::from(declared);
                if path.is_file() {
                    if !listed_externals.iter().any(|listed| same_file(listed, &path)) {
                        problems.push(format!(
                            "缺少外部校验项：{declared}（driver_jars 声明的外部 jar 必须列进 external_files）"
                        ));
                    }
                } else if path.is_dir() {
                    for file in walk_files(&path, false)? {
                        if !listed_externals.iter().any(|listed| same_file(listed, &file)) {
                            problems.push(format!(
                                "缺少外部校验项：{}（driver_jars 声明的目录下每个文件都要列出来）",
                                file.display()
                            ));
                        }
                    }
                }
            }
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
pub(crate) fn required_entries(
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

/// 判断两个路径是否指向同一个文件。
///
/// Windows 上大小写、相对写法、`\` 与 `/` 的差异都能让人写出"看起来一样"的两条路径，
/// 直接字符串比会误判成「没覆盖」；文件都在就 canonicalize 后再比。
fn same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// 键里**任意一层**落在开发残留（目录 / 未段文件）里吗。
///
/// 安装时 `installer::copy_dir` 是按目录名逐层跳的，`lib/__pycache__/x.pyc` 这种
/// 嵌套残留同样不会被复制。只看首段（`layout::is_residue_key`）会把它当正常文件，
/// 校验通过、复制却丢掉——完整性又变成假承诺。
fn is_residue_path(key: &str) -> bool {
    let segments: Vec<&str> = key.split('/').collect();
    let last = segments.len().saturating_sub(1);
    segments
        .iter()
        .enumerate()
        .any(|(index, name)| layout::is_dev_residue(name, index != last))
}

/// 递归列出目录下的普通文件（不跟随符号链接，避免环）。
///
/// `skip_dev_residue`：遇到开发残留目录直接不进（包内遍历用；外部 `driver_jars`
/// 目录照常全量列，那里不该有残留、也不该替作者猜）。
fn walk_files(dir: &Path, skip_dev_residue: bool) -> Result<Vec<PathBuf>, CoreError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .map_err(|e| checksum_error(format!("读取目录 {} 失败：{e}", current.display())))?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                checksum_error(format!("读取目录 {} 的目录项失败：{e}", current.display()))
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|e| {
                checksum_error(format!("读取 {} 的类型失败：{e}", path.display()))
            })?;
            if skip_dev_residue {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if layout::is_dev_residue(&name, file_type.is_dir()) {
                    continue;
                }
            }
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// 生成一份 `checksums.json`：包内文件 + `[backend] driver_jars` 的外部文件。
///
/// 覆盖口径与 [`verify_package`] 对齐：包内按相对路径、跳过安装时会丢掉的开发残留、
/// 跳过 `checksums.json` 自身（哈希无法自指）；外部按绝对路径逐个文件列（目录形式
/// 递归展开）。外部路径不存在直接报错——生成一份「列了不存在」的文件等于给校验埋雷。
///
/// 打包流程里调这个函数，产物原样写进插件目录根即可。
pub fn generate_package(
    manifest: &PluginManifest,
    plugin_dir: &Path,
) -> Result<String, CoreError> {
    let mut files = std::collections::BTreeMap::new();
    for file in walk_files(plugin_dir, true)? {
        let relative = file.strip_prefix(plugin_dir).map_err(|_| {
            checksum_error(format!(
                "内部错误：遍历出的路径不在插件目录内：{}",
                file.display()
            ))
        })?;
        let key = normalized_key(relative);
        if key.is_empty() || key == CHECKSUMS_FILE || is_residue_path(&key) {
            continue;
        }
        files.insert(key, sha256_of(&file)?);
    }

    let mut external_files = std::collections::BTreeMap::new();
    if let Some(backend) = &manifest.backend {
        for declared in &backend.driver_jars {
            let path = PathBuf::from(declared);
            if !path.is_absolute() {
                return Err(checksum_error(format!(
                    "driver_jars 必须是绝对路径，无法生成校验项：{declared}"
                )));
            }
            if path.is_file() {
                external_files.insert(declared.clone(), sha256_of(&path)?);
            } else if path.is_dir() {
                for file in walk_files(&path, false)? {
                    external_files.insert(file.display().to_string(), sha256_of(&file)?);
                }
            } else {
                return Err(checksum_error(format!(
                    "driver_jars 指向的路径不存在，无法生成校验项：{declared}"
                )));
            }
        }
    }

    // 没有外部件时**不输出** `external_files` 键：旧版宿主是 `deny_unknown_fields`，
    // 多一个空键就会把整份文件判成非法 JSON。空表与"不写"在这里等价。
    let mut document = serde_json::Map::new();
    document.insert(
        "algorithm".to_string(),
        serde_json::Value::String(SUPPORTED_ALGORITHM.to_string()),
    );
    document.insert(
        "files".to_string(),
        serde_json::to_value(&files)
            .map_err(|e| checksum_error(format!("序列化文件表失败：{e}")))?,
    );
    if !external_files.is_empty() {
        document.insert(
            "external_files".to_string(),
            serde_json::to_value(&external_files)
                .map_err(|e| checksum_error(format!("序列化外部文件表失败：{e}")))?,
        );
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(document))
        .map_err(|e| checksum_error(format!("序列化 {CHECKSUMS_FILE} 失败：{e}")))
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

    /// 安装时会被跳过的开发残留不能列入：否则校验通过、复制却丢掉，完整性是假承诺。
    #[test]
    fn rejects_dev_residue_entries() {
        let dir = TempDir::new("residue");
        write(dir.path(), "plugin.toml", "stub");
        write_checksums(
            dir.path(),
            serde_json::json!({
                "plugin.toml": digest(dir.path(), "plugin.toml"),
                "target/debug/junk": "0".repeat(64),
            }),
        );

        let err = verify_package(&manifest(""), dir.path()).unwrap_err();
        let text = format!("{err}");
        assert!(
            text.contains("开发残留") && text.contains("target/debug/junk"),
            "{text}"
        );
    }

    /// 插件目录外的 JDBC 包：在 TempDir 根下建 `lib/`，与插件目录平级。
    fn external_jar(temp: &Path, name: &str) -> PathBuf {
        let lib = temp.join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        let jar = lib.join(name);
        std::fs::write(&jar, "jar-bytes").unwrap();
        jar
    }

    /// 带 `driver_jars` 的清单（TOML 里用单引号写 Windows 绝对路径，避免转义）
    fn manifest_with_jars(jars: &[&Path]) -> PluginManifest {
        let list = jars
            .iter()
            .map(|jar| format!("'{}'", jar.display()))
            .collect::<Vec<_>>()
            .join(", ");
        manifest_with(
            "",
            &format!(
                r#"
[backend]
kind = "script"
runtime = "java"
executable = "bin/agent"
driver_jars = [{list}]
"#
            ),
        )
    }

    fn write_checksums_document(dir: &Path, document: serde_json::Value) {
        write(dir, CHECKSUMS_FILE, &document.to_string());
    }

    #[test]
    fn generated_checksums_round_trip_with_external_jars() {
        let temp = TempDir::new("gen-roundtrip");
        let plugin = temp.path().join("plugin");
        write(&plugin, "plugin.toml", "stub");
        write(&plugin, "bin/agent", "binary");
        let jar = external_jar(temp.path(), "postgresql.jar");

        let m = manifest_with_jars(&[&jar]);
        let json = generate_package(&m, &plugin).unwrap();
        write(&plugin, CHECKSUMS_FILE, &json);
        // 生成的东西必须自己先过关
        verify_package(&m, &plugin).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let externals = parsed["external_files"].as_object().unwrap();
        assert_eq!(externals.len(), 1, "外部 jar 应当被列进 external_files");
        assert!(
            externals.keys().any(|key| key.ends_with("postgresql.jar")),
            "{externals:?}"
        );
    }

    #[test]
    fn generated_checksums_walk_external_directories() {
        let temp = TempDir::new("gen-dir");
        let plugin = temp.path().join("plugin");
        write(&plugin, "plugin.toml", "stub");
        write(&plugin, "bin/agent", "binary");
        let lib = temp.path().join("jdbc");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("a.jar"), "a").unwrap();
        std::fs::write(lib.join("b.jar"), "b").unwrap();

        let m = manifest_with_jars(&[&lib]);
        let json = generate_package(&m, &plugin).unwrap();
        write(&plugin, CHECKSUMS_FILE, &json);
        verify_package(&m, &plugin).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["external_files"].as_object().unwrap().len(),
            2,
            "目录下的每个 jar 都要单独列出来"
        );
    }

    #[test]
    fn tampered_external_jar_is_reported() {
        let temp = TempDir::new("gen-tamper");
        let plugin = temp.path().join("plugin");
        write(&plugin, "plugin.toml", "stub");
        write(&plugin, "bin/agent", "binary");
        let jar = external_jar(temp.path(), "postgresql.jar");

        let m = manifest_with_jars(&[&jar]);
        let json = generate_package(&m, &plugin).unwrap();
        write(&plugin, CHECKSUMS_FILE, &json);
        verify_package(&m, &plugin).unwrap();

        // 有人换掉了 jar（或拷了半截）
        std::fs::write(&jar, "not-the-same-jar").unwrap();
        let err = verify_package(&m, &plugin).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("外部文件哈希不匹配"), "{text}");
        assert!(text.contains("postgresql.jar"), "{text}");
    }

    #[test]
    fn external_relative_path_is_rejected() {
        let dir = TempDir::new("ext-relative");
        write(dir.path(), "plugin.toml", "stub");
        write_checksums_document(
            dir.path(),
            serde_json::json!({
                "algorithm": "sha256",
                "files": { "plugin.toml": digest(dir.path(), "plugin.toml") },
                "external_files": { "lib/x.jar": "0".repeat(64) },
            }),
        );

        let err = verify_package(&manifest(""), dir.path()).unwrap_err();
        assert!(format!("{err}").contains("外部文件必须是绝对路径"), "{err}");
    }

    /// 声明了 `external_files` 却不覆盖 `driver_jars`：等于给完整性一个假承诺。
    #[test]
    fn declared_external_files_must_cover_driver_jars() {
        let temp = TempDir::new("ext-coverage");
        let plugin = temp.path().join("plugin");
        write(&plugin, "plugin.toml", "stub");
        write(&plugin, "bin/agent", "binary");
        let jar = external_jar(temp.path(), "postgresql.jar");
        let other = external_jar(temp.path(), "unrelated.jar");

        let m = manifest_with_jars(&[&jar]);
        write_checksums_document(
            &plugin,
            serde_json::json!({
                "algorithm": "sha256",
                "files": {
                    "plugin.toml": digest(&plugin, "plugin.toml"),
                    "bin/agent": digest(&plugin, "bin/agent"),
                },
                "external_files": { other.display().to_string(): digest(temp.path(), "lib/unrelated.jar") },
            }),
        );

        let err = verify_package(&m, &plugin).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("缺少外部校验项"), "{text}");
        assert!(text.contains("postgresql.jar"), "{text}");
    }

    /// 残留判断要按任意一层：安装器按目录名逐层跳，嵌套 `__pycache__` 也不会被复制。
    #[test]
    fn rejects_nested_dev_residue_entries() {
        let dir = TempDir::new("nested-residue");
        write(dir.path(), "plugin.toml", "stub");
        write(dir.path(), "lib/__pycache__/mod.cpython-313.pyc", "bytecode");
        write_checksums(
            dir.path(),
            serde_json::json!({
                "plugin.toml": digest(dir.path(), "plugin.toml"),
                "lib/__pycache__/mod.cpython-313.pyc": digest(dir.path(), "lib/__pycache__/mod.cpython-313.pyc"),
            }),
        );

        let err = verify_package(&manifest(""), dir.path()).unwrap_err();
        let text = format!("{err}");
        assert!(text.contains("开发残留"), "{text}");
        assert!(text.contains("lib/__pycache__"), "{text}");
    }

    #[test]
    fn generated_checksums_skip_nested_dev_residue() {
        let temp = TempDir::new("gen-nested-residue");
        let plugin = temp.path().join("plugin");
        write(&plugin, "plugin.toml", "stub");
        write(&plugin, "bin/agent", "binary");
        write(&plugin, "lib/__pycache__/mod.cpython-313.pyc", "bytecode");
        write(&plugin, "lib/real.jar", "jar");

        let m = manifest(
            r#"
[backend]
kind = "sidecar"
executable = "bin/agent"
"#,
        );
        let json = generate_package(&m, &plugin).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let files = parsed["files"].as_object().unwrap();
        assert!(
            !files.keys().any(|key| key.contains("__pycache__")),
            "生成时不该列嵌套残留：{files:?}"
        );
        assert!(files.keys().any(|key| key == "lib/real.jar"), "{files:?}");
        // 没有外部件时不写 external_files 键：旧宿主 deny_unknown_fields 会拒绝多出来的键
        assert!(
            parsed.get("external_files").is_none(),
            "无外部件时不该输出 external_files：{json}"
        );
    }
}
