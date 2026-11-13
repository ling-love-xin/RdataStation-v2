//! 驱动 / 插件**操作留痕**（`driver_operations` 表，迁移 030）
//!
//! ## 为什么要有这张表
//!
//! `drivers` / `plugins` / `driver_files` 三张表只记**当前是什么**：改了哪次、被谁改的、
//! 失败过几次，一概查不到。生命周期事件此前只进内存 broadcast + tracing 文本日志，
//! 用户「查看」不到，关掉窗口即忘。
//!
//! 这里只放**状态变更类**操作（装/卸/启/停/重载/声明同步/能力位变化/缺驱动被拒）。
//! 每条 SQL 的执行历史仍走 `history_store`（量级与形态都不同，别把两种记录塞一张表）。
//!
//! ## 口径
//!
//! - `before_json` / `after_json` 只存**差异相关字段**（version / checksum / driver_kind /
//!   capabilities / enabled），**不存凭据、不存 URL 里的密码**；
//! - 失败也要留一行（`result = fail` + `reason`）：只回 UI 状态行的失败等于没留痕；
//! - 读接口给 UI 用（插件管理页「操作记录」、驱动能力 Tab 的「最近变更」）。

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use shared::error::{CoreError, StorageError};

/// 一条操作记录（写入用 / 读出用同一形状；`id` 与时间在写入时补）
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct DriverOperation {
    /// `global` / `project`
    pub scope: String,
    /// `ui` / `runtime` / `migration` / `hotplug` / `cli`
    pub actor: String,
    /// `install` / `upgrade` / `uninstall` / `enable` / `disable` / `reload` /
    /// `declare_sync` / `capability_change` / `missing_driver`
    pub action: String,
    /// 插件贡献点（内置驱动为空）
    pub plugin_id: Option<String>,
    /// 驱动 id（如 `mysql` / `mysql_native`）
    pub driver_id: Option<String>,
    /// 项目级动作才有
    pub project_id: Option<String>,
    /// `ok` / `fail`
    pub result: String,
    /// 失败原因（可读文案）；成功后也可写「为什么变」
    pub reason: Option<String>,
    /// 动作前状态（JSON 文本，只含差异相关字段）
    pub before_json: Option<String>,
    /// 动作后状态（JSON 文本）
    pub after_json: Option<String>,
}

impl DriverOperation {
    /// 建一条成功的记录（省掉到处写 `result: "ok".into()`）
    pub fn ok(actor: &str, action: &str) -> Self {
        Self {
            scope: "global".to_string(),
            actor: actor.to_string(),
            action: action.to_string(),
            result: "ok".to_string(),
            ..Default::default()
        }
    }

    /// 建一条失败的记录：`reason` 必填（没有原因的失败留痕没有意义）
    pub fn fail(actor: &str, action: &str, reason: impl Into<String>) -> Self {
        Self {
            reason: Some(reason.into()),
            result: "fail".to_string(),
            ..Self::ok(actor, action)
        }
    }

    pub fn driver(mut self, driver_id: impl Into<String>) -> Self {
        self.driver_id = Some(driver_id.into());
        self
    }

    pub fn plugin(mut self, plugin_id: impl Into<String>) -> Self {
        self.plugin_id = Some(plugin_id.into());
        self
    }

    pub fn before(mut self, json: impl Into<String>) -> Self {
        self.before_json = Some(json.into());
        self
    }

    pub fn after(mut self, json: impl Into<String>) -> Self {
        self.after_json = Some(json.into());
        self
    }

    pub fn project(mut self, project_id: impl Into<String>) -> Self {
        self.scope = "project".to_string();
        self.project_id = Some(project_id.into());
        self
    }

    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

/// 读出的行（含 `id` 与时间）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DriverOperationRow {
    pub id: i64,
    pub occurred_at: String,
    #[serde(flatten)]
    pub operation: DriverOperation,
    pub created_at: String,
}

/// 读过滤条件（都按「给了就过滤」处理）
#[derive(Debug, Clone, Default)]
pub struct DriverOperationFilter {
    pub driver_id: Option<String>,
    pub plugin_id: Option<String>,
    pub action: Option<String>,
    /// `ok` / `fail`
    pub result: Option<String>,
    pub limit: Option<usize>,
}

fn storage_err(operation: &str, reason: String) -> CoreError {
    CoreError::Storage(StorageError::Persistence {
        store: "driver_operation_store".to_string(),
        operation: operation.to_string(),
        reason,
    })
}

/// 写一行操作记录（返回新行的 `id`）
///
/// 调用方一般在**同一事务**里写（安装/卸载本来就有事务）：留痕与状态变更要么都成、
/// 要么都不成，不能出现「装成了但没记」。
pub fn record_operation(conn: &Connection, op: &DriverOperation) -> Result<i64, CoreError> {
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
        "INSERT INTO driver_operations
             (scope, actor, action, plugin_id, driver_id, project_id, result, reason,
              before_json, after_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            op.scope,
            op.actor,
            op.action,
            op.plugin_id,
            op.driver_id,
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

/// 列操作记录（按时间倒序；`filter` 里给了的字段才过滤）
pub fn list_operations(
    conn: &Connection,
    filter: &DriverOperationFilter,
) -> Result<Vec<DriverOperationRow>, CoreError> {
    // 动态 WHERE 只拼**固定片段**，值一律走参数绑定（见模块头「SQL 安全性保障」）
    let mut sql = String::from(
        "SELECT id, occurred_at, scope, actor, action, plugin_id, driver_id, project_id,
                result, reason, before_json, after_json, created_at
           FROM driver_operations",
    );
    let mut clauses: Vec<&str> = Vec::new();
    if filter.driver_id.is_some() {
        clauses.push("driver_id = ?");
    }
    if filter.plugin_id.is_some() {
        clauses.push("plugin_id = ?");
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
    sql.push_str(" ORDER BY occurred_at DESC, id DESC LIMIT ?");

    let limit = filter.limit.unwrap_or(100) as i64;
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(v) = &filter.driver_id {
        values.push(Box::new(v.clone()));
    }
    if let Some(v) = &filter.plugin_id {
        values.push(Box::new(v.clone()));
    }
    if let Some(v) = &filter.action {
        values.push(Box::new(v.clone()));
    }
    if let Some(v) = &filter.result {
        values.push(Box::new(v.clone()));
    }
    values.push(Box::new(limit));
    let refs: Vec<&dyn rusqlite::ToSql> = values.iter().map(|v| v.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| storage_err("prepare_list_operations", e.to_string()))?;
    let rows = stmt
        .query_map(refs.as_slice(), |row| {
            Ok(DriverOperationRow {
                id: row.get(0)?,
                occurred_at: row.get(1)?,
                operation: DriverOperation {
                    scope: row.get(2)?,
                    actor: row.get(3)?,
                    action: row.get(4)?,
                    plugin_id: row.get(5)?,
                    driver_id: row.get(6)?,
                    project_id: row.get(7)?,
                    result: row.get(8)?,
                    reason: row.get(9)?,
                    before_json: row.get(10)?,
                    after_json: row.get(11)?,
                },
                created_at: row.get(12)?,
            })
        })
        .map_err(|e| storage_err("query_list_operations", e.to_string()))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| storage_err("collect_list_operations", e.to_string()))
}

/// 最近一条某驱动的某类操作（能力 Tab 的「最近一次声明同步」用它）
pub fn latest_operation(
    conn: &Connection,
    driver_id: &str,
    action: &str,
) -> Result<Option<DriverOperationRow>, CoreError> {
    let rows = list_operations(
        conn,
        &DriverOperationFilter {
            driver_id: Some(driver_id.to_string()),
            action: Some(action.to_string()),
            limit: Some(1),
            ..Default::default()
        },
    )?;
    Ok(rows.into_iter().next())
}

/// 保留策略：条数 / 天数取先到者（初稿 1000 条、90 天，见方案 §3.5）
///
/// 启动时调一次即可；`keep` / `days` 都为 0 视为「不清理」（显式关掉）。
pub fn prune_operations(conn: &Connection, keep: usize, days: u32) -> Result<usize, CoreError> {
    if keep == 0 && days == 0 {
        return Ok(0);
    }
    let mut removed = 0usize;
    if keep > 0 {
        // 按 id 删：occurred_at 同秒的行顺序才稳定（id 单调）
        removed += conn
            .execute(
                "DELETE FROM driver_operations
                  WHERE id IN (SELECT id FROM driver_operations ORDER BY id DESC LIMIT -1 OFFSET ?1)",
                params![keep as i64],
            )
            .map_err(|e| storage_err("prune_by_count", e.to_string()))?;
    }
    if days > 0 {
        removed += conn
            .execute(
                "DELETE FROM driver_operations
                  WHERE occurred_at < datetime('now', ?1)",
                params![format!("-{} days", days)],
            )
            .map_err(|e| storage_err("prune_by_age", e.to_string()))?;
    }
    Ok(removed)
}

/// 表里有没有这张表（迁移没跑时给出可操作的提示，而不是一个 SQL 报错）
pub fn table_exists(conn: &Connection) -> Result<bool, CoreError> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'driver_operations'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map(|v| v.is_some())
    .map_err(|e| storage_err("table_exists", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn_with_table() -> Connection {
        let conn = Connection::open_in_memory().expect("memory db");
        conn.execute_batch(include_str!(
            "../../migrations/global/030_driver_operations.sql"
        ))
        .expect("create driver_operations");
        conn
    }

    /// A-基础：写一行能读回来，字段一个不少。
    #[test]
    fn record_then_read_roundtrips() {
        let conn = conn_with_table();
        let id = record_operation(
            &conn,
            &DriverOperation::ok("runtime", "install")
                .plugin("rds.driver.sqlx")
                .driver("mysql")
                .after("{\"version\":\"0.1.0\"}")
                .reason("插件目录扫描发现"),
        )
        .expect("record");
        assert_eq!(id, 1);

        let rows = list_operations(&conn, &DriverOperationFilter::default()).expect("list");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.operation.actor, "runtime");
        assert_eq!(row.operation.action, "install");
        assert_eq!(row.operation.plugin_id.as_deref(), Some("rds.driver.sqlx"));
        assert_eq!(row.operation.driver_id.as_deref(), Some("mysql"));
        assert_eq!(row.operation.result, "ok");
        assert_eq!(row.operation.after_json.as_deref(), Some("{\"version\":\"0.1.0\"}"));
        assert!(!row.occurred_at.is_empty());
    }

    /// A-失败留痕：失败也要一行，且必须带原因（否则用户看不出为什么）。
    #[test]
    fn failures_are_recorded_with_a_reason() {
        let conn = conn_with_table();
        record_operation(
            &conn,
            &DriverOperation::fail("ui", "install", "清单缺 [backend] kind"),
        )
        .expect("record fail");
        let rows = list_operations(&conn, &DriverOperationFilter::default()).expect("list");
        assert_eq!(rows[0].operation.result, "fail");
        assert_eq!(
            rows[0].operation.reason.as_deref(),
            Some("清单缺 [backend] kind")
        );
    }

    /// 非法输入直接拒绝：空白动作名 / 怪 result 都是写坏的留痕，不如报错。
    #[test]
    fn invalid_rows_are_rejected() {
        let conn = conn_with_table();
        let mut op = DriverOperation::ok("runtime", " ");
        assert!(record_operation(&conn, &op).is_err());
        op.action = "install".to_string();
        op.result = "maybe".to_string();
        assert!(record_operation(&conn, &op).is_err());
    }

    /// 过滤：按驱动 / 按动作 / 按结果；`limit` 生效。
    #[test]
    fn filters_and_limit_work() {
        let conn = conn_with_table();
        for (driver, action, result) in [
            ("mysql", "install", "ok"),
            ("mysql", "enable", "ok"),
            ("postgres", "install", "fail"),
        ] {
            let op = DriverOperation {
                result: result.to_string(),
                ..DriverOperation::ok("runtime", action).driver(driver)
            };
            record_operation(&conn, &op).expect("record");
        }

        let mysql = list_operations(
            &conn,
            &DriverOperationFilter {
                driver_id: Some("mysql".to_string()),
                ..Default::default()
            },
        )
        .expect("list mysql");
        assert_eq!(mysql.len(), 2);

        let fails = list_operations(
            &conn,
            &DriverOperationFilter {
                result: Some("fail".to_string()),
                ..Default::default()
            },
        )
        .expect("list fail");
        assert_eq!(fails.len(), 1);
        assert_eq!(fails[0].operation.driver_id.as_deref(), Some("postgres"));

        let one = list_operations(
            &conn,
            &DriverOperationFilter {
                limit: Some(1),
                ..Default::default()
            },
        )
        .expect("list limited");
        assert_eq!(one.len(), 1);
    }

    /// 「最近一次声明同步」：能力 Tab 靠它显示最后同步时间。
    #[test]
    fn latest_operation_returns_the_newest_match() {
        let conn = conn_with_table();
        record_operation(
            &conn,
            &DriverOperation::ok("migration", "declare_sync").driver("mysql_native"),
        )
        .expect("first");
        record_operation(
            &conn,
            &DriverOperation::ok("runtime", "declare_sync").driver("mysql_native"),
        )
        .expect("second");
        let latest = latest_operation(&conn, "mysql_native", "declare_sync")
            .expect("latest")
            .expect("should exist");
        assert_eq!(latest.operation.actor, "runtime");
    }

    /// 保留策略：条数上限与天数先到先删。
    #[test]
    fn prune_keeps_the_newest_rows() {
        let conn = conn_with_table();
        for _ in 0..5 {
            record_operation(&conn, &DriverOperation::ok("runtime", "reload")).expect("record");
        }
        let removed = prune_operations(&conn, 2, 0).expect("prune");
        assert_eq!(removed, 3);
        let left = list_operations(&conn, &DriverOperationFilter::default()).expect("list");
        assert_eq!(left.len(), 2);
        // 0/0 = 显式关掉
        assert_eq!(prune_operations(&conn, 0, 0).expect("noop"), 0);
    }

    /// 迁移没跑时能自证「表不在」，调用方据此给提示而不是抛 SQL 错。
    #[test]
    fn table_exists_reports_missing_table() {
        let conn = Connection::open_in_memory().expect("memory db");
        assert!(!table_exists(&conn).expect("exists check"));
    }
}
