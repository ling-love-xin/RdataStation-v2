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

use std::io::Read;
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

    /// 项目级回收站（`.RSmeta/trash/`）：本体删除与还原走它（原型 §4.7）。
    ///
    /// 回收站是**整个项目**的（草稿箱与资产库共用），所以它不认识 `resources/`——
    /// 调用方给绝对路径与 `origin` 标记，还原目标根也由调用方指定。
    pub fn trash(&self) -> engine::persistence::trash::ProjectTrash {
        engine::persistence::trash::ProjectTrash::new(&self.project_root)
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

    /// 归档搬运（copy 模式的默认粒度，2026-10-13 裁决）：把源文件**复制**进 `resources/<rel>`，
    /// 并标记只读；**源文件保留在原处**（草稿箱那份 = 工作副本，checkout 语义）。
    ///
    /// 与 [`archive_in`](Self::archive_in)（move 语义，历史路径 / 回滚用）的区别只在"源留不留"：
    /// 顺序与失败语义一致——目标已存在即报错（**不静默覆盖**）；只读标记失败只警告。
    pub async fn copy_in(&self, source: &Path, rel: &str) -> Result<PathBuf, CoreError> {
        if !source.is_file() {
            return Err(io_err(source, "copy_in", "源文件不存在或不是普通文件"));
        }
        let dest = self.resolve(rel)?;
        if dest.exists() {
            return Err(io_err(
                &dest,
                "copy_in",
                "目标已存在，请改名或先移除旧存档（不覆盖）",
            ));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| io_err(parent, "copy_create_parent", e))?;
        }

        fs::copy(source, &dest)
            .await
            .map_err(|e| io_err(source, "copy_in_copy", e))?;

        if let Err(e) = self.set_readonly(&dest, true).await {
            tracing::warn!(error = %e, path = %dest.display(), "设置只读属性失败（应用层守卫仍生效）");
        }

        Ok(dest)
    }

    /// 解析 **reference 模式**的本体：源文件**绝对**路径（迁移 023 的 `source_path`）。
    ///
    /// 与 [`resolve`](Self::resolve)（`resources/` 相对路径 + 越界守卫）分开：reference 模式的
    /// 本体不在本模块管辖的目录里，守卫不适用——但**必须**是绝对路径（拒绝相对路径逃逸）。
    /// 文件不存在时返回错误：reference 快照的"打开 / 取回"依赖源在位，源没了要明说
    /// （"源已变更 / 源已删，快照失效"由调用方拼文案）。
    pub fn resolve_source(&self, source_path: &str) -> Result<PathBuf, CoreError> {
        let path = PathBuf::from(source_path);
        if !path.is_absolute() {
            return Err(io_err(
                &path,
                "resolve_source",
                "reference 模式的本体必须是绝对路径（数据异常）",
            ));
        }
        if !path.is_file() {
            return Err(io_err(
                &path,
                "resolve_source",
                "源文件已不在（reference 快照失效：源被删除或移动）",
            ));
        }
        Ok(path)
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

    /// **删除** `resources/` 下的一份本体（copy 模式的撤销 / 索引修复路径用）。
    ///
    /// 与 [`move_payload_out`](Self::move_payload_out) 的差别：没有目标位置——直接删掉副本。
    pub async fn remove_payload(&self, rel: &str) -> Result<(), CoreError> {
        let path = self.resolve(rel)?;
        if !path.is_file() {
            return Err(io_err(&path, "remove_payload", "本体不存在"));
        }
        fs::remove_file(&path)
            .await
            .map_err(|e| io_err(&path, "remove_payload", e))?;
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

    /// 取回（检出）的 **reference 变体**：源是**绝对路径**（本体在源位置，不经 `resources/` 守卫）。
    ///
    /// 与 [`copy_out`](Self::copy_out) 同一语义（复制、目标可写），只换源。
    pub async fn copy_out_source(&self, source: &Path, dest: &Path) -> Result<(), CoreError> {
        if !source.is_file() {
            return Err(io_err(source, "copy_out_source", "源文件已不在（reference 快照失效）"));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| io_err(parent, "checkout_create_parent", e))?;
        }
        fs::copy(source, dest)
            .await
            .map_err(|e| io_err(dest, "checkout_copy", e))?;
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

    /// 文件字节数（登记 `file_size` 用）：本体是目录或读不到时给 `None`——不假装 0。
    ///
    /// 传的是**任意路径**（归档时是工作区源文件，修复时是本体）：与 [`content_hash`](Self::content_hash)
    /// 同一口径，本层只管字节，不管它住在哪。
    pub async fn file_size(&self, path: &Path) -> Result<Option<i64>, CoreError> {
        let meta = fs::metadata(path)
            .await
            .map_err(|e| io_err(path, "file_size", e))?;
        Ok(meta.is_file().then(|| meta.len() as i64))
    }

    /// 读本体**开头**（内容预览用，原型 §3.1）。
    ///
    /// 返回值：`None` = **不是文本**（开头就有 NUL 字节），`Some((text, complete))`——
    /// `complete = false` 表示后面还有内容（读到 `max_bytes` 还没读完）。
    ///
    /// 两个刻意的小处理：
    /// 1. 截断时**在最后一个换行处切齐**：不切的话行列表最后会多出一条被砍掉一半的行
    ///    （看起来像文件里真有那么一行），顺带也避开了“切在半个 UTF-8 字符上”；
    /// 2. 只看**开头**判二进制（NUL 字节）：要确定“整个文件是不是文本”得读一遍，
    ///    而预览只要一眼——开头就带 NUL 的，几乎不会是给人看的文本。
    pub async fn read_head(
        &self,
        rel: &str,
        max_bytes: usize,
    ) -> Result<Option<(String, bool)>, CoreError> {
        let path = self.resolve(rel)?;
        let file = fs::File::open(&path)
            .await
            .map_err(|e| io_err(&path, "preview_open", e))?;
        // 多读一个字节用来判“后面还有没有”：只读 `max_bytes` 分不清“刚好读完”与“被截了”。
        let mut buf = Vec::with_capacity(max_bytes + 1);
        file.take(max_bytes as u64 + 1)
            .read_to_end(&mut buf)
            .await
            .map_err(|e| io_err(&path, "preview_read", e))?;
        if buf.iter().take(max_bytes).any(|byte| *byte == 0) {
            return Ok(None);
        }
        let complete = buf.len() <= max_bytes;
        buf.truncate(max_bytes);
        // 截断时切到最后一个换行（含换行本身）；一行都没换（超长单行）时只能原样给。
        if !complete {
            if let Some(cut) = buf.iter().rposition(|byte| *byte == b'\n') {
                buf.truncate(cut + 1);
            }
        }
        Ok(Some((String::from_utf8_lossy(&buf).into_owned(), complete)))
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

    /// 同步内容指纹（事件路径懒校验用：reference 打开时比对源文件与登记指纹）。
    ///
    /// 与 [`Self::content_hash`] 同一 sha256 口径；这里不走 tokio，供宿主事件路径直接调用
    /// （单文件、块大小相同，代价与异步版一致）。
    pub fn content_hash_sync(path: &Path) -> Result<String, CoreError> {
        let mut file = std::fs::File::open(path).map_err(|e| io_err(path, "hash_open", e))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; HASH_CHUNK];
        loop {
            let read = file.read(&mut buf).map_err(|e| io_err(path, "hash_read", e))?;
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
    /// 由界面上以“副本缺失”呈现，而不是让历史查询失败）。
    pub async fn store_version_copy(
        &self,
        resource_id: &str,
        version: i32,
        rel: &str,
    ) -> Result<Option<PathBuf>, CoreError> {
        let source = self.resolve(rel)?;
        self.store_version_copy_from(resource_id, version, &source).await
    }

    /// 同上，但源是 **reference 模式的绝对路径**（本体在源位置，不经 `resources/` 守卫）。
    ///
    /// 与 [`store_version_copy`](Self::store_version_copy) 的差别只在源的解析方式：
    /// 本体缺失时返回 `Ok(None)`（源被删 → 该版本无副本，界面上以“副本缺失”呈现）。
    pub async fn store_version_copy_from(
        &self,
        resource_id: &str,
        version: i32,
        source: &Path,
    ) -> Result<Option<PathBuf>, CoreError> {
        if !source.is_file() {
            return Ok(None);
        }
        let file_name = source
            .file_name()
            .ok_or_else(|| io_err(source, "version_copy", "本体没有文件名"))?;

        let dir = self.version_copy_dir(resource_id)?.join(version.to_string());
        fs::create_dir_all(&dir)
            .await
            .map_err(|e| io_err(&dir, "version_copy_dir", e))?;

        let dest = dir.join(file_name);
        if dest.exists() {
            return Ok(Some(dest));
        }
        fs::copy(source, &dest)
            .await
            .map_err(|e| io_err(&dest, "version_copy", e))?;
        Ok(Some(dest))
    }

    /// 历史副本目录：`.RSmeta/resources/versions/<id>/`（可能不存在）。
    fn version_copy_dir(&self, resource_id: &str) -> Result<PathBuf, CoreError> {
        Ok(self.versions_dir().join(safe_segment(resource_id)?))
    }

    /// 有内容副本的版本号（**降序**）；目录不存在 = 空。
    ///
    /// 只回答“哪些版本能还原 / 能取回”：版本行可能在而副本已被裁剪（架构 §5.2）。
    pub async fn version_copies(&self, resource_id: &str) -> Result<Vec<i32>, CoreError> {
        let dir = self.version_copy_dir(resource_id)?;
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut versions = Vec::new();
        let mut entries = fs::read_dir(&dir)
            .await
            .map_err(|e| io_err(&dir, "version_list_read", e))?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Ok(version) = entry.file_name().to_string_lossy().parse::<i32>() {
                versions.push(version);
            }
        }
        versions.sort_unstable_by(|a, b| b.cmp(a));
        Ok(versions)
    }

    /// 某版本的内容副本文件（`None` = 没有副本：被裁剪过，或从未生成）。
    ///
    /// 副本文件名沿用当时的本体名（可能与当前本体名不同）——故用“目录里第一个文件”
    /// 而不是拼文件名，免得改名后找不到自己的历史。
    pub async fn version_copy_file(
        &self,
        resource_id: &str,
        version: i32,
    ) -> Result<Option<PathBuf>, CoreError> {
        let dir = self.version_copy_dir(resource_id)?.join(version.to_string());
        if !dir.is_dir() {
            return Ok(None);
        }
        let mut entries = fs::read_dir(&dir)
            .await
            .map_err(|e| io_err(&dir, "version_copy_read", e))?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.is_file() {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    /// 删除某版本的内容副本（**只删副本，不删版本行**——与裁剪同一口径）；
    /// 返回是否真的删掉了东西。
    pub async fn delete_version_copy(
        &self,
        resource_id: &str,
        version: i32,
    ) -> Result<bool, CoreError> {
        let dir = self.version_copy_dir(resource_id)?.join(version.to_string());
        if !dir.is_dir() {
            return Ok(false);
        }
        fs::remove_dir_all(&dir)
            .await
            .map_err(|e| io_err(&dir, "version_copy_delete", e))?;
        Ok(true)
    }

    /// 把某版本的内容副本复制到外部路径（“取回该版本为草稿”）；
    /// 返回 `false` = 该版本没有副本。
    ///
    /// 与 `copy_out` 的差别只在**源**；落点的可写处理一致（Windows 下复制会继承只读属性）。
    pub async fn copy_version_out(
        &self,
        resource_id: &str,
        version: i32,
        dest: &Path,
    ) -> Result<bool, CoreError> {
        let Some(source) = self.version_copy_file(resource_id, version).await? else {
            return Ok(false);
        };
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| io_err(parent, "version_checkout_create_parent", e))?;
        }
        fs::copy(&source, dest)
            .await
            .map_err(|e| io_err(dest, "version_checkout_copy", e))?;
        if let Err(e) = self.set_readonly(dest, false).await {
            tracing::warn!(error = %e, path = %dest.display(), "清除工作副本只读属性失败");
        }
        Ok(true)
    }

    /// 把某版本的内容副本**写回本体位置**（“还原为当前版本”的本体侧动作）；
    /// 返回 `false` = 该版本没有副本。
    ///
    /// 与 `replace_payload` 的差别：源是副本而不是工作区文件，所以**复制**而不是搬移
    /// （副本要留着，它仍是历史）。指纹判定与版本递增在服务层做，这一层只管字节。
    pub async fn restore_version_copy(
        &self,
        resource_id: &str,
        version: i32,
        rel: &str,
    ) -> Result<bool, CoreError> {
        let Some(source) = self.version_copy_file(resource_id, version).await? else {
            return Ok(false);
        };
        let dest = self.resolve(rel)?;
        if dest.exists() {
            if let Err(e) = self.set_readonly(&dest, false).await {
                tracing::warn!(error = %e, path = %dest.display(), "清除旧本体只读属性失败");
            }
            fs::remove_file(&dest)
                .await
                .map_err(|e| io_err(&dest, "restore_remove_old", e))?;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| io_err(parent, "restore_create_parent", e))?;
        }
        fs::copy(&source, &dest)
            .await
            .map_err(|e| io_err(&dest, "restore_copy", e))?;
        // 写回的内容同样是本体：恢复只读属性（辅助层，应用守卫才是硬约束）。
        if let Err(e) = self.set_readonly(&dest, true).await {
            tracing::warn!(error = %e, path = %dest.display(), "设置本体只读属性失败");
        }
        Ok(true)
    }

    /// 裁剪历史副本：只保留最近 `keep` 个版本目录，返回被删除的份数。
    ///
    /// **只删内容副本，不删版本行**（架构 §5.2）：版本元数据永久保留，界面以"副本缺失"标注。
    pub async fn prune_version_copies(
        &self,
        resource_id: &str,
        keep: u32,
    ) -> Result<usize, CoreError> {
        let dir = self.version_copy_dir(resource_id)?;
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
mod tests;
