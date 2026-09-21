use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;

use shared::error::{CoreError, DatabaseError};
use shared::models::{QueryResult, Value};

/// Schema 对象类型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub enum SchemaObjectKind {
    Catalog,
    Schema,
    Table,
    View,
    Column,
    Index,
    PrimaryKey,
    ForeignKey,
    Procedure,
    Function,
    Sequence,
    Trigger,
}

/// 列所引用的外键目标。
///
/// 为什么单独一个类型而不是拼成 `"表.列"` 字符串：表名与列名都可能含点（带引号的标识符），
/// 拼串之后就再也分不回来了；而且引用的两个端点各自都是独立的展示位。
/// 取法照 dbui 的 `Column.references`（MIT）——它也是把「引到哪」放在**列上**，
/// 而不是只能从约束里反推。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ForeignKeyRef {
    /// 被引用的表名（同 schema 内不带 schema 前缀，与驱动内省口径一致）。
    pub table: String,
    /// 被引用的列名。
    pub column: String,
}

/// 列详情（完整元数据）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ColumnDetail {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    /// 这一列是否参与外键。
    ///
    /// 与 [`ColumnDetail::references`] 的分工：本字段是「**有没有**」，`references` 是
    /// 「**引到哪**」。保留本字段是因为桥接驱动（JDBC 那类）可能只知道有没有、拿不到目标；
    /// 能拿到目标时，两者应当一致（见 `references` 的文档）。
    pub is_foreign_key: bool,
    /// 引用的目标；`None` = 不引用任何表。
    ///
    /// **与 `is_foreign_key` 的关系**：能拿到目标时 `references.is_some()` 必须等于
    /// `is_foreign_key`；拿不到目标（驱动不支持）时 `references` 为 `None` 而
    /// `is_foreign_key` 可以仍为 `true`。不要在 UI 里把 `references.is_none()` 读成
    /// 「不是外键」——请读 `is_foreign_key`。
    #[serde(default)]
    pub references: Option<ForeignKeyRef>,
    /// 列在表中的序号（**1 基**，与 `information_schema.columns.ordinal_position` 同口径）。
    ///
    /// 0 = 驱动没给。有了它，UI 不必再拿数组下标当序号（筛选 / 分页 / 重排后下标会错位）。
    #[serde(default)]
    pub ordinal: u32,
    pub default_value: Option<String>,
    pub comment: Option<String>,
    #[serde(default)]
    pub extra: std::collections::HashMap<String, String>,
}

/// 索引详情
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct IndexDetail {
    pub name: String,
    pub table_name: String,
    pub column_names: Vec<String>,
    pub is_unique: bool,
    pub is_primary: bool,
    #[serde(default)]
    pub index_type: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

/// 约束详情
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ConstraintDetail {
    pub name: String,
    pub table_name: String,
    pub constraint_type: String,
    pub column_names: Vec<String>,
    #[serde(default)]
    pub referenced_table: Option<String>,
    #[serde(default)]
    pub referenced_columns: Vec<String>,
    #[serde(default)]
    pub update_rule: Option<String>,
    #[serde(default)]
    pub delete_rule: Option<String>,
}

/// 对象列表项（对象树 / 元数据缓存 / 属性面板共用的**唯一**结构对象表示）
///
/// 取代了 v1 的 `SchemaObject`。那个类型多带三个字段，全都是死字段：
/// `children`（懒加载整棵树）无人读取、`table_name` / `event`（触发器专有）无人填充，
/// 而 5 个原生驱动的 `list_tables` 都在做「`NodeInfo` 降级成 `SchemaObject`」的空转。
/// 一个对象只留一份表示：名字 + 类别 + 注释。
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NodeInfo {
    pub name: String,
    pub kind: SchemaObjectKind,
    pub comment: Option<String>,
    /// 关联父对象（触发器的所属表；其他类别为空——列的父对象由路径决定，不在这里）。
    ///
    /// 不演成任意 KV 背包：只放驱动内省时**已经查到**、界面真会显示的那一项。
    pub parent_name: Option<String>,
}

impl NodeInfo {
    /// 只带身份的对象项（绝大多数内省点只需要名字与类别）。
    pub fn new(name: impl Into<String>, kind: SchemaObjectKind) -> Self {
        Self {
            name: name.into(),
            kind,
            comment: None,
            parent_name: None,
        }
    }

    /// 带注释。
    pub fn with_comment(mut self, comment: Option<String>) -> Self {
        self.comment = comment;
        self
    }

    /// 带关联父对象（目前只有触发器用：所属表）。
    pub fn with_parent(mut self, parent: impl Into<String>) -> Self {
        let parent = parent.into();
        self.parent_name = (!parent.is_empty()).then_some(parent);
        self
    }
}

/// 对象详情（完整元数据，按需加载）
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct NodeDetail {
    pub node: NodeInfo,
    pub columns: Vec<ColumnDetail>,
    pub index_count: Option<u32>,
    pub row_count_estimate: Option<u32>,
}

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
