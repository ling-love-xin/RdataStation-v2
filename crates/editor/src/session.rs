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

use serde::{Deserialize, Serialize};

use crate::channel::ExecChannel;
use crate::model::EditorMode;

/// 分析会话（T2d-2）：一份笔记在「哪个连接 + 哪个环境」上跑
///
/// 与 [`SavedSession`]（文档持久化：光标 / 模式 / 内容）是**两回事**：
/// 分析会话回答的是「笔记的单元往哪发」（绑定连接 / 执行环境），
/// 会话切换 → 全部输出标 stale（见 [`crate::cell::Cell::mark_stale`]）。
///
/// 会话是**笔记级**的：落 `.rdsnote` 时跟随文件（重开笔记原样恢复绑定），
/// 但单元输出不跟随（那是会话级内存状态）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// 会话 id（`default` = 默认会话；未来可扩展多会话列表）
    pub id: String,
    /// 绑定连接（`None` = 跟随当前活动连接，与 SQL 模式的 B1 同口径）
    pub default_connection: Option<String>,
    /// 执行环境（通用模式的解释器 id；`None` = 未绑定）
    pub env: Option<String>,
    /// 【#20】上次用过的内核 id（分析模式「默认执行源」的记忆：执行成功后回写，
    /// 菜单「默认（上次用过的内核）」指向它；`None` = 还没用过 → 注册表第一个可用）
    #[serde(default)]
    pub default_kernel: Option<String>,
}

impl Session {
    /// 默认会话（id = `default`，未绑定连接与环境）
    pub fn default_session() -> Self {
        Self {
            id: "default".to_string(),
            default_connection: None,
            env: None,
            default_kernel: None,
        }
    }

    /// 新建一个带绑定的会话
    pub fn new(id: impl Into<String>, default_connection: Option<String>, env: Option<String>) -> Self {
        Self {
            id: id.into(),
            default_connection,
            env,
            default_kernel: None,
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::default_session()
    }
}

/// 一份可恢复的编辑器会话
///
/// 注意：含 `f32`（结果区高度）→ 只派 `PartialEq`，不派 `Eq`。
#[derive(Debug, Clone, PartialEq)]
pub struct SavedSession {
    /// 会话标识：有路径的文档用路径键（重启后仍是同一个文件），未命名文档不保存
    pub id: String,
    /// 文档标题（恢复时的兜底展示；真实标题由路径推断）
    pub path: Option<String>,
    pub mode: EditorMode,
    /// 【B13】执行通道（文档级属性，与模式同类：重启后不能丢）
    pub channel: ExecChannel,
    /// 【B1】绑定的连接 id（`None` = 跟随当前连接）
    ///
    /// 与模式 / 通道同类：它是**这份文档发到哪**的答案，重启后不能默默变成“跟随当前连接”——
    /// 那会让用户以为还在往原来那个库写。恢复时**要校验它还在不在列表里**（不在就回退并说原因，
    /// 见 `view/host.rs::restore_connection`）。
    pub connection: Option<String>,
    pub content: String,
    /// 光标（字节偏移）
    pub cursor: usize,
    /// 选区（字节区间；无选区为 `None`）
    pub selection: Option<(usize, usize)>,
    /// 【T2c-6】工具栏自定义布局（[`crate::toolbar::ToolbarLayout::encode`] 的编码串；
    /// `None` = 旧会话没存过 → 恢复用默认布局）
    pub toolbar: Option<String>,
    /// 【T2c-1 余项】绑定的 schema（`None` = 未选 → 连接默认 schema；空串与未绑同一语义）
    pub schema: Option<String>,
    /// 【T2c-4 余项】文档缩进宽度（默认 2；`None` = 旧会话没存过 → 不覆盖当前）
    pub tab_size: Option<usize>,
    /// 【T2c-4 余项】缩进是否制表符（默认 false；`None` = 旧会话没存过 → 不覆盖当前）
    pub hard_tabs: Option<bool>,
    /// 【遗留 #5】结果区分栏高度（rem；`None` = 旧会话没存过 → 不覆盖当前）
    pub result_height: Option<f32>,
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
///
/// 【T2c-6】布局 / schema / 缩进被改过也值得存：哪怕文档是空的，
/// 这些**文档属性**重启后不该丢（工具栏布局是用户花时间排的）。
/// `None`（旧会话没存过）不等于“改过”：只认**显式非默认**的值。
pub fn worth_saving(session: &SavedSession) -> bool {
    if !session.content.trim().is_empty() || session.cursor > 0 || session.selection.is_some() {
        return true;
    }
    // 工具栏布局 ≠ 默认布局（编码串对比；`None` = 旧会话没这字段 → 不算“改过”)
    if let Some(code) = session.toolbar.as_deref() {
        if code != crate::toolbar::ToolbarLayout::default_layout().encode() {
            return true;
        }
    }
    // schema 绑了 / 缩进显式非默认（宽 2、空格）
    session.schema.is_some()
        || session.tab_size.is_some_and(|size| size != 2)
        || session.hard_tabs == Some(true)
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
            connection: None,
            content: content.to_string(),
            cursor: 7,
            selection: Some((1, 3)),
            toolbar: None,
            schema: None,
            tab_size: None,
            hard_tabs: None,
            result_height: None,
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

    /// 【B1】连接绑定也是文档属性：与模式 / 通道一样得跟着会话回来
    #[test]
    fn the_connection_binding_rides_along_with_the_session() {
        let store = MemorySessionStore::new();
        let mut with_connection = session("d:/sql/b.sql", "select 1;", EditorMode::Sql);
        with_connection.connection = Some("P_orders".to_string());
        store.save(&with_connection).expect("save");

        let loaded = store.load("d:/sql/b.sql").expect("load").expect("有");
        assert_eq!(loaded.connection.as_deref(), Some("P_orders"));

        // 未绑定（跟随当前连接）也要能原样存回来：`None` 不等同于“忘记存了”
        store
            .save(&session("d:/sql/c.sql", "select 2;", EditorMode::Sql))
            .expect("save");
        let unbound = store.load("d:/sql/c.sql").expect("load").expect("有");
        assert_eq!(unbound.connection, None);
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

    /// 【T2c-6】工具栏布局 / schema / 缩进都是文档属性：随会话原样往返
    #[test]
    fn the_toolbar_schema_and_indent_ride_along_with_the_session() {
        let store = MemorySessionStore::new();
        let mut saved = session("d:/sql/a.sql", "select 1;", EditorMode::Sql);
        saved.toolbar = Some(
            "ExecRun,ExecAll,~ExecCurrent,~ExecSelection,~ExecBatch,|,Format,|,More;ExecRun"
                .to_string(),
        );
        saved.schema = Some("analytics".to_string());
        saved.tab_size = Some(4);
        saved.hard_tabs = Some(true);
        store.save(&saved).expect("save");

        let loaded = store.load("d:/sql/a.sql").expect("load").expect("有");
        assert_eq!(
            loaded.toolbar.as_deref(),
            Some("ExecRun,ExecAll,~ExecCurrent,~ExecSelection,~ExecBatch,|,Format,|,More;ExecRun")
        );
        assert_eq!(loaded.schema.as_deref(), Some("analytics"));
        assert_eq!(loaded.tab_size, Some(4));
        assert_eq!(loaded.hard_tabs, Some(true));

        // 旧会话没有这些字段：`None` 原样往返（恢复侧据此不覆盖默认）
        store
            .save(&session("d:/sql/c.sql", "select 2;", EditorMode::Sql))
            .expect("save");
        let plain = store.load("d:/sql/c.sql").expect("load").expect("有");
        assert_eq!(plain.toolbar, None);
        assert_eq!(plain.schema, None);
        assert_eq!(plain.tab_size, None);
        assert_eq!(plain.hard_tabs, None);
    }

    #[test]
    fn latest_follows_the_last_save() {
        let store = MemorySessionStore::new();
        store.save(&session("a.sql", "select 1;", EditorMode::Sql)).expect("save");
        store.save(&session("b.txt", "hello", EditorMode::General)).expect("save");

        let latest = store.load_latest().expect("load").expect("有");
        assert_eq!(latest.id, "b.txt");
        assert_eq!(latest.mode, EditorMode::General);
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

    /// 【T2d-2】分析会话：默认会话 + 带绑定会话，序列化往返（落 `.rdsnote` 用）
    #[test]
    fn analysis_session_round_trips_through_json() {
        use super::Session;

        let default = Session::default_session();
        assert_eq!(default.id, "default");
        assert_eq!(default.default_connection, None);
        assert_eq!(default.env, None);

        let bound = Session::new("s1", Some("P_orders".to_string()), None);
        assert_eq!(bound.id, "s1");
        assert_eq!(bound.default_connection.as_deref(), Some("P_orders"));

        // 序列化往返（`.rdsnote` 会话字段走同一形状）
        let json = serde_json::to_string(&bound).expect("serialize");
        let back: Session = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, bound);

        // `Default` 与 `default_session` 同义（新建笔记的初值）
        assert_eq!(Session::default(), Session::default_session());
    }

    /// 【T2c-6】文档为空但布局 / schema / 缩进被改过 → 值得存（重启不丢文档属性）
    #[test]
    fn customized_document_properties_are_worth_saving_even_when_blank() {
        let mut blank = session("a.sql", "   \n", EditorMode::Sql);
        blank.cursor = 0;
        blank.selection = None;

        // 默认布局（编码串与默认一致）不算改过
        blank.toolbar = Some(crate::toolbar::ToolbarLayout::default_layout().encode());
        assert!(!worth_saving(&blank));

        // 布局被改过 → 值得存
        let mut customized = blank.clone();
        customized.toolbar =
            Some("ExecRun,ExecAll,~ExecCurrent,~ExecSelection,~ExecBatch,|,Format,|,More;ExecRun"
                .to_string());
        assert!(worth_saving(&customized), "排过的工具栏重启不该丢");

        // schema 绑了 / 缩进改了 → 值得存
        let mut with_schema = blank.clone();
        with_schema.schema = Some("analytics".to_string());
        assert!(worth_saving(&with_schema));
        let mut wide_indent = blank.clone();
        wide_indent.tab_size = Some(4);
        assert!(worth_saving(&wide_indent));
        let mut hard = blank.clone();
        hard.hard_tabs = Some(true);
        assert!(worth_saving(&hard));
    }
}
