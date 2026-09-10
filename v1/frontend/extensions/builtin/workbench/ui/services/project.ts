import { invoke } from '@tauri-apps/api/core'

export interface ProjectInfo {
  id: string
  name: string
  description?: string
  path: {
    type: 'Local' | 'Remote'
    path?: string
    url?: string
    project_id?: string
  }
  status: string
  created_at: string
  updated_at: string
  last_opened_at?: string
  version: string
}

export interface CreateProjectInput {
  name: string
  path: string
  description?: string
}

interface RetryConfig {
  maxRetries?: number
  baseDelayMs?: number
  timeoutMs?: number
}

const DEFAULT_RETRY: RetryConfig = {
  maxRetries: 2,
  baseDelayMs: 300,
  timeoutMs: 15000,
}

async function withRetry<T>(fn: () => Promise<T>, config: RetryConfig = {}): Promise<T> {
  const {
    maxRetries = 2,
    baseDelayMs = 300,
    timeoutMs = 15000,
  } = {
    ...DEFAULT_RETRY,
    ...config,
  }

  let lastError: unknown

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      let timeoutId: ReturnType<typeof setTimeout> | undefined
      const result = await Promise.race([
        fn(),
        new Promise<never>((_, reject) => {
          timeoutId = setTimeout(() => reject(new Error('Operation timed out')), timeoutMs)
        }),
      ])
      if (timeoutId) clearTimeout(timeoutId)
      return result
    } catch (error) {
      lastError = error
      if (attempt < maxRetries) {
        const delay = baseDelayMs * Math.pow(2, attempt)
        await new Promise(resolve => setTimeout(resolve, delay))
      }
    }
  }

  throw lastError
}

export class ProjectService {
  /**
   * 获取最近项目列表
   * @param limit 返回数量限制，默认 10
   */
  static async getRecentProjects(limit: number = 10): Promise<ProjectInfo[]> {
    return withRetry(() => invoke<ProjectInfo[]>('get_recent_projects', { limit }))
  }

  /**
   * 根据 ID 打开项目
   * @param id 项目 ID
   */
  static async openProjectById(id: string): Promise<ProjectInfo> {
    return withRetry(() => invoke<ProjectInfo>('open_project_by_id', { id }))
  }

  /**
   * 根据路径打开项目
   * @param path 项目路径
   */
  static async openProjectByPath(path: string): Promise<ProjectInfo> {
    return withRetry(() => invoke<ProjectInfo>('open_project_by_path', { path }))
  }

  /**
   * 创建并保存项目到全局数据库
   * @param input 项目创建参数
   */
  static async createAndSaveProject(input: CreateProjectInput): Promise<ProjectInfo> {
    return withRetry(() => invoke<ProjectInfo>('create_and_save_project', { input }), {
      maxRetries: 1,
    })
  }

  /**
   * 添加到最近项目（更新最后打开时间）
   * @param projectId 项目 ID
   */
  static async addRecentProject(projectId: string): Promise<void> {
    return invoke<void>('add_recent_project', { projectId })
  }

  /**
   * 从最近列表中移除项目（不删除物理文件）
   * @param projectId 项目 ID
   * @returns 被移除的项目信息，用于 UI 回滚
   */
  static async removeFromRecent(projectId: string): Promise<ProjectInfo> {
    return invoke<ProjectInfo>('remove_from_recent', { projectId })
  }

  /**
   * 删除项目（从全局数据库中移除，不删物理文件）
   * @param projectId 项目 ID
   */
  static async deleteProject(projectId: string): Promise<void> {
    return invoke<void>('delete_project', { projectId })
  }

  /**
   * 更新项目信息（名称、描述）
   * @param input 项目 ID + 名称（必须）+ 描述（可选）
   */
  static async updateProject(input: {
    id: string
    name: string
    description?: string
  }): Promise<void> {
    return invoke<void>('update_project', { input })
  }

  /**
   * 删除项目（从数据库 + 物理删除磁盘目录）
   * @param projectId 项目 ID
   */
  static async deleteProjectDisk(projectId: string): Promise<void> {
    return invoke<void>('delete_project_disk', { projectId })
  }
}
