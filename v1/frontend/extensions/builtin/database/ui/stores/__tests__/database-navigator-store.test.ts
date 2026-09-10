// @ts-nocheck -- 测试文件，mock 类型不需要完整实现
/**
 * 测试 useDatabaseNavigatorStore 的核心功能
 *
 * 测试核心 state / getters / actions，包括树读取、连接信息管理和加载委派
 * 所有外部依赖（loader、API、service）均 mock
 */
import { setActivePinia, createPinia } from 'pinia'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ref } from 'vue'

import { useDatabaseNavigatorStore } from '../database-navigator-store'

import type { CatalogNode } from '../types/nav-types'

// ==================== Mock 函数（vi.hoisted 确保在 vi.mock 工厂可用） ====================

const {
  mockLoadCatalogs,
  mockLoadCatalogsFromCache,
  mockLoadCatalogsFromCacheSilent,
  mockLoadSchemas,
  mockLoadTables,
  mockLoadProcedures,
  mockLoadFunctions,
  mockLoadSequences,
  mockLoadTriggers,
  mockLoadColumns,
  mockLoadIndexes,
  mockLoadConstraints,
  mockSetIntrospectionLevel,
  mockCloseConnection,
  mockExecuteSql,
  mockClearMetadataCache,
  mockNodeKeyDecode,
} = vi.hoisted(() => ({
  mockLoadCatalogs: vi.fn(),
  mockLoadCatalogsFromCache: vi.fn(),
  mockLoadCatalogsFromCacheSilent: vi.fn(),
  mockLoadSchemas: vi.fn(),
  mockLoadTables: vi.fn(),
  mockLoadProcedures: vi.fn(),
  mockLoadFunctions: vi.fn(),
  mockLoadSequences: vi.fn(),
  mockLoadTriggers: vi.fn(),
  mockLoadColumns: vi.fn(),
  mockLoadIndexes: vi.fn(),
  mockLoadConstraints: vi.fn(),
  mockSetIntrospectionLevel: vi.fn(),
  mockCloseConnection: vi.fn(),
  mockExecuteSql: vi.fn(),
  mockClearMetadataCache: vi.fn(),
  mockNodeKeyDecode: vi.fn(() => []),
}))

// ==================== Mock 模块 ====================

vi.mock('../nav-loaders/use-catalog-loader', () => ({
  useCatalogLoader: () => ({
    loadCatalogs: mockLoadCatalogs,
    loadCatalogsFromCache: mockLoadCatalogsFromCache,
    loadCatalogsFromCacheSilent: mockLoadCatalogsFromCacheSilent,
    loadSchemas: mockLoadSchemas,
  }),
}))

vi.mock('../nav-loaders/use-table-loader', () => ({
  useTableLoader: () => ({
    loadTables: mockLoadTables,
    computeSchemaStats: vi.fn(),
  }),
}))

vi.mock('../nav-loaders/use-object-loader', () => ({
  useObjectLoader: () => ({
    loadProcedures: mockLoadProcedures,
    loadFunctions: mockLoadFunctions,
    loadSequences: mockLoadSequences,
    loadTriggers: mockLoadTriggers,
    loadingProcedures: ref(new Set<string>()),
    loadingFunctions: ref(new Set<string>()),
    loadingSequences: ref(new Set<string>()),
    loadingTriggers: ref(new Set<string>()),
  }),
}))

vi.mock('../nav-loaders/use-column-loader', () => ({
  useColumnLoader: () => ({
    loadColumns: mockLoadColumns,
    loadIndexes: mockLoadIndexes,
    loadConstraints: mockLoadConstraints,
  }),
}))

vi.mock('../../api/database-api', () => ({
  setIntrospectionLevel: (...args: unknown[]) => mockSetIntrospectionLevel(...args),
}))

vi.mock('@/extensions/builtin/connection/ui/services/connection', () => ({
  closeConnection: (...args: unknown[]) => mockCloseConnection(...args),
  executeSql: (...args: unknown[]) => mockExecuteSql(...args),
}))

vi.mock('../../services/metadata-cache-service', () => ({
  clearMetadataCache: (...args: unknown[]) => mockClearMetadataCache(...args),
}))

vi.mock('../../types/virtual-tree', () => ({
  NodeKeyEncoder: {
    decode: (...args: unknown[]) => mockNodeKeyDecode(...args),
  },
}))

// ==================== Store 导入 ====================

// ==================== 测试辅助 ====================

function createStore() {
  const pinia = createPinia()
  setActivePinia(pinia)
  return useDatabaseNavigatorStore()
}

function resetAllMocks() {
  vi.clearAllMocks()
  mockNodeKeyDecode.mockReturnValue([])
}

/**
 * 创建测试用的 Catalog 树结构
 */
function makeTestCatalogs(): CatalogNode[] {
  return [
    {
      name: 'my_catalog',
      schemas: [
        {
          name: 'public',
          tables: [
            { name: 'users', type: 'table', columns: [] },
            { name: 'orders', type: 'table', columns: [] },
          ],
          views: [{ name: 'user_summary', type: 'view', columns: [] }],
        },
        {
          name: 'analytics',
          tables: [{ name: 'events', type: 'table', columns: [] }],
          views: [],
        },
      ],
    },
  ]
}

/**
 * 将测试数据注入 store 的 connectionCatalogs
 */
function seedCatalogs(
  store: ReturnType<typeof createStore>,
  connectionId: string,
  catalogs: CatalogNode[]
) {
  const newMap = new Map(store.connectionCatalogs)
  newMap.set(connectionId, catalogs)
  store.connectionCatalogs = newMap
}

// ==================== 1. 连接信息管理（registerConnection / getProjectPath / setProjectPath） ====================

describe('连接信息管理', () => {
  beforeEach(() => resetAllMocks())

  it('setConnectionInfo 注册连接信息（类型、项目路径、数据库类型）', () => {
    const store = createStore()

    store.setConnectionInfo('conn-1', 'project', '/projects/my-proj', 'postgresql')

    expect(store.getConnectionType('conn-1')).toBe('project')
    expect(store.getProjectPath('conn-1')).toBe('/projects/my-proj')
    expect(store.getDbType('conn-1')).toBe('postgresql')
  })

  it('getProjectPath 返回项目路径', () => {
    const store = createStore()
    store.setConnectionInfo('conn-1', 'project', '/projects/my-proj')

    expect(store.getProjectPath('conn-1')).toBe('/projects/my-proj')
  })

  it('getProjectPath 未注册时返回 undefined', () => {
    const store = createStore()

    expect(store.getProjectPath('nonexistent')).toBeUndefined()
  })

  it('setConnectionInfo 可仅设置项目路径', () => {
    const store = createStore()
    store.setConnectionInfo('conn-1', 'project', '/new-path')

    expect(store.getConnectionType('conn-1')).toBe('project')
    expect(store.getProjectPath('conn-1')).toBe('/new-path')
  })

  it('getConnectionType 未注册时返回 undefined', () => {
    const store = createStore()

    expect(store.getConnectionType('nonexistent')).toBeUndefined()
  })

  it('getDbType 未注册时返回空字符串', () => {
    const store = createStore()

    expect(store.getDbType('nonexistent')).toBe('')
  })
})

// ==================== 2. getCatalogs 获取 catalog 列表 ====================

describe('getCatalogs', () => {
  beforeEach(() => resetAllMocks())

  it('返回已注册连接的 catalog 列表', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    const result = store.getCatalogs('conn-1')

    expect(result).toHaveLength(1)
    expect(result[0].name).toBe('my_catalog')
  })

  it('未注册连接返回空数组', () => {
    const store = createStore()

    const result = store.getCatalogs('nonexistent')

    expect(result).toEqual([])
  })
})

// ==================== 3. loadCatalogs 加载 catalogs ====================

describe('loadCatalogs', () => {
  beforeEach(() => resetAllMocks())

  it('委派到 catalogLoader.loadCatalogs', async () => {
    mockLoadCatalogs.mockResolvedValue(undefined)
    const store = createStore()

    await store.loadCatalogs('conn-1')

    expect(mockLoadCatalogs).toHaveBeenCalledWith('conn-1')
  })

  it('loadCatalogsFromCacheSilent 委派到 catalogLoader', async () => {
    mockLoadCatalogsFromCacheSilent.mockResolvedValue(true)
    const store = createStore()

    const result = await store.loadCatalogsFromCacheSilent('conn-1')

    expect(result).toBe(true)
    expect(mockLoadCatalogsFromCacheSilent).toHaveBeenCalledWith('conn-1')
  })
})

// ==================== 4. loadSchemas 加载 schemas ====================

describe('loadSchemas', () => {
  beforeEach(() => resetAllMocks())

  it('委派到 catalogLoader.loadSchemas', async () => {
    mockLoadSchemas.mockResolvedValue(undefined)
    const store = createStore()

    await store.loadSchemas('conn-1', 'my_catalog')

    expect(mockLoadSchemas).toHaveBeenCalledWith('conn-1', 'my_catalog')
  })
})

// ==================== 5. loadTables 加载 tables ====================

describe('loadTables', () => {
  beforeEach(() => resetAllMocks())

  it('委派到 tableLoader.loadTables', async () => {
    mockLoadTables.mockResolvedValue(undefined)
    const store = createStore()

    await store.loadTables('conn-1', 'my_catalog', 'public')

    expect(mockLoadTables).toHaveBeenCalledWith('conn-1', 'my_catalog', 'public')
  })
})

// ==================== 6. getSchemaTables 获取 schema 表列表 ====================

describe('getSchemaTables', () => {
  beforeEach(() => resetAllMocks())

  it('返回指定 schema 下的表列表', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    const tables = store.getSchemaTables('conn-1', 'my_catalog', 'public')

    expect(tables).toHaveLength(2)
    expect(tables[0].name).toBe('users')
    expect(tables[1].name).toBe('orders')
  })

  it('返回指定 schema 下的表列表（单表）', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    const tables = store.getSchemaTables('conn-1', 'my_catalog', 'analytics')

    expect(tables).toHaveLength(1)
    expect(tables[0].name).toBe('events')
  })

  it('未注册连接返回空数组', () => {
    const store = createStore()

    const tables = store.getSchemaTables('nonexistent', 'cat', 'sch')

    expect(tables).toEqual([])
  })

  it('不存在的 catalog 返回空数组', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    const tables = store.getSchemaTables('conn-1', 'nonexistent', 'public')

    expect(tables).toEqual([])
  })
})

// ==================== 7. getSchemaViews 获取 schema 视图列表 ====================

describe('getSchemaViews', () => {
  beforeEach(() => resetAllMocks())

  it('返回指定 schema 下的视图列表', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    const views = store.getSchemaViews('conn-1', 'my_catalog', 'public')

    expect(views).toHaveLength(1)
    expect(views[0].name).toBe('user_summary')
  })

  it('无视图的 schema 返回空数组', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    const views = store.getSchemaViews('conn-1', 'my_catalog', 'analytics')

    expect(views).toEqual([])
  })

  it('未注册连接返回空数组', () => {
    const store = createStore()

    const views = store.getSchemaViews('nonexistent', 'cat', 'sch')

    expect(views).toEqual([])
  })
})

// ==================== 8. clearCache 清除连接数据 ====================

describe('clearCache', () => {
  beforeEach(() => resetAllMocks())

  it('清除指定连接的 catalog 数据', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())
    seedCatalogs(store, 'conn-2', makeTestCatalogs())

    store.clearCache('conn-1')

    expect(store.getCatalogs('conn-1')).toEqual([])
    expect(store.getCatalogs('conn-2')).toHaveLength(1)
  })

  it('不传 connectionId 时清除所有数据', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())
    seedCatalogs(store, 'conn-2', makeTestCatalogs())

    store.clearCache()

    expect(store.getCatalogs('conn-1')).toEqual([])
    expect(store.getCatalogs('conn-2')).toEqual([])
  })
})

// ==================== 9. disconnectConnection 关闭连接并清除缓存 ====================

describe('disconnectConnection', () => {
  beforeEach(() => resetAllMocks())

  it('关闭连接并清除缓存', async () => {
    mockCloseConnection.mockResolvedValue(undefined)
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    await store.disconnectConnection('conn-1')

    expect(mockCloseConnection).toHaveBeenCalledWith('conn-1')
    expect(store.getCatalogs('conn-1')).toEqual([])
  })

  it('closeConnection 失败时仍然清除缓存', async () => {
    mockCloseConnection.mockRejectedValue(new Error('连接已失效'))
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    await store.disconnectConnection('conn-1')

    expect(store.getCatalogs('conn-1')).toEqual([])
  })
})

// ==================== 10. 选中对象与搜索 ====================

describe('setSelectedObject / selectNode', () => {
  beforeEach(() => resetAllMocks())

  it('setSelectedObject 设置选中对象', () => {
    const store = createStore()

    store.setSelectedObject({
      name: 'users',
      kind: 'table',
      connectionId: 'conn-1',
    })

    expect(store.selectedObject).toEqual({
      name: 'users',
      kind: 'table',
      connectionId: 'conn-1',
    })
  })

  it('setSelectedObject 设为 null 清除选中', () => {
    const store = createStore()
    store.setSelectedObject({
      name: 'users',
      kind: 'table',
      connectionId: 'conn-1',
    })

    store.setSelectedObject(null)

    expect(store.selectedObject).toBeNull()
  })

  it('selectNode 解码 nodeKey 并设置选中对象', () => {
    mockNodeKeyDecode.mockReturnValue(['table', 'conn-1', 'my_catalog', 'public', 'users'])
    const store = createStore()

    store.selectNode('table:conn-1:my_catalog:public:users')

    expect(store.selectedObject).toMatchObject({
      name: 'users',
      kind: 'table',
      connectionId: 'conn-1',
    })
  })

  it('selectNode 不足 2 段时直接返回不设置', () => {
    mockNodeKeyDecode.mockReturnValue(['conn-1'])
    const store = createStore()

    store.selectNode('conn-1')

    expect(store.selectedObject).toBeNull()
  })
})

// ==================== 11. searchObjects 搜索 ====================

describe('searchObjects', () => {
  beforeEach(() => resetAllMocks())

  it('空查询返回空数组', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    const results = store.searchObjects('')

    expect(results).toEqual([])
  })

  it('按表名搜索返回匹配结果', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    const results = store.searchObjects('users')

    expect(results).toHaveLength(1)
    expect(results[0]).toMatchObject({
      type: 'table',
      name: 'users',
      connectionId: 'conn-1',
    })
  })

  it('不指定 connectionId 搜索全部连接', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())
    const catalogs2: CatalogNode[] = [
      {
        name: 'other_db',
        schemas: [
          { name: 'main', tables: [{ name: 'users', type: 'table', columns: [] }], views: [] },
        ],
      },
    ]
    seedCatalogs(store, 'conn-2', catalogs2)

    const results = store.searchObjects('users')

    expect(results).toHaveLength(2)
  })

  it('不匹配时返回空数组', () => {
    const store = createStore()
    seedCatalogs(store, 'conn-1', makeTestCatalogs())

    const results = store.searchObjects('nonexistent_table')

    expect(results).toEqual([])
  })
})

// ==================== 12. 错误管理 ====================

describe('错误管理', () => {
  beforeEach(() => resetAllMocks())

  it('clearError 清除错误', () => {
    const store = createStore()
    store.error = 'something went wrong'

    store.clearError()

    expect(store.error).toBeNull()
  })

  it('setNodeError / getNodeError 设置和读取节点错误', () => {
    const store = createStore()

    store.setNodeError('conn-1:my_catalog', '加载失败')

    expect(store.getNodeError('conn-1:my_catalog')).toBe('加载失败')
  })

  it('clearNodeError 清除单个节点错误', () => {
    const store = createStore()
    store.setNodeError('conn-1:my_catalog', '加载失败')

    store.clearNodeError('conn-1:my_catalog')

    expect(store.getNodeError('conn-1:my_catalog')).toBeNull()
  })

  it('clearAllNodeErrors 清除所有节点错误', () => {
    const store = createStore()
    store.setNodeError('a', 'err1')
    store.setNodeError('b', 'err2')

    store.clearAllNodeErrors()

    expect(store.getNodeError('a')).toBeNull()
    expect(store.getNodeError('b')).toBeNull()
  })
})

// ==================== 13. 同步时间与模式 ====================

describe('同步时间与模式', () => {
  beforeEach(() => resetAllMocks())

  it('getLastSyncTime 未设置时返回 0', () => {
    const store = createStore()

    expect(store.getLastSyncTime('conn-1')).toBe(0)
  })

  it('setLastSyncTime / getLastSyncTime 设置并读取', () => {
    const store = createStore()

    store.setLastSyncTime('conn-1')

    const time = store.getLastSyncTime('conn-1')
    expect(time).toBeGreaterThan(0)
  })

  it('setSyncMode / getSyncMode 默认 incremental', () => {
    const store = createStore()

    expect(store.getSyncMode('conn-1')).toBe('incremental')

    store.setSyncMode('conn-1', 'full')
    expect(store.getSyncMode('conn-1')).toBe('full')
  })
})

// ==================== 14. executeSql SQL 执行 ====================

describe('executeSql', () => {
  beforeEach(() => resetAllMocks())

  it('委派到 connection service 的 executeSql', async () => {
    mockExecuteSql.mockResolvedValue({ rows: [] })
    const store = createStore()

    const result = await store.executeSql('conn-1', 'my_catalog', 'SELECT 1')

    expect(mockExecuteSql).toHaveBeenCalledWith('conn-1', 'SELECT 1')
    expect(result).toEqual({ rows: [] })
  })
})
