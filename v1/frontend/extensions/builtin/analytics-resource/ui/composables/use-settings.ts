import { DEFAULT_SETTINGS } from '../../types'

import type { AnalyticsResourceSettings } from '../../types'
import type { Ref } from 'vue'

export function useStoreSettings(
  settings: Ref<AnalyticsResourceSettings>,
  resourceCache: {
    updateConfig: (opts: { enabled: boolean; ttl: number; maxSize: number }) => void
    clear: () => void
  },
  folderCache: {
    updateConfig: (opts: { enabled: boolean; ttl: number; maxSize: number }) => void
    clear: () => void
  },
  pageSize: Ref<number>,
  sortBy: Ref<string | null>,
  sortOrder: Ref<string>
) {
  function applySettingsToState() {
    const s = settings.value
    pageSize.value = s.general.defaultPageSize
    sortBy.value = s.general.defaultSortField
    sortOrder.value = s.general.defaultSortOrder || 'desc'
    resourceCache.updateConfig({
      enabled: s.cache.enabled,
      ttl: s.cache.ttlSeconds * 1000,
      maxSize: s.cache.maxSize,
    })
    folderCache.updateConfig({
      enabled: s.cache.enabled,
      ttl: s.cache.ttlSeconds * 1000,
      maxSize: s.cache.maxSize,
    })
  }

  function loadSettings() {
    const stored = localStorage.getItem('analytics_resource_settings')
    if (stored) {
      try {
        settings.value = JSON.parse(stored)
      } catch {
        settings.value = { ...DEFAULT_SETTINGS }
      }
    }
  }

  function saveSettings(newSettings: AnalyticsResourceSettings) {
    settings.value = newSettings
    localStorage.setItem('analytics_resource_settings', JSON.stringify(newSettings))
    applySettingsToState()
  }

  function resetSettings() {
    settings.value = { ...DEFAULT_SETTINGS }
    localStorage.setItem('analytics_resource_settings', JSON.stringify(settings.value))
    applySettingsToState()
  }

  function clearCache() {
    resourceCache.clear()
    folderCache.clear()
  }

  return {
    settings,
    applySettingsToState,
    loadSettings,
    saveSettings,
    resetSettings,
    clearCache,
  }
}
