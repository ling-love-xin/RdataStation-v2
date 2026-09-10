// 分析资源类型定义

export type ResourceType = 'connection' | 'table' | 'file'

export type ResourceScope = 'global' | 'project' | 'session'

export interface AnalyticsResource {
  id: string
  resource_type: ResourceType
  name: string
  alias?: string
  config: Record<string, unknown>
  scope: ResourceScope
  row_count?: number
  column_count?: number
  file_size?: number
  version: number
  parent_version_id?: string
  parent_resource_id?: string
  source_query?: string
  created_at: string
  updated_at: string
  created_by?: string
  deleted_at?: string
}

export interface AnalyticsFolder {
  id: string
  name: string
  scope: ResourceScope
  parent_folder_id?: string
  sort_order: number
  color?: string
  icon?: string
  created_at: string
  updated_at: string
  deleted_at?: string
}

export interface AnalyticsTag {
  id: string
  name: string
  color?: string
  icon?: string
  scope: ResourceScope
  created_at: string
  deleted_at?: string
}

export interface AnalyticsRecycleItem {
  id: string
  resource_id: string
  resource_type: string
  resource_name: string
  resource_data: Record<string, unknown>
  deleted_by?: string
  deleted_at: string
}

export interface ResourceVersion {
  id: string
  resource_id: string
  version: number
  snapshot: Record<string, unknown>
  created_at: string
}

export interface CreateResourceRequest {
  resource_type: ResourceType
  name: string
  alias?: string
  config: Record<string, unknown>
  scope: ResourceScope
  row_count?: number
  column_count?: number
  file_size?: number
  parent_resource_id?: string
  source_query?: string
}

export interface CreateFolderRequest {
  name: string
  scope: ResourceScope
  parent_folder_id?: string
  color?: string
  icon?: string
}

export interface CreateTagRequest {
  name: string
  color?: string
  icon?: string
  scope: ResourceScope
}

// ==================== Pagination & Sorting ====================

export type SortOrder = 'asc' | 'desc'

export type SortField = 'name' | 'created_at' | 'updated_at' | 'row_count' | 'file_size'

export interface PaginationInput {
  page?: number
  pageSize?: number
}

export interface SortInput {
  sortBy?: SortField
  sortOrder?: SortOrder
}

export interface ListResourcesInput {
  scope?: string
  resource_type?: string
  folder_id?: string
  search?: string
  pagination?: PaginationInput
  sort?: SortInput
}

export interface ListResourcesOutput {
  items: AnalyticsResource[]
  total: number
  page: number
  pageSize: number
  totalPages: number
}

export interface ListFoldersInput {
  scope?: string
  parent_folder_id?: string
}

export interface ListTagsInput {
  scope?: string
}

// ==================== Settings ====================

export interface AnalyticsResourceGeneralSettings {
  defaultScope: ResourceScope
  defaultPageSize: number
  defaultSortField: SortField
  defaultSortOrder: SortOrder
}

export interface AnalyticsResourceDisplaySettings {
  showIcons: boolean
  showScopeTags: boolean
  showMetadata: boolean
  enableVirtualScroll: boolean
}

export interface AnalyticsResourceCacheSettings {
  enabled: boolean
  ttlSeconds: number
  maxSize: number
}

export interface AnalyticsResourceSettings {
  general: AnalyticsResourceGeneralSettings
  display: AnalyticsResourceDisplaySettings
  cache: AnalyticsResourceCacheSettings
}

export const DEFAULT_SETTINGS: AnalyticsResourceSettings = {
  general: {
    defaultScope: 'project',
    defaultPageSize: 20,
    defaultSortField: 'created_at',
    defaultSortOrder: 'desc',
  },
  display: {
    showIcons: true,
    showScopeTags: true,
    showMetadata: true,
    enableVirtualScroll: true,
  },
  cache: {
    enabled: true,
    ttlSeconds: 30,
    maxSize: 50,
  },
}
