//! DuckDB 分析入口的**纯模型**（B15 切片一）
//!
//! ## 这件事在做什么
//!
//! 结果集 → 「分析 ▾」→ 在**本地 DuckDB** 上对这份结果跑一条聚合 SQL → 落**新结果集**。
//! 原结果一行不动（与下发筛选 / 排序同一口径：产生新结果集，不就地替换）。
//!
//! ## 数据从哪来（**优先走事实档 batches 直灌，显示矩阵为降级档**）
//!
//! 分析**不重跑源库查询**：优先把当前结果集的事实档 Arrow batches 直灌本地 DuckDB
//! 常驻表（类型来自驱动上报的 schema，不猜型）；事实档缺失（失败 / 写语句 / 加速档 /
//! 联邦档 / JSON 往返）才回退到显示矩阵字符串逐行建表（internal 猜型降级档，界面与报告
//! 须显式标注）。两条路径都只覆盖**已抓到的行**，与「只导已抓取的行」（B7）口径一致——
//! 没抓到的行不参与，并在结果里说清楚。
//!
//! 降级档失真边界（事实档缺失时才出现）：值按展示文本进 DuckDB（`NULL` → 真 NULL，
//! 其余是字符串），`infer_type` 只认 JSON 值类型、整列落 `VARCHAR`——数字列被收窄成
//! 文本、类型失真；要按数值比较就在分析 SQL 里 `CAST(... AS DOUBLE)`。事实档直灌路径
//! 不存在这条失真。
//!
//! ## 为什么只给这几条预置
//!
//! 菜单**只摆真能跑、且结果可预期**的项：计数、逐列分组计数。更复杂的需求属于“自己写 SQL”
//! （切片二的自定义分析 SQL），而不是在这里猜用户想算什么。

use crate::store::ResultEntry;
use ::shared::{FactFrame, StoragePref};
use std::sync::Arc;

/// 一次分析请求：分析 SQL + 桥接原料（`{table}` 会被执行器替换成临时表名）
///
/// 【B3】`fact` 是当前结果集事实档的 `Arc` 快照（工作线程内只读，不回写原槽位）；
/// `pref` 是宿主事件路径读设置快照带入的载体偏好（工作线程无 `cx`，不能自己读 global）。
/// 【B4】`source_set` 是触发分析时的原槽位下标：物化回执据此回挂原结果集（不猜当前选中）。
/// 含 `Arc<FactFrame>` 后不可逐值比较，故去掉 `PartialEq, Eq`（与 `QueryData` 同一口径）。
#[derive(Debug, Clone)]
pub struct AnalysisRequest {
    /// 分析 SQL（引擎把 `{table}` 换成真正的临时表名）
    pub sql: String,
    /// 桥接的行集列名（降级档猜型建表用；事实档直灌以 schema 为准）
    pub columns: Vec<String>,
    /// 桥接的行集（**已字符串化**，与网格一致；`NULL` 是字面量；仅降级档使用）
    pub rows: Vec<Vec<String>>,
    /// 因为太大被截掉了多少行（0 = 没截）
    pub dropped_rows: usize,
    /// 事实档快照（`None` = 无事实档，走显示矩阵猜型降级档）
    pub fact: Option<Arc<FactFrame>>,
    /// 载体偏好快照（工作线程内按它 + 事实档估算字节分流，见 `shared::storage_choice`）
    pub pref: StoragePref,
    /// 触发分析时的原结果集槽位下标（物化回执回挂定位用）
    pub source_set: usize,
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

/// 「自定义分析 SQL…」的菜单标签（跟在预置项后面，中间隔一条分割线）
pub const CUSTOM_LABEL: &str = "自定义分析 SQL…";

/// 对话框标题（不带省略号——省略号是菜单项的事）
pub const CUSTOM_TITLE: &str = "自定义分析 SQL";

/// 自定义分析 SQL 的**初始模板**：点开就有一条能跑的东西，改着用比从空白开始强
///
/// 与「计数行数」同一条（都是 `{table}` 占位符；替换在执行器侧）。
pub const CUSTOM_DEFAULT_SQL: &str = "SELECT count(*) AS \"行数\" FROM {table}";

/// 输入框的占位提示（只在用户把模板清空后出现）
pub const CUSTOM_PLACEHOLDER: &str = "SELECT ... FROM {table}";

/// 自定义分析 SQL 的说明（数据从哪来、`{table}` 是什么、超上限的部分不参与）
///
/// 数字取自 [`request_for`] 已经算好的桥接口径（截断与计数只有一处），不在界面侧另算。
pub fn custom_hint(bridged_rows: usize, dropped_rows: usize) -> String {
    let base = format!(
        "在本地 DuckDB 上跑，数据是当前结果集已抓到的 {bridged_rows} 行（不重跑源库）；\
         SQL 里用 {{table}} 指代这张临时表。"
    );
    if dropped_rows == 0 {
        base
    } else {
        format!("{base}另有 {dropped_rows} 行超出上限不参与。")
    }
}

/// 这条分析 SQL 值不值得提交
///
/// 空白不值得：提交了只会拿到一句驱动报错（“Expected a statement”之类），
/// 不如在本地就说清楚“没写东西”。
pub fn is_runnable(sql: &str) -> bool {
    !sql.trim().is_empty()
}

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
        // 与自定义输入框的模板同一条（只有一处字面量，不会两边写歪）
        sql: CUSTOM_DEFAULT_SQL.to_string(),
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
///
/// `fact` / `pref` 由宿主事件路径取当前槽位事实档快照与设置偏好传入（纯函数不读 `cx`、
/// 不碰 store）；无事实档时执行器回退显示矩阵猜型降级档。`source_set` 是原槽位下标，
/// 供物化回执回挂定位。
pub fn request_for(
    entry: &ResultEntry,
    sql: String,
    fact: Option<Arc<FactFrame>>,
    pref: StoragePref,
    source_set: usize,
) -> AnalysisRequest {
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
        fact,
        pref,
        source_set,
    }
}

/// 结果集 → 分析请求（按菜单项；刻意不猜“分析哪份结果”——调用方给的就是选中那份）
pub fn request_for_item(
    entry: &ResultEntry,
    item: &AnalysisItem,
    fact: Option<Arc<FactFrame>>,
    pref: StoragePref,
    source_set: usize,
) -> AnalysisRequest {
    request_for(entry, item.sql.clone(), fact, pref, source_set)
}

/// SQL 标识符加引号（内部双引号按 SQL 标准双写）
fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        AnalysisItem, CUSTOM_DEFAULT_SQL, custom_hint, is_runnable, menu_items, request_for,
        request_for_item,
    };
    use crate::store::ResultEntry;
    use ::shared::{FactFrame, StoragePref};
    use std::sync::Arc;
    use workbench_shell::model::DocumentId;

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
            None,
            StoragePref::Auto,
            0,
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
        let capped = request_for(
            &big,
            "SELECT count(*) FROM {table}".to_string(),
            None,
            StoragePref::Auto,
            0,
        );
        assert_eq!(capped.bridged_rows(), super::ANALYSIS_MAX_ROWS);
        assert_eq!(capped.dropped_rows, 5);
        assert!(capped.notice().contains("5 行超出上限未参与"), "{}", capped.notice());
    }

    /// 【B3】事实档快照与偏好随请求透传（工作线程据此选保真 / 降级档，不读 global）。
    #[test]
    fn request_carries_fact_snapshot_and_pref() {
        use arrow::array::{ArrayRef, Int64Array, RecordBatch};
        use arrow::datatypes::{Field, Schema};

        let array: ArrayRef = Arc::new(Int64Array::from(vec![1i64, 2]));
        let schema = Arc::new(Schema::new(vec![Field::new(
            "id",
            array.data_type().clone(),
            false,
        )]));
        let batch = RecordBatch::try_new(schema, vec![array]).expect("构造 RecordBatch");
        let fact = Arc::new(FactFrame::new(vec!["id".to_string()], vec![batch]).expect("事实档"));

        let request = request_for(
            &entry(&["id"], &[&["1"]]),
            "SELECT count(*) FROM {table}".to_string(),
            Some(fact.clone()),
            StoragePref::Table,
            0,
        );
        assert!(request.fact.is_some(), "事实档快照随请求带入");
        assert_eq!(request.pref, StoragePref::Table);
        assert_eq!(request.source_set, 0, "原槽位下标随请求带入");
        assert_eq!(
            request.fact.expect("有事实档").row_count(),
            2,
            "快照帧行列数不被显示矩阵覆盖"
        );
    }

    #[test]
    fn the_custom_entry_ships_a_runnable_template() {
        // 模板必须是**能直接跑**的（带占位符，不是一句空话）
        assert!(
            CUSTOM_DEFAULT_SQL.contains("{table}"),
            "模板要带占位符：{CUSTOM_DEFAULT_SQL}"
        );
        assert!(is_runnable(CUSTOM_DEFAULT_SQL));
        assert!(!is_runnable("   \n\t "), "空白不值得提交");

        // 说明里的数字来自桥接口径（截与不截两种说法）
        let plain = custom_hint(1200, 0);
        assert!(plain.contains("1200 行"), "{plain}");
        assert!(plain.contains("{table}"), "要告诉用户占位符怎么说：{plain}");
        assert!(!plain.contains("超出上限"), "没截就别提截断：{plain}");
        let capped = custom_hint(super::ANALYSIS_MAX_ROWS, 7);
        assert!(capped.contains("7 行超出上限"), "{capped}");
    }
}
