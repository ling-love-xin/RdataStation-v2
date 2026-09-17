//! 编辑器会话存储的工作台实现（A12）
//!
//! 编辑器（`crates/editor`）只定义端口（`session::SessionStore`）；这里回答"落到哪"：
//! 全局库的 `editor_contexts` 表（`engine::persistence::WorkbenchContextStore`，
//! 本轮给它补了 `mode` 列）。
//!
//! ## 同步 API 的调用位置（重要）
//!
//! 存储层是同步 API（`acquire_sync` 自建短命 runtime 驱动）。它**不能在 tokio 运行时
//! 上下文里调用**（tokio 拒绝嵌套 `block_on`，那里会得到一条可读错误）。
//! 生产调用点是 GPUI 主线程——没有 runtime，正是可用场景。
//!
//! ## 已知限制（1b 补）
//!
//! 会话标识就是路径键（小写、`/` 分隔）。Windows 上小写路径仍能打开同一文件，
//! 大小写敏感的文件系统上可能认不回原路径——1b 给 `editor_contexts` 加 `path` 列后解决。

use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use editor::model::EditorMode;
use editor::session::{SavedSession, SessionStore};
use editor::shared::EditorShared;
use engine::persistence::workbench_context_store::EditorContext;

/// 把会话存储接到编辑器上（**启动装配调用一次**；拿不到全局库时明确说一句）
pub fn attach(shared: &EditorShared) {
    match WorkbenchSessionStore::open() {
        Some(store) => shared.attach_session_store(Rc::new(store)),
        None => eprintln!(
            "[editor] 会话存储未接入（全局库未就绪）：光标与模式不会跨重启保留"
        ),
    }
}

/// 全局库上的会话存储
///
/// `open()` 走全局单例（生产）；`over()` 接一份给定的 store（测试与嵌入场景，避开进程级单例）。
pub struct WorkbenchSessionStore {
    store: engine::persistence::WorkbenchContextStore,
}

impl WorkbenchSessionStore {
    /// 从全局库管理器取（生产路径）
    pub fn open() -> Option<Self> {
        let manager = engine::migration::get_global_db_manager()?;
        let store = manager.get_workbench_context_store().ok()?;
        Some(Self { store })
    }

    /// 接一份给定的 store（测试用：临时库，不碰全局单例）
    pub fn over(store: engine::persistence::WorkbenchContextStore) -> Self {
        Self { store }
    }

    fn to_context(session: &SavedSession) -> EditorContext {
        EditorContext {
            id: session.id.clone(),
            // 1a 还没有连接绑定（架构 §12 #26）：先留空，1b 接连接绑定后写真实 conn_id
            connection_id: String::new(),
            mode: session.mode.as_key().to_string(),
            // 【B13】通道是文档属性（源库 / 加速 / 联邦）：存短码，重启后认回来
            channel: session.channel.code().to_string(),
            content: session.content.clone(),
            cursor_position: session.cursor,
            selection_start: session.selection.map(|(start, _)| start),
            selection_end: session.selection.map(|(_, end)| end),
            updated_at_ms: now_ms(),
        }
    }

    fn to_session(context: EditorContext) -> SavedSession {
        SavedSession {
            id: context.id.clone(),
            // 会话标识就是路径键：当作路径用（Windows 上大小写不敏感，能打开同一文件）
            path: Some(context.id),
            mode: EditorMode::from_key(&context.mode),
            // 认不出的通道码回源库档（`from_code` 的零值语义）：不假装记住了别的
            channel: editor::channel::ExecChannel::from_code(&context.channel),
            content: context.content,
            cursor: context.cursor_position,
            selection: match (context.selection_start, context.selection_end) {
                (Some(start), Some(end)) => Some((start, end)),
                _ => None,
            },
        }
    }
}

impl SessionStore for WorkbenchSessionStore {
    fn save(&self, session: &SavedSession) -> Result<(), String> {
        self.store
            .save_editor_context(&Self::to_context(session))
            .map_err(|error| error.to_string())
    }

    fn load_latest(&self) -> Result<Option<SavedSession>, String> {
        Ok(self
            .store
            .load_latest_editor_context()
            .map_err(|error| error.to_string())?
            .map(Self::to_session))
    }

    fn load(&self, id: &str) -> Result<Option<SavedSession>, String> {
        Ok(self
            .store
            .load_editor_context(id)
            .map_err(|error| error.to_string())?
            .map(Self::to_session))
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::WorkbenchSessionStore;
    use editor::channel::ExecChannel;
    use editor::model::EditorMode;
    use editor::session::SavedSession;

    fn session() -> SavedSession {
        SavedSession {
            id: "d:/sql/a.sql".to_string(),
            path: Some("D:/sql/A.sql".to_string()),
            mode: EditorMode::Analysis,
            channel: ExecChannel::Source,
            content: "select 1;".to_string(),
            cursor: 7,
            selection: Some((1, 3)),
        }
    }

    #[test]
    fn a_session_round_trips_through_the_row_shape() {
        let row = WorkbenchSessionStore::to_context(&session());
        assert_eq!(row.mode, "analysis", "模式按键存量");
        assert_eq!(row.cursor_position, 7);
        assert_eq!(row.selection_start, Some(1));

        let back = WorkbenchSessionStore::to_session(row);
        assert_eq!(back.mode, EditorMode::Analysis);
        assert_eq!(back.content, "select 1;");
        assert_eq!(back.cursor, 7);
        assert_eq!(back.selection, Some((1, 3)));
    }

    /// 【B13】通道以短码存库、按短码认回来；认不出的码回源库档（不假装记住了别的）
    #[test]
    fn the_channel_travels_as_a_code() {
        let mut accelerated = session();
        accelerated.channel = ExecChannel::Accelerated;
        let row = WorkbenchSessionStore::to_context(&accelerated);
        assert_eq!(row.channel, "accelerated");
        assert_eq!(
            WorkbenchSessionStore::to_session(row).channel,
            ExecChannel::Accelerated
        );

        let mut unknown = WorkbenchSessionStore::to_context(&session());
        unknown.channel = "notebook-v9".to_string();
        assert_eq!(
            WorkbenchSessionStore::to_session(unknown).channel,
            ExecChannel::Source,
            "认不出的通道不该把整份会话弄丢，也不能冒充别的档"
        );
    }

    #[test]
    fn a_row_without_selection_comes_back_without_one() {
        let mut row = WorkbenchSessionStore::to_context(&session());
        row.selection_start = None;
        row.selection_end = None;
        assert_eq!(WorkbenchSessionStore::to_session(row).selection, None);
    }

    #[test]
    fn an_unknown_mode_key_falls_back_to_sql_instead_of_dropping_the_session() {
        let mut row = WorkbenchSessionStore::to_context(&session());
        row.mode = "notebook-v9".to_string();
        assert_eq!(
            WorkbenchSessionStore::to_session(row).mode,
            EditorMode::Sql,
            "认不出的模式不该丢掉整份会话"
        );
    }
}
