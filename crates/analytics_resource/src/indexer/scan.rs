//! 只读扫描：本体缺失、内容已变、未登记文件三类问题。

use super::*;

impl<'a> IndexRepair<'a> {
    /// 只读扫描：**不改任何状态**（可为 0 成本地反复调用，供面板刷新）。
    pub async fn scan(&self) -> Result<IndexScanReport, CoreError> {
        let files = self.payload.list_files().await?;
        let rows = self.store.list_file_archives().await?;

        let mut issues = Vec::new();
        let mut tracked: HashSet<&str> = HashSet::new();

        for row in &rows {
            let Some(rel_path) = row.file_rel_path.as_deref() else {
                continue;
            };
            tracked.insert(rel_path);

            // 分析表的本体也在 `resources/` 里，但它的 `content_hash` 是**结构指纹**
            // （定义 + 列结构，行数不进——见 `analysis.rs` 的裁决）：拿字节 sha256 去比
            // 只会把**每一行分析表都报成“内容已变”**，把「修复…」入口变成噪声。
            // 结构探测（DuckDB `DESCRIBE`）属 P4.1 的第二半，接上之前这里
            // **宁可少报、不可误报**：分析表不参与文件级指纹比对（仍参与本体存在性检查）。
            if ArchiveKind::from_db_str(&row.kind) != ArchiveKind::File {
                continue;
            }
            // reference 模式的本体在**源位置**（`source_path`，可能在项目外），不在
            // `resources/` 下：文件级存在性与指纹比对都不适用（源被改 / 删的校验在打开时做，
            // 见 service 的 `resolve_source`）。扫描跳过，避免把每条引用都报成“本体缺失”。
            if PayloadMode::from_db_str(&row.payload_mode) == PayloadMode::Reference {
                continue;
            }

            let path = self.payload.resolve(rel_path)?;
            if !path.is_file() {
                issues.push(IndexIssue::MissingPayload {
                    resource_id: row.id.clone(),
                    name: row.name.clone(),
                    rel_path: rel_path.to_string(),
                });
                continue;
            }

            // 旧行（007 时代搬运过来的）没有指纹，就没有比对基准——不报"内容已变"。
            let Some(expected_hash) = row.content_hash.as_deref() else {
                continue;
            };
            let actual_hash = self.payload.content_hash(&path).await?;
            if actual_hash != expected_hash {
                issues.push(IndexIssue::ContentChanged {
                    resource_id: row.id.clone(),
                    name: row.name.clone(),
                    rel_path: rel_path.to_string(),
                    expected_hash: expected_hash.to_string(),
                    actual_hash,
                });
            }
        }

        for rel_path in files {
            if !tracked.contains(rel_path.as_str()) {
                issues.push(IndexIssue::UntrackedFile { rel_path });
            }
        }

        Ok(IndexScanReport { issues })
    }

}
