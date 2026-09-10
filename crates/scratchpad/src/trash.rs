//! 项目级回收站（草稿 / 资源等模块共用）。
//!
//! 位置：`{project}/.RSmeta/trash/`。每条记录自包含一个目录：
//! - `<id>/payload`：被删的文件或目录（原样移动进来）；
//! - `<id>/manifest.json`：来源模块、原相对路径、类型、删除时间、大小。
//!
//! 用「自包含目录」而非「平铺文件 + 索引」，保证还原所需信息不依赖额外状态，
//! 且 id 唯一、重名互不覆盖。还原时按 `origin` + `original_rel_path` 放回原模块。
//!
//! 归属说明：当前唯一使用方是草稿箱（M5）。待 `analytics_resource`（M6）落地后，
//! 若确认为稳定共用能力，再按 `rds-architecture` 的「≥2 使用方」规则上提到
//! `shared`/`engine`，此处 API 已按模块无关设计。

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::fs;

use shared::error::{CoreError, StorageError};

use crate::models::{ScratchpadEntry, ScratchpadEntryKind};

const TRASH_DIR_NAME: &str = "trash";
const MANIFEST_FILE: &str = "manifest.json";
const PAYLOAD_NAME: &str = "payload";

/// 回收站条目清单（自包含还原所需信息）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashManifest {
    /// 条目 id（回收站目录名，唯一）。
    pub id: String,
    /// 原始名称。
    pub name: String,
    /// 来源模块标识（`scratchpad` / `resources` / `mock` …）。
    pub origin: String,
    /// 相对来源模块根的路径（还原目标）。
    pub original_rel_path: String,
    /// 文件或目录。
    pub kind: ScratchpadEntryKind,
    /// 删除时间。
    pub deleted_at: DateTime<Utc>,
    /// 字节数（目录为 0）。
    pub size: u64,
}

/// 回收站条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashEntry {
    pub manifest: TrashManifest,
}

/// 项目级回收站。
#[derive(Clone)]
pub struct ProjectTrash {
    dir: PathBuf,
}

impl ProjectTrash {
    /// 由项目根创建（落点 `{project}/.RSmeta/trash`）。
    pub fn new(project_root: &Path) -> Self {
        Self {
            dir: project_root
                .join(crate::store::META_DIR_NAME)
                .join(TRASH_DIR_NAME),
        }
    }

    /// 回收站目录。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub async fn ensure_dir(&self) -> Result<(), CoreError> {
        fs::create_dir_all(&self.dir)
            .await
            .map_err(|e| io_err(&self.dir, "create_trash_dir", e.to_string()))
    }

    /// 将文件/目录移入回收站，记录来源模块与原相对路径。
    pub async fn move_to_trash(
        &self,
        source: &Path,
        origin: &str,
        original_rel_path: &str,
    ) -> Result<TrashEntry, CoreError> {
        self.ensure_dir().await?;

        if !source.exists() {
            return Err(io_err(source, "move_to_trash", "source not found"));
        }
        let metadata = fs::metadata(source)
            .await
            .map_err(|e| io_err(source, "metadata", e.to_string()))?;
        let is_dir = metadata.is_dir();
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| io_err(source, "move_to_trash", "no file name"))?;

        let id = self.unique_id(&name).await;
        let entry_dir = self.dir.join(&id);
        fs::create_dir_all(&entry_dir)
            .await
            .map_err(|e| io_err(&entry_dir, "create_entry_dir", e.to_string()))?;

        let payload = entry_dir.join(PAYLOAD_NAME);
        // 同文件系统 rename 即时；跨设备时退回复制 + 删除。
        if fs::rename(source, &payload).await.is_err() {
            copy_recursive(source, &payload).await?;
            remove_recursive(source).await?;
        }

        let manifest = TrashManifest {
            id,
            name,
            origin: origin.to_string(),
            original_rel_path: original_rel_path.to_string(),
            kind: if is_dir {
                ScratchpadEntryKind::Folder
            } else {
                ScratchpadEntryKind::File
            },
            deleted_at: Utc::now(),
            size: if is_dir { 0 } else { metadata.len() },
        };
        let json = serde_json::to_string_pretty(&manifest).map_err(|e| {
            CoreError::storage(StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;
        fs::write(entry_dir.join(MANIFEST_FILE), json)
            .await
            .map_err(|e| io_err(&entry_dir, "write_manifest", e.to_string()))?;

        Ok(TrashEntry { manifest })
    }

    /// 列出回收站条目（按删除时间倒序）。
    pub async fn list(&self) -> Result<Vec<TrashEntry>, CoreError> {
        if !self.dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let mut read_dir = fs::read_dir(&self.dir)
            .await
            .map_err(|e| io_err(&self.dir, "read_trash", e.to_string()))?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let manifest_path = entry.path().join(MANIFEST_FILE);
            if !manifest_path.is_file() {
                continue;
            }
            let Ok(text) = fs::read_to_string(&manifest_path).await else {
                continue;
            };
            if let Ok(manifest) = serde_json::from_str::<TrashManifest>(&text) {
                out.push(TrashEntry { manifest });
            }
        }
        out.sort_by(|a, b| b.manifest.deleted_at.cmp(&a.manifest.deleted_at));
        Ok(out)
    }

    /// 读取单条条目。
    pub async fn get(&self, id: &str) -> Result<TrashEntry, CoreError> {
        let manifest_path = self.dir.join(id).join(MANIFEST_FILE);
        let text = fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| io_err(&manifest_path, "read_manifest", e.to_string()))?;
        let manifest = serde_json::from_str::<TrashManifest>(&text).map_err(|e| {
            CoreError::storage(StorageError::Deserialization {
                format: "JSON".to_string(),
                data: id.to_string(),
                reason: e.to_string(),
            })
        })?;
        Ok(TrashEntry { manifest })
    }

    /// 还原到 `dest_root`（按原相对路径重建父目录；同名自动改名，不覆盖）。
    pub async fn restore(&self, id: &str, dest_root: &Path) -> Result<ScratchpadEntry, CoreError> {
        let entry = self.get(id).await?;
        let payload = self.dir.join(id).join(PAYLOAD_NAME);
        if !payload.exists() {
            return Err(io_err(&payload, "restore", "payload not found in trash"));
        }

        let rel = entry
            .manifest
            .original_rel_path
            .trim_start_matches(['/', '\\']);
        let target = if rel.is_empty() {
            dest_root.join(&entry.manifest.name)
        } else {
            dest_root.join(rel)
        };
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| io_err(parent, "restore_create_parent", e.to_string()))?;
        }
        let target = crate::store::unique_path(target);

        if fs::rename(&payload, &target).await.is_err() {
            copy_recursive(&payload, &target).await?;
            remove_recursive(&payload).await?;
        }
        let _ = fs::remove_dir_all(self.dir.join(id)).await;

        let is_dir = entry.manifest.kind == ScratchpadEntryKind::Folder;
        Ok(ScratchpadEntry {
            name: target
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.manifest.name.clone()),
            path: target,
            kind: entry.manifest.kind.clone(),
            size: entry.manifest.size,
            modified_at: Some(Utc::now().to_rfc3339()),
            children: if is_dir { Some(Vec::new()) } else { None },
        })
    }

    /// 彻底删除单条（不可恢复）。
    pub async fn purge(&self, id: &str) -> Result<(), CoreError> {
        let entry_dir = self.dir.join(id);
        if entry_dir.exists() {
            fs::remove_dir_all(&entry_dir)
                .await
                .map_err(|e| io_err(&entry_dir, "purge", e.to_string()))?;
        }
        Ok(())
    }

    /// 清空回收站。
    pub async fn empty(&self) -> Result<(), CoreError> {
        if !self.dir.exists() {
            return Ok(());
        }
        fs::remove_dir_all(&self.dir)
            .await
            .map_err(|e| io_err(&self.dir, "empty_trash", e.to_string()))?;
        self.ensure_dir().await
    }

    /// 生成唯一 id（时间戳 + 名称，冲突追加序号）。
    async fn unique_id(&self, name: &str) -> String {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let base = format!("{}_{}", Utc::now().format("%Y%m%d_%H%M%S_%3f"), safe);
        let mut candidate = base.clone();
        let mut i = 1u32;
        while self.dir.join(&candidate).exists() {
            candidate = format!("{base}_{i}");
            i += 1;
        }
        candidate
    }
}

fn io_err(path: &Path, operation: &str, reason: impl Into<String>) -> CoreError {
    CoreError::storage(StorageError::io(
        path.display().to_string(),
        operation,
        reason.into(),
    ))
}

/// 递归复制（跨设备 rename 兜底）。
async fn copy_recursive(src: &Path, dst: &Path) -> Result<(), CoreError> {
    let metadata = fs::metadata(src)
        .await
        .map_err(|e| io_err(src, "copy_metadata", e.to_string()))?;
    if metadata.is_dir() {
        fs::create_dir_all(dst)
            .await
            .map_err(|e| io_err(dst, "copy_mkdir", e.to_string()))?;
        let mut read_dir = fs::read_dir(src)
            .await
            .map_err(|e| io_err(src, "copy_read_dir", e.to_string()))?;
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let child_dst = dst.join(entry.file_name());
            Box::pin(copy_recursive(&entry.path(), &child_dst)).await?;
        }
    } else {
        fs::copy(src, dst)
            .await
            .map_err(|e| io_err(src, "copy_file", e.to_string()))?;
    }
    Ok(())
}

/// 递归删除（跨设备 rename 兜底）。
async fn remove_recursive(path: &Path) -> Result<(), CoreError> {
    let metadata = fs::metadata(path)
        .await
        .map_err(|e| io_err(path, "remove_metadata", e.to_string()))?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .await
            .map_err(|e| io_err(path, "remove_dir_all", e.to_string()))
    } else {
        fs::remove_file(path)
            .await
            .map_err(|e| io_err(path, "remove_file", e.to_string()))
    }
}
