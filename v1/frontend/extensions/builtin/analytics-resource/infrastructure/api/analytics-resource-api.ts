import { invoke } from '@tauri-apps/api/core'

import type {
  AnalyticsResource,
  AnalyticsFolder,
  AnalyticsTag,
  AnalyticsRecycleItem,
  CreateResourceRequest,
  CreateFolderRequest,
  CreateTagRequest,
  ListResourcesInput,
  ListFoldersInput,
  ListTagsInput,
  ListResourcesOutput,
  ResourceVersion,
} from '../../types'

// ==================== Resource API ====================

export async function createAnalyticsResource(
  input: CreateResourceRequest
): Promise<AnalyticsResource> {
  return await invoke('create_analytics_resource', { input })
}

export async function updateAnalyticsResource(
  id: string,
  input: CreateResourceRequest
): Promise<AnalyticsResource> {
  return await invoke('update_analytics_resource', { id, input })
}

export async function getAnalyticsResource(id: string): Promise<AnalyticsResource> {
  return await invoke('get_analytics_resource', { id })
}

export async function listAnalyticsResources(
  input: ListResourcesInput
): Promise<AnalyticsResource[]> {
  return await invoke('list_analytics_resources', { input })
}

export async function listAnalyticsResourcesPaginated(
  input: ListResourcesInput
): Promise<ListResourcesOutput> {
  return await invoke('list_analytics_resources_paginated', { input })
}

export async function deleteAnalyticsResource(id: string): Promise<void> {
  return await invoke('delete_analytics_resource', { id })
}

export async function batchDeleteResources(ids: string[]): Promise<void> {
  return await invoke('batch_delete_analytics_resources', { ids })
}

export async function cloneAnalyticsResource(
  id: string,
  newName?: string
): Promise<AnalyticsResource> {
  return await invoke('clone_analytics_resource', { id, newName })
}

// ==================== Folder API ====================

export async function createAnalyticsFolder(input: CreateFolderRequest): Promise<AnalyticsFolder> {
  return await invoke('create_analytics_folder', { input })
}

export async function getAnalyticsFolder(id: string): Promise<AnalyticsFolder> {
  return await invoke('get_analytics_folder', { id })
}

export async function listAnalyticsFolders(input: ListFoldersInput): Promise<AnalyticsFolder[]> {
  return await invoke('list_analytics_folders', { input })
}

export async function addAnalyticsResourceToFolder(
  resourceId: string,
  folderId: string
): Promise<void> {
  return await invoke('add_analytics_resource_to_folder', {
    input: { resourceId, folderId },
  })
}

export async function removeAnalyticsResourceFromFolder(
  resourceId: string,
  folderId: string
): Promise<void> {
  return await invoke('remove_analytics_resource_from_folder', {
    input: { resourceId, folderId },
  })
}

// ==================== Tag API ====================

export async function createAnalyticsTag(input: CreateTagRequest): Promise<AnalyticsTag> {
  return await invoke('create_analytics_tag', { input })
}

export async function getAnalyticsTag(id: string): Promise<AnalyticsTag> {
  return await invoke('get_analytics_tag', { id })
}

export async function listAnalyticsTags(input: ListTagsInput): Promise<AnalyticsTag[]> {
  return await invoke('list_analytics_tags', { input })
}

export async function addAnalyticsTagToResource(resourceId: string, tagId: string): Promise<void> {
  return await invoke('add_analytics_tag_to_resource', {
    input: { resourceId, tagId },
  })
}

export async function removeAnalyticsTagFromResource(
  resourceId: string,
  tagId: string
): Promise<void> {
  return await invoke('remove_analytics_tag_from_resource', {
    input: { resourceId, tagId },
  })
}

// ==================== Recycle Bin API ====================

export async function getAnalyticsRecycleBin(): Promise<AnalyticsRecycleItem[]> {
  return await invoke('get_analytics_recycle_bin')
}

export async function restoreAnalyticsResourceFromRecycle(
  recycleId: string
): Promise<AnalyticsResource> {
  return await invoke('restore_analytics_resource_from_recycle', { recycleId })
}

export async function permanentDeleteAnalyticsResource(recycleId: string): Promise<void> {
  return await invoke('permanent_delete_analytics_resource', { recycleId })
}

// ==================== Initialization ====================

export async function initAnalyticsResourceStore(): Promise<void> {
  return await invoke('init_analytics_resource_store')
}

// ==================== Version History API ====================

export async function getResourceVersions(resourceId: string): Promise<ResourceVersion[]> {
  return await invoke('get_resource_versions', { resourceId })
}

// ==================== Tag Bidirectional API ====================

export async function getTagsForResource(resourceId: string): Promise<AnalyticsTag[]> {
  return await invoke('get_tags_for_resource', { resourceId })
}

export async function getResourcesByTag(tagId: string): Promise<AnalyticsResource[]> {
  return await invoke('get_resources_by_tag', { tagId })
}
