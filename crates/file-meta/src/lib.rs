//! 文件元信息 crate
//!
//! 每个文件对应一个 Sidecar 元信息文件（隐藏文件）：
//! - **默认**与原文件同目录：`query.sql` → `.query.sql.meta.json`（文件移动时同目录 sidecar 跟着走）
//! - 消费方也可用 [`read_meta_at`] / [`write_meta_at`] 把元信息集中放到自定义隐藏目录
//!   （如草稿箱的 `.RSmeta/scratchpad/meta/`），避免污染资源管理、天然避开文件监控自激
//!
//! **快照式**元信息：只记录「当前是什么状态」，不记录历史
//! （不记书签怎么变的、不记连接历史、不记执行历史）。
//! 元信息文件随原文件移动（同目录）；原文件被删除后可调用 [`cleanup_meta`] 一并清理。
//!
//! 元信息包含：
//! - 结构版本号（[`FileMeta::schema_version`]，供以后迁移）
//! - 书签列表（行号 + 说明文字）
//! - 上次连接 ID（快照）
//! - 执行元信息（模式 + 上次执行行号）
//! - 视图元信息（折叠范围 + 字体大小）
//! - 扩展属性（[`FileMeta::extra`]，任意键值，以后加字段不用改结构）

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 当前结构版本号（以后升级结构时递增，并配合迁移逻辑）
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// 书签：记录一个位置 + 说明文字
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    /// 行号（从 1 开始）
    pub line: usize,
    /// 说明文字
    pub label: String,
}

/// 执行相关元信息（快照：只记当前状态，不记执行历史）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionMeta {
    /// 执行模式（current / all / selection）
    pub mode: String,
    /// 上次执行行号
    pub last_line: Option<usize>,
}

impl Default for ExecutionMeta {
    fn default() -> Self {
        Self {
            mode: "current".to_string(),
            last_line: None,
        }
    }
}

/// 折叠范围（闭区间行号）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FoldRange {
    pub start_line: usize,
    pub end_line: usize,
}

/// 视图相关元信息（快照：只记当前状态）
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ViewMeta {
    /// 折叠范围
    #[serde(default)]
    pub folded_ranges: Vec<FoldRange>,
    /// 字体大小
    #[serde(default)]
    pub font_size: Option<f32>,
}

/// 文件元信息（快照式）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileMeta {
    /// 结构版本号（以后迁移用）
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    /// 书签列表
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,

    /// 上次连接 ID（快照：只记当前连的是哪个，不记连接历史）
    #[serde(default)]
    pub last_connection: Option<String>,

    /// 执行相关元信息
    #[serde(default)]
    pub execution: ExecutionMeta,

    /// 视图相关元信息
    #[serde(default)]
    pub view: ViewMeta,

    /// 扩展属性（以后加新属性不用改结构）
    #[serde(default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for FileMeta {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            bookmarks: Vec::new(),
            last_connection: None,
            execution: ExecutionMeta::default(),
            view: ViewMeta::default(),
            extra: serde_json::Map::new(),
        }
    }
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

/// 错误类型
#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// 获取元信息文件的路径
///
/// `query.sql` → `.query.sql.meta.json`
pub fn meta_path(file_path: &Path) -> PathBuf {
    let file_name = file_path.file_name().unwrap_or_default();
    let file_name_str = file_name.to_string_lossy();
    let meta_name = format!(".{file_name_str}.meta.json");
    file_path.with_file_name(meta_name)
}

/// 读取文件元信息
///
/// 元信息文件不存在时，返回默认的空元信息。
pub fn read_meta(file_path: &Path) -> Result<FileMeta, MetaError> {
    read_meta_at(&meta_path(file_path))
}

/// 以**显式 sidecar 路径**读取元信息
///
/// 供把元信息放到自定义目录的消费方（如 scratchpad 的集中隐藏目录）使用；
/// 与 [`read_meta`] 的语义一致：文件不存在返回默认空元信息，更高 schema 版本降级告警。
pub fn read_meta_at(meta_path: &Path) -> Result<FileMeta, MetaError> {
    if !meta_path.exists() {
        return Ok(FileMeta::default());
    }
    let content = std::fs::read_to_string(meta_path)?;
    let meta: FileMeta = serde_json::from_str(&content)?;
    // 前向兼容提示：读到比当前更新的结构版本，说明磁盘上是更新版本写出的文件
    if meta.schema_version > CURRENT_SCHEMA_VERSION {
        tracing::warn!(
            schema_version = meta.schema_version,
            current = CURRENT_SCHEMA_VERSION,
            path = %meta_path.display(),
            "file meta schema_version 高于当前支持版本，字段可能被降级读取"
        );
    }
    Ok(meta)
}

/// 写入文件元信息
pub fn write_meta(file_path: &Path, meta: &FileMeta) -> Result<(), MetaError> {
    write_meta_at(&meta_path(file_path), meta)
}

/// 以**显式 sidecar 路径**写入元信息（供自定义元数据目录的消费方使用）
///
/// 父目录不存在时自动创建（自定义目录首次写入必需）。
pub fn write_meta_at(meta_path: &Path, meta: &FileMeta) -> Result<(), MetaError> {
    if let Some(parent) = meta_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let content = serde_json::to_string_pretty(meta)?;
    std::fs::write(meta_path, content)?;
    Ok(())
}

/// 清理元信息文件
///
/// 原文件不存在时，自动删除对应的 Sidecar 元信息文件；
/// 原文件仍存在（或元信息文件本就不存在）时为空操作，不报错。
pub fn cleanup_meta(file_path: &Path) -> Result<(), MetaError> {
    if file_path.exists() {
        return Ok(());
    }
    let meta_path = meta_path(file_path);
    if meta_path.exists() {
        std::fs::remove_file(&meta_path)?;
    }
    Ok(())
}

/// 添加书签（同一行号已存在时更新其说明文字，并保持按行号排序）
pub fn add_bookmark(file_path: &Path, line: usize, label: &str) -> Result<(), MetaError> {
    let mut meta = read_meta(file_path)?;
    if let Some(existing) = meta.bookmarks.iter_mut().find(|b| b.line == line) {
        existing.label = label.to_string();
    } else {
        meta.bookmarks.push(Bookmark {
            line,
            label: label.to_string(),
        });
        meta.bookmarks.sort_by_key(|b| b.line);
    }
    write_meta(file_path, &meta)?;
    Ok(())
}

/// 删除书签
pub fn remove_bookmark(file_path: &Path, line: usize) -> Result<(), MetaError> {
    let mut meta = read_meta(file_path)?;
    meta.bookmarks.retain(|b| b.line != line);
    write_meta(file_path, &meta)?;
    Ok(())
}

/// 列出所有书签
pub fn list_bookmarks(file_path: &Path) -> Result<Vec<Bookmark>, MetaError> {
    let meta = read_meta(file_path)?;
    Ok(meta.bookmarks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 建一个真实源文件，返回其路径
    fn write_source(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let file_path = dir.path().join("test.sql");
        fs::write(&file_path, "SELECT 1").unwrap();
        file_path
    }

    #[test]
    fn test_meta_path() {
        let file_path = Path::new("/tmp/test.sql");
        let meta_path = meta_path(file_path);
        assert_eq!(meta_path, Path::new("/tmp/.test.sql.meta.json"));
    }

    #[test]
    fn test_meta_path_no_dir() {
        let meta = meta_path(Path::new("query.sql"));
        assert_eq!(meta, PathBuf::from(".query.sql.meta.json"));
    }

    #[test]
    fn test_default_meta_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("missing.sql");

        let meta = read_meta(&file_path).unwrap();
        assert_eq!(meta.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(meta.bookmarks.is_empty());
        assert_eq!(meta.last_connection, None);
        assert_eq!(meta.execution.mode, "current");
        assert_eq!(meta.execution.last_line, None);
        assert!(meta.view.folded_ranges.is_empty());
        assert_eq!(meta.view.font_size, None);
        assert!(meta.extra.is_empty());
    }

    #[test]
    fn test_schema_version_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = write_source(&dir);

        let meta = FileMeta::default();
        write_meta(&file_path, &meta).unwrap();

        let reloaded = read_meta(&file_path).unwrap();
        assert_eq!(reloaded.schema_version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn test_add_and_list_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = write_source(&dir);

        add_bookmark(&file_path, 1, "第一行").unwrap();
        add_bookmark(&file_path, 5, "第五行").unwrap();

        let bookmarks = list_bookmarks(&file_path).unwrap();
        assert_eq!(bookmarks.len(), 2);
        assert_eq!(bookmarks[0].line, 1);
        assert_eq!(bookmarks[0].label, "第一行");
        assert_eq!(bookmarks[1].line, 5);
        assert_eq!(bookmarks[1].label, "第五行");
    }

    #[test]
    fn test_remove_bookmark() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = write_source(&dir);

        add_bookmark(&file_path, 1, "第一行").unwrap();
        assert_eq!(list_bookmarks(&file_path).unwrap().len(), 1);

        remove_bookmark(&file_path, 1).unwrap();
        assert_eq!(list_bookmarks(&file_path).unwrap().len(), 0);
    }

    #[test]
    fn test_update_bookmark() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = write_source(&dir);

        add_bookmark(&file_path, 1, "第一行").unwrap();
        assert_eq!(list_bookmarks(&file_path).unwrap()[0].label, "第一行");

        add_bookmark(&file_path, 1, "更新后的说明").unwrap();
        let bookmarks = list_bookmarks(&file_path).unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].label, "更新后的说明");
    }

    #[test]
    fn test_cleanup_removes_meta_when_source_gone() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = write_source(&dir);
        add_bookmark(&file_path, 1, "第一行").unwrap();
        assert!(meta_path(&file_path).exists());

        // 原文件仍在：清理为空操作
        cleanup_meta(&file_path).unwrap();
        assert!(meta_path(&file_path).exists());

        // 原文件删除后：清理删除元信息文件
        fs::remove_file(&file_path).unwrap();
        cleanup_meta(&file_path).unwrap();
        assert!(!meta_path(&file_path).exists());

        // 元信息文件本就不存在：空操作、不报错
        cleanup_meta(&file_path).unwrap();
    }

    #[test]
    fn test_execution_and_view_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = write_source(&dir);

        let meta = FileMeta {
            last_connection: Some("conn_9".to_string()),
            execution: ExecutionMeta {
                mode: "all".to_string(),
                last_line: Some(42),
            },
            view: ViewMeta {
                folded_ranges: vec![FoldRange {
                    start_line: 2,
                    end_line: 5,
                }],
                font_size: Some(14.0),
            },
            ..FileMeta::default()
        };
        write_meta(&file_path, &meta).unwrap();

        let reloaded = read_meta(&file_path).unwrap();
        assert_eq!(reloaded.last_connection.as_deref(), Some("conn_9"));
        assert_eq!(reloaded.execution.mode, "all");
        assert_eq!(reloaded.execution.last_line, Some(42));
        assert_eq!(reloaded.view.folded_ranges.len(), 1);
        assert_eq!(reloaded.view.folded_ranges[0].start_line, 2);
        assert_eq!(reloaded.view.folded_ranges[0].end_line, 5);
        assert_eq!(reloaded.view.font_size, Some(14.0));
    }

    #[test]
    fn test_extra_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = write_source(&dir);

        let mut meta = FileMeta::default();
        meta.extra.insert(
            "custom".to_string(),
            serde_json::json!({ "a": 1, "b": [true, false] }),
        );
        write_meta(&file_path, &meta).unwrap();

        let reloaded = read_meta(&file_path).unwrap();
        assert_eq!(
            reloaded.extra.get("custom"),
            Some(&serde_json::json!({ "a": 1, "b": [true, false] }))
        );
    }

    #[test]
    fn test_meta_at_uses_explicit_path() {
        // 显式 sidecar 路径（模拟 scratchpad 集中隐藏目录）：读写在自定义目录，
        // 不依赖源文件同目录；同目录 read_meta 不受影响。
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("notes.sql");
        fs::write(&file_path, "SELECT 1").unwrap();

        let meta_dir = dir.path().join(".meta");
        let sidecar = meta_dir.join("notes.sql.meta.json");
        let mut meta = FileMeta::default();
        meta.last_connection = Some("conn_1".to_string());
        write_meta_at(&sidecar, &meta).unwrap();

        let reloaded = read_meta_at(&sidecar).unwrap();
        assert_eq!(reloaded.last_connection.as_deref(), Some("conn_1"));

        // 同目录没有 sidecar → read_meta 仍返回默认
        assert!(read_meta(&file_path).unwrap().last_connection.is_none());
    }

    #[test]
    fn test_read_missing_field_defaults_leniently() {
        // 旧格式 / 缺字段：缺少的顶层字段按默认值补齐（为以后迁移兜底）
        let dir = tempfile::tempdir().unwrap();
        let meta_path = dir.path().join(".legacy.sql.meta.json");
        fs::write(
            &meta_path,
            r#"{"bookmarks":[{"line":3,"label":"旧书签"}]}"#,
        )
        .unwrap();

        let file_path = dir.path().join("legacy.sql");
        fs::write(&file_path, "SELECT 1").unwrap();

        let meta = read_meta(&file_path).unwrap();
        assert_eq!(meta.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(meta.bookmarks.len(), 1);
        assert_eq!(meta.bookmarks[0].line, 3);
        assert_eq!(meta.execution.mode, "current");
        assert!(meta.extra.is_empty());
    }
}
