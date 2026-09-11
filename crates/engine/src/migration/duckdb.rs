//! DuckDB 迁移执行器
//!
//! `MigrationManager` 基于 rusqlite（SQLite），无法打开 DuckDB 文件；
//! DuckDB 侧迁移统一走本模块：传入**已打开的** DuckDB 连接执行，
//! 避免对同一文件二次 `open`（Windows 上会因文件锁冲突报
//! "unable to open database file"；项目库初始化曾因此失败）。
//!
//! 迁移版本记录在 DuckDB 内的 `schema_version` 表，SQL 文件来自
//! `migrations/<migration_type.dir_name>/`（编译期嵌入）。

use duckdb::Connection;

use shared::error::{CoreError, StorageError};

use super::MigrationType;

/// 用已打开的 DuckDB 连接执行指定类型的迁移，返回本次应用的迁移数。
pub fn apply_migrations(
    conn: &Connection,
    migration_type: MigrationType,
) -> Result<usize, CoreError> {
    // 确保迁移版本表存在
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version     INTEGER PRIMARY KEY,
            name        TEXT NOT NULL,
            applied_at  INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| persistence_error("create_schema_version", e))?;

    // 获取当前版本
    let current_version: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .map_err(|e| persistence_error("get_current_version", e))?;

    // 加载待执行迁移
    use include_dir::include_dir;
    const MIGRATIONS_DIR: include_dir::Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

    let Some(dir) = MIGRATIONS_DIR.get_dir(migration_type.dir_name()) else {
        return Ok(0);
    };

    let mut migrations: Vec<(u32, String, String)> = dir
        .files()
        .filter_map(|f| {
            let filename = f.path().file_name()?.to_str()?;
            if !filename.ends_with(".sql") {
                return None;
            }
            let stem = filename.strip_suffix(".sql")?;
            let mut parts = stem.splitn(2, '_');
            let version = parts.next()?.parse::<u32>().ok()?;
            let name = parts.next()?.to_string();
            if version <= current_version {
                return None;
            }
            let sql = f.contents_utf8()?.to_string();
            Some((version, name, sql))
        })
        .collect();
    migrations.sort_by_key(|m| m.0);

    // 按版本顺序执行并记录
    let mut applied = 0usize;
    for (version, name, sql) in migrations {
        conn.execute_batch(&sql)
            .map_err(|e| persistence_error(&format!("migrate_{name}"), e))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        conn.execute(
            "INSERT INTO schema_version (version, name, applied_at) VALUES (?1, ?2, ?3)",
            [
                &version as &dyn duckdb::ToSql,
                &name as &dyn duckdb::ToSql,
                &now as &dyn duckdb::ToSql,
            ],
        )
        .map_err(|e| persistence_error("record_version", e))?;

        tracing::info!("Applied DuckDB migration {} (version {})", name, version);
        applied += 1;
    }

    Ok(applied)
}

/// 统一的持久化错误包装（store = duckdb）。
fn persistence_error(operation: &str, e: duckdb::Error) -> CoreError {
    CoreError::Storage(StorageError::Persistence {
        store: "duckdb".to_string(),
        operation: operation.to_string(),
        reason: e.to_string(),
    })
}
