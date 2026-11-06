//! 本体探测：只读位、大小、头部读取与内容指纹。

use super::*;

impl PayloadStore {
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
            let read = file
                .read(&mut buf)
                .map_err(|e| io_err(path, "hash_read", e))?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
        }
        Ok(hex::encode(hasher.finalize()))
    }
}
