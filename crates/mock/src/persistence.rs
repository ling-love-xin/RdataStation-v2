use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;

use engine::persistence::project_db::{ProjectSqlitePool, SqlitePoolConnection};
use shared::error::{CommonError, CoreError, StorageError};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockGenerationTask {
    pub id: String,
    pub table_name: String,
    pub table_alias: Option<String>,
    pub row_count: i32,
    pub seed: Option<i32>,
    pub locale: String,
    pub scene_id: Option<String>,
    pub save_format: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub generated_rows: Option<i32>,
    pub generation_time_ms: Option<i32>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockGenerationColumn {
    pub id: String,
    pub task_id: String,
    pub column_name: String,
    pub column_type: String,
    pub generator: String,
    pub generator_params: Option<String>,
    pub null_ratio: f64,
    pub is_unique: bool,
    pub is_primary_key: bool,
    pub is_foreign_key: bool,
    pub ref_table: Option<String>,
    pub ref_column: Option<String>,
    pub comment: Option<String>,
    pub confidence: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockUserTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub row_count: i32,
    pub seed: Option<i32>,
    pub locale: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockTemplateColumn {
    pub id: String,
    pub template_id: String,
    pub column_name: String,
    pub column_type: String,
    pub generator: String,
    pub generator_params: Option<String>,
    pub null_ratio: f64,
    pub is_unique: bool,
    pub is_primary_key: bool,
    pub is_foreign_key: bool,
    pub ref_table: Option<String>,
    pub ref_column: Option<String>,
    pub comment: Option<String>,
    pub confidence: Option<String>,
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MockGenerationDetail {
    pub task: MockGenerationTask,
    pub columns: Vec<MockGenerationColumn>,
}

fn storage_err(store: &str, operation: &str, reason: String) -> CoreError {
    CoreError::storage(StorageError::Persistence {
        store: store.to_string(),
        operation: operation.to_string(),
        reason,
    })
}

pub struct MockGenerationStore {
    pool: Arc<ProjectSqlitePool>,
}

impl MockGenerationStore {
    pub fn new(pool: Arc<ProjectSqlitePool>) -> Self {
        Self { pool }
    }

    async fn get_conn(&self) -> Result<SqlitePoolConnection, CoreError> {
        self.pool.acquire().await
    }

    fn task_from_row(row: &rusqlite::Row) -> rusqlite::Result<MockGenerationTask> {
        Ok(MockGenerationTask {
            id: row.get("id")?,
            table_name: row.get("table_name")?,
            table_alias: row.get("table_alias")?,
            row_count: row.get("row_count")?,
            seed: row.get("seed")?,
            locale: row.get("locale")?,
            scene_id: row.get("scene_id")?,
            save_format: row.get("save_format")?,
            status: row.get("status")?,
            error_message: row.get("error_message")?,
            generated_rows: row.get("generated_rows")?,
            generation_time_ms: row.get("generation_time_ms")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    fn column_from_row(row: &rusqlite::Row) -> rusqlite::Result<MockGenerationColumn> {
        Ok(MockGenerationColumn {
            id: row.get("id")?,
            task_id: row.get("task_id")?,
            column_name: row.get("column_name")?,
            column_type: row.get("column_type")?,
            generator: row.get("generator")?,
            generator_params: row.get("generator_params")?,
            null_ratio: row.get("null_ratio")?,
            is_unique: row.get::<_, i32>("is_unique")? != 0,
            is_primary_key: row.get::<_, i32>("is_primary_key")? != 0,
            is_foreign_key: row.get::<_, i32>("is_foreign_key")? != 0,
            ref_table: row.get("ref_table")?,
            ref_column: row.get("ref_column")?,
            comment: row.get("comment")?,
            confidence: row.get("confidence")?,
            sort_order: row.get("sort_order")?,
        })
    }

    fn template_from_row(row: &rusqlite::Row) -> rusqlite::Result<MockUserTemplate> {
        Ok(MockUserTemplate {
            id: row.get("id")?,
            name: row.get("name")?,
            description: row.get("description")?,
            row_count: row.get("row_count")?,
            seed: row.get("seed")?,
            locale: row.get("locale")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
        })
    }

    fn template_column_from_row(row: &rusqlite::Row) -> rusqlite::Result<MockTemplateColumn> {
        Ok(MockTemplateColumn {
            id: row.get("id")?,
            template_id: row.get("template_id")?,
            column_name: row.get("column_name")?,
            column_type: row.get("column_type")?,
            generator: row.get("generator")?,
            generator_params: row.get("generator_params")?,
            null_ratio: row.get("null_ratio")?,
            is_unique: row.get::<_, i32>("is_unique")? != 0,
            is_primary_key: row.get::<_, i32>("is_primary_key")? != 0,
            is_foreign_key: row.get::<_, i32>("is_foreign_key")? != 0,
            ref_table: row.get("ref_table")?,
            ref_column: row.get("ref_column")?,
            comment: row.get("comment")?,
            confidence: row.get("confidence")?,
            sort_order: row.get("sort_order")?,
        })
    }

    pub async fn save_task(
        &self,
        task: &MockGenerationTask,
        columns: &[MockGenerationColumn],
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        // 事务：父行与 N 个子行分次写，中途失败会留下「有历史没列」的记录
        // （读回来是个不能生成的空配置，而提示还指向这次写失败——很难查）
        let tx = inner
            .unchecked_transaction()
            .map_err(|e| storage_err("mock_generation", "begin", e.to_string()))?;

        tx.execute(
            r#"INSERT INTO mock_generation_tasks (
                id, table_name, table_alias, row_count, seed, locale,
                scene_id, save_format, status, error_message,
                generated_rows, generation_time_ms, created_at, updated_at
            ) VALUES (
                ?, ?, ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?, ?
            )"#,
            rusqlite::params![
                &task.id,
                &task.table_name,
                &task.table_alias,
                task.row_count,
                task.seed,
                &task.locale,
                &task.scene_id,
                &task.save_format,
                &task.status,
                &task.error_message,
                task.generated_rows,
                task.generation_time_ms,
                // 不传就写 NULL：空串是「有一个空时间戳」，与「没有时间戳」在读取侧分不开。
                task.created_at.as_deref(),
                task.updated_at.as_deref(),
            ],
        )
        .map_err(|e| storage_err("mock_generation_tasks", "insert", e.to_string()))?;

        for col in columns {
            tx.execute(
                r#"INSERT INTO mock_generation_columns (
                    id, task_id, column_name, column_type, generator,
                    generator_params, null_ratio, is_unique, is_primary_key,
                    is_foreign_key, ref_table, ref_column, comment,
                    confidence, sort_order
                ) VALUES (
                    ?, ?, ?, ?, ?,
                    ?, ?, ?, ?,
                    ?, ?, ?, ?,
                    ?, ?
                )"#,
                rusqlite::params![
                    &col.id,
                    &col.task_id,
                    &col.column_name,
                    &col.column_type,
                    &col.generator,
                    &col.generator_params,
                    col.null_ratio,
                    col.is_unique as i32,
                    col.is_primary_key as i32,
                    col.is_foreign_key as i32,
                    &col.ref_table,
                    &col.ref_column,
                    &col.comment,
                    &col.confidence,
                    col.sort_order,
                ],
            )
            .map_err(|e| storage_err("mock_generation_columns", "insert", e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| storage_err("mock_generation", "commit", e.to_string()))?;
        Ok(())
    }

    pub async fn get_history(&self, limit: u32) -> Result<Vec<MockGenerationTask>, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;

        let mut stmt = inner
            .prepare(
                r#"SELECT id, table_name, table_alias, row_count, seed, locale,
                   scene_id, save_format, status, error_message,
                   generated_rows, generation_time_ms, created_at, updated_at
                   FROM mock_generation_tasks
                   ORDER BY created_at DESC LIMIT ?"#,
            )
            .map_err(|e| storage_err("mock_generation_tasks", "query", e.to_string()))?;

        let rows = stmt
            .query_map(rusqlite::params![limit], Self::task_from_row)
            .map_err(|e| storage_err("mock_generation_tasks", "query_map", e.to_string()))?;

        let mut tasks = Vec::new();
        for row in rows {
            match row {
                Ok(task) => tasks.push(task),
                Err(e) => return Err(storage_err("mock_generation_tasks", "row", e.to_string())),
            }
        }
        Ok(tasks)
    }

    pub async fn get_detail(&self, task_id: &str) -> Result<MockGenerationDetail, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;

        let task = inner
            .query_row(
                r#"SELECT id, table_name, table_alias, row_count, seed, locale,
                   scene_id, save_format, status, error_message,
                   generated_rows, generation_time_ms, created_at, updated_at
                   FROM mock_generation_tasks WHERE id = ?"#,
                rusqlite::params![task_id],
                Self::task_from_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::common(CommonError::General(format!("Task not found: {}", task_id)))
                }
                other => storage_err("mock_generation_tasks", "query_row", other.to_string()),
            })?;

        let mut stmt = inner
            .prepare(
                r#"SELECT id, task_id, column_name, column_type, generator,
                   generator_params, null_ratio, is_unique, is_primary_key,
                   is_foreign_key, ref_table, ref_column, comment,
                   confidence, sort_order
                   FROM mock_generation_columns
                   WHERE task_id = ? ORDER BY sort_order ASC"#,
            )
            .map_err(|e| storage_err("mock_generation_columns", "query", e.to_string()))?;

        let rows = stmt
            .query_map(rusqlite::params![task_id], Self::column_from_row)
            .map_err(|e| storage_err("mock_generation_columns", "query_map", e.to_string()))?;

        let mut columns = Vec::new();
        for row in rows {
            match row {
                Ok(col) => columns.push(col),
                Err(e) => return Err(storage_err("mock_generation_columns", "row", e.to_string())),
            }
        }

        Ok(MockGenerationDetail { task, columns })
    }

    pub async fn delete_task(&self, task_id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        conn.inner()?
            .execute(
                "DELETE FROM mock_generation_tasks WHERE id = ?",
                rusqlite::params![task_id],
            )
            .map_err(|e| storage_err("mock_generation_tasks", "delete", e.to_string()))?;
        Ok(())
    }

    pub async fn save_template(
        &self,
        template: &MockUserTemplate,
        columns: &[MockTemplateColumn],
    ) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;
        // 事务：模板头与列一次写完（同 `save_task`：中途失败不留半份）
        let tx = inner
            .unchecked_transaction()
            .map_err(|e| storage_err("mock_template", "begin", e.to_string()))?;

        tx.execute(
            r#"INSERT INTO mock_user_templates (
                    id, name, description, row_count, seed, locale, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#,
            rusqlite::params![
                &template.id,
                &template.name,
                &template.description,
                template.row_count,
                template.seed,
                &template.locale,
                // 同上：可空列保持 None，不要退化成空串。
                template.created_at.as_deref(),
                template.updated_at.as_deref(),
            ],
        )
        .map_err(|e| storage_err("mock_user_templates", "insert", e.to_string()))?;

        for col in columns {
            tx.execute(
                r#"INSERT INTO mock_template_columns (
                        id, template_id, column_name, column_type, generator,
                        generator_params, null_ratio, is_unique, is_primary_key,
                        is_foreign_key, ref_table, ref_column, comment,
                        confidence, sort_order
                    ) VALUES (
                        ?, ?, ?, ?, ?,
                        ?, ?, ?, ?,
                        ?, ?, ?, ?,
                        ?, ?
                    )"#,
                rusqlite::params![
                    &col.id,
                    &col.template_id,
                    &col.column_name,
                    &col.column_type,
                    &col.generator,
                    &col.generator_params,
                    col.null_ratio,
                    col.is_unique as i32,
                    col.is_primary_key as i32,
                    col.is_foreign_key as i32,
                    &col.ref_table,
                    &col.ref_column,
                    &col.comment,
                    &col.confidence,
                    col.sort_order,
                ],
            )
            .map_err(|e| storage_err("mock_template_columns", "insert", e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| storage_err("mock_template", "commit", e.to_string()))?;
        Ok(())
    }

    pub async fn get_templates(&self) -> Result<Vec<MockUserTemplate>, CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;

        let mut stmt = inner
            .prepare(
                r#"SELECT id, name, description, row_count, seed, locale, created_at, updated_at
                   FROM mock_user_templates ORDER BY created_at DESC"#,
            )
            .map_err(|e| storage_err("mock_user_templates", "query", e.to_string()))?;

        let rows = stmt
            .query_map([], Self::template_from_row)
            .map_err(|e| storage_err("mock_user_templates", "query_map", e.to_string()))?;

        let mut templates = Vec::new();
        for row in rows {
            match row {
                Ok(t) => templates.push(t),
                Err(e) => return Err(storage_err("mock_user_templates", "row", e.to_string())),
            }
        }
        Ok(templates)
    }

    pub async fn get_template_detail(
        &self,
        template_id: &str,
    ) -> Result<(MockUserTemplate, Vec<MockTemplateColumn>), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;

        let template = inner
            .query_row(
                r#"SELECT id, name, description, row_count, seed, locale, created_at, updated_at
                   FROM mock_user_templates WHERE id = ?"#,
                rusqlite::params![template_id],
                Self::template_from_row,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => CoreError::common(CommonError::General(
                    format!("Template not found: {}", template_id),
                )),
                other => storage_err("mock_user_templates", "query_row", other.to_string()),
            })?;

        let mut stmt = inner
            .prepare(
                r#"SELECT id, template_id, column_name, column_type, generator,
                   generator_params, null_ratio, is_unique, is_primary_key,
                   is_foreign_key, ref_table, ref_column, comment,
                   confidence, sort_order
                   FROM mock_template_columns
                   WHERE template_id = ? ORDER BY sort_order ASC"#,
            )
            .map_err(|e| storage_err("mock_template_columns", "query", e.to_string()))?;

        let rows = stmt
            .query_map(
                rusqlite::params![template_id],
                Self::template_column_from_row,
            )
            .map_err(|e| storage_err("mock_template_columns", "query_map", e.to_string()))?;

        let mut columns = Vec::new();
        for row in rows {
            match row {
                Ok(col) => columns.push(col),
                Err(e) => return Err(storage_err("mock_template_columns", "row", e.to_string())),
            }
        }

        Ok((template, columns))
    }

    pub async fn delete_template(&self, template_id: &str) -> Result<(), CoreError> {
        let conn = self.get_conn().await?;
        let inner = conn.inner()?;

        let tx = inner
            .unchecked_transaction()
            .map_err(|e| storage_err("mock_template", "begin", e.to_string()))?;

        tx.execute(
            "DELETE FROM mock_template_columns WHERE template_id = ?",
            rusqlite::params![template_id],
        )
        .map_err(|e| storage_err("mock_template_columns", "delete", e.to_string()))?;

        tx.execute(
            "DELETE FROM mock_user_templates WHERE id = ?",
            rusqlite::params![template_id],
        )
        .map_err(|e| storage_err("mock_user_templates", "delete", e.to_string()))?;

        tx.commit()
            .map_err(|e| storage_err("mock_template", "commit", e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::error::CoreError;

    #[test]
    fn test_task_serialization_roundtrip() -> Result<(), CoreError> {
        let task = MockGenerationTask {
            id: "t1".to_string(),
            table_name: "orders".to_string(),
            table_alias: Some("订单表".to_string()),
            row_count: 1000,
            seed: Some(42),
            locale: "zh_cn".to_string(),
            scene_id: Some("ecommerce".to_string()),
            save_format: Some("csv".to_string()),
            status: "completed".to_string(),
            error_message: None,
            generated_rows: Some(1000),
            generation_time_ms: Some(1500),
            created_at: Some("2026-05-10T10:00:00Z".to_string()),
            updated_at: None,
        };
        let json = serde_json::to_string(&task)?;
        let parsed: MockGenerationTask = serde_json::from_str(&json)?;
        assert_eq!(parsed.id, "t1");
        assert_eq!(parsed.table_name, "orders");
        assert_eq!(parsed.row_count, 1000);
        assert_eq!(parsed.seed, Some(42));
        Ok(())
    }

    #[test]
    fn test_column_serialization_roundtrip() -> Result<(), CoreError> {
        let col = MockGenerationColumn {
            id: "c1".to_string(),
            task_id: "t1".to_string(),
            column_name: "email".to_string(),
            column_type: "Varchar".to_string(),
            generator: "SafeEmail".to_string(),
            generator_params: None,
            null_ratio: 0.1,
            is_unique: true,
            is_primary_key: false,
            is_foreign_key: false,
            ref_table: None,
            ref_column: None,
            comment: None,
            confidence: Some("high".to_string()),
            sort_order: 1,
        };
        let json = serde_json::to_string(&col)?;
        let parsed: MockGenerationColumn = serde_json::from_str(&json)?;
        assert_eq!(parsed.column_name, "email");
        assert_eq!(parsed.null_ratio, 0.1);
        assert!(parsed.is_unique);
        assert_eq!(parsed.sort_order, 1);
        Ok(())
    }

    #[test]
    fn test_template_serialization_roundtrip() -> Result<(), CoreError> {
        let tmpl = MockUserTemplate {
            id: "tm1".to_string(),
            name: "我的电商模板".to_string(),
            description: Some("自定义电商数据模板".to_string()),
            row_count: 5000,
            seed: Some(99),
            locale: "zh_cn".to_string(),
            created_at: Some("2026-05-10T10:00:00Z".to_string()),
            updated_at: None,
        };
        let json = serde_json::to_string(&tmpl)?;
        let parsed: MockUserTemplate = serde_json::from_str(&json)?;
        assert_eq!(parsed.id, "tm1");
        assert_eq!(parsed.name, "我的电商模板");
        assert_eq!(parsed.row_count, 5000);
        Ok(())
    }

    #[test]
    fn test_storage_err_format() {
        let err = storage_err("mock_tasks", "insert", "DB locked".to_string());
        let msg = err.to_string();
        assert!(msg.contains("mock_tasks"));
        assert!(msg.contains("insert"));
    }

    #[test]
    fn test_task_all_optional_fields_null() -> Result<(), CoreError> {
        let task = MockGenerationTask {
            id: "min".to_string(),
            table_name: "min".to_string(),
            table_alias: None,
            row_count: 1,
            seed: None,
            locale: "en_us".to_string(),
            scene_id: None,
            save_format: None,
            status: "pending".to_string(),
            error_message: None,
            generated_rows: None,
            generation_time_ms: None,
            created_at: None,
            updated_at: None,
        };
        let json = serde_json::to_string(&task)?;
        let parsed: MockGenerationTask = serde_json::from_str(&json)?;
        assert_eq!(parsed.table_alias, None);
        assert_eq!(parsed.seed, None);
        assert_eq!(parsed.error_message, None);
        Ok(())
    }

    #[test]
    fn test_detail_contains_task_and_columns() {
        let detail = MockGenerationDetail {
            task: MockGenerationTask {
                id: "d1".to_string(),
                table_name: "test".to_string(),
                table_alias: None,
                row_count: 10,
                seed: None,
                locale: "en_us".to_string(),
                scene_id: None,
                save_format: None,
                status: "done".to_string(),
                error_message: None,
                generated_rows: None,
                generation_time_ms: None,
                created_at: None,
                updated_at: None,
            },
            columns: vec![MockGenerationColumn {
                id: "c1".to_string(),
                task_id: "d1".to_string(),
                column_name: "col".to_string(),
                column_type: "Integer".to_string(),
                generator: "AutoIncrement".to_string(),
                generator_params: None,
                null_ratio: 0.0,
                is_unique: false,
                is_primary_key: false,
                is_foreign_key: false,
                ref_table: None,
                ref_column: None,
                comment: None,
                confidence: None,
                sort_order: 0,
            }],
        };
        assert_eq!(detail.task.id, "d1");
        assert_eq!(detail.columns.len(), 1);
        assert_eq!(detail.columns[0].column_name, "col");
    }
}
