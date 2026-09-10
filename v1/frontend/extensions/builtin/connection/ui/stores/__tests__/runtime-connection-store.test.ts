/**
 * runtimeConnectionStore 运行时连接管理 Store 单元测试
 *
 * 测试所有 state / getters / actions，包括 DuckDB 偏好管理
 * 依赖 connection service、localStorage 均 mock
 *
 * buildConnectionUrl 是 store 私有函数，提取为纯函数独立测试
 */
import { setActivePinia, createPinia } from 'pinia'
import { describe, expect, it, vi, beforeEach } from 'vitest'

import type { Connection, ConnectionMeta, ConnectionType } from '@/shared/types'

// ==================== 纯函数：buildConnectionUrl ====================

/**
 * 从 runtime-connection-store.ts 中提取的 buildConnectionUrl 纯函数
 * 用于独立测试 URL 构建逻辑
 */
function buildConnectionUrl(projectConn: ProjectConnection): string {
  const dbType = projectConn.driver
  if (!dbType) {
    throw new Error('数据库类型未定义')
  }

  const { host, port, database, username, password } = projectConn

  switch (dbType.toLowerCase()) {
    case 'mysql':
      if (username && password) {
        return `mysql://${username}:${password}@${host}:${port}/${database}`
      }
      return `mysql://${host}:${port}/${database}`

    case 'postgresql':
    case 'postgres':
      if (username && password) {
        return `postgresql://${username}:${password}@${host}:${port}/${database}`
      }
      return `postgresql://${host}:${port}/${database}`

    case 'sqlite': {
      const sqlitePath = host || database || ''
      return `sqlite://${sqlitePath}`
    }

    case 'duckdb': {
      const duckdbPath = host || database || ''
      return `duckdb://${duckdbPath}`
    }

    default:
      throw new Error(`不支持的数据库类型: ${dbType}`)
  }
}

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
    url: 'mysql://admin:secret@localhost:3306/testdb',
    connectionType: 'global' as ConnectionType,
    projectId: null,
    status: 'connected',
    isActive: true,
    meta: { ...mockConnectionMeta },
    ...overrides,
  }
}

const mockConnectionResponse = {
  conn_id: 'runtime-conn-001',
  name: 'Test MySQL',
  db_type: 'mysql',
  url: 'mysql://admin:secret@localhost:3306/testdb',
  connection_type: 'project',
  project_id: '/test/proj',
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

// ==================== Mock localStorage ====================

const localStorageMock = (() => {
  let store: Record<string, string> = {}
  return {
    getItem: vi.fn((key: string) => store[key] ?? null),
    setItem: vi.fn((key: string, value: string) => {
      store[key] = value
    }),
    removeItem: vi.fn((key: string) => {
      delete store[key]
    }),
    clear: vi.fn(() => {
      store = {}
    }),
  }
})()

Object.defineProperty(globalThis, 'localStorage', {
  value: localStorageMock,
  writable: true,
})

// ==================== Mock 依赖 ====================

// Mock @tauri-apps/api/core (used by the real service)
const { mockInvoke } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: mockInvoke,
}))

// Mock connection service
const mockConnectDatabase = vi.fn()
const mockCloseConnection = vi.fn()

vi.mock('../../services/connection', () => ({
  connectDatabase: (...args: unknown[]) => mockConnectDatabase(...args),
  closeConnection: (...args: unknown[]) => mockCloseConnection(...args),
}))

// ==================== Store 导入 ====================

import { useRuntimeConnectionStore } from '../runtime-connection-store'

import type { ProjectConnection } from '../../../types/connection'

// ==================== 测试辅助 ====================

function createStore() {
  const pinia = createPinia()
  setActivePinia(pinia)
  return useRuntimeConnectionStore()
}

function resetAllMocks() {
  vi.clearAllMocks()
  localStorageMock.clear()
  mockInvoke.mockReset()
  mockConnectDatabase.mockReset()
  mockCloseConnection.mockReset()
}

// ==================== 1. Store 初始化 ====================

describe('Store 初始化 - 默认状态', () => {
  beforeEach(() => resetAllMocks())

  it('runtimeConnectionIds 初始为空 Map', () => {
    const store = createStore()
    expect(store.runtimeConnectionIds.size).toBe(0)
  })

  it('currentRuntimeConnId 初始为 null', () => {
    const store = createStore()
    expect(store.currentRuntimeConnId).toBeNull()
  })

  it('duckdbEnabled 初始为空 Map', () => {
    const store = createStore()
    expect(store.duckdbEnabled.size).toBe(0)
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

  it('getRuntimeConnId 返回项目连接对应的运行时连接 ID', () => {
    const store = createStore()
    const newMap = new Map(store.runtimeConnectionIds)
    newMap.set('pc-001', 'runtime-abc')
    store.runtimeConnectionIds = newMap

    const getter = store.getRuntimeConnId
    expect(getter('pc-001')).toBe('runtime-abc')
    expect(getter('pc-nonexistent')).toBeUndefined()
  })

  it('hasRuntimeConnection 检查是否有运行时连接', () => {
    const store = createStore()
    const newMap = new Map(store.runtimeConnectionIds)
    newMap.set('pc-001', 'runtime-abc')
    store.runtimeConnectionIds = newMap

    const checker = store.hasRuntimeConnection
    expect(checker('pc-001')).toBe(true)
    expect(checker('pc-nonexistent')).toBe(false)
  })
})

// ==================== 3. establishRuntimeConnection ====================

describe('establishRuntimeConnection', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：建立新运行时连接', async () => {
    mockConnectDatabase.mockResolvedValue(mockConnectionResponse)
    const store = createStore()
    const projConn = makeMockProjectConnection()

    const connId = await store.establishRuntimeConnection(projConn)

    expect(connId).toBe('runtime-conn-001')
    expect(store.currentRuntimeConnId).toBe('runtime-conn-001')
    expect(store.runtimeConnectionIds.get('pc-001')).toBe('runtime-conn-001')
    expect(store.loading).toBe(false)
  })

  it('已连接路径：返回现有运行时连接 ID', async () => {
    const store = createStore()
    const newMap = new Map(store.runtimeConnectionIds)
    newMap.set('pc-001', 'existing-runtime')
    store.runtimeConnectionIds = newMap

    const projConn = makeMockProjectConnection()
    const connId = await store.establishRuntimeConnection(projConn)

    expect(connId).toBe('existing-runtime')
    expect(store.currentRuntimeConnId).toBe('existing-runtime')
    expect(mockConnectDatabase).not.toHaveBeenCalled()
  })

  it('错误路径：返回 null 并设置 error', async () => {
    mockConnectDatabase.mockRejectedValue(new Error('连接被拒绝'))
    const store = createStore()
    const projConn = makeMockProjectConnection()

    const connId = await store.establishRuntimeConnection(projConn)

    expect(connId).toBeNull()
    expect(store.error).toBe('连接被拒绝')
    expect(store.loading).toBe(false)
  })
})

// ==================== 4. closeRuntimeConnection ====================

describe('closeRuntimeConnection', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：关闭运行时连接', async () => {
    mockCloseConnection.mockResolvedValue(undefined)
    const store = createStore()
    const newMap = new Map(store.runtimeConnectionIds)
    newMap.set('pc-001', 'runtime-abc')
    store.runtimeConnectionIds = newMap
    store.currentRuntimeConnId = 'runtime-abc'

    await store.closeRuntimeConnection('pc-001')

    expect(store.runtimeConnectionIds.has('pc-001')).toBe(false)
    expect(store.currentRuntimeConnId).toBeNull()
    expect(mockCloseConnection).toHaveBeenCalledWith('runtime-abc')
  })

  it('无运行时连接时直接返回', async () => {
    const store = createStore()

    await store.closeRuntimeConnection('nonexistent')

    expect(mockCloseConnection).not.toHaveBeenCalled()
  })
})

// ==================== 5. switchToConnection ====================

describe('switchToConnection', () => {
  beforeEach(() => resetAllMocks())

  it('委托给 establishRuntimeConnection', async () => {
    mockConnectDatabase.mockResolvedValue(mockConnectionResponse)
    const store = createStore()
    const projConn = makeMockProjectConnection()

    const connId = await store.switchToConnection(projConn)

    expect(connId).toBe('runtime-conn-001')
    expect(store.runtimeConnectionIds.get('pc-001')).toBe('runtime-conn-001')
  })
})

// ==================== 6. getCurrentRuntimeConnId ====================

describe('getCurrentRuntimeConnId', () => {
  beforeEach(() => resetAllMocks())

  it('返回当前运行时连接 ID', () => {
    const store = createStore()
    store.currentRuntimeConnId = 'runtime-abc'

    expect(store.getCurrentRuntimeConnId()).toBe('runtime-abc')
  })

  it('无当前连接时返回 null', () => {
    const store = createStore()
    expect(store.getCurrentRuntimeConnId()).toBeNull()
  })
})

// ==================== 7. clearAllRuntimeConnections ====================

describe('clearAllRuntimeConnections', () => {
  beforeEach(() => resetAllMocks())

  it('关闭所有运行时连接', async () => {
    mockCloseConnection.mockResolvedValue(undefined)
    const store = createStore()
    const newMap = new Map<string, string>()
    newMap.set('pc-001', 'rt-1')
    newMap.set('pc-002', 'rt-2')
    store.runtimeConnectionIds = newMap
    store.currentRuntimeConnId = 'rt-1'

    await store.clearAllRuntimeConnections()

    expect(store.runtimeConnectionIds.size).toBe(0)
    expect(store.currentRuntimeConnId).toBeNull()
    expect(mockCloseConnection).toHaveBeenCalledTimes(2)
  })
})

// ==================== 8. establishFromConnection ====================

describe('establishFromConnection', () => {
  beforeEach(() => resetAllMocks())

  it('成功路径：从 Connection 建立运行时连接', async () => {
    mockConnectDatabase.mockResolvedValue(mockConnectionResponse)
    const store = createStore()
    const conn = makeMockConnection()

    const connId = await store.establishFromConnection(conn)

    expect(connId).toBe('runtime-conn-001')
    expect(store.runtimeConnectionIds.get('conn-001')).toBe('runtime-conn-001')
    expect(store.currentRuntimeConnId).toBe('runtime-conn-001')
  })

  it('已连接路径：返回现有运行时连接 ID', async () => {
    const store = createStore()
    const newMap = new Map(store.runtimeConnectionIds)
    newMap.set('conn-001', 'existing-runtime')
    store.runtimeConnectionIds = newMap

    const conn = makeMockConnection()
    const connId = await store.establishFromConnection(conn)

    expect(connId).toBe('existing-runtime')
    expect(mockConnectDatabase).not.toHaveBeenCalled()
  })
})

// ==================== 9. DuckDB 偏好管理 ====================

describe('DuckDB 偏好管理', () => {
  beforeEach(() => {
    resetAllMocks()
  })

  it('isDuckDbEnabled 未加载偏好时从 localStorage 加载', () => {
    localStorageMock.getItem.mockReturnValue(JSON.stringify({ 'conn-001': true }))
    const store = createStore()

    const enabled = store.isDuckDbEnabled('conn-001')

    expect(enabled).toBe(true)
    expect(localStorageMock.getItem).toHaveBeenCalledWith('duckdb-enabled-connections')
  })

  it('isDuckDbEnabled 无记录时返回 false', () => {
    (localStorageMock.getItem as any).mockReturnValue(null)
    const store = createStore()

    const enabled = store.isDuckDbEnabled('conn-001')

    expect(enabled).toBe(false)
  })

  it('toggleDuckDbEnabled 切换开关状态', () => {
    (localStorageMock.getItem as any).mockReturnValue(null)
    const store = createStore()

    const newVal = store.toggleDuckDbEnabled('conn-001')

    expect(newVal).toBe(true)
    expect(store.duckdbEnabled.get('conn-001')).toBe(true)
    expect(localStorageMock.setItem).toHaveBeenCalled()
  })

  it('toggleDuckDbEnabled 再次调用切换回 false', () => {
    localStorageMock.getItem.mockReturnValue(JSON.stringify({ 'conn-001': true }))
    const store = createStore()

    const newVal = store.toggleDuckDbEnabled('conn-001')

    expect(newVal).toBe(false)
    expect(store.duckdbEnabled.get('conn-001')).toBe(false)
  })

  it('loadDuckDbPrefs 从 localStorage 加载偏好', () => {
    localStorageMock.getItem.mockReturnValue(
      JSON.stringify({ 'conn-001': true, 'conn-002': false })
    )

    const store = createStore()
    store.loadDuckDbPrefs()

    expect(store.duckdbEnabled.get('conn-001')).toBe(true)
    expect(store.duckdbEnabled.get('conn-002')).toBe(false)
  })

  it('loadDuckDbPrefs 无存储时保持空 Map', () => {
    (localStorageMock.getItem as any).mockReturnValue(null)

    const store = createStore()
    store.loadDuckDbPrefs()

    expect(store.duckdbEnabled.size).toBe(0)
  })
})

// ==================== 10. buildConnectionUrl 纯函数 ====================

describe('buildConnectionUrl', () => {
  it('MySQL 带认证信息', () => {
    const conn = makeMockProjectConnection({
      driver: 'mysql',
      host: 'db.example.com',
      port: 3306,
      database: 'mydb',
      username: 'user',
      password: 'pass',
    })

    const url = buildConnectionUrl(conn)

    expect(url).toBe('mysql://user:pass@db.example.com:3306/mydb')
  })

  it('MySQL 无认证信息', () => {
    const conn = makeMockProjectConnection({
      driver: 'mysql',
      host: 'localhost',
      port: 3306,
      database: 'mydb',
      username: undefined,
      password: undefined,
    })

    const url = buildConnectionUrl(conn)

    expect(url).toBe('mysql://localhost:3306/mydb')
  })

  it('PostgreSQL 带认证信息', () => {
    const conn = makeMockProjectConnection({
      driver: 'postgresql',
      host: 'pg.example.com',
      port: 5432,
      database: 'pgdb',
      username: 'pguser',
      password: 'pgpass',
    })

    const url = buildConnectionUrl(conn)

    expect(url).toBe('postgresql://pguser:pgpass@pg.example.com:5432/pgdb')
  })

  it('PostgreSQL (postgres 别名)', () => {
    const conn = makeMockProjectConnection({
      driver: 'postgres',
      host: 'localhost',
      port: 5432,
      database: 'pgdb',
      username: 'u',
      password: 'p',
    })

    const url = buildConnectionUrl(conn)

    expect(url).toBe('postgresql://u:p@localhost:5432/pgdb')
  })

  it('SQLite 使用 host 作为文件路径', () => {
    const conn = makeMockProjectConnection({
      driver: 'sqlite',
      host: '/path/to/db.sqlite',
    })

    const url = buildConnectionUrl(conn)

    expect(url).toBe('sqlite:///path/to/db.sqlite')
  })

  it('DuckDB 使用 host 作为文件路径', () => {
    const conn = makeMockProjectConnection({
      driver: 'duckdb',
      host: '/path/to/data.duckdb',
    })

    const url = buildConnectionUrl(conn)

    expect(url).toBe('duckdb:///path/to/data.duckdb')
  })

  it('不支持的数据库类型抛出错误', () => {
    const conn = makeMockProjectConnection({
      driver: 'oracle',
    })

    expect(() => buildConnectionUrl(conn)).toThrow('不支持的数据库类型: oracle')
  })

  it('driver 为 undefined 时抛出错误', () => {
    const conn = makeMockProjectConnection({
      driver: undefined as unknown as string,
    })

    expect(() => buildConnectionUrl(conn)).toThrow('数据库类型未定义')
  })
})
