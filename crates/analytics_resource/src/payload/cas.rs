//! 内容寻址对象库（CAS）：文件本体与文件元数据的唯一真相源。
//!
//! 目录：
//! ```text
//! {项目}/.RSmeta/cas/
//! ├── blobs/<aa>/<sha256>       # 文件本体
//! ├── manifests/<aa>/<root>.json
//! └── staging/<uuid>.tmp        # 写入暂存，校验后原子转正
//! ```
//!
//! 第一版按整文件存 blob，不做 CDC 分块；`chunks/` 留作后续替换 `put_file` 内部实现，
//! manifest 的对外语义不变。

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use shared::error::{CoreError, StorageError};

use super::manifest::{canonical_file_meta, FileManifest};
use super::PayloadStore;

const CAS_DIR_NAME: &str = "cas";
const BLOBS_DIR: &str = "blobs";
const MANIFESTS_DIR: &str = "manifests";
const STAGING_DIR: &str = "staging";
const HASH_CHUNK: usize = 64 * 1024;

/// 一个已入库对象的引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasObject {
    pub hash: String,
    pub size: u64,
}

/// CAS 存储（按项目根实例化，不放进程单例）。
#[derive(Clone)]
pub struct CasStore {
    root: PathBuf,
}

impl CasStore {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        let root = project_root
            .into()
            .join(crate::payload::META_DIR_NAME)
            .join(CAS_DIR_NAME);
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn blobs_dir(&self) -> PathBuf {
        self.root.join(BLOBS_DIR)
    }

    fn manifests_dir(&self) -> PathBuf {
        self.root.join(MANIFESTS_DIR)
    }

    fn staging_dir(&self) -> PathBuf {
        self.root.join(STAGING_DIR)
    }

    fn blob_path(&self, hash: &str) -> Result<PathBuf, CoreError> {
        validate_hash(hash)?;
        Ok(self.blobs_dir().join(&hash[..2]).join(hash))
    }

    fn manifest_path(&self, manifest_id: &str) -> Result<PathBuf, CoreError> {
        validate_hash(manifest_id)?;
        Ok(self
            .manifests_dir()
            .join(&manifest_id[..2])
            .join(format!("{manifest_id}.json")))
    }

    /// 入库文件本体，返回 CAS 对象。
    pub async fn put_file(&self, source: &Path) -> Result<CasObject, CoreError> {
        if !source.is_file() {
            return Err(cas_err(
                "put_file",
                format!("源文件不存在或不是普通文件：{}", source.display()),
            ));
        }
        let staging = self.new_staging_path();
        if let Some(parent) = staging.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|error| cas_io(parent, "create_staging_dir", error))?;
        }
        let mut source_file = fs::File::open(source)
            .await
            .map_err(|error| cas_io(source, "open_source", error))?;
        let mut staging_file = fs::File::create(&staging)
            .await
            .map_err(|error| cas_io(&staging, "create_staging", error))?;

        let mut hasher = Sha256::new();
        let mut size: u64 = 0;
        let mut buf = vec![0u8; HASH_CHUNK];
        loop {
            let read = source_file
                .read(&mut buf)
                .await
                .map_err(|error| cas_io(source, "read_source", error))?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
            staging_file
                .write_all(&buf[..read])
                .await
                .map_err(|error| cas_io(&staging, "write_staging", error))?;
            size += read as u64;
        }
        staging_file
            .flush()
            .await
            .map_err(|error| cas_io(&staging, "flush_staging", error))?;
        drop(staging_file);

        let hash = hex::encode(hasher.finalize());
        self.commit_staging(&staging, &hash, size).await?;
        Ok(CasObject { hash, size })
    }

    /// 入库一段字节（文件元数据 JSON 等）。
    pub async fn put_bytes(&self, bytes: &[u8]) -> Result<CasObject, CoreError> {
        let staging = self.new_staging_path();
        if let Some(parent) = staging.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|error| cas_io(parent, "create_staging_dir", error))?;
        }
        fs::write(&staging, bytes)
            .await
            .map_err(|error| cas_io(&staging, "write_staging_bytes", error))?;
        let hash = sha256_bytes(bytes);
        self.commit_staging(&staging, &hash, bytes.len() as u64)
            .await
    }

    /// 入库文件元数据，返回 `(hash, size)`；没有元数据时由调用方传 `None`。
    pub async fn put_file_meta(
        &self,
        meta: &file_meta::FileMeta,
    ) -> Result<CasObject, CoreError> {
        let bytes = canonical_file_meta(meta)?;
        self.put_bytes(&bytes).await
    }

    /// 写 manifest；返回 manifest id（等于 root hash）。
    pub async fn put_manifest(&self, manifest: &FileManifest) -> Result<String, CoreError> {
        manifest.validate()?;
        let target = self.manifest_path(&manifest.root_hash)?;
        if target.exists() {
            return Ok(manifest.root_hash.clone());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|error| cas_io(parent, "create_manifest_dir", error))?;
        }
        let staging = self.new_staging_path();
        fs::write(&staging, manifest.to_json()?.as_bytes())
            .await
            .map_err(|error| cas_io(&staging, "write_manifest_staging", error))?;
        fs::rename(&staging, &target)
            .await
            .map_err(|error| cas_io(&target, "commit_manifest", error))?;
        Ok(manifest.root_hash.clone())
    }

    /// 读 manifest 并校验；id 不存在返回错误。
    pub async fn read_manifest(&self, manifest_id: &str) -> Result<FileManifest, CoreError> {
        let path = self.manifest_path(manifest_id)?;
        let text = fs::read_to_string(&path)
            .await
            .map_err(|error| cas_io(&path, "read_manifest", error))?;
        let manifest: FileManifest = serde_json::from_str(&text)
            .map_err(|error| cas_err("parse_manifest", error.to_string()))?;
        manifest.validate()?;
        if manifest.root_hash != manifest_id {
            return Err(cas_err(
                "read_manifest",
                format!("manifest id 与 root hash 不一致：{manifest_id} / {}", manifest.root_hash),
            ));
        }
        Ok(manifest)
    }

    /// 把 manifest 的 Content 条目物化到 `dest`，同时返回 FileMeta（若清单里有）。
    ///
    /// 目标已存在即拒绝（不静默覆盖）；复制先落临时文件，hash 校验通过再 rename。
    pub async fn materialize_content(
        &self,
        manifest: &FileManifest,
        dest: &Path,
    ) -> Result<Option<file_meta::FileMeta>, CoreError> {
        manifest.validate()?;
        if dest.exists() {
            return Err(cas_err(
                "materialize",
                format!("目标已存在，拒绝覆盖：{}", dest.display()),
            ));
        }
        let content = manifest
            .content_entry()
            .ok_or_else(|| cas_err("materialize", "manifest 缺少 Content 条目"))?;
        let blob = self.blob_path(&content.hash)?;
        if !blob.is_file() {
            return Err(cas_err(
                "materialize",
                format!("CAS 缺少对象 {}（{}）", content.hash, blob.display()),
            ));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|error| cas_io(parent, "create_materialize_dir", error))?;
        }
        let temp = dest.with_extension(format!("cas-{}.tmp", uuid::Uuid::new_v4().simple()));
        let result = self.copy_verified(&blob, &content.hash, &temp).await;
        if let Err(error) = result {
            let _ = fs::remove_file(&temp).await;
            return Err(error);
        }
        fs::rename(&temp, dest)
            .await
            .map_err(|error| cas_io(dest, "commit_materialize", error))?;

        let meta = match manifest.meta_entry() {
            Some(entry) => {
                let meta_blob = self.blob_path(&entry.hash)?;
                let text = fs::read_to_string(&meta_blob)
                    .await
                    .map_err(|error| cas_io(&meta_blob, "read_meta_blob", error))?;
                let parsed: file_meta::FileMeta = serde_json::from_str(&text)
                    .map_err(|error| cas_err("parse_meta", error.to_string()))?;
                Some(parsed)
            }
            None => None,
        };
        Ok(meta)
    }

    /// 把 manifest 的 FileMeta 条目写到显式 sidecar 路径（宿主草稿箱用）。
    pub async fn write_meta_to(
        &self,
        manifest: &FileManifest,
        meta_path: &Path,
    ) -> Result<bool, CoreError> {
        let Some(entry) = manifest.meta_entry() else {
            return Ok(false);
        };
        let blob = self.blob_path(&entry.hash)?;
        let text = fs::read_to_string(&blob)
            .await
            .map_err(|error| cas_io(&blob, "read_meta_blob", error))?;
        if let Some(parent) = meta_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|error| cas_io(parent, "create_meta_dir", error))?;
        }
        fs::write(meta_path, text)
            .await
            .map_err(|error| cas_io(meta_path, "write_meta_sidecar", error))?;
        Ok(true)
    }

    /// 校验一个 blob 是否还在且 hash 正确。
    pub async fn verify_blob(&self, hash: &str) -> Result<bool, CoreError> {
        let path = self.blob_path(hash)?;
        if !path.is_file() {
            return Ok(false);
        }
        let actual = hash_file(&path).await?;
        Ok(actual == hash)
    }

    async fn copy_verified(
        &self,
        source: &Path,
        expected_hash: &str,
        dest: &Path,
    ) -> Result<(), CoreError> {
        let mut source_file = fs::File::open(source)
            .await
            .map_err(|error| cas_io(source, "open_blob", error))?;
        let mut dest_file = fs::File::create(dest)
            .await
            .map_err(|error| cas_io(dest, "create_temp", error))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; HASH_CHUNK];
        loop {
            let read = source_file
                .read(&mut buf)
                .await
                .map_err(|error| cas_io(source, "read_blob", error))?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
            dest_file
                .write_all(&buf[..read])
                .await
                .map_err(|error| cas_io(dest, "write_temp", error))?;
        }
        dest_file
            .flush()
            .await
            .map_err(|error| cas_io(dest, "flush_temp", error))?;
        let actual = hex::encode(hasher.finalize());
        if actual != expected_hash {
            return Err(cas_err(
                "verify",
                format!("hash 不一致：期望 {expected_hash}，实际 {actual}"),
            ));
        }
        Ok(())
    }

    async fn commit_staging(
        &self,
        staging: &Path,
        hash: &str,
        size: u64,
    ) -> Result<CasObject, CoreError> {
        let target = self.blob_path(hash)?;
        if target.exists() {
            let _ = fs::remove_file(staging).await;
            return Ok(CasObject {
                hash: hash.to_string(),
                size,
            });
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|error| cas_io(parent, "create_blob_dir", error))?;
        }
        fs::rename(staging, &target)
            .await
            .map_err(|error| cas_io(&target, "commit_blob", error))?;
        Ok(CasObject {
            hash: hash.to_string(),
            size,
        })
    }

    fn new_staging_path(&self) -> PathBuf {
        self.staging_dir()
            .join(format!("{}.tmp", uuid::Uuid::new_v4().simple()))
    }
}

impl PayloadStore {
    /// 项目级 CAS：`.RSmeta/cas/`（与 `resources/` 物化缓存分开）。
    pub fn cas(&self) -> CasStore {
        CasStore::new(self.project_root())
    }
}

/// 文件 sha256（流式）。
async fn hash_file(path: &Path) -> Result<String, CoreError> {
    let mut file = fs::File::open(path)
        .await
        .map_err(|error| cas_io(path, "open_hash", error))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_CHUNK];
    loop {
        let read = file
            .read(&mut buf)
            .await
            .map_err(|error| cas_io(path, "read_hash", error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn validate_hash(hash: &str) -> Result<(), CoreError> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(cas_err("validate_hash", format!("非法 sha256：{hash}")));
    }
    Ok(())
}

fn cas_err(operation: &str, reason: impl Into<String>) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "file_asset_cas".to_string(),
        operation: operation.to_string(),
        reason: reason.into(),
    })
}

fn cas_io(path: &Path, operation: &str, error: std::io::Error) -> CoreError {
    CoreError::storage(StorageError::io(
        path.display().to_string(),
        operation,
        error.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::manifest::ManifestEntryKind;
    use std::fs as std_fs;
    use std::path::Path as StdPath;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempRoot(std::path::PathBuf);

    impl TempRoot {
        fn path(&self) -> &StdPath {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std_fs::remove_dir_all(&self.0);
        }
    }

    fn temp_root() -> TempRoot {
        let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rds_cas_{}_{}_{id}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        ));
        std_fs::create_dir_all(&path).expect("tempdir");
        TempRoot(path)
    }

    #[tokio::test]
    async fn put_file_dedupes_same_bytes() {
        let dir = temp_root();
        let source = dir.path().join("a.csv");
        std_fs::write(&source, b"a,b\n1,2\n").unwrap();
        let cas = CasStore::new(dir.path());
        let first = cas.put_file(&source).await.unwrap();
        let second = cas.put_file(&source).await.unwrap();
        assert_eq!(first.hash, second.hash);
        let blob = cas.blob_path(&first.hash).unwrap();
        assert!(blob.is_file());
        let count = std_fs::read_dir(cas.blobs_dir().join(&first.hash[..2]))
            .unwrap()
            .count();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn content_change_changes_hash() {
        let dir = temp_root();
        let source = dir.path().join("a.csv");
        std_fs::write(&source, b"a,b\n1,2\n").unwrap();
        let cas = CasStore::new(dir.path());
        let first = cas.put_file(&source).await.unwrap();
        std_fs::write(&source, b"a,b\n1,3\n").unwrap();
        let second = cas.put_file(&source).await.unwrap();
        assert_ne!(first.hash, second.hash);
    }

    #[tokio::test]
    async fn manifest_roundtrip_materializes_and_returns_meta() {
        let dir = temp_root();
        let source = dir.path().join("a.csv");
        std_fs::write(&source, b"a,b\n1,2\n").unwrap();
        let cas = CasStore::new(dir.path());
        let content = cas.put_file(&source).await.unwrap();
        let mut meta = file_meta::FileMeta::default();
        meta.last_connection = Some("conn_9".into());
        let meta_obj = cas.put_file_meta(&meta).await.unwrap();
        let mut manifest =
            FileManifest::single_file("a.csv", content.hash.clone(), content.size, None);
        manifest.entries.push(super::super::manifest::ManifestEntry {
            path: ".a.csv.meta.json".into(),
            kind: ManifestEntryKind::FileMeta,
            hash: meta_obj.hash.clone(),
            size: meta_obj.size,
            executable: false,
        });
        manifest = FileManifest::from_entries(manifest.entries).unwrap();
        let id = cas.put_manifest(&manifest).await.unwrap();
        let loaded = cas.read_manifest(&id).await.unwrap();
        let dest = dir.path().join("out").join("a.csv");
        let restored = cas.materialize_content(&loaded, &dest).await.unwrap();
        assert!(dest.is_file());
        assert_eq!(std_fs::read(&dest).unwrap(), b"a,b\n1,2\n");
        assert_eq!(restored.unwrap().last_connection.as_deref(), Some("conn_9"));
    }

    #[tokio::test]
    async fn tampered_blob_is_rejected_without_half_output() {
        let dir = temp_root();
        let source = dir.path().join("a.csv");
        std_fs::write(&source, b"good").unwrap();
        let cas = CasStore::new(dir.path());
        let content = cas.put_file(&source).await.unwrap();
        let manifest =
            FileManifest::single_file("a.csv", content.hash.clone(), content.size, None);
        let blob = cas.blob_path(&content.hash).unwrap();
        std_fs::write(&blob, b"bad").unwrap();
        let dest = dir.path().join("out.csv");
        let error = cas.materialize_content(&manifest, &dest).await.unwrap_err();
        assert!(error.to_string().contains("hash 不一致"));
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn overwrite_is_refused() {
        let dir = temp_root();
        let source = dir.path().join("a.csv");
        std_fs::write(&source, b"good").unwrap();
        let cas = CasStore::new(dir.path());
        let content = cas.put_file(&source).await.unwrap();
        let manifest =
            FileManifest::single_file("a.csv", content.hash.clone(), content.size, None);
        let dest = dir.path().join("out.csv");
        std_fs::write(&dest, b"exists").unwrap();
        assert!(cas.materialize_content(&manifest, &dest).await.is_err());
    }
}
