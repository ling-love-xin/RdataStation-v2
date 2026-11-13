//! 操作记录栏（D8d）：把 `driver_operations` 摊在插件管理页上
//!
//! 口径：
//! - 只读展示**最近**若干条（`OPERATION_LOG_LIMIT`），页内再按结果 ok/fail 过滤；
//! - 读盘在**后台 executor** 上做，`render` 只读状态（视图 render 不做 I/O）；
//! - 表不存在（迁移 030 未跑）→ 给出可读提示，不 panic、不装成「0 条」；
//! - 失败动作必须能看到 `reason`——这是「不静默降级」的底线。
//!
//! 这里做「看得见」与「拿得走」：最近 N 条 + 结果过滤 + 刷新（D8d）、导出 JSON（D8e）。
//! 按驱动过滤的视图在驱动能力 Tab（D8d 后半）。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::Context;

use engine::persistence::driver_operation_store::{
    list_operations, table_exists, DriverOperationFilter, DriverOperationRow,
};

/// 一次读取的行数上限（更多历史由导出承接）
pub(crate) const OPERATION_LOG_LIMIT: usize = 50;

/// 导出的行数上限：界面只看最近 50 条，排障要往前翻更多。
/// 1000 是表本身的保留上限（`prune_operations`），再大必然读不全。
pub(crate) const OPERATION_EXPORT_LIMIT: usize = 1000;

/// 操作记录栏的展示状态（事件路径写，render 只读）
#[derive(Default)]
pub(crate) struct OperationLog {
    pub rows: Vec<DriverOperationRow>,
    pub error: Option<String>,
    pub loading: bool,
    /// 是否已经读过一次（用来决定「首次展开时自动读」）
    pub loaded: bool,
    /// 正在导出（防重复点击）
    pub exporting: bool,
    /// 导出成功的落盘说明（含路径）；与 `export_error` 二选一
    pub export_note: Option<String>,
    /// 导出失败原因
    pub export_error: Option<String>,
}

/// 读最近 N 条（同步；调用方保证在后台线程上跑）
pub(crate) fn read_operations(limit: usize) -> Result<Vec<DriverOperationRow>, String> {
    let Some(db) = engine::migration::get_global_db_manager() else {
        return Err("global.db 未初始化（宿主启动流程未完成）".to_string());
    };
    let pool = db.sqlite_pool();
    let filter = DriverOperationFilter {
        limit: Some(limit),
        ..Default::default()
    };

    // 后台线程里通常没有 tokio 上下文：优先借用插件运行时（应用主 tokio 运行时），
    // 退化路径用 `acquire_sync`（它自己建短命 runtime，代价微秒级）。
    if let Some(handle) = plugin::runtime_handle() {
        let pool = pool.clone();
        return handle.block_on(async move {
            let conn = pool.acquire().await.map_err(|e| e.to_string())?;
            let conn_ref = conn.inner().map_err(|e| e.to_string())?;
            if !table_exists(conn_ref).map_err(|e| e.to_string())? {
                return Err("driver_operations 表不存在（迁移 030 未跑）".to_string());
            }
            list_operations(conn_ref, &filter).map_err(|e| e.to_string())
        });
    }

    let conn = pool.acquire_sync().map_err(|e| e.to_string())?;
    let conn_ref = conn.inner().map_err(|e| e.to_string())?;
    if !table_exists(conn_ref).map_err(|e| e.to_string())? {
        return Err("driver_operations 表不存在（迁移 030 未跑）".to_string());
    }
    list_operations(conn_ref, &filter).map_err(|e| e.to_string())
}

/// 导出目录：全局目录下 `exports/`（与 `query_export` 的 `results/` 并列，
/// 不往工作目录/仓库里写）
pub(crate) fn export_dir() -> PathBuf {
    crate::services::workspace_loader::default_global_dir().join("exports")
}

/// 把留痕写成 JSON（同步；调用方保证在后台线程上跑）
///
/// 文件是**自描述**的：带导出时间、条数上限与「时间列是库内原值」的说明，
/// 排障的人拿到文件不必再猜格式。`before_json` / `after_json` 原样保留——
/// 它们是当时写进库里的 JSON **文本**，二次解析再序列化会丢掉「原始写了什么」。
pub(crate) fn export_operations_json(path: &Path, rows: &[DriverOperationRow]) -> Result<(), String> {
    let exported_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let payload = serde_json::json!({
        "exported_at_unix": exported_at_unix,
        "count": rows.len(),
        "limit": OPERATION_EXPORT_LIMIT,
        "note": "driver_operations 导出（排障用）：时间列为库内 occurred_at 原值；\
                 before_json / after_json 是动作前后状态的 JSON 文本，未做二次解析",
        "operations": rows,
    });
    let text =
        serde_json::to_string_pretty(&payload).map_err(|e| format!("序列化失败: {e}"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建导出目录失败: {e}"))?;
    }
    std::fs::write(path, text).map_err(|e| format!("写文件失败: {e}"))
}

impl super::PluginManagerPanel {
    /// 展开 / 收起操作记录栏；首次展开顺带读一次
    pub(crate) fn toggle_operations(&mut self, cx: &mut Context<Self>) {
        self.show_operations = !self.show_operations;
        if self.show_operations && !self.ops.borrow().loaded {
            self.load_operations(cx);
        }
        cx.notify();
    }

    /// 重新读一遍操作记录（后台读，回主线程写状态）
    pub(crate) fn load_operations(&self, cx: &mut Context<Self>) {
        let state: Rc<RefCell<OperationLog>> = self.ops.clone();
        if state.borrow().loading {
            return;
        }
        {
            let mut log = state.borrow_mut();
            log.loading = true;
            log.error = None;
        }
        cx.notify();

        let _ = cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { read_operations(OPERATION_LOG_LIMIT) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let mut log = this.ops.borrow_mut();
                log.loading = false;
                log.loaded = true;
                match loaded {
                    Ok(rows) => {
                        log.rows = rows;
                        log.error = None;
                    }
                    Err(error) => {
                        log.rows.clear();
                        log.error = Some(error);
                    }
                }
                drop(log);
                cx.notify();
            });
        });
    }

    pub(crate) fn operations(&self) -> Rc<RefCell<OperationLog>> {
        self.ops.clone()
    }

    pub(crate) fn show_operations(&self) -> bool {
        self.show_operations
    }

    pub(crate) fn ops_filter(&self) -> super::widgets::OperationResultFilter {
        self.ops_filter.get()
    }

    pub(crate) fn set_ops_filter(
        &self,
        filter: super::widgets::OperationResultFilter,
        cx: &mut Context<Self>,
    ) {
        self.ops_filter.set(filter);
        cx.notify();
    }

    /// 导出操作记录为 JSON（D8e）
    ///
    /// 导出走**重新读一遍**（不是导出屏幕上那 50 条）：排障要的是能拿走的全量，
    /// 屏幕上的条数受界面限制。读 + 写都在后台 executor 上做，回主线程只写状态。
    pub(crate) fn export_operations(&self, cx: &mut Context<Self>) {
        let state: Rc<RefCell<OperationLog>> = self.ops.clone();
        {
            let log = state.borrow();
            if log.exporting {
                return;
            }
        }
        {
            let mut log = state.borrow_mut();
            log.exporting = true;
            log.export_note = None;
            log.export_error = None;
        }
        cx.notify();

        let _ = cx.spawn(async move |this, cx| {
            let written = cx
                .background_executor()
                .spawn(async move {
                    let rows = read_operations(OPERATION_EXPORT_LIMIT)?;
                    let dir = export_dir();
                    let secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let path = dir.join(format!("driver_operations_{secs}.json"));
                    export_operations_json(&path, &rows)?;
                    Ok::<(PathBuf, usize), String>((path, rows.len()))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let mut log = this.ops.borrow_mut();
                log.exporting = false;
                match written {
                    Ok((path, count)) => {
                        log.export_note = Some(format!(
                            "已导出 {count} 条 → {}",
                            path.to_string_lossy()
                        ));
                        log.export_error = None;
                    }
                    Err(error) => {
                        log.export_note = None;
                        log.export_error = Some(format!("导出失败：{error}"));
                    }
                }
                drop(log);
                cx.notify();
            });
        });
    }
}
