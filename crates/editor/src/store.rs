//! 结果存储（**结果集的唯一权威**）
//!
//! 视图只是投影：结果网格从 `ResultStore` 读列与行，自己不缓存数据（架构 §4 状态地图、
//! 不变式"结果只有 `ResultStore`"）。这样将来加第二个视图（内联输出 / 导出 / 洞察）
//! 时不会出现"两个视图各存一份、对不上"。
//!
//! ## 1a 的范围
//!
//! 每份文档保留**最近一次**结果（`push` 替换同文档的旧结果）。多结果标签（执行历史、
//! 血缘、通道徽标）属 1b——那时把 `Vec` 换成按 run_id 索引的列表即可，接口形态不变。

use crate::model::DocumentId;

/// 一次执行在结果区的完整记录
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultEntry {
    pub document: DocumentId,
    /// 实际执行的 SQL（结果区标题与历史用）
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

/// 结果集集合（按文档，1a 每文档一条）
#[derive(Debug, Default)]
pub struct ResultStore {
    entries: Vec<ResultEntry>,
}

impl ResultStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次执行结果（同一文档的旧结果被替换）
    pub fn push(&mut self, entry: ResultEntry) {
        if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|existing| existing.document == entry.document)
        {
            *slot = entry;
            return;
        }
        self.entries.push(entry);
    }

    /// 某文档最近一次的结果
    pub fn latest(&self, document: &DocumentId) -> Option<&ResultEntry> {
        self.entries
            .iter()
            .find(|entry| &entry.document == document)
    }

    /// 丢弃某文档的结果（文档关闭时调用——结果不跟着已关闭的文档留着）
    pub fn clear(&mut self, document: &DocumentId) {
        self.entries.retain(|entry| &entry.document != document);
    }

    /// 已记录的结果数（每文档至多一条）
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{ResultEntry, ResultStore};
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

    #[test]
    fn pushing_the_same_document_replaces_its_result() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1));
        store.push(entry("doc-1", "select 2", 3));

        assert_eq!(store.len(), 1, "每文档只留最近一次");
        let latest = store.latest(&DocumentId::new("doc-1")).expect("有结果");
        assert_eq!(latest.sql, "select 2");
        assert_eq!(latest.row_count(), 3);
    }

    #[test]
    fn documents_keep_their_own_results() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1));
        store.push(entry("doc-2", "select 2", 2));
        assert_eq!(store.len(), 2);
        assert_eq!(
            store.latest(&DocumentId::new("doc-2")).map(|e| e.sql.clone()),
            Some("select 2".to_string())
        );
    }

    #[test]
    fn closing_a_document_drops_its_result() {
        let mut store = ResultStore::new();
        store.push(entry("doc-1", "select 1", 1));
        store.clear(&DocumentId::new("doc-1"));
        assert!(store.is_empty());
        assert!(store.latest(&DocumentId::new("doc-1")).is_none());
    }

    #[test]
    fn failures_are_recorded_with_their_reason_and_no_grid() {
        let mut store = ResultStore::new();
        store.push(ResultEntry::failure(
            DocumentId::new("doc-1"),
            "select boom".to_string(),
            "驱动报错：boom".to_string(),
            3,
        ));
        let latest = store.latest(&DocumentId::new("doc-1")).expect("有记录");
        assert!(!latest.has_grid(), "失败没有网格");
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
}
