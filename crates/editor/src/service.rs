//! 文档集合与生命周期（A1：打开 / 关闭 / 激活 / 重命名 / 脏状态）
//!
//! ## 为什么要有这一层
//!
//! “哪些文档开着、哪个是当前、谁脏了”在 V1 里散落在面板与视图（标签条自己算下标、保存逻辑
//! 自己比对），同一状态两处可写。这里收成**唯一权威**：
//!
//! - `DocumentId` 一经生成**永不变**：另存为只换路径与标题，不换身份。V1 用 `filePath` 派生
//!   面板 id，另存为后新旧 key 失配（标签找不到、结果错位）——“用路径当身份”的代价。
//! - 去重只有一条规则：**同一路径重复打开 = 激活已有文档**（不新开标签）。
//! - 脏状态只有一条判据：`content != baseline`（baseline = 上次加载 / 保存的内容）。
//!
//! ## 边界
//!
//! 本模块**不做 I/O**（读写文件属 `persist`）、不碰 GPUI：入参与出参都是值，因此 A1 的验收
//! 能在单测里跑完（见文件尾）。视图层只从 `documents()` / `active()` 取渲染所需的一切。

use std::path::{Path, PathBuf};

use crate::model::{Capabilities, DocumentId, EditorMode, OutputTarget, ReadOnly};

// ═══════════════════════════════════════════════════════════════════════
// 文档
// ═══════════════════════════════════════════════════════════════════════

/// 打开中的文档（编辑器侧最小状态：身份 + 内容 + 模式 + 只读）
#[derive(Debug, Clone)]
pub struct Document {
    id: DocumentId,
    /// 磁盘路径；未命名文档为 `None`
    path: Option<PathBuf>,
    /// 标签标题（未命名 = `未命名-N`）；另存为 / 重命名后更新，**id 不变**
    title: String,
    content: String,
    /// 上次加载或保存时的内容（脏状态判据）
    baseline: String,
    mode: EditorMode,
    read_only: ReadOnly,
}

impl Document {
    fn new(
        id: DocumentId,
        path: Option<PathBuf>,
        title: String,
        content: String,
        mode: EditorMode,
        read_only: ReadOnly,
    ) -> Self {
        Self {
            id,
            path,
            title,
            baseline: content.clone(),
            content,
            mode,
            read_only,
        }
    }

    /// 稳定身份（永不变）
    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    pub fn read_only(&self) -> ReadOnly {
        self.read_only
    }

    /// 该模式的能力集（chrome 与服务的唯一分叉点）
    pub fn capabilities(&self) -> Capabilities {
        Capabilities::for_mode(self.mode)
    }

    pub fn output_target(&self) -> OutputTarget {
        self.capabilities().output
    }

    /// 是否与基线不同（唯一的脏判据）
    pub fn is_dirty(&self) -> bool {
        self.content != self.baseline
    }

    /// 供视图做差异比较的基线长度等统计（不暴露基线本身，避免外部改写）
    pub fn baseline_len(&self) -> usize {
        self.baseline.len()
    }

    fn set_content(&mut self, content: String) {
        self.content = content;
    }

    /// 把当前内容记为已保存（保存成功后调用）
    fn mark_saved(&mut self) {
        self.baseline = self.content.clone();
    }

    fn set_mode(&mut self, mode: EditorMode) {
        self.mode = mode;
    }

    fn set_read_only(&mut self, read_only: ReadOnly) {
        self.read_only = read_only;
    }

    /// 另存为 / 重命名：只换路径与标题
    fn rename_to(&mut self, path: PathBuf) {
        self.title = title_for_path(&path);
        self.path = Some(path);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 打开请求与结果
// ═══════════════════════════════════════════════════════════════════════

/// 打开请求（参数较多且多为可选，收成结构体避免长参数表）
#[derive(Debug, Clone)]
pub struct OpenRequest {
    pub path: Option<PathBuf>,
    pub content: String,
    pub mode: EditorMode,
    pub read_only: ReadOnly,
}

impl OpenRequest {
    /// 打开磁盘文件
    pub fn file(path: impl Into<PathBuf>, content: impl Into<String>, mode: EditorMode) -> Self {
        Self {
            path: Some(path.into()),
            content: content.into(),
            mode,
            read_only: ReadOnly::none(),
        }
    }

    /// 新建未命名文档
    pub fn untitled(content: impl Into<String>, mode: EditorMode) -> Self {
        Self {
            path: None,
            content: content.into(),
            mode,
            read_only: ReadOnly::none(),
        }
    }

    pub fn with_read_only(mut self, read_only: ReadOnly) -> Self {
        self.read_only = read_only;
        self
    }
}

/// 打开结果：新开标签，还是激活已有标签（去重）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    Opened(DocumentId),
    Activated(DocumentId),
}

impl OpenOutcome {
    pub fn id(&self) -> &DocumentId {
        match self {
            Self::Opened(id) | Self::Activated(id) => id,
        }
    }

    /// 是否只是激活了已有文档（调用方据此跳过重新读盘）
    pub fn is_activated(&self) -> bool {
        matches!(self, Self::Activated(_))
    }
}

/// 重命名 / 另存为的失败原因
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameError {
    /// 目标路径已在另一个标签里打开
    Duplicate(PathBuf),
    /// 文档不存在（已关闭）
    NoSuchDocument(DocumentId),
}

// ═══════════════════════════════════════════════════════════════════════
// 服务
// ═══════════════════════════════════════════════════════════════════════

/// 文档集合的唯一权威
#[derive(Debug, Default)]
pub struct EditorService {
    /// 保持**打开顺序**（标签顺序即此顺序，不参与排序语义）
    documents: Vec<Document>,
    active: Option<DocumentId>,
    /// 未命名文档编号（只增不减：避免重名，也避免 id 复用）
    serial: u64,
}

impl EditorService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn documents(&self) -> &[Document] {
        &self.documents
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    pub fn active_id(&self) -> Option<&DocumentId> {
        self.active.as_ref()
    }

    pub fn active(&self) -> Option<&Document> {
        let id = self.active.as_ref()?;
        self.find(id)
    }

    pub fn find(&self, id: &DocumentId) -> Option<&Document> {
        self.documents.iter().find(|doc| doc.id() == id)
    }

    /// 某路径是否已打开（去重判据；不做 I/O）
    pub fn find_by_path(&self, path: &Path) -> Option<&DocumentId> {
        let key = path_key(path);
        self.documents
            .iter()
            .find(|doc| doc.path.as_deref().is_some_and(|p| path_key(p) == key))
            .map(Document::id)
    }

    /// 已脏文档（关闭全部 / 退出确认用）
    pub fn dirty_ids(&self) -> Vec<DocumentId> {
        self.documents
            .iter()
            .filter(|doc| doc.is_dirty())
            .map(|doc| doc.id().clone())
            .collect()
    }

    /// 打开文档：同路径已打开则**激活**，否则新开并激活
    pub fn open(&mut self, request: OpenRequest) -> OpenOutcome {
        if let Some(path) = request.path.as_deref()
            && let Some(existing) = self.find_by_path(path)
        {
            let id = existing.clone();
            self.active = Some(id.clone());
            return OpenOutcome::Activated(id);
        }

        self.serial += 1;
        let id = DocumentId::new(format!("doc-{}", self.serial));
        let title = match request.path.as_deref() {
            Some(path) => title_for_path(path),
            None => format!("未命名-{}", self.serial),
        };

        self.documents.push(Document::new(
            id.clone(),
            request.path,
            title,
            request.content,
            request.mode,
            request.read_only,
        ));
        self.active = Some(id.clone());

        OpenOutcome::Opened(id)
    }

    /// 关闭文档；返回被关闭的文档（调用方可能需要按脏状态先落草稿）
    ///
    /// 关闭的是当前文档时，激活**同位置的邻居**（没有则最后一个）——与主流编辑器的
    /// “关掉当前标签后落到旁边”一致，避免焦点跳回第一个。
    pub fn close(&mut self, id: &DocumentId) -> Option<Document> {
        let index = self.documents.iter().position(|doc| doc.id() == id)?;
        let removed = self.documents.remove(index);

        if self.active.as_ref() == Some(id) {
            self.active = self
                .documents
                .get(index)
                .or_else(|| self.documents.last())
                .map(|doc| doc.id().clone());
        }

        Some(removed)
    }

    /// 激活某文档（不存在则不改动）
    pub fn activate(&mut self, id: &DocumentId) -> bool {
        if self.find(id).is_some() {
            self.active = Some(id.clone());
            true
        } else {
            false
        }
    }

    /// 重命名 / 另存为：**身份不变**，只换路径与标题
    pub fn rename(
        &mut self,
        id: &DocumentId,
        new_path: impl Into<PathBuf>,
    ) -> Result<(), RenameError> {
        let new_path = new_path.into();
        let key = path_key(&new_path);

        let clash = self
            .documents
            .iter()
            .any(|doc| doc.id() != id && doc.path.as_deref().is_some_and(|p| path_key(p) == key));
        if clash {
            return Err(RenameError::Duplicate(new_path));
        }

        let doc = self
            .documents
            .iter_mut()
            .find(|doc| doc.id() == id)
            .ok_or_else(|| RenameError::NoSuchDocument(id.clone()))?;
        doc.rename_to(new_path);
        Ok(())
    }

    /// 输入内容（编辑器只读时拒绝写入，返回是否被接受）
    pub fn set_content(&mut self, id: &DocumentId, content: String) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) if doc.read_only.can_edit() => {
                doc.set_content(content);
                true
            }
            _ => false,
        }
    }

    /// 保存成功：把当前内容记为基线（清脏）
    pub fn mark_saved(&mut self, id: &DocumentId) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                doc.mark_saved();
                true
            }
            None => false,
        }
    }

    /// 切换模式（合法性由 `mode` 模块的判定规则决定，调用方先判定再切）
    pub fn set_mode(&mut self, id: &DocumentId, mode: EditorMode) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                doc.set_mode(mode);
                true
            }
            None => false,
        }
    }

    /// 更新只读两维度（连接只读随连接策略变化）
    pub fn set_read_only(&mut self, id: &DocumentId, read_only: ReadOnly) -> bool {
        match self.documents.iter_mut().find(|doc| doc.id() == id) {
            Some(doc) => {
                doc.set_read_only(read_only);
                true
            }
            None => false,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 纯函数辅助
// ═══════════════════════════════════════════════════════════════════════

/// 路径去重键
///
/// Windows 的文件系统不区分大小写（`D:\a.sql` 与 `d:\A.SQL` 是同一个文件），Linux 区分。
/// 因此按平台归一：Windows 转小写，其它平台原样；分隔符统一为 `/` 便于比较。
/// **不做 canonicalize**（那是 I/O，且文件可能尚未存在）：调用方应传规范化后的绝对路径。
fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// 标签标题：取文件名；取不到则退回完整路径
fn title_for_path(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

// ═══════════════════════════════════════════════════════════════════════
// 测试（A1 验收 + A8 的纯逻辑部分）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn opened_file(path: &str, content: &str) -> (EditorService, DocumentId) {
        let mut service = EditorService::new();
        let outcome = service.open(OpenRequest::file(path, content, EditorMode::Sql));
        let id = outcome.id().clone();
        assert!(!outcome.is_activated());
        (service, id)
    }

    #[test]
    fn open_activates_and_starts_clean() {
        let (service, id) = opened_file(r"D:\sql\report.sql", "select 1");
        assert_eq!(service.len(), 1);
        assert_eq!(service.active_id(), Some(&id));
        let doc = service.active().expect("active document");
        assert_eq!(doc.title(), "report.sql");
        assert!(!doc.is_dirty(), "刚打开不应是脏的");
        assert_eq!(doc.mode(), EditorMode::Sql);
        assert_eq!(doc.output_target(), OutputTarget::DockPanel);
    }

    #[test]
    fn same_path_opens_once_and_just_activates() {
        let (mut service, first) = opened_file(r"D:\sql\report.sql", "select 1");
        service.open(OpenRequest::file(
            r"D:\sql\other.sql",
            "select 2",
            EditorMode::Sql,
        ));
        assert_eq!(service.len(), 2);

        let again = service.open(OpenRequest::file(
            r"D:\sql\report.sql",
            "被忽略的内容",
            EditorMode::Sql,
        ));
        assert!(again.is_activated(), "同路径重复打开只应激活");
        assert_eq!(again.id(), &first);
        assert_eq!(service.len(), 2, "不得新开标签");
        assert_eq!(service.active_id(), Some(&first));
        // 已有文档内容不被后来者覆盖
        assert_eq!(service.find(&first).expect("doc").content(), "select 1");
    }

    #[test]
    fn path_dedup_ignores_case_and_separator_style() {
        let (mut service, first) = opened_file(r"D:\sql\report.sql", "select 1");
        let same = service.open(OpenRequest::file("d:/SQL/REPORT.SQL", "x", EditorMode::Sql));
        if cfg!(windows) {
            assert!(
                same.is_activated(),
                "Windows 下大小写/分隔符不同仍是同一文件"
            );
            assert_eq!(same.id(), &first);
            assert_eq!(service.len(), 1);
        } else {
            // 非 Windows：区分大小写，属另一个文件（记录平台差异，不假装一致）
            assert!(!same.is_activated());
            assert_eq!(service.len(), 2);
        }
    }

    #[test]
    fn untitled_documents_get_distinct_identity_and_title() {
        let mut service = EditorService::new();
        let a = service
            .open(OpenRequest::untitled("", EditorMode::Text))
            .id()
            .clone();
        let b = service
            .open(OpenRequest::untitled("", EditorMode::Text))
            .id()
            .clone();

        assert_ne!(a, b);
        assert_eq!(service.find(&a).expect("a").title(), "未命名-1");
        assert_eq!(service.find(&b).expect("b").title(), "未命名-2");
        assert_eq!(service.len(), 2);
        assert!(service.find(&a).expect("a").path().is_none());
    }

    #[test]
    fn closing_active_activates_neighbour() {
        let mut service = EditorService::new();
        let a = service
            .open(OpenRequest::file(r"D:\a.sql", "a", EditorMode::Sql))
            .id()
            .clone();
        let b = service
            .open(OpenRequest::file(r"D:\b.sql", "b", EditorMode::Sql))
            .id()
            .clone();
        let c = service
            .open(OpenRequest::file(r"D:\c.sql", "c", EditorMode::Sql))
            .id()
            .clone();
        assert_eq!(service.active_id(), Some(&c));

        // 关掉中间的 b（不是当前）→ 当前不变
        service.close(&b).expect("closed");
        assert_eq!(service.active_id(), Some(&c));

        // 关掉当前的 c → 落到同位置的邻居（没有则最后一个）
        service.close(&c).expect("closed");
        assert_eq!(service.active_id(), Some(&a));

        // 关掉最后一个 → 无文档时 active 清空
        service.close(&a).expect("closed");
        assert!(service.is_empty());
        assert!(service.active_id().is_none());
        assert!(service.active().is_none());
    }

    #[test]
    fn closed_document_disappears_from_lookups() {
        let (mut service, id) = opened_file(r"D:\a.sql", "a");
        service.close(&id).expect("closed");
        assert!(service.find(&id).is_none());
        assert!(service.find_by_path(Path::new(r"D:\a.sql")).is_none());
        assert!(!service.activate(&id), "已关闭的文档不能被激活");
        assert!(!service.set_content(&id, "x".to_string()));
    }

    #[test]
    fn rename_keeps_identity_and_updates_path_and_title() {
        let (mut service, id) = opened_file(r"D:\sql\report.sql", "select 1");
        service
            .rename(&id, r"D:\sql\report_v2.sql")
            .expect("rename");

        let doc = service.find(&id).expect("same id still there");
        assert_eq!(doc.id(), &id, "身份不变（V1 的教训：不能用路径当身份）");
        assert_eq!(doc.title(), "report_v2.sql");
        assert_eq!(doc.path(), Some(Path::new(r"D:\sql\report_v2.sql")));
        // 旧路径不再命中，新路径命中
        assert!(
            service
                .find_by_path(Path::new(r"D:\sql\report.sql"))
                .is_none()
        );
        assert_eq!(
            service.find_by_path(Path::new(r"D:\sql\report_v2.sql")),
            Some(&id)
        );
    }

    #[test]
    fn rename_to_an_open_path_is_rejected() {
        let (mut service, a) = opened_file(r"D:\a.sql", "a");
        let b = service
            .open(OpenRequest::file(r"D:\b.sql", "b", EditorMode::Sql))
            .id()
            .clone();

        let err = service.rename(&a, r"D:\b.sql").expect_err("应拒绝");
        assert_eq!(err, RenameError::Duplicate(PathBuf::from(r"D:\b.sql")));
        // 冲突不产生任何半成品
        assert_eq!(
            service.find(&a).expect("a").path(),
            Some(Path::new(r"D:\a.sql"))
        );
        assert_eq!(
            service.find(&b).expect("b").path(),
            Some(Path::new(r"D:\b.sql"))
        );
    }

    #[test]
    fn editing_marks_dirty_and_reverting_clears_it() {
        let (mut service, id) = opened_file(r"D:\a.sql", "select 1");
        assert!(!service.find(&id).expect("doc").is_dirty());

        assert!(service.set_content(&id, "select 1, 2".to_string()));
        assert!(service.find(&id).expect("doc").is_dirty(), "改过即脏");
        assert_eq!(service.dirty_ids(), vec![id.clone()]);

        // 撤销回原值 → 干净（脏判据只看内容，不看编辑历史）
        assert!(service.set_content(&id, "select 1".to_string()));
        assert!(!service.find(&id).expect("doc").is_dirty());
        assert!(service.dirty_ids().is_empty());
    }

    #[test]
    fn mark_saved_moves_the_baseline() {
        let (mut service, id) = opened_file(r"D:\a.sql", "select 1");
        service.set_content(&id, "select 1, 2".to_string());
        assert!(service.find(&id).expect("doc").is_dirty());

        assert!(service.mark_saved(&id));
        let doc = service.find(&id).expect("doc");
        assert!(!doc.is_dirty(), "保存后应干净");
        assert_eq!(doc.baseline_len(), "select 1, 2".len());

        // 保存之后再改 → 仍能判脏（基线已前移）
        service.set_content(&id, "select 1, 2, 3".to_string());
        assert!(service.find(&id).expect("doc").is_dirty());
    }

    #[test]
    fn read_only_editor_rejects_content_changes() {
        let mut service = EditorService::new();
        let id = service
            .open(
                OpenRequest::file(r"D:\a.sql", "select 1", EditorMode::Sql)
                    .with_read_only(ReadOnly::editor_only()),
            )
            .id()
            .clone();

        assert!(
            !service.set_content(&id, "select 2".to_string()),
            "只读不可写入"
        );
        assert_eq!(service.find(&id).expect("doc").content(), "select 1");
        assert!(!service.find(&id).expect("doc").is_dirty());
        assert!(!service.find(&id).expect("doc").read_only().can_edit());
    }

    #[test]
    fn switching_mode_updates_capabilities() {
        // 模式是**文档属性**（架构 D2）：切换后能力表派生的 chrome / 输出目标随之变化
        let (mut service, id) = opened_file(r"D:\a.sql", "select 1");
        assert_eq!(
            service.find(&id).expect("doc").output_target(),
            OutputTarget::DockPanel
        );

        assert!(service.set_mode(&id, EditorMode::Text));
        let text = service.find(&id).expect("doc");
        assert_eq!(text.mode(), EditorMode::Text);
        assert_eq!(text.output_target(), OutputTarget::None);
        assert!(
            !text.capabilities().talks_to_database(),
            "文本模式禁止一切数据库通信"
        );

        assert!(service.set_mode(&id, EditorMode::Analysis));
        assert_eq!(
            service.find(&id).expect("doc").output_target(),
            OutputTarget::Inline
        );
    }

    #[test]
    fn titles_fall_back_to_full_path_when_no_file_name() {
        assert_eq!(
            title_for_path(Path::new(r"D:\sql\report.sql")),
            "report.sql"
        );
        // 取不到文件名时不能返回空标题
        assert_eq!(title_for_path(Path::new(r"D:\")), r"D:\");
    }
}
