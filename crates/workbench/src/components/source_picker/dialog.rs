//! 对话框外壳：根 tab、三分法主体（一棵树）、底部栏。
//!
//! 为什么从 `mod.rs` 挪出来：这是「一个模态长什么样」的全部代码，
//! 与状态 / 行模型 / 数据访问无关。混在一起时 `mod.rs` 永远是全模块最胖的文件。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::base::{Disableable as _, StyledExt};
use gpui_kit::component::button::Button;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, WindowExt as _};
use gpui_kit::*;

use crate::panels::Shared;

use super::db::{db_rows, ConnBrief};
use super::group::{grouped, SourceGroup};
use super::load::{collect_analysis_tables, collect_drafts, load_db_layer};
use super::node::{ArchiveScope, NodeId, NodeKind, Row};
use super::render::{render_rows, RowAction};
use super::rows::{analysis_table_rows, draft_rows, external_row, group_id_row};
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
    if note.is_none() && config.kinds.contains(&SourceKind::Analysis) && analysis.is_empty() {
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
