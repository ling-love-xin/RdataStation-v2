import { setActivePinia, createPinia } from 'pinia'
import { describe, it, expect, beforeEach, beforeAll, vi } from 'vitest'

const localStorageMock = (() => {
  let store: Record<string, string> = {}
  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value
    },
    removeItem: (key: string) => {
      delete store[key]
    },
    clear: () => {
      store = {}
    },
  }
})()

beforeAll(() => {
  Object.defineProperty(globalThis, 'localStorage', {
    value: localStorageMock,
    writable: true,
  })
})

const mockApi = vi.hoisted(() => ({
  initAnalyticsResourceStore: vi.fn(),
  createAnalyticsResource: vi.fn(),
  updateAnalyticsResource: vi.fn(),
  getAnalyticsResource: vi.fn(),
  listAnalyticsResources: vi.fn(),
  listAnalyticsResourcesPaginated: vi.fn(),
  deleteAnalyticsResource: vi.fn(),
  batchDeleteResources: vi.fn(),
  cloneAnalyticsResource: vi.fn(),
  createAnalyticsFolder: vi.fn(),
  getAnalyticsFolder: vi.fn(),
  listAnalyticsFolders: vi.fn(),
  addAnalyticsResourceToFolder: vi.fn(),
  removeAnalyticsResourceFromFolder: vi.fn(),
  createAnalyticsTag: vi.fn(),
  getAnalyticsTag: vi.fn(),
  listAnalyticsTags: vi.fn(),
  addAnalyticsTagToResource: vi.fn(),
  removeAnalyticsTagFromResource: vi.fn(),
  getAnalyticsRecycleBin: vi.fn(),
  restoreAnalyticsResourceFromRecycle: vi.fn(),
  permanentDeleteAnalyticsResource: vi.fn(),
  getResourceVersions: vi.fn(),
  getTagsForResource: vi.fn(),
  getResourcesByTag: vi.fn(),
}))

vi.mock(
  '../../src/extensions/builtin/analytics-resource/infrastructure/api/analytics-resource-api',
  () => mockApi
)

import { useAnalyticsResourceStore } from '../../src/extensions/builtin/analytics-resource/ui/stores/analytics-resource-store'

import type {
  AnalyticsResource,
  AnalyticsFolder,
  AnalyticsTag,
  AnalyticsRecycleItem,
  CreateResourceRequest,
  CreateFolderRequest,
  CreateTagRequest,
  ResourceVersion,
} from '../../src/extensions/builtin/analytics-resource/types'

function makeResource(overrides: Partial<AnalyticsResource> = {}): AnalyticsResource {
  return {
    id: 'ar_test1',
    resource_type: 'table',
    name: 'test_table',
    config: {},
    scope: 'project',
    version: 1,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

function makeFolder(overrides: Partial<AnalyticsFolder> = {}): AnalyticsFolder {
  return {
    id: 'af_test1',
    name: 'test_folder',
    scope: 'project',
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

function makeTag(overrides: Partial<AnalyticsTag> = {}): AnalyticsTag {
  return {
    id: 'at_test1',
    name: 'test_tag',
    scope: 'project',
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

function makeRecycleItem(overrides: Partial<AnalyticsRecycleItem> = {}): AnalyticsRecycleItem {
  return {
    id: 'rb_test1',
    resource_id: 'ar_test1',
    resource_type: 'table',
    resource_name: 'deleted_table',
    resource_data: {},
    deleted_at: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

describe('AnalyticsResourceStore - Async CRUD Methods', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  describe('createResource', () => {
    it('should prepend created resource to list', async () => {
      const store = useAnalyticsResourceStore()
      const created = makeResource({ id: 'ar_new', name: 'new_table' })
      mockApi.createAnalyticsResource.mockResolvedValueOnce(created)

      const input: CreateResourceRequest = {
        resource_type: 'table',
        name: 'new_table',
        config: {},
        scope: 'project',
      }
      const result = await store.createResource(input)

      expect(result).toEqual(created)
      expect(store.resources).toHaveLength(1)
      expect(store.resources[0].id).toBe('ar_new')
      expect(store.total).toBe(1)
      expect(mockApi.createAnalyticsResource).toHaveBeenCalledWith(input)
    })

    it('should set loading true during creation', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.createAnalyticsResource.mockImplementationOnce(async () => {
        expect(store.loading).toBe(true)
        return makeResource()
      })

      await store.createResource({
        resource_type: 'table',
        name: 't',
        config: {},
        scope: 'project',
      })
      expect(store.loading).toBe(false)
    })

    it('should propagate API errors', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.createAnalyticsResource.mockRejectedValueOnce(new Error('API error'))

      await expect(
        store.createResource({
          resource_type: 'table',
          name: 't',
          config: {},
          scope: 'project',
        })
      ).rejects.toThrow('API error')
    })
  })

  describe('updateResource', () => {
    it('should replace resource in list', async () => {
      const store = useAnalyticsResourceStore()
      store.resources = [makeResource({ id: 'ar_1', name: 'old_name' })]
      store.total = 1

      const updated = makeResource({ id: 'ar_1', name: 'new_name', version: 2 })
      mockApi.updateAnalyticsResource.mockResolvedValueOnce(updated)

      const input: CreateResourceRequest = {
        resource_type: 'table',
        name: 'new_name',
        config: {},
        scope: 'project',
      }
      const result = await store.updateResource('ar_1', input)

      expect(result).toEqual(updated)
      expect(store.resources[0].name).toBe('new_name')
      expect(mockApi.updateAnalyticsResource).toHaveBeenCalledWith('ar_1', input)
    })

    it('should not modify list if id not found', async () => {
      const store = useAnalyticsResourceStore()
      store.resources = [makeResource({ id: 'ar_1' })]

      mockApi.updateAnalyticsResource.mockResolvedValueOnce(makeResource({ id: 'ar_unknown' }))

      await store.updateResource('ar_unknown', {
        resource_type: 'table',
        name: 'x',
        config: {},
        scope: 'project',
      })
      expect(store.resources[0].id).toBe('ar_1')
    })
  })

  describe('deleteResource', () => {
    it('should remove resource and decrement total', async () => {
      const store = useAnalyticsResourceStore()
      store.resources = [makeResource({ id: 'ar_1' }), makeResource({ id: 'ar_2' })]
      store.total = 2

      mockApi.deleteAnalyticsResource.mockResolvedValueOnce(undefined)

      await store.deleteResource('ar_1')
      expect(store.resources).toHaveLength(1)
      expect(store.resources[0].id).toBe('ar_2')
      expect(store.total).toBe(1)
    })

    it('should clear selection for deleted resource', async () => {
      const store = useAnalyticsResourceStore()
      store.resources = [makeResource({ id: 'ar_1' })]
      store.selectedResources = ['ar_1']
      store.total = 1

      mockApi.deleteAnalyticsResource.mockResolvedValueOnce(undefined)

      await store.deleteResource('ar_1')
      expect(store.selectedResources).toHaveLength(0)
    })
  })

  describe('batchDeleteResources', () => {
    it('should remove multiple resources', async () => {
      const store = useAnalyticsResourceStore()
      store.resources = [
        makeResource({ id: 'ar_1' }),
        makeResource({ id: 'ar_2' }),
        makeResource({ id: 'ar_3' }),
      ]
      store.selectedResources = ['ar_1', 'ar_2']
      store.total = 3

      mockApi.batchDeleteResources.mockResolvedValueOnce(undefined)

      await store.batchDeleteResources(['ar_1', 'ar_2'])
      expect(store.resources).toHaveLength(1)
      expect(store.resources[0].id).toBe('ar_3')
      expect(store.selectedResources).toHaveLength(0)
      expect(store.total).toBe(1)
    })
  })

  describe('cloneResource', () => {
    it('should prepend cloned resource', async () => {
      const store = useAnalyticsResourceStore()
      store.resources = [makeResource({ id: 'ar_1', name: 'original' })]
      store.total = 1

      const cloned = makeResource({ id: 'ar_cloned', name: 'original (副本)' })
      mockApi.cloneAnalyticsResource.mockResolvedValueOnce(cloned)

      const result = await store.cloneResource('ar_1', 'original (副本)')
      expect(result).toEqual(cloned)
      expect(store.resources).toHaveLength(2)
      expect(store.resources[0].id).toBe('ar_cloned')
      expect(store.total).toBe(2)
    })
  })
})

describe('AnalyticsResourceStore - Async Folder Methods', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  describe('loadFolders', () => {
    it('should populate folders from API', async () => {
      const store = useAnalyticsResourceStore()
      const folders = [makeFolder({ id: 'af_1' }), makeFolder({ id: 'af_2' })]
      mockApi.listAnalyticsFolders.mockResolvedValueOnce(folders)

      await store.loadFolders()
      expect(store.folders).toEqual(folders)
    })
  })

  describe('createFolder', () => {
    it('should prepend folder to list', async () => {
      const store = useAnalyticsResourceStore()
      const folder = makeFolder({ id: 'af_new', name: 'new_folder' })
      mockApi.createAnalyticsFolder.mockResolvedValueOnce(folder)

      const input: CreateFolderRequest = { name: 'new_folder', scope: 'project' }
      const result = await store.createFolder(input)

      expect(result).toEqual(folder)
      expect(store.folders).toHaveLength(1)
      expect(store.folders[0].id).toBe('af_new')
    })
  })

  describe('addResourceToFolder', () => {
    it('should update resource folder map on add', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.addAnalyticsResourceToFolder.mockResolvedValueOnce(undefined)

      await store.addResourceToFolder('ar_1', 'af_1')
      expect(store.getResourceFolders('ar_1')).toContain('af_1')
    })

    it('should not duplicate entries', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.addAnalyticsResourceToFolder.mockResolvedValueOnce(undefined)
      mockApi.addAnalyticsResourceToFolder.mockResolvedValueOnce(undefined)

      await store.addResourceToFolder('ar_1', 'af_1')
      await store.addResourceToFolder('ar_1', 'af_1')
      expect(store.getResourceFolders('ar_1')).toEqual(['af_1'])
    })
  })

  describe('removeResourceFromFolder', () => {
    it('should update resource folder map on remove', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.addAnalyticsResourceToFolder.mockResolvedValueOnce(undefined)
      mockApi.removeAnalyticsResourceFromFolder.mockResolvedValueOnce(undefined)

      await store.addResourceToFolder('ar_1', 'af_1')
      await store.removeResourceFromFolder('ar_1', 'af_1')
      expect(store.getResourceFolders('ar_1')).toHaveLength(0)
    })
  })

  describe('getResourceFolders', () => {
    it('should return empty array for unmapped resource', () => {
      const store = useAnalyticsResourceStore()
      expect(store.getResourceFolders('unknown')).toEqual([])
    })
  })
})

describe('AnalyticsResourceStore - Async Tag Methods', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  describe('loadTags', () => {
    it('should populate tags from API', async () => {
      const store = useAnalyticsResourceStore()
      const tags = [makeTag({ id: 'at_1' }), makeTag({ id: 'at_2' })]
      mockApi.listAnalyticsTags.mockResolvedValueOnce(tags)

      await store.loadTags()
      expect(store.tags).toEqual(tags)
    })
  })

  describe('createTag', () => {
    it('should prepend tag to list', async () => {
      const store = useAnalyticsResourceStore()
      const tag = makeTag({ id: 'at_new', name: 'new_tag' })
      mockApi.createAnalyticsTag.mockResolvedValueOnce(tag)

      const input: CreateTagRequest = { name: 'new_tag', scope: 'project' }
      const result = await store.createTag(input)

      expect(result).toEqual(tag)
      expect(store.tags).toHaveLength(1)
      expect(store.tags[0].id).toBe('at_new')
    })
  })

  describe('loadResourceTags', () => {
    it('should populate tag map from API', async () => {
      const store = useAnalyticsResourceStore()
      const tags = [makeTag({ id: 'at_1', name: 'important' })]
      mockApi.getTagsForResource.mockResolvedValueOnce(tags)

      await store.loadResourceTags(['ar_1'])
      expect(store.getResourceTags('ar_1')).toEqual(tags)
    })

    it('should handle partial failures gracefully', async () => {
      const store = useAnalyticsResourceStore()
      const tags = [makeTag({ id: 'at_1' })]
      mockApi.getTagsForResource
        .mockResolvedValueOnce(tags)
        .mockRejectedValueOnce(new Error('not found'))

      await store.loadResourceTags(['ar_1', 'ar_missing'])
      expect(store.getResourceTags('ar_1')).toEqual(tags)
      expect(store.getResourceTags('ar_missing')).toEqual([])
    })
  })

  describe('addTagToResource', () => {
    it('should call API successfully', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.addAnalyticsTagToResource.mockResolvedValueOnce(undefined)

      await store.addTagToResource('ar_1', 'at_1')
      expect(mockApi.addAnalyticsTagToResource).toHaveBeenCalledWith('ar_1', 'at_1')
    })

    it('should propagate errors', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.addAnalyticsTagToResource.mockRejectedValueOnce(new Error('tag error'))

      await expect(store.addTagToResource('ar_1', 'at_1')).rejects.toThrow('tag error')
    })
  })

  describe('removeTagFromResource', () => {
    it('should call API successfully', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.removeAnalyticsTagFromResource.mockResolvedValueOnce(undefined)

      await store.removeTagFromResource('ar_1', 'at_1')
      expect(mockApi.removeAnalyticsTagFromResource).toHaveBeenCalledWith('ar_1', 'at_1')
    })
  })

  describe('getAnalyticsTag', () => {
    it('should return tag from API', async () => {
      const store = useAnalyticsResourceStore()
      const tag = makeTag({ id: 'at_1' })
      mockApi.getAnalyticsTag.mockResolvedValueOnce(tag)

      const result = await store.getAnalyticsTag('at_1')
      expect(result).toEqual(tag)
    })
  })

  describe('getTagsForResource', () => {
    it('should return tags from API for a single resource', async () => {
      const store = useAnalyticsResourceStore()
      const tags = [makeTag({ id: 'at_1' })]
      mockApi.getTagsForResource.mockResolvedValueOnce(tags)

      const result = await store.getTagsForResource('ar_1')
      expect(result).toEqual(tags)
    })
  })

  describe('getResourcesByTag', () => {
    it('should return resources filtered by tag', async () => {
      const store = useAnalyticsResourceStore()
      const resources = [makeResource({ id: 'ar_1' })]
      mockApi.getResourcesByTag.mockResolvedValueOnce(resources)

      const result = await store.getResourcesByTag('at_1')
      expect(result).toEqual(resources)
    })
  })
})

describe('AnalyticsResourceStore - Async Recycle Bin Methods', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  describe('loadRecycleBin', () => {
    it('should populate recycle bin from API', async () => {
      const store = useAnalyticsResourceStore()
      const items = [makeRecycleItem(), makeRecycleItem({ id: 'rb_2' })]
      mockApi.getAnalyticsRecycleBin.mockResolvedValueOnce(items)

      await store.loadRecycleBin()
      expect(store.recycleBin).toEqual(items)
    })
  })

  describe('restoreResource', () => {
    it('should prepend restored resource and remove from bin', async () => {
      const store = useAnalyticsResourceStore()
      store.recycleBin = [makeRecycleItem({ id: 'rb_1', resource_id: 'ar_restored' })]

      const restored = makeResource({ id: 'ar_restored', name: 'restored_table' })
      mockApi.restoreAnalyticsResourceFromRecycle.mockResolvedValueOnce(restored)

      const result = await store.restoreResource('rb_1')
      expect(result).toEqual(restored)
      expect(store.resources[0].id).toBe('ar_restored')
      expect(store.recycleBin).toHaveLength(0)
    })
  })

  describe('permanentDeleteResource', () => {
    it('should remove item from recycle bin', async () => {
      const store = useAnalyticsResourceStore()
      store.recycleBin = [makeRecycleItem({ id: 'rb_1' }), makeRecycleItem({ id: 'rb_2' })]

      mockApi.permanentDeleteAnalyticsResource.mockResolvedValueOnce(undefined)

      await store.permanentDeleteResource('rb_1')
      expect(store.recycleBin).toHaveLength(1)
      expect(store.recycleBin[0].id).toBe('rb_2')
    })
  })
})

describe('AnalyticsResourceStore - Async Init & Version Methods', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  describe('initStore', () => {
    it('should initialize once and load data', async () => {
      const store = useAnalyticsResourceStore()
      const resources = [makeResource()]
      const folders = [makeFolder()]
      const tags = [makeTag()]

      mockApi.initAnalyticsResourceStore.mockResolvedValueOnce(undefined)
      mockApi.listAnalyticsResources.mockResolvedValueOnce(resources)
      mockApi.listAnalyticsFolders.mockResolvedValueOnce(folders)
      mockApi.listAnalyticsTags.mockResolvedValueOnce(tags)

      await store.initStore()

      expect(store.initialized).toBe(true)
      expect(store.resources).toEqual(resources)
      expect(store.folders).toEqual(folders)
      expect(store.tags).toEqual(tags)
    })

    it('should skip re-initialization', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.initAnalyticsResourceStore.mockResolvedValueOnce(undefined)

      await store.initStore()
      await store.initStore()

      expect(mockApi.initAnalyticsResourceStore).toHaveBeenCalledTimes(1)
    })

    it('should catch initialization errors', async () => {
      const store = useAnalyticsResourceStore()
      mockApi.initAnalyticsResourceStore.mockRejectedValueOnce(new Error('init failed'))

      await expect(store.initStore()).rejects.toThrow('init failed')
    })
  })

  describe('getResourceVersions', () => {
    it('should return version history from API', async () => {
      const store = useAnalyticsResourceStore()
      const versions: ResourceVersion[] = [
        { id: 'arv_1', resource_id: 'ar_1', version: 1, snapshot: {}, created_at: '2026-01-01' },
        { id: 'arv_2', resource_id: 'ar_1', version: 2, snapshot: {}, created_at: '2026-01-02' },
      ]
      mockApi.getResourceVersions.mockResolvedValueOnce(versions)

      const result = await store.getResourceVersions('ar_1')
      expect(result).toEqual(versions)
    })
  })

  describe('loadResourcesPaginated', () => {
    it('should update pagination state from API', async () => {
      const store = useAnalyticsResourceStore()
      const output = {
        items: [makeResource({ id: 'ar_1' }), makeResource({ id: 'ar_2' })],
        total: 50,
        page: 1,
        pageSize: 20,
        totalPages: 3,
      }
      mockApi.listAnalyticsResourcesPaginated.mockResolvedValueOnce(output)

      await store.loadResourcesPaginated()

      expect(store.resources).toHaveLength(2)
      expect(store.total).toBe(50)
      expect(store.page).toBe(1)
      expect(store.totalPages).toBe(3)
    })
  })
})

describe('AnalyticsResourceStore - State Mutations', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  describe('filteredResources computed', () => {
    it('should filter by scope', () => {
      const store = useAnalyticsResourceStore()
      store.resources = [
        makeResource({ id: 'ar_1', scope: 'project' }),
        makeResource({ id: 'ar_2', scope: 'global' }),
      ]

      store.selectScope('project')
      expect(store.filteredResources).toHaveLength(1)
      expect(store.filteredResources[0].id).toBe('ar_1')
    })

    it('should filter by type', () => {
      const store = useAnalyticsResourceStore()
      store.resources = [
        makeResource({ id: 'ar_1', resource_type: 'table' }),
        makeResource({ id: 'ar_2', resource_type: 'connection' }),
      ]

      store.selectType('table')
      expect(store.filteredResources).toHaveLength(1)
      expect(store.filteredResources[0].id).toBe('ar_1')
    })

    it('should filter by folder', async () => {
      const store = useAnalyticsResourceStore()
      store.resources = [makeResource({ id: 'ar_1' }), makeResource({ id: 'ar_2' })]
      mockApi.addAnalyticsResourceToFolder.mockResolvedValueOnce(undefined)

      await store.addResourceToFolder('ar_1', 'af_1')
      store.selectFolder('af_1')

      expect(store.filteredResources).toHaveLength(1)
      expect(store.filteredResources[0].id).toBe('ar_1')
    })

    it('should return all when no filters active', () => {
      const store = useAnalyticsResourceStore()
      store.resources = [makeResource({ id: 'ar_1' }), makeResource({ id: 'ar_2' })]

      expect(store.filteredResources).toHaveLength(2)
    })
  })
})
