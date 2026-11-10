use std::collections::BTreeMap;

use oracle_rs::{Connection, Value};
use rds_driver_sidecar::{
    DriverError, MetaColumn, MetaIndex, MetaObject, MetaObjectDetail, MetaSchema, normalize_kind,
};

use crate::types::{quote_literal, sql_error};

pub async fn current_schema(conn: &Connection, fallback: &str) -> String {
    let sql = "SELECT SYS_CONTEXT('USERENV','CURRENT_SCHEMA') FROM DUAL";
    match conn.query(sql, &[]).await {
        Ok(result) => result
            .rows
            .first()
            .and_then(|row| row.get_string(0))
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback)
            .to_string(),
        Err(_) => fallback.to_string(),
    }
}

/// 导航第一层。Oracle 没有 MySQL 那种「多库」概念，但宿主树要求
/// `catalog → schema → object` 三层都给得出：这里回当前数据库名（`DB_NAME`，
/// 单实例只有一项），schema 层再走 `all_users`。拿不到 `DB_NAME` 时退回服务名，
/// 两者都拿不到才回空（空会把导航第一层整层空掉，属于最后手段）。
pub async fn catalogs(conn: &Connection) -> Result<Vec<String>, DriverError> {
    let sql =
        "SELECT SYS_CONTEXT('USERENV','DB_NAME'), SYS_CONTEXT('USERENV','SERVICE_NAME') FROM DUAL";
    let result = conn.query(sql, &[]).await.map_err(sql_error)?;
    let row = result.rows.first();
    let db_name = row
        .and_then(|r| r.get_string(0))
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let service = row
        .and_then(|r| r.get_string(1))
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(db_name.or(service).into_iter().collect())
}

pub async fn schemas(conn: &Connection) -> Result<Vec<MetaSchema>, DriverError> {
    // 当前 schema 排最前：导航与 conformance 都取第一项，用户也更想先看到自己的库。
    let sql = "SELECT username FROM all_users \
               ORDER BY CASE WHEN username = SYS_CONTEXT('USERENV','CURRENT_SCHEMA') THEN 0 ELSE 1 END, username";
    let result = conn.query(sql, &[]).await.map_err(sql_error)?;
    let mut out = Vec::new();
    for row in &result.rows {
        if let Some(name) = row.get_string(0) {
            if !name.is_empty() {
                out.push(MetaSchema {
                    name: name.to_string(),
                    comment: None,
                });
            }
        }
    }
    Ok(out)
}

pub async fn objects(conn: &Connection, schema: &str) -> Result<Vec<MetaObject>, DriverError> {
    let owner = quote_literal(&schema.to_uppercase());
    let sql = format!(
        "SELECT object_name, object_type FROM all_objects \
         WHERE owner = {owner} AND object_type IN \
         ('TABLE','VIEW','MATERIALIZED VIEW','PROCEDURE','FUNCTION','SEQUENCE','TRIGGER') \
         ORDER BY object_name"
    );
    let result = conn.query(&sql, &[]).await.map_err(sql_error)?;
    let mut out = Vec::new();
    for row in &result.rows {
        let Some(name) = row.get_string(0) else {
            continue;
        };
        let Some(kind_raw) = row.get_string(1) else {
            continue;
        };
        let Some(kind) = normalize_kind(kind_raw) else {
            eprintln!("[rds-oracle-sidecar] 跳过未知对象类型：{kind_raw}");
            continue;
        };
        out.push(MetaObject {
            name: name.to_string(),
            kind: kind.to_string(),
            comment: None,
            parent: Some(schema.to_string()),
        });
    }
    Ok(out)
}

pub async fn object_detail(
    conn: &Connection,
    schema: &str,
    name: &str,
) -> Result<MetaObjectDetail, DriverError> {
    let owner = quote_literal(&schema.to_uppercase());
    let object = quote_literal(&name.to_uppercase());

    // 不查 `data_default`：它是 LONG，oracle-rs 解析该列会让整条查询返回 0 行。
    let sql = format!(
        "SELECT column_name, data_type, nullable, data_precision, data_scale, \
         data_length, char_length \
         FROM all_tab_columns WHERE owner = {owner} AND table_name = {object} \
         ORDER BY column_id"
    );
    let result = conn.query(&sql, &[]).await.map_err(sql_error)?;
    let mut columns = Vec::new();
    for row in &result.rows {
        let Some(col_name) = row.get_string(0) else {
            continue;
        };
        let data_type = row.get_string(1).unwrap_or("UNKNOWN");
        let nullable = row.get_string(2).map(|v| v.eq_ignore_ascii_case("Y"));
        let precision = row.get(3).and_then(int_value).unwrap_or(0);
        let scale = row.get(4).and_then(int_value).unwrap_or(0);
        let data_length = row.get(5).and_then(int_value).unwrap_or(0);
        let char_length = row.get(6).and_then(int_value).unwrap_or(0);
        let type_raw = compose_type(data_type, precision, scale, data_length, char_length);
        let canonical = if data_type.eq_ignore_ascii_case("NUMBER") {
            if scale > 0 { "FLOAT" } else { "INTEGER" }
        } else {
            canonical_name(data_type)
        };
        columns.push(MetaColumn {
            name: col_name.to_string(),
            type_raw,
            canonical: Some(canonical.to_string()),
            nullable,
            is_pk: Some(false),
            is_fk: None,
            // 默认值在 Oracle 数据字典里是 LONG，oracle-rs 取不了；属性面板允许缺省。
            default: None,
            format: None,
            comment: None,
        });
    }

    mark_primary_key(conn, &owner, &object, &mut columns).await;
    // 协议只给索引个数（明细走 Database::list_indexes），这里只数不塞明细。
    let index_count = indexes(conn, &owner, &object)
        .await
        .map(|list| list.len() as u32)
        .ok();
    let kind = object_kind(conn, &owner, &object).await;
    let row_count = table_row_count(conn, &schema.to_uppercase(), &name.to_uppercase()).await;

    let mut detail = MetaObjectDetail::new(name, &kind, columns);
    detail.indexes = index_count;
    detail.row_count = row_count;
    Ok(detail)
}

/// 行数：先取 `all_tables.num_rows`（统计信息，可能滞后）；新表没有统计信息时
/// 退回精确 `COUNT(*)`。协议口径是「估计值」，但属性面板显示 `None` 不如给个数。
async fn table_row_count(conn: &Connection, owner: &str, table: &str) -> Option<u64> {
    let owner_lit = quote_literal(owner);
    let table_lit = quote_literal(table);
    let sql = format!(
        "SELECT num_rows FROM all_tables WHERE owner = {owner_lit} AND table_name = {table_lit}"
    );
    if let Ok(ok) = conn.query(&sql, &[]).await
        && let Some(count) = ok
            .rows
            .first()
            .and_then(|row| row.get(0))
            .and_then(int_value)
            .and_then(|n| u64::try_from(n).ok())
    {
        return Some(count);
    }
    let ident = |raw: &str| raw.replace('"', "\"\"");
    let count_sql = format!(
        "SELECT COUNT(*) FROM \"{}\".\"{}\"",
        ident(owner),
        ident(table)
    );
    conn.query(&count_sql, &[])
        .await
        .ok()?
        .rows
        .first()
        .and_then(|row| row.get(0))
        .and_then(int_value)
        .and_then(|n| u64::try_from(n).ok())
}

/// 数据字典里的 NUMBER 在 oracle-rs 里可能落到 String 变体，单靠 `as_i64` 会丢精度。
fn int_value(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|raw| raw.trim().parse().ok()))
}

async fn object_kind(conn: &Connection, owner: &str, object: &str) -> String {
    let sql = format!(
        "SELECT object_type FROM all_objects WHERE owner = {owner} AND object_name = {object}"
    );
    match conn.query(&sql, &[]).await {
        Ok(result) => result
            .rows
            .first()
            .and_then(|row| row.get_string(0))
            .and_then(normalize_kind)
            .unwrap_or("table")
            .to_string(),
        Err(_) => "table".to_string(),
    }
}

fn compose_type(
    data_type: &str,
    precision: i64,
    scale: i64,
    data_length: i64,
    char_length: i64,
) -> String {
    let upper = data_type.to_ascii_uppercase();
    match upper.as_str() {
        "NUMBER" if precision > 0 => {
            if scale > 0 {
                format!("NUMBER({precision},{scale})")
            } else {
                format!("NUMBER({precision})")
            }
        }
        "VARCHAR2" | "NVARCHAR2" | "CHAR" | "NCHAR" if char_length > 0 => {
            format!("{upper}({char_length})")
        }
        "RAW" if data_length > 0 => format!("RAW({data_length})"),
        d if d.starts_with("TIMESTAMP") && scale > 0 => format!("{upper}({scale})"),
        _ => data_type.to_string(),
    }
}

fn canonical_name(data_type: &str) -> &'static str {
    match data_type.to_ascii_uppercase().as_str() {
        "NUMBER" | "BINARY_INTEGER" | "INTEGER" | "PLS_INTEGER" => "INTEGER",
        "BINARY_FLOAT" | "BINARY_DOUBLE" | "FLOAT" => "FLOAT",
        "DATE" => "DATE",
        d if d.starts_with("TIMESTAMP") => "TIMESTAMP",
        "RAW" | "LONG RAW" | "BLOB" | "BFILE" => "BINARY",
        "BOOLEAN" => "BOOLEAN",
        "JSON" => "JSON",
        _ => "TEXT",
    }
}

async fn mark_primary_key(conn: &Connection, owner: &str, table: &str, columns: &mut [MetaColumn]) {
    let sql = format!(
        "SELECT cc.column_name FROM all_constraints c, all_cons_columns cc \
         WHERE c.owner = {owner} AND c.table_name = {table} AND c.constraint_type = 'P' \
         AND cc.owner = c.owner AND cc.constraint_name = c.constraint_name"
    );
    let Ok(result) = conn.query(&sql, &[]).await else {
        return;
    };
    let pk: Vec<String> = result
        .rows
        .iter()
        .filter_map(|row| row.get_string(0).map(str::to_string))
        .collect();
    if pk.is_empty() {
        return;
    }
    for column in columns.iter_mut() {
        if pk
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&column.name))
        {
            column.is_pk = Some(true);
        }
    }
}

async fn indexes(
    conn: &Connection,
    owner: &str,
    table: &str,
) -> Result<Vec<MetaIndex>, DriverError> {
    let sql = format!(
        "SELECT i.index_name, i.uniqueness, c.column_name \
         FROM all_indexes i, all_ind_columns c \
         WHERE i.table_owner = {owner} AND i.table_name = {table} \
         AND c.index_owner = i.owner AND c.index_name = i.index_name \
         ORDER BY i.index_name, c.column_position"
    );
    let result = conn.query(&sql, &[]).await.map_err(sql_error)?;
    let mut grouped: BTreeMap<String, MetaIndex> = BTreeMap::new();
    for row in &result.rows {
        let Some(name) = row.get_string(0) else {
            continue;
        };
        let unique = row.get_string(1).map(|v| v.eq_ignore_ascii_case("UNIQUE"));
        let column = row.get_string(2).unwrap_or_default().to_string();
        let entry = grouped
            .entry(name.to_string())
            .or_insert_with(|| MetaIndex {
                name: name.to_string(),
                unique,
                columns: Vec::new(),
            });
        entry.columns.push(column);
    }
    Ok(grouped.into_values().collect())
}
