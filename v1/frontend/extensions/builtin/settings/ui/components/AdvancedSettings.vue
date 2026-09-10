<template>
  <div class="advanced-settings">
    <div class="settings-section">
      <h3>
        <AppIcon name="Network" :size="16" />
        {{ $t('settings.connectionPool') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.maxConnections') }}</span>
          <span class="label-value">{{ localConnectionPool.maxConnections }}</span>
        </div>
        <input
          v-model.number="localConnectionPool.maxConnections"
          type="range"
          min="1"
          max="50"
          step="1"
          class="slider"
        />
        <div class="slider-labels">
          <span>1</span>
          <span>50</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.minIdleConnections') }}</span>
          <span class="label-value">{{ localConnectionPool.minIdleConnections }}</span>
        </div>
        <input
          v-model.number="localConnectionPool.minIdleConnections"
          type="range"
          min="0"
          max="10"
          step="1"
          class="slider"
        />
        <div class="slider-labels">
          <span>0</span>
          <span>10</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.autoReconnect') }}</span>
        </div>
        <label class="switch">
          <input v-model="localConnectionPool.autoReconnect" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>
    </div>

    <div class="settings-section">
      <h3>
        <AppIcon name="Clock" :size="16" />
        {{ $t('settings.history') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.maxHistoryItems') }}</span>
          <span class="label-value">{{ localHistorySettings.maxHistoryItems }}</span>
        </div>
        <input
          v-model.number="localHistorySettings.maxHistoryItems"
          type="range"
          min="100"
          max="10000"
          step="100"
          class="slider"
        />
        <div class="slider-labels">
          <span>100</span>
          <span>10000</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.retentionDays') }}</span>
          <span class="label-value">{{ localHistorySettings.retentionDays }}d</span>
        </div>
        <input
          v-model.number="localHistorySettings.retentionDays"
          type="range"
          min="7"
          max="365"
          step="7"
          class="slider"
        />
        <div class="slider-labels">
          <span>7</span>
          <span>365</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.enableHistory') }}</span>
        </div>
        <label class="switch">
          <input v-model="localHistorySettings.enableHistory" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>
    </div>

    <div class="settings-section">
      <h3>
        <AppIcon name="Activity" :size="16" />
        {{ $t('settings.monitoring') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.enableMonitoring') }}</span>
        </div>
        <label class="switch">
          <input v-model="localMonitoringSettings.enableMonitoring" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div v-if="localMonitoringSettings.enableMonitoring" class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.updateInterval') }}</span>
          <span class="label-value">{{ localMonitoringSettings.updateInterval }}s</span>
        </div>
        <input
          v-model.number="localMonitoringSettings.updateInterval"
          type="range"
          min="1"
          max="60"
          step="1"
          class="slider"
        />
        <div class="slider-labels">
          <span>1s</span>
          <span>60s</span>
        </div>
      </div>
    </div>

    <div class="settings-section">
      <h3>
        <AppIcon name="Zap" :size="16" />
        {{ $t('settings.performance') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.maxCacheSize') }}</span>
          <span class="label-value">{{ localPerformanceSettings.maxCacheSize }}MB</span>
        </div>
        <input
          v-model.number="localPerformanceSettings.maxCacheSize"
          type="range"
          min="32"
          max="1024"
          step="32"
          class="slider"
        />
        <div class="slider-labels">
          <span>32MB</span>
          <span>1GB</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.enableLazyLoad') }}</span>
          <span class="label-hint">{{ $t('settings.enableLazyLoadHint') }}</span>
        </div>
        <label class="switch">
          <input v-model="localPerformanceSettings.enableLazyLoad" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive, watch } from 'vue'

import AppIcon from '@/shared/components/common/AppIcon.vue'
import type {
  ConnectionPoolSettings,
  HistorySettings,
  MonitoringSettings,
  PerformanceSettings,
} from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'

const appStore = useAppStore()

const localConnectionPool = reactive<ConnectionPoolSettings>({
  ...appStore.effectiveConnectionPool,
})
const localHistorySettings = reactive<HistorySettings>({ ...appStore.effectiveHistorySettings })
const localMonitoringSettings = reactive<MonitoringSettings>({
  ...appStore.effectiveMonitoringSettings,
})
const localPerformanceSettings = reactive<PerformanceSettings>({
  ...appStore.effectivePerformanceSettings,
})

watch(
  () => appStore.effectiveConnectionPool,
  val => {
    Object.assign(localConnectionPool, val)
  },
  { deep: true }
)
watch(
  () => appStore.effectiveHistorySettings,
  val => {
    Object.assign(localHistorySettings, val)
  },
  { deep: true }
)
watch(
  () => appStore.effectiveMonitoringSettings,
  val => {
    Object.assign(localMonitoringSettings, val)
  },
  { deep: true }
)
watch(
  () => appStore.effectivePerformanceSettings,
  val => {
    Object.assign(localPerformanceSettings, val)
  },
  { deep: true }
)

function resetToFactory() {
  Object.assign(localConnectionPool, appStore.effectiveConnectionPool)
  Object.assign(localHistorySettings, appStore.effectiveHistorySettings)
  Object.assign(localMonitoringSettings, appStore.effectiveMonitoringSettings)
  Object.assign(localPerformanceSettings, appStore.effectivePerformanceSettings)
}

defineExpose({
  localConnectionPool,
  localHistorySettings,
  localMonitoringSettings,
  localPerformanceSettings,
  resetToFactory,
})
</script>

<style scoped>
@import '../styles/settings-shared.css';
</style>
