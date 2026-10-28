//! 结果集集合（`ResultStore`）与逐文档结果槽——自 store.rs 拆分（2026-10-29）
//!
//! 本模块承载结果集列表的集合语义（入槽 / 淘汰 / 追加 / 选中 / 对比）与配套
//! 平行数组（变更集 / 编辑历史 / 本地叠加层 / 行级历史 / 事实档 / 钉住标记）；
//! 条目与列类型见 `entry`，事实档载体见 `fact`。

use ::shared::FactFrame;
use std::sync::Arc;

use crate::changes::{ChangeSet, EditHistory};
use crate::rowset::{LocalRowSet, RowHistory};
use crate::store::{FactState, ResultEntry, ResultPlacement, MAX_RESULT_SETS};
use workbench_shell::model::DocumentId;
/// 【A3】可追加段事实档合并（方案跳 2：Append 可追加分支新段 batches 并入现有帧）
///
/// - 新段 `None`（无 batches 回执）：旧档原样保留，不抹掉已有事实帧；
/// - 旧档 `None` + 新段 `Some`：直接挂新帧；
/// - 旧内存档 + 新段 `Some`：两段 batches 合并后由 [`FactFrame::new`] 统一形状校验，
///   校验失败整档作废 `None`（不保留可能错位的旧帧）；
/// - 旧表档 + 新段 `Some`：第一期保守落**替换**——不向已有常驻表 appender 追加
///   （表档追加需复用原表 schema 与 Appender 句柄，属后置细化项）；新段 `None` 时表档保留。
fn merge_fact_state(current: &mut FactState, incoming: Option<Arc<FactFrame>>) {
    let Some(incoming) = incoming else {
        return;
    };
    match current {
        FactState::InMemory(old) => {
            let mut batches = old.batches().to_vec();
            batches.extend(incoming.batches().iter().cloned());
            match FactFrame::new(old.columns().to_vec(), batches) {
                Ok(merged) => *current = FactState::InMemory(Arc::new(merged)),
                Err(_) => *current = FactState::none(),
            }
        }
        FactState::None => *current = FactState::InMemory(incoming),
        FactState::OnDuckDb(_) => *current = FactState::InMemory(incoming),
    }
}

/// 一份文档的结果集（列表 + 选中项 + 对比项）
#[derive(Debug)]
struct DocumentResults {
    document: DocumentId,
    sets: Vec<ResultEntry>,
    /// 【T2a-3】每份结果集的**未提交变更**（与 `sets` 同序同增减；选中项切换不清空——
    /// 用户切标签看别的结果，回来编辑还在。清空只发生在结果被替换 / 淘汰 / 已提交时）
    changes: Vec<ChangeSet>,
    /// 【撤销】每份结果集的**编辑历史**（撤销 / 重做双栈；与 `changes` 同序同增减——
    /// 历史属于“这次抓到的数据”，结果被替换 / 淘汰时作废，切标签不清空）
    histories: Vec<EditHistory>,
    /// 【#28 行级叠加层】每份结果集的本地行集合 + 删除标记（与 `sets` 同序同增减；
    /// 生命周期同格级变更集：结果被替换 / 淘汰时作废，切标签不清空）
    locals: Vec<LocalRowSet>,
    /// 【#28】每份结果集的**行级历史**（新增 / 复制 / 删除双栈；与格级 `histories`
    /// 并列、互不混栈；同序同增减，结果作废时清空）
    row_histories: Vec<RowHistory>,
    /// 【A2】每份结果集的事实档状态（与 `sets` 同序同增减；替换 / 淘汰时随结果作废，
    /// 切标签不清空）。渲染第一期不读它，分析 / 导出按它选保真路径（A3 接线后生效）
    facts: Vec<FactState>,
    /// 【C1】每份结果集是否被用户钉住（与 `sets` 同序同增减；钉住的 tab 在
    /// [`evict_oldest`] 里跳过——用户显式保留的结果不能被 FIFO 挤掉；会话级，不落盘）
    pinned: Vec<bool>,
    /// 当前选中的结果集下标（始终指向 `sets` 中的有效位置；`sets` 非空时必有）
    active: usize,
    /// 【并排对比】对比选中的结果集下标（`None` = 未开对比；`Some` ≠ `active` 恒成立）
    ///
    /// 对比模式把 `active` 与 `compare` 两份结果并排渲染。切换 `active` 时若切到
    /// `compare` 本身（自己对比自己没有意义），对比项自动清空；`compare` 指向的那份
    /// 被淘汰时也自动清空（它不在了，对比无从谈起）。
    compare: Option<usize>,
}

/// 结果集集合（按文档）
#[derive(Debug, Default)]
pub struct ResultStore {
    documents: Vec<DocumentResults>,
}

impl ResultStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一条结果（落位见 [`ResultPlacement`]）
    ///
    /// `fact` 为回执事实帧候选（方案跳 1：无 batches 回执为 `None`）；分段追加时同形状并入
    /// 当前事实帧，形状不一致随替换分支作废。
    pub fn push(
        &mut self,
        entry: ResultEntry,
        placement: ResultPlacement,
        fact: Option<Arc<FactFrame>>,
    ) {
        let document = entry.document.clone();
        let slot = match self
            .documents
            .iter_mut()
            .find(|slot| slot.document == document)
        {
            Some(slot) => slot,
            None => {
                self.documents.push(DocumentResults {
                    document,
                    sets: Vec::new(),
                    changes: Vec::new(),
                    histories: Vec::new(),
                    locals: Vec::new(),
                    row_histories: Vec::new(),
                    facts: Vec::new(),
                    pinned: Vec::new(),
                    active: 0,
                    compare: None,
                });
                self.documents.last_mut().expect("刚插入")
            }
        };

        match placement {
            // 替换当前选中的那一份（第一次执行时为唯一的一份）
            ResultPlacement::Replace => {
                if let Some(current) = slot.sets.get_mut(slot.active) {
                    *current = entry;
                    // 【T2a-3】结果被覆盖：旧变更集随旧结果一起作废（新结果是新数据）
                    if let Some(changes) = slot.changes.get_mut(slot.active) {
                        *changes = ChangeSet::new();
                    }
                    // 【撤销】同理：旧结果的编辑历史随结果作废
                    if let Some(history) = slot.histories.get_mut(slot.active) {
                        history.clear();
                    }
                    // 【#28】本地叠加层与行级历史随旧结果一起作废
                    if let Some(local) = slot.locals.get_mut(slot.active) {
                        local.clear();
                    }
                    if let Some(row_history) = slot.row_histories.get_mut(slot.active) {
                        row_history.clear();
                    }
                    // 【A3】旧事实档随旧结果作废：新结果是新数据，旧帧没有可归属的对象；
                    // 新回执带事实帧候选则挂上，无候选（DML / 加速档 / 联邦档）走降级档。
                    if let Some(fact_state) = slot.facts.get_mut(slot.active) {
                        *fact_state = FactState::from_fact(fact.clone());
                    }
                    // 【C1】被覆盖的是新结果：钉住态不继承（旧结果已被替换为新结果）
                    if let Some(pinned) = slot.pinned.get_mut(slot.active) {
                        *pinned = false;
                    }
                } else {
                    slot.sets.push(entry);
                    slot.changes.push(ChangeSet::new());
                    slot.histories.push(EditHistory::new());
                    slot.locals.push(LocalRowSet::new());
                    slot.row_histories.push(RowHistory::new());
                    slot.facts.push(FactState::from_fact(fact));
                    slot.pinned.push(false);
                    slot.active = 0;
                }
            }
            // 追加一份新的，并**切到新结果集**（2026-10-27：用户要「每次执行一个 tab 且看
            // 最新结果」；原实现「选中项不动」导致执行后看不到新结果——观感像『一个 tab
            // 容纳了好多结果』。原型 §4.4「别动用户在看的」就此让位给「每次执行看新结果」）
            ResultPlacement::NewSet => {
                slot.sets.push(entry);
                slot.changes.push(ChangeSet::new());
                slot.histories.push(EditHistory::new());
                slot.locals.push(LocalRowSet::new());
                slot.row_histories.push(RowHistory::new());
                slot.facts.push(FactState::from_fact(fact));
                slot.pinned.push(false);
                slot.active = slot.sets.len() - 1;
            }
            // 【B5b】接在选中那份后面（取下一段）：列形状不一致时落成一份新结果
            ResultPlacement::Append => {
                // 失败的那次取段不入库（调用方应当已经把它变成一句提示）：
                // 已经抓到的行是用户的成果，不能因为“再抓失败”就清掉
                if entry.failed() {
                    return;
                }
                // **先判形状，再搬行**：对得上就把这一段**移**进去（换掉原先无条件
                // `entry.rows.clone()` 的整段深拷贝），对不上则整条 entry 原封不动落下。
                let appendable = slot
                    .sets
                    .get(slot.active)
                    .is_some_and(|current| current.accepts_columns(&entry.columns));
                if appendable {
                    let current = slot.sets.get_mut(slot.active).expect("刚判过这一份在");
                    current.append_rows(entry.rows, entry.elapsed_ms, entry.has_more);
                    current.adopt_column_types(entry.column_types);
                    // 【A3】事实档同形状并入：旧段 + 新段 batches 合并重建；新段无 batches 回执保留旧档
                    if let Some(fact_state) = slot.facts.get_mut(slot.active) {
                        merge_fact_state(fact_state, fact);
                    }
                } else if let Some(current) = slot.sets.get_mut(slot.active) {
                    *current = entry;
                    // 同上：形状对不上落成替换，旧变更作废
                    if let Some(changes) = slot.changes.get_mut(slot.active) {
                        *changes = ChangeSet::new();
                    }
                    // 【撤销】旧编辑历史随替换作废
                    if let Some(history) = slot.histories.get_mut(slot.active) {
                        history.clear();
                    }
                    // 【#28】本地叠加层与行级历史随替换作废
                    if let Some(local) = slot.locals.get_mut(slot.active) {
                        local.clear();
                    }
                    if let Some(row_history) = slot.row_histories.get_mut(slot.active) {
                        row_history.clear();
                    }
                    // 【A3】形状对不上 = 新结果：旧帧不再归属本段，挂新回执事实候选
                    if let Some(fact_state) = slot.facts.get_mut(slot.active) {
                        *fact_state = FactState::from_fact(fact.clone());
                    }
                    // 【C1】替换分支同 Replace：新结果不继承旧钉住
                    if let Some(pinned) = slot.pinned.get_mut(slot.active) {
                        *pinned = false;
                    }
                } else {
                    slot.sets.push(entry);
                    slot.changes.push(ChangeSet::new());
                    slot.histories.push(EditHistory::new());
                    slot.locals.push(LocalRowSet::new());
                    slot.row_histories.push(RowHistory::new());
                    slot.facts.push(FactState::from_fact(fact));
                    slot.pinned.push(false);
                    slot.active = 0;
                }
            }
        }
        evict_oldest(slot);
    }

    /// 某文档的结果集（按产生顺序；没有则空切片）
    pub fn sets(&self, document: &DocumentId) -> &[ResultEntry] {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .map(|slot| slot.sets.as_slice())
            .unwrap_or(&[])
    }

    /// 某文档结果集的条数
    pub fn set_count(&self, document: &DocumentId) -> usize {
        self.sets(document).len()
    }

    /// 某文档当前选中的结果集（结果网格显示的就是这一份）
    pub fn active(&self, document: &DocumentId) -> Option<&ResultEntry> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.sets.get(slot.active))
    }

    /// 某文档当前选中的下标（结果集标签条用它画选中态）
    pub fn active_index(&self, document: &DocumentId) -> Option<usize> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document && !slot.sets.is_empty())
            .map(|slot| slot.active)
    }

    /// 【A2】指定文档第 `index` 份结果的事实档状态（越界 / 文档不存在 = `None`）。
    /// 渲染第一期不读它；A3 接线后分析 / 带类型导出按它选保真路径。
    pub fn fact_at(&self, document: &DocumentId, index: usize) -> Option<&FactState> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.facts.get(index))
    }

    /// 【A2】当前选中结果的事实档状态（与 `active` 同槽位同序）
    pub fn active_fact(&self, document: &DocumentId) -> Option<&FactState> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.facts.get(slot.active))
    }

    /// 【B2】物化回写：把常驻表名挂到指定槽位事实帧（分析 / 带类型导出直灌成功后，
    /// 由主线程回填路径调用；重复分析据此复用同一张表、不重建）。
    ///
    /// - 内存档帧：重建一份挂表名的新帧替换槽位句柄（`Arc` 内帧不可变，不能原地改）；
    /// - 表档帧：表名以既有句柄为准，不再二次变更；
    /// - 无事实档 / 越界返回 `false`。
    pub fn set_fact_table(&mut self, document: &DocumentId, index: usize, table: String) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(state) = slot.facts.get_mut(index) else {
            return false;
        };
        match state {
            FactState::None => false,
            FactState::OnDuckDb(_) => true,
            FactState::InMemory(old) => {
                let Ok(mut frame) =
                    FactFrame::new(old.columns().to_vec(), old.batches().to_vec())
                else {
                    return false;
                };
                frame.set_temp_table(table);
                *state = FactState::InMemory(Arc::new(frame));
                true
            }
        }
    }

    /// 【B2】把指定槽位内存档提升为表档：挂常驻表名后释放 batches，状态切 [`FactState::OnDuckDb`]。
    /// 只对内存档生效；无事实档 / 已是表档返回 `false`（已是表档视为成功复用）。
    pub fn promote_fact_to_table(
        &mut self,
        document: &DocumentId,
        index: usize,
        table: String,
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(state) = slot.facts.get_mut(index) else {
            return false;
        };
        match state {
            FactState::None => false,
            FactState::OnDuckDb(_) => true,
            FactState::InMemory(old) => {
                let Ok(mut frame) =
                    FactFrame::new(old.columns().to_vec(), old.batches().to_vec())
                else {
                    return false;
                };
                frame.set_temp_table(table);
                frame.release_batches();
                *state = FactState::OnDuckDb(Arc::new(frame));
                true
            }
        }
    }

    /// 选中某个结果集（标签条点击；下标越界 = 不改，返回是否真的改了）
    ///
    /// **变更集不因切换选中而清空**：变更集按结果集存（`changes` 与 `sets` 同序），
    /// 换标签看别的结果、再切回来，这份的编辑还在——清空只发生在结果被覆盖 / 淘汰 / 提交后。
    pub fn select(&mut self, document: &DocumentId, index: usize) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        if index >= slot.sets.len() || index == slot.active {
            return false;
        }
        slot.active = index;
        // 【并排对比】切到对比项本身 = 自己对比自己，对比失效
        if slot.compare == Some(index) {
            slot.compare = None;
        }
        true
    }

    /// 【并排对比】当前对比选中的下标（`None` = 未开对比；与 `active` 恒不同）
    pub fn compare_index(&self, document: &DocumentId) -> Option<usize> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.compare)
    }

    /// 【并排对比】对比选中的那份结果（并排渲染读它；未开对比 = `None`）
    pub fn compare_entry(&self, document: &DocumentId) -> Option<&ResultEntry> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.compare.and_then(|index| slot.sets.get(index)))
    }

    /// 【并排对比】把第 `index` 份结果设为对比项（标签条「设为对比」；返回是否真的改了）
    ///
    /// 约束：不能是 `active` 本身（自己对比自己没有意义）、下标必须有效。
    /// 同一份结果集作为对比项**没有先决条件**——失败 / 写语句的结果也可并排对照
    /// （错误信息并排看也是对比的一部分），不摆网格与否由渲染层各自决定。
    pub fn select_compare(&mut self, document: &DocumentId, index: usize) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        if index >= slot.sets.len() || index == slot.active {
            return false;
        }
        slot.compare = Some(index);
        true
    }

    /// 【并排对比】点同一份 = 取消对比（标签条再次点击；返回是否真的清掉了）
    pub fn toggle_compare(&mut self, document: &DocumentId, index: usize) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        if slot.compare == Some(index) {
            slot.compare = None;
            return true;
        }
        if index < slot.sets.len() && index != slot.active {
            slot.compare = Some(index);
            return true;
        }
        false
    }

    /// 【C1】重命名结果集标签（会话级，不落盘）。
    ///
    /// - `Some(name)`：覆盖自动标签（`结果 N`），同名 / 空白串会被归一为 `None`（恢复自动标签）；
    /// - `None`：清空自定义标题，回到 `结果 N`。
    ///
    /// 返回是否真的改到（文档不存在 / 下标越界 = `false`）。
    pub fn set_title(&mut self, document: &DocumentId, index: usize, title: Option<String>) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(entry) = slot.sets.get_mut(index) else {
            return false;
        };
        // 空白串视同恢复自动标签（用户清空输入框 = 不想要别名）
        let normalized = match title {
            Some(name) => {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            None => None,
        };
        if entry.title == normalized {
            return false;
        }
        entry.title = normalized;
        true
    }

    /// 【C1】切换钉住态（钉住的结果集不被 FIFO 淘汰；会话级，不落盘）。
    ///
    /// 返回是否真的改到（文档不存在 / 下标越界 = `false`）。
    pub fn toggle_pin(&mut self, document: &DocumentId, index: usize) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(pinned) = slot.pinned.get_mut(index) else {
            return false;
        };
        *pinned = !*pinned;
        true
    }

    /// 【C1】读某份结果集是否被钉住（越界 = `false`）
    pub fn is_pinned(&self, document: &DocumentId, index: usize) -> bool {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.pinned.get(index).copied())
            .unwrap_or(false)
    }

    /// 【C1】整份文档的钉住标记切片（与 `sets` 同序；投影给 [`crate::sets::tabs`] 用）
    pub fn pinned(&self, document: &DocumentId) -> &[bool] {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .map(|slot| slot.pinned.as_slice())
            .unwrap_or(&[])
    }

    /// 【T2a-3】某份结果集的**未提交变更**（只读：UI 标脏格 / 变更计数都读它）
    pub fn change_set(&self, document: &DocumentId, index: usize) -> Option<&ChangeSet> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.changes.get(index))
    }

    /// 【T2a-3】变更某份结果集的变更集（单元格提交 / Esc 撤销 / 提交回执标记都走这里）
    ///
    /// 返回是否真的改到（文档或结果集下标不存在 = `false`，调用方据此决定要不要报错）。
    pub fn mutate_change_set(
        &mut self,
        document: &DocumentId,
        index: usize,
        update: impl FnOnce(&mut ChangeSet),
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(changes) = slot.changes.get_mut(index) else {
            return false;
        };
        update(changes);
        true
    }

    /// 某份结果集的编辑历史（撤销 / 重做栈；没有则 `None`）
    pub fn history(&self, document: &DocumentId, index: usize) -> Option<&EditHistory> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.histories.get(index))
    }

    /// 【撤销】变更某份结果集的编辑历史（编辑提交入栈 / 撤销 / 重做 / 结果作废清空都走这里）
    ///
    /// 与 [`Self::mutate_change_set`] 同口径：文档或结果集下标不存在 = `false`。
    pub fn mutate_history(
        &mut self,
        document: &DocumentId,
        index: usize,
        update: impl FnOnce(&mut EditHistory),
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(history) = slot.histories.get_mut(index) else {
            return false;
        };
        update(history);
        true
    }

    /// 【#28】某份结果集的本地叠加层（本地行 + 删除标记；只读投影 / 计数用）
    pub fn local_rows(&self, document: &DocumentId, index: usize) -> Option<&LocalRowSet> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.locals.get(index))
    }

    /// 【#28】变更某份结果集的本地叠加层（新增 / 复制 / 删除标记都走这里）
    ///
    /// 与 [`Self::mutate_change_set`] 同口径：文档或结果集下标不存在 = `false`。
    pub fn mutate_local_rows(
        &mut self,
        document: &DocumentId,
        index: usize,
        update: impl FnOnce(&mut LocalRowSet),
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(local) = slot.locals.get_mut(index) else {
            return false;
        };
        update(local);
        true
    }

    /// 【#28】某份结果集的行级历史（新增 / 复制 / 删除撤销重做双栈；没有则 `None`）
    pub fn row_history(&self, document: &DocumentId, index: usize) -> Option<&RowHistory> {
        self.documents
            .iter()
            .find(|slot| &slot.document == document)
            .and_then(|slot| slot.row_histories.get(index))
    }

    /// 【#28】变更某份结果集的行级历史（行级动作入栈 / 撤销 / 重做 / 结果作废清空）
    ///
    /// 与 [`Self::mutate_history`] 同口径：文档或结果集下标不存在 = `false`。
    pub fn mutate_row_history(
        &mut self,
        document: &DocumentId,
        index: usize,
        update: impl FnOnce(&mut RowHistory),
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(history) = slot.row_histories.get_mut(index) else {
            return false;
        };
        update(history);
        true
    }

    /// 【T2a】把某一格的新值写进**指定**结果集的行数据（本地应用；返回是否真的改到）
    ///
    /// 提交回执后调用：`index` = 结果集标签条的下标（不是选中项），列按数据列下标。
    /// 行 / 列越界或结果集不存在返回 `false`——调用方（面板）据此决定要不要报错。
    pub fn apply_cell_edit(
        &mut self,
        document: &DocumentId,
        index: usize,
        row: usize,
        column: usize,
        value: impl Into<String>,
    ) -> bool {
        let Some(slot) = self
            .documents
            .iter_mut()
            .find(|slot| &slot.document == document)
        else {
            return false;
        };
        let Some(entry) = slot.sets.get_mut(index) else {
            return false;
        };
        entry.apply_cell(row, column, value)
    }

    /// 丢弃某文档的全部结果（文档关闭时调用——结果不跟着已关闭的文档留着）
    pub fn clear(&mut self, document: &DocumentId) {
        self.documents.retain(|slot| &slot.document != document);
    }

    /// 已记录结果的文档数（不是结果集总数）
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }
}

/// 超过上限就淘汰**最旧的未选中**结果集（选中项保留，选中下标跟着修正）
///
/// 【C1】用户显式钉住（[`DocumentResults::pinned`]）的结果集**永不被 FIFO 淘汰**：
/// 找 victim 时跳过它们；若全被钉住且还超上限就不再淘汰（钉住是用户的硬约束，
/// 内存压力由用户自己去解钉 / 关文档，界面不替他决定）。
fn evict_oldest(slot: &mut DocumentResults) {
    while slot.sets.len() > MAX_RESULT_SETS {
        let Some(victim) = (0..slot.sets.len())
            .find(|index| *index != slot.active && !slot.pinned.get(*index).copied().unwrap_or(false))
        else {
            break;
        };
        slot.sets.remove(victim);
        // 【T2a-3】变更集跟着结果集一起淘汰（被淘汰那份的编辑没有可提交的对象了）
        slot.changes.remove(victim);
        // 【撤销】编辑历史跟着结果集一起淘汰（同理）
        slot.histories.remove(victim);
        // 【#28】本地叠加层与行级历史跟着结果集一起淘汰（同理）
        slot.locals.remove(victim);
        slot.row_histories.remove(victim);
        // 【A2】事实档跟着结果集一起淘汰（被淘汰那份的帧没有可归属的结果集了）
        slot.facts.remove(victim);
        // 【C1】钉住标记跟着结果集一起淘汰（被淘汰那份的标记没对象了）
        slot.pinned.remove(victim);
        if victim < slot.active {
            slot.active -= 1;
        }
        // 【并排对比】对比项跟着被淘汰 / 前移（对比对象不在了就清掉）
        match slot.compare {
            Some(compare) if compare == victim => slot.compare = None,
            Some(compare) if compare > victim => slot.compare = Some(compare - 1),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：显式列举（避免与 gpui-kit 测试宏冲突）
    use super::ResultStore;
    use crate::store::{ColumnKind, FactState, MAX_RESULT_SETS, ResultEntry, ResultPlacement};
    use ::shared::FactFrame;
    use std::sync::Arc;
    use workbench_shell::model::DocumentId;

    fn entry(document: &str, sql: &str, rows: usize) -> ResultEntry {
        ResultEntry::success(
            DocumentId::new(document),
            sql.to_string(),
            12,
            false,
            vec!["n".to_string()],
            (0..rows).map(|i| vec![i.to_string()]).collect(),
        )
    }

    fn active_sql(store: &ResultStore, document: &str) -> Option<String> {
        store
            .active(&DocumentId::new(document))
            .map(|entry| entry.sql.clone())
    }

    #[test]
    fn replacing_the_same_document_keeps_one_result() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-1", "select 2", 3), ResultPlacement::Replace, None);

        assert_eq!(store.set_count(&DocumentId::new("doc-1")), 1, "同一次执行不叠加");
        assert_eq!(active_sql(&store, "doc-1"), Some("select 2".to_string()));
        assert_eq!(
            store.active(&DocumentId::new("doc-1")).map(|e| e.row_count()),
            Some(3)
        );
    }

    #[test]
    fn documents_keep_their_own_results() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-2", "select 2", 2), ResultPlacement::Replace, None);
        assert_eq!(store.len(), 2);
        assert_eq!(active_sql(&store, "doc-2"), Some("select 2".to_string()));
    }

    #[test]
    fn closing_a_document_drops_its_result() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.clear(&DocumentId::new("doc-1"));
        assert!(store.is_empty());
        assert!(store.active(&DocumentId::new("doc-1")).is_none());
    }

    #[test]
    fn failures_are_recorded_with_their_reason_and_no_grid() {
        let mut store = ResultStore::new();
        store.push(
            ResultEntry::failure(
                DocumentId::new("doc-1"),
                "select boom".to_string(),
                "驱动报错：boom".to_string(),
                3,
            ),
            ResultPlacement::Replace,
            None,
        );
        let latest = store.active(&DocumentId::new("doc-1")).expect("有记录");
        assert!(!latest.has_grid(), "失败没有网格");
        assert!(latest.failed(), "失败要能被标签条标红");
        assert_eq!(latest.row_count(), 0);
        assert!(latest.summary().contains("boom"), "{}", latest.summary());
    }

    /// 【B5b】取下一段：接在后面、耗时累加、`has_more` 以最后一段为准、**不新开结果集**
    #[test]
    fn appending_a_segment_extends_the_same_result() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");

        let mut first = entry("doc-1", "select n from t", 0);
        first.columns = vec!["n".to_string()];
        first.rows = vec![vec!["1".to_string()], vec!["2".to_string()]];
        first.elapsed_ms = 5;
        first.has_more = true;
        store.push(first, ResultPlacement::Replace, None);
        assert!(
            store.active(&document).expect("有结果").can_fetch_more(),
            "第一段拿满了 → 还能取下一段"
        );

        let mut second = entry("doc-1", "select n from t", 0);
        second.columns = vec!["n".to_string()];
        second.rows = vec![vec!["3".to_string()], vec!["4".to_string()]];
        second.elapsed_ms = 3;
        second.has_more = false;
        store.push(second, ResultPlacement::Append, None);

        assert_eq!(store.set_count(&document), 1, "取下一段不新开结果集");
        let active = store.active(&document).expect("有结果");
        assert_eq!(active.row_count(), 4, "两段接起来");
        assert_eq!(active.elapsed_ms, 8, "耗时累加（抓这份结果花了多久）");
        assert!(
            !active.can_fetch_more(),
            "最后一段没拿满 → 没有下一段了"
        );
    }

    /// 列形状变了就不接（当一份新结果落下），不把两行的列错开
    #[test]
    fn appending_a_mismatched_shape_replaces_instead_of_mixing_columns() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");

        let mut first = entry("doc-1", "select n from t", 0);
        first.columns = vec!["n".to_string()];
        first.rows = vec![vec!["1".to_string()]];
        store.push(first, ResultPlacement::Replace, None);

        let mut other = entry("doc-1", "select n from t", 0);
        other.columns = vec!["n".to_string(), "m".to_string()];
        other.rows = vec![vec!["1".to_string(), "2".to_string()]];
        store.push(other, ResultPlacement::Append, None);

        let active = store.active(&document).expect("有结果");
        assert_eq!(active.row_count(), 1, "形状不一致时不接");
        assert_eq!(active.columns, ["n".to_string(), "m".to_string()]);
    }

    /// 新结果集落位：追加一份，用户在看的旧那份**仍然选中**（原型 §4.4）
    /// 追加一份新结果后**切到它**（2026-10-27：每次执行 = 一个新 tab，执行后看最新结果；
    /// 原「选中项不动」让执行后看不到新结果，观感像『一个 tab 容纳了好多结果』）
    #[test]
    fn a_new_set_becomes_selected() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "first", 1), ResultPlacement::NewSet, None);
        store.push(entry("doc-1", "second", 2), ResultPlacement::NewSet, None);

        assert_eq!(store.set_count(&document), 2);
        assert_eq!(store.active_index(&document), Some(1), "新结果被选中");
        assert_eq!(active_sql(&store, "doc-1"), Some("second".to_string()));

        let sqls: Vec<String> = store.sets(&document).iter().map(|e| e.sql.clone()).collect();
        assert_eq!(sqls, ["first".to_string(), "second".to_string()], "按产生顺序");
    }

    /// 首次执行没有可保留的选中项：新结果集就是被选中的那份
    #[test]
    fn the_first_set_is_selected_even_when_it_lands_as_a_new_set() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "first", 1), ResultPlacement::NewSet, None);

        assert_eq!(store.active_index(&document), Some(0));
        assert_eq!(active_sql(&store, "doc-1"), Some("first".to_string()));
    }

    /// 批量：每句一份，用户可以点开任意一份（选中下标跟着走）
    #[test]
    fn selecting_a_set_moves_the_active_one() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        for sql in ["s1", "s2", "s3"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet, None);
        }
        // NewSet 追加后 active = 最新（第三次执行 = s3）
        assert_eq!(store.active_index(&document), Some(2));

        assert!(store.select(&document, 0), "点标签要能切过去");
        assert_eq!(store.active_index(&document), Some(0));
        assert_eq!(active_sql(&store, "doc-1"), Some("s1".to_string()));
        assert!(store.select(&document, 2), "再切到第三份");
        assert_eq!(store.active_index(&document), Some(2));
        assert_eq!(active_sql(&store, "doc-1"), Some("s3".to_string()));

        assert!(!store.select(&document, 9), "越界不改状态");
        assert!(!store.select(&document, 2), "点当前项不算变化（不必重绘）");
        assert!(!store.select(&DocumentId::new("doc-x"), 0), "没有这份文档");
        assert_eq!(store.active_index(&document), Some(2));
    }

    /// 上限 5：超出淘汰**最旧的未选中**项，正在看的那份不会被自己的下一个结果挤掉
    #[test]
    fn the_oldest_unselected_set_is_evicted_at_the_cap() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        for sql in ["s1", "s2", "s3"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet, None);
        }
        // 用户切到第 2 份（下标 1），随后「刷新」（Replace 落到选中项，不增加条数）
        assert!(store.select(&document, 1));
        store.push(entry("doc-1", "replace-active", 1), ResultPlacement::Replace, None);
        assert_eq!(store.set_count(&document), 3);
        assert_eq!(active_sql(&store, "doc-1"), Some("replace-active".to_string()));

        for sql in ["s4", "s5", "s6"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet, None);
        }

        assert_eq!(store.set_count(&document), MAX_RESULT_SETS, "上限生效");
        let sqls: Vec<String> = store.sets(&document).iter().map(|e| e.sql.clone()).collect();
        assert!(
            sqls.contains(&"replace-active".to_string()),
            "选中项必须留下：{sqls:?}"
        );
        assert!(!sqls.contains(&"s1".to_string()), "最旧的先走：{sqls:?}");
        // 每次 NewSet 都会切到最新；最后一次执行（s6）是当前选中（2026-10-27）
        assert_eq!(
            active_sql(&store, "doc-1"),
            Some("s6".to_string()),
            "最新执行的结果被选中"
        );
    }

    /// 取下一段：目标本来没带类型、这一段带了就补上；已有类型不被后一段覆盖
    ///
    /// （类型是**列**的属性，分段抓取的列形状相同——后一段的类型不比前一段更权威。）
    #[test]
    fn appending_a_segment_adopts_types_only_when_the_target_has_none() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select n from t", 2),
            ResultPlacement::Replace,
            None,
        );
        assert!(
            store
                .active(&document)
                .expect("有结果")
                .column_types
                .is_empty()
        );

        store.push(
            entry("doc-1", "select n from t", 2).with_column_types(&["bigint".to_string()]),
            ResultPlacement::Append,
            None,
        );
        let active = store.active(&document).expect("有结果");
        assert_eq!(active.row_count(), 4, "这一段真的接在后面了");
        assert_eq!(
            active.column_kind(0),
            ColumnKind::Integer,
            "第一段没类型、这一段带了 → 补上"
        );

        store.push(
            entry("doc-1", "select n from t", 2).with_column_types(&["text".to_string()]),
            ResultPlacement::Append,
            None,
        );
        assert_eq!(
            store.active(&document).expect("有结果").column_kind(0),
            ColumnKind::Integer,
            "已有类型不被后一段覆盖"
        );
    }

    // ===== 【并排对比】对比选中 =====

    /// 对比项与选中项恒不同：设成 active 本身被拒、取消后能再设
    #[test]
    fn compare_cannot_be_the_active_set_itself() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        // 2026-10-27：NewSet 追加后 active = 最新（index 1）
        assert_eq!(store.active_index(&document), Some(1));

        assert!(
            !store.select_compare(&document, 1),
            "active（1）不能当自己的对比项"
        );
        assert_eq!(store.compare_index(&document), None);

        assert!(store.select_compare(&document, 0), "另一份可以做对比项");
        assert_eq!(store.compare_index(&document), Some(0));
        assert!(store.compare_entry(&document).is_some());
    }

    /// 切换选中到对比项本身 → 对比自动清空（自己对比自己没有意义）
    #[test]
    fn selecting_the_compare_set_clears_compare() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        // active = 1（select 2）；选第 0 份做对比
        assert!(store.select_compare(&document, 0));

        assert!(store.select(&document, 0), "切到对比项本身");
        assert_eq!(store.compare_index(&document), None, "自己对比自己失效");

        assert!(store.select(&document, 1), "切回主选中");
        assert!(store.select_compare(&document, 0), "还能再设回来");
    }

    /// 再点同一份 = 取消对比（toggle 语义）；点别的份 = 换对比项
    #[test]
    fn toggle_compare_switches_or_clears() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        store.push(entry("doc-1", "select 3", 3), ResultPlacement::NewSet, None);
        // active = 2（select 3，最新）；选非 active 的份做对比
        assert_eq!(store.active_index(&document), Some(2));

        assert!(store.toggle_compare(&document, 1), "第一次点 = 设上（非 active）");
        assert_eq!(store.compare_index(&document), Some(1));
        assert!(store.toggle_compare(&document, 1), "再点同一份 = 取消");
        assert_eq!(store.compare_index(&document), None);
        assert!(store.toggle_compare(&document, 0), "点别的份 = 换成它");
        assert_eq!(store.compare_index(&document), Some(0));
        assert!(!store.toggle_compare(&document, 99), "越界不改");
    }

    /// 淘汰：对比项被挤掉 → 清空；对比项位置前移 → 跟着减
    #[test]
    fn evicting_the_compare_set_clears_or_rewinds_it() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        // 灌满到上限（MAX_RESULT_SETS）；NewSet 追加后 active = 最新（2026-10-27）
        for i in 0..MAX_RESULT_SETS {
            store.push(
                entry("doc-1", &format!("select {i}"), i + 1),
                if i == 0 {
                    ResultPlacement::Replace
                } else {
                    ResultPlacement::NewSet
                },
                None,
            );
        }
        assert_eq!(store.active_index(&document), Some(MAX_RESULT_SETS - 1));
        // 对比第 0 份（最旧的未选中，非 active）
        assert!(store.select_compare(&document, 0), "对比第 0 份");
        // 再落一份：淘汰最旧的未选中（第 0 份 = 对比项）→ 对比清空
        store.push(
            entry("doc-1", "select new", 1),
            ResultPlacement::NewSet,
            None,
        );
        assert_eq!(store.compare_index(&document), None, "对比对象被淘汰 = 对比失效");

        // 对比第 2 份；再落一份淘汰它之前的 select1（最旧未选中）→ 对比项前移为 1
        assert!(store.select_compare(&document, 2), "对比第 2 份");
        store.push(
            entry("doc-1", "select new2", 1),
            ResultPlacement::NewSet,
            None,
        );
        assert_eq!(store.compare_index(&document), Some(1), "对比项前移为 1");
    }

    /// 【撤销】编辑历史与变更集同序：新结果带空历史，替换作废旧历史
    #[test]
    fn histories_track_sets_lifecycle() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        // 首次执行：历史存在且为空
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(store.history(&document, 0).is_some_and(|h| !h.can_undo()));

        // 追加新结果：历史与 sets 同长
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        assert_eq!(store.sets(&document).len(), 2);
        assert!(store.history(&document, 1).is_some_and(|h| !h.can_undo()));

        // 替换选中那份：历史作废（旧编辑没有可撤销的对象了）——2026-10-27 起执行落到最新
        // 结果集（active=index 1），替换发生在新的一份上
        store.mutate_history(&document, 1, |h| {
            h.push(crate::changes::CellChange::pending(0, 0, "a", "b"));
        });
        assert!(store.history(&document, 1).is_some_and(|h| h.can_undo()));
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(
            store.history(&document, 1).is_some_and(|h| !h.can_undo()),
            "替换后历史清空"
        );
    }

    /// 【撤销】mutate_history 与 mutate_change_set 同口径：文档 / 下标不存在返回 false
    #[test]
    fn mutate_history_refuses_unknown_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(!store.mutate_history(&document, 7, |_| {}), "下标越界 = false");
        assert!(
            !store.mutate_history(&DocumentId::new("nope"), 0, |_| {}),
            "文档不存在 = false"
        );
        assert!(store.mutate_history(&document, 0, |_| {}), "命中 = true");
    }

    /// 【#28】本地叠加层与变更集同序：新结果带空集合，替换作废旧叠加层
    #[test]
    fn locals_track_sets_lifecycle() {
        use crate::rowset::LocalRow;

        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        // 首次执行：本地叠加层存在且为空
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(store
            .local_rows(&document, 0)
            .is_some_and(|set| set.is_empty()));

        // 追加新结果：叠加层与 sets 同长
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        assert_eq!(store.sets(&document).len(), 2);
        assert!(store.local_rows(&document, 1).is_some_and(|set| set.is_empty()));

        // 往最新那份（active=index 1）写本地行 + 删除标记
        store.mutate_local_rows(&document, 1, |set| {
            let id = set.next_id();
            set.push_local(LocalRow::new(id, vec!["".to_string()]));
            set.mark_deleted(0);
        });
        assert!(store
            .local_rows(&document, 1)
            .is_some_and(|set| !set.is_empty()));

        // 替换选中那份：本地叠加层作废（新数据是新结果，旧本地行没有可归属的对象）
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(
            store
                .local_rows(&document, 1)
                .is_some_and(|set| set.is_empty()),
            "替换后本地叠加层清空"
        );
    }

    /// 【#28】行级历史与格级历史同生命周期：替换作废旧行级历史
    #[test]
    fn row_histories_track_sets_lifecycle() {
        use crate::rowset::RowOp;

        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(store
            .row_history(&document, 0)
            .is_some_and(|h| !h.can_undo()));

        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        assert_eq!(store.sets(&document).len(), 2);
        assert!(store.row_history(&document, 1).is_some_and(|h| !h.can_undo()));

        store.mutate_row_history(&document, 1, |h| {
            h.push(RowOp::InsertLocal {
                local_id: 1,
                anchor: None,
            });
        });
        assert!(store.row_history(&document, 1).is_some_and(|h| h.can_undo()));

        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(
            store
                .row_history(&document, 1)
                .is_some_and(|h| !h.can_undo()),
            "替换后行级历史清空"
        );
    }

    /// 【#28】mutate_local_rows 与 mutate_change_set 同口径：文档 / 下标不存在返回 false
    #[test]
    fn mutate_local_rows_refuses_unknown_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(!store.mutate_local_rows(&document, 7, |_| {}), "下标越界 = false");
        assert!(
            !store.mutate_local_rows(&DocumentId::new("nope"), 0, |_| {}),
            "文档不存在 = false"
        );
        assert!(store.mutate_local_rows(&document, 0, |_| {}), "命中 = true");
    }

    /// 【#28】mutate_row_history 同上口径
    #[test]
    fn mutate_row_history_refuses_unknown_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(!store.mutate_row_history(&document, 7, |_| {}), "下标越界 = false");
        assert!(
            !store.mutate_row_history(&DocumentId::new("nope"), 0, |_| {}),
            "文档不存在 = false"
        );
        assert!(store.mutate_row_history(&document, 0, |_| {}), "命中 = true");
    }

    /// 【A2】事实档与结果集同生命周期：新结果入槽为 None 档，替换 / 追加保持同序同长
    #[test]
    fn facts_track_sets_lifecycle() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");

        // 首次执行：事实档存在且为 None（A2 尚未接线内存档构造路径）
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(store.fact_at(&document, 0).is_some_and(|fact| fact.is_none()));
        assert!(store.active_fact(&document).is_some_and(|fact| fact.is_none()));

        // 追加新结果：事实档与 sets 同长同序
        store.push(entry("doc-1", "select 2", 2), ResultPlacement::NewSet, None);
        assert_eq!(store.set_count(&document), 2);
        assert!(store.fact_at(&document, 1).is_some_and(|fact| fact.is_none()));

        // 替换选中那份：事实档回 None 档（旧帧随旧结果作废；槽位仍有效、不越界）
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(store.fact_at(&document, 0).is_some_and(|fact| fact.is_none()));
        assert!(store.active_fact(&document).is_some_and(|fact| fact.is_none()));
    }

    /// 【A2】fact_at / active_fact 越界与未知文档口径同 sets 系列访问器
    #[test]
    fn fact_access_refuses_unknown_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);

        assert!(store.fact_at(&document, 3).is_none(), "下标越界 = None");
        assert!(
            store.fact_at(&DocumentId::new("nope"), 0).is_none(),
            "文档不存在 = None"
        );
        assert!(store.active_fact(&DocumentId::new("nope")).is_none());
    }

    /// 【A2/B2】None 档不带事实帧、占用报 0；表档持释放后的帧，Rust 堆占用同样报 0。
    #[test]
    fn none_and_table_states_report_no_rust_heap_bytes() {
        let none = FactState::none();
        assert!(none.is_none());
        assert!(none.in_memory().is_none());
        assert!(none.on_duckdb().is_none());
        assert_eq!(none.estimated_bytes(), 0);

        let mut frame = fact_frame(&[1, 2]);
        frame.set_temp_table("tmp_q_result_9".to_string());
        frame.release_batches();
        let table = FactState::OnDuckDb(Arc::new(frame));
        assert!(!table.is_none());
        assert!(table.in_memory().is_none());
        assert!(table.on_duckdb().is_some());
        assert_eq!(table.estimated_bytes(), 0, "表档 batches 已释放，Rust 堆占用报 0");
        assert_eq!(table.temp_table(), Some("tmp_q_result_9"));
        assert_eq!(
            table.on_duckdb().expect("表档帧").row_count(),
            2,
            "行数缓存随表档保留"
        );
    }

    /// 【B2】`set_fact_table` 把常驻表名挂到内存档帧；重复调用复用同一句柄。
    #[test]
    fn set_fact_table_attaches_table_name_to_in_memory_slot() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );

        assert!(store.set_fact_table(&document, 0, "tmp_q_result_3".to_string()));
        let fact = store.active_fact(&document).expect("有事实档");
        assert_eq!(fact.temp_table(), Some("tmp_q_result_3"));
        assert!(fact.in_memory().is_some(), "挂表名不释放 batches，仍为内存档");

        // 无事实档 / 越界 = false
        assert!(!store.set_fact_table(&document, 7, "tmp_q_result_x".to_string()));
        assert!(!store.set_fact_table(&DocumentId::new("nope"), 0, "tmp_q_result_x".to_string()));
    }

    /// 【B2】`promote_fact_to_table`：内存档挂表名后释放 batches，状态切表档；
    /// 已是表档复用成功，无事实档拒绝提升。
    #[test]
    fn promote_fact_to_table_releases_batches_and_switches_tier() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );

        assert!(store.promote_fact_to_table(&document, 0, "tmp_q_result_5".to_string()));
        let fact = store.active_fact(&document).expect("有事实档");
        assert!(fact.on_duckdb().is_some(), "状态切表档");
        assert!(fact.in_memory().is_none());
        assert_eq!(fact.temp_table(), Some("tmp_q_result_5"));
        assert_eq!(fact.estimated_bytes(), 0, "表档 Rust 堆占用归零");
        assert_eq!(
            fact.on_duckdb().expect("表档帧").row_count(),
            2,
            "释放后行数保留"
        );

        // 已是表档：再提升视为成功复用，不报错
        assert!(store.promote_fact_to_table(&document, 0, "tmp_q_result_5".to_string()));
    }

    /// 【B2】无事实档槽位拒绝提升；新结果入槽后旧表档随旧结果作废。
    #[test]
    fn promote_refuses_none_fact_slot_and_resets_on_replace() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace, None);
        assert!(
            !store.promote_fact_to_table(&document, 0, "tmp_q_result_x".to_string()),
            "无事实档不能提升"
        );

        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );
        store.promote_fact_to_table(&document, 0, "tmp_q_result_8".to_string());
        // 无候选回执替换：表档帧随旧结果作废回 None
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(store.active_fact(&document).is_some_and(|fact| fact.is_none()));
    }

    /// 【A2】淘汰最旧结果集时事实档平行缩短：不存在“set 没了、fact 残留”的错位
    #[test]
    fn facts_evict_with_sets_at_cap() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        for sql in ["s1", "s2", "s3", "s4", "s5", "s6"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet, None);
        }
        assert_eq!(store.set_count(&document), MAX_RESULT_SETS);
        assert!(store.fact_at(&document, MAX_RESULT_SETS - 1).is_some());
        assert!(store.fact_at(&document, MAX_RESULT_SETS).is_none());
    }

    /// 【A3】Replace 挂入内存事实档；再以无候选回执替换 → 旧帧随旧结果作废回 None
    #[test]
    fn fact_slot_replaces_in_memory_frame_on_new_result() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );
        let active = store.active_fact(&document).expect("有事实档");
        assert!(active.in_memory().is_some(), "回执带 batches → 内存档");
        assert_eq!(active.in_memory().expect("内存档").row_count(), 2);

        // 再替换为无候选回执（DML / 加速档 / 联邦档）：旧帧随旧结果作废
        store.push(entry("doc-1", "select 1b", 1), ResultPlacement::Replace, None);
        assert!(store.active_fact(&document).is_some_and(|fact| fact.is_none()));
    }

    /// 【A3】Append 同形状两段：事实帧 batches 合并、行数累加；新段无候选回执保留旧帧
    #[test]
    fn fact_slot_merges_batches_when_appending_same_shape() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Append,
            Some(Arc::new(fact_frame(&[3, 4]))),
        );
        let active = store.active_fact(&document).expect("有事实档");
        assert_eq!(
            active.in_memory().expect("内存档").row_count(),
            4,
            "两段 batches 合并累加"
        );

        // 新段无 batches 回执：旧帧原样保留，不清空
        store.push(entry("doc-1", "select 1", 2), ResultPlacement::Append, None);
        assert_eq!(
            store
                .active_fact(&document)
                .expect("有事实档")
                .in_memory()
                .expect("内存档保留")
                .row_count(),
            4
        );
    }

    /// 【A3】Append 形状对不上落替换分支：旧事实帧作废，新候选无 batches → None 档
    #[test]
    fn fact_slot_discards_frame_when_append_shape_mismatches() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(
            entry("doc-1", "select 1", 2),
            ResultPlacement::Replace,
            Some(Arc::new(fact_frame(&[1, 2]))),
        );
        let mut other = entry("doc-1", "select x", 1);
        other.columns = vec!["x".to_string()];
        store.push(other, ResultPlacement::Append, None);
        assert!(
            store.active_fact(&document).is_some_and(|fact| fact.is_none()),
            "形状对不上 = 新结果，旧帧不再归属本段"
        );
    }

    /// 【A3】测试 fixture：单列 `n` 整数事实帧（与 `entry` 的列名对齐，两段可合并）
    fn fact_frame(values: &[i64]) -> FactFrame {
        use arrow::array::{ArrayRef, Int64Array, RecordBatch};
        use arrow::datatypes::{Field, Schema};

        let array: ArrayRef = Arc::new(Int64Array::from(values.to_vec()));
        let schema = Arc::new(Schema::new(vec![Field::new(
            "n",
            array.data_type().clone(),
            true,
        )]));
        let batch = RecordBatch::try_new(schema, vec![array]).expect("构造 RecordBatch");
        FactFrame::new(vec!["n".to_string()], vec![batch]).expect("构造事实档")
    }
}
