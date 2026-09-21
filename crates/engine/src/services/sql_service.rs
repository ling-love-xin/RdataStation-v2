use std::sync::Arc;

use crate::cache::get_query_cache;
use crate::connection_manager::ConnectionManager;
use crate::driver::traits::DynDatabase;
use crate::persistence::history_store::{self, SqlHistoryEntry};
use crate::sql::SqlEngine;
use crate::sql::{SqlDialect, SqlStatementType};
use shared::error::{CoreError, DatabaseError};
use shared::models::QueryResult;

/// 事务状态结果
#[derive(Debug)]
pub struct TransactionStatusResult {
    pub conn_id: String,
    pub is_in_transaction: bool,
    pub transaction_start_time_ms: Option<u64>,
    pub transaction_duration_ms: Option<u64>,
}

/// SQL 执行服务
///
/// 负责 SQL 查询的执行和管理，包括：
/// - 执行查询
/// - 执行事务
/// - 取消查询
/// - SQL 历史记录
pub struct SqlService {
    manager: Arc<ConnectionManager>,
}

/// SQL 执行结果
#[derive(Debug)]
pub struct SqlExecuteResult {
    /// 查询结果
    pub result: QueryResult,
    /// 执行耗时（毫秒）
    pub elapsed_ms: u64,
    /// 结果是否被截断（超出最大行数限制）
    pub truncated: bool,
}

/// 单次查询最大返回行数
pub const MAX_QUERY_ROWS: usize = 10_000;

/// 默认连接键名（无显式连接 ID 时的回退值）
const DEFAULT_CONN_KEY: &str = "active";

/// SQL 执行选项
#[derive(Debug, Default)]
pub struct SqlExecuteOptions {
    /// 是否记录到历史
    pub record_history: bool,
    /// 是否使用事务
    pub use_transaction: bool,
    /// 查询超时时间（毫秒）
    pub timeout_ms: Option<u64>,
    /// 是否使用查询缓存
    pub use_cache: bool,
    /// 【B13】执行通道（`source` / `accelerated` / `federated`）：历史要记“这句在哪儿跑的”
    ///
    /// 由**执行器**填（它是唯一知道通道的一方）；不填 = 没接通道信息（老调用方）。
    pub channel: Option<String>,
}

/// 把一条查询套成**窗口**（`LIMIT n OFFSET m`）——分段抓取的取数方式
///
/// 为什么套子查询而不是让驱动持游标：四个内置驱动的取数路径都是“一次拿全”，持游标要改四份
/// 实现，而且 sqlite / duckdb 的语句借连接的生命周期（自引用）。套一层窗口是**四个方言都认**
/// 的写法（别名 `rds_segment` 是 MySQL 这类要别名的方言要求的），内存被窗口界住。
/// 代价是每段会**重跑一次查询**：`ORDER BY` 不稳的查询可能在两段之间重复或跳过行——
/// 这条取舍要写在界面上，不能假装它和游标等价。
///
/// 不返回行的语句（DML / DDL / 事务控制）没有“分段”可言 → `None`。
///
/// 包装**不对外可见**：`execute_segment` / `execute_first_segment` 会把报错里的位置与回显的
/// SQL 还原成用户的原句（见 [`unwrap_segment_error`]），界面上不会冒出 `rds_segment`。
pub fn window_sql(sql: &str, limit: usize, offset: usize) -> Option<String> {
    window_segment(sql, limit, offset).map(|window| window.sql)
}

/// 窗口包装的产物：套好的 SQL + **原句在它里面占哪一段**
///
/// 包装是内部实现（用户从没写过 `rds_segment`）：驱动报的位置、错误消息里回显的 SQL 都是
/// 相对**发出去的这段文本**的，要还原回用户的原句就得知道原句落在哪（见
/// [`unwrap_segment_error`]）。
struct WindowedSegment {
    /// 真正发给驱动的文本
    sql: String,
    /// 原句裁掉首尾空白与尾分号后，在 `sql` 里的字节区间
    body: std::ops::Range<usize>,
    /// 原句里被裁掉的头部长（`body` 内的偏移加上它就得到原句内的偏移）
    indent: usize,
}

/// 构造函数——[`window_sql`] 与错误还原共用同一套算法（前缀长度必须对上）
fn window_segment(sql: &str, limit: usize, offset: usize) -> Option<WindowedSegment> {
    const PREFIX: &str = "SELECT * FROM (\n";
    let trimmed = sql.trim().trim_end_matches(';').trim_end();
    if trimmed.is_empty()
        || limit == 0
        || !crate::driver::utils::returns_rows(trimmed)
        // 元信息 / 会话控制语句**不能包**（`SELECT * FROM (EXPLAIN …)` 是语法错误，真机踩到）：
        // 它们在 `execute_first_segment` 里走“原路执行”分支，分段自然也就无从谈起
        || !crate::driver::utils::wrappable_in_subquery(trimmed)
    {
        return None;
    }
    let start = PREFIX.len();
    Some(WindowedSegment {
        sql: format!("{PREFIX}{trimmed}\n) AS rds_segment LIMIT {limit} OFFSET {offset}"),
        body: start..start + trimmed.len(),
        indent: sql.len() - sql.trim_start().len(),
    })
}

/// 把「相对窗口 SQL」的错误结论还原成「相对用户原句」的
///
/// 包装不该泄漏到界面上：错误消息里回显的 SQL 换成用户写的那句，位置平移到原句坐标。
/// 位置落在包装部分（`SELECT * FROM (` 或 `) AS rds_segment LIMIT …`）上时**不给位置**——
/// 给个错的位置比不给更糟（用户会照着找，还以为自己的 SQL 没问题）。
fn unwrap_segment_error(
    result: Result<SqlExecuteResult, CoreError>,
    original: &str,
    window: &WindowedSegment,
) -> Result<SqlExecuteResult, CoreError> {
    match result {
        Err(CoreError::Database(DatabaseError::Query {
            reason,
            position,
            location,
            ..
        })) => {
            let position = position.and_then(|pos| {
                let offset_in_body = pos.checked_sub(window.body.start)?;
                (offset_in_body < window.body.len()).then(|| window.indent + offset_in_body)
            });
            Err(CoreError::Database(DatabaseError::Query {
                sql: original.to_string(),
                reason,
                position,
                // 对象位置（表 / 列 / 约束）与「套没套窗口」无关，原样带过去
                location,
            }))
        }
        other => other,
    }
}

impl SqlService {
    /// 创建新的 SQL 服务
    pub fn new(manager: Arc<ConnectionManager>) -> Self {
        Self { manager }
    }

    /// 执行一句：**查询只取第一段**（`limit` 行），写语句走原来的路（B5b）
    ///
    /// 与 [`Self::execute`] 的区别只有一件事：查询会被套成窗口（见 [`window_sql`]）。
    /// 历史记的是**用户执行的原 SQL**（不是套了 `LIMIT` 的变身）——历史面板上出现的
    /// 应当是人写的那句，否则“重放”会变成重放一个带窗口的别名查询。
    ///
    /// 分段抓取的开头一句走这里；后续段走 [`Self::execute_segment`]。
    pub async fn execute_first_segment(
        &self,
        conn_id: Option<String>,
        sql: &str,
        limit: usize,
        options: SqlExecuteOptions,
    ) -> Result<SqlExecuteResult, CoreError> {
        let Some(window) = window_segment(sql, limit, 0) else {
            // 写语句 / 无结果集：没有分段可言，原路执行（历史也由原路记）
            return self.execute(conn_id, sql, options).await;
        };

        let started = std::time::Instant::now();
        let record_history = options.record_history;
        // 通道要先拷出来：下面 `SqlExecuteOptions { ..options }` 会把整个 options 移进内层执行
        let channel = options.channel.clone();
        let result = self
            .execute(
                conn_id.clone(),
                &window.sql,
                SqlExecuteOptions {
                    record_history: false,
                    ..options
                },
            )
            .await;
        // 报错要按**用户写的那句**报（位置与回显的 SQL 都是）
        let result = unwrap_segment_error(result, sql, &window);

        if record_history {
            let conn_key = self
                .conn_key_for(&conn_id)
                .await
                .unwrap_or_else(|_| DEFAULT_CONN_KEY.to_string());
            match &result {
                Ok(outcome) => {
                    self.record_history(
                        conn_id.as_deref(),
                        &conn_key,
                        sql,
                        outcome.elapsed_ms,
                        None,
                        Some(outcome.result.total_rows() as u64),
                        None,
                        channel.as_deref(),
                    )
                    .await;
                }
                Err(error) => {
                    self.record_history(
                        conn_id.as_deref(),
                        &conn_key,
                        sql,
                        started.elapsed().as_millis() as u64,
                        Some(error.to_string()),
                        None,
                        None,
                        channel.as_deref(),
                    )
                    .await;
                }
            }
        }
        result
    }

    /// 写一条执行历史（与 [`Self::execute`] 里那两处同一张表）
    #[allow(clippy::too_many_arguments)]
    async fn record_history(
        &self,
        conn_id: Option<&str>,
        conn_key: &str,
        sql: &str,
        elapsed_ms: u64,
        error_message: Option<String>,
        rows_returned: Option<u64>,
        rows_affected: Option<u64>,
        channel: Option<&str>,
    ) {
        let entry = SqlHistoryEntry {
            conn_id: conn_id.map(str::to_string),
            db_type: self.db_type_of(conn_key).await,
            elapsed_ms,
            success: error_message.is_none(),
            error_message,
            rows_returned,
            rows_affected,
            channel: channel.map(str::to_string),
            // 源库档没有“参与源清单”（那是联邦档的事；联邦不走这条路径）
            sources: None,
        };
        if let Err(e) = history_store::save_sql_history(sql, &entry) {
            tracing::error!(error = %e, "Failed to save SQL history");
        }
    }

    /// 分段抓取的**一段**：把原 SQL 套成窗口（[`window_sql`]）再跑一次
    ///
    /// 每段都走普通执行路径（超时 / 取消 / 事务都一样），只是**不记历史、不用缓存**——
    /// 分段抓取不该把同一句的历史写十几遍，缓存也不该按“带 LIMIT 的变身”去命中。
    ///
    /// 判断“还有没有下一段”由调用方做：一段拿满 `limit` 行就**可能**还有（拿不满就是到底了），
    /// 引擎不去问总数（`COUNT(*)` 对一条重查询是另一笔开销）。
    pub async fn execute_segment(
        &self,
        conn_id: Option<String>,
        sql: &str,
        limit: usize,
        offset: usize,
        timeout_ms: Option<u64>,
    ) -> Result<SqlExecuteResult, CoreError> {
        let Some(window) = window_segment(sql, limit, offset) else {
            return Err(CoreError::database(DatabaseError::Query {
                sql: sql.to_string(),
                reason: "这条语句没有结果集，不能分段抓取".to_string(),
                position: None,
                location: None,
            }));
        };
        let result = self
            .execute(
                conn_id,
                &window.sql,
                SqlExecuteOptions {
                    record_history: false,
                    use_transaction: false,
                    timeout_ms,
                    use_cache: false,
                    // 分段抓取本身不记历史（第一段那条已经记了），通道也无需带
                    channel: None,
                },
            )
            .await;
        // 同上：分段执行也不能把包装掉包给用户看
        unwrap_segment_error(result, sql, &window)
    }

    /// 执行 SQL 查询
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接 ID（使用活动连接如果为 None）
    /// * `sql` - SQL 语句
    /// * `options` - 执行选项
    ///
    /// # Returns
    ///
    /// 返回执行结果
    pub async fn execute(
        &self,
        conn_id: Option<String>,
        sql: &str,
        options: SqlExecuteOptions,
    ) -> Result<SqlExecuteResult, CoreError> {
        let start_time = std::time::Instant::now();

        // 参数校验
        if sql.trim().is_empty() {
            return Err(CoreError::database(DatabaseError::query(
                sql,
                "SQL statement cannot be empty".to_string(),
            )));
        }

        // SQL 语句分类（智能路由）
        let (stmt_type, _normalized) = SqlEngine::parse_and_route(sql, SqlDialect::Ansi);
        let (is_ddl, _is_dml, is_dql) = {
            let is_ddl = matches!(stmt_type, SqlStatementType::Ddl);
            let is_dml = matches!(
                stmt_type,
                SqlStatementType::Insert | SqlStatementType::Update | SqlStatementType::Delete
            );
            let is_dql = matches!(stmt_type, SqlStatementType::Select);
            (is_ddl, is_dml, is_dql)
        };

        // 获取查询缓存
        let query_cache = get_query_cache();

        // 检查缓存（仅当启用缓存、非事务操作、非 DDL/DML 时）
        if options.use_cache && !options.use_transaction && is_dql {
            let connection_id = match conn_id.as_deref() {
                Some(id) => id,
                None => DEFAULT_CONN_KEY,
            };
            if let Some(cached_result) = query_cache.get(connection_id, sql).await {
                let elapsed_ms = start_time.elapsed().as_millis() as u64;

                return Ok(SqlExecuteResult {
                    result: cached_result,
                    elapsed_ms,
                    truncated: false,
                });
            }
        }

        // 获取数据库连接
        let db = self.get_database(conn_id.clone()).await?;

        let conn_key = match conn_id.clone() {
            Some(id) => id,
            None => DEFAULT_CONN_KEY.to_string(),
        };

        // ---- B4：事务路由 ----
        // ① 会话里**已经有事务** → 这条语句必须走事务对象（同一物理连接，架构 §12 #2 的会话亲和）；
        // ② 自动提交关闭（`use_transaction`）且还没事务 → 本次执行先自动开一个。
        // DDL 不自动开事务：MySQL 会隐式提交，那样界面上的“事务已开启”就骗人了。
        //
        // 事务用**真实连接 id**（未绑定时解析成当前活动连接）：拿 “active” 这种虚拟键去开事务，
        // 引擎会报 `Connection 'active' not found`（真机上踩过——缓存/取消令牌可以按虚拟键归档，
        // 事务不行，它要拿 id 去找连接）。
        let tx_key = self.conn_key_for(&conn_id).await?;
        let in_transaction = self.manager.has_transaction(&tx_key).await;
        let auto_begin = options.use_transaction && !is_ddl && !in_transaction;
        if auto_begin {
            self.manager.begin_transaction(&tx_key).await?;
        }

        // 执行查询（支持取消和超时）
        //
        // 这里不用 `?` 提前返回：失败也要写历史（v1 只在成功时记，失败查询在历史里不可见）。
        let query_result = if in_transaction || auto_begin {
            match self.manager.query_in_transaction(&tx_key, sql).await {
                Some(result) => result,
                // 竞态兜底：刚判过还有、这会儿被别处提交/回滚了 → 回落普通路径
                None => run_plain(&self.manager, &db, sql, options.timeout_ms, &conn_key).await,
            }
        } else {
            run_plain(&self.manager, &db, sql, options.timeout_ms, &conn_key).await
        };

        // 失败留痕：v1 只在成功时写历史，失败查询在历史里不可见（排障时无法回溯）
        let mut result = match query_result {
            Ok(result) => result,
            Err(err) => {
                if options.record_history {
                    self.record_history(
                        conn_id.as_deref(),
                        &conn_key,
                        sql,
                        start_time.elapsed().as_millis() as u64,
                        Some(err.to_string()),
                        None,
                        None,
                        options.channel.as_deref(),
                    )
                    .await;
                }
                return Err(err);
            }
        };

        // 应用行数限制，防止内存溢出
        let truncated = result.truncate(MAX_QUERY_ROWS) > 0;

        // 将结果存入缓存（仅当启用缓存、非事务操作、且为 SELECT 查询时）
        if options.use_cache && !options.use_transaction && is_dql {
            let connection_id = match conn_id.as_deref() {
                Some(id) => id,
                None => DEFAULT_CONN_KEY,
            };
            let _ = query_cache
                .set(connection_id, sql, result.clone(), None)
                .await;
        }

        let elapsed_ms = start_time.elapsed().as_millis() as u64;

        // 记录到历史：耗时 / 成功 / 行数均为真实值
        // （行结果集只记 `rows_returned`，作用于源库的写语句只记 `rows_affected`）
        if options.record_history {
            tracing::info!(
                sql = %sql,
                elapsed_ms,
                stmt_type = ?stmt_type,
                "SQL executed"
            );
            self.record_history(
                conn_id.as_deref(),
                &conn_key,
                sql,
                elapsed_ms,
                None,
                if is_dql {
                    // 用方法而**不是字段**：native 驱动只填 Arrow `batches`，
                    // `QueryResult.total_rows` 字段恒为 0（见架构 §12 #21）
                    Some(result.total_rows() as u64)
                } else {
                    None
                },
                // 作用于源库的写语句只记 `rows_affected`
                match result.is_read_only {
                    Some(false) => result.affected_rows.map(u64::from),
                    _ => None,
                },
                options.channel.as_deref(),
            )
            .await;
        }

        Ok(SqlExecuteResult {
            result,
            elapsed_ms,
            truncated,
        })
    }

    /// 在事务中执行多个 SQL
    ///
    /// # Arguments
    ///
    /// * `conn_id` - 连接 ID
    /// * `sqls` - SQL 语句列表
    ///
    /// # Returns
    ///
    /// 返回每个 SQL 的执行结果
    pub async fn execute_in_transaction(
        &self,
        conn_id: Option<String>,
        sqls: Vec<String>,
    ) -> Result<Vec<SqlExecuteResult>, CoreError> {
        if sqls.is_empty() {
            return Ok(Vec::new());
        }

        // 获取数据库连接
        let db = self.get_database(conn_id).await?;

        // 开始事务
        let mut tx = db.begin_transaction().await?;

        let mut results = Vec::with_capacity(sqls.len());
        let mut failed = false;
        let mut error = None;

        for sql in sqls {
            let start_time = std::time::Instant::now();

            match tx.query(&sql).await {
                Ok(result) => {
                    let elapsed_ms = start_time.elapsed().as_millis() as u64;
                    results.push(SqlExecuteResult {
                        result,
                        elapsed_ms,
                        truncated: false,
                    });
                }
                Err(e) => {
                    failed = true;
                    error = Some(e);
                    break;
                }
            }
        }

        if failed {
            let _ = tx.rollback().await;
            return Err(match error {
                Some(e) => e,
                None => CoreError::database(DatabaseError::Query {
                    sql: "unknown".to_string(),
                    reason: "Transaction failed with unknown error".to_string(),
                    position: None,
                    location: None,
                }),
            });
        }

        // 提交事务
        tx.commit().await?;

        Ok(results)
    }

    /// 执行查询（简化版，不带选项）
    pub async fn query(
        &self,
        conn_id: Option<String>,
        sql: &str,
    ) -> Result<QueryResult, CoreError> {
        let result = self
            .execute(
                conn_id,
                sql,
                SqlExecuteOptions {
                    record_history: true,
                    use_cache: true, // 默认启用缓存
                    ..Default::default()
                },
            )
            .await?;
        Ok(result.result)
    }

    /// 获取数据库连接（含自动重连）
    ///
    /// 先从连接管理器获取连接，若连接不存在或 ping 失败则尝试重连。
    async fn get_database(&self, conn_id: Option<String>) -> Result<DynDatabase, CoreError> {
        match conn_id {
            Some(ref id) => self.manager.get_or_reconnect(id).await,
            None => {
                let conn_id = self
                    .manager
                    .get_active_connection_id()
                    .await
                    .ok_or_else(|| {
                        CoreError::connection(shared::error::ConnectionError::NoActiveConnection)
                    })?;
                self.manager.get_or_reconnect(&conn_id).await
            }
        }
    }

    /// 运行时连接的数据库类型（历史记录用；连接信息缺失时返回 `None`）
    async fn db_type_of(&self, conn_key: &str) -> Option<String> {
        self.manager
            .get_connection_info(&conn_key.to_string())
            .await
            .map(|info| info.db_type)
    }

    /// 获取 SQL 执行历史
    pub fn get_sql_history(
        &self,
        limit: usize,
    ) -> Result<Vec<history_store::SqlHistoryRecord>, CoreError> {
        history_store::get_sql_history(limit).map_err(|e| {
            CoreError::storage(shared::error::StorageError::read(
                "sql_history",
                e.to_string(),
            ))
        })
    }

    /// 搜索 SQL 历史
    pub fn search_sql_history(
        &self,
        keyword: &str,
        limit: usize,
    ) -> Result<Vec<history_store::SqlHistoryRecord>, CoreError> {
        history_store::search_sql_history(keyword, limit).map_err(|e| {
            CoreError::storage(shared::error::StorageError::read(
                "sql_history",
                e.to_string(),
            ))
        })
    }

    /// 清空 SQL 历史
    pub fn clear_sql_history(&self) -> Result<(), CoreError> {
        history_store::clear_sql_history().map_err(|e| {
            CoreError::storage(shared::error::StorageError::write(
                "sql_history",
                e.to_string(),
            ))
        })
    }

    /// 删除单条 SQL 历史
    pub fn remove_sql_history(&self, id: &str) -> Result<(), CoreError> {
        history_store::remove_sql_history(id).map_err(|e| {
            CoreError::storage(shared::error::StorageError::write(
                "sql_history",
                e.to_string(),
            ))
        })
    }

    /// 注册外部数据库连接
    pub async fn register_external_database(
        &self,
        conn_id: Option<String>,
        name: &str,
        driver: &str,
        connection_string: &str,
    ) -> Result<(), CoreError> {
        // 获取数据库连接
        let db = self.get_database(conn_id).await?;

        // 检查是否支持联邦查询
        if !db.meta().supports_federated {
            return Err(CoreError::database(DatabaseError::Driver {
                db_type: "generic".to_string(),
                operation: "register_external_database".to_string(),
                source: "Federated queries not supported".to_string(),
            }));
        }

        // 注册外部数据库
        db.register_external_database(name, driver, connection_string)
            .await
    }

    /// 创建外部表
    pub async fn create_external_table(
        &self,
        conn_id: Option<String>,
        external_db_name: &str,
        schema_name: &str,
        table_name: &str,
        external_table_name: &str,
    ) -> Result<(), CoreError> {
        // 获取数据库连接
        let db = self.get_database(conn_id).await?;

        // 检查是否支持联邦查询
        if !db.meta().supports_federated {
            return Err(CoreError::database(DatabaseError::Driver {
                db_type: "generic".to_string(),
                operation: "create_external_table".to_string(),
                source: "Federated queries not supported".to_string(),
            }));
        }

        // 创建外部表
        db.create_external_table(
            external_db_name,
            schema_name,
            table_name,
            external_table_name,
        )
        .await
    }

    /// 开始事务（B4）：走**驱动**的事务接口，事务对象挂到连接管理器上
    ///
    /// 不再是 `db.query("BEGIN TRANSACTION")`：MySQL 的显式 `BEGIN` 会被 prepared 协议拒绍（1295），
    /// 且池里每次取到的未必是同一条物理连接（P0.2c 实测）。
    pub async fn begin_transaction(
        &self,
        conn_id: Option<String>,
    ) -> Result<TransactionStatusResult, CoreError> {
        let conn_key = self.conn_key_for(&conn_id).await?;
        self.manager.begin_transaction(&conn_key).await?;
        Ok(self.transaction_status(&conn_key).await)
    }

    /// 提交事务（返回提交后的状态：已不在事务里）
    pub async fn commit_transaction(
        &self,
        conn_id: Option<String>,
    ) -> Result<TransactionStatusResult, CoreError> {
        let conn_key = self.conn_key_for(&conn_id).await?;
        self.manager.commit_transaction(&conn_key).await?;
        Ok(self.transaction_status(&conn_key).await)
    }

    /// 回滚事务（返回回滚后的状态：已不在事务里）
    pub async fn rollback_transaction(
        &self,
        conn_id: Option<String>,
    ) -> Result<TransactionStatusResult, CoreError> {
        let conn_key = self.conn_key_for(&conn_id).await?;
        self.manager.rollback_transaction(&conn_key).await?;
        Ok(self.transaction_status(&conn_key).await)
    }

    /// 获取事务状态：**读真值**（连接管理器上的活动事务），不再是固定返回“未开启”
    pub async fn get_transaction_status(
        &self,
        conn_id: Option<String>,
    ) -> Result<TransactionStatusResult, CoreError> {
        // 状态查询不该因为「没有连接」而报错：没有连接 = **没有事务**，这就是完整答案。
        // 开事务 / 回滚不一样——它们非要一条连接不可，所以照旧报 NoActiveConnection。
        let key = match self.conn_key_for(&conn_id).await {
            Ok(key) => key,
            Err(_) if conn_id.is_none() => {
                return Ok(TransactionStatusResult {
                    conn_id: DEFAULT_CONN_KEY.to_string(),
                    is_in_transaction: false,
                    transaction_start_time_ms: None,
                    transaction_duration_ms: None,
                });
            }
            Err(error) => return Err(error),
        };
        Ok(self.transaction_status(&key).await)
    }

    /// 事务等接口要的**真实连接 id**：`None` → 当前活动连接
    ///
    /// 不能拿 `DEFAULT_CONN_KEY`（"active"）去开事务：那是个给缓存/取消令牌归档用的虚拟键，
    /// 拿去查连接会得到 `Connection 'active' not found`。
    async fn conn_key_for(&self, conn_id: &Option<String>) -> Result<String, CoreError> {
        match conn_id {
            Some(id) => Ok(id.clone()),
            None => self
                .manager
                .get_active_connection_id()
                .await
                .ok_or_else(|| {
                    CoreError::connection(shared::error::ConnectionError::NoActiveConnection)
                }),
        }
    }

    /// 把一个连接的事务状态拼成结果（时间戳由“已开多久”倒推，界面要的是真实值）
    async fn transaction_status(&self, conn_key: &str) -> TransactionStatusResult {
        let elapsed = self.manager.transaction_elapsed(conn_key).await;
        TransactionStatusResult {
            conn_id: conn_key.to_string(),
            is_in_transaction: elapsed.is_some(),
            transaction_start_time_ms: elapsed
                .map(|elapsed| unix_ms_now().saturating_sub(elapsed.as_millis() as u64)),
            transaction_duration_ms: elapsed.map(|elapsed| elapsed.as_millis() as u64),
        }
    }

    /// 取消指定连接正在执行的查询
    pub async fn cancel_query(&self, conn_id: Option<String>) -> Result<bool, CoreError> {
        let conn_id_str = match conn_id {
            Some(ref id) => id.clone(),
            None => DEFAULT_CONN_KEY.to_string(),
        };
        if conn_id_str.is_empty() {
            return Err(CoreError::connection(
                shared::error::ConnectionError::NoActiveConnection,
            ));
        }
        Ok(self.manager.cancel_query(&conn_id_str).await)
    }
}

/// 查询超时错误（统一构造，执行与历史记录共用）
fn query_timeout_error(sql: &str, timeout_ms: u64) -> CoreError {
    CoreError::database(DatabaseError::Query {
        sql: sql.to_string(),
        reason: format!("Query timed out after {}ms", timeout_ms),
        position: None,
        location: None,
    })
}

/// 普通执行路径（**带取消令牌与超时**）
///
/// 事务内的语句走另一条路（驱动的事务对象只有 `query/commit/rollback`，没有取消入口），
/// 所以只有这里能取消与超时。
async fn run_plain(
    manager: &Arc<ConnectionManager>,
    db: &DynDatabase,
    sql: &str,
    timeout_ms: Option<u64>,
    conn_key: &String,
) -> Result<QueryResult, CoreError> {
    let cancel_token = manager.create_cancel_token(conn_key).await;
    let result = if let Some(timeout_ms) = timeout_ms {
        match tokio::time::timeout(
            tokio::time::Duration::from_millis(timeout_ms),
            db.query_with_cancel(sql, cancel_token.clone()),
        )
        .await
        {
            Ok(inner_result) => inner_result,
            Err(_elapsed) => {
                cancel_token.cancel();
                Err(query_timeout_error(sql, timeout_ms))
            }
        }
    } else {
        db.query_with_cancel(sql, cancel_token).await
    };
    manager.remove_cancel_token(conn_key).await;
    result
}

/// 当前 unix 毫秒时间戳（失败时给 0：界面只拿它减一下算时长，不做绝对时间展示）
fn unix_ms_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

pub fn value_to_sql(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        serde_json::Value::Array(arr) => {
            format!("'{}'", serde_json::to_string(arr).unwrap_or_default())
        }
        serde_json::Value::Object(obj) => {
            format!("'{}'", serde_json::to_string(obj).unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_service() -> SqlService {
        SqlService::new(Arc::new(ConnectionManager::new()))
    }

    #[tokio::test]
    async fn test_execute_empty_sql() {
        let service = new_service();
        let result = service.execute(None, "", Default::default()).await;
        assert!(result.is_err());
    }

    /// 窗口包装：查询套上 `LIMIT/OFFSET`，尾分号与空白先去掉
    #[test]
    fn window_sql_wraps_queries_with_limit_and_offset() {
        use super::window_sql;

        let wrapped = window_sql("SELECT n FROM t", 1000, 2000).expect("查询能分段");
        assert!(wrapped.contains("SELECT n FROM t"), "{wrapped}");
        assert!(wrapped.ends_with("LIMIT 1000 OFFSET 2000"), "{wrapped}");
        // 别名是 MySQL 这类要别名的方言要求的
        assert!(wrapped.contains("AS rds_segment"), "{wrapped}");

        // 尾分号 / 空白不该带进子查询（带进就是语法错）
        let cleaned = window_sql("  SELECT 1;  ", 10, 0).expect("能分段");
        assert!(!cleaned.contains(";"), "{cleaned}");

        // `WITH …` 也是查询：照样能分段（`SELECT * FROM (WITH …)` 四个方言都认）
        let cte = window_sql("WITH x AS (SELECT 1 AS n) SELECT n FROM x", 10, 0);
        assert!(cte.is_some(), "CTE 应当能分段：{cte:?}");
    }

    /// 没有结果集的语句不能分段（DML / DDL / 空 / limit 0）
    #[test]
    fn window_sql_refuses_statements_without_a_result_set() {
        use super::window_sql;

        for sql in [
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET n = 1",
            "DELETE FROM t",
            "CREATE TABLE t (n INT)",
            "",
            "   ;  ",
        ] {
            assert!(window_sql(sql, 1000, 0).is_none(), "不该能分段：{sql}");
        }
        assert!(window_sql("SELECT 1", 0, 0).is_none(), "limit 0 不是一段");
    }

    /// 【B10】元信息语句**不能**被窗口包装（真机踩到）：
    /// `SELECT * FROM (EXPLAIN SELECT …) AS rds_segment LIMIT 1000 OFFSET 0` 在 MySQL 上报 1064。
    /// 拿不到窗口就该走“原路执行”，而不是发一条包坏的语句出去。
    #[test]
    fn window_sql_leaves_meta_statements_alone() {
        use super::window_sql;

        for sql in [
            "EXPLAIN SELECT n FROM t",
            "EXPLAIN QUERY PLAN SELECT n FROM t",
            "SHOW TABLES",
            "PRAGMA table_info(t)",
        ] {
            assert!(
                window_sql(sql, 1000, 0).is_none(),
                "元信息语句不该被包：{sql}"
            );
        }
        // 正常查询照旧能包（分段抓取靠它）
        assert!(window_sql("SELECT n FROM t", 1000, 0).is_some());
    }

    /// 分段执行的报错要还原成**用户写的那句**：位置平移、回显的 SQL 换掉、落在包装上就不给位置
    #[test]
    fn segment_errors_are_rewritten_for_the_original_sql() {
        use super::{unwrap_segment_error, window_segment};

        fn query_error_at(sql: &str, position: usize) -> CoreError {
            CoreError::database(DatabaseError::Query {
                sql: sql.to_string(),
                reason: "boom".to_string(),
                position: Some(position),
                location: None,
            })
        }

        // PG 报的位置是相对**发出去的文本**（= 包装后）的：模拟“写错的列名”在包装里的偏移
        let original = "SELECT no_such_column_xyz FROM t";
        let window = window_segment(original, 1000, 0).expect("查询能分段");
        let typo_at = window.sql.find("no_such_column_xyz").expect("包装里有原句");
        // 先确认场景成立：包装确实把原句推后了（不然这条测试什么都没盯住）
        assert!(typo_at > 0, "原句不在包装的开头");

        let mapped = unwrap_segment_error(
            Err(query_error_at(&window.sql, typo_at)),
            original,
            &window,
        );
        let Err(CoreError::Database(DatabaseError::Query { sql, position, .. })) = mapped else {
            panic!("应当仍是查询错误");
        };
        assert_eq!(sql, original, "回显的 SQL 应当是用户写的那句（不带包装）");
        let position = position.expect("位置该还原出来");
        assert_eq!(
            &original[position..position + "no_such_column_xyz".len()],
            "no_such_column_xyz",
            "还原后的位置要正好落在写错的词上（实得 {position}）"
        );

        // 位置落在包装上（`AS rds_segment` 那一带）→ 不给位置（给错位置比不给更糟）
        let tail = window.sql.find("AS rds_segment").expect("有别名");
        let tailed = unwrap_segment_error(
            Err(query_error_at(&window.sql, tail)),
            original,
            &window,
        );
        assert!(
            matches!(
                tailed,
                Err(CoreError::Database(DatabaseError::Query {
                    position: None,
                    ..
                }))
            ),
            "落在包装上的位置不该给出去"
        );

        // 原句带前导空白：偏移要按**原句**（含空白）算，否则光标会差几个字
        let indented = "   SELECT no_such_column_xyz FROM t";
        let window = window_segment(indented, 10, 0).expect("能分段");
        let typo_at = window.sql.find("no_such_column_xyz").expect("有");
        let mapped = unwrap_segment_error(
            Err(query_error_at(&window.sql, typo_at)),
            indented,
            &window,
        );
        let Err(CoreError::Database(DatabaseError::Query { position, .. })) = mapped else {
            panic!("应当仍是查询错误");
        };
        let position = position.expect("位置该还原出来");
        assert_eq!(
            &indented[position..position + "no_such_column_xyz".len()],
            "no_such_column_xyz",
            "带前导空白的原句也要对得上（实得 {position}）"
        );

        // 不是查询错误就原样放行（位置平移到别的错误域上就是胡乱改）
        let untouched = unwrap_segment_error(
            Err(CoreError::database(DatabaseError::TableNotFound {
                table: "t".to_string(),
            })),
            original,
            &window,
        );
        assert!(
            matches!(
                untouched,
                Err(CoreError::Database(DatabaseError::TableNotFound { .. }))
            ),
            "其它错误域不该被改写"
        );
    }

    #[tokio::test]
    async fn test_execute_whitespace_sql() {
        let service = new_service();
        let result = service.execute(None, "   \n\t  ", Default::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_no_active_connection() {
        let service = new_service();
        let result = service.execute(None, "SELECT 1", Default::default()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_specific_connection_not_found() {
        let service = new_service();
        let result = service
            .execute(Some("nonexistent".into()), "SELECT 1", Default::default())
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_with_record_history_option() {
        let service = new_service();
        let options = SqlExecuteOptions {
            record_history: true,
            ..Default::default()
        };
        let result = service.execute(None, "SELECT 1", options).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_in_transaction_empty() {
        let service = new_service();
        let result = service.execute_in_transaction(None, vec![]).await;
        assert!(result.is_ok());
        let results = match result {
            Ok(r) => r,
            Err(_) => {
                panic!("expected Ok");
            }
        };
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_execute_in_transaction_no_connection() {
        let service = new_service();
        let result = service
            .execute_in_transaction(None, vec!["SELECT 1".into()])
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_query_no_connection() {
        let service = new_service();
        let result = service.query(None, "SELECT 1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_sql_history() {
        let service = new_service();
        let result = service.get_sql_history(10);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_search_sql_history() {
        let service = new_service();
        let result = service.search_sql_history("test", 10);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_clear_sql_history() {
        let service = new_service();
        let result = service.clear_sql_history();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_remove_sql_history_nonexistent() {
        let service = new_service();
        let result = service.remove_sql_history("nonexistent");
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_begin_transaction_no_connection() {
        let service = new_service();
        let result = service.begin_transaction(None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_commit_transaction_no_connection() {
        let service = new_service();
        let result = service.commit_transaction(None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rollback_transaction_no_connection() {
        let service = new_service();
        let result = service.rollback_transaction(None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_transaction_status_no_connection() {
        let service = new_service();
        let result = service.get_transaction_status(None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cancel_query_no_connection() {
        let service = new_service();
        let result = service.cancel_query(None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cancel_query_specific_not_found() {
        let service = new_service();
        let result = service.cancel_query(Some("nonexistent".into())).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_register_external_database_no_connection() {
        let service = new_service();
        let result = service
            .register_external_database(None, "ext", "mysql", "mysql://localhost")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_external_table_no_connection() {
        let service = new_service();
        let result = service
            .create_external_table(None, "ext", "public", "users", "ext_users")
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_sql_execute_options_default() {
        let options = SqlExecuteOptions::default();
        assert!(!options.record_history);
        assert!(!options.use_transaction);
        assert!(options.timeout_ms.is_none());
        assert!(!options.use_cache);
    }

    #[test]
    fn test_sql_execute_result_construction() {
        let result = SqlExecuteResult {
            result: QueryResult::empty(),
            elapsed_ms: 42,
            truncated: false,
        };
        assert_eq!(result.elapsed_ms, 42);
        assert!(!result.truncated);
        assert!(result.result.is_empty());
    }
}
