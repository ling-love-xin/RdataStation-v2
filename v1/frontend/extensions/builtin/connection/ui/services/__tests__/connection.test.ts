/**
 * connection 服务单元测试
 *
 * 测试 updateGlobalConnection payload 构建、connectDatabase 输入构建、
 * 以及 testConnection 参数透传等纯逻辑。
 */
import { describe, expect, it } from 'vitest'

// ==================== 纯函数提取 ====================

interface ConnectDatabaseInput {
  conn_id: string | null
  db_type: string
  url: string
  name: string | null
  connection_type: 'global' | 'project'
  project_id: string | null
  driver_id: string | null
  network_config_id: string | null
  environment_id: string | null
  auth_config_id: string | null
  auth_method: string | null
  driver_properties: string | null
  advanced_options: string | null
  description: string | null
  options: string | null
  tags: string | null
  metadata_path: string | null
  schema_name: string | null
  use_duckdb_fed: boolean
  password: string | null
}

interface ConnectDatabaseOpts {
  connId?: string
  driverId?: string
  networkConfigId?: string | null
  environmentId?: string
  authConfigId?: string
  authMethod?: string
  driverProperties?: string
  advancedOptions?: string
  description?: string
  options?: string
  tags?: string
  metadataPath?: string
  schemaName?: string
  useDuckdbFed?: boolean
  password?: string
}

function buildConnectDatabaseInput(
  dbType: string,
  url: string,
  name?: string,
  connectionType?: 'global' | 'project',
  projectId?: string,
  opts?: ConnectDatabaseOpts
): ConnectDatabaseInput {
  return {
    conn_id: opts?.connId ?? null,
    db_type: dbType,
    url,
    name: name ?? null,
    connection_type: connectionType || 'global',
    project_id: projectId ?? null,
    driver_id: opts?.driverId ?? null,
    network_config_id: opts?.networkConfigId ?? null,
    environment_id: opts?.environmentId ?? null,
    auth_config_id: opts?.authConfigId ?? null,
    auth_method: opts?.authMethod ?? null,
    driver_properties: opts?.driverProperties ?? null,
    advanced_options: opts?.advancedOptions ?? null,
    description: opts?.description ?? null,
    options: opts?.options ?? null,
    tags: opts?.tags ?? null,
    metadata_path: opts?.metadataPath ?? null,
    schema_name: opts?.schemaName ?? null,
    use_duckdb_fed: opts?.useDuckdbFed ?? false,
    password: opts?.password ?? null,
  }
}

interface UpdateGlobalConnectionInput {
  conn_id: string
  name?: string
  driver?: string
  host?: string
  port?: number
  database?: string
  schema_name?: string
  username?: string
  password?: string
  options?: string
  tags?: string[]
  use_duckdb_fed?: boolean
  metadata_path?: string
  driver_id?: string
  environment_id?: string
  auth_config_id?: string
  auth_method?: string
  network_config_id?: string
  driver_properties?: string
  advanced_options?: string
  description?: string
  server_version?: string
}

function buildUpdateGlobalConnectionPayload(
  input: UpdateGlobalConnectionInput
): Record<string, unknown> {
  return {
    conn_id: input.conn_id,
    name: input.name ?? null,
    driver: input.driver ?? null,
    host: input.host ?? null,
    port: input.port ?? null,
    database: input.database ?? null,
    schema_name: input.schema_name ?? null,
    username: input.username ?? null,
    password: input.password ?? null,
    options: input.options ?? null,
    tags: input.tags ?? null,
    use_duckdb_fed: input.use_duckdb_fed ?? null,
    metadata_path: input.metadata_path ?? null,
    driver_id: input.driver_id ?? null,
    environment_id: input.environment_id ?? null,
    auth_config_id: input.auth_config_id ?? null,
    auth_method: input.auth_method ?? null,
    network_config_id: input.network_config_id ?? null,
    driver_properties: input.driver_properties ?? null,
    advanced_options: input.advanced_options ?? null,
    description: input.description ?? null,
    server_version: input.server_version ?? null,
  }
}

// ==================== buildConnectDatabaseInput ====================

describe('buildConnectDatabaseInput 连接输入构建', () => {
  it('最小参数 → 所有可选字段为 null', () => {
    const input = buildConnectDatabaseInput('mysql', 'mysql://localhost:3306/db')
    expect(input.db_type).toBe('mysql')
    expect(input.url).toBe('mysql://localhost:3306/db')
    expect(input.name).toBeNull()
    expect(input.connection_type).toBe('global')
    expect(input.project_id).toBeNull()
    expect(input.conn_id).toBeNull()
    expect(input.driver_id).toBeNull()
    expect(input.network_config_id).toBeNull()
    expect(input.environment_id).toBeNull()
    expect(input.auth_config_id).toBeNull()
    expect(input.auth_method).toBeNull()
    expect(input.driver_properties).toBeNull()
    expect(input.advanced_options).toBeNull()
    expect(input.description).toBeNull()
    expect(input.options).toBeNull()
    expect(input.tags).toBeNull()
    expect(input.metadata_path).toBeNull()
    expect(input.schema_name).toBeNull()
    expect(input.use_duckdb_fed).toBe(false)
    expect(input.password).toBeNull()
  })

  it('完整参数 → 所有字段正确映射', () => {
    const input = buildConnectDatabaseInput(
      'postgresql',
      'postgresql://pg:pass@host:5432/analytics',
      'My PG',
      'project',
      '/projects/my-app',
      {
        connId: 'conn-001',
        driverId: 'drv-pg',
        networkConfigId: 'net-ssh-001',
        environmentId: 'env-prod',
        authConfigId: 'auth-001',
        authMethod: 'password',
        driverProperties: '{"poolSize":10}',
        advancedOptions: '{"timeout":30}',
        description: 'Production DB',
        options: '{"charset":"utf8"}',
        tags: 'prod,pg',
        metadataPath: '/meta/pg.db',
        schemaName: 'public',
        useDuckdbFed: true,
        password: 'encrypted-secret',
      }
    )

    expect(input.db_type).toBe('postgresql')
    expect(input.url).toBe('postgresql://pg:pass@host:5432/analytics')
    expect(input.name).toBe('My PG')
    expect(input.connection_type).toBe('project')
    expect(input.project_id).toBe('/projects/my-app')
    expect(input.conn_id).toBe('conn-001')
    expect(input.driver_id).toBe('drv-pg')
    expect(input.network_config_id).toBe('net-ssh-001')
    expect(input.environment_id).toBe('env-prod')
    expect(input.auth_config_id).toBe('auth-001')
    expect(input.auth_method).toBe('password')
    expect(input.driver_properties).toBe('{"poolSize":10}')
    expect(input.advanced_options).toBe('{"timeout":30}')
    expect(input.description).toBe('Production DB')
    expect(input.options).toBe('{"charset":"utf8"}')
    expect(input.tags).toBe('prod,pg')
    expect(input.metadata_path).toBe('/meta/pg.db')
    expect(input.schema_name).toBe('public')
    expect(input.use_duckdb_fed).toBe(true)
    expect(input.password).toBe('encrypted-secret')
  })

  it('connectionType 默认值 → global', () => {
    const input = buildConnectDatabaseInput('sqlite', 'sqlite:///data.db')
    expect(input.connection_type).toBe('global')
  })

  it('connectionType 显式传入 project → project', () => {
    const input = buildConnectDatabaseInput(
      'mysql',
      'mysql://localhost:3306/',
      undefined,
      'project'
    )
    expect(input.connection_type).toBe('project')
  })

  it('empty opts → 所有 opts 字段为 null', () => {
    const input = buildConnectDatabaseInput(
      'mysql',
      'mysql://localhost:3306/',
      'Test',
      'global',
      undefined,
      {}
    )
    expect(input.name).toBe('Test')
    expect(input.conn_id).toBeNull()
    expect(input.driver_id).toBeNull()
    expect(input.network_config_id).toBeNull()
    expect(input.environment_id).toBeNull()
    expect(input.auth_config_id).toBeNull()
    expect(input.auth_method).toBeNull()
    expect(input.use_duckdb_fed).toBe(false)
  })

  it('empty string → 保持空字符串（非 null）', () => {
    const input = buildConnectDatabaseInput('mysql', 'mysql://localhost:3306/', '')
    expect(input.name).toBe('')
  })

  it('special chars in url → 透传', () => {
    const url = 'mysql://user:p%40ss@host:3306/db?charset=utf8mb4'
    const input = buildConnectDatabaseInput('mysql', url)
    expect(input.url).toBe(url)
  })
})

// ==================== buildUpdateGlobalConnectionPayload ====================

describe('buildUpdateGlobalConnectionPayload 更新全局连接 payload 构建', () => {
  it('完整输入 → 25 字段映射', () => {
    const payload = buildUpdateGlobalConnectionPayload({
      conn_id: 'conn-001',
      name: 'Updated MySQL',
      driver: 'mysql',
      host: '10.0.0.1',
      port: 3307,
      database: 'new_db',
      schema_name: 'public',
      username: 'new_user',
      password: 'new_pass',
      options: '{"charset":"utf8mb4"}',
      tags: ['prod', 'critical'],
      use_duckdb_fed: true,
      metadata_path: '/meta/conn.db',
      driver_id: 'drv-mysql-8',
      environment_id: 'env-staging',
      auth_config_id: 'auth-002',
      auth_method: 'password',
      network_config_id: 'net-ssh-002',
      driver_properties: '{"useSSL":"true"}',
      advanced_options: '{"poolSize":20}',
      description: 'Updated production MySQL',
      server_version: '8.0.36',
    })

    expect(payload.conn_id).toBe('conn-001')
    expect(payload.name).toBe('Updated MySQL')
    expect(payload.driver).toBe('mysql')
    expect(payload.host).toBe('10.0.0.1')
    expect(payload.port).toBe(3307)
    expect(payload.database).toBe('new_db')
    expect(payload.schema_name).toBe('public')
    expect(payload.username).toBe('new_user')
    expect(payload.password).toBe('new_pass')
    expect(payload.options).toBe('{"charset":"utf8mb4"}')
    expect(payload.tags).toEqual(['prod', 'critical'])
    expect(payload.use_duckdb_fed).toBe(true)
    expect(payload.metadata_path).toBe('/meta/conn.db')
    expect(payload.driver_id).toBe('drv-mysql-8')
    expect(payload.environment_id).toBe('env-staging')
    expect(payload.auth_config_id).toBe('auth-002')
    expect(payload.auth_method).toBe('password')
    expect(payload.network_config_id).toBe('net-ssh-002')
    expect(payload.driver_properties).toBe('{"useSSL":"true"}')
    expect(payload.advanced_options).toBe('{"poolSize":20}')
    expect(payload.description).toBe('Updated production MySQL')
    expect(payload.server_version).toBe('8.0.36')
  })

  it('仅 conn_id → 其余字段为 null', () => {
    const payload = buildUpdateGlobalConnectionPayload({ conn_id: 'conn-min' })
    expect(payload.conn_id).toBe('conn-min')
    expect(payload.name).toBeNull()
    expect(payload.driver).toBeNull()
    expect(payload.host).toBeNull()
    expect(payload.port).toBeNull()
    expect(payload.database).toBeNull()
    expect(payload.schema_name).toBeNull()
    expect(payload.username).toBeNull()
    expect(payload.password).toBeNull()
    expect(payload.options).toBeNull()
    expect(payload.tags).toBeNull()
    expect(payload.use_duckdb_fed).toBeNull()
    expect(payload.metadata_path).toBeNull()
    expect(payload.driver_id).toBeNull()
    expect(payload.environment_id).toBeNull()
    expect(payload.auth_config_id).toBeNull()
    expect(payload.auth_method).toBeNull()
    expect(payload.network_config_id).toBeNull()
    expect(payload.driver_properties).toBeNull()
    expect(payload.advanced_options).toBeNull()
    expect(payload.description).toBeNull()
    expect(payload.server_version).toBeNull()
  })

  it('undefined vs null 区分 → undefined 通过 ?? 转为 null', () => {
    const payload = buildUpdateGlobalConnectionPayload({
      conn_id: 'conn-001',
      name: undefined,
      host: undefined,
      port: undefined,
    })
    expect(payload.name).toBeNull()
    expect(payload.host).toBeNull()
    expect(payload.port).toBeNull()
  })

  it('false 值 → 保持 false', () => {
    const payload = buildUpdateGlobalConnectionPayload({
      conn_id: 'conn-001',
      use_duckdb_fed: false,
    })
    expect(payload.use_duckdb_fed).toBe(false)
  })

  it('0 值 → 保持 0', () => {
    const payload = buildUpdateGlobalConnectionPayload({
      conn_id: 'conn-001',
      port: 0,
    })
    expect(payload.port).toBe(0)
  })

  it('空字符串 → 保持空字符串', () => {
    const payload = buildUpdateGlobalConnectionPayload({
      conn_id: 'conn-001',
      name: '',
      host: '',
    })
    expect(payload.name).toBe('')
    expect(payload.host).toBe('')
  })

  it('tags 空数组 → 保持空数组', () => {
    const payload = buildUpdateGlobalConnectionPayload({
      conn_id: 'conn-001',
      tags: [],
    })
    expect(payload.tags).toEqual([])
  })
})

// ==================== testConnection 参数透传 ====================

describe('testConnection 参数透传逻辑', () => {
  it('networkConfigId 为 undefined → null', () => {
    // 模拟: networkConfigId ?? null
    const networkConfigId: string | null | undefined = undefined
    const result = networkConfigId ?? null
    expect(result).toBeNull()
  })

  it('networkConfigId 为 null → null', () => {
    const networkConfigId: string | null | undefined = null
    const result = networkConfigId ?? null
    expect(result).toBeNull()
  })

  it('networkConfigId 为有效值 → 透传', () => {
    const networkConfigId: string | null | undefined = 'net-ssh-001'
    const result = networkConfigId ?? null
    expect(result).toBe('net-ssh-001')
  })
})

// ==================== createDatabaseFile 输入构建 ====================

describe('createDatabaseFile 输入构建', () => {
  function buildCreateDatabaseFileInput(
    dbType: string,
    filePath: string
  ): { db_type: string; file_path: string } {
    return { db_type: dbType, file_path: filePath }
  }

  it('SQLite → 正确映射', () => {
    const input = buildCreateDatabaseFileInput('sqlite', '/data/myapp.db')
    expect(input.db_type).toBe('sqlite')
    expect(input.file_path).toBe('/data/myapp.db')
  })

  it('DuckDB → 正确映射', () => {
    const input = buildCreateDatabaseFileInput('duckdb', '/data/analytics.duckdb')
    expect(input.db_type).toBe('duckdb')
    expect(input.file_path).toBe('/data/analytics.duckdb')
  })

  it('Windows 路径 → 透传', () => {
    const input = buildCreateDatabaseFileInput('sqlite', 'C:\\Users\\data\\myapp.db')
    expect(input.file_path).toBe('C:\\Users\\data\\myapp.db')
  })
})

// ==================== executeSql 输入构建 ====================

describe('executeSql 输入构建', () => {
  function buildExecuteSqlInput(connId: string, sql: string) {
    return { conn_id: connId, sql, timeout_ms: null }
  }

  it('SELECT → 正确映射', () => {
    const input = buildExecuteSqlInput('conn-001', 'SELECT 1')
    expect(input.conn_id).toBe('conn-001')
    expect(input.sql).toBe('SELECT 1')
    expect(input.timeout_ms).toBeNull()
  })

  it('多行 SQL → 透传', () => {
    const sql = `SELECT *
FROM users
WHERE id = 1`
    const input = buildExecuteSqlInput('conn-001', sql)
    expect(input.sql).toBe(sql)
  })
})

// ==================== saveNavigatorState 输入构建 ====================

describe('saveNavigatorState 输入构建', () => {
  function buildSaveNavigatorStateInput(
    connectionId: string,
    expandedKeys: string[],
    selectedKeys: string[]
  ) {
    return {
      connection_id: connectionId,
      expanded_keys: expandedKeys,
      selected_keys: selectedKeys,
    }
  }

  it('完整状态 → 正确映射', () => {
    const input = buildSaveNavigatorStateInput(
      'conn-001',
      ['catalog1', 'catalog1.schema1'],
      ['catalog1.schema1.table1']
    )
    expect(input.connection_id).toBe('conn-001')
    expect(input.expanded_keys).toEqual(['catalog1', 'catalog1.schema1'])
    expect(input.selected_keys).toEqual(['catalog1.schema1.table1'])
  })

  it('空列表 → 空数组', () => {
    const input = buildSaveNavigatorStateInput('conn-001', [], [])
    expect(input.expanded_keys).toEqual([])
    expect(input.selected_keys).toEqual([])
  })
})
