use std::sync::Arc;

use engine::driver::traits::{
    ColumnDetail, ConstraintDetail, DynDatabase, IndexDetail, NodeInfo, SchemaObjectKind,
};
use shared::error::{ConnectionError, CoreError};
use engine::connection_manager::ConnectionManager;

pub struct MetadataService {
    manager: Arc<ConnectionManager>,
}

impl MetadataService {
    pub fn new(manager: Arc<ConnectionManager>) -> Self {
        Self { manager }
    }

    async fn get_database(&self, conn_id: &str) -> Result<DynDatabase, CoreError> {
        self.manager
            .get_connection(&conn_id.to_string())
            .await
            .ok_or_else(|| CoreError::connection(ConnectionError::not_found(conn_id)))
    }

    /// 取「能访问该 catalog 的连接」。
    ///
    /// 两种情况：
    /// * **一条连接覆盖全部 catalog 的驱动**（MySQL：catalog = database；SQLite / DuckDB：单库）
    ///   → 直接用主连接（`catalog_is_connection_scoped()` 为假，这里什么都不做）；
    /// * **PostgreSQL**：一条连接只绑一个库 → 展开别的库时向管理器要一条**连到那个库**的连接
    ///   （按需建、按 `(conn_id, 库名)` 缓存复用、主连接断开时一起关掉）。
    ///
    /// 「是不是自己那个库」由驱动给的 `current_catalog()` 判断（每个连接只查一次，结果缓存在
    /// 管理器里）—— 是自己的库就用主连接，省掉一条连接。
    ///
    /// 为什么不让数据库自行切换（`SET search_path` / `CONNECT` 之类）：PG 的跨库是**连接级**
    /// 的（一条连接只能在一个库里查），换库只能另开连接；而 `dblink` / `postgres_fdw` 那类
    /// 要服务端装扩展，不是默认能力 —— 不引。
    async fn database_for(&self, conn_id: &str, catalog: &str) -> Result<DynDatabase, CoreError> {
        let db = self.get_database(conn_id).await?;
        if catalog.is_empty() {
            return Ok(db);
        }
        let Some(browser) = db.as_metadata_browser() else {
            // 没有浏览器（桥接驱动）的对象内省本来就走 list_* 回退，不涉及跨库
            return Ok(db);
        };
        if !browser.catalog_is_connection_scoped() {
            return Ok(db);
        }

        let key = conn_id.to_string();
        let current = match self.manager.cached_current_catalog(&key).await {
            Some(known) => Some(known),
            None => {
                let probed = browser.current_catalog().await;
                if let Some(name) = probed.as_deref() {
                    self.manager.remember_current_catalog(&key, name).await;
                }
                probed
            }
        };
        if current.as_deref() == Some(catalog) {
            return Ok(db);
        }
        self.manager.get_scoped_connection(&key, catalog).await
    }

    pub async fn list_catalogs(&self, conn_id: &str) -> Result<Vec<String>, CoreError> {
        let db = self.get_database(conn_id).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let nodes = browser.get_catalogs().await?;
            return Ok(nodes.into_iter().map(|n| n.name).collect());
        }
        db.list_catalogs().await
    }

    /// 该连接是否存在独立的 Schema 层级（供导航树决定是否跳过 Schema 层）。
    ///
    /// 未知驱动保底返回 `true`。
    pub async fn has_schema_level(&self, conn_id: &str) -> Result<bool, CoreError> {
        let db = self.get_database(conn_id).await?;
        Ok(db
            .as_metadata_browser()
            .map(|b| b.has_schema_level())
            .unwrap_or(true))
    }

    pub async fn list_schemas(
        &self,
        conn_id: &str,
        catalog: &str,
    ) -> Result<Vec<String>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let nodes = browser.get_schemas(catalog).await?;
            return Ok(nodes.into_iter().map(|n| n.name).collect());
        }
        db.list_schemas(catalog).await
    }

    /// 列举表 / 视图。
    ///
    /// 浏览器与回退返回**同一套结构对象**（`NodeInfo`），所以这里是纯转发——
    /// 2026-09-19 之前两边各一套类型，中间要写一遍丢掉字段的映射。
    pub async fn list_tables(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        if let Some(browser) = db.as_metadata_browser() {
            return browser.get_tables(catalog, schema).await;
        }
        db.list_tables(catalog, Some(schema)).await
    }

    pub async fn list_columns(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnDetail>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let detail = browser.get_table_detail(catalog, schema, table).await?;
            return Ok(detail.columns);
        }
        db.list_columns(catalog, Some(schema), table).await
    }

    pub async fn list_indexes(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<IndexDetail>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        if let Some(browser) = db.as_metadata_browser() {
            return browser.get_indexes(catalog, schema, table).await;
        }
        db.list_indexes(catalog, Some(schema), table).await
    }

    pub async fn list_constraints(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ConstraintDetail>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        if let Some(browser) = db.as_metadata_browser() {
            return browser.get_constraints(catalog, schema, table).await;
        }
        db.list_constraints(catalog, Some(schema), table).await
    }

    pub async fn list_procedures(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        db.list_procedures(catalog, Some(schema)).await
    }

    pub async fn list_functions(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        db.list_functions(catalog, Some(schema)).await
    }

    pub async fn list_sequences(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let nodes = browser.get_sequences(catalog, schema).await?;
            if !nodes.is_empty() {
                return Ok(nodes);
            }
            // 浏览器层返回空：可能是 trait 默认实现（未支持）而非真的没有序列。
            // 回退 `Database::list_sequences`——否则驱动的真实实现会被默认空实现遮蔽
            // （PostgreSQL 即如此，「序列」文件夹永不出现）。
        }
        db.list_sequences(catalog, Some(schema)).await
    }

    pub async fn list_triggers(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let nodes = browser.get_triggers(catalog, schema).await?;
            if !nodes.is_empty() {
                return Ok(nodes);
            }
            // 同上：回退 `Database::list_triggers`（PostgreSQL 有真实实现，曾被遮蔽）。
        }
        db.list_triggers(catalog, Some(schema)).await
    }

    pub async fn get_routine_source(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        name: &str,
        kind: SchemaObjectKind,
    ) -> Result<Option<String>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        db.get_routine_source(catalog, Some(schema), name, kind)
            .await
    }

    /// 取**源版** DDL（该库保存的原文）。
    ///
    /// `None` = 该库给不出来（PostgreSQL 没有 `SHOW CREATE TABLE` 的等价物）——
    /// 由调用方退化成合成（`sql_gen::create_table_ddl`），见 `Database::get_table_ddl`。
    pub async fn get_table_ddl(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Option<String>, CoreError> {
        let db = self.database_for(conn_id, catalog).await?;
        db.get_table_ddl(catalog, Some(schema), table).await
    }
}
