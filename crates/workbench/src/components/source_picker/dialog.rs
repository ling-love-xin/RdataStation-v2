//! 对话框外壳：根 tab、三分法主体（一棵树）、搜索框、键盘、最近使用、底部栏。
//!
//! 为什么从 `mod.rs` 挪出来：这是「一个模态长什么样 + 怎么被操作」的全部代码，
//! 与状态 / 行模型 / 数据访问无关。混在一起时 `mod.rs` 永远是全模块最胖的文件。
//!
//! 为什么是一个 `Render` 实体而不是一段闭包：搜索框的 `InputState` 需要订阅
//! `InputEvent::Change` 才能把词写进树态，而订阅要求 `Context<T>`；
//! `open_dialog` 的 builder 只给 `&mut App`。抬成实体后订阅、焦点句柄、键盘回调
//! 都有地方挂，`open_source_picker` 退化成「预载数据 + 装配 + 打开」。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::v_virtual_list;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::{ActiveTheme, Sizable as _};
use gpui_kit::*;

use workbench_shell::ui;

use crate::panels::Shared;

use super::db::{db_rows, ConnBrief};
use super::group::{grouped, SourceGroup};
use super::load::{collect_analysis_tables, collect_drafts, load_db_layer};
use super::node::{ArchiveScope, NodeId, NodeKind, Row, TempScope};
use super::render::{needs_virtual_list, render_rows, row_element, virtual_viewport_rem, RowAction};
use super::rows::{
    analysis_table_rows, draft_rows, external_row, group_id_row, insight_sample_rows,
    result_set_rows,
};
use super::state::TreeState;
use super::{ConfirmStrategy, Picked, PickerConfig, PickerState, SourceKind, TableCandidate};

/// 外部文件的「动作行」身份：点它开 rfd，不是可确认的对象。
const PICK_EXTERNAL_KEY: &str = "file:pick-external";

/// 三分法按配置顺序拼接，**深度优先**：
/// 每根先出根行，紧接它的子树——`TreeState::flatten` 依赖「父行在前、后代紧随」的顺序。
fn body_rows(s: &PickerState) -> Vec<Row> {
    let mut out = Vec::new();
    for (group, _members) in grouped(s.kinds) {
        out.push(group_id_row(group));
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

/// DuckDB 根的子树：结果集临时表（`tmp_q_`）+ 洞察产物（`tmp_i_`）。
///
/// 列表来自 `engine::analysis::result_set::{list_result_sets, list_insight_samples}`
/// （按前缀查库，权威是库里实际表）；为空时给**明确说不可用**的提示行，而不是
/// 画一个点不动的假节点。两段分开展示：用户在树里选的语义不同（结果集是「我刚跑出来的」，
/// 洞察产物是「我已经取过样的」）。
fn duckdb_group_rows(s: &PickerState) -> Vec<Row> {
    let mut out = Vec::new();

    if s.kinds.contains(&SourceKind::ResultSet) {
        if s.result_sets.is_empty() {
            out.push(
                Row::new(
                    NodeId::Path("duckdb:result-set-empty".to_string()),
                    1,
                    NodeKind::File,
                    "暂无结果集（先跑一次查询）",
                )
                .disabled(),
            );
        } else {
            out.extend(result_set_rows(1, &s.result_sets));
        }
    }

    if s.kinds.contains(&SourceKind::InsightSample) {
        let mut folder = Row::new(
            NodeId::Path("duckdb:insight-sample-folder".to_string()),
            1,
            NodeKind::Folder,
            "洞察产物",
        );
        folder = if s.insight_samples.is_empty() {
            folder.with_meta("暂无（可先做一次洞察）")
        } else {
            folder.with_meta(format!("{} 张样本表", s.insight_samples.len()))
        };
        out.push(folder);
        out.extend(insight_sample_rows(2, &s.insight_samples));
    }

    out
}

/// 文件根的子树：草稿箱 / 资产库（本项目段）/ 外部文件。
///
/// 资产库的另外两段（本项目归档文件、全局跨项目共享）数据源还没接（`collect_*` 缺），
/// 所以**不画**——画出来就是假节点（§09「读不到就必须呈现为不可用」的同一原则）。
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

    // 全局分析表：与「本项目」并列成第二段，不混排——用户必须一眼看出自己动的是
    // 跨项目共用的那一份（`{system}/analytics.duckdb`，只读；取样另开短命会话）。
    if s.kinds.contains(&SourceKind::AnalysisGlobal) {
        let mut folder = Row::new(
            NodeId::Path("file:archive-global".to_string()),
            1,
            NodeKind::Folder,
            "资产库 · 全局",
        );
        if s.global_tables.is_empty() {
            folder = folder.with_meta("只读 · 暂无全局表");
        } else {
            folder = folder.with_meta(format!("只读 · {} 张表", s.global_tables.len()));
        }
        out.push(folder);
        out.extend(analysis_table_rows(
            2,
            &s.global_tables,
            ArchiveScope::Global,
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

/// 最近使用：把 `NodeId` 映射回行（还没在树上展开也能点）。
///
/// 命中不到就跳过——权限变更 / 文件被删之后，最近使用里的东西可能已经不存在了，
/// 这时候画一个点不动的行只会误导。
fn recent_rows(rows: &[Row], tree: &TreeState) -> Vec<Row> {
    if tree.is_searching() {
        return Vec::new();
    }
    tree.recent()
        .iter()
        .filter_map(|id| rows.iter().find(|r| &r.id == id))
        .map(|r| {
            let mut row = r.clone();
            row.depth = 0;
            row.breadcrumb = None;
            row
        })
        .collect()
}

/// 三分法主体：一张树 + 一套回调（三个根共用，行渲染不按根分叉）。
///
/// 行数超过阈值时切虚拟视口：大库（深层 schema 展开）能有上千行，整段铺开等于
/// 一帧建上千个元素（行里还有 Button 与 Icon）。短列表仍走普通列表——对话框
/// 跟着内容长高的观感不变。
fn render_body(
    rows: &[Row],
    state: &Rc<RefCell<PickerState>>,
    shared: &Shared,
    window: &mut Window,
    cx: &mut Context<PickerShell>,
) -> AnyElement {
    let tree = state.borrow().tree.clone();

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
                NodeId::AnalysisTable { scope, table } => {
                    let mut s = state.borrow_mut();
                    s.picked = Some(match scope {
                        ArchiveScope::Project => Picked::Analysis(table.clone()),
                        ArchiveScope::Global => Picked::AnalysisGlobal(table.clone()),
                    });
                    s.tree.select(Some(id.clone()));
                    s.note = None;
                }
                // 结果集临时表：只有本机单例里的能做二次分析（会话态的表随会话消失）
                NodeId::Temp {
                    scope: TempScope::Local,
                    table,
                } => {
                    let mut s = state.borrow_mut();
                    s.picked = Some(Picked::ResultSet(table.clone()));
                    s.tree.select(Some(id.clone()));
                    s.note = None;
                }
                NodeId::InsightSample(table) => {
                    let mut s = state.borrow_mut();
                    s.picked = Some(Picked::InsightSample(table.clone()));
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

    let visible_refs = tree.visible_rows(rows);
    if !needs_virtual_list(visible_refs.len()) {
        return render_rows(rows, &tree, on_toggle, on_select, cx);
    }

    // 虚拟视口：`visible_rows` 给的是 `Vec<&Row>`（借用入参），而视口回调要求 `'static`，
    // 所以先落成自有行；展开态与选中态也**先取成朴素布尔**再交进回调——回调里不能再借
    // `PickerState`（那时借用生命周期与 `Rc<RefCell>` 会撞双借用 panic）。
    let flags: Rc<Vec<(bool, bool)>> = Rc::new(
        visible_refs
            .iter()
            .map(|row| (tree.is_expanded(&row.id), tree.selected() == Some(&row.id)))
            .collect(),
    );
    let visible: Rc<Vec<Row>> = Rc::new(visible_refs.into_iter().cloned().collect());
    let sizes = workbench_shell::tree::row_sizes(visible.len(), window.rem_size(), |_| {
        workbench_shell::tree::RowHeight::new(ui::ROW_HEIGHT)
    });

    let toggle = on_toggle.clone();
    let select = on_select.clone();
    let build_rows = visible.clone();
    let build_flags = flags.clone();
    let list = v_virtual_list(
        cx.entity().clone(),
        "source-picker-tree",
        sizes,
        move |_this, range, _window, cx| {
            range
                .map(|i| {
                    row_element(
                        &build_rows[i],
                        build_flags[i].0,
                        build_flags[i].1,
                        &toggle,
                        &select,
                        cx,
                    )
                })
                .collect::<Vec<AnyElement>>()
        },
    );

    div()
        .w_full()
        .h(rems(virtual_viewport_rem(visible.len())))
        .overflow_hidden()
        .child(list)
        .into_any_element()
}

/// 最近使用区块：单独一条渲染（≤5 行，不会真的滚），行点击只改选中。
fn recent_block(rows: Vec<Row>, state: &Rc<RefCell<PickerState>>, cx: &mut App) -> AnyElement {
    let tree = state.borrow().tree.clone();
    let on_toggle: RowAction = Rc::new(|_, _| {});
    let on_select: RowAction = {
        let state = state.clone();
        Rc::new(move |id: &NodeId, window: &mut Window| {
            state.borrow_mut().tree.select(Some(id.clone()));
            window.refresh();
        })
    };
    render_rows(&rows, &tree, on_toggle, on_select, cx)
}

/// 对话框视图：持有树态、搜索框、焦点与订阅。
struct PickerShell {
    state: Rc<RefCell<PickerState>>,
    shared: Shared,
    config: PickerConfig,
    strategy: ConfirmStrategy,
    search: Entity<InputState>,
    focus_handle: FocusHandle,
    /// 搜索框订阅句柄（仅持有；drop 即退订）
    _search_sub: Subscription,
}

impl PickerShell {
    fn new(
        state: Rc<RefCell<PickerState>>,
        shared: Shared,
        config: PickerConfig,
        strategy: ConfirmStrategy,
        search: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let state_for_sub = state.clone();
        let shared_for_sub = shared.clone();
        let strategy_for_sub = strategy.clone();
        // 用户敲字 → 写树态 → 重绘。程序化写值（Esc 清空）不发事件，那条路径自己写树态。
        //
        // **Enter 必须走这里**：单行 Input 绑了 enter 且会自己处理（`gpui-base` 的
        // `KeyBinding::new("enter"…)`），而 gpui 的绑定先于 `on_key_down` 派发
        // （`gpui-pre/src/window.rs:6412-6427`），一旦被输入框接住就不再往下冒泡——
        // 所以根上的 `on_key_down("enter")` 在搜索框聚焦时**永远收不到**。实测口径
        // 见本仓 `quick_open/palette/render.rs:171-172` 的注释（那里的 Enter 也是
        // 由输入框事件给出的）。缺了这段，「敲完搜索词直接回车」什么都不会发生。
        let search_sub = cx.subscribe_in(
            &search,
            window,
            move |_this, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    let text = input.read(cx).value().to_string();
                    state_for_sub.borrow_mut().tree.set_query(text);
                    cx.notify();
                }
                // 只认「不带修饰的 Enter」：Shift/Ctrl+Enter 在输入框里是换行/提交变体，
                // 选源对话框没有这两种语义，忽略比乱猜安全。
                InputEvent::PressEnter { secondary, shift } if !secondary && !shift => {
                    let armed = state_for_sub.borrow().has_target();
                    if armed {
                        if let Some(id) = state_for_sub.borrow().tree.selected().cloned() {
                            state_for_sub.borrow_mut().tree.push_recent(id);
                        }
                        strategy_for_sub(&shared_for_sub, &state_for_sub, window, cx);
                    }
                }
                _ => {}
            },
        );
        Self {
            state,
            shared,
            config,
            strategy,
            search,
            focus_handle,
            _search_sub: search_sub,
        }
    }
}

impl Render for PickerShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.state.clone();
        let shared = self.shared.clone();
        let strategy = self.strategy.clone();
        let config = self.config;
        let search = self.search.clone();
        let focus_handle = self.focus_handle.clone();

        let rows = body_rows(&state.borrow());
        let recent = recent_rows(&rows, &state.borrow().tree);
        let truncated = state.borrow().tree.search_progress(&rows);
        let body = render_body(&rows, &state, &shared, window, cx);
        let recent_el = recent_block(recent.clone(), &state, cx);

        let colors = cx.theme().colors;
        let groups = grouped(config.kinds);
        let tab_index = groups
            .iter()
            .position(|(g, _)| *g == state.borrow().group)
            .unwrap_or(0);
        let (group_note, can_confirm, confirm_label, note) = {
            let s = state.borrow();
            (
                s.group.note(),
                s.has_target(),
                s.selected_label()
                    .map(|l| format!("{} {}", config.confirm_prefix, l))
                    .unwrap_or_else(|| format!("{}选中目标", config.confirm_prefix)),
                s.note.clone(),
            )
        };

        let tabs = groups
            .iter()
            .map(|(g, _)| Tab::new().label(SharedString::from(g.title())));
        let tab_bar = TabBar::new(ElementId::Name(SharedString::from("source-picker-tabs")))
            .underline()
            .children(tabs)
            .selected_index(tab_index)
            .on_click({
                let state = state.clone();
                let groups = groups.clone();
                move |index, window, _cx: &mut App| {
                    state.borrow_mut().group = groups[*index].0;
                    window.refresh();
                }
            });

        // —— 键盘（§04）：焦点挂在根容器上，内部控件的事件冒泡上来 ——
        let on_key = {
            let state = state.clone();
            let shared = shared.clone();
            let strategy = strategy.clone();
            let search = search.clone();
            let rows = rows.clone();
            move |ev: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                let key = ev.keystroke.key.as_str();
                let ctrl = ev.keystroke.modifiers.control || ev.keystroke.modifiers.platform;
                match key {
                    // Ctrl+F：焦点交给搜索框
                    "f" if ctrl => {
                        let handle = search.read(cx).focus_handle(cx);
                        window.focus(&handle, cx);
                    }
                    "up" => {
                        state.borrow_mut().tree.step_selection(&rows, false);
                        window.refresh();
                    }
                    "down" => {
                        state.borrow_mut().tree.step_selection(&rows, true);
                        window.refresh();
                    }
                    // → 展开当前行（叶子无反应）
                    "right" => {
                        let id = state.borrow().tree.selected().cloned();
                        if let Some(id) = id {
                            let expandable = rows
                                .iter()
                                .find(|r| r.id == id)
                                .map(|r| r.kind.is_expandable())
                                .unwrap_or(false);
                            if expandable {
                                state.borrow_mut().tree.expand(&id);
                                load_db_layer(&state, &shared, &id, window);
                                window.refresh();
                            }
                        }
                    }
                    // ← 收起当前行（选中不变：选中行暂时不可见是允许的）
                    "left" => {
                        let id = state.borrow().tree.selected().cloned();
                        if let Some(id) = id {
                            state.borrow_mut().tree.collapse(&id);
                            window.refresh();
                        }
                    }
                    "enter" => {
                        if state.borrow().has_target() {
                            if let Some(id) = state.borrow().tree.selected().cloned() {
                                state.borrow_mut().tree.push_recent(id);
                            }
                            strategy(&shared, &state, window, cx);
                        }
                    }
                    // Esc：有搜索词先清词（不清选中，§03），没词才关对话框
                    "escape" => {
                        if state.borrow().tree.is_searching() {
                            state.borrow_mut().tree.clear_query();
                            window.refresh();
                            search.update(cx, |input, cx| input.set_value("", window, cx));
                        } else {
                            window.close_dialog(cx);
                        }
                    }
                    _ => {}
                }
            }
        };

        let mut root = div()
            .v_flex()
            .w_full()
            .gap_2()
            .track_focus(&focus_handle)
            .on_key_down(on_key)
            .child(tab_bar)
            // 根的一句话说明：回答「怎么读」，不是「是什么」
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(group_note),
            )
            .child(Input::new(&self.search).small());

        if let Some((shown, total)) = truncated {
            root = root.child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!(
                        "命中 {total} 个，只显示前 {shown} 个；再输入几个字缩小范围"
                    )),
            );
        }
        if !recent.is_empty() {
            root = root
                .child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child("最近使用"),
                )
                .child(recent_el);
        }

        root.child(body).child(
            div()
                .h_flex()
                .items_center()
                .w_full()
                .gap_2()
                .child(match note {
                    Some(note) => div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(colors.warning)
                        .child(note),
                    None => div().flex_1(),
                })
                .child(
                    Button::new(ElementId::Name(SharedString::from("source-picker-cancel")))
                        .label("取消")
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .child(
                    Button::new(ElementId::Name(SharedString::from("source-picker-ok")))
                        .label(confirm_label)
                        .disabled(!can_confirm)
                        .on_click({
                            let state = state.clone();
                            let shared = shared.clone();
                            let strategy = strategy.clone();
                            move |_, window, cx| {
                                if let Some(id) = state.borrow().tree.selected().cloned() {
                                    state.borrow_mut().tree.push_recent(id);
                                }
                                strategy(&shared, &state, window, cx);
                            }
                        }),
                ),
        )
    }
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
    // ③-b 全局分析表：只在启用该叶子时去挂（短命会话只读挂
    // `{system}/analytics.duckdb`；没初始化 / 撞锁都不该让整个对话框开不出来）
    let global_tables = if config.kinds.contains(&SourceKind::AnalysisGlobal) {
        engine::analysis::global::global_tables("C_source_picker").unwrap_or_default()
    } else {
        Vec::new()
    };
    // ⑤ 结果集：只有启用该叶子时才查库（它是进程单例里的 `tmp_q_` 表，查不到也不报错）
    let result_sets = if config.kinds.contains(&SourceKind::ResultSet) {
        engine::analysis::result_set::list_result_sets().unwrap_or_default()
    } else {
        Vec::new()
    };
    // ⑥ 洞察产物（`tmp_i_`，同一台单例）：再分析入口的数据源
    let insight_samples = if config.kinds.contains(&SourceKind::InsightSample) {
        engine::analysis::result_set::list_insight_samples().unwrap_or_default()
    } else {
        Vec::new()
    };
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
    if note.is_none() && config.kinds.contains(&SourceKind::Analysis) && analysis.is_empty() {
        note = Some("资产库为空或不可用（可先做一次分析）".to_string());
    }
    // 全局段的空态单独说：它跟「项目里还没有分析」是两件事（一个不该被另一个掩盖）
    if note.is_none() && config.kinds.contains(&SourceKind::AnalysisGlobal) && global_tables.is_empty()
    {
        note = Some(
            "全局分析库为空或暂时不可用（它由系统库维护；被占用时也会读不到）".to_string(),
        );
    }
    if note.is_none() && config.kinds.contains(&SourceKind::ResultSet) && result_sets.is_empty() {
        note = Some("还没有结果集：先跑一次查询，结果集会作为临时表出现在这里".to_string());
    }
    if note.is_none() && config.kinds.contains(&SourceKind::InsightSample) && insight_samples.is_empty()
    {
        note = Some("还没有洞察产物：先做一次洞察取样，样本表会出现在这里".to_string());
    }

    let state = Rc::new(RefCell::new(PickerState::new(
        config.kinds,
        conns,
        drafts,
        analysis,
        global_tables,
        result_sets,
        insight_samples,
    )));
    state.borrow_mut().note = note;
    // 打开即展开首个根：否则首屏只有三行光秃秃的根，比老的扁平列表更「空」。
    // 只展开根这一层——连接再展开才触发 introspect（懒加载语义不变）。
    if let Some((group, _)) = grouped(config.kinds).first() {
        let root = group_id_row(*group).id;
        state.borrow_mut().tree.expand(&root);
    }

    let search =
        cx.new(|cx| InputState::new(window, cx).placeholder("搜索表 / 视图 / 文件（Ctrl+F）"));
    let shell = cx.new(|cx| {
        PickerShell::new(
            state.clone(),
            shared.clone(),
            config,
            strategy.clone(),
            search.clone(),
            window,
            cx,
        )
    });

    // 打开即聚焦，键盘能直接用（否则要先点一下树）
    let handle = shell.read(cx).focus_handle.clone();
    window.open_dialog(cx, {
        let shell = shell.clone();
        move |dialog, _window, _cx| dialog.title(config.title).child(shell.clone())
    });
    window.focus(&handle, cx);
}
