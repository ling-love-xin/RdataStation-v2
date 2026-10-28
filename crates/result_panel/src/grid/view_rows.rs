//! 视图行序域（`grid/` 第二块）：隐藏列 / 列换算 / 转置 / 冻结 + 筛选 / 排序 /
//! 视图行重建 + 行身份导航 + 类型标签。TSV 文本缓存（[`TextFingerprint`]）也在此。

use gpui_kit::*;

use ::shared::string::tsv_row;
use crate::rowset::{LocalRow, RowRef};
use crate::store::{ColumnKind, ColumnType};
use super::stats::{column_stats, group_by_values, stats_lines};
use super::delegate::{ColumnStatsHook, LoadMoreHook, ResultGridDelegate};
use super::text::cell_matches;
use gpui_kit::component::table::ColumnSort;
use workbench_shell::model::ColumnAnnotation;
/// 【文本视图】TSV 文本指纹（缓存 key）：任何影响文本输出的状态变化都会改变指纹。
/// 指纹不依赖逐个方法标脏——排序 / 筛选 / 隐藏 / 数据行数变化都直接反映在上面；
/// 内容型变更（set_data / set_local_snapshot 替换）在入口显式清缓存兜底。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TextFingerprint {
    col_count: usize,
    view_len: usize,
    hidden: Vec<usize>,
    filter: String,
    column_filter: Option<(usize, String)>,
    sort: Option<(usize, bool)>,
}
impl ResultGridDelegate {
    // ---------- 【P2】隐藏列：视图列序 ↔ 数据列序 ----------

    /// 这一数据列被隐藏了吗
    pub fn is_hidden_column(&self, column: usize) -> bool {
        self.hidden.contains(&column)
    }

    /// 当前可见的数据列数（总列数减隐藏；全藏没有意义，hide 会兜底留 1 列）
    pub fn visible_column_count(&self) -> usize {
        self.columns.len().saturating_sub(self.hidden.len())
    }

    /// 已隐藏的数据列数（列浏览器清单菜单「显示全部列」出现条件）
    pub fn hidden_count(&self) -> usize {
        self.hidden.len()
    }

    /// 隐藏此列（**数据列序**）。隐藏到只剩 1 列时拒绝——全藏了表格没有内容。
    pub fn hide_column(&mut self, column: usize) {
        if self.visible_column_count() <= 1 || self.is_hidden_column(column) {
            return;
        }
        self.hidden.push(column);
        self.hidden.sort_unstable();
    }

    /// 单列取消隐藏（**数据列序**；列头菜单「显示全部」是整批，侧栏眼睛要单列粒度）
    pub fn show_column(&mut self, column: usize) {
        self.hidden.retain(|c| *c != column);
    }

    /// 显示全部列（清空隐藏清单）
    pub fn show_all_columns(&mut self) {
        self.hidden.clear();
    }

    /// 列名（**数据列序**；侧栏列结构列表读它，借读不拷）
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// 每列类型（与 [`Self::columns`] 同序；空 = 这份结果没带类型）
    pub fn column_types(&self) -> &[Option<ColumnType>] {
        &self.column_types
    }

    /// 列统计：网格本地算完转交宿主钩子开弹层（列头菜单与列浏览器清单共用同一条路）。
    /// 没接钩子 = 静默返回（与现有「没接钩子就不摆入口」口径一致；清单菜单上这一项
    /// 只在钩子存在时生成，见 [`crate::grid::column_browser_menu_items`] 的使用方）。
    pub fn fire_column_stats(&self, column: usize, app: &mut App) {
        let Some(hook) = self.on_column_stats.clone() else {
            return;
        };
        let name = self.columns.get(column).cloned().unwrap_or_default();
        let text = stats_lines(&column_stats(&self.rows, column)).join("\n");
        hook(name, text, app);
    }

    /// 列统计菜单载荷（列浏览器清单菜单构造期用）：钩子 + 列名 + 统计文本一次取好，
    /// 点击时直接 `hook(name, text, app)`——不经过 `grid_entity.update` 闭包，
    /// 避免闭包内二次可变借用 `app`（与列头菜单 L2241 同一条范式）。没接钩子 = `None`。
    pub fn column_stats_payload(&self, column: usize) -> Option<(ColumnStatsHook, String, String)> {
        let hook = self.on_column_stats.clone()?;
        let name = self.columns.get(column).cloned().unwrap_or_default();
        let text = stats_lines(&column_stats(&self.rows, column)).join("\n");
        Some((hook, name, text))
    }

    // ---------- 【P2-4】单行转置（只读视图，DBeaver 式） ----------

    /// 开 / 关转置视图。`row` 为要竖排的数据行（越界自动夹取）；
    /// 关掉后原网格原样回来（排序 / 冻结 / 隐藏都没动过）。
    ///
    /// `row` 是**视图行号**（含本地行区段）：内部折成行身份存下，后续筛选 / 排序改了
    /// 视图序列，转置锚定的仍是原来那一行（底层行号 / 本地行 id 不随序列滑移）。
    pub fn set_transposed(&mut self, transposed: bool, row: usize) {
        self.transposed = transposed;
        if transposed {
            self.transpose_row = self.resolve_transpose_ref(row);
        }
    }

    /// 转置行导航：切到另一行（按视图行号夹取到视图序列末位后折成身份）
    pub fn set_transpose_row(&mut self, row: usize) {
        self.transpose_row = self.resolve_transpose_ref(row);
    }

    /// 视图行号夹取到当前视图序列内并折成行身份；空序列兜底 `RowRef::Data(0)`。
    pub(crate) fn resolve_transpose_ref(&self, view_row: usize) -> RowRef {
        let clamped = view_row.min(self.view_len().saturating_sub(1));
        self.row_ref(clamped).unwrap_or(RowRef::Data(0))
    }

    /// 当前是否处于转置视图
    pub fn is_transposed(&self) -> bool {
        self.transposed
    }

    /// 当前转置锚定的行身份（导航 / 测试读它；底层行 / 本地行两类都可能）
    pub fn transpose_row(&self) -> RowRef {
        self.transpose_row
    }

    /// 转置投影的数据列（跳过隐藏列——转置也只投影用户看得见的列）
    pub(crate) fn transpose_columns(&self) -> Vec<usize> {
        (0..self.columns.len())
            .filter(|d| !self.is_hidden_column(*d))
            .collect()
    }

    /// 转置视图的单元格：第 1 列 = 原列名，第 2 列 = 锚定行该列的值
    /// （`#` 行号槽不是数据，返回 `None`）；锚定本地行时从本地快照取值。
    pub(crate) fn transpose_cell(&self, row_ix: usize, col_ix: usize) -> Option<&str> {
        if Self::is_row_number(col_ix) {
            return None;
        }
        let data_col = *self.transpose_columns().get(row_ix)?;
        match col_ix {
            1 => self.columns.get(data_col).map(String::as_str),
            _ => match self.transpose_row {
                RowRef::Data(data_ix) => {
                    // 被删底层行不进转置视图（#28-B1 排除矩阵：转置排除被删行）
                    if self.local.as_ref().is_some_and(|set| set.is_deleted(data_ix)) {
                        return None;
                    }
                    self.rows
                        .get(data_ix)?
                        .get(data_col)
                        .map(String::as_str)
                }
                RowRef::Local(id) => self.local_cell(id, data_col),
            },
        }
    }

    /// 可见数据列序 → 数据列序（跳过隐藏列）。
    ///
    /// 组件给的下标是「用户看着的那一列」（不含 `#` 行号槽），而数据、类型、注释、
    /// 冻结都按**数据列**存——中间隔着隐藏列，换算必须过这里。面板列浏览器联动
    /// （网格选中格 → 清单选中列）同样走它。
    pub fn data_column(&self, view_col: usize) -> usize {
        let mut data = 0;
        let mut visible = 0;
        loop {
            if !self.is_hidden_column(data) {
                if visible == view_col {
                    return data;
                }
                visible += 1;
            }
            data += 1;
        }
    }

    /// 数据列序 → 可见数据列序（该列被隐藏 = `None`；清单双击定位网格用）
    pub fn view_column(&self, data_col: usize) -> Option<usize> {
        if self.is_hidden_column(data_col) {
            return None;
        }
        Some((0..data_col).filter(|d| !self.is_hidden_column(*d)).count())
    }

    /// 【B15】冻结 / 取消冻结（切换；返回切换后的状态）
    pub fn toggle_freeze(&mut self, column: usize) -> bool {
        if let Some(at) = self.frozen.iter().position(|frozen| *frozen == column) {
            self.frozen.remove(at);
            false
        } else {
            self.frozen.push(column);
            true
        }
    }

    /// 【B15】设置本地筛选词（空 = 不筛）；只改视图行序，不重查
    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.rebuild_view_rows();
    }

    /// 【B15】当前筛选词（面板与导出要用同一份真值）
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// 【B15】筛选后的行（导出跟随筛选：原型 §5.5 的口径）
    /// 文本视图序列：底层未删行 + 本地行（#28-B1 拍板 D2：文本视图纳入本地行、排除被删行）。
    /// 隐藏列投影在调用方（[`Self::text_presentation`]）。
    pub fn visible_rows(&self) -> Vec<Vec<String>> {
        (0..self.view_len())
            .filter_map(|view_ix| match self.row_ref(view_ix)? {
                RowRef::Data(data_ix) => {
                    if self.local.as_ref().is_some_and(|set| set.is_deleted(data_ix)) {
                        return None;
                    }
                    self.rows.get(data_ix).cloned()
                }
                RowRef::Local(id) => self
                    .local
                    .as_ref()
                    .and_then(|set| set.local_row(id))
                    .map(|row| row.cells.clone()),
            })
            .collect()
    }

    /// 导出序列：只含底层未删行——本地行不进任何导出档、被删行排除（#28-B1 排除矩阵）。
    /// 与 [`Self::visible_rows`] 的差别只在本地行是否纳入。
    pub fn visible_rows_for_export(&self) -> Vec<Vec<String>> {
        (0..self.view_len())
            .filter_map(|view_ix| {
                let data_ix = self.data_row(view_ix)?;
                if self.local.as_ref().is_some_and(|set| set.is_deleted(data_ix)) {
                    return None;
                }
                self.rows.get(data_ix).cloned()
            })
            .collect()
    }

    /// 【元数据】列注释访问器（与 `columns` 同序；空 = 这份结果没报注释）。
    /// 面板右栏「元数据」页用它画来源表 / 来源列 / 注释 / 类型 / 可空 / 默认值。
    pub fn column_annotations(&self) -> &[Option<ColumnAnnotation>] {
        &self.column_annotations
    }

    /// 【呈现】文本视图：当前结果拼成 TSV 纯文本（DBeaver「显示方式 → 文本」）。
    ///
    /// 列头 = 可见数据列名；行 = `visible_rows()`（已滤筛选 / 排序）按可见列取。
    /// 约定与导出一致：NULL 原样输出、Tab 分隔、行末不带 `\t`。隐藏列不参与。
    /// 文本视图的整份 TSV（可见列头 + 可见行；隐藏列不参与；行序随筛选 / 排序）。
    /// **缓存版**：指纹命中直接复用上次结果（bench：50k×8 一次拼 ~120ms，若每次
    /// 渲染都重拼，动画 / 滚动期间会卡住 UI——用户"点击文本半天才出来"即此）。
    /// 指纹变化（排序 / 筛选 / 隐藏 / 行数）自动失效；内容型变更（set_data /
    /// set_local_snapshot）在入口显式清缓存。
    pub fn text_presentation(&self) -> String {
        let fp = TextFingerprint {
            col_count: self.columns.len(),
            view_len: self.view_len(),
            hidden: self.hidden.clone(),
            filter: self.filter.clone(),
            column_filter: self.column_filter.clone(),
            sort: self.sort,
        };
        if let Some((cached_fp, text)) = &*self.text_cache.borrow() {
            if cached_fp == &fp {
                return text.clone();
            }
        }
        let text = self.compute_text_presentation();
        *self.text_cache.borrow_mut() = Some((fp, text.clone()));
        text
    }

    /// 【文本视图】TSV 拼接本体（`text_presentation` 缓存未命中时调用）
    pub(crate) fn compute_text_presentation(&self) -> String {
        let visible: Vec<usize> = (0..self.columns.len())
            .filter(|col| !self.hidden.contains(col))
            .collect();
        let header: Vec<&str> = visible
            .iter()
            .map(|col| self.columns[*col].as_str())
            .collect();
        let mut lines = vec![header.join("\t")];
        for row in self.visible_rows() {
            let cells: Vec<&str> = visible
                .iter()
                .filter_map(|col| row.get(*col).map(String::as_str))
                .collect();
            lines.push(cells.join("\t"));
        }
        lines.join("\n")
    }

    /// 工具栏整份复制（#28-B1 拍板 D7）：**全列**、底层未删行 + 本地行、排除被删行。
    ///
    /// 返回 `(TSV 文本, 数据行数)`，计数与文本同一口径；不再走 `entry.to_tsv()` 全量底层行
    /// （那条路既漏本地行、又把被删行拷走）。表头一行 + 数据行，Tab 分隔、行末不带 `\t`。
    pub fn active_result_tsv(&self) -> (String, usize) {
        let mut lines = vec![self.columns.join("\t")];
        let mut count = 0usize;
        for view_ix in 0..self.view_len() {
            let Some(reference) = self.row_ref(view_ix) else {
                continue;
            };
            let cells: Vec<String> = match reference {
                RowRef::Data(data_ix) => {
                    if self.local.as_ref().is_some_and(|set| set.is_deleted(data_ix)) {
                        continue;
                    }
                    self.rows.get(data_ix).cloned().unwrap_or_default()
                }
                RowRef::Local(id) => self
                    .local
                    .as_ref()
                    .and_then(|set| set.local_row(id))
                    .map(|row| row.cells.clone())
                    .unwrap_or_default(),
            };
            lines.push(tsv_row(&cells));
            count += 1;
        }
        (lines.join("\n"), count)
    }

    /// 【分组】按一列本地分组计数（纯函数，不引 polars——见 result-panel.md §8.8 选型记录）。
    ///
    /// 保留首次出现顺序（与网格行序一致）；NULL 作为独立组值原样输出；
    /// 越界列返回空组（面板侧先取可见列，防御性兜底）。
    pub fn group_by_count(rows: &[Vec<String>], column: usize) -> Vec<(String, usize)> {
        // 越界列：首行都没有这一列，说明列不存在（面板侧先取可见列，防御性兜底）
        let Some(first) = rows.first() else {
            return Vec::new();
        };
        if first.len() <= column {
            return Vec::new();
        }
        let values: Vec<String> = rows
            .iter()
            .map(|row| row.get(column).cloned().unwrap_or_default())
            .collect();
        group_by_values(&values)
    }

    /// 【B15】当前排序（数据列下标，是否降序）
    pub fn sort_state(&self) -> Option<(usize, bool)> {
        self.sort
    }

    /// 【B15】应用本地排序（`None` = 回原顺序）；组件库的列头点击最终走到这里
    pub fn apply_sort(&mut self, sort: Option<(usize, bool)>) {
        self.sort = sort;
        self.rebuild_view_rows();
    }

    /// 【排序】取消当前本地排序（对齐 DBeaver 排序 `NONE`；只回原顺序，不下发源库重查）
    pub fn clear_sort(&mut self) {
        self.apply_sort(None);
    }

    /// 【B15】重算视图行序（筛选 → 排序；数据不动）
    ///
    /// 行身份两类：`RowRef::Data` 底层未删行、`RowRef::Local` 本地行。
    /// - 全局词 / 列级筛选对两类行都生效（命中才出现）；
    /// - 列排序**只作用底层行**（#28-B1 拍板 D1）：本地行不参与比较，始终登记序追加在
    ///   底层区段末尾；
    /// - 被删底层行不在此排除——网格内保留灰显删除线可见（排除序列供统计 / 分组 / 导出 /
    ///   复制出口使用，那是另一套序列，见 B5 批次）。
    pub(crate) fn rebuild_view_rows(&mut self) {
        let filter = self.filter.trim().to_lowercase();
        // 既没筛选也没排序：视图行序就是基准序列——**不物化**那份映射（`None` 即恒等）。
        // 入库 / 切结果集时这一步占了整一轮逐行工作，而绝大多数时候根本用不上。
        if filter.is_empty() && self.column_filter.is_none() && self.sort.is_none() {
            self.view_rows = None;
            return;
        }
        // 底层行：全局筛选词（所有列包含匹配）与列级筛选（单列包含匹配）**叠加取交集**：
        // 一个词卡所有列，一个值卡一列，同时生效时行要两个都过。被删底层行不进筛选 /
        // 排序序列（#28-B1 排除矩阵：筛选命中、列排序都排除被删行；网格内灰显删除线保留可见）。
        let mut data: Vec<usize> = (0..self.rows.len())
            .filter(|ix| {
                !self.local.as_ref().is_some_and(|set| set.is_deleted(*ix))
                    && (filter.is_empty() || self.row_matches(*ix, &filter))
                    && self.column_filter_matches(*ix)
            })
            .collect();
        if let Some((column, descending)) = self.sort {
            // 稳定排序：同值时按原顺序（用数据行号做 tie-break，与快排的不稳定性无关）
            data.sort_by(|left, right| {
                let ordering = self.compare_rows(*left, *right, column);
                let ordering = if descending { ordering.reverse() } else { ordering };
                ordering.then(left.cmp(right))
            });
        }
        let mut view: Vec<RowRef> = data.into_iter().map(RowRef::Data).collect();
        // 本地行：命中筛选才出现，不参与排序，按登记序追加在底层区段末尾
        if let Some(local) = &self.local {
            for row in local.local_rows() {
                if (filter.is_empty() || self.local_row_matches(row, &filter))
                    && self.local_column_filter_matches(row)
                {
                    view.push(RowRef::Local(row.id));
                }
            }
        }
        self.view_rows = Some(view);
    }

    /// 本地行是否命中全局筛选词（大小写不敏感子串，与底层行同一套口径）
    pub(crate) fn local_row_matches(&self, row: &LocalRow, needle: &str) -> bool {
        row.cells.iter().any(|cell| cell_matches(cell, needle))
    }

    /// 【筛选】本地行是否过列级筛选（单列包含匹配；没有列筛选 = 恒过）
    pub(crate) fn local_column_filter_matches(&self, row: &LocalRow) -> bool {
        match &self.column_filter {
            Some((column, value)) => row
                .cells
                .get(*column)
                .is_some_and(|cell| cell_matches(cell, value)),
            None => true,
        }
    }

    /// 【B5】两行在某列上的比较：内存档数值列取事实帧原生 `f64` 比较（避免 `"1"` 与
    /// `"1.0"` 字符串分裂、避免大整数被 parse 截断口径外的字符串误判）；任一格取不到原生
    /// 数值（NULL / 文本族 / 表档 / 无事实档）整对回退显示矩阵的 [`compare_cells`]，
    /// 不出现同一列两套排序口径混排。
    pub(crate) fn compare_rows(&self, left: usize, right: usize, column: usize) -> std::cmp::Ordering {
        if let Some(frame) = self.fact.as_ref() {
            if let (Some(a), Some(b)) =
                (frame.column_number(left, column), frame.column_number(right, column))
            {
                return a.total_cmp(&b);
            }
        }
        compare_cells(
            self.rows.get(left).and_then(|row| row.get(column)),
            self.rows.get(right).and_then(|row| row.get(column)),
        )
    }

    /// 这一行有没有单元格命中筛选词（大小写不敏感的子串，与 DBeaver 的本地筛选同类）
    pub(crate) fn row_matches(&self, row_ix: usize, needle: &str) -> bool {
        self.rows
            .get(row_ix)
            .is_some_and(|row| row.iter().any(|cell| cell_matches(cell, needle)))
    }

    /// 【筛选】这一行是否过列级筛选（单列包含匹配；没有列筛选 = 恒过）
    pub(crate) fn column_filter_matches(&self, row_ix: usize) -> bool {
        let Some((column, value)) = &self.column_filter else {
            return true;
        };
        self.rows
            .get(row_ix)
            .and_then(|row| row.get(*column))
            .is_some_and(|cell| cell_matches(cell, value))
    }

    /// 【B15】该数据列在表头上的排序标记
    ///
    /// `Default` = 可排但未排（组件库画的是“上下箭头”图标）；当前排序列才给升/降。
    pub(crate) fn sort_mark(&self, index: usize) -> ColumnSort {
        match self.sort {
            Some((column, true)) if column == index => ColumnSort::Descending,
            Some((column, false)) if column == index => ColumnSort::Ascending,
            _ => ColumnSort::Default,
        }
    }

    /// 视图行号 → 行身份（`None` = 没有这一行）。
    ///
    /// 视图序列没物化时（既没筛选也没排序）走基准序列：底层行按原序排前、本地行按登记序
    /// 追加在后；越界按基准长度判，不能把行号当身份直接返回。
    pub(crate) fn row_ref(&self, row_ix: usize) -> Option<RowRef> {
        match &self.view_rows {
            Some(view) => view.get(row_ix).copied(),
            None => {
                if row_ix < self.rows.len() {
                    Some(RowRef::Data(row_ix))
                } else {
                    let local = self.local.as_ref()?;
                    local
                        .local_rows()
                        .get(row_ix - self.rows.len())
                        .map(|row| RowRef::Local(row.id))
                }
            }
        }
    }

    /// 视图行号 → 底层数据行号（本地行返回 `None`：脏格 / 冲突 / 提交批次只认底层行）
    pub(crate) fn data_row(&self, row_ix: usize) -> Option<usize> {
        self.row_ref(row_ix).and_then(|reference| reference.data_index())
    }

    /// 视图行号 → 行身份（底层行 / 本地行；越界 `None`）。宿主分派行级动作时
    /// 经它把视图行号翻译成 `RowRef`，再决定落 `LocalRowSet` 的哪一条。
    pub fn reference_of(&self, row_ix: usize) -> Option<RowRef> {
        self.row_ref(row_ix)
    }

    /// 这一视图行是不是本地新增 / 复制行（渲染底色区分用；底层行 = `false`）
    pub fn row_is_local(&self, row_ix: usize) -> bool {
        self.row_ref(row_ix).is_some_and(|reference| reference.is_local())
    }

    /// 这一视图行是不是被标记删除的底层行（本地行永不标记删除；渲染灰显 + 删除线用）
    pub fn is_deleted_row(&self, row_ix: usize) -> bool {
        let Some(RowRef::Data(data_ix)) = self.row_ref(row_ix) else {
            return false;
        };
        self.local
            .as_ref()
            .is_some_and(|set| set.is_deleted(data_ix))
    }

    /// 视图行数（没筛选 / 排序时 = 底层行数 + 本地行数）
    pub fn view_len(&self) -> usize {
        match &self.view_rows {
            Some(view) => view.len(),
            None => self.rows.len() + self.local_count(),
        }
    }

    /// 这份结果还能不能取下一段（面板在结果变化时同步；`true` 才启用滚动到底加载）
    pub fn set_has_more(&mut self, has_more: bool) {
        self.has_more = has_more;
    }

    /// 正在取下一段（回填之前不该重复触发）
    pub fn set_loading_more(&mut self, loading_more: bool) {
        self.loading_more = loading_more;
    }

    /// 注入取下一段的回调（面板构造时一次）
    pub fn set_load_more_hook(&mut self, hook: LoadMoreHook) {
        self.on_load_more = Some(hook);
    }

    /// 能不能现在就取下一段（纯函数：面板与测试都是同一套判据）
    pub fn wants_more(&self) -> bool {
        self.has_more && !self.loading_more && self.on_load_more.is_some()
    }

    /// 测试用：右键菜单的两个钩子接上了吗（接线正确性；真点击在窗口里驱动）
    ///
    /// 2026-10-23 去 `#[cfg(test)]`：作为**依赖**编译时 cfg(test) 不生成，而 editor 的
    /// 窗口测试（`view/tests.rs`）也断言它——测试访问器成为本 crate 公开 API。
    pub fn has_filter_value_hook(&self) -> bool {
        self.on_filter_value.is_some()
    }

    /// 测试用：视图行序现在是**物化**的吗（`false` = 恒等，入库 / 切结果集时那份映射被省掉了）
    pub fn view_order_materialized_for_test(&self) -> bool {
        self.view_rows.is_some()
    }

    /// 测试用：见上
    pub fn has_sort_down_hook(&self) -> bool {
        self.on_sort_down.is_some()
    }

    /// 网格里的列名（供测试断言；不复制行数据）
    pub fn columns_for_test(&self) -> &[String] {
        &self.columns
    }

    /// 网格里的行数（供测试断言）
    pub fn row_count_for_test(&self) -> usize {
        self.rows.len()
    }

    /// 筛选后能看见多少行（视图行数）
    pub fn visible_row_count(&self) -> usize {
        self.view_len()
    }

    /// 这份结果一共抓到多少行（数据行数，与筛选无关）
    pub fn data_row_count(&self) -> usize {
        self.rows.len()
    }

    /// 第 `col_ix` 列是不是行号槽（首列；没有数据时不画它）
    pub(crate) fn is_row_number(col_ix: usize) -> bool {
        col_ix == 0
    }

    /// 第 `col_ix` 列（**含行号槽**）的语义类型：行号槽与没带类型的列都是 `Unknown`
    ///
    /// 渲染只读这一条（对齐 / 等宽），每格 O(1)：类型在 `set_column_types` 时就已经解析好了
    /// （每列一次，不是每格一次）。
    pub(crate) fn column_kind(&self, col_ix: usize) -> ColumnKind {
        // 【P2-4】转置视图的列是混合类型（每行对应一个不同的原列），列级类型不成立——
        // 单元格自己的类型由 `render_td` 按行取（那里能拿到原列）
        if self.transposed {
            return ColumnKind::Unknown;
        }
        if Self::is_row_number(col_ix) {
            return ColumnKind::Unknown;
        }
        self.column_types
            .get(self.data_column(col_ix - 1))
            .and_then(Option::as_ref)
            .map_or(ColumnKind::Unknown, |column| column.kind)
    }

    /// 表头的类型小标签：驱动报的**原始名字**（行号槽 / 没带类型 = `None`，那就一个字也不摆）
    pub(crate) fn type_label(&self, col_ix: usize) -> Option<&str> {
        // 【P2-4】转置列是混合类型，不给列级标签
        if self.transposed {
            return None;
        }
        if Self::is_row_number(col_ix) {
            return None;
        }
        self.column_types
            .get(self.data_column(col_ix - 1))
            .and_then(Option::as_ref)
            .map(|column| column.name.as_str())
    }

    /// 【A3】类型族 → 符号短码（列头 badge 内文；Unknown 不画 badge）。
    /// 比 A1 的 INT/FLT/TXT 短一半：数字=123 / 文本=Aa / 时间=◷ / 布尔=✓ / 二进制=01。
    pub(crate) fn type_kind_sym(kind: ColumnKind) -> Option<&'static str> {
        match kind {
            ColumnKind::Integer | ColumnKind::Float | ColumnKind::Decimal => Some("123"),
            ColumnKind::Boolean => Some("✓"),
            ColumnKind::Timestamp | ColumnKind::Date | ColumnKind::Time => Some("◷"),
            ColumnKind::Text | ColumnKind::Uuid | ColumnKind::Json => Some("Aa"),
            ColumnKind::Binary => Some("01"),
            ColumnKind::Unknown => None,
        }
    }
}
/// 两个单元格比大小（本地排序用，2026-10-29 拆分时自 [crate::grid] 顶层迁入）
/// 两个单元格比大小（本地排序用）
///
/// 两边都像数字就按**数值**比（`1, 2, 10` 不该排成 `1, 10, 2`——展示文本是字符串，
/// 但用户看的是数）；否则按字符串比。`None`（缺列）排在最后。
pub(crate) fn compare_cells(left: Option<&String>, right: Option<&String>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (Some(left), Some(right)) = (left, right) else {
        return match (left.is_some(), right.is_some()) {
            (false, false) => Ordering::Equal,
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (true, true) => Ordering::Equal,
        };
    };
    if let (Ok(left), Ok(right)) = (left.parse::<f64>(), right.parse::<f64>()) {
        return left.partial_cmp(&right).unwrap_or(Ordering::Equal);
    }
    left.cmp(right)
}
