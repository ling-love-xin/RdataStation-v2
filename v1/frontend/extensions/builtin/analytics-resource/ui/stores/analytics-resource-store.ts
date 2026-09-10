import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import * as analyticsApi from '../../infrastructure/api/analytics-resource-api'
import { DEFAULT_SETTINGS } from '../../types'
import { useCache, createCacheKey } from '../composables/use-cache'
import { usePagination } from '../composables/use-pagination'
import { useSelection } from '../composables/use-selection'
import { useStoreSettings } from '../composables/use-settings'

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
  SortField,
  SortOrder,
  AnalyticsResourceSettings,
} from '../../types'

export const useAnalyticsResourceStore = defineStore('analytics-resource', () => {
  // State
  const resources = ref<AnalyticsResource[]>([])
  const folders = ref<AnalyticsFolder[]>([])
  const tags = ref<AnalyticsTag[]>([])
  const recycleBin = ref<AnalyticsRecycleItem[]>([])
  // Resource → Folder mapping
  const resourceFolderMap = ref<Map<string, string[]>>(new Map())

  // Resource → Tag mapping (batch loaded)
  const resourceTagMap = ref<Map<string, AnalyticsTag[]>>(new Map())
  const loading = ref(false)
  const initialized = ref(false)
  const error = ref<string | null>(null)

  const resourceCache = useCache<AnalyticsResource[]>({ maxSize: 20, ttl: 30000 })
  const folderCache = useCache<AnalyticsFolder[]>({ maxSize: 10, ttl: 60000 })

  // Pagination
  const total = ref(0)
  const page = ref(1)
  const pageSize = ref(20)
  const totalPages = computed(() => Math.ceil(total.value / pageSize.value))

  // Sorting
  const sortBy = ref<SortField | null>(null)
  const sortOrder = ref<SortOrder>('asc')

  // Selected items
  const selectedResources = ref<string[]>([])

  // Settings
  const settings = ref<AnalyticsResourceSettings>({ ...DEFAULT_SETTINGS })
  const selectedFolderId = ref<string | null>(null)
  const selectedScope = ref<string | null>(null)
  const selectedType = ref<string | null>(null)

  const { applySettingsToState, loadSettings, saveSettings, resetSettings, clearCache } =
    useStoreSettings(settings, resourceCache, folderCache, pageSize, sortBy, sortOrder)

  const { setSort, toggleSortOrder, setPage, setPageSize, nextPage, prevPage } = usePagination(
    page,
    pageSize,
    total,
    totalPages,
    sortBy,
    sortOrder
  )

  const { selectResource, clearSelection, selectScope, selectType, selectFolder } = useSelection(
    selectedResources,
    selectedScope,
    selectedType,
    selectedFolderId
  )

  // Computed
  const filteredResources = computed(() => {
    let result = [...resources.value]

    if (selectedScope.value) {
      result = result.filter(r => r.scope === selectedScope.value)
    }

    if (selectedType.value) {
      result = result.filter(r => r.resource_type === selectedType.value)
    }

    if (selectedFolderId.value) {
      const folderResources = resourceFolderMap.value.get(selectedFolderId.value) || []
      result = result.filter(r => folderResources.includes(r.id))
    }

    return result
  })

  // Actions - Initialization
  async function initStore() {
    if (initialized.value) return

    try {
      loading.value = true
      loadSettings()
      applySettingsToState()
      await analyticsApi.initAnalyticsResourceStore()
      await Promise.all([loadResources(), loadFolders(), loadTags()])
      initialized.value = true
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      error.value = msg
      console.error('Failed to init analytics resource store:', err)
      throw err
    } finally {
      loading.value = false
    }
  }

  // Actions - Resources
  async function loadResources(input?: ListResourcesInput) {
    error.value = null
    try {
      const cacheKey = createCacheKey(
        'resources',
        input?.scope,
        input?.resource_type,
        input?.folder_id
      )
      const cached = resourceCache.get(cacheKey)
      if (cached) {
        resources.value = cached
        return
      }
      const data = await analyticsApi.listAnalyticsResources(input || {})
      resources.value = data
      resourceCache.set(cacheKey, data)
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to load resources:', err)
      throw err
    }
  }

  async function loadResourcesPaginated(input?: ListResourcesInput) {
    error.value = null
    try {
      loading.value = true
      const cacheKey = createCacheKey(
        'resources_paginated',
        input?.scope,
        input?.resource_type,
        input?.folder_id,
        input?.search,
        page.value,
        pageSize.value,
        sortBy.value,
        sortOrder.value
      )
      const cached = resourceCache.get(cacheKey)
      if (cached) {
        resources.value = cached
        return
      }
      const result = await analyticsApi.listAnalyticsResourcesPaginated({
        ...input,
        pagination: {
          page: page.value,
          pageSize: pageSize.value,
        },
        sort: sortBy.value
          ? {
              sortBy: sortBy.value,
              sortOrder: sortOrder.value,
            }
          : undefined,
      })
      resources.value = result.items
      total.value = result.total
      page.value = result.page
      pageSize.value = result.pageSize
      resourceCache.set(cacheKey, result.items)
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to load resources paginated:', err)
      throw err
    } finally {
      loading.value = false
    }
  }

  function invalidateResourceCache() {
    resourceCache.clear()
  }

  async function createResource(input: CreateResourceRequest) {
    try {
      loading.value = true
      const resource = await analyticsApi.createAnalyticsResource(input)
      resources.value.unshift(resource)
      total.value += 1
      invalidateResourceCache()
      return resource
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to create resource:', err)
      throw err
    } finally {
      loading.value = false
    }
  }

  async function updateResource(id: string, input: CreateResourceRequest) {
    try {
      loading.value = true
      const resource = await analyticsApi.updateAnalyticsResource(id, input)
      const index = resources.value.findIndex(r => r.id === id)
      if (index !== -1) {
        resources.value[index] = resource
      }
      invalidateResourceCache()
      return resource
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to update resource:', err)
      throw err
    } finally {
      loading.value = false
    }
  }

  async function deleteResource(id: string) {
    try {
      loading.value = true
      await analyticsApi.deleteAnalyticsResource(id)
      resources.value = resources.value.filter(r => r.id !== id)
      selectedResources.value = selectedResources.value.filter(rid => rid !== id)
      total.value -= 1
      invalidateResourceCache()
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to delete resource:', err)
      throw err
    } finally {
      loading.value = false
    }
  }

  async function batchDeleteResources(ids: string[]) {
    try {
      loading.value = true
      await analyticsApi.batchDeleteResources(ids)
      resources.value = resources.value.filter(r => !ids.includes(r.id))
      selectedResources.value = selectedResources.value.filter(id => !ids.includes(id))
      total.value -= ids.length
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to batch delete resources:', err)
      throw err
    } finally {
      loading.value = false
    }
  }

  async function cloneResource(id: string, newName?: string) {
    try {
      loading.value = true
      const cloned = await analyticsApi.cloneAnalyticsResource(id, newName)
      resources.value.unshift(cloned)
      total.value += 1
      return cloned
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to clone resource:', err)
      throw err
    } finally {
      loading.value = false
    }
  }

  // Actions - Folders
  async function loadFolders(input?: ListFoldersInput) {
    try {
      folders.value = await analyticsApi.listAnalyticsFolders(input || {})
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to load folders:', err)
      throw err
    }
  }

  async function createFolder(input: CreateFolderRequest) {
    try {
      loading.value = true
      const folder = await analyticsApi.createAnalyticsFolder(input)
      folders.value.unshift(folder)
      return folder
    } catch (error) {
      console.error('Failed to create folder:', error)
      throw error
    } finally {
      loading.value = false
    }
  }

  async function addResourceToFolder(resourceId: string, folderId: string) {
    try {
      await analyticsApi.addAnalyticsResourceToFolder(resourceId, folderId)
      updateResourceFolderMap(folderId, resourceId, true)
    } catch (error) {
      console.error('Failed to add resource to folder:', error)
      throw error
    }
  }

  async function removeResourceFromFolder(resourceId: string, folderId: string) {
    try {
      await analyticsApi.removeAnalyticsResourceFromFolder(resourceId, folderId)
      updateResourceFolderMap(folderId, resourceId, false)
    } catch (error) {
      console.error('Failed to remove resource from folder:', error)
      throw error
    }
  }

  function updateResourceFolderMap(folderId: string, resourceId: string, add: boolean) {
    const current = resourceFolderMap.value.get(folderId) || []
    if (add) {
      if (!current.includes(resourceId)) {
        current.push(resourceId)
      }
    } else {
      const index = current.indexOf(resourceId)
      if (index !== -1) {
        current.splice(index, 1)
      }
    }
    resourceFolderMap.value.set(folderId, current)
  }

  function getResourceFolders(resourceId: string): string[] {
    const folderIds: string[] = []
    for (const [folderId, resourceIds] of resourceFolderMap.value) {
      if (resourceIds.includes(resourceId)) {
        folderIds.push(folderId)
      }
    }
    return folderIds
  }

  async function loadResourceTags(resourceIds: string[]) {
    try {
      const results = await Promise.allSettled(
        resourceIds.map(id => analyticsApi.getTagsForResource(id))
      )
      results.forEach((result, index) => {
        if (result.status === 'fulfilled') {
          resourceTagMap.value.set(resourceIds[index], result.value)
        }
      })
    } catch (error) {
      console.error('Failed to load resource tags:', error)
    }
  }

  function getResourceTags(resourceId: string): AnalyticsTag[] {
    return resourceTagMap.value.get(resourceId) || []
  }

  // Actions - Tags
  async function loadTags(input?: ListTagsInput) {
    try {
      tags.value = await analyticsApi.listAnalyticsTags(input || {})
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to load tags:', err)
      throw err
    }
  }

  async function createTag(input: CreateTagRequest) {
    try {
      loading.value = true
      const tag = await analyticsApi.createAnalyticsTag(input)
      tags.value.unshift(tag)
      return tag
    } catch (error) {
      console.error('Failed to create tag:', error)
      throw error
    } finally {
      loading.value = false
    }
  }

  async function addTagToResource(resourceId: string, tagId: string) {
    try {
      await analyticsApi.addAnalyticsTagToResource(resourceId, tagId)
    } catch (error) {
      console.error('Failed to add tag to resource:', error)
      throw error
    }
  }

  async function removeTagFromResource(resourceId: string, tagId: string) {
    try {
      await analyticsApi.removeAnalyticsTagFromResource(resourceId, tagId)
    } catch (error) {
      console.error('Failed to remove tag from resource:', error)
      throw error
    }
  }

  // Actions - Recycle Bin
  async function loadRecycleBin() {
    try {
      recycleBin.value = await analyticsApi.getAnalyticsRecycleBin()
    } catch (err) {
      const eMsg = err instanceof Error ? err.message : String(err)
      error.value = eMsg
      console.error('Failed to load recycle bin:', err)
      throw err
    }
  }

  async function restoreResource(recycleId: string) {
    try {
      loading.value = true
      const resource = await analyticsApi.restoreAnalyticsResourceFromRecycle(recycleId)
      resources.value.unshift(resource)
      recycleBin.value = recycleBin.value.filter(item => item.id !== recycleId)
      return resource
    } catch (error) {
      console.error('Failed to restore resource:', error)
      throw error
    } finally {
      loading.value = false
    }
  }

  async function permanentDeleteResource(recycleId: string) {
    try {
      loading.value = true
      await analyticsApi.permanentDeleteAnalyticsResource(recycleId)
      recycleBin.value = recycleBin.value.filter(item => item.id !== recycleId)
    } catch (error) {
      console.error('Failed to permanent delete resource:', error)
      throw error
    } finally {
      loading.value = false
    }
  }

  function clearError() {
    error.value = null
  }

  return {
    // State
    resources,
    folders,
    tags,
    recycleBin,
    loading,
    initialized,
    error,
    selectedResources,
    selectedFolderId,
    selectedScope,
    selectedType,

    // Computed
    filteredResources,

    // Actions - Init
    initStore,

    // Actions - Resources
    loadResources,
    loadResourcesPaginated,
    createResource,
    updateResource,
    deleteResource,
    batchDeleteResources,
    cloneResource,

    // Sorting
    setSort,
    toggleSortOrder,
    sortBy,
    sortOrder,

    // Pagination
    setPage,
    setPageSize,
    nextPage,
    prevPage,
    total,
    page,
    pageSize,
    totalPages,

    // Actions - Folders
    loadFolders,
    createFolder,
    addResourceToFolder,
    removeResourceFromFolder,
    getResourceFolders,

    // Tag helpers
    resourceTagMap,
    loadResourceTags,
    getResourceTags,

    // Actions - Tags
    loadTags,
    createTag,
    addTagToResource,
    removeTagFromResource,

    // Actions - Recycle Bin
    loadRecycleBin,
    restoreResource,
    permanentDeleteResource,

    // Selection
    selectResource,
    clearSelection,
    clearError,
    selectScope,
    selectType,
    selectFolder,

    // Settings
    settings,
    loadSettings,
    saveSettings,
    resetSettings,
    clearCache,

    // Version History
    getResourceVersions,

    // Tag
    getAnalyticsTag,

    // Tag Bidirectional
    getTagsForResource,
    getResourcesByTag,
  }

  // Version History
  async function getResourceVersions(resourceId: string) {
    try {
      return await analyticsApi.getResourceVersions(resourceId)
    } catch (error) {
      console.error('Failed to get resource versions:', error)
      throw error
    }
  }

  // Tag
  async function getAnalyticsTag(id: string) {
    try {
      return await analyticsApi.getAnalyticsTag(id)
    } catch (error) {
      console.error('Failed to get analytics tag:', error)
      throw error
    }
  }

  // Tag Bidirectional
  async function getTagsForResource(resourceId: string) {
    try {
      return await analyticsApi.getTagsForResource(resourceId)
    } catch (error) {
      console.error('Failed to get tags for resource:', error)
      throw error
    }
  }

  async function getResourcesByTag(tagId: string) {
    try {
      return await analyticsApi.getResourcesByTag(tagId)
    } catch (error) {
      console.error('Failed to get resources by tag:', error)
      throw error
    }
  }
})
