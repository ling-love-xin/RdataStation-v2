//! 「计算行数」（宿主侧）：对当前结果的原 SQL 跑一条 `count(*)`，把总数报给状态栏。
//!
//! 对齐 DBeaver `ResultSetViewer.createStatusBar0` 的 `rowCountLabel`（`:2155-2180`）：
//! 那是一颗 `ActiveStatusMessage`（图标 `UIIcon.COMPILE`，文案
//! `controls_resultset_viewer_calculate_row_count`，`isActionEnabled() = hasData()`），
//! 点它异步 `readRowCount(monitor)`，结果按 `ROW_COUNT_FORMAT` 显示——**它算的是源库里的
//! 总行数，不是已抓到的行数**，所以必须真的跑一条查询。
//!
//! 两条边界：
//! - **不产生新结果集**：DBeaver 把数字显示在状态行标签上，不开新标签页。本仓同样只
//!   `set_message`，用户的结果标签不会多一份（这也是不走 `execute(..., NewSet)` 的原因）；
//! - **不阻塞 UI 线程**：查询在后台任务里跑（与引用页取外键同一纪律），回主线程只写文案。

use super::super::*;
use crate::ports::editor_exec::{SqlExecuteOptions, SqlService};
use crate::store::{Pushdown, ResultEntry};

/// 把「原 SQL（可带下发筛选）」包成一条计数查询（纯函数，测试直接钉）。
///
/// 形态与 `engine::services::execution_service::re_execute_with_filter` 的改写一致：
/// 原 SQL 包成子查询再 `count(*)`，下发筛选的 `WHERE` 必须带进来——否则用户看着筛后的
/// 结果、报的却是全表行数。排序对计数没有意义，不拼 `ORDER BY`。
pub(crate) fn rows_count_sql(sql: &str, pushdown: Option<&Pushdown>) -> Option<String> {
    let base = sql.trim().trim_end_matches(';').trim();
    if base.is_empty() {
        return None;
    }
    let inner = match pushdown {
        Some(Pushdown::Filtered { filter, .. }) if !filter.trim().is_empty() => {
            format!("SELECT * FROM ({base}) AS _result WHERE {}", filter.trim())
        }
        _ => base.to_string(),
    };
    Some(format!("SELECT count(*) FROM ({inner}) AS _rds_count"))
}

/// 从计数结果里取那个数字（纯函数）：驱动可能给整数、浮点或文本，驾驶统一认这三种。
pub(crate) fn count_from_rows(rows: &[Vec<shared::Value>]) -> Option<u64> {
    let cell = rows.first()?.first()?;
    match cell {
        shared::Value::Int(n) => (*n >= 0).then_some(*n as u64),
        shared::Value::Float(n) => (*n >= 0.0).then_some(*n as u64),
        shared::Value::Text(text) => text.trim().parse::<u64>().ok(),
        shared::Value::Bool(_) | shared::Value::Bytes(_) | shared::Value::Null => None,
    }
}

/// 千分位（与 `rds_result_panel` 状态行同口径：每三位一个逗号，`1234` → `1,234`）。
///
/// 不复用面板的 `thousands`：那是 `grid::status` 的内部函数（未 re-export），
/// 编辑器这边只为一条状态文案引一次不划算。
pub(crate) fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

impl EditorHostPanel {
    /// 【计算行数】对选中结果的原 SQL 跑 `count(*)`，把总数写成状态消息。
    pub(crate) fn count_result_rows(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self
            .shared
            .results_active(&self.document)
            .filter(ResultEntry::has_grid)
        else {
            self.set_message(Some("这份结果没有可计数的 SQL".to_string()), cx);
            return;
        };
        let Some(conn_id) = self.bound_connection() else {
            self.set_message(Some("未绑定连接，算不了总行数".to_string()), cx);
            return;
        };
        let Some(count_sql) = rows_count_sql(&entry.sql, entry.pushdown.as_ref()) else {
            self.set_message(Some("这份结果没有可计数的 SQL".to_string()), cx);
            return;
        };
        let panel = self.result_panel.clone();
        let executor = cx.background_executor().clone();
        let service = SqlService::new(engine::get_connection_manager().clone());
        self.set_message(Some("正在计算总行数…".to_string()), cx);
        cx.spawn(async move |_this, cx| {
            let query = executor.spawn(async move {
                service
                    .execute(
                        Some(conn_id),
                        &count_sql,
                        SqlExecuteOptions {
                            // 计数是辅助查询：不进历史、不走缓存、不占事务
                            record_history: false,
                            use_cache: false,
                            use_transaction: false,
                            timeout_ms: None,
                            channel: None,
                            schema: None,
                            lineage: None,
                        },
                    )
                    .await
            });
            let outcome = query.await;
            let message = match outcome {
                Ok(executed) => match count_from_rows(executed.result.rows.as_slice()) {
                    Some(total) => format!("总行数 {}", thousands(total as usize)),
                    None => "计数查询没有返回数字".to_string(),
                },
                Err(error) => format!("计算总行数失败：{error}"),
            };
            let _ = panel.update(cx, |panel, cx| panel.set_row_count_notice(message, cx));
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{count_from_rows, rows_count_sql};
    use crate::store::Pushdown;

    /// 计数 SQL：原 SQL 包子查询、去掉尾分号；不拼 ORDER BY
    #[test]
    fn count_sql_wraps_the_original_query() {
        assert_eq!(
            rows_count_sql("select * from t;", None).unwrap(),
            "SELECT count(*) FROM (select * from t) AS _rds_count"
        );
        // 空 SQL 不给查询
        assert!(rows_count_sql("   ", None).is_none());
        // 下发筛选的 WHERE 必须带进来（否则报的是全表行数）
        let pushdown = Pushdown::Filtered {
            filter: " id > 3 ".to_string(),
            columns: vec!["id".to_string()],
        };
        let sql = rows_count_sql("select * from t", Some(&pushdown)).unwrap();
        assert!(sql.contains("WHERE id > 3"), "{sql}");
        assert!(sql.starts_with("SELECT count(*) FROM ("), "{sql}");
        // 排序下发对计数无意义，按普通查询处理
        let pushdown = Pushdown::SortedDown {
            column: "id".to_string(),
            descending: true,
        };
        let sql = rows_count_sql("select * from t", Some(&pushdown)).unwrap();
        assert!(!sql.contains("ORDER BY"), "{sql}");
    }

    /// 计数结果取值：整数 / 浮点 / 文本都认，空结果给 `None`
    #[test]
    fn count_reads_numbers_from_the_first_cell() {
        use shared::Value;
        assert_eq!(count_from_rows(&[vec![Value::Int(42)]]), Some(42));
        assert_eq!(count_from_rows(&[vec![Value::Float(42.0)]]), Some(42));
        assert_eq!(count_from_rows(&[vec![Value::Text("1234".into())]]), Some(1234));
        assert_eq!(count_from_rows(&[]), None);
        assert_eq!(count_from_rows(&[vec![]]), None);
        assert_eq!(count_from_rows(&[vec![Value::Text("abc".into())]]), None);
    }

    /// 千分位：三位一逗号（与面板状态行同口径）
    #[test]
    fn thousands_groups_digits() {
        assert_eq!(super::thousands(0), "0");
        assert_eq!(super::thousands(999), "999");
        assert_eq!(super::thousands(1000), "1,000");
        assert_eq!(super::thousands(1234567), "1,234,567");
    }
}
