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

use crate::channel::ExecChannel;
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
    /// 写语句的真实影响行数（B5；驱动没报就是 `None`，界面不编造）
    pub affected_rows: Option<u32>,
    /// 这份结果是**在哪个连接上**跑出来的（B5 结果工具栏要显示它；`None` = 没绑定）
    pub connection: Option<String>,
    /// 【B13】这份结果是**在哪个通道上**跑出来的（标签徽标 + 切通道后标灰都读它）
    pub channel: ExecChannel,
    /// 【B5b】这一段拿满了没有（拿满 = 可能还有下一段；界面据此摆「取下一段」）
    pub has_more: bool,
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
            affected_rows: None,
            connection: None,
            // 认不出通道时按源库算（默认档；真实通道由 `entry_from` 从结论里带上）
            channel: ExecChannel::default(),
            // 一次拿完的路径（非分段）没有“下一段”可言；分段抓取由 `has_more` 另行标
            has_more: truncated,
            columns,
            rows,
            error: None,
        }
    }

    /// 带上写语句的影响行数（结果区显示「影响 N 行」；没有就不要填）
    pub fn with_affected_rows(mut self, affected_rows: Option<u32>) -> Self {
        self.affected_rows = affected_rows;
        self
    }

    /// 带上这份结果的来源连接（结果工具栏显示它；`None` = 当时没绑定连接）
    pub fn with_connection(mut self, connection: Option<String>) -> Self {
        self.connection = connection;
        self
    }

    /// 【B13】带上这份结果的来源通道（标签徽标与“切通道即失效”的标灰读它）
    pub fn with_channel(mut self, channel: ExecChannel) -> Self {
        self.channel = channel;
        self
    }

    /// 【B5b】标上“这一段拿满了没有”（「取下一段」能不能按就靠它）
    pub fn with_has_more(mut self, has_more: bool) -> Self {
        self.has_more = has_more;
        self
    }

    /// 失败的执行
    pub fn failure(document: DocumentId, sql: String, error: String, elapsed_ms: u64) -> Self {
        Self {
            document,
            sql,
            elapsed_ms,
            truncated: false,
            affected_rows: None,
            connection: None,
            channel: ExecChannel::default(),
            has_more: false,
            columns: Vec::new(),
            rows: Vec::new(),
            error: Some(error),
        }
    }

    /// 这份结果还能不能取下一段（原型 §2.4 的 ⑦：分页 / 取下一段）
    pub fn can_fetch_more(&self) -> bool {
        self.error.is_none() && self.has_more
    }

    /// 【B5b】把新抓到的这一段接在后面（取下一段）
    ///
    /// **列形状不一致就不接**（返回 `false`）：那已经不是“同一份结果的下一段”了，
    /// 调用方应当把它当一份新结果落下，而不是把两行的列错开。
    ///
    /// 耗时取**累加**（抓取这份结果总共花了多久）；`has_more` 以最后一段为准。
    pub fn append_rows(
        &mut self,
        columns: &[String],
        rows: Vec<Vec<String>>,
        elapsed_ms: u64,
        has_more: bool,
    ) -> bool {
        if self.error.is_some() || self.columns != columns {
            return false;
        }
        self.rows.extend(rows);
        self.elapsed_ms = self.elapsed_ms.saturating_add(elapsed_ms);
        self.has_more = has_more;
        true
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

    /// 是不是「只有影响行数、没有结果集」的写语句（结果区要换成一句文案）
    pub fn affects_rows_only(&self) -> bool {
        self.error.is_none() && self.columns.is_empty() && self.affected_rows.is_some()
    }

    /// 结果集文本（TSV：表头一行 + 每行一条；复制到剪贴板用）
    ///
    /// 单元格里出现制表符 / 换行 / 双引号时用双引号包裹、内部引号双写——不转义的话
    /// 带制表符的值会把列错开（Excel / DBeaver 都按这个写法读）。
    ///
    /// 导出的是**已抓到的行**（被截断的那份就只有前若干行）；没有网格时返回空串
    /// （调用方应先看 [`ResultEntry::has_grid`]）。其余导出格式属 B7（`persist.rs`）。
    pub fn to_tsv(&self) -> String {
        if self.columns.is_empty() {
            return String::new();
        }
        let mut text = tsv_row(&self.columns);
        for row in &self.rows {
            text.push('\n');
            text.push_str(&tsv_row(row));
        }
        text
    }

    /// 结果区状态行（真实值，无占位文案）
    pub fn summary(&self) -> String {
        if let Some(error) = &self.error {
            return format!("执行失败：{error}");
        }
        // 写语句没有结果集，只能报影响行数（B5）
        if let Some(affected) = self.affected_rows {
            let mut text = format!("影响 {affected} 行 · {} ms", self.elapsed_ms);
            if self.truncated {
                text.push_str(" · 已截断");
            }
            return text;
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
            // 【B5b】接在选中那份后面（取下一段）：列形状不一致时落成一份新结果
            ResultPlacement::Append => {
                // 失败的那次取段不入库（调用方应当已经把它变成一句提示）：
                // 已经抓到的行是用户的成果，不能因为“再抓失败”就清掉
                if entry.failed() {
                    return;
                }
                let appended = match slot.sets.get_mut(slot.active) {
                    Some(current) => current.append_rows(
                        &entry.columns,
                        entry.rows.clone(),
                        entry.elapsed_ms,
                        entry.has_more,
                    ),
                    None => false,
                };
                if !appended {
                    if let Some(current) = slot.sets.get_mut(slot.active) {
                        *current = entry;
                    } else {
                        slot.sets.push(entry);
                        slot.active = 0;
                    }
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

/// 一行的 TSV（单元格按需加引号）
fn tsv_row(cells: &[String]) -> String {
    let mut line = String::new();
    for (index, cell) in cells.iter().enumerate() {
        if index > 0 {
            line.push('\t');
        }
        if needs_quotes(cell) {
            line.push('"');
            line.push_str(&cell.replace('"', "\"\""));
            line.push('"');
        } else {
            line.push_str(cell);
        }
    }
    line
}

/// 单元格是不是必须加引号（制表符 / 换行 / 双引号——只有它们会破坏 TSV 的形状）
fn needs_quotes(cell: &str) -> bool {
    cell.contains(['\t', '\n', '\r', '"'])
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

    /// 写语句：状态行报影响行数，不当成「0 行 0 列」的结果集
    #[test]
    fn writes_report_affected_rows_instead_of_a_grid() {
        let entry = ResultEntry::success(
            DocumentId::new("doc-1"),
            "insert into t values (1)".to_string(),
            7,
            false,
            Vec::new(),
            Vec::new(),
        )
        .with_affected_rows(Some(3));

        assert!(entry.affects_rows_only(), "只有影响行数");
        assert!(!entry.has_grid(), "写语句没结果集");
        assert_eq!(entry.summary(), "影响 3 行 · 7 ms");
    }

    /// 复制成 TSV：表头 + 行；制表符 / 换行 / 引号要转义（不转义会把列错开）
    #[test]
    fn tsv_quotes_cells_that_would_break_the_shape() {
        let mut entry = entry("doc-1", "select 1", 0);
        entry.columns = vec!["id".to_string(), "note".to_string()];
        entry.rows = vec![
            vec!["1".to_string(), "plain".to_string()],
            vec!["2".to_string(), "two\tcells".to_string()],
            vec!["3".to_string(), "line\nbreak".to_string()],
            vec!["4".to_string(), "say \"hi\"".to_string()],
        ];
        assert_eq!(
            entry.to_tsv(),
            "id\tnote\n1\tplain\n2\t\"two\tcells\"\n3\t\"line\nbreak\"\n4\t\"say \"\"hi\"\"\""
        );
    }

    /// 没有网格就没有可复制的文本（空串，不是一行空表头）
    #[test]
    fn tsv_is_empty_without_a_grid() {
        let failed = ResultEntry::failure(
            DocumentId::new("doc-1"),
            "select boom".to_string(),
            "驱动报错：boom".to_string(),
            3,
        );
        assert_eq!(failed.to_tsv(), "");

        let write = ResultEntry::success(
            DocumentId::new("doc-1"),
            "delete from t".to_string(),
            1,
            false,
            Vec::new(),
            Vec::new(),
        )
        .with_affected_rows(Some(0));
        assert_eq!(write.to_tsv(), "");
    }

    /// 影响行数与来源连接都是"有就给、没就不填"的真值
    #[test]
    fn affected_rows_and_connection_are_optional_truth() {
        let plain = entry("doc-1", "select 1", 1);
        assert_eq!(plain.affected_rows, None);
        assert_eq!(plain.connection, None);

        let tagged = entry("doc-1", "select 1", 1)
            .with_affected_rows(Some(0))
            .with_connection(Some("conn-1".to_string()));
        assert_eq!(tagged.affected_rows, Some(0), "零行也是真值，不等于没有");
        assert_eq!(tagged.connection.as_deref(), Some("conn-1"));
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
        store.push(first, ResultPlacement::Replace);
        assert!(
            store.active(&document).expect("有结果").can_fetch_more(),
            "第一段拿满了 → 还能取下一段"
        );

        let mut second = entry("doc-1", "select n from t", 0);
        second.columns = vec!["n".to_string()];
        second.rows = vec![vec!["3".to_string()], vec!["4".to_string()]];
        second.elapsed_ms = 3;
        second.has_more = false;
        store.push(second, ResultPlacement::Append);

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
        store.push(first, ResultPlacement::Replace);

        let mut other = entry("doc-1", "select n from t", 0);
        other.columns = vec!["n".to_string(), "m".to_string()];
        other.rows = vec![vec!["1".to_string(), "2".to_string()]];
        store.push(other, ResultPlacement::Append);

        let active = store.active(&document).expect("有结果");
        assert_eq!(active.row_count(), 1, "形状不一致时不接");
        assert_eq!(active.columns, ["n".to_string(), "m".to_string()]);
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
