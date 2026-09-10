<template>
  <div class="editor-settings">
    <div class="settings-section">
      <h3>
        <AppIcon name="Type" :size="16" />
        {{ $t('settings.editorDisplay') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.fontSize') }}</span>
          <span class="label-value">{{ localEditorSettings.fontSize }}px</span>
        </div>
        <input
          v-model.number="localEditorSettings.fontSize"
          type="range"
          min="10"
          max="24"
          step="1"
          class="slider"
        />
        <div class="slider-labels">
          <span>10px</span>
          <span>24px</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.tabSize') }}</span>
          <span class="label-value">{{ localEditorSettings.tabSize }}</span>
        </div>
        <div class="theme-selector">
          <button
            v-for="size in [2, 4, 8]"
            :key="size"
            :class="['theme-btn', { active: localEditorSettings.tabSize === size }]"
            @click="localEditorSettings.tabSize = size"
          >
            {{ size }}
          </button>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.fontFamily') }}</span>
          <span class="label-hint">{{ $t('settings.fontFamilyHint') }}</span>
        </div>
        <input
          v-model="localEditorSettings.fontFamily"
          type="text"
          class="text-input"
          spellcheck="false"
        />
      </div>
    </div>

    <div class="settings-section">
      <h3>
        <AppIcon name="Eye" :size="16" />
        {{ $t('settings.editorView') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.wordWrap') }}</span>
          <span class="label-hint">{{ $t('settings.wordWrapHint') }}</span>
        </div>
        <label class="switch">
          <input v-model="localEditorSettings.wordWrap" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.lineNumbers') }}</span>
        </div>
        <label class="switch">
          <input v-model="localEditorSettings.lineNumbers" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.minimap') }}</span>
          <span class="label-hint">{{ $t('settings.minimapHint') }}</span>
        </div>
        <label class="switch">
          <input v-model="localEditorSettings.minimap" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>
    </div>

    <div class="settings-section">
      <h3>
        <AppIcon name="Database" :size="16" />
        {{ $t('settings.defaultEngine') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.engine') }}</span>
          <span class="label-hint">{{ $t('settings.engineHint') }}</span>
        </div>
        <div class="theme-selector">
          <button
            v-for="opt in engineOptions"
            :key="opt.value"
            :class="['theme-btn', { active: localDefaultEngine === opt.value }]"
            @click="localDefaultEngine = opt.value"
          >
            {{ opt.label }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import AppIcon from '@/shared/components/common/AppIcon.vue'
import type { DefaultEngine, EditorSettings } from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'

const appStore = useAppStore()
const { t } = useI18n()

const engineOptions = [
  { value: 'native' as DefaultEngine, label: t('settings.nativeEngine') },
  { value: 'duckdb' as DefaultEngine, label: t('settings.duckdbEngine') },
]

const localEditorSettings = reactive<EditorSettings>({ ...appStore.effectiveEditorSettings })
const localDefaultEngine = ref<DefaultEngine>(appStore.effectiveDefaultEngine)

watch(
  () => appStore.effectiveEditorSettings,
  val => {
    Object.assign(localEditorSettings, val)
  },
  { deep: true }
)
watch(
  () => appStore.effectiveDefaultEngine,
  val => {
    localDefaultEngine.value = val
  }
)

function resetToFactory() {
  Object.assign(localEditorSettings, appStore.effectiveEditorSettings)
  localDefaultEngine.value = appStore.effectiveDefaultEngine
}

defineExpose({ localEditorSettings, localDefaultEngine, resetToFactory })
</script>

<style scoped>
@import '../styles/settings-shared.css';
</style>
