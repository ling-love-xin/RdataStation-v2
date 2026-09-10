<template>
  <div class="scratchpad-tree-node">
    <div
      :class="['node-row', { selected: isSelected, 'drag-over': dragOver }]"
      :style="{ paddingLeft: `${depth * 16 + 8}px` }"
      :draggable="entry.kind === 'file' && !isRenaming"
      @click="handleClick"
      @dblclick="handleDoubleClick"
      @contextmenu.prevent="handleContextMenu"
      @dragstart="handleDragStart"
      @dragover.prevent="handleDragOver"
      @dragleave="handleDragLeave"
      @drop="handleDrop"
    >
      <span v-if="entry.kind === 'folder'" class="folder-toggle" @click.stop="handleToggleExpand">
        <NIcon size="14">
          <component :is="expanded ? ChevronDown : ChevronRight" />
        </NIcon>
      </span>
      <span v-else class="folder-toggle-spacer" />

      <NIcon
        size="14"
        class="node-icon"
        :class="{ 'node-icon-folder': entry.kind === 'folder' }"
        :style="entry.kind === 'folder' ? { color: folderIconColor } : undefined"
      >
        <component :is="entry.kind === 'folder' ? (expanded ? FolderOpen : Folder) : fileIcon" />
      </NIcon>

      <span v-if="renamingKey !== entry.path" class="node-name">
        <span v-if="isNodeDirty" class="dirty-dot">●</span>
        {{ entry.name }}
      </span>
      <div v-else class="rename-wrapper">
        <input
          ref="renameInputRef"
          v-model="renameValue"
          class="rename-input"
          :disabled="renamingSaving"
          @keyup.enter="commitRename"
          @keyup.escape="cancelRename"
          @blur="commitRename"
          @click.stop
        />
        <span v-if="renamingSaving" class="rename-spinner" />
      </div>

      <span v-if="entry.kind === 'file' && entry.size > 0" class="node-size">{{
        formatSize(entry.size)
      }}</span>
      <span v-if="modifiedTime" class="node-time">{{ modifiedTime }}</span>
      <span v-if="entry.kind === 'folder'" class="node-arrow" />
    </div>

    <div
      v-if="isInlineCreateTarget && expanded"
      class="node-row inline-create-row"
      :style="{ paddingLeft: `${(depth + 1) * 16 + 8}px` }"
    >
      <span class="folder-toggle-spacer" />
      <NIcon
        size="14"
        class="node-icon"
        :class="{ 'node-icon-folder': inlineCreateIsFolder }"
        :style="inlineCreateIsFolder ? { color: folderIconColor } : undefined"
      >
        <component :is="inlineCreateIsFolder ? Folder : File" />
      </NIcon>
      <input
        ref="inlineInputRef"
        v-model="inlineCreateName"
        class="rename-input"
        :placeholder="
          inlineCreateIsFolder
            ? t('scratchpad.newFolderNamePlaceholder')
            : t('scratchpad.newFileNamePlaceholder')
        "
        @keyup.enter="commitInlineCreate"
        @keyup.escape="cancelInlineCreate"
        @click.stop
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import {
  File,
  Folder,
  FolderOpen,
  Database,
  FileCode,
  Table2,
  FileText,
  Braces,
  BookOpen,
  ChevronDown,
  ChevronRight,
} from 'lucide-vue-next'
import { NIcon } from 'naive-ui'
import { computed, ref, watch, nextTick, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'

import { useUiStore } from '@/shared/stores/ui'

import type { ScratchpadEntry } from '../../types'

interface Props {
  entry: ScratchpadEntry
  depth: number
  expandedKeys: Set<string>
  selectedKey: string | null
  selectedKeys?: Set<string>
  renamingKey: string | null
  inlineCreateParentPath?: string | null
  inlineCreateIsFolder?: boolean
  dirtyFiles?: Set<string>
}

const props = withDefaults(defineProps<Props>(), {
  inlineCreateParentPath: null,
  inlineCreateIsFolder: false,
  dirtyFiles: () => new Set<string>(),
})

const { t } = useI18n()
const uiStore = useUiStore()

const folderIconColor = computed(() => (uiStore.isDark ? '#e2a348' : '#b8730a'))

const emit = defineEmits<{
  select: [entry: ScratchpadEntry, event?: MouseEvent]
  open: [entry: ScratchpadEntry]
  contextmenu: [event: MouseEvent, entry: ScratchpadEntry]
  'toggle-expand': [entry: ScratchpadEntry]
  'start-rename': [entry: ScratchpadEntry]
  'finish-rename': [entry: ScratchpadEntry, newName: string]
  'cancel-rename': []
  'drag-start': [event: DragEvent, entry: ScratchpadEntry]
  'drop-file': [fromPath: string, toPath: string]
  'create-inline': [name: string]
}>()

function normPath(p: string): string {
  return p.replace(/\\/g, '/').replace(/\/$/, '')
}

const expanded = computed(() => {
  const np = normPath(props.entry.path)
  for (const key of props.expandedKeys) {
    if (normPath(key) === np) return true
  }
  return false
})
const isSelected = computed(() => {
  const np = normPath(props.entry.path)
  if (props.selectedKeys) {
    for (const key of props.selectedKeys) {
      if (normPath(key) === np) return true
    }
    return false
  }
  return props.selectedKey ? normPath(props.selectedKey) === np : false
})
const isRenaming = computed(() =>
  props.renamingKey ? normPath(props.renamingKey) === normPath(props.entry.path) : false
)

const isInlineCreateTarget = computed(
  () =>
    props.entry.kind === 'folder' &&
    (props.inlineCreateParentPath
      ? normPath(props.inlineCreateParentPath) === normPath(props.entry.path)
      : false)
)

const isNodeDirty = computed(() => {
  if (!props.dirtyFiles || props.entry.kind !== 'file') return false
  return [...props.dirtyFiles].some(
    p =>
      p.replace(/\\/g, '/').replace(/\/$/, '') ===
      props.entry.path.replace(/\\/g, '/').replace(/\/$/, '')
  )
})

const inlineCreateName = ref('')
const inlineInputRef = ref<HTMLInputElement | null>(null)
const inlineCreateIsFolder = computed(() => props.inlineCreateIsFolder)
const inlineCreating = ref(false)
const dragOver = ref(false)

const renameValue = ref('')
const renameInputRef = ref<HTMLInputElement | null>(null)
const renamingSaving = ref(false)

const extensionIconMap: Record<string, typeof File> = {
  '.sql': Database,
  '.py': FileCode,
  '.csv': Table2,
  '.json': Braces,
  '.txt': FileText,
  '.md': BookOpen,
  '.duckdb': Database,
  '.parquet': Table2,
}

const fileIcon = computed(() => {
  const name = props.entry.name
  const ext = name.includes('.') ? '.' + name.split('.').pop()?.toLowerCase() : ''
  return extensionIconMap[ext] || File
})

const modifiedTime = computed(() => {
  const ts = props.entry.modified_at
  if (!ts) return ''
  const date = new Date(ts)
  if (isNaN(date.getTime())) return ''
  const now = Date.now()
  const diff = now - date.getTime()
  const minutes = Math.floor(diff / 60000)
  if (minutes < 1) return ''
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.floor(hours / 24)
  if (days < 7) return `${days}d`
  return ''
})

watch(isRenaming, async val => {
  if (val) {
    renameValue.value = props.entry.name
    await nextTick()
    renameInputRef.value?.focus()
    renameInputRef.value?.select()
  }
})

let inlineClickOutsideCleanup: (() => void) | null = null
let inlineClickOutsideTimer: ReturnType<typeof setTimeout> | null = null

watch(
  isInlineCreateTarget,
  async val => {
    if (val) {
      // eslint-disable-next-line no-console
      console.log('[TreeNode] isInlineCreateTarget=true, entry:', props.entry.path)
      inlineCreateName.value = ''
      await nextTick()
      // eslint-disable-next-line no-console
      console.log('[TreeNode] inlineInputRef:', inlineInputRef.value ? 'exists' : 'null')
      inlineInputRef.value?.focus()
      inlineInputRef.value?.scrollIntoView({ block: 'nearest' })
      registerInlineClickOutside()
    } else {
      cleanupInlineClickOutside()
    }
  },
  { immediate: true }
)

function registerInlineClickOutside(): void {
  cleanupInlineClickOutside()
  const handler = (event: MouseEvent): void => {
    const target = event.target as HTMLElement
    if (target.closest('.inline-create-row')) {
      document.addEventListener('click', handler, { once: true })
      return
    }
    commitInlineCreate()
  }
  inlineClickOutsideTimer = setTimeout(() => {
    inlineClickOutsideTimer = null
    document.addEventListener('click', handler, { once: true })
  }, 0)
  inlineClickOutsideCleanup = () => {
    if (inlineClickOutsideTimer !== null) {
      clearTimeout(inlineClickOutsideTimer)
      inlineClickOutsideTimer = null
    }
    document.removeEventListener('click', handler)
  }
}

function cleanupInlineClickOutside(): void {
  if (inlineClickOutsideCleanup) {
    inlineClickOutsideCleanup()
    inlineClickOutsideCleanup = null
  }
}

onUnmounted(() => {
  if (inlineClickOutsideTimer !== null) {
    clearTimeout(inlineClickOutsideTimer)
    inlineClickOutsideTimer = null
  }
  cleanupInlineClickOutside()
})

function handleClick(event: MouseEvent): void {
  emit('select', props.entry, event)
}

function handleDoubleClick(): void {
  emit('open', props.entry)
}

function handleContextMenu(event: MouseEvent): void {
  emit('contextmenu', event, props.entry)
}

function handleDragStart(event: DragEvent): void {
  if (props.entry.kind !== 'file') return
  emit('drag-start', event, props.entry)
}

function handleDragOver(event: DragEvent): void {
  if (props.entry.kind !== 'folder') return
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'move'
  }
  dragOver.value = true
}

function handleDragLeave(): void {
  dragOver.value = false
}

function handleDrop(event: DragEvent): void {
  dragOver.value = false
  if (props.entry.kind !== 'folder') return
  const fromPath = event.dataTransfer?.getData('text/plain')
  if (fromPath && fromPath !== props.entry.path) {
    emit('drop-file', fromPath, props.entry.path)
  }
}

function handleToggleExpand(): void {
  emit('select', props.entry)
  emit('toggle-expand', props.entry)
}

function commitRename(): void {
  const trimmed = renameValue.value.trim()
  if (!trimmed) {
    emit('cancel-rename')
    return
  }
  renamingSaving.value = true
  emit('finish-rename', props.entry, trimmed)
}

function cancelRename(): void {
  emit('cancel-rename')
}

function commitInlineCreate(_event?: FocusEvent): void {
  if (inlineCreating.value) return
  const name = inlineCreateName.value.trim()
  // eslint-disable-next-line no-console
  console.log('[TreeNode] commitInlineCreate, name:', name)
  if (!name) {
    cancelInlineCreate()
    return
  }
  inlineCreating.value = true
  inlineCreateName.value = ''
  emit('create-inline', name)
}

function cancelInlineCreate(): void {
  inlineCreating.value = false
  inlineCreateName.value = ''
  cleanupInlineClickOutside()
  emit('create-inline', '')
}

function formatSize(bytes: number): string {
  if (bytes === 0) return ''
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
</script>

<style scoped>
.scratchpad-tree-node {
  user-select: none;
}

.node-row {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  height: 22px;
  padding-right: var(--spacing-sm);
  cursor: pointer;
  transition: background-color 0.1s;
}

.node-row:hover {
  background-color: var(--color-hover);
}

.node-row.selected {
  background-color: var(--color-selection);
  color: var(--color-text-primary);
}

.node-row.drag-over {
  background-color: var(--primary-color-10, rgba(0, 127, 255, 0.12));
  outline: 1px solid var(--primary-color, #007fff);
  outline-offset: -1px;
}

.folder-toggle,
.folder-toggle-spacer {
  width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.folder-toggle {
  cursor: pointer;
}

.node-icon {
  flex-shrink: 0;
}

.node-icon-folder {
  /* folder icon gets a warm amber/gold color for clear visual distinction */
}

.node-name {
  flex: 1;
  font-size: var(--font-size-md);
  color: var(--color-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  line-height: 22px;
}

.dirty-dot {
  color: var(--color-text-muted);
  margin-right: 2px;
  font-size: var(--font-size-xs);
  line-height: 1;
  flex-shrink: 0;
}

.rename-input {
  flex: 1;
  height: 18px;
  padding: 0 var(--spacing-xs);
  font-size: var(--font-size-md);
  font-family: var(--font-family);
  border: 1px solid var(--primary-color);
  border-radius: var(--border-radius-sm);
  outline: none;
  background: var(--color-bg-primary);
  color: var(--color-text-primary);
}

.rename-input:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.rename-wrapper {
  display: flex;
  align-items: center;
  flex: 1;
  gap: var(--spacing-xs);
}

.rename-spinner {
  width: 14px;
  height: 14px;
  border: 2px solid var(--color-border-subtle);
  border-top-color: var(--brand-accent);
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
  flex-shrink: 0;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.node-arrow {
  width: 12px;
  flex-shrink: 0;
}

.node-size,
.node-time {
  display: none;
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  flex-shrink: 0;
}

.node-row:hover .node-size,
.node-row:hover .node-time {
  display: block;
}

.node-size {
  min-width: 36px;
  text-align: right;
}

.node-time {
  min-width: 28px;
  text-align: right;
}

.node-row.selected .node-size,
.node-row.selected .node-time {
  opacity: 0.7;
}

.inline-create-row {
  background-color: transparent;
}

.inline-create-row .rename-input {
  border-color: var(--primary-color);
}
</style>
