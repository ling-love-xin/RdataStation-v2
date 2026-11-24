//! 取值 / 复制 / 查找域（`grid/` 第四块）：单元格取值（`cell` / `row_cell_text`）、
//! 整列 / 整行 / JSON / INSERT 复制、结果内查找、选择复制文本、悬停全文与菜单预览。

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;

use super::delegate::ResultGridDelegate;
use super::stats::{
    ColumnStats, column_stats_from_values, group_by_values, number_group_key, stats_from_fact,
};
use crate::ports::FindCriteria;
use crate::rowset::RowRef;
use crate::ui;
use ::shared::FactKind;
use ::shared::string::{tsv_cell, tsv_row};
use std::ops::Range;
impl ResultGridDelegate {
    /// 单元格文案：首列是行号，其余按 `col_ix - 1` 取数据
    ///
    /// 行号与数据都走**视图行序**（筛选/排序之后看着的那一行）；
    /// 缺列/缺行给空串而不是 panic（结果行由驱动给出，形状不可全信）。
    pub(crate) fn cell(&self, row_ix: usize, col_ix: usize) -> String {
        if Self::is_row_number(col_ix) {
            return (row_ix + 1).to_string();
        }
        self.row_cell_text(row_ix, col_ix)
            .unwrap_or_default()
            .to_string()
    }

    /// 这一格的文本（**只借不拷**；`#` 行号槽与不存在的格都是 `None`）
    ///
    /// “这一格是什么”只此一份：`cell()` 画它、结果内查找搜它、将来类型化导出也读它。
    /// 行号槽不在这里（它不是一个数据格：显示的是视图行号，而视图行号由渲染现算）。
    /// 底层行从 `rows` 取，本地行从本地快照取（同一列序，不含隐藏列换算在这里完成）。
    pub(crate) fn row_cell_text(&self, row_ix: usize, col_ix: usize) -> Option<&str> {
        // 【P2-4】转置视图：单元格是「字段名 / 值」，走转置投影
        if self.transposed {
            return self.transpose_cell(row_ix, col_ix);
        }
        if Self::is_row_number(col_ix) {
            return None;
        }
        let column = self.data_column(col_ix - 1);
        match self.row_ref(row_ix)? {
            RowRef::Data(data_ix) => self.rows.get(data_ix)?.get(column).map(String::as_str),
            RowRef::Local(id) => self.local_cell(id, column),
        }
    }

    /// 本地行某一列的展示值（行 / 列身份取不到 = `None`；本地行未编辑的空格与已编辑
    /// 空格同口径走快照里的空串）
    pub(crate) fn local_cell(&self, id: u64, column: usize) -> Option<&str> {
        self.local
            .as_ref()?
            .local_row(id)?
            .cells
            .get(column)
            .map(String::as_str)
    }

    /// 结果内查找：从 `after` **之后**找下一处命中，找完一圈回到开头（`None` = 整份结果里没有）
    ///
    /// 口径由 [`FindMatcher`] 决定（默认 = 大小写不敏感子串，勾上「大小写 / 正则 / 全词」
    /// 后随之变化，对齐 DBeaver `SearchQuickFilterInfo`）；只看**看得见的行**
    /// （筛选 / 排序之后的视图行序——找到看不见的行等于没找到）；`#` 行号槽不是数据，不参与。
    ///
    /// `after = None` = 从第一格开始。区间：纯子串分支只在格与词**都是 ASCII** 时给出；
    /// 正则 / 全词分支由 regex 引擎给出合法字符边界的字节区间。
    ///
    /// 为什么是纯读：查找**不改任何状态**——选中哪一格、要不要滚过去、要不要画高亮都是宿主的
    /// 决定（宿主才知道焦点与滚动句柄）。这里只回答“下一处在哪”。
    pub fn find_next(
        &self,
        needle: &str,
        criteria: FindCriteria,
        after: Option<(usize, usize)>,
    ) -> Option<ResultMatch> {
        let matcher = FindMatcher::new(criteria, needle);
        let columns = self.columns.len();
        let rows = self.view_len();
        if matcher.is_empty() || columns == 0 || rows == 0 {
            return None;
        }
        // 把（视图行, 列）当成一条带子上的格子：搜一圈就是一条不大不小的扫描
        let per_row = columns + 1;
        let cells = rows * per_row;
        let start = match after {
            Some((row, col)) if col < per_row => {
                row.saturating_mul(per_row).saturating_add(col + 1) % cells
            }
            // 没给过起点，或起点已经不在这份结果里（刷新 / 换结果之后）：从头找
            _ => 0,
        };
        for step in 0..cells {
            let flat = (start + step) % cells;
            let (row, col) = (flat / per_row, flat % per_row);
            // 被删底层行不进搜索带（#28-B1 排除矩阵：搜索排除被删行）
            if self.is_deleted_row(row) {
                continue;
            }
            let Some(text) = self.row_cell_text(row, col) else {
                continue;
            };
            if let Some(range) = matcher.find_in(text) {
                return Some(ResultMatch { row, col, range });
            }
        }
        None
    }

    /// 结果内查找的命中总数（口径与 [`Self::find_next`] 同一份匹配器、
    /// 只看看得见的行、`#` 槽不算、被删行不算）。
    ///
    /// 用途是查找栏上的「N 个匹配」——DBeaver 的查找浮层也显示命中数
    /// （`ResultsetFindReplaceOverlay` + `SpreadsheetFindReplaceTarget` 的 findAll）。
    /// 纯读：不算选中、不滚屏。
    pub fn count_find_matches(&self, needle: &str, criteria: FindCriteria) -> usize {
        let matcher = FindMatcher::new(criteria, needle);
        if matcher.is_empty() {
            return 0;
        }
        let columns = self.columns.len();
        let mut count = 0;
        for row in 0..self.view_len() {
            if self.is_deleted_row(row) {
                continue;
            }
            for col in 1..=columns {
                if self
                    .row_cell_text(row, col)
                    .is_some_and(|text| matcher.matches(text))
                {
                    count += 1;
                }
            }
        }
        count
    }

    /// 结果内查找的**反向**走法：从 `before` 之前找上一处命中，到头回卷到最后
    /// （`None` = 整份结果里没有）。口径与 [`Self::find_next`] 完全对称。
    pub fn find_prev(
        &self,
        needle: &str,
        criteria: FindCriteria,
        before: Option<(usize, usize)>,
    ) -> Option<ResultMatch> {
        let matcher = FindMatcher::new(criteria, needle);
        let columns = self.columns.len();
        let rows = self.view_len();
        if matcher.is_empty() || columns == 0 || rows == 0 {
            return None;
        }
        let per_row = columns + 1;
        let cells = rows * per_row;
        // 起点：从 before 的前一格往回走；没给起点就从末尾往回走
        let start = match before {
            Some((row, col)) if col < per_row && row < rows => {
                let base = row.saturating_mul(per_row).saturating_add(col);
                (base + cells - 1) % cells
            }
            _ => cells - 1,
        };
        for step in 0..cells {
            let flat = (start + cells - (step % cells)) % cells;
            let (row, col) = (flat / per_row, flat % per_row);
            if self.is_deleted_row(row) {
                continue;
            }
            let Some(text) = self.row_cell_text(row, col) else {
                continue;
            };
            if let Some(range) = matcher.find_in(text) {
                return Some(ResultMatch { row, col, range });
            }
        }
        None
    }

    /// 这一数据列在**当前视图行序**下的全部取值（一行一个，换行分隔；逐格按 TSV 转义）
    ///
    /// 只给**已抓到的行**、不重查源库（与 mock 预览「复制此列」同一口径）：
    /// 一份十万行的结果整列就是十万个值，粘到哪里都不好用——真要全列就导出。
    /// 列浏览器清单「复制此列」与右键菜单共用这份文本。
    pub fn column_text(&self, column: usize) -> String {        let mut text = String::new();
        let mut first = true;
        for view_ix in 0..self.view_len() {
            let Some(reference) = self.row_ref(view_ix) else {
                continue;
            };
            let cell: String = match reference {
                RowRef::Data(data_ix) => {
                    // 被删行不参与复制列（#28-B1 排除矩阵）
                    if self
                        .local
                        .as_ref()
                        .is_some_and(|set| set.is_deleted(data_ix))
                    {
                        continue;
                    }
                    self.rows
                        .get(data_ix)
                        .and_then(|row| row.get(column))
                        .map(String::as_str)
                        .map(tsv_cell)
                        .unwrap_or_default()
                }
                RowRef::Local(id) => self
                    .local
                    .as_ref()
                    .and_then(|set| set.local_row(id))
                    .and_then(|row| row.cells.get(column))
                    .map(String::as_str)
                    .map(tsv_cell)
                    .unwrap_or_default(),
            };
            if !first {
                text.push('\n');
            }
            first = false;
            text.push_str(&cell);
        }
        text
    }

    /// 【P2-2】当前视图行序下，某数据列的全部取值：只 clone 目标列字符串，
    /// 不物化整行矩阵。计算页统计 / 分组页计数共用这份访问器。
    ///
    /// 【#28-B1 排除矩阵】统计 / 分组本期只覆盖底层未删行：本地行经 `data_row` 天然排除，
    /// 被删底层行在此跳过（D9：本地行纳入统计分组后置批次）。
    pub fn column_values(&self, column: usize) -> Vec<String> {
        (0..self.view_len())
            .filter_map(|view_ix| {
                let data_ix = self.data_row(view_ix)?;
                if self
                    .local
                    .as_ref()
                    .is_some_and(|set| set.is_deleted(data_ix))
                {
                    return None;
                }
                Some(
                    self.rows
                        .get(data_ix)
                        .and_then(|row| row.get(column))
                        .cloned()
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    /// 【D11】当前视图行序下，某数据列在**矩形选区的行区间**内的取值
    /// （对齐 DBeaver `AggregateColumnsPanel.aggregateSelection` `:231-241`：
    /// 按列收集「选区里属于该列的那些格」的值，逐列聚合）。
    ///
    /// 取值口径与 [`Self::column_values`] 完全一致（本地行 / 被删底层行同样跳过），
    /// 只是把行范围收窄到选区；越界的行由调用方先裁剪（这里只做闭区间遍历）。
    pub fn column_values_in_rows(&self, column: usize, rows: (usize, usize)) -> Vec<String> {
        let (top, bottom) = rows;
        (top..=bottom)
            .filter_map(|view_ix| {
                let data_ix = self.data_row(view_ix)?;
                if self
                    .local
                    .as_ref()
                    .is_some_and(|set| set.is_deleted(data_ix))
                {
                    return None;
                }
                Some(
                    self.rows
                        .get(data_ix)
                        .and_then(|row| row.get(column))
                        .cloned()
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    /// 【P2-2】计算页：某数据列统计（列级访问器 → 列级纯函数，口径与矩阵版一致）
    /// 【P2-2】计算页：某数据列统计。
    ///
    /// 【B5】内存档数值列走事实帧列式原生聚合（不经过显示字符串 parse）；无事实档、表档
    /// （batches 已释放）或文本 / 二进制 / 其他族回退显示矩阵口径，行为与起步档一致。
    pub fn column_stats_for(&self, column: usize) -> ColumnStats {
        // 被删行存在时，fact 原生整列聚合无法逐行排除（#28-B1 矩阵要求被删排除）：
        // 回退显示矩阵口径，由 [`Self::column_values`] 先把被删行滤掉再统计。
        let has_deleted = self
            .local
            .as_ref()
            .is_some_and(|set| set.deleted_count() > 0);
        if !has_deleted {
            if let Some(frame) = self.fact.as_ref() {
                if let Some(fact_stats) = frame.column_stats(column) {
                    return stats_from_fact(fact_stats);
                }
            }
        }
        column_stats_from_values(&self.column_values(column))
    }

    /// 【P2-2】分组页：某数据列本地分组计数（不物化整行矩阵）。
    ///
    /// 【B5】内存档 Int / Float 列按原生数值归一化键分组（整数统一写成整数字面量，避免
    /// `"1"` 与 `"1.0"` 被字符串等值拆成两组）；文本 / 二进制 / 其他族、表档（batches 已
    /// 释放）、无事实档回退显示矩阵的字符串等值口径。
    pub fn group_by_for(&self, column: usize) -> Vec<(String, usize)> {
        match self.column_group_keys(column) {
            // 原生键可用：直接按归一化键计数（不碰矩阵文本）
            Some(keys) => group_by_values(&keys),
            None => group_by_values(&self.column_values(column)),
        }
    }

    /// 【B5 / R3】某列的原生数值分组键：`Some` = 该列可走事实档原生口径（整数统一写成
    /// 整数字面量，`"1"` 与 `"1.0"` 不拆组）；`None` = 回退矩阵文本口径。
    ///
    /// 回退条件与 [`Self::column_stats_for`] 同源：非 Int / Float 列、无事实档（表档 batches
    /// 已释放）、存在被删行（矩阵与事实帧行号不再一一对应）、该列出现 NULL 或取不到原生值
    /// （避免原生键与文本键混排）。
    ///
    /// 行覆盖口径必须与 [`Self::column_values`] 一致（跳过本地行 / 被删行），否则多列分组时
    /// 原生键列与文本列长度不同、元组会按行号错位。
    fn column_group_keys(&self, column: usize) -> Option<Vec<String>> {
        let has_deleted = self
            .local
            .as_ref()
            .is_some_and(|set| set.deleted_count() > 0);
        if has_deleted {
            return None;
        }
        let frame = self.fact.as_ref()?;
        if !matches!(frame.column_kind(column), Ok(FactKind::Int | FactKind::Float)) {
            return None;
        }
        let mut keys: Vec<String> = Vec::with_capacity(self.view_len());
        for view_ix in 0..self.view_len() {
            // 本地行没有底层数据行：与 `column_values` 同口径跳过
            let Some(data_ix) = self.data_row(view_ix) else {
                continue;
            };
            keys.push(number_group_key(frame.column_number(data_ix, column)?));
        }
        Some(keys)
    }

    /// 【R3】分组页：按一组列分组（每列先取整列可见行键，再按行号拼元组）。
    ///
    /// - 0 列 = 空（UI 侧用当前选中列兜底）
    /// - 1 列：直接走 [`Self::group_by_for`]，保留内存档原生数值键归一化（`"1"` 与 `"1.0"`
    ///   不会被拆成两组）
    /// - 多列：**逐列**各自取键——能走事实档原生数值（内存档 Int / Float 且无被删行 / 无 NULL）
    ///   的列用归一化键（`2.0` 与 `2` 同组），其余列用显示文本；拼元组后交给
    ///   [`crate::grid::stats::group_by_tuples`]。DBeaver 多列分组同样按「列值」比较，
    ///   数值列不会因为显示文本不同而拆组
    pub fn group_by_columns_for(&self, columns: &[usize]) -> Vec<(Vec<String>, usize)> {
        match columns {
            [] => Vec::new(),
            [only] => self
                .group_by_for(*only)
                .into_iter()
                .map(|(value, count)| (vec![value], count))
                .collect(),
            _ => {
                let per_col: Vec<Vec<String>> = columns
                    .iter()
                    .map(|column| {
                        self.column_group_keys(*column)
                            .unwrap_or_else(|| self.column_values(*column))
                    })
                    .collect();
                // 各列长度同源（都按 `view_len` 取），取最小值防御越界
                let rows_count = per_col.iter().map(|values| values.len()).min().unwrap_or(0);
                let rows: Vec<Vec<String>> = (0..rows_count)
                    .map(|row_ix| {
                        per_col
                            .iter()
                            .map(|values| values[row_ix].clone())
                            .collect()
                    })
                    .collect();
                crate::grid::stats::group_by_tuples(&rows)
            }
        }
    }

    /// 这一行拼成 TSV（**视图行序**：复制的是用户看着的那一行，不是数据行序）；
    /// 本地行同样参与整行 TSV 复制（#28-B1 拍板 D7）。
    pub(crate) fn row_text(&self, row_ix: usize) -> String {
        // 【P2-4】转置一行 = 一个字段：字段名 + 值（菜单不摆复制项，这里是兜底）
        if self.transposed {
            let field = self.transpose_cell(row_ix, 1).unwrap_or_default();
            let value = self.transpose_cell(row_ix, 2).unwrap_or_default();
            return format!("{field}\t{value}");
        }
        let Some(reference) = self.row_ref(row_ix) else {
            return String::new();
        };
        // 被删底层行不参与整行复制（#28-B1 排除矩阵）
        if let RowRef::Data(data_ix) = reference {
            if self
                .local
                .as_ref()
                .is_some_and(|set| set.is_deleted(data_ix))
            {
                return String::new();
            }
        }
        tsv_row(&self.visible_row_values(reference))
    }

    /// 【P2】按可见列投影一行（隐藏列不参与复制 / 导出——用户看不到的不该拷走）；
    /// 行身份两类：底层行从 `rows` 取，本地行从本地快照取，同一套可见列序。
    pub(crate) fn visible_row_values(&self, reference: RowRef) -> Vec<String> {
        let visible: Vec<usize> = (0..self.columns.len())
            .filter(|d| !self.is_hidden_column(*d))
            .collect();
        match reference {
            RowRef::Data(data_ix) => visible
                .into_iter()
                .filter_map(|d| self.rows.get(data_ix)?.get(d).cloned())
                .collect(),
            RowRef::Local(id) => self
                .local
                .as_ref()
                .and_then(|set| set.local_row(id))
                .map(|row| {
                    visible
                        .iter()
                        .filter_map(|d| row.cells.get(*d).cloned())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    /// 【P2】可见列名（隐藏列不参与整行 JSON / INSERT）
    pub(crate) fn visible_column_names(&self) -> Vec<String> {
        (0..self.columns.len())
            .filter(|d| !self.is_hidden_column(*d))
            .filter_map(|d| self.columns.get(d).cloned())
            .collect()
    }

    /// 【A9】这一行拼成 JSON 对象（视图行序，与 [`Self::row_text`] 同口径；本地行参与复制）
    pub(crate) fn row_json_text(&self, row_ix: usize) -> String {
        let Some(reference) = self.row_ref(row_ix) else {
            return String::new();
        };
        // 被删底层行不参与行 JSON 复制（#28-B1 排除矩阵）
        if let RowRef::Data(data_ix) = reference {
            if self
                .local
                .as_ref()
                .is_some_and(|set| set.is_deleted(data_ix))
            {
                return String::new();
            }
        }
        crate::export::row_to_json(
            &self.visible_column_names(),
            &self.visible_row_values(reference),
        )
    }

    /// 【A9】这一行拼成 INSERT（视图行序；表名来自面板同步的 [`Self::result_table`]——
    /// 菜单摆出这一项时表名必然已确定，这里只是按当时的行取值）。
    ///
    /// 本地行不下发 INSERT：行级新增 / 删除本期只改本地网格，不进提交批次、不下发
    /// INSERT/DELETE（#28-B1 排除矩阵）；这里只认底层行身份。
    pub(crate) fn row_insert_text(&self, row_ix: usize) -> String {
        let Some(data_ix) = self.data_row(row_ix) else {
            return String::new();
        };
        // 被删底层行同样不拼 INSERT（#28-B1 排除矩阵：INSERT 排除被删行）
        if self
            .local
            .as_ref()
            .is_some_and(|set| set.is_deleted(data_ix))
        {
            return String::new();
        }
        let Some(table) = self.result_table.as_deref() else {
            return String::new();
        };
        crate::export::row_to_insert(
            table,
            &self.visible_column_names(),
            &self.visible_row_values(RowRef::Data(data_ix)),
        )
    }

    /// 【D10-5】选中内容的复制文本（`Ctrl+C` 用）：一格给**原文**，一整行 / 一整列给 TSV
    ///
    /// 与右键「复制此值 / 复制整行（TSV） / 复制整列」同一套口径与同一份实现
    /// （行走 [`Self::row_text`]、列走 [`Self::column_text`]）。
    /// `None` = 没东西可复制（选择指向的行已经不在视图里了——筛选 / 刷新之后可能如此；
    /// 或者列选落在行号槽上，那一列没有可复制的数据文本）。
    pub fn selection_text(&self, selection: GridSelection) -> Option<String> {
        // 【D10-5】列选中没有「单一行」：行存在性检查对它不成立（列里混着被删行，
        // `column_text` 自己会跳过），直接走列文本
        if let GridSelection::Column { col } = selection {
            return self.column_selection_text(col);
        }
        let row = match selection {
            GridSelection::Cell { row, .. } | GridSelection::Row { row } => row,
            GridSelection::Column { .. } => unreachable!("列选中在上面已返回"),
        };
        // 行不在了就说清“没东西可复制”，而不是安静地拷一个空串（用户会粘出一片空白）；
        // 被删底层行同样不参与复制（#28-B1 排除矩阵：复制单格 / 整行 TSV / 行 JSON 排除被删行）。
        if self.row_ref(row).is_none() || self.is_deleted_row(row) {
            return None;
        }
        Some(match selection {
            GridSelection::Cell { row, col } => self.cell(row, col),
            GridSelection::Row { row } => self.row_text(row),
            GridSelection::Column { .. } => unreachable!("列选中在上面已返回"),
        })
    }

    /// 【D10-5】列选中的复制文本：视图列 → 数据列（列 0 是行号槽），
    /// 再交给既有的「复制整列」实现（一行一个值、逐格 TSV 转义）。
    fn column_selection_text(&self, col: usize) -> Option<String> {
        if Self::is_row_number(col) || self.columns.is_empty() {
            return None;
        }
        let data_col = self.data_column(col - 1);
        if data_col >= self.columns.len() {
            return None;
        }
        Some(self.column_text(data_col))
    }
}
/// 单元格 / 表头的悬停全文：取值与列名都可能很宽，`truncate` 之后只剩这一条看全的路。
///
/// 宽度走 `ui::RESULT_TOOLTIP_MAX_WIDTH`（结构尺寸只认常量表），并让它**折行**：
/// 悬停提示跟着鼠标，横向越宽越容易被窗口边缘截掉。
pub(crate) fn result_cell_tooltip(text: String, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, _| {
        div()
            .max_w(rems(ui::RESULT_TOOLTIP_MAX_WIDTH))
            .whitespace_normal()
            .text_xs()
            .child(text.clone())
    })
    .build(window, cx)
}
/// 【B14】网格里**最近一次选中**（`Ctrl+C` 复制谁）
///
/// 为什么由面板记事件而不是读组件状态：组件没公开“当前是选格还是选行”，而
/// `selected_cell()` / `selected_row()` 可以**同时有值**（选过整行再点一个格子）——
/// 猜哪个更新会猜错，所以只认最近一次选择事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSelection {
    /// 选中一格（`col` 是含行号槽的列下标）
    Cell { row: usize, col: usize },
    /// 选中一整行（**视图行序**）
    Row { row: usize },
    /// 【D10-5】选中一整列（`col` 是含行号槽的列下标；DBeaver 列头点击
    /// `LightGrid.java:3185-3204` 的产物）
    Column { col: usize },
}

/// 结果内查找的一个命中（[`ResultGridDelegate::find_next`] 的产物）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultMatch {
    /// **视图行号**（筛选 / 排序之后看着的那一行）
    pub row: usize,
    /// 含行号槽的列下标
    pub col: usize,
    /// 格内区间（字节；`None` = 命中但给不出区间，见 [`ResultGridDelegate::find_next`]）
    pub range: Option<Range<usize>>,
}
/// 菜单里的值预览（短、单行；太长的值不该把菜单撑开）
pub(crate) fn preview_of(value: &str) -> String {
    let folded: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.chars().count() > 24 {
        let mut text: String = folded.chars().take(24).collect();
        text.push('…');
        text
    } else {
        folded
    }
}

/// 按**字节**上限截断，且不切进多字节字符内部；超长时补省略号。
///
/// 为什么不能写 `&s[..80]`：那是**字节**切片，第 80 个字节若落在多字节字符中间会 panic
/// （`byte index 80 is not a char boundary`）。筛选 SQL 预览里列名 / 中文标识符很常见，
/// 而 `s.len() > 80` 这个前置判断又是按字节算的，两者口径必须一致才安全。
/// 长度判断与截断都以字节为准，回退到最近的字符边界（最多 3 字节），所以预览最宽 80 字节。
pub(crate) fn clip_bytes(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 3);
    out.push_str(&value[..end]);
    out.push('…');
    out
}

/// 单元格里找 `needle`（**已小写**）的字节区间：两边都是 ASCII → 零分配
pub(crate) fn ascii_match_range(cell: &str, needle: &str) -> Option<Range<usize>> {
    let (haystack, needle) = (cell.as_bytes(), needle.as_bytes());
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|at| at..at + needle.len())
}

/// 格内命中的唯一实现：本地筛选与结果内查找都走它（一份口径，不许两套）
///
/// - `Some(Some(range))`：命中，且给得出**格内区间**（格与词都是 ASCII —— 折叠不改字节长度）；
/// - `Some(None)`：命中，但给不出区间（含非 ASCII）；
/// - `None`：没命中。
///
/// 为什么要单独一条 ASCII 快路径：筛选框每敲一下都要把**整表**过一遍，原先每格
/// `to_lowercase()` 一次分配（10 万行 × 20 列 = 200 万次），而结果里绝大多数格子是 ASCII。
/// 含非 ASCII 时回落 `to_lowercase`：`str::to_lowercase` 有上下文相关的特例（如希腊文末位
/// sigma），自写的逐字节比较会与它不一致——**口径不能有两种**，宁可慢一点。
pub(crate) fn find_in_cell(cell: &str, needle: &str) -> Option<Option<Range<usize>>> {
    if needle.is_empty() {
        return None;
    }
    if cell.is_ascii() && needle.is_ascii() {
        return ascii_match_range(cell, needle).map(Some);
    }
    cell.to_lowercase().contains(needle).then_some(None)
}

/// 单元格命中判据（筛选用：只看有没有）
pub(crate) fn cell_matches(cell: &str, needle: &str) -> bool {
    find_in_cell(cell, needle).is_some()
}

/// 【结果内查找】一次性编译好的匹配器：同一个词 + 同一套口径在整轮扫描里只编译一次
/// （`rebuild_view_rows` 要过全表，逐格编译正则会直接把 UI 拖死）。
///
/// 口径严格对齐 `SearchQuickFilterInfo`：非正则的词先 `escape`（= `Pattern.quote`），
/// 全词再按首尾字符决定加不加 `\b`，最后 `is_match`（= `matcher.find()`，子串语义）。
/// 纯子串（非正则、非全词）走既有的零分配快路径，与筛选框同一份实现。
pub(crate) struct FindMatcher {
    /// `trim` 后的原词（**不做小写化**：正则里 `[A-Z]`、`\D` 这类写法会因此失真）
    needle: String,
    /// 纯子串、大小写不敏感分支用的小写副本（预存一次，别在每格里 `to_lowercase`）
    needle_lower: String,
    criteria: FindCriteria,
    /// 仅正则 / 全词模式需要；编译失败（非法正则）时为 `None` → 一律不命中
    /// （DBeaver 那边 `Pattern.compile` 抛异常被 job 吞掉，本仓按「零命中」处理）
    compiled: Option<regex::Regex>,
}

impl FindMatcher {
    pub(crate) fn new(criteria: FindCriteria, needle: &str) -> Self {
        let needle = needle.trim().to_string();
        let needle_lower = if criteria.case_sensitive {
            String::new()
        } else {
            needle.to_lowercase()
        };
        // 纯子串（不带正则 / 全词）不进 regex 引擎：筛选与查找都走零分配快路径
        let compiled = if needle.is_empty() || (!criteria.regex && !criteria.whole_word) {
            None
        } else {
            compile_find_pattern(&needle, criteria)
        };
        Self {
            needle,
            needle_lower,
            criteria,
            compiled,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.needle.is_empty()
    }

    /// 这一格文本里的命中：`Some(Some(区间))` = 命中且给得出格内字节区间（用于高亮）；
    /// `Some(None)` = 命中但给不出区间（只在极端非 ASCII 子串下出现）；
    /// `None` = 没命中。
    pub(crate) fn find_in(&self, cell: &str) -> Option<Option<Range<usize>>> {
        if self.needle.is_empty() {
            return None;
        }
        match &self.compiled {
            // 正则 / 全词：regex 给出的区间一定落在字符边界上，可直接用于高亮
            Some(pattern) => pattern.find(cell).map(|found| Some(found.range())),
            // 纯子串：区分大小写时直接 `find`（字节区间天然准确，落在字符边界上）
            None if self.criteria.case_sensitive => cell
                .find(&self.needle)
                .map(|at| Some(at..at + self.needle.len())),
            // 纯子串、不区分大小写：与筛选框同一份实现（ASCII 零分配）
            None => find_in_cell(cell, &self.needle_lower),
        }
    }

    pub(crate) fn matches(&self, cell: &str) -> bool {
        self.find_in(cell).is_some()
    }
}

/// 按 `SearchQuickFilterInfo` 的口径拼出正则（`:44-59`）：
/// `escape(词)`（或用户原词）→ 词首 / 词尾是字母数字时各补一个 `\b` → 大小写标志。
fn compile_find_pattern(needle: &str, criteria: FindCriteria) -> Option<regex::Regex> {
    let body = if criteria.regex {
        needle.to_string()
    } else {
        regex::escape(needle)
    };
    let mut pattern = body;
    if criteria.whole_word {
        if needle.chars().next().is_some_and(|c| c.is_alphanumeric()) {
            pattern = format!("\\b{pattern}");
        }
        if needle
            .chars()
            .last()
            .is_some_and(|c| c.is_alphanumeric())
        {
            pattern = format!("{pattern}\\b");
        }
    }
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(!criteria.case_sensitive)
        .build()
        .ok()
}

