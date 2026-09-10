import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { useProjectStore } from '@/core/project/stores/project'
import type { Project } from '@/core/project/stores/project'
import { useUiStore } from '@/shared/stores/ui'

import type { ToolbarTool } from '../components/title-bar/title-bar-types'

export function useTitleBar() {
  const { t } = useI18n()
  const uiStore = useUiStore()
  const projectStore = useProjectStore()

  /** Full project object (reactive) */
  const currentProject = computed<Project | null>(() => projectStore.currentProject)

  /** Display name for title bar */
  const currentProjectName = computed(
    () => projectStore.currentProject?.name || t('workbench.defaultProject')
  )

  /** Recent project list */
  const recentProjects = computed(() => projectStore.recentProjects)

  /** Loading state for project operations */
  const isOperationLoading = computed(() => projectStore.loading)

  // 加载最近项目
  async function loadRecentProjects() {
    await projectStore.loadRecentProjects()
  }

  // 切换项目
  async function switchProject(project: { id: string; name: string; path: string }) {
    await projectStore.switchProject(project.id)
  }

  // 创建新项目
  async function createProject(name: string, path: string, description?: string) {
    return await projectStore.createProject(name.trim(), path.trim(), description?.trim())
  }

  // 打开项目
  async function openProject(path: string) {
    return await projectStore.openProject(path)
  }

  // 重命名项目
  async function renameProject(projectId: string, newName: string) {
    await projectStore.updateProjectInfo(projectId, newName)
  }

  // 更新项目信息
  async function updateProjectInfo(projectId: string, name: string, description?: string) {
    await projectStore.updateProjectInfo(projectId, name, description)
  }

  // 从最近列表移除
  async function removeFromRecent(projectId: string) {
    await projectStore.removeFromRecent(projectId)
  }

  // 物理删除项目
  async function deleteProjectDisk(projectId: string) {
    await projectStore.deleteProjectDisk(projectId)
  }

  // 主题切换
  function toggleTheme() {
    uiStore.toggleTheme()
  }

  // 工具栏配置持久化
  const TOOLBAR_STORAGE_KEY = 'customToolbar'

  function saveToolbarConfig(tools: ToolbarTool[]) {
    localStorage.setItem(
      TOOLBAR_STORAGE_KEY,
      JSON.stringify(tools.map(t => ({ id: t.id, enabled: t.enabled })))
    )
  }

  function loadToolbarConfig(tools: ToolbarTool[]) {
    const saved = localStorage.getItem(TOOLBAR_STORAGE_KEY)
    if (saved) {
      try {
        const config = JSON.parse(saved) as Array<{ id: string; enabled: boolean }>
        tools.forEach(tool => {
          const savedTool = config.find((c: { id: string }) => c.id === tool.id)
          if (savedTool) {
            tool.enabled = savedTool.enabled
          }
        })
      } catch {
        // 解析失败时忽略
      }
    }
  }

  return {
    // State
    currentProject,
    currentProjectName,
    recentProjects,
    isOperationLoading,

    // Actions
    loadRecentProjects,
    switchProject,
    createProject,
    openProject,
    renameProject,
    updateProjectInfo,
    removeFromRecent,
    deleteProjectDisk,
    toggleTheme,
    saveToolbarConfig,
    loadToolbarConfig,
  }
}
