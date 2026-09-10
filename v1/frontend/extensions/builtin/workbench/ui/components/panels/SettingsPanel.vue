<template>
  <div class="settings-panel">
    <div class="settings-header">
      <h2>{{ t('workbench.settingsTitle') }}</h2>
    </div>

    <div class="settings-nav">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        class="nav-item"
        :class="{ active: activeTab === tab.id }"
        @click="activeTab = tab.id"
      >
        <component :is="tab.icon" :size="16" />
        <span>{{ tab.label }}</span>
      </button>
    </div>

    <div class="settings-content">
      <!-- 连接池设置 -->
      <div v-if="activeTab === 'connection-pool'" class="settings-section">
        <h3>{{ t('workbench.connectionPool') }}</h3>

        <div class="setting-item">
          <label>{{ t('workbench.maxConnections') }}</label>
          <input
            v-model.number="settings.connectionPool.maxConnections"
            type="number"
            min="1"
            max="100"
          />
          <span class="hint">{{ t('workbench.maxConnectionsHint') }}</span>
        </div>

        <div class="setting-item">
          <label>{{ t('workbench.minIdleConnections') }}</label>
          <input
            v-model.number="settings.connectionPool.minIdleConnections"
            type="number"
            min="0"
            max="50"
          />
          <span class="hint">{{ t('workbench.minIdleConnectionsHint') }}</span>
        </div>

        <div class="setting-item">
          <label>{{ t('workbench.connectionTimeout') }}</label>
          <input
            v-model.number="settings.connectionPool.connectionTimeout"
            type="number"
            min="1"
            max="300"
          />
          <span class="hint">{{ t('workbench.connectionTimeoutHint') }}</span>
        </div>

        <div class="setting-item">
          <label>{{ t('workbench.idleTimeout') }}</label>
          <input
            v-model.number="settings.connectionPool.idleTimeout"
            type="number"
            min="10"
            max="3600"
          />
          <span class="hint">{{ t('workbench.idleTimeoutHint') }}</span>
        </div>

        <div class="setting-item">
          <label>
            <input v-model="settings.connectionPool.autoReconnect" type="checkbox" />
            {{ t('workbench.autoReconnect') }}
          </label>
          <span class="hint">{{ t('workbench.autoReconnectHint') }}</span>
        </div>

        <div class="setting-item">
          <label>
            <input v-model="settings.connectionPool.healthCheck" type="checkbox" />
            {{ t('workbench.healthCheck') }}
          </label>
          <span class="hint">{{ t('workbench.healthCheckHint') }}</span>
        </div>

        <div v-if="settings.connectionPool.healthCheck" class="setting-item">
          <label>{{ t('workbench.healthCheckInterval') }}</label>
          <input
            v-model.number="settings.connectionPool.healthCheckInterval"
            type="number"
            min="10"
            max="300"
          />
          <span class="hint">{{ t('workbench.healthCheckIntervalHint') }}</span>
        </div>
      </div>

      <!-- 操作历史设置 -->
      <div v-if="activeTab === 'history'" class="settings-section">
        <h3>{{ t('workbench.historySettings') }}</h3>

        <div class="setting-item">
          <label>{{ t('workbench.maxHistoryItems') }}</label>
          <input
            v-model.number="settings.history.maxHistoryItems"
            type="number"
            min="10"
            max="1000"
          />
          <span class="hint">{{ t('workbench.maxHistoryItemsHint') }}</span>
        </div>

        <div class="setting-item">
          <label>{{ t('workbench.retentionDays') }}</label>
          <input v-model.number="settings.history.retentionDays" type="number" min="1" max="365" />
          <span class="hint">{{ t('workbench.retentionDaysHint') }}</span>
        </div>

        <div class="setting-item">
          <label>
            <input v-model="settings.history.enableHistory" type="checkbox" />
            {{ t('workbench.enableHistory') }}
          </label>
          <span class="hint">{{ t('workbench.enableHistoryHint') }}</span>
        </div>

        <div class="setting-item">
          <label>
            <input v-model="settings.history.includeSQL" type="checkbox" />
            {{ t('workbench.includeSQL') }}
          </label>
          <span class="hint">{{ t('workbench.includeSQLHint') }}</span>
        </div>

        <div class="setting-item">
          <label>
            <input v-model="settings.history.enableUndo" type="checkbox" />
            {{ t('workbench.enableUndo') }}
          </label>
          <span class="hint">{{ t('workbench.enableUndoHint') }}</span>
        </div>

        <button class="btn-clear-history" @click="clearHistory">
          <Trash2 :size="14" />
          {{ t('workbench.clearAllHistory') }}
        </button>
      </div>

      <!-- 健康监控设置 -->
      <div v-if="activeTab === 'monitoring'" class="settings-section">
        <h3>{{ t('workbench.monitoringSettings') }}</h3>

        <div class="setting-item">
          <label>
            <input v-model="settings.monitoring.enableMonitoring" type="checkbox" />
            {{ t('workbench.enableMonitoring') }}
          </label>
          <span class="hint">{{ t('workbench.enableMonitoringHint') }}</span>
        </div>

        <div v-if="settings.monitoring.enableMonitoring" class="setting-item">
          <label>{{ t('workbench.updateInterval') }}</label>
          <input
            v-model.number="settings.monitoring.updateInterval"
            type="number"
            min="1"
            max="60"
          />
          <span class="hint">{{ t('workbench.updateIntervalHint') }}</span>
        </div>

        <div class="setting-item">
          <label>
            <input v-model="settings.monitoring.enableAlerts" type="checkbox" />
            {{ t('workbench.enableAlerts') }}
          </label>
          <span class="hint">{{ t('workbench.enableAlertsHint') }}</span>
        </div>

        <div v-if="settings.monitoring.enableAlerts" class="setting-item">
          <label>
            <input v-model="settings.monitoring.alertOnDisconnect" type="checkbox" />
            {{ t('workbench.alertOnDisconnect') }}
          </label>
        </div>

        <div v-if="settings.monitoring.enableAlerts" class="setting-item">
          <label>
            <input v-model="settings.monitoring.alertOnSlowQuery" type="checkbox" />
            {{ t('workbench.alertOnSlowQuery') }}
          </label>
          <span class="hint">{{ t('workbench.alertOnSlowQueryHint') }}</span>
        </div>

        <div v-if="settings.monitoring.alertOnSlowQuery" class="setting-item">
          <label>{{ t('workbench.slowQueryThreshold') }}</label>
          <input
            v-model.number="settings.monitoring.slowQueryThreshold"
            type="number"
            min="100"
            max="30000"
          />
          <span class="hint">{{ t('workbench.slowQueryThresholdHint') }}</span>
        </div>
      </div>

      <!-- 性能设置 -->
      <div v-if="activeTab === 'performance'" class="settings-section">
        <h3>{{ t('workbench.performanceSettings') }}</h3>

        <div class="setting-item">
          <label>{{ t('workbench.virtualScrollBuffer') }}</label>
          <input
            v-model.number="settings.performance.virtualScrollBuffer"
            type="number"
            min="1"
            max="20"
          />
          <span class="hint">{{ t('workbench.virtualScrollBufferHint') }}</span>
        </div>

        <div class="setting-item">
          <label>{{ t('workbench.maxCacheSize') }}</label>
          <input
            v-model.number="settings.performance.maxCacheSize"
            type="number"
            min="10"
            max="500"
          />
          <span class="hint">{{ t('workbench.maxCacheSizeHint') }}</span>
        </div>

        <div class="setting-item">
          <label>{{ t('workbench.cacheExpireMinutes') }}</label>
          <input
            v-model.number="settings.performance.cacheExpireMinutes"
            type="number"
            min="5"
            max="1440"
          />
          <span class="hint">{{ t('workbench.cacheExpireMinutesHint') }}</span>
        </div>

        <div class="setting-item">
          <label>
            <input v-model="settings.performance.enableLazyLoad" type="checkbox" />
            {{ t('workbench.enableLazyLoad') }}
          </label>
          <span class="hint">{{ t('workbench.enableLazyLoadHint') }}</span>
        </div>

        <div class="setting-item">
          <label>
            <input v-model="settings.performance.enablePreload" type="checkbox" />
            {{ t('workbench.enablePreload') }}
          </label>
          <span class="hint">{{ t('workbench.enablePreloadHint') }}</span>
        </div>
      </div>

      <!-- 快捷键设置 -->
      <div v-if="activeTab === 'shortcuts'" class="settings-section">
        <h3>{{ t('workbench.shortcutsSettings') }}</h3>

        <div class="shortcuts-list">
          <div v-for="shortcut in shortcuts" :key="shortcut.key" class="shortcut-item">
            <span class="shortcut-name">{{ shortcut.name }}</span>
            <input v-model="shortcut.value" type="text" class="shortcut-input" readonly />
            <button class="btn-edit-shortcut">{{ t('workbench.editShortcut') }}</button>
          </div>
        </div>

        <button class="btn-reset-shortcuts">
          <RotateCcw :size="14" />
          {{ t('workbench.resetShortcuts') }}
        </button>
      </div>

      <!-- 外观设置 -->
      <div v-if="activeTab === 'appearance'" class="settings-section">
        <h3>{{ t('workbench.appearanceSettings') }}</h3>

        <div class="setting-item">
          <label>{{ t('workbench.theme') }}</label>
          <div class="theme-options">
            <button
              v-for="theme in themes"
              :key="theme.id"
              class="theme-option"
              :class="{ active: settings.appearance.theme === theme.id }"
              @click="settings.appearance.theme = theme.id"
            >
              <div class="theme-preview" :class="theme.id"></div>
              <span>{{ theme.name }}</span>
            </button>
          </div>
        </div>

        <div class="setting-item">
          <label>{{ t('workbench.fontSize') }}</label>
          <select v-model="settings.appearance.fontSize">
            <option :value="12">12px</option>
            <option :value="13">13px</option>
            <option :value="14">14px</option>
            <option :value="15">15px</option>
            <option :value="16">16px</option>
          </select>
        </div>

        <div class="setting-item">
          <label>
            <input v-model="settings.appearance.compactMode" type="checkbox" />
            {{ t('workbench.compactMode') }}
          </label>
          <span class="hint">{{ t('workbench.compactModeHint') }}</span>
        </div>
      </div>
    </div>

    <div class="settings-footer">
      <button class="btn-cancel" @click="resetSettings">{{ t('common.cancel') }}</button>
      <button class="btn-save" @click="saveSettings">{{ t('workbench.saveSettings') }}</button>
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  Database,
  History,
  Activity,
  Zap,
  Keyboard,
  Palette,
  Trash2,
  RotateCcw,
} from 'lucide-vue-next'
import { ref, reactive } from 'vue'
import { useI18n } from 'vue-i18n'

import { clearHistory as clearSqlHistory } from '@/extensions/builtin/workbench/services/sql-history-service'
import type {
  Theme,
  ConnectionPoolSettings,
  HistorySettings,
  MonitoringSettings,
  PerformanceSettings,
} from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'

const { t } = useI18n()
const appStore = useAppStore()

const tabs = [
  { id: 'connection-pool', label: t('workbench.connectionPool'), icon: Database },
  { id: 'history', label: t('workbench.historySettings'), icon: History },
  { id: 'monitoring', label: t('workbench.monitoringSettings'), icon: Activity },
  { id: 'performance', label: t('workbench.performanceSettings'), icon: Zap },
  { id: 'shortcuts', label: t('workbench.shortcutsSettings'), icon: Keyboard },
  { id: 'appearance', label: t('workbench.appearanceSettings'), icon: Palette },
]

const activeTab = ref('connection-pool')

const themes = [
  { id: 'light', name: t('workbench.lightTheme') },
  { id: 'dark', name: t('workbench.darkTheme') },
  { id: 'system', name: t('workbench.systemTheme') },
]

const shortcuts = reactive([
  { key: 'newConnection', name: t('workbench.newConnectionTooltip'), value: 'Ctrl+N' },
  { key: 'disconnect', name: t('workbench.disconnectTooltip'), value: 'Ctrl+D' },
  { key: 'refresh', name: t('workbench.refreshTooltip'), value: 'Ctrl+R' },
  { key: 'search', name: t('workbench.searchTooltip'), value: 'Ctrl+F' },
  { key: 'beginTransaction', name: t('workbench.beginTransaction'), value: 'Ctrl+B' },
  { key: 'commitTransaction', name: t('workbench.commitTooltip'), value: 'Ctrl+Shift+B' },
  { key: 'rollbackTransaction', name: t('workbench.rollbackTooltip'), value: 'Ctrl+Shift+R' },
])

const SETTINGS_DEFAULTS = {
  connectionPool: {
    maxConnections: 10,
    minIdleConnections: 2,
    connectionTimeout: 30,
    idleTimeout: 300,
    autoReconnect: true,
    healthCheck: true,
    healthCheckInterval: 60,
  },
  history: {
    maxHistoryItems: 100,
    retentionDays: 30,
    enableHistory: true,
    includeSQL: true,
    enableUndo: true,
  },
  monitoring: {
    enableMonitoring: true,
    updateInterval: 5,
    enableAlerts: true,
    alertOnDisconnect: true,
    alertOnSlowQuery: true,
    slowQueryThreshold: 1000,
  },
  performance: {
    virtualScrollBuffer: 5,
    maxCacheSize: 100,
    cacheExpireMinutes: 60,
    enableLazyLoad: true,
    enablePreload: true,
  },
  appearance: {
    theme: 'system',
    fontSize: 13,
    compactMode: false,
  },
} as const

const settings = reactive({
  connectionPool: { ...appStore.effectiveConnectionPool } as ConnectionPoolSettings,
  history: { ...appStore.effectiveHistorySettings } as HistorySettings,
  monitoring: { ...appStore.effectiveMonitoringSettings } as MonitoringSettings,
  performance: { ...appStore.effectivePerformanceSettings } as PerformanceSettings,
  appearance: {
    theme: appStore.effectiveTheme as string,
    fontSize: SETTINGS_DEFAULTS.appearance.fontSize,
    compactMode: SETTINGS_DEFAULTS.appearance.compactMode,
  },
})

async function clearHistory() {
  if (confirm(t('workbench.confirmClearHistory'))) {
    await clearSqlHistory()
    alert(t('workbench.historyCleared'))
  }
}

function resetSettings() {
  Object.assign(settings.connectionPool, SETTINGS_DEFAULTS.connectionPool)
  Object.assign(settings.history, SETTINGS_DEFAULTS.history)
  Object.assign(settings.monitoring, SETTINGS_DEFAULTS.monitoring)
  Object.assign(settings.performance, SETTINGS_DEFAULTS.performance)
  Object.assign(settings.appearance, SETTINGS_DEFAULTS.appearance)
}

async function saveSettings() {
  const newTheme = settings.appearance.theme as Theme
  appStore.setTheme(newTheme)

  await appStore.setConnectionPool(
    structuredClone(settings.connectionPool) as ConnectionPoolSettings
  )
  await appStore.setHistorySettings(structuredClone(settings.history) as HistorySettings)
  await appStore.setMonitoringSettings(structuredClone(settings.monitoring) as MonitoringSettings)
  await appStore.setPerformanceSettings(
    structuredClone(settings.performance) as PerformanceSettings
  )
}
</script>

<style scoped>
.settings-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.settings-header {
  padding: 16px;
  border-bottom: 1px solid var(--border-color);
}

.settings-header h2 {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary);
}

.settings-nav {
  display: flex;
  gap: 4px;
  padding: 8px;
  border-bottom: 1px solid var(--border-color);
  overflow-x: auto;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  background: transparent;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}

.nav-item:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.nav-item.active {
  background: var(--primary-color);
  color: white;
}

.settings-content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

.settings-section {
  animation: fadeIn 0.2s ease;
}

@keyframes fadeIn {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}

.settings-section h3 {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0 0 16px 0;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-color);
}

.setting-item {
  margin-bottom: 16px;
}

.setting-item label {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-primary);
  margin-bottom: 6px;
  cursor: pointer;
}

.setting-item input[type='number'],
.setting-item select {
  width: 150px;
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 13px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  outline: none;
}

.setting-item input[type='number']:focus,
.setting-item select:focus {
  border-color: var(--primary-color);
}

.setting-item input[type='checkbox'] {
  width: 16px;
  height: 16px;
}

.setting-item .hint {
  display: block;
  font-size: 12px;
  color: var(--text-tertiary);
  margin-top: 4px;
}

.btn-clear-history,
.btn-reset-shortcuts {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  background: var(--error-color);
  border: none;
  border-radius: 4px;
  font-size: 12px;
  color: white;
  cursor: pointer;
  transition: background-color 0.15s;
}

.btn-clear-history:hover,
.btn-reset-shortcuts:hover {
  background: var(--error-color-dark);
}

.shortcuts-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.shortcut-item {
  display: flex;
  align-items: center;
  gap: 12px;
}

.shortcut-name {
  flex: 1;
  font-size: 13px;
  color: var(--text-primary);
}

.shortcut-input {
  width: 100px;
  padding: 4px 8px;
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 12px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-family: var(--font-mono);
  text-align: center;
}

.btn-edit-shortcut {
  padding: 4px 10px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.15s;
}

.btn-edit-shortcut:hover {
  background: var(--bg-secondary);
  color: var(--text-primary);
}

.theme-options {
  display: flex;
  gap: 12px;
}

.theme-option {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 10px 16px;
  background: var(--bg-secondary);
  border: 2px solid transparent;
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.15s;
}

.theme-option:hover {
  border-color: var(--border-color);
}

.theme-option.active {
  border-color: var(--primary-color);
}

.theme-preview {
  width: 32px;
  height: 32px;
  border-radius: 4px;
}

.theme-preview.light {
  background: linear-gradient(135deg, #ffffff 50%, #f0f0f0 50%);
  border: 1px solid #e0e0e0;
}

.theme-preview.dark {
  background: linear-gradient(135deg, #2d2d2d 50%, #1a1a1a 50%);
  border: 1px solid #3d3d3d;
}

.theme-preview.system {
  background: linear-gradient(135deg, #ffffff 50%, #2d2d2d 50%);
  border: 1px solid #e0e0e0;
}

.settings-footer {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  padding: 12px 16px;
  border-top: 1px solid var(--border-color);
}

.btn-cancel,
.btn-save {
  padding: 6px 16px;
  border-radius: 4px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

.btn-cancel {
  background: transparent;
  border: 1px solid var(--border-color);
  color: var(--text-secondary);
}

.btn-cancel:hover {
  background: var(--bg-tertiary);
}

.btn-save {
  background: var(--primary-color);
  border: none;
  color: white;
}

.btn-save:hover {
  background: var(--primary-dark);
}
</style>
