//! 行级叠加层（#28 方案 A）：本地新增 / 复制行 + 底层行删除标记 + 行级撤销重做
//!
//! ## 分层口径（2026-10-24 拍板，见 `docs/architecture/editor/result-panel.md` §8.10）
//!
//! - **权威层在 store 结果集槽位**（与 [`crate::changes::ChangeSet`] /
//!   [`crate::changes::EditHistory`] 并列）；delegate 只在 `sync` 时投影，不持本地行真值。
//!   原因：`set_data` 每次投影都会重建 delegate 行状态，本地行挂视图层会在切标签 /
//!   提交 / 撤销 / 重投影后丢失。
//! - **底层 `rows` 与格级 `ChangeSet` 完全不动**；本地行没有底层行号，删除底层行只打
//!   标记不真删，底层行号不滑移。
//! - **纳入范围**：本地行进入网格视图（筛选 / 排序 / 分组 / 统计 / 预览 / 转置 /
//!   复制 TSV / 文本视图）；被删行全链路排除（仅网格内灰显删除线可见，渲染接线在下一轮）；
//!   提交批次与 SQL 导出一律排除本地行与被删行，**不下发 INSERT / DELETE**。
//! - **撤销分栈**：格级编辑走 [`crate::changes::EditHistory`]；行级动作走本模块的
//!   [`RowHistory`]。单组撤销按钮「格级优先、栈空再走行级」的组合语义在面板接线层实现，
//!   两个栈互不混栈。
//!
//! 本轮（模型层）只交付模型结构 + 纯函数 + 单测；动作按钮接线、删除线渲染、本地行格
//! 编辑属于下一轮，不在本模块实现。

use std::collections::BTreeSet;

/// 行身份：底层数据行 / 本地行（稳定 id）
///
/// 现状全链路以「数据行号 `usize`」标识行；叠加层引入本地行后，视图序列、取值、
/// 整行动作、转置、筛选排序、分组统计、导出、选中态都要能区分两类行。id 用变体
/// 区分而不是编号空间平移：本地行 id 与底层行号不做偏移账本（方案 B 已否决——
/// 平移错位风险高、本地行易漏排除混入下发 / 统计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowRef {
    /// 底层数据行（`ResultEntry.rows` 下标；排序 / 筛选后的视图行要先还原成它）
    Data(usize),
    /// 本地行（叠加层分配的稳定 id；不随其它本地行增删滑移）
    Local(u64),
}

impl RowRef {
    pub fn data(index: usize) -> Self {
        Self::Data(index)
    }

    pub fn local(id: u64) -> Self {
        Self::Local(id)
    }

    pub fn is_data(&self) -> bool {
        matches!(self, Self::Data(_))
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local(_))
    }

    /// 底层行号（`Local` 返回 `None`）
    pub fn data_index(&self) -> Option<usize> {
        match self {
            Self::Data(index) => Some(*index),
            Self::Local(_) => None,
        }
    }

    /// 本地行 id（`Data` 返回 `None`）
    pub fn local_id(&self) -> Option<u64> {
        match self {
            Self::Local(id) => Some(*id),
            Self::Data(_) => None,
        }
    }
}

/// 一行本地数据（新增空行 / 复制快照；格值字符串化，与底层 `rows` 同口径）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRow {
    /// 叠加层分配的稳定 id（[`LocalRowSet::next_id`] 取号）
    pub id: u64,
    /// 格值（新增 = 全空；复制 = 来源行当前展示值快照，独立拷贝、不引用底层行）
    pub cells: Vec<String>,
}

impl LocalRow {
    pub fn new(id: u64, cells: Vec<String>) -> Self {
        Self { id, cells }
    }
}

/// 一份结果集的本地叠加层：本地新增 / 复制行 + 底层行删除标记
///
/// 与 [`crate::changes::ChangeSet`] 同槽、同生命周期（结果被替换 / 淘汰时作废，
/// 切标签不清空）。`deleted` 是底层数据行号集合，只标记不真删。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalRowSet {
    /// 本地行（按登记序；视图排序 / 锚点插入位置由视图层在序列上再加工）
    rows: Vec<LocalRow>,
    /// 被标记删除的底层数据行号（BTreeSet 保证有序、去重）
    deleted: BTreeSet<usize>,
    /// 下一个待分配的本地行 id（从 1 起；0 留作“未分配”哨兵，模型内不出现）
    next_id: u64,
}

impl LocalRowSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// 既无本地行也无删除标记（结果投影 / 状态栏计数用）
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.deleted.is_empty()
    }

    /// 清空叠加层（结果被替换 / 淘汰时调用；本地行与删除标记一起作废）
    pub fn clear(&mut self) {
        self.rows.clear();
        self.deleted.clear();
        // id 计数器不归零：旧 id 对应的行已作废，新行继续递增避免与历史快照撞号
    }

    /// 分配下一个稳定本地行 id（新增 / 复制前先取号，动作入栈要带这个 id）
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// 登记一行本地行（id 已由 [`Self::next_id`] 分配；cells = 空行或复制快照）
    pub fn push_local(&mut self, row: LocalRow) {
        self.rows.push(row);
    }

    /// 移除一行本地行，返回快照（删除本地行；撤销时凭快照恢复）。
    /// 找不到该 id 返回 `None`。
    pub fn remove_local(&mut self, id: u64) -> Option<LocalRow> {
        let position = self.rows.iter().position(|row| row.id == id)?;
        Some(self.rows.remove(position))
    }

    pub fn local_row(&self, id: u64) -> Option<&LocalRow> {
        self.rows.iter().find(|row| row.id == id)
    }

    pub fn local_row_mut(&mut self, id: u64) -> Option<&mut LocalRow> {
        self.rows.iter_mut().find(|row| row.id == id)
    }

    pub fn local_rows(&self) -> &[LocalRow] {
        &self.rows
    }

    pub fn local_count(&self) -> usize {
        self.rows.len()
    }

    /// 标记底层数据行删除（只打标记，不真删；底层行号不滑移）
    pub fn mark_deleted(&mut self, data_row: usize) {
        self.deleted.insert(data_row);
    }

    /// 取消底层行删除标记（撤销删除底层行）；返回取消前是否已被标记
    pub fn unmark_deleted(&mut self, data_row: usize) -> bool {
        self.deleted.remove(&data_row)
    }

    /// 这一底层数据行是否被标记删除
    pub fn is_deleted(&self, data_row: usize) -> bool {
        self.deleted.contains(&data_row)
    }

    pub fn deleted_rows(&self) -> &BTreeSet<usize> {
        &self.deleted
    }

    pub fn deleted_count(&self) -> usize {
        self.deleted.len()
    }
}

/// 叠加层全量可见行序列：底层未删行按原序 + 本地行按登记序。
///
/// 被删行**不出现**在序列里（全链路排除口径：不进分组 / 统计 / 导出 / 复制）。
/// 筛选 / 排序 / 「插选中行下方」的锚点落点由视图层在这份基准序列上再加工，
/// 模型层只负责给出“哪些行还在”。
pub fn visible_refs(data_row_count: usize, local: &LocalRowSet) -> Vec<RowRef> {
    let mut refs: Vec<RowRef> = (0..data_row_count)
        .filter(|index| !local.is_deleted(*index))
        .map(RowRef::Data)
        .collect();
    refs.extend(local.rows.iter().map(|row| RowRef::Local(row.id)));
    refs
}

/// 行级动作（撤销 / 重做的一步；携带逆操作所需的全部快照）
///
/// 粒度 = 一次新增 / 复制 / 删除一行（对齐 DBeaver 行操作粒度）；格级编辑不在此栈。
/// 每个变体都带 `anchor`（锚定的底层行号，插在它下方；`None` = 末尾），撤销重做
/// 恢复位置时用；本轮模型层只记录，落点排序由下一轮视图接线处理。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowOp {
    /// 新增本地空行：撤销 = 移除该 id；重做 = 重新登记空行
    InsertLocal {
        local_id: u64,
        anchor: Option<usize>,
    },
    /// 复制一行（底层行或本地行）：撤销 = 移除副本；重做 = 用快照重新登记
    DuplicateLocal {
        local_id: u64,
        source: RowRef,
        snapshot: Vec<String>,
        anchor: Option<usize>,
    },
    /// 删除底层数据行（标记）：撤销 = 取消标记；重做 = 重新标记
    DeleteData(usize),
    /// 删除本地行：撤销 = 凭快照恢复；重做 = 再移除
    RemoveLocal {
        local_id: u64,
        cells: Vec<String>,
        anchor: Option<usize>,
    },
}

/// 行级历史（撤销 / 重做双栈）：与格级 [`crate::changes::EditHistory`] 并列、互不混栈
///
/// 新动作入栈时清空重做栈（标准撤销语义）；撤销 / 重做返回要应用的那条动作，
/// 由调用方（面板接线层）据此更新 [`LocalRowSet`] 并刷新网格。模型层不把动作直接
/// 应用到叠加层——与 `EditHistory` 只存快照、宿主写回变更集的分工一致。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowHistory {
    undo: Vec<RowOp>,
    redo: Vec<RowOp>,
}

impl RowHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// 一次行级动作入栈（新动作清空重做栈）
    pub fn push(&mut self, op: RowOp) {
        self.undo.push(op);
        self.redo.clear();
    }

    /// 撤销一步：弹出并返回上一条动作（`None` = 没得撤）。
    /// 返回的动作由调用方解读并做逆操作（移除本地行 / 取消删除标记 / 恢复快照）。
    pub fn undo(&mut self) -> Option<RowOp> {
        let op = self.undo.pop()?;
        self.redo.push(op.clone());
        Some(op)
    }

    /// 重做一步：重新应用最近撤销的那条（`None` = 没得重做）
    pub fn redo(&mut self) -> Option<RowOp> {
        let op = self.redo.pop()?;
        self.undo.push(op.clone());
        Some(op)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// 结果集切换 / 刷新时清空：行级历史属于「这次抓到的数据」，数据换了历史就该丢
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== RowRef 判别与取值 =====

    #[test]
    fn row_ref_discriminates_data_and_local() {
        let data = RowRef::data(7);
        let local = RowRef::local(3);
        assert!(data.is_data() && !data.is_local());
        assert!(!local.is_data() && local.is_local());
        assert_eq!(data.data_index(), Some(7));
        assert_eq!(data.local_id(), None);
        assert_eq!(local.local_id(), Some(3));
        assert_eq!(local.data_index(), None);
    }

    // ===== LocalRowSet 取号与登记 =====

    #[test]
    fn next_id_increases_and_stable_across_calls() {
        let mut set = LocalRowSet::new();
        let first = set.next_id();
        let second = set.next_id();
        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_ne!(first, second);
    }

    #[test]
    fn push_and_lookup_local_row() {
        let mut set = LocalRowSet::new();
        let id = set.next_id();
        set.push_local(LocalRow::new(id, vec!["a".to_string(), "".to_string()]));
        let row = set.local_row(id).expect("刚登记");
        assert_eq!(row.cells, vec!["a", ""]);
        assert_eq!(set.local_count(), 1);
        assert!(!set.is_empty());
    }

    #[test]
    fn remove_local_returns_snapshot_for_undo() {
        let mut set = LocalRowSet::new();
        let id = set.next_id();
        set.push_local(LocalRow::new(id, vec!["x".to_string()]));
        let snapshot = set.remove_local(id).expect("存在");
        assert_eq!(snapshot.id, id);
        assert_eq!(snapshot.cells, vec!["x"]);
        assert!(set.remove_local(id).is_none(), "重复移除 = None");
        assert_eq!(set.local_count(), 0);
    }

    #[test]
    fn local_row_mut_edits_cells_in_place() {
        let mut set = LocalRowSet::new();
        let id = set.next_id();
        set.push_local(LocalRow::new(id, vec!["old".to_string()]));
        let row = set.local_row_mut(id).expect("存在");
        row.cells[0] = "new".to_string();
        assert_eq!(set.local_row(id).unwrap().cells, vec!["new"]);
    }

    // ===== 底层行删除标记 =====

    #[test]
    fn delete_mark_only_flags_without_real_removal() {
        let mut set = LocalRowSet::new();
        set.mark_deleted(3);
        set.mark_deleted(3);
        assert!(set.is_deleted(3));
        assert_eq!(set.deleted_count(), 1, "重复标记不叠加");
        assert!(!set.is_deleted(0));
    }

    #[test]
    fn unmark_deleted_reports_previous_state() {
        let mut set = LocalRowSet::new();
        set.mark_deleted(1);
        assert!(set.unmark_deleted(1), "之前被标记 = true");
        assert!(!set.unmark_deleted(1), "之前未标记 = false");
        assert_eq!(set.deleted_count(), 0);
    }

    #[test]
    fn clear_drops_local_rows_and_marks_but_keeps_id_counter_moving() {
        let mut set = LocalRowSet::new();
        let id = set.next_id();
        set.push_local(LocalRow::new(id, vec!["y".to_string()]));
        set.mark_deleted(2);
        set.clear();
        assert!(set.is_empty());
        // 计数器不归零：新 id 继续递增，不与已作废快照撞号
        let next = set.next_id();
        assert!(next > id);
    }

    // ===== visible_refs 叠加序列 =====

    #[test]
    fn visible_refs_excludes_deleted_data_rows_and_appends_local() {
        let mut set = LocalRowSet::new();
        set.mark_deleted(1);
        let a = set.next_id();
        let b = set.next_id();
        set.push_local(LocalRow::new(a, vec![]));
        set.push_local(LocalRow::new(b, vec![]));
        let refs = visible_refs(3, &set);
        assert_eq!(
            refs,
            vec![RowRef::data(0), RowRef::data(2), RowRef::local(a), RowRef::local(b)],
            "底层第 1 行被删剔除，本地两行追加在末尾"
        );
    }

    #[test]
    fn visible_refs_empty_when_all_data_rows_deleted() {
        let mut set = LocalRowSet::new();
        set.mark_deleted(0);
        set.mark_deleted(1);
        let refs = visible_refs(2, &set);
        assert!(refs.is_empty());
    }

    #[test]
    fn visible_refs_includes_local_rows_when_no_data_rows() {
        let mut set = LocalRowSet::new();
        let id = set.next_id();
        set.push_local(LocalRow::new(id, vec!["z".to_string()]));
        let refs = visible_refs(0, &set);
        assert_eq!(refs, vec![RowRef::local(id)]);
    }

    // ===== RowHistory 双栈 =====

    fn insert_op(id: u64) -> RowOp {
        RowOp::InsertLocal {
            local_id: id,
            anchor: None,
        }
    }

    #[test]
    fn history_undo_redo_round_trips_step_by_step() {
        let mut history = RowHistory::new();
        assert!(!history.can_undo() && !history.can_redo());

        history.push(insert_op(1));
        history.push(insert_op(2));
        assert_eq!(history.undo_len(), 2);

        let step = history.undo().expect("可撤销");
        assert_eq!(step, insert_op(2));
        assert!(history.can_redo());
        assert_eq!(history.undo_len(), 1);

        let redone = history.redo().expect("可重做");
        assert_eq!(redone, insert_op(2));
        assert!(history.can_undo() && !history.can_redo());
    }

    #[test]
    fn a_new_row_action_clears_the_redo_stack() {
        let mut history = RowHistory::new();
        history.push(insert_op(1));
        history.undo();
        assert!(history.can_redo());
        history.push(insert_op(2));
        assert!(!history.can_redo(), "新动作清空重做栈");
        assert_eq!(history.undo_len(), 1);
    }

    #[test]
    fn history_clear_drops_both_stacks() {
        let mut history = RowHistory::new();
        history.push(insert_op(1));
        history.undo();
        history.clear();
        assert!(!history.can_undo() && !history.can_redo());
    }

    #[test]
    fn row_op_carries_snapshot_needed_for_remove_undo() {
        // RemoveLocal 的快照（cells + anchor）是撤销恢复本地行的全部依据
        let op = RowOp::RemoveLocal {
            local_id: 9,
            cells: vec!["k".to_string(), "v".to_string()],
            anchor: Some(4),
        };
        let RowOp::RemoveLocal {
            local_id,
            cells,
            anchor,
        } = op
        else {
            panic!("应为 RemoveLocal");
        };
        assert_eq!(local_id, 9);
        assert_eq!(cells, vec!["k", "v"]);
        assert_eq!(anchor, Some(4));
    }
}
