//! DuckDB 扩展的**登记**与**留痕**（迁移 031，两张表）
//!
//! ## 为什么与 `plugins` / `driver_operations` 分开
//!
//! 扩展是「进程内 dlopen 的单个文件」：没有插件目录、没有清单、没有贡献点，也不走
//! checksums.json（宿主自己算 sha256）。它与插件的交集只有一条心智——「全局装、
//! 项目引用」（`plugin-architecture.md` §7），物理上必须分开。
//!
//! 留痕也一样：`driver_operations` 的列是 `plugin_id` / `driver_id`，扩展两者都没有；
//! 两边的 `before_json` / `after_json` 语义也不同（那边记驱动状态差异，这边记文件
//! 指纹）。混进一张表会让两边的统计都失真（D8e 正在导出 `driver_operations`）。
//!
//! ## 口径
//!
//! - **终态 vs 历史**：`duckdb_extensions` 记「现在磁盘上是什么」，`extension_operations`
//!   记「什么时候、被谁、改成了什么」——与 `drivers` / `plugin_rows` 那套一致。
//! - 主键是 `(extension_name, kernel_version, platform)`：DuckDB 的扩展文件按内核版本
//!   分目录，升级后是**另一个文件**，不是同一行的覆盖。
//! - 失败也要留一行（`result = fail` + `reason`）：只回 UI 状态行的失败等于没留痕。
//! - `sha256` 是**慢指纹**：只在安装后 / 指纹变化 / 用户点校验时算。日常建连只比对
//!   「长度 + 修改时间」，否则每次连库都要读几十 MB。

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use shared::error::{CoreError, StorageError};

/// 一条扩展登记行（`duckdb_extensions`）
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct DuckdbExtensionRow {
    /// 扩展名（如 `excel` / `oracle_scanner`）
    pub extension_name: String,
    /// DuckDB 内核版本（如 `v1.1.5`）；来源是 `SELECT version()`
    pub kernel_version: String,
    /// DuckDB 的平台串（`win_amd64` / `linux_amd64` / `osx_arm64`）
    pub platform: String,
    /// `official` / `community` / `local_file`
    pub source: String,
    /// `duckdb_extensions()` 给出的权威 `install_path`（不自己拼 `v<x>/<platform>`）
    pub file_path: Option<String>,
    /// 慢指纹：文件内容 sha256（小写十六进制）
    pub sha256: Option<String>,
    pub size_bytes: Option<i64>,
    /// `installed` / `mismatch` / `missing`
    pub state: String,
    /// 最后一次失败的可读原因
    pub last_error: Option<String>,
    pub installed_at: String,
    pub verified_at: Option<String>,
}

impl DuckdbExtensionRow {
    /// 新登记一行（默认 `state = installed`）
    pub fn installed(extension: &str, kernel_version: &str, platform: &str, source: &str) -> Self {
        Self {
            extension_name: extension.to_string(),
            kernel_version: kernel_version.to_string(),
            platform: platform.to_string(),
            source: source.to_string(),
            state: "installed".to_string(),
            ..Default::default()
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    pub fn with_sha256(mut self, sha256: impl Into<String>, size_bytes: i64) -> Self {
        self.sha256 = Some(sha256.into());
        self.size_bytes = Some(size_bytes);
        self
    }
}

/// 一条扩展操作留痕（`extension_operations`）
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct ExtensionOperation {
    /// `global` / `project`
    pub scope: String,
    /// `ui` / `runtime` / `migration` / `cli`
    pub actor: String,
    /// `install` / `load` / `verify` / `import` / `remove` / `missing`
    pub action: String,
    pub extension_name: Option<String>,
    pub kernel_version: Option<String>,
    pub platform: Option<String>,
    pub project_id: Option<String>,
    /// `ok` / `fail`
    pub result: String,
    pub reason: Option<String>,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
}

impl ExtensionOperation {
    pub fn ok(actor: &str, action: &str) -> Self {
        Self {
            scope: "global".to_string(),
            actor: actor.to_string(),
            action: action.to_string(),
            result: "ok".to_string(),
            ..Default::default()
        }
    }

    /// 失败必须带原因：没有原因的失败留痕没有意义
    pub fn fail(actor: &str, action: &str, reason: impl Into<String>) -> Self {
        Self {
            reason: Some(reason.into()),
            result: "fail".to_string(),
            ..Self::ok(actor, action)
        }
    }

    pub fn extension(mut self, name: impl Into<String>) -> Self {
        self.extension_name = Some(name.into());
        self
    }

    pub fn build(
        mut self,
        kernel_version: impl Into<String>,
        platform: impl Into<String>,
    ) -> Self {
        self.kernel_version = Some(kernel_version.into());
        self.platform = Some(platform.into());
        self
    }
}

/// 读出用的一行（带 `id` 与时间）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ExtensionOperationRow {
    pub id: i64,
    pub occurred_at: String,
    pub operation: ExtensionOperation,
}

/// 读过滤条件（给了才过滤）
#[derive(Debug, Clone, Default)]
pub struct ExtensionOperationFilter {
    pub extension_name: Option<String>,
    pub action: Option<String>,
    /// `ok` / `fail`
    pub result: Option<String>,
    pub limit: Option<usize>,
}

fn storage_err(operation: &str, reason: String) -> CoreError {
    CoreError::Storage(StorageError::Persistence {
        store: "extension_store".to_string(),
        operation: operation.to_string(),
        reason,
    })
}

/// 登记 / 更新一行（同一 `(name, kernel, platform)` 覆盖，保留原 `installed_at`）
pub fn upsert_extension(conn: &Connection, row: &DuckdbExtensionRow) -> Result<(), CoreError> {
    if row.extension_name.trim().is_empty()
        || row.kernel_version.trim().is_empty()
        || row.platform.trim().is_empty()
    {
        return Err(storage_err(
            "upsert_extension",
            "extension_name / kernel_version / platform 不能为空".to_string(),
        ));
    }
    conn.execute(
        "INSERT INTO duckdb_extensions
             (extension_name, kernel_version, platform, source, file_path, sha256,
              size_bytes, state, last_error, installed_at, verified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
         ON CONFLICT (extension_name, kernel_version, platform) DO UPDATE SET
             source      = excluded.source,
             file_path   = excluded.file_path,
             sha256      = excluded.sha256,
             size_bytes  = excluded.size_bytes,
             state       = excluded.state,
             last_error  = excluded.last_error,
             verified_at = excluded.verified_at",
        params![
            row.extension_name,
            row.kernel_version,
            row.platform,
            row.source,
            row.file_path,
            row.sha256,
            row.size_bytes,
            row.state,
            row.last_error,
        ],
    )
    .map_err(|e| storage_err("upsert_extension", e.to_string()))?;
    Ok(())
}

/// 列全部登记行（按名字升序，方便面板稳定排序）
pub fn list_extensions(conn: &Connection) -> Result<Vec<DuckdbExtensionRow>, CoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT extension_name, kernel_version, platform, source, file_path, sha256,
                    size_bytes, state, last_error, installed_at, verified_at
               FROM duckdb_extensions
              ORDER BY extension_name ASC, kernel_version DESC",
        )
        .map_err(|e| storage_err("list_extensions", e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DuckdbExtensionRow {
                extension_name: row.get(0)?,
                kernel_version: row.get(1)?,
                platform: row.get(2)?,
                source: row.get(3)?,
                file_path: row.get(4)?,
                sha256: row.get(5)?,
                size_bytes: row.get(6)?,
                state: row.get(7)?,
                last_error: row.get(8)?,
                installed_at: row.get(9)?,
                verified_at: row.get(10)?,
            })
        })
        .map_err(|e| storage_err("list_extensions", e.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("list_extensions", e.to_string()))
}

/// 取某个扩展**最近一次登记**的那行（跨内核版本时取 `installed_at` 最新的一行）
///
/// 刻意不按内核版本过滤：调用方要能看见「登记的是 v1.1.5、现在跑的是 v1.1.6」，
/// 那正是「升级后需重装」的显示依据。
pub fn find_extension(
    conn: &Connection,
    extension_name: &str,
) -> Result<Option<DuckdbExtensionRow>, CoreError> {
    conn.query_row(
        "SELECT extension_name, kernel_version, platform, source, file_path, sha256,
                size_bytes, state, last_error, installed_at, verified_at
           FROM duckdb_extensions
          WHERE extension_name = ?1
          ORDER BY installed_at DESC
          LIMIT 1",
        params![extension_name],
        |row| {
            Ok(DuckdbExtensionRow {
                extension_name: row.get(0)?,
                kernel_version: row.get(1)?,
                platform: row.get(2)?,
                source: row.get(3)?,
                file_path: row.get(4)?,
                sha256: row.get(5)?,
                size_bytes: row.get(6)?,
                state: row.get(7)?,
                last_error: row.get(8)?,
                installed_at: row.get(9)?,
                verified_at: row.get(10)?,
            })
        },
    )
    .optional()
    .map_err(|e| storage_err("find_extension", e.to_string()))
}

/// 只更新状态与最近一次错误（校验失败 / 文件不见了都走这里）
pub fn mark_state(
    conn: &Connection,
    extension_name: &str,
    state: &str,
    last_error: Option<&str>,
) -> Result<(), CoreError> {
    conn.execute(
        "UPDATE duckdb_extensions
            SET state = ?2, last_error = ?3
          WHERE extension_name = ?1",
        params![extension_name, state, last_error],
    )
    .map_err(|e| storage_err("mark_state", e.to_string()))?;
    Ok(())
}

/// 写一行操作留痕
pub fn record_operation(conn: &Connection, op: &ExtensionOperation) -> Result<i64, CoreError> {
    if op.scope.trim().is_empty() || op.actor.trim().is_empty() || op.action.trim().is_empty() {
        return Err(storage_err(
            "record_operation",
            "scope / actor / action 不能为空".to_string(),
        ));
    }
    if op.result != "ok" && op.result != "fail" {
        return Err(storage_err(
            "record_operation",
            format!("result 只能是 ok / fail，收到 '{}'", op.result),
        ));
    }
    conn.execute(
        "INSERT INTO extension_operations
             (scope, actor, action, extension_name, kernel_version, platform, project_id,
              result, reason, before_json, after_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            op.scope,
            op.actor,
            op.action,
            op.extension_name,
            op.kernel_version,
            op.platform,
            op.project_id,
            op.result,
            op.reason,
            op.before_json,
            op.after_json,
        ],
    )
    .map_err(|e| storage_err("record_operation", e.to_string()))?;
    Ok(conn.last_insert_rowid())
}

/// 列操作记录（时间倒序；`filter` 给了的字段才过滤）
pub fn list_operations(
    conn: &Connection,
    filter: &ExtensionOperationFilter,
) -> Result<Vec<ExtensionOperationRow>, CoreError> {
    let mut sql = String::from(
        "SELECT id, occurred_at, scope, actor, action, extension_name, kernel_version,
                platform, project_id, result, reason, before_json, after_json
           FROM extension_operations",
    );
    let mut clauses: Vec<&str> = Vec::new();
    if filter.extension_name.is_some() {
        clauses.push("extension_name = ?");
    }
    if filter.action.is_some() {
        clauses.push("action = ?");
    }
    if filter.result.is_some() {
        clauses.push("result = ?");
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY occurred_at DESC");

    let limit = filter.limit.unwrap_or(200).clamp(1, 2000);
    let mut stmt = conn
        .prepare(&format!("{sql} LIMIT {limit}"))
        .map_err(|e| storage_err("list_operations", e.to_string()))?;

    // 参数绑定：Vec<&dyn ToSql> 里存的是**指向 filter 内部**的引用。
    // 不能 `values.push(&name)`——那取的是 `if let` 临时绑定的地址，循环外就悬空了。
    let mut values: Vec<&dyn rusqlite::ToSql> = Vec::new();
    if let Some(name) = filter.extension_name.as_ref() {
        values.push(name as &dyn rusqlite::ToSql);
    }
    if let Some(action) = filter.action.as_ref() {
        values.push(action as &dyn rusqlite::ToSql);
    }
    if let Some(result) = filter.result.as_ref() {
        values.push(result as &dyn rusqlite::ToSql);
    }

    let rows = stmt
        .query_map(values.as_slice(), |row| {
            Ok(ExtensionOperationRow {
                id: row.get(0)?,
                occurred_at: row.get(1)?,
                operation: ExtensionOperation {
                    scope: row.get(2)?,
                    actor: row.get(3)?,
                    action: row.get(4)?,
                    extension_name: row.get(5)?,
                    kernel_version: row.get(6)?,
                    platform: row.get(7)?,
                    project_id: row.get(8)?,
                    result: row.get(9)?,
                    reason: row.get(10)?,
                    before_json: row.get(11)?,
                    after_json: row.get(12)?,
                },
            })
        })
        .map_err(|e| storage_err("list_operations", e.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("list_operations", e.to_string()))
}

/// 表在不在（迁移 031 没跑时给可操作提示，而不是一句 SQL 报错）
pub fn table_exists(conn: &Connection) -> Result<bool, CoreError> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'duckdb_extensions'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map(|v| v.is_some())
    .map_err(|e| storage_err("table_exists", e.to_string()))
}
