// @ts-nocheck -- 测试文件，dockview IDockviewPanelProps 类型在 mock 中不需要完整实现
/**
 * @vitest-environment jsdom
 */
import { mount } from '@vue/test-utils'
import { describe, it, expect, vi, beforeEach } from 'vitest'

import type { PropertiesResult } from '@/extensions/builtin/database/ui/composables/properties-registry'

import DynamicObjectPropertiesPanel from '../DynamicObjectPropertiesPanel.vue'

// ========== 预声明 mock 函数（vi.hoisted 确保在模块 mock 之前可用） ==========
// 注意：vi.hoisted 在 import 之前执行，不能使用 vue ref 等模块导入

const { mockExtractor, mockConnectionCatalogs } = vi.hoisted(() => {
  const map = new Map<string, unknown[]>()
  return {
    mockExtractor: vi.fn(),
    mockConnectionCatalogs: map,
  }
})

// ========== Mock vue-i18n ==========

vi.mock('vue-i18n', () => ({
  useI18n: () => ({
    t: (key: string) => key,
  }),
}))

// ========== Mock propertiesRegistry ==========

vi.mock('@/extensions/builtin/database/ui/composables/properties-registry', () => ({
  propertiesRegistry: new Proxy(
    {},
    {
      get: () => mockExtractor,
    }
  ),
}))

// ========== Mock databaseNavigatorStore ==========

vi.mock('@/extensions/builtin/database/ui/stores/database-navigator-store', () => ({
  useDatabaseNavigatorStore: () => ({
    connectionCatalogs: mockConnectionCatalogs,
  }),
}))

// ========== Mock navigator.clipboard ==========

Object.defineProperty(navigator, 'clipboard', {
  value: {
    writeText: vi.fn().mockResolvedValue(undefined),
  },
  writable: true,
  configurable: true,
})

// ========== 测试夹具 ==========

function createTableResult(): PropertiesResult {
  return {
    title: 'users (TABLE)',
    objectType: 'TABLE',
    scope: 'global',
    properties: [
      { label: 'Name', value: 'users', label2: 'Type', value2: 'TABLE' },
      { label: 'Schema', value: 'public', label2: 'Catalog', value2: 'mydb' },
      { label: 'Row Count', value: '1,000', label2: 'Data Size', value2: '1.5 MB' },
      { label: 'Index Size', value: '256 KB', label2: 'Total Size', value2: '1.8 MB' },
      { label: 'Columns', value: '3', label2: 'Indexes', value2: '1' },
      { label: 'Scope', value: 'workbench.scopeGlobal', label2: '', value2: '' },
    ],
    subEntities: [
      {
        id: 'columns',
        label: 'Columns',
        icon: 'columns',
        count: 3,
        kind: 'table',
        table: {
          columns: [
            { key: 'pk', label: '#', width: '40px' },
            { key: 'name', label: 'Name', width: '180px' },
            { key: 'type', label: 'Data Type', width: '140px' },
          ],
          rows: [
            ['PK', 'id', 'INTEGER'],
            ['', 'name', 'VARCHAR(255)'],
            ['', 'email', 'VARCHAR(255)'],
          ],
        },
      },
      {
        id: 'indexes',
        label: 'Indexes',
        icon: 'key',
        count: 1,
        kind: 'table',
        table: {
          columns: [
            { key: 'name', label: 'Name', width: '200px' },
            { key: 'type', label: 'Type', width: '100px' },
          ],
          rows: [['idx_users_email', 'BTREE']],
        },
      },
      {
        id: 'ddl',
        label: 'DDL',
        icon: 'code',
        count: 0,
        kind: 'code',
        code: 'CREATE TABLE public.users (\n  id INTEGER PRIMARY KEY\n);',
      },
    ],
  }
}

function createViewResult(): PropertiesResult {
  return {
    title: 'user_view (VIEW)',
    objectType: 'VIEW',
    scope: 'global',
    properties: [
      { label: 'Name', value: 'user_view', label2: 'Type', value2: 'VIEW' },
      { label: 'Schema', value: 'public', label2: 'Catalog', value2: 'mydb' },
      { label: 'Columns', value: '2', label2: '', value2: '' },
      { label: 'Scope', value: 'workbench.scopeGlobal', label2: '', value2: '' },
    ],
    subEntities: [
      {
        id: 'columns',
        label: 'Columns',
        icon: 'columns',
        count: 2,
        kind: 'table',
        table: {
          columns: [
            { key: 'pk', label: '#', width: '40px' },
            { key: 'name', label: 'Name', width: '180px' },
            { key: 'type', label: 'Data Type', width: '140px' },
          ],
          rows: [
            ['', 'id', 'INTEGER'],
            ['', 'full_name', 'VARCHAR(500)'],
          ],
        },
      },
      {
        id: 'ddl',
        label: 'DDL',
        icon: 'code',
        count: 0,
        kind: 'code',
        code: 'CREATE VIEW public.user_view AS SELECT ...',
      },
    ],
  }
}

function createColumnResult(): PropertiesResult {
  return {
    title: 'id (COLUMN)',
    objectType: 'COLUMN',
    scope: 'global',
    properties: [
      { label: 'Name', value: 'id', label2: 'Table', value2: 'users' },
      { label: 'Data Type', value: 'INTEGER', label2: 'Nullable', value2: 'NOT NULL' },
      { label: 'Default', value: '—', label2: 'Primary Key', value2: 'YES' },
      { label: 'Max Length', value: '—', label2: 'Numeric Prec', value2: '—' },
      { label: 'Schema', value: 'public', label2: 'Catalog', value2: 'mydb' },
      { label: 'Scope', value: 'workbench.scopeGlobal', label2: '', value2: '' },
    ],
    subEntities: [],
  }
}

function baseParams(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    connectionId: 'conn-1',
    scope: 'global' as 'global' | 'project',
    dbType: 'mysql',
    dbName: 'mydb',
    schemaName: 'public',
    objectType: 'table',
    objectName: 'users',
    ...overrides,
  } as Record<string, unknown>
}

// ========== 测试 ==========

describe('DynamicObjectPropertiesPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockConnectionCatalogs.clear()
    mockConnectionCatalogs.set('conn-1', [{ name: 'mydb', schemas: [] }])
  })

  // ====================================================================
  // 1. 渲染 table 类型属性
  // ====================================================================

  it('渲染 table 类型属性', () => {
    mockExtractor.mockReturnValue(createTableResult())

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: { params: baseParams({ objectType: 'table' }) as any },
    })

    // 属性网格中应显示表格属性
    expect(wrapper.text()).toContain('users')
    expect(wrapper.text()).toContain('TABLE')
    expect(wrapper.text()).toContain('1,000')
    expect(wrapper.text()).toContain('1.5 MB')

    // 子实体 Tab 应出现
    expect(wrapper.find('.sub-tabs').exists()).toBe(true)
    expect(wrapper.text()).toContain('Columns')
    expect(wrapper.text()).toContain('Indexes')
    expect(wrapper.text()).toContain('DDL')
  })

  // ====================================================================
  // 2. 渲染 view 类型属性
  // ====================================================================

  it('渲染 view 类型属性', () => {
    mockExtractor.mockReturnValue(createViewResult())

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: {
        params: baseParams({
          objectType: 'view',
          objectName: 'user_view',
        }),
      },
    })

    // 属性网格中应显示视图属性
    expect(wrapper.text()).toContain('user_view')
    expect(wrapper.text()).toContain('VIEW')

    // 子实体 Tab 应包含 Columns 和 DDL
    expect(wrapper.find('.sub-tabs').exists()).toBe(true)
    expect(wrapper.text()).toContain('Columns')
    expect(wrapper.text()).toContain('DDL')

    // Columns 表格内容（默认选中第一个子实体）
    expect(wrapper.text()).toContain('id')
    expect(wrapper.text()).toContain('full_name')
  })

  // ====================================================================
  // 3. 渲染 column 类型属性
  // ====================================================================

  it('渲染 column 类型属性', () => {
    mockExtractor.mockReturnValue(createColumnResult())

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: {
        params: baseParams({
          objectType: 'column',
          objectName: 'id',
          tableName: 'users',
        }),
      },
    })

    // 属性网格中应显示列属性
    expect(wrapper.text()).toContain('id')
    expect(wrapper.text()).toContain('INTEGER')
    expect(wrapper.text()).toContain('NOT NULL')
    expect(wrapper.text()).toContain('YES')

    // 列类型没有子实体
    expect(wrapper.find('.sub-tabs').exists()).toBe(false)
  })

  // ====================================================================
  // 4. 无连接 ID 时显示空状态
  // ====================================================================

  it('无连接 ID 时显示空状态', () => {
    mockExtractor.mockReturnValue(createTableResult())

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: {
        params: baseParams({
          connectionId: '',
          objectType: 'table',
        }),
      },
    })

    // 应显示空状态提示
    expect(wrapper.find('.empty-state').exists()).toBe(true)
    expect(wrapper.text()).toContain('workbench.selectObject')

    // 不应渲染属性网格
    expect(wrapper.find('.prop-grid').exists()).toBe(false)
  })

  // ====================================================================
  // 5. 子实体 Tab 切换
  // ====================================================================

  it('子实体 Tab 切换', async () => {
    mockExtractor.mockReturnValue(createTableResult())

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: { params: baseParams({ objectType: 'table' }) as any },
    })

    // 默认应选中第一个子实体（Columns）
    const subTabs = wrapper.findAll('.sub-tab')
    expect(subTabs.length).toBe(3) // Columns, Indexes, DDL
    expect(subTabs[0].classes()).toContain('active')

    // 应显示 Columns 的表格内容
    expect(wrapper.find('.sub-table-wrap').exists()).toBe(true)
    expect(wrapper.text()).toContain('id')
    expect(wrapper.text()).toContain('name')
    expect(wrapper.text()).toContain('email')

    // 点击 Indexes Tab
    await subTabs[1].trigger('click')
    expect(subTabs[1].classes()).toContain('active')
    expect(subTabs[0].classes()).not.toContain('active')

    // 应显示 Indexes 的内容
    expect(wrapper.text()).toContain('idx_users_email')
    expect(wrapper.text()).toContain('BTREE')

    // 点击 DDL Tab
    await subTabs[2].trigger('click')
    expect(subTabs[2].classes()).toContain('active')

    // 应显示 DDL 代码
    expect(wrapper.text()).toContain('CREATE TABLE')
    expect(wrapper.text()).toContain('PRIMARY KEY')
  })

  // ====================================================================
  // 6. 面包屑显示正确路径
  // ====================================================================

  it('面包屑显示正确路径', () => {
    mockExtractor.mockReturnValue(createTableResult())

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: {
        params: baseParams({
          dbName: 'mydb',
          schemaName: 'public',
          objectType: 'table',
          objectName: 'users',
        }),
      },
    })

    const breadcrumb = wrapper.find('.breadcrumb')
    expect(breadcrumb.exists()).toBe(true)

    expect(breadcrumb.text()).toContain('mydb')
    expect(breadcrumb.text()).toContain('public')
    expect(breadcrumb.text()).toContain('users')

    // 最后一个面包屑项应使用 current 样式
    const items = breadcrumb.findAll('.breadcrumb-item')
    expect(items.length).toBe(3)
    expect(items[2].classes()).toContain('breadcrumb-current')
  })

  // ====================================================================
  // 7. scope badge 显示全局/项目
  // ====================================================================

  it('scope badge 显示全局标识', () => {
    mockExtractor.mockReturnValue(createTableResult())

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: {
        params: baseParams({ scope: 'global' }),
      },
    })

    const badge = wrapper.find('.scope-badge')
    expect(badge.exists()).toBe(true)
    expect(badge.text()).toContain('workbench.scopeGlobal')
    expect(badge.classes()).toContain('scope-global')
  })

  it('scope badge 显示项目标识', () => {
    mockExtractor.mockReturnValue({
      ...createTableResult(),
      scope: 'project',
      properties: createTableResult().properties.map(p =>
        p.label === 'Scope' ? { ...p, value: 'workbench.scopeProject' } : p
      ),
    })

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: {
        params: baseParams({ scope: 'project' }),
      },
    })

    const badge = wrapper.find('.scope-badge')
    expect(badge.exists()).toBe(true)
    expect(badge.text()).toContain('workbench.scopeProject')
    expect(badge.classes()).toContain('scope-project')
  })

  // ====================================================================
  // 8. 对象类型 badge 显示
  // ====================================================================

  it('type badge 显示正确对象类型', () => {
    mockExtractor.mockReturnValue(createTableResult())

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: { params: baseParams({ objectType: 'table' }) as any },
    })

    const typeBadge = wrapper.find('.type-badge')
    expect(typeBadge.exists()).toBe(true)
    expect(typeBadge.text()).toBe('TABLE')
  })

  // ====================================================================
  // 9. Data Tab 切换
  // ====================================================================

  it('Data Tab 切换显示占位内容', async () => {
    mockExtractor.mockReturnValue(createTableResult())

    const wrapper = mount(DynamicObjectPropertiesPanel, {
      props: { params: baseParams({ objectType: 'table' }) as any },
    })

    // 默认在 Properties Tab
    expect(wrapper.find('.prop-grid').exists()).toBe(true)

    // 点击 Data Tab
    const topTabs = wrapper.findAll('.top-tab')
    await topTabs[1].trigger('click')

    expect(wrapper.find('.data-placeholder').exists()).toBe(true)
    expect(wrapper.text()).toContain('workbench.dataPlaceholder')
    expect(wrapper.find('.prop-grid').exists()).toBe(false)
  })
})
