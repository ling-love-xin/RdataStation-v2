//! 宿主面板的窗口级测试（GPUI headless 窗口）
//!
//! 覆盖 A2 的验收口径（多文档并存、标题跟随、脏点显示、面板可关闭、渲染不 panic）
//! 与 A10 的动作口径（快捷键真按下 → 动作真生效）。
//!
//! 注意：这里**不使用** `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的 `test`
//! 属性宏带入作用域，而 `#[gpui_kit::test]` 展开出的裸 `#[test]` 会因此解析到它自己
//! （recursion limit reached）。所有依赖显式列举。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::dock::DockArea;
use gpui_kit::component::Root;
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, KeyBinding, ParentElement as _,
    Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
};

use crate::channel::ExecChannel;
use crate::commands::{
    CopyGridSelection, ExecuteAll, ExecuteSql, FormatDocument, NextBookmark, PrevBookmark,
    SaveDocument, ToggleBookmark, ToggleComment, TriggerCompletion,
};
use crate::execution::{self, QueryData, QueryRunner};
use crate::model::{DocumentId, EditorMode};
use crate::service::OpenRequest;
use crate::shared::EditorShared;
use crate::view::host::EditorHostPanel;

/// 测试用最小宿主：一个真 `DockArea` + 挂在里面的编辑面板
///
/// 动作要真正可派发，面板必须**被渲染**（dispatch path 来自已渲染的树），
/// 所以这里得有一个会画出 Dock 的根视图。
struct Harness {
    area: Entity<DockArea>,
    panels: Vec<Entity<EditorHostPanel>>,
}

impl Render for Harness {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.area.clone())
    }
}

/// 临时目录（每个用例一个，互不干扰；用完尽力清理）
fn temp_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("rds_editor_actions_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    dir
}

/// 与 `crates/app/src/main.rs` **同一套**编辑器键位
///
/// 测试里重绑一遍是有意的：context 名或按键串写错时，这里会直接失败，
/// 而不是等到用户按下无反应的键（“注册了才宣传”）。
/// `Ctrl+W` 不在这里：它由宿主执行（见 `close_document_in_dock`），
/// 键位注册仍在 app 层，宿主处理逻辑在 workbench。
fn bind_editor_keys(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("ctrl-s", SaveDocument, Some("editor")),
            KeyBinding::new("ctrl-/", ToggleComment, Some("editor")),
            KeyBinding::new("ctrl-enter", ExecuteSql, Some("editor")),
            KeyBinding::new("ctrl-shift-enter", ExecuteAll, Some("editor")),
            KeyBinding::new("ctrl-shift-f", FormatDocument, Some("editor")),
            KeyBinding::new("ctrl-space", TriggerCompletion, Some("editor")),
            // 【书签】与生产同一套键位：切换 / 下一个 / 上一个
            KeyBinding::new("ctrl-shift-b", ToggleBookmark, Some("editor")),
            KeyBinding::new("f8", NextBookmark, Some("editor")),
            KeyBinding::new("shift-f8", PrevBookmark, Some("editor")),
            // 【B14】结果网格里的 `Ctrl+C`（上下文只在那层元素上，与生产同一份注册）
            KeyBinding::new(
                "ctrl-c",
                CopyGridSelection,
                Some(crate::commands::RESULT_GRID_CONTEXT),
            ),
        ]);
    });
}

/// 建共享状态并打开一份 `.sql` 文档
fn shared_with_document(path: &str, content: &str) -> (EditorShared, DocumentId) {
    let shared = EditorShared::new();
    let id = shared
        .open(OpenRequest::file(path, content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id)
}

// ────────────────────────────────────────────────────────────────────────
// 主题子模块（由 view/tests.rs 拆分，2026-10-29）：
// 辅助与假执行器集中在本文件（mod.rs），子模块经 `use super::` 显式引用。
// ────────────────────────────────────────────────────────────────────────
mod analysis;
mod channels;
mod core;
mod dialogs;
mod encoding;
mod exec;
mod export_pq;
mod format;
mod general_win;
mod legacy;
mod notebook_toolbar;
mod notebook_win;
mod results_a;
mod results_b;
mod toolbar_custom;

// ===== A14：执行（假执行器，真线程）=====

/// 执行器看到的 SQL 序列（A14 断言用）
type SeenSql = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// 执行器看到的连接序列（B1 断言用）
type SeenConnections = std::sync::Arc<std::sync::Mutex<Vec<Option<String>>>>;

/// 执行器看到的**通道**序列（B13 断言用）：文档的“执行位置”必须真的传到执行器
type SeenChannels = std::sync::Arc<std::sync::Mutex<Vec<crate::channel::ExecChannel>>>;

/// 假执行器：记录收到的连接与 SQL，按 SQL 内容决定成败
struct ScriptRunner {
    seen: SeenSql,
    /// 收到的连接（B1）：绑定的连接必须原样传到这里
    seen_connections: SeenConnections,
    /// 收到的通道（B13）：文档的执行位置必须原样传到这里
    seen_channels: SeenChannels,
}

impl QueryRunner for ScriptRunner {
    fn run(
        &self,
        connection: Option<&str>,
        _schema: Option<&str>,
        channel: crate::channel::ExecChannel,
        sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        self.seen.lock().expect("锁").push(sql.to_string());
        self.seen_connections
            .lock()
            .expect("锁")
            .push(connection.map(str::to_string));
        self.seen_channels.lock().expect("锁").push(channel);
        if sql.contains("boom") {
            return Err("驱动报错：boom".to_string());
        }
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 5,
            truncated: false,
            affected_rows: None,
            has_more: false,
            notice: None,
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }
}

/// 带假执行器的共享状态 + 一份文档
fn shared_with_runner(
    content: &str,
    mode: EditorMode,
) -> (EditorShared, DocumentId, SeenSql, SeenConnections) {
    let shared = EditorShared::new();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen_connections = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    shared.attach_runner(std::sync::Arc::new(ScriptRunner {
        seen: seen.clone(),
        seen_connections: seen_connections.clone(),
        seen_channels: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
    }));
    let id = shared
        .open(OpenRequest::untitled(content, mode))
        .id()
        .clone();
    (shared, id, seen, seen_connections)
}

/// 同上，但把**通道**序列也带回来（B13 的断言要看它）
fn shared_with_channel_recorder(content: &str) -> (EditorShared, DocumentId, SeenChannels) {
    let shared = EditorShared::new();
    let channels = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    shared.attach_runner(std::sync::Arc::new(ScriptRunner {
        seen: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        seen_connections: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        seen_channels: channels.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id, channels)
}

/// 假执行器（B2）：按 SQL 里的 `rows=N` 决定结果行数
///
/// 多结果集的测试必须能区分“网格里现在是哪一份”，否则切过去也看不出来。
struct SizedRunner;

impl QueryRunner for SizedRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        // 与 `ScriptRunner` 同一口径：带 `boom` 的语句失败（批量要能验“失败不中断”）
        if sql.contains("boom") {
            return Err("驱动报错：boom".to_string());
        }
        let rows = sql
            .split("rows=")
            .nth(1)
            .and_then(|tail| {
                let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
                digits.parse::<usize>().ok()
            })
            .unwrap_or(1);
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: (0..rows).map(|index| vec![index.to_string()]).collect(),
            column_types: Vec::new(),
            elapsed_ms: 3,
            truncated: false,
            affected_rows: None,
            has_more: false,
            notice: None,
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }
}

/// 带 `SizedRunner` 的共享状态 + 一份文档（B2：多结果集）
fn shared_with_sized_runner(content: &str) -> (EditorShared, DocumentId) {
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(SizedRunner));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id)
}

/// 假执行器（B5）：按 SQL 内容给出「写语句 / 被截断 / 普通结果」三种真实形状
struct ToolbarRunner;

impl QueryRunner for ToolbarRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        if sql.contains("insert") {
            return Ok(QueryData {
                columns: Vec::new(),
                rows: Vec::new(),
                column_types: Vec::new(),
                elapsed_ms: 7,
                truncated: false,
                affected_rows: Some(3),
                has_more: false,
                notice: None,
                fact: None,
                source_fact_table: None,
                source_fact_release: false,
                source_fact_set: 0,
            });
        }
        if sql.contains("truncated") {
            return Ok(QueryData {
                columns: vec!["n".to_string()],
                rows: (0..3).map(|index| vec![index.to_string()]).collect(),
                column_types: Vec::new(),
                elapsed_ms: 4,
                truncated: true,
                affected_rows: None,
                has_more: false,
                notice: None,
                fact: None,
                source_fact_table: None,
                source_fact_release: false,
                source_fact_set: 0,
            });
        }
        Ok(QueryData {
            columns: vec!["n".to_string(), "note".to_string()],
            rows: vec![vec!["1".to_string(), "a".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 2,
            truncated: false,
            affected_rows: None,
            has_more: false,
            notice: None,
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }
}

/// 带 `ToolbarRunner` 的共享状态 + 一份文档（B5：结果工具栏）
fn shared_with_toolbar_runner(content: &str) -> (EditorShared, DocumentId) {
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(ToolbarRunner));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id)
}

/// 假执行器（B6）：回一条**带位置**的真实格式错误（SQLite 的 `no such column: x`）
///
/// 文本用真机抓到的形态之一（另一种是 `near "x": syntax error in … at offset N`）。
struct LocatedFailureRunner;

/// 假执行器（B5b）：首段 2 行且**拿满了**（has_more），取下一段再给 2 行
///
/// 记下每次要的 `(offset, limit)`：界面按的 offset 对不对是这条链路的关键。
struct SegmentRunner {
    asked: std::sync::Arc<std::sync::Mutex<Vec<(usize, usize)>>>,
}

impl QueryRunner for SegmentRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()], vec!["2".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 2,
            truncated: false,
            affected_rows: None,
            has_more: true,
            notice: None,
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }

    fn fetch_next(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        offset: usize,
        limit: usize,
    ) -> Result<QueryData, String> {
        self.asked.lock().expect("锁").push((offset, limit));
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["3".to_string()], vec!["4".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 1,
            truncated: false,
            affected_rows: None,
            has_more: false,
            notice: None,
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }
}

/// 带 `SegmentRunner` 的共享状态 + 一份文档（B5b）
fn shared_with_segment_runner(
    content: &str,
) -> (
    EditorShared,
    DocumentId,
    std::sync::Arc<std::sync::Mutex<Vec<(usize, usize)>>>,
) {
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let shared = EditorShared::new();
    shared.attach_runner(std::sync::Arc::new(SegmentRunner {
        asked: asked.clone(),
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::Sql))
        .id()
        .clone();
    (shared, id, asked)
}

/// 假执行器（B5b）：说“还有下一段”但不支持取（默认 `fetch_next` 说实话）
struct MoreUnsupportedRunner;

impl QueryRunner for MoreUnsupportedRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 1,
            truncated: false,
            affected_rows: None,
            has_more: true,
            notice: None,
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }
}

impl QueryRunner for LocatedFailureRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        if sql.contains("wheree") {
            return Err(format!(
                "[DB_QUERY] Query failed: no such column: wheree (SQL: {sql})"
            ));
        }
        Ok(QueryData {
            columns: vec!["n".to_string()],
            rows: vec![vec!["1".to_string()]],
            column_types: Vec::new(),
            elapsed_ms: 1,
            truncated: false,
            affected_rows: None,
            has_more: false,
            notice: None,
            fact: None,
            source_fact_table: None,
            source_fact_release: false,
            source_fact_set: 0,
        })
    }
}

// ===== 2b：通用模式（环境选择 + 脚本执行）=====
//
// 环境端口（`EnvPort`）与脚本执行（`QueryRunner::run_script`）都是宿主注入的能力：
// 单测里各给一个假实现，把「选环境 → 运行 → 结论变卡片」这条链路整个走一遍。

/// 【2b】假环境端口：固定的环境清单 + 装没装标记（`installed` 缺省 = 没装）
struct FakeEnvPort {
    envs: Vec<crate::env::Environment>,
    installed: std::collections::HashMap<String, bool>,
}

impl crate::env::EnvPort for FakeEnvPort {
    fn list(&self) -> Vec<crate::env::Environment> {
        self.envs.clone()
    }

    fn installed(&self, id: &str) -> bool {
        self.installed.get(id).copied().unwrap_or(false)
    }
}

/// 【2b】假脚本执行器：记录请求并返回预设结论（`run_script` 走独立通道，不碰 SQL）
struct Script2bRunner {
    seen: std::sync::Arc<std::sync::Mutex<Vec<crate::env::ScriptRequest>>>,
    outcome: Result<crate::env::ScriptOutcome, String>,
}

impl QueryRunner for Script2bRunner {
    fn run(
        &self,
        _connection: Option<&str>,
        _schema: Option<&str>,
        _channel: ExecChannel,
        _sql: &str,
        _options: execution::RunOptions,
    ) -> Result<QueryData, String> {
        Err("脚本执行器不跑 SQL".to_string())
    }

    fn run_script(
        &self,
        request: &crate::env::ScriptRequest,
    ) -> Result<crate::env::ScriptOutcome, String> {
        self.seen.lock().expect("锁").push(request.clone());
        self.outcome.clone()
    }
}

/// 常用假环境：Python 3.12（已装）+ Rust（未装）
fn fake_envs() -> Vec<crate::env::Environment> {
    vec![
        crate::env::Environment {
            id: "py".to_string(),
            name: "Python".to_string(),
            kind: crate::env::EnvKind::Python,
            command: "python".to_string(),
            args: vec!["-u".to_string()],
            version: Some("3.12".to_string()),
            readonly: true,
        },
        crate::env::Environment {
            id: "rs".to_string(),
            name: "Rust".to_string(),
            kind: crate::env::EnvKind::Rustc,
            command: "rustc".to_string(),
            args: vec![],
            version: None,
            readonly: true,
        },
    ]
}

/// 【2b】带假环境端口 + 假脚本执行器的共享状态（通用模式文档）
fn shared_with_env(
    content: &str,
    envs: Vec<crate::env::Environment>,
    installed: std::collections::HashMap<String, bool>,
    outcome: Result<crate::env::ScriptOutcome, String>,
) -> (
    EditorShared,
    DocumentId,
    std::sync::Arc<std::sync::Mutex<Vec<crate::env::ScriptRequest>>>,
) {
    let shared = EditorShared::new();
    shared.attach_env(Rc::new(FakeEnvPort { envs, installed }));
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    shared.attach_runner(std::sync::Arc::new(Script2bRunner {
        seen: seen.clone(),
        outcome,
    }));
    let id = shared
        .open(OpenRequest::untitled(content, EditorMode::General))
        .id()
        .clone();
    (shared, id, seen)
}

/// 跑一句话并等回填（B5 用：结果工具栏要真的跟着结果变）
fn run_statement(
    cx: &mut VisualTestContext,
    panel: &Entity<EditorHostPanel>,
    sql: &str,
    placement: execution::ResultPlacement,
) {
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| {
            panel.execute(
                execution::ExecTarget::Statement(sql.to_string()),
                placement,
                cx,
            )
        })
    });
    wait_for_all_pending(cx, panel);
}

/// 等本文档“已提交的语句都回填完”（批量是多条，`wait_for_result` 只等第一条）
fn wait_for_all_pending(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_exec_results(cx);
                panel.pending_for_test() == 0
            })
        });
        if done {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "批量结果迟迟没全部回来"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 等一个条件成立（工作线程把状态摆在内存里时用；超时就直接报出来）
fn wait_until(mut ready: impl FnMut() -> bool, what: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ready() {
        assert!(std::time::Instant::now() < deadline, "等不到：{what}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// 网格当前行数（结果区的真实投影）
fn grid_rows(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) -> usize {
    cx.update(|_window, cx| panel.read(cx).grid_row_count_for_test(cx))
}

/// 结果集标签（文案 + 是否失败）
fn result_tabs(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) -> Vec<(String, bool)> {
    cx.update(|_window, cx| panel.read(cx).result_tabs_for_test(cx))
}

/// 在窗口里建面板（宿主建的窗口第一层视图**
fn open_panel<'a>(
    cx: &'a mut TestAppContext,
    shared: &EditorShared,
    id: &DocumentId,
) -> (Entity<EditorHostPanel>, &'a mut VisualTestContext) {
    let shared = shared.clone();
    let id = id.clone();
    // 窗口挂组件 Root（EditorHostPanel 作为应用内容）：事件路径里的 `has_active_dialog`
    // 等查询依赖组件窗口状态（WindowState 插件），裸 `add_window_view` 缺 Root 会 panic。
    // 与 function_manual.rs 的测试模式同口径；Root::new 会把已注册的组件插件一并挂上。
    let panel_slot = Rc::new(std::cell::RefCell::new(None::<Entity<EditorHostPanel>>));
    let slot = panel_slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let shell = cx.new(|cx| EditorHostPanel::new(shared, id, window, cx));
        *slot.borrow_mut() = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let panel = panel_slot.borrow().clone().expect("EditorHostPanel 已创建");
    (panel, cx)
}

/// 等结果回填（真后台线程；测试里手动跑轮询泵的那一步，带超时）
fn wait_for_result(cx: &mut VisualTestContext, panel: &Entity<EditorHostPanel>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let done = cx.update(|_window, cx| {
            panel.update(cx, |panel, cx| {
                panel.drain_exec_results(cx);
                panel.result_summary_for_test(cx).is_some()
            })
        });
        if done {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "执行结果迟迟没回来");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

