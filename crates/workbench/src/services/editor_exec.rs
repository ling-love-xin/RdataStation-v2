//! 编辑器执行器（A14）：把编辑器的 SQL 交给引擎的 `SqlService`
//!
//! 编辑器（`crates/editor`）只定义执行端口（`QueryRunner`），不知道连接从哪来；这里回答
//! 那个问题：**当前活动连接**（`SqlService::execute(None, …)` 走连接管理器里的活动连接）。
//! 连接绑定到具体数据源是 1b 的事，那之前先"用当前连接跑"，跑不成会明确报错。
//!
//! ## 中断与超时（B3）
//!
//! - **中断**：`SqlService::cancel_query(conn_id)` 翻转该连接的取消令牌（引擎侧
//!   `query_with_cancel` 一直 select 着它），驱动因此回一条 "Query cancelled"。
//! - **超时**：按**连接**的 `query_timeout`（连接对话框的高级选项）交给引擎的
//!   `SqlExecuteOptions::timeout_ms`——到点后引擎自己取消并报 "Query timed out after Nms"。
//!   连接没配 = 没有超时，不在这里编一个默认值。
//!
//! ## 线程模型
//!
//! 编辑器的执行通道在自己的**工作线程**上调用 `QueryRunner::run`，所以这里可以安全地
//! `block_on` 一个专用 tokio runtime（驱动是异步的，而 UI 线程不能等）。
//!
//! ## 取数口径
//!
//! 结果一律经 `QueryResult::batches` / `to_rows()` 取（架构 §12 #21：驱动只填 `batches`，
//! 读 `rows` / `total_rows` **字段**会得到"大结果有数、小结果无数"）。

use std::sync::Arc;

use editor::channel::ExecChannel;
use editor::execution::{QueryData, QueryRunner};
use editor::shared::EditorShared;
use engine::duckdb::accel;
use engine::duckdb::federation::registry::{self as fed_registry, FederatedSource, MountState};
use engine::duckdb::federation::session as fed_session;
use engine::persistence::history_store::{self, SqlHistoryEntry};
use engine::services::sql_service::{SqlExecuteOptions, SqlService, window_sql};
use shared::models::{QueryResult, Value};

/// 引擎执行器（编辑器执行端口的工作台实现）
struct EngineQueryRunner {
    /// 执行器的 tokio runtime（**多线程**，随执行器常驻）
    ///
    /// 为什么不是 `current_thread`：sqlx 的连接在**首次使用时**把驱动任务（读写协议的
    /// 那个后台任务）挂到当时的 runtime 上；单线程 runtime 只在 `block_on` 期间被驱动，
    /// 一旦某条语句死在另一个 runtime 上（事务提交就是这么一条：它走旁路调用），
    /// 驱动任务就没人跑了——真机现象是 `COMMIT` 永远等不到响应。多线程 runtime 留出
    /// 工作线程，让这些后台任务在任何调用路径下都能推进。
    runtime: tokio::runtime::Runtime,
    service: SqlService,
}

impl EngineQueryRunner {
    fn new() -> Option<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("rds-editor-exec-rt")
            .build()
            .ok()?;
        let manager = engine::connection_manager::get_connection_manager();
        Some(Self {
            runtime,
            service: SqlService::new(manager.clone()),
        })
    }

    /// 本次执行该用多长超时（毫秒）：按**连接**的 `query_timeout`（B3）
    ///
    /// 未绑定的文档看当前活动连接（与 `execute(None, …)` 同口径）；连接没配就是
    /// **没有超时**——不在这里造一个默认值（超时是连接属性，不是编辑器属性）。
    async fn query_timeout_ms(&self, connection: Option<&str>) -> Option<u64> {
        let manager = engine::connection_manager::get_connection_manager();
        let conn_id = match connection {
            Some(id) => Some(id.to_string()),
            None => manager.get_active_connection_id().await,
        }?;
        let config = manager.get_connection_config(&conn_id).await?;
        config.query_timeout.map(|secs| u64::from(secs) * 1000)
    }

    /// 本次执行到底用哪个连接 id（绑定优先 → 未绑定回退当前活动连接）
    fn resolve_conn_id(&self, connection: Option<&str>) -> Option<String> {
        let manager = engine::connection_manager::get_connection_manager();
        self.runtime.block_on(async {
            match connection {
                Some(id) => Some(id.to_string()),
                None => manager.get_active_connection_id().await,
            }
        })
    }

    /// 一条连接的**运行时连接串**（含凭据）——只有执行路径才拿它
    ///
    /// 为什么不用 `ConnectionInfo.url`：那是**脱敏**的（`user:******@`），`ATTACH` 拿它只会
    /// 认证失败（真机现场：`Access denied for user 'root'@…`）。为什么不用 Secret：实测
    /// **DuckDB 1.5.5 的 mysql / postgres 扫描器不认 Secret**（见
    /// `crates/engine/tests/federation_credentials_probe.rs` 的台账），凭据只能随连接串进 `ATTACH`
    /// ——代价是离开引擎的文本必须脱敏（引擎侧已收口：`accel::scrub_credentials`）。
    ///
    /// 取的是连接管理器里的 `DriverConnectionConfig.url_override`（重连用的那份，带凭据）；
    /// 拿不到时回退到 `ConnectionInfo.url`（会在认证上如实失败，不会静静挂错库）。
    fn runtime_connection_string(&self, conn_id: &str) -> Option<String> {
        let manager = engine::connection_manager::get_connection_manager();
        let config = self
            .runtime
            .block_on(manager.get_connection_config(&conn_id.to_string()));
        config
            .and_then(|config| config.url_override)
            .filter(|url| !url.trim().is_empty())
    }

    /// 加速档的源：宿主侧的连接信息（`db_type` + **带凭据的**运行时连接串）→ 引擎的 [`accel::AccelSource`]
    ///
    /// 引擎不读连接库、也不解密口令：这里把两样它需要的东西递过去。
    fn accel_source(&self, connection: Option<&str>) -> Result<accel::AccelSource, String> {
        let manager = engine::connection_manager::get_connection_manager();
        let conn_id = self
            .resolve_conn_id(connection)
            .ok_or_else(|| "本地加速需要一个连接：先绑定一个，或在导航里选中它".to_string())?;
        let info = self
            .runtime
            .block_on(manager.get_connection_info(&conn_id))
            .ok_or_else(|| format!("拿不到连接 {conn_id} 的信息，无法在本地挂载它"))?;
        let url = self
            .runtime_connection_string(&conn_id)
            .unwrap_or_else(|| info.url.clone());
        accel::AccelSource::new(&conn_id, &info.db_type, &url)
    }

    /// 加速档执行（B13）：在 DuckDB 上跑，源库以**只读**方式挂着
    ///
    /// 与源库档同口径：查询只取**第一段**（套窗口），写语句原样（DuckDB 侧只读挂载会
    /// 拒掉作用源对象的写，本地临时对象照常允许）。
    fn run_on_accel(&self, connection: Option<&str>, sql: &str) -> Result<QueryData, String> {
        let source = self.accel_source(connection)?;
        let session = accel::ensure_session(&source)?;
        let (to_run, segmented) = match window_sql(sql, editor::execution::SEGMENT_ROWS, 0) {
            Some(wrapped) => (wrapped, true),
            None => (sql.to_string(), false),
        };
        let started = std::time::Instant::now();
        let outcome = session.run(&to_run);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_accel_history(&source, sql, elapsed_ms, &outcome);
        // 【B8】历史面板据此重新加载（与源库档同一节拍）
        editor::history::bump();
        let result = outcome.map_err(|error| error.to_string())?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.has_more = segmented
            && !data.columns.is_empty()
            && data.rows.len() == editor::execution::SEGMENT_ROWS;
        Ok(data)
    }

    /// 加速档的「取下一段」（与源库档同一套窗口包装，见 `sql_service::window_sql`）
    fn fetch_next_on_accel(
        &self,
        connection: Option<&str>,
        sql: &str,
        offset: usize,
        limit: usize,
    ) -> Result<QueryData, String> {
        let source = self.accel_source(connection)?;
        let session = accel::ensure_session(&source)?;
        let wrapped = window_sql(sql, limit, offset)
            .ok_or_else(|| "这份结果没有可再取的部分".to_string())?;
        let started = std::time::Instant::now();
        let outcome = session.run(&wrapped);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_accel_history(&source, sql, elapsed_ms, &outcome);
        editor::history::bump();
        let result = outcome.map_err(|error| error.to_string())?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.has_more = data.rows.len() == limit;
        Ok(data)
    }

    /// 加速档的下发（筛选 / 排序）：改写（引擎的 `sql::rewrite_*`）之后照跑
    fn run_rewritten_on_accel(
        &self,
        connection: Option<&str>,
        rewritten: &engine::sql::Rewrite,
        notice: impl FnOnce(&str) -> String,
    ) -> Result<QueryData, String> {
        let source = self.accel_source(connection)?;
        let session = accel::ensure_session(&source)?;
        let started = std::time::Instant::now();
        let outcome = session.run(&rewritten.sql);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_accel_history(&source, &rewritten.sql, elapsed_ms, &outcome);
        editor::history::bump();
        let result = outcome.map_err(|error| error.to_string())?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.notice = rewritten.dropped_limit.as_deref().map(notice);
        Ok(data)
    }

    // ===== 联邦档（B13 第一期后半） =====

    /// 组装本次联邦的源清单（**宿主组装**：引擎不读连接库、也不解密口令）
    ///
    /// 判据只有一个：连接开了 **「DuckDB 本地加速（联邦查询直连源库）」**（`use_duckdb_fed`）
    /// ——它就是「允许分析引擎直连本连接」这个意图，同时也是**没有原生驱动的库**
    /// （Oracle 这类：应用连不上它，但 DuckDB 扫描器能）唯一的参与入口。
    ///
    /// 两个来源，一套判据：
    ///
    /// 1. **连接记录**（`DataSourceService::list()`，全局表）：**不要求已连接**，连接串现组
    ///    （`build_connection_url` 解密口令）——这是驱动类库能进来的那条路；
    /// 2. **运行态连接**（连接管理器里已建连的，含项目作用域 `P_`）：用重连用的
    ///    `url_override`（带凭据），按同样的开关过滤。
    ///
    /// 驱动能不能挂由 [`accel::AccelKind`] 回答，认不出就如实记一句（L2 的 Oracle 在
    /// T3.2 接进来之前就落在这一条上）；**别名**由连接名生成（`sanitize_alias` + 重名再排），
    /// 按连接 id 排序保证稳定；**主源** = 本文档绑定的那条连接（它不在清单里时由引擎选
    /// 第一个可用源，并留下回退说明）。
    fn federated_plan(&self, connection: Option<&str>) -> Result<FederatedPlan, String> {
        let manager = engine::connection_manager::get_connection_manager();
        let owner = self.resolve_conn_id(connection).ok_or_else(|| {
            "联邦查询需要一个连接：先绑定一个，或在导航里选中它".to_string()
        })?;

        let mut records: Vec<FedSourceRecord> = Vec::new();
        let mut notes: Vec<String> = Vec::new();

        // ① 连接记录（不要求已连接）
        if let Ok(service) = crate::services::data_source_service::DataSourceService::global() {
            match self.runtime.block_on(service.list()) {
                Ok(items) => {
                    for item in items.into_iter().filter(|item| item.use_duckdb_fed) {
                        match connection::url::build_connection_url(&item) {
                            Ok(url) => records.push(FedSourceRecord {
                                conn_id: item.id.clone(),
                                name: item.name.clone(),
                                db_type: item.db_type.clone(),
                                url,
                            }),
                            Err(reason) => notes.push(format!(
                                "连接 {} 的连接串没能组装出来：{reason}",
                                item.name
                            )),
                        }
                    }
                }
                Err(error) => notes.push(format!("读连接记录失败：{error}")),
            }
        } else {
            notes.push("连接库还没就绪，本次只用上了已建立的连接".to_string());
        }

        // ② 运行态连接（含项目作用域；记录里没有的补上）
        let infos = self.runtime.block_on(manager.get_all_connection_info());
        for info in infos.iter().filter(|info| info.use_duckdb_fed) {
            if records.iter().any(|record| record.conn_id == info.id) {
                continue;
            }
            let Some(url) = self.runtime_connection_string(&info.id) else {
                notes.push(format!("连接 {} 拿不到运行时连接串，未参与", info.name));
                continue;
            };
            records.push(FedSourceRecord {
                conn_id: info.id.clone(),
                name: info.name.clone(),
                db_type: info.db_type.clone(),
                url,
            });
        }

        plan_from_records(&records, &notes, &owner)
    }

    /// 联邦档执行：在联邦会话上跑（源清单为空/全挂不上都会提前给可读原因）
    fn run_on_federation(&self, connection: Option<&str>, sql: &str) -> Result<QueryData, String> {
        let (plan, session) = self.federated_session(connection)?;
        // 与另两档同口径：查询只取**第一段**（套窗口），写语句原样交给引擎
        let (to_run, segmented) = match window_sql(sql, editor::execution::SEGMENT_ROWS, 0) {
            Some(wrapped) => (wrapped, true),
            None => (sql.to_string(), false),
        };
        let started = std::time::Instant::now();
        let outcome = session.run(&to_run);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_federation_history(&plan, &session, sql, elapsed_ms, &outcome);
        editor::history::bump();
        let result = outcome
            .map_err(|error| explain_federation_error(&session, error.to_string()))?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.has_more = segmented
            && !data.columns.is_empty()
            && data.rows.len() == editor::execution::SEGMENT_ROWS;
        data.notice = federation_notice(&plan, &session);
        Ok(data)
    }

    /// 联邦档的「取下一段」（同一套窗口包装，见 `sql_service::window_sql`）
    fn fetch_next_on_federation(
        &self,
        connection: Option<&str>,
        sql: &str,
        offset: usize,
        limit: usize,
    ) -> Result<QueryData, String> {
        let (plan, session) = self.federated_session(connection)?;
        let wrapped = window_sql(sql, limit, offset)
            .ok_or_else(|| "这份结果没有可再取的部分".to_string())?;
        let started = std::time::Instant::now();
        let outcome = session.run(&wrapped);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_federation_history(&plan, &session, sql, elapsed_ms, &outcome);
        editor::history::bump();
        let result = outcome
            .map_err(|error| explain_federation_error(&session, error.to_string()))?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.has_more = data.rows.len() == limit;
        Ok(data)
    }

    /// 联邦档的下发（筛选 / 排序）：改写之后照跑（`notice` 由调用方给）
    fn run_rewritten_on_federation(
        &self,
        connection: Option<&str>,
        rewritten: &engine::sql::Rewrite,
        notice: impl FnOnce(&str) -> String,
    ) -> Result<QueryData, String> {
        let (plan, session) = self.federated_session(connection)?;
        let started = std::time::Instant::now();
        let outcome = session.run(&rewritten.sql);
        let elapsed_ms = started.elapsed().as_millis() as u64;
        record_federation_history(&plan, &session, &rewritten.sql, elapsed_ms, &outcome);
        editor::history::bump();
        let result = outcome
            .map_err(|error| explain_federation_error(&session, error.to_string()))?;
        let mut data = to_data(&result, elapsed_ms, false);
        data.notice = rewritten.dropped_limit.as_deref().map(notice);
        Ok(data)
    }

    /// 组装源清单 + 取（必要时建立）会话（两件事总是一起做，收口在这里）
    fn federated_session(
        &self,
        connection: Option<&str>,
    ) -> Result<(FederatedPlan, Arc<fed_session::FederatedSession>), String> {
        let plan = self.federated_plan(connection)?;
        let session = fed_session::ensure_session(&plan.owner, &plan.sources, plan.primary.as_deref())?;
        Ok((plan, session))
    }
}

impl QueryRunner for EngineQueryRunner {
    fn run(
        &self,
        connection: Option<&str>,
        channel: ExecChannel,
        sql: &str,
        run_options: editor::execution::RunOptions,
    ) -> Result<QueryData, String> {
        // 【B13】本地跑的两档不走源库驱动：这条语句在 DuckDB 上跑（源库只读挂载）
        if channel == ExecChannel::Federated {
            return self.run_on_federation(connection, sql);
        }
        if channel != ExecChannel::Source {
            return self.run_on_accel(connection, sql);
        }
        let timeout_ms = self.runtime.block_on(self.query_timeout_ms(connection));
        let options = SqlExecuteOptions {
            // 历史由引擎侧统一记录（含耗时/行数，真实值）
            record_history: true,
            // 超时：引擎在到点后**自己取消**并返回“Query timed out after Nms”
            timeout_ms,
            // B4：自动提交关 → 引擎在没有事务时先自动开一个（DDL 除外）
            use_transaction: run_options.use_transaction,
            // 【B13】历史要记“这句在哪儿跑的”（源库档也记，别让加速档成为唯一带标记的）
            channel: Some(channel.code().to_string()),
            ..Default::default()
        };
        // 连接：文档绑定了就用它（B1）；未绑定回退到“当前活动连接”（1a 口径）
        //
        // 【B5b】查询走 `execute_first_segment`：只取第一段（1000 行），后续段由「取下一段」来要；
        // 写语句没有分段可言，引擎内部会走原路（历史记的都是**用户执行的原 SQL**）。
        let executed = self
            .runtime
            .block_on(self.service.execute_first_segment(
                connection.map(str::to_string),
                sql,
                editor::execution::SEGMENT_ROWS,
                options,
            ));
        // 【B8】不管成败，引擎都记了一条历史（含耗时 / 行数 / 失败原因）——
        // 摇一下版本号：右 Dock 的历史面板据此重新加载（面板不读盘，也不每帧问文件）
        editor::history::bump();
        let executed = executed.map_err(|error| error.to_string())?;
        let mut data = to_data(&executed.result, executed.elapsed_ms, executed.truncated);
        // 【B5b】首段拿满了就标“可能还有下一段”（写语句没有分段可言）：
        // 拿不满 = 这一份结果就这么多，界面不摆「取下一段」。
        data.has_more =
            !data.columns.is_empty() && data.rows.len() == editor::execution::SEGMENT_ROWS;
        Ok(data)
    }

    /// 【B14】下发源库：把筛选词拼成 `WHERE` 重查，结果落**新结果集**
    ///
    /// 方言差异（CAST 目标类型）与 LIMIT 处理都在执行器里（它才知道连接是什么库），
    /// 这里只把原料给过去。
    fn run_filtered(
        &self,
        connection: Option<&str>,
        channel: ExecChannel,
        sql: &str,
        filter: &str,
        columns: &[String],
    ) -> Result<QueryData, String> {
        let manager = engine::connection_manager::get_connection_manager();
        let db_type = self.runtime.block_on(async {
            let conn_id = match connection {
                Some(id) => Some(id.to_string()),
                None => manager.get_active_connection_id().await,
            }?;
            manager
                .get_connection_info(&conn_id)
                .await
                .map(|info| info.db_type)
        });
        // MySQL 没有 `CAST(x AS TEXT)` 这个目标类型（用 CHAR）；其余库用 TEXT
        let cast_type = match db_type.as_deref() {
            Some(kind) if kind.to_ascii_lowercase().starts_with("mysql") => "CHAR",
            _ => "TEXT",
        };
        let rewritten = engine::sql::rewrite_with_filter(sql, columns, filter, cast_type)?;
        // 【B13】本地跑的两档：改写照样做，但语句在 DuckDB 上跑（不是发回源库）
        if channel == ExecChannel::Federated {
            return self.run_rewritten_on_federation(connection, &rewritten, |limit| {
                format!("已去掉原查询的 {limit}（下发筛选要能查到全部行）")
            });
        }
        if channel != ExecChannel::Source {
            return self.run_rewritten_on_accel(connection, &rewritten, |limit| {
                format!("已去掉原查询的 {limit}（下发筛选要能查到全部行）")
            });
        }
        let timeout_ms = self.runtime.block_on(self.query_timeout_ms(connection));
        let options = SqlExecuteOptions {
            // 下发是一次真查询：同样进历史（用户能回头找）
            record_history: true,
            timeout_ms,
            channel: Some(channel.code().to_string()),
            ..Default::default()
        };
        let executed = self
            .runtime
            .block_on(self.service.execute(
                connection.map(str::to_string),
                &rewritten.sql,
                options,
            ));
        editor::history::bump();
        let executed = executed.map_err(|error| error.to_string())?;
        let mut data = to_data(&executed.result, executed.elapsed_ms, executed.truncated);
        // 去掉过 LIMIT 要说出来（场景 34）：不说的话用户会以为“怎么多了这么多行”
        data.notice = rewritten
            .dropped_limit
            .map(|limit| format!("已去掉原查询的 {limit}（下发筛选要能查到全部行）"));
        Ok(data)
    }

    /// 【B14】排序下发：按用户点的那一列重查（不 CAST——让源库按自己的列类型排）
    fn run_sorted_down(
        &self,
        connection: Option<&str>,
        channel: ExecChannel,
        sql: &str,
        column: &str,
        descending: bool,
    ) -> Result<QueryData, String> {
        let rewritten = engine::sql::rewrite_with_order(sql, column, descending)?;
        // 【B13】本地跑的两档：同上——改写服从“在哪儿跑”
        if channel == ExecChannel::Federated {
            return self.run_rewritten_on_federation(connection, &rewritten, |limit| {
                format!("已去掉原查询的 {limit}（排序下发要能排全部行）")
            });
        }
        if channel != ExecChannel::Source {
            return self.run_rewritten_on_accel(connection, &rewritten, |limit| {
                format!("已去掉原查询的 {limit}（排序下发要能排全部行）")
            });
        }
        let timeout_ms = self.runtime.block_on(self.query_timeout_ms(connection));
        let options = SqlExecuteOptions {
            record_history: true,
            timeout_ms,
            channel: Some(channel.code().to_string()),
            ..Default::default()
        };
        let executed = self
            .runtime
            .block_on(self.service.execute(
                connection.map(str::to_string),
                &rewritten.sql,
                options,
            ));
        editor::history::bump();
        let executed = executed.map_err(|error| error.to_string())?;
        let mut data = to_data(&executed.result, executed.elapsed_ms, executed.truncated);
        data.notice = rewritten
            .dropped_limit
            .map(|limit| format!("已去掉原查询的 {limit}（排序下发要能排全部行）"));
        Ok(data)
    }

    /// 【B5b】取下一段：同一条原 SQL 的后一段（引擎套窗口取，见 `SqlService::execute_segment`）
    fn fetch_next(
        &self,
        connection: Option<&str>,
        channel: ExecChannel,
        sql: &str,
        offset: usize,
        limit: usize,
    ) -> Result<QueryData, String> {
        // 【B13】本地跑的两档的分段：同一套窗口包装，但在 DuckDB 上取数
        if channel == ExecChannel::Federated {
            return self.fetch_next_on_federation(connection, sql, offset, limit);
        }
        if channel != ExecChannel::Source {
            return self.fetch_next_on_accel(connection, sql, offset, limit);
        }
        let timeout_ms = self.runtime.block_on(self.query_timeout_ms(connection));
        let executed = self
            .runtime
            .block_on(self.service.execute_segment(
                connection.map(str::to_string),
                sql,
                limit,
                offset,
                timeout_ms,
            ))
            .map_err(|error| error.to_string())?;
        let mut data = to_data(&executed.result, executed.elapsed_ms, executed.truncated);
        // “还有没有下一段”由**拿没拿满**判（引擎不问总数）
        data.has_more = data.rows.len() == limit;
        Ok(data)
    }

    /// 事务状态：读引擎的**真值**（活动事务挂在连接管理器上，不是猜的）
    fn transaction_snapshot(&self, connection: Option<&str>) -> editor::execution::TxSnapshot {
        let status = self
            .runtime
            .block_on(self.service.get_transaction_status(connection.map(str::to_string)));
        match status {
            Ok(status) => editor::execution::TxSnapshot {
                in_transaction: status.is_in_transaction,
            },
            // 读不到就当作“不在事务里”：界面会因此少一个 TX 标记，不会凭空多一个
            Err(_) => editor::execution::TxSnapshot::default(),
        }
    }

    /// 事务动作：走引擎的驱动级事务接口（不是拼 `BEGIN` 文本——MySQL 会报 1295）
    fn transaction(
        &self,
        connection: Option<&str>,
        action: editor::execution::TxAction,
    ) -> Result<(), String> {
        use editor::execution::TxAction;
        let conn = connection.map(str::to_string);
        let result = match action {
            TxAction::Begin => self.runtime.block_on(self.service.begin_transaction(conn)),
            TxAction::Commit => self.runtime.block_on(self.service.commit_transaction(conn)),
            TxAction::Rollback => self.runtime.block_on(self.service.rollback_transaction(conn)),
        };
        result.map(|_status| ()).map_err(|error| error.to_string())
    }

    /// 引擎侧四个原生驱动都支持事务（P0.2 实测：含 MySQL 的驱动级事务）
    fn supports_transactions(&self) -> bool {
        true
    }

    /// 【M8】未绑定连接的文档会落到哪条连接上（洞察入口要「产生这份结果的那条连接」）
    ///
    /// 与 [`Self::resolve_conn_id`] **同一处口径**：否则会出现「执行走 A、洞察取样走 B」
    /// 这种最难查的偏差。借执行器自己的 runtime 做一次内存读锁，代价可忽略。
    fn active_connection(&self) -> Option<String> {
        self.resolve_conn_id(None)
    }

    /// 中断：源库档翻引擎的取消令牌；本地跑的两档叫 DuckDB 中断
    ///
    /// 三处都试是有意的：编辑器只知道“这条文档在执行”，不知道当前那次执行具体落在哪一边
    /// （状态在引擎侧），而每一边的中断都是幂等的（没在跑就是 noop / `false`）。
    fn cancel(&self, connection: Option<&str>) -> Result<bool, String> {
        let mut cancelled = false;
        let mut reason: Option<String> = None;
        if let Some(conn_id) = self.resolve_conn_id(connection) {
            for outcome in [accel::cancel(&conn_id), fed_session::cancel(&conn_id)] {
                match outcome {
                    Some(Ok(true)) => cancelled = true,
                    Some(Ok(false)) | None => {}
                    Some(Err(error)) => reason = Some(error),
                }
            }
        }
        match self
            .runtime
            .block_on(self.service.cancel_query(connection.map(str::to_string)))
        {
            Ok(true) => cancelled = true,
            Ok(false) => {}
            Err(error) => {
                // 只有两遍都没送出去才算失败
                if !cancelled && reason.is_none() {
                    reason = Some(error.to_string());
                }
            }
        }
        match (cancelled, reason) {
            (true, _) => Ok(true),
            (false, Some(reason)) => Err(reason),
            (false, None) => Ok(false),
        }
    }

    /// 【B13/T1.6】重新挂载源（表清单刷新；加速档那一条 / 联邦档某一源或全挂）
    fn refresh_sources(
        &self,
        connection: Option<&str>,
        channel: ExecChannel,
        alias: Option<&str>,
    ) -> Result<String, String> {
        // 加速档：`alias` 无意义（就那一条源）
        if channel != ExecChannel::Federated {
            let source = self.accel_source(connection)?;
            let session = accel::ensure_session(&source)?;
            session.refresh().map_err(|error| error.to_string())?;
            tracing::info!(source = %source.conn_id, "本地加速源已重新挂载（表清单已刷新）");
            return Ok("已重新挂载源库（新表可见了）".to_string());
        }

        // 联邦档：会话已经建好（源清单就是按同一套口径组装的），这里只重挂
        let owner = self.resolve_conn_id(connection).ok_or_else(|| {
            "联邦源需要一个连接：先绑定一个，或在导航里选中它".to_string()
        })?;
        match alias {
            Some(alias) => {
                fed_session::refresh_source(&owner, alias)?;
                // 重挂之后状态就是结论：失败要说原因（不能只报“重挂完了”）
                let state = fed_session::snapshot_for(&owner).and_then(|snapshot| {
                    snapshot
                        .sources
                        .iter()
                        .find(|entry| entry.alias() == alias)
                        .map(|entry| entry.state.clone())
                });
                match state {
                    Some(MountState::Ready { tables }) => {
                        Ok(format!("已重新挂载 {alias}（{tables} 张表）"))
                    }
                    Some(MountState::Failed(reason)) => Err(format!("源 {alias} 仍挂不上：{reason}")),
                    None => Ok(format!("已重新挂载 {alias}")),
                }
            }
            None => {
                let results = fed_session::refresh_all(&owner)?;
                let failed: Vec<String> = results
                    .iter()
                    .filter_map(|(alias, outcome)| {
                        outcome
                            .as_ref()
                            .err()
                            .map(|reason| format!("{alias}：{reason}"))
                    })
                    .collect();
                if failed.is_empty() {
                    Ok(format!("已重新挂载 {} 个源（新表可见了）", results.len()))
                } else {
                    Err(format!(
                        "{} 个源重挂成功，{} 个失败：{}",
                        results.len() - failed.len(),
                        failed.len(),
                        failed.join("；")
                    ))
                }
            }
        }
    }

    /// 【T1.6】换主源（未限定名从此在它里面解析）
    fn set_federated_primary(&self, connection: Option<&str>, alias: &str) -> Result<String, String> {
        let owner = self.resolve_conn_id(connection).ok_or_else(|| {
            "联邦源需要一个连接：先绑定一个，或在导航里选中它".to_string()
        })?;
        fed_session::set_primary(&owner, alias)?;
        Ok(format!("主源已切到 {alias}（未限定名的解析者）"))
    }
}

/// 加速档的执行也进同一条历史流水（与源库档一致）
///
/// `db_type` 写**源库的真实类型**、`channel` 写 `accelerated`：两个维度分开记——
/// “数据是什么库的”与“语句在哪儿跑的”是两件事，历史面板按它们拼出 `MYSQL·加速`。
fn record_accel_history(
    source: &accel::AccelSource,
    sql: &str,
    elapsed_ms: u64,
    outcome: &Result<QueryResult, shared::error::CoreError>,
) {
    let entry = match outcome {
        Ok(result) => SqlHistoryEntry {
            conn_id: Some(source.conn_id.clone()),
            db_type: Some(source.kind.label().to_string()),
            elapsed_ms,
            success: true,
            error_message: None,
            rows_returned: Some(result.total_rows() as u64),
            rows_affected: None,
            channel: Some(editor::channel::ExecChannel::Accelerated.code().to_string()),
            sources: None,
        },
        Err(error) => SqlHistoryEntry {
            conn_id: Some(source.conn_id.clone()),
            db_type: Some(source.kind.label().to_string()),
            elapsed_ms,
            success: false,
            error_message: Some(error.to_string()),
            rows_returned: None,
            rows_affected: None,
            channel: Some(editor::channel::ExecChannel::Accelerated.code().to_string()),
            sources: None,
        },
    };
    if let Err(error) = history_store::save_sql_history(sql, &entry) {
        tracing::error!(error = %error, "本地加速的执行未记入历史");
    }
}

/// 联邦执行的一次组装结果（源清单 + 主源请求 + 组装时的实话）
///
/// 为什么要留住 `notes`：源清单浮层（T1.6）之前，用户看不到“为什么某个连接没进来”，
/// 不能让它静默——结果区那行小字得能说出来。
#[derive(Debug)]
struct FederatedPlan {
    /// 会话主人（按它缓存会话；= 本文档绑定的连接 id）
    owner: String,
    sources: Vec<FederatedSource>,
    /// 请求的主源别名（`None` = 本文档的连接没参与，由引擎选第一个可用源）
    primary: Option<String>,
    /// 组装时的说明（跳过的连接 / 本文档连接未参与）
    notes: Vec<String>,
}

/// 组装用的源记录（宿主从连接库 / 运行态读来；`url` 已带凭据）
#[derive(Debug, Clone, PartialEq, Eq)]
struct FedSourceRecord {
    conn_id: String,
    name: String,
    db_type: String,
    /// 运行时连接串（`accel::AccelSource::new` 认的那份）
    url: String,
}

/// 从源记录组装清单（**纯函数**：不碰连接库，便于逐条断言）
///
/// 口径见 [`EngineQueryRunner::federated_plan`]；`notes` 是取记录时已经攒下的实话
/// （读连接库失败 / 组装连接串失败之类），会一并带到结果区那行小字里。
fn plan_from_records(
    records: &[FedSourceRecord],
    notes: &[String],
    owner: &str,
) -> Result<FederatedPlan, String> {
    // 顺序不稳定（两张表合起来）：按连接 id 排序 → 同一份清单每次组装出同样的别名
    let mut ordered: Vec<&FedSourceRecord> = records.iter().collect();
    ordered.sort_by(|a, b| a.conn_id.cmp(&b.conn_id));

    let mut notes: Vec<String> = notes.to_vec();
    let mut taken: Vec<String> = Vec::new();
    let mut sources: Vec<FederatedSource> = Vec::new();
    let mut primary: Option<String> = None;
    let mut owner_seen = false;

    // 同一个 conn_id 只算一次（记录与运行态都出现时）
    let mut seen: Vec<&str> = Vec::new();
    for record in ordered {
        if record.conn_id == owner {
            owner_seen = true;
        }
        if seen.contains(&record.conn_id.as_str()) {
            continue;
        }
        seen.push(&record.conn_id);
        if let Err(reason) = accel::AccelKind::from_db_type(&record.db_type) {
            notes.push(format!("连接 {} 没参与：{reason}", record.name));
            continue;
        }
        let alias = fed_registry::unique_alias(&fed_registry::sanitize_alias(&record.name), &taken);
        taken.push(alias.clone());
        // 凭据随连接串进 ATTACH（DuckDB 的扫描器不认 Secret，见 `accel::AccelSource::new`）
        let source = FederatedSource::new(&record.conn_id, &alias, &record.db_type, &record.url)?;
        if record.conn_id == owner {
            primary = Some(alias);
        }
        sources.push(source);
    }

    if sources.is_empty() {
        return Err(
            "还没有可用作联邦源的连接：在连接对话框 → 高级里开启「DuckDB 本地加速（联邦查询直连源库）」"
                .to_string(),
        );
    }
    // 联邦与本地加速的区别就是“跨源”：只有一个源时两者是同一件事（门控也是这么挡的）
    if sources.len() < 2 {
        return Err(format!(
            "联邦查询至少需要两个源（现在只有 {}）：把另一个连接也开启「DuckDB 本地加速」",
            sources[0].alias
        ));
    }
    if !owner_seen {
        notes.push(
            "本文档的连接未开启「DuckDB 本地加速」，没有作为联邦源参与".to_string(),
        );
    } else if primary.is_none() {
        notes.push(format!(
            "本文档的连接（{owner}）未能作为联邦源参与（见上一条说明）"
        ));
    }

    Ok(FederatedPlan {
        owner: owner.to_string(),
        sources,
        primary,
        notes,
    })
}

/// 联邦档的执行也进同一条历史流水（与另两档一致）
///
/// `db_type` 取**主源**的库类型（历史面板据此拼出 `MYSQL·联邦`）、`sources` 取本次
/// **真正挂上**的源别名：用户重看这条历史时要能对上是哪几个源。
fn record_federation_history(
    plan: &FederatedPlan,
    session: &fed_session::FederatedSession,
    sql: &str,
    elapsed_ms: u64,
    outcome: &Result<QueryResult, shared::error::CoreError>,
) {
    let snapshot = session.snapshot();
    let ready: Vec<String> = snapshot
        .sources
        .iter()
        .filter(|entry| entry.is_ready())
        .map(|entry| entry.alias().to_string())
        .collect();
    let sources = (!ready.is_empty()).then(|| ready.join(", "));
    let db_type = snapshot
        .primary
        .as_deref()
        .and_then(|primary| plan.sources.iter().find(|source| source.alias == primary))
        .map(|source| source.kind.label().to_string());

    let entry = match outcome {
        Ok(result) => SqlHistoryEntry {
            conn_id: Some(plan.owner.clone()),
            db_type,
            elapsed_ms,
            success: true,
            error_message: None,
            rows_returned: Some(result.total_rows() as u64),
            rows_affected: None,
            channel: Some(ExecChannel::Federated.code().to_string()),
            sources,
        },
        Err(error) => SqlHistoryEntry {
            conn_id: Some(plan.owner.clone()),
            db_type,
            elapsed_ms,
            success: false,
            error_message: Some(error.to_string()),
            rows_returned: None,
            rows_affected: None,
            channel: Some(ExecChannel::Federated.code().to_string()),
            sources,
        },
    };
    if let Err(error) = history_store::save_sql_history(sql, &entry) {
        tracing::error!(error = %error, "联邦执行未记入历史");
    }
}

/// 结果区那行小字（联邦档专属）：挂了哪些源 + 写法提示 + 没挂上的原因
///
/// “写法提示”是**临时的**：源清单浮层（T1.6）上线后它收进浮层底部，这里只留状态。
fn federation_notice(plan: &FederatedPlan, session: &fed_session::FederatedSession) -> Option<String> {
    let snapshot = session.snapshot();
    let ready: Vec<String> = snapshot
        .sources
        .iter()
        .filter(|entry| entry.is_ready())
        .map(|entry| entry.alias().to_string())
        .collect();

    let mut parts: Vec<String> = Vec::new();
    if !ready.is_empty() {
        parts.push(format!("联邦源 {}", ready.join("、")));
    }
    parts.push("跨源请写 别名.schema.表".to_string());
    parts.extend(plan.notes.iter().cloned());
    for entry in &snapshot.sources {
        if let MountState::Failed(reason) = &entry.state {
            parts.push(format!("源 {} 未挂上：{reason}", entry.alias()));
        }
    }
    if let Some(note) = &snapshot.primary_note {
        parts.push(note.clone());
    }
    Some(parts.join(" · "))
}

/// 联邦档的失败原因：点得到源就点名源（D6：错误归属）
///
/// 典型场景：用户写了 `mysql_src.…` 而这个源根本没挂上，DuckDB 只会说
/// “Catalog \"mysql_src\" does not exist”；把挂载失败的原因缀在后面，用户才知道去哪儿修。
///
/// 只认两种提法（引号里的 catalog 名 / `别名.` 限定引用）：纯子串匹配会在 `odbc` 里
/// 匹到别名 `db`，把一句无关的话缀在错误后面——错误归属宁可少说也不能说错。
fn explain_federation_error(
    session: &fed_session::FederatedSession,
    error: String,
) -> String {
    let snapshot = session.snapshot();
    for entry in &snapshot.sources {
        if let MountState::Failed(reason) = &entry.state {
            let quoted = format!("\"{}\"", entry.alias());
            let qualified = format!("{}.", entry.alias());
            if error.contains(&quoted) || error.contains(&qualified) {
                return format!("{error}（源 {} 未挂上：{reason}）", entry.alias());
            }
        }
    }
    error
}

/// 把执行器接到编辑器共享状态上（**启动装配调用一次**）
///
/// 建 runtime 失败时明确说一句：编辑器会以"未接入执行"运行——执行动作会给出
/// 可见的原因，而不是静默什么都不做。
pub fn attach(shared: &EditorShared) {
    match EngineQueryRunner::new() {
        Some(runner) => shared.attach_runner(Arc::new(runner)),
        None => eprintln!("[editor] 执行器未能建立 tokio runtime，编辑器将以“未接入执行”运行"),
    }
}

/// 直接建一个执行器（**给集成测试用**）
///
/// 为什么留这个口子：联邦 / 加速的**源清单组装**发生在执行器里（读连接库、解口令、拼别名），
/// 那是窗口层之下的真逻辑；走窗口跑一遍代价太大，而不验又等于放着不测。
/// 返回 `None` = runtime 建不起来（与 [`attach`] 同口径）。
pub fn runner_for_test() -> Option<Arc<dyn QueryRunner>> {
    EngineQueryRunner::new().map(|runner| Arc::new(runner) as Arc<dyn QueryRunner>)
}

/// 引擎结果 → 编辑器结果（列 + 字符串化行；不读 `rows` 字段）
fn to_data(result: &QueryResult, elapsed_ms: u64, truncated: bool) -> QueryData {
    QueryData {
        columns: column_names(result),
        rows: result
            .to_rows()
            .iter()
            .map(|row| row.iter().map(cell_text).collect())
            .collect(),
        elapsed_ms,
        truncated,
        // B5：写语句的影响行数由驱动报（拿不到就是 `None`，不编造 0）
        affected_rows: result.affected_rows,
        // 非分段路径没有“下一段”可言；分段抓取由调用方按“拿没拿满”标
        has_more: false,
        // 【B14】下发筛选的“已去掉 LIMIT”之类提示由调用方另设
        notice: None,
    }
}

/// 列名：优先驱动填的 `columns`，否则从 Arrow schema 取
///
/// 驱动并不保证填 `columns`（架构 §12 #22），所以这里要有兜底，否则网格会出现无名列。
fn column_names(result: &QueryResult) -> Vec<String> {
    if !result.columns.is_empty() {
        return result.columns.clone();
    }
    result
        .batches
        .first()
        .map(|batch| {
            batch
                .schema()
                .fields()
                .iter()
                .map(|field| field.name().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// 单元格文案
///
/// 直接走 `Value` 的 `Display`（`NULL` / 数字 / 文本都已定义好）。将来若要按类型区分
/// “NULL”与字符串 “NULL”（1b 的类型化网格），改这里一处并补测试。
fn cell_text(value: &Value) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{FedSourceRecord, cell_text, column_names, plan_from_records};
    use engine::duckdb::accel::AccelKind;
    use shared::models::{QueryResult, Value};

    /// 一条源记录（宿主组装好的形状：URL 已带凭据）
    fn record(conn_id: &str, name: &str, db_type: &str, url: &str) -> FedSourceRecord {
        FedSourceRecord {
            conn_id: conn_id.to_string(),
            name: name.to_string(),
            db_type: db_type.to_string(),
            url: url.to_string(),
        }
    }

    #[test]
    fn driver_columns_win_over_the_schema() {
        let mut result = QueryResult::empty();
        result.columns = vec!["n".to_string(), "m".to_string()];
        assert_eq!(column_names(&result), ["n".to_string(), "m".to_string()]);
    }

    #[test]
    fn a_result_without_columns_or_batches_has_no_names() {
        let result = QueryResult::empty();
        assert!(column_names(&result).is_empty());
    }

    #[test]
    fn cells_show_the_value_not_a_debug_dump() {
        assert_eq!(cell_text(&Value::Null), "NULL");
        assert_eq!(cell_text(&Value::Int(42)), "42");
        assert_eq!(cell_text(&Value::Float(1.5)), "1.5");
        assert_eq!(cell_text(&Value::Bool(true)), "true");
        assert_eq!(cell_text(&Value::Text("中文".to_string())), "中文");
    }

    /// 组装源清单：别名稳定、凭据随连接串带上、主源是本文档的连接
    #[test]
    fn the_federated_plan_names_sources_and_points_at_the_primary() {
        // 故意把顺序打乱：组装结果不该跟着读表的顺序漂
        let records = vec![
            record("G_2", "仓库", "postgres", "postgres://u:real@h:5432/w"),
            record("G_1", "订单库", "mysql_native", "mysql://root:real@h:3306/shop"),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");

        assert_eq!(plan.owner, "G_1");
        assert_eq!(plan.sources.len(), 2);
        // 中文连接名整串变成 `db`，第二个重名就排到 `db_2`（顺序按连接 id）
        assert_eq!(plan.sources[0].alias, "db", "G_1 先（按 id 排序）");
        assert_eq!(plan.sources[1].alias, "db_2");
        assert_eq!(plan.primary.as_deref(), Some("db"), "主源 = 本文档的连接");
        assert!(plan.notes.is_empty(), "没出问题就别制造说明：{:?}", plan.notes);
        // 凭据随连接串进 `ATTACH`（DuckDB 扫描器不认 Secret，见 accel::AccelSource::new）
        assert_eq!(plan.sources[0].connection_string, "mysql://root:real@h:3306/shop");
        assert_eq!(plan.sources[1].connection_string, "postgres://u:real@h:5432/w");
        assert_eq!(plan.sources[0].kind, AccelKind::MySql);
        assert_eq!(plan.sources[1].kind, AccelKind::PostgreSql);
    }

    /// 同一个连接在两张表里都出现（记录 + 运行态）：只算一次
    #[test]
    fn a_source_that_shows_up_twice_counts_once() {
        let records = vec![
            record("G_1", "订单库", "mysql_native", "mysql://root:real@h:3306/shop"),
            record("G_1", "订单库", "mysql_native", "mysql://root:real@h:3306/shop"),
            record("G_2", "仓库", "postgres", "postgres://u:real@h:5432/w"),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");
        assert_eq!(plan.sources.len(), 2, "重复的 conn_id 不该占两个源");
    }

    /// 驱动不支持的源 / 组装时攒下的实话：都留在 notes 里，不静默
    #[test]
    fn the_plan_says_what_it_left_out() {
        // 本文档的连接没开开关（不在记录里）：源清单里只有别人，说明里点名
        let records = vec![
            record("G_2", "仓库", "postgres", "postgres://h:5432/w"),
            record("G_3", "分析库", "duckdb", "D:/data/a.duckdb"),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");
        assert_eq!(plan.primary, None, "主源不是它，只能由引擎选第一个可用源");
        assert!(
            plan.notes.iter().any(|note| note.contains("本文档的连接")),
            "要说清本文档的连接为什么没参与：{:?}",
            plan.notes
        );

        // 认不出的驱动（T3.2 之前的 Oracle 就落在这里）：不因为一个连接不可用就整份失败
        let records = vec![
            record("G_1", "订单库", "mysql_native", "mysql://h:3306/a"),
            record("G_2", "仓库", "postgres", "postgres://h:5432/w"),
            record("G_3", "老库", "oracle", "oracle://h:1521/XEPDB1"),
        ];
        let plan = plan_from_records(&records, &[], "G_1").expect("组装");
        assert_eq!(plan.sources.len(), 2, "驱动不支持的连接不占位");
        assert!(
            plan.notes.iter().any(|note| note.contains("老库")),
            "要说清哪个连接没参与：{:?}",
            plan.notes
        );

        // 组装时已经攒下的实话（读连接库失败 / 连接串组不出来）要带到结果区
        let carried = vec!["连接 甲 的连接串没能组装出来：解密失败".to_string()];
        let records = vec![
            record("G_1", "订单库", "mysql_native", "mysql://h:3306/a"),
            record("G_2", "仓库", "postgres", "postgres://h:5432/w"),
        ];
        let plan = plan_from_records(&records, &carried, "G_1").expect("组装");
        assert!(
            plan.notes.iter().any(|note| note.contains("解密失败")),
            "攒下的说明不该丢：{:?}",
            plan.notes
        );
    }

    /// 不够两个源 / 一个都没有：入口就把话说清楚（与门控同一口径）
    #[test]
    fn the_plan_refuses_before_the_session_is_built() {
        let one = vec![record("G_1", "订单库", "mysql_native", "mysql://h:3306/a")];
        let error = plan_from_records(&one, &[], "G_1").expect_err("一个源该拒");
        assert!(error.contains("至少需要两个源"), "{error}");

        let error = plan_from_records(&[], &[], "G_1").expect_err("没有源该拒");
        assert!(error.contains("本地加速"), "{error}");
    }
}
