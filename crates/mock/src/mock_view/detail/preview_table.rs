//! 预览表格委托：预览结果集的列 / 行 / 排序 / 右键菜单。

use super::*;

pub(crate) struct PreviewTableDelegate {
    /// 面板实体（右键排序与表头排序都要把「这一列 + 方向」交给它去重查）
    pub(crate) panel: WeakEntity<MockPanel>,
    /// 本 tab 看的表名
    pub(crate) table: String,
    /// 数据列名（不含行号槽）
    pub(crate) columns: Vec<String>,
    /// 取样行（引擎已渲染成文本；每行长度 == `columns.len()`）
    pub(crate) rows: Vec<Vec<String>>,
    /// 生效中的排序（列下标 + 方向；`None` = 没排序）：表头的箭头就画在它上
    pub(crate) sort: Option<(usize, ColumnSort)>,
    /// 用户拖过的列宽（**按列名**记，不是下标）：重建表头时按名字恢复。
    ///
    /// 为什么必须自己记：列宽归组件（`col_groups` 是 `TableState` 的私有状态），
    /// 而每次重建表头（重查取样 / 重新生成）都会从 `column()` 重新取宽——不记就等于
    /// 把用户拖好的宽度打回默认档。事件 [`TableEvent::ColumnWidthsChanged`] 拖完就发，
    /// 订阅在详情视图那边（`ensure_preview_table`）。
    pub(crate) widths: HashMap<String, Pixels>,
    /// 右键落在哪个单元格（组件库只把 `row_ix` 交给 `context_menu`，
    /// 列得在 `render_td` 里自己记一笔——与结果集网格同一做法）
    pub(crate) context_cell: Option<(usize, usize)>,
}

impl PreviewTableDelegate {
    /// 首列是行号槽（与结果集同形：`#` 钉在左侧，不可拖宽 / 移动 / 选中）
    fn is_row_number(col_ix: usize) -> bool {
        col_ix == 0
    }

    /// 建 delegate（列 / 行 / 排序一次到位：`TableState::new` 当场就会问 `column`）。
    pub(crate) fn new(panel: WeakEntity<MockPanel>, snapshot: PreviewSnapshot) -> Self {
        let PreviewSnapshot {
            table,
            columns,
            rows,
            sort,
        } = snapshot;
        let sort = Self::map_sort(&columns, sort);
        Self {
            panel,
            table,
            columns,
            rows,
            sort,
            widths: HashMap::new(),
            context_cell: None,
        }
    }

    /// 记下组件报来的全部列宽（下标 → 列名；行号槽的宽是钉死的，不记）。
    ///
    /// 列集合变了就把不再存在的名字剔掉（同一次会话里换过表也不会无限长）。
    pub(crate) fn set_widths(&mut self, all: &[Pixels]) {
        let mut widths: HashMap<String, Pixels> = HashMap::new();
        for (ix, width) in all.iter().enumerate() {
            let Some(index) = ix.checked_sub(1) else {
                continue;
            };
            let Some(name) = self.columns.get(index) else {
                continue;
            };
            widths.insert(name.clone(), *width);
        }
        self.widths = widths;
    }

    /// 面板给的「生效中的排序」（列名 + 方向）→ 列下标 + 方向。
    ///
    /// 列名对不上（列被换了）就当没有排序：箭头不画，也不去重查一个不存在的列。
    fn map_sort(columns: &[String], sort: Option<(String, bool)>) -> Option<(usize, ColumnSort)> {
        let (column, descending) = sort?;
        let index = columns.iter().position(|name| name == &column)?;
        Some((
            index,
            if descending {
                ColumnSort::Descending
            } else {
                ColumnSort::Ascending
            },
        ))
    }

    /// 换一份快照；返回是否真的变了（没变就不必 `refresh`：生成中面板每 120ms 通知一次）。
    ///
    /// 排序也是快照的一部分：面板拒绝重查时会把**上一份**排序回推过来，这里就会看到它与
    /// 表头刚才翻到的方向不一致（`perform_sort` 先照抄了一份）——于是 `refresh` 把箭头纠回来。
    pub(crate) fn set_preview(&mut self, snapshot: PreviewSnapshot) -> bool {
        let PreviewSnapshot {
            table,
            columns,
            rows,
            sort,
        } = snapshot;
        let sort = Self::map_sort(&columns, sort);
        if self.table == table && self.columns == columns && self.rows == rows && self.sort == sort
        {
            return false;
        }
        // 列集合换了：与它们对不上的记录宽度没有意义（留着只会在同名列回来时意外生效）
        if self.columns != columns {
            self.widths.retain(|name, _| columns.contains(name));
        }
        self.table = table;
        self.columns = columns;
        self.rows = rows;
        self.sort = sort;
        // 数据换了，之前记下的右键位置不再指向同一个值
        self.context_cell = None;
        true
    }

    /// 这一行拼成 TSV（「复制整行」用：拷进表格 / SQL 都能直接分列）。
    ///
    /// 逐格转义走 `shared::string::tsv_row`（**唯一实现**，结果集网格的右键复制也调它）：
    /// 取值里的制表符不转义就会把一行错成两列——“粘进表格就能直接分列”那时就不成立了。
    pub(crate) fn row_text(&self, row_ix: usize) -> String {
        self.rows
            .get(row_ix)
            .map(|row| tsv_row(row))
            .unwrap_or_default()
    }

    /// 右键那一格的数据列下标：优先用记下的单元格（行号槽除外），否则退到第一列。
    pub(crate) fn context_col(&self, row_ix: usize) -> Option<usize> {
        match self.context_cell {
            Some((cell_row, cell_col)) if cell_row == row_ix && cell_col > 0 => Some(cell_col - 1),
            _ if !self.columns.is_empty() => Some(0),
            _ => None,
        }
    }

    /// 右键菜单里拿得到的那一格。
    pub(crate) fn context_text(&self, row_ix: usize) -> Option<String> {
        let col = self.context_col(row_ix)?;
        self.rows.get(row_ix)?.get(col).cloned()
    }

    /// 右键那一格的列名（排序菜单项要它）。
    pub(crate) fn context_column(&self, row_ix: usize) -> Option<String> {
        let col = self.context_col(row_ix)?;
        self.columns.get(col).cloned()
    }

    /// 这一数据列在**取样窗口内**的全部取值（每行一个，拿去用换行拼成单列 TSV）。
    ///
    /// 只给取样里的值、不重查整列：与「预览是取样」同一口径；而一张十万行的表整列
    /// 可能是十万个值，粘到哪里都不好用（真要全列，落库 / 导出后去查那张表）。
    ///
    /// 返回的每个取值**已经按 TSV 规则转义**（`shared::string::tsv_cell`，与结果集网格
    /// 「复制此列」同一份实现）：取值自身带换行时不转义就会被拼成两行。
    pub(crate) fn column_text(&self, index: usize) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|row| row.get(index))
            .map(|cell| tsv_cell(cell))
            .collect()
    }
}

impl TableDelegate for PreviewTableDelegate {
    fn columns_count(&self, _cx: &App) -> usize {
        // 没列时不画行号槽（空表就交给「（暂无预览）」那条空态）
        if self.columns.is_empty() {
            0
        } else {
            self.columns.len() + 1
        }
    }

    fn rows_count(&self, _cx: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _cx: &App) -> Column {
        if Self::is_row_number(col_ix) {
            return Column::new("row-no", "#")
                .width(ui::PREVIEW_ROW_NUMBER_WIDTH)
                .min_width(ui::PREVIEW_ROW_NUMBER_WIDTH)
                .fixed_left()
                .resizable(false)
                .movable(false)
                .selectable(false);
        }
        let index = col_ix - 1;
        let name = self.columns.get(index).cloned().unwrap_or_default();
        // 列宽：拖过的按列名恢复，没拖过的走组件默认档（100px）+ 可拖宽——
        // 宽表靠横向滚动，看不全就拖
        let mut column = Column::new(format!("col-{index}"), name.clone());
        if let Some(width) = self.widths.get(&name) {
            column = column.width(*width);
        }
        // 可排序：表头挂排序箭头，点一下就把「这一列 + 方向」交给面板去重查。
        // 生效中的那一列把方向画出来（↑/↓），其余画「可点」的默认箭头。
        match self.sort {
            Some((sorted_ix, sort)) if sorted_ix == index => column.sort(sort),
            _ => column.sortable(),
        }
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let fg = cx.theme().colors.foreground;
        let text = self.cell_text(row_ix, col_ix, cx);
        // 行号槽永远是灰的（它不是数据）
        let is_row_number = Self::is_row_number(col_ix);
        // `NULL` 是 SQL 里的一个真实值：灰 + 斜体，与字符串 "NULL" 区分开（与结果集网格同口径）
        let is_null = !is_row_number && text == "NULL";
        let color = if is_row_number || is_null { muted } else { fg };
        // 右键得先知道「点的是哪一格」：组件库只把 `row_ix` 交给 `context_menu`
        // （事件从最内层派发，不会丢）——与结果集网格同一做法
        let table = cx.entity().clone();
        let mut cell = div()
            .id((
                "mock-preview-cell",
                row_ix * (self.columns.len() + 1) + col_ix,
            ))
            .px_2()
            .truncate()
            .text_xs()
            .text_color(color)
            .when(is_null, |cell| cell.italic())
            // 行号槽的单元格挂 `h_full` + 选择器：用例按它量**行距**（表格密度那条口径的哨兵，
            // 与结果集网格同一条用例；`.id()` 不登记坐标）
            .when(is_row_number, |cell| {
                cell.h_full()
                    .debug_selector(move || format!("mock-preview-rowno-{row_ix}"))
            })
            .when(!is_row_number, |cell| {
                cell.on_mouse_down(MouseButton::Right, move |_, _window, app| {
                    table.update(app, |state, _cx| {
                        state.delegate_mut().context_cell = Some((row_ix, col_ix));
                    });
                })
            });
        // 取值可能很宽（JSON / 长文本），`truncate` 之后就只剩悬停这一条看全的路。
        //
        // 为什么要 `id`：gpui 的**流式** `tooltip` 声明在 `StatefulInteractiveElement` 上
        // （没 id 的裸 `div` 只有直调 `Interactivity` 的写法）；组件库自己的表头/行头单元格
        // 也是这么挂 id 的。行号槽不给悬停提示：那是序号，不是数据。
        if !is_row_number && !text.is_empty() {
            let full = text.clone();
            cell = cell.tooltip(move |window, cx| preview_cell_tooltip(full.clone(), window, cx));
        }
        cell.child(text)
    }

    /// 表头单元格：组件默认只画列名（`div().size_full().child(name)`），这里补两件事——
    /// **截断**与**悬停全文**。
    ///
    /// 为什么值得自己画：列名常常比默认列宽（100px）长，截断之后一样没有任何办法知道它是什么；
    /// 单元格取值已经有悬停全文（`preview_cell_tooltip`），表头同样需要。
    /// 行号槽（`#`）不挂悬停——它不是数据。
    fn render_th(
        &mut self,
        col_ix: usize,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let name = if Self::is_row_number(col_ix) {
            "#".to_string()
        } else {
            self.columns.get(col_ix - 1).cloned().unwrap_or_default()
        };
        // `debug_selector` 是给用例找这个表头用的（`.id(...)` 不登记坐标）：
        // 「点列头还能排序」靠真点击验证，不靠直接调 `perform_sort`。
        let head = div()
            .size_full()
            .truncate()
            .debug_selector(move || format!("mock-preview-th-{col_ix}"))
            .child(name.clone());
        if Self::is_row_number(col_ix) || name.is_empty() {
            return head.into_any_element();
        }
        head.id(("mock-preview-th", col_ix))
            .tooltip(move |window, cx| preview_cell_tooltip(name.clone(), window, cx))
            .into_any_element()
    }

    /// 表头的**排序箭头**点击 → 按这一列**重查临时表**（不是把手上这几行换个顺序）。
    ///
    /// 入口是表头右端那个箭头，不是表头正文：组件 0.7.1 的 `on_col_head_click` 只做列选择
    /// （我们关掉了），排序只挂在 `render_sort_icon` 上——与结果集网格同一处入口。
    ///
    /// 组件库已经把方向循环算好了（默认 → 降序 → 升序 → 默认）并先改了自己的箭头，
    /// 这里做两件事：
    /// ① 把这个方向照抄进 delegate——面板那一拍若拒绝（内存库忙 / 列不存在），
    ///    它会把**上一份**排序回推过来，`set_preview` 就会看到差别并 `refresh` 把箭头纠回实际状态；
    /// ② 把「这一列 + 方向」交给面板：重查是 I/O，而临时表名与宿主端口都在它手里。
    ///
    /// 「默认」那一档（点第三下）= 取消排序，回到生成时的前 N 行。
    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _window: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        // 行号槽（`Column` 没开 sortable）不会走到这里；真到了也只当没这回事
        let Some(index) = col_ix.checked_sub(1) else {
            return;
        };
        let Some(column) = self.columns.get(index).cloned() else {
            return;
        };
        self.sort = Some((index, sort));
        let panel = self.panel.upgrade();
        let Some(panel) = panel else {
            return;
        };
        let table = self.table.clone();
        let descending = match sort {
            ColumnSort::Ascending => Some(false),
            ColumnSort::Descending => Some(true),
            ColumnSort::Default => None,
        };
        panel.update(cx, |panel, cx| {
            panel.sort_preview(&table, &column, descending, cx)
        });
    }

    /// 预览的右键菜单：这一格能做的事——按这一列重排取样 / 复制（这一格 · 这一列 · 整行）。
    ///
    /// 排序进菜单的理由：右键落点就是「这一列」，比表头那个小箭头好找。
    /// 它走的是与表头点击同一条路：**重查**临时表取「按该列排序后的前 N 行」
    /// （见 [`MockPanel::sort_preview`]），不是就地重排手上这几行。
    ///
    /// 仍然不做筛选（结果集网格有）：筛选把取样变成长度未知的子集，
    /// 而「前 N 行长什么样」正是这份取样唯一的信息。
    fn context_menu(
        &mut self,
        row_ix: usize,
        menu: PopupMenu,
        _window: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> PopupMenu {
        let mut menu = menu;
        if let Some(column) = self.context_column(row_ix) {
            let panel = self.panel.clone();
            let table = self.table.clone();
            menu = menu
                .item(preview_sort_menu_item(
                    format!("按「{column}」升序"),
                    panel.clone(),
                    table.clone(),
                    column.clone(),
                    Some(false),
                ))
                .item(preview_sort_menu_item(
                    format!("按「{column}」降序"),
                    panel.clone(),
                    table.clone(),
                    column.clone(),
                    Some(true),
                ));
            // 取消排序只在真有排序时出现（没排序时它只是个噪声项）
            if self.sort.is_some() {
                menu = menu.item(preview_sort_menu_item(
                    "取消排序".to_string(),
                    panel,
                    table,
                    column,
                    None,
                ));
            }
        }
        if let Some(value) = self.context_text(row_ix) {
            menu = menu.item(
                PopupMenuItem::new("复制此值").on_click(move |_, _window, app| {
                    app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(value.clone()));
                }),
            );
        }
        // 整列：取样窗口内的全部取值（一行一个，逐格已按 TSV 转义）——粘进表格就是一列
        if let Some(index) = self.context_col(row_ix) {
            let values = self.column_text(index);
            if !values.is_empty() {
                let label = format!("复制此列（{} 个取值）", values.len());
                let text = values.join("\n");
                menu = menu.item(PopupMenuItem::new(label).on_click(move |_, _window, app| {
                    app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.clone()));
                }));
            }
        }
        let row_text = self.row_text(row_ix);
        if !row_text.is_empty() {
            menu = menu.item(PopupMenuItem::new("复制整行（TSV）").on_click(
                move |_, _window, app| {
                    app.write_to_clipboard(gpui_kit::ClipboardItem::new_string(row_text.clone()));
                },
            ));
        }
        menu
    }

    /// 取样的文本表示：组件库用它做 `dump` / 导出（预览的取值本来就是文本，直接给）。
    fn cell_text(&self, row_ix: usize, col_ix: usize, _cx: &App) -> String {
        if Self::is_row_number(col_ix) {
            return (row_ix + 1).to_string();
        }
        self.rows
            .get(row_ix)
            .and_then(|row| row.get(col_ix - 1))
            .cloned()
            .unwrap_or_default()
    }
}

/// 单元格的悬停全文：取值可能很宽（JSON / 长文本），`truncate` 之后只剩这一条看全的路。
///
/// 宽度走 `ui::PREVIEW_TOOLTIP_MAX_WIDTH`（结构尺寸只认常量表），并让它**折行**：
/// 悬停提示跟着鼠标，横向越宽越容易被窗口边缘截掉。
fn preview_cell_tooltip(text: String, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, _| {
        div()
            .max_w(rems(ui::PREVIEW_TOOLTIP_MAX_WIDTH))
            .whitespace_normal()
            .text_xs()
            .child(text.clone())
    })
    .build(window, cx)
}

/// 右键菜单里的排序项：与表头点击同一条路（把「这一列 + 方向」交给面板去重查临时表）。
///
/// `descending = None` = 取消排序（回到生成时的前 N 行）。
fn preview_sort_menu_item(
    label: String,
    panel: WeakEntity<MockPanel>,
    table: String,
    column: String,
    descending: Option<bool>,
) -> PopupMenuItem {
    PopupMenuItem::new(label).on_click(move |_, _window, app| {
        let Some(panel) = panel.upgrade() else {
            return;
        };
        panel.update(app, |panel, cx| {
            panel.sort_preview(&table, &column, descending, cx)
        });
    })
}

// ==================== 中央 tab 的面板协议 ====================
