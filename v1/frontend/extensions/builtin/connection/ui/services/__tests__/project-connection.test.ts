/**
 * project-connection 服务单元测试
 *
 * 测试 buildConnectionUrl、getConnectionDisplayName、mapResponse 等纯函数。
 */
import { describe, expect, it } from 'vitest'

import type { ProjectConnection } from '../../../types/connection'

// ==================== 纯函数提取（从 project-connection.ts 提取） ====================

interface ProjectConnectionResponse {
  id: string
  name: string
  driver: string
  host: string | null
  port: number | null
  database: string | null
  schema_name: string | null
  username: string | null
  password: string | null
  options: string | null
  tags: string | null
  use_duckdb_fed: boolean
  metadata_path: string | null
  description: string | null
  server_version: string | null
  driver_id: string | null
  environment_id: string | null
  auth_config_id: string | null
  auth_method: string | null
  network_config_id: string | null
  driver_properties: string | null
  advanced_options: string | null
  connection_type: string | null
  is_active: boolean
  created_at: string
  updated_at: string
}

function mapResponse(r: ProjectConnectionResponse): ProjectConnection {
  return {
    id: r.id,
    name: r.name,
    driver: r.driver,
    host: r.host ?? undefined,
    port: r.port ?? undefined,
    database: r.database ?? undefined,
    schema_name: r.schema_name ?? undefined,
    username: r.username ?? undefined,
    password: r.password ?? undefined,
    options: r.options ?? undefined,
    tags: r.tags ?? undefined,
    use_duckdb_fed: r.use_duckdb_fed,
    metadata_path: r.metadata_path ?? undefined,
    is_active: r.is_active,
    server_version: r.server_version ?? undefined,
    description: r.description ?? undefined,
    driver_id: r.driver_id ?? undefined,
    environment_id: r.environment_id ?? undefined,
    auth_config_id: r.auth_config_id ?? undefined,
    auth_method: r.auth_method ?? undefined,
    network_config_id: r.network_config_id ?? undefined,
    driver_properties: r.driver_properties ?? undefined,
    advanced_options: r.advanced_options ?? undefined,
    connection_type: (r.connection_type as 'global' | 'project') ?? 'project',
    status: r.is_active ? 'connected' : 'disconnected',
    created_at: r.created_at,
    updated_at: r.updated_at,
  }
}

function buildConnectionUrl(connection: ProjectConnection): string {
  const { driver, host, port, database, username, password } = connection

  switch (driver?.toLowerCase()) {
    case 'mysql':
      if (username && password) {
        return `mysql://${username}:${password}@${host || 'localhost'}:${port || 3306}/${database || ''}`
      }
      return `mysql://${host || 'localhost'}:${port || 3306}/${database || ''}`

    case 'postgresql':
    case 'postgres':
      if (username && password) {
        return `postgresql://${username}:${password}@${host || 'localhost'}:${port || 5432}/${database || ''}`
      }
      return `postgresql://${host || 'localhost'}:${port || 5432}/${database || ''}`

    case 'sqlite':
      return `sqlite://${host || ''}`

    case 'duckdb':
      return `duckdb://${host || ''}`

    default:
      throw new Error(`不支持的数据库类型: ${driver || 'unknown'}`)
  }
}

function getConnectionDisplayName(connection: ProjectConnection): string {
  if (connection.name) {
    return connection.name
  }

  const { driver, host, port, database } = connection

  if (database) {
    return `${driver} - ${database}@${host || 'localhost'}`
  }

  if (port) {
    return `${driver} - ${host || 'localhost'}:${port}`
  }

  return `${driver} - ${host || 'localhost'}`
}

// ==================== 测试数据 ====================

const baseConn: ProjectConnection = {
  id: 'conn-001',
  name: 'My DB',
  driver: 'mysql',
  host: '192.168.1.100',
  port: 3307,
  database: 'prod_db',
  username: 'admin',
  password: 'secret',
  status: 'disconnected',
  is_active: false,
  use_duckdb_fed: false,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  connection_type: 'project',
}

// ==================== buildConnectionUrl ====================

describe('buildConnectionUrl URL 构建', () => {
  it('MySQL 完整凭据 → 完整 URL', () => {
    const url = buildConnectionUrl(baseConn)
    expect(url).toBe('mysql://admin:secret@192.168.1.100:3307/prod_db')
  })

  it('MySQL 无用户名密码 → 省略凭据', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      username: undefined,
      password: undefined,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('mysql://192.168.1.100:3307/prod_db')
  })

  it('MySQL 仅用户名无密码 → 省略凭据', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      username: 'admin',
      password: undefined,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('mysql://192.168.1.100:3307/prod_db')
  })

  it('MySQL 仅密码无用户名 → 省略凭据', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      username: undefined,
      password: 'secret',
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('mysql://192.168.1.100:3307/prod_db')
  })

  it('MySQL 无 host → 默认 localhost', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      host: undefined,
      port: 3306,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('mysql://admin:secret@localhost:3306/prod_db')
  })

  it('MySQL 无 port → 默认 3306', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      host: 'db.example.com',
      port: undefined,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('mysql://admin:secret@db.example.com:3306/prod_db')
  })

  it('MySQL 无 database → 空数据库名', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      database: undefined,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('mysql://admin:secret@192.168.1.100:3307/')
  })

  it('PostgreSQL 完整凭据', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'postgresql',
      host: 'pg.example.com',
      port: 5432,
      database: 'analytics',
      username: 'pguser',
      password: 'pgpass',
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('postgresql://pguser:pgpass@pg.example.com:5432/analytics')
  })

  it('PostgreSQL 无 host → 默认 localhost', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'postgresql',
      host: undefined,
      port: 5432,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('postgresql://admin:secret@localhost:5432/prod_db')
  })

  it('PostgreSQL 无 port → 默认 5432', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'postgresql',
      host: 'pg.example.com',
      port: undefined,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('postgresql://admin:secret@pg.example.com:5432/prod_db')
  })

  it('postgres（短名）→ postgresql 协议', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'postgres',
      host: 'pg.example.com',
      port: 5432,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('postgresql://admin:secret@pg.example.com:5432/prod_db')
  })

  it('SQLite → file 协议', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'sqlite',
      host: '/data/myapp.db',
      port: undefined,
      database: undefined,
      username: undefined,
      password: undefined,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('sqlite:///data/myapp.db')
  })

  it('SQLite 无 host → 空路径', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'sqlite',
      host: undefined,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('sqlite://')
  })

  it('DuckDB → duckdb 协议', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'duckdb',
      host: '/data/analytics.duckdb',
      port: undefined,
      database: undefined,
      username: undefined,
      password: undefined,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('duckdb:///data/analytics.duckdb')
  })

  it('DuckDB 无 host → 空路径', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'duckdb',
      host: undefined,
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('duckdb://')
  })

  it('未知驱动类型 → 抛出错误', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'oracle',
    }
    expect(() => buildConnectionUrl(conn)).toThrow('不支持的数据库类型: oracle')
  })

  it('driver 为 undefined → 抛出错误', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: undefined as unknown as string,
    }
    expect(() => buildConnectionUrl(conn)).toThrow('不支持的数据库类型: unknown')
  })

  it('driver 大小写不敏感 → MYSQL', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      driver: 'MYSQL',
    }
    const url = buildConnectionUrl(conn)
    expect(url).toBe('mysql://admin:secret@192.168.1.100:3307/prod_db')
  })
})

// ==================== getConnectionDisplayName ====================

describe('getConnectionDisplayName 显示名生成', () => {
  it('有 name → 返回 name', () => {
    const name = getConnectionDisplayName(baseConn)
    expect(name).toBe('My DB')
  })

  it('无 name 有 database → driver - database@host', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      name: '',
    }
    const name = getConnectionDisplayName(conn)
    expect(name).toBe('mysql - prod_db@192.168.1.100')
  })

  it('无 name 无 database 有 port → driver - host:port', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      name: '',
      database: undefined,
    }
    const name = getConnectionDisplayName(conn)
    expect(name).toBe('mysql - 192.168.1.100:3307')
  })

  it('无 name 无 database 无 port → driver - host', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      name: '',
      database: undefined,
      port: undefined,
    }
    const name = getConnectionDisplayName(conn)
    expect(name).toBe('mysql - 192.168.1.100')
  })

  it('无 host → 默认 localhost', () => {
    const conn: ProjectConnection = {
      ...baseConn,
      name: '',
      database: undefined,
      host: undefined,
    }
    const name = getConnectionDisplayName(conn)
    expect(name).toBe('mysql - localhost:3307')
  })
})

// ==================== mapResponse ====================

describe('mapResponse 后端响应映射', () => {
  it('完整响应 → ProjectConnection 全字段映射', () => {
    const r: ProjectConnectionResponse = {
      id: 'conn-001',
      name: 'Prod DB',
      driver: 'mysql',
      host: '10.0.0.1',
      port: 3306,
      database: 'production',
      schema_name: 'public',
      username: 'root',
      password: 'encrypted-pass',
      options: '{"charset":"utf8mb4"}',
      tags: 'prod,mysql',
      use_duckdb_fed: false,
      metadata_path: '/meta/prod.db',
      description: 'Production MySQL',
      server_version: '8.0.35',
      driver_id: 'drv-mysql',
      environment_id: 'env-prod',
      auth_config_id: 'auth-001',
      auth_method: 'password',
      network_config_id: 'net-ssh-001',
      driver_properties: '{"allowPublicKeyRetrieval":"true"}',
      advanced_options: '{"poolSize":10}',
      connection_type: 'project',
      is_active: true,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-10T00:00:00Z',
    }

    const conn = mapResponse(r)
    expect(conn.id).toBe('conn-001')
    expect(conn.name).toBe('Prod DB')
    expect(conn.driver).toBe('mysql')
    expect(conn.host).toBe('10.0.0.1')
    expect(conn.port).toBe(3306)
    expect(conn.database).toBe('production')
    expect(conn.schema_name).toBe('public')
    expect(conn.username).toBe('root')
    expect(conn.password).toBe('encrypted-pass')
    expect(conn.options).toBe('{"charset":"utf8mb4"}')
    expect(conn.tags).toBe('prod,mysql')
    expect(conn.use_duckdb_fed).toBe(false)
    expect(conn.metadata_path).toBe('/meta/prod.db')
    expect(conn.description).toBe('Production MySQL')
    expect(conn.server_version).toBe('8.0.35')
    expect(conn.driver_id).toBe('drv-mysql')
    expect(conn.environment_id).toBe('env-prod')
    expect(conn.auth_config_id).toBe('auth-001')
    expect(conn.auth_method).toBe('password')
    expect(conn.network_config_id).toBe('net-ssh-001')
    expect(conn.driver_properties).toBe('{"allowPublicKeyRetrieval":"true"}')
    expect(conn.advanced_options).toBe('{"poolSize":10}')
    expect(conn.connection_type).toBe('project')
    expect(conn.status).toBe('connected')
    expect(conn.created_at).toBe('2026-01-01T00:00:00Z')
    expect(conn.updated_at).toBe('2026-01-10T00:00:00Z')
  })

  it('is_active=false → status=disconnected', () => {
    const r: ProjectConnectionResponse = {
      id: 'conn-002',
      name: 'Offline',
      driver: 'mysql',
      host: null,
      port: null,
      database: null,
      schema_name: null,
      username: null,
      password: null,
      options: null,
      tags: null,
      use_duckdb_fed: false,
      metadata_path: null,
      description: null,
      server_version: null,
      driver_id: null,
      environment_id: null,
      auth_config_id: null,
      auth_method: null,
      network_config_id: null,
      driver_properties: null,
      advanced_options: null,
      connection_type: null,
      is_active: false,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const conn = mapResponse(r)
    expect(conn.status).toBe('disconnected')
  })

  it('null 字段 → undefined（通过 ?? 处理）', () => {
    const r: ProjectConnectionResponse = {
      id: 'conn-003',
      name: 'Minimal',
      driver: 'sqlite',
      host: null,
      port: null,
      database: null,
      schema_name: null,
      username: null,
      password: null,
      options: null,
      tags: null,
      use_duckdb_fed: false,
      metadata_path: null,
      description: null,
      server_version: null,
      driver_id: null,
      environment_id: null,
      auth_config_id: null,
      auth_method: null,
      network_config_id: null,
      driver_properties: null,
      advanced_options: null,
      connection_type: null,
      is_active: false,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const conn = mapResponse(r)
    expect(conn.host).toBeUndefined()
    expect(conn.port).toBeUndefined()
    expect(conn.database).toBeUndefined()
    expect(conn.username).toBeUndefined()
    expect(conn.password).toBeUndefined()
    expect(conn.driver_id).toBeUndefined()
    expect(conn.environment_id).toBeUndefined()
    expect(conn.auth_config_id).toBeUndefined()
  })

  it('connection_type=null → 默认 project', () => {
    const r: ProjectConnectionResponse = {
      id: 'conn-004',
      name: 'Default',
      driver: 'mysql',
      host: null,
      port: null,
      database: null,
      schema_name: null,
      username: null,
      password: null,
      options: null,
      tags: null,
      use_duckdb_fed: false,
      metadata_path: null,
      description: null,
      server_version: null,
      driver_id: null,
      environment_id: null,
      auth_config_id: null,
      auth_method: null,
      network_config_id: null,
      driver_properties: null,
      advanced_options: null,
      connection_type: null,
      is_active: false,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const conn = mapResponse(r)
    expect(conn.connection_type).toBe('project')
  })

  it('connection_type=global → 保持 global', () => {
    const r: ProjectConnectionResponse = {
      id: 'conn-005',
      name: 'Global',
      driver: 'postgresql',
      host: null,
      port: null,
      database: null,
      schema_name: null,
      username: null,
      password: null,
      options: null,
      tags: null,
      use_duckdb_fed: false,
      metadata_path: null,
      description: null,
      server_version: null,
      driver_id: null,
      environment_id: null,
      auth_config_id: null,
      auth_method: null,
      network_config_id: null,
      driver_properties: null,
      advanced_options: null,
      connection_type: 'global',
      is_active: false,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const conn = mapResponse(r)
    expect(conn.connection_type).toBe('global')
  })

  it('use_duckdb_fed=true → 保持 true', () => {
    const r: ProjectConnectionResponse = {
      id: 'conn-006',
      name: 'Fed',
      driver: 'mysql',
      host: null,
      port: null,
      database: null,
      schema_name: null,
      username: null,
      password: null,
      options: null,
      tags: null,
      use_duckdb_fed: true,
      metadata_path: null,
      description: null,
      server_version: null,
      driver_id: null,
      environment_id: null,
      auth_config_id: null,
      auth_method: null,
      network_config_id: null,
      driver_properties: null,
      advanced_options: null,
      connection_type: null,
      is_active: false,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
    }
    const conn = mapResponse(r)
    expect(conn.use_duckdb_fed).toBe(true)
  })
})
