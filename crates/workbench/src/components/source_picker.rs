//! 来源选择器（宿主侧通用内核）：从 5 类来源选一个数据目标。
//!
//! 洞察 / 联邦 / 结果集二次分析共用同一套「来源浏览 + 单选」，差异只在
//! **物化策略**（选中后如何进 DuckDB：洞察→取样来源、联邦→ATTACH/本地加速、
//! 二次分析→联邦 + 结果集直连），由调用方注入确认回调。
//!
//! 来源集合（用户拍板，三个消费方几乎一致）：
//! ① 已连接数据库的表 —— 连接 → catalog → schema → 表 逐层浏览
//!    （同步内省：`NavigatorService` 有 L1/L2 缓存，模态内点击粒度小、可接受短暂阻塞；
//!    首次内省走 `block_on`，与 `visible_databases_dialog` 同款先例）
//! ② 草稿箱里 duckdb 支持的文件格式（scratchpad store 枚举 + 格式过滤）
//! ③ DuckDB 分析引擎的内部表（`{项目}/.RSmeta/analytics.duckdb` `information_schema.tables`，
//!    与 `mock_generator::analysis_tables` 同一查法）
//! ④ 外部资源管理器中 duckdb 支持的文件（rfd 系统文件对话框，格式过滤）
//! ⑤ 结果集临时表（**仅结果集二次分析启用**；洞察 / 联邦裁剪掉——当前未实装，枚举留位）
//!
//! 归属：留在 workbench（三方入口全在宿主侧）。未来出现第二个非 workbench 直接消费者
//! （独立应用 / editor 就地内嵌）时，整体搬成独立 crate 即可（纯搬文件）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, IconName, WindowExt as _};
use gpui_kit::*;

use database::model::{NavFolder, NavNodeKind, NavPath};
use database::navigator_service::NavigatorService;
use scratchpad::ScratchpadEntry;
use workbench_shell::model::ConnectionItem;

use crate::panels::Shared;
use crate::ui;

/// 来源种类（统一内核；各消费方按 `PickerConfig.kinds` 裁剪显示）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// ① 已连接数据库的表（三层浏览）
    Connected,
    /// ② 草稿箱里 duckdb 支持的文件
    Scratchpad,
    /// ③ DuckDB 分析引擎的内部表
    Analysis,
    /// ④ 外部文件（rfd 系统对话框）
    External,
    /// ⑤ 结果集临时表（仅结果集二次分析启用；洞察 / 联邦裁剪掉，当前未实装）
    ResultSet,
}

impl SourceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Connected => "已连接数据库",
            Self::Scratchpad => "草稿箱",
            Self::Analysis => "资源分析",
            Self::External => "外部文件",
            Self::ResultSet => "结果集",
        }
    }
}

/// 表候选（带归属定位，确认时拼限定名）。
#[derive(Debug, Clone)]
pub struct TableCandidate {
    pub name: String,
    pub catalog: String,
    /// 空串 = 该驱动无独立 schema 层（对齐导航侧口径）
    pub schema: String,
}

/// 来源选择状态（模态渲染闭包每次重建 UI，状态持久在这里）。
///
/// `result_set` 留给结果集二次分析（形态未定，仅枚举留位，不预载字段）。
pub struct PickerState {
    tab: SourceKind,
    // ① 已连接库三层浏览
    conns: Vec<ConnectionItem>,
    selected_conn: Option<String>,
    catalogs: Vec<String>,
    selected_catalog: Option<usize>,
    schemas: Vec<String>,
    selected_schema: Option<usize>,
    tables: Vec<TableCandidate>,
    selected_table: Option<usize>,
    // ② 草稿箱
    drafts: Vec<(String, PathBuf)>, // (相对路径, 绝对路径)
    selected_draft: Option<usize>,
    // ③ 资源分析
    analysis_tables: Vec<String>,
    selected_analysis: Option<usize>,
    // ④ 外部文件
    external: Option<PathBuf>,
    /// 底部提示（加载失败 / 源不可用）
    note: Option<String>,
}

impl PickerState {
    fn new(
        conns: Vec<ConnectionItem>,
        drafts: Vec<(String, PathBuf)>,
        analysis: Vec<String>,
        initial_tab: SourceKind,
    ) -> Self {
        Self {
            tab: initial_tab,
            conns,
            selected_conn: None,
            catalogs: Vec::new(),
            selected_catalog: None,
            schemas: Vec::new(),
            selected_schema: None,
            tables: Vec::new(),
            selected_table: None,
            drafts,
            selected_draft: None,
            analysis_tables: analysis,
            selected_analysis: None,
            external: None,
            note: None,
        }
    }

    /// 当前来源是否已选好目标（确认按钮可用性）。
    pub fn has_target(&self) -> bool {
        match self.tab {
            SourceKind::Connected => self.selected_table.is_some(),
            SourceKind::Scratchpad => self.selected_draft.is_some(),
            SourceKind::Analysis => self.selected_analysis.is_some(),
            SourceKind::External => self.external.is_some(),
            SourceKind::ResultSet => false, // 未实装：不可确认
        }
    }

    /// 当前选中目标的一句话描述（确认按钮文案用）。
    pub fn selected_label(&self) -> Option<String> {
        match self.tab {
            SourceKind::Connected => self
                .selected_table
                .and_then(|i| self.tables.get(i))
                .map(|t| t.name.clone()),
            SourceKind::Scratchpad => self
                .selected_draft
                .and_then(|i| self.drafts.get(i))
                .map(|(rel, _)| rel.clone()),
            SourceKind::Analysis => self
                .selected_analysis
                .and_then(|i| self.analysis_tables.get(i))
                .cloned(),
            SourceKind::External => self
                .external
                .as_ref()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())),
            SourceKind::ResultSet => None,
        }
    }

    /// 写底部提示（供物化策略失败时回显）。
    pub fn set_note(&mut self, note: Option<String>) {
        self.note = note;
    }

    // —— 选中目标访问器（物化策略确认时读取；返回克隆避免借用逃逸）——

    /// 当前来源 Tab（物化策略按来源分派出口）。
    pub fn current_tab(&self) -> SourceKind {
        self.tab
    }

    /// ① 已连接库选中的表（`(连接 id, 表候选)`）。
    pub fn picked_table(&self) -> Option<(String, TableCandidate)> {
        let conn_id = self.selected_conn.clone()?;
        let table = self.selected_table.and_then(|i| self.tables.get(i))?.clone();
        Some((conn_id, table))
    }

    /// ② 草稿箱选中的文件 `(相对路径, 绝对路径)`。
    pub fn picked_draft(&self) -> Option<(String, PathBuf)> {
        self.selected_draft.and_then(|i| self.drafts.get(i)).cloned()
    }

    /// ③ 资源分析选中的表名。
    pub fn picked_analysis(&self) -> Option<String> {
        self.selected_analysis
            .and_then(|i| self.analysis_tables.get(i))
            .cloned()
    }

    /// ④ 外部文件选中的绝对路径。
    pub fn picked_external(&self) -> Option<PathBuf> {
        self.external.clone()
    }
}

/// 对话框配置：标题 + 启用的来源（顺序即 Tab 顺序）+ 确认按钮动作前缀。
#[derive(Debug, Clone, Copy)]
pub struct PickerConfig {
    pub title: &'static str,
    pub kinds: &'static [SourceKind],
    /// 确认按钮前缀（洞察="分析"，联邦="联邦"，二次分析="分析"）。
    pub confirm_prefix: &'static str,
}

/// 物化策略（消费方注入）：读 `PickerState` 里选中的目标，执行出口动作。
/// 失败时写 `state.set_note` + `window.refresh()` 回显，不关对话框。
pub type ConfirmStrategy =
    Rc<dyn Fn(&Shared, &Rc<RefCell<PickerState>>, &mut Window, &mut App)>;

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
    rt.block_on(async { svc.list_all_schemas(conn_id, catalog).await.unwrap_or_default() })
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
    let Ok(_) = rt.block_on(async {
        svc.load_children(conn_id, &path, None, false).await
    }) else {
        return out;
    };
    // 类别文件夹展开取表 / 视图节点（folder 路径的 schema 与展开路径一致）
    for folder in [NavFolder::Tables, NavFolder::Views] {
        let folder_path = NavPath::Folder {
            catalog: catalog.to_string(),
            schema: schema.to_string(),
            folder,
        };
        let children = rt.block_on(async {
            svc.load_children(conn_id, &folder_path, None, false).await
        });
        let Ok(children) = children else { continue };
        for node in children {
            let is_table =
                matches!(node.kind, NavNodeKind::Table { .. } | NavNodeKind::View);
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

/// DuckDB 分析引擎内部表清单（`{项目}/.RSmeta/analytics.duckdb`，无项目读全局分析库）。
///
/// 查法与 `mock_generator::analysis_tables` 一致（`duckdb::Connection::open` + 过滤 `main` schema）。
fn collect_analysis_tables(project: Option<&project::ui::OpenProject>) -> Vec<String> {
    let path = project
        .map(|p| p.root.clone())
        .map(|root| {
            engine::persistence::project_db::ProjectDatabaseManager::analysis_db_path(&root)
        })
        .or_else(|| {
            Some(connection_dialog::services::workspace_loader::global_analysis_db_path())
        });
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

/// 打开来源选择对话框（宿主侧通用入口）。
///
/// 已连接库 / 草稿 / 分析表在打开时一次性预载（都走本地文件或已初始化库，
/// 量级小，模态内短暂阻塞可接受）；提示行按通用规则生成（无连接 / 无项目 / 分析库空）。
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
    if conns.is_empty() {
        note = Some("没有已连接的数据库：先在「连接」里连一个数据源".to_string());
    }
    if note.is_none() && shared.project.borrow().is_none() {
        note = Some("未打开项目：草稿箱 / 资源分析不可用".to_string());
    }
    if analysis.is_empty() && note.is_none() {
        note = Some("分析引擎库为空或不可用（可在资源分析里先做一次分析）".to_string());
    }

    let initial_tab = config.kinds.first().copied().unwrap_or(SourceKind::Connected);
    let state = Rc::new(RefCell::new(PickerState::new(conns, drafts, analysis, initial_tab)));
    state.borrow_mut().note = note;
    let state_build = state.clone();
    let shared_build = shared.clone();
    let strategy_build = strategy.clone();

    window.open_dialog(cx, move |dialog, _window, cx| {
        let s = state_build.borrow();

        // —— 各来源内容（先用 `&mut cx` 渲染：render_external 内部取主题色）——
        let body = match s.tab {
            SourceKind::Connected => render_connected(&s, &state_build, &shared_build, cx),
            SourceKind::Scratchpad => render_scratchpad(&s, &state_build, cx),
            SourceKind::Analysis => render_analysis(&s, &state_build, cx),
            SourceKind::External => render_external(&s, &state_build, cx),
            SourceKind::ResultSet => render_result_set(cx),
        };

        // 再取主题（cx.theme() 借用 cx；放最后避免与上面 `&mut cx` 冲突）
        let theme = cx.theme();

        // —— 来源 Tab 条（按配置裁剪显示）——
        let tabs = config.kinds.iter().map(|t| Tab::new().label(SharedString::from(t.label())));
        let tab_bar = TabBar::new(ElementId::Name(SharedString::from("source-picker-tabs")))
            .underline()
            .children(tabs)
            .selected_index(
                config
                    .kinds
                    .iter()
                    .position(|t| *t == s.tab)
                    .unwrap_or(0),
            )
            .on_click({
                let state = state_build.clone();
                move |index, window, _app| {
                    state.borrow_mut().tab = config.kinds[*index];
                    window.refresh();
                }
            });

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
                            Button::new(ElementId::Name(SharedString::from(
                                "source-picker-ok",
                            )))
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

/// ① 已连接库三层浏览（连接 → catalog → schema → 表）。
///
/// 用 `Button::ghost().toggled()` 做单选行：自带 hover / 焦点 / 键盘激活，
/// 不手搓 `div().on_click()`（对齐 gpui-kit-dev 组件选型）。
fn render_connected(
    s: &PickerState,
    state: &Rc<RefCell<PickerState>>,
    shared: &Shared,
    cx: &mut App,
) -> AnyElement {
    let muted = cx.theme().colors.muted_foreground;
    let mut list = div()
        .v_flex()
        .w_full()
        .gap_1()
        .max_h(rems(ui::VIS_DIALOG_LIST_MAX_H))
        .overflow_y_scrollbar();

    // —— 连接层 ——
    for conn in s.conns.iter() {
        let selected = s.selected_conn.as_deref() == Some(conn.id.as_str());
        let host = conn.host.as_deref().unwrap_or("");
        let label = if host.is_empty() {
            format!("{} — {}", conn.name, conn.driver)
        } else {
            format!("{} — {} ({})", conn.name, conn.driver, host)
        };
        list = list.child(
            Button::new(ElementId::Name(SharedString::from(format!(
                "source-picker-conn-{}",
                conn.id
            ))))
            .ghost()
            .label(SharedString::from(label))
            .toggled(selected)
            .on_click({
                let state = state.clone();
                let shared = shared.clone();
                let conn_id = conn.id.clone();
                move |_, window, _app| {
                    let mut s = state.borrow_mut();
                    s.selected_conn = Some(conn_id.clone());
                    s.catalogs = query_catalogs(
                        &conn_id,
                        shared.project.borrow().as_ref().map(|p| p.root.clone()),
                    );
                    s.selected_catalog = None;
                    s.schemas.clear();
                    s.selected_schema = None;
                    s.tables.clear();
                    s.selected_table = None;
                    s.note = None;
                    window.refresh();
                }
            }),
        );
    }
    if s.conns.is_empty() {
        list = list.child(
            div()
                .text_xs()
                .text_color(muted)
                .child("请先连接一个数据源"),
        );
    }

    // —— catalog 层 ——
    if let Some(sel_c) = s.selected_catalog {
        for (i, catalog) in s.catalogs.iter().enumerate() {
            let selected = sel_c == i;
            let label = format!("◈ {catalog}");
            list = list.child(
                Button::new(ElementId::Name(SharedString::from(format!(
                    "source-picker-cat-{i}-{}",
                    catalog
                ))))
                .ghost()
                .label(SharedString::from(label))
                .toggled(selected)
                .on_click({
                    let state = state.clone();
                    let shared = shared.clone();
                    let catalog = catalog.clone();
                    let conn_id = s.selected_conn.clone().unwrap_or_default();
                    move |_, window, _app| {
                        let mut s = state.borrow_mut();
                        s.selected_catalog = Some(i);
                        s.schemas = query_schemas(
                            &conn_id,
                            &catalog,
                            shared.project.borrow().as_ref().map(|p| p.root.clone()),
                        );
                        s.selected_schema = None;
                        s.tables.clear();
                        s.selected_table = None;
                        s.note = None;
                        window.refresh();
                    }
                }),
            );
        }
    }

    // —— schema 层（空 = 无独立 schema 层，直接进表层）——
    for (i, schema) in s.schemas.iter().enumerate() {
        let selected = s.selected_schema == Some(i);
        let label = format!("▸ {schema}");
        list = list.child(
            Button::new(ElementId::Name(SharedString::from(format!(
                "source-picker-sch-{i}-{}",
                schema
            ))))
            .ghost()
            .label(SharedString::from(label))
            .toggled(selected)
            .on_click({
                let state = state.clone();
                let shared = shared.clone();
                let schema = schema.clone();
                let conn_id = s.selected_conn.clone().unwrap_or_default();
                let catalog = s
                    .catalogs
                    .get(s.selected_catalog.unwrap_or(0))
                    .cloned()
                    .unwrap_or_default();
                move |_, window, _app| {
                    let mut s = state.borrow_mut();
                    s.selected_schema = Some(i);
                    s.tables = query_tables(
                        &conn_id,
                        &catalog,
                        &schema,
                        shared.project.borrow().as_ref().map(|p| p.root.clone()),
                    );
                    s.selected_table = None;
                    s.note = None;
                    window.refresh();
                }
            }),
        );
    }

    // —— 表层（选中 schema 或无 schema 层选中 catalog 后）——
    for (i, table) in s.tables.iter().enumerate() {
        let selected = s.selected_table == Some(i);
        let label = if table.schema.is_empty() {
            table.name.clone()
        } else {
            format!("{} · {}", table.schema, table.name)
        };
        list = list.child(
            Button::new(ElementId::Name(SharedString::from(format!(
                "source-picker-tab-{i}-{}",
                table.name
            ))))
            .ghost()
            .label(SharedString::from(label))
            .toggled(selected)
            .on_click({
                let state = state.clone();
                move |_, window, _app| {
                    let mut s = state.borrow_mut();
                    s.selected_table = Some(i);
                    s.note = None;
                    window.refresh();
                }
            }),
        );
    }
    list.into_any_element()
}

/// ② 草稿箱文件列表。
fn render_scratchpad(
    s: &PickerState,
    state: &Rc<RefCell<PickerState>>,
    cx: &mut App,
) -> AnyElement {
    let muted = cx.theme().colors.muted_foreground;
    let mut list = div()
        .v_flex()
        .w_full()
        .gap_1()
        .max_h(rems(ui::VIS_DIALOG_LIST_MAX_H))
        .overflow_y_scrollbar();
    if s.drafts.is_empty() {
        list = list.child(
            div()
                .text_xs()
                .text_color(muted)
                .child("草稿箱里没有 DuckDB 支持的文件（CSV / Parquet / Excel / JSON）"),
        );
    }
    for (i, (rel, _abs)) in s.drafts.iter().enumerate() {
        let selected = s.selected_draft == Some(i);
        list = list.child(
            Button::new(ElementId::Name(SharedString::from(format!(
                "source-picker-draft-{i}-{}",
                rel
            ))))
            .ghost()
            .label(SharedString::from(rel.clone()))
            .toggled(selected)
            .on_click({
                let state = state.clone();
                move |_, window, _app| {
                    let mut s = state.borrow_mut();
                    s.selected_draft = Some(i);
                    s.note = None;
                    window.refresh();
                }
            }),
        );
    }
    list.into_any_element()
}

/// ③ 资源分析（DuckDB 内部表）。
fn render_analysis(
    s: &PickerState,
    state: &Rc<RefCell<PickerState>>,
    cx: &mut App,
) -> AnyElement {
    let muted = cx.theme().colors.muted_foreground;
    let mut list = div()
        .v_flex()
        .w_full()
        .gap_1()
        .max_h(rems(ui::VIS_DIALOG_LIST_MAX_H))
        .overflow_y_scrollbar();
    if s.analysis_tables.is_empty() {
        list = list.child(
            div()
                .text_xs()
                .text_color(muted)
                .child("分析引擎库为空或不可用（先在资源分析里做一次分析）"),
        );
    }
    for (i, table) in s.analysis_tables.iter().enumerate() {
        let selected = s.selected_analysis == Some(i);
        list = list.child(
            Button::new(ElementId::Name(SharedString::from(format!(
                "source-picker-analysis-{i}-{}",
                table
            ))))
            .ghost()
            .label(SharedString::from(table.clone()))
            .toggled(selected)
            .on_click({
                let state = state.clone();
                move |_, window, _app| {
                    let mut s = state.borrow_mut();
                    s.selected_analysis = Some(i);
                    s.note = None;
                    window.refresh();
                }
            }),
        );
    }
    list.into_any_element()
}

/// ④ 外部文件（rfd 系统对话框）。
fn render_external(
    s: &PickerState,
    state: &Rc<RefCell<PickerState>>,
    cx: &mut App,
) -> AnyElement {
    let muted = cx.theme().colors.muted_foreground;
    let selected = s.external.clone();
    let mut list = div()
        .v_flex()
        .w_full()
        .gap_2()
        .max_h(rems(ui::VIS_DIALOG_LIST_MAX_H))
        .overflow_y_scrollbar();
    list = list.child(
        div()
            .h_flex()
            .items_center()
            .w_full()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(if selected.is_some() {
                        cx.theme().colors.foreground
                    } else {
                        muted
                    })
                    .text_ellipsis()
                    .child(
                        selected
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| {
                                "选择本地文件（CSV / Parquet / Excel / JSON）".to_string()
                            }),
                    ),
            )
            .child(
                Button::new(ElementId::Name(SharedString::from("source-picker-browse")))
                    .ghost()
                    .icon(IconName::FolderOpen)
                    .label("浏览…")
                    .on_click({
                        let state = state.clone();
                        move |_, window, _app| {
                            let mut dialog = rfd::FileDialog::new()
                                .set_title("选择数据文件（DuckDB 支持格式）");
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
                                s.external = Some(path);
                                s.note = None;
                            }
                            window.refresh();
                        }
                    }),
            ),
    );
    list.into_any_element()
}

/// ⑤ 结果集（未实装）：给未来二次分析留位的占位内容。
fn render_result_set(cx: &mut App) -> AnyElement {
    let muted = cx.theme().colors.muted_foreground;
    div()
        .v_flex()
        .w_full()
        .gap_1()
        .max_h(rems(ui::VIS_DIALOG_LIST_MAX_H))
        .overflow_y_scrollbar()
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("结果集二次分析尚未实装"),
        )
        .into_any_element()
}
