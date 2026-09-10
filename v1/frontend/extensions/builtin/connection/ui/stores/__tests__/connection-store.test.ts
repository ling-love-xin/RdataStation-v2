/**
 * connectionStore 连接管理 Store 单元测试
 *
 * 测试所有 state / getters / actions，包括事务管理
 * 依赖 @tauri-apps/api/core、connection service、runtime-connection-store、project store 均 mock
 */
import { setActivePinia, createPinia } from 'pinia'
import { describe, expect, it, vi, beforeEach } from 'vitest'

import type { Connection, ConnectionMeta, ConnectionType } from '@/shared/types'

// ==================== Mock 数据 ====================

const mockConnectionMeta: ConnectionMeta = {
  supportsTransaction: true,
  supportsStreaming: false,
  supportsArrow: false,
  supportsFederated: false,
  supportsConcurrentWrite: false,
  isInMemory: false,
}

function makeMockConnection(overrides: Partial<Connection> = {}): Connection {
  return {
    connId: 'conn-001',
    name: 'Test MySQL',
    dbType: 'mysql',
    url: 'mysql://localhost:3306/test',
    connectionType: 'global' as ConnectionType,
    projectId: null,
    status: 'connected',
    isActive: true,
    meta: { ...mockConnectionMeta },
    ...overrides,
  }
}

const mockConnectionResponse = {
  conn_id: 'conn-new',
  name: 'New Connection',
  db_type: 'postgres',
  url: 'postgresql://localhost:5432/newdb',
  connection_type: 'global',
  project_id: null,
  status: 'connected' as const,
  is_active: true,
  meta: {
    supports_transaction: true,
    supports_streaming: false,
    supports_arrow: false,
    supports_federated: false,
    supports_concurrent_write: false,
    is_in_memory: false,
  },
}

const mockRecentConnectionResponse = [
  {
    id: 'rc-001',
    name: 'Recent MySQL',
    db_type: 'mysql',
    url: 'mysql://localhost:3306/recent',
    connection_type: 'global',
    connected_at: '2026-06-01T00:00:00Z',
  },
]

// ==================== Mock 依赖 ====================

const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}))

// Mock runtimeConnectionStore
const { mockUseRuntimeConnectionStore } = vi.hoisted(() => ({
  mockUseRuntimeConnectionStore: vi.fn(() => ({
    runtimeConnectionIds: new Map<string, string>(),
  })),
}))
vi.mock('../runtime-connection-store', () => ({
  useRuntimeConnectionStore: mockUseRuntimeConnectionStore,
}))

// Mock connection service
const {
  mockConnectDatabase,
  mockSwitchConnection,
  mockCloseConnection,
  mockCloseAllConnections,
  mockGetRecentConnections,
  mockRemoveRecentConnection,
  mockTestConnection,
  mockGetProjectConnections,
  mockGetGlobalConnections,
} = vi.hoisted(() => ({
  mockConnectDatabase: vi.fn(),
  mockSwitchConnection: vi.fn(),
  mockCloseConnection: vi.fn(),
  mockCloseAllConnections: vi.fn(),
  mockGetRecentConnections: vi.fn(),
  mockRemoveRecentConnection: vi.fn(),
  mockTestConnection: vi.fn(),
  mockGetProjectConnections: vi.fn(),
  mockGetGlobalConnections: vi.fn(),
}))

vi.mock('../../services/connection', () => ({
  connectDatabase: (...args: unknown[]) => mockConnectDatabase(...args),
  switchConnection: (...args: unknown[]) => mockSwitchConnection(...args),
  closeConnection: (...args: unknown[]) => mockCloseConnection(...args),
  closeAllConnections: () => mockCloseAllConnections(),
  getRecentConnections: () => mockGetRecentConnections(),
  removeRecentConnection: (name: string) => mockRemoveRecentConnection(name),
  testConnection: (...args: unknown[]) => mockTestConnection(...args),
  getProjectConnections: (...args: unknown[]) => mockGetProjectConnections(...args),
  getGlobalConnections: () => mockGetGlobalConnections(),
}))

// Mock project store
const mockProjectStore = {
  currentProject: null as { path: string; id: string } | null,
}
vi.mock('@/core/project/stores/project', () => ({
  useProjectStore: () => mockProjectStore,
}))

// ==================== Store 导入 ====================

import { useConnectionStore } from '../connection-store'

// ==================== 测试辅助 ====================

function createStore() {
  const pinia = createPinia()
  setActivePinia(pinia)
  return useConnectionStore()
}

function resetAllMocks() {
  vi.clearAllMocks()
  mockProjectStore.currentProject = null
  mockConnectDatabase.mockReset()
  mockSwitchConnection.mockReset()
  mockCloseConnection.mockReset()
  mockCloseAllConnections.mockReset()
  mockGetRecentConnections.mockReset()
  mockRemoveRecentConnection.mockReset()
  mockTestConnection.mockReset()
  mockGetProjectConnections.mockReset()
  mockGetGlobalConnections.mockReset()
  mockInvoke.mockReset()
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

  it('recentConnections 初始为空数组', () => {
    const store = createStore()
    expect(store.recentConnections).toEqual([])
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
    store.connections.push(makeMockConnection({ connId: '1' }))
    expect(store.connectionCount).toBe(1)
    store.connections.push(makeMockConnection({ connId: '2' }))
    expect(store.connectionCount).toBe(2)
  })

  it('hasConnections 有连接时返回 true', () => {
    const store = createStore()
    expect(store.hasConnections).toBe(false)
    store.connections.push(makeMockConnection())
    expect(store.hasConnections).toBe(true)
  })

  it('isConnected 有当前连接时返回 true', () => {
    const store = createStore()
    expect(store.isConnected).toBe(false)
    store.currentConnection = makeMockConnection()
    expect(store.isConnected).toBe(true)
  })
})

// ==================== 3. loadConnections ====================

describe('loadConnections', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：无项目时仅加载全局连接', async () => {
    mockProjectStore.currentProject = null
    mockGetGlobalConnections.mockResolvedValue([
      {
        id: 'g-001',
        name: 'Global MySQL',
        driver: 'mysql',
        host: 'localhost',
        port: 3306,
        database: 'globaldb',
      },
    ])

    const store = createStore()
    await store.loadConnections()

    expect(store.loading).toBe(false)
    expect(store.error).toBeNull()
    expect(store.connections).toHaveLength(1)
    expect(store.connections[0].connId).toBe('g-001')
  })

  it('成功路径：有项目时加载项目连接 + 全局连接合并', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockGetProjectConnections.mockResolvedValue([
      {
        id: 'p-001',
        name: 'Project PG',
        db_type: 'postgres',
        host: '10.0.0.1',
        port: 5432,
        database: 'projdb',
      },
    ])
    mockGetGlobalConnections.mockResolvedValue([
      {
        id: 'g-001',
        name: 'Global MySQL',
        driver: 'mysql',
        host: 'localhost',
        port: 3306,
        database: 'globaldb',
      },
    ])

    const store = createStore()
    await store.loadConnections()

    expect(store.connections).toHaveLength(2)
    const ids = store.connections.map(c => c.connId)
    expect(ids).toContain('p-001')
    expect(ids).toContain('g-001')
  })

  it('错误路径：getProjectConnections 失败时设置 error', async () => {
    mockProjectStore.currentProject = { path: '/test/proj', id: 'proj-001' }
    mockGetProjectConnections.mockRejectedValue(new Error('网络错误'))

    const store = createStore()
    await store.loadConnections()

    expect(store.error).toBe('网络错误')
    expect(store.loading).toBe(false)
  })

  it('错误路径：getGlobalConnections 失败时静默处理（catch 兜底）', async () => {
    mockGetGlobalConnections.mockRejectedValue(new Error('全局连接加载失败'))

    const store = createStore()
    await store.loadConnections()

    // getGlobalConnections 内部有 .catch(() => [])，错误被静默处理
    expect(store.error).toBeNull()
    expect(store.loading).toBe(false)
    expect(store.connections).toEqual([])
  })
})

// ==================== 4. syncConnectionStatus ====================

describe('syncConnectionStatus', () => {
  beforeEach(() => resetAllMocks())

  it('connected=true 设置 currentConnection', () => {
    const store = createStore()
    store.connections.push(
      makeMockConnection({ connId: 'c1', status: 'disconnected', isActive: false })
    )

    store.syncConnectionStatus('c1', true)

    const conn = store.connections.find(c => c.connId === 'c1')
    expect(conn?.status).toBe('connected')
    expect(conn?.isActive).toBe(true)
    expect(store.currentConnection?.connId).toBe('c1')
  })

  it('connected=false 清除 currentConnection', () => {
    const store = createStore()
    const conn = makeMockConnection({ connId: 'c1' })
    store.connections.push(conn)
    store.currentConnection = conn

    store.syncConnectionStatus('c1', false)

    expect(store.connections[0].status).toBe('disconnected')
    expect(store.connections[0].isActive).toBe(false)
    expect(store.currentConnection).toBeNull()
  })
})

// ==================== 5. connect ====================

describe('connect', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：创建新连接并加入列表', async () => {
    mockConnectDatabase.mockResolvedValue(mockConnectionResponse)

    const store = createStore()
    const result = await store.connect(
      'postgres',
      'postgresql://localhost:5432/newdb',
      'New Connection'
    )

    expect(result).toBeDefined()
    expect(result.connId).toBe('conn-new')
    expect(store.connections).toHaveLength(1)
    expect(store.currentConnection?.connId).toBe('conn-new')
    expect(store.loading).toBe(false)
  })

  it('失败路径：设置 error 并抛出', async () => {
    mockConnectDatabase.mockRejectedValue(new Error('连接被拒绝'))

    const store = createStore()
    await expect(store.connect('mysql', 'bad-url', 'Fail')).rejects.toThrow('连接被拒绝')
    expect(store.error).toBe('连接被拒绝')
    expect(store.loading).toBe(false)
  })
})

// ==================== 6. switchConnection ====================

describe('switchConnection', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：切换当前连接', async () => {
    mockSwitchConnection.mockResolvedValue(undefined)
    const store = createStore()
    const conn = makeMockConnection({ connId: 'c2' })
    store.connections.push(makeMockConnection({ connId: 'c1' }), conn)

    await store.switchConnection('c2')

    expect(store.currentConnection?.connId).toBe('c2')
    expect(mockSwitchConnection).toHaveBeenCalledWith('c2')
  })

  it('失败路径：设置 error', async () => {
    mockSwitchConnection.mockRejectedValue(new Error('连接不存在'))

    const store = createStore()
    await expect(store.switchConnection('bad-id')).rejects.toThrow('连接不存在')
    expect(store.error).toBe('连接不存在')
  })
})

// ==================== 7. disconnect ====================

describe('disconnect', () => {
  beforeEach(() => resetAllMocks())

  it('带 connId 移除特定连接', async () => {
    mockCloseConnection.mockResolvedValue(undefined)
    const store = createStore()
    const conn1 = makeMockConnection({ connId: 'c1' })
    const conn2 = makeMockConnection({ connId: 'c2' })
    store.connections.push(conn1, conn2)
    store.currentConnection = conn1

    await store.disconnect('c1')

    expect(store.connections).toHaveLength(1)
    expect(store.connections[0].connId).toBe('c2')
    // currentConnection 回退到剩余连接
    expect(store.currentConnection?.connId).toBe('c2')
  })

  it('无 connId 移除所有连接', async () => {
    mockCloseAllConnections.mockResolvedValue(undefined)
    const store = createStore()
    store.connections.push(
      makeMockConnection({ connId: 'c1' }),
      makeMockConnection({ connId: 'c2' })
    )
    store.currentConnection = makeMockConnection({ connId: 'c1' })

    await store.disconnect()

    expect(store.connections).toHaveLength(0)
    expect(store.currentConnection).toBeNull()
  })
})

// ==================== 8. deleteConnection ====================

describe('deleteConnection', () => {
  beforeEach(() => resetAllMocks())

  it('deleteConnection 是 disconnect 的别名', async () => {
    mockCloseConnection.mockResolvedValue(undefined)
    const store = createStore()
    store.connections.push(makeMockConnection({ connId: 'c1' }))

    await store.deleteConnection('c1')

    expect(store.connections).toHaveLength(0)
    expect(mockCloseConnection).toHaveBeenCalledWith('c1')
  })
})

// ==================== 9. testConnection ====================

describe('testConnection', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：返回 true', async () => {
    mockTestConnection.mockResolvedValue({ success: true })
    const store = createStore()
    store.connections.push(
      makeMockConnection({ connId: 'c1', dbType: 'mysql', url: 'mysql://localhost:3306/test' })
    )

    const result = await store.testConnection('c1')

    expect(result).toBe(true)
    expect(mockTestConnection).toHaveBeenCalledWith('mysql', 'mysql://localhost:3306/test')
  })

  it('连接不存在时抛出错误', async () => {
    const store = createStore()
    await expect(store.testConnection('nonexistent')).rejects.toThrow('连接不存在')
  })
})

// ==================== 10. updateConnection ====================

describe('updateConnection', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：替换旧连接', async () => {
    mockCloseConnection.mockResolvedValue(undefined)
    mockConnectDatabase.mockResolvedValue({
      ...mockConnectionResponse,
      conn_id: 'c1-updated',
      name: 'Updated Name',
    })

    const store = createStore()
    store.connections.push(makeMockConnection({ connId: 'c1', name: 'Old Name' }))
    store.currentConnection = store.connections[0]

    const result = await store.updateConnection(
      'c1',
      'mysql',
      'mysql://localhost:3306/updated',
      'Updated Name'
    )

    expect(result.connId).toBe('c1-updated')
    expect(result.name).toBe('Updated Name')
    expect(store.connections).toHaveLength(1)
    expect(store.connections[0].connId).toBe('c1-updated')
    expect(store.currentConnection?.connId).toBe('c1-updated')
  })

  it('连接不在列表中时追加', async () => {
    mockCloseConnection.mockResolvedValue(undefined)
    mockConnectDatabase.mockResolvedValue(mockConnectionResponse)

    const store = createStore()
    await store.updateConnection('missing', 'postgres', 'postgresql://localhost:5432/new', 'New')

    expect(store.connections).toHaveLength(1)
    expect(store.connections[0].connId).toBe('conn-new')
  })
})

// ==================== 11. loadRecentConnections ====================

describe('loadRecentConnections', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：加载最近连接列表', async () => {
    mockGetRecentConnections.mockResolvedValue(mockRecentConnectionResponse)

    const store = createStore()
    await store.loadRecentConnections()

    expect(store.recentConnections).toHaveLength(1)
    expect(store.recentConnections[0].id).toBe('rc-001')
  })

  it('失败路径：静默忽略错误', async () => {
    mockGetRecentConnections.mockRejectedValue(new Error('加载失败'))

    const store = createStore()
    await store.loadRecentConnections()

    expect(store.recentConnections).toHaveLength(0)
  })
})

// ==================== 12. removeRecentConnection ====================

describe('removeRecentConnection', () => {
  beforeEach(() => resetAllMocks())

  it('从列表中移除最近连接', async () => {
    mockRemoveRecentConnection.mockResolvedValue(undefined)
    const store = createStore()
    store.recentConnections.push(
      {
        id: 'rc-1',
        name: 'Conn A',
        dbType: 'mysql',
        url: 'url1',
        connectionType: 'global',
        connectedAt: '',
      },
      {
        id: 'rc-2',
        name: 'Conn B',
        dbType: 'postgres',
        url: 'url2',
        connectionType: 'global',
        connectedAt: '',
      }
    )

    await store.removeRecentConnection('Conn A')

    expect(store.recentConnections).toHaveLength(1)
    expect(store.recentConnections[0].name).toBe('Conn B')
  })
})

// ==================== 13. clearError ====================

describe('clearError', () => {
  beforeEach(() => resetAllMocks())

  it('清除 error 状态', () => {
    const store = createStore()
    store.error = 'some error'

    store.clearError()

    expect(store.error).toBeNull()
  })
})

// ==================== 14. reset ====================

describe('reset', () => {
  beforeEach(() => resetAllMocks())

  it('重置所有状态到默认值', () => {
    const store = createStore()
    store.connections.push(makeMockConnection())
    store.currentConnection = makeMockConnection()
    store.recentConnections.push({
      id: 'rc',
      name: 'R',
      dbType: 'mysql',
      url: 'u',
      connectionType: 'global',
      connectedAt: '',
    })
    store.loading = true
    store.error = 'err'

    store.reset()

    expect(store.connections).toEqual([])
    expect(store.currentConnection).toBeNull()
    expect(store.recentConnections).toEqual([])
    expect(store.loading).toBe(false)
    expect(store.error).toBeNull()
  })
})

// ==================== 15. beginTransaction ====================

describe('beginTransaction', () => {
  beforeEach(() => resetAllMocks())

  it('使用传入 connId 调用 invoke', async () => {
    mockInvoke.mockResolvedValue({ success: true })
    const store = createStore()

    const result = await store.beginTransaction('conn-001')

    expect(result).toEqual({ success: true })
    expect(mockInvoke).toHaveBeenCalledWith('begin_transaction', { connId: 'conn-001' })
  })

  it('使用 currentConnection 的 connId', async () => {
    mockInvoke.mockResolvedValue({ success: true })
    const store = createStore()
    store.currentConnection = makeMockConnection({ connId: 'current-conn' })

    await store.beginTransaction()

    expect(mockInvoke).toHaveBeenCalledWith('begin_transaction', { connId: 'current-conn' })
  })

  it('无活动连接时抛出错误', async () => {
    const store = createStore()
    await expect(store.beginTransaction()).rejects.toThrow('没有活动的连接')
  })
})

// ==================== 16. commitTransaction ====================

describe('commitTransaction', () => {
  beforeEach(() => resetAllMocks())

  it('使用传入 connId 调用 invoke', async () => {
    mockInvoke.mockResolvedValue({ success: true })
    const store = createStore()

    await store.commitTransaction('conn-001')

    expect(mockInvoke).toHaveBeenCalledWith('commit_transaction', { connId: 'conn-001' })
  })
})

// ==================== 17. rollbackTransaction ====================

describe('rollbackTransaction', () => {
  beforeEach(() => resetAllMocks())

  it('使用传入 connId 调用 invoke', async () => {
    mockInvoke.mockResolvedValue({ success: true })
    const store = createStore()

    await store.rollbackTransaction('conn-001')

    expect(mockInvoke).toHaveBeenCalledWith('rollback_transaction', { connId: 'conn-001' })
  })
})

// ==================== 18. getTransactionStatus ====================

describe('getTransactionStatus', () => {
  beforeEach(() => resetAllMocks())

  it('使用传入 connId 调用 invoke', async () => {
    mockInvoke.mockResolvedValue({ success: true, message: 'active' })
    const store = createStore()

    const result = await store.getTransactionStatus('conn-001')

    expect(result).toEqual({ success: true, message: 'active' })
    expect(mockInvoke).toHaveBeenCalledWith('get_transaction_status', { connId: 'conn-001' })
  })

  it('无活动连接时抛出错误', async () => {
    const store = createStore()
    await expect(store.getTransactionStatus()).rejects.toThrow('没有活动的连接')
  })
})
