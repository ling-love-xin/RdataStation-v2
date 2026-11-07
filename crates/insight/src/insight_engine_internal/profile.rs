//! 表画像计算：临时表内省、列摘要、重复行统计

use shared::error::{CommonError, CoreError};

use super::*;
use crate::model::types::{TableColumnMeta, TableProfile};
use crate::model::ColumnStatBrief;

/// 临时表画像（表探查（Tab「表」）的数据源）：DuckDB 内省列元数据 + 行数。
///
/// 面板的表目标指向的是 **DuckDB 临时表**（结果集表 / `tmp_i_` 样本表）——
/// 源库表先取样再探查（D58/D59），所以这里既没有也不需要源库连接信息。
///
/// 走 `DESCRIBE` 而不是 `information_schema`：前者对临时表 / 视图 / CTE 一视同仁，
/// 也不需要自己按 `table_catalog = 'memory'` 过滤（`ATTACH` 进来的文件库表不该混进来）。
pub fn get_temp_table_profile(temp_table: &str) -> Result<TableProfile, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    get_temp_table_profile_on(&conn, temp_table)
}

/// 在**调用方已持有的连接**上内省临时表（同 `*_on` 系列的口径）。
///
/// `DESCRIBE` 的输出列有六个且顺序稳定：`column_name` / `column_type` / `null` / `key` /
/// `default` / `extra`。这里只取前四个，用**下标**而不是列名定位——`Statement::column_names()`
/// 在查询未执行时会 panic，而 `DESCRIBE` 的列定义是 DuckDB 的稳定契约。
pub fn get_temp_table_profile_on(
    conn: &duckdb::Connection,
    temp_table: &str,
) -> Result<TableProfile, CoreError> {
    let sql = format!("DESCRIBE {}", quote_ident(temp_table));
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB describe prepare failed: {}",
            e
        )))
    })?;

    let columns: Vec<TableColumnMeta> = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let data_type: String = row.get(1)?;
            // `null` 列给的是 "YES" / "NO"
            let nullable: String = row.get::<_, Option<String>>(2)?.unwrap_or_default();
            let key: String = row.get::<_, Option<String>>(3)?.unwrap_or_default();
            Ok((name, data_type, nullable, key))
        })
        .map_err(|e| {
            CoreError::common(CommonError::General(format!(
                "DuckDB describe query failed: {}",
                e
            )))
        })?
        .filter_map(|row| row.ok())
        .enumerate()
        .map(|(ix, (name, data_type, nullable, key))| TableColumnMeta {
            column_name: name,
            data_type,
            is_nullable: nullable.eq_ignore_ascii_case("YES"),
            // 临时表多半由查询结果建出，没有主键约束；有则如实标出
            is_primary_key: key.eq_ignore_ascii_case("PRI"),
            ordinal_position: ix as i32 + 1,
        })
        .collect();

    // 行数失败不算致命：列清单仍可看，只是没行数（与源库内省同一取舍）
    let row_count: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM {}", quote_ident(temp_table)),
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(TableProfile {
        table_name: temp_table.to_string(),
        db_type: "DuckDB".to_string(),
        columns,
        row_count: Some(row_count.clamp(0, i32::MAX as i64) as i32),
        schema_name: None,
    })
}

/// EDA 列统计摘要：对临时表**所有列一次查询**拿空值率与唯一值数。
/// 拼一条 `SELECT`（每列两个表达式：`COUNT(col)` 与 `COUNT(DISTINCT col)`），
/// 只跑一次 DuckDB 查询——业务表 200+ 列也不该逐列往返。
/// 空值率 / 唯一率从**精确计数**派生（不落 `AVG` 浮点中转）；计数本身保留在
/// [`ColumnStatBrief`] 上，供逐列评估直接复用（免每列再跑一次 `COUNT(DISTINCT)`
/// 全表聚合）。行数为 0 或统计本身不可用（如空表）率返回 `None`，调用方不编假值。
pub fn compute_table_column_summary_on(
    conn: &duckdb::Connection,
    temp_table: &str,
    columns: &[String],
) -> Result<Vec<ColumnStatBrief>, CoreError> {
    let mut select_parts = Vec::with_capacity(columns.len() * 2 + 1);
    select_parts.push("COUNT(*) AS \"__rows\"".to_string());
    for (ix, column) in columns.iter().enumerate() {
        // 列名经 quote_ident 转义，防含引号列名（CSV 表头带 `"` 不算罕见）
        let quoted = quote_ident(column);
        select_parts.push(format!("COUNT({quoted}) AS \"__c{ix}\""));
        select_parts.push(format!("COUNT(DISTINCT {quoted}) AS \"__u{ix}\""));
    }
    let sql = format!(
        "SELECT {} FROM {}",
        select_parts.join(", "),
        quote_ident(temp_table)
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB column summary prepare failed: {}",
            e
        )))
    })?;

    let mut rows = stmt.query([]).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB column summary query failed: {}",
            e
        )))
    })?;
    let mut result = Vec::with_capacity(columns.len());
    if let Some(row) = rows.next().map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB column summary row failed: {}",
            e
        )))
    })? {
        let total: i64 = row.get::<_, Option<i64>>(0)?.unwrap_or(0);
        for (ix, column) in columns.iter().enumerate() {
            let non_null: i64 = row.get::<_, Option<i64>>(ix * 2 + 1)?.unwrap_or(0);
            let unique: i64 = row.get::<_, Option<i64>>(ix * 2 + 2)?.unwrap_or(0);
            // 空值率 = 空行数 / 总行数（精确整数相除，与旧 AVG 口径等值）
            let null_rate = (total > 0).then(|| {
                let nulls = (total - non_null).max(0);
                nulls as f64 / total as f64
            });
            // 唯一率 = 唯一值数 / **非空行数**（`COUNT(DISTINCT)` 不算 NULL，分母
            // 也不该算；除以总行数会把含空值列的唯一率低估）。全空列兜底 0。
            let unique_rate = (total > 0).then(|| (unique as f64) / (non_null as f64).max(1.0));
            result.push(ColumnStatBrief {
                name: column.clone(),
                total,
                non_null,
                unique,
                null_rate,
                unique_rate,
            });
        }
    }
    Ok(result)
}

/// 整行重复产生的冗余行数：`SUM(组内行数 - 1)`（`GROUP BY ALL` 按全部列分组）。
///
/// 概览五卡「重复行」口径（2026-10-30 对齐 EDA 原型）：可去重行数 = 总行数 -
/// 去重后行数，即「删掉这些行后不留重复整行」。空表 / 全唯一返回 0。
/// 临时表行数受取样上限约束（`SOURCE_SAMPLE_LIMIT`），全表 `GROUP BY ALL` 代价可控；
/// 失败按调用方口径降级（画像不因它失败，五卡显示「—」）。
///
/// SQL 陷阱（2026-10-31 真实连接实测锁定）：子查询必须 `SELECT *, COUNT(*)`——
/// `GROUP BY ALL` 按 **select 列表里未聚合的列** 分组；只选 `COUNT(*)` 时没有
/// 分组键，会把全表并成一组（冗余=总行数-1，数值虚高）。`*` 展开成全部列即
/// 「整行分组」，且与列名无关（临时表列结构不定，不拼列名）。
pub fn compute_dup_rows_on(conn: &duckdb::Connection, temp_table: &str) -> Result<u64, CoreError> {
    let sql = format!(
        "SELECT COALESCE(SUM(n - 1), 0) FROM (SELECT *, COUNT(*) AS n FROM {} GROUP BY ALL)",
        quote_ident(temp_table)
    );
    let dup: i64 = conn.query_row(&sql, [], |row| row.get(0)).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "DuckDB dup rows query failed: {}",
            e
        )))
    })?;
    Ok(dup.max(0) as u64)
}

/// 带并发控制与全局连接的重复行统计入口（调用方拿不到连接时用）。
pub fn compute_dup_rows(temp_table: &str) -> Result<u64, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    compute_dup_rows_on(&conn, temp_table)
}

/// 带并发控制与全局连接的 EDA 列统计摘要入口（调用方拿不到连接时用）。
pub fn compute_table_column_summary(
    temp_table: &str,
    columns: &[String],
) -> Result<Vec<ColumnStatBrief>, CoreError> {
    let _permit = insight_semaphore().try_acquire().map_err(|_| {
        CoreError::common(CommonError::General(ERR_TOO_MANY_CONCURRENT.to_string()))
    })?;

    let duckdb = get_or_create_duckdb()?;
    let conn = duckdb.lock().map_err(|e| {
        CoreError::common(CommonError::General(format!("DuckDB lock error: {}", e)))
    })?;
    compute_table_column_summary_on(&conn, temp_table, columns)
}
