//! 版本副本：写入、列举、导出、还原与清理。

use super::*;

impl PayloadStore {
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
        self.store_version_copy_from(resource_id, version, &source)
            .await
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

        let dir = self
            .version_copy_dir(resource_id)?
            .join(version.to_string());
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
    pub(super) fn version_copy_dir(&self, resource_id: &str) -> Result<PathBuf, CoreError> {
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
        let dir = self
            .version_copy_dir(resource_id)?
            .join(version.to_string());
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
        let dir = self
            .version_copy_dir(resource_id)?
            .join(version.to_string());
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
