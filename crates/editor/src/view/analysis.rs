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
use gpui_kit::base::text::TextView;
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
use gpui_kit::component::Sizable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::cell::{Cell, CellLanguage, CellState, NotebookState, Output};
use crate::connection::ConnectionOption;

use std::collections::HashMap;
use std::sync::Arc;

/// 内联网格单次最多渲染的行数（超出只显示前 N 行——单元格输出不是结果区，
/// 上千行表格在这里只会把笔记撑到没法看；完整浏览去 SQL 结果区）
const ANALYSIS_INLINE_MAX_ROWS: usize = 200;
use crate::model::DocumentId;
use crate::ui;
use crate::view::host::EditorHostPanel;

/// 面板实体（按钮动作都转发给它）
pub type PanelEntity = Entity<EditorHostPanel>;

/// 渲染整个笔记本正文（编辑区的内容；Analysis 模式用，其它模式不调）
///
/// `entity` 是面板实体（`EditorHostPanel`）：所有按钮动作都转发给它。
/// `cell_editors` 是面板已建好的单元编辑器实例（键 = cell_id）：render 是纯读路径，
/// **不能 read 面板实体**（面板正在被 render = 正在被 update，`Entity::read` 会重入
/// panic）——实例由面板按引用传进来。
/// `connections` 是连接列表的内存快照（右下角执行源选择器的数据源；渲染路径可调）
/// `kernels` 是内核注册表快照（执行源「内核」档的菜单数据源；渲染路径可调）
pub fn render(
    cx: &App,
    state: &NotebookState,
    document: &DocumentId,
    entity: PanelEntity,
    cell_editors: &HashMap<String, Entity<EditorState>>,
    connections: &[ConnectionOption],
    kernels: &[rds_kernel::KernelSpec],
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
        list = list.child(cell_card(
            cx,
            theme,
            cell,
            ix + 1,
            document,
            &entity,
            cell_editors,
            connections,
            kernels,
        ));
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
                .on_click(move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_add_cell(&document, CellLanguage::Sql, window, cx);
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
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| {
                    panel.notebook_add_cell(&document, CellLanguage::Sql, window, cx);
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
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| {
                    panel.notebook_add_cell(&document, CellLanguage::Markdown, window, cx);
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

/// 一张单元卡片：工具行（徽标 + 序号 + 动作）+ 正文 + 输出 + 右下角指示器
fn cell_card(
    _cx: &App,
    theme: &gpui_kit::component::Theme,
    cell: &Cell,
    seq: usize,
    document: &DocumentId,
    entity: &PanelEntity,
    cell_editors: &HashMap<String, Entity<EditorState>>,
    connections: &[ConnectionOption],
    kernels: &[rds_kernel::KernelSpec],
) -> impl IntoElement {
    let badge = badge(theme, cell.language);
    let run_button = run_button(cell, document, entity);
    let menu_button = menu_button(cell, document, entity);

    // 【遗留 #10】单元正文的内核编辑器实例（打开 / 新建时由面板建好；
    // 渲染期只读取——没有实体就回退纯文本，不在这里创建）
    let cell_editor = cell_editors.get(&cell.id).cloned();

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
                // 先 `when_none`（吃引用，不 move）再 `when_some`（吃值）：
                // 两个分支互斥，顺序只是子元素追加次序
                .when_none(&cell_editor, |body| {
                    // 回退：编辑器实例没建出来时按纯文本展示（不如实，不假装可编辑）
                    body.child(
                        div()
                            .px_3()
                            .py_2()
                            .text_sm()
                            .text_color(theme.colors.foreground)
                            .font_family(theme.mono_font_family.clone())
                            .whitespace_normal()
                            .child(cell_body_text(cell)),
                    )
                })
                .when_some(cell_editor, |body, editor| {
                    // 内核编辑器（#10）：定高 + 无边框融入卡片；语言在面板创建
                    // EditorState 时按单元类型设好（SQL / markdown）。
                    // 外层 div 带业务键 id + test_support：headless 测试据此定位 / 点击
                    body.child(
                        div()
                            .id(format!("editor-nb-cell-source-{}", cell.id))
                            .test_support()
                            .child(
                                Editor::new(&editor)
                                    .bordered(false)
                                    .h(rems(ui::NOTEBOOK_CELL_EDITOR_HEIGHT)),
                            ),
                    )
                }),
        )
        .when(!cell.outputs.is_empty(), |card| {
            card.child(output_section(theme, cell))
        })
        // 右下角指示器（对齐 VS Code Jupyter：语言 + 执行源，可点切换）
        .child(cell_footer(theme, cell, document, entity, connections, kernels))
}

/// 右下角指示器：语言标签 + 执行源（连接 / 内核）
///
/// - SQL / Python / R / Julia：执行源是一个可点按钮（弹出切换菜单）；
/// - Markdown：只显示语言（不可执行，无执行源概念）。
fn cell_footer(
    theme: &gpui_kit::component::Theme,
    cell: &Cell,
    document: &DocumentId,
    entity: &PanelEntity,
    connections: &[ConnectionOption],
    kernels: &[rds_kernel::KernelSpec],
) -> impl IntoElement {
    let muted = theme.colors.muted_foreground;
    // 语言标签（左下）：与工具行徽标一致的颜色语义
    let language_chip = div()
        .text_xs()
        .text_color(muted)
        .child(cell.language.label());

    // 执行源（右下）：当前值文案 + 切换菜单
    let source_button = match cell.language.runs_on() {
        None => None, // Markdown：无执行源
        Some(_) => {
            let label = execution_source_label(cell, connections, kernels);
            let menu = execution_source_menu(cell, document, entity, connections, kernels);
            Some(
                Button::new(format!("editor-nb-source-{}", cell.id))
                    .ghost()
                    .xsmall()
                    .debug_selector(move || format!("editor-nb-source-{}", cell_kind_key(&cell.id.clone())))
                    .label(label)
                    .dropdown_menu(menu),
            )
        }
    };

    div()
        .w_full()
        .h_flex()
        .items_center()
        .justify_end()
        .gap_2()
        .px_3()
        .py_1()
        .border_t(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .child(language_chip)
        .when_some(source_button, |footer, button| footer.child(button))
        .into_any_element()
}

/// 执行源当前值文案（`None` = 会话默认：SQL 用会话绑定连接 / 内核用上次用过的）
fn execution_source_label(
    cell: &Cell,
    connections: &[ConnectionOption],
    kernels: &[rds_kernel::KernelSpec],
) -> String {
    match &cell.execution_source {
        None => match cell.language.runs_on() {
            Some(crate::cell::ExecutionTarget::Connection) => "默认连接".to_string(),
            Some(crate::cell::ExecutionTarget::Kernel) => "默认内核".to_string(),
            None => String::new(),
        },
        Some(crate::cell::ExecutionSource::Connection(id)) => connections
            .iter()
            .find(|c| &c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| format!("连接 {id}")),
        Some(crate::cell::ExecutionSource::Kernel(id)) => kernels
            .iter()
            .find(|spec| &spec.id == id)
            .map(|spec| spec.display_name.clone())
            .unwrap_or_else(|| format!("内核 {id}")),
    }
}

/// 执行源切换菜单（数据源：连接 = 宿主注入的连接列表；内核 = 注册表快照）
fn execution_source_menu(
    cell: &Cell,
    document: &DocumentId,
    entity: &PanelEntity,
    connections: &[ConnectionOption],
    kernels: &[rds_kernel::KernelSpec],
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    let cell_id = cell.id.clone();
    let entity = entity.clone();
    let document = document.clone();
    let runs_on = cell.language.runs_on();
    let language = cell.language;
    let current = cell.execution_source.clone();
    // 连接 / 内核列表 clone 进闭包（dropdown_menu 要 `'static`；渲染路径的内存快照
    // 本来就该在宿主侧生成，这里只多一次拷贝，量级很小）
    let conns: Vec<ConnectionOption> = connections.to_vec();
    let kernels: Vec<rds_kernel::KernelSpec> = kernels.to_vec();
    move |menu, _window, _cx| {
        // 第一项：回到会话默认
        let default = PopupMenuItem::new(match runs_on {
            Some(crate::cell::ExecutionTarget::Connection) => "默认（当前会话连接）",
            Some(crate::cell::ExecutionTarget::Kernel) => "默认（上次用过的内核）",
            None => "默认",
        })
        .checked(current.is_none())
        .on_click({
            let entity = entity.clone();
            let document = document.clone();
            let cell_id = cell_id.clone();
            move |_, _window, app| {
                entity.update(app, |panel, cx| {
                    panel.notebook_set_execution_source(&document, &cell_id, None, cx);
                });
            }
        });
        let menu = menu.item(default);

        match runs_on {
            Some(crate::cell::ExecutionTarget::Connection) => {
                // 已指定的连接置勾选态；连接列表是宿主快照（渲染路径只读）
                let mut menu = menu;
                for conn in &conns {
                    let selected = matches!(
                        &current,
                        Some(crate::cell::ExecutionSource::Connection(id)) if id == &conn.id
                    );
                    let item = PopupMenuItem::new(conn.name.clone())
                        .checked(selected)
                        .on_click({
                            let entity = entity.clone();
                            let document = document.clone();
                            let cell_id = cell_id.clone();
                            let conn_id = conn.id.clone();
                            move |_, _window, app| {
                                entity.update(app, |panel, cx| {
                                    panel.notebook_set_execution_source(
                                        &document,
                                        &cell_id,
                                        Some(crate::cell::ExecutionSource::Connection(conn_id.clone())),
                                        cx,
                                    );
                                });
                            }
                        });
                    menu = menu.item(item);
                }
                menu
            }
            Some(crate::cell::ExecutionTarget::Kernel) => {
                // 内核注册表（#20）：ready 的可选、未 ready 置灰 + 原因；一个都没有时
                // 说明本机没有该语言的可用内核（「默认」保持可点，执行时给完整原因）
                let mut menu = menu;
                for spec in &kernels {
                    let selected = matches!(
                        &current,
                        Some(crate::cell::ExecutionSource::Kernel(id)) if id == &spec.id
                    );
                    // 未 ready 的内核：原因拼进 label（PopupMenuItem 无 suffix 槽位，
                    // 置灰 + 原因已足够——用户知道要装什么）
                    let label = match (spec.ready, spec.note) {
                        (false, Some(note)) => format!("{}（{}）", spec.display_name, note),
                        _ => spec.display_name.clone(),
                    };
                    let item = PopupMenuItem::new(label)
                        .checked(selected)
                        .disabled(!spec.ready)
                        .on_click({
                            let entity = entity.clone();
                            let document = document.clone();
                            let cell_id = cell_id.clone();
                            let kernel_id = spec.id.clone();
                            move |_, _window, app| {
                                entity.update(app, |panel, cx| {
                                    panel.notebook_set_execution_source(
                                        &document,
                                        &cell_id,
                                        Some(crate::cell::ExecutionSource::Kernel(
                                            kernel_id.clone(),
                                        )),
                                        cx,
                                    );
                                });
                            }
                        });

                    menu = menu.item(item);
                }
                if kernels.is_empty() {
                    let placeholder = PopupMenuItem::new(format!(
                        "{} 内核未检测到（本机没有可用内核）",
                        language.label()
                    ))
                    .disabled(true);
                    menu = menu.item(placeholder);
                }
                menu
            }
            None => menu,
        }
    }
}

/// 单元正文（本切片按纯文本渲染；编辑内核复用属后续）
fn cell_body_text(cell: &Cell) -> String {
    if cell.body.trim().is_empty() {
        if cell.language.is_executable() {
            "（空的 SQL 单元——点 ▶ 会如实说没有可执行的语句）".to_string()
        } else {
            "（空文本单元）".to_string()
        }
    } else {
        cell.body.clone()
    }
}

/// 单元输出区：**逐个渲染**输出（多输出语义：一次执行可有 stdout 流 + 富输出，
/// 顺序 = 到达顺序；每个输出独立成块，之间用顶部分隔线区分）
fn output_section(
    theme: &gpui_kit::component::Theme,
    cell: &Cell,
) -> impl IntoElement {
    let mut body = div().w_full().v_flex();
    for (ix, output) in cell.outputs.iter().enumerate() {
        body = body.child(render_output(theme, output, ix));
    }
    body.into_any_element()
}

/// 单个输出渲染：按输出类型分发（表格 / 文本 / Markdown / 图片 / 错误）。
/// 每个输出块带业务键 id + test_support（headless 测试据此定位；`ix` 是输出序号，
/// 一块内唯一即可，不是单元身份）
fn render_output(
    theme: &gpui_kit::component::Theme,
    output: &Output,
    ix: usize,
) -> impl IntoElement {
    let block = div()
        .id(format!("editor-nb-output-{ix}"))
        .test_support();
    match output {
        Output::Table(entry) => {
            let row_count = entry.rows.len();
            let col_count = entry.columns.len();
            block.child(render_inline_table(
                theme,
                &entry.columns,
                &entry.rows,
                Some(format!(
                    "结果：{row_count} 行 × {col_count} 列（{:.1} ms）",
                    entry.elapsed_ms as f64 / 1000.
                )),
            ))
        }
        Output::TableData { columns, rows } => {
            // HTML 表格解析产物（pandas）：没有结果区字段，纯列 + 行
            block.child(render_inline_table(theme, columns, rows, None))
        }
        Output::Text(text) => block.child(
            div()
                .w_full()
                .px_3()
                .py_2()
                .border_t(ui::HAIRLINE)
                .border_color(theme.colors.border)
                .text_xs()
                .text_color(theme.colors.foreground)
                .font_family(theme.mono_font_family.clone())
                .whitespace_normal()
                .child(text.clone()),
        ),
        Output::Markdown(source) => block.child(
            div()
                .w_full()
                .px_3()
                .py_2()
                .border_t(ui::HAIRLINE)
                .border_color(theme.colors.border)
                // TextView::markdown：自带滚动（scrollable）；id 用稳定键（渲染期
                // 不知道 cell id，markdown 源本身即可去重）
                .child(TextView::markdown("editor-nb-output-md", source).scrollable(true)),
        ),
        Output::Image { bytes, mime } => block.child(render_image(theme, bytes, mime)),
        Output::Error(reason) => block.child(
            div()
                .w_full()
                .px_3()
                .py_2()
                .border_t(ui::HAIRLINE)
                .border_color(theme.colors.border)
                .text_xs()
                .text_color(theme.colors.danger)
                .whitespace_normal()
                .child(reason.clone()),
        ),
    }
    .into_any_element()
}

/// 内联表格（SQL 结果集与 HTML 解析表格共用）：列头 + 行（行数上限 + 横向滚动）
fn render_inline_table(
    theme: &gpui_kit::component::Theme,
    columns: &[String],
    rows: &[Vec<String>],
    caption: Option<String>,
) -> AnyElement {
    let header = TableHeader::new().child(
        TableRow::new().children(columns.iter().map(|name| {
            TableHead::new().child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(name.clone()),
            )
        })),
    );
    let body = TableBody::new().children(
        rows.iter()
            .take(ANALYSIS_INLINE_MAX_ROWS)
            .map(|row| {
                TableRow::new().children(row.iter().map(|cell| {
                    TableCell::new().child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.foreground)
                            .child(cell.clone()),
                    )
                }))
            }),
    );
    div()
        .w_full()
        .px_3()
        .py_2()
        .border_t(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .when_some(caption, |card, caption| {
            card.child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child(caption),
            )
        })
        .child(
            div()
                .w_full()
                // 横向表格滚动走 Scrollable 组件（`overflow_x_scroll` 是 Stateful
                // 元素方法，普通 Div 没有；带滚动条版本顺带可被 headless 观察）
                .overflow_x_scrollbar()
                .child(
                    Table::new()
                        .accessibility_label("分析结果")
                        .child(header)
                        .child(body),
                ),
        )
        .into_any_element()
}

/// 图片输出（matplotlib 的 `image/png` 主力）：`Img` 元素 + Contain 缩放，
/// 限宽防撑破卡片（原图可能很大）
fn render_image(
    theme: &gpui_kit::component::Theme,
    bytes: &[u8],
    mime: &str,
) -> AnyElement {
    let format = match mime {
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        _ => ImageFormat::Png,
    };
    let source = ImageSource::Image(Arc::new(Image::from_bytes(format, bytes.to_vec())));
    div()
        .w_full()
        .px_3()
        .py_2()
        .border_t(ui::HAIRLINE)
        .border_color(theme.colors.border)
        .child(
            img(source)
                .object_fit(ObjectFit::Contain)
                .max_w(rems(ui::NOTEBOOK_OUTPUT_IMAGE_MAX_WIDTH)),
        )
        .into_any_element()
}

/// 语言徽标：颜色 = 可执行性（可执行 = 主色 / 备注 = 次级色），形状 = 语言
fn badge(theme: &gpui_kit::component::Theme, language: CellLanguage) -> impl IntoElement {
    let (label, color, bg) = match language {
        CellLanguage::Sql => ("SQL", theme.colors.primary, theme.colors.primary.opacity(0.12)),
        CellLanguage::Python => ("Py", theme.colors.primary, theme.colors.primary.opacity(0.12)),
        CellLanguage::R => ("R", theme.colors.primary, theme.colors.primary.opacity(0.12)),
        CellLanguage::Julia => ("JL", theme.colors.primary, theme.colors.primary.opacity(0.12)),
        CellLanguage::Markdown => ("M↓", theme.colors.muted_foreground, theme.colors.muted_foreground.opacity(0.12)),
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
    let enabled = !running && cell.language.is_executable() && !cell.body.trim().is_empty();
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
    let language = cell.language;
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
                move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_duplicate_cell(&document, &id, window, cx);
                    });
                }
            });
            let convert = PopupMenuItem::new(if language == CellLanguage::Sql {
                "转 Markdown 单元"
            } else {
                "转 SQL 单元"
            })
            .on_click({
                let entity = entity.clone();
                let document = document.clone();
                let id = id.clone();
                move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_convert_cell(&document, &id, window, cx);
                    });
                }
            });
            let delete = PopupMenuItem::new("删除单元").on_click({
                let entity = entity.clone();
                let document = document.clone();
                let id = id.clone();
                move |_, window, app| {
                    entity.update(app, |panel, cx| {
                        panel.notebook_delete_cell(&document, &id, window, cx);
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
