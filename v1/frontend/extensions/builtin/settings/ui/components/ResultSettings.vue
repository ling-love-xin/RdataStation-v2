<template>
  <div class="result-settings">
    <div class="settings-section">
      <h3>
        <AppIcon name="LayoutGrid" :size="16" />
        {{ $t('settings.resultDisplay') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.pageSize') }}</span>
          <span class="label-value">{{ localResultSettings.pageSize }}</span>
        </div>
        <input
          v-model.number="localResultSettings.pageSize"
          type="range"
          min="10"
          max="500"
          step="10"
          class="slider"
        />
        <div class="slider-labels">
          <span>10</span>
          <span>500</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.defaultViewMode') }}</span>
        </div>
        <div class="theme-selector">
          <button
            v-for="opt in viewModeOptions"
            :key="opt.value"
            :class="['theme-btn', { active: localResultSettings.defaultViewMode === opt.value }]"
            @click="localResultSettings.defaultViewMode = opt.value"
          >
            <AppIcon :name="opt.icon" :size="14" />
            {{ opt.label }}
          </button>
        </div>
      </div>
    </div>

    <div class="settings-section">
      <h3>
        <AppIcon name="FileText" :size="16" />
        {{ $t('settings.resultFormat') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.nullDisplay') }}</span>
          <span class="label-hint">{{ $t('settings.nullDisplayHint') }}</span>
        </div>
        <input
          v-model="localResultSettings.nullDisplay"
          type="text"
          class="text-input"
          spellcheck="false"
          maxlength="10"
        />
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.dateFormat') }}</span>
          <span class="label-hint">{{ $t('settings.dateFormatHint') }}</span>
        </div>
        <input
          v-model="localResultSettings.dateFormat"
          type="text"
          class="text-input"
          spellcheck="false"
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import AppIcon from '@/shared/components/common/AppIcon.vue'
import type { ResultSettings } from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'

const appStore = useAppStore()
const { t } = useI18n()

const viewModeOptions = [
  { value: 'grid' as const, label: t('settings.viewGrid'), icon: 'LayoutGrid' as const },
  { value: 'text' as const, label: t('settings.viewText'), icon: 'FileText' as const },
  { value: 'chart' as const, label: t('settings.viewChart'), icon: 'BarChart3' as const },
]

const localResultSettings = reactive<ResultSettings>({ ...appStore.effectiveResultSettings })

watch(
  () => appStore.effectiveResultSettings,
  val => {
    Object.assign(localResultSettings, val)
  },
  { deep: true }
)

function resetToFactory() {
  Object.assign(localResultSettings, appStore.effectiveResultSettings)
}

defineExpose({ localResultSettings, resetToFactory })
</script>

<style scoped>
@import '../styles/settings-shared.css';
</style>
