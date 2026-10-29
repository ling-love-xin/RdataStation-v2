//! 项目级回收站（**各模块共用**：草稿 / 资源 / 未来的 Mock 产物…）。
//!
//! 位置：`{project}/.RSmeta/trash/`。每条记录自包含一个目录：
//! - `<id>/payload`：被删的文件或目录（原样移动进来）；
//! - `<id>/manifest.json`：来源模块、原相对路径、类型、删除时间、大小。
//!
//! 用「自包含目录」而非「平铺文件 + 索引」，保证还原所需信息不依赖额外状态，
//! 且 id 唯一、重名互不覆盖。还原时按 `origin` + `original_rel_path` 放回原模块。
//!
//! ## 中性（为什么它在 engine）
//!
//! 这里**不认识任何模块的类型**：来源是字符串标记（`origin`，如 `scratchpad` /
//! `resources`），条目类型是 [`TrashKind`]，还原结果用 [`TrashRestoreOutcome`] 返回
//! ——由调用方自己拼回本模块的条目模型。原实现长在 `scratchpad` 里、返回
//! `ScratchpadEntry`，那会让第二个使用方（M6）不得不依赖 M5。
//!
//! 「跨模块还原必须被拒」是**调用方**的责任（比对本方 origin 再还原），本层只如实
//! 记录来源——它没有立场决定谁可以还原什么。

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::fs;

use shared::error::{CoreError, StorageError};

use super::connection_org_store::RS_META_DIR_NAME;

/// 回收站目录名（位于 `.RSmeta/` 内）。
pub const TRASH_DIR_NAME: &str = "trash";
const MANIFEST_FILE: &str = "manifest.json";
const PAYLOAD_NAME: &str = "payload";

/// 条目类型（中性：文件 / 目录，不绑定任何模块的条目模型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrashKind {
    File,
    Folder,
}

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
    pub kind: TrashKind,
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

/// 还原结果：调用方据此拼回本模块的条目（名字可能因重名避让而与原名不同）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashRestoreOutcome {
    /// 还原落点（绝对路径）。
    pub path: PathBuf,
    /// 落点文件名（可能带避让后缀）。
    pub name: String,
    pub kind: TrashKind,
    pub size: u64,
    /// 是否因原名被占而改名（调用方应据此提示"已还原为 X_1"）。
    pub renamed: bool,
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
            dir: project_root.join(RS_META_DIR_NAME).join(TRASH_DIR_NAME),
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
                TrashKind::Folder
            } else {
                TrashKind::File
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
    ///
    /// **不做 origin 校验**：那是调用方的事（它才知道自己的根是哪个、允不允许跨模块还原）。
    pub async fn restore(
        &self,
        id: &str,
        dest_root: &Path,
    ) -> Result<TrashRestoreOutcome, CoreError> {
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
        let (target, renamed) = unique_path(target);

        if fs::rename(&payload, &target).await.is_err() {
            copy_recursive(&payload, &target).await?;
            remove_recursive(&payload).await?;
        }
        let _ = fs::remove_dir_all(self.dir.join(id)).await;

        Ok(TrashRestoreOutcome {
            name: target
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.manifest.name.clone()),
            path: target,
            kind: entry.manifest.kind,
            size: entry.manifest.size,
            renamed,
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

    /// 整仓清空（测试 / 工具用）。
    ///
    /// **模块界面不要用它**：回收站是项目级的，整仓清空会把别的模块的条目也删掉——
    /// 界面上那个「清空」要走 [`ProjectTrash::empty_origin`]。
    pub async fn empty(&self) -> Result<(), CoreError> {
        if !self.dir.exists() {
            return Ok(());
        }
        fs::remove_dir_all(&self.dir)
            .await
            .map_err(|e| io_err(&self.dir, "empty_trash", e.to_string()))?;
        self.ensure_dir().await
    }

    /// 只清空 `origin` 名下的条目（返回删掉的条数）。
    ///
    /// 这是模块界面上的「清空回收站」该走的口：仓库是共用的，删别人的东西不是清空，是越权。
    pub async fn empty_origin(&self, origin: &str) -> Result<usize, CoreError> {
        let mut purged = 0usize;
        for entry in self.list().await? {
            if entry.manifest.origin == origin {
                self.purge(&entry.manifest.id).await?;
                purged += 1;
            }
        }
        Ok(purged)
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

/// 同名避让：`x.sql` → `x_1.sql`（试到 999，再不行加时间戳）；返回是否改过名。
fn unique_path(path: PathBuf) -> (PathBuf, bool) {
    if !path.exists() {
        return (path, false);
    }

    let parent = match path.parent() {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from("."),
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| String::from("file"));
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();

    for i in 1..1000 {
        let new_name = format!("{}_{}{}", stem, i, ext);
        let new_path = parent.join(&new_name);
        if !new_path.exists() {
            return (new_path, true);
        }
    }

    let ts = Utc::now().timestamp_millis();
    let new_name = format!("{}_{}{}", stem, ts, ext);
    (parent.join(&new_name), true)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_trash_{tag}_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create temp project");
        dir
    }

    #[tokio::test]
    async fn move_list_get_restore_and_purge_roundtrip() {
        let project = temp_project("roundtrip");
        let trash = ProjectTrash::new(&project);
        let module_root = project.join("scratchpad");
        std::fs::create_dir_all(&module_root).expect("mkdir module root");

        let source = module_root.join("doomed.sql");
        std::fs::write(&source, b"select 1;").expect("write");
        let entry = trash
            .move_to_trash(&source, "scratchpad", "doomed.sql")
            .await
            .expect("move");

        assert!(!source.exists(), "移入回收站 = 源位置不再有它");
        assert_eq!(entry.manifest.origin, "scratchpad");
        assert_eq!(entry.manifest.kind, TrashKind::File);
        assert_eq!(entry.manifest.size, 9);
        assert!(trash.dir().join(&entry.manifest.id).join("payload").is_file());

        // 清单往返：list / get 都能读回同一条（manifest 自包含）。
        let listed = trash.list().await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].manifest.id, entry.manifest.id);
        let fetched = trash.get(&entry.manifest.id).await.expect("get");
        assert_eq!(fetched.manifest.original_rel_path, "doomed.sql");

        // 还原：按原相对路径放回，且报告没改名。
        let restored = trash
            .restore(&entry.manifest.id, &module_root)
            .await
            .expect("restore");
        assert_eq!(restored.path, source);
        assert!(!restored.renamed);
        assert_eq!(restored.kind, TrashKind::File);
        assert_eq!(std::fs::read(&source).expect("read"), b"select 1;");
        assert!(trash.list().await.expect("list").is_empty(), "还原后条目消失");

        // purge：真删目录，不留残骸。
        let again = module_root.join("again.sql");
        std::fs::write(&again, b"x").expect("write");
        let entry = trash
            .move_to_trash(&again, "scratchpad", "again.sql")
            .await
            .expect("move");
        trash.purge(&entry.manifest.id).await.expect("purge");
        assert!(!trash.dir().join(&entry.manifest.id).exists());

        let _ = std::fs::remove_dir_all(&project);
    }

    #[tokio::test]
    async fn restore_avoids_conflict_and_reports_it() {
        let project = temp_project("conflict");
        let trash = ProjectTrash::new(&project);
        let module_root = project.join("resources");
        std::fs::create_dir_all(&module_root).expect("mkdir");

        let source = module_root.join("taken.sql");
        std::fs::write(&source, b"old").expect("write");
        let entry = trash
            .move_to_trash(&source, "resources", "taken.sql")
            .await
            .expect("move");
        // 原位置被新文件占住：还原必须避让，不能覆盖。
        std::fs::write(&source, b"new").expect("write new");

        let restored = trash
            .restore(&entry.manifest.id, &module_root)
            .await
            .expect("restore");
        assert!(restored.renamed, "同名被占 → 必须报告改名");
        assert_eq!(restored.name, "taken_1.sql");
        assert_eq!(std::fs::read(&source).expect("read"), b"new", "原文件不被覆盖");
        assert_eq!(
            std::fs::read(module_root.join("taken_1.sql")).expect("read"),
            b"old"
        );

        let _ = std::fs::remove_dir_all(&project);
    }

    #[tokio::test]
    async fn folders_roundtrip_and_empty_clears_everything() {
        let project = temp_project("folders");
        let trash = ProjectTrash::new(&project);
        let module_root = project.join("scratchpad");
        let folder = module_root.join("reports");
        std::fs::create_dir_all(&folder).expect("mkdir folder");
        std::fs::write(folder.join("a.sql"), b"a").expect("write child");

        let entry = trash
            .move_to_trash(&folder, "scratchpad", "reports")
            .await
            .expect("move folder");
        assert_eq!(entry.manifest.kind, TrashKind::Folder);
        assert_eq!(entry.manifest.size, 0, "目录不计大小");

        let restored = trash
            .restore(&entry.manifest.id, &module_root)
            .await
            .expect("restore folder");
        assert_eq!(restored.kind, TrashKind::Folder);
        assert!(module_root.join("reports").join("a.sql").is_file(), "目录内容一起回来");

        // 清空：目录被清掉后立刻重建（下次能用）。
        let another = module_root.join("b.sql");
        std::fs::write(&another, b"b").expect("write");
        trash
            .move_to_trash(&another, "scratchpad", "b.sql")
            .await
            .expect("move");
        trash.empty().await.expect("empty");
        assert!(trash.list().await.expect("list").is_empty());
        assert!(trash.dir().is_dir(), "清空后目录要重建");

        let _ = std::fs::remove_dir_all(&project);
    }

    #[tokio::test]
    async fn empty_origin_leaves_other_modules_alone() {
        let project = temp_project("empty_origin");
        let trash = ProjectTrash::new(&project);
        let scratchpad = project.join("scratchpad");
        let resources = project.join("resources");
        std::fs::create_dir_all(&scratchpad).expect("mkdir scratchpad");
        std::fs::create_dir_all(&resources).expect("mkdir resources");
        std::fs::write(scratchpad.join("a.sql"), b"a").expect("write draft");
        std::fs::write(resources.join("b.parquet"), b"b").expect("write payload");

        trash
            .move_to_trash(&scratchpad.join("a.sql"), "scratchpad", "a.sql")
            .await
            .expect("move draft");
        trash
            .move_to_trash(&resources.join("b.parquet"), "resources", "b.parquet")
            .await
            .expect("move payload");

        assert_eq!(trash.empty_origin("resources").await.expect("empty origin"), 1);
        let left = trash.list().await.expect("list");
        assert_eq!(left.len(), 1, "别的模块的条目必须原样留着");
        assert_eq!(left[0].manifest.origin, "scratchpad");
        assert_eq!(
            trash.empty_origin("resources").await.expect("idempotent"),
            0,
            "没有自己名下的条目时是 0，不是错"
        );

        let _ = std::fs::remove_dir_all(&project);
    }
}
