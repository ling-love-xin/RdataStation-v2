//! 编辑器共享状态（各面板共用同一份文档集合、结果集与执行通道）
//!
//! 唯一权威是 `EditorService`（文档，A1）与 `ResultStore`（结果，A14）：这里只提供
//! **可克隆的跨面板句柄**，让"一个面板 = 一个标签 = 一份文档"的多面板结构共享同一份状态
//! （标签条、状态栏、关闭逻辑、结果网格都读它）。
//!
//! 约定：**视图只读**（`service()` / `results()`），**变更走事件路径**（`update()` /
//! `update_results()`）——与全仓"render 是纯读路径"一致，避免渲染期改状态。
//!
//! 执行通道（`exec`）由**宿主注入**（`attach_runner`）：editor 不知道连接从哪来，
//! 只要求一个 `QueryRunner`（见 `execution` 模块）。未注入时执行动作会明确报"未接入执行"，
//! 而不是静默什么都不做。

use std::cell::{Ref, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use crate::channel::ChannelsHandle;
use crate::completion::{Catalog, CompletionHandle};
use crate::connection::ConnectionsHandle;
use crate::execution::ExecQueue;
use crate::project::ProjectHandle;
use crate::service::{EditorService, OpenOutcome, OpenRequest};
use crate::session::SessionStore;
use crate::sources::SourcesHandle;
use crate::store::ResultStore;

/// 另存为路径选择端口（由宿主注入：编辑器不依赖 `rfd`）
///
/// 入参 = （当前路径，默认文件名）；返回 = 用户选的路径（`None` = 用户取消，**不是错误**）。
pub type SavePathPicker = Rc<dyn Fn(Option<PathBuf>, String) -> Option<PathBuf>>;

/// 导出落盘的路径选择端口（由宿主注入：编辑器不依赖 `rfd`）
///
/// 入参 = （格式，默认文件名）；返回 = 用户选的路径（`None` = 用户取消，**不是错误**）。
/// 与另存为分开：导出的过滤条件与默认名规则不同（格式决定扩展名），且导出**不改文档身份**。
pub type ExportPathPicker = Rc<dyn Fn(crate::export::ExportFormat, String) -> Option<PathBuf>>;

/// 【M8】结果集列头右键「洞察此列」交给**宿主**的请求。
///
/// 编辑器不依赖 `insight`（分层：洞察是另一个特性 crate）：它只说「这一列 + 产生它的
/// SQL + 在哪条连接上跑出来的」，宿主自己翻译成 `SampleSource`。
///
/// **不物化结果集**（记录在案的决策）：洞察侧拿这段 SQL 重跑一句带 `LIMIT` 的取样。
/// 所以这里不给行数据，也不需要在编辑器侧管临时表生命周期。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsightColumnRequest {
    /// 点的那一列（列名）
    pub column: String,
    /// 产生这份结果的 SQL（洞察侧据此取样）
    pub sql: String,
    /// 这份结果跑在哪条连接上（取样要走源库；未绑定的结果不会给这个入口）
    pub connection: String,
    /// 展示用来源（面板副标题 / 快照来源），如「结果 2」
    pub label: String,
}

/// 【M8】「洞察此列」端口（由宿主注入：workbench 接到右 Dock 洞察面板）
pub type InsightColumnPort = Rc<dyn Fn(InsightColumnRequest, &mut gpui_kit::App)>;

/// 【B8 余项】「打开历史面板」端口（由宿主注入：workbench 接到右 Dock 历史面板）。
///
/// 工具栏「⟲ 历史」按钮点击后经它通知宿主——编辑器不依赖工作台的 Dock。
pub type OpenHistoryPort = Rc<dyn Fn(&mut gpui_kit::App)>;

/// 【#54 C6/C7】「跳转定义目标解析」端口（由宿主注入：workbench 接到导航树定位）。
///
/// 内核 `go_to_definition` 对非 http/https 的 `target_uri` **先问 `show_document`
/// handler**（返回 `true` = 宿主自处理，跳转就此结束；`false` = 内核走默认——
/// 当前文档内移动光标，见 vendor gpui-base `input/editor/lsp/definitions.rs`）。
/// 编辑器只负责桥接：签名对齐内核 `ShowDocumentHandler`
/// （`Rc<dyn Fn(&ShowDocumentParams, &mut Window, &mut App) -> bool>`，输入
/// `input/mod.rs` 实测），宿主实现按需忽略 `Window`。
pub type ShowDocumentPort =
    Rc<dyn Fn(&lsp_types::ShowDocumentParams, &mut gpui_kit::Window, &mut gpui_kit::App) -> bool>;

/// 一次执行完成后的**回执**（给宿主看，不给编辑区看）。
///
/// 编辑器的结果归编辑区（`drain_exec`），回执只回答“哪份文档、用了哪个连接、成没成”——
/// 草稿箱据此把“这份草稿最后一次用哪个连接”写回 `file_meta`（M5 Phase C-2）。
/// 编辑器不关心谁在听，也不做任何过滤（是不是草稿由消费方判定）。
#[derive(Debug, Clone)]
pub struct ExecReceipt {
    pub document: crate::model::DocumentId,
    /// 实际使用的连接（`None` = 未绑定，跟随当前活动连接）。
    pub connection: Option<String>,
    /// 成功才值得记“最近执行”；也留着失败，交给消费方决定。
    pub succeeded: bool,
}

/// 无人取走时回执队列的上限（只保留最近的一批，避免无限长）。
const MAX_EXEC_RECEIPTS: usize = 64;

/// 一个内核条目的运行态（`kernel_id` → 会话 + 事件流 + 启动命令）
pub struct KernelEntry {
    /// 真内核会话（进程 + ZMQ 四通道；命令经它发出）
    pub session: rds_kernel::KernelSession,
    /// spawn 后立即取走的唯一事件流（持续 poll；`take_events` 每次都会换新通道，
    /// 但取走之后 session 内部那个就作废了——这里只持有最初那个）
    pub events: tokio::sync::mpsc::UnboundedReceiver<rds_kernel::KernelEvent>,
    /// 启动该内核用的解释器可执行路径（指示器回显；来自注册表 spec，见 [`crate::kernel`]）
    pub python: std::path::PathBuf,
}

/// 【#17 → #20】旧的单解释器来源已被注册表取代（见 [`crate::kernel`]）：本函数仅保留给
/// 测试 / 集成环境（`RDS_KERNEL_PYTHON` 优先，否则系统 `python`），不再参与 UI 路由。
pub fn kernel_python_path() -> std::path::PathBuf {
    std::env::var_os("RDS_KERNEL_PYTHON")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("python"))
}

/// 从语言推导默认内核 id（**#20 已废弃**：UI 路由走 [`Self::resolve_kernel_for`]，
/// 显式指定 / 会话默认 / 注册表第一个可用；本函数仅保留给测试断言语言→内核档位）
pub fn default_kernel_id(language: crate::cell::CellLanguage) -> &'static str {
    match language {
        crate::cell::CellLanguage::Python => "python3",
        crate::cell::CellLanguage::R => "r",
        crate::cell::CellLanguage::Julia => "julia",
        crate::cell::CellLanguage::Sql | crate::cell::CellLanguage::Markdown => "python3",
    }
}

/// 共享句柄（`Clone` 即克隆 `Rc`，各面板指向同一份状态）
pub struct EditorShared {
    pub(crate) service: Rc<RefCell<EditorService>>,
    pub(crate) results: Rc<RefCell<ResultStore>>,
    /// 执行通道：宿主注入后才有（无宿主 = 无执行，这是真实的能力状态而非错误）
    pub(crate) exec: Rc<RefCell<Option<ExecQueue>>>,
    /// 会话存储：宿主注入后才有（无宿主 = 不持久化光标/模式）
    pub(crate) sessions: Rc<RefCell<Option<Rc<dyn SessionStore>>>>,
    /// 另存为路径选择器：宿主注入后才有（无宿主 = 另存为明确报“未接入”，不静默失败）
    pub(crate) save_path: Rc<RefCell<Option<SavePathPicker>>>,
    /// 导出落盘路径选择器：宿主注入后才有（无宿主 = 导出明确报“未接入”）
    pub(crate) export_path: Rc<RefCell<Option<ExportPathPicker>>>,
    /// 【M8】「洞察此列」端口：宿主注入后才有（未接时列头右键不摆这一项）
    pub(crate) insight_column: Rc<RefCell<Option<InsightColumnPort>>>,
    /// 【B8 余项】「打开历史面板」端口：宿主注入后才有（未接时工具栏「历史」按钮置灰并给原因）
    pub(crate) open_history: Rc<RefCell<Option<OpenHistoryPort>>>,
    /// 连接列表 / 建连端口：宿主注入后才有（B1；无宿主 = 选择器说“未接入”）
    pub(crate) connections: Rc<RefCell<Option<ConnectionsHandle>>>,
    /// 【T2c-1】库 / schema 列表端口：宿主注入后才有（无宿主 = 状态栏面包屑没有可切项）
    pub(crate) schemas: Rc<RefCell<Option<crate::connection::SchemasHandle>>>,
    /// 【B13】通道门控端口：宿主注入后才有（无宿主 = 加速 / 联邦两档都不可用并给原因）
    pub(crate) channels: Rc<RefCell<Option<ChannelsHandle>>>,
    /// 【B13/T1.6】源清单端口：宿主注入后才有（无宿主 = 「源清单 ▾」没有内容）
    pub(crate) sources: Rc<RefCell<Option<SourcesHandle>>>,
    /// 项目态端口：宿主注入后才有（无宿主 = 不读只；项目锁是宿主的事实）
    pub(crate) project: Rc<RefCell<Option<ProjectHandle>>>,
    /// 补全端口：宿主注入后才有（无宿主 = 只给关键字与函数，不给元数据候选）
    pub(crate) completion: Rc<RefCell<Option<CompletionHandle>>>,
    /// 【2b】环境端口：宿主注入后才有（无宿主 = 环境列表为空，通用模式运行不亮）
    pub(crate) env: Rc<RefCell<Option<crate::env::EnvHandle>>>,
    /// 【LSP】语言服务端口：宿主注入后才有（无宿主 = 无语言服务，通用模式不接 LSP）
    pub(crate) lsp: Rc<RefCell<Option<crate::lsp::LspPortHandle>>>,
    /// 【B7 切片三】表结构端口（键列）：宿主注入后才有（无宿主 = 导出菜单不摆
    /// `SQL Updates`——没有键列的 `UPDATE` 只能猜，宁可回绝）
    pub(crate) table_keys: Rc<RefCell<Option<crate::export::TableKeysHandle>>>,
    /// 【2d】分析笔记状态（每文档一份：会话 + 单元序列；输出是会话级内存状态）
    pub(crate) notebooks: Rc<RefCell<std::collections::HashMap<crate::model::DocumentId, crate::cell::NotebookState>>>,
    /// 【#17】分析模式的内核注册表（kernel_id → 真内核会话 + 事件流）
    ///
    /// 生命周期 = 笔记本生命周期：`notebook_close` 时逐个 shutdown。启动 / 执行都是
    /// 事件路径（`KernelSession::spawn` 内部起后台线程，不阻塞 UI）；render 不碰它。
    pub(crate) kernels: Rc<RefCell<std::collections::HashMap<String, KernelEntry>>>,
    /// 【#20】内核注册表端口（宿主注入 `rds_kernel::detect_kernels()` 的快照；
    /// 未注入 = 空列表，菜单如实置灰）
    pub(crate) kernel_port: Rc<RefCell<Option<Rc<dyn crate::kernel::KernelsPort>>>>,
    /// 【2026-10-27】用户折叠开关（工具栏按钮切换；默认开）。
    ///
    /// 与能力表 / 大文件档位并成 [`Self::folding_enabled`] 的最终判据：关掉时折叠候选
    /// 整体不喂（不是"只看不折"）——`sync_fold` 会落 `set_folding(false)`，槽位与
    /// chevron 一并消失，与能力表关闭同口径。
    pub(crate) show_folds: Rc<std::cell::Cell<bool>>,
    /// 【2026-10-28】基于缩进的折叠开关（设置页切换；默认开）。
    /// 开启后只要下一行缩进比当前行大，就会出现折叠按钮。
    pub(crate) indent_folding: Rc<std::cell::Cell<bool>>,
    /// 【2026-10-28】自定义开块关键字列表（折叠用；逗号分隔解析后）。
    pub(crate) fold_open_keywords: Rc<std::cell::RefCell<Vec<String>>>,
    /// 【2026-10-28】自定义闭块关键字列表（折叠用；逗号分隔解析后）。
    pub(crate) fold_close_keywords: Rc<std::cell::RefCell<Vec<String>>>,
    /// 执行回执队列（宿主轮询取走；见 [`ExecReceipt`]）
    pub(crate) receipts: Rc<RefCell<Vec<ExecReceipt>>>,
    /// 【#54 B】语义诊断目录快照（**按文档存**：多文档可各绑各连接，扫描只认
    /// 自己那份文档的目录，不跨文档串）。宿主（workbench）按文档绑定连接注入；
    /// 无条目 = 扫描不参与语义诊断（与「空目录退化」同一口径，宁缺毋滥）。目录是
    /// 纯数据（`Vec<String>`），可跨扫描工作线程 `Arc` 传递。
    pub(crate) catalog_snapshots: Rc<RefCell<std::collections::HashMap<crate::model::DocumentId, Arc<Catalog>>>>,
    /// 【#54 C6/C7】跳转定义目标解析端口（宿主注入；未注入 = 内核走默认文档内移动）
    pub(crate) show_document_port: Rc<RefCell<Option<ShowDocumentPort>>>,
    /// 【书签】每文档一份书签（与 sidecar 同一数据源：`file_meta::Bookmark`）。
    ///
    /// 打开文档时从 `.filename.meta.json` 读入、切换后写回（事件路径，`view/host`）；
    /// 渲染只读。无条目 = 该文档还没有书签记录（与空书签同口径）。
    pub(crate) bookmarks: Rc<RefCell<std::collections::HashMap<crate::model::DocumentId, Vec<file_meta::Bookmark>>>>,
}

impl Clone for EditorShared {
    fn clone(&self) -> Self {
        // 全部字段都是 `Rc`，克隆 = 复制句柄（KernelEntry 不 Clone——tokio 事件接收端
        // 只能有一个消费者，所以这里不能 derive）
        Self {
            service: self.service.clone(),
            results: self.results.clone(),
            exec: self.exec.clone(),
            sessions: self.sessions.clone(),
            save_path: self.save_path.clone(),
            export_path: self.export_path.clone(),
            insight_column: self.insight_column.clone(),
            open_history: self.open_history.clone(),
            connections: self.connections.clone(),
            schemas: self.schemas.clone(),
            channels: self.channels.clone(),
            sources: self.sources.clone(),
            project: self.project.clone(),
            completion: self.completion.clone(),
            env: self.env.clone(),
            lsp: self.lsp.clone(),
            table_keys: self.table_keys.clone(),
            notebooks: self.notebooks.clone(),
            kernels: self.kernels.clone(),
            kernel_port: self.kernel_port.clone(),
            receipts: self.receipts.clone(),
            catalog_snapshots: self.catalog_snapshots.clone(),
            show_document_port: self.show_document_port.clone(),
            show_folds: self.show_folds.clone(),
            indent_folding: self.indent_folding.clone(),
            fold_open_keywords: self.fold_open_keywords.clone(),
            fold_close_keywords: self.fold_close_keywords.clone(),
            bookmarks: self.bookmarks.clone(),
        }
    }
}

impl Default for EditorShared {
    fn default() -> Self {
        Self::new()
    }
}

// ==== Step 4 拆分（2026-10-29）：impl 方法块按能力域迁入子模块 ====
pub mod exec;
pub mod notebook;
pub mod tx;
pub mod session;
pub mod meta;
pub mod env_lsp;
pub mod fold;

impl EditorShared {
    pub fn new() -> Self {
        Self {
            service: Rc::new(RefCell::new(EditorService::new())),
            results: Rc::new(RefCell::new(ResultStore::new())),
            exec: Rc::new(RefCell::new(None)),
            sessions: Rc::new(RefCell::new(None)),
            save_path: Rc::new(RefCell::new(None)),
            export_path: Rc::new(RefCell::new(None)),
            insight_column: Rc::new(RefCell::new(None)),
            open_history: Rc::new(RefCell::new(None)),
            connections: Rc::new(RefCell::new(None)),
            schemas: Rc::new(RefCell::new(None)),
            channels: Rc::new(RefCell::new(None)),
            sources: Rc::new(RefCell::new(None)),
            project: Rc::new(RefCell::new(None)),
            completion: Rc::new(RefCell::new(None)),
            env: Rc::new(RefCell::new(None)),
            lsp: Rc::new(RefCell::new(None)),
            table_keys: Rc::new(RefCell::new(None)),
            notebooks: Rc::new(RefCell::new(std::collections::HashMap::new())),
            kernels: Rc::new(RefCell::new(std::collections::HashMap::new())),
            kernel_port: Rc::new(RefCell::new(None)),
            receipts: Rc::new(RefCell::new(Vec::new())),
            catalog_snapshots: Rc::new(RefCell::new(std::collections::HashMap::new())),
            show_document_port: Rc::new(RefCell::new(None)),
            show_folds: Rc::new(std::cell::Cell::new(true)),
            indent_folding: Rc::new(std::cell::Cell::new(true)),
            fold_open_keywords: Rc::new(std::cell::RefCell::new(
                "CASE,BEGIN,IF,LOOP,WHILE,FOR,REPEAT".split(',').map(|s| s.to_string()).collect()
            )),
            fold_close_keywords: Rc::new(std::cell::RefCell::new(
                "END".split(',').map(|s| s.to_string()).collect()
            )),
            bookmarks: Rc::new(RefCell::new(std::collections::HashMap::new())),
        }
    }

    /// 只读访问文档集合（渲染路径）
    pub fn service(&self) -> Ref<'_, EditorService> {
        self.service.borrow()
    }

    /// 变更文档集合（事件路径）：返回闭包结果，借用边界内完成
    pub fn update<R>(&self, change: impl FnOnce(&mut EditorService) -> R) -> R {
        change(&mut self.service.borrow_mut())
    }

    /// 只读访问结果集（渲染路径）
    pub fn results(&self) -> Ref<'_, ResultStore> {
        self.results.borrow()
    }

    /// 变更结果集（事件路径）
    pub fn update_results<R>(&self, change: impl FnOnce(&mut ResultStore) -> R) -> R {
        change(&mut self.results.borrow_mut())
    }

    /// 某文档当前选中的结果集（克隆一份：调用方拿着它做決定，不拖住 `Ref`）
    pub fn results_active(
        &self,
        document: &crate::model::DocumentId,
    ) -> Option<crate::store::ResultEntry> {
        self.results.borrow().active(document).cloned()
    }

    /// 便捷：打开文档（同路径重复打开 = 激活）
    pub fn open(&self, request: OpenRequest) -> OpenOutcome {
        self.update(|service| service.open(request))
    }

    /// 某文档当前书签（克隆一份；无记录 = 空，与空书签同口径）
    pub fn bookmarks(&self, document: &crate::model::DocumentId) -> Vec<file_meta::Bookmark> {
        self.bookmarks
            .borrow()
            .get(document)
            .cloned()
            .unwrap_or_default()
    }

    /// 覆写某文档书签（打开加载 / 切换写回共用；事件路径）
    pub fn set_bookmarks(
        &self,
        document: &crate::model::DocumentId,
        bookmarks: Vec<file_meta::Bookmark>,
    ) {
        self.bookmarks
            .borrow_mut()
            .insert(document.clone(), bookmarks);
    }
}

/// 【2d】把单元执行的结论拼成内联表格要的 [`crate::store::ResultEntry`]
///
/// 内联表格复用 2a 共用表格（吃 `ResultEntry`）：连接 / 通道在单元上下文里没有面板那一套
/// 结果区元数据，这里用 `None` / 默认通道如实带过（单元卡片自己展示连接信息）。
fn build_inline_entry(
    document: &crate::model::DocumentId,
    outcome: &crate::cell::CellOutcome,
) -> crate::store::ResultEntry {
    let mut entry = match &outcome.result {
        Ok(query) => crate::store::ResultEntry::success(
            document.clone(),
            outcome.sql.clone(),
            outcome.elapsed_ms,
            outcome.truncated,
            query.columns.clone(),
            query.rows.clone(),
        )
        .with_column_types(&query.column_types),
        Err(reason) => crate::store::ResultEntry::failure(
            document.clone(),
            outcome.sql.clone(),
            reason.clone(),
            outcome.elapsed_ms,
        ),
    };
    entry.affected_rows = outcome.affected_rows;
    entry.lineage = Some("单元执行".to_string());
    entry
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::EditorShared;
    use crate::cell::{Cell, CellLanguage, CellNote, CellOutcome, CellRequest, CellState, NotebookState, Output};
    use crate::model::DocumentId;
    use crate::session::Session;

    fn notebook(doc: &DocumentId) -> (EditorShared, NotebookState) {
        let shared = EditorShared::new();
        let mut state = NotebookState::new();
        let mut c1 = Cell::new("c1", CellLanguage::Sql, "select 1");
        c1.mark_running();
        state.cells = vec![c1, Cell::new("c2", CellLanguage::Markdown, "# 备注")];
        shared.set_notebook_state(doc, state.clone());
        (shared, state)
    }

    fn note(doc: &DocumentId, session_id: &str, cell_id: &str, ok: bool) -> CellNote {
        let result = if ok {
            Ok(CellOutcome {
                sql: "select 1".to_string(),
                elapsed_ms: 2,
                affected_rows: None,
                truncated: false,
                result: Ok(crate::execution::QueryData::default()),
            })
        } else {
            Err("表不存在".to_string())
        };
        CellNote {
            document: doc.clone(),
            session_id: session_id.to_string(),
            cell_id: cell_id.to_string(),
            result,
        }
    }

    /// 【2d】回执落单元：Running → Done（带内联 Table 输出）；失败 → Error
    #[test]
    fn applying_a_note_marks_the_running_cell_done_or_error() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);

        // 成功回执 → Done + Table 输出（内联表格吃 ResultEntry）
        shared.apply_cell_note(&doc, &note(&doc, "default", "c1", true));
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells[0].state, CellState::Done);
        assert!(
            matches!(&cells[0].outputs[0], Output::Table(entry) if entry.columns.is_empty()),
            "成功回执拼成内联 Table"
        );
        assert!(
            cells[0]
                .outputs
                .iter()
                .any(|o| matches!(o, Output::Table(_))),
            "回执输出是 Table"
        );
    }

    /// 会话切换后到期的回执**不落**（会话已经换了，别把旧结果贴进新会话）
    #[test]
    fn a_note_from_a_stale_session_is_dropped() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);

        // 先切会话（c1 在跑 → 状态回 Idle，输出清空）
        shared.switch_notebook_session(&doc, Session::new("s2", Some("P_orders".to_string()), None));
        // 旧会话（default）的回执到了：不落
        shared.apply_cell_note(&doc, &note(&doc, "default", "c1", true));
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells[0].state, CellState::Idle, "旧会话回执不落");
        assert!(cells[0].outputs.is_empty());
        assert_eq!(shared.notebook_session(&doc).id, "s2");
        assert_eq!(shared.notebook_session(&doc).default_connection.as_deref(), Some("P_orders"));
    }

    /// 没在跑的回执不落（防迟到的旧回执覆盖新结果）
    #[test]
    fn a_note_for_an_idle_cell_is_dropped() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);
        // c2 从没跑过（Idle）：回执到了不落
        shared.apply_cell_note(&doc, &note(&doc, "default", "c2", true));
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells[1].state, CellState::Idle);
        assert!(cells[1].outputs.is_empty());
    }

    /// 提交单元执行：没有执行器 = 如实拒绝（不静默）
    #[test]
    fn submit_cell_without_a_runner_is_rejected() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);
        let request = CellRequest {
            session_id: "default".to_string(),
            cell_id: "c1".to_string(),
            sql: "select 1".to_string(),
            conn_id: None,
        };
        let err = shared.submit_cell(doc.clone(), &request).expect_err("没接执行器该被拒");
        assert!(!err.is_empty());
    }

    /// 替换单元序列 / 会话是纯内存操作：快照原样往返
    #[test]
    fn cells_and_session_round_trip_through_the_holder() {
        let doc = DocumentId::new("nb");
        let shared = EditorShared::new();
        let mut state = NotebookState::new();
        state.switch_session(Session::new("s9", None, Some("py".to_string())));
        state.cells = vec![Cell::new("x1", CellLanguage::Sql, "select 9")];
        shared.set_notebook_state(&doc, state.clone());

        assert_eq!(shared.notebook_session(&doc).id, "s9");
        assert_eq!(shared.notebook_session(&doc).env.as_deref(), Some("py"));
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].id, "x1");
    }

    /// 【2d】切换会话会把全部有输出的单元标 stale（T2d-2 验收口径）
    #[test]
    fn switching_session_stales_cells_in_the_holder() {
        let doc = DocumentId::new("nb");
        let (shared, _) = notebook(&doc);
        // 先让 c1 跑出结果（Done），再切会话
        shared.apply_cell_note(&doc, &note(&doc, "default", "c1", true));
        assert_eq!(shared.notebook_cells(&doc)[0].state, CellState::Done);

        shared.switch_notebook_session(&doc, Session::default_session());
        let cells = shared.notebook_cells(&doc);
        assert_eq!(cells[0].state, CellState::Stale, "切换会话 → 有输出单元 stale");
    }

    /// 【#54 B】语义诊断目录快照按文档存取：互不串；推 None 即移除；retain 清残留
    #[test]
    fn catalog_snapshots_are_per_document_and_retained() {
        let shared = EditorShared::new();
        let doc_a = DocumentId::new("a.sql");
        let doc_b = DocumentId::new("b.sql");
        let catalog = |name: &str| {
            std::sync::Arc::new(crate::completion::Catalog {
                objects: vec![crate::completion::Candidate {
                    label: name.to_string(),
                    kind: crate::completion::CandidateKind::Table,
                    detail: None,
                }],
                columns: vec![(name.to_string(), "id".to_string(), Some("INTEGER".to_string()))],
                truncated: false,
            })
        };

        // 文档 A 注入后只有 A 读得到（B 无条目 = 语义段跳过）
        shared.set_catalog_snapshot(&doc_a, Some(catalog("t_a")));
        assert!(shared.catalog_snapshot_for(&doc_a).is_some());
        assert!(
            shared.catalog_snapshot_for(&doc_b).is_none(),
            "文档 B 没注入过：不拿 A 的目录猜"
        );

        // 推 None = 移除该文档条目
        shared.set_catalog_snapshot(&doc_a, None);
        assert!(shared.catalog_snapshot_for(&doc_a).is_none());

        // retain：只留现存文档；已关闭文档（不在 alive 里）清掉
        shared.set_catalog_snapshot(&doc_a, Some(catalog("t_a")));
        shared.set_catalog_snapshot(&doc_b, Some(catalog("t_b")));
        shared.retain_catalog_snapshots(&[doc_a.clone()]);
        assert!(shared.catalog_snapshot_for(&doc_a).is_some());
        assert!(
            shared.catalog_snapshot_for(&doc_b).is_none(),
            "b.sql 已关闭 → 残留快照被清"
        );
    }
}
