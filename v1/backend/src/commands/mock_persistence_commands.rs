use std::sync::Arc;

use crate::core::error::CoreError;
use crate::core::persistence::project_db::ProjectSqlitePool;
use crate::mock::persistence::{
    MockGenerationColumn, MockGenerationDetail, MockGenerationStore, MockGenerationTask,
    MockTemplateColumn, MockUserTemplate,
};

async fn open_store(project_path: &str) -> Result<MockGenerationStore, CoreError> {
    let db_path = format!("{}/.RSMETA/project.db", project_path);
    let pool = ProjectSqlitePool::new(std::path::PathBuf::from(&db_path), 1)
        .await
        .map_err(|e| CoreError::from(format!("Failed to open project database: {}", e)))?;

    Ok(MockGenerationStore::new(Arc::new(pool)))
}

/// 保存生成任务到项目数据库
///
/// 将当前生成配置（MockGenerationTask）和列定义（MockGenerationColumn）
/// 持久化到项目 SQLite 数据库（`.RSMETA/project.db`），
/// 用于后续重复生成或审计追溯。
#[tauri::command]
#[specta::specta]
pub async fn save_mock_generation_task(
    project_path: String,
    task: MockGenerationTask,
    columns: Vec<MockGenerationColumn>,
) -> Result<String, CoreError> {
    let store = open_store(&project_path).await?;
    store
        .save_task(&task, &columns)
        .await
        .map_err(|e| CoreError::from(format!("Failed to save task: {}", e)))?;
    Ok(task.id.clone())
}

/// 查询生成历史记录列表
///
/// 从项目 SQLite 数据库读取最近 N 条生成任务记录，
/// 按创建时间倒序排列。`limit` 可选，默认 20。
#[tauri::command]
#[specta::specta]
pub async fn get_mock_generation_history(
    project_path: String,
    limit: Option<u32>,
) -> Result<Vec<MockGenerationTask>, CoreError> {
    let store = open_store(&project_path).await?;
    store
        .get_history(limit.unwrap_or(20))
        .await
        .map_err(|e| CoreError::from(format!("Failed to query history: {}", e)))
}

/// 查询生成任务详情（含列定义）
///
/// 按任务 ID 读取完整生成配置，包含表结构、
/// 生成器配置和每列的 nullable_ratio / unique 设置。
#[tauri::command]
#[specta::specta]
pub async fn get_mock_generation_detail(
    project_path: String,
    task_id: String,
) -> Result<MockGenerationDetail, CoreError> {
    let store = open_store(&project_path).await?;
    store
        .get_detail(&task_id)
        .await
        .map_err(|e| CoreError::from(format!("Failed to query detail: {}", e)))
}

/// 删除生成任务记录
///
/// 从项目 SQLite 数据库删除指定任务及其关联列定义（CASCADE）。
#[tauri::command]
#[specta::specta]
pub async fn delete_mock_generation_task(
    project_path: String,
    task_id: String,
) -> Result<(), CoreError> {
    let store = open_store(&project_path).await?;
    store
        .delete_task(&task_id)
        .await
        .map_err(|e| CoreError::from(format!("Failed to delete task: {}", e)))
}

/// 保存用户自定义模板
///
/// 将用户创建的模板（MockUserTemplate）和列定义（MockTemplateColumn）
/// 持久化到项目 SQLite 数据库，支持跨会话复用。
#[tauri::command]
#[specta::specta]
pub async fn save_mock_template(
    project_path: String,
    template: MockUserTemplate,
    columns: Vec<MockTemplateColumn>,
) -> Result<String, CoreError> {
    let store = open_store(&project_path).await?;
    store
        .save_template(&template, &columns)
        .await
        .map_err(|e| CoreError::from(format!("Failed to save template: {}", e)))?;
    Ok(template.id.clone())
}

/// 查询用户自定义模板列表
///
/// 从项目 SQLite 数据库读取所有用户保存的模板元信息，
/// 按更新时间倒序排列。
#[tauri::command]
#[specta::specta]
pub async fn get_mock_templates(project_path: String) -> Result<Vec<MockUserTemplate>, CoreError> {
    let store = open_store(&project_path).await?;
    store
        .get_templates()
        .await
        .map_err(|e| CoreError::from(format!("Failed to query templates: {}", e)))
}

/// 查询模板详情（含列定义）
///
/// 按模板 ID 读取完整模板定义，返回模板元信息和列配置元组。
#[tauri::command]
#[specta::specta]
pub async fn get_mock_template_detail(
    project_path: String,
    template_id: String,
) -> Result<(MockUserTemplate, Vec<MockTemplateColumn>), CoreError> {
    let store = open_store(&project_path).await?;
    store
        .get_template_detail(&template_id)
        .await
        .map_err(|e| CoreError::from(format!("Failed to query template detail: {}", e)))
}

/// 删除用户自定义模板
///
/// 从项目 SQLite 数据库删除指定模板及其关联列定义（CASCADE）。
/// 与 `delete_mock_generation_task` 不同，此命令操作的是 `mock_user_templates` 表。
#[tauri::command]
#[specta::specta]
pub async fn delete_mock_template(
    project_path: String,
    template_id: String,
) -> Result<(), CoreError> {
    let store = open_store(&project_path).await?;
    store
        .delete_template(&template_id)
        .await
        .map_err(|e| CoreError::from(format!("Failed to delete template: {}", e)))
}
