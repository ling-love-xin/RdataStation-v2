//! 全局文件资产的本体落盘与历史副本（`data/system/resources/`）。
//!
//! 与项目侧 `payload` 同构：当前文件是权威副本，历史版本在
//! `resources/versions/<asset_key>/<version>/<filename>`；复制先落同卷 `.staging`，
//! 校验 hash 后原子替换，失败不留半成品。这里只管文件系统，不碰登记表。

use super::*;
use crate::payload::PayloadStore;
use tokio::fs;

/// 全局文件落盘结果（复制 + 指纹 + 体积都取自**实际落下的字节**）。
#[derive(Debug, Clone)]
pub(super) struct GlobalStagedFile {
    pub rel: String,
    pub abs: PathBuf,
    pub content_hash: String,
    pub file_size: i64,
}

/// 全局本体目录（由系统目录推出，测试注入的临时目录也走同一规则）。
pub(super) fn resources_dir(system_dir: &Path) -> PathBuf {
    system_dir.join("resources")
}

/// 历史副版本根。
pub(super) fn versions_dir(system_dir: &Path) -> PathBuf {
    resources_dir(system_dir).join("versions")
}

/// 某个资产的历史版本根（跨版本稳定）。
pub(super) fn asset_versions_dir(system_dir: &Path, asset_key: &str) -> PathBuf {
    versions_dir(system_dir).join(asset_key)
}

/// 某个资产的历史版本目录。
pub(super) fn version_dir(system_dir: &Path, asset_key: &str, version: i32) -> PathBuf {
    asset_versions_dir(system_dir, asset_key).join(format!("v{version}"))
}

/// 把文件名收成安全的单段：只保留最后一段，拒绝空名 / 点前缀。
pub(super) fn safe_filename(rel: &str) -> Result<String, CoreError> {
    let name = Path::new(rel)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.starts_with('.'))
        .ok_or_else(|| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources_global".to_string(),
                operation: "safe_filename".to_string(),
                reason: format!("非法文件名：{rel:?}"),
            })
        })?;
    Ok(name.to_string())
}

/// 复制源文件到 `resources/<rel>`，返回真实 hash / 体积。
///
/// `rel` 由调用方给出（项目侧相对路径）；缺省时用资产名 + 源扩展名补一个。
pub(super) async fn stage_and_commit(
    source: &Path,
    system_dir: &Path,
    rel: Option<&str>,
    fallback_name: &str,
) -> Result<GlobalStagedFile, CoreError> {
    if !source.is_file() {
        return Err(CoreError::storage(StorageError::Persistence {
            store: "analytics_resources_global".to_string(),
            operation: "stage_global_file".to_string(),
            reason: format!("源文件不存在或不是普通文件：{}", source.display()),
        }));
    }
    let rel = match rel.map(str::trim).filter(|rel| !rel.is_empty()) {
        Some(rel) => rel.replace('\\', "/"),
        None => {
            let ext = source
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| format!(".{ext}"))
                .unwrap_or_default();
            format!("{fallback_name}{ext}")
        }
    };
    safe_filename(&rel)?;
    let store = PayloadStore::new(system_dir.to_path_buf());
    let staged = store.stage_payload(source, &rel).await?;
    let hash = PayloadStore::content_hash_sync(&staged)?;
    let file_size = std::fs::metadata(&staged)
        .map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources_global".to_string(),
                operation: "staged_metadata".to_string(),
                reason: format!("{}: {e}", staged.display()),
            })
        })?
        .len() as i64;
    let abs = store.commit_staged(&staged, &rel).await?;
    Ok(GlobalStagedFile {
        rel,
        abs,
        content_hash: hash,
        file_size,
    })
}

/// 把当前文件存进历史版本目录；已有则不覆盖（同一版本只应有一份字节）。
pub(super) async fn store_version_copy(
    system_dir: &Path,
    asset_key: &str,
    version: i32,
    source: &Path,
    rel: &str,
) -> Result<PathBuf, CoreError> {
    if !source.is_file() {
        return Err(CoreError::storage(StorageError::Persistence {
            store: "analytics_resources_global".to_string(),
            operation: "store_global_version_copy".to_string(),
            reason: format!("当前本体不存在，无法补历史副本：{}", source.display()),
        }));
    }
    let dir = version_dir(system_dir, asset_key, version);
    fs::create_dir_all(&dir).await.map_err(|e| {
        CoreError::storage(StorageError::Persistence {
            store: "analytics_resources_global".to_string(),
            operation: "create_version_dir".to_string(),
            reason: format!("{}: {e}", dir.display()),
        })
    })?;
    let dest = dir.join(safe_filename(rel)?);
    if !dest.is_file() {
        fs::copy(source, &dest).await.map_err(|e| {
            CoreError::storage(StorageError::Persistence {
                store: "analytics_resources_global".to_string(),
                operation: "copy_version".to_string(),
                reason: format!("{}: {e}", dest.display()),
            })
        })?;
    }
    Ok(dest)
}

/// 读历史版本副本；副本已被裁剪时返回 `None`（不拿当前内容冒充）。
pub(super) fn version_copy(
    system_dir: &Path,
    asset_key: &str,
    version: i32,
    filename: &str,
) -> Option<PathBuf> {
    let path = version_dir(system_dir, asset_key, version).join(safe_filename(filename).ok()?);
    path.is_file().then_some(path)
}

/// 资产目录键：由资产名生成稳定、可读、无路径分隔符的一段。
///
/// 表型一版一行、id 会换，但文件历史目录必须跨版本稳定，所以不用行 id。
pub(super) fn asset_key(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let safe = safe.trim_matches(['.', '_', '-']);
    let safe = if safe.is_empty() { "asset" } else { safe };
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(name.as_bytes());
    format!("{safe}_{}", hex::encode(&digest[..6]))
}
