//! 分析资源管理相关命令
//!
//! 处理分析资源（连接、表、文件）的增删改查、文件夹、标签等操作

use specta::Type;
use std::sync::{Arc, OnceLock};
use tauri::State;

use crate::commands::project_commands::ProjectState;
use crate::core::error::{CommonError, CoreError};
use crate::core::persistence::{
    AnalyticsFolder, AnalyticsRecycleItem, AnalyticsResource, AnalyticsResourceStore, AnalyticsTag,
    CreateFolderRequest, CreateResourceRequest, CreateTagRequest, ListResourcesOutput,
    ResourceVersion,
};

const STORE_UNINITIALIZED: &str = "分析资源存储未初始化";

/// 分析资源状态（OnceLock 单例，无锁读取）
pub struct AnalyticsResourceState {
    pub store: Arc<OnceLock<AnalyticsResourceStore>>,
}

impl Default for AnalyticsResourceState {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalyticsResourceState {
    pub fn new() -> Self {
        Self {
            store: Arc::new(OnceLock::new()),
        }
    }
}

fn get_store(state: &AnalyticsResourceState) -> Result<&AnalyticsResourceStore, CoreError> {
    state
        .store
        .get()
        .ok_or_else(|| CoreError::common(CommonError::General(STORE_UNINITIALIZED.into())))
}

/// IPC 版本号（SemVer），用于运行时前后端兼容性检测
pub const ANALYTICS_RESOURCE_API_VERSION: &str = "1.7.0";

/// 获取分析资源模块 API 版本号
#[tauri::command]
#[specta::specta]
pub fn get_analytics_resource_api_version() -> String {
    ANALYTICS_RESOURCE_API_VERSION.to_string()
}

// ==================== Resource Commands ====================

/// 创建资源
#[tauri::command]
pub async fn create_analytics_resource(
    input: CreateResourceRequest,
    analytics_state: State<'_, AnalyticsResourceState>,
    _project_state: State<'_, ProjectState>,
) -> Result<AnalyticsResource, CoreError> {
    tracing::info!(resource_type = %input.resource_type, name = %input.name, "Creating analytics resource");

    let resource = get_store(&analytics_state)?.create_resource(input).await?;

    tracing::info!(resource_id = %resource.id, "Analytics resource created successfully");

    Ok(resource)
}

/// 更新资源
#[tauri::command]
pub async fn update_analytics_resource(
    id: String,
    input: CreateResourceRequest,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<AnalyticsResource, CoreError> {
    tracing::info!(resource_id = %id, "Updating analytics resource");

    let resource = get_store(&analytics_state)?
        .update_resource(&id, input)
        .await?;

    tracing::info!(resource_id = %resource.id, "Analytics resource updated successfully");

    Ok(resource)
}

/// 获取资源详情
#[tauri::command]
pub async fn get_analytics_resource(
    id: String,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<AnalyticsResource, CoreError> {
    tracing::debug!(resource_id = %id, "Getting analytics resource");

    let resource = get_store(&analytics_state)?.get_resource_by_id(&id).await?;

    Ok(resource)
}

/// 列出资源
#[derive(serde::Deserialize, Debug, Type)]
pub struct ListAnalyticsResourcesInput {
    pub scope: Option<String>,
    pub resource_type: Option<String>,
    pub folder_id: Option<String>,
}

#[tauri::command]
pub async fn list_analytics_resources(
    input: ListAnalyticsResourcesInput,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<Vec<AnalyticsResource>, CoreError> {
    tracing::debug!(
        scope = ?input.scope,
        resource_type = ?input.resource_type,
        folder_id = ?input.folder_id,
        "Listing analytics resources"
    );

    let resources = get_store(&analytics_state)?
        .list_resources(
            input.scope.as_deref(),
            input.resource_type.as_deref(),
            input.folder_id.as_deref(),
        )
        .await?;

    Ok(resources)
}

/// 删除资源（软删除，移入回收站）
#[tauri::command]
pub async fn delete_analytics_resource(
    id: String,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<(), CoreError> {
    tracing::info!(resource_id = %id, "Deleting analytics resource");

    get_store(&analytics_state)?.delete_resource(&id).await?;

    tracing::info!(resource_id = %id, "Analytics resource deleted successfully");

    Ok(())
}

/// 批量删除资源（事务支持）
#[tauri::command]
pub async fn batch_delete_analytics_resources(
    ids: Vec<String>,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<(), CoreError> {
    tracing::info!(count = ids.len(), "Batch deleting analytics resources");

    get_store(&analytics_state)?
        .batch_delete_resources(&ids)
        .await?;

    tracing::info!(count = ids.len(), "Batch delete completed successfully");

    Ok(())
}

/// 克隆资源
#[tauri::command]
pub async fn clone_analytics_resource(
    id: String,
    new_name: Option<String>,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<AnalyticsResource, CoreError> {
    tracing::info!(resource_id = %id, "Cloning analytics resource");

    let cloned = get_store(&analytics_state)?
        .clone_resource(&id, new_name.as_deref())
        .await?;

    tracing::info!(new_resource_id = %cloned.id, "Analytics resource cloned successfully");

    Ok(cloned)
}

/// 分页列出资源
#[derive(serde::Deserialize, Debug, Type)]
pub struct ListResourcesInput {
    pub scope: Option<String>,
    pub resource_type: Option<String>,
    pub folder_id: Option<String>,
    pub search: Option<String>,
    pub pagination: Option<PaginationInput>,
    pub sort: Option<SortInput>,
}

#[derive(serde::Deserialize, Debug, Type)]
pub struct PaginationInput {
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(serde::Deserialize, Debug, Type)]
pub struct SortInput {
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

#[tauri::command]
pub async fn list_analytics_resources_paginated(
    input: ListResourcesInput,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<ListResourcesOutput, CoreError> {
    tracing::debug!(
        scope = ?input.scope,
        resource_type = ?input.resource_type,
        "Listing analytics resources paginated"
    );

    let page = input
        .pagination
        .as_ref()
        .and_then(|p| p.page)
        .unwrap_or_else(|| {
            tracing::trace!("No page specified, defaulting to page 1");
            1
        });
    let page_size = input
        .pagination
        .as_ref()
        .and_then(|p| p.page_size)
        .unwrap_or_else(|| {
            tracing::trace!("No page_size specified, defaulting to 20");
            20
        });
    let sort_by = input.sort.as_ref().and_then(|s| s.sort_by.clone());
    let sort_order = input.sort.as_ref().and_then(|s| s.sort_order.clone());

    let result = get_store(&analytics_state)?
        .list_resources_paginated(
            input.scope.as_deref(),
            input.resource_type.as_deref(),
            input.folder_id.as_deref(),
            input.search.as_deref(),
            page,
            page_size,
            sort_by.as_deref(),
            sort_order.as_deref(),
        )
        .await?;

    Ok(result)
}

// ==================== Folder Commands ====================

/// 创建文件夹
#[tauri::command]
pub async fn create_analytics_folder(
    input: CreateFolderRequest,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<AnalyticsFolder, CoreError> {
    tracing::info!(name = %input.name, scope = %input.scope, "Creating analytics folder");

    let folder = get_store(&analytics_state)?.create_folder(input).await?;

    tracing::info!(folder_id = %folder.id, "Analytics folder created successfully");

    Ok(folder)
}

/// 获取文件夹详情
#[tauri::command]
pub async fn get_analytics_folder(
    id: String,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<AnalyticsFolder, CoreError> {
    tracing::debug!(folder_id = %id, "Getting analytics folder");

    let folder = get_store(&analytics_state)?.get_folder_by_id(&id).await?;

    Ok(folder)
}

/// 列出文件夹
#[derive(serde::Deserialize, Debug, Type)]
pub struct ListAnalyticsFoldersInput {
    pub scope: Option<String>,
    pub parent_folder_id: Option<String>,
}

#[tauri::command]
pub async fn list_analytics_folders(
    input: ListAnalyticsFoldersInput,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<Vec<AnalyticsFolder>, CoreError> {
    tracing::debug!(
        scope = ?input.scope,
        parent_folder_id = ?input.parent_folder_id,
        "Listing analytics folders"
    );

    let folders = get_store(&analytics_state)?
        .list_folders(input.scope.as_deref(), input.parent_folder_id.as_deref())
        .await?;

    Ok(folders)
}

/// 将资源添加到文件夹
#[derive(serde::Deserialize, Debug, Type)]
pub struct AddResourceToFolderInput {
    pub resource_id: String,
    pub folder_id: String,
}

#[tauri::command]
pub async fn add_analytics_resource_to_folder(
    input: AddResourceToFolderInput,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<(), CoreError> {
    tracing::info!(
        resource_id = %input.resource_id,
        folder_id = %input.folder_id,
        "Adding resource to folder"
    );

    get_store(&analytics_state)?
        .add_resource_to_folder(&input.resource_id, &input.folder_id)
        .await?;

    Ok(())
}

/// 从文件夹移除资源
#[tauri::command]
pub async fn remove_analytics_resource_from_folder(
    input: AddResourceToFolderInput,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<(), CoreError> {
    tracing::info!(
        resource_id = %input.resource_id,
        folder_id = %input.folder_id,
        "Removing resource from folder"
    );

    get_store(&analytics_state)?
        .remove_resource_from_folder(&input.resource_id, &input.folder_id)
        .await?;

    Ok(())
}

// ==================== Tag Commands ====================

/// 创建标签
#[tauri::command]
pub async fn create_analytics_tag(
    input: CreateTagRequest,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<AnalyticsTag, CoreError> {
    tracing::info!(name = %input.name, scope = %input.scope, "Creating analytics tag");

    let tag = get_store(&analytics_state)?.create_tag(input).await?;

    tracing::info!(tag_id = %tag.id, "Analytics tag created successfully");

    Ok(tag)
}

/// 列出标签
#[derive(serde::Deserialize, Debug, Type)]
pub struct ListAnalyticsTagsInput {
    pub scope: Option<String>,
}

#[tauri::command]
pub async fn list_analytics_tags(
    input: ListAnalyticsTagsInput,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<Vec<AnalyticsTag>, CoreError> {
    tracing::debug!(scope = ?input.scope, "Listing analytics tags");

    let tags = get_store(&analytics_state)?
        .list_tags(input.scope.as_deref())
        .await?;

    Ok(tags)
}

/// 将标签添加到资源
#[derive(serde::Deserialize, Debug, Type)]
pub struct AddTagToResourceInput {
    pub resource_id: String,
    pub tag_id: String,
}

#[tauri::command]
pub async fn add_analytics_tag_to_resource(
    input: AddTagToResourceInput,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<(), CoreError> {
    tracing::info!(
        resource_id = %input.resource_id,
        tag_id = %input.tag_id,
        "Adding tag to resource"
    );

    get_store(&analytics_state)?
        .add_tag_to_resource(&input.resource_id, &input.tag_id)
        .await?;

    Ok(())
}

/// 从资源移除标签
#[tauri::command]
pub async fn remove_analytics_tag_from_resource(
    input: AddTagToResourceInput,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<(), CoreError> {
    tracing::info!(
        resource_id = %input.resource_id,
        tag_id = %input.tag_id,
        "Removing tag from resource"
    );

    get_store(&analytics_state)?
        .remove_tag_from_resource(&input.resource_id, &input.tag_id)
        .await?;

    Ok(())
}

/// 获取单个标签详情
#[tauri::command]
pub async fn get_analytics_tag(
    id: String,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<AnalyticsTag, CoreError> {
    tracing::debug!(tag_id = %id, "Getting analytics tag");

    let tag = get_store(&analytics_state)?.get_tag_by_id(&id).await?;

    Ok(tag)
}

// ==================== Recycle Bin Commands ====================

/// 获取回收站列表
#[tauri::command]
pub async fn get_analytics_recycle_bin(
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<Vec<AnalyticsRecycleItem>, CoreError> {
    tracing::debug!("Getting analytics recycle bin");

    let items = get_store(&analytics_state)?.get_recycle_items().await?;

    Ok(items)
}

/// 从回收站恢复资源
#[tauri::command]
pub async fn restore_analytics_resource_from_recycle(
    recycle_id: String,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<AnalyticsResource, CoreError> {
    tracing::info!(recycle_id = %recycle_id, "Restoring analytics resource from recycle bin");

    let resource = get_store(&analytics_state)?
        .restore_from_recycle(&recycle_id)
        .await?;

    tracing::info!(resource_id = %resource.id, "Analytics resource restored successfully");

    Ok(resource)
}

/// 永久删除资源
#[tauri::command]
pub async fn permanent_delete_analytics_resource(
    recycle_id: String,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<(), CoreError> {
    tracing::info!(recycle_id = %recycle_id, "Permanent deleting analytics resource");

    get_store(&analytics_state)?
        .permanent_delete(&recycle_id)
        .await?;

    tracing::info!(recycle_id = %recycle_id, "Analytics resource permanently deleted");

    Ok(())
}

// ==================== Version History Commands ====================

/// 获取资源的版本历史
#[tauri::command]
pub async fn get_resource_versions(
    resource_id: String,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<Vec<ResourceVersion>, CoreError> {
    tracing::debug!(resource_id = %resource_id, "Getting resource versions");

    let versions = get_store(&analytics_state)?
        .get_resource_versions(&resource_id)
        .await?;

    Ok(versions)
}

// ==================== Tag Bidirectional Commands ====================

/// 获取资源的标签列表
#[tauri::command]
pub async fn get_tags_for_resource(
    resource_id: String,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<Vec<AnalyticsTag>, CoreError> {
    tracing::debug!(resource_id = %resource_id, "Getting tags for resource");

    let tags = get_store(&analytics_state)?
        .get_tags_for_resource(&resource_id)
        .await?;

    Ok(tags)
}

/// 获取标签关联的资源列表
#[tauri::command]
pub async fn get_resources_by_tag(
    tag_id: String,
    analytics_state: State<'_, AnalyticsResourceState>,
) -> Result<Vec<AnalyticsResource>, CoreError> {
    tracing::debug!(tag_id = %tag_id, "Getting resources by tag");

    let resources = get_store(&analytics_state)?
        .get_resources_by_tag(&tag_id)
        .await?;

    Ok(resources)
}

// ==================== Initialization ====================

/// 初始化分析资源存储
#[tauri::command]
pub async fn init_analytics_resource_store(
    analytics_state: State<'_, AnalyticsResourceState>,
    project_state: State<'_, ProjectState>,
) -> Result<(), CoreError> {
    tracing::info!("Initializing analytics resource store");

    let project_guard = project_state.store.lock().await;
    let project_store = project_guard
        .as_ref()
        .ok_or_else(|| CoreError::common(CommonError::General("项目存储未初始化".into())))?;

    if analytics_state.store.get().is_some() {
        tracing::info!("Analytics resource store already initialized");
        return Ok(());
    }

    let store = AnalyticsResourceStore::new(project_store.db_manager.sqlite_pool());
    analytics_state
        .store
        .set(store)
        .map_err(|_| CoreError::common(CommonError::General("分析资源存储重复初始化".into())))?;

    tracing::info!("Analytics resource store initialized successfully");

    Ok(())
}
