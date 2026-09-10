/**
 * projectConnectionStore 项目连接管理 Store 单元测试
 *
 * 测试所有 state / getters / actions
 * 依赖 @/core/project/stores/project、project-connection service 均 mock
 */
import { setActivePinia, createPinia } from 'pinia'
import { describe, expect, it, vi, beforeEach } from 'vitest'

import { useProjectConnectionStore } from '../project-connection-store'

import type { ProjectConnection } from '../../../types/connection'

// ==================== Mock 数据 ====================

function makeMockProjectConnection(overrides: Partial<ProjectConnection> = {}): ProjectConnection {
  return {
    id: 'pc-001',
    name: 'Test MySQL',
    driver: 'mysql',
    host: 'localhost',
    port: 3306,
    database: 'testdb',
    username: 'admin',
    password: 'secret',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-02T00:00:00Z',
    connection_type: 'project',
    project_path: '/test/proj',
    status: 'disconnected',
    ...overrides,
  }
}

const mockCreatedConnectionResponse: ProjectConnection = {
  id: 'pc-new',
  name: 'New PG',
  driver: 'postgres',
  host: '10.0.0.1',
  port: 5432,
  database: 'newdb',
  created_at: '2026-06-01T00:00:00Z',
  updated_at: '2026-06-01T00:00:00Z',
  connection_type: 'project',
  project_path: '/test/proj',
  status: 'disconnected',
}

// ==================== Mock 依赖 ====================

// Mock @tauri-apps/api/core (used by the real service)
const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}))

// Mock project store
const mockProjectStore = {
  currentProject: null as { path: string; id: string } | null,
}
vi.mock('@/core/project/stores/project', () => ({
  useProjectStore: () => mockProjectStore,
}))

// Mock project-connection service
const mockGetProjectConnections = vi.fn()
const mockCreateProjectConnection = vi.fn()
const mockUpdateProjectConnection = vi.fn()
const mockDeleteProjectConnection = vi.fn()
const mockUpdateProjectConnectionStatus = vi.fn()
const mockSearchProjectConnections = vi.fn()
const mockBuildConnectionUrl = vi.fn()
const mockGetConnectionDisplayName = vi.fn()

vi.mock('../../services/project-connection', () => ({
  getProjectConnections: (...args: unknown[]) => mockGetProjectConnections(...args),
  createProjectConnection: (...args: unknown[]) => mockCreateProjectConnection(...args),
  updateProjectConnection: (...args: unknown[]) => mockUpdateProjectConnection(...args),
  deleteProjectConnection: (...args: unknown[]) => mockDeleteProjectConnection(...args),
  updateProjectConnectionStatus: (...args: unknown[]) => mockUpdateProjectConnectionStatus(...args),
  searchProjectConnections: (...args: unknown[]) => mockSearchProjectConnections(...args),
  buildConnectionUrl: (conn: ProjectConnection) => mockBuildConnectionUrl(conn),
  getConnectionDisplayName: (conn: ProjectConnection) => mockGetConnectionDisplayName(conn),
}))

// ==================== Store 导入 ====================

// ==================== 测试辅助 ====================

function createStore() {
  const pinia = createPinia()
  setActivePinia(pinia)
  return useProjectConnectionStore()
}

function resetAllMocks() {
  vi.clearAllMocks()
  mockProjectStore.currentProject = null
  mockInvoke.mockReset()
  mockGetProjectConnections.mockReset()
  mockCreateProjectConnection.mockReset()
  mockUpdateProjectConnection.mockReset()
  mockDeleteProjectConnection.mockReset()
  mockUpdateProjectConnectionStatus.mockReset()
  mockSearchProjectConnections.mockReset()
  mockBuildConnectionUrl.mockReset()
  mockGetConnectionDisplayName.mockReset()
}

// ==================== 1. Store 初始化 ====================

describe('Store 初始化 - 默认状态', () => {
  beforeEach(() => resetAllMocks())

  it('connections 初始为空数组', () => {
    const store = createStore()
    expect(store.connections).toEqual([])
  })

  it('currentConnection 初始为 null', () => {
    const store = createStore()
    expect(store.currentConnection).toBeNull()
  })

  it('loading 初始为 false', () => {
    const store = createStore()
    expect(store.loading).toBe(false)
  })

  it('error 初始为 null', () => {
    const store = createStore()
    expect(store.error).toBeNull()
  })
})

// ==================== 2. Computed Getters ====================

describe('Computed Getters', () => {
  beforeEach(() => resetAllMocks())

  it('connectionCount 返回连接数量', () => {
    const store = createStore()
    expect(store.connectionCount).toBe(0)
    store.connections.push(makeMockProjectConnection({ id: '1' }))
    expect(store.connectionCount).toBe(1)
    store.connections.push(makeMockProjectConnection({ id: '2' }))
    expect(store.connectionCount).toBe(2)
  })

  it('hasConnections 有连接时返回 true', () => {
    const store = createStore()
    expect(store.hasConnections).toBe(false)
    store.connections.push(makeMockProjectConnection())
    expect(store.hasConnections).toBe(true)
  })

  it('isConnected 有当前连接时返回 true', () => {
    const store = createStore()
    expect(store.isConnected).toBe(false)
    store.currentConnection = makeMockProjectConnection()
    expect(store.isConnected).toBe(true)
  })

  it('connectionsByType 按 driver 分组', () => {
    const store = createStore()
    store.connections.push(
      makeMockProjectConnection({ id: '1', driver: 'mysql' }),
      makeMockProjectConnection({ id: '2', driver: 'mysql' }),
      makeMockProjectConnection({ id: '3', driver: 'postgres' }),
      makeMockProjectConnection({ id: '4', driver: 'sqlite' })
    )

    const grouped = store.connectionsByType
    expect(Object.keys(grouped)).toHaveLength(3)
    expect(grouped['mysql']).toHaveLength(2)
    expect(grouped['postgres']).toHaveLength(1)
    expect(grouped['sqlite']).toHaveLength(1)
  })

  it('connectionsByType 空列表返回空对象', () => {
    const store = createStore()
    expect(store.connectionsByType).toEqual({})
  })
})

// ==================== 3. loadConnections ====================

describe('loadConnections', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：加载项目连接列表', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockGetProjectConnections.mockResolvedValue([
      makeMockProjectConnection({ id: 'pc-1', name: 'Conn 1' }),
      makeMockProjectConnection({ id: 'pc-2', name: 'Conn 2' }),
    ])

    const store = createStore()
    await store.loadConnections()

    expect(store.connections).toHaveLength(2)
    expect(store.connections[0].connection_type).toBe('project')
    expect(store.loading).toBe(false)
  })

  it('错误路径：没有打开的项目时设置 error', async () => {
    mockProjectStore.currentProject = null

    const store = createStore()
    await store.loadConnections()

    expect(store.error).toBe('没有打开的项目')
    expect(store.connections).toHaveLength(0)
  })

  it('服务调用失败时设置 error', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockGetProjectConnections.mockRejectedValue(new Error('网络错误'))

    const store = createStore()
    await store.loadConnections()

    expect(store.error).toBe('网络错误')
    expect(store.loading).toBe(false)
  })
})

// ==================== 4. createConnection ====================

describe('createConnection', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：创建连接并加入列表', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockCreateProjectConnection.mockResolvedValue(mockCreatedConnectionResponse)

    const store = createStore()
    const result = await store.createConnection({
      name: 'New PG',
      driver: 'postgres',
      host: '10.0.0.1',
      port: 5432,
      database: 'newdb',
    })

    expect(result).toBeDefined()
    expect(result!.id).toBe('pc-new')
    expect(store.connections).toHaveLength(1)
    expect(store.loading).toBe(false)
  })

  it('错误路径：没有打开的项目时返回 null', async () => {
    mockProjectStore.currentProject = null

    const store = createStore()
    const result = await store.createConnection({
      name: 'Test',
      driver: 'mysql',
    })

    expect(result).toBeNull()
    expect(store.error).toBe('没有打开的项目')
  })

  it('服务调用失败时设置 error 并抛出', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockCreateProjectConnection.mockRejectedValue(new Error('创建失败'))

    const store = createStore()
    await expect(store.createConnection({ name: 'Fail', driver: 'mysql' })).rejects.toThrow(
      '创建失败'
    )
    expect(store.error).toBe('创建失败')
  })
})

// ==================== 5. updateConnection ====================

describe('updateConnection', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：更新连接并替换本地状态', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockUpdateProjectConnection.mockResolvedValue(undefined)
    const store = createStore()
    const original = makeMockProjectConnection({ id: 'pc-1', name: 'Old Name' })
    store.connections.push(original)

    const updated = { ...original, name: 'Updated Name' }
    await store.updateConnection(updated)

    expect(store.connections[0].name).toBe('Updated Name')
    expect(mockUpdateProjectConnection).toHaveBeenCalledWith(updated, '/test/proj')
  })

  it('更新当前连接时同步 currentConnection', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockUpdateProjectConnection.mockResolvedValue(undefined)
    const store = createStore()
    const original = makeMockProjectConnection({ id: 'pc-1', name: 'Old' })
    store.connections.push(original)
    store.currentConnection = original

    const updated = { ...original, name: 'Updated' }
    await store.updateConnection(updated)

    expect(store.currentConnection?.name).toBe('Updated')
  })

  it('错误路径：没有打开的项目', async () => {
    mockProjectStore.currentProject = null
    const store = createStore()

    await store.updateConnection(makeMockProjectConnection())

    expect(store.error).toBe('没有打开的项目')
  })
})

// ==================== 6. deleteConnection ====================

describe('deleteConnection', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：删除连接并更新本地状态', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockDeleteProjectConnection.mockResolvedValue(undefined)
    const store = createStore()
    store.connections.push(
      makeMockProjectConnection({ id: 'pc-1' }),
      makeMockProjectConnection({ id: 'pc-2' })
    )

    await store.deleteConnection('pc-1')

    expect(store.connections).toHaveLength(1)
    expect(store.connections[0].id).toBe('pc-2')
  })

  it('删除当前连接时清空 currentConnection', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockDeleteProjectConnection.mockResolvedValue(undefined)
    const store = createStore()
    const conn = makeMockProjectConnection({ id: 'pc-1' })
    store.connections.push(conn)
    store.currentConnection = conn

    await store.deleteConnection('pc-1')

    expect(store.currentConnection).toBeNull()
  })

  it('错误路径：没有打开的项目', async () => {
    mockProjectStore.currentProject = null
    const store = createStore()

    await store.deleteConnection('pc-1')

    expect(store.error).toBe('没有打开的项目')
  })
})

// ==================== 7. updateConnectionStatus ====================

describe('updateConnectionStatus', () => {
  beforeEach(() => resetAllMocks())

  it('connected 状态更新本地连接', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockUpdateProjectConnectionStatus.mockResolvedValue(undefined)
    const store = createStore()
    store.connections.push(makeMockProjectConnection({ id: 'pc-1', status: 'disconnected' }))

    await store.updateConnectionStatus('pc-1', 'connected')

    const conn = store.connections[0]
    expect(conn.status).toBe('connected')
    expect(conn.last_connected_at).toBeDefined()
  })

  it('disconnected 状态清除连接状态', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockUpdateProjectConnectionStatus.mockResolvedValue(undefined)
    const store = createStore()
    store.connections.push(makeMockProjectConnection({ id: 'pc-1', status: 'connected' }))

    await store.updateConnectionStatus('pc-1', 'disconnected')

    expect(store.connections[0].status).toBe('disconnected')
  })

  it('同步更新 currentConnection 状态', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockUpdateProjectConnectionStatus.mockResolvedValue(undefined)
    const store = createStore()
    const conn = makeMockProjectConnection({ id: 'pc-1', status: 'disconnected' })
    store.connections.push(conn)
    store.currentConnection = conn

    await store.updateConnectionStatus('pc-1', 'connected')

    expect(store.currentConnection?.status).toBe('connected')
  })
})

// ==================== 8. setCurrentConnection ====================

describe('setCurrentConnection', () => {
  beforeEach(() => resetAllMocks())

  it('设置当前连接', () => {
    const store = createStore()
    const conn = makeMockProjectConnection()

    store.setCurrentConnection(conn)

    expect(store.currentConnection).toEqual(conn)
  })

  it('清除当前连接', () => {
    const store = createStore()
    store.currentConnection = makeMockProjectConnection()

    store.setCurrentConnection(null)

    expect(store.currentConnection).toBeNull()
  })
})

// ==================== 9. searchConnections ====================

describe('searchConnections', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：返回搜索结果', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    const results = [makeMockProjectConnection({ id: 'pc-1', name: 'MySQL Prod' })]
    mockSearchProjectConnections.mockResolvedValue(results)

    const store = createStore()
    const found = await store.searchConnections('MySQL')

    expect(found).toHaveLength(1)
    expect(found[0].name).toBe('MySQL Prod')
    expect(mockSearchProjectConnections).toHaveBeenCalledWith('/test/proj', 'MySQL')
  })

  it('错误路径：没有项目时返回空数组', async () => {
    mockProjectStore.currentProject = null

    const store = createStore()
    const found = await store.searchConnections('test')

    expect(found).toEqual([])
  })
})

// ==================== 10. getConnectionUrl ====================

describe('getConnectionUrl', () => {
  beforeEach(() => resetAllMocks())

  it('委托给 service.buildConnectionUrl', () => {
    mockBuildConnectionUrl.mockReturnValue('mysql://localhost:3306/test')
    const store = createStore()
    const conn = makeMockProjectConnection()

    const url = store.getConnectionUrl(conn)

    expect(url).toBe('mysql://localhost:3306/test')
    expect(mockBuildConnectionUrl).toHaveBeenCalledWith(conn)
  })
})

// ==================== 11. getConnectionDisplayName ====================

describe('getConnectionDisplayName', () => {
  beforeEach(() => resetAllMocks())

  it('委托给 service.getConnectionDisplayName', () => {
    mockGetConnectionDisplayName.mockReturnValue('MySQL - testdb')
    const store = createStore()
    const conn = makeMockProjectConnection()

    const name = store.getConnectionDisplayName(conn)

    expect(name).toBe('MySQL - testdb')
    expect(mockGetConnectionDisplayName).toHaveBeenCalledWith(conn)
  })
})

// ==================== 12. clearError / reset ====================

describe('clearError / reset', () => {
  beforeEach(() => resetAllMocks())

  it('clearError 清除 error 状态', () => {
    const store = createStore()
    store.error = 'some error'

    store.clearError()

    expect(store.error).toBeNull()
  })

  it('reset 重置所有状态到默认值', () => {
    const store = createStore()
    store.connections.push(makeMockProjectConnection())
    store.currentConnection = makeMockProjectConnection()
    store.loading = true
    store.error = 'err'

    store.reset()

    expect(store.connections).toEqual([])
    expect(store.currentConnection).toBeNull()
    expect(store.loading).toBe(false)
    expect(store.error).toBeNull()
  })
})
