use std::sync::Arc;

use crate::core::driver::traits::{
    ColumnDetail, ConstraintDetail, DynDatabase, IndexDetail, SchemaObject, SchemaObjectKind,
};
use crate::core::error::{ConnectionError, CoreError};
use crate::core::services::connection_manager::ConnectionManager;

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

    pub async fn list_catalogs(&self, conn_id: &str) -> Result<Vec<String>, CoreError> {
        let db = self.get_database(conn_id).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let nodes = browser.get_catalogs().await?;
            return Ok(nodes.into_iter().map(|n| n.name).collect());
        }
        db.list_catalogs().await
    }

    pub async fn list_schemas(
        &self,
        conn_id: &str,
        catalog: &str,
    ) -> Result<Vec<String>, CoreError> {
        let db = self.get_database(conn_id).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let nodes = browser.get_schemas(catalog).await?;
            return Ok(nodes.into_iter().map(|n| n.name).collect());
        }
        db.list_schemas(catalog).await
    }

    pub async fn list_tables(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let db = self.get_database(conn_id).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let nodes = browser.get_tables(catalog, schema).await?;
            return Ok(nodes
                .into_iter()
                .map(|n| SchemaObject {
                    name: n.name,
                    kind: n.kind,
                    children: None,
                    comment: n.comment,
                    table_name: None,
                    event: None,
                })
                .collect());
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
        let db = self.get_database(conn_id).await?;
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
        let db = self.get_database(conn_id).await?;
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
        let db = self.get_database(conn_id).await?;
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
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let db = self.get_database(conn_id).await?;
        db.list_procedures(catalog, Some(schema)).await
    }

    pub async fn list_functions(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let db = self.get_database(conn_id).await?;
        db.list_functions(catalog, Some(schema)).await
    }

    pub async fn list_sequences(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let db = self.get_database(conn_id).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let nodes = browser.get_sequences(catalog, schema).await?;
            return Ok(nodes
                .into_iter()
                .map(|n| SchemaObject {
                    name: n.name,
                    kind: n.kind,
                    children: None,
                    comment: n.comment,
                    table_name: None,
                    event: None,
                })
                .collect());
        }
        db.list_sequences(catalog, Some(schema)).await
    }

    pub async fn list_triggers(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        let db = self.get_database(conn_id).await?;
        if let Some(browser) = db.as_metadata_browser() {
            let nodes = browser.get_triggers(catalog, schema).await?;
            return Ok(nodes
                .into_iter()
                .map(|n| SchemaObject {
                    name: n.name,
                    kind: n.kind,
                    children: None,
                    comment: n.comment,
                    table_name: None,
                    event: None,
                })
                .collect());
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
        let db = self.get_database(conn_id).await?;
        db.get_routine_source(catalog, Some(schema), name, kind)
            .await
    }
}
