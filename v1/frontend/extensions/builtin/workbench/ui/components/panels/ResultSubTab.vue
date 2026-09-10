<template>
  <div class="result-sub-tab" @contextmenu="onResultTabContextMenu">
    <NTabs
      v-model:value="activeResultTab"
      type="bar"
      size="small"
      :closable="resultTabs.length > 1"
      @close="handleResultClose"
    >
      <NTabPane v-for="tab in resultTabs" :key="tab.id" :name="tab.id" :tab="tab.title" />
    </NTabs>
    <div class="result-sub-actions">
      <NButton v-if="showGridToolbar" quaternary size="tiny" @click="toggleGridToolbar">
        {{ gridToolbarVisible ? '隐藏工具栏' : '显示工具栏' }}
      </NButton>
    </div>

    <Teleport to="body">
      <div
        v-if="contextMenu.visible"
        class="result-context-menu"
        :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
        @click.stop
      >
        <div class="context-menu-item" @click="handleRenameFromMenu">重命名</div>
        <div class="context-menu-item dangerous" @click="handleCloseResultFromMenu">关闭</div>
      </div>
      <div
        v-if="contextMenu.visible"
        class="context-menu-backdrop"
        @click="closeContextMenu"
        @contextmenu.prevent="closeContextMenu"
      />
    </Teleport>

    <NModal v-model:show="renameModal.visible" :mask-closable="true" title="重命名结果集">
      <div class="rename-modal-body">
        <NInput
          ref="renameInputRef"
          v-model:value="renameModal.value"
          placeholder="输入新名称"
          @keydown.enter="confirmRename"
        />
        <div class="rename-modal-actions">
          <NButton size="small" @click="renameModal.visible = false">取消</NButton>
          <NButton size="small" type="primary" @click="confirmRename">确认</NButton>
        </div>
      </div>
    </NModal>
  </div>
</template>

<script setup lang="ts">
import { NTabs, NTabPane, NButton, NModal, NInput } from 'naive-ui'
import { ref, computed, nextTick } from 'vue'

import { EditorManager } from '@/extensions/builtin/workbench/manager/EditorManager'
import { ResultPanelManager } from '@/extensions/builtin/workbench/manager/ResultPanelManager'
import type { ResultSetMetadata } from '@/extensions/builtin/workbench/types/editor-types'

const gridToolbarVisible = ref(false)

const resultTabs = computed<ResultSetMetadata[]>(() => {
  const active = EditorManager.activeFileInfo
  return active?.resultSets ?? []
})

const activeResultTab = computed({
  get: () => {
    const active = EditorManager.activeFileInfo
    if (!active || active.activeResultIndex < 0) return ''
    return active.resultSets[active.activeResultIndex]?.id ?? ''
  },
  set: (val: string) => {
    const active = EditorManager.activeFileInfo
    if (!active) return
    const idx = active.resultSets.findIndex(rs => rs.id === val)
    if (idx >= 0) {
      EditorManager.setActiveResultIndex(active.filePath, idx)
      const panelId = active.resultPanelIds[idx]
      if (panelId && EditorManager.dockviewApi) {
        try {
          EditorManager.dockviewApi.getPanel(panelId)?.focus()
        } catch {
          console.warn('[ResultSubTab] dockview focus failed')
        }
      }
    }
  },
})

const showGridToolbar = computed(() => {
  const active = EditorManager.activeFileInfo
  return active ? active.resultSets.length > 0 : false
})

function handleResultClose(name: string) {
  const active = EditorManager.activeFileInfo
  if (!active) return
  ResultPanelManager.removeResultSet(active.filePath, name)
}

function toggleGridToolbar() {
  gridToolbarVisible.value = !gridToolbarVisible.value
}

const contextMenu = ref<{
  visible: boolean
  x: number
  y: number
  resultSetId: string
  panelId: string
}>({
  visible: false,
  x: 0,
  y: 0,
  resultSetId: '',
  panelId: '',
})

const renameModal = ref<{ visible: boolean; value: string; panelId: string }>({
  visible: false,
  value: '',
  panelId: '',
})

const renameInputRef = ref<InstanceType<typeof NInput> | null>(null)

function onResultTabContextMenu(e: MouseEvent) {
  const target = e.target as HTMLElement
  const tabEl = target.closest('.n-tabs-tab') as HTMLElement | null
  if (!tabEl) return
  e.preventDefault()

  const tabsContainer = tabEl.parentElement
  if (!tabsContainer) return
  const allTabs = Array.from(tabsContainer.querySelectorAll('.n-tabs-tab'))
  const idx = allTabs.indexOf(tabEl)
  if (idx < 0) return

  const tabs = resultTabs.value
  if (idx >= tabs.length) return

  const tab = tabs[idx]
  const active = EditorManager.activeFileInfo
  if (!active) return

  const panelId = active.resultPanelIds[idx] ?? ''
  contextMenu.value = {
    visible: true,
    x: e.clientX,
    y: e.clientY,
    resultSetId: tab.id,
    panelId,
  }
}

function closeContextMenu() {
  contextMenu.value.visible = false
}

function handleRenameFromMenu() {
  const panelId = contextMenu.value.panelId
  if (!panelId) return
  closeContextMenu()

  const active = EditorManager.activeFileInfo
  if (!active) return
  const rs = active.resultSets.find(r => r.id === contextMenu.value.resultSetId)
  renameModal.value = { visible: true, value: rs?.title ?? '结果集', panelId }
  nextTick(() => {
    renameInputRef.value?.focus()
  })
}

function confirmRename() {
  const newTitle = renameModal.value.value.trim()
  if (!newTitle) return
  EditorManager.renameResultSet(renameModal.value.panelId, newTitle)
  renameModal.value.visible = false
}

function handleCloseResultFromMenu() {
  const active = EditorManager.activeFileInfo
  if (!active) return
  ResultPanelManager.removeResultSet(active.filePath, contextMenu.value.resultSetId)
  closeContextMenu()
}
</script>

<style scoped>
.result-sub-tab {
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid var(--n-border-color);
  padding: 0 4px;
  flex-shrink: 0;
}

.result-sub-actions {
  flex-shrink: 0;
  padding: 0 8px;
}
</style>

<style>
.result-context-menu {
  position: fixed;
  z-index: 9999;
  background: var(--menu-bg, #252526);
  border: 1px solid var(--menu-border, #3c3c3c);
  border-radius: 6px;
  padding: 4px 0;
  min-width: 140px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.48);
  backdrop-filter: blur(8px);
}

.rename-modal-body {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-width: 280px;
}

.rename-modal-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
