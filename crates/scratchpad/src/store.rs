use std::path::{Path, PathBuf};

use chrono::Utc;
use tokio::fs;
use tokio::sync::Mutex;

use crate::models::{
    AnalyzableFile, DiffLine, DiffLineKind, DiffResult, ExternalReference, ExternalReferenceStatus,
    FileMeta, FlatFile, ReplaceResult, ScratchpadConfig, ScratchpadEntry, ScratchpadEntryKind,
    ScratchpadResponse,
};
use engine::persistence::trash::{ProjectTrash, TrashEntry, TrashKind};
use shared::error::{CoreError, StorageError};

const MAX_DEPTH: u32 = 4;
/// 草稿箱配置文件名（位于 `.RSmeta/scratchpad/` 内）。
const CONFIG_FILE: &str = "config.json";
/// 旧回收站目录名（v1 `.scratchpad/.trash`、上一版 `.RSmeta/scratchpad/.trash`）。
const TRASH_DIR: &str = ".trash";
/// 复制重名避让的最大尝试次数（`name_copy`、`name_copy_1` …）。
const MAX_COPY_ATTEMPTS: usize = 1000;
/// 项目元数据目录（与 `project` crate 的 `RS_META_DIR_NAME` 保持一致）。
pub(crate) const META_DIR_NAME: &str = ".RSmeta";
/// 草稿箱在项目元数据目录下的子目录名。
const META_SUBDIR: &str = "scratchpad";
/// 草稿箱内容目录名（模块根，位于项目根下、可见）。
pub const MODULE_DIR_NAME: &str = "scratchpad";
/// 回收站条目来源：草稿箱。
pub const ORIGIN_SCRATCHPAD: &str = "scratchpad";
/// v1 旧布局：隐藏草稿目录与其中的配置文件名。
const LEGACY_DIR: &str = ".scratchpad";
const LEGACY_CONFIG_FILE: &str = ".scratchpad.json";

/// 草稿箱存储。
///
/// v2 语义：**草稿箱根目录 = 模块目录 `{project}/scratchpad/`**（每个应用实例即一个项目），
/// 相对路径均以模块根为基准；草稿箱内部元数据（配置 / 文件元数据）落在
/// `{project}/.RSmeta/scratchpad/`，回收站为**项目级** `{project}/.RSmeta/trash/`
/// （与资源模块共用，条目自带来源信息）。
#[derive(Clone)]
pub struct ScratchpadStore {
    /// 项目根目录（定位 `.RSmeta/` 用）。
    project_root: PathBuf,
    /// 草稿箱模块根：`{project}/scratchpad/`（相对路径基准）。
    scratchpad_dir: PathBuf,
    /// 草稿箱内部元数据目录：`{project}/.RSmeta/scratchpad`。
    meta_dir: PathBuf,
    config_path: PathBuf,
    /// 项目级回收站：`{project}/.RSmeta/trash`。
    trash: ProjectTrash,
    config_cache: std::sync::Arc<Mutex<Option<ScratchpadConfig>>>,
}

impl ScratchpadStore {
    pub fn new(project_path: PathBuf) -> Self {
        let scratchpad_dir = project_path.join(MODULE_DIR_NAME);
        let meta_dir = project_path.join(META_DIR_NAME).join(META_SUBDIR);
        let config_path = meta_dir.join(CONFIG_FILE);
        let trash = ProjectTrash::new(&project_path);
        Self {
            project_root: project_path,
            scratchpad_dir,
            meta_dir,
            config_path,
            trash,
            config_cache: std::sync::Arc::new(Mutex::new(None)),
        }
    }

    /// 草稿箱模块根目录：`{project}/scratchpad/`。
    pub fn scratchpad_dir(&self) -> &Path {
        &self.scratchpad_dir
    }

    /// 草稿箱内部元数据目录：`{project}/.RSmeta/scratchpad`。
    pub fn meta_dir(&self) -> &Path {
        &self.meta_dir
    }

    /// 项目根目录。
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// 项目级回收站。
    pub fn trash(&self) -> &ProjectTrash {
        &self.trash
    }

    /// 绝对路径 → 模块内相对路径（不在模块目录下、或指向内部/隐藏路径时返回 `None`）。
    ///
    /// 与 `resolve_path` 反向：`resolve_path(relative_path_of(p)?)` 回到同一路径。
    /// 给「这个绝对路径是不是草稿」的判定用：打开草稿时预选连接、执行后回写元数据、
    /// 脏点回显都需要先把中央编辑区的路径对回模块内身份。
    pub fn relative_path_of(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.scratchpad_dir).ok()?;
        let parts: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let first = parts.first()?;
        // 内部/隐藏目录（`.RSmeta` 等）不经 API 读写，与 `resolve_path` 的拒绝规则一致。
        if first.starts_with('.') {
            return None;
        }
        Some(parts.join("/"))
    }

    /// 执行回执 → 要回写的草稿（只保留模块目录内的文件），`(相对路径, 连接 id)`。
    ///
    /// 宿主拿到的是编辑器的一串「文件 + 用过的连接」；这里回答「哪些属于草稿箱、该写回什么」：
    /// - 模块外的路径（`resources/`、`mock/`、临时文件）一律跳过；
    /// - 同一份草稿有多条回执时**后到者胜**（最后一次执行用的那个连接才是“上次用的”）。
    pub fn draft_targets(
        &self,
        receipts: impl IntoIterator<Item = (PathBuf, Option<String>)>,
    ) -> Vec<(String, Option<String>)> {
        let mut targets: Vec<(String, Option<String>)> = Vec::new();
        for (path, connection) in receipts {
            let Some(relative) = self.relative_path_of(&path) else {
                continue;
            };
            match targets.iter_mut().find(|(rel, _)| rel == &relative) {
                Some(slot) => slot.1 = connection,
                None => targets.push((relative, connection)),
            }
        }
        targets
    }

    pub async fn ensure_dir(&self) -> Result<(), CoreError> {
        // 先做一次旧布局迁移（仅当旧目录存在时执行，幂等且非破坏）。
        self.migrate_legacy_layout().await;

        fs::create_dir_all(&self.scratchpad_dir)
            .await
            .map_err(|e| io_err(&self.scratchpad_dir, "create_module_dir", e.to_string()))?;
        fs::create_dir_all(&self.meta_dir)
            .await
            .map_err(|e| io_err(&self.meta_dir, "create_meta_dir", e.to_string()))?;
        self.trash.ensure_dir().await?;
        Ok(())
    }

    /// 旧布局迁移（幂等、非破坏）：
    /// - v1 `{project}/.scratchpad/` 内容 → `{project}/scratchpad/`（同名保留）；
    ///   旧 `.scratchpad.json` → `config.json`；旧 `.trash/` → 项目级回收站；
    /// - 上一版实现遗留的 `{meta}/.trash` → 项目级回收站。
    ///
    /// 单步失败只记录日志，不阻断存储使用。
    async fn migrate_legacy_layout(&self) {
        let legacy_dir = self.project_root.join(LEGACY_DIR);
        if legacy_dir.is_dir() {
            if let Err(e) = fs::create_dir_all(&self.scratchpad_dir).await {
                tracing::warn!("[Scratchpad] 迁移：创建模块目录失败: {e}");
            }
            if let Err(e) = fs::create_dir_all(&self.meta_dir).await {
                tracing::warn!("[Scratchpad] 迁移：创建元数据目录失败: {e}");
            }
            let _ = self.trash.ensure_dir().await;

            // 1. 旧配置迁移（rename 失败时退回复制，兼容跨设备场景）。
            let legacy_config = legacy_dir.join(LEGACY_CONFIG_FILE);
            if legacy_config.is_file() && !self.config_path.exists() {
                if fs::rename(&legacy_config, &self.config_path).await.is_err() {
                    if let Ok(content) = fs::read(&legacy_config).await {
                        let _ = fs::write(&self.config_path, content).await;
                    }
                }
            }

            // 2. 用户文件搬进模块根（跳过隐藏项；同名保留）。
            self.move_dir_contents(&legacy_dir, &self.scratchpad_dir, true)
                .await;

            // 3. 旧回收站条目并入项目级回收站。
            self.ingest_trash_dir(&legacy_dir.join(TRASH_DIR)).await;

            // 4. 旧目录空了再删（非空则保留，点前缀保证不可见）。
            let _ = fs::remove_dir(&legacy_dir).await;
        }

        // 上一版实现遗留：`{meta}/.trash` → 项目级回收站。
        self.ingest_trash_dir(&self.meta_dir.join(TRASH_DIR)).await;
    }

    /// 把 `src` 下的条目移到 `dst`（可选跳过隐藏项；同名保留不覆盖）。
    async fn move_dir_contents(&self, src: &Path, dst: &Path, skip_hidden: bool) {
        let Ok(mut read_dir) = fs::read_dir(src).await else {
            return;
        };
        // 先收集再移动，避免边遍历边改动目录。
        let mut names: Vec<std::ffi::OsString> = Vec::new();
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            names.push(entry.file_name());
        }
        for name in names {
            if skip_hidden && name.to_string_lossy().starts_with('.') {
                continue;
            }
            let dest = dst.join(&name);
            if dest.exists() {
                continue;
            }
            if let Err(e) = fs::rename(src.join(&name), &dest).await {
                tracing::warn!("[Scratchpad] 迁移：移动 {name:?} 失败: {e}");
            }
        }
    }

    /// 把旧回收站目录里的条目逐一移入项目级回收站（按名称还原到模块根）。
    async fn ingest_trash_dir(&self, old_trash: &Path) {
        if !old_trash.is_dir() {
            return;
        }
        if self.trash.ensure_dir().await.is_err() {
            return;
        }
        let Ok(mut read_dir) = fs::read_dir(old_trash).await else {
            return;
        };
        let mut names: Vec<std::ffi::OsString> = Vec::new();
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            names.push(entry.file_name());
        }
        for name in names {
            let name_str = name.to_string_lossy().to_string();
            let source = old_trash.join(&name);
            let _ = self
                .trash
                .move_to_trash(&source, ORIGIN_SCRATCHPAD, &name_str)
                .await;
        }
        let _ = fs::remove_dir(old_trash).await;
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

    /// 树 → **扁平文件清单**（Quick Open 的文件源；目录不占行，它已体现在 `folder` 上）。
    ///
    /// - 内部 / 隐藏路径沿用 [`Self::relative_path_of`] 的守卫（与草稿箱自己的 API 同一口径，
    ///   不在这里重写一份“哪些算草稿”的判定）；
    /// - 排序用相对路径：稳定、可解释（文件系统顺序与 `read_dir` 平台行为有关，不能当展示顺序）；
    /// - 超 `limit` 就截断并回传 `true`——**调用方要把「没全列」说出来**，不当成「就这么多」。
    pub fn flatten_files(
        &self,
        entries: &[ScratchpadEntry],
        limit: usize,
    ) -> (Vec<FlatFile>, bool) {
        let mut out = Vec::new();
        self.collect_files(entries, &mut out);
        out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        let truncated = out.len() > limit;
        out.truncate(limit);
        (out, truncated)
    }

    fn collect_files(&self, entries: &[ScratchpadEntry], out: &mut Vec<FlatFile>) {
        for entry in entries {
            match entry.kind {
                ScratchpadEntryKind::File => {
                    let Some(relative_path) = self.relative_path_of(&entry.path) else {
                        continue;
                    };
                    out.push(FlatFile {
                        folder: relative_path
                            .rsplit_once('/')
                            .map(|(dir, _)| dir.to_string())
                            .unwrap_or_default(),
                        relative_path,
                        name: entry.name.clone(),
                        path: entry.path.clone(),
                    });
                }
                ScratchpadEntryKind::Folder => {
                    if let Some(children) = entry.children.as_ref() {
                        self.collect_files(children, out);
                    }
                }
            }
        }
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
        self.trash
            .move_to_trash(&target_path, ORIGIN_SCRATCHPAD, relative_path)
            .await?;
        Ok(())
    }

    /// 列出**草稿箱的**回收站条目。
    ///
    /// 回收站是项目级的（与资产库共用一处）：这里按来源过滤——别的模块的条目该在它自己的
    /// 界面里还原 / 删除，摆到草稿箱的列表里再拒一次还原只是噪声。
    pub async fn list_trash(&self) -> Result<Vec<TrashEntry>, CoreError> {
        Ok(self
            .trash
            .list()
            .await?
            .into_iter()
            .filter(|entry| entry.manifest.origin == ORIGIN_SCRATCHPAD)
            .collect())
    }

    /// 从回收站还原到草稿箱；条目属于其他模块时报错（应在其模块中还原）。
    ///
    /// 回收站层不做 origin 校验（它只是如实记录来源）——“跨模块还原必须被拒”是调用方
    /// 的纪律，就在这里。
    pub async fn restore_from_trash(&self, trash_id: &str) -> Result<ScratchpadEntry, CoreError> {
        let entry = self.trash.get(trash_id).await?;
        if entry.manifest.origin != ORIGIN_SCRATCHPAD {
            return Err(io_err(
                &self.trash.dir().join(trash_id),
                "restore_from_trash",
                format!(
                    "该条目属于模块 '{}'，请在其模块中还原",
                    entry.manifest.origin
                ),
            ));
        }
        let restored = self.trash.restore(trash_id, &self.scratchpad_dir).await?;
        // 中性结果 → 草稿箱条目：这一层转换本来就是调用方的活。
        Ok(ScratchpadEntry {
            name: restored.name,
            path: restored.path,
            kind: match restored.kind {
                TrashKind::Folder => ScratchpadEntryKind::Folder,
                TrashKind::File => ScratchpadEntryKind::File,
            },
            size: restored.size,
            modified_at: Some(Utc::now().to_rfc3339()),
            children: (restored.kind == TrashKind::Folder).then(Vec::new),
        })
    }

    /// 清空**草稿箱的**回收站条目。
    ///
    /// 不走 `ProjectTrash::empty`（那是整仓）：仓库是共用的，把资产库的条目一起删掉不是“清空”，
    /// 是越权（模块硬约束 5 的另一面：只有一套回收站，但每个模块只能动自己的）。
    pub async fn empty_trash(&self) -> Result<(), CoreError> {
        self.trash.empty_origin(ORIGIN_SCRATCHPAD).await?;
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

    /// 判断相对路径是否为目录（不存在则报错）。
    pub async fn is_directory(&self, relative_path: &str) -> Result<bool, CoreError> {
        let path = self.resolve_path(relative_path)?;
        let metadata = fs::metadata(&path)
            .await
            .map_err(|e| io_err(&path, "metadata", e.to_string()))?;
        Ok(metadata.is_dir())
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

    /// 用系统默认应用打开条目（`opener::open`：SQL → 本机编辑器、MD → 默认阅读器）。
    ///
    /// 名字里的 explorer 是历史遗留（行为是 open，不是文件管理器定位）；定位请用
    /// [`Self::reveal_in_file_manager`]。
    pub async fn open_in_system_explorer(&self, path_to_open: &Path) -> Result<(), CoreError> {
        opener::open(path_to_open).map_err(|e| {
            CoreError::storage(StorageError::io(
                path_to_open.display().to_string(),
                "open_explorer",
                e.to_string(),
            ))
        })
    }

    /// 在系统文件管理器中**定位**条目（与 `open` 语义分离：不改文件、只把资源管理器
    /// 打开到该文件所在目录并选中它）。
    ///
    /// Windows 用 `explorer /select,<path>`；其他平台退化到“打开父目录”。
    pub async fn reveal_in_file_manager(&self, path_to_open: &Path) -> Result<(), CoreError> {
        #[cfg(target_os = "windows")]
        {
            use std::process::Command;
            let arg = format!("/select,{}", path_to_open.display());
            Command::new("explorer.exe")
                .arg(&arg)
                .spawn()
                .map(|_| ())
                .map_err(|e| {
                    CoreError::storage(StorageError::io(
                        path_to_open.display().to_string(),
                        "reveal_explorer",
                        e.to_string(),
                    ))
                })
        }
        #[cfg(not(target_os = "windows"))]
        {
            let parent = path_to_open.parent().unwrap_or(path_to_open);
            opener::open(parent).map_err(|e| {
                CoreError::storage(StorageError::io(
                    path_to_open.display().to_string(),
                    "reveal_explorer",
                    e.to_string(),
                ))
            })
        }
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

    /// 重命名外部引用别名（仅改别名，路径不变）。
    pub async fn rename_external_reference(
        &self,
        old_alias: &str,
        new_alias: &str,
    ) -> Result<(), CoreError> {
        let new_alias = new_alias.trim();
        if new_alias.is_empty()
            || new_alias.contains('/')
            || new_alias.contains('\\')
            || new_alias.contains("..")
        {
            return Err(CoreError::storage(StorageError::persistence(
                "scratchpad_config",
                "rename_reference",
                format!("invalid alias: '{new_alias}'"),
            )));
        }

        let mut config = self.load_config().await?;
        if config
            .external_references
            .iter()
            .any(|r| r.alias == new_alias)
        {
            return Err(CoreError::storage(StorageError::persistence(
                "scratchpad_config",
                "rename_reference",
                format!("alias '{new_alias}' already exists"),
            )));
        }

        let mut found = false;
        for reference in config.external_references.iter_mut() {
            if reference.alias == old_alias {
                reference.alias = new_alias.to_string();
                found = true;
                break;
            }
        }
        if !found {
            return Err(CoreError::storage(StorageError::persistence(
                "scratchpad_config",
                "rename_reference",
                format!("alias '{old_alias}' not found"),
            )));
        }

        self.save_config(&config).await
    }

    /// 重新定位外部引用（仅改路径，别名与创建时间不变）。
    ///
    /// 源文件被移动/重命名后用它恢复链接（与新增引用一样不校验路径是否存在，
    /// 可用性仍由 `external_reference_status` 探测）。
    pub async fn update_external_reference_path(
        &self,
        alias: &str,
        new_path: PathBuf,
    ) -> Result<(), CoreError> {
        let mut config = self.load_config().await?;
        let mut found = false;
        for reference in config.external_references.iter_mut() {
            if reference.alias == alias {
                reference.path = new_path.clone();
                found = true;
                break;
            }
        }
        if !found {
            return Err(CoreError::storage(StorageError::persistence(
                "scratchpad_config",
                "relink_reference",
                format!("alias '{alias}' not found"),
            )));
        }
        self.save_config(&config).await
    }

    /// 外部引用可用性（加载时探测路径是否存在；失效项供 UI 置灰并允许重新定位）。
    pub async fn external_reference_status(
        &self,
    ) -> Result<Vec<ExternalReferenceStatus>, CoreError> {
        let config = self.load_config().await?;
        Ok(config
            .external_references
            .into_iter()
            .map(|r| ExternalReferenceStatus {
                exists: r.path.exists(),
                is_dir: r.path.is_dir(),
                alias: r.alias,
                path: r.path,
            })
            .collect())
    }

    /// 绑定文件使用的数据源（仅存连接 ID，不落凭据）。
    pub async fn bind_connections(
        &self,
        relative_path: &str,
        connections: Vec<String>,
    ) -> Result<(), CoreError> {
        let mut config = self.load_config().await?;
        let entry = config
            .file_meta
            .entry(relative_path.to_string())
            .or_default();
        entry.bound_connections = connections;
        self.save_config(&config).await
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

    /// 读取单个草稿的元数据（连接绑定 / 最近执行）——打开草稿预选连接的读侧。
    ///
    /// 无记录时返回默认值（空绑定、无最近执行）：调用方只需问「该用哪个连接」，
    /// 不需要区分「没有记录」与「记录为空」。
    pub async fn file_meta(&self, relative_path: &str) -> Result<FileMeta, CoreError> {
        let config = self.load_config().await?;
        Ok(config
            .file_meta
            .get(relative_path)
            .cloned()
            .unwrap_or_default())
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

    /// 复制条目（文件或文件夹，文件夹递归）到目标目录。
    ///
    /// - 命名：`name_copy` / `name_copy_1` … 直到不重名（最多尝试 1000 次）；
    /// - `target_parent` 为空串表示模块根；
    /// - 拒绝把文件夹复制到自身或其子树内（否则递归无界）。
    pub async fn copy_entry(
        &self,
        relative_path: &str,
        target_parent: &str,
    ) -> Result<ScratchpadEntry, CoreError> {
        let source_path = self.resolve_path(relative_path)?;
        let source_name = source_path
            .file_name()
            .ok_or_else(|| {
                CoreError::storage(StorageError::io(
                    source_path.display().to_string(),
                    "copy",
                    "invalid source path: no file name",
                ))
            })?
            .to_string_lossy()
            .to_string();
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
        let parent = if target_parent.is_empty() {
            None
        } else {
            Some(target_parent)
        };
        if is_dir {
            let dest_parent = match parent {
                Some(p) => self.resolve_path(p)?,
                None => self.scratchpad_dir.clone(),
            };
            if dest_parent == source_path || dest_parent.starts_with(&source_path) {
                return Err(CoreError::storage(StorageError::io(
                    relative_path.to_string(),
                    "copy",
                    "cannot copy a folder into itself",
                )));
            }
        }

        let (stem, ext) = split_name_ext(&source_name);
        let mut last_error: Option<CoreError> = None;
        for i in 0..MAX_COPY_ATTEMPTS {
            let candidate = if i == 0 {
                format!("{stem}_copy{ext}")
            } else {
                format!("{stem}_copy_{i}{ext}")
            };
            match self.create_entry(&candidate, parent, is_dir).await {
                Ok(entry) => {
                    let dest_rel = join_relative(target_parent, &candidate);
                    if is_dir {
                        Box::pin(self.copy_dir_contents(&source_path, &dest_rel, 0)).await?;
                    } else {
                        let dest_path = self.resolve_path(&dest_rel)?;
                        // 二进制安全：直接拷字节，不做 UTF-8 解码。
                        fs::copy(&source_path, &dest_path).await.map_err(|e| {
                            CoreError::storage(StorageError::io(
                                source_path.display().to_string(),
                                "copy",
                                e.to_string(),
                            ))
                        })?;
                    }
                    return Ok(entry);
                }
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error.unwrap_or_else(|| {
            CoreError::storage(StorageError::io(
                source_path.display().to_string(),
                "copy",
                "too many name collisions",
            ))
        }))
    }

    /// 递归复制目录内容（深度上限 [`MAX_DEPTH`]，防异常深层目录）。
    async fn copy_dir_contents(
        &self,
        source_dir: &Path,
        dest_relative: &str,
        depth: u32,
    ) -> Result<(), CoreError> {
        if depth >= MAX_DEPTH {
            return Ok(());
        }
        let children = self.scan_dir_tree(source_dir, 0, 0).await?;
        for child in children {
            let name = child.name.clone();
            let is_dir = child.kind == ScratchpadEntryKind::Folder;
            self.create_entry(&name, Some(dest_relative), is_dir)
                .await?;
            let child_dest = join_relative(dest_relative, &name);
            if is_dir {
                Box::pin(self.copy_dir_contents(&child.path, &child_dest, depth + 1)).await?;
            } else {
                let dest_path = self.resolve_path(&child_dest)?;
                fs::copy(&child.path, &dest_path).await.map_err(|e| {
                    CoreError::storage(StorageError::io(
                        child.path.display().to_string(),
                        "copy",
                        e.to_string(),
                    ))
                })?;
            }
        }
        Ok(())
    }

    /// 正则 / 字面量替换（字面量模式转义后仍走 regex，以共享大小写开关）。
    ///
    /// - 正则模式的替换串支持 `$1` 分组引用；字面量模式不解析 `$`（用 `NoExpand`）；
    /// - 无命中时不写盘。
    pub async fn replace_in_file(
        &self,
        relative_path: &str,
        pattern: &str,
        replacement: &str,
        is_regex: bool,
        case_sensitive: bool,
        whole_word: bool,
        preserve_case: bool,
    ) -> Result<ReplaceResult, CoreError> {
        let file_path = self.resolve_path(relative_path)?;
        let original = fs::read_to_string(&file_path).await.map_err(|e| {
            CoreError::storage(StorageError::io(
                file_path.display().to_string(),
                "read",
                e.to_string(),
            ))
        })?;

        // 全字匹配（VSCode `ab` 开关）：非正则模式转义后包 `\b(?:…)\b`，正则模式
        // 直接在用户表达式外包 `\b(?:…)\b` —— 与 `rds_search::search_directory` 同口径。
        let expression = match (is_regex, whole_word) {
            (true, true) => format!("\\b(?:{pattern})\\b"),
            (false, true) => format!("\\b(?:{})\\b", regex::escape(pattern)),
            (true, false) => pattern.to_string(),
            (false, false) => regex::escape(pattern),
        };
        let re = regex::RegexBuilder::new(&expression)
            .case_insensitive(!case_sensitive)
            .build()
            .map_err(|e| {
                CoreError::storage(StorageError::io(
                    file_path.display().to_string(),
                    "regex_compile",
                    e.to_string(),
                ))
            })?;
        let replaced = re.find_iter(&original).count();
        let new_content = if replaced == 0 {
            original
        } else if preserve_case {
            // AB 保留大小写（2026-10-11 接线）：逐匹配替换，替换文本先按捕获组展开
            // （正则 `$1` 等），再按**命中文本**的大小写形态转换（VSCode 语义：
            // 全大写 → 全大写；首字母大写 → 首字母大写；混合/全小写 → 原样）。
            let mut out = String::with_capacity(original.len() + replaced * replacement.len());
            let mut last = 0;
            for caps in re.captures_iter(&original) {
                let m = caps.get(0).expect("捕获组 0 恒存在");
                out.push_str(&original[last..m.start()]);
                let hit = &original[m.start()..m.end()];
                let mut expanded = String::new();
                caps.expand(replacement, &mut expanded);
                out.push_str(&preserve_case_transform(hit, &expanded));
                last = m.end();
            }
            out.push_str(&original[last..]);
            out
        } else if is_regex {
            re.replace_all(&original, replacement).to_string()
        } else {
            re.replace_all(&original, regex::NoExpand(replacement))
                .to_string()
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

/// 拆分文件名与后缀（`foo.sql` → (`foo`, `.sql`)；无后缀时后缀为空串）。
fn split_name_ext(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

/// VSCode 式保留大小写（AB 开关，2026-10-11 接线）：替换文本按**命中文本**的大小写
/// 形态自适应——命中全大写（`FOO`）→ 替换全大写；命中首字母大写、其余小写（`Foo`）
/// → 替换首字母大写、其余小写；混合（`fOO`）/ 全小写 → 原样。
///
/// 空命中（正则 `*` 之类可匹配空串）或空替换文本按原样返回，不做形态转换。
fn preserve_case_transform(hit: &str, replacement: &str) -> String {
    if replacement.is_empty() || hit.is_empty() {
        return replacement.to_string();
    }
    let hit_has_upper = hit.chars().any(char::is_uppercase);
    let hit_has_lower = hit.chars().any(char::is_lowercase);
    if hit_has_upper && !hit_has_lower {
        // 全大写（如 `FOO` / `FOO_BAR`）
        replacement.to_uppercase()
    } else if hit_has_upper
        && hit.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
        && hit.chars().skip(1).all(|c| !c.is_uppercase())
    {
        // 首字母大写、其余小写（如 `Foo`）——只对「其余全小写」的命中做首字母转换，
        // `FooBar` 这类驼峰命中保持原样（VSCode 同口径：不猜驼峰）。
        let mut chars = replacement.chars();
        let first = chars.next().expect("非空已判");
        let mut out = String::new();
        out.extend(first.to_uppercase());
        for c in chars {
            out.extend(c.to_lowercase());
        }
        out
    } else {
        replacement.to_string()
    }
}

/// 拼接模块内相对路径（父目录为空串时退化为子项名）。
fn join_relative(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent.trim_end_matches(['/', '\\']), name)
    }
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

/// 构造存储 IO 错误（统一 path / operation / reason）。
fn io_err(path: &Path, operation: &str, reason: impl Into<String>) -> CoreError {
    CoreError::storage(StorageError::io(
        path.display().to_string(),
        operation,
        reason.into(),
    ))
}

pub(crate) fn unique_path(path: PathBuf) -> PathBuf {
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

// ===== 标签（统一规格 §7.3：project tags + scratchpad_tags 关联） =====
//
// 草稿条目是磁盘文件，身份 = 模块内相对路径（entry_id）；标签定义在项目库 `tags` 表
// （scope=project，由 rds-tag 懒建），关联在 `scratchpad_tags`（迁移 025，同库无外键，
// 引用完整性由「定义先写、关联后写」+ 读取侧过滤 deleted_at 保证，规格 §3.4 D1）。
// 标签是索引，不写文件元数据（规格 §7.3「文件本体不动」）。

impl ScratchpadStore {
    /// 打开项目库标签表（scope=project；`tags` 表由 TagTable 懒建 ensure_tables，幂等）。
    fn project_tags(&self) -> Result<tag::TagTable, CoreError> {
        tag::TagTable::open_project(&self.project_root)
    }

    /// 项目标签定义（活跃，按名称升序）。
    pub fn list_project_tags(&self) -> Result<Vec<tag::Tag>, CoreError> {
        use ::tag::TagDefinitions as _;
        self.project_tags()?.list_tags()
    }

    /// 新建项目标签（同名活跃存在时撞部分唯一索引报错；`key:value` 约定见规格 §6）。
    pub fn create_project_tag(&self, name: &str) -> Result<tag::Tag, CoreError> {
        use ::tag::TagDefinitions as _;
        self.project_tags()?.create_tag(name, None, None)
    }

    /// 某草稿条目的活跃标签（关联 + 定义同库 JOIN，过滤软删；按名称升序）。
    pub fn tags_of_entry(&self, rel_path: &str) -> Result<Vec<tag::Tag>, CoreError> {
        let table = self.project_tags()?;
        let mut stmt = table
            .conn()
            .prepare(
                "SELECT t.id, t.name, t.color, t.icon, t.scope, t.created_at, t.deleted_at
                 FROM scratchpad_tags st
                 JOIN tags t ON t.id = st.tag_id
                 WHERE st.entry_id = ?1 AND t.deleted_at IS NULL
                 ORDER BY t.name COLLATE NOCASE, t.created_at",
            )
            .map_err(|e| self.tag_err("prepare_entry_tags", e))?;
        let rows = stmt
            .query_map([rel_path], |row| {
                Ok(tag::Tag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    icon: row.get(3)?,
                    scope: row.get(4)?,
                    created_at: row.get(5)?,
                    deleted_at: row.get(6)?,
                })
            })
            .map_err(|e| self.tag_err("query_entry_tags", e))?;
        let mut tags = Vec::new();
        for row in rows {
            tags.push(row.map_err(|e| self.tag_err("row_entry_tags", e))?);
        }
        Ok(tags)
    }

    /// 全部草稿条目的活跃标签（一次查全，列表标签筛选的数据源；键 = 模块内相对路径）。
    pub fn tags_by_entry(&self) -> Result<std::collections::HashMap<String, Vec<tag::Tag>>, CoreError> {
        let table = self.project_tags()?;
        let mut stmt = table
            .conn()
            .prepare(
                "SELECT st.entry_id, t.id, t.name, t.color, t.icon, t.scope, t.created_at, t.deleted_at
                 FROM scratchpad_tags st
                 JOIN tags t ON t.id = st.tag_id
                 WHERE t.deleted_at IS NULL
                 ORDER BY t.name COLLATE NOCASE, t.created_at",
            )
            .map_err(|e| self.tag_err("prepare_all_entry_tags", e))?;
        let rows = stmt
            .query_map([], |row| {
                let tag = tag::Tag {
                    id: row.get(1)?,
                    name: row.get(2)?,
                    color: row.get(3)?,
                    icon: row.get(4)?,
                    scope: row.get(5)?,
                    created_at: row.get(6)?,
                    deleted_at: row.get(7)?,
                };
                Ok((row.get::<_, String>(0)?, tag))
            })
            .map_err(|e| self.tag_err("query_all_entry_tags", e))?;
        let mut out = std::collections::HashMap::<String, Vec<tag::Tag>>::new();
        for row in rows {
            let (entry_id, tag) = row.map_err(|e| self.tag_err("row_all_entry_tags", e))?;
            out.entry(entry_id).or_default().push(tag);
        }
        Ok(out)
    }

    /// 打标（幂等：重复关联忽略）。
    pub fn attach_tag(&self, rel_path: &str, tag_id: &str) -> Result<(), CoreError> {
        let table = self.project_tags()?;
        table
            .conn()
            .execute(
                "INSERT OR IGNORE INTO scratchpad_tags (entry_id, tag_id) VALUES (?1, ?2)",
                rusqlite::params![rel_path, tag_id],
            )
            .map_err(|e| self.tag_err("attach_tag", e))?;
        Ok(())
    }

    /// 去标（不存在则无操作）。
    pub fn detach_tag(&self, rel_path: &str, tag_id: &str) -> Result<(), CoreError> {
        let table = self.project_tags()?;
        table
            .conn()
            .execute(
                "DELETE FROM scratchpad_tags WHERE entry_id = ?1 AND tag_id = ?2",
                rusqlite::params![rel_path, tag_id],
            )
            .map_err(|e| self.tag_err("detach_tag", e))?;
        Ok(())
    }

    /// 标签操作错误（定义侧错误由 `tag::TagTable` 自带透传）。
    fn tag_err(&self, operation: &str, reason: impl std::fmt::Display) -> CoreError {
        CoreError::storage(StorageError::Persistence {
            store: "scratchpad_tags".to_string(),
            operation: operation.to_string(),
            reason: reason.to_string(),
        })
    }
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
    async fn module_root_and_meta_isolated() {
        let project = temp_project("root");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();

        store.create_entry("a.sql", None, false).await.unwrap();
        store.create_entry("sub", None, true).await.unwrap();

        // 模块根 = 项目根下可见的 `scratchpad/`；内部元数据在 `.RSmeta/scratchpad`。
        assert!(project.join(MODULE_DIR_NAME).join("a.sql").is_file());
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

        assert!(
            store
                .read_file(".RSmeta/scratchpad/config.json")
                .await
                .is_err()
        );
        assert!(store.read_file("../outside.txt").await.is_err());
        assert!(
            store
                .create_entry("x.sql", Some(".RSmeta"), false)
                .await
                .is_err()
        );

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn trash_is_project_level_with_origin() {
        let project = temp_project("trash");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store.create_entry("doomed.sql", None, false).await.unwrap();

        store.delete_entry("doomed.sql").await.unwrap();
        assert!(!project.join(MODULE_DIR_NAME).join("doomed.sql").exists());

        let entries = store.list_trash().await.unwrap();
        let entry = entries
            .iter()
            .find(|e| e.manifest.name == "doomed.sql")
            .expect("回收站应含 doomed.sql");
        assert_eq!(entry.manifest.origin, ORIGIN_SCRATCHPAD);
        assert_eq!(entry.manifest.original_rel_path, "doomed.sql");
        assert!(
            project
                .join(META_DIR_NAME)
                .join("trash")
                .join(&entry.manifest.id)
                .join("payload")
                .exists()
        );

        let id = entry.manifest.id.clone();
        store.restore_from_trash(&id).await.unwrap();
        assert!(project.join(MODULE_DIR_NAME).join("doomed.sql").is_file());
        assert!(store.list_trash().await.unwrap().is_empty());

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn restore_refuses_other_module_entries() {
        let project = temp_project("other_origin");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();

        // 模拟资源模块删除的文件（由同一项目级回收站收录）。
        let resources = project.join("resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::write(resources.join("keep.sql"), "select 1").unwrap();
        let entry = store
            .trash()
            .move_to_trash(&resources.join("keep.sql"), "resources", "keep.sql")
            .await
            .unwrap();

        assert!(store.restore_from_trash(&entry.manifest.id).await.is_err());

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn external_reference_persists_and_reports_status() {
        let project = temp_project("ref");
        let existing = project.join("linked");
        std::fs::create_dir_all(&existing).unwrap();

        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store
            .add_external_reference("已存在".into(), existing.clone())
            .await
            .unwrap();
        store
            .add_external_reference("已丢失".into(), project.join("nope"))
            .await
            .unwrap();

        assert!(
            project
                .join(META_DIR_NAME)
                .join(META_SUBDIR)
                .join(CONFIG_FILE)
                .is_file()
        );

        let statuses = store.external_reference_status().await.unwrap();
        assert_eq!(statuses.len(), 2);
        assert!(statuses.iter().any(|s| s.alias == "已存在" && s.exists));
        assert!(statuses.iter().any(|s| s.alias == "已存在" && s.is_dir));
        assert!(statuses.iter().any(|s| s.alias == "已丢失" && !s.exists));

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn bound_connections_roundtrip() {
        let project = temp_project("bind");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();

        store
            .bind_connections("q.sql", vec!["P_1".to_string(), "G_2".to_string()])
            .await
            .unwrap();

        let reloaded = ScratchpadStore::new(project.clone());
        let cfg = reloaded.load_config().await.unwrap();
        assert_eq!(
            cfg.file_meta
                .get("q.sql")
                .map(|m| m.bound_connections.clone()),
            Some(vec!["P_1".to_string(), "G_2".to_string()])
        );

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn file_meta_roundtrip_covers_binding_and_last_execution() {
        let project = temp_project("meta");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();

        // 没绑定、没执行过的草稿是常态：返回默认值，不报错。
        let fresh = store.file_meta("q.sql").await.unwrap();
        assert!(fresh.bound_connections.is_empty());
        assert_eq!(fresh.preferred_connection(), None);

        store
            .bind_connections("q.sql", vec!["P_1".to_string()])
            .await
            .unwrap();
        assert_eq!(
            store.file_meta("q.sql").await.unwrap().preferred_connection(),
            Some("P_1")
        );

        // 执行回存：显式绑定保留，最近执行补齐；换实例读回，确认已落盘。
        store
            .update_file_meta("q.sql", Some("G_9".to_string()))
            .await
            .unwrap();
        let reloaded = ScratchpadStore::new(project.clone());
        let meta = reloaded.file_meta("q.sql").await.unwrap();
        assert_eq!(meta.bound_connections, vec!["P_1".to_string()]);
        assert_eq!(meta.last_connection_id.as_deref(), Some("G_9"));
        assert!(meta.last_executed_at.is_some(), "执行时间应被记下");
        assert_eq!(
            meta.preferred_connection(),
            Some("P_1"),
            "预选看显式绑定，执行记录只做回退"
        );

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn diff_with_content_classifies_lines_and_numbers_sides() {
        let project = temp_project("diff");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store.create_entry("q.sql", None, false).await.unwrap();
        store
            .save_file("q.sql", "select 1\nselect 2\n")
            .await
            .unwrap();

        let result = store
            .diff_with_content("q.sql", "select 1\nselect 3\n", "磁盘", "编辑器")
            .await
            .unwrap();

        assert_eq!(result.left_label, "磁盘");
        assert_eq!(result.right_label, "编辑器");
        assert!(result.lines.iter().any(|l| {
            l.kind == DiffLineKind::Unchanged && l.content.trim_end() == "select 1"
        }));
        assert!(result
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Removed && l.content.contains("select 2")));
        assert!(result
            .lines
            .iter()
            .any(|l| l.kind == DiffLineKind::Added && l.content.contains("select 3")));

        // 两侧行号各自编号：新增行没有左侧行号，删除行没有右侧行号。
        let added = result
            .lines
            .iter()
            .find(|l| l.kind == DiffLineKind::Added)
            .unwrap();
        assert!(added.line_number_left.is_none());
        assert!(added.line_number_right.is_some());
        let removed = result
            .lines
            .iter()
            .find(|l| l.kind == DiffLineKind::Removed)
            .unwrap();
        assert!(removed.line_number_left.is_some());
        assert!(removed.line_number_right.is_none());

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn relative_path_of_maps_editor_paths_back_into_the_module() {
        let project = temp_project("relpath");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store.create_entry("q.sql", None, false).await.unwrap();
        store.create_entry("sub", None, true).await.unwrap();
        store
            .create_entry("deep.sql", Some("sub"), false)
            .await
            .unwrap();

        let root = store.scratchpad_dir();
        assert_eq!(
            store.relative_path_of(&root.join("q.sql")),
            Some("q.sql".to_string())
        );
        assert_eq!(
            store.relative_path_of(&root.join("sub").join("deep.sql")),
            Some("sub/deep.sql".to_string()),
            "子目录用 `/` 连接（与条目相对路径同一口径）"
        );
        assert_eq!(store.relative_path_of(root), None, "模块根自身不是草稿");
        assert_eq!(
            store.relative_path_of(&project.join("other.sql")),
            None,
            "模块外的文件不是草稿"
        );
        assert_eq!(
            store.relative_path_of(&root.join(".tmp").join("x.sql")),
            None,
            "内部/隐藏路径不经 API 读写"
        );

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn draft_targets_only_keeps_files_inside_the_module() {
        let project = temp_project("targets");
        let store = ScratchpadStore::new(project.clone());
        let module = store.scratchpad_dir().to_path_buf();

        let targets = store.draft_targets([
            (module.join("q.sql"), Some("P_1".to_string())),
            (project.join("resources").join("r.sql"), Some("P_9".to_string())),
            (module.join("sub").join("a.sql"), None),
        ]);

        assert_eq!(
            targets,
            vec![
                ("q.sql".to_string(), Some("P_1".to_string())),
                ("sub/a.sql".to_string(), None),
            ],
            "只回写模块内的草稿；子目录用 `/` 连接"
        );
        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn draft_targets_let_the_last_receipt_win() {
        let project = temp_project("targets_last");
        let store = ScratchpadStore::new(project.clone());
        let draft = store.scratchpad_dir().join("q.sql");

        let targets = store.draft_targets([
            (draft.clone(), Some("P_1".to_string())),
            (draft, Some("G_2".to_string())),
        ]);

        assert_eq!(
            targets,
            vec![("q.sql".to_string(), Some("G_2".to_string()))],
            "同一份草稿多条回执：最后一次执行的那个连接生效"
        );
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
        assert!(
            project.join(MODULE_DIR_NAME).join("old.sql").is_file(),
            "旧用户文件应迁到模块根"
        );
        assert!(meta.join(CONFIG_FILE).is_file(), "配置应迁到元数据目录");
        assert!(
            store
                .list_trash()
                .await
                .unwrap()
                .iter()
                .any(|e| e.manifest.name == "gone.sql"),
            "旧回收站条目应并入项目级回收站"
        );

        std::fs::remove_dir_all(&project).ok();
    }


    #[tokio::test]
    async fn copy_entry_recurses_and_avoids_name_collisions() {
        let project = temp_project("copy");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store.create_entry("dir", None, true).await.unwrap();
        store.create_entry("a.sql", Some("dir"), false).await.unwrap();
        store
            .save_file("dir/a.sql", "select 1")
            .await
            .unwrap();
        store.create_entry("sub", Some("dir"), true).await.unwrap();
        store.create_entry("b.txt", Some("dir/sub"), false).await.unwrap();
        store.save_file("dir/sub/b.txt", "hi").await.unwrap();

        // 复制到模块根：同名再复制一次 → `dir_copy` → `dir_copy_1`。
        store.copy_entry("dir", "").await.unwrap();
        store.copy_entry("dir", "").await.unwrap();
        assert_eq!(
            std::fs::read_to_string(project.join(MODULE_DIR_NAME).join("dir_copy/a.sql")).unwrap(),
            "select 1",
            "文件夹递归复制应带上内容"
        );
        assert_eq!(
            std::fs::read_to_string(project.join(MODULE_DIR_NAME).join("dir_copy/sub/b.txt"))
                .unwrap(),
            "hi",
            "子目录也应递归复制"
        );
        assert!(project.join(MODULE_DIR_NAME).join("dir_copy_1").is_dir());

        // 复制到自身内部 → 拒绝（防递归无界）。
        assert!(store.copy_entry("dir", "dir").await.is_err());
        assert!(store.copy_entry("dir", "dir/sub").await.is_err());

        // 单文件复制到指定目录（目标目录内已存在同名时自动避让）。
        store.create_entry("root.sql", None, false).await.unwrap();
        store.save_file("root.sql", "select 2").await.unwrap();
        store.copy_entry("root.sql", "dir_copy").await.unwrap();
        assert_eq!(
            std::fs::read_to_string(project.join(MODULE_DIR_NAME).join("dir_copy/root_copy.sql"))
                .unwrap(),
            "select 2"
        );
        store.copy_entry("root.sql", "").await.unwrap();
        assert!(project.join(MODULE_DIR_NAME).join("root_copy.sql").is_file());

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn replace_in_file_handles_case_regex_and_literal_dollar() {
        let project = temp_project("replace");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store.create_entry("a.sql", None, false).await.unwrap();
        store
            .save_file("a.sql", "select ID from t\nselect id from t\n")
            .await
            .unwrap();

        // 字面量 + 不区分大小写：两行都替掉，且替换串里的 `$` 不当作分组引用。
        let r = store
            .replace_in_file("a.sql", "id", "$key", false, false, false, false)
            .await
            .unwrap();
        assert_eq!(r.replaced, 2);
        let content = store.read_file("a.sql").await.unwrap();
        assert!(content.contains("select $key"), "字面量替换不解析 `$`：{content}");

        // 区分大小写：仅匹配大写。
        let r = store
            .replace_in_file("a.sql", "KEY", "id", false, true, false, false)
            .await
            .unwrap();
        assert_eq!(r.replaced, 0, "区分大小写时不应命中 `$key`");

        // 正则分组引用。
        store.save_file("a.sql", "select a1, a2 from t\n").await.unwrap();
        let r = store
            .replace_in_file("a.sql", "a([0-9])", "col_$1", true, true, false, false)
            .await
            .unwrap();
        assert_eq!(r.replaced, 2);
        assert!(
            store
                .read_file("a.sql")
                .await
                .unwrap()
                .contains("col_1, col_2")
        );

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn replace_in_file_preserves_case_like_vscode() {
        let project = temp_project("replace_pc");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store.create_entry("a.sql", None, false).await.unwrap();

        // 命中全大写（SELECT）→ 替换文本全大写；首字母大写（From）→ 首字母大写；
        // 小写（where）→ 原样。
        store
            .save_file("a.sql", "SELECT id FROM t\nselect id from t\nSelect x\n")
            .await
            .unwrap();
        let r = store
            .replace_in_file("a.sql", "(select|from)", "into", true, false, false, true)
            .await
            .unwrap();
        assert_eq!(r.replaced, 5, "SELECT/FROM/select/from/Select 共 5 处命中");
        let content = store.read_file("a.sql").await.unwrap();
        assert!(content.contains("INTO id INTO t"), "全大写命中应全大写：{content}");
        assert!(content.contains("into id into t"), "小写命中应原样：{content}");
        assert!(content.contains("Into x"), "首字母大写命中应首字母大写：{content}");

        // 混合形态（SeLeCt）保持原样。
        store.save_file("a.sql", "SeLeCt 1\n").await.unwrap();
        let r = store
            .replace_in_file("a.sql", "select", "into", false, false, false, true)
            .await
            .unwrap();
        assert_eq!(r.replaced, 1);
        assert!(
            store.read_file("a.sql").await.unwrap().contains("into 1"),
            "混合大小写命中应原样替换：{}",
            store.read_file("a.sql").await.unwrap()
        );

        // 正则分组 + 保留大小写：展开捕获组后再按命中形态转换。
        // 注：regex crate 的 `$1` 后紧跟字母/下划线会当作命名组（`$1_y` → 组名 `1_y`，
        // 不存在则展开为空），故分组引用后接普通字符需用 `${1}` 花括号形式。
        store.save_file("a.sql", "col_A1\n").await.unwrap();
        let r = store
            .replace_in_file("a.sql", "col_([A-Z])[0-9]", "x_${1}_y", true, true, false, true)
            .await
            .unwrap();
        assert_eq!(r.replaced, 1);
        let content = store.read_file("a.sql").await.unwrap();
        assert!(
            content.contains("x_A_y"),
            "正则捕获组展开后再转换，实际内容：{content:?}"
        );

        // 未开保留大小写：原语义不变（全大写命中也按字面替换）。
        store.save_file("a.sql", "ABC\n").await.unwrap();
        let r = store
            .replace_in_file("a.sql", "abc", "def", false, false, false, false)
            .await
            .unwrap();
        assert_eq!(r.replaced, 1);
        assert!(
            store.read_file("a.sql").await.unwrap().contains("def"),
            "未开保留大小写按字面替换"
        );

        std::fs::remove_dir_all(&project).ok();
    }


    #[tokio::test]
    async fn external_reference_relink_updates_path_only() {
        let project = temp_project("ref_relink");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store
            .add_external_reference("数据".into(), project.join("gone"))
            .await
            .unwrap();
        assert!(
            !store.external_reference_status().await.unwrap()[0].exists,
            "路径不存在时应报告失效"
        );

        let new_path = project.join("moved");
        store
            .update_external_reference_path("数据", new_path.clone())
            .await
            .unwrap();
        let status = &store.external_reference_status().await.unwrap()[0];
        assert_eq!(status.alias, "数据", "重新引用只改路径，不改别名");
        assert_eq!(status.path, new_path);
        // 旧路径变成“已存在”（仅路径替换，未知足切换）
        assert!(store.update_external_reference_path("不存在", new_path).await.is_err());

        std::fs::remove_dir_all(&project).ok();
    }

    #[tokio::test]
    async fn external_reference_rename_and_validation() {
        let project = temp_project("ref_rename");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        store
            .add_external_reference("旧名".into(), project.join("linked"))
            .await
            .unwrap();

        // 改名成功。
        store
            .rename_external_reference("旧名", "新名")
            .await
            .unwrap();
        let statuses = store.external_reference_status().await.unwrap();
        assert!(statuses.iter().any(|s| s.alias == "新名"));

        // 空别名 / 含分隔符 → 拒绍。
        assert!(store.rename_external_reference("新名", "  ").await.is_err());
        assert!(
            store
                .rename_external_reference("新名", "a/b")
                .await
                .is_err()
        );

        // 目标别名不存在 → 拒绝。
        assert!(
            store
                .rename_external_reference("不存在", "x")
                .await
                .is_err()
        );

        std::fs::remove_dir_all(&project).ok();
    }

    /// 测试里建 `scratchpad_tags` 表（迁移 025 的等价 DDL；生产由迁移系统建）。
    fn ensure_scratchpad_tags_table(store: &ScratchpadStore) {
        let table = store.project_tags().expect("open project tags");
        table
            .conn()
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS scratchpad_tags (
                    entry_id TEXT NOT NULL,
                    tag_id   TEXT NOT NULL,
                    PRIMARY KEY (entry_id, tag_id)
                );
                CREATE INDEX IF NOT EXISTS idx_scratchpad_tags_tag ON scratchpad_tags(tag_id);",
            )
            .expect("create scratchpad_tags");
    }

    #[tokio::test]
    async fn tag_attach_detach_and_read_back() {
        let project = temp_project("tags");
        let store = ScratchpadStore::new(project.clone());
        store.ensure_dir().await.unwrap();
        ensure_scratchpad_tags_table(&store);

        // 新建项目标签（scope=project）。
        let a = store.create_project_tag("报表").unwrap();
        let b = store.create_project_tag("重要").unwrap();
        assert_eq!(a.scope, tag::SCOPE_PROJECT, "草稿箱标签作用域 = project");
        // 同名活跃拒绝（部分唯一索引）。
        assert!(store.create_project_tag("报表").is_err());

        // 打标 + 读取（幂等：重复关联忽略）。
        store.attach_tag("drafts/a.sql", &a.id).unwrap();
        store.attach_tag("drafts/a.sql", &b.id).unwrap();
        store.attach_tag("drafts/a.sql", &a.id).unwrap();
        store.attach_tag("drafts/b.sql", &a.id).unwrap();
        let tags_a = store.tags_of_entry("drafts/a.sql").unwrap();
        assert_eq!(tags_a.len(), 2, "两条标签，重复打标不叠加");
        assert_eq!(tags_a[0].name, "报表", "按名称升序：报表在前");
        assert_eq!(tags_a[1].name, "重要");

        // 一次查全（列表筛选数据源）。
        let by_entry = store.tags_by_entry().unwrap();
        assert_eq!(by_entry.len(), 2, "两个条目各带标签");
        assert_eq!(by_entry["drafts/b.sql"][0].id, a.id);

        // 去标（不存在则无操作）。
        store.detach_tag("drafts/a.sql", &a.id).unwrap();
        store.detach_tag("drafts/a.sql", &a.id).unwrap();
        let tags_a = store.tags_of_entry("drafts/a.sql").unwrap();
        assert_eq!(tags_a.len(), 1);
        assert_eq!(tags_a[0].id, b.id);

        // 软删定义后读取侧过滤（规格 §5 D2：删除不清关联）。
        use ::tag::TagDefinitions as _;
        let table = store.project_tags().unwrap();
        table.delete_tag(&b.id).unwrap();
        let tags_a = store.tags_of_entry("drafts/a.sql").unwrap();
        assert!(tags_a.is_empty(), "定义软删后关联读取侧过滤掉");

        std::fs::remove_dir_all(&project).ok();
    }
}
