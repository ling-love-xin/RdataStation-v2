//! 【2d】分析笔记本视图（T2d-3 / T2d-6）：单元卡片 + 空态引导
//!
//! ## 只做渲染，状态在共享层
//!
//! 本文件全是**纯渲染函数**：状态（单元序列 / 会话）在 [`crate::shared::EditorShared`]
//! 的笔记本持有器里，动作（新增 / 删除 / 上移下移 / 复制 / 运行）是面板
//! （`EditorHostPanel`）的方法——渲染函数拿到面板实体，按钮点击转发给面板。
//!
//! 单元正文在这个骨架切片里按**纯文本**渲染（编辑内核复用属后续；任务书风险条款
//! 记了「单元编辑器 = 内核实例复用 + ElementId 业务键」，不是本切片）。类型徽标两条
//! 语义：**颜色** = 可执行性（SQL = 主色 / 文本 = 次级色），**形状** = 语言
//! （SQL = `SELECT` 徽标 / 文本 = `M↓` 徽标，见 [`badge`]）。
//!
//! ## 渲染路径零 I/O
//!
//! render 纯读 `NotebookState` 快照；运行 / 会话切换都是事件路径（面板方法）+ 泵
//! （`drain_cell_notes`）落回执。零裸 px / 零裸色值：尺寸走 [`crate::ui`] 常量 + Tailwind
//! 尺度，取色走 `cx.theme().colors.*`。

use gpui_kit::base::Disableable as _;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::cell::{Cell, CellKind, CellState, NotebookState, Output};
use crate::model::DocumentId;
use crate::ui;
use crate::view::host::EditorHostPanel;

/// 面板实体（按钮动作都转发给它）
pub type PanelEntity = Entity<EditorHostPanel>;

/// 渲染整个笔记本正文（编辑区的内容；Analysis 模式用，其它模式不调）
///
/// `entity` 是面板实体（`EditorHostPanel`）：所有按钮动作都转发给它。
pub fn render(
    cx: &App,
    state: &NotebookState,
    document: &DocumentId,
    entity: PanelEntity,
) -> impl IntoElement {
    let theme = cx.theme();
    if state.cells.is_empty() {
        return empty_state(cx, document, entity).into_any_element();
    }
    let mut body = div()
        .v_flex()
        .w_full()
        .items_center()
        .gap_1()
        .py_2();
    let mut list = div().v_flex().w_full().max_w(rems(ui::NOTEBOOK_CELL_MAX_WIDTH));
    for (ix, cell) in state.cells.iter().enumerate() {
        list = list.child(cell_card(theme, cell, ix + 1, document, &entity));
    }
    // 底部「＋ 插入单元」（列表尾追加；空态另有引导卡）
    list = list.child(add_cell_row(document, &entity));
    body = body.child(list);
    body.into_any_element()
}

/// 底部插入行：一个「＋ 插入单元」入口（新增 SQL 单元；类型切换在 ⋮ 菜单里）
fn add_cell_row(document: &DocumentId, entity: &PanelEntity) -> impl IntoElement {
    let entity = entity.clone();
    let document = document.clone();
    div()
        .w_full()
        .h_flex()
        .items_center()
        .justify_center()
        .py_2()
        .child(
            Button::new("editor-nb-add-cell")
                .ghost()
                .small()
                .debug_selector(|| "editor-nb-add-cell".to_string())
                .label("＋ 插入单元")
                .on_click(move |_, _window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_add_cell(&document, CellKind::Sql, cx);
                    });
                }),
        )
        .into_any_element()
}

/// 空态引导卡（T2d-6 / 原型 §4.4）：没单元时不是空白，而是告诉用户笔记是什么、从哪开始
fn empty_state(
    cx: &App,
    document: &DocumentId,
    entity: PanelEntity,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.colors.muted_foreground;
    let entity = entity.clone();
    let document = document.clone();
    let add_sql = {
        let entity = entity.clone();
        let document = document.clone();
        Button::new("editor-nb-empty-sql")
            .small()
            .debug_selector(|| "editor-nb-empty-sql".to_string())
            .label("＋ SQL 单元")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| {
                    panel.notebook_add_cell(&document, CellKind::Sql, cx);
                });
            })
    };
    let add_text = {
        let entity = entity.clone();
        let document = document.clone();
        Button::new("editor-nb-empty-text")
            .with_variant(ButtonVariant::Secondary)
            .small()
            .debug_selector(|| "editor-nb-empty-text".to_string())
            .label("＋ 文本单元")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| {
                    panel.notebook_add_cell(&document, CellKind::Text, cx);
                });
            })
    };
    div()
        .w_full()
        .h_flex()
        .items_center()
        .justify_center()
        .py_4()
        .child(
            div()
                .v_flex()
                .w_full()
                .max_w(rems(ui::NOTEBOOK_EMPTY_MAX_WIDTH))
                .items_center()
                .gap_2()
                .p_4()
                .rounded_md()
                .border_1()
                .border_color(theme.colors.border)
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.colors.foreground)
                        .child("分析笔记"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child("笔记由「单元」组成：SQL 单元可执行（结果内联），文本单元做备注。"),
                )
                .child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .child(add_sql)
                        .child(add_text),
                ),
        )
}

/// 一张单元卡片：工具行（徽标 + 序号 + 动作）+ 正文 + 输出 / 元信息
fn cell_card(
    theme: &gpui_kit::component::Theme,
    cell: &Cell,
    seq: usize,
    document: &DocumentId,
    entity: &PanelEntity,
) -> impl IntoElement {
    let badge = badge(theme, cell.kind);
    let run_button = run_button(cell, document, entity);
    let menu_button = menu_button(cell, document, entity);

    // 元信息：序号 + 状态文案（运行中耗时 / stale 计数提示 / 成功失败原因）
    let state_text = match cell.state {
        CellState::Running => "运行中…".to_string(),
        CellState::Stale => "会话已切换，输出作废".to_string(),
        CellState::Error => "执行失败".to_string(),
        CellState::Done => "已执行".to_string(),
        CellState::Idle => seq.to_string(),
    };
    let state_color = match cell.state {
        CellState::Running => theme.colors.primary,
        CellState::Stale => theme.colors.warning,
        CellState::Error => theme.colors.danger,
        _ => theme.colors.muted_foreground,
    };

    div()
        .w_full()
        .v_flex()
        .rounded_md()
        .border_1()
        .border_color(theme.colors.border)
        .overflow_hidden()
        .child(
            div()
                .h_flex()
                .w_full()
                .items_center()
                .gap_1()
                .px_2()
                .py_1()
                .child(badge)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_xs()
                        .text_color(state_color)
                        .child(state_text),
                )
                .child(run_button)
                .child(menu_button),
        )
        .child(
            div()
                .w_full()
                .px_3()
                .py_2()
                .text_sm()
                .text_color(theme.colors.foreground)
                .font_family(theme.mono_font_family.clone())
                .whitespace_normal()
                .child(cell_body_text(cell)),
        )
        .when(cell.output.is_some(), |card| {
            card.child(output_section(theme, cell))
        })
}

/// 单元正文（本切片按纯文本渲染；编辑内核复用属后续）
fn cell_body_text(cell: &Cell) -> String {
    if cell.body.trim().is_empty() {
        if cell.kind.is_executable() {
            "（空的 SQL 单元——点 ▶ 会如实说没有可执行的语句）".to_string()
        } else {
            "（空文本单元）".to_string()
        }
    } else {
        cell.body.clone()
    }
}

/// 单元输出区：表格（内联 Table 复用 2a）／ 文本 / 错误卡片
fn output_section(
    theme: &gpui_kit::component::Theme,
    cell: &Cell,
) -> impl IntoElement {
    let Some(output) = &cell.output else {
        return div().into_any_element();
    };
    match output {
        Output::Table(entry) => {
            // 内联表格复用 2a 共用表格：本切片先以只读文本投影呈现行数（完整网格
            // 复用接线见 T2d-3 后续——吃 `ResultEntry` 的 `result_grid::render`）。
            let row_count = entry.rows.len();
            let col_count = entry.columns.len();
            div()
                .w_full()
                .px_3()
                .py_2()
                .border_t(ui::HAIRLINE)
                .border_color(theme.colors.border)
                .text_xs()
                .text_color(theme.colors.muted_foreground)
                .child(format!(
                    "结果：{row_count} 行 × {col_count} 列（{:.1} ms）",
                    entry.elapsed_ms as f64 / 1000.
                ))
                .into_any_element()
        }
        Output::Text(text) => div()
            .w_full()
            .px_3()
            .py_2()
            .border_t(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .text_xs()
            .text_color(theme.colors.foreground)
            .font_family(theme.mono_font_family.clone())
            .whitespace_normal()
            .child(text.clone())
            .into_any_element(),
        Output::Error(reason) => div()
            .w_full()
            .px_3()
            .py_2()
            .border_t(ui::HAIRLINE)
            .border_color(theme.colors.border)
            .text_xs()
            .text_color(theme.colors.danger)
            .whitespace_normal()
            .child(reason.clone())
            .into_any_element(),
    }
}

/// 类型徽标：颜色 = 可执行性（SQL = 主色 / 文本 = 次级色），形状 = 语言
fn badge(theme: &gpui_kit::component::Theme, kind: CellKind) -> impl IntoElement {
    let (label, color, bg) = match kind {
        CellKind::Sql => ("SQL", theme.colors.primary, theme.colors.primary.opacity(0.12)),
        CellKind::Text => ("M↓", theme.colors.muted_foreground, theme.colors.muted_foreground.opacity(0.12)),
    };
    div()
        .flex_none()
        .px_1()
        .py_0p5()
        .rounded_sm()
        .bg(bg)
        .text_xs()
        .text_color(color)
        .child(label)
}

/// ▶ 运行 / 中断：只有 SQL 单元可执行（Text 单元如实不亮）
fn run_button(
    cell: &Cell,
    document: &DocumentId,
    entity: &PanelEntity,
) -> impl IntoElement {
    let running = cell.state == CellState::Running;
    let entity = entity.clone();
    let document = document.clone();
    let id = cell.id.clone();
    let id_debug = id.clone();
    let label = if running { "■ 中断" } else { "▶" };
    let enabled = !running && cell.kind.is_executable() && !cell.body.trim().is_empty();
    Button::new(format!("editor-nb-run-{id}"))
        .ghost()
        .xsmall()
        .debug_selector(move || format!("editor-nb-run-{}", cell_kind_key(&id_debug)))
        .label(label)
        .disabled(!enabled)
        .on_click(move |_, _window, app| {
            if running {
                entity.update(app, |panel, cx| panel.notebook_cancel_run(cx));
            } else {
                entity.update(app, |panel, cx| {
                    panel.notebook_run_cell(&document, &id, cx);
                });
            }
        })
        .into_any_element()
}

/// ⋮ 菜单：上移 / 下移 / 复制 / 转文本 / 删除（全真实动作）
///
/// 用 `Button::dropdown_menu` 承载（与 host.rs 的环境选择器同款）：`PopupMenu` 的构造器
/// 在组件库里是 `pub(crate)`，editor 侧不能直接 new，只能走弹层菜单的 builder 闭包。
fn menu_button(
    cell: &Cell,
    document: &DocumentId,
    entity: &PanelEntity,
) -> impl IntoElement {
    let id = cell.id.clone();
    let id_debug = id.clone();
    let kind = cell.kind;
    let entity = entity.clone();
    let document = document.clone();
    Button::new(format!("editor-nb-menu-{id}"))
        .ghost()
        .xsmall()
        .debug_selector(move || format!("editor-nb-menu-{}", cell_kind_key(&id_debug)))
        .label("⋮")
        .dropdown_menu(move |menu, _window, _cx| {
            // 动作全部转发给面板；`PopupMenuItem` 闭包是 `Fn`，每项先克隆句柄
            let menu = menu;
            let up = PopupMenuItem::new("上移").on_click({
                let entity = entity.clone();
                let document = document.clone();
                let id = id.clone();
                move |_, _window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_move_cell(&document, &id, -1, cx);
                    });
                }
            });
            let down = PopupMenuItem::new("下移").on_click({
                let entity = entity.clone();
                let document = document.clone();
                let id = id.clone();
                move |_, _window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_move_cell(&document, &id, 1, cx);
                    });
                }
            });
            let duplicate = PopupMenuItem::new("复制单元").on_click({
                let entity = entity.clone();
                let document = document.clone();
                let id = id.clone();
                move |_, _window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_duplicate_cell(&document, &id, cx);
                    });
                }
            });
            let convert = PopupMenuItem::new(if kind == CellKind::Sql {
                "转文本单元"
            } else {
                "转 SQL 单元"
            })
            .on_click({
                let entity = entity.clone();
                let document = document.clone();
                let id = id.clone();
                move |_, _window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_convert_cell(&document, &id, cx);
                    });
                }
            });
            let delete = PopupMenuItem::new("删除单元").on_click({
                let entity = entity.clone();
                let document = document.clone();
                let id = id.clone();
                move |_, _window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_delete_cell(&document, &id, cx);
                    });
                }
            });
            menu.item(up)
                .item(down)
                .item(duplicate)
                .item(convert)
                .item(delete)
        })
        .into_any_element()
}

/// 测试用：单元 id 拼 debug_selector 的辅助（闭包捕获 id，与 `Button::new` 的格式错开）
fn cell_kind_key(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}
