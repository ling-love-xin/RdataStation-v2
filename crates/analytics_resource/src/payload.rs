//! 本体层：`{项目}/resources/` 受管文件 + 历史内容副本（架构 §3 / §5.2）。
//!
//! 职责边界：
//! - **只管文件系统**：路径守卫、归档搬运、只读标记、内容指纹、版本副本；不碰 `project.db`
//!   （索引层在 `resource.rs` / `folder.rs` / `tag.rs` / `version.rs`）。
//! - **文件系统是本体权威，登记表是索引**：任何不一致以文件为准，由 `indexer` 侧检测修复。
//!
//! 目录约定（与草稿箱一致：内容可见、内部态隐藏）：
//! ```text
//! {项目}/resources/                          ← 受管内容（可见、归档后只读）
//! {项目}/.RSmeta/resources/versions/<id>/<v>/ ← 历史内容副本（隐藏，按 keep 份数裁剪）
//! ```

use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use shared::error::{CoreError, StorageError};
use tokio::fs;
use tokio::io::AsyncReadExt;

/// 受管内容目录名（项目根下**可见**的模块目录）。
pub const RESOURCES_DIR_NAME: &str = "resources";
/// 项目内部元数据目录名。
///
/// 直接引用 engine 已有常量，而不是再写一份字面量：`.RSmeta` 这一拼写当前有多处各自声明
/// （`project::store` / `project::lock` / `engine::connection_org_store` / `engine::project_db`
/// 内的字面量 / `scratchpad::store`），单一来源收敛见开发方案 P0.3。
pub const META_DIR_NAME: &str = engine::persistence::connection_org_store::RS_META_DIR_NAME;
/// 历史内容副本目录名（位于元数据目录内）。
const VERSIONS_DIR_NAME: &str = "versions";
/// 计算指纹时的读取块大小。
const HASH_CHUNK: usize = 64 * 1024;

/// 本体存储：按**项目根**实例化（项目态不得放进程单例，沿用草稿箱的隔离口径）。
#[derive(Clone)]
pub struct PayloadStore {
    project_root: PathBuf,
}

impl PayloadStore {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
        }
    }

    /// 项目根。
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// 受管内容根：`{项目}/resources/`。
    pub fn resources_dir(&self) -> PathBuf {
        self.project_root.join(RESOURCES_DIR_NAME)
    }

    /// 历史内容副本根：`{项目}/.RSmeta/resources/versions/`。
    fn versions_dir(&self) -> PathBuf {
        self.project_root
            .join(META_DIR_NAME)
            .join(RESOURCES_DIR_NAME)
            .join(VERSIONS_DIR_NAME)
    }

    /// 建目录（首次归档时调用；幂等）。
    pub async fn ensure_dir(&self) -> Result<(), CoreError> {
        let dir = self.resources_dir();
        fs::create_dir_all(&dir)
            .await
            .map_err(|e| io_err(&dir, "create_resources_dir", e))
    }

    /// 相对路径 → 绝对路径，带**越界守卫**与规范化。
    ///
    /// 拒绝：空路径、绝对路径与根相对路径（"/x"、"C:\\x"、"\\x"）、`..`（穿越）、
    /// 以及任何点前缀组件（`.RSmeta`、`.git`、隐藏文件）——内部态不得经任何 API 触达，
    /// 与草稿箱的 `resolve_path` 同一口径。`./` 视为无操作（不参与结果路径）。
    ///
    /// 不做"去前导分隔符"的宽容处理：把 `/abs/x.sql` 悄悄当成 `resources/abs/x.sql`
    /// 比报错危险得多（调用方给错了绝对路径，却得到一个看似成功的归档）。
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, CoreError> {
        let rel = rel.trim();
        if rel.is_empty() {
            return Err(invalid_rel(rel, "路径为空"));
        }

        let candidate = Path::new(rel);
        if candidate.is_absolute() || candidate.has_root() {
            return Err(invalid_rel(rel, "不允许绝对路径 / 根相对路径"));
        }

        let mut resolved = self.resources_dir();
        for component in candidate.components() {
            match component {
                Component::Normal(name) => {
                    if name.to_string_lossy().starts_with('.') {
                        return Err(invalid_rel(rel, "不允许点前缀（内部态）路径"));
                    }
                    resolved.push(name);
                }
                Component::CurDir => {}
                _ => return Err(invalid_rel(rel, "不允许 .. / 根 / 前缀组件")),
            }
        }

        Ok(resolved)
    }

    /// 写入守卫：落在 `resources/` 下的路径一律拒写（只读三重守卫的第一层，原型 §4.6）。
    pub fn ensure_writable(&self, path: &Path) -> Result<(), CoreError> {
        if path.starts_with(self.resources_dir()) {
            return Err(io_err(
                path,
                "ensure_writable",
                "这是归档本体（只读）；要修改请先「取回（检出）」为草稿工作副本",
            ));
        }
        Ok(())
    }

    /// 归档搬运：把源文件**移动**进 `resources/<rel>`，并标记只读。
    ///
    /// 顺序与失败语义（架构 §6.3）：目标已存在即报错（**不静默覆盖**）；跨设备 `rename`
    /// 失败退回复制 + 删除；只读标记失败只警告（Windows / 网络盘不可靠），不影响归档成立。
    pub async fn archive_in(&self, source: &Path, rel: &str) -> Result<PathBuf, CoreError> {
        if !source.is_file() {
            return Err(io_err(source, "archive_in", "源文件不存在或不是普通文件"));
        }
        let dest = self.resolve(rel)?;
        if dest.exists() {
            return Err(io_err(
                &dest,
                "archive_in",
                "目标已存在，请改名或先移除旧存档（不覆盖）",
            ));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| io_err(parent, "archive_create_parent", e))?;
        }

        // 同文件系统 rename 即时；跨设备退回复制 + 删除。
        if fs::rename(source, &dest).await.is_err() {
            fs::copy(source, &dest)
                .await
                .map_err(|e| io_err(source, "archive_copy", e))?;
            fs::remove_file(source)
                .await
                .map_err(|e| io_err(source, "archive_remove_source", e))?;
        }

        if let Err(e) = self.set_readonly(&dest, true).await {
            tracing::warn!(error = %e, path = %dest.display(), "设置只读属性失败（应用层守卫仍生效）");
        }

        Ok(dest)
    }

    /// 把本体**移出** `resources/`（回滚归档、或为"再归档"腾位）：先清只读再 rename。
    ///
    /// 回滚场景下目标回到用户工作区，必须可写，故这里会清除只读属性。
    pub async fn move_payload_out(&self, rel: &str, dest: &Path) -> Result<(), CoreError> {
        let source = self.resolve(rel)?;
        if !source.is_file() {
            return Err(io_err(&source, "move_payload_out", "本体不存在"));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| io_err(parent, "move_out_create_parent", e))?;
        }
        if let Err(e) = self.set_readonly(&source, false).await {
            tracing::warn!(error = %e, path = %source.display(), "清除本体只读属性失败");
        }
        if fs::rename(&source, dest).await.is_err() {
            fs::copy(&source, dest)
                .await
                .map_err(|e| io_err(&source, "move_out_copy", e))?;
            fs::remove_file(&source)
                .await
                .map_err(|e| io_err(&source, "move_out_remove_source", e))?;
        }
        Ok(())
    }

    /// 覆盖本体（"再归档"路径）：旧内容的内容副本由调用方先行保存，这里只做替换并恢复只读。
    pub async fn replace_payload(&self, source: &Path, rel: &str) -> Result<PathBuf, CoreError> {
        let dest = self.resolve(rel)?;
        if dest.exists() {
            if let Err(e) = self.set_readonly(&dest, false).await {
                tracing::warn!(error = %e, path = %dest.display(), "清除旧本体只读属性失败");
            }
            fs::remove_file(&dest)
                .await
                .map_err(|e| io_err(&dest, "replace_remove_old", e))?;
        }
        self.archive_in(source, rel).await
    }

    /// 递归列出本体目录下的全部文件（相对路径，`/` 分隔，已排序）。
    ///
    /// 供索引修复扫描"有文件、无记录"。隐藏项与目录不入结果（与 `resolve` 的守卫口径一致）。
    pub async fn list_files(&self) -> Result<Vec<String>, CoreError> {
        let root = self.resources_dir();
        let mut files = Vec::new();
        if !root.is_dir() {
            return Ok(files);
        }

        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let mut entries = fs::read_dir(&dir)
                .await
                .map_err(|e| io_err(&dir, "list_files_read", e))?;
            while let Ok(Some(entry)) = entries.next_entry().await {
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                let path = entry.path();
                let file_type = entry
                    .file_type()
                    .await
                    .map_err(|e| io_err(&path, "list_files_type", e))?;
                if file_type.is_dir() {
                    stack.push(path);
                } else if file_type.is_file() {
                    if let Ok(rel) = path.strip_prefix(&root) {
                        let normalized = rel
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy().to_string())
                            .collect::<Vec<_>>()
                            .join("/");
                        files.push(normalized);
                    }
                }
            }
        }

        files.sort();
        Ok(files)
    }

    /// 取回（检出）：把本体**复制**到 `dest`（原件不动、仍只读）。
    pub async fn copy_out(&self, rel: &str, dest: &Path) -> Result<(), CoreError> {
        let source = self.resolve(rel)?;
        if !source.is_file() {
            return Err(io_err(&source, "copy_out", "本体不存在"));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| io_err(parent, "checkout_create_parent", e))?;
        }
        fs::copy(&source, dest)
            .await
            .map_err(|e| io_err(dest, "checkout_copy", e))?;

        // 工作副本必须可写：Windows 下 CopyFile 会**继承源文件的只读属性**，
        // 而本体的只读属性正是我们设的，不清理就会出现"取回的文件改不了"。
        if let Err(e) = self.set_readonly(dest, false).await {
            tracing::warn!(error = %e, path = %dest.display(), "清除工作副本只读属性失败");
        }

        Ok(())
    }

    /// 目标相对路径是否已被占用（**文件系统层面**：本体在位）。
    ///
    /// 给对话框做冲突预探用：登记表层面的占用由服务层在提交时拒绝（两处各管一层，
    /// 这里不查库——查库是异步的，而它要在开窗前同步拿到）。
    pub fn rel_path_taken(&self, rel: &str) -> bool {
        self.resolve(rel).map(|path| path.exists()).unwrap_or(false)
    }

    /// 找一个不冲突的相对路径：主名加 `-2` / `-3`…（原型 §4.1：目标已存在就改名，不静默覆盖）。
    ///
    /// 命名规则与取回的落点避让同一套（`workbench` 的 `resource_jobs::free_dest`）：先到先得的
    /// 后缀，不把时间戳塞进文件名——用户后来要拿这个名字去说话，得念得出来。
    /// 试到 `-999` 仍冲突就退回原名，让服务层给出明确的失败。
    pub fn free_rel_path(&self, rel: &str) -> String {
        if !self.rel_path_taken(rel) {
            return rel.to_string();
        }
        let path = Path::new(rel);
        let parent = path.parent().filter(|dir| !dir.as_os_str().is_empty());
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("archive");
        let ext = path.extension().and_then(|ext| ext.to_str());
        for index in 2..1000 {
            let name = match ext {
                Some(ext) => format!("{stem}-{index}.{ext}"),
                None => format!("{stem}-{index}"),
            };
            let candidate = match parent {
                Some(dir) => dir.join(name),
                None => PathBuf::from(name),
            };
            // 统一用 `/`：`file_rel_path` 是要入库并展示的字符串，不能随平台变。
            let candidate = candidate.to_string_lossy().replace('\\', "/");
            if !self.rel_path_taken(&candidate) {
                return candidate;
            }
        }
        rel.to_string()
    }

    /// 设置只读属性（辅助手段；应用层守卫才是硬约束）。
    pub async fn set_readonly(&self, path: &Path, readonly: bool) -> Result<(), CoreError> {
        let mut perms = fs::metadata(path)
            .await
            .map_err(|e| io_err(path, "set_readonly_metadata", e))?
            .permissions();
        perms.set_readonly(readonly);
        fs::set_permissions(path, perms)
            .await
            .map_err(|e| io_err(path, "set_readonly", e))
    }

    /// 是否带只读属性（同步元数据读取；失败视为非只读）。
    pub fn is_readonly(&self, path: &Path) -> bool {
        std::fs::metadata(path)
            .map(|m| m.permissions().readonly())
            .unwrap_or(false)
    }

    /// 内容指纹（sha256，小写十六进制）：版本是否递增、是否"内容已变"的唯一依据（架构 §5.1）。
    pub async fn content_hash(&self, path: &Path) -> Result<String, CoreError> {
        let mut file = fs::File::open(path)
            .await
            .map_err(|e| io_err(path, "hash_open", e))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; HASH_CHUNK];
        loop {
            let read = file
                .read(&mut buf)
                .await
                .map_err(|e| io_err(path, "hash_read", e))?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
        }
        Ok(hex::encode(hasher.finalize()))
    }

    /// 存放一份历史内容副本：`.RSmeta/resources/versions/<id>/<version>/<文件名>`。
    ///
    /// 幂等：同版本副本已存在时直接返回；本体缺失时返回 `Ok(None)`（版本行仍保留，
    /// 由界面上以"副本缺失"呈现，而不是让历史查询失败）。
    pub async fn store_version_copy(
        &self,
        resource_id: &str,
        version: i32,
        rel: &str,
    ) -> Result<Option<PathBuf>, CoreError> {
        let source = self.resolve(rel)?;
        if !source.is_file() {
            return Ok(None);
        }
        let file_name = source
            .file_name()
            .ok_or_else(|| io_err(&source, "version_copy", "本体没有文件名"))?;

        let dir = self
            .versions_dir()
            .join(safe_segment(resource_id)?)
            .join(version.to_string());
        fs::create_dir_all(&dir)
            .await
            .map_err(|e| io_err(&dir, "version_copy_dir", e))?;

        let dest = dir.join(file_name);
        if dest.exists() {
            return Ok(Some(dest));
        }
        fs::copy(&source, &dest)
            .await
            .map_err(|e| io_err(&dest, "version_copy", e))?;
        Ok(Some(dest))
    }

    /// 裁剪历史副本：只保留最近 `keep` 个版本目录，返回被删除的份数。
    ///
    /// **只删内容副本，不删版本行**（架构 §5.2）：版本元数据永久保留，界面以"副本缺失"标注。
    pub async fn prune_version_copies(
        &self,
        resource_id: &str,
        keep: u32,
    ) -> Result<usize, CoreError> {
        let dir = self.versions_dir().join(safe_segment(resource_id)?);
        if !dir.is_dir() {
            return Ok(0);
        }

        let mut versions: Vec<i32> = Vec::new();
        let mut entries = fs::read_dir(&dir)
            .await
            .map_err(|e| io_err(&dir, "version_prune_read", e))?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(version) = entry.file_name().to_string_lossy().parse::<i32>() {
                versions.push(version);
            }
        }

        // 新版本在前，跳过要保留的份数。
        versions.sort_unstable_by(|a, b| b.cmp(a));
        let mut removed = 0usize;
        for version in versions.into_iter().skip(keep as usize) {
            let target = dir.join(version.to_string());
            if fs::remove_dir_all(&target).await.is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// 目录名安全化：id 进入路径前必须是没有分隔符 / 点前缀的单个段。
fn safe_segment(raw: &str) -> Result<String, CoreError> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('.')
        || trimmed.contains(['/', '\\'])
        || trimmed.contains("..")
    {
        return Err(CoreError::storage(StorageError::Persistence {
            store: "analytics_resources".to_string(),
            operation: "safe_segment".to_string(),
            reason: format!("非法 id：{raw}"),
        }));
    }
    Ok(trimmed.to_string())
}

fn invalid_rel(rel: &str, reason: &str) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: "analytics_resources".to_string(),
        operation: "resolve_rel".to_string(),
        reason: format!("非法相对路径 {rel:?}：{reason}"),
    })
}

fn io_err(path: &Path, operation: &str, reason: impl std::fmt::Display) -> CoreError {
    CoreError::storage(StorageError::io(
        path.display().to_string(),
        operation,
        reason.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_payload_{tag}_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("create temp project");
        dir
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn free_rel_path_avoids_taken_names_keeping_dirs_and_extension() {
        let project = temp_project("free_rel");
        let store = PayloadStore::new(&project);

        // 没人占用：原样返回（不无谓地改名）。
        assert_eq!(store.free_rel_path("dau.sql"), "dau.sql");

        let reports = store.resources_dir().join("reports");
        std::fs::create_dir_all(&reports).expect("create dir");
        std::fs::write(reports.join("dau.sql"), b"x").expect("write");
        std::fs::write(reports.join("dau-2.sql"), b"x").expect("write");

        assert!(store.rel_path_taken("reports/dau.sql"));
        assert!(!store.rel_path_taken("reports/other.sql"));
        assert_eq!(
            store.free_rel_path("reports/dau.sql"),
            "reports/dau-3.sql",
            "目录保留、后缀递增、扩展名在末尾"
        );

        // 越界路径不会被当成“可用名”：resolve 失败按已占用对待，退回原名交给服务层报错。
        assert_eq!(store.free_rel_path("../outside.sql"), "../outside.sql");

        cleanup(&project);
    }

    #[test]
    fn resolve_rejects_escape_and_internal_paths() {
        let project = temp_project("resolve");
        let store = PayloadStore::new(&project);

        assert!(store.resolve("a/b.sql").is_ok());
        assert!(store.resolve("../outside.sql").is_err());
        assert!(store.resolve("a/../../outside.sql").is_err());
        assert!(store.resolve(".RSmeta/project.db").is_err());
        assert!(store.resolve("a/.hidden").is_err());
        assert!(store.resolve("/abs.sql").is_err());
        assert!(store.resolve("C:\\abs.sql").is_err());
        assert!(store.resolve("\\abs\\x.sql").is_err());
        assert!(store.resolve("   ").is_err());

        cleanup(&project);
    }

    #[test]
    fn resolve_normalizes_curdir_component() {
        let project = temp_project("normalize");
        let store = PayloadStore::new(&project);

        let normalized = store.resolve("reports/./dau.sql").expect("./ 应视为无操作");
        assert!(normalized.starts_with(store.resources_dir()));
        assert!(
            normalized.ends_with(Path::new("reports").join("dau.sql")),
            "结果路径应已规范化：{}",
            normalized.display()
        );

        cleanup(&project);
    }

    #[tokio::test]
    async fn archive_in_moves_file_and_marks_readonly() {
        let project = temp_project("archive");
        let store = PayloadStore::new(&project);
        let source = project.join("draft.sql");
        tokio::fs::write(&source, b"select 1;")
            .await
            .expect("write draft");

        let dest = store
            .archive_in(&source, "reports/dau.sql")
            .await
            .expect("archive");

        assert!(dest.is_file());
        assert!(!source.exists(), "归档是移动：源文件不应还在原处");
        assert!(dest.starts_with(store.resources_dir()));
        assert!(store.is_readonly(&dest), "归档后应带只读属性");
        assert!(store.ensure_writable(&dest).is_err());

        cleanup(&project);
    }

    #[tokio::test]
    async fn archive_in_refuses_existing_target() {
        let project = temp_project("conflict");
        let store = PayloadStore::new(&project);

        let first = project.join("a.sql");
        tokio::fs::write(&first, b"one").await.expect("write a");
        store.archive_in(&first, "same.sql").await.expect("first");

        let second = project.join("b.sql");
        tokio::fs::write(&second, b"two").await.expect("write b");
        assert!(
            store.archive_in(&second, "same.sql").await.is_err(),
            "目标已存在时必须报错（不静默覆盖）"
        );
        assert!(second.exists(), "失败时源文件必须原样保留");

        cleanup(&project);
    }

    #[tokio::test]
    async fn content_hash_is_stable_and_content_sensitive() {
        let project = temp_project("hash");
        let store = PayloadStore::new(&project);
        let file = project.join("x.sql");

        tokio::fs::write(&file, b"select 1;").await.expect("write");
        let first = store.content_hash(&file).await.expect("hash 1");
        let second = store.content_hash(&file).await.expect("hash 2");
        assert_eq!(first, second, "同一内容指纹必须稳定");
        assert_eq!(first.len(), 64, "sha256 十六进制长度");

        tokio::fs::write(&file, b"select 2;").await.expect("rewrite");
        let third = store.content_hash(&file).await.expect("hash 3");
        assert_ne!(first, third, "内容变化必须改变指纹");

        cleanup(&project);
    }

    #[tokio::test]
    async fn version_copy_is_idempotent_and_prunes_oldest() {
        let project = temp_project("versions");
        let store = PayloadStore::new(&project);
        let source = project.join("draft.sql");
        tokio::fs::write(&source, b"v").await.expect("write");
        store
            .archive_in(&source, "keep.sql")
            .await
            .expect("archive");

        for version in 1..=4 {
            let stored = store
                .store_version_copy("ar_test", version, "keep.sql")
                .await
                .expect("version copy")
                .expect("some path");
            assert!(stored.is_file());
        }
        // 幂等：重复写同版本不报错，也不新增目录。
        assert!(
            store
                .store_version_copy("ar_test", 4, "keep.sql")
                .await
                .expect("idempotent")
                .is_some()
        );

        let removed = store
            .prune_version_copies("ar_test", 2)
            .await
            .expect("prune");
        assert_eq!(removed, 2, "保留 2 份、应删 2 份");
        assert!(store.versions_dir().join("ar_test").join("4").is_dir());
        assert!(!store.versions_dir().join("ar_test").join("1").is_dir());

        cleanup(&project);
    }

    #[tokio::test]
    async fn copy_out_keeps_payload_in_place() {
        let project = temp_project("checkout");
        let store = PayloadStore::new(&project);
        let source = project.join("draft.sql");
        tokio::fs::write(&source, b"select 1;").await.expect("write");
        let payload = store
            .archive_in(&source, "dau.sql")
            .await
            .expect("archive");

        let dest = project.join("scratchpad").join("dau（工作副本）.sql");
        store.copy_out("dau.sql", &dest).await.expect("checkout");

        assert!(dest.is_file(), "取回应产生草稿副本");
        assert!(payload.is_file(), "取回不得移动本体");
        assert!(!store.is_readonly(&dest), "工作副本不应只读");

        cleanup(&project);
    }

    #[tokio::test]
    async fn replace_payload_overwrites_readonly_payload() {
        let project = temp_project("replace");
        let store = PayloadStore::new(&project);

        let first = project.join("v1.sql");
        tokio::fs::write(&first, b"select 1;").await.expect("write v1");
        let payload = store.archive_in(&first, "dau.sql").await.expect("archive v1");
        assert!(store.is_readonly(&payload));
        let hash_v1 = store.content_hash(&payload).await.expect("hash v1");

        let second = project.join("v2.sql");
        tokio::fs::write(&second, b"select 2;").await.expect("write v2");
        let replaced = store.replace_payload(&second, "dau.sql").await.expect("replace");

        assert_eq!(replaced, payload, "路径不变（引用不断）");
        assert!(store.is_readonly(&replaced), "替换后仍只读");
        assert_ne!(
            store.content_hash(&replaced).await.expect("hash v2"),
            hash_v1,
            "内容应为新版本"
        );

        cleanup(&project);
    }

    #[tokio::test]
    async fn move_payload_out_returns_writable_file() {
        let project = temp_project("moveout");
        let store = PayloadStore::new(&project);

        let source = project.join("draft.sql");
        tokio::fs::write(&source, b"select 1;").await.expect("write");
        store.archive_in(&source, "dau.sql").await.expect("archive");

        // 回滚 / 腾位：移出后应回到工作区且可写。
        store
            .move_payload_out("dau.sql", &source)
            .await
            .expect("move out");
        assert!(source.is_file());
        assert!(!store.is_readonly(&source), "移出的文件必须可写");
        assert!(!store.resources_dir().join("dau.sql").exists());

        cleanup(&project);
    }

    #[tokio::test]
    async fn list_files_walks_recursively_and_skips_hidden() {
        let project = temp_project("list");
        let store = PayloadStore::new(&project);
        store.ensure_dir().await.expect("ensure");

        for rel in ["a.sql", "reports/dau.sql", "reports/sub/x.sql"] {
            let src = project.join(format!("src_{}", rel.replace('/', "_")));
            tokio::fs::write(&src, b"select 1;").await.expect("write");
            store.archive_in(&src, rel).await.expect("archive");
        }
        // 隐藏目录不入结果（内部态不得被当成"未登记本体"）。
        tokio::fs::create_dir_all(store.resources_dir().join(".cache"))
            .await
            .expect("mkdir hidden");
        tokio::fs::write(store.resources_dir().join(".cache").join("x"), b"x")
            .await
            .expect("write hidden");

        let files = store.list_files().await.expect("list");
        assert_eq!(
            files,
            vec![
                "a.sql".to_string(),
                "reports/dau.sql".to_string(),
                "reports/sub/x.sql".to_string()
            ],
            "应递归、排序、跳过隐藏项"
        );

        cleanup(&project);
    }

    #[test]
    fn safe_segment_rejects_traversal() {
        assert!(safe_segment("ar_1234").is_ok());
        assert!(safe_segment("../etc").is_err());
        assert!(safe_segment("a/b").is_err());
        assert!(safe_segment(".hidden").is_err());
        assert!(safe_segment("  ").is_err());
    }
}
