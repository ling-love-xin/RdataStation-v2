//! 结果集单元格变更集（T2a-3）：脏单元格的收集、撤销与批量提交的 UPDATE 生成
//!
//! 变更集是面板持有的**唯一权威**：网格只画（`grid` 的脏格标记由它投影），
//! 批量提交只从这里取数，回执只改这里的状态。
//!
//! 与 `export::encode_updates`（导出档，整行快照 SET 全部非键列）不同：
//! 提交只 `SET` **变更过的列**——用户改了一格，不该把整行重写成结果里的样子
//! （那是导出语义，写回语义是"只改你改的那一格"，与 DBeaver 一致）。

use std::collections::BTreeMap;

use crate::export::{cell_literal, is_null_cell, quote_ident, RowKey};

/// 一个单元格的变更
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellChange {
    /// 数据行号（`ResultEntry.rows` 的下标，不是视图行号——排序/筛选后的视图行要先还原）
    pub row: usize,
    /// 数据列下标（0 = 第一数据列；不含行号槽）
    pub column: usize,
    /// 原值（进入编辑那一刻的展示文本；撤销 / 冲突提示用）
    pub original: String,
    /// 改后的值
    pub edited: String,
    /// 状态（Pending → Committed / Conflict）
    pub status: CellChangeStatus,
}

/// 变更状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellChangeStatus {
    /// 待提交（用户已改、还没批量提交）
    Pending,
    /// 已提交成功（回执后本地面应用，从"待提交"集合里消失）
    Committed,
    /// 提交冲突（影响行数 0：源库里的这一行已被别人改过）
    Conflict,
}

impl CellChange {
    /// 新建一条待提交变更（原值/新值都不为空——空原值也该能改）
    pub fn pending(row: usize, column: usize, original: impl Into<String>, edited: impl Into<String>) -> Self {
        Self {
            row,
            column,
            original: original.into(),
            edited: edited.into(),
            status: CellChangeStatus::Pending,
        }
    }

    /// 编辑值有没有真的变（变了才入变更集；原值 == 新值是无效变更）
    pub fn is_dirty(&self) -> bool {
        self.original != self.edited
    }
}

/// 一份结果集的变更集合（同一份结果集一份；随结果集切换清空）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    cells: Vec<CellChange>,
}

impl ChangeSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 待提交（Pending）的条数——工具栏「提交 N 处」的数字
    pub fn pending_count(&self) -> usize {
        self.cells
            .iter()
            .filter(|change| change.status == CellChangeStatus::Pending)
            .count()
    }

    /// 插入 / 更新一个变更（同一格重复编辑只留最新一条）；返回是否新增
    pub fn upsert(&mut self, change: CellChange) -> bool {
        let is_new = !self
            .cells
            .iter()
            .any(|existing| existing.row == change.row && existing.column == change.column);
        self.remove(change.row, change.column);
        self.cells.push(change);
        is_new
    }

    /// 移除一格（撤销未提交的编辑）；返回是否真的移除了
    pub fn remove(&mut self, row: usize, column: usize) -> bool {
        let before = self.cells.len();
        self.cells
            .retain(|change| !(change.row == row && change.column == column));
        self.cells.len() != before
    }

    /// 清除已提交的（提交回执后调用：Committed 已落库，Conflict 保留给用户看）
    pub fn clear_committed(&mut self) {
        self.cells
            .retain(|change| change.status != CellChangeStatus::Committed);
    }

    /// 全部待提交变更（提交时取数）
    pub fn pending(&self) -> Vec<&CellChange> {
        self.cells
            .iter()
            .filter(|change| change.status == CellChangeStatus::Pending)
            .collect()
    }

    /// 待提交变更按行分组（每行一个 UPDATE 的前提；BTreeMap 保证行序）
    pub fn pending_by_row(&self) -> BTreeMap<usize, Vec<&CellChange>> {
        let mut by_row: BTreeMap<usize, Vec<&CellChange>> = BTreeMap::new();
        for change in self.pending() {
            by_row.entry(change.row).or_default().push(change);
        }
        by_row
    }

    /// 把一行提交成功的变更标为 Committed（本地应用后由面板调用）
    pub fn mark_committed(&mut self, row: usize, column: usize) {
        if let Some(change) = self
            .cells
            .iter_mut()
            .find(|change| change.row == row && change.column == column)
        {
            change.status = CellChangeStatus::Committed;
        }
    }

    /// 把一行提交冲突的变更标为 Conflict（回执带冲突时由面板调用）
    pub fn mark_conflict(&mut self, row: usize, column: usize) {
        if let Some(change) = self
            .cells
            .iter_mut()
            .find(|change| change.row == row && change.column == column)
        {
            change.status = CellChangeStatus::Conflict;
        }
    }

    /// 有冲突变更吗（结果工具栏据此标「有冲突，需处理」）
    pub fn has_conflicts(&self) -> bool {
        self.cells
            .iter()
            .any(|change| change.status == CellChangeStatus::Conflict)
    }

    /// 冲突明细（`(行, 列)`，提示条 / 状态栏消息用）
    pub fn conflicts(&self) -> Vec<(usize, usize)> {
        self.cells
            .iter()
            .filter(|change| change.status == CellChangeStatus::Conflict)
            .map(|change| (change.row, change.column))
            .collect()
    }
}

/// 批量提交生成的 UPDATE 文本（**每行一条**，只 SET 变更列）
///
/// `rows` = 结果集的**数据行**（`ResultEntry.rows`）：键列原值从它取（变更集不存键列，
/// 键列也不可改）。行号必须与 `pending_by_row` 的键对应（`pending` 里的 `row` 就是 rows 下标）。
///
/// ## 语义（口径定在这里，调用方不用再猜）
///
/// - 一行一条 `UPDATE <表> SET 变更列 = 新值, … WHERE 键列 = 原键值;`
///   ——键列是**定位依据**，不进 `SET`；多列键按 [`RowKey`] 顺序 `AND` 串。
/// - 值字面量走 [`cell_literal`]（与导出三档同一套启发式，见 `export` 模块）。
/// - 键值 `NULL` 的行**跳过**（`WHERE k = NULL` 永远不成立）：宁可少改一行并如实报告，
///   也不生成一条"看着对、跑起来改不到"的 UPDATE。
/// - 键列不在结果列里 → 整份回绝（`Err`）：键都定位不到，改任何一格都是改错行的风险。
///
/// ## 返回
///
/// `Ok((sql, skipped))`：`sql` 是多条 UPDATE 拼接（空行分隔），`skipped` 是键值为
/// NULL 而跳过的行数。`Err(说明)`：整份无法生成（键列缺失 / 没有变更 / 没有可 SET 的列）。
pub fn commit_updates_rows(
    table: &str,
    key: &RowKey,
    columns: &[String],
    rows: &[Vec<String>],
    pending_by_row: &BTreeMap<usize, Vec<&CellChange>>,
) -> Result<(String, usize), String> {
    if pending_by_row.is_empty() {
        return Err("没有待提交的变更".to_string());
    }
    let key_ix = key
        .indices(columns)
        .ok_or_else(|| format!("键列 {} 不在结果列里（列名要按结果集里的原样给）", key.columns().join(", ")))?;

    let target = quote_ident(table);
    let mut lines = Vec::with_capacity(pending_by_row.len());
    let mut skipped = 0usize;
    for (row, changes) in pending_by_row {
        // 键列原值：从数据行取（键列下标在结果列里；行比列短 / 缺格按 NULL 处理）
        let key_missing = key_ix
            .iter()
            .any(|ix| is_null_cell(rows.get(*row).and_then(|r| r.get(*ix)).map(String::as_str)));
        if key_missing {
            skipped += 1;
            continue;
        }
        // 只 SET 变更列（键列即使出现在变更集里也忽略——键不可改，改了等于换行）
        let assignments = changes
            .iter()
            .filter(|change| !key_ix.contains(&change.column))
            .map(|change| {
                format!(
                    "{} = {}",
                    quote_ident(&columns[change.column]),
                    cell_literal(Some(change.edited.as_str()))
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        if assignments.is_empty() {
            skipped += 1;
            continue;
        }
        let predicate = key_ix
            .iter()
            .map(|ix| {
                let value = rows[*row][*ix].as_str();
                format!(
                    "{} = {}",
                    quote_ident(&columns[*ix]),
                    cell_literal(Some(value))
                )
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        lines.push(format!("UPDATE {target} SET {assignments} WHERE {predicate};"));
    }

    if lines.is_empty() {
        let reason = if skipped > 0 {
            format!("{skipped} 行的键值是 NULL（定位不到一行），已全部跳过")
        } else {
            "没有可生成 UPDATE 的变更（变更列全是键列？）".to_string()
        };
        return Err(reason);
    }
    Ok((format!("{}\n", lines.join("\n")), skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(row: usize, col: usize, original: &str, edited: &str) -> CellChange {
        CellChange::pending(row, col, original, edited)
    }

    #[test]
    fn upsert_keeps_only_latest_per_cell() {
        let mut set = ChangeSet::new();
        assert!(set.upsert(change(0, 1, "a", "b")));
        assert!(!set.upsert(change(0, 1, "b", "c"))); // 同一格重复编辑 → 不是新增
        assert_eq!(set.pending_count(), 1);
        let pending = set.pending();
        assert_eq!(pending[0].edited, "c");
        assert_eq!(pending[0].original, "b"); // 原值 = 上一次编辑后的值
    }

    #[test]
    fn remove_undoes_a_pending_edit() {
        let mut set = ChangeSet::new();
        set.upsert(change(0, 1, "a", "b"));
        set.upsert(change(1, 2, "x", "y"));
        assert!(set.remove(0, 1));
        assert!(!set.remove(0, 1)); // 已移除
        assert_eq!(set.pending_count(), 1);
    }

    #[test]
    fn pending_by_row_groups_and_sorts() {
        let mut set = ChangeSet::new();
        set.upsert(change(2, 0, "a", "b"));
        set.upsert(change(0, 1, "c", "d"));
        set.upsert(change(2, 2, "e", "f"));
        set.upsert(change(0, 0, "g", "h"));
        let by_row = set.pending_by_row();
        let keys: Vec<usize> = by_row.keys().copied().collect();
        assert_eq!(keys, vec![0, 2]); // BTreeMap 行序
        assert_eq!(by_row[&0].len(), 2);
        assert_eq!(by_row[&2].len(), 2);
    }

    #[test]
    fn committed_cleared_conflicts_kept() {
        let mut set = ChangeSet::new();
        set.upsert(change(0, 0, "a", "b"));
        set.upsert(change(1, 0, "c", "d"));
        set.mark_committed(0, 0);
        set.mark_conflict(1, 0);
        set.clear_committed();
        // Committed 被清走；Conflict 既不算待提交也不算已提交——它留着给用户看
        assert_eq!(set.pending_count(), 0);
        assert!(set.has_conflicts());
        assert_eq!(set.conflicts(), vec![(1, 0)]);
    }

    #[test]
    fn dirty_requires_actual_change() {
        assert!(!change(0, 0, "a", "a").is_dirty());
        assert!(change(0, 0, "a", "b").is_dirty());
    }

    #[test]
    fn commit_updates_rows_emits_one_update_per_row() {
        let key = RowKey::single("id");
        let columns = vec!["id".to_string(), "name".to_string(), "price".to_string()];
        let rows = vec![
            vec!["1".to_string(), "苹果".to_string(), "3.5".to_string()],
            vec!["2".to_string(), "香蕉".to_string(), "2.0".to_string()],
        ];
        let mut set = ChangeSet::new();
        set.upsert(change(0, 1, "苹果", "苹果(特价)"));
        set.upsert(change(0, 2, "3.5", "3.0"));
        set.upsert(change(1, 2, "2.0", "1.8"));
        let by_row = set.pending_by_row();
        let (sql, skipped) = commit_updates_rows("fruit", &key, &columns, &rows, &by_row).unwrap();
        assert_eq!(skipped, 0);
        assert!(sql.contains("UPDATE fruit SET name = '苹果(特价)', price = 3.0 WHERE id = 1;"));
        assert!(sql.contains("UPDATE fruit SET price = 1.8 WHERE id = 2;"));
        // 键列（id）不进 SET
        assert!(!sql.contains("SET id"));
    }

    #[test]
    fn commit_updates_rows_skips_null_key() {
        let key = RowKey::single("id");
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec!["NULL".to_string(), "无名".to_string()],
            vec!["2".to_string(), "有号".to_string()],
        ];
        let mut set = ChangeSet::new();
        set.upsert(change(0, 1, "无名", "改名"));
        set.upsert(change(1, 1, "有号", "改名2"));
        let by_row = set.pending_by_row();
        let (sql, skipped) = commit_updates_rows("t", &key, &columns, &rows, &by_row).unwrap();
        assert_eq!(skipped, 1);
        assert!(!sql.contains("id = NULL"));
        assert!(sql.contains("WHERE id = 2"));
    }

    #[test]
    fn commit_updates_rows_refuses_when_key_column_missing() {
        let key = RowKey::single("id");
        let columns = vec!["name".to_string()]; // 结果里没有 id
        let rows = vec![vec!["无名".to_string()]];
        let mut set = ChangeSet::new();
        set.upsert(change(0, 0, "无名", "改名"));
        let by_row = set.pending_by_row();
        let err = commit_updates_rows("t", &key, &columns, &rows, &by_row).unwrap_err();
        assert!(err.contains("键列 id 不在结果列里"));
    }

    #[test]
    fn commit_updates_rows_refuses_when_nothing_pending() {
        let key = RowKey::single("id");
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![vec!["1".to_string(), "a".to_string()]];
        let by_row = BTreeMap::new();
        let err = commit_updates_rows("t", &key, &columns, &rows, &by_row).unwrap_err();
        assert!(err.contains("没有待提交的变更"));
    }
}
