<template>
  <div ref="panelRef" class="project-switcher">
    <button class="trigger-btn no-drag" @click="togglePanel">
      <Loader2 v-if="projectStore.loading" :size="12" class="trigger-loading spin" />
      <FolderOpen v-else :size="14" class="trigger-icon" />
      <span class="trigger-name">{{ displayName }}</span>
      <ChevronDown :size="12" class="trigger-chevron" :class="{ open: showPanel }" />
    </button>

    <Transition name="panel-dropdown">
      <div v-if="showPanel" class="switcher-panel" :style="panelPosition">
        <!-- 顶部操作栏 -->
        <div class="panel-actions">
          <NButton size="small" quaternary @click="handleNewProject">
            <template #icon>
              <Plus :size="14" />
            </template>
            {{ t('workbench.newProject') }}
          </NButton>
          <NButton size="small" quaternary @click="handleOpenProject">
            <template #icon>
              <FolderSearch :size="14" />
            </template>
            {{ t('workbench.openExistingProject') }}
          </NButton>
        </div>

        <div class="panel-divider" />

        <!-- 最近项目列表 -->
        <div v-if="recentProjects.length > 0" class="project-list">
          <div class="section-label">{{ t('workbench.recentProjects') }}</div>
          <div
            v-for="project in recentProjects"
            :key="project.id"
            class="project-card"
            :class="{
              'is-current': project.id === currentProjectId,
            }"
            @click="handleCardClick(project)"
            @dblclick="handleCardDoubleClick(project)"
          >
            <div class="card-main">
              <FolderGit2 :size="18" class="card-icon" />
              <div class="card-info">
                <div class="card-name-row">
                  <NInput
                    v-if="renamingProjectId === project.id"
                    :value="renameValue"
                    size="small"
                    class="rename-input"
                    :placeholder="project.name"
                    @keyup.enter="handleRenameConfirm(project)"
                    @keyup.esc="cancelRename"
                    @update:value="renameValue = $event"
                    @blur="cancelRename"
                  />
                  <span v-else class="card-name">{{ project.name }}</span>
                </div>
                <div v-if="project.description" class="card-desc">{{ project.description }}</div>
                <div class="card-path">{{ project.path }}</div>
                <div v-if="project.updatedAt" class="card-meta">
                  {{ t('workbench.lastOpened') }}: {{ formatDateShort(project.updatedAt) }}
                </div>
              </div>
            </div>
            <div class="card-actions">
              <button
                class="action-btn more-btn"
                :title="t('workbench.moreActions')"
                @click.stop="openContextMenu($event, project)"
              >
                <MoreHorizontal :size="14" />
              </button>
              <button
                class="action-btn remove-btn"
                :title="t('workbench.removeFromRecent')"
                @click.stop="handleRemoveFromRecent(project.id)"
              >
                <X :size="14" />
              </button>
            </div>
          </div>
        </div>

        <!-- 空状态 -->
        <div v-else class="empty-state">
          <FolderOpen :size="32" class="empty-icon" />
          <div class="empty-title">{{ t('workbench.noRecentProjects') }}</div>
          <div class="empty-hint">{{ t('workbench.noRecentProjectsHint') }}</div>
        </div>

        <!-- 右键菜单 -->
        <div
          v-if="contextMenuProject"
          class="context-menu"
          :style="{
            top: contextMenuPosition.y + 'px',
            left: contextMenuPosition.x + 'px',
          }"
        >
          <div class="menu-item" @click="handleRenameStart">
            <Pencil :size="14" />
            <span>{{ t('workbench.renameProject') }}</span>
          </div>
          <div class="menu-item" @click="handleEditProject">
            <FileText :size="14" />
            <span>{{ t('workbench.editProjectInfo') }}</span>
          </div>
          <div class="menu-divider" />
          <div class="menu-item" @click="handleRemoveFromRecent(contextMenuProject.id)">
            <Trash2 :size="14" />
            <span>{{ t('workbench.removeFromRecent') }}</span>
          </div>
          <div class="menu-item menu-danger" @click="handleDeleteProject">
            <CircleAlert :size="14" />
            <span>{{ t('workbench.deleteProjectTitle') }}</span>
          </div>
        </div>

        <div class="panel-divider" />

        <!-- 底部入口 -->
        <div class="panel-footer">
          <NButton size="small" quaternary disabled>
            <template #icon>
              <LayoutList :size="14" />
            </template>
            {{ t('workbench.manageAllProjects') }}
          </NButton>
          <span class="coming-soon-tag">{{ t('workbench.comingSoon') }}</span>
        </div>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
import {
  ChevronDown,
  CircleAlert,
  FileText,
  FolderGit2,
  FolderOpen,
  FolderSearch,
  LayoutList,
  Loader2,
  MoreHorizontal,
  Pencil,
  Plus,
  Trash2,
  X,
} from 'lucide-vue-next'
import { NButton, NInput, useMessage } from 'naive-ui'
import { storeToRefs } from 'pinia'
import { nextTick, onBeforeUnmount, onMounted, ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { useProjectStore } from '@/core/project/stores/project'
import type { Project } from '@/core/project/stores/project'

import { formatDateShort } from '../utils/format'

interface Props {
  currentProjectId?: string | null
}

defineProps<Props>()

const emit = defineEmits<{
  'new-project': []
  'open-project': []
  'edit-project': [project: Project]
  'delete-project': [project: Project]
  close: []
}>()

const { t } = useI18n()
const message = useMessage()
const projectStore = useProjectStore()
const { recentProjects } = storeToRefs(projectStore)

const showPanel = ref(false)
const panelRef = ref<HTMLElement | null>(null)
const contextMenuProject = ref<Project | null>(null)
const contextMenuPosition = ref({ x: 0, y: 0 })
const renamingProjectId = ref<string | null>(null)
const renameValue = ref('')
const renameOriginalValue = ref('')

const displayName = computed(
  () => projectStore.currentProject?.name || t('workbench.defaultProject')
)

function togglePanel() {
  showPanel.value = !showPanel.value
  if (showPanel.value) {
    nextTick(() => {
      adjustPanelPosition()
      document.addEventListener('click', handleClickOutside, true)
      document.addEventListener('keydown', handleKeyDown)
      window.addEventListener('resize', handleResize)
    })
  } else {
    closePanel()
  }
}

const panelPosition = ref({ left: '0px', top: '0px' })

function adjustPanelPosition() {
  const trigger = panelRef.value?.querySelector('.trigger-btn') as HTMLElement
  if (!trigger) return

  const triggerRect = trigger.getBoundingClientRect()
  const panelWidth = 420
  const padding = 8
  const gap = 6

  let left = triggerRect.left
  if (triggerRect.right + panelWidth > window.innerWidth - padding) {
    left = window.innerWidth - panelWidth - padding
  }
  if (left < padding) {
    left = padding
  }

  const top = triggerRect.bottom + gap

  panelPosition.value = {
    left: `${left}px`,
    top: `${top}px`,
  }
}

function closePanel() {
  showPanel.value = false
  contextMenuProject.value = null
  renamingProjectId.value = null
  document.removeEventListener('click', handleClickOutside, true)
  document.removeEventListener('keydown', handleKeyDown)
  window.removeEventListener('resize', handleResize)
}

function handleResize() {
  if (showPanel.value) {
    adjustPanelPosition()
  }
}

function handleClickOutside(event: MouseEvent) {
  if (panelRef.value && !panelRef.value.contains(event.target as HTMLElement)) {
    closePanel()
  }
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    closePanel()
  }
}

function handleCardClick(project: Project) {
  if (project.id === projectStore.currentProject?.id) return

  contextMenuProject.value = null
  closePanel()
  projectStore.switchProject(project.id).catch(() => {
    message.error(t('workbench.switchProjectFailed'))
  })
}

function handleCardDoubleClick(project: Project) {
  handleCardClick(project)
}

function openContextMenu(event: MouseEvent, project: Project) {
  event.stopPropagation()
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()

  const menuWidth = 180
  const menuHeight = 160
  const padding = 8

  let menuX = rect.left - menuWidth
  let menuY = rect.bottom + 4

  if (menuX < padding) {
    menuX = rect.right + 4
  }

  if (menuX + menuWidth > window.innerWidth - padding) {
    menuX = window.innerWidth - menuWidth - padding
  }

  if (menuY + menuHeight > window.innerHeight - padding) {
    menuY = rect.top - menuHeight - 4
  }

  contextMenuPosition.value = { x: menuX, y: menuY }
  contextMenuProject.value = project

  nextTick(() => {
    document.addEventListener('click', closeContextMenu, { once: true })
  })
}

function closeContextMenu() {
  contextMenuProject.value = null
}

function handleRenameStart() {
  if (!contextMenuProject.value) return
  renamingProjectId.value = contextMenuProject.value.id
  renameValue.value = contextMenuProject.value.name
  renameOriginalValue.value = contextMenuProject.value.name
  contextMenuProject.value = null

  nextTick(() => {
    const input = document.querySelector('.rename-input input') as HTMLInputElement
    if (input) {
      input.focus()
      input.select()
    }
  })
}

function cancelRename() {
  renamingProjectId.value = null
  renameValue.value = ''
}

async function handleRenameConfirm(project: Project) {
  const newName = renameValue.value.trim()
  if (!newName || newName === project.name) {
    cancelRename()
    return
  }

  try {
    await projectStore.updateProjectInfo(project.id, newName, project.description)
    message.success(t('workbench.renameSuccess'))
    renamingProjectId.value = null
    renameValue.value = ''
    renameOriginalValue.value = ''
  } catch {
    message.error(t('workbench.renameFailed'))
    renameValue.value = renameOriginalValue.value
  }
}

function handleEditProject() {
  if (!contextMenuProject.value) return
  const project = contextMenuProject.value
  contextMenuProject.value = null
  closePanel()
  emit('edit-project', project)
}

async function handleRemoveFromRecent(projectId: string) {
  contextMenuProject.value = null
  try {
    await projectStore.removeFromRecent(projectId)
    message.success(t('workbench.projectRemoved'))
  } catch {
    message.error(t('workbench.removeFailed'))
  }
}

function handleDeleteProject() {
  if (!contextMenuProject.value) return
  const project = contextMenuProject.value
  contextMenuProject.value = null
  closePanel()
  emit('delete-project', project)
}

function handleNewProject() {
  emit('new-project')
  closePanel()
}

function handleOpenProject() {
  emit('open-project')
  closePanel()
}

onMounted(() => {
  document.addEventListener('keydown', handleEscapeGlobal)
})

onBeforeUnmount(() => {
  document.removeEventListener('click', handleClickOutside, true)
  document.removeEventListener('keydown', handleKeyDown)
  document.removeEventListener('keydown', handleEscapeGlobal)
  window.removeEventListener('resize', handleResize)
})

function handleEscapeGlobal(event: KeyboardEvent) {
  if (event.key === 'Escape' && showPanel.value) {
    closePanel()
  }
}
</script>

<style scoped>
.project-switcher {
  position: relative;
  margin-left: var(--spacing-sm);
}

.trigger-btn {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  padding: var(--spacing-xs) var(--spacing-sm);
  background: color-mix(in srgb, var(--color-bg-secondary) 94%, var(--color-text-primary) 6%);
  border: none;
  border-radius: var(--border-radius-sm);
  box-shadow:
    inset 0 1px 2px rgba(0, 0, 0, 0.1),
    inset 0 0 0 0.5px rgba(0, 0, 0, 0.06);
  color: var(--color-text-secondary);
  font-size: var(--font-size-md);
  cursor: pointer;
  transition: all 0.2s;
  height: 28px;
}

.trigger-btn:hover {
  background: color-mix(in srgb, var(--color-bg-secondary) 90%, var(--color-text-primary) 10%);
  box-shadow:
    inset 0 1px 2px rgba(0, 0, 0, 0.15),
    inset 0 0 0 0.5px rgba(0, 0, 0, 0.1);
}

.trigger-btn:active {
  box-shadow: inset 0 2px 3px rgba(0, 0, 0, 0.15);
}

.trigger-icon {
  color: var(--brand-accent);
  flex-shrink: 0;
}

.trigger-loading {
  color: var(--brand-accent);
  flex-shrink: 0;
}

.spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.trigger-name {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.trigger-chevron {
  transition: transform 0.2s;
  flex-shrink: 0;
}

.trigger-chevron.open {
  transform: rotate(180deg);
}

.switcher-panel {
  position: fixed;
  width: 420px;
  max-height: 520px;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-md);
  box-shadow: var(--dropdown-shadow);
  z-index: 1000;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.panel-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-sm) var(--spacing-md);
}

.panel-divider {
  height: 1px;
  background: var(--color-border-subtle);
  margin: 0;
  flex-shrink: 0;
}

.project-list {
  flex: 1;
  overflow-y: auto;
  padding: var(--spacing-xs) 0;
}

.section-label {
  padding: var(--spacing-xs) var(--spacing-md);
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.project-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-sm) var(--spacing-md);
  margin: 0 var(--spacing-xs);
  border: 1px solid transparent;
  border-radius: var(--border-radius-sm);
  cursor: pointer;
  transition:
    background 0.15s,
    border-color 0.15s;
}

.project-card:hover {
  background: var(--color-hover);
}

.project-card.is-current {
  border-color: var(--brand-accent);
  background: var(--brand-accent-soft);
}

.project-card.is-current:hover {
  background: var(--brand-accent-soft);
}

.card-main {
  display: flex;
  align-items: flex-start;
  gap: var(--spacing-sm);
  min-width: 0;
  flex: 1;
}

.card-icon {
  color: var(--brand-accent);
  flex-shrink: 0;
  margin-top: 2px;
}

.card-info {
  min-width: 0;
  flex: 1;
}

.card-name-row {
  display: flex;
  align-items: center;
}

.card-name {
  font-size: var(--font-size-md);
  font-weight: 500;
  color: var(--color-text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.rename-input {
  width: 100%;
}

.card-desc {
  font-size: var(--font-size-sm);
  color: var(--color-text-secondary);
  margin-top: 1px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.card-path {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  margin-top: 2px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: var(--font-mono);
}

.card-meta {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  margin-top: 2px;
}

.card-actions {
  display: flex;
  align-items: center;
  gap: var(--spacing-xs);
  opacity: 0;
  flex-shrink: 0;
  margin-left: var(--spacing-sm);
  transition: opacity 0.15s;
}

.project-card:hover .card-actions,
.card-actions.is-visible {
  opacity: 1;
}

.action-btn {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--color-text-secondary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--border-radius-sm);
  transition: all 0.15s;
}

.action-btn:hover {
  background: var(--color-hover);
  color: var(--color-text-primary);
}

.remove-btn:hover {
  background: transparent;
  color: var(--brand-danger);
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: var(--spacing-xl) var(--spacing-lg);
  text-align: center;
}

.empty-icon {
  color: var(--color-text-muted);
  margin-bottom: var(--spacing-md);
}

.empty-title {
  font-size: var(--font-size-lg);
  color: var(--color-text-secondary);
  margin-bottom: var(--spacing-xs);
}

.empty-hint {
  font-size: var(--font-size-sm);
  color: var(--color-text-muted);
  line-height: 1.5;
  max-width: 320px;
}

.context-menu {
  position: fixed;
  min-width: 180px;
  background: var(--color-bg-primary);
  border: 1px solid var(--color-border);
  border-radius: var(--border-radius-md);
  box-shadow: var(--shadow-lg);
  z-index: 2000;
  padding: var(--spacing-xs) 0;
}

.menu-item {
  display: flex;
  align-items: center;
  gap: var(--spacing-sm);
  padding: var(--spacing-sm) var(--spacing-md);
  font-size: var(--font-size-md);
  color: var(--color-text-secondary);
  cursor: pointer;
  transition: background 0.15s;
}

.menu-item:hover {
  background: var(--color-hover);
  color: var(--color-text-primary);
}

.menu-divider {
  height: 1px;
  background: var(--color-border-subtle);
  margin: var(--spacing-xs) 0;
}

.menu-danger {
  color: var(--brand-danger);
}

.menu-danger:hover {
  background: var(--brand-danger-soft);
  color: var(--brand-danger);
}

.panel-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--spacing-sm) var(--spacing-md);
}

.coming-soon-tag {
  font-size: var(--font-size-xs);
  color: var(--color-text-muted);
  background: var(--color-bg-tertiary);
  padding: 2px var(--spacing-sm);
  border-radius: var(--border-radius-pill);
}

.panel-dropdown-enter-active,
.panel-dropdown-leave-active {
  transition: all 0.15s ease;
}

.panel-dropdown-enter-from,
.panel-dropdown-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
