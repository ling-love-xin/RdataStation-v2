/**
 * 项目状态管理
 *
 * 管理当前打开的项目
 * 使用 SQLite 作为数据源（混合架构）
 */

import { invoke } from '@tauri-apps/api/core'
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

import { ProjectService, type ProjectInfo } from '@/extensions/builtin/workbench/ui/services/project'

const DEBUG_PROJECT_STORE = false

function debugLog(...args: unknown[]): void {
  if (DEBUG_PROJECT_STORE) {
    // eslint-disable-next-line no-console
    console.log(...args)
  }
}

export interface Project {
  id: string
  name: string
  description?: string
  path: string
  createdAt: string
  updatedAt: string
}

export const useProjectStore = defineStore('project', () => {
  // ==================== State ====================
  const currentProject = ref<Project | null>(null)
  const recentProjects = ref<Project[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  // 缓存相关
  const lastLoadTime = ref<number>(0)
  const CACHE_TTL = 5 * 60 * 1000 // 5 分钟缓存有效期

  // ==================== Getters ====================
  const hasProject = computed(() => currentProject.value !== null)
  const projectPath = computed(() => currentProject.value?.path || null)
  const isCacheValid = computed(() => {
    return lastLoadTime.value > 0 && Date.now() - lastLoadTime.value < CACHE_TTL
  })

  // ==================== Actions ====================

  /**
   * 设置当前项目
   */
  async function setCurrentProject(project: Project | null): Promise<void> {
    currentProject.value = project

    if (project?.path) {
      try {
        debugLog('初始化项目存储:', project.path)
        await invoke('init_project_store', { projectPath: project.path })
        debugLog('项目存储初始化成功:', project.path)

        debugLog('初始化草稿箱存储:', project.path)
        await invoke('init_scratchpad_store', { projectPath: project.path })
        debugLog('草稿箱存储初始化成功:', project.path)
      } catch (e) {
        console.error('存储初始化失败:', e)
      }
    }
  }

  /**
   * 加载最近项目列表（从 SQLite，带缓存）
   * @param force 强制刷新，忽略缓存
   */
  async function loadRecentProjects(force = false): Promise<void> {
    // 如果缓存有效且不强制刷新，跳过
    if (!force && isCacheValid.value && recentProjects.value.length > 0) {
      return
    }

    try {
      const projects = await ProjectService.getRecentProjects(10)
      recentProjects.value = projects.map(p => ({
        id: p.id,
        name: p.name,
        description: p.description,
        path: p.path?.type === 'Local' ? p.path.path || '' : p.path.url || '',
        createdAt: p.created_at,
        updatedAt: p.updated_at,
      }))
      lastLoadTime.value = Date.now()
    } catch (e) {
      // 在开发环境或 Tauri 未初始化时，静默失败
      const errorMsg = e instanceof Error ? e.message : String(e)
      if (
        errorMsg.includes('invoke') ||
        errorMsg.includes('Tauri') ||
        errorMsg.includes('undefined')
      ) {
        recentProjects.value = []
        return
      }
      console.error('加载最近项目失败:', e)
      recentProjects.value = []
    }
  }

  /**
   * 加载上次打开的项目
   */
  async function loadLastProject(): Promise<Project | null> {
    try {
      // 从 SQLite 加载最近项目
      await loadRecentProjects()

      if (recentProjects.value.length > 0) {
        // 自动打开最近的项目
        const lastProject = recentProjects.value[0]
        await openProjectById(lastProject.id)
        return currentProject.value
      }

      // 如果没有项目，返回 null（由 UI 显示欢迎页面）
      return null
    } catch (e) {
      console.error('加载上次打开的项目失败:', e)
      return null
    }
  }

  /**
   * 通用的项目打开逻辑
   */
  async function openProjectInternal(openFn: () => Promise<ProjectInfo>): Promise<Project | null> {
    loading.value = true
    error.value = null

    try {
      const result = await openFn()

      const project: Project = {
        id: result.id,
        name: result.name,
        description: result.description,
        path: result.path?.type === 'Local' ? result.path.path || '' : result.path.url || '',
        createdAt: result.created_at,
        updatedAt: result.updated_at,
      }

      // 乐观更新
      currentProject.value = project

      // 初始化项目存储
      await setCurrentProject(project)

      // 强制刷新最近项目列表
      await loadRecentProjects(true)

      // 发射项目切换事件
      window.dispatchEvent(
        new CustomEvent('project-switched', {
          detail: { project },
        })
      )

      return project
    } catch (e) {
      error.value = e instanceof Error ? e.message : '打开项目失败'
      return null
    } finally {
      loading.value = false
    }
  }

  /**
   * 打开项目（根据路径）
   */
  async function openProject(path: string): Promise<Project | null> {
    return openProjectInternal(() => ProjectService.openProjectByPath(path))
  }

  /**
   * 打开项目（根据 ID）
   */
  async function openProjectById(id: string): Promise<Project | null> {
    return openProjectInternal(() => ProjectService.openProjectById(id))
  }

  /**
   * 创建新项目
   */
  async function createProject(
    name: string,
    path: string,
    description?: string
  ): Promise<Project | null> {
    loading.value = true
    error.value = null

    debugLog('[ProjectStore] 开始创建项目:', { name, path, description })

    try {
      const result = await ProjectService.createAndSaveProject({
        name,
        path,
        description,
      })

      debugLog('[ProjectStore] 后端返回结果:', result)

      const project: Project = {
        id: result.id,
        name: result.name,
        description: result.description,
        path: result.path?.type === 'Local' ? result.path.path || '' : result.path.url || '',
        createdAt: result.created_at,
        updatedAt: result.updated_at,
      }

      debugLog('[ProjectStore] 项目对象:', project)

      // 乐观更新
      currentProject.value = project

      // 初始化项目存储
      await setCurrentProject(project)

      // 强制刷新最近项目列表
      await loadRecentProjects(true)

      // 发射项目切换事件
      window.dispatchEvent(
        new CustomEvent('project-switched', {
          detail: { project },
        })
      )

      debugLog('[ProjectStore] 项目创建成功')

      return project
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      console.error('[ProjectStore] 创建项目失败:', errorMessage)
      error.value = errorMessage
      return null
    } finally {
      loading.value = false
    }
  }

  /**
   * 切换项目（乐观更新 + 后台同步 + 失败回滚）
   */
  async function switchProject(projectId: string): Promise<void> {
    // 保存当前状态用于回滚
    const previousProject = currentProject.value
    const previousRecentProjects = [...recentProjects.value]

    // 1. 乐观更新 UI
    const project = recentProjects.value.find(p => p.id === projectId)
    if (!project) {
      error.value = '项目不存在'
      throw new Error('项目不存在')
    }

    currentProject.value = project

    // 2. 后台同步到 SQLite
    try {
      await ProjectService.addRecentProject(projectId)

      // 强制刷新最近项目列表
      await loadRecentProjects(true)

      // 4. 初始化项目存储
      await setCurrentProject(project)

      // 5. 发射项目切换事件
      window.dispatchEvent(
        new CustomEvent('project-switched', {
          detail: { project },
        })
      )
    } catch (e) {
      // 失败回滚
      currentProject.value = previousProject
      recentProjects.value = previousRecentProjects
      error.value = e instanceof Error ? e.message : '切换项目失败'
      throw e
    }
  }

  /**
   * 删除项目（从全局数据库中移除）
   */
  async function deleteProject(projectId: string): Promise<void> {
    try {
      await ProjectService.deleteProject(projectId)

      if (currentProject.value?.id === projectId) {
        currentProject.value = null
      }

      await loadRecentProjects(true)
    } catch (e) {
      error.value = e instanceof Error ? e.message : '删除项目失败'
      throw e
    }
  }

  /**
   * 从最近列表中移除项目（不删除物理文件）
   */
  async function removeFromRecent(projectId: string): Promise<void> {
    try {
      await ProjectService.removeFromRecent(projectId)

      if (currentProject.value?.id === projectId) {
        currentProject.value = null
      }

      await loadRecentProjects(true)
    } catch (e) {
      error.value = e instanceof Error ? e.message : '移除项目失败'
      throw e
    }
  }

  /**
   * 物理删除项目（数据库 + 磁盘）
   */
  async function deleteProjectDisk(projectId: string): Promise<void> {
    try {
      await ProjectService.deleteProjectDisk(projectId)

      if (currentProject.value?.id === projectId) {
        currentProject.value = null
      }

      await loadRecentProjects(true)
    } catch (e) {
      error.value = e instanceof Error ? e.message : '删除项目失败'
      throw e
    }
  }

  /**
   * 更新项目信息（名称、描述）
   */
  async function updateProjectInfo(
    projectId: string,
    name: string,
    description?: string
  ): Promise<void> {
    try {
      await ProjectService.updateProject({
        id: projectId,
        name,
        description: description || undefined,
      })

      // 如果更新的是当前项目，更新本地状态
      if (currentProject.value?.id === projectId) {
        currentProject.value = {
          ...currentProject.value,
          name,
          description,
        }
      }

      // 强制刷新最近项目列表
      await loadRecentProjects(true)
    } catch (e) {
      error.value = e instanceof Error ? e.message : '更新项目失败'
      throw e
    }
  }

  /**
   * 关闭当前项目
   */
  async function closeProject(): Promise<void> {
    try {
      await invoke('close_project_store')
      currentProject.value = null
    } catch (e) {
      console.error('关闭项目存储失败:', e)
      throw e
    }
  }

  /**
   * 清除错误状态
   */
  function clearError(): void {
    error.value = null
  }

  return {
    // State
    currentProject,
    recentProjects,
    loading,
    error,
    // Getters
    hasProject,
    projectPath,
    isCacheValid,
    // Actions
    setCurrentProject,
    loadRecentProjects,
    loadLastProject,
    openProject,
    openProjectById,
    createProject,
    switchProject,
    deleteProject,
    removeFromRecent,
    deleteProjectDisk,
    updateProjectInfo,
    closeProject,
    clearError,
  }
})
