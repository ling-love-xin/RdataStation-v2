//! 本体搬运：拷入、归档、移出、替换与导出。

use super::*;

/// 暂存目录名（点前缀，`list_files` 会跳过，不会被误报成未登记本体）。
const STAGING_DIR: &str = ".staging";

/// 暂存 sidecar：光有 UUID 文件名无法反查目标 `rel`，重放就无从谈起。
#[derive(serde::Serialize, serde::Deserialize)]
struct StagedMeta {
    rel: String,
}

/// 一条待重放的暂存内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedPayload {
    /// 暂存文件本体（`<uuid>.tmp`）。
    pub staged: PathBuf,
    /// 它应替换到的 `resources/` 相对路径。
    pub rel: String,
}

fn sidecar(staged: &Path) -> PathBuf {
    staged.with_extension("meta")
}

/// 只向下找 `.staging` 目录（不进 `.staging` 本身），深度有上限防异常目录树。
fn collect_staging(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some(STAGING_DIR) {
            if let Ok(files) = std::fs::read_dir(&path) {
                for file in files.flatten() {
                    let candidate = file.path();
                    if candidate.extension().and_then(|e| e.to_str()) == Some("tmp") {
                        out.push(candidate);
                    }
                }
            }
            continue;
        }
        collect_staging(&path, out, depth + 1);
    }
}

impl PayloadStore {
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
            return Err(io_err(
                source,
                "copy_out_source",
                "源文件已不在（reference 快照失效）",
            ));
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

    /// 为「再归档 / 历史还原」把新内容落到**同卷暂存区**，不碰本体。
    ///
    /// 为什么必须有这一步：原来的顺序是「覆盖本体 → 更新索引」，中间失败会留下
    /// 「本体已是新内容、索引还是旧指纹」的错位，且新内容可能再也找不回来。改成
    /// 「暂存 → 索引事务提交 → 原子替换」后，任一步失败都有凭据可恢复：旧内容在版本副本、
    /// 新内容在暂存文件、索引状态可被 `indexer` 检出。
    ///
    /// 暂存目录用点前缀：与 `list_files` 跳过隐藏项的口径一致，不会被误报成未登记本体。
    pub async fn stage_payload(&self, source: &Path, rel: &str) -> Result<PathBuf, CoreError> {
        if !source.is_file() {
            return Err(io_err(source, "stage", "新内容不存在或不是普通文件（副本可能已消失）"));
        }
        let dest = self.resolve(rel)?;
        let parent = dest
            .parent()
            .ok_or_else(|| io_err(&dest, "stage", "本体没有父目录"))?;
        let staging = parent.join(STAGING_DIR);
        fs::create_dir_all(&staging)
            .await
            .map_err(|e| io_err(&staging, "stage_create_dir", e))?;
        let staged = staging.join(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
        fs::copy(source, &staged)
            .await
            .map_err(|e| io_err(source, "stage_copy", e))?;
        // sidecar 先落：没有它，进程崩溃后的重放无法知道这份内容该替换谁。
        let meta = sidecar(&staged);
        let encoded = serde_json::to_string(&StagedMeta {
            rel: rel.to_string(),
        })
        .map_err(|e| io_err(&meta, "stage_meta_serialize", e))?;
        if let Err(error) = fs::write(&meta, encoded).await {
            let _ = fs::remove_file(&staged).await;
            return Err(io_err(&meta, "stage_meta_write", error));
        }
        Ok(staged)
    }

    /// 列出全部待重放的暂存内容（同步：只扫目录树，不读本体字节）。
    ///
    /// 没有可用 sidecar 的老残留**一律跳过并告警**：猜路径去覆盖归档本体，
    /// 比留着一份没人认领的暂存文件危险得多。
    pub fn list_staged(&self) -> Vec<StagedPayload> {
        let mut files = Vec::new();
        collect_staging(&self.resources_dir(), &mut files, 0);
        let mut out = Vec::new();
        for staged in files {
            let meta = sidecar(&staged);
            let text = match std::fs::read_to_string(&meta) {
                Ok(text) => text,
                Err(error) => {
                    tracing::warn!(error = %error, path = %staged.display(), "暂存缺少 sidecar 元数据，跳过重放");
                    continue;
                }
            };
            match serde_json::from_str::<StagedMeta>(&text) {
                Ok(value) => out.push(StagedPayload {
                    staged,
                    rel: value.rel,
                }),
                Err(error) => {
                    tracing::warn!(error = %error, path = %meta.display(), "暂存元数据损坏，跳过重放");
                }
            }
        }
        out
    }

    /// 把暂存内容替换到本体（同卷 `rename`，Windows 上会覆盖已存在的目标）。
    ///
    /// 目标只读属性先清：`replace_payload` 的老路径也是这么做的，否则 Windows 下换不动。
    /// 跨卷 / 占用导致 rename 失败时退回复制——窗口比原来小得多，且暂存文件仍在，可重试。
    pub async fn commit_staged(&self, staged: &Path, rel: &str) -> Result<PathBuf, CoreError> {
        let dest = self.resolve(rel)?;
        if dest.exists() {
            if let Err(e) = self.set_readonly(&dest, false).await {
                tracing::warn!(error = %e, path = %dest.display(), "替换前清除本体只读属性失败");
            }
        }
        if let Err(rename_error) = fs::rename(staged, &dest).await {
            fs::copy(staged, &dest)
                .await
                .map_err(|e| io_err(staged, "commit_staged_copy", format!("{rename_error}; {e}")))?;
            // rename 没搬走 staged：copy 成功后清掉，别留下「已提交但暂存还在」的假象，
            // 否则下次刷新会把它当成待重放内容又覆盖一次本体。
            if let Err(error) = fs::remove_file(staged).await {
                tracing::warn!(error = %error, path = %staged.display(), "copy 兜底后清理暂存文件失败");
            }
        }
        if let Err(error) = fs::remove_file(sidecar(staged)).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %error, path = %staged.display(), "清理暂存元数据失败");
            }
        }
        if let Err(e) = self.set_readonly(&dest, true).await {
            tracing::warn!(error = %e, path = %dest.display(), "替换后设置只读属性失败（应用层守卫仍生效）");
        }
        Ok(dest)
    }

    /// 丢弃暂存文件（含 sidecar）。`rename` 成功后它已经不在了，不存在不算错；其它失败只记日志。
    pub async fn discard_staged(&self, staged: &Path) {
        if let Err(error) = fs::remove_file(staged).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %error, path = %staged.display(), "清理暂存文件失败");
            }
        }
        if let Err(error) = fs::remove_file(sidecar(staged)).await {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %error, path = %staged.display(), "清理暂存元数据失败");
            }
        }
    }
}
