//! 结果存储（**结果集的唯一权威**）
//!
//! 视图只是投影：结果网格从 `ResultStore` 读列与行，自己不缓存数据（架构 §4 状态地图、
//! 不变式"结果只有 `ResultStore`"）。这样将来加第二个视图（内联输出 / 导出 / 洞察）
//! 时不会出现"两个视图各存一份、对不上"。
//!
//! ## 每份文档一份**结果集列表**（B2）
//!
//! 原型 §2.4：「每次执行产生一个结果集」——批量执行每句一个、重查与分析派生新结果集
//! 而不就地覆盖。因此每份文档记的是 `Vec<ResultEntry>` + **当前选中项**：
//!
//! - [`ResultPlacement::Replace`]：替换**当前选中**的那一份（普通执行的既有语义）；
//! - [`ResultPlacement::NewSet`]：追加一份新的，**选中项不动**（原型 §4.4：别动用户在看的
//!   那一份，新的放旁边）——首次执行没有可保留的选中项时才落到新的一份上。
//!
//! 上限 [`MAX_RESULT_SETS`]（原型 §2.4：上限 5、超出淘汰最旧）淘汰的是**最旧的未选中项**：
//! 正在看的那一份永远不会被自己的下一个结果挤掉。

use crate::execution::ResultPlacement;
use crate::model::DocumentId;

/// 每份文档保留的结果集上限（原型 §2.4；超出淘汰最旧的**未选中**项）
pub const MAX_RESULT_SETS: usize = 5;

/// 一次执行在结果区的完整记录（= 一个结果集）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultEntry {
    pub document: DocumentId,
    /// 实际执行的 SQL（结果区标题与历史用；结果集标签的悬停摘要也用它）
    pub sql: String,
    /// 执行耗时（毫秒，驱动给的真实值）
    pub elapsed_ms: u64,
    /// 结果是否被截断（超出驱动行数上限）
    pub truncated: bool,
    /// 列名（失败时为空）
    pub columns: Vec<String>,
    /// 行数据（已字符串化；失败时为空）
    pub rows: Vec<Vec<String>>,
    /// 失败原因；`None` = 成功
    pub error: Option<String>,
}

impl ResultEntry {
    /// 成功的执行结果
    pub fn success(
        document: DocumentId,
        sql: String,
        elapsed_ms: u64,
        truncated: bool,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    ) -> Self {
        Self {
            document,
            sql,
            elapsed_ms,
            truncated,
            columns,
            rows,
            error: None,
        }
    }

    /// 失败的执行
    pub fn failure(document: DocumentId, sql: String, error: String, elapsed_ms: u64) -> Self {
        Self {
            document,
            sql,
            elapsed_ms,
            truncated: false,
            columns: Vec::new(),
            rows: Vec::new(),
            error: Some(error),
        }
    }

    /// 结果行数（真实值：来自行数据，不读驱动的 `total_rows` 字段，见架构 §12 #21）
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 是否可渲染成网格（成功且有列）
    pub fn has_grid(&self) -> bool {
        self.error.is_none() && !self.columns.is_empty()
    }

    /// 是不是失败的结果集（结果集标签的 `danger` 点用它）
    pub fn failed(&self) -> bool {
        self.error.is_some()
    }

    /// 结果区状态行（真实值，无占位文案）
    pub fn summary(&self) -> String {
        if let Some(error) = &self.error {
            return format!("执行失败：{error}");
        }
        let mut text = format!("{} 行 × {} 列 · {} ms", self.row_count(), self.columns.len(), self.elapsed_ms);
        if self.truncated {
            text.push_str(" · 已截断");
        }
        text
    }
}

/// 一份文档的结果集（列表 + 选中项）
#[derive(Debug)]
struct DocumentResults {
    document: DocumentId,
    sets: Vec<ResultEntry>,
    /// 当前选中的结果集下标（始终指向 `sets` 中的有效位置；`sets` 非空时必有）
    active: usize,
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
    pub fn push(&mut self, entry: ResultEntry, placement: ResultPlacement) {
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
                    active: 0,
                });
                self.documents.last_mut().expect("刚插入")
            }
        };

        match placement {
            // 替换当前选中的那一份（第一次执行时为唯一的一份）
            ResultPlacement::Replace => {
                if let Some(current) = slot.sets.get_mut(slot.active) {
                    *current = entry;
                } else {
                    slot.sets.push(entry);
                    slot.active = 0;
                }
            }
            // 追加一份新的，选中项**不动**；只有“还没有任何结果”时新结果才需要被选中
            ResultPlacement::NewSet => {
                slot.sets.push(entry);
                if slot.sets.len() == 1 {
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

    /// 选中某个结果集（标签条点击；下标越界 = 不改，返回是否真的改了）
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
        true
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
fn evict_oldest(slot: &mut DocumentResults) {
    while slot.sets.len() > MAX_RESULT_SETS {
        let victim = (0..slot.sets.len())
            .find(|index| *index != slot.active)
            .expect("选中项只有一个，上限 > 1 时必有可淘汰项");
        slot.sets.remove(victim);
        if victim < slot.active {
            slot.active -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{MAX_RESULT_SETS, ResultEntry, ResultStore};
    use crate::execution::ResultPlacement;
    use crate::model::DocumentId;

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
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace);
        store.push(entry("doc-1", "select 2", 3), ResultPlacement::Replace);

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
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace);
        store.push(entry("doc-2", "select 2", 2), ResultPlacement::Replace);
        assert_eq!(store.len(), 2);
        assert_eq!(active_sql(&store, "doc-2"), Some("select 2".to_string()));
    }

    #[test]
    fn closing_a_document_drops_its_result() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1), ResultPlacement::Replace);
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
        );
        let latest = store.active(&DocumentId::new("doc-1")).expect("有记录");
        assert!(!latest.has_grid(), "失败没有网格");
        assert!(latest.failed(), "失败要能被标签条标红");
        assert_eq!(latest.row_count(), 0);
        assert!(latest.summary().contains("boom"), "{}", latest.summary());
    }

    #[test]
    fn summary_reports_real_numbers_only() {
        let mut entry = entry("doc-1", "select 1", 4);
        entry.elapsed_ms = 9;
        assert_eq!(entry.summary(), "4 行 × 1 列 · 9 ms");

        entry.truncated = true;
        assert!(entry.summary().contains("已截断"), "{}", entry.summary());
    }

    /// 新结果集落位：追加一份，用户在看的旧那份**仍然选中**（原型 §4.4）
    #[test]
    fn a_new_set_keeps_the_previous_one_selected() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "first", 1), ResultPlacement::NewSet);
        store.push(entry("doc-1", "second", 2), ResultPlacement::NewSet);

        assert_eq!(store.set_count(&document), 2);
        assert_eq!(store.active_index(&document), Some(0), "选中项不动");
        assert_eq!(active_sql(&store, "doc-1"), Some("first".to_string()));

        let sqls: Vec<String> = store.sets(&document).iter().map(|e| e.sql.clone()).collect();
        assert_eq!(sqls, ["first".to_string(), "second".to_string()], "按产生顺序");
    }

    /// 首次执行没有可保留的选中项：新结果集就是被选中的那份
    #[test]
    fn the_first_set_is_selected_even_when_it_lands_as_a_new_set() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        store.push(entry("doc-1", "first", 1), ResultPlacement::NewSet);

        assert_eq!(store.active_index(&document), Some(0));
        assert_eq!(active_sql(&store, "doc-1"), Some("first".to_string()));
    }

    /// 批量：每句一份，用户可以点开任意一份（选中下标跟着走）
    #[test]
    fn selecting_a_set_moves_the_active_one() {
        let mut store = ResultStore::new();
        let document = DocumentId::new("doc-1");
        for sql in ["s1", "s2", "s3"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet);
        }

        assert!(store.select(&document, 2), "点标签要能切过去");
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
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet);
        }
        // 用户切到第 2 份（下标 1），随后反复执行（Replace 落到选中项，不增加条数）
        assert!(store.select(&document, 1));
        store.push(entry("doc-1", "replace-active", 1), ResultPlacement::Replace);
        assert_eq!(store.set_count(&document), 3);

        for sql in ["s4", "s5", "s6"] {
            store.push(entry("doc-1", sql, 1), ResultPlacement::NewSet);
        }

        assert_eq!(store.set_count(&document), MAX_RESULT_SETS, "上限生效");
        let sqls: Vec<String> = store.sets(&document).iter().map(|e| e.sql.clone()).collect();
        assert!(
            sqls.contains(&"replace-active".to_string()),
            "选中项必须留下：{sqls:?}"
        );
        assert!(!sqls.contains(&"s1".to_string()), "最旧的先走：{sqls:?}");
        assert_eq!(
            active_sql(&store, "doc-1"),
            Some("replace-active".to_string()),
            "淘汰后选中项还是原来那份"
        );
    }
}
