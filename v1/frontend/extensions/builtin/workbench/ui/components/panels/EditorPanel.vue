<template>
  <div class="editor-panel">
    <EditorToolbar
      v-if="showToolbar"
      :toolbar-position="'top'"
      :is-duck-db="false"
      :show-advanced="isAnalysisMode"
      :editor-mode="editorModeCategory"
      :executing="isEditorExecuting"
      :connection-options="connectionOptions"
      :selected-connection="myFileInfo?.connectionId || ''"
      @execute="handleExecute"
      @execute-new="handleExecuteNew"
      @execute-batch="handleExecuteBatch"
      @format="handleFormat"
      @validate="handleValidate"
      @transpile="handleTranspile"
      @explain="handleExplain"
      @save-snippet="handleSaveSnippet"
      @duckdb-execute="handleDuckDbExecute"
      @toggle-minimap="handleToggleMinimap"
      @toggle-settings="handleToggleSettings"
      @mode-change="handleModeChange"
      @connection-change="handleConnectionChange"
    />

    <div class="editor-body">
      <div v-if="isReadonly" class="readonly-warning">
        <EyeOff :size="12" :stroke-width="1.5" />
        <span>文件已在其他标签页编辑中，当前为只读模式</span>
      </div>

      <!-- 编辑器标签栏（深度优化版） -->
      <div class="tab-bar-wrap">
        <div :class="['tab-mode-accent', modeAccentClass]" />
        <button
          class="tab-scroll-btn"
          :disabled="!canScrollLeft"
          title="向左滚动"
          @click="scrollTabs(-200)"
        >
          <ChevronLeft :size="12" />
        </button>
        <div
          ref="tabBarRef"
          class="tab-bar"
          @wheel.prevent="onTabWheel"
          @dragover.prevent="onTabDragOver"
          @drop.prevent="onTabDrop"
        >
          <div
            v-for="(tab, idx) in orderedTabs"
            :key="tab.key"
            :ref="el => setTabRef(tab.key, el)"
            :class="[
              'tab-item',
              {
                active: tab.isActive,
                pinned: tab.pinned,
                'has-dirty': tab.isDirty,
                'drag-over': dragOverIndex === idx,
                'drag-source': dragSourceKey === tab.key,
              },
            ]"
            :data-type="getTabType(tab)"
            :data-index="idx"
            :draggable="!tab.pinned"
            :title="tab.key"
            @click="handleTabClick(tab.key)"
            @mousedown="closeOnMiddleClick($event, tab.key)"
            @contextmenu.prevent="onTabContextMenu($event, tab.key)"
            @dragstart="onTabDragStart($event, tab.key)"
            @dragend="onTabDragEnd"
            @dragover.prevent="onTabItemDragOver($event, idx)"
            @dragleave="onTabDragLeave"
            @drop.prevent="onTabItemDrop($event, idx)"
          >
            <span class="tab-type-dot" />
            <span class="tab-icon">
              <Database v-if="getTabType(tab) === 'sql'" :size="13" :stroke-width="1.5" />
              <BarChart3
                v-else-if="getTabType(tab) === 'analysis'"
                :size="13"
                :stroke-width="1.5"
              />
              <Braces v-else-if="getTabType(tab) === 'code'" :size="13" :stroke-width="1.5" />
              <FileText v-else-if="tab.language === 'plaintext'" :size="13" :stroke-width="1.5" />
              <File v-else :size="13" :stroke-width="1.5" />
            </span>
            <span :class="['tab-label', { modified: tab.isDirty }]">{{ tab.label }}</span>
            <span v-if="tab.isDirty" class="tab-dirty" />
            <span
              class="tab-pin"
              :class="{ 'is-pinned': tab.pinned }"
              title="固定/取消固定标签页"
              @click.stop="togglePinTab(tab.key)"
            >
              <Pin :size="10" />
            </span>
            <span
              :class="['tab-close', { always: tab.isDirty }]"
              @click.stop="handleTabClose(tab.key)"
            >
              <X :size="12" :stroke-width="1.5" />
            </span>
            <span class="tab-drag-handle" />
          </div>
        </div>
        <button
          class="tab-scroll-btn"
          :disabled="!canScrollRight"
          title="向右滚动"
          @click="scrollTabs(200)"
        >
          <ChevronRight :size="12" />
        </button>
        <!-- 溢出标签下拉菜单 -->
        <div v-if="overflowTabs.length > 0" class="tab-overflow-wrap">
          <button
            class="tab-overflow-btn"
            title="更多标签页"
            @click="showOverflowMenu = !showOverflowMenu"
          >
            <ChevronDown :size="12" />
          </button>
          <div v-if="showOverflowMenu" class="tab-overflow-menu">
            <div
              v-for="ot in overflowTabs"
              :key="ot.key"
              :class="['overflow-item', { active: ot.isActive }]"
              @click="handleTabClick(ot.key); showOverflowMenu = false"
            >
              <span :class="['overflow-dot', getTabType(ot)]" />
              <span class="overflow-label">{{ ot.label }}</span>
              <span v-if="ot.isDirty" class="overflow-dirty">●</span>
              <span class="overflow-close" @click.stop="handleTabClose(ot.key)">&times;</span>
            </div>
          </div>
        </div>
        <button class="tab-new-btn" title="新建文件 (Ctrl+N)" @click="newEditorTab">
          <Plus :size="14" />
        </button>
      </div>

      <!-- 面包屑导航 -->
      <div v-if="currentFilePath" class="editor-breadcrumb">
        <template v-for="(crumb, idx) in breadcrumbs" :key="idx">
          <span v-if="idx > 0" class="crumb-sep">›</span>
          <span
            :class="['crumb', { 'crumb-active': idx === breadcrumbs.length - 1 }]"
            @click="idx < breadcrumbs.length - 1 && showToast('导航到: ' + crumb.path)"
          >
            {{ crumb.label }}
          </span>
        </template>
      </div>

      <Teleport to="body">
        <div
          v-if="contextMenu.visible"
          class="tab-context-menu"
          :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
          @click.stop
        >
          <div class="context-menu-item" @click="handleCloseFromMenu">关闭</div>
          <div class="context-menu-item" @click="handleCloseOthersFromMenu">关闭其他</div>
          <div class="context-menu-item" @click="handleCloseRightFromMenu">关闭右侧</div>
          <div class="context-menu-item" @click="handleCloseAllFromMenu">关闭所有</div>
          <div class="context-menu-sep" />
          <div class="context-menu-item" @click="handlePinFromMenu">
            <Pin :size="12" />
            <span>{{ isContextMenuPinned ? '取消固定' : '固定' }}</span>
          </div>
          <div class="context-menu-item" @click="handleCopyPathFromMenu">
            <Copy :size="12" />
            <span>复制路径</span>
          </div>
        </div>
        <div
          v-if="contextMenu.visible"
          class="context-menu-backdrop"
          @click="closeContextMenu"
          @contextmenu.prevent="closeContextMenu"
        />
      </Teleport>

      <div class="editor-split">
        <div v-if="largeFileTier === 'rejected'" class="large-file-warning">
          <div class="large-file-icon">&#9888;</div>
          <div class="large-file-title">文件过大</div>
          <div class="large-file-desc">
            文件大小 {{ largeFileSizeMB.toFixed(1) }}MB，超出编辑器支持范围（最大 200MB）。
          </div>
          <div class="large-file-desc">建议使用外部工具打开此文件。</div>
        </div>
        <div class="editor-area" :style="{ flex: hasResults ? `${splitRatio}` : '1 1 auto' }">
          <div ref="editorContainerRef" class="cm-container" />
          <EditorWelcome
            v-if="showWelcome"
            :visible="showWelcome"
            @connect="handleWelcomeConnect"
          />
        </div>

        <div v-if="hasResults" class="split-handle" @mousedown="startSplitDrag" />

        <div v-if="hasResults" class="result-area" :style="{ flex: `calc(1 - ${splitRatio})` }">
          <ResultArea :editor-id="currentFilePath" />
        </div>
      </div>
    </div>

    <EditorStatusbar v-bind="statusbarProps" @cancel="handleStatusbarCancel" @commit="handleStatusbarCommit" @rollback="handleStatusbarRollback" />
  </div>
</template>

<script setup lang="ts">
import {
  File,
  FileText,
  X,
  EyeOff,
  Pin,
  ChevronLeft,
  ChevronRight,
  ChevronDown,
  Plus,
  Copy,
  Database,
  BarChart3,
  Braces,
} from 'lucide-vue-next'
import { createDiscreteApi } from 'naive-ui'
import {
  ref,
  computed,
  watch,
  onMounted,
  onUnmounted,
  nextTick,
  type ComponentPublicInstance,
} from 'vue'

import { EditorManager } from '@/extensions/builtin/workbench/manager/EditorManager'
import { closeFileChecked } from '@/extensions/builtin/workbench/manager/file-manager'
import { useCodeMirror } from '@/extensions/builtin/workbench/ui/composables/useCodeMirror'
import {
  classifyFileSize,
  getChunkedContent,
  type FileSizeTier,
} from '@/extensions/builtin/workbench/ui/composables/useLargeFile'
import { useResultStore } from '@/extensions/builtin/workbench/ui/stores/result-store'
import { useUiStore } from '@/shared/stores/ui'
import { copyToClipboard } from '@/shared/utils/clipboard'

import EditorStatusbar from './EditorStatusbar.vue'
import EditorToolbar from './EditorToolbar.vue'
import EditorWelcome from './EditorWelcome.vue'
import ResultArea from './ResultArea.vue'

const { message: toast } = createDiscreteApi(['message'])

const uiStore = useUiStore()
const resultStore = useResultStore()
const { createView, destroyView, setTheme, setLanguage, view, getEditorState, setEditorState } =
  useCodeMirror()

const props = defineProps<{
  params: Record<string, unknown>
}>()

// 响应式包装 EditorManager.isExecuting
const isEditorExecuting = computed(() => EditorManager.isExecuting)

const editorContainerRef = ref<HTMLElement | null>(null)
const tabBarRef = ref<HTMLElement | null>(null)
const showWelcome = ref(true)
const cursorPosition = ref('Ln 1, Col 1')
const selectedTextInfo = ref('')
const splitRatio = ref(0.55)
const isDragging = ref(false)
const largeFileTier = ref<FileSizeTier>('normal')
const largeFileSizeMB = ref(0)
const canScrollLeft = ref(false)
const canScrollRight = ref(false)
const showOverflowMenu = ref(false)
let domObserver: MutationObserver | null = null
let scrollObserver: ResizeObserver | null = null

// ─── 拖拽排序 ───────────────────────────────────────
const dragSourceKey = ref<string | null>(null)
const dragOverIndex = ref<number | null>(null)
const tabRefs = ref<Map<string, HTMLElement>>(new Map())
const tabOrder = ref<string[]>([])

function setTabRef(key: string, el: unknown) {
  if (el) {
    tabRefs.value.set(
      key,
      ((el as ComponentPublicInstance)?.$el as HTMLElement) || (el as HTMLElement)
    )
  } else {
    tabRefs.value.delete(key)
  }
}

function getTabType(tab: { language: string; key: string }): string {
  const info = EditorManager.openFiles.get(tab.key)
  if (!info) return tab.language
  if (info.type === 'analysis') return 'analysis'
  if (info.language === 'sql') return 'sql'
  if (
    ['rust', 'typescript', 'javascript', 'python', 'go', 'java', 'c', 'cpp'].includes(info.language)
  )
    return 'code'
  return info.language
}

function onTabDragStart(e: DragEvent, key: string) {
  dragSourceKey.value = key
  if (e.dataTransfer) {
    e.dataTransfer.effectAllowed = 'move'
    e.dataTransfer.setData('text/plain', key)
  }
}

function onTabDragEnd() {
  dragSourceKey.value = null
  dragOverIndex.value = null
}

function onTabDragOver(e: DragEvent) {
  if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
}

function onTabItemDragOver(_e: DragEvent, idx: number) {
  dragOverIndex.value = idx
}

function onTabDragLeave() {
  dragOverIndex.value = null
}

function onTabDrop(_e: DragEvent) {
  dragSourceKey.value = null
  dragOverIndex.value = null
}

function onTabItemDrop(_e: DragEvent, targetIdx: number) {
  const srcKey = dragSourceKey.value
  if (!srcKey) return
  const currentOrder =
    tabOrder.value.length > 0 ? [...tabOrder.value] : orderedTabs.value.map(t => t.key)
  const srcIdx = currentOrder.indexOf(srcKey)
  if (srcIdx === -1 || srcIdx === targetIdx) return
  currentOrder.splice(srcIdx, 1)
  currentOrder.splice(targetIdx, 0, srcKey)
  tabOrder.value = currentOrder
  dragSourceKey.value = null
  dragOverIndex.value = null
}

// ─── 溢出标签菜单 ──────────────────────────────────
const MAX_VISIBLE_TABS = 8
const overflowTabs = computed(() => {
  const all = orderedTabs.value
  if (all.length <= MAX_VISIBLE_TABS) return []
  return all.slice(MAX_VISIBLE_TABS)
})

const currentFilePath = computed(() => String(props.params.filePath || ''))
const currentLanguage = computed(() => String(props.params.language || 'sql'))

const myFileInfo = computed(() => EditorManager.openFiles.get(currentFilePath.value))

const modeAccentClass = computed(() => {
  const info = myFileInfo.value
  if (!info) return 'sql'
  if (info.type === 'analysis') return 'analysis'
  if (info.language === 'sql') return 'sql'
  if (
    ['rust', 'typescript', 'javascript', 'python', 'go', 'java', 'c', 'cpp'].includes(info.language)
  )
    return 'code'
  return 'sql'
})

const editorMode = computed(() => {
  const info = myFileInfo.value
  let base = 'Plain Text'
  if (info) {
    if (info.language === 'sql') base = 'SQL'
    else base = info.language
  }
  if (largeFileTier.value === 'rejected') base += ' (too large)'
  else if (largeFileTier.value === 'chunked') base += ' (chunked)'
  else if (largeFileTier.value === 'large') base += ' (large)'
  else if (largeFileTier.value === 'reduced') base += ' (large)'
  if (isReadonly.value) base += ' (read-only)'
  return base
})

const isDirty = computed(() => myFileInfo.value?.isDirty ?? false)

const isReadonly = computed(() => {
  const fp = currentFilePath.value
  if (!fp) return false
  return !EditorManager.isPrimaryInstance(fp)
})

const statusbarProps = computed(() => ({
  cursorPosition: cursorPosition.value,
  selectedTextInfo: selectedTextInfo.value,
  editorMode:
    editorModeCategory.value === 'sql'
      ? 'SQL'
      : editorModeCategory.value === 'analysis'
        ? 'Analysis'
        : editorMode.value,
  executing: EditorManager.isExecuting,
  canCancel: EditorManager.isExecuting,
  lastExecutionTime: null,
  connectionInfoText: myFileInfo.value?.connectionId || '',
  connectionStatus: (myFileInfo.value?.connectionId ? 'connected' : 'disconnected') as
    | 'connected'
    | 'disconnected'
    | 'connecting',
  popselectOptions: [],
  selectedConnection: myFileInfo.value?.connectionId || '',
  inTransaction: false,
  statementCount: 0,
  isDirty: isDirty.value,
}))

const showToolbar = computed(() => {
  const info = myFileInfo.value
  if (!info) return false
  return true // 所有模式均显示工具栏
})

const isAnalysisMode = computed(() => myFileInfo.value?.type === 'analysis')

const editorModeCategory = computed<'sql' | 'analysis' | 'code'>(() => {
  const info = myFileInfo.value
  if (!info) return 'sql'
  if (info.type === 'analysis') return 'analysis'
  if (info.language === 'sql') return 'sql'
  if (
    ['rust', 'typescript', 'javascript', 'python', 'go', 'java', 'c', 'cpp'].includes(info.language)
  )
    return 'code'
  return 'sql'
})

const hasResults = computed(() => {
  return resultStore.hasResults(currentFilePath.value)
})

const connectionOptions = computed(() => {
  const conns: Array<{ label: string; value: string }> = []
  for (const [, info] of EditorManager.openFiles.entries()) {
    if (info.connectionId) {
      const label = `${info.fileName}${info.databaseName ? ` / ${info.databaseName}` : ''}`
      if (!conns.some(c => c.value === info.connectionId)) {
        conns.push({ label, value: info.connectionId })
      }
    }
  }
  return conns
})

const tabs = computed(() =>
  Array.from(EditorManager.openFiles.entries()).map(([path, info]) => ({
    key: path,
    label: info.fileName,
    isDirty: info.isDirty,
    pinned: info.pinned,
    isActive: path === EditorManager.activeFilePath,
    language: info.language,
  }))
)

const sortedTabs = computed(() => {
  const all = [...tabs.value]
  return all.sort((a, b) => {
    if (a.pinned && !b.pinned) return -1
    if (!a.pinned && b.pinned) return 1
    return 0
  })
})

const orderedTabs = computed(() => {
  const all = sortedTabs.value
  const order = tabOrder.value
  if (order.length === 0) return all
  const keyMap = new Map(all.map(t => [t.key, t]))
  const result: typeof all = []
  const seen = new Set<string>()
  for (const key of order) {
    const tab = keyMap.get(key)
    if (tab && !seen.has(key)) {
      result.push(tab)
      seen.add(key)
    }
  }
  // 新标签（不在 order 中）追加到末尾
  for (const tab of all) {
    if (!seen.has(tab.key)) {
      result.push(tab)
    }
  }
  return result
})

const breadcrumbs = computed(() => {
  const fp = currentFilePath.value
  if (!fp) return []
  const parts = fp.replace(/\\/g, '/').split('/').filter(Boolean)
  const result: { label: string; path: string }[] = []
  let accumulated = ''
  for (let i = 0; i < parts.length; i++) {
    accumulated += (i > 0 ? '/' : '') + parts[i]
    result.push({
      label: parts[i],
      path: accumulated,
    })
  }
  return result
})

const contextMenu = ref<{ visible: boolean; x: number; y: number; filePath: string }>({
  visible: false,
  x: 0,
  y: 0,
  filePath: '',
})

const isContextMenuPinned = computed(() => {
  const info = EditorManager.openFiles.get(contextMenu.value.filePath)
  return info?.pinned ?? false
})

function updateScrollState() {
  const el = tabBarRef.value
  if (!el) {
    canScrollLeft.value = false
    canScrollRight.value = false
    return
  }
  canScrollLeft.value = el.scrollLeft > 0
  canScrollRight.value = el.scrollLeft + el.clientWidth < el.scrollWidth - 1
}

function scrollTabs(delta: number) {
  const el = tabBarRef.value
  if (!el) return
  el.scrollBy({ left: delta, behavior: 'smooth' })
  setTimeout(updateScrollState, 300)
}

function onTabWheel(e: WheelEvent) {
  const el = tabBarRef.value
  if (!el) return
  el.scrollLeft += e.deltaY
  updateScrollState()
}

function togglePinTab(filePath: string) {
  const info = EditorManager.openFiles.get(filePath)
  if (!info) return
  info.pinned = !info.pinned
}

function newEditorTab() {
  EditorManager.newFile('sql')
}

function showToast(msg: string) {
  toast.info(msg)
}

function onDocumentClick(e: MouseEvent) {
  if (showOverflowMenu.value) {
    const target = e.target as HTMLElement
    if (!target.closest('.tab-overflow-wrap')) {
      showOverflowMenu.value = false
    }
  }
  if (contextMenu.value.visible) {
    const target = e.target as HTMLElement
    if (!target.closest('.tab-context-menu') && !target.closest('.tab-item')) {
      closeContextMenu()
    }
  }
}

function onTabContextMenu(e: MouseEvent, filePath: string) {
  e.preventDefault()
  contextMenu.value = { visible: true, x: e.clientX, y: e.clientY, filePath }
}

function closeContextMenu() {
  contextMenu.value.visible = false
}

function handleCloseFromMenu() {
  const fp = contextMenu.value.filePath
  if (fp) handleTabClose(fp)
  closeContextMenu()
}

function handleCloseOthersFromMenu() {
  const fp = contextMenu.value.filePath
  const paths = Array.from(EditorManager.openFiles.keys()).filter(p => p !== fp)
  for (const path of paths) {
    handleTabClose(path)
  }
  closeContextMenu()
}

function handleCloseRightFromMenu() {
  const fp = contextMenu.value.filePath
  const paths = Array.from(EditorManager.openFiles.keys())
  const idx = paths.indexOf(fp)
  if (idx >= 0) {
    for (let i = idx + 1; i < paths.length; i++) {
      handleTabClose(paths[i])
    }
  }
  closeContextMenu()
}

function handleCloseAllFromMenu() {
  const paths = Array.from(EditorManager.openFiles.keys())
  for (const path of paths) {
    handleTabClose(path)
  }
  closeContextMenu()
}

function handlePinFromMenu() {
  const fp = contextMenu.value.filePath
  if (fp) togglePinTab(fp)
  closeContextMenu()
}

function handleCopyPathFromMenu() {
  const fp = contextMenu.value.filePath
  if (fp) copyToClipboard(fp)
  closeContextMenu()
}

function handleTabClick(filePath: string) {
  if (filePath !== EditorManager.activeFilePath) {
    EditorManager.switchToFile(filePath)
  }
}

function closeOnMiddleClick(e: MouseEvent, filePath: string) {
  if (e.button === 1) {
    e.preventDefault()
    handleTabClose(filePath)
  }
}

function handleTabClose(name: string) {
  closeFileChecked(name)
}

function handleExecute() {
  EditorManager.executeCurrentSQL()
}

function handleExecuteNew() {
  EditorManager.executeNewTabSQL()
}

function handleExecuteBatch() {
  EditorManager.executeBatchSQL()
}

function handleFormat() {
  EditorManager.formatSQL()
}

function handleValidate() {
  EditorManager.validateSQL()
}

function handleTranspile() {
  EditorManager.transpileSQL()
}

function handleExplain() {
  EditorManager.explainSQL()
}

function handleSaveSnippet() {
  EditorManager.saveSnippet()
}

function handleDuckDbExecute() {
  EditorManager.executeDuckDBAccelerated()
}

function handleConnectionChange(connId: string) {
  const fp = currentFilePath.value
  if (!fp) return
  const info = EditorManager.openFiles.get(fp)
  if (info) {
    info.connectionId = connId
  }
  showToast(`已切换连接: ${connId}`)
}

function handleStatusbarCancel() {
  EditorManager.cancelExecution()
}

function handleStatusbarCommit() {
  showToast('事务提交')
}

function handleStatusbarRollback() {
  showToast('事务回滚')
}

function handleToggleMinimap() {
  // 迷你地图切换由 useCodeMirror 处理
  showToast('迷你地图切换')
}

function handleToggleSettings() {
  showToast('编辑器设置')
}

function handleModeChange(mode: 'sql' | 'analysis' | 'code') {
  const fp = currentFilePath.value
  if (!fp) {
    showToast(
      `已切换到: ${mode === 'sql' ? 'SQL 模式' : mode === 'analysis' ? '分析模式' : '代码模式'}`
    )
    return
  }

  // 根据模式确定新的文件类型和语言
  let newType: 'file' | 'analysis' = 'file'
  let newLanguage = 'sql'
  switch (mode) {
    case 'sql':
      newType = 'file'
      newLanguage = 'sql'
      break
    case 'analysis':
      newType = 'analysis'
      newLanguage = 'sql'
      break
    case 'code':
      newType = 'file'
      newLanguage = 'typescript'
      break
  }

  // 更新文件信息
  EditorManager.changeFileType(fp, newType, newLanguage)
  // 更新 CodeMirror 语言模式
  setLanguage(newLanguage)

  showToast(
    `已切换到: ${mode === 'sql' ? 'SQL 模式' : mode === 'analysis' ? '分析模式' : '代码模式'}`
  )
}

function handleWelcomeConnect() {
  showWelcome.value = false
}

function startSplitDrag(e: MouseEvent) {
  e.preventDefault()
  isDragging.value = true
  document.addEventListener('mousemove', onSplitDrag)
  document.addEventListener('mouseup', stopSplitDrag)
}

function onSplitDrag(e: MouseEvent) {
  if (!isDragging.value) return
  const container = (e.target as HTMLElement).closest('.editor-split')
  if (!container) return
  const rect = container.getBoundingClientRect()
  const ratio = (e.clientY - rect.top) / rect.height
  splitRatio.value = Math.min(0.9, Math.max(0.1, ratio))
}

function stopSplitDrag() {
  isDragging.value = false
  document.removeEventListener('mousemove', onSplitDrag)
  document.removeEventListener('mouseup', stopSplitDrag)
}

onMounted(async () => {
  await nextTick()
  const el = editorContainerRef.value
  if (!el) return

  // 关闭溢出菜单的点击外部监听
  document.addEventListener('click', onDocumentClick)

  const theme = uiStore.theme === 'dark' ? 'dark' : 'light'
  const content = String(props.params.content || '')
  const strategy = classifyFileSize(content)
  largeFileTier.value = strategy.tier
  largeFileSizeMB.value = strategy.sizeMB

  if (strategy.tier === 'rejected') {
    showWelcome.value = true
    return
  }

  createView(
    el,
    strategy.tier === 'chunked' ? getChunkedContent(content, 0, 5000, 500) : content,
    currentLanguage.value,
    theme,
    (_doc, line, col, hasSelection) => {
      cursorPosition.value = `Ln ${line}, Col ${col}`
      if (hasSelection && view.value) {
        const sel = view.value.state.selection.main
        const text = view.value.state.doc.sliceString(sel.from, sel.to)
        const chars = text.length
        const lines = text.split('\n').length
        selectedTextInfo.value = `${chars} chars, ${lines} lines selected`
      } else {
        selectedTextInfo.value = ''
      }
      showWelcome.value = false
    },
    [],
    strategy
  )

  const fp = currentFilePath.value
  const currentView = view.value
  if (currentView) {
    EditorManager.setEditor(currentView)
    EditorManager.registerFileEditor(fp, currentView)
  }

  const savedState = EditorManager.getSavedStateForFile(fp)
  if (savedState && currentView) {
    setEditorState(savedState)
  }

  const parentEl = el.parentElement
  if (parentEl && currentView) {
    domObserver = new MutationObserver(mutations => {
      for (const m of mutations) {
        if (m.type === 'childList') {
          for (const node of m.addedNodes) {
            if (node === el) {
              currentView.requestMeasure()
              return
            }
          }
        }
      }
    })
    domObserver.observe(parentEl, { childList: true })
  }

  if (tabBarRef.value) {
    updateScrollState()
    scrollObserver = new ResizeObserver(() => updateScrollState())
    scrollObserver.observe(tabBarRef.value)
  }
})

onUnmounted(() => {
  document.removeEventListener('click', onDocumentClick)
  const fp = currentFilePath.value
  const state = getEditorState()
  if (state) EditorManager.saveEditorStateForFile(fp, state)
  EditorManager.unregisterFileEditor(fp)
  resultStore.clearEditorResults(fp)
  if (domObserver) {
    domObserver.disconnect()
    domObserver = null
  }
  if (scrollObserver) {
    scrollObserver.disconnect()
    scrollObserver = null
  }
  destroyView()
})

watch(
  () => EditorManager.activeFileInfo,
  info => {
    if (info) {
      showWelcome.value = false
    }
  },
  { immediate: true }
)

watch(
  () => uiStore.theme,
  theme => {
    setTheme(theme === 'dark' ? 'dark' : 'light')
  }
)
</script>

<style scoped>
.editor-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

.editor-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.readonly-warning {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 12px;
  font-size: 12px;
  color: #b8956a;
  background: rgba(184, 149, 106, 0.08);
  border-bottom: 1px solid rgba(184, 149, 106, 0.2);
}

/* ========== 标签栏包裹 ========== */
.tab-bar-wrap {
  flex-shrink: 0;
  display: flex;
  align-items: stretch;
  height: 35px;
  background: #252526;
  border-bottom: 1px solid var(--border-color, #3e3e42);
  position: relative;
}

/* 模式颜色条 */
.tab-mode-accent {
  width: 3px;
  flex-shrink: 0;
  transition: background 0.35s ease;
}
.tab-mode-accent.sql {
  background: #007acc;
}
.tab-mode-accent.analysis {
  background: #0e639c;
}
.tab-mode-accent.code {
  background: #68217a;
}

/* 滚动按钮 */
.tab-scroll-btn {
  width: 20px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: #6a6a6a;
  background: transparent;
  border: none;
  opacity: 0.4;
  transition: opacity 0.15s;
}
.tab-scroll-btn:hover {
  opacity: 1;
  color: #ccc;
}
.tab-scroll-btn:disabled {
  opacity: 0.1;
  cursor: default;
}

/* 标签栏 */
.tab-bar {
  flex: 1;
  display: flex;
  align-items: stretch;
  overflow-x: auto;
  overflow-y: hidden;
  scroll-behavior: smooth;
}
.tab-bar::-webkit-scrollbar {
  height: 0;
}

/* 标签项 */
.tab-item {
  display: flex;
  align-items: center;
  gap: 5px;
  height: 100%;
  min-width: 0;
  max-width: 180px;
  padding: 0 10px;
  font-size: 12px;
  color: #858585;
  background: #2d2d2d;
  border-right: 1px solid #252525;
  cursor: pointer;
  user-select: none;
  white-space: nowrap;
  transition:
    background 0.13s ease,
    color 0.13s ease;
  position: relative;
}
.tab-item:last-of-type {
  border-right: none;
}
.tab-item:hover {
  background: #383838;
  color: #ccc;
}
.tab-item.active {
  background: #1e1e1e;
  color: #e0e0e0;
  border-top: 2px solid #0066b8;
  padding-top: 0;
}

/* 文件类型颜色点 */
.tab-type-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
  opacity: 0.35;
  transition: opacity 0.2s;
}
.tab-item[data-type='sql'] .tab-type-dot {
  background: #4fc3f7;
}
.tab-item[data-type='analysis'] .tab-type-dot {
  background: #64b5f6;
}
.tab-item[data-type='code'] .tab-type-dot,
.tab-item[data-type='plaintext'] .tab-type-dot {
  background: #ce93d8;
}
.tab-item.active .tab-type-dot {
  opacity: 0.9;
}

/* 拖拽状态 */
.tab-item.drag-source {
  opacity: 0.4;
}
.tab-item.drag-over {
  border-left: 2px solid #007acc;
  background: rgba(0, 120, 212, 0.12);
}

/* 固定图标视觉增强 */
.tab-pin.is-pinned {
  opacity: 0.6 !important;
  color: #4fc3f7 !important;
}

/* 固定标签 */
.tab-item.pinned {
  max-width: 36px;
  min-width: 36px;
  justify-content: center;
  padding: 0 8px;
}
.tab-item.pinned .tab-label,
.tab-item.pinned .tab-type-dot,
.tab-item.pinned .tab-icon {
  display: none;
}
.tab-item.pinned .tab-dirty {
  position: absolute;
  top: 3px;
  right: 3px;
  width: 5px;
  height: 5px;
}
.tab-item.pinned .tab-close {
  display: none;
}

/* 固定图标 */
.tab-pin {
  font-size: 10px;
  opacity: 0;
  flex-shrink: 0;
  color: #6a6a6a;
  transition: all 0.15s;
  cursor: pointer;
  display: flex;
  align-items: center;
}
.tab-item.pinned .tab-pin {
  opacity: 0.5;
  color: #4fc3f7;
}
.tab-item:hover .tab-pin {
  opacity: 0.35;
}
.tab-item .tab-pin:hover {
  opacity: 1;
  color: #007acc;
}

/* 拖拽手柄 */
.tab-drag-handle {
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 4px;
  cursor: grab;
  opacity: 0;
  transition: opacity 0.15s;
}
.tab-item:hover .tab-drag-handle {
  opacity: 0.25;
}
.tab-drag-handle:hover {
  opacity: 0.7;
  background: #007acc;
}

/* 新建按钮 */
.tab-new-btn {
  width: 28px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: #858585;
  background: transparent;
  border: none;
  font-size: 16px;
  border-radius: 3px;
  margin: 3px 3px;
  transition: all 0.12s;
}
.tab-new-btn:hover {
  background: #333;
  color: #fff;
}

/* 溢出菜单 */
.tab-overflow-wrap {
  position: relative;
  flex-shrink: 0;
}
.tab-overflow-btn {
  width: 22px;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: #858585;
  background: transparent;
  border: none;
  border-radius: 0;
  transition: all 0.12s;
}
.tab-overflow-btn:hover {
  background: #333;
  color: #fff;
}
.tab-overflow-menu {
  position: absolute;
  top: 100%;
  right: 0;
  z-index: 10002;
  min-width: 180px;
  max-height: 300px;
  overflow-y: auto;
  background: #252526;
  border: 1px solid #3c3c3c;
  border-radius: 6px;
  padding: 4px 0;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.48);
}
.overflow-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  font-size: 12px;
  color: #ccc;
  cursor: pointer;
  user-select: none;
  transition: background 0.08s;
}
.overflow-item:hover {
  background: #094771;
  color: #fff;
}
.overflow-item.active {
  background: #094771;
}
.overflow-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  flex-shrink: 0;
}
.overflow-dot.sql {
  background: #4fc3f7;
}
.overflow-dot.analysis {
  background: #64b5f6;
}
.overflow-dot.code {
  background: #ce93d8;
}
.overflow-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.overflow-dirty {
  color: #f0c040;
  font-size: 10px;
  flex-shrink: 0;
}
.overflow-close {
  font-size: 14px;
  opacity: 0;
  flex-shrink: 0;
  padding: 0 2px;
}
.overflow-item:hover .overflow-close {
  opacity: 0.5;
}
.overflow-close:hover {
  opacity: 1 !important;
}

/* 面包屑 */
.editor-breadcrumb {
  height: 22px;
  display: flex;
  align-items: center;
  padding: 0 10px;
  gap: 2px;
  flex-shrink: 0;
  background: #1e1e1e;
  border-bottom: 1px solid var(--border-color, #3e3e42);
  font-size: 11px;
  color: #6a6a6a;
  overflow: hidden;
}
.crumb {
  cursor: pointer;
  padding: 1px 5px;
  border-radius: 3px;
  transition: all 0.1s;
  white-space: nowrap;
}
.crumb:hover {
  color: #ccc;
  background: #333;
}
.crumb-sep {
  color: #6a6a6a;
  opacity: 0.35;
  user-select: none;
  font-size: 9px;
}
.crumb-active {
  color: #ccc;
  font-weight: 500;
}

.tab-icon {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  color: inherit;
  opacity: 0.65;
}
.tab-item.active .tab-icon {
  opacity: 0.9;
}

.tab-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  min-width: 0;
}
.tab-label.modified {
  font-style: italic;
}

.tab-dirty {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: #f0c040;
  flex-shrink: 0;
  animation: dirty-pulse 2s ease-in-out infinite;
}

@keyframes dirty-pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.6;
  }
}

.tab-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 4px;
  flex-shrink: 0;
  opacity: 0;
  transition:
    opacity 0.1s ease,
    background 0.1s ease;
}

.tab-item:hover .tab-close,
.tab-item .tab-close.always {
  opacity: 0.6;
}
.tab-close:hover {
  opacity: 1 !important;
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
}

.tab-item.active .tab-close {
  opacity: 0;
}
.tab-item.active:hover .tab-close,
.tab-item.active .tab-close.always {
  opacity: 0.5;
}
.tab-item.active .tab-close:hover {
  opacity: 1 !important;
}

.editor-split {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.editor-area {
  min-height: 60px;
  overflow: hidden;
  position: relative;
}

.cm-container {
  width: 100%;
  height: 100%;
}

.split-handle {
  height: 4px;
  cursor: row-resize;
  background: var(--n-border-color);
  flex-shrink: 0;
}

.split-handle:hover {
  background: var(--n-color-target);
}

.result-area {
  min-height: 80px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  flex: 1;
}
</style>

<style>
.tab-context-menu {
  position: fixed;
  z-index: 10001;
  background: #252526;
  border: 1px solid #3c3c3c;
  border-radius: 6px;
  padding: 4px 0;
  min-width: 180px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.48);
}

.context-menu-item {
  padding: 7px 16px;
  font-size: 12px;
  color: #ccc;
  cursor: pointer;
  user-select: none;
  transition: background 0.08s ease;
  display: flex;
  align-items: center;
  gap: 8px;
}

.context-menu-item:hover {
  background: #094771;
  color: #fff;
}

.context-menu-sep {
  height: 1px;
  background: #3c3c3c;
  margin: 4px 0;
}

.context-menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 10000;
}

.large-file-warning {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  padding: 48px 24px;
  text-align: center;
  color: var(--n-text-color-2);
}

.large-file-icon {
  font-size: 48px;
  margin-bottom: 12px;
  opacity: 0.6;
}

.large-file-title {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 8px;
  color: var(--n-text-color);
}

.large-file-desc {
  font-size: 13px;
  line-height: 1.6;
  color: var(--n-text-color-3);
}
</style>
