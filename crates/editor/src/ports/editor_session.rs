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

use crate::model::EditorMode;
use crate::session::{SavedSession, SessionStore};
use crate::shared::EditorShared;
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
            // 【B1】连接绑定：`None`（跟随当前连接）在库里就是空串
            connection_id: session.connection.clone().unwrap_or_default(),
            mode: session.mode.as_key().to_string(),
            // 【B13】通道是文档属性（源库 / 加速 / 联邦）：存短码，重启后认回来
            channel: session.channel.code().to_string(),
            content: session.content.clone(),
            cursor_position: session.cursor,
            selection_start: session.selection.map(|(start, _)| start),
            selection_end: session.selection.map(|(_, end)| end),
            // 【T2c-6】工具栏布局编码串（`None` = 没存过 → 空串，恢复用默认布局）
            toolbar: session.toolbar.clone().unwrap_or_default(),
            // 【T2c-1 余项】schema 绑定（`None` = 未选 → 空串）
            schema: session.schema.clone().unwrap_or_default(),
            // 【T2c-4 余项】缩进宽度 / 制表符（`None` = 旧会话没存过 → 列默认）
            tab_size: session.tab_size.unwrap_or(2),
            hard_tabs: session.hard_tabs.unwrap_or(false),
            // 【遗留 #5】结果区高度：editor 会话模型已随结果面板下沉移除该字段
            //（结果区高度由结果面板自持）；engine 持久化列保留供旧库兼容，
            // 桥接层固定写列默认 18——editor 不再消费它。
            result_height: 18.0,
            // 【结果集 #296】列序编码为逗号串（空串 = 没存过 → 默认序）
            result_column_order: session
                .result_column_order
                .clone()
                .unwrap_or_default()
                .join(","),
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
            channel: crate::channel::ExecChannel::from_code(&context.channel),
            // 空串 = 没绑（跟随当前连接）；绑定的 id 原样回来（校验在恢复那一侧做）
            connection: (!context.connection_id.is_empty()).then_some(context.connection_id),
            content: context.content,
            cursor: context.cursor_position,
            selection: match (context.selection_start, context.selection_end) {
                (Some(start), Some(end)) => Some((start, end)),
                _ => None,
            },
            // 【T2c-6】空串 = 没存过（恢复用默认布局 / 连接默认 schema）；非空原样回来
            toolbar: (!context.toolbar.is_empty()).then_some(context.toolbar),
            schema: (!context.schema.is_empty()).then_some(context.schema),
            // 缩进列总有值（默认 2 / false）：旧会话恢复成默认而不是不覆盖——列默认即语义默认
            tab_size: Some(context.tab_size),
            hard_tabs: Some(context.hard_tabs),
            // 【结果集 #296】空串 = 没存过 → 默认序；非空按列名顺序恢复
            result_column_order: (!context.result_column_order.is_empty())
                .then(|| {
                    context
                        .result_column_order
                        .split(',')
                        .map(|column| column.to_string())
                        .collect()
                }),
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

    /// 全量加载（启动多文档恢复用）：全局库按最近更新倒序返回，未命名/空文档不落库天然缺席
    fn load_all(&self) -> Result<Vec<SavedSession>, String> {
        Ok(self
            .store
            .load_all_editor_contexts()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(Self::to_session)
            .collect())
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
    use crate::channel::ExecChannel;
    use crate::model::EditorMode;
    use crate::session::SavedSession;

    fn session() -> SavedSession {
        SavedSession {
            id: "d:/sql/a.sql".to_string(),
            path: Some("D:/sql/A.sql".to_string()),
            mode: EditorMode::Analysis,
            channel: ExecChannel::Source,
            connection: None,
            content: "select 1;".to_string(),
            cursor: 7,
            selection: Some((1, 3)),
            toolbar: None,
            schema: None,
            tab_size: None,
            hard_tabs: None,
            result_column_order: None,
        }
    }

    /// 【B1】连接绑定存在 `connection_id` 列上（空串 = 没绑）；两种情形都要能往返
    #[test]
    fn the_connection_binding_travels_through_the_row() {
        let mut bound = session();
        bound.connection = Some("P_orders".to_string());
        let row = WorkbenchSessionStore::to_context(&bound);
        assert_eq!(row.connection_id, "P_orders");
        assert_eq!(
            WorkbenchSessionStore::to_session(row).connection.as_deref(),
            Some("P_orders")
        );

        let row = WorkbenchSessionStore::to_context(&session());
        assert_eq!(row.connection_id, "", "没绑就是空串（列是 NOT NULL）");
        assert_eq!(
            WorkbenchSessionStore::to_session(row).connection,
            None,
            "空串回来是“跟随当前连接”，不是一条空 id 的绑定"
        );
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

    /// 【T2c-6】工具栏布局 / schema / 缩进都存在列上（空串 = 没存过）；两种情形都要能往返
    #[test]
    fn the_toolbar_schema_and_indent_travel_through_the_row() {
        let mut customized = session();
        customized.toolbar =
            Some("ExecRun,ExecAll,~ExecCurrent,~ExecSelection,~ExecBatch,|,Format,|,More".to_string());
        customized.schema = Some("analytics".to_string());
        customized.tab_size = Some(4);
        customized.hard_tabs = Some(true);

        let row = WorkbenchSessionStore::to_context(&customized);
        assert_eq!(row.toolbar, customized.toolbar.as_deref().unwrap_or_default());
        assert_eq!(row.schema, "analytics");
        assert_eq!(row.tab_size, 4);
        assert!(row.hard_tabs);

        let back = WorkbenchSessionStore::to_session(row);
        assert_eq!(back.toolbar, customized.toolbar);
        assert_eq!(back.schema.as_deref(), Some("analytics"));
        assert_eq!(back.tab_size, Some(4));
        assert_eq!(back.hard_tabs, Some(true));

        // 没存过（空串 / 列默认）→ 回来是 None / 默认值：恢复侧不覆盖
        let plain = WorkbenchSessionStore::to_session(WorkbenchSessionStore::to_context(&session()));
        assert_eq!(plain.toolbar, None, "空串 = 没存过布局");
        assert_eq!(plain.schema, None, "空串 = 没绑 schema");
        assert_eq!(plain.tab_size, Some(2), "列默认缩进宽度 2");
        assert_eq!(plain.hard_tabs, Some(false), "列默认空格缩进");
    }

    #[test]
    fn a_row_without_selection_comes_back_without_one() {
        let mut row = WorkbenchSessionStore::to_context(&session());
        row.selection_start = None;
        row.selection_end = None;
        assert_eq!(WorkbenchSessionStore::to_session(row).selection, None);
    }

    /// 【结果集 #296】列序以逗号串存库、按列名数组认回来；没存过 = 默认序
    #[test]
    fn the_result_column_order_travels_as_a_comma_list() {
        let mut customized = session();
        customized.result_column_order =
            Some(vec!["name".to_string(), "id".to_string(), "created_at".to_string()]);
        let row = WorkbenchSessionStore::to_context(&customized);
        assert_eq!(row.result_column_order, "name,id,created_at");

        let back = WorkbenchSessionStore::to_session(row);
        assert_eq!(
            back.result_column_order.as_deref(),
            Some(&["name".to_string(), "id".to_string(), "created_at".to_string()][..]),
            "按列名顺序原样回来"
        );

        // 空串 = 没存过 → 默认序（None）
        let plain = WorkbenchSessionStore::to_session(WorkbenchSessionStore::to_context(&session()));
        assert_eq!(plain.result_column_order, None, "空串 = 没拖过列序");
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
