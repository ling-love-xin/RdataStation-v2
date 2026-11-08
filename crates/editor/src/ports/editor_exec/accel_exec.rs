//! 加速档（DuckDB 本地加速）：跑、取下一段、重跑改写后的 SQL。

use super::*;
use super::convert::*;
use super::history::*;

impl EngineQueryRunner {
    /// 加速档执行（B13）：在 DuckDB 上跑，源库以**只读**方式挂着
    ///
    /// 与源库档同口径：查询只取**第一段**（套窗口），写语句原样（DuckDB 侧只读挂载会
    /// 拒掉作用源对象的写，本地临时对象照常允许）。
    /// `lineage` 随历史落库（【B5 余项】；`run()` 把队列注入的执行血缘递过来）。
    pub(crate) fn run_on_accel(
        &self,
        connection: Option<&str>,
        sql: &str,
        lineage: &str,
    ) -> Result<QueryData, String> {
        let source = self.accel_source(connection)?;
        let session = accel::ensure_session(&source)?;
        let (to_run, segmented) = match window_sql(sql, crate::execution::SEGMENT_ROWS, 0) {
            Some(wrapped) => (wrapped, true),
            None => (sql.to_string(), false),
        };
        let started = std::time::Instant::now();
        let outcome = session.run(&to_run);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_accel_history(&source, sql, elapsed_ms, &outcome, Some(lineage));
        // 【B8】历史面板据此重新加载（与源库档同一节拍）
        crate::history::bump();
        let result = match outcome {
            Ok(ok) => ok,
            Err(error) => {
                tracing::error!(
                    target = "rds_workbench::exec",
                    channel = "accelerated",
                    connection = ?connection,
                    source = %source.conn_id,
                    elapsed_ms,
                    error = %error,
                    "本地加速执行失败"
                );
                return Err(error.to_string());
            }
        };
        let mut data = to_data(&result, elapsed_ms, false);
        data.has_more = segmented
            && !data.columns.is_empty()
            && data.rows.len() == crate::execution::SEGMENT_ROWS;
        Ok(data)
    }

    /// 加速档的「取下一段」（与源库档同一套窗口包装，见 `sql_service::window_sql`）
    pub(crate) fn fetch_next_on_accel(
        &self,
        connection: Option<&str>,
        sql: &str,
        offset: usize,
        limit: usize,
    ) -> Result<QueryData, String> {
        let source = self.accel_source(connection)?;
        let session = accel::ensure_session(&source)?;
        let wrapped =
            window_sql(sql, limit, offset).ok_or_else(|| "这份结果没有可再取的部分".to_string())?;
        let started = std::time::Instant::now();
        let outcome = session.run(&wrapped);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_accel_history(
            &source,
            sql,
            elapsed_ms,
            &outcome,
            Some(crate::execution::LINEAGE_SEGMENT),
        );
        crate::history::bump();
        let result = outcome.map_err(|error| error.to_string())?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.has_more = data.rows.len() == limit;
        Ok(data)
    }

    /// 加速档的下发（筛选 / 排序）：改写（引擎的 `sql::rewrite_*`）之后照跑
    pub(crate) fn run_rewritten_on_accel(
        &self,
        connection: Option<&str>,
        rewritten: &engine::sql::Rewrite,
        notice: impl FnOnce(&str) -> String,
        lineage: &str,
    ) -> Result<QueryData, String> {
        let source = self.accel_source(connection)?;
        let session = accel::ensure_session(&source)?;
        let started = std::time::Instant::now();
        let outcome = session.run(&rewritten.sql);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_accel_history(&source, &rewritten.sql, elapsed_ms, &outcome, Some(lineage));
        crate::history::bump();
        let result = outcome.map_err(|error| error.to_string())?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.notice = rewritten.dropped_limit.as_deref().map(notice);
        Ok(data)
    }
}
