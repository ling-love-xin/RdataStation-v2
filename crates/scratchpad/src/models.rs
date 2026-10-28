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

/// sidecar 里承载 scratchpad 专属字段的 `extra` key。
pub(crate) const EXTRA_BOUND_CONNECTIONS: &str = "scratchpad.bound_connections";
pub(crate) const EXTRA_LAST_EXECUTED_AT: &str = "scratchpad.last_executed_at";

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

    /// 从 sidecar 的 `rds-file-meta::FileMeta` 提取本模块的连接元信息。
    ///
    /// 只读 `last_connection` 与 `extra` 里本模块的两个 key；sidecar 里书签/折叠等
    /// 其他字段与本模块无关，一律忽略（互不干扰）。
    pub(crate) fn from_file_meta(fm: &file_meta::FileMeta) -> Self {
        let bound = fm
            .extra
            .get(EXTRA_BOUND_CONNECTIONS)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let last_executed = fm
            .extra
            .get(EXTRA_LAST_EXECUTED_AT)
            .and_then(|v| v.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        Self {
            last_connection_id: fm.last_connection.clone(),
            last_executed_at: last_executed,
            bound_connections: bound,
        }
    }

    /// 把本模块的连接元信息写进 sidecar 的 `rds-file-meta::FileMeta`。
    ///
    /// 只改 `last_connection` 与 `extra` 里本模块的两个 key，**保留** sidecar 里
    /// 已有的其他字段（书签 / 折叠 / 其他模块的扩展属性）——read-modify-write 由
    /// 调用方（`store`）负责先读全量。
    pub(crate) fn apply_to(&self, fm: &mut file_meta::FileMeta) {
        fm.last_connection = self.last_connection_id.clone();
        fm.extra.insert(
            EXTRA_BOUND_CONNECTIONS.to_string(),
            serde_json::json!(self.bound_connections),
        );
        if let Some(ts) = self.last_executed_at {
            fm.extra.insert(
                EXTRA_LAST_EXECUTED_AT.to_string(),
                serde_json::json!(ts.to_rfc3339()),
            );
        } else {
            fm.extra.remove(EXTRA_LAST_EXECUTED_AT);
        }
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

    #[test]
    fn file_meta_roundtrip_via_sidecar() {
        let mut meta = FileMeta::default();
        meta.last_connection_id = Some("G_9".to_string());
        meta.bound_connections = vec!["P_1".to_string(), "P_2".to_string()];
        meta.last_executed_at = Some(Utc::now());

        let mut fm = file_meta::FileMeta::default();
        // 预置一个与 scratchpad 无关的书签，验证 apply_to 保留它
        fm.bookmarks.push(file_meta::Bookmark {
            line: 3,
            label: "旧书签".to_string(),
        });
        meta.apply_to(&mut fm);

        let back = FileMeta::from_file_meta(&fm);
        assert_eq!(back.last_connection_id.as_deref(), Some("G_9"));
        assert_eq!(back.bound_connections, vec!["P_1", "P_2"]);
        assert!(back.last_executed_at.is_some(), "时间戳经 RFC3339 往返保留");
        assert_eq!(fm.bookmarks.len(), 1, "apply_to 不动书签等无关字段");
    }

    #[test]
    fn from_file_meta_missing_keys_means_default() {
        let fm = file_meta::FileMeta::default();
        let meta = FileMeta::from_file_meta(&fm);
        assert_eq!(meta.last_connection_id, None);
        assert!(meta.bound_connections.is_empty());
        assert!(meta.last_executed_at.is_none());
        assert_eq!(meta.preferred_connection(), None);
    }
}
