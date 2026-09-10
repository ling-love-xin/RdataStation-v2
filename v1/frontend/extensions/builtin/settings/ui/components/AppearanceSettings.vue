<template>
  <div class="appearance-settings">
    <!-- 外观 -->
    <div class="settings-section">
      <h3>
        <AppIcon name="Palette" :size="16" />
        {{ $t('settings.appearance') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.theme') }}</span>
          <span v-if="hasProjectThemeOverride" class="label-hint"
            >({{ $t('settings.projectOverride') }})</span
          >
        </div>
        <div class="theme-selector">
          <button
            v-for="opt in themeOptions"
            :key="opt.value"
            :class="['theme-btn', { active: localTheme === opt.value }]"
            @click="localTheme = opt.value"
          >
            <AppIcon :name="opt.icon" :size="16" />
            {{ opt.label }}
          </button>
        </div>
        <button v-if="hasProjectThemeOverride" class="reset-btn" @click="resetProjectTheme">
          <AppIcon name="RotateCcw" :size="14" />
          {{ $t('settings.resetToGlobal') }}
        </button>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.language') }}</span>
          <span class="label-hint">{{ $t('settings.restartHint') }}</span>
        </div>
        <div class="theme-selector">
          <button
            v-for="opt in languageOptions"
            :key="opt.value"
            :class="['theme-btn', { active: localLanguage === opt.value }]"
            @click="localLanguage = opt.value"
          >
            {{ opt.label }}
          </button>
        </div>
      </div>
    </div>

    <!-- UI 外观定制 -->
    <div class="settings-section">
      <h3>
        <AppIcon name="Paintbrush" :size="16" />
        {{ $t('settings.uiAppearance') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.accentColor') }}</span>
          <span v-if="hasProjectAppearanceOverride" class="label-hint"
            >({{ $t('settings.projectOverride') }})</span
          >
        </div>
        <div class="color-picker-row">
          <input v-model="localAppearance.accentColor" type="color" class="color-input" />
          <input
            v-model="localAppearance.accentColor"
            type="text"
            class="text-input color-text"
            placeholder="#E17055"
            maxlength="7"
            spellcheck="false"
          />
          <button
            v-if="localAppearance.accentColor"
            class="reset-btn"
            @click="localAppearance.accentColor = null"
          >
            <AppIcon name="RotateCcw" :size="12" />
            {{ $t('settings.useDefault') }}
          </button>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.uiFontFamily') }}</span>
          <span class="label-hint">{{ $t('settings.fontFamilyHint') }}</span>
        </div>
        <input
          v-model="localAppearance.fontFamily"
          type="text"
          class="text-input"
          spellcheck="false"
        />
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.borderRadius') }}</span>
          <span class="label-value">{{ localAppearance.borderRadius }}px</span>
        </div>
        <input
          v-model.number="localAppearance.borderRadius"
          type="range"
          min="4"
          max="12"
          step="1"
          class="slider"
        />
        <div class="slider-labels">
          <span>4px</span>
          <span>12px</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.density') }}</span>
        </div>
        <div class="theme-selector">
          <button
            v-for="opt in densityOptions"
            :key="opt.value"
            :class="['theme-btn', { active: localAppearance.density === opt.value }]"
            @click="localAppearance.density = opt.value"
          >
            {{ opt.label }}
          </button>
        </div>
      </div>
    </div>

    <!-- 编辑器 -->
    <div class="settings-section">
      <h3>
        <AppIcon name="FileCode" :size="16" />
        {{ $t('settings.editor') }}
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
          <span class="label-text">{{ $t('settings.wordWrap') }}</span>
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
        </div>
        <label class="switch">
          <input v-model="localEditorSettings.minimap" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
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

    <!-- 默认引擎 -->
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
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import AppIcon from '@/shared/components/common/AppIcon.vue'
import { CONFIG_KEYS, DEFAULT_EDITOR_SETTINGS, DEFAULT_GLOBAL_CONFIG } from '@/stores/config'
import type {
  AppearanceDensity,
  AppearanceSettings as AppearanceSettingsType,
  DefaultEngine,
  EditorSettings,
  Language,
  Theme,
} from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'

const appStore = useAppStore()
const { t } = useI18n()

const themeOptions = [
  { value: 'dark' as Theme, label: t('settings.dark'), icon: 'Moon' as const },
  { value: 'light' as Theme, label: t('settings.light'), icon: 'Sun' as const },
  { value: 'system' as Theme, label: t('settings.system'), icon: 'Monitor' as const },
]

const languageOptions = [
  { value: 'zh-CN' as Language, label: t('settings.simplifiedChinese') },
  { value: 'en' as Language, label: t('settings.english') },
]

const engineOptions = [
  { value: 'native' as DefaultEngine, label: t('settings.nativeEngine') },
  { value: 'duckdb' as DefaultEngine, label: t('settings.duckdbEngine') },
]

const densityOptions = [
  { value: 'compact' as AppearanceDensity, label: t('settings.compact') },
  { value: 'comfortable' as AppearanceDensity, label: t('settings.comfortable') },
  { value: 'spacious' as AppearanceDensity, label: t('settings.spacious') },
]

const localTheme = ref<Theme>(appStore.effectiveTheme)
const localLanguage = ref<Language>(appStore.effectiveLanguage)
const localEditorSettings = reactive<EditorSettings>({ ...appStore.effectiveEditorSettings })
const localDefaultEngine = ref<DefaultEngine>(appStore.effectiveDefaultEngine)
const localAppearance = reactive<AppearanceSettingsType>({
  ...appStore.effectiveAppearanceSettings,
})

const hasProjectThemeOverride = computed(() => appStore.hasProjectOverride(CONFIG_KEYS.THEME))
const hasProjectAppearanceOverride = computed(() =>
  appStore.hasProjectOverride(CONFIG_KEYS.APPEARANCE_SETTINGS)
)

function resetProjectTheme() {
  appStore.resetProjectOverride(CONFIG_KEYS.THEME)
  localTheme.value = appStore.effectiveTheme
}

watch(
  () => appStore.effectiveTheme,
  val => {
    localTheme.value = val
  }
)
watch(
  () => appStore.effectiveLanguage,
  val => {
    localLanguage.value = val
  }
)
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
watch(
  () => appStore.effectiveAppearanceSettings,
  val => {
    Object.assign(localAppearance, val)
  },
  { deep: true }
)

function resetToDefault() {
  localTheme.value = DEFAULT_GLOBAL_CONFIG.theme
  localLanguage.value = DEFAULT_GLOBAL_CONFIG.language
  Object.assign(localEditorSettings, DEFAULT_EDITOR_SETTINGS)
  localDefaultEngine.value = DEFAULT_GLOBAL_CONFIG.defaultEngine
  Object.assign(localAppearance, DEFAULT_GLOBAL_CONFIG.appearanceSettings)
}

function resetToFactory() {
  localTheme.value = appStore.effectiveTheme
  localLanguage.value = appStore.effectiveLanguage
  Object.assign(localEditorSettings, appStore.effectiveEditorSettings)
  localDefaultEngine.value = appStore.effectiveDefaultEngine
  Object.assign(localAppearance, appStore.effectiveAppearanceSettings)
}

defineExpose({
  localTheme,
  localLanguage,
  localEditorSettings,
  localDefaultEngine,
  localAppearance,
  resetToDefault,
  resetToFactory,
})
</script>

<style scoped>
@import '../styles/settings-shared.css';

.appearance-settings {
  display: flex;
  flex-direction: column;
}
</style>
