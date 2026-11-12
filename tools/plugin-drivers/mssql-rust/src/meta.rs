//! SQL Server 系统视图查询：库、schema、对象、列、主键与索引。

use std::collections::BTreeMap;

use rds_driver_sidecar::{
    DriverError, MetaColumn, MetaIndex, MetaObject, MetaObjectDetail, MetaSchema, normalize_kind,
};

use crate::driver::{MssqlClient, canonical_for_type, query_rows};

pub async fn server_info(client: &mut MssqlClient) -> (Option<String>, String) {
    let rows = query_rows(
        client,
        "SELECT CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(128)), SCHEMA_NAME()",
        Vec::new(),
    )
    .await
    .unwrap_or_default();
    match rows.first() {
        Some(row) => (
            row.get::<&str, _>(0).map(str::to_string),
            row.get::<&str, _>(1).unwrap_or("dbo").to_string(),
        ),
        None => (None, "dbo".to_string()),
    }
}

pub async fn catalogs(client: &mut MssqlClient) -> Result<Vec<String>, DriverError> {
    let rows = query_rows(
        client,
        "SELECT name FROM sys.databases ORDER BY name",
        Vec::new(),
    )
    .await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get::<&str, _>(0))
        .map(str::to_string)
        .collect())
}

pub async fn schemas(client: &mut MssqlClient) -> Result<Vec<MetaSchema>, DriverError> {
    let rows = query_rows(
        client,
        "SELECT name FROM sys.schemas WHERE schema_id < 16384 ORDER BY name",
        Vec::new(),
    )
    .await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.get::<&str, _>(0))
        .map(|name| MetaSchema {
            name: name.to_string(),
            comment: None,
        })
        .collect())
}

pub async fn objects(
    client: &mut MssqlClient,
    schema: &str,
) -> Result<Vec<MetaObject>, DriverError> {
    let rows = query_rows(
        client,
        "SELECT o.name, o.type FROM sys.objects o \
         JOIN sys.schemas s ON s.schema_id = o.schema_id \
         WHERE s.name = @P1 AND o.type IN ('U','V','P','FN','IF','TF','SO') \
         ORDER BY o.name",
        vec![schema.to_string()],
    )
    .await?;

    let mut objects = Vec::new();
    for row in &rows {
        let Some(name) = row.get::<&str, _>(0) else {
            continue;
        };
        let raw_kind = row.get::<&str, _>(1).unwrap_or("");
        let Some(kind) = normalize_kind(raw_kind) else {
            eprintln!("[rds-mssql-sidecar] 跳过无法识别的对象类型：{raw_kind} / {name}");
            continue;
        };
        objects.push(MetaObject {
            name: name.to_string(),
            kind: kind.to_string(),
            comment: None,
            parent: Some(schema.to_string()),
        });
    }
    Ok(objects)
}

pub async fn object_detail(
    client: &mut MssqlClient,
    schema: &str,
    name: &str,
) -> Result<MetaObjectDetail, DriverError> {
    let qualified = format!("[{}].[{}]", schema, name);
    let kind = object_kind(client, schema, name).await;
    let mut columns = columns(client, &qualified).await?;
    mark_primary_key(client, &qualified, &mut columns).await?;
    let index_count = indexes(client, &qualified)
        .await
        .map(|list| list.len() as u32)
        .ok();
    let row_count = if kind == "table" {
        table_row_count(client, &qualified).await?
    } else {
        None
    };

    let mut detail = MetaObjectDetail::new(name, kind, columns);
    detail.indexes = index_count;
    detail.row_count = row_count;
    Ok(detail)
}

async fn object_kind(client: &mut MssqlClient, schema: &str, name: &str) -> String {
    let rows = query_rows(
        client,
        "SELECT o.type FROM sys.objects o \
         JOIN sys.schemas s ON s.schema_id = o.schema_id \
         WHERE s.name = @P1 AND o.name = @P2",
        vec![schema.to_string(), name.to_string()],
    )
    .await
    .unwrap_or_default();
    rows.first()
        .and_then(|row| row.get::<&str, _>(0))
        .and_then(normalize_kind)
        .unwrap_or("table")
        .to_string()
}

async fn columns(
    client: &mut MssqlClient,
    qualified: &str,
) -> Result<Vec<MetaColumn>, DriverError> {
    let rows = query_rows(
        client,
        "SELECT c.name, t.name, c.max_length, c.precision, c.scale, c.is_nullable, \
         dc.definition, c.is_identity \
         FROM sys.columns c \
         JOIN sys.types t ON c.user_type_id = t.user_type_id \
         LEFT JOIN sys.default_constraints dc ON c.default_object_id = dc.object_id \
         WHERE c.object_id = OBJECT_ID(@P1) \
         ORDER BY c.column_id",
        vec![qualified.to_string()],
    )
    .await?;

    let mut columns = Vec::new();
    for row in &rows {
        let Some(name) = row.get::<&str, _>(0) else {
            continue;
        };
        let type_name = row.get::<&str, _>(1).unwrap_or("unknown");
        // `sys.columns` 的列类型：`max_length` 是 smallint，`precision` / `scale` 是 tinyint。
        // 按 `i16` 取 tinyint 会让 tiberius 的 `Row::get` 在内部 unwrap 时 panic
        // （`cannot interpret U8(Some(0)) as an i16 value`），进程一死，整条会话连同
        // 同一实例上的其他连接全部断掉 —— 2026-11-12 真机 `sidecar_conformance`
        // 第 6 条（导航面）实测复现。tinyint 必须按 `u8` 取。
        let max_length = row.get::<i16, _>(2).unwrap_or(0);
        let precision = i16::from(row.get::<u8, _>(3).unwrap_or(0));
        let scale = i16::from(row.get::<u8, _>(4).unwrap_or(0));
        let is_identity = row.get::<bool, _>(7).unwrap_or(false);

        let type_raw = compose_type(type_name, max_length, precision, scale);
        let mut default = row.get::<&str, _>(6).map(str::to_string);
        if is_identity {
            default = Some(match default {
                Some(existing) => format!("IDENTITY / {existing}"),
                None => "IDENTITY".to_string(),
            });
        }
        columns.push(MetaColumn {
            name: name.to_string(),
            type_raw,
            canonical: Some(canonical_for_type(type_name).as_str().to_string()),
            nullable: Some(row.get::<bool, _>(5).unwrap_or(true)),
            is_pk: None,
            is_fk: None,
            default,
            format: None,
            comment: None,
        });
    }
    Ok(columns)
}

async fn mark_primary_key(
    client: &mut MssqlClient,
    qualified: &str,
    columns: &mut [MetaColumn],
) -> Result<(), DriverError> {
    let rows = query_rows(
        client,
        "SELECT c.name FROM sys.indexes i \
         JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
         JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
         WHERE i.is_primary_key = 1 AND i.object_id = OBJECT_ID(@P1)",
        vec![qualified.to_string()],
    )
    .await?;
    let pk: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.get::<&str, _>(0))
        .collect();
    for column in columns.iter_mut() {
        column.is_pk = Some(pk.iter().any(|name| *name == column.name));
    }
    Ok(())
}

async fn indexes(client: &mut MssqlClient, qualified: &str) -> Result<Vec<MetaIndex>, DriverError> {
    let rows = query_rows(
        client,
        "SELECT i.name, i.is_unique, c.name, ic.key_ordinal \
         FROM sys.indexes i \
         JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
         JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
         WHERE i.object_id = OBJECT_ID(@P1) AND i.is_hypothetical = 0 AND i.name IS NOT NULL \
         ORDER BY i.name, ic.key_ordinal",
        vec![qualified.to_string()],
    )
    .await?;

    let mut grouped: BTreeMap<String, MetaIndex> = BTreeMap::new();
    for row in &rows {
        let Some(index_name) = row.get::<&str, _>(0) else {
            continue;
        };
        let unique = row.get::<bool, _>(1);
        let column = row.get::<&str, _>(2);
        let entry = grouped
            .entry(index_name.to_string())
            .or_insert_with(|| MetaIndex {
                name: index_name.to_string(),
                unique,
                columns: Vec::new(),
            });
        if let Some(column) = column {
            entry.columns.push(column.to_string());
        }
    }
    Ok(grouped.into_values().collect())
}

async fn table_row_count(
    client: &mut MssqlClient,
    qualified: &str,
) -> Result<Option<u64>, DriverError> {
    let rows = query_rows(
        client,
        "SELECT SUM(p.rows) FROM sys.partitions p \
         WHERE p.object_id = OBJECT_ID(@P1) AND p.index_id IN (0, 1)",
        vec![qualified.to_string()],
    )
    .await
    .unwrap_or_default();
    Ok(rows
        .first()
        .and_then(|row| row.get::<i64, _>(0))
        .and_then(|n| u64::try_from(n).ok()))
}

fn compose_type(type_name: &str, max_length: i16, precision: i16, scale: i16) -> String {
    let upper = type_name.to_ascii_uppercase();
    match type_name.to_ascii_lowercase().as_str() {
        "varchar" | "char" | "varbinary" | "binary" => {
            if max_length < 0 {
                format!("{upper}(MAX)")
            } else {
                format!("{upper}({max_length})")
            }
        }
        "nvarchar" | "nchar" => {
            if max_length < 0 {
                format!("{upper}(MAX)")
            } else {
                format!("{upper}({})", max_length / 2)
            }
        }
        "decimal" | "numeric" => format!("{upper}({precision},{scale})"),
        _ => upper,
    }
}
