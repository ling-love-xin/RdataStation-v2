//! 来源选择器（宿主侧通用内核）：从三分法三根里选一个数据目标。
//!
//! 洞察 / 联邦 / 结果集二次分析共用同一套「来源浏览 + 单选」，差异只在
//! **物化策略**（选中后如何进 DuckDB：洞察→取样来源、联邦→ATTACH/本地加速、
//! 二次分析→联邦 + 结果集直连），由调用方注入确认回调。
//!
//! ## 形态（`prototypes/entry/01-source-picker.html` + `06-object-tree.html`）
//!
//! tab 不是五个叶子，而是**三个根**（`group::SourceGroup`）：
//! - **数据库**：驱动 + 连接，SQL 下推；连接在，表就在；
//! - **DuckDB**：本地 SQL，装临时产物（结果集 / Mock / 插件 / 会话表）；
//! - **文件**：reader 函数读本机持久物（草稿箱 / 资产库 / 外部文件）。
//!
//! 叶子枚举（5 个）保持不动——分组是视图层的纯函数运算（`group::grouped`），
//! `PickerConfig.kinds` / `confirm_prefix` 都不因此改动。
//!
//! ## 选中模型
//!
//! 树里选的是**节点**，确认时要的是「哪一类目标 + 定位」，所以状态只留一个
//! `picked: Option<Picked>`。这样 `current_tab()` 由**选中的东西**决定，而不是由
//! 「当前停在哪一屏」决定——用户选了草稿再切到数据库 tab，确认按钮仍然说的是草稿
//! （老实现按 tab 分派，这里会答错）。
//!
//! 归属：留在 workbench（三方入口全在宿主侧）。未来出现第二个非 workbench 直接消费者
//! （独立应用 / editor 就地内嵌）时，整体搬成独立 crate 即可（纯搬文件）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::Button;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, WindowExt as _};
use gpui_kit::*;

use database::model::{NavFolder, NavNodeKind, NavPath};
use database::navigator_service::NavigatorService;
use scratchpad::ScratchpadEntry;
use workbench_shell::model::ConnectionItem;

use crate::panels::Shared;

mod db;
mod group;
mod node;
mod render;
mod rows;
mod search;
mod state;

pub use db::{catalog_level, db_rows, schema_rows, table_rows, CatalogLevel, ConnBrief, DbCache};
pub use group::{group_of, grouped, SourceGroup};
pub use node::{ArchiveScope, NodeId, NodeKind, Row, TempScope, MAX_DEPTH};
pub use render::{render_rows, RowAction};
pub use rows::{analysis_table_rows, draft_rows, external_row, root_rows};
pub use search::{rank, search, HitRank, SearchOutcome, SEARCH_LIMIT};
pub use state::{TreeState, RECENT_CAP};

/// 来源种类（5 个叶子；分组见 `group::group_of`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// ① 已连接数据库的表
    Connected,
    /// ② 草稿箱里 duckdb 支持的文件
    Scratchpad,
    /// ③ 资产库（原「资源分析」）——`{项目}/.RSmeta/analytics.duckdb` 的内部表
    Analysis,
    /// ④ 外部文件（rfd 系统对话框）
    External,
    /// ⑤ 结果集临时表（仅结果集二次分析启用；洞察 / 联邦裁剪掉，当前未实装）
    ResultSet,
}

impl SourceKind {
    /// 叶子文案（调试与日志用；tab 上是根标题，不是这个）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Connected => "已连接数据库",
            Self::Scratchpad => "草稿箱",
            Self::Analysis => "资产库",
            Self::External => "外部文件",
            Self::ResultSet => "结果集",
        }
    }
}

/// 表候选（带归属定位，确认时拼限定名）。
#[derive(Debug, Clone, PartialEq)]
pub struct TableCandidate {
    pub name: String,
    pub catalog: String,
    /// 空串 = 该驱动无独立 schema 层（对齐导航侧口径）
    pub schema: String,
}

/// 选中的目标。树里选的是节点，这里落成「哪一类 + 定位」。
#[derive(Debug, Clone, PartialEq)]
pub enum Picked {
    /// 库表 `(连接 id, 表候选)`
    Db(String, TableCandidate),
    /// 草稿文件 `(相对路径, 绝对路径)`
    Draft(String, PathBuf),
    /// 资产库分析表（当前只接本项目副本）
    Analysis(String),
    /// 外部文件绝对路径
    External(PathBuf),
}

impl Picked {
    /// 这个目标属于哪个叶子（`current_tab()` 的真值来源）。
    pub fn kind(&self) -> SourceKind {
        match self {
            Self::Db(..) => SourceKind::Connected,
            Self::Draft(..) => SourceKind::Scratchpad,
            Self::Analysis(..) => SourceKind::Analysis,
            Self::External(..) => SourceKind::External,
        }
    }

    /// 确认按钮上的一句话描述。
    pub fn label(&self) -> String {
        match self {
            Self::Db(_, t) if t.schema.is_empty() => t.name.clone(),
            Self::Db(_, t) => format!("{} · {}", t.schema, t.name),
            Self::Draft(rel, _) => rel.clone(),
            Self::Analysis(t) => t.clone(),
            Self::External(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string()),
        }
    }
}

/// 来源选择状态（模态渲染闭包每次重建 UI，状态持久在这里）。
pub struct PickerState {
    /// 当前激活的根（tab）
    group: SourceGroup,
    /// 本次启用的叶子（来自 `PickerConfig.kinds`）
    kinds: &'static [SourceKind],
    // ① 数据库（懒加载树：每个分支各自展开，缓存按 NodeId 记）
    conns: Vec<ConnectionItem>,
    /// 各层已加载的结果（空结果也记，见 `db.rs` 模块文档末段）
    db_cache: DbCache,
    // ② 草稿箱
    drafts: Vec<(String, PathBuf)>, // (相对路径, 绝对路径)
    // ③ 资产库
    analysis_tables: Vec<String>,
    /// 选中的目标（唯一真值；其余访问器都从它派生）
    picked: Option<Picked>,
    /// 底部提示（加载失败 / 源不可用 / 物化失败回显）
    note: Option<String>,
    /// 树态：展开 / 搜索词 / 选中 / 最近使用（纯内存，随对话框关闭丢弃，见 `state.rs`）
    tree: TreeState,
}

impl PickerState {
    fn new(
        kinds: &'static [SourceKind],
        conns: Vec<ConnectionItem>,
        drafts: Vec<(String, PathBuf)>,
        analysis: Vec<String>,
    ) -> Self {
        let group = grouped(kinds)
            .first()
            .map(|(g, _)| *g)
            .unwrap_or(SourceGroup::Database);
        Self {
            group,
            kinds,
            conns,
            db_cache: DbCache::new(),
            drafts,
            analysis_tables: analysis,
            picked: None,
            note: None,
            tree: TreeState::new(),
        }
    }

    /// 当前来源是否已选好目标（确认按钮可用性）。
    pub fn has_target(&self) -> bool {
        self.picked.is_some()
    }

    /// 当前选中目标的一句话描述（确认按钮文案用）。
    pub fn selected_label(&self) -> Option<String> {
        self.picked.as_ref().map(Picked::label)
    }

    /// 写底部提示（供物化策略失败时回显）。
    pub fn set_note(&mut self, note: Option<String>) {
        self.note = note;
    }

    // —— 选中目标访问器（物化策略确认时读取；返回克隆避免借用逃逸）——

    /// 当前来源（物化策略按来源分派出口）。
    ///
    /// 由**选中的目标**决定，不由激活的 tab 决定：选了草稿再切到数据库 tab，
    /// 确认时仍然按草稿导出（否则会拿错出口）。
    pub fn current_tab(&self) -> SourceKind {
        self.picked
            .as_ref()
            .map(Picked::kind)
            .or_else(|| grouped(self.kinds).first().and_then(|(_, m)| m.first().copied()))
            .unwrap_or(SourceKind::Connected)
    }

    /// ① 已连接库选中的表（`(连接 id, 表候选)`）。
    pub fn picked_table(&self) -> Option<(String, TableCandidate)> {
        match &self.picked {
            Some(Picked::Db(conn, table)) => Some((conn.clone(), table.clone())),
            _ => None,
        }
    }

    /// ② 草稿箱选中的文件 `(相对路径, 绝对路径)`。
    pub fn picked_draft(&self) -> Option<(String, PathBuf)> {
        match &self.picked {
            Some(Picked::Draft(rel, abs)) => Some((rel.clone(), abs.clone())),
            _ => None,
        }
    }

    /// ③ 资产库选中的表名。
    pub fn picked_analysis(&self) -> Option<String> {
        match &self.picked {
            Some(Picked::Analysis(t)) => Some(t.clone()),
            _ => None,
        }
    }

    /// ④ 外部文件选中的绝对路径。
    pub fn picked_external(&self) -> Option<PathBuf> {
        match &self.picked {
            Some(Picked::External(p)) => Some(p.clone()),
            _ => None,
        }
    }
}

/// 对话框配置：标题 + 启用的叶子（顺序决定根与组内顺序）+ 确认按钮动作前缀。
#[derive(Debug, Clone, Copy)]
pub struct PickerConfig {
    pub title: &'static str,
    pub kinds: &'static [SourceKind],
    /// 确认按钮前缀（洞察="分析"，联邦="联邦"，二次分析="分析"）。
    pub confirm_prefix: &'static str,
}

/// 物化策略（消费方注入）：读 `PickerState` 里选中的目标，执行出口动作。
/// 失败时写 `state.set_note` + `window.refresh()` 回显，不关对话框。
pub type ConfirmStrategy = Rc<dyn Fn(&Shared, &Rc<RefCell<PickerState>>, &mut Window, &mut App)>;

/// 导航内省服务（同步 block_on：模态点击粒度小，有缓存时零 I/O）。
fn navigator(project_root: Option<PathBuf>) -> NavigatorService {
    NavigatorService::with_context(
        engine::get_connection_manager().clone(),
        project_root.map(|p| p.to_string_lossy().to_string()),
        false,
    )
}

/// 同步查某连接的 catalog 清单。
fn query_catalogs(conn_id: &str, project_root: Option<PathBuf>) -> Vec<String> {
    let svc = navigator(project_root);
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return Vec::new(),
    };
    rt.block_on(async { svc.list_all_catalogs(conn_id).await.unwrap_or_default() })
}

/// 同步查某 catalog 的 schema 清单。
fn query_schemas(conn_id: &str, catalog: &str, project_root: Option<PathBuf>) -> Vec<String> {
    let svc = navigator(project_root);
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return Vec::new(),
    };
    rt.block_on(async {
        svc.list_all_schemas(conn_id, catalog)
            .await
            .unwrap_or_default()
    })
}

/// 同步查某 schema 下的表（含视图；对齐导航「类别文件夹 → Tables/Views」模型）。
///
/// `schema` 为空串 = 无独立 schema 层：catalog 直接承载类别文件夹。
fn query_tables(
    conn_id: &str,
    catalog: &str,
    schema: &str,
    project_root: Option<PathBuf>,
) -> Vec<TableCandidate> {
    let svc = navigator(project_root);
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    // 有 schema 层：先展开 Schema 节点拿类别文件夹；无则 catalog 直接承载。
    let path = if schema.is_empty() {
        NavPath::Catalog {
            catalog: catalog.to_string(),
        }
    } else {
        NavPath::Schema {
            catalog: catalog.to_string(),
            schema: schema.to_string(),
        }
    };
    let Ok(_) = rt.block_on(async { svc.load_children(conn_id, &path, None, false).await }) else {
        return out;
    };
    // 类别文件夹展开取表 / 视图节点（folder 路径的 schema 与展开路径一致）
    for folder in [NavFolder::Tables, NavFolder::Views] {
        let folder_path = NavPath::Folder {
            catalog: catalog.to_string(),
            schema: schema.to_string(),
            folder,
        };
        let children =
            rt.block_on(async { svc.load_children(conn_id, &folder_path, None, false).await });
        let Ok(children) = children else { continue };
        for node in children {
            let is_table = matches!(node.kind, NavNodeKind::Table { .. } | NavNodeKind::View);
            if is_table {
                out.push(TableCandidate {
                    name: node.name.clone(),
                    catalog: catalog.to_string(),
                    schema: schema.to_string(),
                });
            }
        }
    }
    out
}

/// 草稿箱里 duckdb 支持格式的文件（相对路径 + 绝对路径，按相对路径排序）。
fn collect_drafts(shared: &Shared) -> Vec<(String, PathBuf)> {
    let project_borrow = shared.project.borrow();
    let Some(project) = project_borrow.as_ref() else {
        return Vec::new();
    };
    let root = project.root.clone();
    let Ok((store, rt)) = shared.scratchpad_store() else {
        return Vec::new();
    };
    // 深度 = 递归枚举草稿箱（对齐草稿树展开语义）
    let Ok(entries) = rt.block_on(store.list_local_entries(u32::MAX)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_flattened(&entries, &root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 递归把草稿树拍平成「相对路径 → 绝对路径」（仅保留 duckdb 支持格式）。
fn collect_flattened(
    entries: &[ScratchpadEntry],
    root: &PathBuf,
    out: &mut Vec<(String, PathBuf)>,
) {
    for entry in entries {
        match entry.kind {
            scratchpad::ScratchpadEntryKind::File => {
                let rel = entry.path.to_string_lossy().to_string();
                let abs = root.join(&entry.path);
                if engine::file_reader_function(abs.to_string_lossy().as_ref()).is_some() {
                    out.push((rel, abs));
                }
            }
            scratchpad::ScratchpadEntryKind::Folder => {
                if let Some(children) = &entry.children {
                    collect_flattened(children, root, out);
                }
            }
        }
    }
}

/// 资产库内部表清单（`{项目}/.RSmeta/analytics.duckdb`，无项目读全局分析库）。
///
/// 查法与 `mock_generator::analysis_tables` 一致（`duckdb::Connection::open` + 过滤 `main` schema）。
fn collect_analysis_tables(project: Option<&project::ui::OpenProject>) -> Vec<String> {
    let path = project
        .map(|p| p.root.clone())
        .map(|root| {
            engine::persistence::project_db::ProjectDatabaseManager::analysis_db_path(&root)
        })
        .or_else(|| Some(connection_dialog::services::workspace_loader::global_analysis_db_path()));
    let Some(path) = path else {
        return Vec::new();
    };
    let Ok(conn) = duckdb::Connection::open(&path) else {
        return Vec::new();
    };
    let mut stmt = match conn.prepare(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = 'main' ORDER BY 1",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

// —— 树的三个根：行由各数据源给，可见性由 TreeState 折叠 ——

/// 三分法按配置顺序拼接，**深度优先**：
/// 每根先出根行，紧接它的子树——`TreeState::flatten` 依赖「父行在前、后代紧随」的顺序。
fn body_rows(s: &PickerState) -> Vec<Row> {
    let mut out = Vec::new();
    for (group, _members) in grouped(s.kinds) {
        out.push(rows::group_id_row(group));
        match group {
            SourceGroup::Database => out.extend(db_group_rows(s)),
            SourceGroup::DuckDb => out.extend(duckdb_group_rows(s)),
            SourceGroup::File => out.extend(file_group_rows(s)),
        }
    }
    out
}

/// 数据库根的子树（连接行在 depth 1，其余由缓存给）。
fn db_group_rows(s: &PickerState) -> Vec<Row> {
    if s.conns.is_empty() {
        return Vec::new();
    }
    let briefs: Vec<ConnBrief<'_>> = s
        .conns
        .iter()
        .map(|c| ConnBrief {
            id: c.id.as_str(),
            name: c.name.as_str(),
            driver: c.driver.as_str(),
            host: c.host.as_deref().unwrap_or(""),
        })
        .collect();
    db_rows(&briefs, &s.db_cache)
}

/// DuckDB 根的子树：目前只有「结果集」这一叶，且未实装——显式置灰而不是假装可选。
fn duckdb_group_rows(s: &PickerState) -> Vec<Row> {
    if !s.kinds.contains(&SourceKind::ResultSet) {
        return Vec::new();
    }
    vec![Row::new(
        NodeId::Path("duckdb:result-set".to_string()),
        1,
        NodeKind::File,
        "结果集临时表",
    )
    .with_meta("未实装")
    .disabled()]
}

/// 文件根的子树：草稿箱 / 资产库（本项目段）/ 外部文件。
///
/// 资产库的另外两段（本项目归档文件、全局跨项目共享）数据源还没接（`collect_*` 缺），
/// 所以**不画**——画出来就是假节点（§09：「全局表读不到就必须呈现为不可用」的同一原则）。
fn file_group_rows(s: &PickerState) -> Vec<Row> {
    let mut out = Vec::new();

    if s.kinds.contains(&SourceKind::Scratchpad) {
        let mut row = Row::new(
            NodeId::Path("file:scratchpad".to_string()),
            1,
            NodeKind::Folder,
            "草稿箱",
        );
        if !s.drafts.is_empty() {
            row = row.with_meta(format!("{} 个文件", s.drafts.len()));
        }
        out.push(row);
        // 文件层 = depth 2（草稿箱是 1）
        out.extend(draft_rows(2, &s.drafts));
    }

    if s.kinds.contains(&SourceKind::Analysis) {
        out.push(Row::new(
            NodeId::Path("file:archive-project".to_string()),
            1,
            NodeKind::Folder,
            "资产库 · 本项目",
        ));
        out.extend(analysis_table_rows(
            2,
            &s.analysis_tables,
            ArchiveScope::Project,
        ));
    }

    if s.kinds.contains(&SourceKind::External) {
        out.push(
            Row::new(
                NodeId::Path(PICK_EXTERNAL_KEY.to_string()),
                1,
                NodeKind::File,
                "从磁盘选择文件…",
            )
            .with_meta("CSV / Parquet / Excel / JSON"),
        );
        if let Some(Picked::External(path)) = &s.picked {
            out.push(external_row(2, path));
        }
    }

    out
}

/// 外部文件的「动作行」身份：点它开 rfd，不开树（它不是可确认的对象）。
const PICK_EXTERNAL_KEY: &str = "file:pick-external";

/// 三分法主体：一张树 + 一套回调（三个根共用，行渲染不按根分叉）。
fn render_body(
    s: &PickerState,
    state: &Rc<RefCell<PickerState>>,
    shared: &Shared,
    cx: &mut App,
) -> AnyElement {
    let rows = body_rows(s);

    let on_toggle: RowAction = {
        let state = state.clone();
        let shared = shared.clone();
        Rc::new(move |id: &NodeId, window: &mut Window| {
            // 先切展开态让视觉立即响应，再按需加载（只在「展开」时问连接层）
            let expanding = !state.borrow().tree.is_expanded(id);
            state.borrow_mut().tree.toggle(id);
            window.refresh();
            if expanding {
                load_db_layer(&state, &shared, id, window);
            }
        })
    };

    let on_select: RowAction = {
        let state = state.clone();
        Rc::new(move |id: &NodeId, window: &mut Window| {
            match id {
                NodeId::DbTable {
                    conn,
                    catalog,
                    schema,
                    table,
                } => {
                    let mut s = state.borrow_mut();
                    s.picked = Some(Picked::Db(
                        conn.clone(),
                        TableCandidate {
                            name: table.clone(),
                            catalog: catalog.clone(),
                            schema: schema.clone(),
                        },
                    ));
                    s.tree.select(Some(id.clone()));
                    s.note = None;
                }
                NodeId::AnalysisTable { table, .. } => {
                    let mut s = state.borrow_mut();
                    s.picked = Some(Picked::Analysis(table.clone()));
                    s.tree.select(Some(id.clone()));
                    s.note = None;
                }
                NodeId::Path(p) if p == PICK_EXTERNAL_KEY => {
                    // rfd 是系统模态：阻塞在事件路径里（与老实现同一做法），不在 render
                    let mut dialog =
                        rfd::FileDialog::new().set_title("选择数据文件（DuckDB 支持格式）");
                    for (name, exts) in [
                        ("CSV", &["csv"][..]),
                        ("Parquet", &["parquet"]),
                        ("Excel", &["xlsx", "xls"]),
                        ("JSON", &["json"]),
                    ] {
                        dialog = dialog.add_filter(name, exts);
                    }
                    if let Some(path) = dialog.pick_file() {
                        let mut s = state.borrow_mut();
                        s.picked = Some(Picked::External(path));
                        s.note = None;
                    }
                }
                NodeId::Path(p) => {
                    // 只有草稿文件行能给 Picked::Draft；组行 / 目录行 / 动作行都查不到
                    let mut s = state.borrow_mut();
                    let hit = s
                        .drafts
                        .iter()
                        .find(|(rel, _)| rel == p)
                        .map(|(rel, abs)| (rel.clone(), abs.clone()));
                    if let Some((rel, abs)) = hit {
                        s.picked = Some(Picked::Draft(rel, abs));
                        s.tree.select(Some(id.clone()));
                        s.note = None;
                    }
                }
                _ => return,
            }
            window.refresh();
        })
    };

    render_rows(&rows, &s.tree, on_toggle, on_select, cx)
}

/// 展开某一层时的按需 introspect。
///
/// 深度规则在这里落地（与 `db.rs` 模块文档一致）：
/// - 连接：先问 catalog；只有一个就**跳过 catalog 行**，继续问它的 schema（无 schema 层则直接问表）；
/// - catalog（多库才会出现）：把各 schema 的表汇总成第 3 层，schema 折进标签；
/// - schema：只问它自己的表。
///
/// 缓存命中（含「空结果」）直接返回，所以同一层只会 introspect 一次。
fn load_db_layer(
    state: &Rc<RefCell<PickerState>>,
    shared: &Shared,
    id: &NodeId,
    window: &mut Window,
) {
    if state.borrow().db_cache.is_loaded(id) {
        return;
    }
    let project_root = shared.project.borrow().as_ref().map(|p| p.root.clone());

    let rows = match id {
        NodeId::Conn(conn) => {
            let catalogs = query_catalogs(conn, project_root.clone());
            match catalog_level(conn, &catalogs, 2) {
                CatalogLevel::None => Vec::new(),
                CatalogLevel::Many(rows) => rows,
                CatalogLevel::Single(catalog) => {
                    let schemas = query_schemas(conn, &catalog, project_root.clone());
                    if schemas.is_empty() {
                        // 驱动无独立 schema 层（MySQL 口径）：表直接挂第 2 层
                        let tables = query_tables(conn, &catalog, "", project_root);
                        table_rows(conn, &catalog, "", &tables, 2)
                    } else {
                        schema_rows(conn, &catalog, &schemas, 2)
                    }
                }
            }
        }
        NodeId::DbCatalog { conn, catalog } => {
            let schemas = query_schemas(conn, catalog, project_root.clone());
            let tables = if schemas.is_empty() {
                query_tables(conn, catalog, "", project_root)
            } else {
                let mut all = Vec::new();
                for schema in &schemas {
                    all.extend(query_tables(conn, catalog, schema, project_root.clone()));
                }
                all
            };
            // 第 3 层放表，schema 折进标签（`schema · 表`）
            table_rows(conn, catalog, "", &tables, 3)
        }
        NodeId::DbSchema {
            conn,
            catalog,
            schema,
        } => {
            let tables = query_tables(conn, catalog, schema, project_root);
            table_rows(conn, catalog, schema, &tables, 3)
        }
        _ => return,
    };

    state.borrow_mut().db_cache.put(id.clone(), rows);
    window.refresh();
}

/// 打开来源选择对话框（宿主侧通用入口）。
///
/// 已连接库 / 草稿 / 资产库在打开时一次性预载（都走本地文件或已初始化库，量级小，
/// 模态内短暂阻塞可接受）；连接的表按层懒加载（展开才问）。提示行按通用规则生成。
pub fn open_source_picker(
    shared: &Shared,
    config: PickerConfig,
    strategy: ConfirmStrategy,
    window: &mut Window,
    cx: &mut App,
) {
    let conns = shared
        .connections
        .borrow()
        .iter()
        .filter(|c| c.connected)
        .cloned()
        .collect::<Vec<_>>();
    let drafts = collect_drafts(shared);
    let project = shared.project.borrow().clone();
    let analysis = collect_analysis_tables(project.as_ref());
    let mut note = None;
    if conns.is_empty() && config.kinds.contains(&SourceKind::Connected) {
        note = Some("没有已连接的数据库：先在「连接」里连一个数据源".to_string());
    }
    if note.is_none()
        && shared.project.borrow().is_none()
        && config
            .kinds
            .iter()
            .any(|k| matches!(k, SourceKind::Scratchpad | SourceKind::Analysis))
    {
        note = Some("未打开项目：草稿箱 / 资产库不可用".to_string());
    }
    if note.is_none()
        && config.kinds.contains(&SourceKind::Analysis)
        && analysis.is_empty()
    {
        note = Some("资产库为空或不可用（可先做一次分析）".to_string());
    }

    let state = Rc::new(RefCell::new(PickerState::new(
        config.kinds, conns, drafts, analysis,
    )));
    state.borrow_mut().note = note;
    let state_build = state.clone();
    let shared_build = shared.clone();
    let strategy_build = strategy.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let s = state_build.borrow();
        let groups = grouped(config.kinds);

        // —— 主体：三分法一张树（先渲染，再取主题，避免与 &mut cx 冲突）——
        let body = render_body(&s, &state_build, &shared_build, cx);

        // —— 根 tab 条 ——
        let tabs = groups
            .iter()
            .map(|(g, _)| Tab::new().label(SharedString::from(g.title())));
        let tab_bar = TabBar::new(ElementId::Name(SharedString::from("source-picker-tabs")))
            .underline()
            .children(tabs)
            .selected_index(
                groups
                    .iter()
                    .position(|(g, _)| *g == s.group)
                    .unwrap_or(0),
            )
            .on_click({
                let state = state_build.clone();
                let groups = groups.clone();
                move |index, window, _app| {
                    state.borrow_mut().group = groups[*index].0;
                    window.refresh();
                }
            });

        let theme = cx.theme();
        let group_note = s.group.note();

        let can_confirm = s.has_target();
        let confirm_label = s
            .selected_label()
            .map(|l| format!("{} {}", config.confirm_prefix, l))
            .unwrap_or_else(|| format!("{}选中目标", config.confirm_prefix));

        let dialog = dialog.title(config.title).child(
            div()
                .v_flex()
                .w_full()
                .gap_3()
                .child(tab_bar)
                // 根的一句话说明：回答「怎么读」，不是「是什么」
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.colors.muted_foreground)
                        .child(group_note),
                )
                .child(body)
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .w_full()
                        .gap_2()
                        .child(if let Some(note) = &s.note {
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(theme.colors.warning)
                                .child(note.clone())
                        } else {
                            div().flex_1()
                        })
                        .child(
                            Button::new(ElementId::Name(SharedString::from(
                                "source-picker-cancel",
                            )))
                            .label("取消")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new(ElementId::Name(SharedString::from("source-picker-ok")))
                                .label(confirm_label)
                                .disabled(!can_confirm)
                                .on_click({
                                    let state = state_build.clone();
                                    let shared = shared_build.clone();
                                    let strategy = strategy_build.clone();
                                    move |_, window, cx| {
                                        strategy(&shared, &state, window, cx);
                                    }
                                }),
                        ),
                ),
        );
        dialog
    });
}

#[cfg(test)]
mod tests {
    // 刻意不用 `use super::*`：会把 gpui 的 `test` 属性宏带进作用域，
    // `#[test]` 会解析到它自己（recursion limit reached，见 gpui-kit-dev「窗口测试」）
    use super::{Picked, SourceKind, TableCandidate};
    use std::path::PathBuf;

    /// 出口派发必须看「选中的东西」，不是「停在哪一屏」：
    /// 用户选了草稿再切到数据库 tab，确认时仍然按草稿导出。
    #[test]
    fn picked_kind_is_the_dispatch_source() {
        assert_eq!(
            Picked::Draft("数据/a.parquet".into(), PathBuf::from("C:/p/a.parquet")).kind(),
            SourceKind::Scratchpad
        );
        assert_eq!(Picked::Analysis("orders".into()).kind(), SourceKind::Analysis);
        assert_eq!(
            Picked::External(PathBuf::from("C:/data/x.csv")).kind(),
            SourceKind::External
        );
    }

    /// 确认按钮文案：有 schema 时折进标签，与树上的表行标签一致（同一张表在树里和按钮上不能长得不一样）。
    #[test]
    fn db_label_matches_the_row_label() {
        let table = TableCandidate {
            name: "orders".into(),
            catalog: "analytics".into(),
            schema: "sales".into(),
        };
        assert_eq!(
            Picked::Db("c1".into(), table.clone()).label(),
            "sales · orders"
        );

        let no_schema = TableCandidate {
            name: "orders".into(),
            catalog: "shop".into(),
            schema: String::new(),
        };
        assert_eq!(Picked::Db("c1".into(), no_schema).label(), "orders");
    }

    /// 资产库正名（裁决 8）：叶子文案不再叫「资源分析」。
    #[test]
    fn analysis_kind_is_renamed_to_archive() {
        assert_eq!(SourceKind::Analysis.label(), "资产库");
    }
}
