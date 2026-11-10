use std::collections::BTreeMap;

use oracle_rs::{Connection, Value};
use rds_driver_sidecar::{
    normalize_kind, DriverError, MetaColumn, MetaIndex, MetaObject, MetaObjectDetail, MetaSchema,
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

pub async fn schemas(conn: &Connection) -> Result<Vec<MetaSchema>, DriverError> {
    let sql = "SELECT username FROM all_users ORDER BY username";
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
        let Some(name) = row.get_string(0) else { continue };
        let Some(kind_raw) = row.get_string(1) else { continue };
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

    let sql = format!(
        "SELECT column_name, data_type, nullable, data_default, data_precision, data_scale \
         FROM all_tab_columns WHERE owner = {owner} AND table_name = {object} \
         ORDER BY column_id"
    );
    let result = conn.query(&sql, &[]).await.map_err(sql_error)?;
    let mut columns = Vec::new();
    for row in &result.rows {
        let Some(col_name) = row.get_string(0) else { continue };
        let data_type = row.get_string(1).unwrap_or("UNKNOWN");
        let nullable = row.get_string(2).map(|v| v.eq_ignore_ascii_case("Y"));
        let default = row.get_string(3).filter(|s| !s.is_empty()).map(str::to_string);
        let precision = row.get(4).and_then(Value::as_i64).unwrap_or(0);
        let scale = row.get(5).and_then(Value::as_i64).unwrap_or(0);
        let type_raw = compose_type(data_type, precision, scale);
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
            default,
            format: None,
            comment: None,
        });
    }

    mark_primary_key(conn, &owner, &object, &mut columns).await;
    let indexes = indexes(conn, &owner, &object).await.unwrap_or_default();
    let kind = object_kind(conn, &owner, &object).await;

    let mut detail = MetaObjectDetail::new(name, &kind, columns);
    detail.indexes = indexes;
    Ok(detail)
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

fn compose_type(data_type: &str, precision: i64, scale: i64) -> String {
    let upper = data_type.to_ascii_uppercase();
    if upper == "NUMBER" && precision > 0 {
        if scale > 0 {
            return format!("NUMBER({precision},{scale})");
        }
        return format!("NUMBER({precision})");
    }
    data_type.to_string()
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
        if pk.iter().any(|name| name.eq_ignore_ascii_case(&column.name)) {
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
        let Some(name) = row.get_string(0) else { continue };
        let unique = row.get_string(1).map(|v| v.eq_ignore_ascii_case("UNIQUE"));
        let column = row.get_string(2).unwrap_or_default().to_string();
        let entry = grouped.entry(name.to_string()).or_insert_with(|| MetaIndex {
            name: name.to_string(),
            unique,
            columns: Vec::new(),
        });
        entry.columns.push(column);
    }
    Ok(grouped.into_values().collect())
}
