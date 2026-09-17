//! 编辑器会话（A12）：光标 / 选区 / 模式 / 内容 的保存与恢复
//!
//! 与「工作区上下文持久化」同名的一件事，但**落点分开**：
//! - 本节只定义**会话长什么样**（[`SavedSession`]）与**往哪存**（[`SessionStore`] 端口）；
//! - 真正的库表是 `engine` 的 `workbench_context_store`（`editor_contexts`，A12 给它补了
//!   `mode` 列），由**宿主**注入一个实现（workbench 的 `services/editor_session.rs`）。
//!
//! 为什么用端口而不是直接调库：和 `QueryRunner` 同一个理由——editor 不该知道
//! "全局库落在哪、连接池怎么拿"。测试里注入内存实现，就能在没有数据库的情况下把
//! "存了什么 / 恢复成什么"断言清楚。
//!
//! ## 恢复范围（1a）
//!
//! 只恢复**最近更新的那一份**（`load_latest`）。多文档恢复要等 layout 表与"上次打开的面板集合"
//! 一起做，属 1b；1a 先保证"关掉 app 再打开，上次那份 SQL 还在，光标和模式也在"。

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::channel::ExecChannel;
use crate::model::EditorMode;

/// 一份可恢复的编辑器会话
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSession {
    /// 会话标识：有路径的文档用路径键（重启后仍是同一个文件），未命名文档不保存
    pub id: String,
    /// 文档标题（恢复时的兜底展示；真实标题由路径推断）
    pub path: Option<String>,
    pub mode: EditorMode,
    /// 【B13】执行通道（文档级属性，与模式同类：重启后不能丢）
    pub channel: ExecChannel,
    pub content: String,
    /// 光标（字节偏移）
    pub cursor: usize,
    /// 选区（字节区间；无选区为 `None`）
    pub selection: Option<(usize, usize)>,
}

/// 会话存储端口（宿主注入）
///
/// 不要求 `Send + Sync`：会话读写发生在 UI 线程的事件路径上（与执行通道不同，
/// 后者要在工作线程里跑）。宿主实现持有的是连接池（`Arc`），本身也是 Send + Sync。
pub trait SessionStore: 'static {
    fn save(&self, session: &SavedSession) -> Result<(), String>;
    /// 取最近更新的那一份（没有则 `None`）
    fn load_latest(&self) -> Result<Option<SavedSession>, String>;
    /// 按会话标识取（测试与将来的"恢复指定文档"用）
    fn load(&self, id: &str) -> Result<Option<SavedSession>, String>;
}

/// 内存实现（测试用；也用于"宿主未接存储"时让功能可用而不是失败）
#[derive(Default)]
pub struct MemorySessionStore {
    sessions: RefCell<HashMap<String, SavedSession>>,
    /// 最近写入的会话 id（`load_latest` 用；真实现按 `updated_at_ms` 排）
    latest: RefCell<Option<String>>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 包成端口对象（方便注入到 `EditorShared`）
    pub fn shared() -> Rc<Self> {
        Rc::new(Self::new())
    }

    /// 当前存了几份（测试断言用）
    pub fn len(&self) -> usize {
        self.sessions.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.borrow().is_empty()
    }
}

impl SessionStore for MemorySessionStore {
    fn save(&self, session: &SavedSession) -> Result<(), String> {
        self.sessions
            .borrow_mut()
            .insert(session.id.clone(), session.clone());
        *self.latest.borrow_mut() = Some(session.id.clone());
        Ok(())
    }

    fn load_latest(&self) -> Result<Option<SavedSession>, String> {
        let latest = self.latest.borrow().clone();
        match latest {
            Some(id) => Ok(self.sessions.borrow().get(&id).cloned()),
            None => Ok(None),
        }
    }

    fn load(&self, id: &str) -> Result<Option<SavedSession>, String> {
        Ok(self.sessions.borrow().get(id).cloned())
    }
}

/// 会话标识：由文档生成
///
/// - 有路径 → 用路径键（大小写与分隔符归一，见 `service::path_key`），重启后仍是同一把钥匙
/// - 未命名 → `None`（未命名文档重启后没有对应关系，不该恢复成"某份没名字的文档"）
pub fn session_id_for_path(path: &std::path::Path) -> String {
    crate::service::path_key(path)
}

/// 从文档 id + 路径构造会话标识（未命名返回 `None`）
pub fn session_id(document: &crate::service::Document) -> Option<String> {
    document
        .path()
        .map(|path| session_id_for_path(std::path::Path::new(path)))
}

/// 会话内容是否值得恢复（空文档 + 无光标的不写库：避免每次开 app 都留一条空记录）
pub fn worth_saving(session: &SavedSession) -> bool {
    !session.content.trim().is_empty() || session.cursor > 0 || session.selection.is_some()
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{MemorySessionStore, SavedSession, SessionStore, session_id_for_path, worth_saving};
    use crate::channel::ExecChannel;
    use crate::model::EditorMode;

    fn session(id: &str, content: &str, mode: EditorMode) -> SavedSession {
        SavedSession {
            id: id.to_string(),
            path: Some(id.to_string()),
            mode,
            channel: ExecChannel::default(),
            content: content.to_string(),
            cursor: 7,
            selection: Some((1, 3)),
        }
    }

    #[test]
    fn round_trips_through_the_store() {
        let store = MemorySessionStore::new();
        store
            .save(&session("d:/sql/a.sql", "select 1;", EditorMode::Sql))
            .expect("save");

        let loaded = store.load("d:/sql/a.sql").expect("load").expect("有");
        assert_eq!(loaded.content, "select 1;");
        assert_eq!(loaded.mode, EditorMode::Sql);
        assert_eq!(loaded.cursor, 7);
        assert_eq!(loaded.selection, Some((1, 3)));
        assert_eq!(store.len(), 1);
    }

    /// 【B13】通道是文档属性，与模式一样得跟着会话回来（否则重启都回源库档）
    #[test]
    fn the_channel_rides_along_with_the_session() {
        let store = MemorySessionStore::new();
        let mut saved = session("d:/sql/a.sql", "select 1;", EditorMode::Sql);
        saved.channel = ExecChannel::Accelerated;
        store.save(&saved).expect("save");

        let loaded = store.load("d:/sql/a.sql").expect("load").expect("有");
        assert_eq!(loaded.channel, ExecChannel::Accelerated);
    }

    #[test]
    fn latest_follows_the_last_save() {
        let store = MemorySessionStore::new();
        store.save(&session("a.sql", "select 1;", EditorMode::Sql)).expect("save");
        store.save(&session("b.txt", "hello", EditorMode::Text)).expect("save");

        let latest = store.load_latest().expect("load").expect("有");
        assert_eq!(latest.id, "b.txt");
        assert_eq!(latest.mode, EditorMode::Text);
    }

    #[test]
    fn an_empty_store_has_nothing_to_restore() {
        let store = MemorySessionStore::new();
        assert!(store.is_empty());
        assert_eq!(store.load_latest().expect("load"), None);
        assert_eq!(store.load("nope.sql").expect("load"), None);
    }

    #[test]
    fn session_id_normalizes_case_and_separators() {
        // 同一个文件的不同写法要落同一条记录（否则重启会多出一份）
        let a = session_id_for_path(std::path::Path::new(r"D:\SQL\Report.SQL"));
        let b = session_id_for_path(std::path::Path::new("d:/sql/report.sql"));
        assert_eq!(a, b, "路径键要归一：{a} != {b}");
    }

    #[test]
    fn empty_documents_are_not_worth_saving() {
        let mut blank = session("a.sql", "   \n", EditorMode::Sql);
        blank.cursor = 0;
        blank.selection = None;
        assert!(!worth_saving(&blank));

        let mut typed = session("a.sql", "select 1", EditorMode::Sql);
        typed.cursor = 0;
        typed.selection = None;
        assert!(worth_saving(&typed), "有内容就该存");
    }
}
