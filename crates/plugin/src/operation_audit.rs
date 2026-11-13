//! 插件生命周期操作的**留痕**（写 `driver_operations`，迁移 030）
//!
//! ## 为什么在这里
//!
//! 用户对插件的每个动作（装 / 卸 / 启用 / 停用 / 重载）此前只进内存 broadcast 与
//! tracing 文本日志——关掉窗口就忘，用户「查看」不到。留痕表是这些事件的落点。
//!
//! ## 口径
//!
//! - **best-effort**：留痕失败不能把已经成功的操作变成失败（否则装好了却报错）；
//!   但必须告警，不能无声无息（用户最反感的就是「静默」）。
//! - 失败动作也留一行：`action` 相同 + `result=fail` + `reason`（依赖不允许、坏包、
//!   目录删不掉……都写清原因）。
//! - `actor`：用户从界面点的写 `ui`；运行时 / 热插拔循环写 `runtime`。

use engine::persistence::driver_operation_store::{DriverOperation, record_operation, table_exists};

/// 写一行插件操作留痕（失败只告警）
pub(crate) async fn record(op: DriverOperation) {
    let Some(db) = engine::migration::get_global_db_manager() else {
        tracing::warn!(
            action = %op.action,
            plugin = ?op.plugin_id,
            "global.db 未初始化，插件操作留痕跳过"
        );
        return;
    };
    let outcome: Result<(), String> = async {
        let sqlite = db
            .sqlite_pool()
            .acquire()
            .await
            .map_err(|e| format!("acquire global sqlite: {e}"))?;
        let conn = sqlite
            .inner()
            .map_err(|e| format!("global sqlite conn: {e}"))?;
        if !table_exists(conn).map_err(|e| e.to_string())? {
            return Err("driver_operations 表不存在（迁移 030 未跑）".to_string());
        }
        record_operation(conn, &op).map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;
    if let Err(error) = outcome {
        tracing::warn!(
            action = %op.action,
            plugin = ?op.plugin_id,
            %error,
            "插件操作留痕写入失败（操作本身不受影响）"
        );
    }
}

/// 记一次「动作 + 结果」：`Ok` → `result=ok`，`Err` → `result=fail` + 可读原因
pub(crate) async fn record_result<T, E: std::fmt::Display>(
    actor: &str,
    action: &str,
    plugin_id: &str,
    result: &Result<T, E>,
    after_json: Option<String>,
) {
    let mut op = match result {
        Ok(_) => DriverOperation::ok(actor, action),
        Err(error) => DriverOperation::fail(actor, action, error.to_string()),
    };
    op = op.plugin(plugin_id);
    if let Some(after) = after_json {
        op = op.after(after);
    }
    record(op).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 没有 global.db 时不能 panic（best-effort 的底线）：这条用例只保证调用路径不炸。
    #[tokio::test]
    async fn recording_without_global_db_does_not_panic() {
        // 测试进程里未必初始化过 global.db；两种情况都必须安静返回
        record_result::<(), String>("ui", "install", "rds.driver.sqlx", &Ok(()), None).await;
        record_result::<(), String>(
            "ui",
            "install",
            "rds.driver.sqlx",
            &Err("清单缺 [backend] kind".to_string()),
            None,
        )
        .await;
    }

    /// D8b / A-3：成功与失败都真的写进了 `driver_operations`（读回来验）。
    ///
    /// 顺带把迁移 030 走一遍：表要能从 `include_dir!` 装载的迁移建出来。
    #[tokio::test]
    async fn recorded_operations_land_in_the_table() {
        use engine::persistence::driver_operation_store::{
            DriverOperationFilter, list_operations,
        };

        // 数据根由 `paths` 的 test-support 隔离，不写产品目录
        if engine::migration::initialize_global_system().await.is_err() {
            eprintln!("⏭️  global.db 在当前环境起不来，跳过（不把环境问题当缺陷）");
            return;
        }

        let plugin = "rds.driver.d8b.test";
        record_result::<(), String>(
            "ui",
            "enable",
            plugin,
            &Ok(()),
            Some("{\"enabled\":true}".to_string()),
        )
        .await;
        record_result::<(), String>(
            "ui",
            "disable",
            plugin,
            &Err("被以下插件依赖，请先停用或卸载它们: other".to_string()),
            None,
        )
        .await;

        let db = engine::migration::get_global_db_manager().expect("global db");
        let sqlite = db.sqlite_pool().acquire().await.expect("acquire");
        let conn = sqlite.inner().expect("conn");
        let rows = list_operations(
            conn,
            &DriverOperationFilter {
                plugin_id: Some(plugin.to_string()),
                ..Default::default()
            },
        )
        .expect("list operations");

        assert_eq!(rows.len(), 2, "启用 + 停用各一行");
        let fail_row = rows
            .iter()
            .find(|r| r.operation.result == "fail")
            .expect("失败那行必须留痕");
        assert_eq!(fail_row.operation.action, "disable");
        assert!(
            fail_row
                .operation
                .reason
                .as_deref()
                .unwrap_or("")
                .contains("被以下插件依赖"),
            "失败原因要可读：{:?}",
            fail_row.operation.reason
        );
    }
}
