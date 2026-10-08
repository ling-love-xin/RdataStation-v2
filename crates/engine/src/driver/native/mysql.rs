use sqlx::{Column, ConnectOptions as _, MySql, Pool, Row, TypeInfo as _};

/// 单列名字结果集 → 对象列表（例程 / 序列等只在内省里露名字的类别）。
fn names_to_nodes(
    result: &QueryResult,
    kind: crate::driver::SchemaObjectKind,
) -> Vec<crate::driver::NodeInfo> {
    use arrow::array::StringArray;
    let mut nodes: Vec<crate::driver::NodeInfo> = Vec::new();
    for row_idx in 0..result.total_rows() {
        if let Some(batch) = result.batches.first() {
            if row_idx < batch.num_rows() {
                if let Some(arr) = batch.column(0).as_any().downcast_ref::<StringArray>() {
                    let name = arr.value(row_idx);
                    if !name.is_empty() {
                        nodes.push(crate::driver::NodeInfo::new(name.to_string(), kind.clone()));
                    }
                }
            }
        }
    }
    nodes
}
use arrow::array::{ArrayRef, BinaryArray, BooleanArray, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

use crate::driver::traits::MetadataBrowser;
use crate::driver::utils::{
    affected_rows_result, is_read_only_sql as shared_is_read_only_sql, returns_rows,
    sqlx_value_is_null, MY_LIST_CONSTRAINTS_SQL, SqlFlavor,
};
use crate::driver::{
    ColumnDetail, ConstraintDetail, DataSourceMeta, Database, IndexDetail, PoolStatus,
    SchemaObjectKind, Transaction,
};
use shared::error::{ConnectionError, CoreError, DatabaseError};
use shared::models::{ArrowBatch, QueryResult, Value};

/// MySQL 数据库连接
///
/// 封装一条**常驻物理连接**（DBeaver Main 连接模型：无连接池、无 acquire 等待、
/// 无 `test_before_acquire` 验证——池化设计在 PG 真机上反复撞「空闲连接被网络层
/// 静默丢弃，验证挂 TCP 半开 8s 超时」，用户已拍板摒弃池、全参考 DBeaver），
/// 通过 `Database` trait 提供统一的查询/执行接口。
/// 元数据浏览通过 `MetadataBrowser` trait 实现，查询 `information_schema`。
///
/// # 字段
/// * `conn` - **会话级单连接**（`None` = 已断/已回收，下次使用时按 `connect_url` 懒重建）
/// * `last_used` - 连接最后使用时间：空闲超过 [`MYSQL_IDLE_RECYCLE`] 即回收
///   （DBeaver「按需建连 + 空闲回收」轻量会话模型，见 postgres.rs 同款注释）
/// * `connect_url` - 重建连接的 URL
/// * `server_version` - MySQL 服务器版本号，首次连接时获取并缓存
pub struct MySqlDatabase {
    conn: Arc<tokio::sync::Mutex<Option<sqlx::mysql::MySqlConnection>>>,
    last_used: std::sync::Mutex<Option<std::time::Instant>>,
    connect_url: String,
    server_version: Option<String>,
    max_connections: usize,
    min_connections: usize,
}

/// 连接类错误判定：查询失败后把常驻连接置 `None`，下次使用时懒重建。
fn is_mysql_conn_error(e: &sqlx::Error) -> bool {
    if matches!(
        e,
        sqlx::Error::Io(_)
            | sqlx::Error::PoolClosed
            | sqlx::Error::Protocol(_)
            | sqlx::Error::Tls(_)
    ) {
        return true;
    }
    // 服务端主动断开的 Database 错误（sqlx 不总是归一成 Io）——认常见关键字。
    let msg = e.to_string().to_ascii_lowercase();
    msg.contains("server closed") || msg.contains("connection closed")
}

impl MySqlDatabase {
    pub async fn new(url: &str) -> Result<Self, CoreError> {
        // 对 URL 中的密码进行脱敏
        let masked_url = if let Some(at_pos) = url.find('@') {
            if let Some(colon_pos) = url[..at_pos].rfind(':') {
                format!("{}:****{}", &url[..colon_pos], &url[at_pos..])
            } else {
                url.to_string()
            }
        } else {
            url.to_string()
        };
        tracing::info!(
            "MySqlDatabase::new: url_has_creds={}, url_len={}, masked_url={}",
            url.contains('@'),
            url.len(),
            masked_url
        );
        let t_new = std::time::Instant::now();

        // MySQL 8.0 默认使用 caching_sha2_password 认证插件，可能需要在 URL 中禁用 SSL
        // 或配置 TLS。添加 ssl-mode=disabled 以兼容本地开发环境。
        let effective_url = if url.contains("ssl-mode") || url.contains("ssl_mode") {
            url.to_string()
        } else {
            let sep = if url.contains('?') { '&' } else { '?' };
            format!("{}{}ssl-mode=disabled", url, sep)
        };

        // 单连接直连（2026-10-08 摒弃池，同 postgres.rs）：`MySqlConnectOptions::connect`
        // 建**一条**物理连接并持有。此前建池（min=3/max=6/acquire_timeout/test_before_acquire）
        // 的真机结论（app_run49~58，PG 侧）：池 acquire 静默等待 + 死连接验证挂 TCP 半开
        // 超时——DBeaver 无池、每库 Main+Metadata 常驻单连接、串行内省；单连接查询串行
        // （`conn` Mutex），连接失效 → 查询报错 → 置 None → 下次懒重建（握手 ~10-30ms）。
        let mut conn = connect_mysql_single(&effective_url).await.map_err(|e| {
            tracing::warn!(elapsed_ms = %t_new.elapsed().as_millis(), "MySqlDatabase::new connect FAIL");
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql".to_string(),
                operation: "connect".to_string(),
                source: e.to_string(),
            })
        })?;
        tracing::info!(elapsed_ms = %t_new.elapsed().as_millis(), "MySqlDatabase::new connect OK");
        // 连接验证不吞错：SELECT VERSION() 失败视为连接失败（此前 .ok() 吞错，
        // 导致认证/网络失败被当作成功，UI 显示「在线」但内省查询全部失败）。
        let server_version = sqlx::query_scalar::<_, String>("SELECT VERSION()")
            .fetch_one(&mut conn)
            .await
            .map_err(|e| {
                tracing::warn!(elapsed_ms = %t_new.elapsed().as_millis(), "MySqlDatabase::new verify FAIL");
                CoreError::database(DatabaseError::Driver {
                    db_type: "mysql".to_string(),
                    operation: "verify".to_string(),
                    source: e.to_string(),
                })
            })?;
        tracing::info!(elapsed_ms = %t_new.elapsed().as_millis(), "MySqlDatabase::new verify OK");
        Ok(Self {
            conn: Arc::new(tokio::sync::Mutex::new(Some(conn))),
            last_used: std::sync::Mutex::new(Some(std::time::Instant::now())),
            connect_url: effective_url,
            server_version: Some(server_version),
            max_connections: 10,
            min_connections: 2,
        })
    }

    pub fn from_pool(pool: Pool<MySql>) -> Self {
        // 兼容入口：无法从池反推 URL，且 sqlx 的 `PoolConnection` 没有公开的底层
        // 连接取出接口——这里不取连接（`None`，等 `new` 路径重建）。
        // `from_pool` 仅服务遗留的 `mysql_pool.rs` 封装（无实际调用方），
        // 主路径一律走 `new`（单连接直连）。
        let _ = pool;
        Self {
            conn: Arc::new(tokio::sync::Mutex::new(None)),
            last_used: std::sync::Mutex::new(None),
            connect_url: String::new(),
            server_version: None,
            max_connections: 10,
            min_connections: 0,
        }
    }

    pub fn from_pool_with_config(
        pool: Pool<MySql>,
        max_connections: usize,
        min_connections: usize,
    ) -> Self {
        let mut this = Self::from_pool(pool);
        this.max_connections = max_connections;
        this.min_connections = min_connections;
        this
    }
}

/// 常驻连接查询级超时：超过即销毁连接（半开连接挂起的根治手段，见 `query` 注释）。
const MYSQL_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// 连接空闲回收阈值：超过即主动关闭（DBeaver「按需建连 + 空闲回收」轻量会话模型）。
/// **1s 实证调参**（2026-10-08，同 postgres.rs）：建连实测 150-380ms 环境固有；
/// 一次展开 4 类别查询间隔 <10ms 复用同一连接；闲置 >1s 即回收新建，
/// 避开"闲置复用挂起"铁证窗口（2.4s/9.8s 即挂）。
const MYSQL_IDLE_RECYCLE: std::time::Duration = std::time::Duration::from_secs(1);

/// 按 URL 建一条 MySQL 物理连接（单连接化后的统一入口）。
async fn connect_mysql_single(url: &str) -> Result<sqlx::mysql::MySqlConnection, sqlx::Error> {
    let options: sqlx::mysql::MySqlConnectOptions = url.parse()?;
    options.connect().await
}

/// 拿会话级连接的锁（`None` 时按 `connect_url` 懒重建）；调用方持有 guard 期间
/// 连接被独占，查询串行（等价 DBeaver SingleThreadLock）。
///
/// **空闲回收**（2026-10-08 对齐 DBeaver「按需建连 + 空闲回收」，同 postgres.rs）：
/// 连接闲置超过 [`MYSQL_IDLE_RECYCLE`] 时置 `None` 重建——连接短命、用完回收，
/// 避免常驻连接被网络层静默丢弃后下一次查询挂起。
async fn lock_mysql_conn<'a>(
    conn: &'a Arc<tokio::sync::Mutex<Option<sqlx::mysql::MySqlConnection>>>,
    last_used: &std::sync::Mutex<Option<std::time::Instant>>,
    connect_url: &str,
) -> Result<tokio::sync::MutexGuard<'a, Option<sqlx::mysql::MySqlConnection>>, CoreError> {
    let mut guard = conn.lock().await;
    // 空闲回收：超过阈值即弃旧建新（"按需建连"）。
    if let Some(last) = *last_used.lock().unwrap_or_else(|p| p.into_inner()) {
        if last.elapsed() > MYSQL_IDLE_RECYCLE {
            *guard = None;
        }
    }
    if guard.is_none() {
        if connect_url.is_empty() {
            return Err(CoreError::database(DatabaseError::Driver {
                db_type: "mysql".to_string(),
                operation: "reconnect".to_string(),
                source: "连接已断开，且没有可重建的 URL（from_pool 路径）".to_string(),
            }));
        }
        *guard = Some(
            connect_mysql_single(connect_url)
                .await
                .map_err(|e| {
                    CoreError::database(DatabaseError::Driver {
                        db_type: "mysql".to_string(),
                        operation: "reconnect".to_string(),
                        source: e.to_string(),
                    })
                })?,
        );
    }
    // 刷新"最后使用时间"（拿锁即视为使用）。
    *last_used.lock().unwrap_or_else(|p| p.into_inner()) = Some(std::time::Instant::now());
    Ok(guard)
}

/// 查询结果错误映射：连接类错误 → 常驻连接置 `None`（下次懒重建）。
fn mysql_conn_result<T>(
    guard: &mut Option<sqlx::mysql::MySqlConnection>,
    sql: &str,
    result: Result<T, sqlx::Error>,
) -> Result<T, CoreError> {
    match result {
        Ok(v) => Ok(v),
        Err(e) => {
            if is_mysql_conn_error(&e) {
                // 连接已死：置 None，下次懒重建
                *guard = None;
            }
            Err(mysql_query_error(sql, &e))
        }
    }
}

/// 只读语句判定：口径表在 [`crate::driver::utils::is_read_only_sql`]（六个驱动共用一份）。
fn is_read_only_sql(sql: &str) -> bool {
    shared_is_read_only_sql(sql, SqlFlavor::MySql)
}

#[async_trait::async_trait]
impl Database for MySqlDatabase {
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError> {
        // 事务控制 / 会话类语句必须走文本协议（prepared 协议下 MySQL 报 1295）
        if needs_text_protocol(sql) {
            return execute_via_text_protocol(&self.conn, &self.last_used, &self.connect_url, sql).await;
        }

        let is_read_only = is_read_only_sql(sql);
        // B5 / P0.6：不返回结果集的写语句走 `execute`，拿驱动的真实影响行数
        if !is_read_only && !returns_rows(sql) {
            return execute_writing(&self.conn, &self.last_used, &self.connect_url, sql).await;
        }

        // **会话级复用 + 1s 空闲回收**（2026-10-08 实证调参，同 postgres.rs：
        // 建连 150-380ms 环境固有；一次展开 4 类别间隔 <10ms 复用同一连接；
        // 闲置 >1s 由 `lock_mysql_conn` 回收重建，避开"闲置复用挂起"铁证窗口）。
        let mut guard = lock_mysql_conn(&self.conn, &self.last_used, &self.connect_url).await?;
        let query_result = {
            let conn = guard.as_mut().expect("lock_mysql_conn 保证 Some");
            // 查询级超时（15s 无响应 → 销毁连接 → 报明确错误；下次懒重建）。
            tokio::time::timeout(MYSQL_QUERY_TIMEOUT, async {
                sqlx::query(sqlx::AssertSqlSafe(sql)).fetch_all(&mut *conn).await
            })
            .await
            .map_err(|_| {
                *guard = None;
                CoreError::database(DatabaseError::Query {
                    sql: sql.to_string(),
                    reason: format!("查询超时（{} 秒无响应），连接已重建，请重试", MYSQL_QUERY_TIMEOUT.as_secs()),
                    position: None,
                    location: None,
                })
            })?
        };
        let rows = mysql_conn_result(&mut guard, sql, query_result)?;

        let columns: Vec<String> = if let Some(first) = rows.first() {
            first
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect()
        } else {
            vec![]
        };

        build_query_result(&columns, &rows, is_read_only)
    }

    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<shared::models::Value>,
    ) -> Result<QueryResult, CoreError> {
        let is_read_only = is_read_only_sql(sql);

        let mut query_builder = sqlx::query(sqlx::AssertSqlSafe(sql));

        for param in &params {
            query_builder = match param {
                shared::models::Value::Null => query_builder.bind(None::<String>),
                shared::models::Value::Bool(v) => query_builder.bind(*v),
                shared::models::Value::Int(v) => query_builder.bind(*v),
                shared::models::Value::Float(v) => query_builder.bind(*v),
                shared::models::Value::Text(v) => query_builder.bind(v),
                shared::models::Value::Bytes(v) => query_builder.bind(v.clone()),
            };
        }

        // **会话级复用 + 1s 空闲回收**（同 `query`，见其上注释）。
        let mut guard = lock_mysql_conn(&self.conn, &self.last_used, &self.connect_url).await?;
        let query_result = {
            let conn = guard.as_mut().expect("lock_mysql_conn 保证 Some");
            // 同 `query`：15s 查询级超时，超时销毁连接。
            tokio::time::timeout(MYSQL_QUERY_TIMEOUT, async {
                query_builder.fetch_all(&mut *conn).await
            })
            .await
            .map_err(|_| {
                *guard = None;
                CoreError::database(DatabaseError::Query {
                    sql: sql.to_string(),
                    reason: format!("查询超时（{} 秒无响应），连接已重建，请重试", MYSQL_QUERY_TIMEOUT.as_secs()),
                    position: None,
                    location: None,
                })
            })?
        };
        let rows = mysql_conn_result(&mut guard, sql, query_result)?;

        let columns: Vec<String> = if let Some(first) = rows.first() {
            first
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect()
        } else {
            vec![]
        };

        build_query_result(&columns, &rows, is_read_only)
    }

    async fn query_with_cancel(
        &self,
        sql: &str,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<QueryResult, CoreError> {
        let sql_owned = sql.to_string();
        let sql_for_cancel = sql.to_string();
        // 单连接模型下，取消查询用**临时独立连接**（同 postgres.rs）：select 丢弃
        // 临时连接即可中止等待，不影响常驻连接。
        let url = self.connect_url.clone();

        tokio::select! {
            result = async move {
                // 取消路径也不能绕过文本协议：`BEGIN` 走 prepared 会被 MySQL 直接拒绝
                if needs_text_protocol(&sql_owned) {
                    return execute_via_text_protocol_single(&url, &sql_owned).await;
                }

                let is_read_only = is_read_only_sql(&sql_owned);
                // B5 / P0.6：写语句走 `execute` 拿影响行数（取消路径同样）
                if !is_read_only && !returns_rows(&sql_owned) {
                    return execute_writing_single(&url, &sql_owned).await;
                }

                let mut conn = connect_mysql_single(&url).await.map_err(|e| mysql_query_error(&sql_owned, &e))?;
                let rows = sqlx::query(sqlx::AssertSqlSafe(sql_owned.as_str()))
                    .fetch_all(&mut conn)
                    .await
                    .map_err(|e| mysql_query_error(&sql_owned, &e))?;

                let columns: Vec<String> = if let Some(first) = rows.first() {
                    first.columns().iter().map(|c| c.name().to_string()).collect()
                } else {
                    vec![]
                };

                build_query_result(&columns, &rows, is_read_only)
            } => result,
            _ = cancel_token.cancelled() => {
                Err(CoreError::database(DatabaseError::Query {
                    sql: sql_for_cancel,
                    reason: "Query cancelled".to_string(),
                    position: None,
                    location: None,
                }))
            }
        }
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, CoreError> {
        // 事务用**一次性连接通道**（DBeaver SQLEditor 同款：事务独立连接，不占常驻
        // 查询通道）。sqlx 0.9 的 `Connection::begin`（&mut self）返回借用生命周期的
        // `Transaction<'_, _>`，无法装进 `Box<dyn Transaction>`（隐含 'static）；
        // 只有 `Pool::begin` 能产出 `Transaction<'static>`（连接 owned 进事务）。
        // 这里建 max=1 的一次性池仅作「owned 连接载体」，用后即弃（事务结束连接
        // 关闭），**不是常驻池**——与主连接的单连接模型不冲突。
        let url = self.connect_url.clone();
        if url.is_empty() {
            return Err(CoreError::database(DatabaseError::Driver {
                db_type: "mysql".to_string(),
                operation: "begin_transaction".to_string(),
                source: "连接已断开，且没有可重建的 URL（from_pool 路径）".to_string(),
            }));
        }
        let pool = sqlx::mysql::MySqlPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "mysql".to_string(),
                    operation: "begin_transaction".to_string(),
                    source: e.to_string(),
                })
            })?;
        let tx = pool.begin().await.map_err(|e| {
            CoreError::database(DatabaseError::Driver {
                db_type: "mysql".to_string(),
                operation: "begin_transaction".to_string(),
                source: e.to_string(),
            })
        })?;
        Ok(Box::new(MySqlTransaction::new(tx)))
    }

    fn meta(&self) -> DataSourceMeta {
        DataSourceMeta {
            server_version: self.server_version.clone(),
            ..DataSourceMeta::mysql()
        }
    }

    async fn ping(&self) -> Result<(), CoreError> {
        let mut guard = lock_mysql_conn(&self.conn, &self.last_used, &self.connect_url).await?;
        let ping_result = {
            let conn = guard.as_mut().expect("lock_mysql_conn 保证 Some");
            sqlx::query("SELECT 1").fetch_one(&mut *conn).await
        };
        mysql_conn_result(&mut guard, "SELECT 1", ping_result).map_err(|e| {
            CoreError::connection(ConnectionError::Other {
                conn_id: "mysql".to_string(),
                reason: format!("Ping failed: {}", e),
            })
        })?;
        Ok(())
    }

    async fn pool_status(&self) -> Option<PoolStatus> {
        // 单连接口径：常驻 1 条（活跃 0 空闲 1；连接已断 = None 时按 0/0）
        let guard = self.conn.try_lock();
        let connected = guard.as_deref().map(|c| c.is_some()).unwrap_or(false);
        Some(PoolStatus {
            size: usize::from(connected),
            idle: usize::from(connected),
            active: 0,
            waiting: 0,
            max_connections: self.max_connections,
            min_connections: self.min_connections,
        })
    }

    async fn list_catalogs(&self) -> Result<Vec<String>, CoreError> {
        self.get_catalogs()
            .await
            .map(|nodes| nodes.into_iter().map(|n| n.name).collect())
    }

    async fn list_tables(
        &self,
        catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        // MySQL 的 database 即 schema（见 `has_schema_level`）。
        self.get_tables(catalog, catalog).await
    }

    async fn list_columns(
        &self,
        catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ColumnDetail>, CoreError> {
        let detail = self.get_table_detail(catalog, catalog, table).await?;
        Ok(detail.columns)
    }

    async fn list_indexes(
        &self,
        catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<IndexDetail>, CoreError> {
        let sql = format!("SHOW INDEX FROM `{}`.`{}`", catalog, table);
        let result = self.query_with_params(&sql, vec![]).await?;

        let mut indexes: Vec<IndexDetail> = Vec::new();
        if let Some(batch) = result.batches.first() {
            let name_col = batch
                .column_by_name("Key_name")
                .and_then(|c| c.as_any().downcast_ref::<arrow::array::StringArray>());
            let col_col = batch
                .column_by_name("Column_name")
                .and_then(|c| c.as_any().downcast_ref::<arrow::array::StringArray>());
            let non_unique_col = batch
                .column_by_name("Non_unique")
                .and_then(|c| c.as_any().downcast_ref::<arrow::array::Int64Array>());

            if let (Some(name_arr), Some(col_arr)) = (name_col, col_col) {
                let mut seen: std::collections::HashMap<String, IndexDetail> =
                    std::collections::HashMap::new();
                for row_idx in 0..batch.num_rows() {
                    let name = name_arr.value(row_idx).to_string();
                    let col_name = col_arr.value(row_idx).to_string();
                    let is_unique = non_unique_col
                        .map(|a| a.value(row_idx) == 0)
                        .unwrap_or(false);

                    seen.entry(name.clone())
                        .and_modify(|idx| {
                            idx.column_names.push(col_name.clone());
                        })
                        .or_insert(IndexDetail {
                            name,
                            table_name: table.to_string(),
                            column_names: vec![col_name],
                            is_unique,
                            is_primary: false,
                            index_type: None,
                            comment: None,
                        });
                }
                indexes = seen.into_values().collect();
            }
        }
        Ok(indexes)
    }

    /// 列举约束（三张 information_schema 表拼出来）。
    ///
    /// 为什么必须补：本方法此前**没实现**，而 `MetadataBrowser::get_constraints` 是
    /// `self.list_constraints(...)` 的纯转发 → 落到 trait 默认空实现 →
    /// **属性面板的「约束」分区恒为空**（与已修的「序列 / 触发器被空实现遮蔽」同一形态）。
    ///
    /// 三张表各管一段：`TABLE_CONSTRAINTS`（有哪些约束）→ `KEY_COLUMN_USAGE`（涉及哪些列、
    /// 引用谁）→ `REFERENTIAL_CONSTRAINTS`（更新 / 删除规则）。
    /// `GROUP_CONCAT(... ORDER BY ORDINAL_POSITION)` 把组合约束的列按**声明顺序**拼回一个
    /// 字符串 —— 组合主键的顺序有意义，不能靠聚合的默认顺序。
    async fn list_constraints(
        &self,
        _catalog: &str,
        schema: Option<&str>,
        table: &str,
    ) -> Result<Vec<ConstraintDetail>, CoreError> {
        let sql = MY_LIST_CONSTRAINTS_SQL;
        let result = self
            .query_with_params(
                sql,
                vec![
                    Value::Text(schema.unwrap_or_default().to_string()),
                    Value::Text(table.to_string()),
                ],
            )
            .await?;

        let mut out: Vec<ConstraintDetail> = Vec::new();
        let Some(batch) = result.batches.first() else {
            return Ok(out);
        };
        let col = |name: &str| {
            batch
                .column_by_name(name)
                .and_then(|c| c.as_any().downcast_ref::<arrow::array::StringArray>())
        };
        let (Some(cname), Some(ctype), Some(cols), Some(ref_table), Some(ref_cols), Some(upd), Some(del)) = (
            col("cname"),
            col("ctype"),
            col("cols"),
            col("ref_table"),
            col("ref_cols"),
            col("upd"),
            col("del"),
        ) else {
            return Ok(out);
        };

        let split = |text: &str| -> Vec<String> {
            text.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };

        for row in 0..batch.num_rows() {
            let ref_table = ref_table.value(row);
            out.push(ConstraintDetail {
                name: cname.value(row).to_string(),
                table_name: table.to_string(),
                // MySQL 的 CONSTRAINT_TYPE 本来就是展示名（PRIMARY KEY / FOREIGN KEY /
                // UNIQUE / CHECK），不用再映射
                constraint_type: ctype.value(row).to_string(),
                column_names: split(cols.value(row)),
                referenced_table: (!ref_table.is_empty()).then(|| ref_table.to_string()),
                referenced_columns: split(ref_cols.value(row)),
                update_rule: (!upd.value(row).is_empty()).then(|| upd.value(row).to_string()),
                delete_rule: (!del.value(row).is_empty()).then(|| del.value(row).to_string()),
            });
        }
        Ok(out)
    }

    async fn list_procedures(
        &self,
        catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let sql = "\
            SELECT ROUTINE_NAME FROM INFORMATION_SCHEMA.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'PROCEDURE' \
             ORDER BY ROUTINE_NAME";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(names_to_nodes(&result, SchemaObjectKind::Procedure))
    }

    async fn list_functions(
        &self,
        catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let sql = "\
            SELECT ROUTINE_NAME FROM INFORMATION_SCHEMA.ROUTINES \
             WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'FUNCTION' \
             ORDER BY ROUTINE_NAME";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        Ok(names_to_nodes(&result, SchemaObjectKind::Function))
    }

    async fn get_routine_source(
        &self,
        catalog: &str,
        _schema: Option<&str>,
        name: &str,
        kind: SchemaObjectKind,
    ) -> Result<Option<String>, CoreError> {
        let stmt_type = match kind {
            SchemaObjectKind::Procedure => "PROCEDURE",
            SchemaObjectKind::Function => "FUNCTION",
            _ => return Ok(None),
        };
        let esc_catalog = catalog.replace('`', "``");
        let esc_name = name.replace('`', "``");
        let sql = format!("SHOW CREATE {} `{}`.`{}`", stmt_type, esc_catalog, esc_name,);
        let result = self.query(&sql).await?;
        Ok(crate::driver::utils::create_statement(&result))
    }

    /// 源版 DDL：`SHOW CREATE TABLE`（对视图也有效 —— 返回的列名是 `Create View`）。
    async fn get_table_ddl(
        &self,
        catalog: &str,
        _schema: Option<&str>,
        table: &str,
    ) -> Result<Option<String>, CoreError> {
        use crate::driver::utils::quote_identifier;
        let sql = format!(
            "SHOW CREATE TABLE {}.{}",
            quote_identifier(catalog, '`'),
            quote_identifier(table, '`')
        );
        let result = self.query(&sql).await?;
        Ok(crate::driver::utils::create_statement(&result))
    }

    fn as_metadata_browser(&self) -> Option<&dyn crate::driver::MetadataBrowser> {
        Some(self)
    }

    /// **一次建连批量内省**（DBeaver 会话语义，2026-10-08 根治 prepared 复用挂起，
    /// 同 postgres.rs）：一条**新**连接 + 每类对象各执行一次不同 SQL，连接用完即弃。
    /// MySQL 无独立序列对象（AUTO_INCREMENT 内嵌在表上）→ `sequences` 恒为空。
    async fn introspect_schema_objects(
        &self,
        catalog: &str,
        schema: &str,
    ) -> Result<crate::driver::SchemaObjects, CoreError> {
        use sqlx::Row as _;
        // MySQL 的 schema 参数即 catalog（information_schema 按 ROUTINE_SCHEMA/TABLE_SCHEMA 过滤）。
        let _ = schema;
        let mut conn = connect_mysql_single(&self.connect_url)
            .await
            .map_err(|e| mysql_query_error(catalog, &e))?;

        // 1) 表 / 视图
        let rows = tokio::time::timeout(MYSQL_QUERY_TIMEOUT, async {
            sqlx::query(
                "SELECT table_name, table_type FROM information_schema.tables \
                 WHERE table_schema = ? ORDER BY table_name",
            )
            .bind(catalog)
            .fetch_all(&mut conn)
            .await
        })
        .await
        .map_err(|_| mysql_batch_timeout())?
        .map_err(|e| mysql_query_error(catalog, &e))?;
        let mut tables = Vec::with_capacity(rows.len());
        for r in &rows {
            let name: String = r.get(0);
            let ty: String = r.get(1);
            tables.push(crate::driver::NodeInfo::new(
                name,
                if ty == "VIEW" {
                    crate::driver::SchemaObjectKind::View
                } else {
                    crate::driver::SchemaObjectKind::Table
                },
            ));
        }

        // 2) 过程（ROUTINES WHERE TYPE='PROCEDURE'）
        let rows = tokio::time::timeout(MYSQL_QUERY_TIMEOUT, async {
            sqlx::query(
                "SELECT ROUTINE_NAME FROM INFORMATION_SCHEMA.ROUTINES \
                 WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'PROCEDURE' ORDER BY ROUTINE_NAME",
            )
            .bind(catalog)
            .fetch_all(&mut conn)
            .await
        })
        .await
        .map_err(|_| mysql_batch_timeout())?
        .map_err(|e| mysql_query_error(catalog, &e))?;
        let procedures = rows
            .iter()
            .map(|r| crate::driver::NodeInfo::new(r.get::<String, _>(0), crate::driver::SchemaObjectKind::Procedure))
            .collect();

        // 3) 函数（ROUTINES WHERE TYPE='FUNCTION'）
        let rows = tokio::time::timeout(MYSQL_QUERY_TIMEOUT, async {
            sqlx::query(
                "SELECT ROUTINE_NAME FROM INFORMATION_SCHEMA.ROUTINES \
                 WHERE ROUTINE_SCHEMA = ? AND ROUTINE_TYPE = 'FUNCTION' ORDER BY ROUTINE_NAME",
            )
            .bind(catalog)
            .fetch_all(&mut conn)
            .await
        })
        .await
        .map_err(|_| mysql_batch_timeout())?
        .map_err(|e| mysql_query_error(catalog, &e))?;
        let functions = rows
            .iter()
            .map(|r| crate::driver::NodeInfo::new(r.get::<String, _>(0), crate::driver::SchemaObjectKind::Function))
            .collect();

        // 4) 序列：MySQL 无独立序列对象 → 空
        let sequences = Vec::new();

        // 5) 触发器（名字 + 所属表）
        let rows = tokio::time::timeout(MYSQL_QUERY_TIMEOUT, async {
            sqlx::query(
                "SELECT trigger_name, event_object_table FROM information_schema.triggers \
                 WHERE trigger_schema = ? ORDER BY trigger_name",
            )
            .bind(catalog)
            .fetch_all(&mut conn)
            .await
        })
        .await
        .map_err(|_| mysql_batch_timeout())?
        .map_err(|e| mysql_query_error(catalog, &e))?;
        let triggers = rows
            .iter()
            .map(|r| {
                crate::driver::NodeInfo::new(r.get::<String, _>(0), crate::driver::SchemaObjectKind::Trigger)
                    .with_parent(r.get::<String, _>(1))
            })
            .collect();

        Ok(crate::driver::SchemaObjects {
            tables,
            procedures,
            functions,
            sequences,
            triggers,
        })
    }
}

/// MySQL 事务句柄
///
/// 封装 `sqlx::Transaction`，支持 begin/commit/rollback。
/// Drop 时若未提交且未回滚则自动回滚，避免悬挂事务。
pub struct MySqlTransaction {
    tx: Option<sqlx::Transaction<'static, MySql>>,
}

impl MySqlTransaction {
    fn new(tx: sqlx::Transaction<'static, MySql>) -> Self {
        Self { tx: Some(tx) }
    }
}

#[async_trait::async_trait]
impl Transaction for MySqlTransaction {
    async fn query(&mut self, sql: &str) -> Result<QueryResult, CoreError> {
        if let Some(ref mut tx) = self.tx {
            let sql_upper = sql.trim_start().to_uppercase();
            let _is_read_only = sql_upper.starts_with("SELECT")
                || sql_upper.starts_with("SHOW")
                || sql_upper.starts_with("DESCRIBE");

            // B5 / P0.6：事务内的写语句同样要真实影响行数
            if !is_read_only_sql(sql) && !returns_rows(sql) {
                let result = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| mysql_query_error(sql, &e))?;
                return Ok(affected_rows_result(result.rows_affected()));
            }

            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .fetch_all(&mut **tx)
                .await
                .map_err(|e| mysql_query_error(sql, &e))?;

            if rows.is_empty() {
                return Ok(QueryResult {
                    columns: vec![],
                    batches: vec![],
                    ..Default::default()
                });
            }

            let columns: Vec<String> = rows[0]
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect();

            let batch = mysql_rows_to_arrow(&columns, &rows)?;

            Ok(QueryResult {
                columns,
                batches: vec![batch],
                ..Default::default()
            })
        } else {
            Err(CoreError::database(DatabaseError::Driver {
                db_type: "mysql".to_string(),
                operation: "query".to_string(),
                source: "Transaction already closed".to_string(),
            }))
        }
    }

    async fn commit(&mut self) -> Result<(), CoreError> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await.map_err(|e| {
                CoreError::database(DatabaseError::Driver {
                    db_type: "mysql".to_string(),
                    operation: "commit".to_string(),
                    source: e.to_string(),
                })
            })?;
        }
        Ok(())
    }

    async fn rollback(&mut self) -> Result<(), CoreError> {
        if let Some(tx) = self.tx.take() {
            if let Err(e) = tx.rollback().await {
                tracing::warn!("MySQL transaction rollback error: {}", e);
            }
        }
        Ok(())
    }
}

/// 必须走**文本协议**（`COM_QUERY`）的语句
///
/// MySQL 在 prepared 协议下拒绝执行事务控制与部分会话语句：
/// `1295 (HY000): This command is not supported in the prepared statement protocol yet`。
/// 而产品必须能在同一会话里发 `BEGIN` / `COMMIT` / `ROLLBACK`（事务 UI 与批量执行），
/// 因此这几类改走 `sqlx::raw_sql`（文本协议）。
///
/// 判定只看首关键字：宁可多走文本协议（代价仅是不复用 prepared 语句），也不能漏
/// —— 漏了就是事务按钮在 MySQL 上直接报错。
fn needs_text_protocol(sql: &str) -> bool {
    matches!(
        first_keywords(sql, 2).as_str(),
        "BEGIN"
            | "START TRANSACTION"
            | "COMMIT"
            | "ROLLBACK"
            | "SAVEPOINT"
            | "RELEASE SAVEPOINT"
            | "SET"
            | "USE"
            | "LOCK TABLES"
            | "UNLOCK TABLES"
    )
}

/// 取前 N 个关键字（大写），跳过前导空白与三种注释（`--` / `#` / `/* */`）
fn first_keywords(sql: &str, count: usize) -> String {
    let mut rest = sql;
    loop {
        rest = rest.trim_start();
        let skipped = if let Some(tail) = rest.strip_prefix("--") {
            tail.split_once('\n').map(|(_, after)| after).unwrap_or("")
        } else if let Some(tail) = rest.strip_prefix('#') {
            tail.split_once('\n').map(|(_, after)| after).unwrap_or("")
        } else if let Some(tail) = rest.strip_prefix("/*") {
            tail.split_once("*/").map(|(_, after)| after).unwrap_or("")
        } else {
            break;
        };
        rest = skipped;
    }

    rest.split_whitespace()
        .take(count)
        .map(|token| token.trim_end_matches([';', '(', ',']).to_ascii_uppercase())
        .collect::<Vec<String>>()
        .join(" ")
}

/// mysql 错误 → 引擎错误；**带上数据库指认的对象**（表 / 列 / 约束）。
///
/// MySQL 的协议层没有这些字段（那是 PG 才有的），所以从消息文本里认 ——
/// 形态与样本见 `crate::driver::error_location::mysql`。拿不到就是 `None`，
/// 界面不写一句空话。
fn mysql_query_error(sql: &str, error: &sqlx::Error) -> CoreError {
    let message = error.to_string();
    let location = crate::driver::error_location::mysql(&message);
    CoreError::database(DatabaseError::query(sql, message).with_location(location))
}

/// 批量内省单条 SQL 超时的统一错误（连接随函数结束 drop）。
fn mysql_batch_timeout() -> CoreError {
    CoreError::database(DatabaseError::Query {
        sql: String::new(),
        reason: format!("查询超时（{} 秒无响应）", MYSQL_QUERY_TIMEOUT.as_secs()),
        position: None,
        location: None,
    })
}

/// 写语句（不返回结果集）走 `execute`：拿驱动的**真实影响行数**（B5 / P0.6）。
/// 走**会话级单连接**（与 `query` 同通道，含空闲回收）。
async fn execute_writing(
    conn: &Arc<tokio::sync::Mutex<Option<sqlx::mysql::MySqlConnection>>>,
    last_used: &std::sync::Mutex<Option<std::time::Instant>>,
    connect_url: &str,
    sql: &str,
) -> Result<QueryResult, CoreError> {
    let mut guard = lock_mysql_conn(conn, last_used, connect_url).await?;
    let exec_result = {
        let conn = guard.as_mut().expect("lock_mysql_conn 保证 Some");
        sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await
    };
    let result = mysql_conn_result(&mut guard, sql, exec_result)?;
    Ok(affected_rows_result(result.rows_affected()))
}

/// 写语句走**临时独立连接**（`query_with_cancel` 的取消分支用；连接随调用结束关闭）。
async fn execute_writing_single(url: &str, sql: &str) -> Result<QueryResult, CoreError> {
    let mut conn = connect_mysql_single(url)
        .await
        .map_err(|e| mysql_query_error(sql, &e))?;
    let result = sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(&mut conn)
        .await
        .map_err(|e| mysql_query_error(sql, &e))?;
    Ok(affected_rows_result(result.rows_affected()))
}

/// 用文本协议执行并返回「无结果集」的结果
///
/// 事务控制 / 会话语句都没有结果集；`affected_rows` 取驱动的真实值（写语句才有意义）。
/// 走**会话级单连接**（与 `query` 同通道，含空闲回收）。
async fn execute_via_text_protocol(
    conn: &Arc<tokio::sync::Mutex<Option<sqlx::mysql::MySqlConnection>>>,
    last_used: &std::sync::Mutex<Option<std::time::Instant>>,
    connect_url: &str,
    sql: &str,
) -> Result<QueryResult, CoreError> {
    let mut guard = lock_mysql_conn(conn, last_used, connect_url).await?;
    let exec_result = {
        let conn = guard.as_mut().expect("lock_mysql_conn 保证 Some");
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await
    };
    let result = mysql_conn_result(&mut guard, sql, exec_result)?;

    Ok(QueryResult {
        columns: vec![],
        batches: vec![],
        affected_rows: Some(result.rows_affected().min(u64::from(u32::MAX)) as u32),
        is_read_only: Some(false),
        ..Default::default()
    })
}

/// 文本协议走**临时独立连接**（`query_with_cancel` 的取消分支用）。
async fn execute_via_text_protocol_single(url: &str, sql: &str) -> Result<QueryResult, CoreError> {
    let mut conn = connect_mysql_single(url)
        .await
        .map_err(|e| mysql_query_error(sql, &e))?;
    let result = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .execute(&mut conn)
        .await
        .map_err(|e| mysql_query_error(sql, &e))?;
    Ok(QueryResult {
        columns: vec![],
        batches: vec![],
        affected_rows: Some(result.rows_affected().min(u64::from(u32::MAX)) as u32),
        is_read_only: Some(false),
        ..Default::default()
    })
}

/// 把查询结果装进 `QueryResult`。
///
/// `is_read_only` 要**真的填进去**：出口层（`SqlService` 的历史记录）据它决定这行结论
/// 该记 `rows_returned` 还是 `rows_affected`。此前这个参数带下划线、值被丢掉，
/// 查询结果的 `is_read_only` 一律是 `None`——结论算出来了，没人用。
fn build_query_result(
    columns: &[String],
    rows: &[sqlx::mysql::MySqlRow],
    is_read_only: bool,
) -> Result<QueryResult, CoreError> {
    if rows.is_empty() {
        return Ok(QueryResult {
            columns: columns.to_vec(),
            batches: vec![],
            is_read_only: Some(is_read_only),
            ..Default::default()
        });
    }
    let batch = mysql_rows_to_arrow(columns, rows)?;
    Ok(QueryResult {
        columns: columns.to_vec(),
        batches: vec![batch],
        is_read_only: Some(is_read_only),
        ..Default::default()
    })
}

/// 将 MySQL 行转换为 Arrow 批处理
/// MySQL 声明类型名是否属于文本族（含 `ENUM` / `SET` / `JSON`）。
fn is_text_type_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.contains("CHAR")
        || n.contains("TEXT")
        || n.contains("ENUM")
        || n.contains("SET")
        || n.contains("JSON")
}

/// 字节可否视为文本。
///
/// MySQL 协议层把 TEXT 与 BLOB 都报成 `BLOB`（sqlx 名 = `BLOB`），无法只凭声明名区分，
/// 故用「可否解码为 UTF-8」作为判据；真正二进制（不可解码）才落 `Binary`。
fn bytes_are_text(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

/// 由 MySQL 声明类型名直接判定**数值**排行（`None` = 不是数值族，交给原有探测）
///
/// 为什么需要：MySQL 的 `BOOL` / `BOOLEAN` 在协议层就是 `TINYINT(1)`，声明名往往只报
/// `TINYINT`，无法据此区分布尔与 1/0 数值列；而 `try_get::<bool>` 对任何 1/0 列都会成功。
/// 因此数值族**一律按数值处理**——宁可把布尔显示成 `1`/`0`，也不能把计数、标志位列
/// （如 `COUNT(*)`）显示成 `true`/`false`。
fn declared_numeric_rank(name: &str) -> Option<u8> {
    // 无符号列在协议层报 `BIGINT UNSIGNED` 这类**带后缀**的名字（PG 侧则是前缀）：
    // 不剭掉就会落进下面的 `try_get::<bool>` 盲探，值为 0/1 的无符号列（如 MySQL
    // `information_schema` 的 `ordinal_position`）会被判成布尔——网格里显示 `true`，
    // 洞察侧取序数也拿不到数字。
    let base = name.replace("UNSIGNED", "");
    Some(match base.trim() {
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "INTEGER" | "BIGINT" | "YEAR" => 2,
        "FLOAT" | "DOUBLE" | "REAL" => 3,
        _ => return None,
    })
}

fn mysql_rows_to_arrow(
    columns: &[String],
    rows: &[sqlx::mysql::MySqlRow],
) -> Result<ArrowBatch, CoreError> {
    let num_rows = rows.len();
    let num_cols = columns.len();

    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(num_cols);

    for col_idx in 0..num_cols {
        let mut string_values: Vec<Option<String>> = Vec::with_capacity(num_rows);
        let mut int_values: Vec<Option<i64>> = Vec::with_capacity(num_rows);
        let mut float_values: Vec<Option<f64>> = Vec::with_capacity(num_rows);
        let mut bool_values: Vec<Option<bool>> = Vec::with_capacity(num_rows);
        let mut binary_values: Vec<Option<Vec<u8>>> = Vec::with_capacity(num_rows);

        // 先用**声明类型**判定字符串 / 二进制。
        // 为何必须：sqlx 的 `Vec<u8>` 在 MySQL 上对 VARCHAR/TEXT 也会解码成功，若只靠
        // `try_get` 探测会把字符串列误判为 `Binary`，导致下游 `StringArray` 下转型全部失败、
        // 元数据内省（catalog / schema / table / column）一律返回空。
        let declared = rows
            .first()
            .map(|r| r.column(col_idx).type_info().name().to_ascii_uppercase())
            .unwrap_or_default();
        let declared_text = is_text_type_name(&declared);
        // 不在按声明名强制二进制：MySQL 把 TEXT 列也报成协议类型 BLOB（sqlx 名 = "BLOB"），
        // 无法只凭声明名区分 TEXT 与真 BLOB；统一交给下面的**字节 UTF-8 判定**。
        //
        // 数值族则**直接按声明名定排行**：不能再靠 `try_get::<bool>` 盲探——MySQL 的
        // `COUNT(*)` 报 BIGINT UNSIGNED，而 sqlx 会把 1/0 成功解成 `bool`，于是 bool 优先的
        // 旧逻辑把计数列判成布尔（网格里 `COUNT(*)` 显示 true）。文本/二进制仍走原有路径。
        // 非空却解不出来的单元格数（列级聚合，填值循环后面留痕）
        let mut undecodable = 0usize;
        let mut detected_rank: u8 = if declared_text {
            5
        } else {
            declared_numeric_rank(&declared).unwrap_or(0)
        };

        if detected_rank == 0 {
            for row in rows {
                use sqlx::Row;

                let row_rank = if let Ok(Some(_)) = row.try_get::<Option<bool>, _>(col_idx) {
                    1 // Boolean
                } else if let Ok(Some(_)) = row.try_get::<Option<i64>, _>(col_idx) {
                    2 // Int64
                } else if let Ok(Some(_)) = row.try_get::<Option<f64>, _>(col_idx) {
                    3 // Float64
                } else if let Ok(Some(bytes)) = row.try_get::<Option<Vec<u8>>, _>(col_idx) {
                    // 字节可解码为 UTF-8 → 文本（MySQL 会把部分文本列（如 information_schema
                    // 的 `TABLE_TYPE`）报成 BINARY 字符集）；否则才是真正的二进制。
                    if bytes_are_text(&bytes) {
                        5 // Utf8
                    } else {
                        4 // Binary
                    }
                } else if let Ok(Some(_)) = row.try_get::<Option<String>, _>(col_idx) {
                    5 // Utf8
                } else {
                    0 // NULL — 不影响类型推断
                };
                if row_rank > detected_rank {
                    detected_rank = row_rank;
                }
                if detected_rank == 5 {
                    break; // Utf8 为最宽类型，无需继续
                }
            }
        }

        let effective_type = match detected_rank {
            1 => DataType::Boolean,
            2 => DataType::Int64,
            3 => DataType::Float64,
            4 => DataType::Binary,
            _ => DataType::Utf8,
        };

        for row in rows {
            use sqlx::Row;

            match effective_type {
                DataType::Boolean => {
                    bool_values.push(row.try_get::<Option<bool>, _>(col_idx).ok().flatten());
                }
                DataType::Int64 => {
                    // 无符号列（如 `COUNT(*)` 的 BIGINT UNSIGNED）用 `i64` 解码会失败，
                    // 旧写法把失败吞成 NULL → 结果集里“计数列全空”。回退按 `u64` 解码。
                    let value = row
                        .try_get::<Option<i64>, _>(col_idx)
                        .ok()
                        .flatten()
                        .or_else(|| {
                            row.try_get::<Option<u64>, _>(col_idx)
                                .ok()
                                .flatten()
                                .map(|v| v.min(i64::MAX as u64) as i64)
                        });
                    int_values.push(value);
                }
                DataType::Float64 => {
                    float_values.push(row.try_get::<Option<f64>, _>(col_idx).ok().flatten());
                }
                DataType::Binary => {
                    binary_values.push(row.try_get::<Option<Vec<u8>>, _>(col_idx).ok().flatten());
                }
                _ => {
                    // Utf8。解码链见 `utils.rs::sqlx_cell_as_text`：
                    // `String` → `BigDecimal`（DECIMAL 保精度）→ 时间族 → UUID → JSON → 字节 lossy。
                    // 最后那条 lossy 是给「MySQL 把部分**文本**列报成 BINARY 字符集」用的
                    // （sqlx 对它们拒绝 `String` 类型解码）。
                    // 2026-09-21 之前只试 `String` + 字节，于是 DECIMAL / DATETIME / JSON 静默变 NULL。
                    let v = crate::sqlx_cell_as_text!(row, col_idx);
                    if v.is_none() && !sqlx_value_is_null(row, col_idx) {
                        undecodable += 1;
                    }
                    string_values.push(v);
                }
            }
        }

        // 解不出来 ≠ 值为 NULL：非空单元格必须留痕（同 postgres.rs 的口径）
        if undecodable > 0 {
            let declared = rows
                .first()
                .map(|r| r.column(col_idx).type_info().name().to_string())
                .unwrap_or_default();
            tracing::warn!(
                driver = "mysql",
                column = %columns[col_idx],
                declared_type = %declared,
                undecodable_rows = undecodable,
                total_rows = num_rows,
                "列里有非空单元格无法解码，已按 NULL 交出（出口层目前不区分「解不出来」与「本来就是 NULL」）"
            );
        }

        let array: ArrayRef = match effective_type {
            DataType::Boolean => Arc::new(BooleanArray::from(bool_values)),
            DataType::Int64 => Arc::new(Int64Array::from(int_values)),
            DataType::Float64 => Arc::new(Float64Array::from(float_values)),
            DataType::Binary => {
                let refs: Vec<Option<&[u8]>> = binary_values
                    .iter()
                    .map(|opt| opt.as_ref().map(|v| v.as_slice()))
                    .collect();
                Arc::new(BinaryArray::from(refs))
            }
            _ => Arc::new(StringArray::from(string_values)),
        };

        arrays.push(array);
    }

    let fields: Vec<Field> = columns
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let data_type = arrays[i].data_type().clone();
            Field::new(name, data_type, true)
        })
        .collect();

    let schema = Arc::new(Schema::new(fields));

    RecordBatch::try_new(schema, arrays).map_err(|e| {
        CoreError::database(DatabaseError::Driver {
            db_type: "mysql".to_string(),
            operation: "arrow_conversion".to_string(),
            source: e.to_string(),
        })
    })
}

#[async_trait::async_trait]
impl crate::driver::MetadataBrowser for MySqlDatabase {
    fn has_schema_level(&self) -> bool {
        false
    }

    async fn get_catalogs(&self) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let result = self
            .query("SELECT schema_name FROM information_schema.schemata ORDER BY schema_name")
            .await?;
        // 系统库过滤（#14）：MySQL 侧此前未过滤，information_schema / mysql /
        // performance_schema / sys 全列出（真机实证 5 库全显）。与 PG 侧
        // `PG_LIST_SCHEMAS_SQL` 的系统对象过滤保持一致，只留业务库。
        const SYSTEM_SCHEMAS: [&str; 4] = [
            "information_schema",
            "mysql",
            "performance_schema",
            "sys",
        ];
        let nodes: Vec<crate::driver::NodeInfo> = (0..result.total_rows())
            .filter_map(|row_idx| {
                result.batches.iter().find_map(|batch| {
                    if row_idx < batch.num_rows() {
                        batch
                            .column(0)
                            .as_any()
                            .downcast_ref::<StringArray>()
                            .map(|arr| {
                                crate::driver::NodeInfo::new(
                                    arr.value(row_idx).to_string(),
                                    crate::driver::SchemaObjectKind::Catalog,
                                )
                            })
                    } else {
                        None
                    }
                })
            })
            .filter(|node| !SYSTEM_SCHEMAS.contains(&node.name.as_str()))
            .collect();
        Ok(nodes)
    }

    async fn get_schemas(&self, _catalog: &str) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        // MySQL 的 database 即 schema，没有独立的 Schema 层级（`has_schema_level` 为 false）。
        // 返回空而非回退为 catalog 列表，避免导航树出现同名重复层。
        Ok(vec![])
    }

    async fn get_tables(
        &self,
        catalog: &str,
        _schema: &str,
    ) -> Result<Vec<crate::driver::NodeInfo>, CoreError> {
        let sql = "SELECT table_name, table_type FROM information_schema.tables WHERE table_schema = ? ORDER BY table_name";
        let result = self
            .query_with_params(sql, vec![Value::Text(catalog.to_string())])
            .await?;
        let nodes: Vec<crate::driver::NodeInfo> = (0..result.total_rows())
            .filter_map(|row_idx| {
                result.batches.iter().find_map(|batch| {
                    if row_idx < batch.num_rows() {
                        if let (Some(name_arr), Some(type_arr)) = (
                            batch.column(0).as_any().downcast_ref::<StringArray>(),
                            batch.column(1).as_any().downcast_ref::<StringArray>(),
                        ) {
                            let table_type = type_arr.value(row_idx);
                            let kind = if table_type == "VIEW" {
                                crate::driver::SchemaObjectKind::View
                            } else {
                                crate::driver::SchemaObjectKind::Table
                            };
                            Some(crate::driver::NodeInfo::new(
                                name_arr.value(row_idx).to_string(),
                                kind,
                            ))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            })
            .collect();
        Ok(nodes)
    }

    async fn get_table_detail(
        &self,
        catalog: &str,
        _schema: &str,
        table: &str,
    ) -> Result<crate::driver::NodeDetail, CoreError> {
        let sql = crate::driver::utils::MY_TABLE_DETAIL_SQL;
        let result = self
            .query_with_params(
                sql,
                vec![
                    Value::Text(catalog.to_string()),
                    Value::Text(table.to_string()),
                ],
            )
            .await?;
        let columns = crate::driver::utils::columns_from_detail_rows(&result);

        Ok(crate::driver::NodeDetail {
            node: crate::driver::NodeInfo::new(table, crate::driver::SchemaObjectKind::Table),
            columns,
            index_count: None,
            row_count_estimate: None,
        })
    }

    async fn get_indexes(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<crate::driver::IndexDetail>, CoreError> {
        self.list_indexes(catalog, Some(schema), table).await
    }

    async fn get_constraints(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<crate::driver::ConstraintDetail>, CoreError> {
        self.list_constraints(catalog, Some(schema), table).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_text_and_binary_classification() {
        // 文本族声明名（大小写无关）。
        for n in ["VARCHAR", "char", "TEXT", "LONGTEXT", "ENUM", "SET", "JSON"] {
            assert!(is_text_type_name(n), "{n} 应视为文本");
        }
        for n in ["BIGINT", "BLOB", "BINARY", "DOUBLE", "DATETIME"] {
            assert!(!is_text_type_name(n), "{n} 不应视为文本");
        }
        // MySQL 把 TEXT 与 BLOB 都报成 `BLOB`：靠字节可否解码为 UTF-8 区分。
        assert!(bytes_are_text(b"BASE TABLE"));
        assert!(!bytes_are_text(&[0xff, 0xfe, 0x00]));
    }

    /// 无符号列在协议层带 `UNSIGNED` 后缀（`BIGINT UNSIGNED`）：要剥掉再判数值族，
    /// 否则 0/1 值的列会被 `try_get::<bool>` 盲探判成布尔（真机：`ordinal_position` → `true`）。
    #[test]
    fn unsigned_declared_types_still_count_as_numeric() {
        for (name, rank) in [
            ("BIGINT UNSIGNED", 2),
            ("INT UNSIGNED", 2),
            ("UNSIGNED BIGINT", 2),
            ("BIGINT", 2),
            ("DOUBLE", 3),
        ] {
            assert_eq!(declared_numeric_rank(name), Some(rank), "{name}");
        }
        // 非数值族仍交给原探测
        assert_eq!(declared_numeric_rank("VARCHAR"), None);
        assert_eq!(declared_numeric_rank("BLOB"), None);
    }

    /// `DECIMAL` / `NUMERIC` **不归数值档**（2026-09-21 改）。
    ///
    /// 老口径把它们算作 f64 档，于是整列被声明成 `Float64`，而 sqlx 对 `DECIMAL`
    /// 只认 `BigDecimal` —— 解码必然失败 → `.ok().flatten()` → **「Float64 列 + NULL 值」**，
    /// 比纯 NULL 更误导（网格按数值列排版，值却是空的）。
    /// 现在交给文本档，由 `sqlx_cell_as_text!` 按 `BigDecimal::to_string()` 原样取出，
    /// **标度保住**（`numeric(10,2)` 出 `"1234.5600"` 而不是 `1234.56` 的浮点近似）。
    #[test]
    fn decimal_is_not_a_float_rank() {
        assert_eq!(declared_numeric_rank("DECIMAL"), None);
        assert_eq!(declared_numeric_rank("NUMERIC"), None);
        assert_eq!(declared_numeric_rank("DECIMAL UNSIGNED"), None);
        assert_eq!(declared_numeric_rank("NUMERIC UNSIGNED"), None);
        // 浮点族照旧
        assert_eq!(declared_numeric_rank("DOUBLE"), Some(3));
        assert_eq!(declared_numeric_rank("FLOAT"), Some(3));
    }
    use crate::driver::Database;

    const MYSQL_URL: &str = "mysql://root:root@localhost:3306/";

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_connect() {
        let db = MySqlDatabase::new(MYSQL_URL).await;
        assert!(db.is_ok(), "Failed to connect to MySQL: {:?}", db.err());
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_query_select_one() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let result = db.query("SELECT 1 AS val").await?;
        assert_eq!(result.columns, vec!["val"]);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_crud_roundtrip() -> Result<(), CoreError> {
        let setup_db = MySqlDatabase::new(MYSQL_URL).await?;
        setup_db
            .query("CREATE DATABASE IF NOT EXISTS _rd_test_db")
            .await?;

        let db_url = format!("{}_rd_test_db", MYSQL_URL);
        let db = MySqlDatabase::new(&db_url).await?;

        db.query("CREATE TABLE IF NOT EXISTS _rd_test (id INT PRIMARY KEY, name VARCHAR(100), value DOUBLE)")
            .await?;

        db.query("INSERT INTO _rd_test (id, name, value) VALUES (1, 'hello', 3.14)")
            .await?;

        let result = db
            .query("SELECT id, name, value FROM _rd_test WHERE id = 1")
            .await?;
        assert_eq!(result.columns, vec!["id", "name", "value"]);

        db.query("DROP TABLE IF EXISTS _rd_test").await?;
        setup_db
            .query("DROP DATABASE IF EXISTS _rd_test_db")
            .await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_error_handling() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let result = db.query("SELECT * FROM _non_existent_table_rd").await;
        assert!(result.is_err(), "应返回不存在的表错误");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_list_tables() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let tables = db.list_tables("mysql", None).await;
        assert!(tables.is_ok(), "list_tables 失败: {:?}", tables.err());
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_meta() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let meta = db.meta();
        assert!(meta.supports_transaction);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "需要运行中的 MySQL 服务"]
    async fn test_is_read_only_flag() -> Result<(), CoreError> {
        let db = MySqlDatabase::new(MYSQL_URL).await?;
        let result = db.query("SELECT 1").await?;
        assert_eq!(result.is_read_only, Some(true));
        Ok(())
    }
}
