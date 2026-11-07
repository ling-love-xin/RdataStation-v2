//! 两张洞察临时表的同名列值重叠报告（T19 关系排查引擎）

use std::collections::HashMap;

use shared::error::{CommonError, CoreError};

use super::*;
use crate::model::{is_datetime_type, is_numeric_type};

/// 临时表列类型清单（只取 memory / temp schema，避免 `ATTACH` 进来的文件库
/// 列清单混进来只会让同名列比对失真（临时表 vs 文件库同名同型表，值域不相关）。
fn temp_table_column_types(
    conn: &duckdb::Connection,
    table: &str,
) -> Result<HashMap<String, String>, CoreError> {
    let sql = format!(
        "SELECT column_name, data_type FROM information_schema.columns \
         WHERE table_catalog IN ('memory', 'temp') AND table_schema = 'main' AND table_name = ? \
         ORDER BY ordinal_position"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| CoreError::common(CommonError::General(format!("DuckDB error: {e}"))))?;
    let mut rows = stmt
        .query(duckdb::params![table])
        .map_err(|e| CoreError::common(CommonError::General(format!("DuckDB error: {e}"))))?;
    let mut map = HashMap::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| CoreError::common(CommonError::General(format!("DuckDB error: {e}"))))?
    {
        let name: String = row.get(0).unwrap_or_default();
        let ty: String = row.get(1).unwrap_or_default();
        map.insert(name, ty);
    }
    Ok(map)
}

/// 类型是否属于同一**比较族**（数值 / 文本 / 日期 / 布尔）。
///
/// 值重叠只对同族列有意义：`id INTEGER` 与 `id VARCHAR` 值域类型不同，
/// `INTERSECT` 虽能跑但比对的是不同语义，报出来只是噪音。
fn same_type_family(a: &str, b: &str) -> bool {
    let family = |t: &str| -> u8 {
        let t = t.to_ascii_lowercase();
        if is_numeric_type(&t) {
            1
        } else if is_datetime_type(&t) {
            2
        } else if t.starts_with("bool") {
            3
        } else if t.starts_with("varchar")
            || t.starts_with("text")
            || t == "string"
            || t.contains("char")
        {
            4
        } else {
            0
        }
    };
    family(a) != 0 && family(a) == family(b)
}

/// 两张洞察临时表的**同名列值重叠**报告（T19 关系排查引擎）。
///
/// 口径：对每个类型兼容的同名列，取两表各自去重值域的交集，
/// 报交集数 + 占双方值域的比例。`INTERSECT` 在 DuckDB 对 NULL 的处理是
/// 「NULL 与 NULL 相同」——但两表同列都是同一来源的样本时，NULL 重叠
/// 不携带「关系」信息，这里不把 NULL 算进值域（`DISTINCT` 已含 NULL，
/// 需要时再按列排除）。
///
/// 阻塞（内部加全局 DuckDB 锁），调用方放后台线程。
pub fn compute_column_overlap(
    conn: &duckdb::Connection,
    table_a: &str,
    table_b: &str,
) -> Result<Vec<crate::model::OverlapRow>, CoreError> {
    let cols_a = temp_table_column_types(conn, table_a)?;
    let cols_b = temp_table_column_types(conn, table_b)?;

    let mut rows = Vec::new();
    for (name, ty_a) in &cols_a {
        let Some(ty_b) = cols_b.get(name) else {
            continue;
        };
        if !same_type_family(ty_a, ty_b) {
            continue;
        }
        // 三个标量子查询一次查：避免三次往返（DuckDB 单连接串行，往返是主要开销）。
        let sql = format!(
            "SELECT \
               (SELECT COUNT(DISTINCT {c}) FROM {a}), \
               (SELECT COUNT(DISTINCT {c}) FROM {b}), \
               (SELECT COUNT(*) FROM (SELECT DISTINCT {c} FROM {a} INTERSECT SELECT DISTINCT {c} FROM {b}))",
            c = quote_ident(name),
            a = quote_ident(table_a),
            b = quote_ident(table_b)
        );
        let (rate_a, rate_b, inter): (i64, i64, i64) = conn
            .query_row(&sql, [], |row| {
                Ok((
                    row.get::<_, i64>(0).unwrap_or(0),
                    row.get::<_, i64>(1).unwrap_or(0),
                    row.get::<_, i64>(2).unwrap_or(0),
                ))
            })
            .map_err(|e| CoreError::common(CommonError::General(format!("DuckDB error: {e}"))))?;
        if inter == 0 {
            continue; // 无重叠的列不进结果（值域不同是常态，不刷屏）
        }
        rows.push(crate::model::OverlapRow {
            column: name.clone(),
            overlap: inter,
            rate_a: inter as f64 / rate_a.max(1) as f64,
            rate_b: inter as f64 / rate_b.max(1) as f64,
        });
    }

    rows.sort_by(|x, y| y.overlap.cmp(&x.overlap));
    Ok(rows)
}
