import { setActivePinia, createPinia } from 'pinia'
import { describe, it, expect, beforeEach, afterEach, beforeAll } from 'vitest'

import { useAnalyticsResourceStore } from '../../src/extensions/builtin/analytics-resource/ui/stores/analytics-resource-store'

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

describe('AnalyticsResourceStore - Local Methods', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  afterEach(() => {
    localStorage.clear()
  })

  describe('Pagination', () => {
    it('should set page within valid range', () => {
      const store = useAnalyticsResourceStore()
      store.total = 100
      store.setPage(3)
      expect(store.page).toBe(3)
    })

    it('should not set page beyond total pages', () => {
      const store = useAnalyticsResourceStore()
      store.total = 100
      store.setPage(10)
      expect(store.page).not.toBe(10)
    })

    it('should reset page when page size changes', () => {
      const store = useAnalyticsResourceStore()
      store.total = 100
      store.setPage(3)
      store.setPageSize(20)
      expect(store.page).toBe(1)
      expect(store.pageSize).toBe(20)
    })

    it('nextPage should increment page', () => {
      const store = useAnalyticsResourceStore()
      store.total = 100
      store.nextPage()
      expect(store.page).toBe(2)
    })

    it('nextPage should not exceed total pages', () => {
      const store = useAnalyticsResourceStore()
      store.total = 20
      store.nextPage()
      expect(store.page).toBe(1)
    })

    it('prevPage should decrement page', () => {
      const store = useAnalyticsResourceStore()
      store.total = 100
      store.setPage(3)
      store.prevPage()
      expect(store.page).toBe(2)
    })

    it('prevPage should not go below 1', () => {
      const store = useAnalyticsResourceStore()
      store.prevPage()
      expect(store.page).toBe(1)
    })
  })

  describe('Sorting', () => {
    it('should set sort field and order', () => {
      const store = useAnalyticsResourceStore()
      store.setSort('name', 'asc')
      expect(store.sortBy).toBe('name')
      expect(store.sortOrder).toBe('asc')
    })

    it('should toggle sort order when same field selected', () => {
      const store = useAnalyticsResourceStore()
      store.sortBy = 'name'
      store.sortOrder = 'asc'
      store.setSort('name')
      expect(store.sortOrder).toBe('desc')
    })

    it('toggleSortOrder should flip order', () => {
      const store = useAnalyticsResourceStore()
      store.sortOrder = 'asc'
      store.toggleSortOrder()
      expect(store.sortOrder).toBe('desc')
    })
  })

  describe('Selection', () => {
    it('should select a single resource', () => {
      const store = useAnalyticsResourceStore()
      store.selectResource('res-1')
      expect(store.selectedResources).toContain('res-1')
      expect(store.selectedResources).toHaveLength(1)
    })

    it('should clear previous selection when selecting single', () => {
      const store = useAnalyticsResourceStore()
      store.selectedResources = ['res-1', 'res-2']
      store.selectResource('res-3')
      expect(store.selectedResources).toEqual(['res-3'])
    })

    it('should toggle selection in multiple mode', () => {
      const store = useAnalyticsResourceStore()
      store.selectResource('res-1', true)
      store.selectResource('res-2', true)
      expect(store.selectedResources).toContain('res-1')
      expect(store.selectedResources).toContain('res-2')
      store.selectResource('res-1', true)
      expect(store.selectedResources).not.toContain('res-1')
      expect(store.selectedResources).toContain('res-2')
    })

    it('clearSelection should empty all selected', () => {
      const store = useAnalyticsResourceStore()
      store.selectedResources = ['res-1', 'res-2']
      store.clearSelection()
      expect(store.selectedResources).toHaveLength(0)
    })

    it('should set selection filters', () => {
      const store = useAnalyticsResourceStore()
      store.selectScope('project')
      store.selectType('table')
      store.selectFolder('folder-1')
      expect(store.selectedScope).toBe('project')
      expect(store.selectedType).toBe('table')
      expect(store.selectedFolderId).toBe('folder-1')
    })
  })

  describe('Settings', () => {
    it('should load default settings when localStorage is empty', () => {
      const store = useAnalyticsResourceStore()
      store.loadSettings()
      expect(store.settings).toBeDefined()
      expect(store.settings.general).toBeDefined()
    })

    it('should persist settings to localStorage via saveSettings', () => {
      const store = useAnalyticsResourceStore()
      const newSettings = {
        ...store.settings,
        general: {
          ...store.settings.general,
          defaultPageSize: 25,
        },
      }
      store.saveSettings(newSettings)
      expect(store.settings.general.defaultPageSize).toBe(25)

      const raw = localStorage.getItem('analytics_resource_settings')
      expect(raw).toBeTruthy()
      if (raw) {
        const parsed = JSON.parse(raw)
        expect(parsed.general.defaultPageSize).toBe(25)
      }
    })

    it('should restore defaults on reset', () => {
      const store = useAnalyticsResourceStore()
      const customSettings = {
        ...store.settings,
        general: { ...store.settings.general, defaultPageSize: 100 },
      }
      store.saveSettings(customSettings)
      expect(store.settings.general.defaultPageSize).toBe(100)

      store.resetSettings()
      expect(store.settings.general.defaultPageSize).toBe(20)

      const raw = localStorage.getItem('analytics_resource_settings')
      if (raw) {
        const parsed = JSON.parse(raw)
        expect(parsed.general.defaultPageSize).toBe(20)
      }
    })
  })
})
