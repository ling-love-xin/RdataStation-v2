use std::collections::BTreeMap;

use oracle_rs::{Connection, Value};
use rds_driver_sidecar::{
    DriverError, MetaColumn, MetaIndex, MetaObject, MetaObjectDetail, MetaSchema, normalize_kind,
};

use crate::types::sql_error;

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
pub async fn catalogs(conn: &Connection) -> Result<Vec<String>, DriverError> {    let sql =
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
    // 用绑定变量，不把 schema 名拼进 SQL 文本：Oracle 按 SQL 文本硬解析，
    // 每换一个名字就是一条新语句（真机实测字典视图一次硬解析 3–4 秒，连续几十条
    // 之后服务端还会直接把会话踢掉）。文本固定 + 绑定值才能命中 oracle-rs 的
    // statement cache。
    let sql = "SELECT object_name, object_type FROM all_objects \
               WHERE owner = :1 AND object_type IN \
               ('TABLE','VIEW','MATERIALIZED VIEW','PROCEDURE','FUNCTION','SEQUENCE','TRIGGER') \
               ORDER BY object_name";
    let result = conn
        .query(sql, &[Value::String(schema.to_uppercase())])
        .await
        .map_err(sql_error)?;
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
    let owner_name = schema.to_uppercase();
    let object_name = name.to_uppercase();

    // 不查 `data_default`：它是 LONG，oracle-rs 解析该列会让整条查询返回 0 行。
    // 同样用绑定变量（理由见 `objects`）：对象名一变就是新 SQL，硬解析代价 3–4 秒。
    let sql = "SELECT column_name, data_type, nullable, data_precision, data_scale, \
               data_length, char_length \
               FROM all_tab_columns WHERE owner = :1 AND table_name = :2 \
               ORDER BY column_id";
    let result = conn
        .query(
            sql,
            &[
                Value::String(owner_name.clone()),
                Value::String(object_name.clone()),
            ],
        )
        .await
        .map_err(sql_error)?;
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

    mark_primary_key(conn, &owner_name, &object_name, &mut columns).await;
    // 协议只给索引个数（明细走 Database::list_indexes），这里只数不塞明细。
    // 取不到就把 `None` 交给宿主（界面显示「未知」），但把原因写进 stderr：
    // 真机上线前这条一直静默，结果「连接被服务端关掉」被当成「这表没索引」。
    let index_count = match indexes(conn, &owner_name, &object_name).await {
        Ok(list) => Some(list.len() as u32),
        Err(error) => {
            eprintln!("[rds-oracle-sidecar] 取索引个数失败（{object_name}）：{error}");
            None
        }
    };
    let kind = object_kind(conn, &owner_name, &object_name).await;
    // 只有表才给行数：视图没有统计信息，精确数一遍的代价见 `table_row_count` 的注释。
    let row_count = if kind == "table" {
        table_row_count(conn, &owner_name, &object_name).await
    } else {
        None
    };

    let mut detail = MetaObjectDetail::new(name, &kind, columns);
    detail.indexes = index_count;
    detail.row_count = row_count;
    Ok(detail)
}

/// 行数：只取 `all_tables.num_rows`（统计信息），**不做精确 `COUNT(*)`**。
///
/// 真机教训（2026-11-12）：原实现在统计信息为空时退回精确 `COUNT(*)`，结果在
/// `SYS.ALL_ANALYTIC_VIEW_HIERS` 上跑了 60 秒还没回来（字典视图的 COUNT 要全量
/// 物化内部 join），属性面板打开一个对象就挂死。协议口径本来就是**估计值**，
/// 没有统计信息时给 `None`（界面显示「未知」）远好过在交互路径上跑无上限全表扫描。
/// 只有表才走这里；视图在 `all_tables` 里没有条目，本来也给不出行数。
async fn table_row_count(conn: &Connection, owner: &str, table: &str) -> Option<u64> {
    let sql = "SELECT num_rows FROM all_tables WHERE owner = :1 AND table_name = :2";
    let result = match conn
        .query(
            sql,
            &[
                Value::String(owner.to_string()),
                Value::String(table.to_string()),
            ],
        )
        .await
    {
        Ok(result) => result,
        Err(error) => {
            eprintln!("[rds-oracle-sidecar] 取行数失败（{table}）：{error}");
            return None;
        }
    };
    result
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
    let sql = "SELECT object_type FROM all_objects WHERE owner = :1 AND object_name = :2";
    match conn
        .query(
            sql,
            &[
                Value::String(owner.to_string()),
                Value::String(object.to_string()),
            ],
        )
        .await
    {
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
    let sql = "SELECT cc.column_name FROM all_constraints c, all_cons_columns cc \
               WHERE c.owner = :1 AND c.table_name = :2 AND c.constraint_type = 'P' \
               AND cc.owner = c.owner AND cc.constraint_name = c.constraint_name";
    let result = match conn
        .query(
            sql,
            &[
                Value::String(owner.to_string()),
                Value::String(table.to_string()),
            ],
        )
        .await
    {
        Ok(result) => result,
        Err(error) => {
            eprintln!("[rds-oracle-sidecar] 取主键失败（{table}）：{error}");
            return;
        }
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

/// 例程源码（`meta.routine_source`）：Oracle 的源码正文在 `ALL_SOURCE`，一行一条源文件行。
///
/// 绑定变量同理——`ALL_SOURCE` 也是字典视图，把 owner/名字拼进 SQL 文本会重复硬解析。
/// 只认 `PROCEDURE` / `FUNCTION`：协议点名的就是这两类；`PACKAGE` 与 `PACKAGE BODY`
/// 不在这里混进来，否则同名包体会把过程源码拼到一块。
pub async fn routine_source(
    conn: &Connection,
    schema: &str,
    name: &str,
) -> Result<Option<String>, DriverError> {
    let sql = "SELECT text FROM all_source \
               WHERE owner = :1 AND name = :2 AND type IN ('PROCEDURE','FUNCTION') \
               ORDER BY type, line";
    let result = conn
        .query(
            sql,
            &[
                Value::String(schema.to_uppercase()),
                Value::String(name.to_uppercase()),
            ],
        )
        .await
        .map_err(sql_error)?;
    if result.rows.is_empty() {
        // 查不到 ≠ 出错：私有无权限的对象、非源码对象都落在这里，界面留空即可。
        return Ok(None);
    }
    let mut source = String::new();
    for row in &result.rows {
        let Some(line) = row.get_string(0) else {
            continue;
        };
        source.push_str(line);
        // `ALL_SOURCE.TEXT` 自带换行；万一某些版本没有，补一个，避免整段黏成一行。
        if !line.ends_with('\n') {
            source.push('\n');
        }
    }
    Ok(Some(source))
}

async fn indexes(
    conn: &Connection,
    owner: &str,
    table: &str,
) -> Result<Vec<MetaIndex>, DriverError> {
    let sql = "SELECT i.index_name, i.uniqueness, c.column_name \
               FROM all_indexes i, all_ind_columns c \
               WHERE i.table_owner = :1 AND i.table_name = :2 \
               AND c.index_owner = i.owner AND c.index_name = i.index_name \
               ORDER BY i.index_name, c.column_position";
    let result = conn
        .query(
            sql,
            &[
                Value::String(owner.to_string()),
                Value::String(table.to_string()),
            ],
        )
        .await
        .map_err(sql_error)?;
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
