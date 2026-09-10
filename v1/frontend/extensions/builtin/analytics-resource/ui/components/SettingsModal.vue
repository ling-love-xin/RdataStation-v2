<template>
  <div class="modal-overlay" @click.self="$emit('close')">
    <div class="modal settings-modal">
      <div class="modal-header">
        <h3>⚙️ {{ t('analytics.settings') }}</h3>
        <button class="close-btn" @click="$emit('close')">✕</button>
      </div>

      <div class="modal-body">
        <div class="settings-tabs">
          <button
            v-for="tab in tabs"
            :key="tab.id"
            :class="['tab-btn', { active: activeTab === tab.id }]"
            @click="activeTab = tab.id"
          >
            {{ tab.icon }} {{ tab.label }}
          </button>
        </div>

        <div class="settings-content">
          <!-- 通用设置 -->
          <div v-if="activeTab === 'general'" class="settings-section">
            <h4>{{ t('analytics.generalSettings') }}</h4>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.defaultScope') }}</span>
                <span class="setting-description">{{ t('analytics.defaultScopeDesc') }}</span>
              </label>
              <select v-model="localSettings.general.defaultScope" class="form-input">
                <option value="project">📂 {{ t('analytics.scopeProject') }}</option>
                <option value="global">🌍 {{ t('analytics.scopeGlobal') }}</option>
                <option value="session">📌 {{ t('analytics.scopeSession') }}</option>
              </select>
            </div>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.pageSize') }}</span>
                <span class="setting-description">{{ t('analytics.pageSizeDesc') }}</span>
              </label>
              <select v-model.number="localSettings.general.defaultPageSize" class="form-input">
                <option :value="10">10</option>
                <option :value="20">20</option>
                <option :value="50">50</option>
                <option :value="100">100</option>
              </select>
            </div>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.defaultSortField') }}</span>
                <span class="setting-description">{{ t('analytics.defaultSortFieldDesc') }}</span>
              </label>
              <select v-model="localSettings.general.defaultSortField" class="form-input">
                <option value="name">{{ t('analytics.sortName') }}</option>
                <option value="created_at">{{ t('analytics.sortCreatedAt') }}</option>
                <option value="updated_at">{{ t('analytics.sortUpdatedAt') }}</option>
                <option value="row_count">{{ t('analytics.sortRowCount') }}</option>
              </select>
            </div>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.defaultSortDirection') }}</span>
              </label>
              <select v-model="localSettings.general.defaultSortOrder" class="form-input">
                <option value="asc">{{ t('analytics.sortAsc') }}</option>
                <option value="desc">{{ t('analytics.sortDesc') }}</option>
              </select>
            </div>
          </div>

          <!-- 显示设置 -->
          <div v-if="activeTab === 'display'" class="settings-section">
            <h4>{{ t('analytics.displaySettings') }}</h4>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.showResourceIcon') }}</span>
                <span class="setting-description">{{ t('analytics.showResourceIconDesc') }}</span>
              </label>
              <label class="toggle-switch">
                <input v-model="localSettings.display.showIcons" type="checkbox" />
                <span class="slider"></span>
              </label>
            </div>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.showScopeTag') }}</span>
                <span class="setting-description">{{ t('analytics.showScopeTagDesc') }}</span>
              </label>
              <label class="toggle-switch">
                <input v-model="localSettings.display.showScopeTags" type="checkbox" />
                <span class="slider"></span>
              </label>
            </div>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.showMetadata') }}</span>
                <span class="setting-description">{{ t('analytics.showMetadataDesc') }}</span>
              </label>
              <label class="toggle-switch">
                <input v-model="localSettings.display.showMetadata" type="checkbox" />
                <span class="slider"></span>
              </label>
            </div>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.enableVirtualScroll') }}</span>
                <span class="setting-description">{{
                  t('analytics.enableVirtualScrollDesc')
                }}</span>
              </label>
              <label class="toggle-switch">
                <input v-model="localSettings.display.enableVirtualScroll" type="checkbox" />
                <span class="slider"></span>
              </label>
            </div>
          </div>

          <!-- 缓存设置 -->
          <div v-if="activeTab === 'cache'" class="settings-section">
            <h4>{{ t('analytics.cacheSettings') }}</h4>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.enableQueryCache') }}</span>
                <span class="setting-description">{{ t('analytics.enableQueryCacheDesc') }}</span>
              </label>
              <label class="toggle-switch">
                <input v-model="localSettings.cache.enabled" type="checkbox" />
                <span class="slider"></span>
              </label>
            </div>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.cacheTtl') }}</span>
                <span class="setting-description">{{ t('analytics.cacheTtlDesc') }}</span>
              </label>
              <input
                v-model.number="localSettings.cache.ttlSeconds"
                type="number"
                min="10"
                max="3600"
                class="form-input"
              />
            </div>

            <div class="setting-item">
              <label class="setting-label">
                <span>{{ t('analytics.maxCacheSize') }}</span>
                <span class="setting-description">{{ t('analytics.maxCacheSizeDesc') }}</span>
              </label>
              <input
                v-model.number="localSettings.cache.maxSize"
                type="number"
                min="5"
                max="200"
                class="form-input"
              />
            </div>

            <button class="btn btn-secondary" @click="clearCache">
              🗑️ {{ t('analytics.clearCache') }}
            </button>
          </div>

          <!-- 快捷键设置 -->
          <div v-if="activeTab === 'shortcuts'" class="settings-section">
            <h4>{{ t('analytics.shortcuts') }}</h4>

            <div class="shortcuts-list">
              <div v-for="shortcut in shortcuts" :key="shortcut.key" class="shortcut-item">
                <span class="shortcut-action">{{ shortcut.label }}</span>
                <kbd class="shortcut-key">{{ shortcut.key }}</kbd>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button class="btn btn-secondary" @click="resetSettings">
          {{ t('analytics.resetDefault') }}
        </button>
        <button class="btn btn-primary" @click="handleSave">
          {{ t('analytics.saveSettings') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import type { AnalyticsResourceSettings } from '../../types'

const { t } = useI18n()

const props = defineProps<{
  settings: AnalyticsResourceSettings
}>()
const emit = defineEmits<{
  close: []
  save: [settings: AnalyticsResourceSettings]
  clearCache: []
}>()
const tabs = [
  { id: 'general', label: t('analytics.generalSettings'), icon: '⚙️' },
  { id: 'display', label: t('analytics.displaySettings'), icon: '🎨' },
  { id: 'cache', label: t('analytics.cacheSettings'), icon: '💾' },
  { id: 'shortcuts', label: t('analytics.shortcuts'), icon: '⌨️' },
]
const activeTab = ref('general')
const shortcuts = [
  { key: 'Ctrl+N', label: t('analytics.newResource') },
  { key: 'Ctrl+E', label: t('analytics.editResource') },
  { key: 'Ctrl+D', label: t('analytics.deleteResource') },
  { key: 'Ctrl+Shift+C', label: t('analytics.cloneResource') },
  { key: 'Ctrl+Shift+V', label: t('analytics.viewVersions') },
  { key: 'Ctrl+F', label: t('common.search') },
  { key: 'Ctrl+A', label: t('common.selectAll') },
  { key: 'Delete', label: t('common.deleteSelected') },
]
const localSettings = reactive<AnalyticsResourceSettings>(
  JSON.parse(JSON.stringify(props.settings))
)
watch(
  () => props.settings,
  newSettings => {
    Object.assign(localSettings, newSettings)
  },
  { deep: true }
)
function handleSave() {
  emit('save', JSON.parse(JSON.stringify(localSettings)))
}
function resetSettings() {
  Object.assign(localSettings, {
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
  })
}
function clearCache() {
  emit('clearCache')
}
</script>

<style scoped>
.settings-modal {
  width: 600px;
}

.settings-tabs {
  display: flex;
  gap: var(--size-sm);
  padding-bottom: var(--size-md);
  border-bottom: 1px solid var(--border-color);
  margin-bottom: var(--size-lg);
}

.tab-btn {
  padding: var(--size-sm) var(--size-md);
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--font-size-md);
  cursor: pointer;
  transition: all 0.2s;
}

.tab-btn:hover {
  background: var(--bg-secondary);
}

.tab-btn.active {
  background: var(--primary-light);
  color: var(--primary-color);
}

.settings-content {
  max-height: 400px;
  overflow-y: auto;
}

.settings-section h4 {
  font-size: var(--font-size-lg);
  font-weight: 600;
  color: var(--text-primary);
  margin-bottom: var(--size-md);
  padding-bottom: var(--size-sm);
  border-bottom: 1px solid var(--border-color-light);
}

.setting-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--size-md) 0;
  border-bottom: 1px solid var(--border-color-light);
}

.setting-label {
  display: flex;
  flex-direction: column;
  gap: var(--spacing-xs);
}

.setting-label span:first-child {
  font-size: var(--font-size-md);
  color: var(--text-primary);
  font-weight: 500;
}

.setting-description {
  font-size: var(--font-size-xs);
  color: var(--text-tertiary);
}

.toggle-switch {
  position: relative;
  display: inline-block;
  width: 48px;
  height: 26px;
  cursor: pointer;
}

.toggle-switch input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle-switch .slider {
  position: absolute;
  cursor: pointer;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background-color: var(--border-color);
  transition: 0.3s;
  border-radius: var(--border-radius-pill);
}

.toggle-switch .slider:before {
  position: absolute;
  content: '';
  height: 20px;
  width: 20px;
  left: 3px;
  bottom: 3px;
  background-color: white;
  transition: 0.3s;
  border-radius: 50%;
  box-shadow: var(--shadow-sm);
}

.toggle-switch input:checked + .slider {
  background-color: var(--primary-color);
}

.toggle-switch input:checked + .slider:before {
  transform: translateX(22px);
}

.shortcuts-list {
  display: flex;
  flex-direction: column;
  gap: var(--size-sm);
}

.shortcut-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--size-sm) var(--size-md);
  background: var(--bg-secondary);
  border-radius: var(--radius-md);
}

.shortcut-action {
  font-size: var(--font-size-md);
  color: var(--text-primary);
}

.shortcut-key {
  padding: 4px 8px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  font-family: var(--font-mono);
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
}
</style>
