//! QueryRunner 端口实现（trait impl 同一类型只能有一个块，故整块留在此文件）。

use super::*;
use super::convert::*;

impl QueryRunner for EngineQueryRunner {
    fn run(
        &self,
        connection: Option<&str>,
        schema: Option<&str>,
        channel: ExecChannel,
        sql: &str,
        run_options: crate::execution::RunOptions,
    ) -> Result<QueryData, String> {
        let sql_preview: String = sql.chars().take(80).collect();
        tracing::info!(
            target = "rds_workbench::exec",
            channel = ?channel,
            connection = ?connection,
            schema = ?schema,
            use_transaction = run_options.use_transaction,
            sql = sql_preview,
            "执行器：接收一条语句"
        );
        // 【B13】本地跑的两档不走源库驱动：这条语句在 DuckDB 上跑（源库只读挂载）。
        // 这两档**不接 schema 上下文**：schema 面包屑是**源库**的概念，DuckDB 侧挂载的
        // 源表名通常已限定（`alias.table`）；等加速/联邦做自己的 schema 语义再接。
        // 【B5 余项】血缘跟着走（老调用方没给就按「原查询」，不编别的）
        let lineage = run_options
            .lineage
            .unwrap_or(crate::execution::LINEAGE_ORIGINAL);
        if channel == ExecChannel::Federated {
            return self.run_on_federation(connection, sql, lineage);
        }
        if channel != ExecChannel::Source {
            return self.run_on_accel(connection, sql, lineage);
        }
        let started = std::time::Instant::now();
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
            // 【T2c-1】文档绑定的 schema：引擎在发语句前在同一连接上应用会话上下文
            schema: schema.map(str::to_string),
            // 【B5 余项】血缘随执行进历史（队列从 `ExecTarget::lineage()` 注入）
            lineage: run_options.lineage.map(str::to_string),
            ..Default::default()
        };
        // 连接：文档绑定了就用它（B1）；未绑定回退到“当前活动连接”（1a 口径）
        //
        // 【B5b】查询走 `execute_first_segment`：只取第一段（1000 行），后续段由「取下一段」来要；
        // 写语句没有分段可言，引擎内部会走原路（历史记的都是**用户执行的原 SQL**）。
        let executed = self.runtime.block_on(self.service.execute_first_segment(
            connection.map(str::to_string),
            sql,
            crate::execution::SEGMENT_ROWS,
            options,
        ));
        // 【B8】不管成败，引擎都记了一条历史（含耗时 / 行数 / 失败原因）——
        // 摇一下版本号：右 Dock 的历史面板据此重新加载（面板不读盘，也不每帧问文件）
        crate::history::bump();
        match executed {
            Ok(executed) => {
                let elapsed_ms = executed.elapsed_ms;
                let mut data = to_data(&executed.result, elapsed_ms, executed.truncated);
                // 【B5b】首段拿满了就标“可能还有下一段”（写语句没有分段可言）：
                // 拿不满 = 这一份结果就这么多，界面不摆「取下一段」。
                data.has_more =
                    !data.columns.is_empty() && data.rows.len() == crate::execution::SEGMENT_ROWS;
                tracing::info!(
                    target = "rds_workbench::exec",
                    channel = ?channel,
                    connection = ?connection,
                    schema = ?schema,
                    rows = data.rows.len(),
                    cols = data.columns.len(),
                    elapsed_ms,
                    "源库执行成功"
                );
                Ok(data)
            }
            Err(error) => {
                tracing::error!(
                    target = "rds_workbench::exec",
                    channel = ?channel,
                    connection = ?connection,
                    schema = ?schema,
                    sql = sql_preview,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    error = %error,
                    "源库执行失败"
                );
                Err(error.to_string())
            }
        }
    }

    /// 【B14】下发源库：把筛选词拼成 `WHERE` 重查，结果落**新结果集**
    ///
    /// 方言差异（CAST 目标类型）与 LIMIT 处理都在执行器里（它才知道连接是什么库），
    /// 这里只把原料给过去。
    fn run_filtered(
        &self,
        connection: Option<&str>,
        schema: Option<&str>,
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
            return self.run_rewritten_on_federation(
                connection,
                &rewritten,
                |limit| format!("已去掉原查询的 {limit}（下发筛选要能查到全部行）"),
                crate::execution::LINEAGE_FILTERED,
            );
        }
        if channel != ExecChannel::Source {
            return self.run_rewritten_on_accel(
                connection,
                &rewritten,
                |limit| format!("已去掉原查询的 {limit}（下发筛选要能查到全部行）"),
                crate::execution::LINEAGE_FILTERED,
            );
        }
        let timeout_ms = self.runtime.block_on(self.query_timeout_ms(connection));
        let options = SqlExecuteOptions {
            // 下发是一次真查询：同样进历史（用户能回头找）
            record_history: true,
            timeout_ms,
            channel: Some(channel.code().to_string()),
            // 【T2c-1】下发筛选在原查询的 schema 上下文里重查（不能漂回连接默认）
            schema: schema.map(str::to_string),
            // 【B5 余项】下发筛选的血缘就是动作本身（与 ExecTarget::Filtered 同源）
            lineage: Some(crate::execution::LINEAGE_FILTERED.to_string()),
            ..Default::default()
        };
        let executed = self.runtime.block_on(self.service.execute(
            connection.map(str::to_string),
            &rewritten.sql,
            options,
        ));
        crate::history::bump();
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
        schema: Option<&str>,
        channel: ExecChannel,
        sql: &str,
        column: &str,
        descending: bool,
    ) -> Result<QueryData, String> {
        let rewritten = engine::sql::rewrite_with_order(sql, column, descending)?;
        // 【B13】本地跑的两档：同上——改写服从“在哪儿跑”
        if channel == ExecChannel::Federated {
            return self.run_rewritten_on_federation(
                connection,
                &rewritten,
                |limit| format!("已去掉原查询的 {limit}（排序下发要能排全部行）"),
                crate::execution::LINEAGE_SORTED_DOWN,
            );
        }
        if channel != ExecChannel::Source {
            return self.run_rewritten_on_accel(
                connection,
                &rewritten,
                |limit| format!("已去掉原查询的 {limit}（排序下发要能排全部行）"),
                crate::execution::LINEAGE_SORTED_DOWN,
            );
        }
        let timeout_ms = self.runtime.block_on(self.query_timeout_ms(connection));
        let options = SqlExecuteOptions {
            record_history: true,
            timeout_ms,
            channel: Some(channel.code().to_string()),
            // 【T2c-1】排序下发同筛选：在原查询的 schema 上下文里重查
            schema: schema.map(str::to_string),
            // 【B5 余项】排序下发的血缘就是动作本身（与 ExecTarget::SortedDown 同源）
            lineage: Some(crate::execution::LINEAGE_SORTED_DOWN.to_string()),
            ..Default::default()
        };
        let executed = self.runtime.block_on(self.service.execute(
            connection.map(str::to_string),
            &rewritten.sql,
            options,
        ));
        crate::history::bump();
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
        schema: Option<&str>,
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
                schema.map(str::to_string),
            ))
            .map_err(|error| error.to_string())?;
        let mut data = to_data(&executed.result, executed.elapsed_ms, executed.truncated);
        // “还有没有下一段”由**拿没拿满**判（引擎不问总数）
        data.has_more = data.rows.len() == limit;
        Ok(data)
    }

    /// 事务状态：读引擎的**真值**（活动事务挂在连接管理器上，不是猜的）
    fn transaction_snapshot(&self, connection: Option<&str>) -> crate::execution::TxSnapshot {
        let status = self.runtime.block_on(
            self.service
                .get_transaction_status(connection.map(str::to_string)),
        );
        match status {
            Ok(status) => crate::execution::TxSnapshot {
                in_transaction: status.is_in_transaction,
            },
            // 读不到就当作“不在事务里”：界面会因此少一个 TX 标记，不会凭空多一个
            Err(_) => crate::execution::TxSnapshot::default(),
        }
    }

    /// 事务动作：走引擎的驱动级事务接口（不是拼 `BEGIN` 文本——MySQL 会报 1295）
    fn transaction(
        &self,
        connection: Option<&str>,
        action: crate::execution::TxAction,
    ) -> Result<(), String> {
        use crate::execution::TxAction;
        let conn = connection.map(str::to_string);
        let result = match action {
            TxAction::Begin => self.runtime.block_on(self.service.begin_transaction(conn)),
            TxAction::Commit => self.runtime.block_on(self.service.commit_transaction(conn)),
            TxAction::Rollback => self
                .runtime
                .block_on(self.service.rollback_transaction(conn)),
        };
        result.map(|_status| ()).map_err(|error| error.to_string())
    }

    /// 【T2a】批量提交：把编辑器生成的 UPDATE 文本逐条跑在**同一个事务**里
    ///
    /// 口径（与 `QueryRunner::commit_changes` 的文档一致，这里定实现细节）：
    ///
    /// - **通道闸**：写回只走源库通道——加速 / 联邦是**只读挂载**（DuckDB 侧会拒作用源对象的
    ///   写），在这里提前说清，而不是让用户等一个"看着跑完、实际没写进去"的结果。
    /// - **独立事务**：提交 = 自己的原子单元。连接**已在一个事务里**（B4 手动事务开着）就回绝——
    ///   混进用户事务会让"已提交 N 条"说不清（它可能随用户的下一次回滚一起没了）。
    ///   提交自己负责 `BEGIN` → 逐条 → `COMMIT`；任一条失败 → `ROLLBACK` 并如实报错。
    /// - **影响行数**：每条 UPDATE 的 `affected_rows` 按语句顺序收集进
    ///   [`CommitReport::per_statement`]（**执行器只报数**——它不知道每条对应结果集哪一行哪一列，
    ///   那个映射在编辑器生成时才有，面板组装回执用）。`committed` = 影响行数 ≥ 1 的条数；
    ///   驱动没报行数（异常，UPDATE 一般都会报）按 0 记——宁可标冲突让用户回看，不假装成功。
    /// - **历史**：逐条 UPDATE 都进历史（引擎 `execute` 的 `record_history: true`；耗时/行数是真值）。
    fn commit_changes(
        &self,
        connection: Option<&str>,
        schema: Option<&str>,
        channel: ExecChannel,
        _table: &str,
        _key: &crate::export::RowKey,
        updates: &str,
    ) -> Result<crate::execution::CommitReport, String> {
        // ① 通道闸：写回只走源库通道
        if channel != ExecChannel::Source {
            return Err(format!(
                "提交只支持源库通道（当前是 {}）：加速 / 联邦是只读挂载，改不了源库",
                channel.code()
            ));
        }

        // ② 解析连接（绑定优先 → 当前活动连接；与 run 同一口径）
        let conn_id = self
            .resolve_conn_id(connection)
            .ok_or_else(|| "提交需要一个连接：先绑定一个，或在导航里选中它".to_string())?;

        // ③ 逐条 UPDATE（空行分隔；去掉空白行——生成器以空行拼接）
        let statements: Vec<&str> = updates
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if statements.is_empty() {
            return Err("没有可提交的 UPDATE".to_string());
        }

        // ④ 独立事务：已有事务（B4 手动开着）→ 回绝（见方法文档）
        let manager = engine::connection_manager::get_connection_manager();
        if self.runtime.block_on(manager.has_transaction(&conn_id)) {
            return Err(
                "当前连接已在事务中：先提交 / 回滚它，再批量提交（提交要自己的原子事务）"
                    .to_string(),
            );
        }

        // ⑤ 单事务跑：全部成功 COMMIT，任一条失败 ROLLBACK 并整体报错
        let timeout_ms = self.runtime.block_on(self.query_timeout_ms(Some(&conn_id)));
        let affected = self.runtime.block_on(async {
            let conn = Some(conn_id.clone());
            self.service
                .begin_transaction(conn.clone())
                .await
                .map_err(|error| error.to_string())?;
            let mut per_statement = Vec::with_capacity(statements.len());
            for statement in &statements {
                let options = SqlExecuteOptions {
                    // 提交语句进历史（真值：耗时 / 影响行数由驱动报）
                    record_history: true,
                    timeout_ms,
                    // 已有事务（刚开的这个）→ 引擎走事务对象；不会另开一个
                    use_transaction: false,
                    channel: Some(channel.code().to_string()),
                    // 【T2c-1】提交的 UPDATE 表名未限定（`changes::commit_updates_rows`），
                    // schema 上下文必须带上，否则落到连接默认 schema
                    schema: schema.map(str::to_string),
                    ..Default::default()
                };
                match self.service.execute(conn.clone(), statement, options).await {
                    Ok(outcome) => {
                        // 影响行数由驱动报（B5 口径）；UPDATE 报 0 = 冲突（旧值已变）；
                        // 驱动没报（异常，UPDATE 一般都会报）按 0 记——不假装成功
                        per_statement.push(outcome.result.affected_rows.unwrap_or(0) as usize);
                    }
                    Err(error) => {
                        // 一条 UPDATE 语法错 / 超时：整批回滚（原子性），原因如实上报
                        let reason = error.to_string();
                        self.service
                            .rollback_transaction(conn.clone())
                            .await
                            .map_err(|error| error.to_string())?;
                        return Err(format!("批量提交失败（已回滚）：{reason}"));
                    }
                }
            }
            self.service
                .commit_transaction(conn.clone())
                .await
                .map_err(|error| error.to_string())?;
            Ok(per_statement)
        })?;

        let committed = affected.iter().filter(|&&n| n >= 1).count();
        // 冲突的行/列映射在编辑器侧组装（面板有生成时的行/列对应），这里只报数
        Ok(crate::execution::CommitReport {
            committed,
            skipped: 0,
            per_statement: affected,
            conflicts: Vec::new(),
        })
    }

    /// 事务闸门：读**该连接实际的运行时能力位**（`DataSourceMeta::supports_transaction`），
    /// 不再恒 `true`——能力 Tab 的声明与运行时位必须是同一份事实
    /// （键 → 位的对应见 `engine::driver::capability`；两边不漂移由引擎侧单测钉住）。
    ///
    /// 连接未解析 / 取不到 meta 时保守返回 `true`：保持历史行为，不因读不到而误伤事务 UI。
    fn supports_transactions(&self) -> bool {
        let Some(conn_id) = self.resolve_conn_id(None) else {
            return true;
        };
        let manager = engine::connection_manager::get_connection_manager();
        self.runtime
            .block_on(async { manager.get_connection(&conn_id).await })
            .map(|db| db.meta().supports_transaction)
            .unwrap_or(true)
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

    /// 【B15】本地分析：在内存 DuckDB 上对当前结果集跑一条聚合 SQL（**不碰源库**）。
    ///
    /// 引擎侧临时表由 `DuckDBManager` 登记（上限 / TTL / 关项目清场都走那一套）；
    /// `{table}` 占位符由引擎替换成真正的临时表名。
    ///
    /// 【B4】三分支选源（方案 §7-B②）：
    /// 1. **表档复用**：事实档已挂常驻表名 → 直接复用，不重复建表；
    /// 2. **内存档直灌**：事实档在 Rust 堆 → 按偏好 + 双份估算分流（`storage_choice`），
    ///    batches 经 Arrow Appender 直灌新建表；目标表档（`OnDuckDb`）时回带释放信号，
    ///    host 据此把原槽位提升表档并释放 batches；
    /// 3. **显示矩阵猜型降级档**：无事实档 / batches 已释放 / 空批 → 走逐行 internal 建表，
    ///    整列落 `VARCHAR`（`infer_type` 只认 JSON 值类型、不 parse 文本），数字列被收窄成
    ///    文本；notice 显式标注降级档。
    ///
    /// 分析产物本身只回列名 + JSON 行、没有原始 batches，所以 `fact: None`——后续对分析
    /// 结果再做分析 / 带类型导出仍走显示矩阵降级档（链式产物新 FactFrame 依赖引擎
    /// `query_duckdb` 回 Arrow，属后置增强项）。
    fn analyze(&self, request: &AnalysisRequest) -> Result<QueryData, String> {
        let mut source_fact_table: Option<String> = None;
        let mut source_fact_release = false;
        let mut degraded = false;

        let result = match &request.fact {
            // ①② 有事实档且 batches 未释放、非空批
            Some(frame) if !frame.is_released() && !frame.batches().is_empty() => {
                if let Some(table) = frame.temp_table() {
                    // ① 表档常驻表复用：不重复建表，表名原样回挂（无需新信号）
                    source_fact_table = Some(table.to_string());
                    crate::ports::result_service::ResultService::execute_duckdb_analysis_from_batches(
                        Some(table),
                        &request.sql,
                        frame.batches(),
                    )
                    .map_err(|error| error.to_string())?
                } else {
                    // ② 内存档：按偏好 + 双份估算分流后直灌新建表
                    let dual = dual_estimate(frame.estimated_bytes());
                    let tier = storage_choice(dual, request.pref, CurrentTier::InMemory);
                    source_fact_release = tier == FactTier::OnDuckDb;
                    let res =
                        crate::ports::result_service::ResultService::execute_duckdb_analysis_from_batches(
                            None,
                            &request.sql,
                            frame.batches(),
                        )
                        .map_err(|error| error.to_string())?;
                    source_fact_table = Some(res.temp_table.clone());
                    res
                }
            }
            // ③ 无事实档 / 已释放 / 空批：显示矩阵猜型降级档
            _ => {
                degraded = true;
                let rows: Vec<Vec<serde_json::Value>> = request
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|cell| cell_json(cell)).collect())
                    .collect();
                crate::ports::result_service::ResultService::execute_duckdb_analysis(
                    "",
                    &request.sql,
                    Some(request.columns.clone()),
                    Some(rows),
                )
                .map_err(|error| error.to_string())?
            }
        };

        // 降级档在原有桥接说明后追加一句口径标注（不掩盖“基于已抓取 N 行”主文案）；
        // `notice()` 总是返回拼好的文案（不是 Option），降级档只做字符串追加。
        let notice = if degraded {
            let mark = "文本猜型降级档：整列按文本存储，数字类型可能失真";
            Some(format!("{}；{mark}", request.notice()))
        } else {
            Some(request.notice())
        };

        Ok(QueryData {
            columns: result.columns,
            // 【Q6】本地分析走内存 DuckDB：ResultSet 不带列类型（引擎侧 query_duckdb
            // 只回列名+行值），这里按无类型档渲染；驱动直连路径的类型上报见 P0 记录
            column_types: Vec::new(),
            rows: result
                .rows
                .iter()
                .map(|row| row.iter().map(json_cell_text).collect())
                .collect(),
            elapsed_ms: result.elapsed_ms as u64,
            truncated: false,
            affected_rows: None,
            // 分析结果没有“下一段”可言（它一次算完）
            has_more: false,
            notice,
            // 【A3】本地 DuckDB 分析产物只回列名 + JSON 行，没有原始 batches → 事实档 `None`。
            fact: None,
            // 【B4】物化回执：host 回填原槽位挂表名；释放信号仅目标表档时置真
            source_fact_table,
            source_fact_release,
            source_fact_set: request.source_set,
        })
    }

    /// 【2b】通用模式的脚本执行：解析环境命令 → 在专用 runtime 上跑进程
    ///
    /// 编辑器在工作线程上调用（可阻塞）：这里把 env_id 翻译成命令（`editor_env::resolve`），
    /// 再交给 `run_script_process`（stdin 写代码 + 并发收输出 + 超时杀进程树）。
    /// 超时用**请求自带的** `timeout_ms`（宿主按设置读出后传入）——执行线程不读设置。
    /// 环境未注册（T2b-3 管理环境落地前只有内置的 python / Rscript / rustc）如实拒绝。
    fn run_script(
        &self,
        request: &crate::env::ScriptRequest,
    ) -> Result<crate::env::ScriptOutcome, String> {
        let env = crate::ports::editor_env::resolve(&request.env_id)?;
        self.runtime
            .block_on(crate::ports::editor_env::run_script_process(
                &env.command,
                &env.args,
                &request.code,
                request.timeout_ms,
            ))
    }

    /// 【D 批待改造】经 DuckDB 落盘（Parquet / XLSX）
    ///
    /// 现状走显示矩阵：`"NULL"` → 真 null，其余经 `cell_json` 一律包成字符串，internal
    /// 【D1】Parquet / XLSX 经 DuckDB `COPY` 落盘，按事实档状态三分支分流（方案 §7-D）：
    ///
    /// - 内存档（batches 未释放、非空批、未挂表名）：batches 直灌新建临时表 → `COPY` →
    ///   临时表成败都收、即用即收；列类型来自 Arrow schema，不猜型；
    /// - 表档（已挂常驻表名，含分析过回挂的内存档）：复用常驻表 `COPY`，不新建、不回收；
    /// - 无事实档 / 已释放且无表名 / 空批：显示矩阵 internal 猜型降级档，整列落 `VARCHAR`，
    ///   回执由提交侧标注降级口径。
    ///
    /// 同步阻塞（XLSX 首次装扩展要联网几秒）：调用方把它放在一次性线程上。
    fn export_via_duckdb(&self, request: &DuckDbExportRequest) -> Result<usize, String> {
        let format = match request.format {
            ExportFormat::Parquet => DuckDbExportFormat::Parquet,
            ExportFormat::Xlsx => DuckDbExportFormat::Xlsx,
            other => {
                return Err(format!(
                    "{} 不走 DuckDB 导出（它在编辑侧编码）",
                    other.label()
                ));
            }
        };

        let fact = request.fact.as_deref();
        let mode = fact_export_mode(fact);
        let stats = match mode {
            FactExportMode::Batches => {
                let frame = fact.expect("Batches 分支必有事实档");
                crate::ports::result_service::ResultService::export_batches_via_duckdb(
                    frame.batches(),
                    &request.path,
                    format,
                )
            }
            FactExportMode::ExistingTable => {
                let frame = fact.expect("ExistingTable 分支必有事实档");
                let table = frame.temp_table().expect("表档必有常驻表名");
                crate::ports::result_service::ResultService::export_existing_table_via_duckdb(
                    table,
                    frame.row_count(),
                    &request.path,
                    format,
                )
            }
            FactExportMode::Matrix => {
                let rows: Vec<Vec<serde_json::Value>> = request
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|cell| cell_json(cell)).collect())
                    .collect();
                crate::ports::result_service::ResultService::export_rows_via_duckdb(
                    &request.columns,
                    &rows,
                    &request.path,
                    format,
                )
            }
        }
        .map_err(|error| error.to_string())?;

        tracing::info!(
            format = request.format.label(),
            mode = ?mode,
            rows = stats.rows,
            path = %request.path.display(),
            "结果集已导出（经 DuckDB COPY）"
        );
        Ok(stats.rows)
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
        let owner = self
            .resolve_conn_id(connection)
            .ok_or_else(|| "联邦源需要一个连接：先绑定一个，或在导航里选中它".to_string())?;
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
                    Some(MountState::Failed(reason)) => {
                        Err(format!("源 {alias} 仍挂不上：{reason}"))
                    }
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
    fn set_federated_primary(
        &self,
        connection: Option<&str>,
        alias: &str,
    ) -> Result<String, String> {
        let owner = self
            .resolve_conn_id(connection)
            .ok_or_else(|| "联邦源需要一个连接：先绑定一个，或在导航里选中它".to_string())?;
        fed_session::set_primary(&owner, alias)?;
        Ok(format!("主源已切到 {alias}（未限定名的解析者）"))
    }
}
