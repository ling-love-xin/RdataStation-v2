use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScratchpadEntryKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadEntry {
    pub name: String,
    pub path: PathBuf,
    pub kind: ScratchpadEntryKind,
    pub size: u64,
    pub modified_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<ScratchpadEntry>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchMatch {
    pub file: String,
    pub line_number: usize,
    pub line_content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before_context: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after_context: Vec<String>,
    /// 行内命中区间（按字节偏移，作用于 `line_content` 的 `[start, end)`）。
    ///
    /// 供前端渲染命中高亮；每行最多保留 16 段，避免超长行渲染开销。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub match_spans: Vec<(usize, usize)>,
}

/// 扁平行（Quick Open 的文件源）：**模块内相对路径**是身份，绝对路径是打开用的落地值。
///
/// 与树形条目（[`ScratchpadEntry`]）分开：文件搜索要的是「一次拿全 + 能排序 / 能截断」的平表，
/// 而树形加载按目录懒展开（深度、展开态都是 UI 状态，搜索用不上）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FlatFile {
    /// 相对模块根、`/` 分隔（如 `drafts/notes.sql`）。
    pub relative_path: String,
    /// 所在目录（根下为空串）；行右侧展示用。
    pub folder: String,
    /// 文件名（展示主文本）。
    pub name: String,
    /// 绝对路径（编辑器按它打开）。
    pub path: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub total_files_scanned: usize,
    pub total_files_skipped: usize,
    pub skipped_files: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalReference {
    pub alias: String,
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
}

/// 外部引用的可用性（加载时探测路径是否存在，用于置灰/重新定位）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalReferenceStatus {
    pub alias: String,
    pub path: PathBuf,
    pub exists: bool,
    /// 是否为目录（路径不存在时为 `false`）。
    pub is_dir: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzableFile {
    pub name: String,
    pub relative_path: String,
    pub file_type: String,
    pub size_bytes: u64,
    pub duckdb_query_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileMeta {
    /// 最近执行时使用的数据源连接 ID（仅存 ID，凭据在 auth_store）。
    pub last_connection_id: Option<String>,
    pub last_executed_at: Option<DateTime<Utc>>,
    /// 显式绑定的数据源连接 ID（多数据源场景；用于打开文件时预选）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bound_connections: Vec<String>,
}

impl FileMeta {
    /// 打开草稿时预选的连接：**显式绑定优先**，其次回退到最近一次执行用过的连接。
    ///
    /// 两者都没有时返回 `None`——宿主按「未选连接」呈现，不替用户猜一个。
    pub fn preferred_connection(&self) -> Option<&str> {
        self.bound_connections
            .first()
            .map(String::as_str)
            .or(self.last_connection_id.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScratchpadConfig {
    pub external_references: Vec<ExternalReference>,
    #[serde(default)]
    pub file_meta: HashMap<String, FileMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadResponse {
    pub local_entries: Vec<ScratchpadEntry>,
    pub external_references: Vec<ExternalReference>,
    pub scratchpad_path: PathBuf,
    pub file_meta: HashMap<String, FileMeta>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScratchpadChangeEntry {
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScratchpadChangeEvent {
    pub changes: Vec<ScratchpadChangeEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReplaceResult {
    pub replaced: usize,
    pub file_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Unchanged,
    Added,
    Removed,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffLine {
    pub line_number_left: Option<usize>,
    pub line_number_right: Option<usize>,
    pub kind: DiffLineKind,
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffResult {
    pub lines: Vec<DiffLine>,
    pub left_label: String,
    pub right_label: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_connection_prefers_explicit_binding() {
        let mut meta = FileMeta::default();
        assert_eq!(meta.preferred_connection(), None, "未绑定且未执行过 → 不预选");

        meta.last_connection_id = Some("G_9".to_string());
        assert_eq!(
            meta.preferred_connection(),
            Some("G_9"),
            "没有显式绑定时回退最近执行"
        );

        meta.bound_connections = vec!["P_1".to_string(), "P_2".to_string()];
        assert_eq!(
            meta.preferred_connection(),
            Some("P_1"),
            "显式绑定列表取首个（顺序即优先级）"
        );
    }
}
