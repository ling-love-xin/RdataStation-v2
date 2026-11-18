//! 「引用」页的取数与导航（宿主侧）
//!
//! 自 host/results 拆分纪律：这一块是**新增能力**（不是搬家），单独成文件。
//!
//! 两条边界写死在这里：
//! - 外键元数据只走 `database::metadata_service::MetadataService::list_constraints`
//!   （与 ER 视图同一套服务，`crates/database/src/er/loader.rs`），**不直连库、不碰凭据**；
//! - 取数一律在后台任务里跑（UI 线程不 await 元数据查询，与导航 M4 同一纪律）。
//!
//! 与 DBeaver 的对应：`ReferencesResultsContainer.refreshReferenceKeyList`
//! （`ReferencesResultsContainer.java:219-357`）。

use super::super::*;

impl EditorHostPanel {
    /// 【引用页】取当前结果集来源表的外键约束，回填面板。
    ///
    /// 失败分两类，处理口径不同：
    /// - **宿主自身原因**（没绑定连接）→ `Failed`，页面显示原因（用户能去修）；
    /// - **驱动不支持 / 查询报错** → 按 DBeaver 的口径**静默**成「无引用」
    ///   （`ReferencesResultsContainer.java:347-350` 的注释原文就是
    ///   「References or FKs may be unsupported by current database」，那边只 `log.debug`），
    ///   这里记一条 `debug` 日志后落 `Loaded([])`。
    pub(crate) fn load_references(
        &mut self,
        schema: String,
        table: String,
        visible_columns: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(conn_id) = self.bound_connection() else {
            self.result_panel.update(cx, |panel, cx| {
                panel.set_references(
                    result_panel::references::ReferencesState::Failed(
                        "未绑定连接，读不到外键".to_string(),
                    ),
                    cx,
                );
            });
            return;
        };

        let panel = self.result_panel.clone();
        let executor = cx.background_executor().clone();
        let metadata = database::metadata_service::MetadataService::new(
            engine::get_connection_manager().clone(),
        );
        // 日志用的副本（`table` 要 move 进后台任务）
        let table_for_log = table.clone();
        cx.spawn(async move |_this, cx| {
            let query = executor.spawn(async move {
                metadata
                    .list_constraints(&conn_id, "", &schema, &table)
                    .await
            });
            let state = match query.await {
                Ok(constraints) => result_panel::references::ReferencesState::Loaded(
                    result_panel::references::keys_from_constraints(&constraints, &visible_columns),
                ),
                Err(error) => {
                    tracing::debug!(
                        table = %table_for_log,
                        "【引用页】读外键失败，按 DBeaver 口径当作无引用：{error}"
                    );
                    result_panel::references::ReferencesState::Loaded(Vec::new())
                }
            };
            let _ = panel.update(cx, |panel, cx| panel.set_references(state, cx));
        })
        .detach();
    }

    /// 【引用页】「打开目标对象」：把目标表在导航树里定位出来。
    ///
    /// 对齐 DBeaver `refs_open_target`（`ReferencesResultsContainer.java:124-140`）：
    /// 那边经 `DBNUtils.getNodeByObject` + `NavigatorUtils.openNavigatorNode` 打开目标节点；
    /// 本仓复用「跳转定义」同一条 `request_reveal` 端口（Quick Open `⌥↵` 也走它），
    /// 经注入端口只入队、导航面板下一帧消费。
    pub(crate) fn open_reference_target(
        &mut self,
        schema: String,
        table: String,
        cx: &mut Context<Self>,
    ) {
        let Some(conn_id) = self.bound_connection() else {
            self.set_message(Some("未绑定连接，无法定位目标表".to_string()), cx);
            return;
        };
        let Some(port) = self.shared.reveal_object_port() else {
            self.set_message(Some("宿主未接入导航定位".to_string()), cx);
            return;
        };
        // 目录层本仓暂不解析（与 `load_references` 同口径传空串），目标表在 schema 内定位
        port(engine::ObjectRef::table(conn_id, "" /* catalog */, schema, table));
    }
}
