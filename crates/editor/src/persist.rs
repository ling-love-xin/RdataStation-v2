//! 文件持久化（A9：打开 / 保存 / 外部修改检测）
//!
//! ## 为什么单独一层
//!
//! `EditorService` 是**纯逻辑**（不碰文件系统），渲染路径也要求零 I/O。读写盘集中在
//! 本模块，**只从事件路径调用**（打开菜单 / `Ctrl+S` / 关闭前保存），这样：
//! - 服务层的单测不需要临时目录，视图层的帧不需要等磁盘；
//! - "哪些动作会落盘"只有一个地方可查。
//!
//! ## 外部修改检测
//!
//! 保存会覆盖磁盘内容。若文件在编辑器打开后被别的程序改过（另存为、`git checkout`、
//! 别的工具写入），直接覆盖就是**静默丢数据**。因此记录读取时的 mtime，保存前比对，
//! 由调用方决定是否让用户确认（A9 的三态分支）。

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::model::{DocumentId, EditorMode};
use crate::service::OpenOutcome;
use crate::shared::EditorShared;

/// 持久化失败原因（都带路径：错误提示要能指出是哪个文件）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistError {
    /// 读 / 写失败（权限、占用、磁盘）
    Io { path: PathBuf, message: String },
    /// 未命名的文档没有路径：调用方应先走"另存为"
    Untitled(DocumentId),
    /// 文档不存在（已关闭）
    UnknownDocument(DocumentId),
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(f, "{}：{message}", path.display()),
            Self::Untitled(_) => f.write_str("未命名文档需要先「另存为」"),
            Self::UnknownDocument(_) => f.write_str("文档已关闭"),
        }
    }
}

impl std::error::Error for PersistError {}

/// 读文件（打开入口用）
pub fn load(path: &Path) -> Result<String, PersistError> {
    std::fs::read_to_string(path).map_err(|error| PersistError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

/// 写文件（保存用）
pub fn write(path: &Path, content: &str) -> Result<(), PersistError> {
    std::fs::write(path, content).map_err(|error| PersistError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

/// 文件的修改时间（读不到返回 `None`：文件不存在 / 无权限）
pub fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

/// 磁盘上的文件是否在 `recorded` 之后被改过（用于"磁盘副本已变"提示）
///
/// 粒度取决于文件系统（多数为秒级）：比"精确检测"更重要的是**不静默覆盖**。
pub fn external_modified(path: &Path, recorded: SystemTime) -> bool {
    match modified_time(path) {
        Some(current) => current > recorded,
        // 文件消失也算"外部变了"：保存会把它重新创建，值得提示
        None => true,
    }
}

/// 从磁盘打开：读盘 → 服务层落状态
///
/// **同路径已打开时只激活、不重读**（A1 的去重规则）：重读会把用户未保存的编辑冲掉。
pub fn open_file(
    shared: &EditorShared,
    path: &Path,
    mode: EditorMode,
) -> Result<OpenOutcome, PersistError> {
    // 先取出结果再变更：`if let Some(x) = shared.service()…` 会把只读借用活到分支体内，
    // 分支里再 `update()` 就会撞上 `RefCell already borrowed`
    let existing = shared.service().find_by_path(path).cloned();
    if let Some(id) = existing {
        shared.update(|service| service.activate(&id));
        return Ok(OpenOutcome::Activated(id));
    }

    // A13 文件档位：按**实际大小**定策略（>50MB 关重能力，≥200MB 不加载内容）
    let tier = crate::limits::tier_for_path(path);
    let content = if tier.opens_read_only() {
        // 超大文件**不整份读进来**——那正是这个档位要避免的事：读进来就已经把内存吃掉了，
        // 之后再置只读也救不回来。原型 §4 的语义是“不建编辑器会话，只给提示卡”。
        String::new()
    } else {
        load(path)?
    };
    Ok(shared.open(
        crate::service::OpenRequest::file(path, content, mode).with_tier(tier),
    ))
}

/// 保存某文档：写盘成功后清脏（**先写盘、后清脏**，写失败不留"已保存"的假状态）
///
/// 返回落盘路径（未命名文档返回 [`PersistError::Untitled`]，调用方先另存为）。
pub fn save_document(
    shared: &EditorShared,
    id: &DocumentId,
) -> Result<PathBuf, PersistError> {
    let (path, content) = {
        let service = shared.service();
        let document = service
            .find(id)
            .ok_or_else(|| PersistError::UnknownDocument(id.clone()))?;
        let path = document
            .path()
            .map(Path::to_path_buf)
            .ok_or_else(|| PersistError::Untitled(id.clone()))?;
        (path, document.content().to_string())
    };

    write(&path, &content)?;
    shared.update(|service| {
        service.mark_saved(id);
    });
    Ok(path)
}

/// 另存为：先改名（服务层只换路径与标题、身份不变），再写盘
pub fn save_as(
    shared: &EditorShared,
    id: &DocumentId,
    path: &Path,
) -> Result<PathBuf, PersistError> {
    let content = {
        let service = shared.service();
        let document = service
            .find(id)
            .ok_or_else(|| PersistError::UnknownDocument(id.clone()))?;
        document.content().to_string()
    };

    write(path, &content)?;
    shared.update(|service| {
        let _ = service.rename(id, path);
        service.mark_saved(id);
    });
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    // 安全模式：不通配导入（不在本文件，但保持一致习惯）
    use std::path::PathBuf;

    use super::*;
    use crate::service::OpenRequest;

    /// 临时目录（每个用例一个，互不干扰；用完尽力清理）
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rds_editor_persist_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    #[test]
    fn write_then_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("query.sql");
        write(&path, "select '中文' as x;").expect("写盘");

        let content = load(&path).expect("读盘");
        assert_eq!(content, "select '中文' as x;");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_reports_path() {
        let dir = temp_dir("missing");
        let path = dir.join("nope.sql");
        let error = load(&path).expect_err("不存在的文件应报错");

        // 错误提示要能指出是哪个文件（先拿文案再借匹配，避免部分移动）
        let message = error.to_string();
        assert!(message.contains("nope.sql"), "{message}");
        match &error {
            PersistError::Io { path: reported, .. } => assert_eq!(reported, &path),
            other => panic!("应为 Io 错误：{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_a_file_loads_content_and_keeps_it_clean() {
        let dir = temp_dir("open");
        let path = dir.join("report.sql");
        write(&path, "select 1").expect("写盘");

        let shared = EditorShared::new();
        let outcome = open_file(&shared, &path, EditorMode::Sql).expect("打开");
        let id = outcome.id().clone();

        let service = shared.service();
        let document = service.find(&id).expect("文档存在");
        assert_eq!(document.content(), "select 1");
        assert!(!document.is_dirty(), "刚打开的文档不脏");
        assert_eq!(document.mode(), EditorMode::Sql);
        drop(service);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_the_same_path_again_activates_without_rereading() {
        let dir = temp_dir("dedup");
        let path = dir.join("report.sql");
        write(&path, "select 1").expect("写盘");

        let shared = EditorShared::new();
        let first = open_file(&shared, &path, EditorMode::Sql).expect("打开");
        let id = first.id().clone();

        // 用户改了内容但没保存
        shared.update(|service| {
            service.set_content(&id, "select 1, 2".to_string());
        });
        // 磁盘同时被外部改过
        write(&path, "select 99").expect("外部改写");

        let again = open_file(&shared, &path, EditorMode::Sql).expect("再打开");
        assert!(again.is_activated(), "同路径应只激活");
        assert_eq!(again.id(), &id);
        assert_eq!(
            shared.service().find(&id).expect("文档").content(),
            "select 1, 2",
            "不得用磁盘内容冲掉未保存的编辑"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_writes_content_and_clears_dirty() {
        let dir = temp_dir("save");
        let path = dir.join("report.sql");
        write(&path, "select 1").expect("写盘");

        let shared = EditorShared::new();
        let id = open_file(&shared, &path, EditorMode::Sql)
            .expect("打开")
            .id()
            .clone();
        shared.update(|service| {
            service.set_content(&id, "select 1, 2, 3".to_string());
        });
        assert!(shared.service().find(&id).expect("文档").is_dirty());

        let saved = save_document(&shared, &id).expect("保存");
        assert_eq!(saved, path);
        assert_eq!(load(&path).expect("读盘"), "select 1, 2, 3");
        assert!(!shared.service().find(&id).expect("文档").is_dirty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_an_untitled_document_asks_for_a_path() {
        let shared = EditorShared::new();
        let id = shared
            .open(OpenRequest::untitled("select 1", EditorMode::Sql))
            .id()
            .clone();

        match save_document(&shared, &id) {
            Err(PersistError::Untitled(reported)) => assert_eq!(reported, id),
            other => panic!("未命名文档应要求另存为：{other:?}"),
        }
    }

    #[test]
    fn save_as_renames_then_writes() {
        let dir = temp_dir("save_as");
        let path = dir.join("new_name.sql");

        let shared = EditorShared::new();
        let id = shared
            .open(OpenRequest::untitled("select 7", EditorMode::Sql))
            .id()
            .clone();

        save_as(&shared, &id, &path).expect("另存为");
        assert_eq!(load(&path).expect("读盘"), "select 7");

        let service = shared.service();
        let document = service.find(&id).expect("文档");
        assert_eq!(document.path(), Some(path.as_path()), "路径已更新");
        assert_eq!(document.title(), "new_name.sql", "标题跟随文件名");
        assert!(!document.is_dirty(), "另存为即已保存");
        drop(service);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn external_modification_is_detected_by_mtime() {
        let dir = temp_dir("external");
        let path = dir.join("report.sql");
        write(&path, "select 1").expect("写盘");

        let recorded = modified_time(&path).expect("mtime");
        assert!(
            !external_modified(&path, recorded),
            "刚写过的文件不算被外部改过"
        );

        // 记录一个远早于文件的时刻 → 视为外部修改（模拟“打开后被别的程序改过”）
        let very_old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
        assert!(external_modified(&path, very_old));

        // 文件消失也算外部变化（保存会重建它，值得提示）
        std::fs::remove_file(&path).expect("删除");
        assert!(external_modified(&path, recorded));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
