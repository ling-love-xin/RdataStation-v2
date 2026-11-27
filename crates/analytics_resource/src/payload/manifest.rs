//! 文件版本清单（manifest）：一次版本由哪些 CAS 对象组成。
//!
//! 清单是**不可变**的：内容或文件元数据任何一项变化，都会生成新的 manifest。
//! `content_hash` 只回答“文件字节有没有变”，`manifest_id` 回答“这一版完整快照有没有变”。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use shared::error::{CoreError, StorageError};

/// manifest 的结构版本；升级清单形状时递增。
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// 清单条目种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestEntryKind {
    /// 文件本体字节。
    Content,
    /// `rds-file-meta` sidecar 快照（书签 / 连接 / 视图）。
    FileMeta,
}

/// 一条 CAS 条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// 逻辑文件名（展示 / 物化时用；不是项目内相对路径）。
    pub path: String,
    /// 条目种类。
    pub kind: ManifestEntryKind,
    /// sha256(bytes)，小写十六进制。
    pub hash: String,
    /// 字节数。
    pub size: u64,
    /// 是否可执行（Windows 上只作元信息，不落 ACL）。
    pub executable: bool,
}

/// 一个文件版本的完整清单。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileManifest {
    /// 清单结构版本。
    pub schema_version: u32,
    /// 条目；文件资产通常一条 Content + 一条 FileMeta。
    pub entries: Vec<ManifestEntry>,
    /// `sha256(canonical(entries))`，同时也是 manifest 的持久 id。
    pub root_hash: String,
    /// Content 条目的 hash；没有 Content 时为空。
    pub content_hash: String,
    /// 文件本体字节数。
    pub file_size: u64,
    /// FileMeta 条目的 hash；没有元数据时为空。
    pub meta_hash: Option<String>,
}

impl FileManifest {
    /// 单文件清单：一个 Content + 可选 FileMeta。
    pub fn single_file(
        file_name: impl Into<String>,
        content_hash: impl Into<String>,
        file_size: u64,
        meta_hash: Option<String>,
    ) -> Self {
        let file_name = file_name.into();
        let content_hash = content_hash.into();
        let mut entries = vec![ManifestEntry {
            path: file_name.clone(),
            kind: ManifestEntryKind::Content,
            hash: content_hash.clone(),
            size: file_size,
            executable: false,
        }];
        if let Some(meta_hash) = meta_hash.clone() {
            entries.push(ManifestEntry {
                path: format!(".{file_name}.meta.json"),
                kind: ManifestEntryKind::FileMeta,
                hash: meta_hash,
                size: 0,
                executable: false,
            });
        }
        let root_hash = compute_root_hash(&entries);
        Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            entries,
            root_hash,
            content_hash,
            file_size,
            meta_hash,
        }
    }

    /// 从条目重建并校验 root hash（用于读取磁盘清单）。
    pub fn from_entries(entries: Vec<ManifestEntry>) -> Result<Self, CoreError> {
        let root_hash = compute_root_hash(&entries);
        let content = entries
            .iter()
            .find(|entry| entry.kind == ManifestEntryKind::Content);
        let meta = entries
            .iter()
            .find(|entry| entry.kind == ManifestEntryKind::FileMeta);
        let content_hash = content.map(|entry| entry.hash.clone()).unwrap_or_default();
        let file_size = content.map(|entry| entry.size).unwrap_or(0);
        let meta_hash = meta.map(|entry| entry.hash.clone());
        Ok(Self {
            schema_version: MANIFEST_SCHEMA_VERSION,
            entries,
            root_hash,
            content_hash,
            file_size,
            meta_hash,
        })
    }

    /// 读取后校验：清单自己声明的 root hash 必须与条目重算一致。
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.schema_version > MANIFEST_SCHEMA_VERSION {
            return Err(manifest_err(
                "validate",
                format!(
                    "manifest schema_version={} 高于当前支持 {}",
                    self.schema_version, MANIFEST_SCHEMA_VERSION
                ),
            ));
        }
        let recomputed = compute_root_hash(&self.entries);
        if recomputed != self.root_hash {
            return Err(manifest_err(
                "validate",
                format!("manifest root hash 不一致：声明 {}，实算 {recomputed}", self.root_hash),
            ));
        }
        let content = self
            .entries
            .iter()
            .filter(|entry| entry.kind == ManifestEntryKind::Content)
            .collect::<Vec<_>>();
        if content.len() != 1 {
            return Err(manifest_err(
                "validate",
                format!("文件 manifest 必须恰好一条 Content，实际 {}", content.len()),
            ));
        }
        if content[0].hash != self.content_hash || content[0].size != self.file_size {
            return Err(manifest_err(
                "validate",
                "manifest content_hash/file_size 与 Content 条目不匹配",
            ));
        }
        Ok(())
    }

    /// 文件本体条目。
    pub fn content_entry(&self) -> Option<&ManifestEntry> {
        self.entries
            .iter()
            .find(|entry| entry.kind == ManifestEntryKind::Content)
    }

    /// 文件元数据条目。
    pub fn meta_entry(&self) -> Option<&ManifestEntry> {
        self.entries
            .iter()
            .find(|entry| entry.kind == ManifestEntryKind::FileMeta)
    }

    /// 清单 JSON（落盘用；字段顺序固定由 serde 结构决定）。
    pub fn to_json(&self) -> Result<String, CoreError> {
        serde_json::to_string_pretty(self)
            .map_err(|error| manifest_err("serialize", error.to_string()))
    }
}

/// `sha256(canonical(entries))`。
///
/// 只对**条目内容**做 canonical，不把 `root_hash` 自己算进去（否则自引用）。
pub fn compute_root_hash(entries: &[ManifestEntry]) -> String {
    let canonical = serde_json::to_vec(entries).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(canonical);
    hex::encode(hasher.finalize())
}

/// 文件元数据 JSON 的 canonical bytes：对象键排序，数组保序。
///
/// 同一份 `FileMeta` 在不同写入顺序下得到同一字节串，元数据 hash 才能稳定。
pub fn canonical_file_meta(meta: &file_meta::FileMeta) -> Result<Vec<u8>, CoreError> {
    let value = serde_json::to_value(meta)
        .map_err(|error| manifest_err("meta_serialize", error.to_string()))?;
    let sorted = sort_json(value);
    serde_json::to_vec(&sorted).map_err(|error| manifest_err("meta_canonical", error.to_string()))
}

fn sort_json(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sort_json(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(sort_json).collect())
        }
        other => other,
    }
}

fn manifest_err(operation: &str, reason: impl Into<String>) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "file_asset_cas".to_string(),
        operation: operation.to_string(),
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(hash: &str, kind: ManifestEntryKind) -> ManifestEntry {
        ManifestEntry {
            path: "a.csv".to_string(),
            kind,
            hash: hash.to_string(),
            size: 3,
            executable: false,
        }
    }

    #[test]
    fn root_hash_changes_when_meta_changes_but_content_hash_does_not() {
        let left = FileManifest::single_file("a.csv", "content-a", 3, Some("meta-a".into()));
        let right = FileManifest::single_file("a.csv", "content-a", 3, Some("meta-b".into()));
        assert_eq!(left.content_hash, right.content_hash);
        assert_ne!(left.root_hash, right.root_hash);
    }

    #[test]
    fn manifest_roundtrip_and_validate() {
        let manifest = FileManifest::single_file("a.csv", "content-a", 3, None);
        let json = manifest.to_json().unwrap();
        let back: FileManifest = serde_json::from_str(&json).unwrap();
        back.validate().unwrap();
        assert_eq!(back.root_hash, manifest.root_hash);
        assert_eq!(back.content_hash, "content-a");
        assert!(back.meta_entry().is_none());
    }

    #[test]
    fn tampered_root_is_rejected() {
        let mut manifest = FileManifest::single_file("a.csv", "content-a", 3, None);
        manifest.root_hash = "bad".to_string();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn canonical_meta_is_key_order_independent() {
        let mut left = file_meta::FileMeta::default();
        left.extra.insert("b".into(), serde_json::json!(2));
        left.extra.insert("a".into(), serde_json::json!(1));
        let mut right = file_meta::FileMeta::default();
        right.extra.insert("a".into(), serde_json::json!(1));
        right.extra.insert("b".into(), serde_json::json!(2));
        assert_eq!(
            canonical_file_meta(&left).unwrap(),
            canonical_file_meta(&right).unwrap()
        );
    }

    #[test]
    fn two_content_entries_are_rejected() {
        let entries = vec![
            entry("a", ManifestEntryKind::Content),
            entry("b", ManifestEntryKind::Content),
        ];
        let mut manifest = FileManifest::from_entries(entries).unwrap();
        // 手动改坏，模拟外部清单
        manifest.content_hash = "a".to_string();
        assert!(manifest.validate().is_err());
    }
}
