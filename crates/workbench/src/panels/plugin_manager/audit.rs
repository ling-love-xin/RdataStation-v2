//! 操作记录栏（D8d）：把 `driver_operations` 摊在插件管理页上
//!
//! 口径：
//! - 只读展示**最近**若干条（`OPERATION_LOG_LIMIT`），页内再按结果 ok/fail 过滤；
//! - 读盘在**后台 executor** 上做，`render` 只读状态（视图 render 不做 I/O）；
//! - 表不存在（迁移 030 未跑）→ 给出可读提示，不 panic、不装成「0 条」；
//! - 失败动作必须能看到 `reason`——这是「不静默降级」的底线。
//!
//! 这里只做「看得见」。导出 / 翻页留给 D8e；按驱动过滤的视图在驱动能力 Tab（D8d 后半）。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::Context;

use engine::persistence::driver_operation_store::{
    list_operations, table_exists, DriverOperationFilter, DriverOperationRow,
};

/// 一次读取的行数上限（更多历史由 D8e 的导出承接）
pub(crate) const OPERATION_LOG_LIMIT: usize = 50;

/// 操作记录栏的展示状态（事件路径写，render 只读）
#[derive(Default)]
pub(crate) struct OperationLog {
    pub rows: Vec<DriverOperationRow>,
    pub error: Option<String>,
    pub loading: bool,
    /// 是否已经读过一次（用来决定「首次展开时自动读」）
    pub loaded: bool,
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
}
