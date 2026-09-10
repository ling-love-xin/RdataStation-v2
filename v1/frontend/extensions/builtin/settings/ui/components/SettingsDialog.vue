<template>
  <NModal :show="show" :mask-closable="true" @update:show="handleClose">
    <div class="settings-card" :style="cardStyle">
      <div class="card-header">
        <AppIcon name="Settings" :size="16" accent class="card-icon" />
        <span class="card-title">{{ $t('settings.title') }}</span>

        <div class="header-search">
          <AppIcon name="Search" :size="14" class="search-icon" />
          <input
            ref="searchInputRef"
            v-model="searchQuery"
            type="text"
            :placeholder="$t('settings.searchPlaceholder')"
            class="search-input"
          />
          <button v-if="searchQuery" class="search-clear-btn" @click="searchQuery = ''">
            <AppIcon name="X" :size="14" />
          </button>
        </div>

        <div class="header-spacer" />
      </div>

      <div class="card-main">
        <nav class="card-sidebar">
          <button
            v-for="cat in filteredCategories"
            :key="cat.id"
            :class="['nav-item', { active: activeCategory === cat.id }]"
            @click="activeCategory = cat.id"
          >
            <AppIcon :name="cat.icon" :size="16" />
            <span class="nav-label">{{ cat.label }}</span>
          </button>
        </nav>

        <div class="card-content">
          <div class="scope-bar">
            <div class="scope-toggle">
              <button
                :class="['scope-btn', { active: scope === 'global' }]"
                @click="scope = 'global'"
              >
                <AppIcon name="Globe" :size="14" />
                <span>{{ $t('settings.globalScope') }}</span>
              </button>
              <button
                :class="['scope-btn', { active: scope === 'project' }]"
                @click="scope = 'project'"
              >
                <AppIcon name="Folder" :size="14" />
                <span>{{ $t('settings.projectScope') }}</span>
              </button>
            </div>
            <span class="scope-hint">
              {{
                scope === 'project'
                  ? $t('settings.projectScopeHint')
                  : $t('settings.globalScopeHint')
              }}
            </span>
          </div>

          <div class="content-body">
            <Transition name="fade-slide" mode="out-in">
              <AppearanceSettings v-if="activeCategory === 'appearance'" key="appearance" />
              <EditorSettings v-else-if="activeCategory === 'editor'" key="editor" />
              <ResultSettings v-else-if="activeCategory === 'results'" key="results" />
              <InterfaceSettings v-else-if="activeCategory === 'interface'" key="interface" />
              <ShortcutSettings v-else-if="activeCategory === 'shortcuts'" key="shortcuts" />
              <AdvancedSettings v-else-if="activeCategory === 'advanced'" key="advanced" />
              <div v-else key="empty" class="content-placeholder">
                <AppIcon name="Settings" :size="32" muted />
                <span>{{ $t('settings.title') }}</span>
              </div>
            </Transition>
          </div>
        </div>
      </div>

      <div class="card-footer">
        <NButton @click="handleClose">{{ $t('settings.cancel') }}</NButton>
        <NButton type="primary">{{ $t('settings.confirm') }}</NButton>
        <NButton type="info">{{ $t('settings.apply') }}</NButton>
      </div>

      <div class="resize-handle" @mousedown="onResizeStart" />
    </div>
  </NModal>
</template>

<script setup lang="ts">
import { NButton, NModal } from 'naive-ui'
import { computed, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import AppIcon from '@/shared/components/common/AppIcon.vue'

import AdvancedSettings from './AdvancedSettings.vue'
import AppearanceSettings from './AppearanceSettings.vue'
import EditorSettings from './EditorSettings.vue'
import InterfaceSettings from './InterfaceSettings.vue'
import ResultSettings from './ResultSettings.vue'
import ShortcutSettings from './ShortcutSettings.vue'

const MIN_W = 480
const MIN_H = 360
const MAX_W = 1200
const MAX_H = 900

interface Props {
  show: boolean
}

defineProps<Props>()

interface Emits {
  'update:show': [value: boolean]
}

const emit = defineEmits<Emits>()

const { t } = useI18n()

const scope = ref<'global' | 'project'>('global')
const activeCategory = ref('appearance')
const searchQuery = ref('')
const searchInputRef = ref<HTMLInputElement | null>(null)

interface Cat {
  id: string
  icon: string
  label: string
  keywords: string[]
}

const categories = computed<Cat[]>(() => [
  {
    id: 'appearance',
    icon: 'Palette',
    label: t('settings.appearanceTab'),
    keywords: ['theme', 'color', 'font', 'language', 'color'],
  },
  {
    id: 'editor',
    icon: 'Code2',
    label: t('settings.editorTab'),
    keywords: ['font', 'tab', 'wrap', 'minimap'],
  },
  {
    id: 'results',
    icon: 'Table2',
    label: t('settings.resultsTab'),
    keywords: ['page', 'null', 'date', 'export'],
  },
  {
    id: 'interface',
    icon: 'LayoutTemplate',
    label: t('settings.interfaceTab'),
    keywords: ['title', 'status', 'command'],
  },
  {
    id: 'shortcuts',
    icon: 'Keyboard',
    label: t('settings.shortcutsTab'),
    keywords: ['key', 'bind', 'hotkey'],
  },
  {
    id: 'advanced',
    icon: 'Settings2',
    label: t('settings.advancedTab'),
    keywords: ['pool', 'history', 'cache', 'monitor'],
  },
])

const filteredCategories = computed(() => {
  const q = searchQuery.value.toLowerCase().trim()
  if (!q) return categories.value
  return categories.value.filter(
    c => c.label.toLowerCase().includes(q) || c.keywords.some(k => k.includes(q))
  )
})

const cardWidth = ref(640)
const cardHeight = ref(480)

const cardStyle = computed(() => ({
  width: `${cardWidth.value}px`,
  height: `${cardHeight.value}px`,
}))

let isResizing = false
let startX = 0
let startY = 0
let startW = 0
let startH = 0

function clamp(v: number, min: number, max: number) {
  return Math.max(min, Math.min(max, v))
}

function onResizeStart(e: MouseEvent) {
  isResizing = true
  startX = e.clientX
  startY = e.clientY
  startW = cardWidth.value
  startH = cardHeight.value
  document.addEventListener('mousemove', onResizeMove)
  document.addEventListener('mouseup', onResizeEnd)
  e.preventDefault()
  e.stopPropagation()
}

function onResizeMove(e: MouseEvent) {
  if (!isResizing) return
  cardWidth.value = clamp(startW + (e.clientX - startX), MIN_W, MAX_W)
  cardHeight.value = clamp(startH + (e.clientY - startY), MIN_H, MAX_H)
}

function onResizeEnd() {
  isResizing = false
  document.removeEventListener('mousemove', onResizeMove)
  document.removeEventListener('mouseup', onResizeEnd)
}

onUnmounted(() => {
  document.removeEventListener('mousemove', onResizeMove)
  document.removeEventListener('mouseup', onResizeEnd)
})

function handleClose() {
  emit('update:show', false)
}
</script>

<style scoped>
.settings-card {
  display: flex;
  flex-direction: column;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-md);
  overflow: hidden;
  position: relative;
}

/* ========== header ========== */
.card-header {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: 6px var(--spacing-lg);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.card-icon {
  color: var(--brand-accent);
  flex-shrink: 0;
}

.card-title {
  font-size: var(--font-size-md);
  font-weight: 600;
  color: var(--color-text-primary);
  white-space: nowrap;
  flex-shrink: 0;
}

.header-search {
  flex: 1;
  position: relative;
  max-width: 280px;
  margin: 0 auto;
}

.header-search .search-icon {
  position: absolute;
  left: 8px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--color-text-muted);
  pointer-events: none;
}

.header-search .search-input {
  width: 100%;
  height: 26px;
  padding: 0 26px 0 26px;
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-sm);
  background: var(--color-bg-secondary);
  color: var(--color-text-primary);
  font-size: var(--font-size-sm);
  box-sizing: border-box;
  outline: none;
  transition: border-color 0.15s ease;
}

.header-search .search-input:focus {
  border-color: var(--brand-accent);
}

.header-search .search-input::placeholder {
  color: var(--color-text-muted);
}

.search-clear-btn {
  position: absolute;
  right: 2px;
  top: 50%;
  transform: translateY(-50%);
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: var(--border-radius-sm);
  background: transparent;
  color: var(--color-text-muted);
  cursor: pointer;
  transition:
    color 0.15s ease,
    background 0.15s ease;
}

.search-clear-btn:hover {
  color: var(--color-text-primary);
  background: var(--color-hover);
}

.header-spacer {
  flex-shrink: 0;
  width: 0;
}

/* ========== body = sidebar + content ========== */
.card-main {
  flex: 1;
  display: flex;
  overflow: hidden;
  min-height: 0;
}

.card-sidebar {
  width: 140px;
  min-width: 140px;
  border-right: 1px solid var(--color-border);
  overflow-y: auto;
  display: flex;
  flex-direction: column;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: 6px 12px;
  border: none;
  border-left: 2px solid transparent;
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-sm);
  cursor: pointer;
  text-align: left;
  transition: all 0.15s ease;
}

.nav-item:hover {
  background: var(--color-hover);
  color: var(--color-text-primary);
}

.nav-item.active {
  background: var(--color-selection);
  color: var(--brand-accent);
  border-left-color: var(--brand-accent);
  font-weight: 500;
}

.nav-label {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ========== content ========== */
.card-content {
  flex: 1;
  overflow: hidden;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.scope-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px var(--spacing-md);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.scope-toggle {
  display: flex;
  gap: 1px;
  background: var(--color-bg-secondary);
  border-radius: var(--border-radius-sm);
  padding: 1px;
}

.scope-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px 10px;
  border: none;
  border-radius: var(--border-radius-sm);
  background: transparent;
  color: var(--color-text-secondary);
  font-size: var(--font-size-xs);
  cursor: pointer;
  transition: all 0.15s ease;
  white-space: nowrap;
}

.scope-btn:hover {
  color: var(--color-text-primary);
}

.scope-btn.active {
  background: var(--color-bg-elevated);
  color: var(--brand-accent);
  box-shadow: var(--shadow-inset-subtle);
  font-weight: 500;
}

.scope-hint {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
}

.content-body {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-sm) var(--spacing-md);
}

.content-placeholder {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--spacing-sm);
  color: var(--color-text-muted);
  font-size: var(--font-size-md);
}

/* ========== footer ========== */
.card-footer {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: var(--spacing-sm);
  padding: 6px var(--spacing-lg);
  border-top: 1px solid var(--color-border);
  flex-shrink: 0;
}

/* ========== resize ========== */
.resize-handle {
  position: absolute;
  right: 0;
  bottom: 0;
  width: 16px;
  height: 16px;
  cursor: nwse-resize;
  z-index: 10;
}

.resize-handle::after {
  content: '';
  position: absolute;
  right: 3px;
  bottom: 3px;
  width: 8px;
  height: 8px;
  border-right: 2px solid var(--color-text-muted);
  border-bottom: 2px solid var(--color-text-muted);
  opacity: 0.4;
  transition: opacity 0.15s ease;
}

.resize-handle:hover::after {
  opacity: 0.8;
}

/* ========== transition ========== */
.fade-slide-enter-active,
.fade-slide-leave-active {
  transition:
    opacity 0.12s ease,
    transform 0.12s ease;
}

.fade-slide-enter-from {
  opacity: 0;
  transform: translateY(4px);
}

.fade-slide-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
