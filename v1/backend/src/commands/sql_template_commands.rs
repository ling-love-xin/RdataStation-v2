use crate::core::error::CoreError;
use crate::core::migration::global_init;
use crate::core::persistence::SqlTemplate;

#[derive(serde::Serialize, Debug, specta::Type)]
pub struct SqlTemplateResponse {
    pub id: String,
    pub name: String,
    pub content: String,
    pub db_type: Option<String>,
    pub category: String,
    pub description: Option<String>,
    pub tags: Option<String>,
    pub is_builtin: bool,
    pub created_at_ms: u32,
    pub updated_at_ms: u32,
}

impl From<SqlTemplate> for SqlTemplateResponse {
    fn from(template: SqlTemplate) -> Self {
        Self {
            id: template.id,
            name: template.name,
            content: template.content,
            db_type: template.db_type,
            category: template.category,
            description: template.description,
            tags: template.tags,
            is_builtin: template.is_builtin,
            created_at_ms: template.created_at_ms as u32,
            updated_at_ms: template.updated_at_ms as u32,
        }
    }
}

#[derive(serde::Deserialize, Debug)]
pub struct CreateSqlTemplateInput {
    pub name: String,
    pub content: String,
    pub db_type: Option<String>,
    pub category: String,
    pub description: Option<String>,
    pub tags: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn create_sql_template(
    input: CreateSqlTemplateInput,
) -> Result<SqlTemplateResponse, CoreError> {
    let global_db = global_init::get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not initialized".to_string()))?;

    let template = SqlTemplate::new(
        input.name,
        input.content,
        input.db_type,
        input.category,
        input.description,
        input.tags,
    );

    let store = global_db
        .get_sql_template_store()
        .map_err(|e| CoreError::from(format!("获取模板存储失败: {}", e)))?;

    store
        .save(&template)
        .map_err(|e| CoreError::from(format!("保存模板失败: {}", e)))?;

    Ok(template.into())
}

#[tauri::command]
#[specta::specta]
pub async fn get_all_sql_templates() -> Result<Vec<SqlTemplateResponse>, CoreError> {
    let global_db = global_init::get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not initialized".to_string()))?;

    let store = global_db
        .get_sql_template_store()
        .map_err(|e| CoreError::from(format!("获取模板存储失败: {}", e)))?;

    let templates = store
        .get_all()
        .map_err(|e| CoreError::from(format!("获取模板列表失败: {}", e)))?;

    Ok(templates.into_iter().map(|t| t.into()).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn get_sql_templates_by_category(
    category: String,
) -> Result<Vec<SqlTemplateResponse>, CoreError> {
    let global_db = global_init::get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not initialized".to_string()))?;

    let store = global_db
        .get_sql_template_store()
        .map_err(|e| CoreError::from(format!("获取模板存储失败: {}", e)))?;

    let templates = store
        .get_by_category(&category)
        .map_err(|e| CoreError::from(format!("获取模板列表失败: {}", e)))?;

    Ok(templates.into_iter().map(|t| t.into()).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn get_sql_templates_by_db_type(
    db_type: String,
) -> Result<Vec<SqlTemplateResponse>, CoreError> {
    let global_db = global_init::get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not initialized".to_string()))?;

    let store = global_db
        .get_sql_template_store()
        .map_err(|e| CoreError::from(format!("获取模板存储失败: {}", e)))?;

    let templates = store
        .get_by_db_type(&db_type)
        .map_err(|e| CoreError::from(format!("获取模板列表失败: {}", e)))?;

    Ok(templates.into_iter().map(|t| t.into()).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_sql_template(template_id: String) -> Result<bool, CoreError> {
    let global_db = global_init::get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not initialized".to_string()))?;

    let store = global_db
        .get_sql_template_store()
        .map_err(|e| CoreError::from(format!("获取模板存储失败: {}", e)))?;

    store
        .delete(&template_id)
        .map_err(|e| CoreError::from(format!("删除模板失败: {}", e)))
}

#[tauri::command]
#[specta::specta]
pub async fn get_sql_template_categories() -> Result<Vec<String>, CoreError> {
    let global_db = global_init::get_global_db_manager()
        .ok_or_else(|| CoreError::from("Global database manager not initialized".to_string()))?;

    let store = global_db
        .get_sql_template_store()
        .map_err(|e| CoreError::from(format!("获取模板存储失败: {}", e)))?;

    store
        .get_categories()
        .map_err(|e| CoreError::from(format!("获取分类列表失败: {}", e)))
}
