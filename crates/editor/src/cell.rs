//! 分析笔记的单元模型（T2d-1）与 `.rdsnote` 落盘形状（T2d-5）
//!
//! ## 单元（Cell）
//!
//! 一份分析笔记 = 一个**单元序列**。每个单元要么是 SQL（可执行）要么是文本
//! （Markdown 备注，不可执行）。单元状态是会话级的：切换会话（T2d-2）会把全部输出
//! 标 stale，但**单元本身（id / kind / body）不动**。
//!
//! ## 输出（Output）
//!
//! 单元输出是**内联**的（分析模式独有，`OutputTarget::Inline`）：表格复用 2a 共用表格
//! （吃 [`crate::store::ResultEntry`]），文本 / 错误是卡片。输出**跟随会话不跟随文件**：
//! `.rdsnote` 落盘只存单元（id / kind / body），不存输出与运行状态——重开笔记，
//! 单元原样回来，输出清空（如实，不假装记得）。
//!
//! ## 落盘格式
//!
//! ```json
//! { "version": 1, "session": { "id": "default", "default_connection": null, "env": null },
//!   "cells": [ { "id": "c1", "kind": "sql", "body": "select 1;" } ] }
//! ```
//!
//! - `version` 是**顶层版本号**：格式一旦落盘难改，先钉版本字段（任务书风险条款）；
//! - `kind` 存持久化键（`sql` / `text`），不是展示名——展示文案可改，存盘的值不能；
//! - 单元 id 在**新建时生成**（稳定；跨编辑 / 落盘往返不变，界面不得用下标当身份）。

use serde::{Deserialize, Serialize};

use crate::model::DocumentId;
use crate::session::Session;
use crate::store::ResultEntry;

/// 单元种类
///
/// 只有 SQL 单元可执行（T2d-4 验收：Text 单元 ▶ 不亮，状态栏说原因）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellKind {
    /// 可执行：跑在会话绑定的连接上（复用 1b 执行链）
    Sql,
    /// 不可执行：Markdown 备注（原型 §4.4 的「文本单元」）
    Text,
}

impl CellKind {
    /// 界面展示名
    pub fn label(self) -> &'static str {
        match self {
            Self::Sql => "SQL",
            Self::Text => "文本",
        }
    }

    /// 持久化键（存 `.rdsnote` / 存库；展示名可改，存盘的值不能）
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Sql => "sql",
            Self::Text => "text",
        }
    }

    /// 从持久化键还原（认不出来的值 = 文本，不 panic）
    pub fn from_key(key: &str) -> Self {
        match key {
            "sql" => Self::Sql,
            _ => Self::Text,
        }
    }

    /// 是否可执行（决定 ▶ 亮不亮）
    pub fn is_executable(self) -> bool {
        matches!(self, Self::Sql)
    }
}

/// 单元运行状态（会话级：切换会话 → 全部 stale）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellState {
    /// 没跑过（或输出被清）
    #[default]
    Idle,
    /// 正在跑（执行中不可再提交，面板据此亮「中断」）
    Running,
    /// 跑完且成功
    Done,
    /// 跑完但失败（输出 = `Error`）
    Error,
    /// 会话切换后旧输出作废（**不删**，标灰 + 计数让用户知道要重跑）
    Stale,
}

/// 单元输出（内联；分析模式独有）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// 结果集（表格复用 2a 共用表格；`ResultEntry` 是结果区唯一权威的形状）
    Table(ResultEntry),
    /// 文本输出（脚本 stdout 等；卡片渲染）
    Text(String),
    /// 失败原因（卡片渲染，不是表格）
    Error(String),
}

/// 一个单元
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// 稳定 id（新建时生成；跨落盘往返不变，界面不得用下标当身份）
    pub id: String,
    pub kind: CellKind,
    /// 单元正文（SQL 语句 / Markdown）
    pub body: String,
    /// 运行状态（会话级）
    pub state: CellState,
    /// 输出（`None` = 没跑过 / 没输出）
    pub output: Option<Output>,
}

impl Cell {
    /// 新建一个单元（初始 Idle、无输出）
    pub fn new(id: impl Into<String>, kind: CellKind, body: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            body: body.into(),
            state: CellState::Idle,
            output: None,
        }
    }

    /// 标为运行中（`Idle` / `Done` / `Error` / `Stale` → `Running`；旧的失败输出清掉）
    pub fn mark_running(&mut self) {
        self.state = CellState::Running;
        self.output = None;
    }

    /// 标为成功（带输出）
    pub fn mark_done(&mut self, output: Output) {
        self.state = CellState::Done;
        self.output = Some(output);
    }

    /// 标为失败（带原因）
    pub fn mark_error(&mut self, reason: String) {
        self.state = CellState::Error;
        self.output = Some(Output::Error(reason));
    }

    /// 会话切换：输出作废（stale），但单元本体不动
    pub fn mark_stale(&mut self) {
        if matches!(self.state, CellState::Running) {
            // 正在跑的单元切会话：跑完的回执到了也别落（会话已经换了）
            self.state = CellState::Idle;
            self.output = None;
        } else if self.output.is_some() {
            self.state = CellState::Stale;
        }
    }
}

/// 一次单元执行请求（编辑器只表达意图；执行器照 1b 链跑）
///
/// 与 [`crate::env::ScriptRequest`] 同口径：编辑器不猜连接 / 通道 / 方言，
/// 执行器（宿主）拿 `sql` 走**仅 SQL 单元**的 1b 执行链。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellRequest {
    /// 目标会话 id（回执按它定位；切换会话后回执到了不落）
    pub session_id: String,
    /// 目标单元 id（回执按它回填输出）
    pub cell_id: String,
    /// 单元正文（SQL；只有 SQL 单元会提交执行）
    pub sql: String,
}

/// 一次单元执行的结论（宿主给的真实值；编辑器据此拼 `Output::Table` / `Error`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellOutcome {
    /// 实际执行的 SQL（内联表格标题与血缘用）
    pub sql: String,
    /// 执行耗时（毫秒，驱动给的真实值）
    pub elapsed_ms: u64,
    /// 写语句的真实影响行数（驱动没报就是 `None`，界面不编造）
    pub affected_rows: Option<u32>,
    /// 结果是否被截断
    pub truncated: bool,
    /// 查询结论（`Ok` = 列 + 行；`Err` = 失败原因）
    pub result: Result<crate::execution::QueryData, String>,
}

/// 单元执行回执（主线程轮询取走；与 `ExecOutcome` 分开——结果不是停靠面板的形状）
#[derive(Debug, Clone)]
pub struct CellNote {
    pub document: DocumentId,
    /// 回执属于哪个会话（切会话后到期的回执据此丢弃）
    pub session_id: String,
    pub cell_id: String,
    pub result: Result<CellOutcome, String>,
}

/// `.rdsnote` 落盘的单元形状（**不带输出与状态**：输出跟随会话不跟随文件）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellFile {
    pub id: String,
    pub kind: CellKind,
    pub body: String,
}

impl CellFile {
    fn from_cell(cell: &Cell) -> Self {
        Self {
            id: cell.id.clone(),
            kind: cell.kind,
            body: cell.body.clone(),
        }
    }

    fn into_cell(self) -> Cell {
        Cell::new(self.id, self.kind, self.body)
    }
}

/// `.rdsnote` 落盘形状（顶层带版本号；格式一旦落盘难改，先钉版本）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotebookFile {
    /// 格式版本（当前 1；读盘时版本不认识 = 拒绝，不猜测旧格式）
    pub version: u32,
    /// 会话绑定（默认连接 / 环境；重开笔记原样恢复）
    pub session: Session,
    /// 单元序列（输出不跟随文件）
    pub cells: Vec<CellFile>,
}

/// `.rdsnote` 当前格式版本
pub const NOTEBOOK_VERSION: u32 = 1;

/// 一份分析笔记的**运行时状态**（每文档一份，`EditorShared` 持有）
///
/// - 会话绑定（连接 / 环境）：落 `.rdsnote` 跟随文件；
/// - 单元序列：落盘时只存 id / kind / body；
/// - 输出是**会话级内存状态**：切会话 → 全部 stale，重开文件 → 清空（不跟随）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotebookState {
    /// 当前会话（默认会话 = 未绑定连接）
    pub session: crate::session::Session,
    /// 单元序列（顺序 = 显示顺序）
    pub cells: Vec<Cell>,
}

impl NotebookState {
    /// 一份空笔记：默认会话 + 无单元
    pub fn new() -> Self {
        Self {
            session: crate::session::Session::default_session(),
            cells: Vec::new(),
        }
    }

    /// 会话切换：绑定换成新会话，全部有输出的单元标 stale（T2d-2 验收）
    pub fn switch_session(&mut self, session: crate::session::Session) {
        self.session = session;
        for cell in &mut self.cells {
            cell.mark_stale();
        }
    }

    /// 按 id 找单元（可变）
    pub fn cell_mut(&mut self, id: &str) -> Option<&mut Cell> {
        self.cells.iter_mut().find(|cell| cell.id == id)
    }

    /// 当前有多少个 stale 输出（会话切换后的作废计数，界面展示用）
    pub fn stale_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|cell| cell.state == CellState::Stale)
            .count()
    }

    /// 是否有单元在跑（防并发提交）
    pub fn has_running(&self) -> bool {
        self.cells.iter().any(|cell| cell.state == CellState::Running)
    }
}

impl NotebookFile {
    /// 从内存状态生成落盘形状（输出与运行状态**不落盘**）
    pub fn from_state(session: &Session, cells: &[Cell]) -> Self {
        Self {
            version: NOTEBOOK_VERSION,
            session: session.clone(),
            cells: cells.iter().map(CellFile::from_cell).collect(),
        }
    }

    /// 还原成内存形状：单元按 id / kind / body 原样回来，输出清空（如实）
    pub fn into_state(self) -> (Session, Vec<Cell>) {
        (self.session, self.cells.into_iter().map(CellFile::into_cell).collect())
    }

    /// 序列化为 JSON 文本（落盘）
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("序列化 .rdsnote 失败：{e}"))
    }

    /// 从 JSON 文本还原；版本不认识 / 结构坏 = 如实报错（不猜旧格式）
    pub fn from_json(text: &str) -> Result<Self, String> {
        let file: Self =
            serde_json::from_str(text).map_err(|e| format!("解析 .rdsnote 失败：{e}"))?;
        if file.version != NOTEBOOK_VERSION {
            return Err(format!(
                "不认识的 .rdsnote 版本 {}（当前支持 {}）",
                file.version, NOTEBOOK_VERSION
            ));
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        Cell, CellFile, CellKind, CellState, NotebookFile, NotebookState, NOTEBOOK_VERSION, Output,
    };
    use crate::session::Session;

    fn sql_cell(id: &str, body: &str) -> Cell {
        Cell::new(id, CellKind::Sql, body)
    }

    /// 单元状态机：Idle → Running → Done / Error；会话切换 → Stale
    #[test]
    fn cell_state_machine_transitions() {
        let mut cell = sql_cell("c1", "select 1;");
        assert_eq!(cell.state, CellState::Idle);
        assert_eq!(cell.output, None);

        cell.mark_running();
        assert_eq!(cell.state, CellState::Running);

        cell.mark_done(Output::Text("1".to_string()));
        assert_eq!(cell.state, CellState::Done);
        assert!(matches!(cell.output, Some(Output::Text(_))));

        // 失败：Error + 原因输出
        let mut failing = sql_cell("c2", "select * from nope;");
        failing.mark_running();
        failing.mark_error("表不存在".to_string());
        assert_eq!(failing.state, CellState::Error);
        assert_eq!(failing.output, Some(Output::Error("表不存在".to_string())));

        // 会话切换：有输出的标 stale（不删），没跑过的不动
        let mut with_output = sql_cell("c3", "select 2;");
        with_output.mark_done(Output::Text("2".to_string()));
        with_output.mark_stale();
        assert_eq!(with_output.state, CellState::Stale);
        assert!(with_output.output.is_some(), "stale 不删输出，只是作废");

        // 正在跑的切会话：回执作废，输出清空、状态回 Idle（不假装 Done）
        let mut running = sql_cell("c4", "select 3;");
        running.mark_running();
        running.mark_stale();
        assert_eq!(running.state, CellState::Idle);
        assert_eq!(running.output, None);
    }

    /// 单元 kind 的持久化键往返（键与展示名分开）
    #[test]
    fn cell_kind_keys_round_trip() {
        assert_eq!(CellKind::Sql.label(), "SQL");
        assert_eq!(CellKind::Text.label(), "文本");
        assert_eq!(CellKind::Sql.as_key(), "sql");
        assert_eq!(CellKind::Text.as_key(), "text");
        assert_eq!(CellKind::from_key("sql"), CellKind::Sql);
        assert_eq!(CellKind::from_key("text"), CellKind::Text);
        assert_eq!(CellKind::from_key("wat"), CellKind::Text, "认不出的值回文本");
        assert!(CellKind::Sql.is_executable());
        assert!(!CellKind::Text.is_executable());
    }

    /// `.rdsnote` JSON 往返：单元 id / kind / body 原样回来；输出与状态不落盘
    #[test]
    fn notebook_file_round_trips_and_drops_outputs() {
        let session = Session::default_session();
        let mut cells = vec![sql_cell("c1", "select 1;")];
        cells.push(Cell::new("c2", CellKind::Text, "# 备注\n一段说明"));
        // 给第一个单元一个"运行过"的样子：输出与状态**不落盘**
        cells[0].mark_done(Output::Text("1".to_string()));
        cells[0].state = CellState::Done;

        let file = NotebookFile::from_state(&session, &cells);
        assert_eq!(file.version, NOTEBOOK_VERSION);

        let json = file.to_json().expect("serialize");
        assert!(json.contains("\"version\": 1"), "顶层版本号要落盘");
        assert!(!json.contains("output"), "输出不落盘");

        let back = NotebookFile::from_json(&json).expect("parse");
        assert_eq!(back.version, NOTEBOOK_VERSION);
        let (session_back, cells_back) = back.into_state();
        assert_eq!(session_back, session);
        assert_eq!(cells_back.len(), 2);
        assert_eq!(cells_back[0].id, "c1");
        assert_eq!(cells_back[0].kind, CellKind::Sql);
        assert_eq!(cells_back[0].body, "select 1;");
        assert_eq!(cells_back[0].state, CellState::Idle, "状态不跟随文件");
        assert_eq!(cells_back[0].output, None, "输出不跟随文件");
        assert_eq!(cells_back[1].body, "# 备注\n一段说明");
    }

    /// 版本不认识 = 如实拒绝（格式一旦落盘难改，不猜旧格式）
    #[test]
    fn unknown_version_is_rejected() {
        let json = r#"{"version": 99, "session": {"id": "default"}, "cells": []}"#;
        let err = NotebookFile::from_json(json).expect_err("不认识的版本必须拒绝");
        assert!(err.contains("99"), "错误里要说清是哪个版本：{err}");
    }

    /// 结构坏 = 如实报错（不 panic、不静默回空）
    #[test]
    fn broken_json_is_rejected() {
        assert!(NotebookFile::from_json("not json at all").is_err());
        assert!(NotebookFile::from_json(r#"{"cells": []}"#).is_err(), "缺版本字段要报错");
    }

    /// CellFile 的转换是纯函数：落盘形状 = 内存形状的投影
    #[test]
    fn cell_file_is_a_clean_projection() {
        let cell = sql_cell("c1", "select 1;");
        let file = CellFile::from_cell(&cell);
        assert_eq!(file.id, "c1");
        assert_eq!(file.kind, CellKind::Sql);
        assert_eq!(file.body, "select 1;");
        let back = file.into_cell();
        assert_eq!(back.id, "c1");
        assert_eq!(back.state, CellState::Idle);
        assert_eq!(back.output, None);
    }

    /// 【T2d-2】会话切换：绑定换掉，全部有输出的单元 stale；没跑过的不动
    #[test]
    fn switching_session_stales_all_outputs() {
        let mut notebook = NotebookState::new();
        assert_eq!(notebook.session, Session::default_session());

        let mut c1 = sql_cell("c1", "select 1;");
        c1.mark_done(Output::Text("1".to_string()));
        let mut c2 = sql_cell("c2", "select 2;");
        c2.mark_error("表不存在".to_string());
        let idle = sql_cell("c3", "select 3;");
        notebook.cells = vec![c1, c2, idle];

        notebook.switch_session(Session::new("s2", Some("P_orders".to_string()), None));
        assert_eq!(notebook.session.id, "s2");
        assert_eq!(notebook.session.default_connection.as_deref(), Some("P_orders"));
        assert_eq!(notebook.stale_count(), 2, "有输出的两个单元都 stale");
        assert_eq!(notebook.cells[0].state, CellState::Stale);
        assert_eq!(notebook.cells[1].state, CellState::Stale);
        assert_eq!(notebook.cells[2].state, CellState::Idle, "没跑过的不动");
        assert!(notebook.cells[0].output.is_some(), "stale 不删输出");
    }

    /// 运行中 + 会话切换：状态回 Idle、输出清空（回执到了也不落）
    #[test]
    fn switching_session_during_run_drops_the_running_marker() {
        let mut notebook = NotebookState::new();
        let mut c1 = sql_cell("c1", "select 1;");
        c1.mark_running();
        notebook.cells = vec![c1];
        assert!(notebook.has_running());

        notebook.switch_session(Session::default_session());
        assert!(!notebook.has_running());
        assert_eq!(notebook.cells[0].state, CellState::Idle);
        assert_eq!(notebook.cells[0].output, None);
    }
}
