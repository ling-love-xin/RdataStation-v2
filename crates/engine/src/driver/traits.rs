use std::sync::Arc;

use shared::error::{CoreError, DatabaseError};
use shared::models::{QueryResult, Value};

// 数据库对象元数据 DTO 已下沉 `shared::meta`（engine 瘦身第二刀前置小手术）：
// persistence::metadata_cache 与 driver 两侧共用，此处重导出保持路径不变。
pub use shared::meta::{
    ColumnDetail, ConstraintDetail, ForeignKeyRef, IndexDetail, NodeDetail, NodeInfo,
    SchemaObjectKind, SchemaObjects,
};

/// 元数据浏览器 trait
///
/// 提供统一的对象树导航能力，适用于所有数据库类型（关系型、NoSQL、图等）。
/// 与 Database trait 分离，支持按需实现。
///
/// ## 树层级（SQL 标准）
/// Server（连接本身）→ Catalog → Schema → Table/View/Procedure/Function
#[async_trait::async_trait]
pub trait MetadataBrowser: Send + Sync {
    /// 获取 Catalog 列表（SQL 标准顶层容器）
    ///
    /// PostgreSQL 返回真实 Catalog（数据库级别），MySQL 返回 Schema 列表
    /// （MySQL 的 database = schema），SQLite/DuckDB 返回固定值。
    async fn get_catalogs(&self) -> Result<Vec<NodeInfo>, CoreError>;

    /// 该数据源是否存在独立的 Schema 层级。
    ///
    /// 返回 `false` 时导航树跳过 Schema 层，Catalog 直接承载类别文件夹
    /// （MySQL 的 database 即 schema；SQLite / DuckDB 单库场景）。
    /// 默认 `true`，未知驱动保底保留 Schema 层。
    fn has_schema_level(&self) -> bool {
        true
    }

    /// 获取 Schema 列表
    ///
    /// 仅当 [`Self::has_schema_level`] 为 `true` 时才有意义；无 Schema 层的
    /// 驱动应返回空列表（而非回退为 Catalog 列表，否则导航树会出现同名重复层）。
    async fn get_schemas(&self, catalog: &str) -> Result<Vec<NodeInfo>, CoreError>;

    /// 获取表/视图/集合列表
    async fn get_tables(&self, catalog: &str, schema: &str) -> Result<Vec<NodeInfo>, CoreError>;

    /// 获取表/视图详情（含列信息）
    async fn get_table_detail(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<NodeDetail, CoreError>;

    /// 获取表的索引列表
    async fn get_indexes(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<IndexDetail>, CoreError> {
        let _ = (catalog, schema, table);
        Ok(vec![])
    }

    /// 获取表的约束列表（主键/外键/唯一约束等）
    async fn get_constraints(
        &self,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ConstraintDetail>, CoreError> {
        let _ = (catalog, schema, table);
        Ok(vec![])
    }

    /// 获取 Schema 的序列列表
    async fn get_sequences(
        &self,
        _catalog: &str,
        _schema: &str,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(vec![])
    }

    /// 获取 Schema 的触发器列表
    async fn get_triggers(
        &self,
        _catalog: &str,
        _schema: &str,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(vec![])
    }

    // ---- S5-A① 对象类别能力位（导航据此短路内省，免一次无谓查询）----
    //
    // 默认全 `true`（A② 能力降级模式：未知驱动保底保留全部类别，不藏对象）；
    // 明确不支持的驱动覆写为 `false`。文件夹在树上**仍然固定显示**（用户决策
    // 2026-10-08：所有类别固定显示，点击空类别显示空态）——能力位只省查询，
    // 不做显隐。
    //
    // 为什么落在 `MetadataBrowser` 而不是 `DataSourceMeta`：导航消费的是「这个
    // 连接跑起来的内省能力」，与驱动静态 meta 分开（连接可用性 / 指纹影响前者）。

    /// 是否支持视图内省（`get_tables` 已含视图时可忽略本位，默认 `true`）。
    fn supports_views(&self) -> bool {
        true
    }

    /// 是否支持存储过程 / 函数内省（`get_routines`）。
    fn supports_routines(&self) -> bool {
        true
    }

    /// 是否支持序列内省（`get_sequences`）。
    fn supports_sequences(&self) -> bool {
        true
    }

    /// 是否支持触发器内省（`get_triggers`）。
    fn supports_triggers(&self) -> bool {
        true
    }

    /// 这条连接是否**只覆盖一个 catalog**。
    ///
    /// `true` 的驱动（PostgreSQL：一条连接绑定一个数据库，`information_schema` 只看得到当前库）：
    /// 展开服务器上**别的库**时，上层要另开一条连到那个库的连接 —— 见
    /// `ConnectionManager::get_scoped_connection`。
    ///
    /// `false`（默认）的驱动一条连接就能看全所有 catalog：MySQL（catalog = database，
    /// `information_schema` 跨库可见）、SQLite / DuckDB（单库）。它们不会走跨库那条路。
    fn catalog_is_connection_scoped(&self) -> bool {
        false
    }

    /// 这条连接**当前绑的库**（只有 [`catalog_is_connection_scoped`] 为真时有意义）。
    ///
    /// 用途：区分「展开的是不是自己这个库」—— 是就用主连接，不是才另开（少一条连接）。
    /// 默认 `None` = 不适用。
    ///
    /// [`catalog_is_connection_scoped`]: MetadataBrowser::catalog_is_connection_scoped
    async fn current_catalog(&self) -> Option<String> {
        None
    }
}

/// 数据源能力描述
///
/// 描述数据库支持的特性，用于运行时能力检测。
///
/// **按驱动实现给，不按数据库族给**：同一个库的 sqlx 版与官方版是两套客户端库，
/// 能力/参数/限制都可能不同（已知实例：TLS 参数词汇与证书格式，见
/// `docs/architecture/driver-capability-matrix.md` §2.1）。
/// 因此下面每个构造器对应**一个驱动 id**，各驱动的 `meta()` 只用自己的那一份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataSourceMeta {
    /// 数据库服务器版本
    pub server_version: Option<String>,
    /// 是否支持事务
    pub supports_transaction: bool,
    /// 是否支持流式查询（大数据集分批返回）
    pub supports_streaming: bool,
    /// 是否支持 Arrow 格式（用于插件通信）
    pub supports_arrow: bool,
    /// 是否支持联邦查询（跨库查询）
    pub supports_federated: bool,
    /// 是否支持并发写入
    pub supports_concurrent_write: bool,
    /// 是否为内存数据库
    pub is_in_memory: bool,
}

impl DataSourceMeta {
    /// MySQL（sqlx 实现）
    pub fn mysql() -> Self {
        Self {
            server_version: None,
            supports_transaction: true,
            supports_streaming: true,
            supports_arrow: false,
            supports_federated: false,
            supports_concurrent_write: true,
            is_in_memory: false,
        }
    }

    /// MySQL（官方实现 mysql_async）
    ///
    /// 目前与 sqlx 版同值（真机上两者的事务/取消/取数都已跑通）；
    /// **单独一份是为了让差异可表达**：将来哪一项只在一边成立，改这里 + 加用例即可，
    /// 不需要再去拆“族”的概念（能力键与运行时位的对应见 `driver::capability`）。
    pub fn mysql_native() -> Self {
        Self::mysql()
    }

    /// PostgreSQL 元数据
    pub fn postgres() -> Self {
        Self {
            server_version: None,
            supports_transaction: true,
            supports_streaming: true,
            supports_arrow: false,
            supports_federated: false,
            supports_concurrent_write: true,
            is_in_memory: false,
        }
    }

    /// PostgreSQL（官方实现 tokio-postgres）
    ///
    /// 同 MySQL 两版：目前与 sqlx 版同值，单列一份以便表达差异。
    pub fn postgres_native() -> Self {
        Self::postgres()
    }

    /// SQLite 元数据
    pub fn sqlite() -> Self {
        Self {
            server_version: None,
            supports_transaction: true,
            supports_streaming: false,
            supports_arrow: false,
            supports_federated: false,
            supports_concurrent_write: false,
            is_in_memory: false,
        }
    }

    /// DuckDB 元数据
    pub fn duckdb() -> Self {
        Self {
            server_version: None,
            supports_transaction: true,
            supports_streaming: true,
            supports_arrow: true,
            supports_federated: true,
            supports_concurrent_write: true,
            is_in_memory: false,
        }
    }
}

/// 数据库事务
#[async_trait::async_trait]
pub trait Transaction: Send + Sync {
    /// 执行查询
    async fn query(&mut self, sql: &str) -> Result<QueryResult, CoreError>;

    /// 提交事务
    async fn commit(&mut self) -> Result<(), CoreError>;

    /// 回滚事务
    async fn rollback(&mut self) -> Result<(), CoreError>;
}

/// 数据库抽象接口
#[async_trait::async_trait]
pub trait Database: Send + Sync {
    /* ===== 核心查询能力 ===== */

    /// 执行查询
    async fn query(&self, sql: &str) -> Result<QueryResult, CoreError>;

    /// 执行参数化查询（防止 SQL 注入）
    ///
    /// 所有原生驱动均已实现真正的 prepared statement：
    /// - MySQL: sqlx::query(sql).bind(param)
    /// - PostgreSQL: sqlx::query(sql).bind(param) ($1/$2 占位符)
    /// - SQLite: conn.prepare(sql).query(params)
    /// - DuckDB: conn.prepare(sql).query(params)
    ///
    /// 默认实现回退到普通查询，仅用于 stub/WASM/JDBC 驱动
    async fn query_with_params(
        &self,
        sql: &str,
        _params: Vec<Value>,
    ) -> Result<QueryResult, CoreError> {
        // 子类应覆盖此方法以支持真正的参数化查询
        self.query(sql).await
    }

    /// 执行可取消的查询
    async fn query_with_cancel(
        &self,
        sql: &str,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Result<QueryResult, CoreError>;

    /// 开始事务
    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>, CoreError>;

    /// 获取数据源元数据
    fn meta(&self) -> DataSourceMeta;

    /// 连接健康检查（ping）
    ///
    /// 执行轻量级查询验证连接是否存活。
    /// 默认返回 Ok(())，驱动可覆盖实现真正的 ping。
    async fn ping(&self) -> Result<(), CoreError> {
        Ok(())
    }

    /// 尝试将 self 转型为 MetadataBrowser，用于统一元数据浏览路径
    ///
    /// 默认返回 None（不支持 MetadataBrowser 的驱动）。
    /// 实现了 MetadataBrowser 的驱动（MySQL/PostgreSQL/DuckDB/SQLite）应覆盖此方法返回 Some(self)。
    fn as_metadata_browser(&self) -> Option<&dyn MetadataBrowser> {
        None
    }

    /// 获取连接池状态（仅池化数据库支持）
    ///
    /// 返回连接池的运行时指标。非池化数据库（如 SQLite/DuckDB 单连接）返回 None。
    async fn pool_status(&self) -> Option<PoolStatus> {
        None
    }

    /* ===== 对象树能力（Schema 浏览） ===== */

    /// 列举 Catalog（SQL 标准顶层容器）
    async fn list_catalogs(&self) -> Result<Vec<String>, CoreError> {
        Ok(vec![])
    }

    /// 列举 schema（SQLite 可返回空）
    async fn list_schemas(&self, _catalog: &str) -> Result<Vec<String>, CoreError> {
        Ok(vec![])
    }

    /// 列举表 / 视图
    ///
    /// 与 [`MetadataBrowser::get_tables`] 返回同一类型：实现了浏览器的驱动直接转发，
    /// 只实现本方法的驱动（自定义 / 桥接驱动）也不必再降级成另一套结构。
    async fn list_tables(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(vec![])
    }

    /// 列举列
    async fn list_columns(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        _table: &str,
    ) -> Result<Vec<ColumnDetail>, CoreError> {
        Ok(vec![])
    }

    /// 批量列举列（2026-10-09，列预热全量化）。
    ///
    /// 默认实现循环 `list_columns`（本地驱动无建连成本，直接可用）；
    /// 网络驱动（sqlx PG/MySQL）覆盖为**复用常驻连接 + 循环列 SQL**
    /// （`lock_pg_conn`/`lock_mysql_conn` 内置 100ms 快速探活，健康复用、
    /// 冻结即弃重建）——大 schema（如 public 24 表）一次建连全部列，
    /// 不再每表新建连接（24 次建连 = 7s+ 不可接受）。
    async fn list_columns_batch(
        &self,
        catalog: &str,
        schema: Option<&str>,
        tables: &[String],
    ) -> Result<Vec<(String, Vec<ColumnDetail>)>, CoreError> {
        let mut out = Vec::with_capacity(tables.len());
        for t in tables {
            let cols = self.list_columns(catalog, schema, t).await?;
            out.push((t.clone(), cols));
        }
        Ok(out)
    }

    /// 列举索引
    async fn list_indexes(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        _table: &str,
    ) -> Result<Vec<IndexDetail>, CoreError> {
        Ok(vec![])
    }

    /// 列举约束
    async fn list_constraints(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        _table: &str,
    ) -> Result<Vec<ConstraintDetail>, CoreError> {
        Ok(vec![])
    }

    /// 列举存储过程
    async fn list_procedures(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(vec![])
    }

    /// 列举函数
    async fn list_functions(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(vec![])
    }

    /// 列举序列
    async fn list_sequences(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(vec![])
    }

    /// 列举触发器
    async fn list_triggers(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        Ok(vec![])
    }

    /// **一次建连批量内省 schema 全部对象类别**（DBeaver 会话语义：连接生命周期 =
    /// 一次展开，用完即弃）。
    ///
    /// 2026-10-08 真机铁证：同连接上「同 SQL 的 prepared statement 二次执行」会挂起
    /// （biz_483 展开成功 3ms 后 biz_477 同连接 tables 查询挂 15s；app_run63 每次
    /// 新建连接=每次新 prepared 无红字，探针每次新建 10-82ms 全快）。批量方法 = 一条
    /// 新连接 + 每类对象各执行一次不同 SQL（无重复 prepared），展开结束连接即弃。
    ///
    /// 默认实现逐类别调用（无连接数优化，安全但慢）；PG/MySQL 覆盖为单连接批量。
    async fn introspect_schema_objects(
        &self,
        catalog: &str,
        schema: &str,
    ) -> Result<SchemaObjects, CoreError> {
        let tables = self.list_tables(catalog, Some(schema)).await?;
        let procedures = self.list_procedures(catalog, Some(schema)).await?;
        let functions = self.list_functions(catalog, Some(schema)).await?;
        let sequences = self.list_sequences(catalog, Some(schema)).await?;
        let triggers = self.list_triggers(catalog, Some(schema)).await?;
        Ok(SchemaObjects {
            tables,
            procedures,
            functions,
            sequences,
            triggers,
        })
    }

    /// 获取过程/函数的 DDL 源码
    ///
    /// 返回完整的 CREATE PROCEDURE/FUNCTION 语句。
    /// 不支持或不存在的 routine 返回 None。
    async fn get_routine_source(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        _name: &str,
        _kind: SchemaObjectKind, // Procedure 或 Function
    ) -> Result<Option<String>, CoreError> {
        Ok(None) // 默认：不支持
    }

    /// 取**源版** DDL：该库自己保存的那一份 `CREATE` 原文。
    ///
    /// `None` = **该库给不出来**（没有等价语句），**不是**「对象不存在」——
    /// 后者是 `Err`。上层据此决定要不要退化成合成（`rds_database::sql_gen`）。
    ///
    /// 谁能给（2026-09-21 真机确认）：
    /// * MySQL / MySQL(Official)：`SHOW CREATE TABLE`（对**视图**同样有效，返回
    ///   `Create View` 列 —— 不必再分一套 `SHOW CREATE VIEW`）；
    /// * SQLite：`sqlite_master.sql`（表与视图都是用户写下的原文）；
    /// * DuckDB：`duckdb_tables()` / `duckdb_views()` 的 `sql` 列。
    ///
    /// 谁给不出来：**PostgreSQL**（没有 `SHOW CREATE TABLE` 的等价物，要拿原文得走
    /// `pg_get_*def` 拼装 —— 那已经算「合成」，不如交给上层那份统一的合成器）。
    ///
    /// 为什么源版更值钱：合成器吃不下的东西（存储引擎 / 字符集与排序规则 / 分区 /
    /// 表级选项 / `CHECK` 表达式 / 视图定义）在原文里**本来就有**。
    async fn get_table_ddl(
        &self,
        _catalog: &str,
        _schema: Option<&str>,
        _table: &str,
    ) -> Result<Option<String>, CoreError> {
        Ok(None) // 默认：该库没有源版
    }

    /* ===== 联邦查询能力 ===== */

    /// 注册外部数据库连接
    async fn register_external_database(
        &self,
        _name: &str,
        _driver: &str,
        _connection_string: &str,
    ) -> Result<(), CoreError> {
        Err(CoreError::database(DatabaseError::Driver {
            db_type: "generic".to_string(),
            operation: "register_external_database".to_string(),
            source: "Not supported".to_string(),
        }))
    }

    /// 创建外部表
    async fn create_external_table(
        &self,
        _external_db_name: &str,
        _schema_name: &str,
        _table_name: &str,
        _external_table_name: &str,
    ) -> Result<(), CoreError> {
        Err(CoreError::database(DatabaseError::Driver {
            db_type: "generic".to_string(),
            operation: "create_external_table".to_string(),
            source: "Not supported".to_string(),
        }))
    }
}

/// 动态数据库类型
pub type DynDatabase = Arc<dyn Database + Send + Sync>;

/// 数据库连接池抽象接口
///
/// 统一不同数据库驱动的连接池管理，支持：
/// - sqlx (MySQL/PostgreSQL)
/// - rusqlite (SQLite)
/// - duckdb (DuckDB)
/// - 未来: JDBC/ODBC 桥接
#[async_trait::async_trait]
pub trait DbPool: Send + Sync {
    /// 从连接池获取一个数据库连接
    ///
    /// # Returns
    ///
    /// 返回一个实现了 Database trait 的连接
    async fn acquire(&self) -> Result<Box<dyn Database + Send + Sync>, CoreError>;

    /// 关闭连接池，释放所有资源
    async fn close(&self) -> Result<(), CoreError>;

    /// 检查连接池是否已关闭
    fn is_closed(&self) -> bool;

    /// 获取连接池状态信息
    fn status(&self) -> PoolStatus;
}

/// 连接池状态
#[derive(Debug, Clone)]
pub struct PoolStatus {
    /// 连接池大小
    pub size: usize,
    /// 空闲连接数
    pub idle: usize,
    /// 活跃连接数
    pub active: usize,
    /// 等待获取连接的请求数
    pub waiting: usize,
    /// 最大连接数
    pub max_connections: usize,
    /// 最小连接数
    pub min_connections: usize,
}

impl PoolStatus {
    /// 创建未知状态（用于不支持状态查询的驱动）
    pub fn unknown() -> Self {
        Self {
            size: 0,
            idle: 0,
            active: 0,
            waiting: 0,
            max_connections: 10,
            min_connections: 2,
        }
    }
}

#[cfg(test)]
mod metadata_browser_capability_tests {
    //! S5-A①/A②：能力位默认值（保底全开）+ 覆写路径。
    //!
    //! 新驱动接入时只覆写「明确不支持」的类别；不覆写 = 全部支持（保底，不藏对象）。

    use super::*;
    use crate::driver::NodeInfo;

    struct DummyBrowser;

    #[async_trait::async_trait]
    impl MetadataBrowser for DummyBrowser {
        async fn get_catalogs(&self) -> Result<Vec<NodeInfo>, CoreError> {
            Ok(vec![])
        }
        async fn get_schemas(&self, _catalog: &str) -> Result<Vec<NodeInfo>, CoreError> {
            Ok(vec![])
        }
        async fn get_tables(
            &self,
            _catalog: &str,
            _schema: &str,
        ) -> Result<Vec<NodeInfo>, CoreError> {
            Ok(vec![])
        }
        async fn get_table_detail(
            &self,
            _catalog: &str,
            _schema: &str,
            _table: &str,
        ) -> Result<NodeDetail, CoreError> {
            Ok(NodeDetail {
                node: NodeInfo::new(_table, SchemaObjectKind::Table),
                columns: vec![],
                index_count: None,
                row_count_estimate: None,
            })
        }
    }

    #[test]
    fn default_capabilities_are_all_true() {
        let b = DummyBrowser;
        assert!(b.supports_views(), "保底不藏对象：视图默认支持");
        assert!(b.supports_routines(), "保底不藏对象：例程默认支持");
        assert!(b.supports_sequences(), "保底不藏对象：序列默认支持");
        assert!(b.supports_triggers(), "保底不藏对象：触发器默认支持");
    }
}
