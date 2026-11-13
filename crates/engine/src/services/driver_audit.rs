//! 驱动层操作的引擎侧留痕（D8c）：**缺驱动被拒**（`action=missing_driver`）
//!
//! 为什么单独拎一个模块：`driver_operations` 的写入点分布在三个 crate——
//! `rds-driver`（声明同步 / 能力位，D8a）、`rds-plugin`（插件生命周期，D8b）、
//! 引擎（缺驱动被拒，本刀）。三处都要 best-effort，口径必须一致，所以各自有
//! 同一形态的小助手，而不是各自去拼 SQL。
//!
//! 口径（与 D8a/D8b 一致）：
//! - 写失败只 `tracing::warn!`，**不改调用方结果**（连不上就是连不上，别因为留痕
//!   再叠一个错误把真实原因盖掉）；
//! - 表不存在（迁移 030 未跑）也只告警，不阻断连接；
//! - `actor=runtime`：用户点的是「连接」，这一行记的是**被拒**这件事。

use rds_store::persistence::driver_operation_store::{
    record_operation, table_exists, DriverOperation,
};

/// 记一行「缺驱动 → 拒绝执行」。`driver_id` 是连接配置里写的驱动 id（如 `mysql`）。
///
/// `reason` 由调用方给出可读原因（要点名驱动 id 与去向），这里只负责落盘。
pub async fn record_missing_driver(driver_id: &str, reason: impl Into<String>) {
    let op = DriverOperation::fail("runtime", "missing_driver", reason).driver(driver_id);
    let Some(db) = crate::migration::get_global_db_manager() else {
        tracing::warn!(
            driver = %driver_id,
            "global.db 未初始化，缺驱动留痕跳过"
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
            driver = %driver_id,
            %error,
            "缺驱动留痕写入失败（拒绝本身不受影响）"
        );
    }
}

#[cfg(test)]
mod tests {
    /// 没有 global.db 时不能 panic（best-effort 底线）
    #[tokio::test]
    async fn missing_driver_record_is_best_effort() {
        super::record_missing_driver("mysql", "驱动 'mysql' 未注册").await;
    }
}
