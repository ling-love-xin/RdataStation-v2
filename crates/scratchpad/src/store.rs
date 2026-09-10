use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};

use crate::models::{
    AnalyzableFile, DiffLine, DiffLineKind, DiffResult, ExternalReference, ReplaceResult,
    ScratchpadConfig, ScratchpadEntry, ScratchpadEntryKind, ScratchpadResponse, SearchMatch,
    SearchResult,
};
use shared::error::{CoreError, StorageError};

const MAX_DEPTH: u32 = 4;
/// 草稿箱配置文件名（位于 `.RSmeta/scratchpad/` 内）。
const CONFIG_FILE: &str = "config.json";
const TRASH_DIR: &str = ".trash";
const MAX_SEARCH_RESULTS: usize = 500;
const SEARCH_PER_FILE_TIMEOUT_SECS: u64 = 30;
/// 项目元数据目录（与 `project` crate 的 `RS_META_DIR_NAME` 保持一致）。
const META_DIR_NAME: &str = ".RSmeta";
/// 草稿箱在项目元数据目录下的子目录名。
const META_SUBDIR: &str = "scratchpad";
/// v1 旧布局：隐藏草稿目录与其中的配置文件名。
const LEGACY_DIR: &str = ".scratchpad";
const LEGACY_CONFIG_FILE: &str = ".scratchpad.json";

/// 草稿箱存储。
///
/// v2 语义：**草稿箱根目录 = 项目根目录**（每个应用实例即一个项目），因此
/// `scratchpad_dir` 承载项目根，相对路径均以此为基准；草稿箱自身的内部元数据
/// （外部引用、file_meta、回收站）收纳到 `{project}/.RSmeta/scratchpad/`，
/// 不污染项目根，也不出现在面板树中（顶层点目录一律隐藏）。
#[derive(Clone)]
pub struct ScratchpadStore {
    /// 项目根目录（草稿箱根）。
    scratchpad_dir: PathBuf,
    /// 草稿箱内部元数据目录：`{project}/.RSmeta/scratchpad`。
    meta_dir: PathBuf,
    config_path: PathBuf,
    config_cache: std::sync::Arc<Mutex<Option<ScratchpadConfig>>>,
}

impl ScratchpadStore {
    pub fn new(project_path: PathBuf) -> Self {
        let meta_dir = project_path.join(META_DIR_NAME).join(META_SUBDIR);
        let config_path = meta_dir.join(CONFIG_FILE);
        Self {
            scratchpad_dir: project_path,
            meta_dir,
            config_path,
            config_cache: std::sync::Arc::new(Mutex::new(None)),
        }
    }

    /// 草稿箱根目录（= 项目根）。
    pub fn scratchpad_dir(&self) -> &Path {
        &self.scratchpad_dir
    }

    /// 草稿箱内部元数据目录：`{project}/.RSmeta/scratchpad`。
    pub fn meta_dir(&self) -> &Path {
        &self.meta_dir
    }

    /// 回收站目录：`{project}/.RSmeta/scratchpad/.trash`。
    fn trash_dir(&self) -> PathBuf {
        self.meta_dir.join(TRASH_DIR)
    }

    pub async fn ensure_dir(&self) -> Result<(), CoreError> {
        // 先做一次旧布局迁移（仅当旧目录存在且尚未迁移时执行，幂等且非破坏）。
        self.migrate_legacy_layout().await;

        fs::create_dir_all(&self.meta_dir).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                self.meta_dir.display().to_string(),
                "create_meta_dir",
                e.to_string(),
            ))
        })?;

        let trash_dir = self.trash_dir();
        fs::create_dir_all(&trash_dir).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                trash_dir.display().to_string(),
                "create_trash_dir",
                e.to_string(),
            ))
        })?;
        Ok(())
    }

    /// 旧布局迁移：v1 草稿箱位于 `{project}/.scratchpad/`，v2 改为「根 = 项目目录」。
    ///
    /// 仅在 `旧目录存在` 且 `新配置尚未生成` 时执行一次：
    /// 1. 旧配置 `.scratchpad.json` → `.RSmeta/scratchpad/config.json`；
    /// 2. 旧目录下的用户文件 → 项目根（**同名冲突保留原文件，不覆盖**）；
    /// 3. 旧回收站 `.trash/` → `.RSmeta/scratchpad/.trash/`；
    /// 4. 旧目录若已空则删除（非空保留，因点前缀仍不可见）。
    ///
    /// 迁移是尽力而为：任何单步失败只记录日志，不阻断存储使用。
    async fn migrate_legacy_layout(&self) {
        let legacy_dir = self.scratchpad_dir.join(LEGACY_DIR);
        if !legacy_dir.is_dir() {
            return;
        }
        // 新配置已存在说明此前已迁移，避免重复搬运导致 `_1` 副本。
        if self.config_path.exists() {
            return;
        }

        if let Err(e) = fs::create_dir_all(&self.meta_dir).await {
            tracing::warn!("[Scratchpad] 迁移：创建元数据目录失败: {e}");
            return;
        }

        // 1. 旧配置迁移（rename 失败时退回复制，兼容跨设备场景）。
        let legacy_config = legacy_dir.join(LEGACY_CONFIG_FILE);
        if legacy_config.is_file() {
            if fs::rename(&legacy_config, &self.config_path).await.is_err() {
                match fs::read(&legacy_config).await {
                    Ok(content) => {
                        let _ = fs::write(&self.config_path, content).await;
                    }
                    Err(e) => tracing::warn!("[Scratchpad] 迁移：读取旧配置失败: {e}"),
                }
            }
        }

        // 2. 用户文件迁移到项目根（跳过隐藏项；同名保留）。
        if let Ok(mut read_dir) = fs::read_dir(&legacy_dir).await {
            while let Ok(Some(entry)) = read_dir.next_entry().await {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with('.') {
                    continue;
                }
                let dest = self.scratchpad_dir.join(&name);
                if dest.exists() {
                    continue;
                }
                if let Err(e) = fs::rename(entry.path(), &dest).await {
                    tracing::warn!("[Scratchpad] 迁移：移动文件失败 {:?}: {e}", entry.path());
                }
            }
        }

        // 3. 旧回收站迁移（重名走 unique_path）。
        let legacy_trash = legacy_dir.join(TRASH_DIR);
        if legacy_trash.is_dir() {
            let _ = fs::create_dir_all(&self.trash_dir()).await;
            if let Ok(mut read_dir) = fs::read_dir(&legacy_trash).await {
                while let Ok(Some(entry)) = read_dir.next_entry().await {
                    let dest = unique_path(self.trash_dir().join(entry.file_name()));
                    let _ = fs::rename(entry.path(), &dest).await;
                }
            }
        }

        // 4. 旧目录空了再删（非空则保留，点前缀保证不可见）。
        let _ = fs::remove_dir(&legacy_dir).await;
    }

    pub async fn load_config(&self) -> Result<ScratchpadConfig, CoreError> {
        {
            let cache = self.config_cache.lock().await;
            if let Some(ref config) = *cache {
                return Ok(config.clone());
            }
        }

        let config = if !self.config_path.exists() {
            ScratchpadConfig::default()
        } else {
            let content = fs::read_to_string(&self.config_path).await.map_err(|e| {
                CoreError::storage(StorageError::io(
                    self.config_path.display().to_string(),
                    "read",
                    e.to_string(),
                ))
            })?;
            serde_json::from_str(&content).map_err(|e| {
                CoreError::storage(StorageError::Deserialization {
                    format: "JSON".to_string(),
                    data: content[..content.len().min(200)].to_string(),
                    reason: e.to_string(),
                })
            })?
        };

        {
            let mut cache = self.config_cache.lock().await;
            *cache = Some(config.clone());
        }
        Ok(config)
    }

    pub async fn save_config(&self, config: &ScratchpadConfig) -> Result<(), CoreError> {
        self.ensure_dir().await?;
        let json = serde_json::to_string_pretty(config).map_err(|e| {
            CoreError::storage(StorageError::Serialization {
                format: "JSON".to_string(),
                reason: e.to_string(),
            })
        })?;
        fs::write(&self.config_path, &json).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                self.config_path.display().to_string(),
                "write",
                e.to_string(),
            ))
        })?;

        {
            let mut cache = self.config_cache.lock().await;
            *cache = Some(config.clone());
        }
        Ok(())
    }

    pub async fn list_local_entries(&self, depth: u32) -> Result<Vec<ScratchpadEntry>, CoreError> {
        self.ensure_dir().await?;
        self.scan_dir_tree(&self.scratchpad_dir, 0, depth).await
    }

    async fn scan_dir_tree(
        &self,
        dir: &Path,
        current_depth: u32,
        max_depth: u32,
    ) -> Result<Vec<ScratchpadEntry>, CoreError> {
        if current_depth > max_depth {
            return Ok(Vec::new());
        }

        let mut read_dir = fs::read_dir(dir).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                dir.display().to_string(),
                "read_dir",
                e.to_string(),
            ))
        })?;

        let mut entries = Vec::new();

        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();

            // 隐藏项（`.RSmeta`、`.git`、点文件）一律不进入树；内部元数据因此天然不可见。
            if name.starts_with('.') {
                continue;
            }

            let file_type = entry.file_type().await.map_err(|e| {
                CoreError::storage(StorageError::io(
                    entry.path().display().to_string(),
                    "file_type",
                    e.to_string(),
                ))
            })?;

            let metadata = entry.metadata().await.map_err(|e| {
                CoreError::storage(StorageError::io(
                    entry.path().display().to_string(),
                    "metadata",
                    e.to_string(),
                ))
            })?;

            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| {
                        chrono::DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
                            .map(|dt| dt.to_rfc3339())
                    })
                })
                .flatten();

            if file_type.is_dir() {
                let children = if current_depth < max_depth {
                    Some(
                        Box::pin(self.scan_dir_tree(&entry.path(), current_depth + 1, max_depth))
                            .await?,
                    )
                } else {
                    None
                };

                entries.push(ScratchpadEntry {
                    name: name.clone(),
                    path: entry.path(),
                    kind: ScratchpadEntryKind::Folder,
                    size: 0,
                    modified_at,
                    children,
                });
            } else if file_type.is_file() {
                entries.push(ScratchpadEntry {
                    name,
                    path: entry.path(),
                    kind: ScratchpadEntryKind::File,
                    size: metadata.len(),
                    modified_at,
                    children: None,
                });
            }
        }

        Ok(entries)
    }

    fn flatten_entries_from_ref(entries: &[ScratchpadEntry]) -> Vec<&ScratchpadEntry> {
        let mut result = Vec::new();
        for entry in entries {
            result.push(entry);
            if let Some(ref children) = entry.children {
                result.extend(Self::flatten_entries_from_ref(children));
            }
        }
        result
    }

    pub async fn get_full_response(&self) -> Result<ScratchpadResponse, CoreError> {
        let config = self.load_config().await?;
        let local_entries = self.list_local_entries(0).await?;

        Ok(ScratchpadResponse {
            local_entries,
            external_references: config.external_references.clone(),
            scratchpad_path: self.scratchpad_dir.clone(),
            file_meta: config.file_meta.clone(),
        })
    }

    pub async fn list_directory_entries(
        &self,
        parent_path: &str,
    ) -> Result<Vec<ScratchpadEntry>, CoreError> {
        self.ensure_dir().await?;
        let parent = self.resolve_path(parent_path)?;
        if !parent.is_dir() {
            return Err(CoreError::storage(StorageError::io(
                parent.display().to_string(),
                "list_directory",
                "path is not a directory".to_string(),
            )));
        }
        self.scan_dir_tree(&parent, 0, 0).await
    }

    pub async fn create_entry(
        &self,
        name: &str,
        parent_path: Option<&str>,
        is_folder: bool,
    ) -> Result<ScratchpadEntry, CoreError> {
        self.ensure_dir().await?;
        self.validate_name(name)?;

        let base_dir = match parent_path {
            Some(p) if !p.is_empty() => {
                let parent = self.resolve_path(p)?;
                if !parent.is_dir() {
                    return Err(CoreError::storage(StorageError::io(
                        parent.display().to_string(),
                        "create",
                        "parent path is not a directory".to_string(),
                    )));
                }
                parent
            }
            _ => self.scratchpad_dir.clone(),
        };

        let target_path = base_dir.join(name);

        if target_path.exists() {
            return Err(CoreError::storage(StorageError::io(
                target_path.display().to_string(),
                "create",
                "entry already exists".to_string(),
            )));
        }

        if is_folder {
            fs::create_dir(&target_path).await.map_err(|e| {
                CoreError::storage(StorageError::io(
                    target_path.display().to_string(),
                    "create_dir",
                    e.to_string(),
                ))
            })?;
        } else {
            fs::write(&target_path, "").await.map_err(|e| {
                CoreError::storage(StorageError::io(
                    target_path.display().to_string(),
                    "create_file",
                    e.to_string(),
                ))
            })?;
        }

        Ok(ScratchpadEntry {
            name: name.to_string(),
            path: target_path,
            kind: if is_folder {
                ScratchpadEntryKind::Folder
            } else {
                ScratchpadEntryKind::File
            },
            size: 0,
            modified_at: Some(Utc::now().to_rfc3339()),
            children: if is_folder { Some(Vec::new()) } else { None },
        })
    }

    pub async fn delete_entry(&self, relative_path: &str) -> Result<(), CoreError> {
        let target_path = self.resolve_path(relative_path)?;

        let trash_dir = self.trash_dir();
        if !trash_dir.exists() {
            fs::create_dir_all(&trash_dir).await.map_err(|e| {
                CoreError::storage(StorageError::io(
                    trash_dir.display().to_string(),
                    "create_trash_dir",
                    e.to_string(),
                ))
            })?;
        }

        let file_name = target_path
            .file_name()
            .ok_or_else(|| {
                CoreError::storage(StorageError::io(
                    target_path.display().to_string(),
                    "delete_entry",
                    "invalid file path: no file name",
                ))
            })?
            .to_string_lossy()
            .to_string();
        let trash_target = unique_path(trash_dir.join(&file_name));

        fs::rename(&target_path, &trash_target).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                target_path.display().to_string(),
                "move_to_trash",
                e.to_string(),
            ))
        })?;

        Ok(())
    }

    pub async fn list_trash(&self) -> Result<Vec<ScratchpadEntry>, CoreError> {
        let trash_dir = self.trash_dir();
        if !trash_dir.exists() {
            return Ok(Vec::new());
        }

        self.scan_dir_tree(&trash_dir, 0, 1).await
    }

    pub async fn restore_from_trash(&self, trash_name: &str) -> Result<ScratchpadEntry, CoreError> {
        let trash_path = self.trash_dir().join(trash_name);
        if !trash_path.exists() {
            return Err(CoreError::storage(StorageError::io(
                trash_path.display().to_string(),
                "restore",
                "entry not found in trash".to_string(),
            )));
        }

        let target_path = unique_path(self.scratchpad_dir.join(trash_name));
        fs::rename(&trash_path, &target_path).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                trash_path.display().to_string(),
                "restore_from_trash",
                e.to_string(),
            ))
        })?;

        let is_dir = fs::metadata(&target_path)
            .await
            .map_err(|e| {
                CoreError::storage(StorageError::io(
                    target_path.display().to_string(),
                    "metadata",
                    e.to_string(),
                ))
            })?
            .is_dir();

        Ok(ScratchpadEntry {
            name: target_path
                .file_name()
                .ok_or_else(|| {
                    CoreError::storage(StorageError::io(
                        target_path.display().to_string(),
                        "restore_from_trash",
                        "invalid path: no file name",
                    ))
                })?
                .to_string_lossy()
                .to_string(),
            path: target_path,
            kind: if is_dir {
                ScratchpadEntryKind::Folder
            } else {
                ScratchpadEntryKind::File
            },
            size: 0,
            modified_at: Some(Utc::now().to_rfc3339()),
            children: if is_dir { Some(Vec::new()) } else { None },
        })
    }

    pub async fn empty_trash(&self) -> Result<(), CoreError> {
        let trash_dir = self.trash_dir();
        if trash_dir.exists() {
            fs::remove_dir_all(&trash_dir).await.map_err(|e| {
                CoreError::storage(StorageError::io(
                    trash_dir.display().to_string(),
                    "empty_trash",
                    e.to_string(),
                ))
            })?;
        }
        Ok(())
    }

    pub async fn rename_entry(
        &self,
        relative_path: &str,
        new_name: &str,
    ) -> Result<ScratchpadEntry, CoreError> {
        let old_path = self.resolve_path(relative_path)?;
        self.validate_name(new_name)?;

        if !old_path.exists() {
            return Err(CoreError::storage(StorageError::io(
                old_path.display().to_string(),
                "rename",
                "source not found".to_string(),
            )));
        }

        let parent = old_path.parent().ok_or_else(|| {
            CoreError::storage(StorageError::io(
                old_path.display().to_string(),
                "rename",
                "invalid path: no parent directory",
            ))
        })?;
        let new_path = parent.join(new_name);

        if new_path.exists() {
            return Err(CoreError::storage(StorageError::io(
                new_path.display().to_string(),
                "rename",
                "target already exists".to_string(),
            )));
        }

        let is_dir = fs::metadata(&old_path)
            .await
            .map_err(|e| {
                CoreError::storage(StorageError::io(
                    old_path.display().to_string(),
                    "metadata",
                    e.to_string(),
                ))
            })?
            .is_dir();

        fs::rename(&old_path, &new_path).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                old_path.display().to_string(),
                "rename",
                e.to_string(),
            ))
        })?;

        if let Ok(mut config) = self.load_config().await {
            if let Some(meta) = config.file_meta.remove(relative_path) {
                if let Ok(new_relative) = new_path.strip_prefix(&self.scratchpad_dir) {
                    config
                        .file_meta
                        .insert(new_relative.to_string_lossy().to_string(), meta);
                    let _ = self.save_config(&config).await;
                }
            }
        }

        Ok(ScratchpadEntry {
            name: new_name.to_string(),
            path: new_path,
            kind: if is_dir {
                ScratchpadEntryKind::Folder
            } else {
                ScratchpadEntryKind::File
            },
            size: 0,
            modified_at: Some(Utc::now().to_rfc3339()),
            children: if is_dir { Some(Vec::new()) } else { None },
        })
    }

    pub async fn read_file(&self, relative_path: &str) -> Result<String, CoreError> {
        let file_path = self.resolve_path(relative_path)?;

        fs::read_to_string(&file_path).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                file_path.display().to_string(),
                "read",
                e.to_string(),
            ))
        })
    }

    pub async fn check_file_size(&self, relative_path: &str) -> Result<u64, CoreError> {
        let file_path = self.resolve_path(relative_path)?;
        let metadata = fs::metadata(&file_path).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                file_path.display().to_string(),
                "metadata",
                e.to_string(),
            ))
        })?;
        Ok(metadata.len())
    }

    pub async fn get_entry_metadata(
        &self,
        relative_path: &str,
    ) -> Result<Option<ScratchpadEntry>, CoreError> {
        let file_path = self.resolve_path_maybe_missing(relative_path)?;
        if !file_path.exists() {
            return Ok(None);
        }

        let metadata = fs::metadata(&file_path).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                file_path.display().to_string(),
                "metadata",
                e.to_string(),
            ))
        })?;

        let name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| {
                CoreError::storage(StorageError::io(
                    file_path.display().to_string(),
                    "metadata",
                    "invalid file path: no file name component",
                ))
            })?;

        let modified_at = metadata
            .modified()
            .ok()
            .and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| {
                    chrono::DateTime::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
                        .map(|dt| dt.to_rfc3339())
                })
            })
            .flatten();

        let kind = if metadata.is_dir() {
            ScratchpadEntryKind::Folder
        } else {
            ScratchpadEntryKind::File
        };

        let size = if metadata.is_file() {
            metadata.len()
        } else {
            0
        };

        Ok(Some(ScratchpadEntry {
            name,
            path: file_path,
            kind: kind.clone(),
            size,
            modified_at,
            children: if matches!(kind, ScratchpadEntryKind::Folder) {
                Some(Vec::new())
            } else {
                None
            },
        }))
    }

    pub async fn open_in_system_explorer(&self, path_to_open: &Path) -> Result<(), CoreError> {
        opener::open(path_to_open).map_err(|e| {
            CoreError::storage(StorageError::io(
                path_to_open.display().to_string(),
                "open_explorer",
                e.to_string(),
            ))
        })
    }

    pub async fn save_file(&self, relative_path: &str, content: &str) -> Result<(), CoreError> {
        let file_path = self.resolve_path(relative_path)?;

        let tmp_ext = file_path
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_else(|| String::from("tmp"));
        let tmp_path = file_path.with_extension(format!("{}.tmp", tmp_ext));

        fs::write(&tmp_path, content).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                tmp_path.display().to_string(),
                "write_tmp",
                e.to_string(),
            ))
        })?;

        fs::rename(&tmp_path, &file_path).await.map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            CoreError::storage(StorageError::io(
                file_path.display().to_string(),
                "rename_tmp",
                e.to_string(),
            ))
        })
    }

    pub async fn import_external_file(&self, source: &Path) -> Result<ScratchpadEntry, CoreError> {
        self.ensure_dir().await?;

        if !source.exists() {
            return Err(CoreError::storage(StorageError::io(
                source.display().to_string(),
                "import",
                "source not found".to_string(),
            )));
        }

        let file_name = source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| {
                CoreError::storage(StorageError::io(
                    source.display().to_string(),
                    "import",
                    "invalid source path: no file name",
                ))
            })?;

        let dest = self.scratchpad_dir.join(&file_name);
        let dest = unique_path(dest);

        fs::copy(source, &dest).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                source.display().to_string(),
                "copy",
                e.to_string(),
            ))
        })?;

        let size = fs::metadata(&dest)
            .await
            .map_err(|e| {
                CoreError::storage(StorageError::io(
                    dest.display().to_string(),
                    "metadata",
                    e.to_string(),
                ))
            })?
            .len();

        Ok(ScratchpadEntry {
            name: dest
                .file_name()
                .ok_or_else(|| {
                    CoreError::storage(StorageError::io(
                        dest.display().to_string(),
                        "import",
                        "invalid dest path: no file name",
                    ))
                })?
                .to_string_lossy()
                .to_string(),
            path: dest,
            kind: ScratchpadEntryKind::File,
            size,
            modified_at: Some(Utc::now().to_rfc3339()),
            children: None,
        })
    }

    pub async fn add_external_reference(
        &self,
        alias: String,
        path: PathBuf,
    ) -> Result<ExternalReference, CoreError> {
        let mut config = self.load_config().await?;

        if config.external_references.iter().any(|r| r.alias == alias) {
            return Err(CoreError::storage(StorageError::persistence(
                "scratchpad_config",
                "add_reference",
                format!("alias '{}' already exists", alias),
            )));
        }

        let reference = ExternalReference {
            alias,
            path,
            created_at: Utc::now(),
        };

        config.external_references.push(reference.clone());
        self.save_config(&config).await?;

        Ok(reference)
    }

    pub async fn remove_external_reference(&self, alias: &str) -> Result<(), CoreError> {
        let mut config = self.load_config().await?;

        let len_before = config.external_references.len();
        config.external_references.retain(|r| r.alias != alias);

        if config.external_references.len() == len_before {
            return Err(CoreError::storage(StorageError::persistence(
                "scratchpad_config",
                "remove_reference",
                format!("alias '{}' not found", alias),
            )));
        }

        self.save_config(&config).await?;
        Ok(())
    }

    pub async fn update_file_meta(
        &self,
        relative_path: &str,
        connection_id: Option<String>,
    ) -> Result<(), CoreError> {
        let mut config = self.load_config().await?;

        let entry = config
            .file_meta
            .entry(relative_path.to_string())
            .or_default();

        if let Some(cid) = connection_id {
            entry.last_connection_id = Some(cid);
        }
        entry.last_executed_at = Some(Utc::now());

        self.save_config(&config).await
    }

    pub async fn search_file_content(
        &self,
        query: &str,
        case_sensitive: bool,
        context_lines: usize,
    ) -> Result<SearchResult, CoreError> {
        let entries = self.list_local_entries(MAX_DEPTH).await?;
        let flat = Self::flatten_entries_from_ref(&entries);
        let mut matches = Vec::new();
        let mut total_scanned = 0usize;
        let mut truncated = false;
        let query_lower = query.to_lowercase();
        let query_owned = query.to_string();

        for entry in &flat {
            if entry.kind == ScratchpadEntryKind::Folder {
                continue;
            }
            if truncated {
                break;
            }

            let rel_path = match entry.path.strip_prefix(&self.scratchpad_dir) {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => entry.path.to_string_lossy().to_string(),
            };

            let remaining = MAX_SEARCH_RESULTS.saturating_sub(matches.len());
            if remaining == 0 {
                truncated = true;
                break;
            }

            let future = search_single_file(
                entry.path.clone(),
                query_owned.clone(),
                case_sensitive,
                query_lower.clone(),
                rel_path.clone(),
                remaining,
                context_lines,
            );

            match timeout(Duration::from_secs(SEARCH_PER_FILE_TIMEOUT_SECS), future).await {
                Ok(Ok(mut file_matches)) => {
                    total_scanned += 1;
                    matches.append(&mut file_matches);
                }
                Ok(Err(_)) => {
                    total_scanned += 1;
                }
                Err(_) => {
                    total_scanned += 1;
                }
            }

            if matches.len() >= MAX_SEARCH_RESULTS {
                truncated = true;
            }
        }

        Ok(SearchResult {
            matches,
            total_files_scanned: total_scanned,
            total_files_skipped: 0,
            skipped_files: vec![],
            truncated,
        })
    }

    fn resolve_path(&self, relative_path: &str) -> Result<PathBuf, CoreError> {
        self.resolve_path_impl(relative_path, true)
    }

    fn resolve_path_maybe_missing(&self, relative_path: &str) -> Result<PathBuf, CoreError> {
        self.resolve_path_impl(relative_path, false)
    }

    fn resolve_path_impl(
        &self,
        relative_path: &str,
        must_exist: bool,
    ) -> Result<PathBuf, CoreError> {
        let clean = relative_path
            .trim_start_matches("\\\\?\\")
            .trim_start_matches('/')
            .trim_start_matches('\\');

        if clean.contains("..") {
            return Err(CoreError::storage(StorageError::io(
                relative_path.to_string(),
                "resolve",
                "path traversal detected".to_string(),
            )));
        }

        // 内部/隐藏目录（`.RSmeta` 等）不可经 API 读写，与面板隐藏规则保持一致。
        let first_component = clean
            .split(['/', '\\'])
            .find(|s| !s.is_empty())
            .unwrap_or("");
        if first_component.starts_with('.') {
            return Err(CoreError::storage(StorageError::io(
                relative_path.to_string(),
                "resolve",
                "hidden/internal path is not accessible".to_string(),
            )));
        }

        let target = self.scratchpad_dir.join(clean);

        if must_exist {
            if !target.exists() {
                return Err(CoreError::storage(StorageError::io(
                    target.display().to_string(),
                    "resolve",
                    "path not found".to_string(),
                )));
            }

            let canonical_base = self.scratchpad_dir.canonicalize().map_err(|e| {
                CoreError::storage(StorageError::io(
                    self.scratchpad_dir.display().to_string(),
                    "canonicalize_base",
                    e.to_string(),
                ))
            })?;

            let canonical_target = target.canonicalize().map_err(|e| {
                CoreError::storage(StorageError::io(
                    target.display().to_string(),
                    "canonicalize_target",
                    e.to_string(),
                ))
            })?;

            if !canonical_target.starts_with(&canonical_base) {
                return Err(CoreError::storage(StorageError::io(
                    relative_path.to_string(),
                    "resolve",
                    "path outside scratchpad directory".to_string(),
                )));
            }

            Ok(canonical_target)
        } else {
            let parent = match target.parent() {
                Some(p) => p,
                None => &self.scratchpad_dir,
            };
            if parent.starts_with(&self.scratchpad_dir) {
                Ok(target)
            } else {
                Err(CoreError::storage(StorageError::io(
                    relative_path.to_string(),
                    "resolve",
                    "path outside scratchpad directory".to_string(),
                )))
            }
        }
    }

    pub async fn get_analyzable_files(&self) -> Result<Vec<AnalyzableFile>, CoreError> {
        let response = self.get_full_response().await?;
        let flat = Self::flatten_entries_from_ref(&response.local_entries);
        let analyzable_extensions: std::collections::HashSet<&str> = [
            "csv", "tsv", "parquet", "json", "ndjson", "xlsx", "xls", "sqlite", "db", "duckdb",
        ]
        .iter()
        .cloned()
        .collect();

        let mut results = Vec::new();
        for entry in &flat {
            let ext = entry
                .path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or(String::new())
                .to_lowercase();

            if !analyzable_extensions.contains(ext.as_str()) {
                continue;
            }

            let hint = duckdb_query_hint(&ext, &entry.name);

            if let Ok(rel_path) = entry.path.strip_prefix(&self.scratchpad_dir) {
                results.push(AnalyzableFile {
                    name: entry.name.clone(),
                    relative_path: rel_path.to_string_lossy().to_string(),
                    file_type: ext,
                    size_bytes: entry.size,
                    duckdb_query_hint: hint,
                });
            }
        }

        Ok(results)
    }

    fn validate_name(&self, name: &str) -> Result<(), CoreError> {
        if name.is_empty() {
            return Err(CoreError::storage(StorageError::io(
                name.to_string(),
                "validate",
                "name cannot be empty".to_string(),
            )));
        }

        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(CoreError::storage(StorageError::io(
                name.to_string(),
                "validate",
                "name contains invalid characters".to_string(),
            )));
        }

        Ok(())
    }

    pub async fn move_entry(
        &self,
        from_relative_path: &str,
        to_parent_path: &str,
    ) -> Result<ScratchpadEntry, CoreError> {
        let source_path = self.resolve_path(from_relative_path)?;
        let dest_parent = if to_parent_path.is_empty() {
            self.scratchpad_dir.clone()
        } else {
            let parent = self.resolve_path(to_parent_path)?;
            if !parent.is_dir() {
                return Err(CoreError::storage(StorageError::io(
                    parent.display().to_string(),
                    "move",
                    "destination parent is not a directory".to_string(),
                )));
            }
            parent
        };

        if !source_path.exists() {
            return Err(CoreError::storage(StorageError::io(
                source_path.display().to_string(),
                "move",
                "source not found".to_string(),
            )));
        }

        let source_name = source_path
            .file_name()
            .ok_or_else(|| {
                CoreError::storage(StorageError::io(
                    source_path.display().to_string(),
                    "move",
                    "invalid source path: no file name",
                ))
            })?
            .to_string_lossy()
            .to_string();

        let dest_path = dest_parent.join(&source_name);

        if dest_path == source_path {
            return Err(CoreError::storage(StorageError::io(
                source_path.display().to_string(),
                "move",
                "source and destination are the same".to_string(),
            )));
        }

        if dest_path.exists() {
            return Err(CoreError::storage(StorageError::io(
                dest_path.display().to_string(),
                "move",
                "destination already exists".to_string(),
            )));
        }

        let is_dir = fs::metadata(&source_path)
            .await
            .map_err(|e| {
                CoreError::storage(StorageError::io(
                    source_path.display().to_string(),
                    "metadata",
                    e.to_string(),
                ))
            })?
            .is_dir();

        fs::rename(&source_path, &dest_path).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                source_path.display().to_string(),
                "move",
                e.to_string(),
            ))
        })?;

        if let Ok(mut config) = self.load_config().await {
            if let Some(meta) = config.file_meta.remove(from_relative_path) {
                if let Ok(new_relative) = dest_path.strip_prefix(&self.scratchpad_dir) {
                    config
                        .file_meta
                        .insert(new_relative.to_string_lossy().to_string(), meta);
                    let _ = self.save_config(&config).await;
                }
            }
        }

        Ok(ScratchpadEntry {
            name: source_name,
            path: dest_path,
            kind: if is_dir {
                ScratchpadEntryKind::Folder
            } else {
                ScratchpadEntryKind::File
            },
            size: 0,
            modified_at: Some(Utc::now().to_rfc3339()),
            children: if is_dir { Some(Vec::new()) } else { None },
        })
    }

    pub async fn replace_in_file(
        &self,
        relative_path: &str,
        pattern: &str,
        replacement: &str,
        is_regex: bool,
    ) -> Result<ReplaceResult, CoreError> {
        let file_path = self.resolve_path(relative_path)?;
        let original = fs::read_to_string(&file_path).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                file_path.display().to_string(),
                "read",
                e.to_string(),
            ))
        })?;

        let (replaced, new_content) = if is_regex {
            let re = regex::Regex::new(pattern).map_err(|e| {
                CoreError::storage(StorageError::io(
                    file_path.display().to_string(),
                    "regex_compile",
                    e.to_string(),
                ))
            })?;
            let count = re.find_iter(&original).count();
            let result = re.replace_all(&original, replacement).to_string();
            (count, result)
        } else {
            let count = original.matches(pattern).count();
            let result = original.replace(pattern, replacement);
            (count, result)
        };

        if replaced > 0 {
            self.save_file(relative_path, &new_content).await?;
        }

        Ok(ReplaceResult {
            replaced,
            file_path: relative_path.to_string(),
        })
    }

    pub async fn diff_with_content(
        &self,
        relative_path: &str,
        other_content: &str,
        left_label: &str,
        right_label: &str,
    ) -> Result<DiffResult, CoreError> {
        let file_path = self.resolve_path(relative_path)?;
        let file_content = fs::read_to_string(&file_path).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                file_path.display().to_string(),
                "read",
                e.to_string(),
            ))
        })?;

        let other_owned = other_content.to_string();
        let diff = similar::TextDiff::from_lines(&file_content, &other_owned);
        let mut lines = Vec::new();

        for change in diff.iter_all_changes() {
            let (kind, left_num, right_num) = match change.tag() {
                similar::ChangeTag::Equal => (
                    DiffLineKind::Unchanged,
                    change.old_index().map(|i| i + 1),
                    change.new_index().map(|i| i + 1),
                ),
                similar::ChangeTag::Delete => (
                    DiffLineKind::Removed,
                    change.old_index().map(|i| i + 1),
                    None,
                ),
                similar::ChangeTag::Insert => {
                    (DiffLineKind::Added, None, change.new_index().map(|i| i + 1))
                }
            };
            lines.push(DiffLine {
                line_number_left: left_num,
                line_number_right: right_num,
                kind,
                content: change.to_string_lossy().to_string(),
            });
        }

        Ok(DiffResult {
            lines,
            left_label: left_label.to_string(),
            right_label: right_label.to_string(),
        })
    }
}

async fn search_single_file(
    path: PathBuf,
    query: String,
    case_sensitive: bool,
    query_lower: String,
    rel_path: String,
    max_results: usize,
    context_lines: usize,
) -> Result<Vec<SearchMatch>, CoreError> {
    // Read the entire file into memory so that we can extract context lines
    // around matches. This trades off memory usage for complete context support.
    // For typical scratchpad file sizes, this is acceptable.
    let file = fs::File::open(&path).await.map_err(|e| {
        CoreError::storage(StorageError::io(
            path.display().to_string(),
            "search_open",
            e.to_string(),
        ))
    })?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut all_lines: Vec<String> = Vec::new();
    while let Ok(Some(line)) = lines.next_line().await {
        all_lines.push(line);
    }

    let mut matches = Vec::new();
    for (i, line) in all_lines.iter().enumerate() {
        if matches.len() >= max_results {
            break;
        }
        let line_number = i + 1;
        let found = if case_sensitive {
            line.contains(&query)
        } else {
            line.to_lowercase().contains(&query_lower)
        };
        if found {
            let before: Vec<String> = if context_lines > 0 {
                let start = i.saturating_sub(context_lines);
                all_lines[start..i].to_vec()
            } else {
                Vec::new()
            };
            let after: Vec<String> = if context_lines > 0 {
                let end = (i + 1 + context_lines).min(all_lines.len());
                all_lines[i + 1..end].to_vec()
            } else {
                Vec::new()
            };
            matches.push(SearchMatch {
                file: rel_path.clone(),
                line_number,
                line_content: line.clone(),
                before_context: before,
                after_context: after,
            });
        }
    }
    Ok(matches)
}

fn duckdb_query_hint(ext: &str, name: &str) -> String {
    let escaped = name.replace('\'', "''");
    match ext {
        "csv" => format!("SELECT * FROM read_csv_auto('{}');", escaped),
        "tsv" => format!("SELECT * FROM read_csv_auto('{}', delim='\\t');", escaped),
        "parquet" => format!("SELECT * FROM read_parquet('{}');", escaped),
        "json" | "ndjson" => format!("SELECT * FROM read_json_auto('{}');", escaped),
        "xlsx" | "xls" => format!("SELECT * FROM st_read('{}');", escaped),
        "sqlite" | "db" => format!("ATTACH '{}' AS sqlite_db (TYPE sqlite);", escaped),
        "duckdb" => format!("ATTACH '{}' AS duckdb_db;", escaped),
        _ => format!("-- Unsupported type: {}", ext),
    }
}

fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
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
            return new_path;
        }
    }

    let ts = Utc::now().timestamp_millis();
    let new_name = format!("{}_{}{}", stem, ts, ext);
    parent.join(&new_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// 创建独立临时项目目录（避免测试间干扰）。
    fn temp_project(tag: &str) -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rds_scratchpad_{}_{}_{}",
            tag,
            std::process::id(),
            n
        ));
        std::fs::create_dir_all(&dir).expect("create temp project");
        dir
    }

    #[tokio::test]
    async fn root_is_project_dir_and_meta_isolated() {
        let project = temp_project("root");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();

        store.create_entry("a.sql", None, false).await.unwrap();
        store.create_entry("sub", None, true).await.unwrap();

        // 根即项目目录：用户文件落在项目根，内部元数据落 `.RSmeta/scratchpad`。
        assert!(project.join("a.sql").is_file());
        assert!(project.join(META_DIR_NAME).join(META_SUBDIR).is_dir());

        let entries = store.list_local_entries(2).await.unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.clone()).collect();
        assert!(names.contains(&"a.sql".to_string()));
        assert!(names.contains(&"sub".to_string()));
        assert!(
            !names.iter().any(|n| n.starts_with('.')),
            "隐藏/内部目录不应出现在树中: {names:?}"
        );

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn internal_paths_are_blocked() {
        let project = temp_project("guard");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();

        assert!(store
            .read_file(".RSmeta/scratchpad/config.json")
            .await
            .is_err());
        assert!(store.read_file("../outside.txt").await.is_err());
        assert!(store
            .create_entry("x.sql", Some(".RSmeta"), false)
            .await
            .is_err());

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn trash_lives_under_meta() {
        let project = temp_project("trash");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store.create_entry("doomed.sql", None, false).await.unwrap();

        store.delete_entry("doomed.sql").await.unwrap();
        assert!(!project.join("doomed.sql").exists());
        assert!(project
            .join(META_DIR_NAME)
            .join(META_SUBDIR)
            .join(TRASH_DIR)
            .join("doomed.sql")
            .exists());

        let trash = store.list_trash().await.unwrap();
        assert!(trash.iter().any(|e| e.name == "doomed.sql"));

        store.restore_from_trash("doomed.sql").await.unwrap();
        assert!(project.join("doomed.sql").is_file());

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn external_reference_persists_in_meta_config() {
        let project = temp_project("ref");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store
            .add_external_reference("下载".into(), PathBuf::from("D:/data"))
            .await
            .unwrap();

        assert!(project
            .join(META_DIR_NAME)
            .join(META_SUBDIR)
            .join(CONFIG_FILE)
            .is_file());

        // 重新打开应从新位置读取到引用。
        let reloaded = ScratchpadStore::new(project.clone());
        let cfg = reloaded.load_config().await.unwrap();
        assert_eq!(cfg.external_references.len(), 1);

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn legacy_layout_is_migrated() {
        let project = temp_project("legacy");
        let legacy = project.join(LEGACY_DIR);
        std::fs::create_dir_all(legacy.join(TRASH_DIR)).unwrap();
        std::fs::write(
            legacy.join(LEGACY_CONFIG_FILE),
            r#"{"external_references":[],"file_meta":{}}"#,
        )
        .unwrap();
        std::fs::write(legacy.join("old.sql"), "select 1").unwrap();
        std::fs::write(legacy.join(TRASH_DIR).join("gone.sql"), "x").unwrap();

        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();

        let meta = project.join(META_DIR_NAME).join(META_SUBDIR);
        assert!(project.join("old.sql").is_file(), "用户文件应迁到项目根");
        assert!(meta.join(CONFIG_FILE).is_file(), "配置应迁到元数据目录");
        assert!(meta.join(TRASH_DIR).join("gone.sql").exists());

        std::fs::remove_dir_all(&project).ok();
    }
}
