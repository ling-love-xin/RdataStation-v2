<template>
  <div class="shortcut-settings">
    <div class="shortcut-intro">
      <AppIcon name="Info" :size="14" />
      <span>{{ $t('settings.shortcutInfo') }}</span>
    </div>

    <div class="shortcut-search">
      <AppIcon name="Search" :size="14" class="search-icon" />
      <input
        v-model="filterQuery"
        type="text"
        :placeholder="$t('settings.searchShortcuts')"
        class="search-input"
      />
    </div>

    <div class="shortcut-groups">
      <div v-for="group in filteredGroups" :key="group.name" class="settings-section">
        <h3>{{ group.name }}</h3>
        <div class="shortcut-list">
          <div v-for="item in group.items" :key="item.key" class="shortcut-row">
            <div class="shortcut-desc">
              <span class="shortcut-label">{{ item.label }}</span>
            </div>
            <div class="shortcut-keys">
              <template v-for="(key, ki) in item.keys" :key="ki">
                <kbd class="key-chip">{{ key }}</kbd>
                <span v-if="ki < item.keys.length - 1" class="key-plus">+</span>
              </template>
            </div>
          </div>
        </div>
      </div>

      <div v-if="filteredGroups.length === 0" class="no-results">
        {{ $t('settings.noShortcutsFound') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import AppIcon from '@/shared/components/common/AppIcon.vue'

const { t } = useI18n()

const filterQuery = ref('')

interface ShortcutItem {
  key: string
  label: string
  keys: string[]
}

interface ShortcutGroup {
  name: string
  items: ShortcutItem[]
}

const shortcutGroups: ShortcutGroup[] = [
  {
    name: t('settings.shortcutGroupGeneral'),
    items: [
      { key: 'settings', label: t('settings.shortcutOpenSettings'), keys: ['Ctrl', ','] },
      {
        key: 'commandPalette',
        label: t('settings.shortcutCommandPalette'),
        keys: ['Ctrl', 'Shift', 'P'],
      },
      {
        key: 'newConnection',
        label: t('settings.shortcutNewConnection'),
        keys: ['Ctrl', 'Shift', 'N'],
      },
    ],
  },
  {
    name: t('settings.shortcutGroupEditor'),
    items: [
      { key: 'execute', label: t('settings.shortcutExecute'), keys: ['Ctrl', 'Enter'] },
      {
        key: 'executeAll',
        label: t('settings.shortcutExecuteAll'),
        keys: ['Ctrl', 'Shift', 'Enter'],
      },
      { key: 'format', label: t('settings.shortcutFormat'), keys: ['Ctrl', 'Shift', 'F'] },
      { key: 'comment', label: t('settings.shortcutToggleComment'), keys: ['Ctrl', '/'] },
      { key: 'find', label: t('settings.shortcutFind'), keys: ['Ctrl', 'F'] },
      { key: 'replace', label: t('settings.shortcutReplace'), keys: ['Ctrl', 'H'] },
    ],
  },
  {
    name: t('settings.shortcutGroupNavigation'),
    items: [
      { key: 'switchTab', label: t('settings.shortcutSwitchTab'), keys: ['Ctrl', 'Tab'] },
      { key: 'closeTab', label: t('settings.shortcutCloseTab'), keys: ['Ctrl', 'W'] },
      { key: 'toggleLeft', label: t('settings.shortcutToggleLeft'), keys: ['Ctrl', 'B'] },
      { key: 'toggleBottom', label: t('settings.shortcutToggleBottom'), keys: ['Ctrl', 'J'] },
    ],
  },
]

const filteredGroups = computed(() => {
  const q = filterQuery.value.toLowerCase().trim()
  if (!q) return shortcutGroups
  return shortcutGroups
    .map(g => ({
      ...g,
      items: g.items.filter(
        i => i.label.toLowerCase().includes(q) || i.keys.some(k => k.toLowerCase().includes(q))
      ),
    }))
    .filter(g => g.items.length > 0)
})
</script>

<style scoped>
@import '../styles/settings-shared.css';

.shortcut-intro {
  display: flex;
  align-items: flex-start;
  gap: var(--spacing-sm);
  padding: var(--spacing-md) var(--spacing-lg);
  margin-bottom: var(--spacing-lg);
  border-radius: var(--border-radius-md);
  background: var(--brand-accent-soft);
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  line-height: 1.5;
}

.shortcut-search {
  position: relative;
  margin-bottom: var(--setting-section-gap);
}

.search-icon {
  position: absolute;
  left: 10px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--color-text-muted);
  pointer-events: none;
}

.search-input {
  width: 100%;
  padding: 6px 10px 6px 30px;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-sm);
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
  font-size: var(--font-size-md);
  box-sizing: border-box;
  outline: none;
  transition: border-color 0.15s ease;
}

.search-input:focus {
  border-color: var(--brand-accent);
}

.shortcut-groups {
  display: flex;
  flex-direction: column;
}

.shortcut-list {
  display: flex;
  flex-direction: column;
}

.shortcut-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--spacing-sm) 0;
}

.shortcut-row + .shortcut-row {
  border-top: 1px solid var(--color-border);
}

.shortcut-desc {
  flex: 1;
}

.shortcut-label {
  font-size: var(--font-size-md);
  color: var(--color-text-primary);
}

.shortcut-keys {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.key-chip {
  display: inline-flex;
  align-items: center;
  padding: var(--spacing-xs) var(--spacing-sm);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-sm);
  background: var(--color-bg-secondary);
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  font-family: 'JetBrains Mono', 'Consolas', monospace;
  line-height: 1.6;
}

.key-plus {
  color: var(--color-text-muted);
  font-size: var(--font-size-sm);
}

.no-results {
  padding: var(--spacing-xl) 0;
  text-align: center;
  color: var(--color-text-muted);
  font-size: var(--font-size-md);
}
</style>
