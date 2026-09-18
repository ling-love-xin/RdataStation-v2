//! DuckDB 分析入口的**纯模型**（B15 切片一）
//!
//! ## 这件事在做什么
//!
//! 结果集 → 「分析 ▾」→ 在**本地 DuckDB** 上对这份结果跑一条聚合 SQL → 落**新结果集**。
//! 原结果一行不动（与下发筛选 / 排序同一口径：产生新结果集，不就地替换）。
//!
//! ## 数据从哪来（**桥接已抓到的行**）
//!
//! 分析**不重跑源库查询**：把当前结果集已经抓到的行（`columns` + `rows`）建成一张 DuckDB
//! 临时表，再在它上面跑分析 SQL。这样三件事同时成立：
//!
//! 1. 源库连接断了 / 慢查询不想再跑一遍，分析照样能用；
//! 2. 分析的对象**就是用户眼前这份数据**（不会悄悄换成“重新查一遍”的结果）；
//! 3. 与「只导已抓取的行」（B7）口径一致——**没抓到的行不参与**，并在结果里说清楚。
//!
//! 代价也得说清楚：**值按展示文本进 DuckDB**（`NULL` → 真 NULL，其余是字符串）。
//! 数字列会被推断成数值类型（引擎按整列推断），但 `id = "007"` 这类**前导零的文本**会按
//! 数字落库；要按原样比较就在分析 SQL 里 `CAST(... AS VARCHAR)`。
//!
//! ## 为什么只给这几条预置
//!
//! 菜单**只摆真能跑、且结果可预期**的项：计数、逐列分组计数。更复杂的需求属于“自己写 SQL”
//! （切片二的自定义分析 SQL），而不是在这里猜用户想算什么。

use crate::model::DocumentId;
use crate::store::ResultEntry;

/// 一次分析请求：分析 SQL + 桥接用的行集（`{table}` 会被执行器替换成临时表名）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisRequest {
    /// 分析 SQL（引擎把 `{table}` 换成真正的临时表名）
    pub sql: String,
    /// 桥接的行集列名
    pub columns: Vec<String>,
    /// 桥接的行集（**已字符串化**，与网格一致；`NULL` 是字面量）
    pub rows: Vec<Vec<String>>,
    /// 因为太大被截掉了多少行（0 = 没截）
    pub dropped_rows: usize,
}

impl AnalysisRequest {
    /// 桥接的行数（结果集标签与状态栏文案用）
    pub fn bridged_rows(&self) -> usize {
        self.rows.len()
    }

    /// 落进结果区的说明（“基于多少行、有没有截”都在这里说清楚）
    pub fn notice(&self) -> String {
        if self.dropped_rows == 0 {
            format!("本地 DuckDB 分析 · 基于已抓取的 {} 行", self.rows.len())
        } else {
            format!(
                "本地 DuckDB 分析 · 基于已抓取的 {} 行（另有 {} 行超出上限未参与）",
                self.rows.len(),
                self.dropped_rows
            )
        }
    }
}

/// 桥接行数的上限（超过就截断，并在结果里如实说明）
///
/// 为什么是这个量级：分析要把行**逐行**写进 DuckDB（参数化 INSERT），20 万行会让“点一下
/// 分析”变成一次可感知的等待；20000 行在毫秒级，且覆盖绝大多数“看一眼统计”的场景。
pub const ANALYSIS_MAX_ROWS: usize = 20_000;

/// 菜单里逐列分组最多摆几列（再多菜单就没法用了；不等于不能分析其他列——自己写 SQL）
pub const ANALYSIS_MAX_COLUMN_ITEMS: usize = 8;

/// 结果集标题（B10 那套 `pending_labels` 用它，标签上直接写「分析」）
pub const ANALYSIS_TITLE: &str = "分析";

/// 菜单里的一项（纯数据：`sql` 已经带 `{table}` 占位符）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisItem {
    pub label: String,
    pub sql: String,
}

/// 分析菜单（**纯函数**：只看结果集，不碰执行器）
///
/// 没有网格（失败 / 写语句）就不给分析——那种结果没有行可桥接。
pub fn menu_items(entry: &ResultEntry) -> Vec<AnalysisItem> {
    if !entry.has_grid() || entry.columns.is_empty() {
        return Vec::new();
    }
    let mut items = vec![AnalysisItem {
        label: "计数行数".to_string(),
        sql: format!("SELECT count(*) AS \"行数\" FROM {{table}}"),
    }];
    for column in entry.columns.iter().take(ANALYSIS_MAX_COLUMN_ITEMS) {
        let quoted = quote_identifier(column);
        items.push(AnalysisItem {
            label: format!("按「{column}」分组计数"),
            sql: format!(
                "SELECT {quoted} AS \"值\", count(*) AS \"计数\" FROM {{table}} \
                 GROUP BY 1 ORDER BY 2 DESC"
            ),
        });
    }
    items
}

/// 结果集 → 分析请求（**纯函数**：桥接当前已抓到的行，超出上限就截断并计数）
pub fn request_for(entry: &ResultEntry, sql: String) -> AnalysisRequest {
    let total = entry.rows.len();
    let rows: Vec<Vec<String>> = if total > ANALYSIS_MAX_ROWS {
        entry.rows[..ANALYSIS_MAX_ROWS].to_vec()
    } else {
        entry.rows.clone()
    };
    AnalysisRequest {
        sql,
        columns: entry.columns.clone(),
        rows,
        dropped_rows: total.saturating_sub(ANALYSIS_MAX_ROWS),
    }
}

/// 结果集 → 分析请求（按菜单项；刻意不猜“分析哪份结果”——调用方给的就是选中那份）
pub fn request_for_item(entry: &ResultEntry, item: &AnalysisItem) -> AnalysisRequest {
    request_for(entry, item.sql.clone())
}

/// 一张分析结果的空壳（执行器回填用：把 `QueryData` 落成结果记录）
///
/// 放在这里而不是面板里：**“分析结果长什么样”是模型问题**（标题 / 通道 / 桥接说明），
/// 面板只负责画。
pub fn result_entry(document: &DocumentId, request: &AnalysisRequest, data: &crate::execution::QueryData) -> ResultEntry {
    ResultEntry::success(
        document.clone(),
        request.sql.clone(),
        data.elapsed_ms,
        data.truncated,
        data.columns.clone(),
        data.rows.clone(),
    )
    .with_has_more(false)
    .with_analysis(true)
}

/// SQL 标识符加引号（内部双引号按 SQL 标准双写）
fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        ANALYSIS_TITLE, AnalysisItem, menu_items, request_for, request_for_item, result_entry,
    };
    use crate::execution::QueryData;
    use crate::model::DocumentId;
    use crate::store::ResultEntry;

    fn entry(columns: &[&str], rows: &[&[&str]]) -> ResultEntry {
        ResultEntry::success(
            DocumentId::new("doc"),
            "select * from t".to_string(),
            12,
            false,
            columns.iter().map(|c| c.to_string()).collect(),
            rows.iter()
                .map(|row| row.iter().map(|v| v.to_string()).collect())
                .collect(),
        )
    }

    #[test]
    fn the_menu_offers_count_and_per_column_grouping() {
        let items = menu_items(&entry(&["id", "tag"], &[&["1", "a"]]));
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["计数行数", "按「id」分组计数", "按「tag」分组计数"]);
        assert_eq!(
            items[0].sql, "SELECT count(*) AS \"行数\" FROM {table}",
            "占位符留给引擎替换：{}",
            items[0].sql
        );
        assert_eq!(
            items[1].sql,
            "SELECT \"id\" AS \"值\", count(*) AS \"计数\" FROM {table} GROUP BY 1 ORDER BY 2 DESC"
        );
    }

    #[test]
    fn a_column_with_quotes_is_escaped() {
        let items = menu_items(&entry(&["a\"b"], &[&["1"]]));
        assert!(
            items[1].sql.contains("\"a\"\"b\""),
            "标识符里的双引号要双写：{}",
            items[1].sql
        );
    }

    #[test]
    fn no_grid_means_no_analysis() {
        // 写语句 / 失败：没有行可桥接
        let write = ResultEntry::success(
            DocumentId::new("doc"),
            "insert into t values (1)".to_string(),
            3,
            false,
            Vec::new(),
            Vec::new(),
        )
        .with_affected_rows(Some(1));
        assert!(menu_items(&write).is_empty());

        let failed = ResultEntry::failure(
            DocumentId::new("doc"),
            "select nope".to_string(),
            "no such column".to_string(),
            0,
        );
        assert!(menu_items(&failed).is_empty());
    }

    #[test]
    fn the_bridge_takes_the_rows_and_caps_them() {
        let small = request_for_item(
            &entry(&["id"], &[&["1"], &["2"]]),
            &AnalysisItem {
                label: "计数行数".to_string(),
                sql: "SELECT count(*) FROM {table}".to_string(),
            },
        );
        assert_eq!(small.bridged_rows(), 2);
        assert_eq!(small.dropped_rows, 0);
        assert!(small.notice().contains("基于已抓取的 2 行"), "{}", small.notice());

        // 超过上限：截到上限并如实计数
        let rows: Vec<Vec<String>> = (0..(super::ANALYSIS_MAX_ROWS + 5))
            .map(|i| vec![i.to_string()])
            .collect();
        let big = ResultEntry::success(
            DocumentId::new("doc"),
            "select * from big".to_string(),
            1,
            false,
            vec!["id".to_string()],
            rows,
        );
        let capped = request_for(&big, "SELECT count(*) FROM {table}".to_string());
        assert_eq!(capped.bridged_rows(), super::ANALYSIS_MAX_ROWS);
        assert_eq!(capped.dropped_rows, 5);
        assert!(capped.notice().contains("5 行超出上限未参与"), "{}", capped.notice());
    }

    #[test]
    fn an_analysis_result_is_marked_and_titled_by_the_caller() {
        let request = request_for(&entry(&["id"], &[&["1"]]), "SELECT count(*) FROM {table}".to_string());
        let data = QueryData {
            columns: vec!["行数".to_string()],
            rows: vec![vec!["1".to_string()]],
            elapsed_ms: 4,
            truncated: false,
            notice: None,
            ..Default::default()
        };
        let result = result_entry(&DocumentId::new("doc"), &request, &data);
        assert!(result.analysis, "分析结果要有标记（工具栏据此不摆“刷新”）");
        assert!(!result.can_fetch_more(), "分析结果没有“下一段”");
        assert_eq!(result.row_count(), 1);
        assert_eq!(ANALYSIS_TITLE, "分析");
    }
}
