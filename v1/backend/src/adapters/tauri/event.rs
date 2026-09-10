/// Tauri 事件处理模块
use tauri::Emitter;

/// 事件类型
#[derive(Debug, Clone)]
pub enum TauriEvent {
    /// 数据库连接状态变化
    ConnectionStatusChanged { conn_id: String, status: bool },
    /// 查询执行完成
    QueryCompleted {
        query_id: String,
        success: bool,
        error: Option<String>,
    },
    /// 数据更新
    DataUpdated {
        source: String,
        data: serde_json::Value,
    },
}

/// 发送事件
pub fn send_event<T: tauri::Runtime>(app: &tauri::AppHandle<T>, event: TauriEvent) {
    match event {
        TauriEvent::ConnectionStatusChanged { conn_id, status } => {
            let _ = app.emit(
                "connection-status-changed",
                serde_json::json!({
                    "conn_id": conn_id,
                    "status": status,
                }),
            );
        }
        TauriEvent::QueryCompleted {
            query_id,
            success,
            error,
        } => {
            let _ = app.emit(
                "query-completed",
                serde_json::json!({
                    "query_id": query_id,
                    "success": success,
                    "error": error,
                }),
            );
        }
        TauriEvent::DataUpdated { source, data } => {
            let _ = app.emit(
                "data-updated",
                serde_json::json!({
                    "source": source,
                    "data": data,
                }),
            );
        }
    }
}
