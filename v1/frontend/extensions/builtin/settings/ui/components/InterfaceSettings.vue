<template>
  <div class="interface-settings">
    <!-- 标题栏设置 -->
    <div class="settings-section">
      <h3>
        <AppIcon name="PanelTop" :size="16" />
        {{ $t('settings.titleBar') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.menuStyle') }}</span>
        </div>
        <div class="theme-selector">
          <button
            v-for="opt in menuStyleOptions"
            :key="opt.value"
            :class="['theme-btn', { active: localTitleBarSettings.menuStyle === opt.value }]"
            @click="localTitleBarSettings.menuStyle = opt.value"
          >
            {{ opt.label }}
          </button>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.showProjectSelector') }}</span>
        </div>
        <label class="switch">
          <input v-model="localTitleBarSettings.showProjectSelector" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.showCommandCenter') }}</span>
        </div>
        <label class="switch">
          <input v-model="localTitleBarSettings.showCommandCenter" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.recentProjectCount') }}</span>
          <span class="label-value">{{ localTitleBarSettings.recentProjectCount }}</span>
        </div>
        <input
          v-model.number="localTitleBarSettings.recentProjectCount"
          type="range"
          min="1"
          max="10"
          step="1"
          class="slider"
        />
        <div class="slider-labels">
          <span>1</span>
          <span>10</span>
        </div>
      </div>
    </div>

    <!-- 工具栏设置 -->
    <div class="settings-section">
      <h3>
        <AppIcon name="Wrench" :size="16" />
        {{ $t('settings.toolbar') }}
      </h3>

      <div class="toolbar-tools-config">
        <div v-for="tool in availableToolbarTools" :key="tool.id" class="tool-toggle-item">
          <label class="tool-toggle-label">
            <input v-model="localTitleBarSettings.toolbarTools" :value="tool.id" type="checkbox" />
            <AppIcon :name="tool.iconName" :size="16" />
            <span>{{ tool.name }}</span>
          </label>
        </div>
      </div>
    </div>

    <!-- 状态栏设置 -->
    <div class="settings-section">
      <h3>
        <AppIcon name="PanelBottom" :size="16" />
        {{ $t('settings.statusBar') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.showStatusBar') }}</span>
        </div>
        <label class="switch">
          <input v-model="localStatusBarSettings.visible" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.showConnectionStatus') }}</span>
        </div>
        <label class="switch">
          <input v-model="localStatusBarSettings.showConnectionStatus" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.showExecutionTime') }}</span>
        </div>
        <label class="switch">
          <input v-model="localStatusBarSettings.showExecutionTime" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.showRowCount') }}</span>
        </div>
        <label class="switch">
          <input v-model="localStatusBarSettings.showRowCount" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.showDuckDBIndicator') }}</span>
        </div>
        <label class="switch">
          <input v-model="localStatusBarSettings.showDuckDBIndicator" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.showEncoding') }}</span>
        </div>
        <label class="switch">
          <input v-model="localStatusBarSettings.showEncoding" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.showVersion') }}</span>
        </div>
        <label class="switch">
          <input v-model="localStatusBarSettings.showVersion" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>
    </div>

    <!-- 命令面板设置 -->
    <div class="settings-section">
      <h3>
        <AppIcon name="Command" :size="16" />
        {{ $t('settings.commandPalette') }}
      </h3>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.maxRecentCommands') }}</span>
          <span class="label-value">{{ localCommandPaletteSettings.maxRecentCommands }}</span>
        </div>
        <input
          v-model.number="localCommandPaletteSettings.maxRecentCommands"
          type="range"
          min="1"
          max="20"
          step="1"
          class="slider"
        />
        <div class="slider-labels">
          <span>1</span>
          <span>20</span>
        </div>
      </div>

      <div class="setting-item">
        <div class="setting-label">
          <span class="label-text">{{ $t('settings.includeDisabledCommands') }}</span>
        </div>
        <label class="switch">
          <input v-model="localCommandPaletteSettings.includeDisabledCommands" type="checkbox" />
          <span class="slider-switch"></span>
        </label>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import AppIcon from '@/shared/components/common/AppIcon.vue'
import { DEFAULT_GLOBAL_CONFIG } from '@/stores/config'
import type { CommandPaletteSettings, StatusBarSettings, TitleBarSettings } from '@/stores/config'
import { useAppStore } from '@/stores/useAppStore'

const appStore = useAppStore()
const { t } = useI18n()

const menuStyleOptions = [
  { value: 'full' as const, label: t('settings.menuStyleFull') },
  { value: 'compact' as const, label: t('settings.menuStyleCompact') },
  { value: 'hidden' as const, label: t('settings.menuStyleHidden') },
]

const availableToolbarTools = [
  { id: 'settings', name: t('workbench.settings'), iconName: 'Settings' as const },
  { id: 'history', name: t('workbench.history'), iconName: 'History' as const },
  { id: 'docs', name: t('workbench.docs'), iconName: 'BookOpen' as const },
  { id: 'shortcuts', name: t('workbench.shortcuts'), iconName: 'Keyboard' as const },
  { id: 'terminal', name: t('workbench.terminal'), iconName: 'Terminal' as const },
  { id: 'quick', name: t('workbench.quickActions'), iconName: 'Zap' as const },
]

const localTitleBarSettings = reactive<TitleBarSettings>({ ...appStore.effectiveTitleBarSettings })
const localStatusBarSettings = reactive<StatusBarSettings>({
  ...appStore.effectiveStatusBarSettings,
})
const localCommandPaletteSettings = reactive<CommandPaletteSettings>({
  ...appStore.effectiveCommandPaletteSettings,
})

watch(
  () => appStore.effectiveTitleBarSettings,
  val => {
    Object.assign(localTitleBarSettings, val)
  },
  { deep: true }
)
watch(
  () => appStore.effectiveStatusBarSettings,
  val => {
    Object.assign(localStatusBarSettings, val)
  },
  { deep: true }
)
watch(
  () => appStore.effectiveCommandPaletteSettings,
  val => {
    Object.assign(localCommandPaletteSettings, val)
  },
  { deep: true }
)

function resetToDefault() {
  Object.assign(localTitleBarSettings, DEFAULT_GLOBAL_CONFIG.titleBarSettings)
  Object.assign(localStatusBarSettings, DEFAULT_GLOBAL_CONFIG.statusBarSettings)
  Object.assign(localCommandPaletteSettings, DEFAULT_GLOBAL_CONFIG.commandPaletteSettings)
}

function resetToFactory() {
  Object.assign(localTitleBarSettings, appStore.effectiveTitleBarSettings)
  Object.assign(localStatusBarSettings, appStore.effectiveStatusBarSettings)
  Object.assign(localCommandPaletteSettings, appStore.effectiveCommandPaletteSettings)
}

defineExpose({
  localTitleBarSettings,
  localStatusBarSettings,
  localCommandPaletteSettings,
  resetToDefault,
  resetToFactory,
})
</script>

<style scoped>
@import '../styles/settings-shared.css';

.interface-settings {
  display: flex;
  flex-direction: column;
}

.toolbar-tools-config {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: var(--spacing-sm);
}

.tool-toggle-item {
  padding: var(--spacing-sm);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-sm);
  background: var(--color-bg-secondary);
}

.tool-toggle-label {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  cursor: pointer;
  font-size: var(--font-size-sm);
  color: var(--color-text-primary);
}

.tool-toggle-label input[type='checkbox'] {
  margin: 0;
}
</style>
