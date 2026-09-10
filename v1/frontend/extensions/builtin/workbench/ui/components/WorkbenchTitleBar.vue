<template>
  <div class="title-bar" data-tauri-drag-region>
    <div class="title-bar-left">
      <MenuBar
        v-if="titleBarSettings.menuStyle !== 'hidden'"
        :menus="menuConfig"
        @menu-action="handleMenuAction"
      />
      <ProjectSwitcherPanel
        v-if="titleBarSettings.showProjectSelector"
        :current-project-id="projectStore.currentProject?.id"
        @new-project="showNewProjectModal = true"
        @open-project="handleOpenProject"
        @edit-project="handleEditProject"
        @delete-project="handleDeleteProjectBtn"
      />
    </div>

    <div class="title-bar-center" data-tauri-drag-region @dblclick="handleTitleBarDoubleClick">
      <CommandCenter v-if="titleBarSettings.showCommandCenter" @open="handleOpenCommandPalette" />
    </div>

    <div class="title-bar-right">
      <ToolbarActions
        :tools="toolbarTools"
        @tool-action="handleToolAction"
        @toggle-tool="handleToggleTool"
        @reset-toolbar="handleResetToolbar"
      />

      <button
        class="icon-btn theme-toggle-btn"
        :title="themeToggleTooltip"
        :aria-label="themeToggleTooltip"
        @click="handleToggleTheme"
      >
        <Sun v-if="appStore.effectiveTheme === 'light'" :size="14" />
        <Moon v-else-if="appStore.effectiveTheme === 'dark'" :size="14" />
        <Monitor v-else :size="14" />
      </button>

      <div class="layout-controls">
        <button
          class="icon-btn layout-btn"
          :title="t('workbench.customizeLayout')"
          @click="handleCustomizeLayout"
        >
          <LayoutTemplate :size="14" />
        </button>
        <button class="icon-btn layout-btn" :title="t('workbench.maximize')">
          <PanelTop :size="14" />
        </button>
      </div>

      <WindowControls
        :is-maximized="isMaximized"
        @minimize="$emit('minimize')"
        @maximize="$emit('maximize')"
        @close="$emit('close')"
      />
    </div>
  </div>

  <NewProjectModal
    :visible="showNewProjectModal"
    @confirm="handleCreateProject"
    @cancel="showNewProjectModal = false"
  />

  <EditProjectModal
    :visible="showEditProjectModal"
    :project="editProjectTarget"
    @confirm="handleProjectInfoUpdated"
    @cancel="showEditProjectModal = false"
  />

  <DeleteProjectConfirmModal
    :visible="showDeleteProjectModal"
    :project="deleteProjectTarget"
    @confirm="handleProjectDeleted"
    @cancel="showDeleteProjectModal = false"
  />

  <CommandPalette :visible="showCommandPalette" @close="showCommandPalette = false" />

  <InvalidProjectDialog
    :visible="showInvalidProjectDialog"
    :selected-path="invalidPath"
    @browse="handleBrowseAgain"
    @close="showInvalidProjectDialog = false"
  />
</template>

<script setup lang="ts">
import { LayoutTemplate, Monitor, Moon, PanelTop, Sun } from 'lucide-vue-next'
import { useMessage } from 'naive-ui'
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { useProjectStore } from '@/core/project/stores/project'
import type { Project } from '@/core/project/stores/project'
import { useUiStore } from '@/shared/stores/ui'
import { useAppStore } from '@/stores/useAppStore'

import DeleteProjectConfirmModal from './DeleteProjectConfirmModal.vue'
import EditProjectModal from './EditProjectModal.vue'
import ProjectSwitcherPanel from './ProjectSwitcherPanel.vue'
import { useTitleBar } from '../composables/useTitleBar'
import {
  createMenuActionMap,
  createMenuConfig,
  createToolbarConfig,
} from '../config/title-bar-config'
import { WorkbenchEvent, dispatchWorkbenchEvent } from '../constants/workbench-events'
import { useCommandStore } from '../stores/command-store'
import CommandCenter from './title-bar/CommandCenter.vue'
import CommandPalette from './title-bar/CommandPalette.vue'
import InvalidProjectDialog from './title-bar/InvalidProjectDialog.vue'
import MenuBar from './title-bar/MenuBar.vue'
import NewProjectModal from './title-bar/NewProjectModal.vue'
import ToolbarActions from './title-bar/ToolbarActions.vue'
import WindowControls from './title-bar/WindowControls.vue'

import type { MenuItem, ToolbarTool } from './title-bar/title-bar-types'

interface Props {
  isMaximized?: boolean
}

withDefaults(defineProps<Props>(), {
  isMaximized: false,
})

defineEmits<{
  minimize: []
  maximize: []
  close: []
}>()

const { t } = useI18n()
const message = useMessage()
const uiStore = useUiStore()
const appStore = useAppStore()
const projectStore = useProjectStore()
const titleBar = useTitleBar()

// 从配置系统读取标题栏设置
const titleBarSettings = computed(() => appStore.effectiveTitleBarSettings)

const themeToggleTooltip = computed(() => {
  switch (appStore.effectiveTheme) {
    case 'dark':
      return t('workbench.themeDark')
    case 'light':
      return t('workbench.themeLight')
    default:
      return t('workbench.themeSystem')
  }
})

async function handleToggleTheme() {
  try {
    await uiStore.toggleTheme()
  } catch {
    message.error(t('common.operationFailed'))
  }
}

const showNewProjectModal = ref(false)
const showCommandPalette = ref(false)
const showEditProjectModal = ref(false)
const editProjectTarget = ref<Project | null>(null)
const showDeleteProjectModal = ref(false)
const deleteProjectTarget = ref<Project | null>(null)
const showInvalidProjectDialog = ref(false)
const invalidPath = ref('')

// 命令注册
const commandStore = useCommandStore()

function registerCommands() {
  commandStore.register({
    id: 'newQuery',
    label: t('menu.newQuery'),
    category: 'file',
    shortcut: 'Ctrl+N',
    action: () => window.dispatchEvent(new CustomEvent('workbench:new-query')),
  })
  commandStore.register({
    id: 'newConnection',
    label: t('menu.newConnection'),
    category: 'connection',
    shortcut: 'Ctrl+Shift+N',
    action: () => window.dispatchEvent(new CustomEvent('workbench:new-connection')),
  })
  commandStore.register({
    id: 'openProject',
    label: t('menu.openProject'),
    category: 'file',
    shortcut: 'Ctrl+O',
    action: () => handleOpenProject(),
  })
  commandStore.register({
    id: 'save',
    label: t('menu.save'),
    category: 'file',
    shortcut: 'Ctrl+S',
    action: () => window.dispatchEvent(new CustomEvent('workbench:save')),
  })
  commandStore.register({
    id: 'executeSql',
    label: t('menu.executeSql'),
    category: 'run',
    shortcut: 'Ctrl+Enter',
    action: () => window.dispatchEvent(new CustomEvent('workbench:execute-sql')),
  })
  commandStore.register({
    id: 'settings',
    label: t('menu.settings'),
    category: 'tools',
    shortcut: 'Ctrl+,',
    action: () => window.dispatchEvent(new CustomEvent('workbench:open-settings')),
  })
  commandStore.register({
    id: 'commandPalette',
    label: t('menu.commandPalette'),
    category: 'view',
    shortcut: 'Ctrl+Shift+P',
    action: () => (showCommandPalette.value = true),
  })
}

// 菜单配置
const menuConfig = createMenuConfig(t, handleOpenProject, handleOpenCommandPalette)

// 工具栏配置 - 从配置系统读取启用状态
const toolbarTools = computed<ToolbarTool[]>(() => {
  const enabledIds = new Set(titleBarSettings.value.toolbarTools)
  return createToolbarConfig(t, handleOpenCommandPalette).map(tool => ({
    ...tool,
    enabled: enabledIds.has(tool.id),
  }))
})

// 菜单动作映射表
const menuActionMap = createMenuActionMap(handleOpenProject, handleOpenCommandPalette)

// 菜单动作处理
function handleMenuAction(item: MenuItem): void {
  const action = menuActionMap[item.id]
  if (action) {
    action()
  }
}

function handleEditProject(project: Project) {
  editProjectTarget.value = project
  showEditProjectModal.value = true
}

function handleDeleteProjectBtn(project: Project) {
  deleteProjectTarget.value = project
  showDeleteProjectModal.value = true
}

function handleProjectInfoUpdated() {
  showEditProjectModal.value = false
  editProjectTarget.value = null
}

function handleProjectDeleted() {
  showDeleteProjectModal.value = false
  deleteProjectTarget.value = null
}

async function handleOpenProject() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const selected = await open({
      directory: true,
      multiple: false,
      title: t('workbench.selectProjectFolder'),
    })

    if (selected && typeof selected === 'string') {
      const project = await titleBar.openProject(selected)
      if (!project) {
        invalidPath.value = selected
        showInvalidProjectDialog.value = true
      }
    }
  } catch {
    message.error(t('workbench.openProjectFailed'))
  }
}

async function handleBrowseAgain() {
  showInvalidProjectDialog.value = false
  await handleOpenProject()
}

async function handleCreateProject(name: string, path: string, description?: string) {
  try {
    const project = await titleBar.createProject(name, path, description)
    if (project) {
      showNewProjectModal.value = false
      message.success(t('workbench.createProjectSuccess'))
    }
  } catch {
    message.error(t('workbench.createProjectFailed'))
  }
}

function handleOpenCommandPalette() {
  showCommandPalette.value = true
}

function handleToolAction(toolId: string) {
  const tool = toolbarTools.value.find(t => t.id === toolId)
  if (tool?.action) {
    tool.action()
  }
}

function handleToggleTool(toolId: string, enabled: boolean) {
  const currentTools = [...titleBarSettings.value.toolbarTools]
  if (enabled) {
    if (!currentTools.includes(toolId)) {
      currentTools.push(toolId)
    }
  } else {
    const index = currentTools.indexOf(toolId)
    if (index > -1) {
      currentTools.splice(index, 1)
    }
  }
  appStore.setTitleBarSettings({ ...titleBarSettings.value, toolbarTools: currentTools })
}

function handleResetToolbar() {
  appStore.setTitleBarSettings({ ...titleBarSettings.value, toolbarTools: [] })
}

function handleCustomizeLayout() {
  dispatchWorkbenchEvent(WorkbenchEvent.OpenCustomizeLayout)
}

function handleTitleBarDoubleClick() {
  dispatchWorkbenchEvent(WorkbenchEvent.TitleBarDoubleClick)
}

// 全局键盘快捷键
function handleGlobalKeyDown(event: KeyboardEvent) {
  // Ctrl+Shift+P / Cmd+Shift+P: 打开命令面板
  if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 'p') {
    event.preventDefault()
    handleOpenCommandPalette()
    return
  }

  // Ctrl+Shift+N / Cmd+Shift+N: 新建连接
  if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === 'n') {
    event.preventDefault()
    window.dispatchEvent(new CustomEvent('workbench:new-connection'))
    return
  }

  // Ctrl+N / Cmd+N: 新建查询
  if ((event.ctrlKey || event.metaKey) && !event.shiftKey && event.key.toLowerCase() === 'n') {
    event.preventDefault()
    window.dispatchEvent(new CustomEvent('workbench:new-query'))
    return
  }
}

// 生命周期
onMounted(async () => {
  await titleBar.loadRecentProjects()
  registerCommands()
  document.addEventListener('keydown', handleGlobalKeyDown, true)
})

onUnmounted(() => {
  document.removeEventListener('keydown', handleGlobalKeyDown, true)
})
</script>

<style scoped>
@import './title-bar/title-bar.css';

.theme-toggle-btn {
  margin-right: var(--spacing-xs);
}
</style>
