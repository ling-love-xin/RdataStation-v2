import { invoke } from '@tauri-apps/api/core'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))

vi.mock('@/extensions/builtin/workbench/ui/services/project', () => ({
  ProjectService: {
    getRecentProjects: vi.fn(),
    openProjectById: vi.fn(),
    openProjectByPath: vi.fn(),
    createAndSaveProject: vi.fn(),
    addRecentProject: vi.fn(),
    removeFromRecent: vi.fn(),
    deleteProject: vi.fn(),
    deleteProjectDisk: vi.fn(),
    updateProject: vi.fn(),
  },
}))
import { ProjectService } from '@/extensions/builtin/workbench/ui/services/project'

import { useProjectStore, type Project } from './project'

const mockInvoke = invoke as ReturnType<typeof vi.fn>
const mockService = ProjectService as unknown as Record<string, ReturnType<typeof vi.fn>>

function mockProjectInfo(overrides: Record<string, unknown> = {}) {
  return {
    id: 'proj-1',
    name: 'Test Project',
    description: 'test desc',
    path: { type: 'Local', path: '/test/path' },
    status: 'active',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-05-01T00:00:00Z',
    last_opened_at: '2026-05-10T00:00:00Z',
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-05-01T00:00:00Z',
    version: '1.0',
    ...overrides,
  }
}

describe('useProjectStore', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  describe('initial state', () => {
    it('should have null currentProject', () => {
      const store = useProjectStore()
      expect(store.currentProject).toBeNull()
    })

    it('should have empty recentProjects', () => {
      const store = useProjectStore()
      expect(store.recentProjects).toEqual([])
    })

    it('should have loading = false', () => {
      const store = useProjectStore()
      expect(store.loading).toBe(false)
    })

    it('should have null error', () => {
      const store = useProjectStore()
      expect(store.error).toBeNull()
    })

    it('should have hasProject = false', () => {
      const store = useProjectStore()
      expect(store.hasProject).toBe(false)
    })
  })

  describe('setCurrentProject', () => {
    it('should set currentProject and init store', async () => {
      const store = useProjectStore()
      mockInvoke.mockResolvedValue(undefined)

      const project = { id: 'p1', name: 'P1', path: '/p1', createdAt: '', updatedAt: '' }
      await store.setCurrentProject(project)

      expect(store.currentProject).toEqual(project)
      expect(mockInvoke).toHaveBeenCalledWith('init_project_store', {
        projectPath: '/p1',
      })
    })

    it('should handle init_project_store failure gracefully', async () => {
      const store = useProjectStore()
      mockInvoke.mockRejectedValue(new Error('init failed'))

      const project = { id: 'p1', name: 'P1', path: '/p1', createdAt: '', updatedAt: '' }
      await store.setCurrentProject(project)

      expect(store.currentProject).toEqual(project)
    })
  })

  describe('loadRecentProjects', () => {
    it('should load and map recent projects', async () => {
      const store = useProjectStore()
      mockService.getRecentProjects.mockResolvedValue([
        mockProjectInfo({ id: 'p1', name: 'Project 1' }),
        mockProjectInfo({ id: 'p2', name: 'Project 2' }),
      ])

      await store.loadRecentProjects(true)

      expect(store.recentProjects).toHaveLength(2)
      expect(store.recentProjects[0].name).toBe('Project 1')
      expect(store.recentProjects[0].path).toBe('/test/path')
    })

    it('should use cache when valid', async () => {
      const store = useProjectStore()
      mockService.getRecentProjects.mockResolvedValue([
        mockProjectInfo({ id: 'p1', name: 'Cached' }),
      ])

      await store.loadRecentProjects(true)
      vi.clearAllMocks()

      await store.loadRecentProjects(false)

      expect(mockService.getRecentProjects).not.toHaveBeenCalled()
    })
  })

  describe('openProjectById', () => {
    it('should open project and emit event', async () => {
      const store = useProjectStore()
      mockService.openProjectById.mockResolvedValue(mockProjectInfo())
      mockInvoke.mockResolvedValue(undefined)
      mockService.getRecentProjects.mockResolvedValue([])

      const eventSpy = vi.fn()
      window.addEventListener('project-switched', eventSpy)

      const result = await store.openProjectById('proj-1')

      expect(result).not.toBeNull()
      // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
      expect(result!.name).toBe('Test Project')
      expect(store.currentProject).not.toBeNull()
      expect(eventSpy).toHaveBeenCalled()

      window.removeEventListener('project-switched', eventSpy)
    })

    it('should return null and set error on failure', async () => {
      const store = useProjectStore()
      mockService.openProjectById.mockRejectedValue(new Error('Not found'))

      const result = await store.openProjectById('bad-id')

      expect(result).toBeNull()
      expect(store.error).toContain('Not found')
    })
  })

  describe('switchProject', () => {
    it('should optimistically update currentProject then sync', async () => {
      const store = useProjectStore()
      const project = mockProjectInfo({ id: 'p1', name: 'Switched' })
      store.recentProjects = [project as unknown as Project]
      mockService.addRecentProject.mockResolvedValue(undefined)
      mockService.getRecentProjects.mockResolvedValue([project])
      mockInvoke.mockResolvedValue(undefined)

      await store.switchProject('p1')

      expect(store.currentProject).toEqual(project)
      expect(mockService.addRecentProject).toHaveBeenCalledWith('p1')
    })

    it('should throw and keep previous project on sync failure', async () => {
      const store = useProjectStore()
      const project = mockProjectInfo({ id: 'p1', name: 'Switched' })
      store.recentProjects = [project as unknown as Project]
      mockService.addRecentProject.mockRejectedValue(new Error('Network error'))

      await expect(store.switchProject('p1')).rejects.toThrow('Network error')
      expect(store.currentProject).toBeNull()
      expect(store.error).toContain('Network error')
    })
  })

  describe('removeFromRecent', () => {
    it('should remove project and refresh list', async () => {
      const store = useProjectStore()
      mockService.removeFromRecent.mockResolvedValue(mockProjectInfo())
      mockService.getRecentProjects.mockResolvedValue([])

      await store.removeFromRecent('proj-1')

      expect(mockService.removeFromRecent).toHaveBeenCalledWith('proj-1')
      expect(mockService.getRecentProjects).toHaveBeenCalled()
    })
  })

  describe('deleteProject', () => {
    it('should clear currentProject if deleting current', async () => {
      const store = useProjectStore()
      mockService.deleteProject.mockResolvedValue(undefined)
      mockService.getRecentProjects.mockResolvedValue([])
      mockInvoke.mockResolvedValue(undefined)

      const project = { id: 'p1', name: 'P1', path: '/p1', createdAt: '', updatedAt: '' }
      await store.setCurrentProject(project)
      expect(store.currentProject).not.toBeNull()

      await store.deleteProject('p1')

      expect(store.currentProject).toBeNull()
      expect(mockService.deleteProject).toHaveBeenCalledWith('p1')
    })
  })

  describe('closeProject', () => {
    it('should close store and clear currentProject', async () => {
      const store = useProjectStore()
      mockInvoke.mockResolvedValue(undefined)

      const project = { id: 'p1', name: 'P1', path: '/p1', createdAt: '', updatedAt: '' }
      store.setCurrentProject(project)
      await store.closeProject()

      expect(store.currentProject).toBeNull()
      expect(mockInvoke).toHaveBeenCalledWith('close_project_store')
    })

    it('should throw and NOT clear currentProject on failure', async () => {
      const store = useProjectStore()
      mockInvoke.mockRejectedValue(new Error('close failed'))

      const project = { id: 'p1', name: 'P1', path: '/p1', createdAt: '', updatedAt: '' }
      await store.setCurrentProject(project)
      mockInvoke.mockClear()

      await expect(store.closeProject()).rejects.toThrow('close failed')
      expect(store.currentProject).not.toBeNull()
    })
  })

  describe('clearError', () => {
    it('should clear error state', () => {
      const store = useProjectStore()
      store.$patch({ error: 'some error' })

      store.clearError()

      expect(store.error).toBeNull()
    })
  })
})
